//! tracker-bridge 插件（Native，docs/plugins/tracker-bridge/）：pt-tracker 的数据来源。
//!
//! 写入 Redis（`tracker_sync` 角色，键格式见 design/10 §5.5 与 catalog/redis-keys.md）：
//! - `trk:pk:<passkey>` → HASH `uid`、`st`（0 正常、1 禁止下载、2 不可用）、`ver`；`trk:u:<uid>` → 当前 passkey（反向索引，换 key 时删旧键）
//! - `trk:t:<info_hash_hex>` → HASH `tid`、`st`（0 正常、1 不可用）、`acct`（hybrid 种子的 v2 截断 hash 为 0）、`size`、`ver`
//! - `trk:clients` → JSON 客户端白名单；`trk:cfg` → HASH 运行参数
//! - `trk:sync` → 变更流 `kind`、`key`、`op`、`ver`（pt-tracker 据此刷新本地缓存）
//!
//! 本版本实现：启动与每 10 分钟全量同步（限制类变化没有事件，由全量同步兜底）、用户注册 / 换 passkey / 状态变化 / 删除与
//! 种子发布 / 状态变化 / 删除的实时同步、客户端白名单与参数下发。`ver` 由 Redis 计数器 `trk:ver` 生成（单调递增）。
//! 节点心跳、peer 查询、客户端规则后台编辑、分片配置在后续迭代实现。

use std::sync::Arc;

use async_trait::async_trait;
use pt_plugin_api::{
    Ctx, DeliveryInfo, EventEnvelope, JobInvocation, Lifecycle, NativePlugin, OnCron, OnEvent, PResult,
    PluginError, json,
};
use pt_plugin_identity::api::{USERS, UsersService};
use serde_json::Value;
use sqlx::Row;

pub const MANIFEST: &str = include_str!("../plugin.toml");

const ROLE: &str = "tracker_sync";

/// 写快照（HASH）并追加变更流；返回 ver
const PUT: &str = r"
local ver = redis.call('INCR', KEYS[3])
redis.call('DEL', KEYS[1])
local f = {}
for i = 3, #ARGV do f[#f + 1] = ARGV[i] end
f[#f + 1] = 'ver'; f[#f + 1] = ver
redis.call('HSET', KEYS[1], unpack(f))
redis.call('XADD', KEYS[2], 'MAXLEN', '~', '1000000', '*', 'kind', ARGV[1], 'key', ARGV[2], 'op', 'upsert', 'ver', ver)
return ver
";

/// 删除快照并追加变更流（键不存在时不追加）
const DEL: &str = r"
if redis.call('EXISTS', KEYS[1]) == 0 then return 0 end
local ver = redis.call('INCR', KEYS[3])
redis.call('DEL', KEYS[1])
redis.call('XADD', KEYS[2], 'MAXLEN', '~', '1000000', '*', 'kind', ARGV[1], 'key', ARGV[2], 'op', 'delete', 'ver', ver)
return ver
";

/// 写 JSON 快照（STRING）并追加变更流
const PUT_JSON: &str = r"
local ver = redis.call('INCR', KEYS[3])
-- ARGV[3] 是字段名（与 PUT 共用参数格式），ARGV[4] 是 JSON
local obj = cjson.decode(ARGV[4])
obj['ver'] = ver
redis.call('SET', KEYS[1], cjson.encode(obj))
redis.call('XADD', KEYS[2], 'MAXLEN', '~', '1000000', '*', 'kind', ARGV[1], 'key', ARGV[2], 'op', 'upsert', 'ver', ver)
return ver
";

fn redis_err(e: &redis::RedisError) -> PluginError {
    PluginError::new("tracker-bridge.redis_unavailable", format!("redis: {e}")).retryable()
}

async fn eval(
    ctx: &Ctx,
    script: &str,
    key: &str,
    kind: &str,
    id: &str,
    fields: &[(&str, String)],
) -> PResult<i64> {
    let mut conn = ctx.redis.global(ROLE)?;
    let mut cmd = redis::cmd("EVAL");
    cmd.arg(script)
        .arg(3)
        .arg(key)
        .arg("trk:sync")
        .arg("trk:ver")
        .arg(kind)
        .arg(id);
    for (k, v) in fields {
        cmd.arg(*k).arg(v);
    }
    cmd.query_async(&mut conn).await.map_err(|e| redis_err(&e))
}

/// 用户状态 → `st`：0 正常、1 禁止下载、2 不可用（封禁、待审核、已删除等）
pub fn user_state(status: &str) -> u8 {
    match status {
        "active" => 0,
        "leech_disabled" => 1,
        _ => 2,
    }
}

/// 写入（或换掉）一个用户的 passkey
async fn sync_user(ctx: &Ctx, uid: i64) -> PResult<()> {
    let users = ctx.service::<dyn UsersService>(USERS)?;
    let id = u64::try_from(uid).map_err(|_| PluginError::bad_request("bad user id"))?;
    let summary = users.call(move |s, c| s.get(c, id)).await?;
    let passkey = users.call(move |s, c| s.passkey_of(c, id)).await?;
    let mut conn = ctx.redis.global(ROLE)?;
    let old: Option<String> = redis::cmd("GET")
        .arg(format!("trk:u:{uid}"))
        .query_async(&mut conn)
        .await
        .map_err(|e| redis_err(&e))?;
    let current = summary.zip(passkey.filter(|p| p.len() == 32));
    if let Some(old) = old.filter(|o| current.as_ref().is_none_or(|(_, pk)| pk != o)) {
        eval(ctx, DEL, &format!("trk:pk:{old}"), "pk", &old, &[]).await?;
    }
    match current {
        Some((u, pk)) => {
            eval(
                ctx,
                PUT,
                &format!("trk:pk:{pk}"),
                "pk",
                &pk,
                &[
                    ("uid", uid.to_string()),
                    ("st", user_state(&u.status).to_string()),
                ],
            )
            .await?;
            let _: () = redis::cmd("SET")
                .arg(format!("trk:u:{uid}"))
                .arg(&pk)
                .query_async(&mut conn)
                .await
                .map_err(|e| redis_err(&e))?;
        }
        None => {
            let _: () = redis::cmd("DEL")
                .arg(format!("trk:u:{uid}"))
                .query_async(&mut conn)
                .await
                .map_err(|e| redis_err(&e))?;
        }
    }
    Ok(())
}

/// 种子的 hash 列表：(hex, 是否计账)。hybrid 种子的 v2 hash 截断为 20 字节，只参与 swarm、不计账
pub fn torrent_hashes(v1: Option<&[u8]>, v2: Option<&[u8]>) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    if let Some(h) = v1.filter(|h| h.len() == 20) {
        out.push((hex::encode(h), true));
    }
    if let Some(h) = v2.filter(|h| h.len() == 32) {
        out.push((hex::encode(&h[..20]), out.is_empty()));
    }
    out
}

async fn sync_torrent(ctx: &Ctx, tid: i64) -> PResult<()> {
    let mut c = ctx.sql.conn().await?;
    let row = sqlx::query(
        "SELECT info_hash_v1, info_hash_v2, size, status, deleted_at IS NOT NULL FROM torrent.v_torrent_tracker WHERE id = $1",
    )
    .bind(tid)
    .fetch_optional(&mut *c)
    .await?;
    drop(c);
    let Some(r) = row else { return Ok(()) };
    let v1: Option<Vec<u8>> = r.get(0);
    let v2: Option<Vec<u8>> = r.get(1);
    let size: i64 = r.get(2);
    let status: String = r.get(3);
    let deleted: bool = r.get(4);
    for (h, acct) in torrent_hashes(v1.as_deref(), v2.as_deref()) {
        let key = format!("trk:t:{h}");
        if deleted {
            eval(ctx, DEL, &key, "t", &h, &[]).await?;
        } else {
            let st = u8::from(status != "approved");
            eval(
                ctx,
                PUT,
                &key,
                "t",
                &h,
                &[
                    ("tid", tid.to_string()),
                    ("st", st.to_string()),
                    ("acct", u8::from(acct).to_string()),
                    ("size", size.to_string()),
                ],
            )
            .await?;
        }
    }
    Ok(())
}

/// 默认客户端白名单（Azureus 风格 peer_id 前缀）
const DEFAULT_CLIENTS: &[(&str, &str)] = &[
    ("qBittorrent", "-qB"),
    ("Transmission", "-TR"),
    ("Deluge", "-DE"),
    ("libtorrent (Rasterbar)", "-lt"),
    ("rTorrent / libTorrent", "-LT"),
    ("µTorrent", "-UT"),
    ("µTorrent Mac", "-UM"),
    ("Vuze / Azureus", "-AZ"),
    ("BiglyBT", "-BI"),
];

/// 客户端白名单与运行参数（来自后台设置）
pub fn client_rules(settings: &Value) -> Value {
    let rules: Vec<Value> = match settings["clients"].as_array().filter(|a| !a.is_empty()) {
        Some(list) => list
            .iter()
            .filter_map(|r| {
                Some(json!({ "name": r["name"].as_str()?, "peer_id_prefix": r["peer_id_prefix"].as_str()?, "action": "allow" }))
            })
            .collect(),
        None => DEFAULT_CLIENTS
            .iter()
            .map(|(n, p)| json!({ "name": n, "peer_id_prefix": p, "action": "allow" }))
            .collect(),
    };
    let default = if settings["allow_unknown_clients"].as_bool() == Some(true) {
        "allow"
    } else {
        "deny"
    };
    json!({ "default": default, "rules": rules })
}

async fn sync_config(ctx: &Ctx) -> PResult<()> {
    let s = ctx.config.get_settings().await;
    let int = |k: &str, d: i64| s[k].as_i64().unwrap_or(d).to_string();
    eval(
        ctx,
        PUT,
        "trk:cfg",
        "cfg",
        "cfg",
        &[
            ("interval", int("interval", 1800)),
            ("min_interval", int("min_interval", 900)),
            ("max_peers", int("max_peers", 50)),
        ],
    )
    .await?;
    let rules = client_rules(&s).to_string();
    eval(
        ctx,
        PUT_JSON,
        "trk:clients",
        "clients",
        "clients",
        &[("json", rules)],
    )
    .await?;
    Ok(())
}

/// 全量同步：参数、白名单、全部用户的 passkey、全部种子
async fn full_sync(ctx: &Ctx) -> PResult<(usize, usize)> {
    sync_config(ctx).await?;
    let mut c = ctx.sql.conn().await?;
    let users: Vec<i64> = sqlx::query_scalar("SELECT id FROM identity.v_user_public ORDER BY id")
        .fetch_all(&mut *c)
        .await?;
    let torrents: Vec<i64> = sqlx::query_scalar("SELECT id FROM torrent.v_torrent_tracker ORDER BY id")
        .fetch_all(&mut *c)
        .await?;
    drop(c);
    for u in &users {
        if let Err(e) = sync_user(ctx, *u).await {
            tracing::warn!(user = u, error = %e.message, "tracker sync failed for user");
        }
    }
    for t in &torrents {
        sync_torrent(ctx, *t).await?;
    }
    Ok((users.len(), torrents.len()))
}

struct Bridge;

#[async_trait]
impl Lifecycle for Bridge {
    async fn init(&self, ctx: &Ctx) -> PResult<()> {
        // 同步失败不阻止站点启动（Redis 恢复后由定时任务补齐）
        match full_sync(ctx).await {
            Ok((u, t)) => tracing::info!(users = u, torrents = t, "tracker state synced"),
            Err(e) => tracing::warn!(error = %e.message, "initial tracker sync failed"),
        }
        Ok(())
    }
}

#[async_trait]
impl OnEvent for Bridge {
    async fn handle(&self, ctx: &Ctx, evt: EventEnvelope, _d: DeliveryInfo) -> PResult<()> {
        let p = &evt.payload;
        if evt.event_type.starts_with("user.") {
            let uid = p["user_id"]
                .as_i64()
                .ok_or_else(|| PluginError::validation("user_id"))?;
            return sync_user(ctx, uid).await;
        }
        let tid = p["torrent_id"]
            .as_i64()
            .ok_or_else(|| PluginError::validation("torrent_id"))?;
        sync_torrent(ctx, tid).await
    }
}

#[async_trait]
impl OnCron for Bridge {
    async fn run(&self, ctx: &Ctx, job: JobInvocation) -> PResult<()> {
        if job.name != "resync" {
            return Err(PluginError::not_found("unknown job"));
        }
        full_sync(ctx).await.map(|_| ())
    }
}

pub fn plugin() -> NativePlugin {
    let b = Arc::new(Bridge);
    NativePlugin {
        manifest: MANIFEST,
        lifecycle: Some(b.clone()),
        on_event: Some(b.clone()),
        on_cron: Some(b),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states() {
        assert_eq!(user_state("active"), 0);
        assert_eq!(user_state("leech_disabled"), 1);
        assert_eq!(user_state("banned"), 2);
        assert_eq!(user_state("pending"), 2);
    }

    #[test]
    fn hashes() {
        let v1 = [1u8; 20];
        let v2 = [2u8; 32];
        assert_eq!(
            torrent_hashes(Some(&v1), Some(&v2)),
            vec![(hex::encode(v1), true), (hex::encode(&v2[..20]), false)]
        );
        assert_eq!(
            torrent_hashes(None, Some(&v2)),
            vec![(hex::encode(&v2[..20]), true)]
        );
        assert!(torrent_hashes(Some(&[0u8; 3]), None).is_empty());
    }

    #[test]
    fn clients() {
        let r = client_rules(&json!({}));
        assert_eq!(r["default"], "deny");
        assert!(
            r["rules"]
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x["peer_id_prefix"] == "-qB"))
        );
        let r = client_rules(
            &json!({ "allow_unknown_clients": true, "clients": [{ "name": "x", "peer_id_prefix": "-XX" }] }),
        );
        assert_eq!(
            (r["default"].as_str(), r["rules"][0]["peer_id_prefix"].as_str()),
            (Some("allow"), Some("-XX"))
        );
    }
}
