//! 邮件挑战（identity 03 §1.8）：邮箱验证、找回密码。
//!
//! identity 只发布 `user.challenge_issued`（不含令牌）；投递插件（notify）订阅后调用 `ctx.auth.challenge_delivery`
//! 取收件人与链接，链接不进事件、不进通知库。确认失败统一返回 `identity.challenge_invalid`，不区分原因。

use chrono::{DateTime, Duration, Utc};
use pt_plugin_api::{Ctx, NewEvent, PResult, PluginError, json};
use serde_json::Value;
use sqlx::Row;

use crate::{Identity, accounts, crypto, risk, status};

pub const EMAIL_VERIFY: &str = "email_verify";
pub const PASSWORD_RESET: &str = "password_reset";
/// 同一挑战最多投递次数（重发需要签发新挑战）
const MAX_DELIVERIES: i16 = 3;

fn ttl(purpose: &str) -> Duration {
    match purpose {
        PASSWORD_RESET => Duration::hours(1),
        _ => Duration::hours(48),
    }
}

/// 站内处理页（notify 以站点地址补全为绝对链接）
fn route(purpose: &str) -> &'static str {
    match purpose {
        PASSWORD_RESET => "/reset-password",
        _ => "/verify-email",
    }
}

fn invalid() -> PluginError {
    PluginError::new(
        "identity.challenge_invalid",
        "the link is invalid or has expired",
    )
    .with_status(400)
}

/// 签发挑战（同一用户同一用途的旧挑战作废）并发布 `user.challenge_issued`
pub async fn issue(id: &Identity, ctx: &Ctx, uid: i64, purpose: &str) -> PResult<uuid::Uuid> {
    let cid = uuid::Uuid::now_v7();
    let nonce = ctx.random.bytes(16);
    let token = id.keys()?.challenge_token(&cid, &nonce);
    let expires = Utc::now() + ttl(purpose);
    let locale: String = {
        let mut conn = ctx.sql.conn().await?;
        sqlx::query(
            "UPDATE challenges SET superseded_at = now()
              WHERE user_id = $1 AND purpose = $2 AND consumed_at IS NULL AND superseded_at IS NULL",
        )
        .bind(uid)
        .bind(purpose)
        .execute(&mut *conn)
        .await?;
        sqlx::query(
            "INSERT INTO challenges (id, user_id, purpose, nonce, token_hash, expires_at) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(cid)
        .bind(uid)
        .bind(purpose)
        .bind(&nonce)
        .bind(crypto::sha256(token.as_bytes()))
        .bind(expires)
        .execute(&mut *conn)
        .await?;
        sqlx::query_scalar("SELECT locale FROM users WHERE id = $1")
            .bind(uid)
            .fetch_one(&mut *conn)
            .await?
    };
    ctx.events
        .publish(NewEvent::v1(
            "user.challenge_issued",
            format!("user:{uid}"),
            json!({ "user_id": uid, "challenge_id": cid, "purpose": purpose,
                    "expires_at": expires.timestamp_millis(), "locale": locale }),
        ))
        .await?;
    Ok(cid)
}

/// `ctx.auth.challenge_delivery`：作用域 `challenges:deliver` + 敏感能力 `user.email`。
/// 同一幂等键重复调用返回同一结果；不同幂等键最多 3 次。
pub async fn delivery(id: &Identity, ctx: &Ctx, cid: uuid::Uuid, idem: &str) -> PResult<Value> {
    if !ctx.invocation.has_scope("challenges:deliver") {
        return Err(PluginError::forbidden("scope challenges:deliver required"));
    }
    if !ctx.invocation.has_scope("sensitive:user.email") {
        return Err(PluginError::capability_denied(
            "sensitive capability user.email required",
        ));
    }
    let keys = id.keys()?;
    let mut conn = ctx.sql.conn().await?;
    let row = sqlx::query(
        "SELECT c.user_id, c.purpose, c.nonce, c.expires_at, c.consumed_at IS NOT NULL OR c.superseded_at IS NOT NULL,
                c.delivery_count, c.last_delivery_key, u.username, u.locale, e.email_enc
           FROM challenges c JOIN users u ON u.id = c.user_id LEFT JOIN user_emails e ON e.user_id = c.user_id
          WHERE c.id = $1 AND u.deleted_at IS NULL FOR UPDATE OF c",
    )
    .bind(cid)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(r) = row else { return Err(invalid()) };
    let expires: DateTime<Utc> = r.get(3);
    let done: bool = r.get(4);
    if done || expires < Utc::now() {
        return Err(invalid());
    }
    let uid: i64 = r.get(0);
    let count: i16 = r.get(5);
    let last: Option<String> = r.get(6);
    if last.as_deref() != Some(idem) {
        if count >= MAX_DELIVERIES {
            return Err(PluginError::new(
                "identity.challenge_delivery_exhausted",
                "this challenge has been delivered too many times",
            )
            .with_status(429));
        }
        sqlx::query("UPDATE challenges SET delivery_count = delivery_count + 1, last_delivery_key = $2 WHERE id = $1")
            .bind(cid)
            .bind(idem)
            .execute(&mut *conn)
            .await?;
    }
    let Some(enc) = r.get::<Option<Vec<u8>>, _>(9) else {
        return Err(invalid());
    };
    drop(conn);
    let purpose: String = r.get(1);
    let token = keys.challenge_token(&cid, &r.get::<Vec<u8>, _>(2));
    ctx.audit
        .write(pt_plugin_api::AuditEntry {
            action: "identity.sensitive_read".into(),
            target: format!("user:{uid}"),
            details: json!({ "field": "email", "purpose": purpose }),
        })
        .await?;
    Ok(json!({
        "to": keys.email_open(uid, &enc)?,
        "purpose": purpose,
        "link": format!("{}?c={cid}&t={token}", route(&purpose)),
        "expires_at": expires.timestamp_millis(),
        "username": r.get::<String, _>(7),
        "locale": r.get::<String, _>(8),
    }))
}

/// 校验并消费挑战，返回用户 ID
async fn consume(ctx: &Ctx, cid: &str, token: &str, purpose: &str) -> PResult<i64> {
    let cid = uuid::Uuid::parse_str(cid).map_err(|_| invalid())?;
    if token.is_empty() || token.len() > 64 {
        return Err(invalid());
    }
    let mut conn = ctx.sql.conn().await?;
    let row = sqlx::query(
        "SELECT user_id, purpose, token_hash, expires_at, consumed_at IS NOT NULL OR superseded_at IS NOT NULL
           FROM challenges WHERE id = $1 FOR UPDATE",
    )
    .bind(cid)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(r) = row else { return Err(invalid()) };
    let hash: Vec<u8> = r.get(2);
    let expires: DateTime<Utc> = r.get(3);
    let ok = r.get::<String, _>(1) == purpose
        && !r.get::<bool, _>(4)
        && expires >= Utc::now()
        && crypto::ct_eq(&hash, &crypto::sha256(token.as_bytes()));
    if !ok {
        return Err(invalid());
    }
    sqlx::query("UPDATE challenges SET consumed_at = now() WHERE id = $1")
        .bind(cid)
        .execute(&mut *conn)
        .await?;
    Ok(r.get(0))
}

/// 标记邮箱已验证并解除“邮箱未验证”的待验证限制（只解除 identity 自己施加的这一条）
async fn mark_verified(ctx: &Ctx, uid: i64) -> PResult<bool> {
    let n = {
        let mut conn = ctx.sql.conn().await?;
        sqlx::query("UPDATE user_emails SET verified_at = now(), updated_at = now() WHERE user_id = $1 AND verified_at IS NULL")
            .bind(uid)
            .execute(&mut *conn)
            .await?
            .rows_affected()
    };
    status::lift(ctx, uid, Some("pending"), "identity", "email_verified", None, false).await?;
    if n > 0 {
        ctx.events
            .publish(NewEvent::v1(
                "user.verified",
                format!("user:{uid}"),
                json!({ "user_id": uid, "method": "email" }),
            ))
            .await?;
    }
    Ok(n > 0)
}

pub async fn confirm_email(ctx: &Ctx, cid: &str, token: &str) -> PResult<i64> {
    let uid = consume(ctx, cid, token, EMAIL_VERIFY).await?;
    mark_verified(ctx, uid).await?;
    risk::security_event(ctx, uid, "email_verified", json!({})).await?;
    Ok(uid)
}

/// 已登录用户重发验证邮件；已验证时报错
pub async fn resend_verification(id: &Identity, ctx: &Ctx, uid: i64) -> PResult<()> {
    let verified: Option<bool> = {
        let mut conn = ctx.sql.conn().await?;
        sqlx::query_scalar("SELECT verified_at IS NOT NULL FROM user_emails WHERE user_id = $1")
            .bind(uid)
            .fetch_optional(&mut *conn)
            .await?
    };
    match verified {
        None => Err(PluginError::new("identity.email_missing", "no email address on file")),
        Some(true) => Err(PluginError::new(
            "identity.email_already_verified",
            "email address is already verified",
        )
        .with_status(409)),
        Some(false) => issue(id, ctx, uid, EMAIL_VERIFY).await.map(|_| ()),
    }
}

/// 申请找回密码：按用户名或邮箱查找；无论是否找到都成功返回（防枚举）
pub async fn request_reset(id: &Identity, ctx: &Ctx, login: &str) -> PResult<()> {
    let keys = id.keys()?;
    let login = login.trim();
    let uid: Option<i64> = {
        let mut conn = ctx.sql.conn().await?;
        if login.contains('@') {
            match accounts::canon_email(login) {
                Ok((email, _)) => sqlx::query_scalar(
                    "SELECT u.id FROM user_emails e JOIN users u ON u.id = e.user_id
                      WHERE e.email_mac = $1 AND u.deleted_at IS NULL AND u.status <> 'banned'",
                )
                .bind(keys.email_mac(&email))
                .fetch_optional(&mut *conn)
                .await?,
                Err(_) => None,
            }
        } else {
            sqlx::query_scalar(
                "SELECT u.id FROM users u JOIN user_emails e ON e.user_id = u.id
                  WHERE u.username_canonical = $1 AND u.deleted_at IS NULL AND u.status <> 'banned'",
            )
            .bind(login.to_lowercase())
            .fetch_optional(&mut *conn)
            .await?
        }
    };
    if let Some(uid) = uid {
        issue(id, ctx, uid, PASSWORD_RESET).await?;
        risk::security_event(ctx, uid, "password_reset_requested", risk::client_details(ctx)).await?;
    }
    Ok(())
}

/// 用找回链接设置新密码：撤销全部会话、递增 auth_version；同时视为邮箱验证成功
pub async fn confirm_reset(ctx: &Ctx, cid: &str, token: &str, new_password: &str) -> PResult<i64> {
    let uid = consume(ctx, cid, token, PASSWORD_RESET).await?;
    let username: String = {
        let mut conn = ctx.sql.conn().await?;
        sqlx::query_scalar("SELECT username FROM users WHERE id = $1")
            .bind(uid)
            .fetch_one(&mut *conn)
            .await?
    };
    accounts::password_policy(new_password, &username)?;
    let phc = crypto::hash_password(new_password.to_owned()).await?;
    {
        let mut conn = ctx.sql.conn().await?;
        sqlx::query(
            "INSERT INTO user_passwords (user_id, algo, phc) VALUES ($1, 'argon2id', $2)
             ON CONFLICT (user_id) DO UPDATE SET algo = 'argon2id', phc = EXCLUDED.phc, legacy_hash = NULL, salt = NULL, changed_at = now()",
        )
        .bind(uid)
        .bind(&phc)
        .execute(&mut *conn)
        .await?;
        sqlx::query(
            "UPDATE sessions SET revoked_at = now(), revoked_reason = 'password_reset' WHERE user_id = $1 AND revoked_at IS NULL",
        )
        .bind(uid)
        .execute(&mut *conn)
        .await?;
        sqlx::query("UPDATE users SET auth_version = auth_version + 1, updated_at = now() WHERE id = $1")
            .bind(uid)
            .execute(&mut *conn)
            .await?;
    }
    mark_verified(ctx, uid).await?;
    risk::security_event(ctx, uid, "password_reset", risk::client_details(ctx)).await?;
    ctx.events
        .publish(NewEvent::v1(
            "user.password_changed",
            format!("user:{uid}"),
            json!({ "user_id": uid, "by": "reset" }),
        ))
        .await?;
    Ok(uid)
}
