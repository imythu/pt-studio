//! tracker-bridge 插件（Native，docs/plugins/tracker-bridge/）：pt-tracker 的数据来源。
//!
//! 写入 Redis（`tracker_sync` 角色，键格式见 design/10 §5.5 与 catalog/redis-keys.md）：
//! - `trk:pk:<passkey>` → HASH `uid`、`st`（0 正常、1 禁止下载、2 不可用）、`ver`；`trk:u:<uid>` → 当前 passkey（反向索引，换 key 时删旧键）
//! - `trk:t:<info_hash_hex>` → HASH `tid`、`st`（0 正常、1 不可用）、`acct`（hybrid 种子的 v2 截断 hash 为 0）、`size`、`ver`
//! - `trk:clients` → JSON 客户端规则（[`clients`]，表 `client_rules`）；`trk:cfg` → HASH 运行参数
//! - `trk:sync` → 变更流 `kind`、`key`、`op`、`ver`（pt-tracker 据此刷新本地缓存）
//!
//! 本版本实现：启动与每 10 分钟全量同步（限制类变化没有事件，由全量同步兜底）、用户注册 / 换 passkey / 状态变化 / 删除与
//! 种子发布 / 状态变化 / 删除的实时同步、客户端规则与参数下发。`ver` 由 Redis 计数器 `trk:ver` 生成（单调递增）。
//! 服务 `ctx.tracker.peers`：从 pt-tracker 的 `ps:{H}`（`tracker_state` 角色，只读）读取种子的当前 peer；
//! 返回 IP 须调用方获授敏感能力 `peer.ip`（读取写审计，IP 不落库）。
//! 节点状态 `GET /v1/admin/status`：读 pt-tracker 每 10 秒写出的 `trk:stats:node:<node_id>`（TTL 30 秒，键在即在线），
//! 对比 `trk:sync` 最新条目给出落后时长，并检查 announce 间隔不小于节点的 peer 超时。
//! 客户端规则后台（[`admin`]）：读取 / 整体替换（`If-Match`）/ 测试 / 预设模板、被拒客户端排行；
//! cron `node_health` 每分钟检查节点延迟与失联（指标 + `tracker.inconsistency_detected`）。
//! 一致性校验、手动重建接口、分片配置在后续迭代实现。

mod admin;
pub mod clients;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Json, Router,
    routing::{get, post},
};
use pt_plugin_api::{
    Ctx, DeliveryInfo, EventEnvelope, HttpRoutes, JobInvocation, Lifecycle, NativePlugin, OnCron, OnEvent,
    PResult, PluginError, RequestCtx, json,
};
use pt_plugin_identity::api::{USERS, UsersService};
use serde_json::Value;
use sqlx::Row;

pub const MANIFEST: &str = include_str!("../plugin.toml");
pub const MIGRATIONS: &[(&str, &str)] = &[(
    "0001_client_rules.sql",
    include_str!("../migrations/0001_client_rules.sql"),
)];

pub const TRACKER: &str = "ctx.tracker";

const ROLE: &str = "tracker_sync";
/// pt-tracker 的 swarm 会话状态
const STATE_ROLE: &str = "tracker_state";
/// 最近一次 announce 在此之内的会话视为在线（与 accounting `v_swarm_peers` 一致）
const PEER_FRESH_SECS: u64 = 3600;
/// 单次查询最多返回的 peer 数
const MAX_PEERS: usize = 2000;
/// 节点落后变更流超过此时长视为同步延迟（FR-TRB-020 默认 5 秒）
const NODE_LAG_ALERT_MS: u64 = 5000;

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

/// 删除快照，但只在键仍属于这条记录时（ARGV[3] 字段等于 ARGV[4]）：同一 hash 删除后可以重新发布，
/// 旧记录不能动新记录的键
const DEL_OWNED: &str = r"
if redis.call('HGET', KEYS[1], ARGV[3]) ~= ARGV[4] then return 0 end
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
            eval(ctx, DEL_OWNED, &key, "t", &h, &[("tid", tid.to_string())]).await?;
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

/// 旧版默认客户端白名单（Azureus 风格 peer_id 前缀）：没有任何规则时迁移为规则集
const DEFAULT_CLIENTS: &[(&str, &str)] = &[
    ("qBittorrent", "-qB"),
    ("Transmission", "-TR"),
    ("Deluge", "-DE"),
    ("rTorrent / libTorrent", "-lt"),
    ("libtorrent (Rasterbar)", "-LT"),
    ("µTorrent", "-UT"),
    ("µTorrent Mac", "-UM"),
    ("Vuze / Azureus", "-AZ"),
    ("BiglyBT", "-BI"),
];

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
    let set = admin::ensure(ctx).await?;
    admin::export(ctx, &set).await?;
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

/// `ps:{H}` 的一条会话（pt-tracker `store::Stored`）
pub fn parse_session(field: &str, state: &str, fresh_after: u64) -> Option<Value> {
    let (uid, pid_hex) = field.split_once(':')?;
    let uid: u64 = uid.parse().ok()?;
    let pid = hex::decode(pid_hex).ok().filter(|p| p.len() == 20)?;
    let s: Value = serde_json::from_str(state).ok()?;
    let ts = s["ts"].as_u64()?;
    if ts < fresh_after {
        return None;
    }
    let prefix: String = pid[..8]
        .iter()
        .map(|b| if b.is_ascii_graphic() { *b as char } else { '?' })
        .collect();
    let left = s["left"].as_u64().unwrap_or(0);
    Some(json!({
        "user_id": uid, "peer_id_prefix": prefix, "seeder": left == 0,
        "uploaded": s["up"].as_u64().unwrap_or(0), "downloaded": s["down"].as_u64().unwrap_or(0), "left": left,
        "last_announce_at": ts, "ip": s["ip"].as_str(), "alt": s["alt"].as_str(),
    }))
}

/// `ctx.tracker.peers {torrent_id, include_ip}`：种子当前的 peer（hybrid 种子合并 v1 与截断 v2 两个 swarm）
async fn peers(ctx: &Ctx, tid: i64, include_ip: bool) -> PResult<Value> {
    if include_ip && !ctx.invocation.has_scope("sensitive:peer.ip") {
        return Err(PluginError::capability_denied(
            "sensitive capability peer.ip required",
        ));
    }
    let mut c = ctx.sql.conn().await?;
    let row = sqlx::query(
        "SELECT info_hash_v1, info_hash_v2 FROM torrent.v_torrent_tracker WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(tid)
    .fetch_optional(&mut *c)
    .await?;
    drop(c);
    let Some(r) = row else {
        return Err(PluginError::not_found("torrent not found"));
    };
    let v1: Option<Vec<u8>> = r.get(0);
    let v2: Option<Vec<u8>> = r.get(1);
    let fresh_after = u64::try_from(ctx.time.now() / 1000)
        .unwrap_or(0)
        .saturating_sub(PEER_FRESH_SECS);
    let mut conn = ctx.redis.global(STATE_ROLE)?;
    let mut out = Vec::new();
    let mut truncated = false;
    'hashes: for (h, _) in torrent_hashes(v1.as_deref(), v2.as_deref()) {
        let key = format!("ps:{{{h}}}");
        let mut cursor = 0u64;
        loop {
            let (next, kv): (u64, Vec<String>) = redis::cmd("HSCAN")
                .arg(&key)
                .arg(cursor)
                .arg("COUNT")
                .arg(500)
                .query_async(&mut conn)
                .await
                .map_err(|e| redis_err(&e))?;
            for [field, state] in kv.as_chunks::<2>().0 {
                if let Some(mut p) = parse_session(field, state, fresh_after) {
                    if out.len() >= MAX_PEERS {
                        truncated = true;
                        break 'hashes;
                    }
                    if !include_ip && let Some(o) = p.as_object_mut() {
                        o.remove("ip");
                        o.remove("alt");
                    }
                    out.push(p);
                }
            }
            if next == 0 {
                break;
            }
            cursor = next;
        }
    }
    if include_ip {
        ctx.audit
            .write(pt_plugin_api::AuditEntry {
                action: "tracker-bridge.peer_ip_read".into(),
                target: format!("torrent:{tid}"),
                details: json!({ "caller": ctx.invocation.caller_plugin, "peers": out.len() }),
            })
            .await?;
    }
    Ok(json!({ "peers": out, "truncated": truncated }))
}

/// `XRANGE` / `XREVRANGE` 的应答
type StreamEntries = Vec<(String, Vec<(String, String)>)>;

/// 流 ID `<ms>-<seq>` → (ms, seq)
fn stream_id(id: &str) -> Option<(u64, u64)> {
    let (ms, seq) = id.split_once('-')?;
    Some((ms.parse().ok()?, seq.parse().ok()?))
}

/// 一个节点状态键 → 接口形式；`last` 为 `trk:sync` 最新条目 ID（流为空时为 None）
fn node_status(node_id: &str, h: &HashMap<String, String>, last: Option<&str>, now_ms: u64) -> Value {
    let num = |k: &str| h.get(k).and_then(|v| v.parse::<u64>().ok());
    let sid = h.get("sid").map(String::as_str).unwrap_or_default();
    // 落后：节点已应用的条目早于流的最新条目时，按最新条目写入时刻计算已等待的时长
    let behind_ms = match (stream_id(sid), last.and_then(stream_id)) {
        (Some(a), Some(b)) if a < b => now_ms.saturating_sub(b.0),
        (None, Some(b)) => now_ms.saturating_sub(b.0),
        _ => 0,
    };
    json!({
        "node_id": node_id,
        "version": h.get("ver"),
        "boot_at": num("boot").map(|s| s * 1000),
        "sync_id": sid,
        "lag_ms": num("lag_ms").unwrap_or(0),
        "behind_ms": behind_ms,
        "passkeys": num("pk").unwrap_or(0),
        "torrents_cached": num("t").unwrap_or(0),
        "max_peer_timeout": num("mpt"),
        "rps": h.get("rps").and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0),
        "updated_at": num("ts").map(|s| s * 1000),
    })
}

/// 节点状态键与 `trk:sync` 的快照
struct Nodes {
    nodes: Vec<Value>,
    length: u64,
    last: Option<String>,
}

/// 读取全部节点状态键（`SCAN trk:stats:node:*`）与变更流的长度、最新条目
async fn nodes(ctx: &Ctx) -> PResult<Nodes> {
    let mut conn = ctx.redis.global(ROLE)?;
    let mut keys: Vec<String> = Vec::new();
    let mut cursor = 0u64;
    loop {
        let (next, batch): (u64, Vec<String>) = redis::cmd("SCAN")
            .arg(cursor)
            .arg("MATCH")
            .arg("trk:stats:node:*")
            .arg("COUNT")
            .arg(100)
            .query_async(&mut conn)
            .await
            .map_err(|e| redis_err(&e))?;
        keys.extend(batch);
        if next == 0 {
            break;
        }
        cursor = next;
    }
    keys.sort();
    let (length, last): (u64, StreamEntries) = redis::pipe()
        .cmd("XLEN")
        .arg("trk:sync")
        .cmd("XREVRANGE")
        .arg("trk:sync")
        .arg("+")
        .arg("-")
        .arg("COUNT")
        .arg(1)
        .query_async(&mut conn)
        .await
        .map_err(|e| redis_err(&e))?;
    let last = last.into_iter().next().map(|e| e.0);
    let now_ms = u64::try_from(ctx.time.now()).unwrap_or(0);
    let mut nodes = Vec::new();
    for k in &keys {
        let h: HashMap<String, String> = redis::cmd("HGETALL")
            .arg(k)
            .query_async(&mut conn)
            .await
            .map_err(|e| redis_err(&e))?;
        // 扫描与读取之间过期的键跳过
        if let (Some(id), false) = (k.strip_prefix("trk:stats:node:"), h.is_empty()) {
            nodes.push(node_status(id, &h, last.as_deref(), now_ms));
        }
    }
    Ok(Nodes { nodes, length, last })
}

/// 节点与同步状态（tracker-bridge LLD §1.9）
async fn status(ctx: RequestCtx) -> PResult<Json<Value>> {
    let Nodes { nodes, length, last } = nodes(&ctx).await?;
    let s = ctx.config.get_settings().await;
    let interval = s["interval"].as_u64().unwrap_or(1800);
    let mut warnings = Vec::new();
    for n in &nodes {
        if n["behind_ms"].as_u64().unwrap_or(0) > NODE_LAG_ALERT_MS {
            warnings.push(json!({ "code": "node_lag", "node_id": n["node_id"] }));
        }
        // 间隔不小于 peer 超时会让正常客户端在两次 announce 之间被当作离线
        if n["max_peer_timeout"].as_u64().is_some_and(|m| interval >= m) {
            warnings.push(json!({ "code": "interval_exceeds_peer_timeout", "node_id": n["node_id"] }));
        }
    }
    // 曾经上报、现在失联的节点（node_health 每分钟更新）
    let mut lost: Vec<_> = admin::health(&ctx)
        .await?
        .nodes
        .into_iter()
        .filter(|(_, n)| n.lost)
        .map(|(id, n)| (id, n.last_seen))
        .collect();
    lost.sort();
    for (id, seen) in &lost {
        warnings.push(json!({ "code": "node_lost", "node_id": id, "last_seen_at": seen }));
    }
    if nodes.is_empty() {
        warnings.push(json!({ "code": "no_nodes" }));
    }
    Ok(Json(json!({
        "nodes": nodes,
        "stream": { "length": length, "last_id": last },
        "interval": interval,
        "warnings": warnings,
    })))
}

struct Routes;

impl HttpRoutes for Routes {
    fn router(&self) -> Router {
        Router::new()
            .route("/v1/admin/status", get(status))
            .route(
                "/v1/admin/client-rules",
                get(admin::get_rules).put(admin::put_rules),
            )
            .route("/v1/admin/client-rules:test", post(admin::test_rules))
            .route("/v1/admin/client-rule-presets", get(admin::presets))
            .route("/v1/admin/denied-clients", get(admin::denied))
    }
}

struct Svc;

#[async_trait]
impl pt_plugin_api::JsonService for Svc {
    async fn call(&self, ctx: &Ctx, method: &str, a: Value) -> PResult<Value> {
        match method {
            "peers" => {
                let tid = a["torrent_id"]
                    .as_i64()
                    .filter(|t| *t > 0)
                    .ok_or_else(|| PluginError::validation("torrent_id"))?;
                peers(ctx, tid, a["include_ip"].as_bool().unwrap_or(false)).await
            }
            other => Err(PluginError::new(
                "kernel.not_found",
                format!("ctx.tracker has no method `{other}`"),
            )
            .with_status(404)),
        }
    }
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
        match job.name.as_str() {
            "resync" => full_sync(ctx).await.map(|_| ()),
            "node_health" => admin::node_health(ctx).await,
            _ => Err(PluginError::not_found("unknown job")),
        }
    }
}

pub fn plugin() -> NativePlugin {
    let b = Arc::new(Bridge);
    NativePlugin {
        manifest: MANIFEST,
        migrations: MIGRATIONS,
        lifecycle: Some(b.clone()),
        on_event: Some(b.clone()),
        on_cron: Some(b),
        http: Some(Arc::new(Routes)),
        json_services: vec![(TRACKER, Arc::new(Svc))],
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions() {
        let pid = format!("{}{}", hex::encode(b"-qB5120-"), "ab".repeat(12));
        let st = r#"{"up":10,"down":5,"left":0,"ts":1000,"completed":true,"ip":"203.0.113.9","port":51413}"#;
        let p = parse_session(&format!("7:{pid}"), st, 900).expect("fresh session parses");
        assert_eq!(p["user_id"], 7);
        assert_eq!(p["peer_id_prefix"], "-qB5120-");
        assert_eq!(
            (p["seeder"].as_bool(), p["uploaded"].as_u64()),
            (Some(true), Some(10))
        );
        assert_eq!(p["ip"], "203.0.113.9");
        // 过期会话、格式不对的 field 跳过
        assert!(parse_session(&format!("7:{pid}"), st, 2000).is_none());
        assert!(parse_session("7:zz", st, 0).is_none());
    }

    #[test]
    fn nodes() {
        let h: HashMap<String, String> = [
            ("ver", "0.1.0"),
            ("boot", "100"),
            ("sid", "5000-0"),
            ("lag_ms", "12"),
            ("pk", "3"),
            ("t", "2"),
            ("mpt", "2700"),
            ("rps", "1.5"),
            ("ts", "200"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
        let n = node_status("n1", &h, Some("5000-0"), 9000);
        assert_eq!(
            (n["behind_ms"].as_u64(), n["boot_at"].as_u64()),
            (Some(0), Some(100_000))
        );
        assert_eq!((n["passkeys"].as_u64(), n["rps"].as_f64()), (Some(3), Some(1.5)));
        // 落后：按流最新条目的时刻算等待时长
        assert_eq!(node_status("n1", &h, Some("7000-1"), 9000)["behind_ms"], 2000);
        assert_eq!(node_status("n1", &h, Some("5000-1"), 9000)["behind_ms"], 4000);
        // 流为空、节点尚未应用任何条目
        assert_eq!(node_status("n1", &h, None, 9000)["behind_ms"], 0);
        assert_eq!(stream_id("0-0"), Some((0, 0)));
        assert!(stream_id("x").is_none());
    }

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
}
