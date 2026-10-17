//! 两步验证（identity 03 §1.3、§1.4）：TOTP（RFC 6238，SHA-1 / 6 位 / 30 秒，防重放）、恢复码、登录第二步票据、近期重新验证。
//!
//! 偏差：强制 2FA 按“最低角色”配置（设计为角色列表），默认不强制。

use chrono::Utc;
use data_encoding::BASE32_NOPAD;
use hmac::{Hmac, Mac};
use pt_plugin_api::{Ctx, NewEvent, PResult, PluginError, RedisCommand, RedisReply, json};
use serde_json::Value;
use sha1::Sha1;
use sqlx::Row;

use crate::{
    Identity,
    auth::{self, LoginTarget, NewSession},
    crypto,
    policy::Policy,
    rbac, risk,
};

const STEP_SECS: i64 = 30;
const RECOVERY_CODES: usize = 10;
/// 恢复码字母表：去掉易混淆的 0 / o / 1 / l / i
const RECOVERY_ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
const TICKET_TTL_SECS: i64 = 300;
const TICKET_MAX_ATTEMPTS: i64 = 5;

/// RFC 4226 HOTP（6 位）
pub fn hotp(secret: &[u8], counter: u64) -> u32 {
    let Ok(mut mac) = <Hmac<Sha1> as Mac>::new_from_slice(secret) else {
        return u32::MAX;
    };
    mac.update(&counter.to_be_bytes());
    let h = mac.finalize().into_bytes();
    let off = usize::from(h[19] & 0x0f);
    let bin = (u32::from(h[off]) & 0x7f) << 24
        | u32::from(h[off + 1]) << 16
        | u32::from(h[off + 2]) << 8
        | u32::from(h[off + 3]);
    bin % 1_000_000
}

/// base32 密钥在某 Unix 秒对应的 6 位码（测试与运维工具用）
pub fn totp_at(secret_b32: &str, unix_secs: i64) -> Option<String> {
    let secret = BASE32_NOPAD.decode(secret_b32.trim_end_matches('=').as_bytes()).ok()?;
    Some(format!("{:06}", hotp(&secret, u64::try_from(unix_secs / STEP_SECS).ok()?)))
}

fn current_step(ctx: &Ctx) -> i64 {
    ctx.time.now() / 1000 / STEP_SECS
}

fn uid_of(ctx: &Ctx) -> PResult<i64> {
    i64::try_from(ctx.require_user()?).map_err(|_| PluginError::bad_request("bad user id"))
}

fn session_of(ctx: &Ctx) -> PResult<uuid::Uuid> {
    ctx.actor()
        .credential_id
        .as_deref()
        .and_then(|c| uuid::Uuid::parse_str(c).ok())
        .ok_or_else(|| PluginError::forbidden("only browser sessions can do this"))
}

fn nonce(ctx: &Ctx) -> [u8; 12] {
    let b = ctx.random.bytes(12);
    let mut n = [0u8; 12];
    n.copy_from_slice(&b[..12]);
    n
}

fn normalize_code(code: &str) -> String {
    code.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .flat_map(char::to_lowercase)
        .collect()
}

/// 用户是否开启了 TOTP（已确认）
pub async fn has_totp(ctx: &Ctx, uid: i64) -> PResult<bool> {
    let mut conn = ctx.sql.conn().await?;
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM totp_secrets WHERE user_id = $1 AND confirmed_at IS NOT NULL)",
    )
    .bind(uid)
    .fetch_one(&mut *conn)
    .await?)
}

/// 用户的角色是否要求 2FA（设置 `two_factor_required_role`：该角色及以上）
pub async fn required_for(ctx: &Ctx, pol: &Policy, uid: i64) -> PResult<bool> {
    let Some(min) = pol.two_factor_required_rank else {
        return Ok(false);
    };
    let (_, roles) = rbac::effective_roles(ctx, uid).await?;
    Ok(rbac::top_rank(&roles) >= min)
}

pub async fn status(ctx: &Ctx, uid: i64) -> PResult<Value> {
    let pol = Policy::load(ctx).await;
    let (totp, recovery): (bool, i64) = {
        let mut conn = ctx.sql.conn().await?;
        let row = sqlx::query(
            "SELECT EXISTS (SELECT 1 FROM totp_secrets WHERE user_id = $1 AND confirmed_at IS NOT NULL),
                    (SELECT count(*) FROM recovery_codes WHERE user_id = $1 AND used_at IS NULL)",
        )
        .bind(uid)
        .fetch_one(&mut *conn)
        .await?;
        (row.get(0), row.get(1))
    };
    Ok(json!({ "totp": totp, "recovery_remaining": recovery, "required": required_for(ctx, &pol, uid).await? }))
}

/// 开始绑定 TOTP：生成密钥（覆盖未确认的旧密钥），返回 base32 密钥、otpauth URI 与二维码 SVG
pub async fn begin(id: &Identity, ctx: &Ctx) -> PResult<Value> {
    let uid = uid_of(ctx)?;
    if has_totp(ctx, uid).await? {
        return Err(PluginError::new(
            "identity.two_factor_enabled",
            "two-factor authentication is already on; turn it off first",
        )
        .with_status(409));
    }
    let secret = ctx.random.bytes(20);
    let blob = id.keys()?.totp_seal(uid, &secret, nonce(ctx))?;
    let username: String = {
        let mut conn = ctx.sql.conn().await?;
        sqlx::query(
            "INSERT INTO totp_secrets (user_id, secret_enc) VALUES ($1, $2)
             ON CONFLICT (user_id) DO UPDATE SET secret_enc = EXCLUDED.secret_enc, last_step = 0, confirmed_at = NULL, created_at = now()",
        )
        .bind(uid)
        .bind(&blob)
        .execute(&mut *conn)
        .await?;
        sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
            .bind(uid)
            .fetch_one(&mut *conn)
            .await?
    };
    let site = ctx.config.get_static()["_site"]["name"]
        .as_str()
        .unwrap_or("PT")
        .to_owned();
    let b32 = BASE32_NOPAD.encode(&secret);
    let enc = |s: &str| {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
                _ => format!("%{b:02X}"),
            })
            .collect::<String>()
    };
    let uri = format!(
        "otpauth://totp/{}:{}?secret={b32}&issuer={}&algorithm=SHA1&digits=6&period=30",
        enc(&site),
        enc(&username),
        enc(&site)
    );
    let svg = qrcode::QrCode::new(uri.as_bytes())
        .map(|q| {
            q.render::<qrcode::render::svg::Color<'_>>()
                .min_dimensions(200, 200)
                .quiet_zone(true)
                .build()
        })
        .unwrap_or_default();
    Ok(json!({ "secret": b32, "otpauth_uri": uri, "qr_svg": svg }))
}

/// 校验 TOTP（行锁 + last_step 单调：同一个码并发提交只成功一次）。`pending = true` 校验未确认的密钥。
async fn verify_totp(id: &Identity, ctx: &Ctx, uid: i64, code: &str, pending: bool) -> PResult<bool> {
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(false);
    }
    let want: u32 = code.parse().unwrap_or(u32::MAX);
    let mut conn = ctx.sql.conn().await?;
    let row = sqlx::query(
        "SELECT secret_enc, last_step FROM totp_secrets WHERE user_id = $1 AND (confirmed_at IS NULL) = $2 FOR UPDATE",
    )
    .bind(uid)
    .bind(pending)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else { return Ok(false) };
    let secret = id.keys()?.totp_open(uid, &row.get::<Vec<u8>, _>(0))?;
    let last: i64 = row.get(1);
    let now = current_step(ctx);
    for step in [now - 1, now, now + 1] {
        if step > last && hotp(&secret, u64::try_from(step).unwrap_or(0)) == want {
            sqlx::query("UPDATE totp_secrets SET last_step = $2 WHERE user_id = $1")
                .bind(uid)
                .bind(step)
                .execute(&mut *conn)
                .await?;
            return Ok(true);
        }
    }
    Ok(false)
}

/// 生成一组新的恢复码（旧的全部作废），返回明文（只展示这一次）
async fn new_recovery_codes(ctx: &Ctx, uid: i64) -> PResult<Vec<String>> {
    let mut codes = Vec::with_capacity(RECOVERY_CODES);
    let mut conn = ctx.sql.conn().await?;
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = $1")
        .bind(uid)
        .execute(&mut *conn)
        .await?;
    for _ in 0..RECOVERY_CODES {
        let raw = ctx.random.bytes(10);
        let s: String = raw
            .iter()
            .map(|b| RECOVERY_ALPHABET[usize::from(*b) % RECOVERY_ALPHABET.len()] as char)
            .collect();
        let code = format!("{}-{}", &s[..5], &s[5..]);
        let salt = ctx.random.bytes(16);
        let mut data = salt.clone();
        data.extend_from_slice(normalize_code(&code).as_bytes());
        sqlx::query("INSERT INTO recovery_codes (user_id, salt, code_hash) VALUES ($1, $2, $3)")
            .bind(uid)
            .bind(&salt)
            .bind(crypto::sha256(&data))
            .execute(&mut *conn)
            .await?;
        codes.push(code);
    }
    Ok(codes)
}

/// 使用一个恢复码（成功即作废）
async fn use_recovery_code(ctx: &Ctx, uid: i64, code: &str) -> PResult<bool> {
    let code = normalize_code(code);
    if code.len() != 10 {
        return Ok(false);
    }
    let mut conn = ctx.sql.conn().await?;
    let rows = sqlx::query("SELECT id, salt, code_hash FROM recovery_codes WHERE user_id = $1 AND used_at IS NULL FOR UPDATE")
        .bind(uid)
        .fetch_all(&mut *conn)
        .await?;
    let mut hit = None;
    for r in &rows {
        let mut data: Vec<u8> = r.get(1);
        data.extend_from_slice(code.as_bytes());
        if crypto::ct_eq(&crypto::sha256(&data), &r.get::<Vec<u8>, _>(2)) {
            hit = Some(r.get::<i64, _>(0));
        }
    }
    let Some(rid) = hit else { return Ok(false) };
    sqlx::query("UPDATE recovery_codes SET used_at = now() WHERE id = $1")
        .bind(rid)
        .execute(&mut *conn)
        .await?;
    Ok(true)
}

/// 校验第二因素：6 位数字按 TOTP，其余按恢复码；返回所用方式
pub async fn verify_code(id: &Identity, ctx: &Ctx, uid: i64, code: &str) -> PResult<Option<&'static str>> {
    let c = normalize_code(code);
    if c.len() == 6 && c.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(verify_totp(id, ctx, uid, &c, false).await?.then_some("totp"));
    }
    if use_recovery_code(ctx, uid, &c).await? {
        risk::security_event(ctx, uid, "recovery_code_used", risk::client_details(ctx)).await?;
        return Ok(Some("recovery"));
    }
    Ok(None)
}

async fn changed(ctx: &Ctx, uid: i64, kind: &str, enabled: bool) -> PResult<()> {
    risk::security_event(
        ctx,
        uid,
        if enabled { "two_factor_enabled" } else { "two_factor_disabled" },
        json!({ "kind": kind }),
    )
    .await?;
    ctx.events
        .publish(NewEvent::v1(
            "user.two_factor_changed",
            format!("user:{uid}"),
            json!({ "user_id": uid, "kind": kind, "enabled": enabled, "by": "user" }),
        ))
        .await?;
    Ok(())
}

/// 确认 TOTP 绑定：校验一个码后开启，返回恢复码；当前会话视为已通过 2FA
pub async fn confirm(id: &Identity, ctx: &Ctx, code: &str) -> PResult<Vec<String>> {
    let uid = uid_of(ctx)?;
    if !verify_totp(id, ctx, uid, &normalize_code(code), true).await? {
        return Err(PluginError::new("identity.two_factor_invalid", "the code is not valid").with_status(400));
    }
    {
        let mut conn = ctx.sql.conn().await?;
        sqlx::query("UPDATE totp_secrets SET confirmed_at = now() WHERE user_id = $1")
            .bind(uid)
            .execute(&mut *conn)
            .await?;
        if let Ok(sid) = session_of(ctx) {
            sqlx::query("UPDATE sessions SET two_factor_at = now() WHERE id = $1")
                .bind(sid)
                .execute(&mut *conn)
                .await?;
        }
    }
    let codes = new_recovery_codes(ctx, uid).await?;
    changed(ctx, uid, "totp", true).await?;
    Ok(codes)
}

/// 关闭 TOTP（同时删除恢复码）；角色要求 2FA 且没有其他第二因素时拒绝
pub async fn disable(ctx: &Ctx) -> PResult<()> {
    let uid = uid_of(ctx)?;
    if !has_totp(ctx, uid).await? {
        return Err(PluginError::not_found("two-factor authentication is not on"));
    }
    let pol = Policy::load(ctx).await;
    if required_for(ctx, &pol, uid).await? {
        return Err(PluginError::new(
            "identity.two_factor_required",
            "your role requires two-factor authentication",
        )
        .with_status(403));
    }
    {
        let mut conn = ctx.sql.conn().await?;
        sqlx::query("DELETE FROM totp_secrets WHERE user_id = $1")
            .bind(uid)
            .execute(&mut *conn)
            .await?;
        sqlx::query("DELETE FROM recovery_codes WHERE user_id = $1")
            .bind(uid)
            .execute(&mut *conn)
            .await?;
    }
    changed(ctx, uid, "totp", false).await
}

/// 重新生成恢复码（需要已开启某种第二因素）
pub async fn regenerate(ctx: &Ctx) -> PResult<Vec<String>> {
    let uid = uid_of(ctx)?;
    if !has_totp(ctx, uid).await? {
        return Err(PluginError::not_found("two-factor authentication is not on"));
    }
    let codes = new_recovery_codes(ctx, uid).await?;
    risk::security_event(ctx, uid, "recovery_codes_regenerated", json!({})).await?;
    Ok(codes)
}

// ---------------- 登录第二步票据 ----------------

/// 密码正确且需要第二因素：签发 5 分钟票据（Redis，最多 5 次尝试）
pub async fn new_ticket(ctx: &Ctx, uid: i64, remember: bool) -> PResult<String> {
    let ticket = format!("tkt_{}", crypto::b64(&ctx.random.bytes(24)));
    let v = json!({ "uid": uid, "remember": remember, "attempts": 0, "exp": ctx.time.now() + TICKET_TTL_SECS * 1000 });
    ctx.redis
        .call(
            RedisCommand::new("SET")
                .arg(format!("tkt:{}", crypto::b64(&crypto::sha256(ticket.as_bytes()))))
                .arg(v.to_string())
                .arg("EX")
                .arg(TICKET_TTL_SECS.to_string()),
        )
        .await?;
    Ok(ticket)
}

fn ticket_invalid() -> PluginError {
    PluginError::new(
        "identity.ticket_invalid",
        "the sign-in has expired, please start again",
    )
    .with_status(401)
}

/// 取出票据（原子 GETDEL）；校验失败时由调用方写回
async fn take_ticket(ctx: &Ctx, ticket: &str) -> PResult<(String, Value)> {
    if !ticket.starts_with("tkt_") || ticket.len() > 64 {
        return Err(ticket_invalid());
    }
    let key = format!("tkt:{}", crypto::b64(&crypto::sha256(ticket.as_bytes())));
    let r = ctx.redis.call(RedisCommand::new("GETDEL").arg(&key)).await?;
    let v: Value = match r {
        RedisReply::Bulk(b) => serde_json::from_slice(&b).map_err(|_| ticket_invalid())?,
        _ => return Err(ticket_invalid()),
    };
    if v["exp"].as_i64().unwrap_or(0) < ctx.time.now() {
        return Err(ticket_invalid());
    }
    Ok((key, v))
}

/// 校验失败：尝试次数 +1，未达上限则写回（保持原过期时间）
async fn put_back(ctx: &Ctx, key: &str, mut v: Value) -> PResult<()> {
    let attempts = v["attempts"].as_i64().unwrap_or(0) + 1;
    let left_ms = v["exp"].as_i64().unwrap_or(0) - ctx.time.now();
    if attempts >= TICKET_MAX_ATTEMPTS || left_ms <= 0 {
        return Ok(());
    }
    v["attempts"] = json!(attempts);
    ctx.redis
        .call(
            RedisCommand::new("SET")
                .arg(key)
                .arg(v.to_string())
                .arg("PX")
                .arg(left_ms.to_string()),
        )
        .await?;
    Ok(())
}

/// 第二步：TOTP 或恢复码
pub async fn login_second_step(
    id: &Identity,
    ctx: &Ctx,
    ticket: &str,
    code: &str,
    device_cookie: Option<&str>,
) -> PResult<NewSession> {
    let (key, v) = take_ticket(ctx, ticket).await?;
    let uid = v["uid"].as_i64().ok_or_else(ticket_invalid)?;
    let method = verify_code(id, ctx, uid, code).await?;
    let Some(method) = method else {
        put_back(ctx, &key, v).await?;
        risk::security_event(ctx, uid, "two_factor_failed", risk::client_details(ctx)).await?;
        ctx.sql.commit().await?;
        return Err(PluginError::new("identity.two_factor_invalid", "the code is not valid").with_status(400));
    };
    let target = login_target(ctx, uid).await?;
    let pol = Policy::load(ctx).await;
    let mut s = auth::finish_login(
        id,
        ctx,
        &pol,
        target,
        "password",
        true,
        v["remember"].as_bool().unwrap_or(true),
        device_cookie,
    )
    .await?;
    s.second_factor = Some(method);
    Ok(s)
}

pub async fn login_target(ctx: &Ctx, uid: i64) -> PResult<LoginTarget> {
    let mut conn = ctx.sql.conn().await?;
    let r = sqlx::query(
        "SELECT username, class_id, status, auth_version FROM users WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(uid)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(ticket_invalid)?;
    let status: String = r.get(2);
    if status == "banned" || status == "deleted" {
        return Err(PluginError::new("identity.account_banned", "account is banned"));
    }
    Ok(LoginTarget {
        uid,
        username: r.get(0),
        class_id: r.get(1),
        status,
        auth_version: r.get(3),
    })
}

// ---------------- 近期重新验证 ----------------

/// `POST /sessions/current/reauth`：密码或第二因素；成功后刷新会话的 reauth_at（第二因素同时刷新 two_factor_at）
pub async fn reauth(id: &Identity, ctx: &Ctx, password: Option<&str>, code: Option<&str>) -> PResult<i64> {
    let uid = uid_of(ctx)?;
    let sid = session_of(ctx)?;
    let ok_2fa = match code {
        Some(c) => verify_code(id, ctx, uid, c).await?.is_some(),
        None => false,
    };
    let ok_pw = match (ok_2fa, password) {
        (false, Some(pw)) => {
            let phc: Option<String> = {
                let mut conn = ctx.sql.conn().await?;
                sqlx::query_scalar("SELECT phc FROM user_passwords WHERE user_id = $1")
                    .bind(uid)
                    .fetch_optional(&mut *conn)
                    .await?
                    .flatten()
            };
            match phc {
                Some(phc) => crypto::verify_password(phc, pw.to_owned()).await,
                None => false,
            }
        }
        _ => false,
    };
    if !ok_2fa && !ok_pw {
        risk::security_event(ctx, uid, "reauth_failed", risk::client_details(ctx)).await?;
        ctx.sql.commit().await?;
        return Err(PluginError::new("identity.invalid_credentials", "verification failed").with_status(403));
    }
    let mut conn = ctx.sql.conn().await?;
    sqlx::query(
        "UPDATE sessions SET reauth_at = now(), two_factor_at = CASE WHEN $2 THEN now() ELSE two_factor_at END WHERE id = $1",
    )
    .bind(sid)
    .bind(ok_2fa)
    .execute(&mut *conn)
    .await?;
    Ok(Utc::now().timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6238_vectors() {
        // RFC 6238 附录 B（SHA-1，种子 "12345678901234567890"），取 8 位结果的后 6 位
        let seed = b"12345678901234567890";
        assert_eq!(hotp(seed, 59 / 30), 287_082);
        assert_eq!(hotp(seed, 1_111_111_109 / 30), 81_804);
        assert_eq!(hotp(seed, 1_234_567_890 / 30), 5_924);
        assert_eq!(hotp(seed, 2_000_000_000 / 30), 279_037);
    }

    #[test]
    fn codes_normalize() {
        assert_eq!(normalize_code(" AbCde-fGh2k "), "abcdefgh2k");
        assert_eq!(normalize_code("123 456"), "123456");
    }
}
