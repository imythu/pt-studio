//! 登录、会话与鉴权（identity 03 §1.3）。P1 子集：密码登录、会话 cookie；2FA、token、设备在后续迭代。

use chrono::{DateTime, Duration, Utc};
use pt_plugin_api::{Credential, Ctx, NewEvent, PResult, PluginError, Principal, host::HookOutcome, json};
use sqlx::Row;

use crate::{Identity, accounts, crypto};

pub const IDLE_DAYS: i64 = 14;
pub const ABSOLUTE_DAYS: i64 = 90;
pub const SHORT_IDLE_HOURS: i64 = 12;
pub const TOUCH_SECS: i64 = 15 * 60;

#[derive(Debug, Clone)]
pub struct NewSession {
    pub session_id: uuid::Uuid,
    pub token: String,
    pub csrf: String,
    pub user_id: i64,
    pub username: String,
    pub class_id: i32,
    pub status: String,
    pub max_age_secs: i64,
}

fn invalid() -> PluginError {
    PluginError::new("identity.invalid_credentials", "invalid username or password")
}

/// 密码登录（无 2FA 时直接完成）。
pub async fn login_password(
    id: &Identity,
    ctx: &Ctx,
    login: &str,
    password: &str,
    remember: bool,
) -> PResult<NewSession> {
    let keys = id.keys()?;
    let login = login.trim();
    // 用户名或邮箱
    let row = {
        let mut conn = ctx.sql.conn().await?;
        if login.contains('@') {
            let (email, _) = accounts::canon_email(login).map_err(|_| invalid())?;
            sqlx::query(
                "SELECT u.id, u.username, u.class_id, u.status, u.auth_version, p.phc
                   FROM user_emails e JOIN users u ON u.id = e.user_id LEFT JOIN user_passwords p ON p.user_id = u.id
                  WHERE e.email_mac = $1 AND u.deleted_at IS NULL",
            )
            .bind(keys.email_mac(&email))
            .fetch_optional(&mut *conn)
            .await?
        } else {
            sqlx::query(
                "SELECT u.id, u.username, u.class_id, u.status, u.auth_version, p.phc
                   FROM users u LEFT JOIN user_passwords p ON p.user_id = u.id
                  WHERE u.username_canonical = $1 AND u.deleted_at IS NULL",
            )
            .bind(login.to_lowercase())
            .fetch_optional(&mut *conn)
            .await?
        }
    };
    let Some(row) = row else {
        let _ = crypto::verify_password(crypto::DUMMY_PHC.into(), password.into()).await;
        failed(ctx, None, "password").await?;
        return Err(invalid());
    };
    let uid: i64 = row.get(0);
    let username: String = row.get(1);
    let class_id: i32 = row.get(2);
    let status: String = row.get(3);
    let auth_version: i32 = row.get(4);
    let phc: Option<String> = row.get(5);
    let ok = match phc {
        Some(phc) => crypto::verify_password(phc, password.into()).await,
        None => {
            let _ = crypto::verify_password(crypto::DUMMY_PHC.into(), password.into()).await;
            false
        }
    };
    if !ok {
        failed(ctx, Some(uid), "password").await?;
        return Err(invalid());
    }
    if status == "banned" || status == "deleted" {
        failed(ctx, Some(uid), "status").await?;
        return Err(PluginError::new("identity.account_banned", "account is banned"));
    }

    let mut input = json!({ "version": 1, "actor": ctx.invocation.actor, "user_id": uid, "username": username,
                            "method": "password", "new_device": false, "risk_flags": [] });
    if let Some(ip) = &ctx.invocation.client_ip {
        input["ip"] = json!(ip);
        input["ip_hmac"] = json!(hex::encode(keys.ip_mac(ip)));
    }
    if let HookOutcome::Denied {
        code,
        message,
        details,
        ..
    } = ctx.hooks.run("user.before_login", input).await?
    {
        failed(ctx, Some(uid), "hook").await?;
        let mut e = PluginError::new(code, message);
        e.details = details;
        return Err(e);
    }

    let sid = uuid::Uuid::now_v7();
    let token = crypto::b64(&ctx.random.bytes(32));
    let csrf = crypto::b64(&ctx.random.bytes(32));
    let now = Utc::now();
    let idle = if remember {
        Duration::days(IDLE_DAYS)
    } else {
        Duration::hours(SHORT_IDLE_HOURS)
    };
    let ua: Option<String> = ctx
        .invocation
        .user_agent
        .as_ref()
        .map(|u| u.chars().take(128).collect());
    {
        let mut conn = ctx.sql.conn().await?;
        sqlx::query(
            "INSERT INTO sessions (id, user_id, token_hash, remember, method, reauth_at, ua_summary, ip_mac, auth_version,
                                   idle_expires_at, absolute_expires_at)
             VALUES ($1,$2,$3,$4,'password',now(),$5,$6,$7,$8,$9)",
        )
        .bind(sid)
        .bind(uid)
        .bind(crypto::sha256(token.as_bytes()))
        .bind(remember)
        .bind(ua)
        .bind(ctx.invocation.client_ip.as_ref().map(|ip| keys.ip_mac(ip)))
        .bind(auth_version)
        .bind(now + idle)
        .bind(now + Duration::days(ABSOLUTE_DAYS))
        .execute(&mut *conn)
        .await?;
        sqlx::query("UPDATE users SET last_seen_at = now() WHERE id = $1")
            .bind(uid)
            .execute(&mut *conn)
            .await?;
    }
    ctx.events
        .publish(NewEvent::v1(
            "user.logged_in",
            format!("user:{uid}"),
            json!({ "user_id": uid, "method": "password", "two_factor": false, "session_id": sid, "new_device": false }),
        ))
        .await?;
    Ok(NewSession {
        session_id: sid,
        token,
        csrf,
        user_id: uid,
        username,
        class_id,
        status,
        max_age_secs: if remember { ABSOLUTE_DAYS * 86400 } else { 0 },
    })
}

async fn failed(ctx: &Ctx, uid: Option<i64>, stage: &str) -> PResult<()> {
    // 失败事件不含用户名与 IP；限流在后续迭代接入 Redis（03 §1.9）
    ctx.events
        .publish(NewEvent::v1(
            "user.login_failed",
            uid.map_or_else(|| "global".to_owned(), |u| format!("user:{u}")),
            json!({ "user_id": uid, "stage": stage }),
        ))
        .await?;
    // 失败也要提交事件：登录 handler 返回错误会回滚工作单元，因此在此提前提交
    ctx.sql.commit().await
}

pub async fn logout(ctx: &Ctx) -> PResult<()> {
    let uid = i64::try_from(ctx.require_user()?).map_err(|_| PluginError::bad_request("bad user id"))?;
    let sid = ctx
        .invocation
        .actor
        .credential_id
        .as_deref()
        .and_then(|c| uuid::Uuid::parse_str(c).ok())
        .ok_or_else(|| PluginError::bad_request("not a session credential"))?;
    let mut conn = ctx.sql.conn().await?;
    sqlx::query(
        "UPDATE sessions SET revoked_at = now(), revoked_reason = 'logout' WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
    )
    .bind(sid)
    .bind(uid)
    .execute(&mut *conn)
    .await?;
    drop(conn);
    ctx.events
        .publish(NewEvent::v1(
            "user.logged_out",
            format!("user:{uid}"),
            json!({ "user_id": uid, "session_id": sid }),
        ))
        .await?;
    Ok(())
}

/// `ctx.auth.authenticate`
pub async fn authenticate(ctx: &Ctx, cred: Credential) -> PResult<Option<Principal>> {
    match cred {
        Credential::Session(token) => {
            if token.is_empty() || token.len() > 128 {
                return Ok(None);
            }
            let mut conn = ctx.sql.conn().await?;
            let row = sqlx::query(
                "SELECT s.id, s.user_id, s.idle_expires_at, s.absolute_expires_at, s.revoked_at, s.reauth_at,
                        s.last_seen_at, s.remember, s.auth_version, u.status, u.auth_version
                   FROM sessions s JOIN users u ON u.id = s.user_id
                  WHERE s.token_hash = $1",
            )
            .bind(crypto::sha256(token.as_bytes()))
            .fetch_optional(&mut *conn)
            .await?;
            let Some(r) = row else { return Ok(None) };
            let sid: uuid::Uuid = r.get(0);
            let uid: i64 = r.get(1);
            let idle: DateTime<Utc> = r.get(2);
            let absolute: DateTime<Utc> = r.get(3);
            let revoked: Option<DateTime<Utc>> = r.get(4);
            let reauth: Option<DateTime<Utc>> = r.get(5);
            let last_seen: DateTime<Utc> = r.get(6);
            let remember: bool = r.get(7);
            let seen_version: i32 = r.get(8);
            let status: String = r.get(9);
            let auth_version: i32 = r.get(10);
            let now = Utc::now();
            if revoked.is_some() || now > idle || now > absolute || seen_version != auth_version {
                return Ok(None);
            }
            if status == "banned" || status == "deleted" {
                return Ok(None);
            }
            if (now - last_seen).num_seconds() > TOUCH_SECS {
                let idle = if remember {
                    Duration::days(IDLE_DAYS)
                } else {
                    Duration::hours(SHORT_IDLE_HOURS)
                };
                sqlx::query("UPDATE sessions SET last_seen_at = now(), idle_expires_at = least($2, absolute_expires_at) WHERE id = $1")
                    .bind(sid)
                    .bind(now + idle)
                    .execute(&mut *conn)
                    .await?;
            }
            Ok(Some(Principal {
                user_id: u64::try_from(uid).unwrap_or_default(),
                credential_id: sid.to_string(),
                credential_kind: "session".into(),
                scopes: vec!["*".into()],
                reauth_at: reauth.map(|t| t.timestamp_millis()),
                status,
            }))
        }
        // 个人 / MCP token（03 §1.5）
        Credential::Bearer(secret) => crate::tokens::authenticate(ctx, &secret, None).await,
        // feed key：只对签发它的插件（调用方）有效
        Credential::Feed(secret) => {
            crate::tokens::authenticate(ctx, &secret, Some(&ctx.invocation.caller_plugin)).await
        }
    }
}
