//! 批量发放 / 扣除（docs/plugins/ledger/03 §1.12）：按条件（状态、等级）或用户名单给一批用户各发（或各扣）
//! 同一数量的货币，可选给每人发一封系统站内信。
//!
//! 创建时把对象快照到 `grant_recipients`；延迟任务 `grant_batch` 分块执行，每块一个事务：
//! 先记账（每块 ≤ 500 人，每人一笔 `ledger.admin_grant` / `ledger.admin_deduct`，幂等键 `grant:<批>:<用户>`），
//! 再通知（每块 ≤ 100 人，`ctx.messages.send_system`）。扣除不会形成负余额：余额不足时按 `shortfall`
//! 跳过此人或扣到可用余额为 0。站内信暂不可用时本块停下、稍后重试；未安装站内信插件时跳过通知。

use axum::{
    Json,
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use pt_plugin_api::{Ctx, PResult, PluginError, RequestCtx, json};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    api::TransferRequest,
    core::{self, MAX_AMOUNT},
};

pub const JOB: &str = "grant_batch";
const GRANT_CHUNK: i64 = 500;
const NOTIFY_CHUNK: i64 = 100;
const MAX_RECIPIENTS: i64 = 200_000;
const MAX_USERNAMES: usize = 5000;
/// 可选的用户状态；封禁、已删除的用户永远不发
const STATUSES: &[&str] = &["active", "leech_disabled", "pending"];
/// 站内信连续可重试失败超过此次数后放弃剩余通知
const MAX_NOTIFY_RETRIES: i32 = 5;
const NOTIFY_RETRY_SECS: i64 = 60;

pub const DEFAULT_SUBJECT: &str = "你收到了 {amount} {currency}";
pub const DEFAULT_BODY: &str = "{username}，管理员给你发放了 **{amount}** {currency}。\n\n原因：{memo}\n\n当前余额：{balance} {currency}";
pub const DEDUCT_SUBJECT: &str = "你被扣除了 {amount} {currency}";
pub const DEDUCT_BODY: &str = "{username}，管理员从你的账户扣除了 **{amount}** {currency}。\n\n原因：{memo}\n\n当前余额：{balance} {currency}";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    #[default]
    Grant,
    Deduct,
}

/// 扣除时余额不足的处理
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shortfall {
    /// 跳过此人（记 `ledger.insufficient_balance`）
    #[default]
    Skip,
    /// 扣到可用余额为 0
    Partial,
}

impl Direction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Grant => "grant",
            Self::Deduct => "deduct",
        }
    }
    fn defaults(self) -> (&'static str, &'static str) {
        match self {
            Self::Grant => (DEFAULT_SUBJECT, DEFAULT_BODY),
            Self::Deduct => (DEDUCT_SUBJECT, DEDUCT_BODY),
        }
    }
}

impl Shortfall {
    fn as_str(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Partial => "partial",
        }
    }
}

/// 扣除时此人实际扣多少：可用 = 余额 − 冻结 − 最低余额（与 core::post 的 IgnoreLock 检查一致）；0 = 跳过
fn deduct_amount(amount: i64, available: i64, shortfall: Shortfall) -> i64 {
    if available >= amount {
        amount
    } else if shortfall == Shortfall::Partial {
        available.max(0)
    } else {
        0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// 按状态与等级筛选；`class_ids` 为空表示所有等级
    Filter {
        #[serde(default = "default_statuses")]
        statuses: Vec<String>,
        #[serde(default)]
        class_ids: Vec<i64>,
    },
    /// 指定用户名（不区分大小写）
    Users { usernames: Vec<String> },
}

fn default_statuses() -> Vec<String> {
    vec!["active".into()]
}

impl Target {
    fn check(&self) -> PResult<()> {
        match self {
            Self::Filter { statuses, .. } => {
                if statuses.is_empty() || statuses.iter().any(|s| !STATUSES.contains(&s.as_str())) {
                    return Err(PluginError::validation(
                        "statuses must be a non-empty subset of active, leech_disabled, pending",
                    ));
                }
            }
            Self::Users { usernames } => {
                if usernames.is_empty() || usernames.len() > MAX_USERNAMES {
                    return Err(PluginError::validation(format!(
                        "usernames must contain 1..={MAX_USERNAMES} names"
                    )));
                }
            }
        }
        Ok(())
    }

    fn lowered(&self) -> Vec<String> {
        match self {
            Self::Users { usernames } => usernames
                .iter()
                .map(|u| u.trim().to_lowercase())
                .filter(|u| !u.is_empty())
                .collect(),
            Self::Filter { .. } => vec![],
        }
    }
}

/// 收款人查询：`$1` 状态、`$2` 等级、`$3` 小写用户名（`$3` 非空时按名单，否则按条件）
const RESOLVE: &str = "SELECT id FROM identity.v_user_public
    WHERE CASE WHEN cardinality($3::text[]) > 0
               THEN lower(username) = ANY($3) AND status NOT IN ('banned', 'deleted')
               ELSE status = ANY($1) AND (cardinality($2::bigint[]) = 0 OR class_id::bigint = ANY($2)) END";

fn resolve_args(t: &Target) -> (Vec<String>, Vec<i64>, Vec<String>) {
    match t {
        Target::Filter { statuses, class_ids } => (statuses.clone(), class_ids.clone(), vec![]),
        Target::Users { .. } => (vec![], vec![], t.lowered()),
    }
}

/// ctx.messages 是否可用（未安装或未加载时为 false）
async fn messages_available(ctx: &Ctx) -> bool {
    let uid = ctx.user_id().unwrap_or_default();
    ctx.call_json("ctx.messages", "unread_count", json!({ "user_id": uid }))
        .await
        .is_ok()
}

/// 金额按小数位格式化（去掉末尾的 0）
pub fn fmt_amount(v: i64, scale: u8) -> String {
    if scale == 0 {
        return v.to_string();
    }
    let p = 10_i64.pow(u32::from(scale));
    let sign = if v < 0 { "-" } else { "" };
    let (i, f) = (
        v.unsigned_abs() / p.unsigned_abs(),
        v.unsigned_abs() % p.unsigned_abs(),
    );
    let frac = format!("{f:0width$}", width = usize::from(scale));
    let frac = frac.trim_end_matches('0');
    if frac.is_empty() {
        format!("{sign}{i}")
    } else {
        format!("{sign}{i}.{frac}")
    }
}

pub fn render(tpl: &str, vars: &[(&str, &str)]) -> String {
    vars.iter()
        .fold(tpl.to_owned(), |s, (k, v)| s.replace(&format!("{{{k}}}"), v))
}

// ---------------- HTTP ----------------

#[derive(Debug, Deserialize)]
pub struct PreviewBody {
    currency: String,
    amount: i64,
    target: Target,
    #[serde(default)]
    direction: Direction,
    #[serde(default)]
    shortfall: Shortfall,
}

pub async fn http_preview(ctx: RequestCtx, Json(b): Json<PreviewBody>) -> PResult<Response> {
    b.target.check()?;
    let mut c = ctx.sql.conn().await?;
    let cur = core::load_currency(&mut c, &b.currency).await?;
    let (st, cl, names) = resolve_args(&b.target);
    // 扣除：按当前可用余额估算余额不足的人数与实际扣除总额（执行时以记账时的余额为准）
    let r = sqlx::query(&format!(
        "SELECT count(*), count(*) FILTER (WHERE av < $4),
                coalesce(sum(CASE WHEN av >= $4 THEN $4 WHEN $5 THEN greatest(av, 0) ELSE 0 END), 0)::bigint
           FROM (SELECT coalesce(a.balance - a.held, 0) - $7 AS av
                   FROM ({RESOLVE}) t LEFT JOIN accounts a ON a.user_id = t.id AND a.currency = $6) x"
    ))
    .bind(&st)
    .bind(&cl)
    .bind(&names)
    .bind(b.amount.max(0))
    .bind(b.shortfall == Shortfall::Partial)
    .bind(&b.currency)
    .bind(cur.min_balance)
    .fetch_one(&mut *c)
    .await?;
    let (count, short, deduct_total): (i64, i64, i64) = (r.get(0), r.get(1), r.get(2));
    // 名单中找不到（或已封禁）的用户名
    let missing: Vec<String> = if names.is_empty() {
        vec![]
    } else {
        sqlx::query_scalar(
            "SELECT n FROM unnest($1::text[]) AS n
              WHERE NOT EXISTS (SELECT 1 FROM identity.v_user_public u
                                 WHERE lower(u.username) = n AND u.status NOT IN ('banned', 'deleted'))
              LIMIT 50",
        )
        .bind(&names)
        .fetch_all(&mut *c)
        .await?
    };
    drop(c);
    let (total, short) = match b.direction {
        Direction::Grant => (i128::from(b.amount.max(0)) * i128::from(count), 0),
        Direction::Deduct => (i128::from(deduct_total), short),
    };
    let (subject, body) = b.direction.defaults();
    Ok(Json(json!({
        "count": count,
        "insufficient": short,
        "total_amount": i64::try_from(total).ok().filter(|t| *t <= MAX_AMOUNT),
        "scale": cur.info.scale,
        "missing": missing,
        "max_recipients": MAX_RECIPIENTS,
        "notify_available": messages_available(&ctx).await,
        "default_subject": subject,
        "default_body": body,
    }))
    .into_response())
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    currency: String,
    amount: i64,
    memo: String,
    target: Target,
    #[serde(default)]
    direction: Direction,
    #[serde(default)]
    shortfall: Shortfall,
    #[serde(default)]
    notify: Option<NotifyBody>,
}

#[derive(Debug, Deserialize)]
pub struct NotifyBody {
    #[serde(default)]
    subject: String,
    #[serde(default)]
    body: String,
}

pub async fn http_create(
    ctx: RequestCtx,
    headers: HeaderMap,
    Json(b): Json<CreateBody>,
) -> PResult<Response> {
    let actor = ctx.require_user()?;
    let key = crate::idem_header(&headers, &ctx)?;
    b.target.check()?;
    if !(1..=MAX_AMOUNT).contains(&b.amount) {
        return Err(PluginError::new(
            "ledger.amount_out_of_range",
            "amount must be between 1 and 2^53-1",
        ));
    }
    let memo = core::clean_memo(&b.memo)?.ok_or_else(|| PluginError::validation("memo is required"))?;
    let (def_subject, def_body) = b.direction.defaults();
    let notify = match &b.notify {
        Some(n) => {
            let subject = if n.subject.trim().is_empty() {
                def_subject
            } else {
                n.subject.trim()
            };
            let body = if n.body.trim().is_empty() {
                def_body
            } else {
                n.body.trim()
            };
            if subject.chars().count() > 120 || body.chars().count() > 5000 {
                return Err(PluginError::validation(
                    "subject ≤ 120 and body ≤ 5000 characters",
                ));
            }
            Some((subject.to_owned(), body.to_owned()))
        }
        None => None,
    };
    let mut c = ctx.sql.conn().await?;
    if let Some(id) = sqlx::query_scalar::<_, i64>("SELECT id FROM grant_batches WHERE request_key = $1")
        .bind(&key)
        .fetch_optional(&mut *c)
        .await?
    {
        drop(c);
        return Ok((StatusCode::OK, Json(batch_json(&ctx, id).await?)).into_response());
    }
    let cur = core::load_currency(&mut c, &b.currency).await?;
    if cur.info.disabled {
        return Err(PluginError::new(
            "ledger.currency_disabled",
            "currency is disabled",
        ));
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO grant_batches (request_key, currency, amount, memo, target, notify, subject, body, state, created_by,
                                    direction, shortfall)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 'granting', $9, $10, $11) RETURNING id",
    )
    .bind(&key)
    .bind(&b.currency)
    .bind(b.amount)
    .bind(&memo)
    .bind(serde_json::to_value(&b.target)?)
    .bind(notify.is_some())
    .bind(notify.as_ref().map(|n| n.0.as_str()))
    .bind(notify.as_ref().map(|n| n.1.as_str()))
    .bind(i64::try_from(actor).unwrap_or_default())
    .bind(b.direction.as_str())
    .bind(b.shortfall.as_str())
    .fetch_one(&mut *c)
    .await?;
    let (st, cl, names) = resolve_args(&b.target);
    let n = sqlx::query(&format!(
        "INSERT INTO grant_recipients (batch_id, user_id) SELECT $4, id FROM ({RESOLVE}) t ON CONFLICT DO NOTHING"
    ))
    .bind(&st)
    .bind(&cl)
    .bind(&names)
    .bind(id)
    .execute(&mut *c)
    .await?
    .rows_affected();
    let n = i64::try_from(n).unwrap_or(i64::MAX);
    if n == 0 {
        return Err(PluginError::new(
            "ledger.grant_no_recipients",
            "no user matches the target",
        ));
    }
    if n > MAX_RECIPIENTS {
        // `_too_many` 后缀默认映射为 429，这里是参数问题
        return Err(PluginError::new("ledger.grant_too_many", "too many recipients")
            .with_status(422)
            .with_details(json!({ "max": MAX_RECIPIENTS, "count": n })));
    }
    if i128::from(b.amount) * i128::from(n) > i128::from(MAX_AMOUNT) {
        return Err(PluginError::new(
            "ledger.amount_out_of_range",
            "total amount is too large",
        ));
    }
    sqlx::query("UPDATE grant_batches SET total = $2 WHERE id = $1")
        .bind(id)
        .bind(i32::try_from(n).unwrap_or(i32::MAX))
        .execute(&mut *c)
        .await?;
    drop(c);
    schedule(&ctx, id, 0).await?;
    ctx.audit
        .write(pt_plugin_api::AuditEntry {
            action: match b.direction {
                Direction::Grant => "ledger.grant",
                Direction::Deduct => "ledger.deduct",
            }
            .into(),
            target: format!("grant:{id}"),
            details: json!({ "currency": b.currency, "amount": b.amount, "memo": memo, "target": b.target,
                             "recipients": n, "notify": notify.is_some(), "direction": b.direction,
                             "shortfall": b.shortfall }),
        })
        .await?;
    Ok((StatusCode::CREATED, Json(batch_json(&ctx, id).await?)).into_response())
}

const BATCH_COLS: &str = "b.id, b.currency, b.amount, b.memo, b.target, b.notify, b.subject, b.body, b.state, b.total,
    b.granted, b.failed, b.notified, b.notify_failed, b.last_error, b.created_by, u.username,
    (extract(epoch FROM b.created_at) * 1000)::bigint, (extract(epoch FROM b.finished_at) * 1000)::bigint, c.scale,
    b.direction, b.shortfall, b.moved";

fn row_json(r: &sqlx::postgres::PgRow) -> serde_json::Value {
    json!({
        "id": r.get::<i64, _>(0), "currency": r.get::<String, _>(1), "amount": r.get::<i64, _>(2),
        "memo": r.get::<String, _>(3), "target": r.get::<serde_json::Value, _>(4), "notify": r.get::<bool, _>(5),
        "subject": r.get::<Option<String>, _>(6), "body": r.get::<Option<String>, _>(7),
        "state": r.get::<String, _>(8), "total": r.get::<i32, _>(9), "granted": r.get::<i32, _>(10),
        "failed": r.get::<i32, _>(11), "notified": r.get::<i32, _>(12), "notify_failed": r.get::<i32, _>(13),
        "last_error": r.get::<Option<String>, _>(14), "created_by": r.get::<i64, _>(15),
        "created_by_name": r.get::<Option<String>, _>(16), "created_at": r.get::<Option<i64>, _>(17),
        "finished_at": r.get::<Option<i64>, _>(18), "scale": r.get::<i16, _>(19),
        "direction": r.get::<String, _>(20), "shortfall": r.get::<String, _>(21), "moved": r.get::<i64, _>(22),
    })
}

async fn batch_json(ctx: &Ctx, id: i64) -> PResult<serde_json::Value> {
    let mut c = ctx.sql.conn().await?;
    let r = sqlx::query(&format!(
        "SELECT {BATCH_COLS} FROM grant_batches b JOIN currencies c ON c.code = b.currency
           LEFT JOIN identity.v_user_public u ON u.id = b.created_by WHERE b.id = $1"
    ))
    .bind(id)
    .fetch_optional(&mut *c)
    .await?
    .ok_or_else(|| PluginError::not_found("grant batch not found"))?;
    Ok(row_json(&r))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    before: Option<i64>,
    limit: Option<i64>,
}

pub async fn http_list(ctx: RequestCtx, Query(q): Query<ListQuery>) -> PResult<Response> {
    let limit = q.limit.unwrap_or(20).clamp(1, 100);
    let mut c = ctx.sql.conn().await?;
    let rows = sqlx::query(&format!(
        "SELECT {BATCH_COLS} FROM grant_batches b JOIN currencies c ON c.code = b.currency
           LEFT JOIN identity.v_user_public u ON u.id = b.created_by
          WHERE b.id < $1 ORDER BY b.id DESC LIMIT $2"
    ))
    .bind(q.before.unwrap_or(i64::MAX))
    .bind(limit + 1)
    .fetch_all(&mut *c)
    .await?;
    let more = i64::try_from(rows.len()).unwrap_or(0) > limit;
    let items: Vec<_> = rows
        .iter()
        .take(usize::try_from(limit).unwrap_or(20))
        .map(row_json)
        .collect();
    let next = if more {
        items.last().map(|i| i["id"].clone())
    } else {
        None
    };
    Ok(Json(json!({ "items": items, "next_before": next })).into_response())
}

/// 详情：进度与异常名单（发放 / 扣除失败、只扣了一部分、通知失败，最多 200 条）
pub async fn http_get(ctx: RequestCtx, Path(id): Path<i64>) -> PResult<Response> {
    let mut v = batch_json(&ctx, id).await?;
    let mut c = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT r.user_id, u.username, r.error, r.notify_error, r.amount FROM grant_recipients r
           JOIN grant_batches b ON b.id = r.batch_id
           LEFT JOIN identity.v_user_public u ON u.id = r.user_id
          WHERE r.batch_id = $1 AND (r.error IS NOT NULL OR r.notify_error IS NOT NULL OR r.amount <> b.amount)
          ORDER BY r.user_id LIMIT 200",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await?;
    v["failures"] = json!(
        rows.iter()
            .map(|r| json!({
                "user_id": r.get::<i64, _>(0), "username": r.get::<Option<String>, _>(1),
                "error": r.get::<Option<String>, _>(2), "notify_error": r.get::<Option<String>, _>(3),
                "amount": r.get::<Option<i64>, _>(4),
            }))
            .collect::<Vec<_>>()
    );
    Ok(Json(v).into_response())
}

/// 继续：任务重试耗尽后（如数据库故障）重新排队；已完成的批不受影响
pub async fn http_resume(ctx: RequestCtx, Path(id): Path<i64>) -> PResult<Response> {
    let mut c = ctx.sql.conn().await?;
    let state: Option<String> = sqlx::query_scalar(
        "UPDATE grant_batches SET notify_retries = 0, last_error = NULL WHERE id = $1 RETURNING state",
    )
    .bind(id)
    .fetch_optional(&mut *c)
    .await?;
    drop(c);
    match state.as_deref() {
        None => Err(PluginError::not_found("grant batch not found")),
        Some("done") => Err(PluginError::new(
            "ledger.grant_finished",
            "the batch has already finished",
        )),
        Some(_) => {
            schedule(&ctx, id, 0).await?;
            Ok(Json(batch_json(&ctx, id).await?).into_response())
        }
    }
}

async fn schedule(ctx: &Ctx, id: i64, delay_secs: i64) -> PResult<()> {
    ctx.jobs
        .schedule_once(
            JOB,
            ctx.time.now() + delay_secs * 1000,
            json!({ "batch_id": id }),
            None,
        )
        .await?;
    Ok(())
}

// ---------------- 任务 ----------------

struct Batch {
    state: String,
    direction: Direction,
    shortfall: Shortfall,
    currency: String,
    amount: i64,
    memo: String,
    notify: bool,
    subject: String,
    body: String,
    retries: i32,
}

/// 执行一块；还有剩余时排队下一块
pub async fn run(ctx: &Ctx, id: i64) -> PResult<()> {
    let b = {
        let mut c = ctx.sql.conn().await?;
        let Some(r) = sqlx::query(
            "SELECT state, currency, amount, memo, notify, coalesce(subject, ''), coalesce(body, ''), notify_retries,
                    direction, shortfall
               FROM grant_batches WHERE id = $1 FOR UPDATE",
        )
        .bind(id)
        .fetch_optional(&mut *c)
        .await?
        else {
            return Ok(());
        };
        Batch {
            state: r.get(0),
            direction: if r.get::<String, _>(8) == "deduct" {
                Direction::Deduct
            } else {
                Direction::Grant
            },
            shortfall: if r.get::<String, _>(9) == "partial" {
                Shortfall::Partial
            } else {
                Shortfall::Skip
            },
            currency: r.get(1),
            amount: r.get(2),
            memo: r.get(3),
            notify: r.get(4),
            subject: r.get(5),
            body: r.get(6),
            retries: r.get(7),
        }
    };
    match b.state.as_str() {
        "granting" => grant_chunk(ctx, id, &b).await,
        "notifying" => notify_chunk(ctx, id, &b).await,
        _ => Ok(()),
    }
}

/// 单笔业务错误只记在该收款人上；基础设施错误让整块回滚重试
fn business(e: &PluginError) -> bool {
    e.code.starts_with("ledger.")
        || e.code == "kernel.validation_failed"
        || e.code == "kernel.idempotency_key_reused"
}

async fn grant_chunk(ctx: &Ctx, id: i64, b: &Batch) -> PResult<()> {
    let users: Vec<i64> = {
        let mut c = ctx.sql.conn().await?;
        sqlx::query_scalar(
            "SELECT user_id FROM grant_recipients WHERE batch_id = $1 AND entry_id IS NULL AND error IS NULL
              ORDER BY user_id LIMIT $2",
        )
        .bind(id)
        .bind(GRANT_CHUNK)
        .fetch_all(&mut *c)
        .await?
    };
    // 扣除：先按可用余额算出每人实际扣多少（不足且 skip 的直接跳过，不进入记账）
    let available: std::collections::HashMap<i64, i64> = if b.direction == Direction::Deduct {
        let mut c = ctx.sql.conn().await?;
        let cur = core::load_currency(&mut c, &b.currency).await?;
        let rows: Vec<(i64, i64)> = sqlx::query_as(
            "SELECT user_id, balance - held - $3 FROM accounts WHERE currency = $1 AND user_id = ANY($2)",
        )
        .bind(&b.currency)
        .bind(&users)
        .bind(cur.min_balance)
        .fetch_all(&mut *c)
        .await?;
        let mut m: std::collections::HashMap<i64, i64> = rows.into_iter().collect();
        for u in &users {
            m.entry(*u).or_insert(-cur.min_balance);
        }
        m
    } else {
        std::collections::HashMap::new()
    };
    let (mut uids, mut entries, mut balances, mut amounts, mut errors) =
        (vec![], vec![], vec![], vec![], vec![]);
    for &u in &users {
        uids.push(u);
        let uid = Some(u64::try_from(u).unwrap_or_default());
        let (amount, from, to, reason) = match b.direction {
            Direction::Grant => (b.amount, None, uid, "ledger.admin_grant"),
            Direction::Deduct => (
                deduct_amount(b.amount, available.get(&u).copied().unwrap_or(0), b.shortfall),
                uid,
                None,
                "ledger.admin_deduct",
            ),
        };
        if amount <= 0 {
            entries.push(None);
            balances.push(None);
            amounts.push(None);
            errors.push(Some("ledger.insufficient_balance".to_owned()));
            continue;
        }
        let req = TransferRequest {
            currency: b.currency.clone(),
            from_user: from,
            to_user: to,
            amount,
            reason: reason.into(),
            memo: b.memo.clone(),
            idempotency_key: format!("grant:{id}:{u}"),
        };
        match core::transfer(ctx, req, true).await {
            Ok(r) => {
                entries.push(uuid::Uuid::parse_str(&r.entry_id).ok());
                balances.push(r.to_balance_after.or(r.from_balance_after));
                amounts.push(Some(amount));
                errors.push(None);
            }
            Err(e) if business(&e) => {
                entries.push(None);
                balances.push(None);
                amounts.push(None);
                errors.push(Some(e.code));
            }
            Err(e) => return Err(e),
        }
    }
    let ok = i32::try_from(errors.iter().filter(|e| e.is_none()).count()).unwrap_or(0);
    let bad = i32::try_from(errors.len()).unwrap_or(0) - ok;
    let moved: i64 = amounts.iter().flatten().sum();
    let finished = i64::try_from(users.len()).unwrap_or(0) < GRANT_CHUNK;
    let next = match (finished, b.notify) {
        (false, _) => "granting",
        (true, true) if messages_available(ctx).await => "notifying",
        (true, true) => "done_skip",
        (true, false) => "done",
    };
    let mut c = ctx.sql.conn().await?;
    sqlx::query(
        "UPDATE grant_recipients r SET entry_id = x.e, balance_after = x.b, error = x.err, amount = x.a, granted_at = now()
           FROM unnest($2::bigint[], $3::uuid[], $4::bigint[], $5::text[], $6::bigint[]) AS x(u, e, b, err, a)
          WHERE r.batch_id = $1 AND r.user_id = x.u",
    )
    .bind(id)
    .bind(&uids)
    .bind(&entries)
    .bind(&balances)
    .bind(&errors)
    .bind(&amounts)
    .execute(&mut *c)
    .await?;
    sqlx::query(
        "UPDATE grant_batches SET granted = granted + $2, failed = failed + $3, moved = moved + $5,
                state = CASE WHEN $4 IN ('done', 'done_skip') THEN 'done' ELSE $4 END,
                finished_at = CASE WHEN $4 IN ('done', 'done_skip') THEN now() END,
                last_error = CASE WHEN $4 = 'done_skip' THEN 'messages_unavailable' ELSE last_error END,
                notify_failed = CASE WHEN $4 = 'done_skip' THEN granted + $2 ELSE notify_failed END
          WHERE id = $1",
    )
    .bind(id)
    .bind(ok)
    .bind(bad)
    .bind(next)
    .bind(moved)
    .execute(&mut *c)
    .await?;
    drop(c);
    if !next.starts_with("done") {
        schedule(ctx, id, 0).await?;
    }
    Ok(())
}

/// 待通知的人：(用户 ID, 用户名, 记账后余额, 实际数量)
type NotifyRow = (i64, Option<String>, Option<i64>, Option<i64>);

async fn notify_chunk(ctx: &Ctx, id: i64, b: &Batch) -> PResult<()> {
    let (rows, cur) = {
        let mut c = ctx.sql.conn().await?;
        let cur = core::load_currency(&mut c, &b.currency).await?;
        let rows: Vec<NotifyRow> = sqlx::query_as(
            "SELECT r.user_id, u.username, r.balance_after, r.amount FROM grant_recipients r
               LEFT JOIN identity.v_user_public u ON u.id = r.user_id
              WHERE r.batch_id = $1 AND r.entry_id IS NOT NULL AND r.notified_at IS NULL AND r.notify_error IS NULL
              ORDER BY r.user_id LIMIT $2",
        )
        .bind(id)
        .bind(NOTIFY_CHUNK)
        .fetch_all(&mut *c)
        .await?;
        (rows, cur)
    };
    let scale = cur.info.scale;
    let cname = ctx
        .terms
        .get(&cur.info.name_term, None)
        .await
        .map_or_else(|| cur.info.code.clone(), |t| t.name);
    let (mut sent, mut failed): (Vec<i64>, Vec<(i64, String)>) = (vec![], vec![]);
    // stop：Some(true) = 站内信不可用（放弃剩余）；Some(false) = 暂时失败（稍后重试）
    let mut stop: Option<(bool, String)> = None;
    for (uid, name, bal, amt) in &rows {
        let name = name.clone().unwrap_or_else(|| format!("#{uid}"));
        let bal = bal.map(|v| fmt_amount(v, scale)).unwrap_or_default();
        // 部分扣除时为此人实际扣除的数量
        let amount = fmt_amount(amt.unwrap_or(b.amount), scale);
        let vars = [
            ("username", name.as_str()),
            ("amount", amount.as_str()),
            ("currency", cname.as_str()),
            ("balance", bal.as_str()),
            ("memo", b.memo.as_str()),
        ];
        let r = ctx
            .call_json(
                "ctx.messages",
                "send_system",
                json!({ "user_id": uid, "subject": render(&b.subject, &vars), "body": render(&b.body, &vars) }),
            )
            .await;
        match r {
            Ok(_) => sent.push(*uid),
            Err(e) if e.code == "kernel.capability_denied" || e.code == "kernel.service_unavailable" => {
                stop = Some((true, e.code));
                break;
            }
            Err(e) if e.retryable => {
                stop = Some((false, e.code));
                break;
            }
            Err(e) => failed.push((*uid, e.code)),
        }
    }
    let mut c = ctx.sql.conn().await?;
    sqlx::query("UPDATE grant_recipients SET notified_at = now() WHERE batch_id = $1 AND user_id = ANY($2)")
        .bind(id)
        .bind(&sent)
        .execute(&mut *c)
        .await?;
    let (fu, fe): (Vec<i64>, Vec<String>) = failed.into_iter().unzip();
    sqlx::query(
        "UPDATE grant_recipients r SET notify_error = x.err FROM unnest($2::bigint[], $3::text[]) AS x(u, err)
          WHERE r.batch_id = $1 AND r.user_id = x.u",
    )
    .bind(id)
    .bind(&fu)
    .bind(&fe)
    .execute(&mut *c)
    .await?;
    let n_sent = i32::try_from(sent.len()).unwrap_or(0);
    let n_fail = i32::try_from(fu.len()).unwrap_or(0);
    let give_up = match &stop {
        Some((true, _)) => true,
        Some((false, _)) => b.retries + 1 >= MAX_NOTIFY_RETRIES,
        None => false,
    };
    if give_up {
        let code = stop.as_ref().map(|s| s.1.clone()).unwrap_or_default();
        let rest = sqlx::query(
            "UPDATE grant_recipients SET notify_error = $2
              WHERE batch_id = $1 AND entry_id IS NOT NULL AND notified_at IS NULL AND notify_error IS NULL",
        )
        .bind(id)
        .bind(&code)
        .execute(&mut *c)
        .await?
        .rows_affected();
        sqlx::query(
            "UPDATE grant_batches SET notified = notified + $2, notify_failed = notify_failed + $3, state = 'done',
                    finished_at = now(), last_error = $4 WHERE id = $1",
        )
        .bind(id)
        .bind(n_sent)
        .bind(n_fail + i32::try_from(rest).unwrap_or(0))
        .bind(&code)
        .execute(&mut *c)
        .await?;
        return Ok(());
    }
    let done = stop.is_none() && i64::try_from(rows.len()).unwrap_or(0) < NOTIFY_CHUNK;
    sqlx::query(
        "UPDATE grant_batches SET notified = notified + $2, notify_failed = notify_failed + $3,
                notify_retries = $4, last_error = $5,
                state = CASE WHEN $6 THEN 'done' ELSE state END, finished_at = CASE WHEN $6 THEN now() END
          WHERE id = $1",
    )
    .bind(id)
    .bind(n_sent)
    .bind(n_fail)
    .bind(if stop.is_some() { b.retries + 1 } else { 0 })
    .bind(stop.as_ref().map(|s| s.1.clone()))
    .bind(done)
    .execute(&mut *c)
    .await?;
    drop(c);
    if !done {
        schedule(ctx, id, if stop.is_some() { NOTIFY_RETRY_SECS } else { 0 }).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts() {
        assert_eq!(fmt_amount(1000, 1), "100");
        assert_eq!(fmt_amount(1005, 1), "100.5");
        assert_eq!(fmt_amount(-25, 2), "-0.25");
        assert_eq!(fmt_amount(7, 0), "7");
    }

    #[test]
    fn templates() {
        let s = render(DEFAULT_SUBJECT, &[("amount", "100"), ("currency", "魔力值")]);
        assert_eq!(s, "你收到了 100 魔力值");
        assert_eq!(render("{x}{y}{x}", &[("x", "a")]), "a{y}a");
    }

    #[test]
    fn deduct_shortfall() {
        assert_eq!(deduct_amount(100, 500, Shortfall::Skip), 100);
        assert_eq!(deduct_amount(100, 30, Shortfall::Skip), 0);
        assert_eq!(deduct_amount(100, 30, Shortfall::Partial), 30);
        assert_eq!(deduct_amount(100, -5, Shortfall::Partial), 0);
    }
}
