//! 本人设置与后台用户管理（identity 03 §2.4 子集）：资料、改密码、会话、passkey 重置；用户列表 / 详情 / 编辑 / 撤销会话。

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use pt_plugin_api::{AuditEntry, Ctx, NewEvent, PResult, PluginError, RequestCtx, json};
use serde::Deserialize;
use sqlx::Row;

use crate::{Identity, accounts, crypto, rbac, status};

fn uid_of(ctx: &Ctx) -> PResult<i64> {
    i64::try_from(ctx.require_user()?).map_err(|_| PluginError::bad_request("bad user id"))
}

/// 当前会话 ID（会话凭据的 credential_id）
fn current_session(ctx: &Ctx) -> Option<uuid::Uuid> {
    ctx.actor()
        .credential_id
        .as_deref()
        .and_then(|c| uuid::Uuid::parse_str(c).ok())
}

fn ms(t: chrono::DateTime<chrono::Utc>) -> i64 {
    t.timestamp_millis()
}

// ---------------- 资料 ----------------

/// merge-patch：字段缺省 → None；显式 null → Some(None)
fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Debug, Deserialize)]
pub struct ProfilePatch {
    #[serde(default, deserialize_with = "present")]
    title: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    bio: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    avatar_url: Option<Option<String>>,
    #[serde(default)]
    locale: Option<String>,
}

fn clean(v: Option<String>, max: usize, field: &str) -> PResult<Option<String>> {
    let v = v
        .map(|s| {
            s.trim()
                .chars()
                .filter(|c| !c.is_control() || *c == '\n')
                .collect::<String>()
        })
        .filter(|s| !s.is_empty());
    if v.as_ref().is_some_and(|s| s.chars().count() > max) {
        return Err(
            PluginError::validation(format!("{field} is longer than {max} characters")).with_details(
                json!({ "errors": [{ "path": format!("/{field}"), "code": "kernel.validation_failed" }] }),
            ),
        );
    }
    Ok(v)
}

/// 资料修改（merge-patch：缺省字段不变，null 清空）；返回修改了的字段
async fn update_profile(
    ctx: &Ctx,
    uid: i64,
    p: ProfilePatch,
    locales: &[String],
) -> PResult<Vec<&'static str>> {
    let mut changed = Vec::new();
    let mut c = ctx.sql.conn().await?;
    if let Some(t) = p.title {
        sqlx::query("UPDATE users SET title = $2 WHERE id = $1")
            .bind(uid)
            .bind(clean(t, 30, "title")?)
            .execute(&mut *c)
            .await?;
        changed.push("title");
    }
    if let Some(b) = p.bio {
        sqlx::query("UPDATE users SET bio = $2 WHERE id = $1")
            .bind(uid)
            .bind(clean(b, 2000, "bio")?)
            .execute(&mut *c)
            .await?;
        changed.push("bio");
    }
    if let Some(a) = p.avatar_url {
        let a = clean(a, 512, "avatar_url")?;
        if a.as_ref().is_some_and(|u| !u.starts_with("https://") && !is_builtin_avatar(u)) {
            return Err(PluginError::validation("avatar_url must be an https URL or a built-in avatar"));
        }
        sqlx::query("UPDATE users SET avatar_url = $2 WHERE id = $1")
            .bind(uid)
            .bind(a)
            .execute(&mut *c)
            .await?;
        changed.push("avatar_url");
    }
    if let Some(l) = p.locale {
        if !locales.is_empty() && !locales.contains(&l) {
            return Err(PluginError::validation("unsupported locale"));
        }
        sqlx::query("UPDATE users SET locale = $2 WHERE id = $1")
            .bind(uid)
            .bind(&l)
            .execute(&mut *c)
            .await?;
        changed.push("locale");
    }
    if !changed.is_empty() {
        sqlx::query("UPDATE users SET updated_at = now() WHERE id = $1")
            .bind(uid)
            .execute(&mut *c)
            .await?;
        drop(c);
        ctx.events
            .publish(NewEvent::v1(
                "user.profile_updated",
                format!("user:{uid}"),
                json!({ "user_id": uid, "fields": changed }),
            ))
            .await?;
    }
    Ok(changed)
}

async fn profile_json(ctx: &Ctx, uid: i64) -> PResult<serde_json::Value> {
    let mut c = ctx.sql.conn().await?;
    let r = sqlx::query("SELECT id, username, class_id, status, title, bio, avatar_url, locale, created_at FROM users WHERE id = $1 AND deleted_at IS NULL")
        .bind(uid)
        .fetch_optional(&mut *c)
        .await?
        .ok_or_else(|| PluginError::not_found("user not found"))?;
    Ok(json!({
        "id": r.get::<i64, _>(0), "username": r.get::<String, _>(1), "class_id": r.get::<i32, _>(2), "status": r.get::<String, _>(3),
        "title": r.get::<Option<String>, _>(4), "bio": r.get::<Option<String>, _>(5), "avatar_url": r.get::<Option<String>, _>(6),
        "locale": r.get::<String, _>(7), "created_at": ms(r.get(8)),
    }))
}

pub async fn get_profile(ctx: RequestCtx) -> PResult<Response> {
    let uid = uid_of(&ctx)?;
    Ok(Json(profile_json(&ctx, uid).await?).into_response())
}

pub async fn patch_me(
    State(id): State<Arc<Identity>>,
    ctx: RequestCtx,
    Json(p): Json<ProfilePatch>,
) -> PResult<Response> {
    let uid = uid_of(&ctx)?;
    update_profile(&ctx, uid, p, &id.locales()).await?;
    Ok(Json(profile_json(&ctx, uid).await?).into_response())
}

// ---------------- 密码 ----------------

#[derive(Debug, Deserialize)]
pub struct PasswordChange {
    current_password: String,
    new_password: String,
}

/// 改密码：校验当前密码（代替 reauth），撤销其他会话，发布 `user.password_changed`
pub async fn change_password(ctx: RequestCtx, Json(b): Json<PasswordChange>) -> PResult<Response> {
    let uid = uid_of(&ctx)?;
    let mut c = ctx.sql.conn().await?;
    let row = sqlx::query("SELECT u.username, p.phc FROM users u LEFT JOIN user_passwords p ON p.user_id = u.id WHERE u.id = $1 FOR UPDATE OF u")
        .bind(uid)
        .fetch_one(&mut *c)
        .await?;
    let (username, phc): (String, Option<String>) = (row.get(0), row.get(1));
    drop(c);
    let ok = match phc {
        Some(phc) => crypto::verify_password(phc, b.current_password.clone()).await,
        None => false,
    };
    if !ok {
        return Err(
            PluginError::new("identity.invalid_credentials", "current password is wrong").with_status(403),
        );
    }
    accounts::password_policy(&b.new_password, &username)?;
    let phc = crypto::hash_password(b.new_password).await?;
    let mut c = ctx.sql.conn().await?;
    sqlx::query(
        "INSERT INTO user_passwords (user_id, algo, phc) VALUES ($1, 'argon2id', $2)
         ON CONFLICT (user_id) DO UPDATE SET algo = 'argon2id', phc = EXCLUDED.phc, legacy_hash = NULL, salt = NULL, changed_at = now()",
    )
    .bind(uid)
    .bind(&phc)
    .execute(&mut *c)
    .await?;
    let n = sqlx::query("UPDATE sessions SET revoked_at = now(), revoked_reason = 'password_changed' WHERE user_id = $1 AND revoked_at IS NULL AND id IS DISTINCT FROM $2")
        .bind(uid)
        .bind(current_session(&ctx))
        .execute(&mut *c)
        .await?
        .rows_affected();
    drop(c);
    ctx.events
        .publish(NewEvent::v1(
            "user.password_changed",
            format!("user:{uid}"),
            json!({ "user_id": uid }),
        ))
        .await?;
    Ok(Json(json!({ "revoked_sessions": n })).into_response())
}

// ---------------- 会话 ----------------

async fn sessions_of(ctx: &Ctx, uid: i64) -> PResult<Vec<serde_json::Value>> {
    let cur = current_session(ctx);
    let mut c = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT id, method, ua_summary, created_at, last_seen_at, remember FROM sessions
          WHERE user_id = $1 AND revoked_at IS NULL AND absolute_expires_at > now() AND idle_expires_at > now()
          ORDER BY last_seen_at DESC LIMIT 100",
    )
    .bind(uid)
    .fetch_all(&mut *c)
    .await?;
    Ok(rows
        .iter()
        .map(|r| {
            let id: uuid::Uuid = r.get(0);
            json!({ "id": id, "method": r.get::<String, _>(1), "user_agent": r.get::<Option<String>, _>(2),
                    "created_at": ms(r.get(3)), "last_seen_at": ms(r.get(4)), "remember": r.get::<bool, _>(5),
                    "current": Some(id) == cur })
        })
        .collect())
}

pub async fn list_sessions(ctx: RequestCtx) -> PResult<Response> {
    let uid = uid_of(&ctx)?;
    Ok(Json(json!({ "items": sessions_of(&ctx, uid).await? })).into_response())
}

pub async fn revoke_session(ctx: RequestCtx, Path(sid): Path<uuid::Uuid>) -> PResult<Response> {
    let uid = uid_of(&ctx)?;
    let mut c = ctx.sql.conn().await?;
    let n = sqlx::query("UPDATE sessions SET revoked_at = now(), revoked_reason = 'user' WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL")
        .bind(sid)
        .bind(uid)
        .execute(&mut *c)
        .await?
        .rows_affected();
    if n == 0 {
        return Err(PluginError::not_found("session not found"));
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Debug, Deserialize)]
pub struct Revocation {
    scope: String,
}

/// `{scope: "others"}`：撤销除当前外的全部会话
pub async fn revoke_others(ctx: RequestCtx, Json(b): Json<Revocation>) -> PResult<Response> {
    if b.scope != "others" {
        return Err(PluginError::validation("scope must be `others`"));
    }
    let uid = uid_of(&ctx)?;
    let mut c = ctx.sql.conn().await?;
    let n = sqlx::query("UPDATE sessions SET revoked_at = now(), revoked_reason = 'user_others' WHERE user_id = $1 AND revoked_at IS NULL AND id IS DISTINCT FROM $2")
        .bind(uid)
        .bind(current_session(&ctx))
        .execute(&mut *c)
        .await?
        .rows_affected();
    Ok(Json(json!({ "revoked": n })).into_response())
}

// ---------------- passkey ----------------

fn nonce(ctx: &Ctx) -> [u8; 12] {
    let b = ctx.random.bytes(12);
    let mut n = [0u8; 12];
    n.copy_from_slice(&b[..12]);
    n
}

/// 重置 passkey：旧 passkey 立即失效（tracker-bridge 订阅 `user.passkey_reset` 后刷新缓存）
pub async fn reset_passkey(State(id): State<Arc<Identity>>, ctx: RequestCtx) -> PResult<Response> {
    if current_session(&ctx).is_none() {
        return Err(PluginError::forbidden(
            "passkey can only be reset from a browser session",
        ));
    }
    let uid = uid_of(&ctx)?;
    let keys = id.keys()?;
    let passkey = hex::encode(ctx.random.bytes(16));
    let mut c = ctx.sql.conn().await?;
    sqlx::query(
        "INSERT INTO passkeys (user_id, passkey_enc, passkey_mac) VALUES ($1, $2, $3)
         ON CONFLICT (user_id) DO UPDATE SET passkey_enc = EXCLUDED.passkey_enc, passkey_mac = EXCLUDED.passkey_mac,
                version = passkeys.version + 1, revoked_at = NULL, updated_at = now()",
    )
    .bind(uid)
    .bind(keys.passkey_seal(uid, &passkey, nonce(&ctx))?)
    .bind(keys.passkey_mac(&passkey))
    .execute(&mut *c)
    .await?;
    drop(c);
    ctx.events
        .publish(NewEvent::v1(
            "user.passkey_reset",
            format!("user:{uid}"),
            json!({ "user_id": uid }),
        ))
        .await?;
    let mut resp = Json(json!({ "passkey": passkey })).into_response();
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(resp)
}

// ---------------- 后台 ----------------

#[derive(Debug, Deserialize)]
pub struct UserQuery {
    status: Option<String>,
    class_id: Option<i32>,
    username_prefix: Option<String>,
    role: Option<String>,
    cursor: Option<i64>,
    limit: Option<i64>,
}

pub async fn admin_list(ctx: RequestCtx, Query(q): Query<UserQuery>) -> PResult<Response> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let prefix = q.username_prefix.map(|p| {
        accounts::canon_username(&p, true)
            .map(|(_, c)| c)
            .unwrap_or(p.to_lowercase())
    });
    let mut c = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT u.id, u.username, u.class_id, u.status, u.created_at, u.last_seen_at,
                (SELECT array_agg(role ORDER BY role) FROM user_roles r WHERE r.user_id = u.id AND (r.expires_at IS NULL OR r.expires_at > now()))
           FROM users u
          WHERE u.deleted_at IS NULL AND u.id < $1
            AND ($2::text IS NULL OR u.status = $2) AND ($3::int IS NULL OR u.class_id = $3)
            AND ($4::text IS NULL OR u.username_canonical LIKE replace(replace($4, '%', '\\%'), '_', '\\_') || '%')
            AND ($5::text IS NULL OR EXISTS (SELECT 1 FROM user_roles r WHERE r.user_id = u.id AND r.role = $5))
          ORDER BY u.id DESC LIMIT $6",
    )
    .bind(q.cursor.unwrap_or(i64::MAX))
    .bind(&q.status)
    .bind(q.class_id)
    .bind(&prefix)
    .bind(&q.role)
    .bind(limit + 1)
    .fetch_all(&mut *c)
    .await?;
    let more = rows.len() as i64 > limit;
    let items: Vec<serde_json::Value> = rows
        .iter()
        .take(usize::try_from(limit).unwrap_or(50))
        .map(|r| {
            json!({ "id": r.get::<i64, _>(0), "username": r.get::<String, _>(1), "class_id": r.get::<i32, _>(2),
                    "status": r.get::<String, _>(3), "created_at": ms(r.get(4)),
                    "last_seen_at": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>(5).map(ms),
                    "roles": r.get::<Option<Vec<String>>, _>(6).unwrap_or_default() })
        })
        .collect();
    let next = if more {
        items.last().map(|i| i["id"].clone())
    } else {
        None
    };
    Ok(Json(json!({ "items": items, "next_cursor": next })).into_response())
}

pub async fn admin_get(ctx: RequestCtx, Path(uid): Path<i64>) -> PResult<Response> {
    let mut v = profile_json(&ctx, uid).await?;
    let (_, roles) = rbac::effective_roles(&ctx, uid).await?;
    let mut roles: Vec<String> = roles.into_iter().collect();
    roles.sort();
    let mut c = ctx.sql.conn().await?;
    let granted =
        sqlx::query("SELECT role, granted_by, expires_at FROM user_roles WHERE user_id = $1 ORDER BY role")
            .bind(uid)
            .fetch_all(&mut *c)
            .await?;
    drop(c);
    v["roles"] = json!(roles);
    v["granted_roles"] = json!(
        granted
            .iter()
            .map(
                |r| json!({ "role": r.get::<String, _>(0), "granted_by": r.get::<String, _>(1),
                          "expires_at": r.get::<Option<chrono::DateTime<chrono::Utc>>, _>(2).map(ms) })
            )
            .collect::<Vec<_>>()
    );
    v["sessions"] = json!(sessions_of(&ctx, uid).await?.len());
    v["restrictions"] = status::list(&ctx, uid).await?["items"].clone();
    Ok(Json(v).into_response())
}

pub async fn admin_patch(
    State(id): State<Arc<Identity>>,
    ctx: RequestCtx,
    Path(uid): Path<i64>,
    Json(p): Json<ProfilePatch>,
) -> PResult<Response> {
    let changed = update_profile(&ctx, uid, ProfilePatch { locale: None, ..p }, &id.locales()).await?;
    ctx.audit
        .write(AuditEntry {
            action: "identity.users.update".into(),
            target: format!("user:{uid}"),
            details: json!({ "fields": changed }),
        })
        .await?;
    Ok(Json(profile_json(&ctx, uid).await?).into_response())
}

pub async fn admin_revoke_sessions(ctx: RequestCtx, Path(uid): Path<i64>) -> PResult<Response> {
    let mut c = ctx.sql.conn().await?;
    let n = sqlx::query("UPDATE sessions SET revoked_at = now(), revoked_reason = 'admin' WHERE user_id = $1 AND revoked_at IS NULL")
        .bind(uid)
        .execute(&mut *c)
        .await?
        .rows_affected();
    drop(c);
    ctx.audit
        .write(AuditEntry {
            action: "identity.sessions.revoke".into(),
            target: format!("user:{uid}"),
            details: json!({ "revoked": n }),
        })
        .await?;
    Ok(Json(json!({ "revoked": n })).into_response())
}

pub async fn admin_remove_role(ctx: RequestCtx, Path((uid, role)): Path<(i64, String)>) -> PResult<Response> {
    let by = uid_of(&ctx)?;
    // 只能移除低于自己最高 rank 的内置角色（与授予规则一致）
    let (_, mine) = rbac::effective_roles(&ctx, by).await?;
    let rank = |r: &str| rbac::BUILTIN.iter().find(|(n, _)| *n == r).map(|(_, k)| *k);
    let my_rank = mine.iter().filter_map(|r| rank(r)).max().unwrap_or(0);
    if rank(&role).is_none_or(|k| k >= my_rank) {
        return Err(PluginError::forbidden(
            "cannot remove a role at or above your own rank",
        ));
    }
    let mut c = ctx.sql.conn().await?;
    let n = sqlx::query("DELETE FROM user_roles WHERE user_id = $1 AND role = $2")
        .bind(uid)
        .bind(&role)
        .execute(&mut *c)
        .await?
        .rows_affected();
    drop(c);
    if n == 0 {
        return Err(PluginError::not_found("role is not granted"));
    }
    ctx.audit
        .write(AuditEntry {
            action: "identity.roles.revoke".into(),
            target: format!("user:{uid}"),
            details: json!({ "role": role }),
        })
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// 内置头像（web/assets/avatars，由 web/tools/avatars.py 生成）：注册时随机分配一个，设置页可改选
pub const BUILTIN_AVATARS: &[&str] = &["frieren", "fern", "stark", "himmel", "heiter", "eisen", "sein", "ubel", "land", "denken", "serie", "flamme", "methode", "lawine"];

pub fn builtin_avatars() -> Vec<String> {
    BUILTIN_AVATARS.iter().map(|s| format!("/avatars/{s}.svg")).collect()
}

fn is_builtin_avatar(u: &str) -> bool {
    u.strip_prefix("/avatars/")
        .and_then(|r| r.strip_suffix(".svg"))
        .is_some_and(|s| BUILTIN_AVATARS.contains(&s))
}
