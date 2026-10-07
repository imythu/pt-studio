//! 记账核心（ledger 03 §1.2～§1.7）：守卫、幂等、分录、冻结、锁定、冲正。

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use pt_plugin_api::{ActorKind, Ctx, NewEvent, PResult, PluginError, host::HookOutcome, json};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, Row};

use crate::api::{
    CaptureRequest, CurrencyDef, CurrencyInfo, HoldRequest, LockRequest, TransferRequest, TransferResult,
};

pub const MAX_AMOUNT: i64 = 9_007_199_254_740_991; // 2^53 - 1
pub const SHARDS: i64 = 16;

/// 调用方：服务调用为调用方插件及其作用域；ledger 自身的 HTTP / 任务为 "ledger"（不受作用域限制）。
#[derive(Debug, Clone)]
pub struct Caller {
    pub plugin: String,
    pub scopes: Vec<String>,
}

impl Caller {
    pub fn of(ctx: &Ctx) -> Self {
        match ctx.invocation.kind {
            pt_plugin_api::InvocationKind::Service => Self {
                plugin: ctx.invocation.caller_plugin.clone(),
                scopes: ctx.invocation.caller_scopes.clone(),
            },
            _ => Self {
                plugin: "ledger".into(),
                scopes: vec![],
            },
        }
    }
    pub fn is_self(&self) -> bool {
        self.plugin == "ledger"
    }
    pub fn has(&self, scope: &str) -> bool {
        self.is_self() || self.scopes.iter().any(|s| s == scope)
    }
    fn require_currency(&self, code: &str) -> PResult<()> {
        if self.has(&format!("currency:{code}")) {
            Ok(())
        } else {
            Err(PluginError::new(
                "ledger.currency_not_allowed",
                format!("scope currency:{code} required"),
            )
            .with_status(403))
        }
    }
}

#[derive(Debug, Clone)]
pub struct Currency {
    pub info: CurrencyInfo,
    pub min_balance: i64,
    pub max_hold_secs: i64,
}

fn validation(path: &str, msg: &str) -> PluginError {
    PluginError::validation(msg.to_owned()).with_details(
        json!({ "errors": [{ "path": path, "code": "kernel.validation_failed", "message": msg }] }),
    )
}

fn check_amount(a: i64) -> PResult<()> {
    if (1..=MAX_AMOUNT).contains(&a) {
        Ok(())
    } else {
        Err(PluginError::new(
            "ledger.amount_out_of_range",
            "amount must be between 1 and 2^53-1",
        ))
    }
}

fn check_key(k: &str) -> PResult<()> {
    if (1..=128).contains(&k.len()) && k.bytes().all(|c| (0x21..=0x7e).contains(&c)) {
        Ok(())
    } else {
        Err(validation(
            "/idempotency_key",
            "idempotency key must be 1-128 visible ASCII characters",
        ))
    }
}

fn check_reason(caller: &Caller, reason: &str) -> PResult<()> {
    let ok = reason.split_once('.').is_some_and(|(p, c)| {
        p == caller.plugin
            && !c.is_empty()
            && c.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
            && c.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    });
    if ok {
        Ok(())
    } else {
        Err(validation(
            "/reason",
            "reason must be `<caller_plugin>.<snake_case>`",
        ))
    }
}

pub fn clean_memo(m: &str) -> PResult<Option<String>> {
    let s: String = m
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_owned();
    if s.chars().count() > 200 {
        return Err(validation("/memo", "memo must be at most 200 characters"));
    }
    Ok((!s.is_empty()).then_some(s))
}

pub fn request_hash(v: &serde_json::Value) -> Vec<u8> {
    // serde_json 的 Map 默认按键排序（BTreeMap），to_vec 即为规范形
    Sha256::digest(serde_json::to_vec(v).unwrap_or_default()).to_vec()
}

fn u2i(u: u64) -> PResult<i64> {
    i64::try_from(u).map_err(|_| PluginError::bad_request("user id out of range"))
}

// ---------------- 币种（§1.1） ----------------

pub async fn load_currency(c: &mut PgConnection, code: &str) -> PResult<Currency> {
    let r = sqlx::query(
        "SELECT c.code, c.scale, c.name_term, c.owner_plugin, s.transferable, s.fee_bps, s.min_transfer,
                s.daily_out_limit, s.disabled, s.min_balance, s.max_hold_secs
           FROM currencies c JOIN currency_settings s ON s.currency = c.code WHERE c.code = $1",
    )
    .bind(code)
    .fetch_optional(&mut *c)
    .await?
    .ok_or_else(|| {
        PluginError::new(
            "ledger.currency_not_found",
            format!("currency `{code}` not found"),
        )
        .with_status(404)
    })?;
    Ok(row_currency(&r))
}

fn row_currency(r: &sqlx::postgres::PgRow) -> Currency {
    Currency {
        info: CurrencyInfo {
            code: r.get(0),
            scale: u8::try_from(r.get::<i16, _>(1)).unwrap_or(0),
            name_term: r.get(2),
            owner_plugin: r.get(3),
            transferable: r.get(4),
            fee_bps: u32::try_from(r.get::<i32, _>(5)).unwrap_or(0),
            min_transfer: r.get(6),
            daily_out_limit: r.get(7),
            disabled: r.get(8),
        },
        min_balance: r.get(9),
        max_hold_secs: i64::from(r.get::<i32, _>(10)),
    }
}

pub async fn list_currencies(c: &mut PgConnection) -> PResult<Vec<CurrencyInfo>> {
    let rows = sqlx::query(
        "SELECT c.code, c.scale, c.name_term, c.owner_plugin, s.transferable, s.fee_bps, s.min_transfer,
                s.daily_out_limit, s.disabled, s.min_balance, s.max_hold_secs
           FROM currencies c JOIN currency_settings s ON s.currency = c.code ORDER BY c.code",
    )
    .fetch_all(&mut *c)
    .await?;
    Ok(rows.iter().map(|r| row_currency(r).info).collect())
}

async fn writable_currency(c: &mut PgConnection, caller: &Caller, code: &str) -> PResult<Currency> {
    let cur = load_currency(c, code).await?;
    if cur.info.disabled {
        return Err(PluginError::new(
            "ledger.currency_disabled",
            format!("currency `{code}` is disabled"),
        )
        .with_status(403));
    }
    caller.require_currency(code)?;
    Ok(cur)
}

pub async fn register_currency(ctx: &Ctx, def: CurrencyDef) -> PResult<()> {
    let caller = Caller::of(ctx);
    caller.require_currency(&def.code)?;
    let code_ok = def.code.len() >= 2
        && def.code.len() <= 16
        && def.code.bytes().next().is_some_and(|b| b.is_ascii_lowercase())
        && def
            .code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    if !code_ok || def.scale > 6 {
        return Err(validation(
            "/code",
            "currency code must match ^[a-z][a-z0-9_]{1,15}$ and scale 0..=6",
        ));
    }
    if !(def.name_term.starts_with(&format!("{}.", caller.plugin))
        || def.name_term.starts_with("ledger.currency."))
    {
        return Err(validation(
            "/name_term",
            "name_term must be prefixed with the caller plugin id",
        ));
    }
    let mut c = ctx.sql.conn().await?;
    let inserted: Option<String> = sqlx::query_scalar(
        "INSERT INTO currencies (code, scale, name_term, owner_plugin) VALUES ($1,$2,$3,$4)
         ON CONFLICT (code) DO NOTHING RETURNING code",
    )
    .bind(&def.code)
    .bind(i16::from(def.scale))
    .bind(&def.name_term)
    .bind(&caller.plugin)
    .fetch_optional(&mut *c)
    .await?;
    if inserted.is_some() {
        let o = &def.options;
        sqlx::query(
            "INSERT INTO currency_settings (currency, transferable, fee_bps, min_transfer, daily_out_limit) VALUES ($1,$2,$3,$4,$5)",
        )
        .bind(&def.code)
        .bind(o.transferable.unwrap_or(false))
        .bind(i32::try_from(o.fee_bps.unwrap_or(0).min(10_000)).unwrap_or(0))
        .bind(o.min_transfer.unwrap_or(1).max(1))
        .bind(o.daily_out_limit.unwrap_or(0).max(0))
        .execute(&mut *c)
        .await?;
        sqlx::query("INSERT INTO supply_shards (currency, shard) SELECT $1, g FROM generate_series(0, 15) g")
            .bind(&def.code)
            .execute(&mut *c)
            .await?;
        drop(c);
        ctx.events
            .publish(NewEvent::v1("ledger.currency_registered", "global", json!({
                "currency": def.code, "scale": def.scale, "name_term": def.name_term, "owner_plugin": caller.plugin })))
            .await?;
        return Ok(());
    }
    let r = sqlx::query("SELECT scale, name_term, owner_plugin FROM currencies WHERE code = $1")
        .bind(&def.code)
        .fetch_one(&mut *c)
        .await?;
    if r.get::<String, _>(2) != caller.plugin {
        return Err(PluginError::new(
            "ledger.currency_owned_by_other",
            "currency belongs to another plugin",
        )
        .with_status(409));
    }
    if r.get::<i16, _>(0) != i16::from(def.scale) || r.get::<String, _>(1) != def.name_term {
        return Err(
            PluginError::new("ledger.currency_immutable", "scale and name_term cannot change")
                .with_status(409),
        );
    }
    Ok(())
}

// ---------------- 幂等（§1.2、§1.4 第 1 步） ----------------

pub enum Idem {
    Fresh,
    Replay(serde_json::Value),
}

/// 写操作开始前的只读预检（钩子之前；命中则不触发钩子）。
pub async fn idem_peek(c: &mut PgConnection, caller: &Caller, key: &str, hash: &[u8]) -> PResult<Idem> {
    let r = sqlx::query("SELECT request_hash, result FROM idempotency WHERE caller_plugin = $1 AND key = $2")
        .bind(&caller.plugin)
        .bind(key)
        .fetch_optional(&mut *c)
        .await?;
    replay_of(r, hash)
}

fn replay_of(r: Option<sqlx::postgres::PgRow>, hash: &[u8]) -> PResult<Idem> {
    let Some(r) = r else { return Ok(Idem::Fresh) };
    if r.get::<Vec<u8>, _>(0) != hash {
        return Err(PluginError::new(
            "kernel.idempotency_key_reused",
            "idempotency key was used for a different request",
        ));
    }
    match r.get::<Option<serde_json::Value>, _>(1) {
        Some(v) => Ok(Idem::Replay(v)),
        None => Err(PluginError::new(
            "ledger.idempotency_in_progress",
            "request with this key is in progress",
        )
        .with_status(409)
        .retryable()),
    }
}

/// 在事务中登记幂等键；并发同键时等待先到者提交。
pub async fn idem_begin(c: &mut PgConnection, caller: &Caller, key: &str, hash: &[u8]) -> PResult<Idem> {
    let ins: Option<i32> = sqlx::query_scalar(
        "INSERT INTO idempotency (caller_plugin, key, request_hash) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING RETURNING 1",
    )
    .bind(&caller.plugin)
    .bind(key)
    .bind(hash)
    .fetch_optional(&mut *c)
    .await?;
    if ins.is_some() {
        return Ok(Idem::Fresh);
    }
    let r = sqlx::query(
        "SELECT request_hash, result FROM idempotency WHERE caller_plugin = $1 AND key = $2 FOR SHARE",
    )
    .bind(&caller.plugin)
    .bind(key)
    .fetch_optional(&mut *c)
    .await?;
    replay_of(r, hash)
}

pub async fn idem_finish(
    c: &mut PgConnection,
    caller: &Caller,
    key: &str,
    entry_id: Option<uuid::Uuid>,
    result: &serde_json::Value,
) -> PResult<()> {
    sqlx::query("UPDATE idempotency SET entry_id = $3, result = $4 WHERE caller_plugin = $1 AND key = $2")
        .bind(&caller.plugin)
        .bind(key)
        .bind(entry_id)
        .bind(result)
        .execute(&mut *c)
        .await?;
    Ok(())
}

fn replayed(v: serde_json::Value) -> PResult<TransferResult> {
    let mut r: TransferResult =
        serde_json::from_value(v).map_err(|_| PluginError::internal("bad stored result"))?;
    r.duplicate = true;
    Ok(r)
}

// ---------------- 记账（§1.4） ----------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// 未锁定且可用余额 ≥ min_balance
    Normal,
    /// 只检查余额（冲正、管理员回收、capture）
    IgnoreLock,
    /// 允许负余额（管理员冲正 allow_negative）
    None,
}

#[derive(Debug, Clone)]
pub struct Change {
    pub user: Option<i64>,
    pub delta: i64,
    pub check: Check,
    pub counterparty: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Op {
    pub kind: &'static str,
    pub reason: String,
    pub memo: Option<String>,
    pub changes: Vec<Change>,
    pub issued: i64,
    pub burned: i64,
    pub reversal_of: Option<uuid::Uuid>,
    pub hold_id: Option<uuid::Uuid>,
    /// 用户扣减时同时减少的冻结额（capture）
    pub release_held: Option<(i64, i64)>,
}

pub struct Posted {
    pub entry_id: uuid::Uuid,
    pub created_at: DateTime<Utc>,
    pub balances: BTreeMap<i64, i64>,
}

async fn diagnose(c: &mut PgConnection, uid: i64, cur: &Currency, required: i64) -> PResult<PluginError> {
    let r =
        sqlx::query("SELECT balance, held, lock_count FROM accounts WHERE user_id = $1 AND currency = $2")
            .bind(uid)
            .bind(&cur.info.code)
            .fetch_optional(&mut *c)
            .await?;
    let (bal, held, locks): (i64, i64, i32) = r.map_or((0, 0, 0), |r| (r.get(0), r.get(1), r.get(2)));
    if locks > 0 {
        return Ok(PluginError::new("ledger.account_locked", "account is locked").with_status(403));
    }
    Ok(
        PluginError::new("ledger.insufficient_balance", "insufficient balance").with_details(
            json!({ "available": bal - held, "required": required, "currency": cur.info.code }),
        ),
    )
}

/// 一次分录：账户变动（按 user_id 升序）、流水、分录行、供给分片。
pub async fn post(
    c: &mut PgConnection,
    ctx: &Ctx,
    cur: &Currency,
    caller: &Caller,
    op: &Op,
) -> PResult<Posted> {
    let sum: i64 = op.changes.iter().map(|ch| ch.delta).sum();
    if sum != 0 {
        return Err(PluginError::internal("unbalanced entry"));
    }
    let mut users: Vec<&Change> = op.changes.iter().filter(|ch| ch.user.is_some()).collect();
    users.sort_by_key(|ch| ch.user);
    let mut balances = BTreeMap::new();
    for ch in users {
        let uid = ch.user.unwrap_or_default();
        let bal: Option<i64> = if ch.delta < 0 {
            let held_release = op.release_held.filter(|(u, _)| *u == uid).map_or(0, |(_, h)| h);
            let sql = match ch.check {
                Check::Normal => {
                    "UPDATE accounts SET balance = balance + $3, held = held - $5, version = version + 1, updated_at = now()
                      WHERE user_id = $1 AND currency = $2 AND lock_count = 0 AND balance + $3 - (held - $5) >= $4 RETURNING balance"
                }
                Check::IgnoreLock => {
                    "UPDATE accounts SET balance = balance + $3, held = held - $5, version = version + 1, updated_at = now()
                      WHERE user_id = $1 AND currency = $2 AND balance + $3 - (held - $5) >= $4 RETURNING balance"
                }
                Check::None => {
                    "UPDATE accounts SET balance = balance + $3, held = held - $5, version = version + 1, updated_at = now()
                      WHERE user_id = $1 AND currency = $2 AND $4 = $4 RETURNING balance"
                }
            };
            let r: Option<i64> = sqlx::query_scalar(sql)
                .bind(uid)
                .bind(&cur.info.code)
                .bind(ch.delta)
                .bind(cur.min_balance)
                .bind(held_release)
                .fetch_optional(&mut *c)
                .await?;
            if r.is_none() && ch.check == Check::None {
                // 允许负余额但账户不存在：创建
                Some(
                    sqlx::query_scalar(
                        "INSERT INTO accounts (user_id, currency, balance) VALUES ($1,$2,$3)
                         ON CONFLICT (user_id, currency) DO UPDATE SET balance = accounts.balance + EXCLUDED.balance RETURNING balance",
                    )
                    .bind(uid)
                    .bind(&cur.info.code)
                    .bind(ch.delta)
                    .fetch_one(&mut *c)
                    .await?,
                )
            } else {
                r
            }
        } else {
            let r = sqlx::query_scalar(
                "INSERT INTO accounts (user_id, currency, balance, lock_count)
                 VALUES ($1, $2, $3, (SELECT count(*) FROM account_locks WHERE user_id = $1 AND currency IS NULL AND released_at IS NULL))
                 ON CONFLICT (user_id, currency) DO UPDATE
                   SET balance = accounts.balance + EXCLUDED.balance, version = accounts.version + 1, updated_at = now()
                 RETURNING balance",
            )
            .bind(uid)
            .bind(&cur.info.code)
            .bind(ch.delta)
            .fetch_one(&mut *c)
            .await;
            match r {
                Ok(b) => Some(b),
                Err(sqlx::Error::Database(db)) if db.constraint() == Some("accounts_balance_range_check") => {
                    return Err(PluginError::new(
                        "ledger.amount_out_of_range",
                        "balance would exceed 2^53-1",
                    ));
                }
                Err(e) => return Err(e.into()),
            }
        };
        match bal {
            Some(b) => {
                balances.insert(uid, b);
            }
            None => return Err(diagnose(c, uid, cur, -ch.delta).await?),
        }
    }
    let entry_id = uuid::Uuid::now_v7();
    let created_at = Utc::now();
    let actor = ctx.actor();
    let actor_kind = match actor.kind {
        ActorKind::Anonymous => "anonymous",
        ActorKind::User => "user",
        ActorKind::System => "system",
        ActorKind::Plugin => "plugin",
        ActorKind::Mcp => "mcp",
    };
    sqlx::query(
        "INSERT INTO entries (id, created_at, currency, kind, reason, memo, caller_plugin, actor_kind, actor_user_id, reversal_of, hold_id)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(entry_id)
    .bind(created_at)
    .bind(&cur.info.code)
    .bind(op.kind)
    .bind(&op.reason)
    .bind(&op.memo)
    .bind(&caller.plugin)
    .bind(actor_kind)
    .bind(actor.user_id.and_then(|u| i64::try_from(u).ok()))
    .bind(op.reversal_of)
    .bind(op.hold_id)
    .execute(&mut *c)
    .await?;
    let mut line_users = Vec::new();
    let mut amounts = Vec::new();
    let mut after = Vec::new();
    let mut counter = Vec::new();
    let mut nos: Vec<i16> = Vec::new();
    for (i, ch) in op.changes.iter().enumerate() {
        if ch.delta == 0 {
            continue;
        }
        nos.push(i16::try_from(i).unwrap_or(i16::MAX));
        line_users.push(ch.user);
        amounts.push(ch.delta);
        after.push(ch.user.and_then(|u| balances.get(&u).copied()));
        counter.push(ch.counterparty);
    }
    sqlx::query(
        "INSERT INTO lines (entry_id, created_at, line_no, currency, user_id, amount, balance_after, counterparty_user_id)
         SELECT $1, $2, n, $3, u, a, b, cp FROM unnest($4::smallint[], $5::bigint[], $6::bigint[], $7::bigint[], $8::bigint[]) AS t(n, u, a, b, cp)",
    )
    .bind(entry_id)
    .bind(created_at)
    .bind(&cur.info.code)
    .bind(&nos)
    .bind(&line_users)
    .bind(&amounts)
    .bind(&after)
    .bind(&counter)
    .execute(&mut *c)
    .await?;
    if op.issued != 0 || op.burned != 0 {
        let shard_user = op.changes.iter().find_map(|ch| ch.user).unwrap_or(0);
        sqlx::query("UPDATE supply_shards SET issued = issued + $3, burned = burned + $4 WHERE currency = $1 AND shard = $2")
            .bind(&cur.info.code)
            .bind(i16::try_from(shard_user.rem_euclid(SHARDS)).unwrap_or(0))
            .bind(op.issued)
            .bind(op.burned)
            .execute(&mut *c)
            .await?;
    }
    Ok(Posted {
        entry_id,
        created_at,
        balances,
    })
}

/// 每个用户分录一条 `ledger.entry_posted`（不含 memo）。
pub async fn publish_posted(ctx: &Ctx, cur: &Currency, caller: &Caller, op: &Op, p: &Posted) -> PResult<()> {
    for ch in op.changes.iter().filter(|ch| ch.user.is_some() && ch.delta != 0) {
        let uid = ch.user.unwrap_or_default();
        ctx.events
            .publish(NewEvent::v1(
                "ledger.entry_posted",
                format!("user:{uid}"),
                json!({
                "entry_id": p.entry_id, "currency": cur.info.code, "user_id": uid, "amount": ch.delta,
                "balance_after": p.balances.get(&uid), "reason": op.reason, "kind": op.kind,
                "caller_plugin": caller.plugin, "counterparty_user_id": ch.counterparty }),
            ))
            .await?;
    }
    Ok(())
}

async fn run_hook(
    ctx: &Ctx,
    operation: &str,
    cur: &str,
    from: i64,
    to: Option<i64>,
    amount: i64,
    reason: &str,
) -> PResult<()> {
    let mut input = json!({ "version": 1, "actor": ctx.invocation.actor, "operation": operation, "currency": cur,
                            "from_user": from, "amount": amount, "reason": reason });
    if let Some(t) = to {
        input["to_user"] = json!(t);
    }
    match ctx.hooks.run("ledger.before_transfer", input).await? {
        HookOutcome::Allowed(_) => Ok(()),
        HookOutcome::Denied {
            code,
            message,
            details,
            ..
        } => {
            let mut e = PluginError::new(code, message);
            e.details = details;
            Err(e)
        }
    }
}

/// 发行 / 回收 / 转账（§1.4）。`adjust = true` 为管理员调整：kind = adjust，回收不检查锁定、不触发钩子。
pub async fn transfer(ctx: &Ctx, req: TransferRequest, adjust: bool) -> PResult<TransferResult> {
    let caller = Caller::of(ctx);
    check_key(&req.idempotency_key)?;
    check_reason(&caller, &req.reason)?;
    check_amount(req.amount)?;
    let memo = clean_memo(&req.memo)?;
    let from = req.from_user.map(u2i).transpose()?;
    let to = req.to_user.map(u2i).transpose()?;
    if from.is_some() && from == to {
        return Err(PluginError::new(
            "ledger.self_transfer",
            "cannot transfer to yourself",
        ));
    }
    let mut hashed = serde_json::to_value(&req)?;
    if let Some(o) = hashed.as_object_mut() {
        o.remove("idempotency_key");
        o.insert("adjust".into(), json!(adjust));
    }
    let hash = request_hash(&hashed);
    let cur = {
        let mut c = ctx.sql.conn().await?;
        let cur = writable_currency(&mut c, &caller, &req.currency).await?;
        if let Idem::Replay(v) = idem_peek(&mut c, &caller, &req.idempotency_key, &hash).await? {
            return replayed(v);
        }
        cur
    };
    let a = req.amount;
    let (op, fee) = match (from, to) {
        (None, Some(t)) => (
            Op {
                kind: if adjust { "adjust" } else { "issue" },
                reason: req.reason.clone(),
                memo,
                changes: vec![
                    Change {
                        user: Some(t),
                        delta: a,
                        check: Check::Normal,
                        counterparty: None,
                    },
                    Change {
                        user: None,
                        delta: -a,
                        check: Check::None,
                        counterparty: None,
                    },
                ],
                issued: a,
                burned: 0,
                reversal_of: None,
                hold_id: None,
                release_held: None,
            },
            0,
        ),
        (Some(f), None) => {
            if !adjust {
                run_hook(ctx, "burn", &req.currency, f, None, a, &req.reason).await?;
            }
            (
                Op {
                    kind: if adjust { "adjust" } else { "burn" },
                    reason: req.reason.clone(),
                    memo,
                    changes: vec![
                        Change {
                            user: Some(f),
                            delta: -a,
                            check: if adjust { Check::IgnoreLock } else { Check::Normal },
                            counterparty: None,
                        },
                        Change {
                            user: None,
                            delta: a,
                            check: Check::None,
                            counterparty: None,
                        },
                    ],
                    issued: 0,
                    burned: a,
                    reversal_of: None,
                    hold_id: None,
                    release_held: None,
                },
                0,
            )
        }
        (Some(f), Some(t)) => {
            run_hook(ctx, "transfer", &req.currency, f, Some(t), a, &req.reason).await?;
            let fee = i64::try_from(i128::from(a) * i128::from(cur.info.fee_bps) / 10_000).unwrap_or(0);
            let total = a
                .checked_add(fee)
                .ok_or_else(|| PluginError::new("ledger.amount_out_of_range", "amount too large"))?;
            let mut changes = vec![
                Change {
                    user: Some(f),
                    delta: -total,
                    check: Check::Normal,
                    counterparty: Some(t),
                },
                Change {
                    user: Some(t),
                    delta: a,
                    check: Check::Normal,
                    counterparty: Some(f),
                },
            ];
            if fee > 0 {
                changes.push(Change {
                    user: None,
                    delta: fee,
                    check: Check::None,
                    counterparty: None,
                });
            }
            (
                Op {
                    kind: "transfer",
                    reason: req.reason.clone(),
                    memo,
                    changes,
                    issued: 0,
                    burned: fee,
                    reversal_of: None,
                    hold_id: None,
                    release_held: None,
                },
                fee,
            )
        }
        (None, None) => {
            return Err(validation(
                "/from_user",
                "from_user and to_user cannot both be empty",
            ));
        }
    };

    let mut c = ctx.sql.conn().await?;
    if let Idem::Replay(v) = idem_begin(&mut c, &caller, &req.idempotency_key, &hash).await? {
        return replayed(v);
    }
    let posted = post(&mut c, ctx, &cur, &caller, &op).await?;
    // 用户间转账的每日转出限额（§1.4 第 5 步）
    if let (Some(f), Some(_)) = (from, to)
        && cur.info.daily_out_limit > 0
    {
        let ok: Option<i64> = sqlx::query_scalar(
            "INSERT INTO daily_outflow (day, user_id, currency, amount) VALUES (current_date, $1, $2, $3)
             ON CONFLICT (day, user_id, currency) DO UPDATE SET amount = daily_outflow.amount + EXCLUDED.amount
               WHERE daily_outflow.amount + EXCLUDED.amount <= $4
             RETURNING amount",
        )
        .bind(f)
        .bind(&cur.info.code)
        .bind(a)
        .bind(cur.info.daily_out_limit)
        .fetch_optional(&mut *c)
        .await?;
        if ok.is_none() || ok.is_some_and(|x| x > cur.info.daily_out_limit) {
            return Err(PluginError::new(
                "ledger.daily_limit_exceeded",
                "daily transfer limit exceeded",
            ));
        }
    }
    let result = TransferResult {
        entry_id: posted.entry_id.to_string(),
        from_balance_after: from.and_then(|f| posted.balances.get(&f).copied()),
        to_balance_after: to.and_then(|t| posted.balances.get(&t).copied()),
        fee,
        duplicate: false,
    };
    idem_finish(
        &mut c,
        &caller,
        &req.idempotency_key,
        Some(posted.entry_id),
        &serde_json::to_value(&result)?,
    )
    .await?;
    drop(c);
    publish_posted(ctx, &cur, &caller, &op, &posted).await?;
    Ok(result)
}

// ---------------- 冲正（§1.7） ----------------

pub async fn reverse(
    ctx: &Ctx,
    entry_id: &str,
    reason: &str,
    key: &str,
    allow_negative: bool,
) -> PResult<String> {
    let caller = Caller::of(ctx);
    check_key(key)?;
    check_reason(&caller, reason)?;
    let eid = uuid::Uuid::parse_str(entry_id)
        .map_err(|_| PluginError::new("ledger.entry_not_found", "entry not found").with_status(404))?;
    if allow_negative && !caller.is_self() {
        return Err(PluginError::new(
            "ledger.negative_not_allowed",
            "only administrators may reverse into a negative balance",
        )
        .with_status(403));
    }
    let hash =
        request_hash(&json!({ "reverse": entry_id, "reason": reason, "allow_negative": allow_negative }));
    let mut c = ctx.sql.conn().await?;
    let e = sqlx::query("SELECT currency, kind, caller_plugin FROM entries WHERE id = $1")
        .bind(eid)
        .fetch_optional(&mut *c)
        .await?
        .ok_or_else(|| PluginError::new("ledger.entry_not_found", "entry not found").with_status(404))?;
    let currency: String = e.get(0);
    let kind: String = e.get(1);
    let owner: String = e.get(2);
    let allowed = caller.is_self()
        || caller.has("reverse")
        || (caller.plugin == owner && caller.has(&format!("currency:{currency}")));
    if !allowed {
        return Err(
            PluginError::new("ledger.currency_not_allowed", "not allowed to reverse this entry")
                .with_status(403),
        );
    }
    if kind == "reversal" {
        return Err(PluginError::new(
            "ledger.cannot_reverse_reversal",
            "a reversal cannot be reversed",
        ));
    }
    if let Idem::Replay(v) = idem_begin(&mut c, &caller, key, &hash).await? {
        return v["reversal_entry_id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| PluginError::internal("bad stored result"));
    }
    let first: Option<uuid::Uuid> = sqlx::query_scalar(
        "INSERT INTO entry_reversals (entry_id) VALUES ($1) ON CONFLICT DO NOTHING RETURNING entry_id",
    )
    .bind(eid)
    .fetch_optional(&mut *c)
    .await?;
    if first.is_none() {
        return Err(
            PluginError::new("ledger.already_reversed", "entry has already been reversed").with_status(409),
        );
    }
    let cur = load_currency(&mut c, &currency).await?;
    let lines = sqlx::query(
        "SELECT user_id, amount, counterparty_user_id FROM lines WHERE entry_id = $1 ORDER BY line_no",
    )
    .bind(eid)
    .fetch_all(&mut *c)
    .await?;
    let mut changes = Vec::new();
    let (mut issued, mut burned) = (0i64, 0i64);
    for l in &lines {
        let user: Option<i64> = l.get(0);
        let amount: i64 = l.get(1);
        if user.is_none() {
            // 系统分录：发行时为负（冲正后 issued 减少），回收 / 手续费为正（冲正后 burned 减少）
            if amount < 0 {
                issued -= -amount;
            } else {
                burned -= amount;
            }
        }
        changes.push(Change {
            user,
            delta: -amount,
            check: if allow_negative {
                Check::None
            } else {
                Check::IgnoreLock
            },
            counterparty: l.get(2),
        });
    }
    let op = Op {
        kind: "reversal",
        reason: reason.to_owned(),
        memo: None,
        changes,
        issued,
        burned,
        reversal_of: Some(eid),
        hold_id: None,
        release_held: None,
    };
    let posted = post(&mut c, ctx, &cur, &caller, &op).await?;
    sqlx::query("UPDATE entry_reversals SET reversal_entry_id = $2 WHERE entry_id = $1")
        .bind(eid)
        .bind(posted.entry_id)
        .execute(&mut *c)
        .await?;
    let rid = posted.entry_id.to_string();
    idem_finish(
        &mut c,
        &caller,
        key,
        Some(posted.entry_id),
        &json!({ "reversal_entry_id": rid }),
    )
    .await?;
    drop(c);
    publish_posted(ctx, &cur, &caller, &op, &posted).await?;
    for uid in posted.balances.keys() {
        ctx.events
            .publish(NewEvent::v1("ledger.entry_reversed", format!("user:{uid}"),
                json!({ "entry_id": eid, "reversal_entry_id": posted.entry_id, "user_id": uid, "reason": reason })))
            .await?;
    }
    Ok(rid)
}

// ---------------- 冻结（§1.5） ----------------

pub async fn hold(ctx: &Ctx, req: HoldRequest) -> PResult<String> {
    let caller = Caller::of(ctx);
    check_key(&req.idempotency_key)?;
    check_reason(&caller, &req.reason)?;
    check_amount(req.amount)?;
    let uid = u2i(req.user)?;
    let hash = request_hash(
        &json!({ "hold": req.currency, "user": req.user, "amount": req.amount, "reason": req.reason,
                                     "expires_at": req.expires_at }),
    );
    let cur = {
        let mut c = ctx.sql.conn().await?;
        let cur = writable_currency(&mut c, &caller, &req.currency).await?;
        if let Idem::Replay(v) = idem_peek(&mut c, &caller, &req.idempotency_key, &hash).await? {
            return v["hold_id"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| PluginError::internal("bad stored result"));
        }
        cur
    };
    let now = Utc::now().timestamp_millis();
    let max_exp = now + cur.max_hold_secs * 1000;
    let exp = req.expires_at.unwrap_or(max_exp);
    if exp > max_exp {
        return Err(PluginError::new(
            "ledger.hold_ttl_too_long",
            "hold expiry exceeds the maximum",
        ));
    }
    if exp <= now {
        return Err(validation("/expires_at", "expires_at must be in the future"));
    }
    run_hook(ctx, "hold", &req.currency, uid, None, req.amount, &req.reason).await?;
    let mut c = ctx.sql.conn().await?;
    if let Idem::Replay(v) = idem_begin(&mut c, &caller, &req.idempotency_key, &hash).await? {
        return v["hold_id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| PluginError::internal("bad stored result"));
    }
    let ok: Option<i64> = sqlx::query_scalar(
        "UPDATE accounts SET held = held + $3, version = version + 1, updated_at = now()
          WHERE user_id = $1 AND currency = $2 AND lock_count = 0 AND balance - held - $3 >= $4 RETURNING held",
    )
    .bind(uid)
    .bind(&cur.info.code)
    .bind(req.amount)
    .bind(cur.min_balance)
    .fetch_optional(&mut *c)
    .await?;
    if ok.is_none() {
        return Err(diagnose(&mut c, uid, &cur, req.amount).await?);
    }
    let id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO holds (id, currency, user_id, amount, remaining, state, reason, caller_plugin, expires_at)
         VALUES ($1,$2,$3,$4,$4,'active',$5,$6,$7)",
    )
    .bind(id)
    .bind(&cur.info.code)
    .bind(uid)
    .bind(req.amount)
    .bind(&req.reason)
    .bind(&caller.plugin)
    .bind(DateTime::<Utc>::from_timestamp_millis(exp))
    .execute(&mut *c)
    .await?;
    idem_finish(
        &mut c,
        &caller,
        &req.idempotency_key,
        None,
        &json!({ "hold_id": id }),
    )
    .await?;
    drop(c);
    // 到期自动释放（内核延迟任务，按 hold 去重）
    ctx.jobs
        .schedule_once(
            "expire_hold",
            exp,
            json!({ "hold_id": id }),
            Some(&id.to_string()),
        )
        .await?;
    Ok(id.to_string())
}

struct HoldRow {
    id: uuid::Uuid,
    currency: String,
    user_id: i64,
    remaining: i64,
    state: String,
    caller_plugin: String,
}

async fn lock_hold(c: &mut PgConnection, caller: &Caller, hold_id: &str) -> PResult<HoldRow> {
    let not_found = || PluginError::new("ledger.hold_not_found", "hold not found").with_status(404);
    let id = uuid::Uuid::parse_str(hold_id).map_err(|_| not_found())?;
    let r = sqlx::query(
        "SELECT id, currency, user_id, remaining, state, caller_plugin FROM holds WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *c)
    .await?
    .ok_or_else(not_found)?;
    let h = HoldRow {
        id: r.get(0),
        currency: r.get(1),
        user_id: r.get(2),
        remaining: r.get(3),
        state: r.get(4),
        caller_plugin: r.get(5),
    };
    if h.caller_plugin != caller.plugin && !caller.is_self() && !caller.has("reverse") {
        return Err(not_found());
    }
    Ok(h)
}

pub async fn release_hold(ctx: &Ctx, hold_id: &str, key: &str, final_state: &str) -> PResult<()> {
    let caller = Caller::of(ctx);
    check_key(key)?;
    let hash = request_hash(&json!({ "release": hold_id }));
    let mut c = ctx.sql.conn().await?;
    if let Idem::Replay(_) = idem_begin(&mut c, &caller, key, &hash).await? {
        return Ok(());
    }
    let h = lock_hold(&mut c, &caller, hold_id).await?;
    if h.state != "active" {
        if final_state == "expired" {
            // 到期任务遇到已结束的 hold：无事可做
            idem_finish(&mut c, &caller, key, None, &json!({})).await?;
            return Ok(());
        }
        return Err(PluginError::new("ledger.hold_not_active", "hold is no longer active").with_status(409));
    }
    sqlx::query("UPDATE accounts SET held = held - $3, version = version + 1, updated_at = now() WHERE user_id = $1 AND currency = $2")
        .bind(h.user_id)
        .bind(&h.currency)
        .bind(h.remaining)
        .execute(&mut *c)
        .await?;
    sqlx::query("UPDATE holds SET state = $2, remaining = 0, closed_at = now() WHERE id = $1")
        .bind(h.id)
        .bind(final_state)
        .execute(&mut *c)
        .await?;
    idem_finish(&mut c, &caller, key, None, &json!({})).await?;
    Ok(())
}

pub async fn capture_hold(ctx: &Ctx, req: CaptureRequest) -> PResult<TransferResult> {
    let caller = Caller::of(ctx);
    check_key(&req.idempotency_key)?;
    check_reason(&caller, &req.reason)?;
    check_amount(req.amount)?;
    let memo = clean_memo(&req.memo)?;
    let to = req.to_user.map(u2i).transpose()?;
    let mut hashed = serde_json::to_value(&req)?;
    if let Some(o) = hashed.as_object_mut() {
        o.remove("idempotency_key");
    }
    let hash = request_hash(&hashed);
    let mut c = ctx.sql.conn().await?;
    if let Idem::Replay(v) = idem_begin(&mut c, &caller, &req.idempotency_key, &hash).await? {
        return replayed(v);
    }
    let h = lock_hold(&mut c, &caller, &req.hold_id).await?;
    if h.state != "active" {
        return Err(PluginError::new("ledger.hold_not_active", "hold is no longer active").with_status(409));
    }
    if req.amount > h.remaining {
        return Err(PluginError::new(
            "ledger.hold_amount_exceeded",
            "capture amount exceeds the hold",
        ));
    }
    if to == Some(h.user_id) {
        return Err(PluginError::new(
            "ledger.self_transfer",
            "cannot capture to the same user",
        ));
    }
    let cur = writable_currency(&mut c, &caller, &h.currency).await?;
    let mut changes = vec![Change {
        user: Some(h.user_id),
        delta: -req.amount,
        check: Check::IgnoreLock,
        counterparty: to,
    }];
    let burned = match to {
        Some(t) => {
            changes.push(Change {
                user: Some(t),
                delta: req.amount,
                check: Check::Normal,
                counterparty: Some(h.user_id),
            });
            0
        }
        None => {
            changes.push(Change {
                user: None,
                delta: req.amount,
                check: Check::None,
                counterparty: None,
            });
            req.amount
        }
    };
    let op = Op {
        kind: "hold_capture",
        reason: req.reason.clone(),
        memo,
        changes,
        issued: 0,
        burned,
        reversal_of: None,
        hold_id: Some(h.id),
        release_held: Some((h.user_id, h.remaining)),
    };
    let posted = post(&mut c, ctx, &cur, &caller, &op).await?;
    sqlx::query("UPDATE holds SET state = 'captured', captured_amount = $2, remaining = 0, closed_at = now() WHERE id = $1")
        .bind(h.id)
        .bind(req.amount)
        .execute(&mut *c)
        .await?;
    let result = TransferResult {
        entry_id: posted.entry_id.to_string(),
        from_balance_after: posted.balances.get(&h.user_id).copied(),
        to_balance_after: to.and_then(|t| posted.balances.get(&t).copied()),
        fee: 0,
        duplicate: false,
    };
    idem_finish(
        &mut c,
        &caller,
        &req.idempotency_key,
        Some(posted.entry_id),
        &serde_json::to_value(&result)?,
    )
    .await?;
    drop(c);
    publish_posted(ctx, &cur, &caller, &op, &posted).await?;
    Ok(result)
}

// ---------------- 锁定（§1.6） ----------------

pub async fn lock_account(ctx: &Ctx, req: LockRequest) -> PResult<String> {
    let caller = Caller::of(ctx);
    if !caller.has("lock") {
        return Err(PluginError::new("ledger.scope_required", "scope `lock` required").with_status(403));
    }
    check_key(&req.idempotency_key)?;
    let uid = u2i(req.user)?;
    let hash = request_hash(&serde_json::to_value(&req)?);
    let mut c = ctx.sql.conn().await?;
    if let Idem::Replay(v) = idem_begin(&mut c, &caller, &req.idempotency_key, &hash).await? {
        return v["lock_id"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| PluginError::internal("bad stored result"));
    }
    if let Some(cur) = &req.currency {
        load_currency(&mut c, cur).await?;
        sqlx::query("INSERT INTO accounts (user_id, currency) VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(uid)
            .bind(cur)
            .execute(&mut *c)
            .await?;
    }
    let id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO account_locks (id, user_id, currency, reason, ref, caller_plugin, created_by) VALUES ($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(id)
    .bind(uid)
    .bind(&req.currency)
    .bind(req.reason.chars().take(64).collect::<String>())
    .bind(req.reference.chars().take(128).collect::<String>())
    .bind(&caller.plugin)
    .bind(ctx.user_id().and_then(|u| i64::try_from(u).ok()))
    .execute(&mut *c)
    .await?;
    sqlx::query("UPDATE accounts SET lock_count = lock_count + 1, updated_at = now() WHERE user_id = $1 AND ($2::text IS NULL OR currency = $2)")
        .bind(uid)
        .bind(&req.currency)
        .execute(&mut *c)
        .await?;
    idem_finish(
        &mut c,
        &caller,
        &req.idempotency_key,
        None,
        &json!({ "lock_id": id }),
    )
    .await?;
    drop(c);
    ctx.audit
        .write(pt_plugin_api::AuditEntry {
            action: "ledger.lock".into(),
            target: format!("user:{uid}"),
            details: json!({ "lock_id": id, "currency": req.currency, "reason": req.reason, "ref": req.reference }),
        })
        .await?;
    Ok(id.to_string())
}

pub async fn unlock_account(ctx: &Ctx, lock_id: &str, key: &str) -> PResult<()> {
    let caller = Caller::of(ctx);
    check_key(key)?;
    let not_found = || PluginError::new("ledger.lock_not_found", "lock not found").with_status(404);
    let id = uuid::Uuid::parse_str(lock_id).map_err(|_| not_found())?;
    let hash = request_hash(&json!({ "unlock": lock_id }));
    let mut c = ctx.sql.conn().await?;
    if let Idem::Replay(_) = idem_begin(&mut c, &caller, key, &hash).await? {
        return Ok(());
    }
    let r = sqlx::query(
        "SELECT user_id, currency, caller_plugin, released_at FROM account_locks WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *c)
    .await?
    .ok_or_else(not_found)?;
    let (uid, cur, owner, released): (i64, Option<String>, String, Option<DateTime<Utc>>) =
        (r.get(0), r.get(1), r.get(2), r.get(3));
    if owner != caller.plugin && !caller.is_self() {
        return Err(not_found());
    }
    if released.is_none() {
        sqlx::query("UPDATE account_locks SET released_at = now(), released_by = $2 WHERE id = $1")
            .bind(id)
            .bind(ctx.user_id().and_then(|u| i64::try_from(u).ok()))
            .execute(&mut *c)
            .await?;
        sqlx::query(
            "UPDATE accounts SET lock_count = lock_count - 1, updated_at = now()
              WHERE user_id = $1 AND ($2::text IS NULL OR currency = $2) AND lock_count > 0",
        )
        .bind(uid)
        .bind(&cur)
        .execute(&mut *c)
        .await?;
    }
    idem_finish(&mut c, &caller, key, None, &json!({})).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_rules() {
        let c = Caller {
            plugin: "bonus".into(),
            scopes: vec!["currency:bonus".into()],
        };
        assert!(check_reason(&c, "bonus.hourly").is_ok());
        assert!(check_reason(&c, "shop.hourly").is_err());
        assert!(check_reason(&c, "bonus.Hourly").is_err());
        assert!(check_reason(&c, "bonus.").is_err());
        assert!(check_amount(0).is_err() && check_amount(MAX_AMOUNT + 1).is_err() && check_amount(1).is_ok());
        assert!(check_key("abc:1").is_ok() && check_key("a b").is_err() && check_key("").is_err());
        assert!(c.require_currency("bonus").is_ok() && c.require_currency("gold").is_err());
        assert_eq!(clean_memo(" hi\u{7} ").ok(), Some(Some("hi".into())));
        assert_eq!(
            request_hash(&json!({"b": 1, "a": 2})),
            request_hash(&json!({"a": 2, "b": 1}))
        );
    }
}
