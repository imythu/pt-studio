//! 网关限流（kernel/03 §8.3，GCRA）与幂等键（§8.4）。

use pt_plugin_api::{PResult, PluginError, json};
use redis::{Script, aio::ConnectionManager};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    Ip,
    User,
    Token,
    Global,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub subject: Subject,
    pub count: u32,
    pub period_ms: u64,
    /// 原文（`ip:30/m`），用于 RateLimit-Policy 头
    pub text: String,
}

/// `ip:30/m,user:120/m` → 规则列表。
pub fn parse(spec: &str) -> Result<Vec<Rule>, String> {
    spec.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|part| {
            let (subj, rest) = part
                .split_once(':')
                .ok_or_else(|| format!("bad rate rule `{part}`"))?;
            let (count, period) = rest
                .split_once('/')
                .ok_or_else(|| format!("bad rate rule `{part}`"))?;
            let subject = match subj {
                "ip" => Subject::Ip,
                "user" => Subject::User,
                "token" => Subject::Token,
                "global" => Subject::Global,
                _ => return Err(format!("unknown subject in `{part}`")),
            };
            let count: u32 = count
                .parse()
                .ok()
                .filter(|c| *c > 0)
                .ok_or_else(|| format!("bad count in `{part}`"))?;
            // 周期：s / m / h / d，可带倍数前缀（如 5m）
            let (mult, unit) = period.split_at(period.len().saturating_sub(1));
            let mult: u64 = if mult.is_empty() {
                1
            } else {
                mult.parse().map_err(|_| format!("bad period in `{part}`"))?
            };
            let unit_ms = match unit {
                "s" => 1_000,
                "m" => 60_000,
                "h" => 3_600_000,
                "d" => 86_400_000,
                _ => return Err(format!("bad period in `{part}`")),
            };
            Ok(Rule {
                subject,
                count,
                period_ms: mult * unit_ms,
                text: part.to_owned(),
            })
        })
        .collect()
}

/// GCRA：返回 {允许(1/0), 剩余, retry_after_ms, reset_ms}
const GCRA: &str = r"
local t = redis.call('TIME')
local now = tonumber(t[1]) * 1000 + math.floor(tonumber(t[2]) / 1000)
local interval = tonumber(ARGV[1])
local burst = tonumber(ARGV[2])
local tat = tonumber(redis.call('GET', KEYS[1]) or now)
if tat < now then tat = now end
local new_tat = tat + interval
local limit = interval * burst
if new_tat - now > limit then
  return {0, 0, new_tat - now - limit, tat - now}
end
redis.call('SET', KEYS[1], new_tat, 'PX', new_tat - now)
return {1, math.floor((limit - (new_tat - now)) / interval), 0, new_tat - now}
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub policy: String,
    pub remaining: i64,
    pub reset_secs: i64,
}

/// 依次检查全部规则；任一超限 → `kernel.rate_limited`（429，details.retry_after）。
/// `ids`：(ip, user, token) 标识；匿名请求的 user 规则退化为 ip。
pub async fn check(
    redis: &ConnectionManager,
    scope: &str,
    rules: &[Rule],
    ip: Option<&str>,
    user: Option<u64>,
    token: Option<&str>,
) -> PResult<Option<Verdict>> {
    let mut tightest: Option<Verdict> = None;
    let mut c = redis.clone();
    let script = Script::new(GCRA);
    for r in rules {
        let who = match r.subject {
            Subject::Ip => ip.map(|i| format!("ip:{i}")),
            Subject::User => user
                .map(|u| format!("u:{u}"))
                .or_else(|| ip.map(|i| format!("ip:{i}"))),
            Subject::Token => token
                .map(|t| format!("t:{t}"))
                .or_else(|| ip.map(|i| format!("ip:{i}"))),
            Subject::Global => Some("g".into()),
        };
        let Some(who) = who else { continue };
        let key = format!("k:rl:{scope}:{}:{who}", r.text);
        let interval = (r.period_ms / u64::from(r.count)).max(1);
        let out: Vec<i64> = match script
            .key(&key)
            .arg(interval)
            .arg(r.count)
            .invoke_async(&mut c)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                // 限流后端不可用时放行（可用性优先），记录告警
                tracing::warn!(error = %e, "rate limiter unavailable");
                continue;
            }
        };
        let (allowed, remaining, retry_ms, reset_ms) = (
            out.first() == Some(&1),
            out.get(1).copied().unwrap_or(0),
            out.get(2).copied().unwrap_or(0),
            out.get(3).copied().unwrap_or(0),
        );
        if !allowed {
            let retry = (retry_ms + 999) / 1000;
            return Err(PluginError::new("kernel.rate_limited", "too many requests")
                .with_details(json!({ "retry_after": retry.max(1), "policy": r.text }))
                .retryable());
        }
        let v = Verdict {
            policy: r.text.clone(),
            remaining,
            reset_secs: (reset_ms + 999) / 1000,
        };
        if tightest.as_ref().is_none_or(|t| v.remaining < t.remaining) {
            tightest = Some(v);
        }
    }
    Ok(tightest)
}

// ---------------- 幂等键（§8.4） ----------------

pub enum IdemState {
    /// 首次：本请求负责执行并写入结果
    Fresh,
    /// 已完成：原样返回
    Done {
        status: u16,
        content_type: String,
        body: Vec<u8>,
    },
}

const TTL_SECS: u64 = 86_400;
pub const MAX_STORED_BODY: usize = 64 * 1024;

pub fn valid_key(k: &str) -> bool {
    (1..=64).contains(&k.len()) && k.bytes().all(|c| (0x21..=0x7e).contains(&c))
}

/// HMGET state fp status ct body 的结果
type IdemRow = (
    Option<String>,
    Option<String>,
    Option<u16>,
    Option<String>,
    Option<Vec<u8>>,
);

pub async fn begin(redis: &ConnectionManager, key: &str, fp: &str) -> PResult<IdemState> {
    let mut c = redis.clone();
    let created: i64 = redis::cmd("HSETNX")
        .arg(key)
        .arg("state")
        .arg("processing")
        .query_async(&mut c)
        .await?;
    if created == 1 {
        let _: () = redis::pipe()
            .cmd("HSET")
            .arg(key)
            .arg("fp")
            .arg(fp)
            .ignore()
            .cmd("EXPIRE")
            .arg(key)
            .arg(TTL_SECS)
            .ignore()
            .query_async(&mut c)
            .await?;
        return Ok(IdemState::Fresh);
    }
    let (state, old_fp, status, ct, body): IdemRow = redis::cmd("HMGET")
        .arg(key)
        .arg("state")
        .arg("fp")
        .arg("status")
        .arg("ct")
        .arg("body")
        .query_async(&mut c)
        .await?;
    if old_fp.as_deref().is_some_and(|f| f != fp) {
        return Err(PluginError::new(
            "kernel.idempotency_key_reused",
            "idempotency key was used for a different request",
        ));
    }
    match state.as_deref() {
        Some("done") => Ok(IdemState::Done {
            status: status.unwrap_or(200),
            content_type: ct.unwrap_or_else(|| "application/json".into()),
            body: body.unwrap_or_default(),
        }),
        _ => Err(PluginError::new(
            "kernel.idempotency_in_progress",
            "a request with this key is in progress",
        )
        .retryable()),
    }
}

pub async fn complete(redis: &ConnectionManager, key: &str, status: u16, content_type: &str, body: &[u8]) {
    let mut c = redis.clone();
    let body = if body.len() <= MAX_STORED_BODY { body } else { &[] };
    let r: redis::RedisResult<()> = redis::pipe()
        .cmd("HSET")
        .arg(key)
        .arg("state")
        .arg("done")
        .arg("status")
        .arg(status)
        .arg("ct")
        .arg(content_type)
        .arg("body")
        .arg(body)
        .ignore()
        .cmd("EXPIRE")
        .arg(key)
        .arg(TTL_SECS)
        .ignore()
        .query_async(&mut c)
        .await;
    if let Err(e) = r {
        tracing::warn!(error = %e, "failed to store idempotent response");
    }
}

/// 服务端错误：删除记录，允许客户端以同一键重试。
pub async fn abandon(redis: &ConnectionManager, key: &str) {
    let mut c = redis.clone();
    let _: redis::RedisResult<i64> = redis::cmd("DEL").arg(key).query_async(&mut c).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rules() {
        let r = parse("ip:30/m, user:120/m,user:1/5m,global:10/s").unwrap_or_default();
        assert_eq!(r.len(), 4);
        assert_eq!(
            (r[0].subject, r[0].count, r[0].period_ms),
            (Subject::Ip, 30, 60_000)
        );
        assert_eq!(r[2].period_ms, 300_000);
        assert!(parse("ip:0/m").is_err());
        assert!(parse("cookie:1/m").is_err());
        assert!(parse("ip:1/x").is_err());
        assert!(valid_key("01928c3e-7a10") && !valid_key("") && !valid_key("a b"));
    }
}
