//! 登录、会话与鉴权（identity 03 §1.3）：密码登录、限流与设备（risk）、会话 cookie。

use chrono::{DateTime, Duration, Utc};
use pt_plugin_api::{Credential, Ctx, NewEvent, PResult, PluginError, Principal, host::HookOutcome, json};
use sqlx::Row;

use crate::{Identity, accounts, crypto, policy::Policy, risk};

/// “保持登录”会话：30 天不访问才过期（每次访问顺延），最长 365 天后需要重新登录
pub const IDLE_DAYS: i64 = 30;
pub const ABSOLUTE_DAYS: i64 = 365;
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
    /// 新下发的设备 cookie
    pub device_cookie: Option<String>,
    /// 登录所用的第二因素（totp / recovery / webauthn）
    pub second_factor: Option<&'static str>,
}

/// 密码登录的结果：直接建会话，或需要第二因素（票据）
#[derive(Debug)]
pub enum LoginOutcome {
    Session(NewSession),
    SecondFactor { ticket: String, methods: Vec<&'static str> },
}

fn invalid() -> PluginError {
    PluginError::new("identity.invalid_credentials", "invalid username or password")
}

/// 密码登录（无 2FA 时直接完成）。`device_cookie`：请求带来的设备 cookie（`__Host-pt_dev`）。
pub async fn login_password(
    id: &Identity,
    ctx: &Ctx,
    login: &str,
    password: &str,
    remember: bool,
    device_cookie: Option<&str>,
) -> PResult<LoginOutcome> {
    let keys = id.keys()?;
    let pol = Policy::load(ctx).await;
    let login = login.trim();
    let tk = risk::ThrottleKeys::new(keys, login, ctx.invocation.client_ip.as_deref());
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
    let known = match &row {
        Some(r) => risk::device_known(ctx, keys, r.get(0), device_cookie).await?,
        None => false,
    };
    risk::throttle_check(ctx, &pol, &tk, known).await?;
    let Some(row) = row else {
        let _ = crypto::verify_password(crypto::DUMMY_PHC.into(), password.into()).await;
        let locked = risk::throttle_fail(ctx, &pol, &tk).await;
        failed(ctx, None, "password", locked).await?;
        return Err(invalid());
    };
    let uid: i64 = row.get(0);
    let username: String = row.get(1);
    let class_id: i32 = row.get(2);
    let status: String = row.get(3);
    let phc: Option<String> = row.get(5);
    let ok = match phc {
        Some(phc) => crypto::verify_password(phc, password.into()).await,
        None => {
            let _ = crypto::verify_password(crypto::DUMMY_PHC.into(), password.into()).await;
            false
        }
    };
    if !ok {
        let locked = risk::throttle_fail(ctx, &pol, &tk).await;
        failed(ctx, Some(uid), "password", locked).await?;
        return Err(invalid());
    }
    if status == "banned" || status == "deleted" {
        failed(ctx, Some(uid), "status", false).await?;
        return Err(PluginError::new("identity.account_banned", "account is banned"));
    }
    // 第一因素已通过：清除用户名维度的失败计数
    risk::throttle_success(ctx, &tk).await;
    if crate::two_factor::has_totp(ctx, uid).await? {
        let ticket = crate::two_factor::new_ticket(ctx, uid, remember).await?;
        return Ok(LoginOutcome::SecondFactor {
            ticket,
            methods: vec!["totp", "recovery"],
        });
    }
    let s = finish_login(
        id,
        ctx,
        &pol,
        LoginTarget {
            uid,
            username,
            class_id,
            status,
            auth_version: row.get(4),
        },
        "password",
        false,
        remember,
        device_cookie,
    )
    .await?;
    Ok(LoginOutcome::Session(s))
}

/// 已通过第一因素（及所需的第二因素）的登录对象
pub struct LoginTarget {
    pub uid: i64,
    pub username: String,
    pub class_id: i32,
    pub status: String,
    pub auth_version: i32,
}

/// 登录收尾（03 §1.3 finish_login）：登录钩子、建会话、登记设备与 IP、安全事件、新设备提醒。
#[allow(clippy::too_many_arguments)]
pub async fn finish_login(
    id: &Identity,
    ctx: &Ctx,
    pol: &Policy,
    u: LoginTarget,
    method: &str,
    two_factor: bool,
    remember: bool,
    device_cookie: Option<&str>,
) -> PResult<NewSession> {
    let keys = id.keys()?;
    let uid = u.uid;
    let known = risk::device_known(ctx, keys, uid, device_cookie).await?;
    let mut input = json!({ "version": 1, "actor": ctx.invocation.actor, "user_id": uid, "username": u.username,
                            "method": method, "new_device": !known, "risk_flags": [] });
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
        failed(ctx, Some(uid), "hook", false).await?;
        let mut e = PluginError::new(code, message);
        e.details = details;
        return Err(e);
    }

    let device = risk::upsert_device(ctx, keys, uid, device_cookie).await?;
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
    let ip = ctx
        .invocation
        .client_ip
        .clone()
        .filter(|ip| ip.parse::<std::net::IpAddr>().is_ok());
    {
        let mut conn = ctx.sql.conn().await?;
        sqlx::query(
            "INSERT INTO sessions (id, user_id, token_hash, remember, method, two_factor_at, reauth_at, ua_summary, ip_mac,
                                   last_ip, device_id, auth_version, idle_expires_at, absolute_expires_at)
             VALUES ($1,$2,$3,$4,$5,CASE WHEN $6 THEN now() END,now(),$7,$8,$9::inet,$10,$11,$12,$13)",
        )
        .bind(sid)
        .bind(uid)
        .bind(crypto::sha256(token.as_bytes()))
        .bind(remember)
        .bind(method)
        .bind(two_factor)
        .bind(ua)
        .bind(ip.as_ref().map(|ip| keys.ip_mac(ip)))
        .bind(ip.as_deref())
        .bind(device.id)
        .bind(u.auth_version)
        .bind(now + idle)
        .bind(now + Duration::days(ABSOLUTE_DAYS))
        .execute(&mut *conn)
        .await?;
        sqlx::query("UPDATE users SET last_seen_at = now() WHERE id = $1")
            .bind(uid)
            .execute(&mut *conn)
            .await?;
    }
    if let Some(ip) = &ip {
        risk::ip_log_upsert(ctx, keys, uid, ip, "login").await?;
    }
    let mut details = risk::client_details(ctx);
    details["method"] = json!(method);
    details["new_device"] = json!(device.new_device);
    risk::security_event(ctx, uid, "login", details.clone()).await?;
    ctx.events
        .publish(NewEvent::v1(
            "user.logged_in",
            format!("user:{uid}"),
            json!({ "user_id": uid, "method": method, "two_factor": two_factor, "session_id": sid,
                    "new_device": device.new_device, "alert": device.new_device && pol.login_alert,
                    "ua": details["ua"], "at": now.to_rfc3339() }),
        ))
        .await?;
    Ok(NewSession {
        session_id: sid,
        token,
        csrf,
        user_id: uid,
        username: u.username,
        class_id: u.class_id,
        status: u.status,
        max_age_secs: if remember { ABSOLUTE_DAYS * 86400 } else { 0 },
        device_cookie: device.set_cookie,
        second_factor: None,
    })
}

async fn failed(ctx: &Ctx, uid: Option<i64>, stage: &str, locked: bool) -> PResult<()> {
    // 失败事件不含用户名与 IP
    ctx.events
        .publish(NewEvent::v1(
            "user.login_failed",
            uid.map_or_else(|| "global".to_owned(), |u| format!("user:{u}")),
            json!({ "user_id": uid, "stage": stage }),
        ))
        .await?;
    if locked {
        if let Some(u) = uid {
            risk::security_event(ctx, u, "login_locked", risk::client_details(ctx)).await?;
        }
        ctx.events
            .publish(NewEvent::v1(
                "user.login_failed",
                uid.map_or_else(|| "global".to_owned(), |u| format!("user:{u}")),
                json!({ "user_id": uid, "stage": "throttled" }),
            ))
            .await?;
    }
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
pub async fn authenticate(id: &Identity, ctx: &Ctx, cred: Credential) -> PResult<Option<Principal>> {
    match cred {
        Credential::Session(token) => {
            if token.is_empty() || token.len() > 128 {
                return Ok(None);
            }
            let mut conn = ctx.sql.conn().await?;
            let row = sqlx::query(
                "SELECT s.id, s.user_id, s.idle_expires_at, s.absolute_expires_at, s.revoked_at, s.reauth_at,
                        s.last_seen_at, s.remember, s.auth_version, u.status, u.auth_version, u.class_id, s.ip_mac,
                        s.two_factor_at IS NOT NULL
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
            let class_id: i32 = r.get(11);
            let seen_ip_mac: Option<Vec<u8>> = r.get(12);
            let two_factor: bool = r.get(13);
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
            // 会话 IP 变化：更新会话并记入 IP 历史
            if let Some(ip) = ctx
                .invocation
                .client_ip
                .as_deref()
                .filter(|ip| ip.parse::<std::net::IpAddr>().is_ok())
            {
                let keys = id.keys()?;
                let mac = keys.ip_mac(ip);
                if seen_ip_mac.as_deref() != Some(mac.as_slice()) {
                    sqlx::query("UPDATE sessions SET ip_mac = $2, last_ip = $3::inet WHERE id = $1")
                        .bind(sid)
                        .bind(&mac)
                        .bind(ip)
                        .execute(&mut *conn)
                        .await?;
                    drop(conn);
                    risk::ip_log_upsert(ctx, keys, uid, ip, "session").await?;
                }
            }
            Ok(Some(Principal {
                user_id: u64::try_from(uid).unwrap_or_default(),
                credential_id: sid.to_string(),
                credential_kind: "session".into(),
                // "2fa"：本会话通过了两步验证（强制 2FA 的角色只在这样的会话中生效）
                scopes: if two_factor {
                    vec!["*".into(), "2fa".into()]
                } else {
                    vec!["*".into()]
                },
                reauth_at: reauth.map(|t| t.timestamp_millis()),
                status,
                class_id: Some(class_id),
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
