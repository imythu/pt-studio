//! accounting 插件（docs/plugins/accounting/）：消费 pt-tracker 的 `acct:<p>` 流，按时间窗口结算流量（`ctx.accounting`）。
//!
//! 本版本实现：分区 worker（微批、eid 去重、倍率、peers / snatches / 用户总量 / 日统计、结算窗口与
//! `traffic.settled`、首次完成与 `traffic.snatch_completed`、游标与 fencing 的恰好一次提交）、倍率规则服务、
//! 流量调整、种子镜像、统计 HTTP 接口、基础信号与交叉校验（`traffic.signal_raised`）。swarm 统计、重算在后续迭代实现。

pub mod ingest;
pub mod rates;
pub mod signals;
pub mod snapshot;

use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use pt_plugin_api::{
    Ctx, DeliveryInfo, EventEnvelope, Lifecycle, NativePlugin, NewEvent, OnEvent, PResult, PartitionWorker,
    PluginError, PollOutcome, RequestCtx, json, service_object,
};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::rates::{EffectiveRate, RateIndex, RateRule};

pub const MANIFEST: &str = include_str!("../plugin.toml");
pub const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_init.sql", include_str!("../migrations/0001_init.sql")),
    (
        "0002_hourly_slots.sql",
        include_str!("../migrations/0002_hourly_slots.sql"),
    ),
    (
        "0003_torrent_peers_view.sql",
        include_str!("../migrations/0003_torrent_peers_view.sql"),
    ),
    ("0004_signals.sql", include_str!("../migrations/0004_signals.sql")),
];
pub const ACCOUNTING: &str = "ctx.accounting";

/// 静态配置（`site.toml [plugin.accounting]`，03 §4）。
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "d_parts")]
    pub default_partitions: u32,
    #[serde(default = "d_window")]
    pub window_secs: u32,
    #[serde(default = "d_grace")]
    pub settle_grace_secs: u32,
    #[serde(default = "d_batch")]
    pub batch_max_entries: u32,
    #[serde(default = "d_idle")]
    pub idle_sleep_ms: u64,
    #[serde(default = "d_max_up")]
    pub max_up_permille: u32,
    #[serde(default)]
    pub signals: signals::SignalCfg,
}

fn d_parts() -> u32 {
    64
}
fn d_window() -> u32 {
    900
}
fn d_grace() -> u32 {
    10
}
fn d_batch() -> u32 {
    5000
}
/// 空闲分区的轮询间隔：64 个分区 × 每秒一次；窗口 900 s，1 s 延迟无影响
fn d_idle() -> u64 {
    1000
}
fn d_max_up() -> u32 {
    10_000
}

/// 插件进程内状态。
#[derive(Debug, Default)]
pub struct State {
    cfg: OnceLock<Config>,
    pub eids: ingest::EidCache,
    /// (索引, 版本号, 装载时间 ms)
    rates: Mutex<(Arc<RateIndex>, i64, i64)>,
}

impl State {
    pub fn cfg(&self) -> PResult<&Config> {
        self.cfg
            .get()
            .ok_or_else(|| PluginError::unavailable("accounting is not initialised"))
    }

    pub fn rate_index(&self) -> Arc<RateIndex> {
        self.rates.lock().map(|g| g.0.clone()).unwrap_or_default()
    }

    /// 规则版本号变化或超过 10 s 时重载索引（03 §1.3）。
    pub async fn refresh_rates(&self, ctx: &Ctx) -> PResult<()> {
        let ver = ctx
            .redis
            .call(pt_plugin_api::RedisCommand::new("GET").arg("rate:ver"))
            .await?
            .as_int()
            .unwrap_or(0);
        let now = chrono::Utc::now().timestamp_millis();
        let (cur_ver, loaded) = self.rates.lock().map(|g| (g.1, g.2)).unwrap_or((-1, 0));
        if ver == cur_ver && now - loaded < 10_000 {
            return Ok(());
        }
        let idx = {
            let mut c = ctx.sql.conn().await?;
            rates::load_index(&mut c).await?
        };
        if let Ok(mut g) = self.rates.lock() {
            *g = (Arc::new(idx), ver, now);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserTotals {
    pub user_id: u64,
    pub uploaded: i64,
    pub downloaded: i64,
    pub uploaded_raw: i64,
    pub downloaded_raw: i64,
    pub seed_secs: i64,
    pub leech_secs: i64,
    pub completed_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdjustResult {
    pub adjustment_id: u64,
    pub applied_up: i64,
    pub applied_down: i64,
    pub duplicate: bool,
}

/// `ctx.accounting` 服务契约（03 §2.1 子集）。
#[async_trait]
pub trait AccountingService: Send + Sync {
    async fn user_totals(&self, ctx: &Ctx, user: u64) -> PResult<UserTotals>;
    async fn user_totals_many(&self, ctx: &Ctx, users: &[u64]) -> PResult<Vec<UserTotals>>;
    async fn seeding_now(&self, ctx: &Ctx, user: u64) -> PResult<Vec<u64>>;
    async fn set_rate_rule(&self, ctx: &Ctx, rule: RateRule) -> PResult<u64>;
    async fn remove_rate_rule(&self, ctx: &Ctx, rule_id: u64) -> PResult<()>;
    async fn effective_rate(
        &self,
        ctx: &Ctx,
        user: u64,
        torrent_id: u64,
        at: Option<i64>,
    ) -> PResult<EffectiveRate>;
    async fn adjust(
        &self,
        ctx: &Ctx,
        user: u64,
        d_up: i64,
        d_down: i64,
        reason: &str,
        idempotency_key: &str,
    ) -> PResult<AdjustResult>;
    /// 用户的基础信号（作用域 `read:signals`，区间 ≤ 366 天）
    async fn signals(
        &self,
        ctx: &Ctx,
        user: u64,
        from: i64,
        to: i64,
        cursor: Option<String>,
    ) -> PResult<signals::SignalPage>;
}

fn scope(ctx: &Ctx, s: &str) -> PResult<()> {
    let own = ctx.invocation.kind != pt_plugin_api::InvocationKind::Service;
    if own || ctx.invocation.has_scope(s) {
        Ok(())
    } else {
        Err(PluginError::new("accounting.scope_required", format!("scope `{s}` required")).with_status(403))
    }
}

fn u2i(u: u64) -> PResult<i64> {
    i64::try_from(u).map_err(|_| PluginError::bad_request("id out of range"))
}

pub async fn totals_many(ctx: &Ctx, users: &[i64]) -> PResult<Vec<UserTotals>> {
    if users.len() > 500 {
        return Err(PluginError::new("accounting.too_many_ids", "at most 500 ids"));
    }
    let mut c = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT x.u, coalesce(t.uploaded_traffic + t.uploaded_adjust, 0), coalesce(t.downloaded_traffic + t.downloaded_adjust, 0),
                coalesce(t.uploaded_raw, 0), coalesce(t.downloaded_raw, 0), coalesce(t.seed_secs, 0), coalesce(t.leech_secs, 0),
                coalesce(t.completed_count, 0)::bigint
           FROM unnest($1::bigint[]) AS x(u) LEFT JOIN user_totals t ON t.user_id = x.u AND NOT t.hidden ORDER BY x.u",
    )
    .bind(users)
    .fetch_all(&mut *c)
    .await?;
    Ok(rows
        .iter()
        .map(|r| UserTotals {
            user_id: u64::try_from(r.get::<i64, _>(0)).unwrap_or(0),
            uploaded: r.get(1),
            downloaded: r.get(2),
            uploaded_raw: r.get(3),
            downloaded_raw: r.get(4),
            seed_secs: r.get(5),
            leech_secs: r.get(6),
            completed_count: r.get(7),
        })
        .collect())
}

/// 流量调整（03 §2.1 `adjust`）：同一 (调用方, 键) 幂等；结果不得使计入总量为负。
#[allow(clippy::too_many_arguments)]
pub async fn adjust(
    ctx: &Ctx,
    caller: &str,
    user: i64,
    d_up: i64,
    d_down: i64,
    reason: &str,
    note: Option<&str>,
    key: &str,
) -> PResult<AdjustResult> {
    let limit = 1i64 << 50;
    if d_up.abs() > limit || d_down.abs() > limit || (d_up == 0 && d_down == 0) {
        return Err(PluginError::new(
            "accounting.adjust_out_of_range",
            "adjustment out of range",
        ));
    }
    let reason_ok = reason.split_once('.').is_some_and(|(p, c)| {
        p == caller
            && !c.is_empty()
            && c.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    });
    if !reason_ok || !(1..=128).contains(&key.len()) {
        return Err(PluginError::validation(
            "reason must be `<caller>.<code>` and key 1-128 chars",
        ));
    }
    let hash = <sha2::Sha256 as sha2::Digest>::digest(format!("{user}:{d_up}:{d_down}:{reason}").as_bytes())
        .to_vec();
    let mut c = ctx.sql.conn().await?;
    if let Some(r) = sqlx::query("SELECT id, request_hash, applied_up, applied_down FROM adjustments WHERE caller_plugin = $1 AND idempotency_key = $2")
        .bind(caller)
        .bind(key)
        .fetch_optional(&mut *c)
        .await?
    {
        if r.get::<Vec<u8>, _>(1) != hash {
            return Err(PluginError::new("kernel.idempotency_key_reused", "idempotency key was used for a different request"));
        }
        return Ok(AdjustResult { adjustment_id: u64::try_from(r.get::<i64, _>(0)).unwrap_or(0), applied_up: r.get(2), applied_down: r.get(3), duplicate: true });
    }
    // 先 UPDATE：INSERT … ON CONFLICT 会先对待插入行检查 CHECK 约束（负调整必然失败）
    let mut r = sqlx::query(
        "UPDATE user_totals SET uploaded_adjust = uploaded_adjust + $2, downloaded_adjust = downloaded_adjust + $3, updated_at = now()
          WHERE user_id = $1",
    )
    .bind(user)
    .bind(d_up)
    .bind(d_down)
    .execute(&mut *c)
    .await;
    if matches!(&r, Ok(done) if done.rows_affected() == 0) {
        r = sqlx::query(
            "INSERT INTO user_totals (user_id, uploaded_adjust, downloaded_adjust) VALUES ($1, $2, $3)",
        )
        .bind(user)
        .bind(d_up)
        .bind(d_down)
        .execute(&mut *c)
        .await;
    }
    match r {
        Ok(_) => {}
        Err(sqlx::Error::Database(db)) if db.constraint().is_some_and(|n| n.ends_with("_nonneg_check")) => {
            return Err(PluginError::new(
                "accounting.adjust_would_be_negative",
                "adjustment would make totals negative",
            ));
        }
        Err(e) => return Err(e.into()),
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO adjustments (user_id, d_up, d_down, applied_up, applied_down, reason, note, caller_plugin, actor_user_id, idempotency_key, request_hash)
         VALUES ($1,$2,$3,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING id",
    )
    .bind(user)
    .bind(d_up)
    .bind(d_down)
    .bind(reason)
    .bind(note)
    .bind(caller)
    .bind(ctx.user_id().and_then(|u| i64::try_from(u).ok()))
    .bind(key)
    .bind(&hash)
    .fetch_one(&mut *c)
    .await?;
    drop(c);
    let by = match ctx.user_id() {
        Some(u) if caller == "accounting" => json!({ "kind": "user", "user_id": u }),
        _ => json!({ "kind": "plugin", "plugin_id": caller }),
    };
    ctx.events
        .publish(NewEvent::v1("traffic.adjusted", format!("user:{user}"), json!({
            "user_id": user, "adjustment_id": id, "d_up": d_up, "d_down": d_down, "reason": reason, "by": by })))
        .await?;
    Ok(AdjustResult {
        adjustment_id: u64::try_from(id).unwrap_or(0),
        applied_up: d_up,
        applied_down: d_down,
        duplicate: false,
    })
}

struct Accounting(Arc<State>);

#[async_trait]
impl Lifecycle for Accounting {
    async fn init(&self, ctx: &Ctx) -> PResult<()> {
        let cfg: Config = serde_json::from_value(ctx.config.get_static())
            .map_err(|e| PluginError::validation(format!("site.toml [plugin.accounting]: {e}")))?;
        if cfg.window_secs == 0 || cfg.batch_max_entries == 0 {
            return Err(PluginError::validation(
                "window_secs and batch_max_entries must be positive",
            ));
        }
        let _ = self.0.cfg.set(cfg);
        Ok(())
    }
}

#[async_trait]
impl PartitionWorker for Accounting {
    async fn poll(&self, ctx: &Ctx, partition: u32, fence: u64) -> PResult<PollOutcome> {
        ingest::poll(&self.0, ctx, partition, fence).await
    }
}

#[async_trait]
impl pt_plugin_api::OnCron for Accounting {
    async fn run(&self, ctx: &Ctx, job: pt_plugin_api::JobInvocation) -> PResult<()> {
        match job.name.as_str() {
            "seeding_snapshot" => {
                let cfg = self.0.cfg()?;
                let mut conn = ctx.redis.global("acct")?;
                let parts: u32 = redis::cmd("HGET")
                    .arg("acct:_meta")
                    .arg("partitions")
                    .query_async::<Option<u32>>(&mut conn)
                    .await?
                    .unwrap_or(cfg.default_partitions);
                // 手动运行可指定 {"hour": H(t), "force": true}
                let hour = job.payload.get("hour").and_then(serde_json::Value::as_i64);
                let force = job
                    .payload
                    .get("force")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);
                let n = snapshot::run(ctx, parts, cfg.settle_grace_secs, hour, force).await?;
                tracing::info!(users = n, "seeding snapshot done");
                Ok(())
            }
            "cross_check" => {
                let cfg = self.0.cfg()?;
                let n = signals::cross_check(ctx, &cfg.signals).await?;
                signals::purge_dedupe(ctx).await?;
                tracing::info!(signals = n, "cross check done");
                Ok(())
            }
            other => Err(PluginError::internal(format!("unknown job `{other}`"))),
        }
    }
}

#[async_trait]
impl OnEvent for Accounting {
    async fn handle(&self, ctx: &Ctx, evt: EventEnvelope, _d: DeliveryInfo) -> PResult<()> {
        // 种子镜像（03 §1.12）：体积供信号与快照使用
        let id = evt.payload["torrent_id"]
            .as_i64()
            .ok_or_else(|| PluginError::validation("torrent_id"))?;
        let size = evt.payload["size"].as_i64().unwrap_or(0).max(0);
        let status = evt.payload["status"].as_str().unwrap_or("approved");
        let mut c = ctx.sql.conn().await?;
        sqlx::query(
            "INSERT INTO torrents (id, size, status) VALUES ($1, $2, $3)
             ON CONFLICT (id) DO UPDATE SET size = EXCLUDED.size, status = EXCLUDED.status, updated_at = now()",
        )
        .bind(id)
        .bind(size)
        .bind(status)
        .execute(&mut *c)
        .await?;
        Ok(())
    }
}

#[async_trait]
impl AccountingService for Accounting {
    async fn user_totals(&self, ctx: &Ctx, user: u64) -> PResult<UserTotals> {
        scope(ctx, "read")?;
        Ok(totals_many(ctx, &[u2i(user)?]).await?.pop().unwrap_or_default())
    }
    async fn user_totals_many(&self, ctx: &Ctx, users: &[u64]) -> PResult<Vec<UserTotals>> {
        scope(ctx, "read")?;
        let ids = users.iter().map(|u| u2i(*u)).collect::<PResult<Vec<_>>>()?;
        totals_many(ctx, &ids).await
    }
    async fn seeding_now(&self, ctx: &Ctx, user: u64) -> PResult<Vec<u64>> {
        scope(ctx, "read")?;
        let mut c = ctx.sql.conn().await?;
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT DISTINCT torrent_id FROM peers WHERE user_id = $1 AND is_active AND is_seeder
                AND last_announce_at > now() - interval '1 hour' ORDER BY torrent_id LIMIT 10000",
        )
        .bind(u2i(user)?)
        .fetch_all(&mut *c)
        .await?;
        Ok(ids.into_iter().filter_map(|i| u64::try_from(i).ok()).collect())
    }
    async fn set_rate_rule(&self, ctx: &Ctx, rule: RateRule) -> PResult<u64> {
        scope(ctx, "write:rates")?;
        let max_up = self.0.cfg()?.max_up_permille;
        rates::set_rate_rule(ctx, &ctx.invocation.caller_plugin, rule, max_up).await
    }
    async fn remove_rate_rule(&self, ctx: &Ctx, rule_id: u64) -> PResult<()> {
        scope(ctx, "write:rates")?;
        rates::remove_rate_rule(ctx, Some(&ctx.invocation.caller_plugin), rule_id).await
    }
    async fn effective_rate(
        &self,
        ctx: &Ctx,
        user: u64,
        torrent_id: u64,
        at: Option<i64>,
    ) -> PResult<EffectiveRate> {
        scope(ctx, "read")?;
        self.0.refresh_rates(ctx).await?;
        let t = at.unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
        let (up, down, _) = rates::effective(
            &self.0.rate_index(),
            user,
            torrent_id,
            t,
            self.0.cfg()?.max_up_permille,
        );
        Ok(EffectiveRate {
            up_permille: up,
            down_permille: down,
        })
    }
    async fn adjust(
        &self,
        ctx: &Ctx,
        user: u64,
        d_up: i64,
        d_down: i64,
        reason: &str,
        key: &str,
    ) -> PResult<AdjustResult> {
        scope(ctx, "write:adjust")?;
        adjust(
            ctx,
            &ctx.invocation.caller_plugin,
            u2i(user)?,
            d_up,
            d_down,
            reason,
            None,
            key,
        )
        .await
    }
    async fn signals(
        &self,
        ctx: &Ctx,
        user: u64,
        from: i64,
        to: i64,
        cursor: Option<String>,
    ) -> PResult<signals::SignalPage> {
        scope(ctx, "read:signals")?;
        signals::list(ctx, Some(u2i(user)?), None, from, to, cursor.as_deref(), 200).await
    }
}

#[async_trait]
impl pt_plugin_api::JsonService for Accounting {
    async fn call(&self, ctx: &Ctx, method: &str, a: serde_json::Value) -> PResult<serde_json::Value> {
        fn arg<T: serde::de::DeserializeOwned>(v: &serde_json::Value, k: &str) -> PResult<T> {
            serde_json::from_value(v.get(k).cloned().unwrap_or(serde_json::Value::Null))
                .map_err(|e| PluginError::validation(format!("argument `{k}`: {e}")))
        }
        let v = match method {
            "user_totals" => {
                serde_json::to_value(AccountingService::user_totals(self, ctx, arg(&a, "user")?).await?)?
            }
            "user_totals_many" => {
                let ids: Vec<u64> = arg(&a, "users")?;
                serde_json::to_value(AccountingService::user_totals_many(self, ctx, &ids).await?)?
            }
            "seeding_now" => {
                serde_json::to_value(AccountingService::seeding_now(self, ctx, arg(&a, "user")?).await?)?
            }
            "set_rate_rule" => {
                serde_json::to_value(AccountingService::set_rate_rule(self, ctx, arg(&a, "rule")?).await?)?
            }
            "remove_rate_rule" => serde_json::to_value(
                AccountingService::remove_rate_rule(self, ctx, arg(&a, "rule_id")?).await?,
            )?,
            "effective_rate" => serde_json::to_value(
                AccountingService::effective_rate(
                    self,
                    ctx,
                    arg(&a, "user")?,
                    arg(&a, "torrent_id")?,
                    arg(&a, "at")?,
                )
                .await?,
            )?,
            "adjust" => serde_json::to_value(
                AccountingService::adjust(
                    self,
                    ctx,
                    arg(&a, "user")?,
                    arg(&a, "d_up")?,
                    arg(&a, "d_down")?,
                    &arg::<String>(&a, "reason")?,
                    &arg::<String>(&a, "idempotency_key")?,
                )
                .await?,
            )?,
            "signals" => serde_json::to_value(
                AccountingService::signals(
                    self,
                    ctx,
                    arg(&a, "user")?,
                    arg(&a, "from")?,
                    arg(&a, "to")?,
                    arg(&a, "cursor")?,
                )
                .await?,
            )?,
            other => {
                return Err(PluginError::new(
                    "kernel.not_found",
                    format!("ctx.accounting has no method `{other}`"),
                )
                .with_status(404));
            }
        };
        Ok(v)
    }
}

// ---------------- HTTP（03 §2.4 子集） ----------------

fn ratio(up: i64, down: i64) -> Option<String> {
    (down > 0).then(|| {
        let r = u128::try_from(up.max(0)).unwrap_or(0) * 10_000 / u128::try_from(down).unwrap_or(1);
        format!("{}.{:04}", r / 10_000, r % 10_000)
    })
}

async fn stats_json(ctx: &Ctx, uid: i64, full: bool) -> PResult<serde_json::Value> {
    let t = totals_many(ctx, &[uid]).await?.pop().unwrap_or_default();
    if !full {
        return Ok(
            json!({ "user_id": uid, "uploaded": t.uploaded, "downloaded": t.downloaded,
                          "ratio": ratio(t.uploaded, t.downloaded), "seed_secs": t.seed_secs }),
        );
    }
    let mut c = ctx.sql.conn().await?;
    let r = sqlx::query(
        "SELECT count(DISTINCT torrent_id) FILTER (WHERE is_seeder), count(DISTINCT torrent_id) FILTER (WHERE NOT is_seeder)
           FROM peers WHERE user_id = $1 AND is_active AND last_announce_at > now() - interval '1 hour'",
    )
    .bind(uid)
    .fetch_one(&mut *c)
    .await?;
    let today = sqlx::query(
        "SELECT uploaded, downloaded, seed_secs FROM user_daily WHERE user_id = $1 AND day = current_date",
    )
    .bind(uid)
    .fetch_optional(&mut *c)
    .await?;
    Ok(json!({
        "user_id": uid, "uploaded": t.uploaded, "downloaded": t.downloaded, "uploaded_raw": t.uploaded_raw,
        "downloaded_raw": t.downloaded_raw, "seed_secs": t.seed_secs, "leech_secs": t.leech_secs,
        "completed_count": t.completed_count, "ratio": ratio(t.uploaded, t.downloaded),
        "seeding_count": r.get::<i64, _>(0), "leeching_count": r.get::<i64, _>(1),
        "today": today.map(|d| json!({ "uploaded": d.get::<i64, _>(0), "downloaded": d.get::<i64, _>(1), "seed_secs": d.get::<i64, _>(2) })),
    }))
}

async fn can_read_others(ctx: &Ctx) -> PResult<bool> {
    let Some(uid) = ctx.user_id() else {
        return Ok(false);
    };
    let rbac = ctx.service::<dyn pt_plugin_api::services::RbacService>(pt_plugin_api::services::RBAC)?;
    rbac.call(move |s, c| Box::pin(async move { s.check(c, uid, "accounting.stats.read").await }))
        .await
}

struct Tools;

#[async_trait]
impl pt_plugin_api::McpTool for Tools {
    async fn call(&self, ctx: &Ctx, tool: &str, _a: serde_json::Value) -> PResult<serde_json::Value> {
        match tool {
            "get_my_stats" => {
                let uid = u2i(ctx.require_user()?)?;
                stats_json(ctx, uid, true).await
            }
            other => Err(PluginError::not_found(format!("unknown tool `{other}`"))),
        }
    }
}

async fn http_my_stats(ctx: RequestCtx) -> PResult<Response> {
    let uid = u2i(ctx.require_user()?)?;
    Ok(Json(stats_json(&ctx, uid, true).await?).into_response())
}

async fn http_user_stats(ctx: RequestCtx, Path(id): Path<i64>) -> PResult<Response> {
    let me = u2i(ctx.require_user()?)?;
    let full = me == id || can_read_others(&ctx).await?;
    Ok(Json(stats_json(&ctx, id, full).await?).into_response())
}

#[derive(Debug, Deserialize)]
struct SnatchQuery {
    state: Option<String>,
    limit: Option<i64>,
}

async fn http_snatches(
    ctx: RequestCtx,
    Path(id): Path<i64>,
    Query(q): Query<SnatchQuery>,
) -> PResult<Response> {
    let me = u2i(ctx.require_user()?)?;
    if me != id && !can_read_others(&ctx).await? {
        return Err(PluginError::forbidden("not allowed to view this user's snatches"));
    }
    let filter = match q.state.as_deref().unwrap_or("all") {
        "completed" => "s.completed_at IS NOT NULL",
        "incomplete" => "s.completed_at IS NULL",
        "seeding" => {
            "EXISTS (SELECT 1 FROM peers p WHERE p.user_id = s.user_id AND p.torrent_id = s.torrent_id AND p.is_active AND p.is_seeder AND p.last_announce_at > now() - interval '1 hour')"
        }
        "leeching" => {
            "EXISTS (SELECT 1 FROM peers p WHERE p.user_id = s.user_id AND p.torrent_id = s.torrent_id AND p.is_active AND NOT p.is_seeder AND p.last_announce_at > now() - interval '1 hour')"
        }
        "all" => "true",
        _ => return Err(PluginError::bad_request("unknown state")),
    };
    let mut c = ctx.sql.conn().await?;
    let rows = sqlx::query(&format!(
        "SELECT s.torrent_id, s.uploaded, s.downloaded, s.seed_secs, s.leech_secs, s.completed_at, s.last_action_at
           FROM snatches s WHERE s.user_id = $1 AND {filter} ORDER BY s.last_action_at DESC, s.torrent_id DESC LIMIT $2"
    ))
    .bind(id)
    .bind(q.limit.unwrap_or(50).clamp(1, 100))
    .fetch_all(&mut *c)
    .await?;
    let items: Vec<_> = rows
        .iter()
        .map(|r| json!({ "torrent_id": r.get::<i64, _>(0), "uploaded": r.get::<i64, _>(1), "downloaded": r.get::<i64, _>(2),
                         "seed_secs": r.get::<i64, _>(3), "leech_secs": r.get::<i64, _>(4),
                         "completed_at": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>(5).map(|d| d.to_rfc3339()),
                         "last_action_at": r.get::<chrono::DateTime<chrono::Utc>, _>(6).to_rfc3339() }))
        .collect();
    Ok(Json(json!({ "items": items, "next_cursor": null })).into_response())
}

#[derive(Debug, Deserialize)]
struct SwarmBatchQuery {
    ids: String,
}

/// 列表页批量取做种 / 下载 / 完成人数（`?ids=1,2,3`，最多 100 个）
async fn http_swarm_batch(ctx: RequestCtx, Query(q): Query<SwarmBatchQuery>) -> PResult<Response> {
    ctx.require_user()?;
    let ids: Vec<i64> = q
        .ids
        .split(',')
        .filter_map(|x| x.trim().parse().ok())
        .take(100)
        .collect();
    let mut c = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT torrent_id, seeders, leechers, completed FROM v_torrent_peers WHERE torrent_id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(&mut *c)
    .await?;
    let mut items = serde_json::Map::new();
    for r in &rows {
        items.insert(
            r.get::<i64, _>(0).to_string(),
            json!({ "seeders": r.get::<i64, _>(1), "leechers": r.get::<i64, _>(2), "completed": r.get::<i64, _>(3) }),
        );
    }
    Ok(Json(json!({ "items": items })).into_response())
}

/// 单个种子的 swarm：人数、活跃 peer 列表（只含客户端与时间；他人身份仅对有 `accounting.stats.read` 的用户可见）、
/// 以及当前用户自己的记录
async fn http_swarm(ctx: RequestCtx, Path(id): Path<i64>) -> PResult<Response> {
    let me = u2i(ctx.require_user()?)?;
    let staff = can_read_others(&ctx).await?;
    let mut c = ctx.sql.conn().await?;
    let counts =
        sqlx::query("SELECT seeders, leechers, completed FROM v_torrent_peers WHERE torrent_id = $1")
            .bind(id)
            .fetch_optional(&mut *c)
            .await?;
    let (seeders, leechers, completed) = counts.map_or((0, 0, 0), |r| {
        (r.get::<i64, _>(0), r.get::<i64, _>(1), r.get::<i64, _>(2))
    });
    let peers = sqlx::query(
        "SELECT user_id, is_seeder, client_prefix, first_seen_at, last_announce_at FROM peers
          WHERE torrent_id = $1 AND is_active ORDER BY is_seeder DESC, last_announce_at DESC LIMIT 100",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await?;
    let peers: Vec<_> = peers
        .iter()
        .map(|r| {
            let uid: i64 = r.get(0);
            json!({
                "user_id": (staff || uid == me).then_some(uid), "mine": uid == me, "seeder": r.get::<bool, _>(1),
                "client": r.get::<Option<String>, _>(2),
                "since": r.get::<chrono::DateTime<chrono::Utc>, _>(3).to_rfc3339(),
                "last_announce": r.get::<chrono::DateTime<chrono::Utc>, _>(4).to_rfc3339(),
            })
        })
        .collect();
    let mine = sqlx::query(
        "SELECT uploaded, downloaded, seed_secs, leech_secs, completed_at, last_action_at FROM snatches WHERE user_id = $1 AND torrent_id = $2",
    )
    .bind(me)
    .bind(id)
    .fetch_optional(&mut *c)
    .await?
    .map(|r| {
        json!({ "uploaded": r.get::<i64, _>(0), "downloaded": r.get::<i64, _>(1), "seed_secs": r.get::<i64, _>(2),
                "leech_secs": r.get::<i64, _>(3),
                "completed_at": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>(4).map(|d| d.to_rfc3339()),
                "last_action_at": r.get::<chrono::DateTime<chrono::Utc>, _>(5).to_rfc3339() })
    });
    Ok(Json(json!({ "seeders": seeders, "leechers": leechers, "completed": completed, "peers": peers, "mine": mine })).into_response())
}

#[derive(Debug, Deserialize)]
struct AdjustBody {
    d_up: i64,
    d_down: i64,
    note: String,
}

async fn http_adjust(
    ctx: RequestCtx,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(b): Json<AdjustBody>,
) -> PResult<Response> {
    let key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .filter(|k| (1..=64).contains(&k.len()))
        .ok_or_else(|| PluginError::bad_request("Idempotency-Key header is required"))?;
    if b.note.trim().is_empty() || b.note.chars().count() > 500 {
        return Err(PluginError::validation("note is required (≤ 500 characters)"));
    }
    let key = format!(
        "http:{}:{key}",
        ctx.actor().credential_id.clone().unwrap_or_default()
    );
    let r = adjust(
        &ctx,
        "accounting",
        id,
        b.d_up,
        b.d_down,
        "accounting.manual",
        Some(&b.note),
        &key,
    )
    .await?;
    ctx.audit
        .write(pt_plugin_api::AuditEntry {
            action: "accounting.adjust".into(),
            target: format!("user:{id}"),
            details: json!({ "d_up": b.d_up, "d_down": b.d_down, "note": b.note, "adjustment_id": r.adjustment_id }),
        })
        .await?;
    let status = if r.duplicate {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((status, Json(r)).into_response())
}

#[derive(Debug, Deserialize)]
struct SignalQuery {
    user_id: Option<i64>,
    signal: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
    cursor: Option<String>,
    limit: Option<i64>,
}

/// 后台：信号列表（`accounting.signals.read`）
async fn http_signals(ctx: RequestCtx, Query(q): Query<SignalQuery>) -> PResult<Response> {
    let now = ctx.time.now();
    let to = q.to.unwrap_or(now);
    let from = q.from.unwrap_or(to - 30 * 86_400_000);
    let page = signals::list(
        &ctx,
        q.user_id,
        q.signal.as_deref(),
        from,
        to,
        q.cursor.as_deref(),
        q.limit.unwrap_or(50),
    )
    .await?;
    Ok(Json(page).into_response())
}

struct Routes;

impl pt_plugin_api::HttpRoutes for Routes {
    fn router(&self) -> Router {
        Router::new()
            .route("/v1/users/me/stats", get(http_my_stats))
            .route("/v1/users/{id}/stats", get(http_user_stats))
            .route("/v1/users/{id}/snatches", get(http_snatches))
            .route("/v1/torrents/swarm", get(http_swarm_batch))
            .route("/v1/torrents/{id}/swarm", get(http_swarm))
            .route("/v1/admin/users/{id}/adjustments", post(http_adjust))
            .route("/v1/admin/signals", get(http_signals))
    }
}

pub fn plugin() -> NativePlugin {
    let a = Arc::new(Accounting(Arc::new(State::default())));
    NativePlugin {
        manifest: MANIFEST,
        migrations: MIGRATIONS,
        lifecycle: Some(a.clone()),
        on_event: Some(a.clone()),
        on_cron: Some(a.clone()),
        http: Some(Arc::new(Routes)),
        mcp_tool: Some(Arc::new(Tools)),
        workers: vec![("ingest", a.clone())],
        services: vec![(ACCOUNTING, service_object::<dyn AccountingService>(a.clone()))],
        json_services: vec![(ACCOUNTING, a)],
        ..Default::default()
    }
}
