//! 宿主操作的便捷封装：SQL、服务调用、事件、审计、指标、设置、令牌加密。

use pt_plugin_sdk::{
    PluginError, R, RowExt, audit, config, err, kv, metrics, random,
    serde_json::{Value, json},
    services,
    sql::{self, SqlValue},
    time,
};
use sha2::{Digest, Sha256};

pub type Row = Vec<SqlValue>;

pub fn q(stmt: &str, params: &[SqlValue]) -> R<Vec<Row>> {
    Ok(sql::query(stmt, params)?.rows)
}

pub fn q1(stmt: &str, params: &[SqlValue]) -> R<Option<Row>> {
    Ok(sql::query(stmt, params)?.rows.into_iter().next())
}

pub fn x(stmt: &str, params: &[SqlValue]) -> R<u64> {
    sql::execute(stmt, params)
}

pub fn commit() -> R<()> {
    sql::commit()
}

pub fn i(v: i64) -> SqlValue {
    SqlValue::Int(v)
}

pub fn t(v: &str) -> SqlValue {
    SqlValue::Text(v.to_owned())
}

pub fn oi(v: Option<i64>) -> SqlValue {
    v.map_or(SqlValue::Null, SqlValue::Int)
}

pub fn ot(v: Option<&str>) -> SqlValue {
    v.map_or(SqlValue::Null, |s| SqlValue::Text(s.to_owned()))
}

pub fn j(v: &Value) -> SqlValue {
    SqlValue::Json(v.to_string())
}

pub fn ints(v: Vec<i64>) -> SqlValue {
    SqlValue::IntArray(v)
}

pub fn texts(v: Vec<String>) -> SqlValue {
    SqlValue::TextArray(v)
}

/// 行读取（缺失 / NULL 时给默认值）
pub trait RowGet {
    fn n(&self, i: usize) -> i64;
    fn on(&self, i: usize) -> Option<i64>;
    fn s(&self, i: usize) -> String;
    fn os(&self, i: usize) -> Option<String>;
    fn b(&self, i: usize) -> bool;
    fn v(&self, i: usize) -> Value;
    fn arr(&self, i: usize) -> Vec<String>;
}

impl RowGet for Row {
    fn n(&self, i: usize) -> i64 {
        self.int(i)
            .or_else(|| self.float(i).map(|f| f as i64))
            .unwrap_or(0)
    }
    fn on(&self, i: usize) -> Option<i64> {
        self.int(i)
    }
    fn s(&self, i: usize) -> String {
        self.text(i).unwrap_or_default()
    }
    fn os(&self, i: usize) -> Option<String> {
        self.text(i)
    }
    fn b(&self, i: usize) -> bool {
        self.bool(i).unwrap_or(false)
    }
    fn v(&self, i: usize) -> Value {
        self.json(i).unwrap_or(Value::Null)
    }
    fn arr(&self, i: usize) -> Vec<String> {
        match self.get(i) {
            Some(SqlValue::TextArray(a)) => a.clone(),
            _ => vec![],
        }
    }
}

/// `timestamptz` 列 → Unix 毫秒的 SQL 表达式
pub fn ms(col: &str) -> String {
    format!("(extract(epoch FROM {col}) * 1000)::bigint")
}

/// 参数（Unix 毫秒）→ `timestamptz` 的 SQL 表达式
pub fn ts(n: usize) -> String {
    format!("to_timestamp(${n}::float8 / 1000)")
}

pub fn now() -> i64 {
    time::now()
}

pub const DAY: i64 = 86_400_000;

/// RFC 3339（UTC，毫秒）
pub fn iso(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let milli = ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    // 公历日期（Howard Hinnant 算法）
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}.{milli:03}Z")
}

pub fn iso_opt(ms: Option<i64>) -> Value {
    ms.map_or(Value::Null, |m| json!(iso(m)))
}

/// 调用业务服务
pub fn call(service: &str, method: &str, args: &Value) -> R<Value> {
    let out = services::call(service, method, &args.to_string())?;
    serde_json::from_str(&out).map_err(|_| err("anticheat.bad_service_reply", "invalid JSON from service"))
}

/// 外部错误是否可重试
pub fn retryable(e: &PluginError) -> bool {
    e.retryable
        || matches!(
            e.code.as_str(),
            "kernel.timeout" | "kernel.service_unavailable" | "kernel.resource_exhausted" | "kernel.internal"
        )
}

pub fn publish(event_type: &str, user: i64, payload: &Value) -> R<()> {
    pt_plugin_sdk::ctx::publish(event_type, &format!("user:{user}"), payload)?;
    Ok(())
}

pub fn audit(action: &str, target: &str, details: &Value) -> R<()> {
    audit::write(&audit::AuditEntry {
        action: action.into(),
        target: target.into(),
        details: details.to_string(),
    })
}

pub fn counter(name: &str, labels: &[(&str, &str)]) {
    metrics::counter_add(
        name,
        1,
        &labels
            .iter()
            .map(|(k, v)| metrics::Label {
                key: (*k).into(),
                value: (*v).into(),
            })
            .collect::<Vec<_>>(),
    );
}

pub fn gauge(name: &str, value: f64, labels: &[(&str, &str)]) {
    metrics::gauge_set(
        name,
        value,
        &labels
            .iter()
            .map(|(k, v)| metrics::Label {
                key: (*k).into(),
                value: (*v).into(),
            })
            .collect::<Vec<_>>(),
    );
}

pub fn bad(code: &str, msg: &str) -> PluginError {
    err(code, msg)
}

pub fn validation(msg: &str) -> PluginError {
    err("kernel.validation_failed", msg)
}

/// 文本输入：去控制字符（保留换行），长度 1..=max
pub fn clean_text(s: &str, max: usize, keep_newlines: bool) -> R<String> {
    let c: String = s
        .chars()
        .filter(|ch| !ch.is_control() || (keep_newlines && (*ch == '\n' || *ch == '\t')))
        .collect();
    let c = c.trim().to_owned();
    if c.is_empty() || c.chars().count() > max {
        return Err(validation(&format!("text must be 1..={max} characters")));
    }
    Ok(c)
}

// ---------------- 设置（LLD §4；后台设置为扁平键） ----------------

#[derive(Debug, Clone)]
pub struct Settings {
    pub auto_actions_enabled: bool,
    pub auto_ban_enabled: bool,
    pub four_eyes_for_ban: bool,
    pub four_eyes_for_appeal: bool,
    pub protected_min_role: String,
    pub breaker_max_auto_per_hour: i64,
    pub breaker_tripped: bool,
    pub claim_ttl_secs: i64,
    pub review_sla_hours: i64,
    pub warn_default_days: i64,
    pub freeze_default_days: i64,
    pub freeze_down_permille: Option<i64>,
    pub appeal_window_days: i64,
    pub appeal_max_per_case: i64,
    pub appeal_token_ttl_days: i64,
    pub appeal_suspend_sanctions: bool,
    pub spike_min_bytes: i64,
    pub spike_factor: i64,
    pub enrich_top_torrents: i64,
    pub hooks_block_operations: Vec<String>,
    pub hooks_block_checkin_when_frozen: bool,
    pub require_simulation_before_activate: bool,
    pub stats_min_precision: i64,
    pub retention_case_months: i64,
    pub user_view_show_torrents: bool,
    pub llm_profile: String,
}

pub fn settings() -> Settings {
    let v: Value = serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null);
    let b = |k: &str, d: bool| v.get(k).and_then(Value::as_bool).unwrap_or(d);
    let n = |k: &str, d: i64, lo: i64, hi: i64| v.get(k).and_then(Value::as_i64).unwrap_or(d).clamp(lo, hi);
    let s = |k: &str, d: &str| v.get(k).and_then(Value::as_str).unwrap_or(d).to_owned();
    Settings {
        auto_actions_enabled: b("auto_actions_enabled", true),
        auto_ban_enabled: b("auto_ban_enabled", false),
        four_eyes_for_ban: b("four_eyes_for_ban", true),
        four_eyes_for_appeal: b("four_eyes_for_appeal", true),
        protected_min_role: s("protected_min_role", "moderator"),
        breaker_max_auto_per_hour: n("breaker_max_auto_per_hour", 50, 1, 10_000),
        breaker_tripped: b("breaker_tripped", false),
        claim_ttl_secs: n("claim_ttl_secs", 1800, 300, 14_400),
        review_sla_hours: n("review_sla_hours", 72, 1, 720),
        warn_default_days: n("warn_default_days", 14, 1, 365),
        freeze_default_days: n("freeze_default_days", 7, 1, 365),
        freeze_down_permille: v
            .get("freeze_down_permille")
            .and_then(Value::as_i64)
            .map(|x| x.clamp(0, 10_000)),
        appeal_window_days: n("appeal_window_days", 14, 1, 90),
        appeal_max_per_case: n("appeal_max_per_case", 1, 1, 5),
        appeal_token_ttl_days: n("appeal_token_ttl_days", 30, 1, 90),
        appeal_suspend_sanctions: b("appeal_suspend_sanctions", false),
        spike_min_bytes: n("spike_min_bytes", 536_870_912_000, 1 << 30, i64::MAX),
        spike_factor: n("spike_factor", 5, 2, 100),
        enrich_top_torrents: n("enrich_top_torrents", 5, 0, 20),
        hooks_block_operations: v
            .get("hooks_block_operations")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_else(|| vec!["transfer".into()]),
        hooks_block_checkin_when_frozen: b("hooks_block_checkin_when_frozen", false),
        require_simulation_before_activate: b("require_simulation_before_activate", true),
        stats_min_precision: n("stats_min_precision", 70, 0, 100),
        retention_case_months: n("retention_case_months", 36, 12, 120),
        user_view_show_torrents: b("user_view_show_torrents", true),
        llm_profile: s("llm_profile", "default"),
    }
}

pub fn role_rank(r: &str) -> i64 {
    match r {
        "guest" => 0,
        "user" => 1,
        "uploader" => 2,
        "moderator" => 3,
        "admin" => 4,
        "sysop" => 5,
        _ => -1,
    }
}

// ---------------- 加密（申诉令牌） ----------------

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn hmac(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&sha256(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(msg);
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// base64url（无填充）
pub fn b64url(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len() * 4 / 3 + 2);
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        let take = c.len() + 1;
        for k in 0..take {
            out.push(A[((n >> (18 - 6 * k)) & 63) as usize] as char);
        }
    }
    out
}

/// 申诉令牌密钥：site.toml `appeal_token_secret`（≥ 32 字节）；未配置时使用插件 kv 中生成的随机密钥
pub fn token_secret() -> R<Vec<u8>> {
    let st: Value = serde_json::from_str(&config::get_static()).unwrap_or(Value::Null);
    if let Some(s) = st
        .get("appeal_token_secret")
        .and_then(Value::as_str)
        .filter(|s| s.len() >= 32)
    {
        return Ok(s.as_bytes().to_vec());
    }
    if let Some(v) = kv::get("token_secret")? {
        return Ok(v);
    }
    let fresh = random::bytes(32);
    if kv::compare_and_set("token_secret", None, &fresh)? {
        return Ok(fresh);
    }
    kv::get("token_secret")?.ok_or_else(|| err("kernel.internal", "token secret unavailable"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_and_b64() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_791_981_502_123), "2026-10-14T12:38:22.123Z");
        assert_eq!(iso(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(b64url(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64url(b"fooba"), "Zm9vYmE");
        assert_eq!(b64url(&[0xfb, 0xff]), "-_8");
        assert_eq!(b64url(&[0u8; 32]).len(), 43);
    }

    #[test]
    fn hmac_rfc4231_case2() {
        assert_eq!(
            hex(&hmac(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }
}
