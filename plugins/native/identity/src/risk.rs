//! 登录限流、设备、IP 记录、安全事件（identity 03 §1.9）。
//!
//! 偏差：没有 GeoIP（new_region / new_asn 不产生）；ip_log 不分区，保留期由 cron `ip_retention` 处理。

use pt_plugin_api::{Ctx, PResult, PluginError, RedisCommand, RedisReply, json};
use sqlx::Row;

use crate::{crypto, policy::Policy};

/// 登录限流的两个维度：规范化用户名（或邮箱）与客户端 IP 的 HMAC
pub struct ThrottleKeys {
    pub user: String,
    pub ip: Option<String>,
}

impl ThrottleKeys {
    pub fn new(keys: &crypto::Keyring, login: &str, ip: Option<&str>) -> Self {
        Self {
            user: hex::encode(keys.login_mac(&login.trim().to_lowercase())),
            ip: ip.map(|ip| hex::encode(keys.ip_mac(ip))),
        }
    }
}

fn throttled(retry_after: i64) -> PluginError {
    PluginError::new(
        "identity.login_throttled",
        "too many failed sign-in attempts, try again later",
    )
    .with_status(429)
    .with_details(json!({ "retry_after": retry_after.max(1) }))
}

async fn ttl(ctx: &Ctx, key: &str) -> PResult<i64> {
    Ok(ctx
        .redis
        .call(RedisCommand::new("TTL").arg(key))
        .await?
        .as_int()
        .unwrap_or(-2))
}

/// 登录前检查：IP 封锁、用户名软锁（已知设备不受软锁影响）、第 3 次失败起的指数延迟。
/// Redis 不可用时放行（不阻断登录），只记日志。
pub async fn throttle_check(ctx: &Ctx, p: &Policy, k: &ThrottleKeys, device_known: bool) -> PResult<()> {
    match check_inner(ctx, p, k, device_known).await {
        Ok(()) => Ok(()),
        Err(e) if e.code == "identity.login_throttled" => Err(e),
        Err(e) => {
            tracing::warn!(error = %e, "login throttle degraded");
            Ok(())
        }
    }
}

async fn check_inner(ctx: &Ctx, p: &Policy, k: &ThrottleKeys, device_known: bool) -> PResult<()> {
    if let Some(ip) = &k.ip {
        let t = ttl(ctx, &format!("lock:ip:{ip}")).await?;
        if t > 0 {
            return Err(throttled(t));
        }
    }
    if !device_known {
        let t = ttl(ctx, &format!("lock:u:{}", k.user)).await?;
        if t > 0 {
            return Err(throttled(t));
        }
    }
    let r = ctx
        .redis
        .pipeline(
            vec![
                RedisCommand::new("GET").arg(format!("lf:u:{}", k.user)),
                RedisCommand::new("GET").arg(format!("lft:u:{}", k.user)),
            ],
            false,
        )
        .await?;
    let n = r.first().and_then(RedisReply::as_int).unwrap_or(0);
    let last = r.get(1).and_then(RedisReply::as_int).unwrap_or(0);
    if n >= 3 && p.max_delay_secs > 0 {
        let delay = (1i64 << (n - 3).min(20)).min(p.max_delay_secs);
        let wait = last + delay * 1000 - ctx.time.now();
        if wait > 0 {
            return Err(throttled((wait + 999) / 1000));
        }
    }
    Ok(())
}

/// 记一次失败；返回本次是否触发了用户名软锁（每次上锁只返回一次 true）
pub async fn throttle_fail(ctx: &Ctx, p: &Policy, k: &ThrottleKeys) -> bool {
    match fail_inner(ctx, p, k).await {
        Ok(locked) => locked,
        Err(e) => {
            tracing::warn!(error = %e, "login throttle degraded");
            false
        }
    }
}

async fn fail_inner(ctx: &Ctx, p: &Policy, k: &ThrottleKeys) -> PResult<bool> {
    let lf = format!("lf:u:{}", k.user);
    let uw = p.user_window_secs.to_string();
    let mut cmds = vec![
        RedisCommand::new("INCR").arg(&lf),
        RedisCommand::new("EXPIRE").arg(&lf).arg(&uw).arg("NX"),
        RedisCommand::new("SET")
            .arg(format!("lft:u:{}", k.user))
            .arg(ctx.time.now().to_string())
            .arg("EX")
            .arg(&uw),
    ];
    if let Some(ip) = &k.ip {
        let key = format!("lf:ip:{ip}");
        cmds.push(RedisCommand::new("INCR").arg(&key));
        cmds.push(
            RedisCommand::new("EXPIRE")
                .arg(&key)
                .arg(p.ip_window_secs.to_string())
                .arg("NX"),
        );
    }
    let r = ctx.redis.pipeline(cmds, false).await?;
    let n_user = r.first().and_then(RedisReply::as_int).unwrap_or(0);
    let n_ip = r.get(3).and_then(RedisReply::as_int).unwrap_or(0);
    if let Some(ip) = &k.ip
        && n_ip >= p.ip_block_after
    {
        ctx.redis
            .call(
                RedisCommand::new("SET")
                    .arg(format!("lock:ip:{ip}"))
                    .arg("1")
                    .arg("EX")
                    .arg(p.ip_window_secs.to_string()),
            )
            .await?;
    }
    if n_user >= p.user_lock_after {
        let set = ctx
            .redis
            .call(
                RedisCommand::new("SET")
                    .arg(format!("lock:u:{}", k.user))
                    .arg("1")
                    .arg("EX")
                    .arg(&uw)
                    .arg("NX"),
            )
            .await?;
        return Ok(set == RedisReply::Ok);
    }
    Ok(false)
}

/// 登录成功：清除用户名维度的计数与软锁（IP 维度保留）
pub async fn throttle_success(ctx: &Ctx, k: &ThrottleKeys) {
    let r = ctx
        .redis
        .call(
            RedisCommand::new("DEL")
                .arg(format!("lf:u:{}", k.user))
                .arg(format!("lft:u:{}", k.user))
                .arg(format!("lock:u:{}", k.user)),
        )
        .await;
    if let Err(e) = r {
        tracing::warn!(error = %e, "login throttle degraded");
    }
}

// ---------------- 人机验证 ----------------

/// 经 `ctx.captcha.verify` 校验人机验证令牌（action：login / register / password_reset）。
/// 没有安装验证码插件时放行；插件按自己的设置决定该动作是否需要验证。
pub async fn captcha_check(ctx: &Ctx, action: &str, token: Option<&str>) -> PResult<()> {
    let mut args = json!({ "action": action, "token": token.unwrap_or("") });
    if let Some(ip) = &ctx.invocation.client_ip {
        args["ip"] = json!(ip);
    }
    match ctx.call_json("ctx.captcha", "verify", args).await {
        Ok(v) if v["ok"] == true => Ok(()),
        Ok(_) => Err(PluginError::new(
            "identity.captcha_failed",
            "human verification failed, please try again",
        )
        .with_status(400)),
        Err(e)
            if (e.code == "kernel.capability_denied" && e.message.contains("did not declare"))
                || e.message.contains("has no JSON-callable provider") =>
        {
            Ok(())
        }
        Err(e) => Err(e),
    }
}

// ---------------- 设备 ----------------

pub const DEVICE_COOKIE: &str = "__Host-pt_dev";
pub const DEVICE_MAX_AGE: i64 = 400 * 86_400;

/// 从 Cookie 头取设备 cookie（测试部署的无前缀形式也接受）
pub fn device_cookie(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == DEVICE_COOKIE || *k == DEVICE_COOKIE.trim_start_matches("__Host-"))
        .map(|(_, v)| v.to_owned())
        .filter(|v| !v.is_empty() && v.len() <= 64)
}

pub async fn device_known(ctx: &Ctx, keys: &crypto::Keyring, uid: i64, cookie: Option<&str>) -> PResult<bool> {
    let Some(c) = cookie else { return Ok(false) };
    let mut conn = ctx.sql.conn().await?;
    Ok(
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM devices WHERE user_id = $1 AND device_mac = $2)")
            .bind(uid)
            .bind(keys.device_mac(c))
            .fetch_one(&mut *conn)
            .await?,
    )
}

pub struct DeviceSeen {
    pub id: i64,
    /// 该用户此前登录过、但不是从这台设备
    pub new_device: bool,
    /// 需要下发的新设备 cookie
    pub set_cookie: Option<String>,
}

/// 登录成功后登记设备（没有设备 cookie 时生成一个）
pub async fn upsert_device(
    ctx: &Ctx,
    keys: &crypto::Keyring,
    uid: i64,
    cookie: Option<&str>,
) -> PResult<DeviceSeen> {
    let (value, set_cookie) = match cookie {
        Some(c) => (c.to_owned(), None),
        None => {
            let c = crypto::b64(&ctx.random.bytes(32));
            (c.clone(), Some(c))
        }
    };
    let ua: Option<String> = ctx
        .invocation
        .user_agent
        .as_ref()
        .map(|u| u.chars().take(128).collect());
    let mut conn = ctx.sql.conn().await?;
    let had_any: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM devices WHERE user_id = $1)")
        .bind(uid)
        .fetch_one(&mut *conn)
        .await?;
    let row = sqlx::query(
        "INSERT INTO devices (user_id, device_mac, ua_summary) VALUES ($1, $2, $3)
         ON CONFLICT (user_id, device_mac) DO UPDATE SET last_seen_at = now(), ua_summary = EXCLUDED.ua_summary
         RETURNING id, (xmax = 0) AS inserted",
    )
    .bind(uid)
    .bind(keys.device_mac(&value))
    .bind(ua)
    .fetch_one(&mut *conn)
    .await?;
    let inserted: bool = row.get(1);
    Ok(DeviceSeen {
        id: row.get(0),
        new_device: inserted && had_any,
        set_cookie,
    })
}

// ---------------- IP 记录与安全事件 ----------------

/// 记录用户从某 IP 访问（同月同来源合并计数）
pub async fn ip_log_upsert(ctx: &Ctx, keys: &crypto::Keyring, uid: i64, ip: &str, source: &str) -> PResult<()> {
    if ip.parse::<std::net::IpAddr>().is_err() {
        return Ok(());
    }
    let mut conn = ctx.sql.conn().await?;
    sqlx::query(
        "INSERT INTO ip_log (user_id, ip, ip_mac, source, first_seen_month)
         VALUES ($1, $2::inet, $3, $4, date_trunc('month', now())::date)
         ON CONFLICT (user_id, ip_mac, source, first_seen_month)
         DO UPDATE SET last_seen_at = now(), hits = ip_log.hits + 1",
    )
    .bind(uid)
    .bind(ip)
    .bind(keys.ip_mac(ip))
    .bind(source)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// 写一条安全事件；`details` 中不得含完整 IP（调用方用 [`crypto::truncate_ip`]）
pub async fn security_event(ctx: &Ctx, uid: i64, kind: &str, details: serde_json::Value) -> PResult<()> {
    let mut conn = ctx.sql.conn().await?;
    sqlx::query("INSERT INTO security_events (user_id, kind, details) VALUES ($1, $2, $3)")
        .bind(uid)
        .bind(kind)
        .bind(details)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// 当前请求的 IP、截断形式与 UA 摘要（安全事件 details）
pub fn client_details(ctx: &Ctx) -> serde_json::Value {
    let mut d = json!({});
    if let Some(ip) = &ctx.invocation.client_ip {
        d["ip_prefix"] = json!(crypto::truncate_ip(ip));
    }
    if let Some(ua) = &ctx.invocation.user_agent {
        d["ua"] = json!(ua.chars().take(128).collect::<String>());
    }
    d
}

pub async fn security_events_of(ctx: &Ctx, uid: i64, limit: i64) -> PResult<Vec<serde_json::Value>> {
    let mut conn = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT id, kind, details, created_at FROM security_events WHERE user_id = $1 ORDER BY id DESC LIMIT $2",
    )
    .bind(uid)
    .bind(limit.clamp(1, 200))
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            json!({ "id": r.get::<i64, _>(0), "kind": r.get::<String, _>(1), "details": r.get::<serde_json::Value, _>(2),
                    "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>(3).timestamp_millis() })
        })
        .collect())
}

/// 某用户的 IP 历史（按最近使用倒序）；`full = false` 时只给截断形式
pub async fn ip_history(ctx: &Ctx, uid: i64, full: bool, limit: i64) -> PResult<Vec<serde_json::Value>> {
    let mut conn = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT host(ip), masklen(ip), source, hits, truncated, first_seen_at, last_seen_at
           FROM ip_log WHERE user_id = $1 ORDER BY last_seen_at DESC LIMIT $2",
    )
    .bind(uid)
    .bind(limit.clamp(1, 500))
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let host: String = r.get(0);
            let truncated: bool = r.get(4);
            let ip = if truncated {
                format!("{host}/{}", r.get::<i32, _>(1))
            } else if full {
                host
            } else {
                crypto::truncate_ip(&host)
            };
            json!({ "ip": ip, "source": r.get::<String, _>(2), "hits": r.get::<i32, _>(3), "truncated": truncated,
                    "first_seen_at": r.get::<chrono::DateTime<chrono::Utc>, _>(5).timestamp_millis(),
                    "last_seen_at": r.get::<chrono::DateTime<chrono::Utc>, _>(6).timestamp_millis() })
        })
        .collect())
}

/// 解析管理员输入的 IP 或 CIDR；CIDR 前缀至少 IPv4 /16、IPv6 /32（防止一次扫出全站）
pub fn parse_ip_query(q: &str) -> PResult<(String, bool)> {
    let bad = || PluginError::validation("ip must be an address or a CIDR (IPv4 >= /16, IPv6 >= /32)");
    let q = q.trim();
    match q.split_once('/') {
        None => q.parse::<std::net::IpAddr>().map(|_| (q.to_owned(), false)).map_err(|_| bad()),
        Some((addr, bits)) => {
            let ip: std::net::IpAddr = addr.parse().map_err(|_| bad())?;
            let bits: u8 = bits.parse().map_err(|_| bad())?;
            let ok = match ip {
                std::net::IpAddr::V4(_) => (16..=32).contains(&bits),
                std::net::IpAddr::V6(_) => (32..=128).contains(&bits),
            };
            if !ok {
                return Err(bad());
            }
            Ok((format!("{addr}/{bits}"), true))
        }
    }
}

/// 用过某 IP（或落在某 CIDR 内）的账户
pub async fn users_by_ip(ctx: &Ctx, keys: &crypto::Keyring, q: &str) -> PResult<Vec<serde_json::Value>> {
    let (ip, cidr) = parse_ip_query(q)?;
    let mut conn = ctx.sql.conn().await?;
    // 单个地址同时按 HMAC 匹配：截断后的旧记录仍能命中
    let rows = sqlx::query(
        "SELECT l.user_id, u.username, u.status, max(l.last_seen_at), sum(l.hits)::bigint
           FROM ip_log l JOIN users u ON u.id = l.user_id
          WHERE CASE WHEN $3 THEN l.ip <<= $1::inet ELSE (l.ip_mac = $2 OR l.ip = $1::inet) END
          GROUP BY l.user_id, u.username, u.status
          ORDER BY max(l.last_seen_at) DESC LIMIT 200",
    )
    .bind(&ip)
    .bind(keys.ip_mac(&ip))
    .bind(cidr)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            json!({ "user_id": r.get::<i64, _>(0), "username": r.get::<String, _>(1), "status": r.get::<String, _>(2),
                    "last_seen_at": r.get::<chrono::DateTime<chrono::Utc>, _>(3).timestamp_millis(), "hits": r.get::<i64, _>(4) })
        })
        .collect())
}

/// 与某用户共用过 IP 的其他账户（按共用 IP 数倒序）
pub async fn shared_ip_users(ctx: &Ctx, uid: i64) -> PResult<Vec<serde_json::Value>> {
    let mut conn = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT o.user_id, u.username, u.status, count(DISTINCT o.ip_mac)::bigint, max(o.last_seen_at)
           FROM ip_log m JOIN ip_log o ON o.ip_mac = m.ip_mac AND o.user_id <> m.user_id
           JOIN users u ON u.id = o.user_id
          WHERE m.user_id = $1
          GROUP BY o.user_id, u.username, u.status
          ORDER BY 4 DESC, 5 DESC LIMIT 50",
    )
    .bind(uid)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            json!({ "user_id": r.get::<i64, _>(0), "username": r.get::<String, _>(1), "status": r.get::<String, _>(2),
                    "shared_ips": r.get::<i64, _>(3), "last_seen_at": r.get::<chrono::DateTime<chrono::Utc>, _>(4).timestamp_millis() })
        })
        .collect())
}

/// cron `ip_retention`：截断旧 IP、删除过期记录（每批提交，可重入）
pub async fn retention(ctx: &Ctx) -> PResult<()> {
    let p = Policy::load(ctx).await;
    loop {
        let n = {
            let mut conn = ctx.sql.conn().await?;
            sqlx::query(
                "UPDATE ip_log SET ip = network(set_masklen(ip, CASE family(ip) WHEN 4 THEN 24 ELSE 48 END))::inet, truncated = true
                  WHERE (user_id, ip_mac, source, first_seen_month) IN (
                        SELECT user_id, ip_mac, source, first_seen_month FROM ip_log
                         WHERE NOT truncated AND last_seen_at < now() - make_interval(days => $1::int) LIMIT 2000)",
            )
            .bind(i32::try_from(p.ip_truncate_after_days).unwrap_or(90))
            .execute(&mut *conn)
            .await?
            .rows_affected()
        };
        ctx.sql.commit().await?;
        if n == 0 {
            break;
        }
    }
    let days = i32::try_from(p.ip_delete_after_days).unwrap_or(365);
    for stmt in [
        "DELETE FROM ip_log WHERE ctid IN (SELECT ctid FROM ip_log WHERE last_seen_at < now() - make_interval(days => $1::int) LIMIT 5000)",
        "DELETE FROM security_events WHERE id IN (SELECT id FROM security_events WHERE created_at < now() - make_interval(days => $1::int) LIMIT 5000)",
    ] {
        loop {
            let n = {
                let mut conn = ctx.sql.conn().await?;
                sqlx::query(stmt).bind(days).execute(&mut *conn).await?.rows_affected()
            };
            ctx.sql.commit().await?;
            if n == 0 {
                break;
            }
        }
    }
    {
        let mut conn = ctx.sql.conn().await?;
        sqlx::query(
            "UPDATE sessions SET last_ip = network(set_masklen(last_ip, CASE family(last_ip) WHEN 4 THEN 24 ELSE 48 END))::inet
              WHERE last_ip IS NOT NULL AND masklen(last_ip) IN (32, 128) AND last_seen_at < now() - make_interval(days => $1::int)",
        )
        .bind(i32::try_from(p.ip_truncate_after_days).unwrap_or(90))
        .execute(&mut *conn)
        .await?;
        sqlx::query("DELETE FROM devices WHERE last_seen_at < now() - interval '400 days'")
            .execute(&mut *conn)
            .await?;
        sqlx::query("DELETE FROM challenges WHERE expires_at < now() - interval '7 days'")
            .execute(&mut *conn)
            .await?;
    }
    ctx.sql.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_queries() {
        assert_eq!(parse_ip_query(" 1.2.3.4 ").ok(), Some(("1.2.3.4".into(), false)));
        assert_eq!(parse_ip_query("1.2.0.0/16").ok(), Some(("1.2.0.0/16".into(), true)));
        assert!(parse_ip_query("1.0.0.0/8").is_err());
        assert!(parse_ip_query("2001:db8::/16").is_err());
        assert!(parse_ip_query("2001:db8::/48").is_ok());
        assert!(parse_ip_query("nope").is_err());
    }

    #[test]
    fn device_cookie_parsing() {
        let mut h = axum::http::HeaderMap::new();
        h.insert(
            axum::http::header::COOKIE,
            "a=b; __Host-pt_dev=abc; c=d".parse().unwrap_or_else(|_| unreachable!()),
        );
        assert_eq!(device_cookie(&h).as_deref(), Some("abc"));
        let mut h = axum::http::HeaderMap::new();
        h.insert(
            axum::http::header::COOKIE,
            "pt_dev=xyz".parse().unwrap_or_else(|_| unreachable!()),
        );
        assert_eq!(device_cookie(&h).as_deref(), Some("xyz"));
    }
}
