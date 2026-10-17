//! 注册与用户名 / 邮箱规范化（identity 03 §1.2）。

use pt_plugin_api::{Ctx, NewEvent, PResult, PluginError, host::HookOutcome, json};
use serde::Deserialize;
use unicode_normalization::UnicodeNormalization;

use crate::{Identity, crypto};

#[derive(Debug, Clone, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    #[serde(default)]
    pub invite_code: Option<String>,
    #[serde(default)]
    pub locale: Option<String>,
    #[serde(default)]
    pub form: serde_json::Value,
    /// 人机验证令牌（captcha 插件的前端写入）
    #[serde(default)]
    pub captcha_token: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Http,
    Service,
    Admin,
}

pub const MIN_USERNAME: usize = 3;
pub const MAX_USERNAME: usize = 20;
pub const MIN_PASSWORD: usize = 8;

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF   // 平假名、片假名
        | 0x3400..=0x4DBF // CJK 扩展 A
        | 0x4E00..=0x9FFF // CJK 统一汉字
        | 0xAC00..=0xD7AF // 谚文音节
        | 0xF900..=0xFAFF)
}

/// 返回 (显示名, 规范形)。规范形 = 小写（UTS #39 混淆骨架待接入 unicode-security，见 03 §1.2）。
pub fn canon_username(raw: &str, allow_cjk: bool) -> PResult<(String, String)> {
    let bad = |m: &str| {
        PluginError::new("identity.username_invalid", m.to_owned()).with_details(json!({ "errors": [
            { "path": "/username", "code": "identity.username_invalid", "message": m }
        ]}))
    };
    let s: String = raw.nfc().collect::<String>().trim().to_owned();
    let n = s.chars().count();
    if !(MIN_USERNAME..=MAX_USERNAME).contains(&n) {
        return Err(bad("username must be 3-20 characters"));
    }
    for c in s.chars() {
        let ok = c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') || (allow_cjk && is_cjk(c));
        if !ok {
            return Err(bad("username may only contain letters, digits, '_', '.' and '-'"));
        }
    }
    let first = s.chars().next().unwrap_or('_');
    let last = s.chars().last().unwrap_or('_');
    if matches!(first, '_' | '.' | '-') || matches!(last, '_' | '.' | '-') {
        return Err(bad("username must not start or end with a symbol"));
    }
    if s.contains("..") || s.contains("__") || s.contains("--") {
        return Err(bad("username must not contain repeated symbols"));
    }
    let canonical = s.to_lowercase();
    if RESERVED.contains(&canonical.as_str()) {
        return Err(bad("username is reserved"));
    }
    Ok((s, canonical))
}

const RESERVED: &[&str] = &[
    "admin",
    "administrator",
    "root",
    "system",
    "sysop",
    "moderator",
    "anonymous",
    "deleted",
    "null",
];

/// 返回 (规范化邮箱, 域名)。
pub fn canon_email(raw: &str) -> PResult<(String, String)> {
    let bad = || {
        PluginError::new("identity.email_invalid", "invalid email address").with_details(json!({ "errors": [
            { "path": "/email", "code": "identity.email_invalid", "message": "invalid email address" }
        ]}))
    };
    let s: String = raw.nfc().collect::<String>().trim().to_owned();
    if s.len() > 254 {
        return Err(bad());
    }
    let (local, domain) = s.rsplit_once('@').ok_or_else(bad)?;
    let domain = domain.to_ascii_lowercase();
    let local_ok = !local.is_empty()
        && local.len() <= 64
        && local
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "!#$%&'*+/=?^_`{|}~.-".contains(c))
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..");
    let labels: Vec<&str> = domain.split('.').collect();
    let domain_ok = labels.len() >= 2
        && labels.iter().all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    if !local_ok || !domain_ok {
        return Err(bad());
    }
    Ok((format!("{}@{domain}", local.to_lowercase()), domain))
}

pub fn password_policy(password: &str, username: &str) -> PResult<()> {
    let bad = |m: &str| {
        PluginError::new("identity.password_weak", m.to_owned()).with_details(json!({ "errors": [
            { "path": "/password", "code": "identity.password_weak", "message": m }
        ]}))
    };
    let n = password.chars().count();
    if n < MIN_PASSWORD {
        return Err(bad("password must be at least 8 characters"));
    }
    if n > 256 {
        return Err(bad("password is too long"));
    }
    if password.to_lowercase().contains(&username.to_lowercase()) {
        return Err(bad("password must not contain the username"));
    }
    Ok(())
}

fn derive_mode(origin: Origin, req: &RegisterRequest) -> &'static str {
    match origin {
        Origin::Admin => "admin",
        Origin::Service => "application",
        Origin::Http if req.invite_code.as_deref().is_some_and(|c| !c.is_empty()) => "invite",
        Origin::Http => "open",
    }
}

fn nonce(ctx: &Ctx) -> [u8; 12] {
    let b = ctx.random.bytes(12);
    let mut n = [0u8; 12];
    n.copy_from_slice(&b[..12]);
    n
}

/// 注册（§1.2 注册算法）。返回用户 ID。
pub async fn register(id: &Identity, ctx: &Ctx, req: RegisterRequest, origin: Origin) -> PResult<i64> {
    let keys = id.keys()?;
    let (display, canonical) = canon_username(&req.username, true)?;
    let (email, domain) = canon_email(&req.email)?;
    password_policy(&req.password, &display)?;
    let email_mac = keys.email_mac(&email);
    {
        let mut conn = ctx.sql.conn().await?;
        let taken: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE username_canonical = $1)")
                .bind(&canonical)
                .fetch_one(&mut *conn)
                .await?;
        if taken {
            return Err(PluginError::new(
                "identity.username_taken",
                "username is already taken",
            ));
        }
        let email_taken: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM user_emails WHERE email_mac = $1)")
                .bind(&email_mac)
                .fetch_one(&mut *conn)
                .await?;
        if email_taken {
            return Err(PluginError::new(
                "identity.email_taken",
                "email is already registered",
            ));
        }
    }
    let ip = ctx.invocation.client_ip.clone();
    let mut input = json!({
        "version": 1,
        "actor": ctx.invocation.actor,
        "username": display,
        "email_domain": domain,
        "email": email,
        "locale": req.locale.clone().unwrap_or_else(|| ctx.invocation.locale.clone()),
        "form": if req.form.is_null() { json!({}) } else { req.form.clone() },
        "attributes": { "registration_mode": derive_mode(origin, &req) },
    });
    if let Some(code) = req.invite_code.as_deref().filter(|c| !c.is_empty()) {
        input["invite_code"] = json!(code);
    }
    if let Some(ip) = &ip {
        input["ip"] = json!(ip);
        input["ip_hmac"] = json!(hex::encode(keys.ip_mac(ip)));
        input["ip_prefix_hmac"] = json!(hex::encode(keys.ip_prefix_mac(ip)));
    }
    let attrs = match ctx.hooks.run("user.before_register", input).await? {
        HookOutcome::Denied {
            code,
            message,
            details,
            by_plugin,
        } => {
            tracing::info!(by = %by_plugin, %code, "registration denied by hook");
            let mut e = PluginError::new(code, message);
            e.details = details;
            return Err(e);
        }
        HookOutcome::Allowed(out) => out.get("attributes").cloned().unwrap_or_else(|| json!({})),
    };

    let hash = crypto::hash_password(req.password.clone()).await?;
    let passkey = hex::encode(ctx.random.bytes(16));
    let class_id = attrs
        .get("initial_class_id")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(0);
    // 邮箱验证：HTTP 注册在验证前为待验证（管理员与服务建号不需要）
    let verify_email =
        origin == Origin::Http && crate::policy::Policy::load(ctx).await.require_email_verification;
    let status = match attrs.get("initial_status").and_then(serde_json::Value::as_str) {
        Some("pending") => "pending",
        _ if verify_email => "pending",
        _ => "active",
    };
    let mode = attrs
        .get("registration_mode")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("open")
        .to_owned();
    let inviter = attrs.get("inviter_id").and_then(serde_json::Value::as_i64);
    let tags: Vec<String> = attrs
        .get("tags")
        .and_then(serde_json::Value::as_array)
        .map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    let locale = req.locale.unwrap_or_else(|| ctx.invocation.locale.clone());

    let uid: i64 = {
        let mut conn = ctx.sql.conn().await?;
        let r = sqlx::query_scalar(
            "INSERT INTO users (username, username_canonical, class_id, status, locale, registration_mode, inviter_id, tags)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id",
        )
        .bind(&display)
        .bind(&canonical)
        .bind(i32::try_from(class_id).unwrap_or(0))
        .bind(status)
        .bind(&locale)
        .bind(&mode)
        .bind(inviter)
        .bind(&tags)
        .fetch_one(&mut *conn)
        .await;
        let uid: i64 = match r {
            Ok(v) => v,
            Err(sqlx::Error::Database(db)) if db.constraint() == Some("users_username_canonical_key") => {
                return Err(PluginError::new(
                    "identity.username_taken",
                    "username is already taken",
                ));
            }
            Err(e) => return Err(e.into()),
        };
        let r = sqlx::query(
            "INSERT INTO user_emails (user_id, email_enc, email_mac, email_domain) VALUES ($1,$2,$3,$4)",
        )
        .bind(uid)
        .bind(keys.email_seal(uid, &email, nonce(ctx))?)
        .bind(&email_mac)
        .bind(&domain)
        .execute(&mut *conn)
        .await;
        match r {
            Ok(_) => {}
            Err(sqlx::Error::Database(db)) if db.constraint() == Some("user_emails_email_mac_key") => {
                return Err(PluginError::new(
                    "identity.email_taken",
                    "email is already registered",
                ));
            }
            Err(e) => return Err(e.into()),
        }
        sqlx::query("INSERT INTO user_passwords (user_id, algo, phc) VALUES ($1, 'argon2id', $2)")
            .bind(uid)
            .bind(&hash)
            .execute(&mut *conn)
            .await?;
        sqlx::query("INSERT INTO passkeys (user_id, passkey_enc, passkey_mac) VALUES ($1,$2,$3)")
            .bind(uid)
            .bind(keys.passkey_seal(uid, &passkey, nonce(ctx))?)
            .bind(keys.passkey_mac(&passkey))
            .execute(&mut *conn)
            .await?;
        if verify_email {
            sqlx::query(
                "INSERT INTO restrictions (user_id, kind, source, reason_code) VALUES ($1, 'pending', 'identity', 'email_unverified')",
            )
            .bind(uid)
            .execute(&mut *conn)
            .await?;
        }
        uid
    };
    if verify_email {
        crate::challenges::issue(id, ctx, uid, crate::challenges::EMAIL_VERIFY).await?;
    }
    if let Some(ip) = ip.as_deref() {
        crate::risk::ip_log_upsert(ctx, keys, uid, ip, "register").await?;
    }
    let mut public_attrs = serde_json::Map::new();
    for k in [
        "invite_id",
        "inviter_id",
        "initial_status",
        "initial_class_id",
        "tags",
        "registration_mode",
    ] {
        if let Some(v) = attrs.get(k) {
            public_attrs.insert(k.to_owned(), v.clone());
        }
    }
    ctx.events
        .publish(NewEvent::v1(
            "user.registered",
            format!("user:{uid}"),
            json!({ "user_id": uid, "username": display, "status": status, "inviter_id": inviter,
                    "registration_mode": mode, "class_id": class_id, "attributes": public_attrs }),
        ))
        .await?;
    Ok(uid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usernames() {
        assert_eq!(
            canon_username(" Alice_01 ", false).ok(),
            Some(("Alice_01".into(), "alice_01".into()))
        );
        assert!(canon_username("ab", false).is_err());
        assert!(canon_username("_alice", false).is_err());
        assert!(canon_username("al..ice", false).is_err());
        assert!(canon_username("al ice", false).is_err());
        assert!(canon_username("Admin", false).is_err());
        assert!(canon_username("张三丰", true).is_ok());
        assert!(canon_username("张三丰", false).is_err());
    }

    #[test]
    fn emails() {
        assert_eq!(
            canon_email(" Bob@Example.COM ").ok(),
            Some(("bob@example.com".into(), "example.com".into()))
        );
        assert!(canon_email("bob@localhost").is_err());
        assert!(canon_email("bob..x@example.com").is_err());
        assert!(canon_email("@example.com").is_err());
    }

    #[test]
    fn passwords() {
        assert!(password_policy("short", "alice").is_err());
        assert!(password_policy("xxalicexx", "alice").is_err());
        assert!(password_policy("correct horse", "alice").is_ok());
    }
}
