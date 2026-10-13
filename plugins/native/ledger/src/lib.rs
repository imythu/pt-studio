//! ledger 插件（docs/plugins/ledger/）：站点货币的复式记账（`ctx.ledger`）。
//!
//! 本版本实现：币种注册、发行 / 回收 / 转账（手续费、每日限额、钩子）、批量发行、幂等、冻结与到期释放、
//! 账户锁定、冲正、余额与流水查询、供给统计、用户转账与管理员调整的 HTTP 接口。
//! 日对账、月结余快照、归档、用户删除处理在后续迭代实现（03 §1.8～§1.10）。

pub mod api;
pub mod core;

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use pt_plugin_api::{
    Ctx, JobInvocation, NativePlugin, OnCron, PResult, PluginError, RequestCtx, json, service_object,
};
use serde::Deserialize;
use sqlx::Row;

use crate::{
    api::{
        BalanceInfo, CaptureRequest, CurrencyDef, CurrencyInfo, HistoryItem, HistoryPage, HoldRequest,
        LEDGER, LedgerService, LockRequest, SupplyInfo, TransferRequest, TransferResult,
    },
    core::Caller,
};

pub const MANIFEST: &str = include_str!("../plugin.toml");
pub const MIGRATIONS: &[(&str, &str)] = &[("0001_init.sql", include_str!("../migrations/0001_init.sql"))];

fn u2i(u: u64) -> PResult<i64> {
    i64::try_from(u).map_err(|_| PluginError::bad_request("user id out of range"))
}

/// 读余额与流水：调用方需 `read`，或持有该币种作用域（只能查该币种）。
fn require_read(caller: &Caller, currency: Option<&str>) -> PResult<()> {
    if caller.has("read") || currency.is_some_and(|c| caller.has(&format!("currency:{c}"))) {
        Ok(())
    } else {
        Err(PluginError::new("ledger.scope_required", "scope `read` required").with_status(403))
    }
}

pub async fn balances(ctx: &Ctx, user: i64) -> PResult<Vec<BalanceInfo>> {
    let mut c = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT a.currency, c.scale, a.balance, a.held, a.lock_count FROM accounts a JOIN currencies c ON c.code = a.currency
          WHERE a.user_id = $1 ORDER BY a.currency",
    )
    .bind(user)
    .fetch_all(&mut *c)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let (bal, held): (i64, i64) = (r.get(2), r.get(3));
            BalanceInfo {
                currency: r.get(0),
                scale: u8::try_from(r.get::<i16, _>(1)).unwrap_or(0),
                balance: bal,
                held,
                available: bal - held,
                locked: r.get::<i32, _>(4) > 0,
            }
        })
        .collect())
}

#[derive(Debug, serde::Serialize, Deserialize)]
struct Cursor {
    t: i64,
    e: uuid::Uuid,
    l: i16,
}

pub async fn history(
    ctx: &Ctx,
    currency: Option<&str>,
    user: i64,
    cursor: Option<&str>,
    limit: u32,
) -> PResult<HistoryPage> {
    let limit = i64::from(limit.clamp(1, 200));
    let cur: Option<Cursor> = cursor
        .map(|c| {
            URL_SAFE_NO_PAD
                .decode(c)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .ok_or_else(|| PluginError::bad_request("invalid cursor"))
        })
        .transpose()?;
    let mut c = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT l.entry_id, l.currency, e.kind, l.amount, l.balance_after, e.reason, e.memo, l.counterparty_user_id,
                e.caller_plugin, (r.entry_id IS NOT NULL) AS reversed, l.created_at, l.line_no
           FROM lines l
           JOIN entries e ON e.id = l.entry_id AND e.created_at = l.created_at
           LEFT JOIN entry_reversals r ON r.entry_id = l.entry_id
          WHERE l.user_id = $1 AND ($2::text IS NULL OR l.currency = $2)
            AND ($3::timestamptz IS NULL OR (l.created_at, l.entry_id, l.line_no) < ($3, $4, $5))
          ORDER BY l.created_at DESC, l.entry_id DESC, l.line_no DESC
          LIMIT $6",
    )
    .bind(user)
    .bind(currency)
    .bind(cur.as_ref().and_then(|c| chrono::DateTime::<chrono::Utc>::from_timestamp_micros(c.t)))
    .bind(cur.as_ref().map(|c| c.e))
    .bind(cur.as_ref().map(|c| c.l))
    .bind(limit + 1)
    .fetch_all(&mut *c)
    .await?;
    let more = i64::try_from(rows.len()).unwrap_or(0) > limit;
    let rows = &rows[..rows.len().min(usize::try_from(limit).unwrap_or(0))];
    let next_cursor = if more {
        rows.last().map(|r| {
            let at: chrono::DateTime<chrono::Utc> = r.get(10);
            let c = Cursor {
                t: at.timestamp_micros(),
                e: r.get(0),
                l: r.get(11),
            };
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&c).unwrap_or_default())
        })
    } else {
        None
    };
    let items = rows
        .iter()
        .map(|r| HistoryItem {
            entry_id: r.get::<uuid::Uuid, _>(0).to_string(),
            currency: r.get(1),
            kind: r.get(2),
            amount: r.get(3),
            balance_after: r.get::<Option<i64>, _>(4).unwrap_or_default(),
            reason: r.get(5),
            memo: r.get(6),
            counterparty_user_id: r.get::<Option<i64>, _>(7).and_then(|u| u64::try_from(u).ok()),
            caller_plugin: r.get(8),
            reversed: r.get(9),
            created_at: r.get::<chrono::DateTime<chrono::Utc>, _>(10).timestamp_millis(),
        })
        .collect();
    Ok(HistoryPage { items, next_cursor })
}

pub async fn supply(ctx: &Ctx, currency: &str) -> PResult<SupplyInfo> {
    let mut c = ctx.sql.conn().await?;
    core::load_currency(&mut c, currency).await?;
    let r = sqlx::query(
        "SELECT coalesce(sum(issued), 0)::bigint, coalesce(sum(burned), 0)::bigint,
                (SELECT coalesce(sum(held), 0)::bigint FROM accounts WHERE currency = $1)
           FROM supply_shards WHERE currency = $1",
    )
    .bind(currency)
    .fetch_one(&mut *c)
    .await?;
    let (issued, burned, held): (i64, i64, i64) = (r.get(0), r.get(1), r.get(2));
    Ok(SupplyInfo {
        currency: currency.to_owned(),
        issued,
        burned,
        circulating: issued - burned,
        held,
    })
}

/// 批量发行（03 §1.4）：同币种、全部为发行；预检剔除有错的笔，其余同一事务执行。
async fn transfer_batch(ctx: &Ctx, reqs: Vec<TransferRequest>) -> PResult<Vec<PResult<TransferResult>>> {
    if reqs.len() > 1000 {
        return Err(PluginError::new(
            "ledger.batch_too_large",
            "at most 1000 transfers per batch",
        ));
    }
    let Some(first) = reqs.first() else {
        return Ok(vec![]);
    };
    if reqs
        .iter()
        .any(|r| r.currency != first.currency || r.from_user.is_some() || r.to_user.is_none())
    {
        return Err(PluginError::new(
            "ledger.batch_mixed",
            "a batch must issue a single currency to users",
        ));
    }
    let mut out = Vec::with_capacity(reqs.len());
    let mut order: Vec<usize> = (0..reqs.len()).collect();
    order.sort_by_key(|i| reqs[*i].to_user);
    let mut results: Vec<Option<PResult<TransferResult>>> = vec![None; reqs.len()];
    for i in order {
        // 发行只增加余额，单笔业务错误（参数、幂等冲突）不影响其他笔；基础设施错误整批失败
        match core::transfer(ctx, reqs[i].clone(), false).await {
            Ok(r) => results[i] = Some(Ok(r)),
            Err(e)
                if e.code.starts_with("ledger.")
                    || e.code == "kernel.validation_failed"
                    || e.code == "kernel.idempotency_key_reused" =>
            {
                results[i] = Some(Err(e));
            }
            Err(e) => return Err(e),
        }
    }
    for r in results {
        out.push(r.unwrap_or_else(|| Err(PluginError::internal("missing result"))));
    }
    Ok(out)
}

struct Ledger;

#[async_trait]
impl LedgerService for Ledger {
    async fn register_currency(&self, ctx: &Ctx, def: CurrencyDef) -> PResult<()> {
        core::register_currency(ctx, def).await
    }
    async fn currencies(&self, ctx: &Ctx) -> PResult<Vec<CurrencyInfo>> {
        let mut c = ctx.sql.conn().await?;
        core::list_currencies(&mut c).await
    }
    async fn transfer(&self, ctx: &Ctx, req: TransferRequest) -> PResult<TransferResult> {
        core::transfer(ctx, req, false).await
    }
    async fn transfer_batch(
        &self,
        ctx: &Ctx,
        reqs: Vec<TransferRequest>,
    ) -> PResult<Vec<PResult<TransferResult>>> {
        transfer_batch(ctx, reqs).await
    }
    async fn balance(&self, ctx: &Ctx, currency: &str, user: u64) -> PResult<i64> {
        require_read(&Caller::of(ctx), Some(currency))?;
        let mut c = ctx.sql.conn().await?;
        core::load_currency(&mut c, currency).await?;
        let b: Option<i64> =
            sqlx::query_scalar("SELECT balance FROM accounts WHERE user_id = $1 AND currency = $2")
                .bind(u2i(user)?)
                .bind(currency)
                .fetch_optional(&mut *c)
                .await?;
        Ok(b.unwrap_or(0))
    }
    async fn balances(&self, ctx: &Ctx, user: u64) -> PResult<Vec<BalanceInfo>> {
        require_read(&Caller::of(ctx), None)?;
        balances(ctx, u2i(user)?).await
    }
    async fn history(
        &self,
        ctx: &Ctx,
        currency: Option<String>,
        user: u64,
        cursor: Option<String>,
        limit: u32,
    ) -> PResult<HistoryPage> {
        require_read(&Caller::of(ctx), currency.as_deref())?;
        history(ctx, currency.as_deref(), u2i(user)?, cursor.as_deref(), limit).await
    }
    async fn reverse(
        &self,
        ctx: &Ctx,
        entry_id: &str,
        reason: &str,
        key: &str,
        allow_negative: bool,
    ) -> PResult<String> {
        core::reverse(ctx, entry_id, reason, key, allow_negative).await
    }
    async fn hold(&self, ctx: &Ctx, req: HoldRequest) -> PResult<String> {
        core::hold(ctx, req).await
    }
    async fn release_hold(&self, ctx: &Ctx, hold_id: &str, key: &str) -> PResult<()> {
        core::release_hold(ctx, hold_id, key, "released").await
    }
    async fn capture_hold(&self, ctx: &Ctx, req: CaptureRequest) -> PResult<TransferResult> {
        core::capture_hold(ctx, req).await
    }
    async fn lock_account(&self, ctx: &Ctx, req: LockRequest) -> PResult<String> {
        core::lock_account(ctx, req).await
    }
    async fn unlock_account(&self, ctx: &Ctx, lock_id: &str, key: &str) -> PResult<()> {
        core::unlock_account(ctx, lock_id, key).await
    }
    async fn supply(&self, ctx: &Ctx, currency: &str) -> PResult<SupplyInfo> {
        require_read(&Caller::of(ctx), Some(currency))?;
        supply(ctx, currency).await
    }
}

#[async_trait]
impl OnCron for Ledger {
    async fn run(&self, ctx: &Ctx, job: JobInvocation) -> PResult<()> {
        match job.name.as_str() {
            "expire_hold" => {
                let id = job.payload["hold_id"]
                    .as_str()
                    .ok_or_else(|| PluginError::validation("hold_id"))?;
                core::release_hold(ctx, id, &format!("expire:{id}"), "expired").await
            }
            other => Err(PluginError::internal(format!("unknown job `{other}`"))),
        }
    }
}

// ---------------- HTTP（03 §2.4） ----------------

fn me(ctx: &Ctx) -> PResult<i64> {
    u2i(ctx.require_user()?)
}

fn idem_header(h: &HeaderMap, ctx: &Ctx) -> PResult<String> {
    let k = h
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .filter(|k| (1..=64).contains(&k.len()))
        .ok_or_else(|| PluginError::bad_request("Idempotency-Key header is required"))?;
    let cred = ctx.actor().credential_id.clone().unwrap_or_default();
    Ok(format!("http:{cred}:{k}"))
}

async fn http_currencies(ctx: RequestCtx) -> PResult<Response> {
    let mut c = ctx.sql.conn().await?;
    Ok(Json(json!({ "items": core::list_currencies(&mut c).await? })).into_response())
}

async fn http_my_balances(ctx: RequestCtx) -> PResult<Response> {
    Ok(Json(json!({ "items": balances(&ctx, me(&ctx)?).await? })).into_response())
}

#[derive(Debug, Deserialize)]
struct EntriesQuery {
    currency: Option<String>,
    cursor: Option<String>,
    limit: Option<u32>,
}

async fn http_my_entries(ctx: RequestCtx, Query(q): Query<EntriesQuery>) -> PResult<Response> {
    let page = history(
        &ctx,
        q.currency.as_deref(),
        me(&ctx)?,
        q.cursor.as_deref(),
        q.limit.unwrap_or(50),
    )
    .await?;
    let more = page.next_cursor.is_some();
    Ok(
        Json(json!({ "items": page.items, "next_cursor": page.next_cursor, "has_more": more }))
            .into_response(),
    )
}

#[derive(Debug, Deserialize)]
struct TransferBody {
    currency: String,
    to_username: Option<String>,
    to_user_id: Option<u64>,
    amount: i64,
    #[serde(default)]
    memo: String,
}

async fn http_transfer(
    ctx: RequestCtx,
    headers: HeaderMap,
    Json(b): Json<TransferBody>,
) -> PResult<Response> {
    let from = ctx.require_user()?;
    let key = idem_header(&headers, &ctx)?;
    let cur = {
        let mut c = ctx.sql.conn().await?;
        core::load_currency(&mut c, &b.currency).await?
    };
    if !cur.info.transferable {
        return Err(PluginError::new(
            "ledger.transfer_disabled",
            "transfers are disabled for this currency",
        )
        .with_status(403));
    }
    if b.amount < cur.info.min_transfer {
        return Err(
            PluginError::new("ledger.amount_below_minimum", "amount is below the minimum")
                .with_details(json!({ "min_transfer": cur.info.min_transfer })),
        );
    }
    // 收款方：不存在或不可用一律 recipient_invalid（不泄露原因）
    let invalid = || PluginError::new("ledger.recipient_invalid", "recipient is not valid");
    let to: i64 = {
        let mut c = ctx.sql.conn().await?;
        let row = match (&b.to_username, b.to_user_id) {
            (Some(name), _) => {
                sqlx::query("SELECT id, status FROM identity.v_user_public WHERE lower(username) = lower($1)")
                    .bind(name)
                    .fetch_optional(&mut *c)
                    .await?
            }
            (None, Some(id)) => {
                sqlx::query("SELECT id, status FROM identity.v_user_public WHERE id = $1")
                    .bind(u2i(id)?)
                    .fetch_optional(&mut *c)
                    .await?
            }
            (None, None) => return Err(invalid()),
        };
        let row = row.ok_or_else(invalid)?;
        if matches!(row.get::<String, _>(1).as_str(), "banned" | "deleted") {
            return Err(invalid());
        }
        row.get(0)
    };
    let r = core::transfer(
        &ctx,
        TransferRequest {
            currency: b.currency,
            from_user: Some(from),
            to_user: Some(u64::try_from(to).unwrap_or_default()),
            amount: b.amount,
            reason: "ledger.user_gift".into(),
            memo: b.memo,
            idempotency_key: key,
        },
        false,
    )
    .await?;
    let status = if r.duplicate {
        StatusCode::OK
    } else {
        StatusCode::CREATED
    };
    Ok((
        status,
        Json(json!({ "entry_id": r.entry_id, "amount": b.amount, "fee": r.fee,
                             "balance_after": r.from_balance_after, "duplicate": r.duplicate })),
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
struct AdjustBody {
    user_id: u64,
    currency: String,
    amount: i64,
    reason_note: String,
}

async fn http_adjust(ctx: RequestCtx, headers: HeaderMap, Json(b): Json<AdjustBody>) -> PResult<Response> {
    let key = idem_header(&headers, &ctx)?;
    if b.amount == 0 || b.reason_note.trim().is_empty() {
        return Err(PluginError::validation(
            "amount must be non-zero and reason_note is required",
        ));
    }
    let (from, to) = if b.amount > 0 {
        (None, Some(b.user_id))
    } else {
        (Some(b.user_id), None)
    };
    let r = core::transfer(
        &ctx,
        TransferRequest {
            currency: b.currency.clone(),
            from_user: from,
            to_user: to,
            amount: b.amount.abs(),
            reason: "ledger.admin_adjust".into(),
            memo: b.reason_note.clone(),
            idempotency_key: key,
        },
        true,
    )
    .await?;
    ctx.audit
        .write(pt_plugin_api::AuditEntry {
            action: "ledger.adjust".into(),
            target: format!("user:{}", b.user_id),
            details: json!({ "currency": b.currency, "amount": b.amount, "note": b.reason_note, "entry_id": r.entry_id,
                             "balance_after": r.from_balance_after.or(r.to_balance_after) }),
        })
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(
            json!({ "entry_id": r.entry_id, "balance_after": r.from_balance_after.or(r.to_balance_after),
                                          "duplicate": r.duplicate }),
        ),
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
struct ReverseBody {
    reason_note: String,
    #[serde(default)]
    allow_negative: bool,
}

async fn http_reverse(
    ctx: RequestCtx,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(b): Json<ReverseBody>,
) -> PResult<Response> {
    let key = idem_header(&headers, &ctx)?;
    let rid = core::reverse(&ctx, &id, "ledger.admin_reversal", &key, b.allow_negative).await?;
    ctx.audit
        .write(pt_plugin_api::AuditEntry {
            action: "ledger.reverse".into(),
            target: format!("entry:{id}"),
            details: json!({ "reversal_entry_id": rid, "note": b.reason_note, "allow_negative": b.allow_negative }),
        })
        .await?;
    Ok((StatusCode::CREATED, Json(json!({ "reversal_entry_id": rid }))).into_response())
}

#[derive(Debug, Deserialize)]
struct SettingsPatch {
    transferable: Option<bool>,
    fee_bps: Option<i32>,
    min_transfer: Option<i64>,
    daily_out_limit: Option<i64>,
    min_balance: Option<i64>,
    disabled: Option<bool>,
}

async fn http_settings(
    ctx: RequestCtx,
    Path(code): Path<String>,
    Json(p): Json<SettingsPatch>,
) -> PResult<Response> {
    if p.fee_bps.is_some_and(|f| !(0..=10_000).contains(&f))
        || p.min_transfer.is_some_and(|m| m < 1)
        || p.daily_out_limit.is_some_and(|d| d < 0)
    {
        return Err(PluginError::validation("invalid currency settings"));
    }
    let mut c = ctx.sql.conn().await?;
    core::load_currency(&mut c, &code).await?;
    sqlx::query(
        "UPDATE currency_settings SET transferable = coalesce($2, transferable), fee_bps = coalesce($3, fee_bps),
                min_transfer = coalesce($4, min_transfer), daily_out_limit = coalesce($5, daily_out_limit),
                min_balance = coalesce($6, min_balance), disabled = coalesce($7, disabled), updated_at = now()
          WHERE currency = $1",
    )
    .bind(&code)
    .bind(p.transferable)
    .bind(p.fee_bps)
    .bind(p.min_transfer)
    .bind(p.daily_out_limit)
    .bind(p.min_balance)
    .bind(p.disabled)
    .execute(&mut *c)
    .await?;
    let cur = core::load_currency(&mut c, &code).await?;
    drop(c);
    ctx.audit
        .write(pt_plugin_api::AuditEntry {
            action: "ledger.currency_settings".into(),
            target: format!("currency:{code}"),
            details: serde_json::to_value(&cur.info)?,
        })
        .await?;
    Ok(Json(cur.info).into_response())
}

async fn http_admin_balances(ctx: RequestCtx, Path(id): Path<i64>) -> PResult<Response> {
    Ok(Json(json!({ "items": balances(&ctx, id).await? })).into_response())
}

struct Routes;

impl pt_plugin_api::HttpRoutes for Routes {
    fn router(&self) -> Router {
        Router::new()
            .route("/v1/currencies", get(http_currencies))
            .route("/v1/users/me/balances", get(http_my_balances))
            .route("/v1/users/me/entries", get(http_my_entries))
            .route("/v1/transfers", post(http_transfer))
            .route("/v1/admin/adjustments", post(http_adjust))
            .route("/v1/admin/entries/{id}/reversals", post(http_reverse))
            .route("/v1/admin/currencies/{code}", patch(http_settings))
            .route("/v1/admin/users/{id}/balances", get(http_admin_balances))
    }
}

// ---------------- JSON 分派（pt:plugin@1 services，供 WASM 插件调用） ----------------

fn arg<T: serde::de::DeserializeOwned>(v: &serde_json::Value, k: &str) -> PResult<T> {
    serde_json::from_value(v.get(k).cloned().unwrap_or(serde_json::Value::Null))
        .map_err(|e| PluginError::validation(format!("argument `{k}`: {e}")))
}

fn whole<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> PResult<T> {
    serde_json::from_value(v).map_err(|e| PluginError::validation(format!("arguments: {e}")))
}

fn out<T: serde::Serialize>(v: T) -> PResult<serde_json::Value> {
    Ok(serde_json::to_value(v)?)
}

#[async_trait]
impl pt_plugin_api::JsonService for Ledger {
    async fn call(&self, ctx: &Ctx, method: &str, a: serde_json::Value) -> PResult<serde_json::Value> {
        match method {
            "register_currency" => out(LedgerService::register_currency(self, ctx, whole(a)?).await?),
            "currencies" => out(LedgerService::currencies(self, ctx).await?),
            "transfer" => out(LedgerService::transfer(self, ctx, whole(a)?).await?),
            "transfer_batch" => {
                let r = LedgerService::transfer_batch(self, ctx, arg(&a, "requests")?).await?;
                out(r
                    .into_iter()
                    .map(|x| match x {
                        Ok(v) => json!({ "ok": v }),
                        Err(e) => json!({ "err": e }),
                    })
                    .collect::<Vec<_>>())
            }
            "balance" => {
                out(
                    LedgerService::balance(self, ctx, &arg::<String>(&a, "currency")?, arg(&a, "user")?)
                        .await?,
                )
            }
            "balances" => out(LedgerService::balances(self, ctx, arg(&a, "user")?).await?),
            "history" => out(LedgerService::history(
                self,
                ctx,
                arg(&a, "currency")?,
                arg(&a, "user")?,
                arg(&a, "cursor")?,
                arg::<Option<u32>>(&a, "limit")?.unwrap_or(50),
            )
            .await?),
            "reverse" => out(LedgerService::reverse(
                self,
                ctx,
                &arg::<String>(&a, "entry_id")?,
                &arg::<String>(&a, "reason")?,
                &arg::<String>(&a, "idempotency_key")?,
                arg::<Option<bool>>(&a, "allow_negative")?.unwrap_or(false),
            )
            .await?),
            "hold" => out(LedgerService::hold(self, ctx, whole(a)?).await?),
            "release_hold" => out(LedgerService::release_hold(
                self,
                ctx,
                &arg::<String>(&a, "hold_id")?,
                &arg::<String>(&a, "idempotency_key")?,
            )
            .await?),
            "capture_hold" => out(LedgerService::capture_hold(self, ctx, whole(a)?).await?),
            "lock_account" => out(LedgerService::lock_account(self, ctx, whole(a)?).await?),
            "unlock_account" => out(LedgerService::unlock_account(
                self,
                ctx,
                &arg::<String>(&a, "lock_id")?,
                &arg::<String>(&a, "idempotency_key")?,
            )
            .await?),
            "supply" => out(LedgerService::supply(self, ctx, &arg::<String>(&a, "currency")?).await?),
            other => Err(
                PluginError::new("kernel.not_found", format!("ctx.ledger has no method `{other}`"))
                    .with_status(404),
            ),
        }
    }
}

pub fn plugin() -> NativePlugin {
    let l = Arc::new(Ledger);
    NativePlugin {
        manifest: MANIFEST,
        migrations: MIGRATIONS,
        on_cron: Some(l.clone()),
        http: Some(Arc::new(Routes)),
        services: vec![(LEDGER, service_object::<dyn LedgerService>(l.clone()))],
        json_services: vec![(LEDGER, l)],
        ..Default::default()
    }
}
