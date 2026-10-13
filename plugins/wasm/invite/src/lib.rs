//! invite 插件（WASM，docs/plugins/invite/）。
//!
//! 本版本实现：码派生与只存哈希、个人 / 系统 / 活动码签发（资格检查、`invite.before_issue`、FIFO 消耗额度、`request_id` 重放）、
//! 注册钩子预留 + `user.registered` 确认（超额兑换标记、邀请树边与闭包表）、额度批次与流水（初始、晋升、周期、管理员、服务发放与扣减、
//! 到期落账）、撤销 / 过期（按配置返还原批次）、预留释放、邀请树查询、发邀权暂停、连坐规则（封禁触发、扣额度 / 暂停 / 撤销未用 /
//! 限制邀请人 / 复核，解封撤销）、`ctx.invites`、HTTP。批量发放、整树操作、日统计与问题邀请人评分、对账、MCP、通知在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

pt_plugin_sdk::world!("invite");

pub mod codes;

use std::collections::BTreeMap;

use exports::pt::plugin::{hooks, http_handler, lifecycle, on_cron, on_event, service_provider};
use pt_plugin_sdk::{
    audit, config, events, hook_runner, invocation, kv, random, services,
    sql::{self, SqlValue},
    time,
    types::PluginError,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::codes::Keys;

struct Invite;

type R<T> = Result<T, PluginError>;
type Row = Vec<SqlValue>;

fn err(code: &str, message: &str) -> PluginError {
    PluginError {
        code: code.into(),
        message: message.into(),
        details: None,
        retryable: false,
    }
}

fn err_d(code: &str, message: &str, details: Value) -> PluginError {
    PluginError {
        code: code.into(),
        message: message.into(),
        details: Some(details.to_string()),
        retryable: false,
    }
}

fn invalid(message: &str) -> PluginError {
    err("kernel.validation_failed", message)
}

fn q(stmt: &str, params: &[SqlValue]) -> R<Vec<Row>> {
    Ok(sql::query(stmt, params)?.rows)
}

fn x(stmt: &str, params: &[SqlValue]) -> R<u64> {
    sql::execute(stmt, params)
}

fn int(v: &SqlValue) -> i64 {
    match v {
        SqlValue::Int(i) => *i,
        SqlValue::Numeric(s) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

fn oint(v: &SqlValue) -> Option<i64> {
    match v {
        SqlValue::Int(i) => Some(*i),
        _ => None,
    }
}

fn text(v: &SqlValue) -> String {
    match v {
        SqlValue::Text(s) => s.clone(),
        _ => String::new(),
    }
}

fn otext(v: &SqlValue) -> Option<String> {
    match v {
        SqlValue::Text(s) => Some(s.clone()),
        _ => None,
    }
}

fn boolean(v: &SqlValue) -> bool {
    matches!(v, SqlValue::Boolean(true))
}

fn oi(v: Option<i64>) -> SqlValue {
    v.map_or(SqlValue::Null, SqlValue::Int)
}

fn ot(v: Option<&str>) -> SqlValue {
    v.map_or(SqlValue::Null, |s| SqlValue::Text(s.into()))
}

fn t(s: &str) -> SqlValue {
    SqlValue::Text(s.into())
}

/// SQL：时间列 → Unix 毫秒
fn ms(col: &str) -> String {
    format!("(extract(epoch FROM {col}) * 1000)::bigint")
}

/// Unix 毫秒 → RFC 3339（UTC，毫秒精度）
fn iso(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let (days, sod) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // civil_from_days（H. Hinnant）
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        sod / 3600,
        sod % 3600 / 60,
        sod % 60,
        ms.rem_euclid(1000)
    )
}

fn oiso(v: Option<i64>) -> Value {
    v.map_or(Value::Null, |m| json!(iso(m)))
}

fn publish(event_type: &str, key: String, payload: Value) -> R<()> {
    events::publish(&events::NewEvent {
        event_type: event_type.into(),
        version: 1,
        key,
        payload: payload.to_string(),
    })?;
    Ok(())
}

fn svc(service: &str, method: &str, args: &Value) -> R<Value> {
    let out = services::call(service, method, &args.to_string())?;
    serde_json::from_str(&out).map_err(|_| err("invite.bad_service_reply", "invalid JSON from service"))
}

fn has_perm(uid: i64, perm: &str) -> R<bool> {
    Ok(svc("ctx.rbac", "check", &json!({ "user": uid, "permission": perm }))?.as_bool() == Some(true))
}

fn write_audit(action: &str, target: String, details: Value) -> R<()> {
    audit::write(&audit::AuditEntry {
        action: action.into(),
        target,
        details: details.to_string(),
    })
}

// ---------------- 设置 ----------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct ClassRule {
    max_outstanding: i64,
    require_email_binding: bool,
    can_bypass_min_age: bool,
}

impl Default for ClassRule {
    fn default() -> Self {
        Self {
            max_outstanding: 1,
            require_email_binding: false,
            can_bypass_min_age: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct GrantRule {
    amount: i64,
    #[serde(default)]
    expires_days: Option<i64>,
    #[serde(default = "d_max_balance")]
    max_balance: i64,
}

fn d_max_balance() -> i64 {
    1000
}

#[derive(Debug, Clone, Deserialize)]
struct ClassGrant {
    class_id: i64,
    #[serde(default)]
    on_promotion: Option<GrantRule>,
    #[serde(default)]
    periodic: Option<GrantRule>,
    #[serde(default)]
    repeat_on_repromotion: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct Initial {
    #[serde(default)]
    amount: i64,
    #[serde(default)]
    expires_days: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
struct Level {
    depth: i64,
    actions: Vec<Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct PenaltyRule {
    id: String,
    #[serde(default = "yes")]
    enabled: bool,
    #[serde(default)]
    group: Option<String>,
    trigger: String,
    #[serde(default = "d_within")]
    within_days: i64,
    #[serde(default = "one")]
    max_depth: i64,
    levels: Vec<Level>,
    #[serde(default = "yes")]
    reverse_on_lift: bool,
}

fn yes() -> bool {
    true
}
fn one() -> i64 {
    1
}
fn d_within() -> i64 {
    30
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct Settings {
    enabled: bool,
    redeem_when_disabled: bool,
    redeem_if_inviter_unavailable: bool,
    default_expiry_days: i64,
    max_expiry_days: i64,
    system_expiry_days: i64,
    reservation_ttl_secs: i64,
    min_account_days: i64,
    daily_limit: i64,
    class_rules: BTreeMap<String, ClassRule>,
    refund_on_revoke: bool,
    refund_on_expire: bool,
    class_grants: Vec<ClassGrant>,
    periodic_schedule: String,
    initial_quota: Initial,
    penalty_rules: Vec<PenaltyRule>,
    dedupe_window_days: i64,
    show_inviter_on_check: bool,
    campaign_inviter: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            redeem_when_disabled: true,
            redeem_if_inviter_unavailable: false,
            default_expiry_days: 7,
            max_expiry_days: 14,
            system_expiry_days: 3,
            reservation_ttl_secs: 900,
            min_account_days: 30,
            daily_limit: 5,
            class_rules: BTreeMap::new(),
            refund_on_revoke: true,
            refund_on_expire: false,
            class_grants: vec![],
            periodic_schedule: "monthly".into(),
            initial_quota: Initial::default(),
            penalty_rules: vec![],
            dedupe_window_days: 30,
            show_inviter_on_check: true,
            campaign_inviter: "none".into(),
        }
    }
}

impl Settings {
    fn class_rule(&self, class: i64) -> ClassRule {
        self.class_rules
            .get(&class.to_string())
            .or_else(|| self.class_rules.get("*"))
            .cloned()
            .unwrap_or_default()
    }
}

fn settings() -> Settings {
    serde_json::from_str(&config::get_settings()).unwrap_or_default()
}

fn static_cfg() -> Value {
    serde_json::from_str(&config::get_static()).unwrap_or(Value::Null)
}

fn keys() -> R<Keys> {
    let secret = static_cfg()["secret"]
        .as_str()
        .and_then(codes::base64_decode)
        .filter(|k| k.len() == 32)
        .ok_or_else(|| {
            err(
                "kernel.internal",
                "site.toml [plugin.invite] secret must be 32 bytes of base64",
            )
        })?;
    Ok(Keys::new(&secret))
}

fn register_path() -> String {
    static_cfg()["register_path"]
        .as_str()
        .unwrap_or("/p/identity/register?invite=")
        .to_owned()
}

// ---------------- 用户与账户 ----------------

struct UserInfo {
    status: String,
    class_id: i64,
    created_ms: i64,
}

fn user_info(uid: i64) -> R<Option<UserInfo>> {
    let r = q(
        &format!(
            "SELECT status, class_id, {} FROM identity.v_user_public WHERE id = $1",
            ms("created_at")
        ),
        &[SqlValue::Int(uid)],
    )?;
    Ok(r.first().map(|r| UserInfo {
        status: text(&r[0]),
        class_id: int(&r[1]),
        created_ms: int(&r[2]),
    }))
}

/// 同一用户的额度 / 发邀操作串行化；返回已赠送过晋升额度的等级
fn lock_account(uid: i64) -> R<Vec<i64>> {
    x(
        "INSERT INTO accounts (user_id) VALUES ($1) ON CONFLICT (user_id) DO NOTHING",
        &[SqlValue::Int(uid)],
    )?;
    let r = q(
        "SELECT promo_classes FROM accounts WHERE user_id = $1 FOR UPDATE",
        &[SqlValue::Int(uid)],
    )?;
    Ok(match r.first().map(|r| &r[0]) {
        Some(SqlValue::IntArray(a)) => a.clone(),
        _ => vec![],
    })
}

fn me() -> R<i64> {
    invocation::current()
        .actor
        .user_id
        .and_then(|u| i64::try_from(u).ok())
        .ok_or_else(|| err("kernel.unauthenticated", "login required"))
}

// ---------------- 额度（§1.5） ----------------

const AVAIL: &str = "remaining > 0 AND (expires_at IS NULL OR expires_at > now())";

fn balance(uid: i64) -> R<(i64, i64, Option<i64>)> {
    let r = q(
        &format!(
            "SELECT COALESCE(sum(remaining), 0), COALESCE(sum(remaining) FILTER (WHERE expires_at IS NULL), 0),
                    {} FROM quota_lots WHERE user_id = $1 AND {AVAIL}",
            ms("min(expires_at) FILTER (WHERE expires_at IS NOT NULL)")
        ),
        &[SqlValue::Int(uid)],
    )?;
    let row = &r[0];
    Ok((int(&row[0]), int(&row[1]), oint(&row[2])))
}

#[allow(clippy::too_many_arguments)]
fn ledger(
    uid: i64,
    lot: Option<i64>,
    delta: i64,
    reason: &str,
    rf: &str,
    actor: Option<i64>,
    note: Option<&str>,
) -> R<i64> {
    let (bal, _, _) = balance(uid)?;
    x(
        "INSERT INTO quota_ledger (user_id, lot_id, delta, reason, ref, balance_after, actor_id, note) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        &[
            SqlValue::Int(uid),
            oi(lot),
            SqlValue::Int(delta),
            t(reason),
            t(rf),
            SqlValue::Int(bal),
            oi(actor),
            ot(note),
        ],
    )?;
    let exp = match lot {
        Some(l) => q(
            &format!("SELECT {} FROM quota_lots WHERE id = $1", ms("expires_at")),
            &[SqlValue::Int(l)],
        )?
        .first()
        .and_then(|r| oint(&r[0])),
        None => None,
    };
    publish(
        "invite.quota_changed",
        format!("user:{uid}"),
        json!({ "user_id": uid, "delta": delta, "balance": bal, "reason": reason, "lot_id": lot, "expires_at": exp }),
    )?;
    Ok(bal)
}

/// FIFO 消耗一个（调用前已 lock_account）
fn consume_one(uid: i64, rf: &str) -> R<i64> {
    let r = q(
        &format!(
            "SELECT id FROM quota_lots WHERE user_id = $1 AND {AVAIL} ORDER BY expires_at ASC NULLS LAST, id ASC LIMIT 1 FOR UPDATE"
        ),
        &[SqlValue::Int(uid)],
    )?;
    let lot = r
        .first()
        .map(|r| int(&r[0]))
        .ok_or_else(|| err("invite.quota_exhausted", "no invite quota left"))?;
    x(
        "UPDATE quota_lots SET remaining = remaining - 1, updated_at = now() WHERE id = $1",
        &[SqlValue::Int(lot)],
    )?;
    ledger(uid, Some(lot), -1, "issue", rf, None, None)?;
    Ok(lot)
}

struct Grant<'a> {
    uid: i64,
    amount: i64,
    source: &'a str,
    source_ref: String,
    expires_ms: Option<i64>,
    reason: &'a str,
    actor: Option<i64>,
    note: Option<&'a str>,
    max_balance: Option<i64>,
}

/// 发放；自然键 (user, source, source_ref) 去重。返回 (批次, 实发, 余额)
fn grant(g: &Grant<'_>) -> R<(Option<i64>, i64, i64)> {
    if !(1..=100_000).contains(&g.amount) {
        return Err(invalid("amount must be 1..=100000"));
    }
    if g.expires_ms.is_some_and(|e| e <= time::now()) {
        return Err(invalid("expires_at must be in the future"));
    }
    lock_account(g.uid)?;
    let mut amount = g.amount;
    if let Some(max) = g.max_balance {
        amount = amount.min(max - balance(g.uid)?.0);
        if amount <= 0 {
            return Ok((None, 0, balance(g.uid)?.0));
        }
    }
    let r = q(
        "INSERT INTO quota_lots (user_id, source, source_ref, amount, remaining, expires_at)
         VALUES ($1, $2, $3, $4, $4, CASE WHEN $5::bigint IS NULL THEN NULL ELSE to_timestamp($5::float8 / 1000) END)
         ON CONFLICT (user_id, source, source_ref) DO NOTHING RETURNING id",
        &[SqlValue::Int(g.uid), t(g.source), t(&g.source_ref), SqlValue::Int(amount), oi(g.expires_ms)],
    )?;
    let Some(lot) = r.first().map(|r| int(&r[0])) else {
        let e = q(
            "SELECT id, amount FROM quota_lots WHERE user_id = $1 AND source = $2 AND source_ref = $3",
            &[SqlValue::Int(g.uid), t(g.source), t(&g.source_ref)],
        )?;
        return Ok((e.first().map(|r| int(&r[0])), 0, balance(g.uid)?.0));
    };
    let bal = ledger(g.uid, Some(lot), amount, g.reason, &g.source_ref, g.actor, g.note)?;
    Ok((Some(lot), amount, bal))
}

/// 扣减（先扣 `first_ref` 批次，再 FIFO），不产生负余额。返回 (实扣, 余额)
fn deduct(
    uid: i64,
    amount: i64,
    reason: &str,
    first_ref: Option<&str>,
    rf: &str,
    actor: Option<i64>,
) -> R<(i64, i64)> {
    if !(1..=100_000).contains(&amount) {
        return Err(invalid("amount must be 1..=100000"));
    }
    lock_account(uid)?;
    let lots = q(
        &format!(
            "SELECT id, remaining FROM quota_lots WHERE user_id = $1 AND {AVAIL}
              ORDER BY (source_ref = $2) DESC, expires_at ASC NULLS LAST, id ASC FOR UPDATE"
        ),
        &[SqlValue::Int(uid), ot(first_ref)],
    )?;
    let mut left = amount;
    for l in lots {
        if left == 0 {
            break;
        }
        let d = int(&l[1]).min(left);
        x(
            "UPDATE quota_lots SET remaining = remaining - $2, updated_at = now() WHERE id = $1",
            &[SqlValue::Int(int(&l[0])), SqlValue::Int(d)],
        )?;
        ledger(uid, Some(int(&l[0])), -d, reason, rf, actor, None)?;
        left -= d;
    }
    Ok((amount - left, balance(uid)?.0))
}

fn quota_json(uid: i64, s: &Settings) -> R<Value> {
    let (bal, perm, next) = balance(uid)?;
    let lots = q(
        &format!(
            "SELECT id, source, amount, remaining, {} FROM quota_lots WHERE user_id = $1 AND {AVAIL}
              ORDER BY expires_at ASC NULLS LAST, id ASC LIMIT 100",
            ms("expires_at")
        ),
        &[SqlValue::Int(uid)],
    )?;
    let class = user_info(uid)?.map_or(0, |u| u.class_id);
    let (outstanding, today) = outstanding_today(uid)?;
    Ok(json!({
        "balance": bal, "permanent": perm, "temporary": bal - perm, "next_expiry": oiso(next),
        "lots": lots.iter().map(|r| json!({ "lot_id": int(&r[0]), "source": text(&r[1]), "amount": int(&r[2]),
                                             "remaining": int(&r[3]), "expires_at": oiso(oint(&r[4])) })).collect::<Vec<_>>(),
        "max_outstanding": s.class_rule(class).max_outstanding, "outstanding": outstanding,
        "daily_limit": s.daily_limit, "issued_today": today,
    }))
}

fn outstanding_today(uid: i64) -> R<(i64, i64)> {
    let r = q(
        "SELECT count(*) FILTER (WHERE status = 'active'),
                count(*) FILTER (WHERE created_at >= (date_trunc('day', now() AT TIME ZONE $2) AT TIME ZONE $2))
           FROM invites WHERE inviter_id = $1 AND kind = 'personal'",
        &[SqlValue::Int(uid), t(&time::site_timezone())],
    )?;
    Ok((int(&r[0][0]), int(&r[0][1])))
}

// ---------------- 发邀（§1.2） ----------------

struct Check {
    name: &'static str,
    code: &'static str,
    ok: bool,
    details: Value,
}

/// 资格检查 1～7（不触发钩子）；返回逐项结果、等级与未用数
fn eligibility(uid: i64, s: &Settings, bypass: bool) -> R<(Vec<Check>, i64, i64)> {
    let u = user_info(uid)?.ok_or_else(|| err("invite.user_not_found", "user not found"))?;
    let rule = s.class_rule(u.class_id);
    let available_at = u.created_ms + s.min_account_days * 86_400_000;
    let susp = q(
        &format!(
            "SELECT {} FROM suspensions WHERE user_id = $1 AND lifted_at IS NULL AND (until IS NULL OR until > now()) ORDER BY until DESC NULLS FIRST LIMIT 1",
            ms("until")
        ),
        &[SqlValue::Int(uid)],
    )?;
    let (outstanding, today) = outstanding_today(uid)?;
    let (bal, _, _) = balance(uid)?;
    let checks = vec![
        Check {
            name: "enabled",
            code: "invite.disabled",
            ok: s.enabled,
            details: Value::Null,
        },
        Check {
            name: "account_active",
            code: "invite.account_inactive",
            ok: u.status == "active",
            details: Value::Null,
        },
        Check {
            name: "account_age",
            code: "invite.account_too_new",
            ok: rule.can_bypass_min_age || available_at <= time::now(),
            details: json!({ "available_at": iso(available_at) }),
        },
        Check {
            name: "not_suspended",
            code: "invite.privilege_suspended",
            ok: susp.is_empty(),
            details: json!({ "until": susp.first().map_or(Value::Null, |r| oiso(oint(&r[0]))) }),
        },
        Check {
            name: "outstanding",
            code: "invite.outstanding_limit_reached",
            ok: bypass || outstanding < rule.max_outstanding,
            details: json!({ "outstanding": outstanding, "max": rule.max_outstanding }),
        },
        Check {
            name: "daily_limit",
            code: "invite.daily_limit_reached",
            ok: bypass || s.daily_limit == 0 || today < s.daily_limit,
            details: json!({ "issued_today": today, "limit": s.daily_limit }),
        },
        Check {
            name: "quota",
            code: "invite.quota_exhausted",
            ok: bypass || bal >= 1,
            details: json!({ "balance": bal }),
        },
    ];
    Ok((checks, u.class_id, outstanding))
}

#[derive(Debug, Default, Deserialize)]
struct IssueBody {
    email: Option<String>,
    username: Option<String>,
    expires_days: Option<i64>,
    note: Option<String>,
    request_id: Option<String>,
    // 系统 / 活动码
    #[serde(default)]
    kind: Option<String>,
    inviter_id: Option<i64>,
    max_uses: Option<i64>,
}

struct Binding {
    email_hmac: Option<[u8; 32]>,
    domain: Option<String>,
    username: Option<String>,
}

fn bind(k: &Keys, email: Option<&str>, username: Option<&str>) -> R<Binding> {
    let email = email.map(str::trim).filter(|e| !e.is_empty());
    let domain = match email {
        Some(e) => Some(codes::email_domain(e).ok_or_else(|| {
            err_d(
                "kernel.validation_failed",
                "invalid email",
                json!({ "errors": [{ "path": "/email" }] }),
            )
        })?),
        None => None,
    };
    let username = username.map(codes::holder).filter(|u| !u.is_empty());
    if username.as_ref().is_some_and(|u| u.chars().count() > 64) {
        return Err(invalid("username binding is too long"));
    }
    Ok(Binding {
        email_hmac: email.map(|e| k.email_hmac(e)),
        domain,
        username,
    })
}

const INV_COLS: &str = "id, kind, source, status, code_hint, inviter_id, max_uses, use_count, (extract(epoch FROM expires_at) * 1000)::bigint,
    email_hmac IS NOT NULL, email_domain, username_binding IS NOT NULL, note, (extract(epoch FROM created_at) * 1000)::bigint, refunded";

fn invite_json(r: &Row) -> Value {
    json!({
        "id": int(&r[0]), "kind": text(&r[1]), "source": text(&r[2]), "status": text(&r[3]), "code_hint": text(&r[4]),
        "inviter_id": oint(&r[5]), "max_uses": int(&r[6]), "use_count": int(&r[7]), "expires_at": iso(int(&r[8])),
        "email_bound": boolean(&r[9]), "email_domain": otext(&r[10]), "username_bound": boolean(&r[11]),
        "note": otext(&r[12]), "created_at": iso(int(&r[13])), "refunded": boolean(&r[14]),
    })
}

fn issued(inv: Value, code: &str) -> Value {
    json!({ "invite": inv, "code": code, "url": format!("{}{code}", register_path()) })
}

struct NewInvite<'a> {
    kind: &'a str,
    source: &'a str,
    scope: String,
    key: String,
    inviter: Option<i64>,
    issued_by: Option<i64>,
    issued_by_plugin: Option<String>,
    max_uses: i64,
    days: i64,
    binding: Binding,
    note: Option<String>,
    lot: Option<i64>,
}

fn insert_invite(k: &Keys, n: NewInvite<'_>) -> R<Value> {
    let code = k.derive_code(&n.scope, &n.key);
    let r = q(
        &format!(
            "INSERT INTO invites (kind, source, code_hash, code_hint, issue_key_hash, inviter_id, issued_by, issued_by_plugin, max_uses,
                                  expires_at, email_hmac, email_domain, username_binding, note, lot_id)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now() + make_interval(days => $10::int), $11, $12, $13, $14, $15)
             RETURNING {INV_COLS}"
        ),
        &[
            t(n.kind),
            t(n.source),
            SqlValue::Bytes(codes::code_hash(&code).to_vec()),
            t(&codes::code_hint(&code)),
            SqlValue::Bytes(codes::sha256(format!("{}:{}", n.scope, n.key).as_bytes()).to_vec()),
            oi(n.inviter),
            oi(n.issued_by),
            ot(n.issued_by_plugin.as_deref()),
            SqlValue::Int(n.max_uses),
            SqlValue::Int(n.days),
            n.binding.email_hmac.map_or(SqlValue::Null, |h| SqlValue::Bytes(h.to_vec())),
            ot(n.binding.domain.as_deref()),
            ot(n.binding.username.as_deref()),
            ot(n.note.as_deref()),
            oi(n.lot),
        ],
    )?;
    let inv = invite_json(&r[0]);
    let key = n
        .inviter
        .map_or_else(|| "global".to_owned(), |u| format!("user:{u}"));
    publish(
        "invite.issued",
        key,
        json!({ "invite_id": inv["id"], "inviter_id": n.inviter, "expires_at": inv["expires_at"], "kind": n.kind, "source": n.source,
                "max_uses": n.max_uses, "email_bound": inv["email_bound"], "username_bound": inv["username_bound"],
                "code_hint": inv["code_hint"] }),
    )?;
    if let Some(lot) = n.lot {
        x(
            "UPDATE quota_ledger SET ref = $2 WHERE id = (SELECT max(id) FROM quota_ledger WHERE lot_id = $1 AND reason = 'issue')",
            &[SqlValue::Int(lot), t(&format!("invite:{}", inv["id"]))],
        )?;
    }
    Ok(issued(inv, &code))
}

/// 重放：同一签发键 → 同一结果
fn replay(k: &Keys, scope: &str, key: &str) -> R<Option<Value>> {
    let r = q(
        &format!("SELECT {INV_COLS} FROM invites WHERE issue_key_hash = $1"),
        &[SqlValue::Bytes(
            codes::sha256(format!("{scope}:{key}").as_bytes()).to_vec(),
        )],
    )?;
    Ok(r.first()
        .map(|row| issued(invite_json(row), &k.derive_code(scope, key))))
}

fn clean_note(note: Option<String>) -> R<Option<String>> {
    let note = note
        .map(|n| n.trim().chars().filter(|c| !c.is_control()).collect::<String>())
        .filter(|n| !n.is_empty());
    if note.as_ref().is_some_and(|n| n.chars().count() > 200) {
        return Err(invalid("note must be at most 200 characters"));
    }
    Ok(note)
}

/// 个人发邀；返回 (结果, 是否重放)
fn issue_personal(uid: i64, b: IssueBody) -> R<(Value, bool)> {
    let s = settings();
    let k = keys()?;
    let scope = format!("user:{uid}");
    let key = b
        .request_id
        .clone()
        .filter(|r| (1..=64).contains(&r.len()))
        .unwrap_or_else(random::uuid_v7);
    lock_account(uid)?;
    if let Some(v) = replay(&k, &scope, &key)? {
        return Ok((v, true));
    }
    let bypass = has_perm(uid, "invite.quota.bypass")?;
    let (checks, class_id, outstanding) = eligibility(uid, &s, bypass)?;
    if let Some(c) = checks.iter().find(|c| !c.ok) {
        return Err(err_d(c.code, c.name, c.details.clone()));
    }
    let binding = bind(&k, b.email.as_deref(), b.username.as_deref())?;
    if s.class_rule(class_id).require_email_binding && binding.email_hmac.is_none() {
        return Err(err_d(
            "kernel.validation_failed",
            "an email binding is required",
            json!({ "errors": [{ "path": "/email" }] }),
        ));
    }
    let days = b
        .expires_days
        .unwrap_or(s.default_expiry_days)
        .clamp(1, s.max_expiry_days.max(1));
    let note = clean_note(b.note)?;
    let input = json!({
        "version": 1, "actor": { "kind": "user", "user_id": uid }, "inviter_id": uid, "kind": "personal", "source": "user",
        "class_id": class_id, "outstanding": outstanding, "email_domain": binding.domain, "email_bound": binding.email_hmac.is_some(),
    });
    if let hook_runner::HookOutcome::Denied(d) = hook_runner::run("invite.before_issue", &input.to_string())?
    {
        return Err(err(&d.code, &d.message));
    }
    let lot = if bypass {
        None
    } else {
        Some(consume_one(uid, "invite")?)
    };
    let v = insert_invite(
        &k,
        NewInvite {
            kind: "personal",
            source: "user",
            scope,
            key,
            inviter: Some(uid),
            issued_by: Some(uid),
            issued_by_plugin: None,
            max_uses: 1,
            days,
            binding,
            note,
            lot,
        },
    )?;
    Ok((v, false))
}

/// 系统 / 活动码（管理员或服务）：不检查资格、不触发钩子、不消耗额度
fn issue_system(
    scope: String,
    by_user: Option<i64>,
    by_plugin: Option<String>,
    source: &str,
    b: IssueBody,
) -> R<(Value, bool)> {
    let s = settings();
    let k = keys()?;
    let key = b
        .request_id
        .clone()
        .filter(|r| (1..=64).contains(&r.len()))
        .unwrap_or_else(random::uuid_v7);
    if let Some(v) = replay(&k, &scope, &key)? {
        return Ok((v, true));
    }
    let kind = b.kind.as_deref().unwrap_or("system");
    let max_uses = match kind {
        "system" => b.max_uses.unwrap_or(1),
        "campaign" => b
            .max_uses
            .ok_or_else(|| invalid("max_uses is required for campaign codes"))?,
        _ => return Err(invalid("kind must be system or campaign")),
    };
    if !(1..=100_000).contains(&max_uses) || (kind == "campaign" && max_uses < 2) {
        return Err(invalid("max_uses out of range"));
    }
    if kind == "campaign" && b.inviter_id.is_some() {
        return Err(invalid("campaign codes have no inviter"));
    }
    if let Some(i) = b.inviter_id
        && user_info(i)?.is_none()
    {
        return Err(err("invite.user_not_found", "inviter not found"));
    }
    let binding = bind(&k, b.email.as_deref(), b.username.as_deref())?;
    let days = b.expires_days.unwrap_or(s.system_expiry_days).clamp(1, 90);
    let source = if kind == "campaign" { "campaign" } else { source };
    let v = insert_invite(
        &k,
        NewInvite {
            kind,
            source,
            scope,
            key,
            inviter: b.inviter_id,
            issued_by: by_user,
            issued_by_plugin: by_plugin,
            max_uses,
            days,
            binding,
            note: clean_note(b.note)?,
            lot: None,
        },
    )?;
    Ok((v, false))
}

// ---------------- 撤销与过期（§1.8） ----------------

/// 返还原批次（未过期才返还，DD-INV-11）
fn refund_to_lot(id: i64, inviter: i64, lot: i64, reason: &str) -> R<bool> {
    let n = x(
        "UPDATE quota_lots SET remaining = remaining + 1, updated_at = now()
          WHERE id = $1 AND remaining < amount AND (expires_at IS NULL OR expires_at > now())",
        &[SqlValue::Int(lot)],
    )?;
    if n == 1 {
        ledger(inviter, Some(lot), 1, reason, &format!("invite:{id}"), None, None)?;
        x(
            "UPDATE invites SET refunded = true WHERE id = $1",
            &[SqlValue::Int(id)],
        )?;
    }
    Ok(n == 1)
}

enum By {
    User(i64),
    Admin(i64),
    Plugin(String),
    System,
}

/// 撤销；已非 active 返回 Ok(false)（幂等），已兑换的返回 `invite.already_redeemed`
fn revoke(id: i64, by: &By, reason: &str, refund: Option<bool>) -> R<bool> {
    let r = q(
        "SELECT status, use_count, inviter_id, lot_id, kind, issued_by_plugin FROM invites WHERE id = $1 FOR UPDATE",
        &[SqlValue::Int(id)],
    )?;
    let row = r
        .first()
        .ok_or_else(|| err("kernel.not_found", "invite not found"))?;
    match by {
        By::User(u) if oint(&row[2]) != Some(*u) => return Err(err("invite.not_owner", "not your invite")),
        By::Plugin(p) if otext(&row[5]).as_deref() != Some(p.as_str()) => {
            return Err(err("invite.not_owner", "invite was not issued by the caller"));
        }
        _ => {}
    }
    if text(&row[0]) != "active" {
        return if int(&row[1]) > 0 && text(&row[0]) == "exhausted" {
            Err(err("invite.already_redeemed", "invite has already been used"))
        } else {
            Ok(false)
        };
    }
    if matches!(by, By::User(_)) {
        let busy = q(
            "SELECT 1 FROM reservations WHERE invite_id = $1 AND state = 'active' AND reserved_until > now() LIMIT 1",
            &[SqlValue::Int(id)],
        )?;
        if !busy.is_empty() {
            return Err(err(
                "invite.invite_reserved",
                "the invite is being used for a registration right now",
            ));
        }
    }
    let by_user = match by {
        By::User(u) | By::Admin(u) => Some(*u),
        _ => None,
    };
    x(
        "UPDATE invites SET status = $2, revoked_at = now(), revoked_by = $3, revoke_reason = $4, updated_at = now() WHERE id = $1",
        &[
            SqlValue::Int(id),
            t(if reason == "expired" { "expired" } else { "revoked" }),
            oi(by_user),
            t(reason),
        ],
    )?;
    let s = settings();
    let want = refund.unwrap_or(if reason == "expired" {
        s.refund_on_expire
    } else {
        s.refund_on_revoke
    });
    let inviter = oint(&row[2]);
    let mut refunded = false;
    if want
        && text(&row[4]) == "personal"
        && let (Some(inv), Some(lot)) = (inviter, oint(&row[3]))
    {
        refunded = refund_to_lot(
            id,
            inv,
            lot,
            if reason == "expired" {
                "expire_refund"
            } else {
                "revoke_refund"
            },
        )?;
    }
    publish(
        "invite.revoked",
        inviter.map_or_else(|| "global".to_owned(), |u| format!("user:{u}")),
        json!({ "invite_id": id, "inviter_id": inviter, "reason": reason, "refunded": refunded, "by_user_id": by_user }),
    )?;
    Ok(true)
}

fn revoke_all_active(uid: i64, reason: &str) -> R<i64> {
    let ids = q(
        "SELECT id FROM invites WHERE inviter_id = $1 AND status = 'active' ORDER BY id",
        &[SqlValue::Int(uid)],
    )?;
    let mut n = 0;
    for r in ids {
        if revoke(int(&r[0]), &By::System, reason, Some(false))? {
            n += 1;
        }
    }
    Ok(n)
}

// ---------------- 兑换（§1.3、§1.4） ----------------

fn deny(code: &str, msg: &str, details: Value) -> hooks::HookDecision {
    hooks::HookDecision::Deny(hooks::DenyInfo {
        code: code.into(),
        message: msg.into(),
        details: Some(details.to_string()),
    })
}

fn on_before_register(input: &Value) -> R<hooks::HookDecision> {
    let Some(code) = input["invite_code"].as_str().filter(|c| !c.is_empty()) else {
        return Ok(hooks::HookDecision::Allow);
    };
    let s = settings();
    let norm = codes::normalize(code);
    // 格式不合也走一次查找，对齐耗时
    let h = norm.as_deref().map_or_else(
        || codes::sha256(format!("INV-invalid:{code}").as_bytes()),
        codes::code_hash,
    );
    let holder = codes::holder(input["username"].as_str().unwrap_or(""));
    let r = q(
                    "SELECT i.id, i.kind, i.status, i.expires_at <= now(), i.max_uses, i.use_count, i.inviter_id, i.issued_by,
                    i.email_hmac, i.email_domain, i.username_binding, u.status
               FROM invites i LEFT JOIN identity.v_user_public u ON u.id = i.inviter_id
              WHERE i.code_hash = $1 FOR UPDATE OF i",
        &[SqlValue::Bytes(h.to_vec())],
    )?;
    let reason = match r.first() {
        None => "not_found",
        Some(_) if !s.enabled && !s.redeem_when_disabled => "disabled",
        Some(row) => match text(&row[2]).as_str() {
            "revoked" => "revoked",
            "exhausted" => "exhausted",
            "expired" => "expired",
            _ if boolean(&row[3]) => "expired",
            _ if oint(&row[6]).is_some()
                && !matches!(otext(&row[11]).as_deref(), Some("active" | "leech_disabled"))
                && !s.redeem_if_inviter_unavailable =>
            {
                // 对外不区分邀请人不可用
                "revoked"
            }
            _ => "ok",
        },
    };
    if reason != "ok" {
        return Ok(deny(
            "invite.code_invalid",
            "the invite code is invalid or has expired",
            json!({ "reason": reason }),
        ));
    }
    let row = &r[0];
    let id = int(&row[0]);
    if let Some(u) = otext(&row[10])
        && u != holder
    {
        return Ok(deny(
            "invite.binding_invalid",
            "this invite is bound to another user",
            json!({ "field": "username" }),
        ));
    }
    if let SqlValue::Bytes(hm) = &row[8] {
        let ok = match input["email"].as_str() {
            Some(e) => keys()?.email_hmac(e).as_slice() == hm.as_slice(),
            None => input["email_domain"].as_str().map(str::to_lowercase) == otext(&row[9]),
        };
        if !ok {
            return Ok(deny(
                "invite.binding_invalid",
                "this invite is bound to another email",
                json!({ "field": "email" }),
            ));
        }
    }
    let others = q(
        &format!(
            "SELECT count(*), {} FROM reservations
              WHERE invite_id = $1 AND state = 'active' AND reserved_until > now() AND holder <> $2",
            "COALESCE(extract(epoch FROM min(reserved_until) - now())::bigint, 0)"
        ),
        &[SqlValue::Int(id), t(&holder)],
    )?;
    if int(&row[5]) + int(&others[0][0]) >= int(&row[4]) {
        return Ok(deny(
            "invite.code_reserved",
            "the invite code is being used by another registration",
            json!({ "retry_after_secs": int(&others[0][1]) }),
        ));
    }
    x(
        "INSERT INTO reservations (invite_id, holder, reserved_until, state)
         VALUES ($1, $2, now() + make_interval(secs => $3::int), 'active')
         ON CONFLICT (invite_id, holder) WHERE state = 'active'
         DO UPDATE SET reserved_until = EXCLUDED.reserved_until, updated_at = now()",
        &[
            SqlValue::Int(id),
            t(&holder),
            SqlValue::Int(s.reservation_ttl_secs.clamp(60, 3600)),
        ],
    )?;
    let inviter = match text(&row[1]).as_str() {
        "campaign" => oint(&row[7]).filter(|_| s.campaign_inviter == "creator"),
        _ => oint(&row[6]),
    };
    let mut attrs = json!({ "invite_id": id });
    if let Some(i) = inviter {
        attrs["inviter_id"] = json!(i);
    }
    Ok(hooks::HookDecision::Modify(
        json!({ "attributes": attrs }).to_string(),
    ))
}

fn add_edge(invitee: i64, inviter: i64, invite_id: i64, registered_ms: i64) -> R<()> {
    if invitee == inviter {
        return Ok(());
    }
    let n = x(
        "INSERT INTO tree_edges (invitee_id, inviter_id, invite_id, invitee_registered_at)
         VALUES ($1, $2, $3, to_timestamp($4::float8 / 1000)) ON CONFLICT (invitee_id) DO NOTHING",
        &[
            SqlValue::Int(invitee),
            SqlValue::Int(inviter),
            SqlValue::Int(invite_id),
            SqlValue::Int(registered_ms),
        ],
    )?;
    if n == 0 {
        return Ok(());
    }
    x(
        "INSERT INTO tree_closure (ancestor_id, descendant_id, depth) VALUES ($1, $1, 0), ($2, $2, 0) ON CONFLICT DO NOTHING",
        &[SqlValue::Int(inviter), SqlValue::Int(invitee)],
    )?;
    x(
        "INSERT INTO tree_closure (ancestor_id, descendant_id, depth)
         SELECT ancestor_id, $2, depth + 1 FROM tree_closure WHERE descendant_id = $1 AND depth < 32767 ON CONFLICT DO NOTHING",
        &[SqlValue::Int(inviter), SqlValue::Int(invitee)],
    )?;
    Ok(())
}

fn on_registered(evt: &on_event::EventEnvelope, p: &Value) -> R<()> {
    let uid = p["user_id"].as_i64().unwrap_or(0);
    if let Some(iid) = p["attributes"]["invite_id"].as_i64() {
        let r = q(
            "SELECT kind, status, max_uses, use_count, inviter_id, issued_by FROM invites WHERE id = $1 FOR UPDATE",
            &[SqlValue::Int(iid)],
        )?;
        let dup = q(
            "SELECT 1 FROM redemptions WHERE invitee_id = $1",
            &[SqlValue::Int(uid)],
        )?;
        if let (Some(inv), true) = (r.first(), dup.is_empty()) {
            let holder = codes::holder(p["username"].as_str().unwrap_or(""));
            x(
                "UPDATE reservations SET state = 'consumed', user_id = $3, updated_at = now()
                  WHERE id = (SELECT id FROM reservations WHERE invite_id = $1 AND holder = $2 AND state IN ('active', 'released')
                               ORDER BY (state = 'active') DESC, id DESC LIMIT 1)",
                &[SqlValue::Int(iid), t(&holder), SqlValue::Int(uid)],
            )?;
            let over =
                int(&inv[3]) >= int(&inv[2]) || matches!(text(&inv[1]).as_str(), "revoked" | "expired");
            x(
                "UPDATE invites SET use_count = use_count + 1,
                        status = CASE WHEN status = 'active' AND use_count + 1 >= max_uses THEN 'exhausted' ELSE status END,
                        updated_at = now() WHERE id = $1",
                &[SqlValue::Int(iid)],
            )?;
            let inviter = p["inviter_id"].as_i64().or_else(|| oint(&inv[4]));
            x(
                "INSERT INTO redemptions (invite_id, invitee_id, inviter_id, event_id, over_redeemed) VALUES ($1, $2, $3, $4, $5)",
                &[
                    SqlValue::Int(iid),
                    SqlValue::Int(uid),
                    oi(inviter),
                    SqlValue::Uuid(evt.id.clone()),
                    SqlValue::Boolean(over),
                ],
            )?;
            if let Some(i) = inviter {
                add_edge(uid, i, iid, evt.occurred_at)?;
            }
            publish(
                "invite.redeemed",
                inviter.map_or_else(|| "global".to_owned(), |u| format!("user:{u}")),
                json!({ "invite_id": iid, "inviter_id": inviter, "invitee_id": uid, "kind": text(&inv[0]), "over_redeemed": over }),
            )?;
        }
    }
    let init = settings().initial_quota;
    if init.amount > 0 {
        grant(&Grant {
            uid,
            amount: init.amount.min(10),
            source: "initial",
            source_ref: evt.id.clone(),
            expires_ms: init.expires_days.map(|d| time::now() + d * 86_400_000),
            reason: "initial",
            actor: None,
            note: None,
            max_balance: None,
        })?;
    }
    Ok(())
}

fn on_promoted(evt: &on_event::EventEnvelope, p: &Value) -> R<()> {
    let (uid, to) = (
        p["user_id"].as_i64().unwrap_or(0),
        p["to_class"].as_i64().unwrap_or(-1),
    );
    let s = settings();
    let Some(cg) = s.class_grants.iter().find(|g| g.class_id == to) else {
        return Ok(());
    };
    let Some(rule) = &cg.on_promotion else {
        return Ok(());
    };
    let promo = lock_account(uid)?;
    if promo.contains(&to) && !cg.repeat_on_repromotion {
        return Ok(());
    }
    grant(&Grant {
        uid,
        amount: rule.amount,
        source: "level_promotion",
        source_ref: evt.id.clone(),
        expires_ms: rule.expires_days.map(|d| time::now() + d * 86_400_000),
        reason: "level_promotion",
        actor: None,
        note: None,
        max_balance: None,
    })?;
    x(
        "UPDATE accounts SET promo_classes = array_append(promo_classes, $2::int), updated_at = now()
          WHERE user_id = $1 AND NOT ($2::int = ANY(promo_classes))",
        &[SqlValue::Int(uid), SqlValue::Int(to)],
    )?;
    Ok(())
}

// ---------------- 连坐（§1.7） ----------------

fn on_trigger(event_id: &str, trigger: &str, invitee: i64, occurred_ms: i64) -> R<()> {
    let edge = q(
        &format!(
            "SELECT {} FROM tree_edges WHERE invitee_id = $1",
            ms("invitee_registered_at")
        ),
        &[SqlValue::Int(invitee)],
    )?;
    let Some(registered) = edge.first().map(|r| int(&r[0])) else {
        return Ok(());
    };
    let s = settings();
    for rule in s
        .penalty_rules
        .iter()
        .filter(|r| r.enabled && r.trigger == trigger)
    {
        if registered + rule.within_days * 86_400_000 < occurred_ms {
            continue;
        }
        let group = rule.group.clone().unwrap_or_else(|| rule.id.clone());
        let ancestors = q(
            "SELECT ancestor_id, depth FROM tree_closure WHERE descendant_id = $1 AND depth BETWEEN 1 AND $2 ORDER BY depth",
            &[SqlValue::Int(invitee), SqlValue::Int(rule.max_depth.clamp(1, 10))],
        )?;
        for a in ancestors {
            let (target, depth) = (int(&a[0]), int(&a[1]));
            let Some(level) = rule.levels.iter().find(|l| l.depth == depth) else {
                continue;
            };
            let dup = q(
                "SELECT 1 FROM penalties WHERE invitee_id = $1 AND target_user_id = $2 AND rule_group = $3
                    AND state IN ('applied','pending_review','confirmed') AND created_at > now() - make_interval(days => $4::int)",
                &[SqlValue::Int(invitee), SqlValue::Int(target), t(&group), SqlValue::Int(s.dedupe_window_days)],
            )?;
            if !dup.is_empty() {
                continue;
            }
            let review = level.actions.iter().any(|a| a["type"] == "flag_review");
            let state = if has_perm(target, "invite.penalty.exempt")? {
                "exempt"
            } else if review {
                "pending_review"
            } else {
                "applied"
            };
            let r = q(
                "INSERT INTO penalties (rule_id, rule_group, trigger, trigger_event_id, invitee_id, target_user_id, depth, actions, state)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                 ON CONFLICT (trigger_event_id, rule_id, target_user_id) DO NOTHING RETURNING id",
                &[
                    t(&rule.id),
                    t(&group),
                    t(trigger),
                    SqlValue::Uuid(event_id.into()),
                    SqlValue::Int(invitee),
                    SqlValue::Int(target),
                    SqlValue::Int(depth),
                    SqlValue::Json(Value::Array(level.actions.clone()).to_string()),
                    t(state),
                ],
            )?;
            let Some(pid) = r.first().map(|r| int(&r[0])) else {
                continue;
            };
            if state == "applied" {
                execute_penalty(pid, "applied")?;
            }
            let types: Vec<Value> = level.actions.iter().map(|a| a["type"].clone()).collect();
            publish(
                "invite.penalty_applied",
                format!("user:{target}"),
                json!({ "penalty_id": pid, "rule_id": rule.id, "trigger": trigger, "invitee_id": invitee,
                        "target_user_id": target, "depth": depth, "actions": types, "state": state }),
            )?;
        }
    }
    Ok(())
}

fn execute_penalty(pid: i64, final_state: &str) -> R<()> {
    let r = q(
        "SELECT target_user_id, actions FROM penalties WHERE id = $1 FOR UPDATE",
        &[SqlValue::Int(pid)],
    )?;
    let Some(p) = r.first() else { return Ok(()) };
    let target = int(&p[0]);
    let actions: Vec<Value> = match &p[1] {
        SqlValue::Json(j) | SqlValue::Text(j) => serde_json::from_str(j).unwrap_or_default(),
        _ => vec![],
    };
    let mut applied = serde_json::Map::new();
    let mut errors = Vec::new();
    lock_account(target)?;
    for a in &actions {
        match a["type"].as_str() {
            Some("deduct_quota") => {
                let (d, _) = deduct(
                    target,
                    a["amount"].as_i64().unwrap_or(1).clamp(1, 1000),
                    "penalty",
                    None,
                    &format!("pen:{pid}"),
                    None,
                )?;
                applied.insert("deducted".into(), json!(d));
            }
            Some("suspend_invite") => {
                let r = q(
                    "INSERT INTO suspensions (user_id, source, ref, reason, until)
                     VALUES ($1, 'penalty', $2, 'invite.penalty', CASE WHEN $3::bigint IS NULL THEN NULL ELSE now() + make_interval(days => $3::int) END)
                     ON CONFLICT (user_id, source, ref) DO UPDATE SET until = EXCLUDED.until RETURNING id",
                    &[SqlValue::Int(target), t(&pid.to_string()), oi(a["days"].as_i64())],
                )?;
                applied.insert("suspension_id".into(), json!(int(&r[0][0])));
            }
            Some("revoke_outstanding") => {
                applied.insert("revoked".into(), json!(revoke_all_active(target, "penalty")?));
            }
            Some("restrict_inviter") => {
                let days = a["days"].as_i64().unwrap_or(7).clamp(1, 365);
                let res = svc(
                    "ctx.users",
                    "apply_restriction",
                    &json!({ "user": target, "kind": "leech_disabled", "reason_code": "invite.penalty",
                             "until": time::now() + days * 86_400_000 }),
                );
                match res {
                    Ok(v) => {
                        applied.insert("restriction_id".into(), v["restriction_id"].clone());
                    }
                    Err(e) if e.retryable => return Err(e),
                    Err(e) => errors.push(json!({ "action": "restrict_inviter", "code": e.code })),
                }
            }
            // 通知需要 ctx.notify（未实现）；flag_review 无动作
            _ => {}
        }
    }
    if !errors.is_empty() {
        applied.insert("errors".into(), Value::Array(errors));
    }
    x(
        "UPDATE penalties SET applied = $2, state = $3, version = version + 1, updated_at = now() WHERE id = $1",
        &[
            SqlValue::Int(pid),
            SqlValue::Json(Value::Object(applied).to_string()),
            t(final_state),
        ],
    )?;
    Ok(())
}

fn reverse_penalty(pid: i64, reason: &str) -> R<bool> {
    let r = q(
        "SELECT state, target_user_id, applied::text FROM penalties WHERE id = $1 FOR UPDATE",
        &[SqlValue::Int(pid)],
    )?;
    let Some(p) = r.first() else {
        return Err(err("kernel.not_found", "penalty not found"));
    };
    let state = text(&p[0]);
    if !matches!(state.as_str(), "applied" | "confirmed" | "pending_review") {
        return Ok(false);
    }
    let target = int(&p[1]);
    if state != "pending_review" {
        let applied: Value = serde_json::from_str(&text(&p[2])).unwrap_or(Value::Null);
        if let Some(d) = applied["deducted"].as_i64().filter(|d| *d > 0) {
            grant(&Grant {
                uid: target,
                amount: d,
                source: "penalty_refund",
                source_ref: format!("pen:{pid}"),
                expires_ms: None,
                reason: "penalty_refund",
                actor: None,
                note: None,
                max_balance: None,
            })?;
        }
        if let Some(sid) = applied["suspension_id"].as_i64() {
            x(
                "UPDATE suspensions SET lifted_at = now(), lifted_reason = $2 WHERE id = $1 AND lifted_at IS NULL",
                &[SqlValue::Int(sid), t(reason)],
            )?;
        }
        if applied["restriction_id"].is_i64() {
            svc(
                "ctx.users",
                "lift_restriction",
                &json!({ "user": target, "kind": "leech_disabled", "reason": "invite.penalty_reversed" }),
            )?;
        }
    }
    x(
        "UPDATE penalties SET state = 'reversed', reversed_at = now(), reverse_reason = $2, version = version + 1, updated_at = now() WHERE id = $1",
        &[SqlValue::Int(pid), t(reason)],
    )?;
    publish(
        "invite.penalty_reversed",
        format!("user:{target}"),
        json!({ "penalty_id": pid, "target_user_id": target, "reason": reason }),
    )?;
    Ok(true)
}

fn on_status_changed(evt: &on_event::EventEnvelope, p: &Value) -> R<()> {
    let uid = p["user_id"].as_i64().unwrap_or(0);
    let (from, to) = (p["from"].as_str().unwrap_or(""), p["to"].as_str().unwrap_or(""));
    if to == "banned" {
        x(
            "INSERT INTO invitee_bans (invitee_id, banned_at, event_id) VALUES ($1, to_timestamp($2::float8 / 1000), $3) ON CONFLICT DO NOTHING",
            &[
                SqlValue::Int(uid),
                SqlValue::Int(evt.occurred_at),
                SqlValue::Uuid(evt.id.clone()),
            ],
        )?;
        // 不由本插件自己的限制引发连锁
        if p["restriction_source"].as_str() != Some("invite") {
            on_trigger(&evt.id, "invitee_banned", uid, evt.occurred_at)?;
        }
    } else if from == "banned" && to != "deleted" {
        let s = settings();
        let rules: Vec<String> = s
            .penalty_rules
            .iter()
            .filter(|r| r.reverse_on_lift)
            .map(|r| r.id.clone())
            .collect();
        let pids = q(
            "SELECT id FROM penalties WHERE invitee_id = $1 AND trigger = 'invitee_banned' AND rule_id = ANY($2)
                AND state IN ('applied','confirmed','pending_review') ORDER BY id",
            &[SqlValue::Int(uid), SqlValue::TextArray(rules)],
        )?;
        for r in pids {
            reverse_penalty(int(&r[0]), "invitee_restored")?;
        }
    }
    Ok(())
}

// ---------------- 树与统计（§1.6、§1.9） ----------------

fn tree(uid: i64, depth: i64, cursor: Option<&str>, limit: i64) -> R<Value> {
    let depth = depth.clamp(1, 10);
    let limit = limit.clamp(1, 100);
    let (cd, cu) = cursor
        .and_then(|c| c.split_once('.'))
        .and_then(|(d, u)| Some((d.parse::<i64>().ok()?, u.parse::<i64>().ok()?)))
        .unwrap_or((0, 0));
    let rows = q(
        &format!(
            "SELECT c.depth, c.descendant_id, e.inviter_id, {}, u.username, u.class_id, u.status, e.purged,
                    (SELECT count(*) FROM tree_edges e2 WHERE e2.inviter_id = c.descendant_id)
               FROM tree_closure c
               JOIN tree_edges e ON e.invitee_id = c.descendant_id
               LEFT JOIN identity.v_user_public u ON u.id = c.descendant_id
              WHERE c.ancestor_id = $1 AND c.depth BETWEEN 1 AND $2 AND (c.depth, c.descendant_id) > ($3::smallint, $4::bigint)
              ORDER BY c.depth, c.descendant_id LIMIT $5",
            ms("e.invitee_registered_at")
        ),
        &[SqlValue::Int(uid), SqlValue::Int(depth), SqlValue::Int(cd), SqlValue::Int(cu), SqlValue::Int(limit + 1)],
    )?;
    let counts = q(
        "SELECT depth, count(*) FROM tree_closure WHERE ancestor_id = $1 AND depth BETWEEN 1 AND $2 GROUP BY depth ORDER BY depth",
        &[SqlValue::Int(uid), SqlValue::Int(depth)],
    )?;
    let has_more = rows.len() > usize::try_from(limit).unwrap_or(100);
    let items: Vec<Value> = rows
        .iter()
        .take(usize::try_from(limit).unwrap_or(100))
        .map(|r| {
            let purged = boolean(&r[7]);
            json!({ "user_id": int(&r[1]), "depth": int(&r[0]), "inviter_id": int(&r[2]), "registered_at": iso(int(&r[3])),
                    "username": if purged { Value::Null } else { json!(otext(&r[4])) }, "class_id": oint(&r[5]),
                    "status": otext(&r[6]), "purged": purged, "direct_count": int(&r[8]) })
        })
        .collect();
    let next = if has_more {
        items.last().map(|i| format!("{}.{}", i["depth"], i["user_id"]))
    } else {
        None
    };
    Ok(
        json!({ "root": uid, "level_counts": counts.iter().map(|r| int(&r[1])).collect::<Vec<_>>(),
               "items": items, "next_cursor": next, "has_more": has_more }),
    )
}

fn ancestors(uid: i64, max_depth: i64) -> R<Value> {
    let rows = q(
        "SELECT c.ancestor_id, c.depth, u.username FROM tree_closure c LEFT JOIN identity.v_user_public u ON u.id = c.ancestor_id
          WHERE c.descendant_id = $1 AND c.depth BETWEEN 1 AND $2 ORDER BY c.depth",
        &[SqlValue::Int(uid), SqlValue::Int(max_depth.clamp(1, 10))],
    )?;
    Ok(
        json!({ "items": rows.iter().map(|r| json!({ "user_id": int(&r[0]), "depth": int(&r[1]), "username": otext(&r[2]) })).collect::<Vec<_>>() }),
    )
}

fn stats(uid: i64) -> R<Value> {
    let a = q(
        "SELECT count(*) FILTER (WHERE kind = 'personal'), count(*) FILTER (WHERE status = 'active'),
                count(*) FILTER (WHERE status = 'expired'), count(*) FILTER (WHERE status = 'revoked')
           FROM invites WHERE inviter_id = $1",
        &[SqlValue::Int(uid)],
    )?;
    let b = q(
        "SELECT u.status, count(*) FROM tree_edges e JOIN identity.v_user_public u ON u.id = e.invitee_id WHERE e.inviter_id = $1 GROUP BY u.status",
        &[SqlValue::Int(uid)],
    )?;
    let by_status: serde_json::Map<String, Value> =
        b.iter().map(|r| (text(&r[0]), json!(int(&r[1])))).collect();
    let invitees: i64 = b.iter().map(|r| int(&r[1])).sum();
    Ok(
        json!({ "issued": int(&a[0][0]), "outstanding": int(&a[0][1]), "expired": int(&a[0][2]), "revoked": int(&a[0][3]),
               "invitees": invitees, "invitees_by_status": by_status }),
    )
}

fn validate(code: &str) -> R<Value> {
    let s = settings();
    let h = codes::normalize(code).map_or_else(
        || codes::sha256(format!("INV-invalid:{code}").as_bytes()),
        |c| codes::code_hash(&c),
    );
    let r = q(
        &format!(
            "SELECT i.kind, i.status, i.expires_at <= now(), {}, i.email_hmac IS NOT NULL, i.username_binding IS NOT NULL,
                    i.inviter_id, u.username, u.status
               FROM invites i LEFT JOIN identity.v_user_public u ON u.id = i.inviter_id WHERE i.code_hash = $1",
            ms("i.expires_at")
        ),
        &[SqlValue::Bytes(h.to_vec())],
    )?;
    let reason = match r.first() {
        None => "not_found",
        Some(_) if !s.enabled && !s.redeem_when_disabled => "disabled",
        Some(row) => match text(&row[1]).as_str() {
            "active" if boolean(&row[2]) => "expired",
            "active"
                if oint(&row[6]).is_some()
                    && !matches!(otext(&row[8]).as_deref(), Some("active" | "leech_disabled"))
                    && !s.redeem_if_inviter_unavailable =>
            {
                "revoked"
            }
            "active" => "ok",
            other => match other {
                "exhausted" => "exhausted",
                "expired" => "expired",
                _ => "revoked",
            },
        },
    };
    if reason != "ok" {
        return Err(err_d(
            "invite.code_invalid",
            "the invite code is invalid or has expired",
            json!({ "reason": reason }),
        ));
    }
    let row = &r[0];
    Ok(json!({
        "valid": true, "kind": text(&row[0]), "expires_at": iso(int(&row[3])), "email_bound": boolean(&row[4]),
        "username_bound": boolean(&row[5]),
        "inviter_username": if s.show_inviter_on_check { json!(otext(&row[7])) } else { Value::Null },
    }))
}

// ---------------- 定时任务 ----------------

fn expire_invites() -> R<()> {
    loop {
        let rows = q(
            "SELECT id FROM invites WHERE status = 'active' AND expires_at <= now() ORDER BY expires_at LIMIT 500 FOR UPDATE SKIP LOCKED",
            &[],
        )?;
        if rows.is_empty() {
            return Ok(());
        }
        for r in rows {
            revoke(int(&r[0]), &By::System, "expired", None)?;
        }
        sql::commit()?;
    }
}

fn expire_lots() -> R<()> {
    loop {
        let rows = q(
            "SELECT id, user_id, remaining FROM quota_lots WHERE remaining > 0 AND expires_at <= now()
              ORDER BY expires_at LIMIT 500 FOR UPDATE SKIP LOCKED",
            &[],
        )?;
        if rows.is_empty() {
            return Ok(());
        }
        for r in rows {
            let (id, uid, rem) = (int(&r[0]), int(&r[1]), int(&r[2]));
            x(
                "UPDATE quota_lots SET remaining = 0, updated_at = now() WHERE id = $1",
                &[SqlValue::Int(id)],
            )?;
            ledger(
                uid,
                Some(id),
                -rem,
                "lot_expired",
                &format!("lot:{id}"),
                None,
                None,
            )?;
        }
        sql::commit()?;
    }
}

fn periodic_grants(force: bool) -> R<()> {
    let s = settings();
    let p = q(
        "SELECT to_char(d, 'YYYY-MM'), to_char(d, 'IYYY-\"W\"IW'), extract(day FROM d)::bigint, extract(isodow FROM d)::bigint
           FROM (SELECT (now() AT TIME ZONE $1)::date AS d) x",
        &[t(&time::site_timezone())],
    )?;
    let (month, week, dom, dow) = (text(&p[0][0]), text(&p[0][1]), int(&p[0][2]), int(&p[0][3]));
    let period = match s.periodic_schedule.as_str() {
        "weekly" if force || dow == 1 => week,
        "monthly" if force || dom == 1 => month,
        _ => return Ok(()),
    };
    let rules: Vec<&ClassGrant> = s.class_grants.iter().filter(|g| g.periodic.is_some()).collect();
    if rules.is_empty() || kv::get(&format!("periodic:{period}:done"))?.is_some() {
        return Ok(());
    }
    let classes: Vec<i64> = rules.iter().map(|g| g.class_id).collect();
    let ck = format!("periodic:{period}:cursor");
    let mut cursor: i64 = kv::get(&ck)?
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    loop {
        let users = q(
            "SELECT id, class_id FROM identity.v_user_public WHERE status = 'active' AND class_id = ANY($1) AND id > $2 ORDER BY id LIMIT 1000",
            &[SqlValue::IntArray(classes.clone()), SqlValue::Int(cursor)],
        )?;
        let Some(last) = users.last().map(|r| int(&r[0])) else {
            kv::set(
                &format!("periodic:{period}:done"),
                time::now().to_string().as_bytes(),
                None,
            )?;
            return Ok(());
        };
        for u in &users {
            let (uid, class) = (int(&u[0]), int(&u[1]));
            let Some(g) = rules
                .iter()
                .find(|g| g.class_id == class)
                .and_then(|g| g.periodic.as_ref())
            else {
                continue;
            };
            grant(&Grant {
                uid,
                amount: g.amount,
                source: "periodic",
                source_ref: format!("periodic:{period}:{class}"),
                expires_ms: g.expires_days.map(|d| time::now() + d * 86_400_000),
                reason: "periodic",
                actor: None,
                note: None,
                max_balance: Some(g.max_balance),
            })?;
        }
        cursor = last;
        kv::set(&ck, cursor.to_string().as_bytes(), None)?;
        sql::commit()?;
    }
}

// ---------------- 导出 ----------------

impl lifecycle::Guest for Invite {
    fn init() -> R<()> {
        keys().map(|_| ())
    }
}

impl hooks::Guest for Invite {
    fn handle(c: hooks::HookCall) -> R<hooks::HookDecision> {
        let input: Value = serde_json::from_str(&c.input).unwrap_or(Value::Null);
        match c.hook.as_str() {
            "user.before_register" => on_before_register(&input),
            _ => Ok(hooks::HookDecision::Allow),
        }
    }
}

impl on_event::Guest for Invite {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        let p: Value =
            serde_json::from_str(&evt.payload).map_err(|_| err("invite.bad_event", "invalid payload"))?;
        match evt.event_type.as_str() {
            "user.registered" => on_registered(&evt, &p),
            "user.status_changed" => on_status_changed(&evt, &p),
            "level.promoted" => on_promoted(&evt, &p),
            _ => Ok(()),
        }
    }
}

impl on_cron::Guest for Invite {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        let payload: Value = serde_json::from_str(&job.payload).unwrap_or(Value::Null);
        match job.name.as_str() {
            "expire_invites" => expire_invites(),
            "expire_lots" => expire_lots(),
            "release_reservations" => x(
                "UPDATE reservations SET state = 'released', updated_at = now()
                  WHERE id IN (SELECT id FROM reservations WHERE state = 'active' AND reserved_until < now() - interval '1 minute' LIMIT 1000)",
                &[],
            )
            .map(|_| ()),
            "periodic_grants" => periodic_grants(payload["force"].as_bool() == Some(true)),
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

// ---------------- HTTP（LLD §2.4） ----------------

fn resp(status: u16, v: &Value, no_store: bool) -> http_handler::Response {
    let mut headers = vec![http_handler::Header {
        name: "content-type".into(),
        value: "application/json".into(),
    }];
    if no_store {
        headers.push(http_handler::Header {
            name: "cache-control".into(),
            value: "no-store".into(),
        });
    }
    http_handler::Response {
        status,
        headers,
        body: v.to_string().into_bytes(),
    }
}

fn no_content() -> http_handler::Response {
    http_handler::Response {
        status: 204,
        headers: vec![],
        body: vec![],
    }
}

fn param(req: &http_handler::Request, k: &str) -> R<i64> {
    req.params
        .iter()
        .find(|(n, _)| n == k)
        .and_then(|(_, v)| v.parse().ok())
        .ok_or_else(|| err("kernel.bad_request", "bad path parameter"))
}

fn query<'a>(req: &'a http_handler::Request, k: &str) -> Option<&'a str> {
    req.query.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

fn body<T: for<'de> Deserialize<'de>>(req: &http_handler::Request) -> R<T> {
    let b: &[u8] = if req.body.is_empty() { b"{}" } else { &req.body };
    serde_json::from_slice(b).map_err(|e| err("kernel.bad_request", &format!("invalid JSON body: {e}")))
}

/// 本人或有 `invite.tree.read`
fn can_view(uid: i64) -> R<()> {
    let me = me()?;
    if me == uid || has_perm(me, "invite.tree.read")? {
        Ok(())
    } else {
        Err(err("kernel.forbidden", "not allowed to view this invite tree"))
    }
}

fn list_ledger(uid: i64, cursor: Option<&str>) -> R<Value> {
    let before = cursor.and_then(|c| c.parse::<i64>().ok()).unwrap_or(i64::MAX);
    let rows = q(
        &format!(
            "SELECT id, lot_id, delta, reason, ref, balance_after, note, {} FROM quota_ledger WHERE user_id = $1 AND id < $2 ORDER BY id DESC LIMIT 51",
            ms("created_at")
        ),
        &[SqlValue::Int(uid), SqlValue::Int(before)],
    )?;
    let items: Vec<Value> = rows
        .iter()
        .take(50)
        .map(|r| json!({ "id": int(&r[0]), "lot_id": oint(&r[1]), "delta": int(&r[2]), "reason": text(&r[3]), "ref": otext(&r[4]),
                         "balance_after": int(&r[5]), "note": otext(&r[6]), "created_at": iso(int(&r[7])) }))
        .collect();
    let next = (rows.len() > 50)
        .then(|| items.last().map(|i| i["id"].to_string()))
        .flatten();
    Ok(json!({ "items": items, "next_cursor": next }))
}

fn list_invites(filter: &str, params: Vec<SqlValue>, cursor: Option<&str>) -> R<Value> {
    let before = cursor.and_then(|c| c.parse::<i64>().ok()).unwrap_or(i64::MAX);
    let n = params.len();
    let mut ps = params;
    ps.push(SqlValue::Int(before));
    let rows = q(
        &format!(
            "SELECT {INV_COLS} FROM invites WHERE {filter} AND id < ${} ORDER BY id DESC LIMIT 51",
            n + 1
        ),
        &ps,
    )?;
    let items: Vec<Value> = rows.iter().take(50).map(invite_json).collect();
    let next = (rows.len() > 50)
        .then(|| items.last().map(|i| i["id"].to_string()))
        .flatten();
    Ok(json!({ "items": items, "next_cursor": next }))
}

fn statuses(req: &http_handler::Request) -> Vec<String> {
    query(req, "status")
        .map(|s| s.split(',').map(str::to_owned).collect())
        .unwrap_or_default()
}

fn list_penalties(filter: &str, params: &[SqlValue]) -> R<Value> {
    let rows = q(
        &format!(
            "SELECT id, rule_id, trigger, invitee_id, target_user_id, depth, actions::text, applied::text, state, version, {}
               FROM penalties WHERE {filter} ORDER BY id DESC LIMIT 100",
            ms("created_at")
        ),
        params,
    )?;
    Ok(json!({ "items": rows.iter().map(|r| json!({
        "id": int(&r[0]), "rule_id": text(&r[1]), "trigger": text(&r[2]), "invitee_id": int(&r[3]), "target_user_id": int(&r[4]),
        "depth": int(&r[5]), "actions": serde_json::from_str::<Value>(&text(&r[6])).unwrap_or(Value::Null),
        "applied": serde_json::from_str::<Value>(&text(&r[7])).unwrap_or(Value::Null), "state": text(&r[8]),
        "version": int(&r[9]), "created_at": iso(int(&r[10])) })).collect::<Vec<_>>() }))
}

#[derive(Deserialize)]
struct CheckBody {
    code: String,
}

#[derive(Deserialize)]
struct AdjustBody {
    delta: i64,
    expires_at_ms: Option<i64>,
    reason: String,
    request_id: Option<String>,
}

#[derive(Deserialize)]
struct ReviewBody {
    decision: String,
    note: Option<String>,
    version: i64,
}

#[derive(Deserialize)]
struct SuspendBody {
    until_ms: Option<i64>,
    reason: String,
}

impl http_handler::Guest for Invite {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let s = settings();
        let v = match req.route_id.as_str() {
            "check_code" => validate(&body::<CheckBody>(&req)?.code)?,
            "my_quota" => quota_json(me()?, &s)?,
            "my_quota_ledger" => list_ledger(me()?, query(&req, "cursor"))?,
            "my_eligibility" => {
                let uid = me()?;
                let bypass = has_perm(uid, "invite.quota.bypass")?;
                let (checks, _, _) = eligibility(uid, &s, bypass)?;
                json!({ "eligible": checks.iter().all(|c| c.ok), "bypass": bypass,
                        "checks": checks.iter().map(|c| json!({ "name": c.name, "ok": c.ok, "code": if c.ok { Value::Null } else { json!(c.code) }, "details": c.details })).collect::<Vec<_>>() })
            }
            "my_invites" => {
                let st = statuses(&req);
                list_invites(
                    "inviter_id = $1 AND kind = 'personal' AND (cardinality($2::text[]) = 0 OR status = ANY($2))",
                    vec![SqlValue::Int(me()?), SqlValue::TextArray(st)],
                    query(&req, "cursor"),
                )?
            }
            "issue_invite" => {
                let (v, replayed) = issue_personal(me()?, body(&req)?)?;
                return Ok(resp(if replayed { 200 } else { 201 }, &v, true));
            }
            "revoke_my_invite" => {
                revoke(param(&req, "id")?, &By::User(me()?), "user", None)?;
                return Ok(no_content());
            }
            "my_invitees" => tree(
                me()?,
                1,
                query(&req, "cursor"),
                query(&req, "limit").and_then(|l| l.parse().ok()).unwrap_or(50),
            )?,
            "my_stats" => stats(me()?)?,
            "my_penalties" => list_penalties("target_user_id = $1", &[SqlValue::Int(me()?)])?,
            "user_tree" => {
                let uid = param(&req, "id")?;
                can_view(uid)?;
                tree(
                    uid,
                    query(&req, "depth").and_then(|d| d.parse().ok()).unwrap_or(3),
                    query(&req, "cursor"),
                    query(&req, "limit").and_then(|l| l.parse().ok()).unwrap_or(100),
                )?
            }
            "user_ancestors" => {
                let uid = param(&req, "id")?;
                can_view(uid)?;
                ancestors(uid, 10)?
            }
            "user_stats" => {
                let uid = param(&req, "id")?;
                can_view(uid)?;
                stats(uid)?
            }
            "admin_list_invites" => {
                let st = statuses(&req);
                let inviter = query(&req, "inviter_id").and_then(|v| v.parse().ok());
                list_invites(
                    "(cardinality($1::text[]) = 0 OR status = ANY($1)) AND ($2::bigint IS NULL OR inviter_id = $2)
                     AND ($3::text IS NULL OR kind = $3) AND ($4::text IS NULL OR code_hint = upper($4))",
                    vec![SqlValue::TextArray(st), oi(inviter), ot(query(&req, "kind")), ot(query(&req, "code_hint"))],
                    query(&req, "cursor"),
                )?
            }
            "admin_issue_invite" => {
                let uid = me()?;
                let (v, replayed) =
                    issue_system(format!("admin:{uid}"), Some(uid), None, "admin", body(&req)?)?;
                write_audit(
                    "invite.issue_system",
                    format!("invite:{}", v["invite"]["id"]),
                    json!({ "kind": v["invite"]["kind"], "max_uses": v["invite"]["max_uses"] }),
                )?;
                return Ok(resp(if replayed { 200 } else { 201 }, &v, true));
            }
            "admin_revoke_invite" => {
                let id = param(&req, "id")?;
                let refund = query(&req, "refund").map(|r| r == "true");
                revoke(id, &By::Admin(me()?), "admin", refund)?;
                write_audit(
                    "invite.revoke_admin",
                    format!("invite:{id}"),
                    json!({ "refund": refund }),
                )?;
                return Ok(no_content());
            }
            "admin_user_quota" => {
                let uid = param(&req, "id")?;
                let mut v = quota_json(uid, &s)?;
                v["ledger"] = list_ledger(uid, None)?["items"].clone();
                v
            }
            "admin_adjust_quota" => {
                let uid = param(&req, "id")?;
                let b: AdjustBody = body(&req)?;
                if b.reason.trim().is_empty() || b.reason.chars().count() > 500 || b.delta == 0 {
                    return Err(invalid(
                        "delta (non-zero) and reason (≤ 500 characters) are required",
                    ));
                }
                if user_info(uid)?.is_none() {
                    return Err(err("invite.user_not_found", "user not found"));
                }
                let actor = me()?;
                let before = balance(uid)?.0;
                let (changed, after) = if b.delta > 0 {
                    let key = b.request_id.clone().unwrap_or_else(random::uuid_v7);
                    let (_, g, bal) = grant(&Grant {
                        uid,
                        amount: b.delta,
                        source: "admin",
                        source_ref: format!("admin:{key}"),
                        expires_ms: b.expires_at_ms,
                        reason: "admin",
                        actor: Some(actor),
                        note: Some(&b.reason),
                        max_balance: None,
                    })?;
                    (g, bal)
                } else {
                    let (d, bal) = deduct(uid, -b.delta, "deduct", None, "admin", Some(actor))?;
                    (-d, bal)
                };
                write_audit(
                    "invite.quota_adjust",
                    format!("user:{uid}"),
                    json!({ "before": before, "after": after, "delta": changed, "reason": b.reason }),
                )?;
                json!({ "user_id": uid, "changed": changed, "balance": after })
            }
            "admin_list_penalties" => {
                let st = statuses(&req);
                let target = query(&req, "target_user_id").and_then(|v| v.parse().ok());
                list_penalties(
                    "(cardinality($1::text[]) = 0 OR state = ANY($1)) AND ($2::bigint IS NULL OR target_user_id = $2)",
                    &[SqlValue::TextArray(st), oi(target)],
                )?
            }
            "admin_review_penalty" => {
                let pid = param(&req, "id")?;
                let b: ReviewBody = body(&req)?;
                let r = q(
                    "SELECT state, version FROM penalties WHERE id = $1 FOR UPDATE",
                    &[SqlValue::Int(pid)],
                )?;
                let row = r
                    .first()
                    .ok_or_else(|| err("kernel.not_found", "penalty not found"))?;
                if int(&row[1]) != b.version {
                    return Err(err("kernel.conflict", "penalty was modified; reload and retry"));
                }
                let state = text(&row[0]);
                match (b.decision.as_str(), state.as_str()) {
                    ("confirm", "pending_review") => execute_penalty(pid, "confirmed")?,
                    ("reverse", "applied" | "confirmed" | "pending_review") => {
                        reverse_penalty(pid, "review")?;
                    }
                    ("confirm" | "reverse", _) => {
                        return Err(err(
                            "invite.penalty_state_conflict",
                            &format!("penalty is {state}"),
                        ));
                    }
                    _ => return Err(invalid("decision must be confirm or reverse")),
                }
                x(
                    "UPDATE penalties SET reviewed_by = $2, reviewed_at = now(), review_note = $3 WHERE id = $1",
                    &[SqlValue::Int(pid), SqlValue::Int(me()?), ot(b.note.as_deref())],
                )?;
                write_audit(
                    "invite.penalty_review",
                    format!("penalty:{pid}"),
                    json!({ "decision": b.decision }),
                )?;
                list_penalties("id = $1", &[SqlValue::Int(pid)])?["items"][0].clone()
            }
            "admin_list_suspensions" => {
                let rows = q(
                    &format!(
                        "SELECT id, source, ref, reason, {}, {}, {} FROM suspensions WHERE user_id = $1 ORDER BY id DESC LIMIT 100",
                        ms("until"),
                        ms("created_at"),
                        ms("lifted_at")
                    ),
                    &[SqlValue::Int(param(&req, "id")?)],
                )?;
                json!({ "items": rows.iter().map(|r| json!({ "id": int(&r[0]), "source": text(&r[1]), "ref": text(&r[2]), "reason": otext(&r[3]),
                    "until": oiso(oint(&r[4])), "created_at": iso(int(&r[5])), "lifted_at": oiso(oint(&r[6])) })).collect::<Vec<_>>() })
            }
            "admin_add_suspension" => {
                let uid = param(&req, "id")?;
                let b: SuspendBody = body(&req)?;
                if b.reason.trim().is_empty() || b.reason.chars().count() > 500 {
                    return Err(invalid("reason is required"));
                }
                let actor = me()?;
                let r = q(
                    "INSERT INTO suspensions (user_id, source, ref, reason, until, created_by)
                     VALUES ($1, 'admin', $2, $3, CASE WHEN $4::bigint IS NULL THEN NULL ELSE to_timestamp($4::float8 / 1000) END, $5) RETURNING id",
                    &[SqlValue::Int(uid), t(&random::uuid_v7()), t(&b.reason), oi(b.until_ms), SqlValue::Int(actor)],
                )?;
                write_audit(
                    "invite.suspension_add",
                    format!("user:{uid}"),
                    json!({ "suspension_id": int(&r[0][0]), "until": b.until_ms }),
                )?;
                return Ok(resp(201, &json!({ "id": int(&r[0][0]) }), false));
            }
            "admin_lift_suspension" => {
                let (uid, sid) = (param(&req, "id")?, param(&req, "sid")?);
                let n = x(
                    "UPDATE suspensions SET lifted_at = now(), lifted_by = $3, lifted_reason = 'admin' WHERE id = $2 AND user_id = $1 AND lifted_at IS NULL",
                    &[SqlValue::Int(uid), SqlValue::Int(sid), SqlValue::Int(me()?)],
                )?;
                if n == 0 {
                    return Err(err("kernel.not_found", "suspension not found"));
                }
                write_audit(
                    "invite.suspension_lift",
                    format!("user:{uid}"),
                    json!({ "suspension_id": sid }),
                )?;
                return Ok(no_content());
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(resp(200, &v, false))
    }
}

// ---------------- ctx.invites（LLD §2.1） ----------------

fn scoped(scope: &str) -> R<()> {
    if invocation::current().caller_scopes.iter().any(|s| s == scope) {
        Ok(())
    } else {
        Err(err("invite.scope_denied", &format!("scope `{scope}` required")))
    }
}

/// 服务写方法幂等：`<caller>:<key>` → 首次结果
fn idem<F: FnOnce() -> R<Value>>(op: &str, key: Option<&str>, f: F) -> R<Value> {
    let Some(key) = key.filter(|k| (1..=128).contains(&k.len())) else {
        return f();
    };
    let k = format!("{}:{key}", invocation::current().caller_plugin);
    if let Some(r) = q("SELECT result::text FROM idempotency WHERE key = $1", &[t(&k)])?.first() {
        return Ok(serde_json::from_str(&text(&r[0])).unwrap_or(Value::Null));
    }
    let v = f()?;
    x(
        "INSERT INTO idempotency (key, op, result) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        &[t(&k), t(op), SqlValue::Json(v.to_string())],
    )?;
    Ok(v)
}

impl service_provider::Guest for Invite {
    fn call(service: String, method: String, args: String) -> R<String> {
        if service != "ctx.invites" {
            return Err(err("kernel.not_found", "unknown service"));
        }
        let a: Value =
            serde_json::from_str(&args).map_err(|_| err("kernel.bad_request", "invalid JSON arguments"))?;
        let user = a["user"].as_i64().or_else(|| a["user_id"].as_i64()).unwrap_or(0);
        let key = a["idempotency_key"].as_str();
        let caller = invocation::current().caller_plugin;
        let out = match method.as_str() {
            "validate" => validate(a["code"].as_str().unwrap_or(""))?,
            "quota" => quota_json(user, &settings())?,
            "tree" => tree(
                user,
                a["depth"].as_i64().unwrap_or(1),
                a["cursor"].as_str(),
                a["limit"].as_i64().unwrap_or(100),
            )?,
            "ancestors" => ancestors(user, a["max_depth"].as_i64().unwrap_or(10))?,
            "inviter_of" => {
                let r = q(
                    "SELECT inviter_id FROM tree_edges WHERE invitee_id = $1",
                    &[SqlValue::Int(user)],
                )?;
                json!(r.first().map(|r| int(&r[0])))
            }
            "stats" => stats(user)?,
            "issue" => {
                scoped("write:issue")?;
                let mut b: IssueBody =
                    serde_json::from_value(a.clone()).map_err(|e| invalid(&format!("issue request: {e}")))?;
                b.request_id = b.request_id.or_else(|| key.map(str::to_owned));
                let (v, _) = issue_system(format!("svc:{caller}"), None, Some(caller.clone()), "api", b)?;
                json!({ "invite_id": v["invite"]["id"], "code": v["code"], "url": v["url"], "expires_at": v["invite"]["expires_at"] })
            }
            "revoke" => {
                scoped("write:issue")?;
                let id = a["invite_id"].as_i64().ok_or_else(|| invalid("invite_id"))?;
                json!({ "revoked": revoke(id, &By::Plugin(caller.clone()), "service", a["refund"].as_bool())? })
            }
            "grant_quota" => {
                scoped("write:quota")?;
                let amount = a["amount"].as_i64().unwrap_or(0);
                let k = key.ok_or_else(|| invalid("idempotency_key is required"))?;
                let src_ref = format!("{caller}:{k}");
                idem("grant", Some(k), || {
                    let (lot, granted, bal) = grant(&Grant {
                        uid: user,
                        amount: amount.min(1000),
                        source: if caller == "shop" { "shop" } else { "service" },
                        source_ref: src_ref.clone(),
                        expires_ms: a["expires_at"].as_i64(),
                        reason: a["reason"].as_str().unwrap_or("service"),
                        actor: None,
                        note: None,
                        max_balance: None,
                    })?;
                    Ok(json!({ "lot_id": lot, "granted": granted, "balance": bal }))
                })?
            }
            "deduct_quota" => {
                scoped("write:quota")?;
                let k = key.ok_or_else(|| invalid("idempotency_key is required"))?;
                let first = a["ref_idempotency_key"].as_str().map(|r| format!("{caller}:{r}"));
                idem("deduct", Some(k), || {
                    let amount = a["amount"].as_i64().unwrap_or(0);
                    let (d, bal) = deduct(
                        user,
                        amount,
                        a["reason"].as_str().unwrap_or("deduct"),
                        first.as_deref(),
                        &format!("{caller}:{k}"),
                        None,
                    )?;
                    Ok(json!({ "deducted": d, "shortfall": amount - d, "balance": bal }))
                })?
            }
            _ => return Err(err("kernel.not_found", "unknown method")),
        };
        Ok(out.to_string())
    }
}

export!(Invite);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_format() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_759_654_800_123), "2025-10-05T09:00:00.123Z");
        assert_eq!(iso(951_782_400_000), "2000-02-29T00:00:00.000Z");
    }

    #[test]
    fn settings_defaults() {
        let s: Settings = serde_json::from_str(
            r#"{"class_rules": {"*": {"max_outstanding": 2}, "30": {"max_outstanding": 5}}}"#,
        )
        .unwrap_or_default();
        assert_eq!(s.class_rule(30).max_outstanding, 5);
        assert_eq!(s.class_rule(20).max_outstanding, 2);
        assert_eq!(s.default_expiry_days, 7);
        assert!(s.enabled);
    }
}
