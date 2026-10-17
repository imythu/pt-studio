//! registration-policy 插件（WASM，docs/plugins/registration-policy/）：注册策略。
//!
//! 本版本实现：通道（开放 / 邀请；管理员与服务建号不受限）、开放注册开关与每周时段、限时开放窗口（可限名额）、
//! 全站用户上限、同 IP / 同网段注册上限（只用 identity 给出的 `ip_hmac` / `ip_prefix_hmac`，不接触原始 IP）、
//! 邮箱域名黑白名单（精确与后缀）、开放注册审核（`initial_status = pending`）、公开状态接口。
//! 名额占位：钩子中预留 10 分钟，收到 `user.registered` 后确认；同池并发经 `claim_locks` 行锁串行化。
//! 人机验证由 captcha 插件负责；问卷、申请制、设备上限在后续迭代实现。

pt_plugin_sdk::world!("registration-policy", http);

use exports::pt::plugin::{hooks, http_handler, on_cron, on_event};
use pt_plugin_sdk::{prelude::*, serde_json, sql, time};

struct Policy;

const RESERVE_SECS: i64 = 600;

// ---------------- 设置 ----------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub days: Vec<u8>,
    pub from: u32,
    pub to: u32,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub open_enabled: bool,
    pub schedule: Vec<Rule>,
    pub open_review: bool,
    pub invite_enabled: bool,
    pub max_users: i64,
    pub per_ip: (i64, i64),
    pub per_prefix: (i64, i64),
    pub deny: Vec<String>,
    pub allow: Vec<String>,
    pub allowlist_only: bool,
    pub offset_min: i64,
    pub closed_message: Option<String>,
}

/// "HH:MM" → 当天的分钟数
fn hm(s: &str) -> Option<u32> {
    let (h, m) = s.split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// 常见时区的固定偏移（分钟）；有夏令时的时区不在表中，需在设置中填偏移
fn tz_offset(tz: &str) -> i64 {
    match tz {
        "Asia/Shanghai" | "Asia/Chongqing" | "Asia/Hong_Kong" | "Asia/Macau" | "Asia/Taipei" | "Asia/Singapore"
        | "Asia/Kuala_Lumpur" | "Asia/Manila" => 480,
        "Asia/Tokyo" | "Asia/Seoul" => 540,
        "Asia/Kolkata" => 330,
        _ => 0,
    }
}

fn norm_domain(d: &str) -> String {
    let d = d.trim().to_ascii_lowercase();
    d.strip_prefix("*.").map_or(d.clone(), |rest| format!(".{rest}"))
}

impl Settings {
    pub fn from(s: &Value, site_tz: &str) -> Self {
        let int = |k: &str, d: i64| s[k].as_i64().unwrap_or(d).max(0);
        let list = |k: &str| -> Vec<String> {
            s[k].as_array()
                .map(|a| a.iter().filter_map(Value::as_str).map(norm_domain).filter(|d| !d.is_empty()).collect())
                .unwrap_or_default()
        };
        let schedule = s["schedule"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|r| {
                        Some(Rule {
                            days: r["days"]
                                .as_array()?
                                .iter()
                                .filter_map(|d| d.as_u64().and_then(|d| u8::try_from(d).ok()).filter(|d| *d < 7))
                                .collect(),
                            from: hm(r["from"].as_str()?)?,
                            to: hm(r["to"].as_str()?)?,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            open_enabled: s["open_enabled"].as_bool().unwrap_or(true),
            schedule,
            open_review: s["open_review"].as_bool().unwrap_or(false),
            invite_enabled: s["invite_enabled"].as_bool().unwrap_or(true),
            max_users: int("max_users", 0),
            per_ip: (int("per_ip_max", 3), int("per_ip_window_hours", 24).max(1)),
            per_prefix: (int("per_prefix_max", 0), int("per_prefix_window_hours", 24).max(1)),
            deny: list("email_domain_deny"),
            allow: list("email_domain_allow"),
            allowlist_only: s["email_allowlist_only"].as_bool().unwrap_or(false),
            offset_min: s["utc_offset_minutes"].as_i64().unwrap_or_else(|| tz_offset(site_tz)),
            closed_message: s["closed_message"].as_str().filter(|m| !m.trim().is_empty()).map(str::to_owned),
        }
    }

    pub fn load() -> Self {
        let st = ctx::static_config();
        Self::from(&ctx::settings(), st["_site"]["timezone"].as_str().unwrap_or("UTC"))
    }
}

// ---------------- 时段 ----------------

/// (星期 0 = 周日, 当天分钟) —— 本地时间
fn local(now_ms: i64, offset_min: i64) -> (u8, u32) {
    let secs = now_ms.div_euclid(1000) + offset_min * 60;
    let days = secs.div_euclid(86_400);
    // 1970-01-01 是周四
    let dow = u8::try_from((days + 4).rem_euclid(7)).unwrap_or(0);
    let minute = u32::try_from(secs.rem_euclid(86_400) / 60).unwrap_or(0);
    (dow, minute)
}

/// 规则命中（from > to 表示跨午夜，后半段属于前一天的规则）
pub fn in_schedule(rules: &[Rule], now_ms: i64, offset_min: i64) -> bool {
    if rules.is_empty() {
        return true;
    }
    let (d, t) = local(now_ms, offset_min);
    let prev = (d + 6) % 7;
    rules.iter().any(|r| {
        if r.from <= r.to {
            r.days.contains(&d) && r.from <= t && t < r.to
        } else {
            (r.days.contains(&d) && t >= r.from) || (r.days.contains(&prev) && t < r.to)
        }
    })
}

/// 下一次开放的开始时刻（Unix 毫秒），8 天内没有则 None
pub fn next_open(rules: &[Rule], now_ms: i64, offset_min: i64) -> Option<i64> {
    let (d, t) = local(now_ms, offset_min);
    let today_start = now_ms - i64::from(t) * 60_000 - (now_ms.rem_euclid(60_000));
    let mut best: Option<i64> = None;
    for k in 0..8u8 {
        let day = (d + k) % 7;
        for r in rules.iter().filter(|r| r.days.contains(&day)) {
            let at = today_start + i64::from(k) * 86_400_000 + i64::from(r.from) * 60_000;
            if at > now_ms && best.is_none_or(|b| at < b) {
                best = Some(at);
            }
        }
    }
    best
}

// ---------------- 邮箱域名 ----------------

fn matches(domain: &str, pattern: &str) -> bool {
    match pattern.strip_prefix('.') {
        Some(suffix) => domain.ends_with(&format!(".{suffix}")),
        None => domain == pattern,
    }
}

/// None = 允许；Some(原因) = 拒绝
pub fn domain_verdict(s: &Settings, domain: &str) -> Option<&'static str> {
    let domain = domain.to_ascii_lowercase();
    if s.deny.iter().any(|p| matches(&domain, p)) {
        return Some("denylisted");
    }
    if s.allowlist_only && !s.allow.iter().any(|p| matches(&domain, p)) {
        return Some("not_in_allowlist");
    }
    None
}

// ---------------- 状态 ----------------

struct Window {
    id: i64,
    slots: Option<i64>,
    ends_at: i64,
}

fn active_window() -> R<Option<Window>> {
    let r = sql::query(
        "SELECT id, slots, (extract(epoch FROM ends_at) * 1000)::bigint FROM windows
          WHERE cancelled_at IS NULL AND starts_at <= now() AND ends_at > now() ORDER BY ends_at LIMIT 1",
        &[],
    )?;
    Ok(r.rows.first().map(|row| Window {
        id: row.int(0).unwrap_or(0),
        slots: row.int(1),
        ends_at: row.int(2).unwrap_or(0),
    }))
}

/// 开放注册当前是否可用：(可用, 经由窗口)
fn open_now(s: &Settings, win: Option<&Window>) -> (bool, bool) {
    if s.open_enabled && in_schedule(&s.schedule, time::now(), s.offset_min) {
        return (true, false);
    }
    (win.is_some(), win.is_some())
}

fn window_used(id: i64) -> R<i64> {
    let r = sql::query(
        "SELECT count(*) FROM claims WHERE pool = $1 AND (state = 'confirmed' OR (state = 'reserved' AND reserved_until > now()))",
        &[SqlValue::Text(format!("window:{id}"))],
    )?;
    Ok(r.rows.first().and_then(|row| row.int(0)).unwrap_or(0))
}

fn status() -> R<Value> {
    let s = Settings::load();
    let win = active_window()?;
    let (open, via_window) = open_now(&s, win.as_ref());
    let next = if open {
        None
    } else {
        let mut n = if s.open_enabled { next_open(&s.schedule, time::now(), s.offset_min) } else { None };
        let w = sql::query(
            "SELECT (extract(epoch FROM min(starts_at)) * 1000)::bigint FROM windows WHERE cancelled_at IS NULL AND starts_at > now()",
            &[],
        )?;
        if let Some(ws) = w.rows.first().and_then(|r| r.int(0)) {
            n = Some(n.map_or(ws, |x| x.min(ws)));
        }
        n
    };
    let window = match (&win, via_window) {
        (Some(w), true) => {
            let left = w.slots.map(|n| (n - window_used(w.id).unwrap_or(0)).max(0));
            json!({ "ends_at": w.ends_at, "slots": w.slots, "slots_left": left })
        }
        _ => Value::Null,
    };
    Ok(json!({ "open": open, "invite": s.invite_enabled, "review": s.open_review && open,
               "next_open_at": next, "window": window, "message": s.closed_message }))
}

// ---------------- 名额占位 ----------------

struct Pool {
    key: String,
    cap: i64,
    /// IP 类池：确认记录的计数窗口（小时）；None = 永久（cap / window）
    hours: Option<i64>,
    gate: &'static str,
}

fn deny(code: &str, message: &str, details: &Value) -> hooks::HookDecision {
    hooks::HookDecision::Deny(hooks::DenyInfo {
        code: code.into(),
        message: message.into(),
        details: Some(details.to_string()),
    })
}

fn users_total() -> R<i64> {
    let r = sql::query("SELECT count(*) FROM identity.v_user_public WHERE status <> 'deleted'", &[])?;
    Ok(r.rows.first().and_then(|row| row.int(0)).unwrap_or(0))
}

/// 检查并预留全部池；返回拒绝的关卡
fn claim_all(mut pools: Vec<Pool>, holder: &str) -> R<Option<&'static str>> {
    pools.sort_by(|a, b| a.key.cmp(&b.key));
    for p in &pools {
        sql::execute(
            "INSERT INTO claim_locks (pool) VALUES ($1) ON CONFLICT (pool) DO UPDATE SET touched_at = now()",
            &[SqlValue::Text(p.key.clone())],
        )?;
    }
    for p in &pools {
        let others = sql::query(
            "SELECT count(*) FROM claims WHERE pool = $1 AND (
                 (state = 'confirmed' AND ($3::int IS NULL OR confirmed_at > now() - make_interval(hours => $3::int)))
              OR (state = 'reserved' AND reserved_until > now() AND holder IS DISTINCT FROM $2))",
            &[
                SqlValue::Text(p.key.clone()),
                SqlValue::Text(holder.into()),
                p.hours.map_or(SqlValue::Null, SqlValue::Int),
            ],
        )?
        .rows
        .first()
        .and_then(|r| r.int(0))
        .unwrap_or(0);
        // 全站上限按现有用户数 + 他人预留计算（确认记录不重复计）
        let used = if p.key == "cap" {
            let reserved = sql::query(
                "SELECT count(*) FROM claims WHERE pool = 'cap' AND state = 'reserved' AND reserved_until > now() AND holder IS DISTINCT FROM $1",
                &[SqlValue::Text(holder.into())],
            )?
            .rows
            .first()
            .and_then(|r| r.int(0))
            .unwrap_or(0);
            users_total()? + reserved
        } else {
            others
        };
        if used >= p.cap {
            return Ok(Some(p.gate));
        }
    }
    for p in &pools {
        sql::execute(
            "INSERT INTO claims (pool, holder, state, reserved_until) VALUES ($1, $2, 'reserved', now() + make_interval(secs => $3::int))
             ON CONFLICT (pool, holder) WHERE state = 'reserved' DO UPDATE SET reserved_until = EXCLUDED.reserved_until, updated_at = now()",
            &[SqlValue::Text(p.key.clone()), SqlValue::Text(holder.into()), SqlValue::Int(RESERVE_SECS)],
        )?;
    }
    Ok(None)
}

fn holder(username: &str) -> String {
    username.trim().to_lowercase()
}

fn on_before_register(input: &Value) -> R<hooks::HookDecision> {
    let ch = input["attributes"]["registration_mode"].as_str().unwrap_or("open");
    if !matches!(ch, "open" | "invite") {
        return Ok(hooks::HookDecision::Allow);
    }
    let s = Settings::load();
    let win = active_window()?;
    let mut via_window = false;
    if ch == "invite" {
        if !s.invite_enabled {
            return Ok(deny("registration-policy.closed", "invitations are not accepted right now", &json!({ "reason": "invite_disabled" })));
        }
    } else {
        let (open, w) = open_now(&s, win.as_ref());
        if !open {
            let next = if s.open_enabled { next_open(&s.schedule, time::now(), s.offset_min) } else { None };
            if s.invite_enabled {
                return Ok(deny(
                    "registration-policy.invite_required",
                    "registration is invite-only right now",
                    &json!({ "next_open_at": next }),
                ));
            }
            return Ok(deny("registration-policy.closed", "registration is closed", &json!({ "reason": "closed", "next_open_at": next })));
        }
        via_window = w;
    }
    if let Some(reason) = domain_verdict(&s, input["email_domain"].as_str().unwrap_or("")) {
        return Ok(deny(
            "registration-policy.email_domain_denied",
            "this email domain cannot be used to register",
            &json!({ "reason": reason, "errors": [{ "path": "/email", "code": "registration-policy.email_domain_denied" }] }),
        ));
    }
    let mut pools = Vec::new();
    if s.max_users > 0 {
        pools.push(Pool { key: "cap".into(), cap: s.max_users, hours: None, gate: "cap" });
    }
    if let Some(w) = win.as_ref().filter(|_| via_window)
        && let Some(n) = w.slots
    {
        pools.push(Pool { key: format!("window:{}", w.id), cap: n, hours: None, gate: "window" });
    }
    // 只用 identity 的盲化摘要；摘要缺失（未授予 user.ip 的部署）时不做 IP 限制
    if let (true, Some(h)) = (s.per_ip.0 > 0, input["ip_hmac"].as_str()) {
        pools.push(Pool { key: format!("ip:{}", &h[..h.len().min(32)]), cap: s.per_ip.0, hours: Some(s.per_ip.1), gate: "ip" });
    }
    if let (true, Some(h)) = (s.per_prefix.0 > 0, input["ip_prefix_hmac"].as_str()) {
        pools.push(Pool { key: format!("pfx:{}", &h[..h.len().min(32)]), cap: s.per_prefix.0, hours: Some(s.per_prefix.1), gate: "prefix" });
    }
    if !pools.is_empty()
        && let Some(gate) = claim_all(pools, &holder(input["username"].as_str().unwrap_or("")))?
    {
        return Ok(match gate {
            "cap" => deny("registration-policy.closed", "the site has reached its user limit", &json!({ "reason": "cap_reached" })),
            "window" => deny("registration-policy.closed", "all places in this registration round are taken", &json!({ "reason": "slots_exhausted" })),
            scope => deny(
                "registration-policy.ip_limit_reached",
                "too many accounts were registered from your network recently",
                &json!({ "scope": scope }),
            ),
        });
    }
    if ch == "open" && s.open_review {
        return Ok(hooks::HookDecision::Modify(json!({ "attributes": { "initial_status": "pending" } }).to_string()));
    }
    Ok(hooks::HookDecision::Allow)
}

impl hooks::Guest for Policy {
    fn handle(c: hooks::HookCall) -> R<hooks::HookDecision> {
        let input: Value = serde_json::from_str(&c.input).unwrap_or(Value::Null);
        match c.hook.as_str() {
            "user.before_register" => on_before_register(&input),
            _ => Ok(hooks::HookDecision::Allow),
        }
    }
}

/// 注册成功：确认该用户名的预留；IP 类占位不再关联用户
fn on_registered(p: &Value) -> R<()> {
    let h = holder(p["username"].as_str().unwrap_or(""));
    sql::execute(
        "UPDATE claims SET state = 'confirmed', confirmed_at = now(), updated_at = now(),
                user_id = CASE WHEN pool LIKE 'ip:%' OR pool LIKE 'pfx:%' THEN NULL ELSE $2 END,
                holder  = CASE WHEN pool LIKE 'ip:%' OR pool LIKE 'pfx:%' THEN NULL ELSE holder END
          WHERE holder = $1 AND state = 'reserved'",
        &[SqlValue::Text(h), p["user_id"].as_i64().map_or(SqlValue::Null, SqlValue::Int)],
    )?;
    Ok(())
}

impl on_event::Guest for Policy {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        match evt.event_type.as_str() {
            "user.registered" => on_registered(&p),
            _ => Ok(()),
        }
    }
}

impl on_cron::Guest for Policy {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        if job.name != "sweep" {
            return Err(err("kernel.not_found", "unknown job"));
        }
        let s = Settings::load();
        let keep_hours = s.per_ip.1.max(s.per_prefix.1);
        sql::execute(
            "UPDATE claims SET state = 'released', updated_at = now() WHERE state = 'reserved' AND reserved_until < now()",
            &[],
        )?;
        sql::execute("DELETE FROM claims WHERE state = 'released' AND updated_at < now() - interval '1 day'", &[])?;
        sql::execute(
            "DELETE FROM claims WHERE (pool LIKE 'ip:%' OR pool LIKE 'pfx:%') AND state = 'confirmed'
                AND confirmed_at < now() - make_interval(hours => $1::int)",
            &[SqlValue::Int(keep_hours)],
        )?;
        sql::execute("DELETE FROM claim_locks WHERE touched_at < now() - interval '30 days'", &[])?;
        Ok(())
    }
}

// ---------------- HTTP ----------------

fn list_windows() -> R<Value> {
    let r = sql::query(
        "SELECT id, (extract(epoch FROM starts_at) * 1000)::bigint, (extract(epoch FROM ends_at) * 1000)::bigint, slots, note,
                cancelled_at IS NOT NULL,
                (SELECT count(*) FROM claims c WHERE c.pool = 'window:' || w.id AND c.state = 'confirmed')
           FROM windows w WHERE ends_at > now() - interval '30 days' ORDER BY starts_at DESC LIMIT 100",
        &[],
    )?;
    let items: Vec<Value> = r
        .rows
        .iter()
        .map(|row| {
            json!({ "id": row.int(0), "starts_at": row.int(1), "ends_at": row.int(2), "slots": row.int(3),
                    "note": row.text(4), "cancelled": row.bool(5), "registered": row.int(6) })
        })
        .collect();
    Ok(json!({ "items": items }))
}

fn create_window(b: &Value) -> R<Value> {
    let (Some(starts), Some(ends)) = (b["starts_at"].as_i64(), b["ends_at"].as_i64()) else {
        return Err(err("kernel.validation_failed", "starts_at and ends_at (Unix ms) are required"));
    };
    if ends <= starts || ends <= time::now() {
        return Err(err("kernel.validation_failed", "the window must end after it starts and in the future"));
    }
    let slots = b["slots"].as_i64().filter(|n| *n > 0);
    let note = b["note"].as_str().map(|n| n.chars().take(200).collect::<String>());
    let overlap = sql::query(
        "SELECT 1 FROM windows WHERE cancelled_at IS NULL AND starts_at < to_timestamp($2::float8 / 1000) AND ends_at > to_timestamp($1::float8 / 1000)",
        &[SqlValue::Int(starts), SqlValue::Int(ends)],
    )?;
    if !overlap.rows.is_empty() {
        return Err(err("registration-policy.window_overlap", "the window overlaps another one"));
    }
    let r = sql::query(
        "INSERT INTO windows (starts_at, ends_at, slots, note, created_by)
         VALUES (to_timestamp($1::float8 / 1000), to_timestamp($2::float8 / 1000), $3, $4, $5) RETURNING id",
        &[
            SqlValue::Int(starts),
            SqlValue::Int(ends),
            slots.map_or(SqlValue::Null, SqlValue::Int),
            note.map_or(SqlValue::Null, SqlValue::Text),
            ctx::current_user_opt().map_or(SqlValue::Null, SqlValue::Int),
        ],
    )?;
    let id = r.rows.first().and_then(|row| row.int(0)).unwrap_or(0);
    pt_plugin_sdk::audit::write(&pt_plugin_sdk::audit::AuditEntry {
        action: "registration-policy.window_create".into(),
        target: format!("window:{id}"),
        details: json!({ "starts_at": starts, "ends_at": ends, "slots": slots }).to_string(),
    })?;
    Ok(json!({ "id": id }))
}

fn cancel_window(id: i64) -> R<()> {
    let n = sql::execute(
        "UPDATE windows SET cancelled_at = now() WHERE id = $1 AND cancelled_at IS NULL",
        &[SqlValue::Int(id)],
    )?;
    if n == 0 {
        return Err(err("kernel.not_found", "window not found"));
    }
    pt_plugin_sdk::audit::write(&pt_plugin_sdk::audit::AuditEntry {
        action: "registration-policy.window_cancel".into(),
        target: format!("window:{id}"),
        details: "{}".into(),
    })?;
    Ok(())
}

impl http_handler::Guest for Policy {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let reply = match req.route() {
            "status" => http::Reply::ok(&status()?),
            "list_windows" => http::Reply::ok(&list_windows()?),
            "create_window" => http::Reply::created(&create_window(&req.body_json::<Value>()?)?),
            "cancel_window" => {
                cancel_window(req.param_i64("id")?)?;
                http::Reply::no_content()
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(Policy);

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(days: &[u8], from: &str, to: &str) -> Rule {
        Rule { days: days.to_vec(), from: hm(from).unwrap_or(0), to: hm(to).unwrap_or(0) }
    }

    // 2026-10-17 是周六；12:00 UTC = 20:00 北京时间
    const SAT_NOON_UTC: i64 = 1_792_238_400_000;

    #[test]
    fn schedules() {
        assert_eq!(local(SAT_NOON_UTC, 0), (6, 720));
        assert_eq!(local(SAT_NOON_UTC, 480), (6, 1200));
        let evening = [rule(&[6], "19:00", "23:00")];
        assert!(in_schedule(&evening, SAT_NOON_UTC, 480));
        assert!(!in_schedule(&evening, SAT_NOON_UTC, 0));
        // 跨午夜：周五 22:00 – 02:00，周六 01:00 命中
        let night = [rule(&[5], "22:00", "02:00")];
        assert!(in_schedule(&night, SAT_NOON_UTC - 11 * 3_600_000, 0));
        assert!(!in_schedule(&night, SAT_NOON_UTC, 0));
        assert!(in_schedule(&[], SAT_NOON_UTC, 0));
        // 下次开放：UTC 周六 12:00 之后，周日 09:00
        let sun = [rule(&[0], "09:00", "10:00")];
        assert_eq!(next_open(&sun, SAT_NOON_UTC, 0), Some(SAT_NOON_UTC + 21 * 3_600_000));
    }

    #[test]
    fn domains() {
        let s = Settings::from(
            &json!({ "email_domain_deny": ["mailinator.com", "*.tempmail.io"], "email_domain_allow": [".edu.cn", "gmail.com"] }),
            "UTC",
        );
        assert_eq!(domain_verdict(&s, "Mailinator.com"), Some("denylisted"));
        assert_eq!(domain_verdict(&s, "x.tempmail.io"), Some("denylisted"));
        assert_eq!(domain_verdict(&s, "tempmail.io"), None, "a suffix rule does not match the bare domain");
        let mut s = s;
        s.allowlist_only = true;
        assert_eq!(domain_verdict(&s, "pku.edu.cn"), None);
        assert_eq!(domain_verdict(&s, "outlook.com"), Some("not_in_allowlist"));
        assert_eq!(Settings::from(&json!({}), "Asia/Shanghai").offset_min, 480);
    }
}
