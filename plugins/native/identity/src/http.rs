//! HTTP 路由（identity 03 §2.4 子集）。

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use pt_plugin_api::{PResult, PluginError, RequestCtx, json};
use serde::Deserialize;

use crate::{
    Identity, accounts,
    accounts::{Origin, RegisterRequest},
    auth, challenges,
    policy::Policy,
    rbac, risk, settings, status, tokens, two_factor, webauthn,
};

pub fn router(id: Arc<Identity>) -> Router {
    Router::new()
        .route("/v1/sessions/current", delete(logout))
        .route("/v1/users", post(register))
        .route("/v1/users/me", get(me).patch(settings::patch_me))
        .route("/v1/users/me/profile", get(settings::get_profile))
        .route(
            "/v1/users/me/password",
            axum::routing::put(settings::change_password),
        )
        .route("/v1/users/me/passkey-resets", post(settings::reset_passkey))
        .route("/v1/sessions", get(settings::list_sessions).post(login))
        .route("/v1/sessions/revocations", post(settings::revoke_others))
        .route("/v1/sessions/{sid}", delete(settings::revoke_session))
        .route("/v1/admin/users", get(settings::admin_list))
        .route(
            "/v1/admin/users/{id}",
            get(settings::admin_get).patch(settings::admin_patch),
        )
        .route(
            "/v1/admin/users/{id}/session-revocations",
            post(settings::admin_revoke_sessions),
        )
        .route(
            "/v1/admin/users/{id}/roles/{role}",
            delete(settings::admin_remove_role),
        )
        .route("/v1/users/me/permissions", get(my_permissions))
        .route(
            "/v1/users/me/tokens",
            get(tokens::http_list).post(tokens::http_create),
        )
        .route("/v1/users/me/tokens/{tid}", delete(tokens::http_revoke))
        .route("/v1/users/me/passkey", get(my_passkey))
        .route("/v1/users/{id}", get(get_user))
        .route("/v1/admin/users/{id}/roles", post(grant_role))
        .route(
            "/v1/admin/users/{id}/restrictions",
            get(list_restrictions).post(apply_restriction),
        )
        .route(
            "/v1/admin/users/{id}/restrictions/{rid}",
            delete(lift_restriction),
        )
        .route("/v1/users/me/security-events", get(my_security_events))
        .route("/v1/users/me/ip-history", get(my_ip_history))
        .route("/v1/admin/users/{id}/ip-history", get(admin_ip_history))
        .route("/v1/admin/users/{id}/shared-ips", get(admin_shared_ips))
        .route(
            "/v1/admin/users/{id}/security-events",
            get(admin_security_events),
        )
        .route("/v1/admin/ip-lookups", get(admin_ip_lookup))
        .route("/v1/email-verifications", post(resend_verification))
        .route("/v1/email-verifications/confirm", post(confirm_email))
        .route("/v1/password-resets", post(request_reset))
        .route("/v1/password-resets/confirm", post(confirm_reset))
        .route("/v1/sessions/two-factor", post(login_two_factor))
        .route("/v1/sessions/current/reauth", post(reauth))
        .route("/v1/users/me/two-factor", get(two_factor_status))
        .route("/v1/users/me/totp", post(totp_begin).delete(totp_disable))
        .route("/v1/users/me/totp/confirm", post(totp_confirm))
        .route("/v1/users/me/recovery-codes", post(recovery_regenerate))
        .route(
            "/v1/sessions/two-factor/webauthn-options",
            post(two_factor_webauthn_options),
        )
        .route("/v1/webauthn/assertion-options", post(passwordless_options))
        .route("/v1/sessions/webauthn", post(passwordless_login))
        .route(
            "/v1/users/me/webauthn/registration-options",
            post(webauthn_registration_options),
        )
        .route("/v1/users/me/webauthn-credentials", post(webauthn_register))
        .route(
            "/v1/users/me/webauthn-credentials/{cid}",
            delete(webauthn_remove),
        )
        .with_state(id)
}

async fn two_factor_status(ctx: RequestCtx) -> PResult<Response> {
    let uid = me_id(&ctx)?;
    Ok(Json(two_factor::status(&ctx, uid).await?).into_response())
}

fn no_store(v: serde_json::Value) -> Response {
    let mut resp = Json(v).into_response();
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

async fn totp_begin(State(id): State<Arc<Identity>>, ctx: RequestCtx) -> PResult<Response> {
    Ok(no_store(two_factor::begin(&id, &ctx).await?))
}

#[derive(Debug, Deserialize)]
struct CodeBody {
    code: String,
}

async fn totp_confirm(
    State(id): State<Arc<Identity>>,
    ctx: RequestCtx,
    Json(b): Json<CodeBody>,
) -> PResult<Response> {
    let codes = two_factor::confirm(&id, &ctx, &b.code).await?;
    Ok(no_store(json!({ "recovery_codes": codes })))
}

async fn totp_disable(ctx: RequestCtx) -> PResult<Response> {
    two_factor::disable(&ctx).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn recovery_regenerate(ctx: RequestCtx) -> PResult<Response> {
    let codes = two_factor::regenerate(&ctx).await?;
    Ok(no_store(json!({ "recovery_codes": codes })))
}

#[derive(Debug, Deserialize)]
struct ReauthBody {
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    code: Option<String>,
}

async fn reauth(
    State(id): State<Arc<Identity>>,
    ctx: RequestCtx,
    Json(b): Json<ReauthBody>,
) -> PResult<Response> {
    let at = two_factor::reauth(&id, &ctx, b.password.as_deref(), b.code.as_deref()).await?;
    Ok(Json(json!({ "reauth_at": at })).into_response())
}

async fn resend_verification(State(id): State<Arc<Identity>>, ctx: RequestCtx) -> PResult<Response> {
    let uid = me_id(&ctx)?;
    challenges::resend_verification(&id, &ctx, uid).await?;
    Ok(StatusCode::ACCEPTED.into_response())
}

#[derive(Debug, Deserialize)]
struct ChallengeBody {
    challenge_id: String,
    token: String,
    #[serde(default)]
    new_password: Option<String>,
}

async fn confirm_email(ctx: RequestCtx, Json(b): Json<ChallengeBody>) -> PResult<Response> {
    challenges::confirm_email(&ctx, &b.challenge_id, &b.token).await?;
    Ok(Json(json!({ "verified": true })).into_response())
}

#[derive(Debug, Deserialize)]
struct ResetRequest {
    login: String,
    #[serde(default)]
    captcha_token: Option<String>,
}

/// 永远 202（防枚举）
async fn request_reset(
    State(id): State<Arc<Identity>>,
    ctx: RequestCtx,
    Json(b): Json<ResetRequest>,
) -> PResult<Response> {
    risk::captcha_check(&ctx, "password_reset", b.captcha_token.as_deref()).await?;
    challenges::request_reset(&id, &ctx, &b.login).await?;
    Ok(StatusCode::ACCEPTED.into_response())
}

async fn confirm_reset(ctx: RequestCtx, Json(b): Json<ChallengeBody>) -> PResult<Response> {
    let pw = b
        .new_password
        .as_deref()
        .ok_or_else(|| PluginError::validation("new_password is required"))?;
    challenges::confirm_reset(&ctx, &b.challenge_id, &b.token, pw).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

fn me_id(ctx: &RequestCtx) -> PResult<i64> {
    i64::try_from(ctx.require_user()?).map_err(|_| PluginError::bad_request("bad user id"))
}

async fn my_security_events(ctx: RequestCtx) -> PResult<Response> {
    let uid = me_id(&ctx)?;
    Ok(Json(json!({ "items": risk::security_events_of(&ctx, uid, 50).await? })).into_response())
}

async fn my_ip_history(ctx: RequestCtx) -> PResult<Response> {
    let uid = me_id(&ctx)?;
    let full = Policy::load(&ctx).await.ip_show_full_to_owner;
    Ok(Json(json!({ "items": risk::ip_history(&ctx, uid, full, 100).await? })).into_response())
}

async fn audit_sensitive(ctx: &RequestCtx, target: String, details: serde_json::Value) -> PResult<()> {
    ctx.audit
        .write(pt_plugin_api::AuditEntry {
            action: "identity.sensitive_read".into(),
            target,
            details,
        })
        .await
}

async fn admin_ip_history(ctx: RequestCtx, Path(uid): Path<i64>) -> PResult<Response> {
    audit_sensitive(&ctx, format!("user:{uid}"), json!({ "field": "ip_history" })).await?;
    Ok(Json(json!({ "items": risk::ip_history(&ctx, uid, true, 500).await? })).into_response())
}

async fn admin_shared_ips(ctx: RequestCtx, Path(uid): Path<i64>) -> PResult<Response> {
    audit_sensitive(&ctx, format!("user:{uid}"), json!({ "field": "shared_ips" })).await?;
    Ok(Json(json!({ "items": risk::shared_ip_users(&ctx, uid).await? })).into_response())
}

async fn admin_security_events(ctx: RequestCtx, Path(uid): Path<i64>) -> PResult<Response> {
    Ok(Json(json!({ "items": risk::security_events_of(&ctx, uid, 200).await? })).into_response())
}

#[derive(Debug, Deserialize)]
struct IpLookup {
    ip: String,
}

async fn admin_ip_lookup(
    State(id): State<Arc<Identity>>,
    ctx: RequestCtx,
    axum::extract::Query(q): axum::extract::Query<IpLookup>,
) -> PResult<Response> {
    let items = risk::users_by_ip(&ctx, id.keys()?, &q.ip).await?;
    audit_sensitive(
        &ctx,
        "ip".into(),
        json!({ "field": "ip_lookup", "query": crate::crypto::truncate_ip(&q.ip), "results": items.len() }),
    )
    .await?;
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
struct LoginBody {
    username: String,
    password: String,
    /// 缺省为保持登录（与网页登录表单的默认值一致）
    #[serde(default = "yes")]
    remember: bool,
    /// 人机验证令牌（captcha 插件的前端写入）
    #[serde(default)]
    captcha_token: Option<String>,
}

fn yes() -> bool {
    true
}

/// `insecure` = `[plugin.identity] insecure_cookies`：无 `__Host-` 前缀、无 `Secure`，仅供 HTTP 测试部署。
fn cookie(name: &str, value: &str, max_age: i64, http_only: bool, insecure: bool) -> HeaderValue {
    let name = if insecure {
        name.trim_start_matches("__Host-")
    } else {
        name
    };
    let secure = if insecure { "" } else { "; Secure" };
    // > 0：持久 cookie；0：浏览器会话 cookie；< 0：立即删除（登出）
    let age = match max_age {
        0 => String::new(),
        n if n < 0 => "; Max-Age=0".to_owned(),
        n => format!("; Max-Age={n}"),
    };
    let ho = if http_only { "; HttpOnly" } else { "" };
    HeaderValue::from_str(&format!("{name}={value}; Path=/{secure}{ho}; SameSite=Lax{age}"))
        .unwrap_or_else(|_| HeaderValue::from_static(""))
}

async fn login(
    State(id): State<Arc<Identity>>,
    ctx: RequestCtx,
    headers: HeaderMap,
    Json(b): Json<LoginBody>,
) -> PResult<Response> {
    let dev = risk::device_cookie(&headers);
    risk::captcha_check(&ctx, "login", b.captcha_token.as_deref()).await?;
    match auth::login_password(&id, &ctx, &b.username, &b.password, b.remember, dev.as_deref()).await? {
        auth::LoginOutcome::Session(s) => Ok(session_response(&id, &s)),
        auth::LoginOutcome::SecondFactor { ticket, methods } => {
            let mut resp = Json(json!({ "two_factor_required": true, "ticket": ticket, "methods": methods })).into_response();
            resp.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            Ok(resp)
        }
    }
}

/// 登录成功：会话 / CSRF cookie（以及新设备 cookie）
fn session_response(id: &Identity, s: &auth::NewSession) -> Response {
    let body = json!({
        "session_id": s.session_id,
        "user": { "id": s.user_id, "username": s.username, "class_id": s.class_id, "status": s.status },
        "two_factor_passed": s.second_factor.is_some(),
        "csrf_token": s.csrf,
    });
    let mut resp = (StatusCode::OK, Json(body)).into_response();
    let h = resp.headers_mut();
    h.append(
        header::SET_COOKIE,
        cookie(
            "__Host-pt_session",
            &s.token,
            s.max_age_secs,
            true,
            id.insecure_cookies(),
        ),
    );
    h.append(
        header::SET_COOKIE,
        cookie(
            "__Host-pt_csrf",
            &s.csrf,
            s.max_age_secs,
            false,
            id.insecure_cookies(),
        ),
    );
    if let Some(d) = &s.device_cookie {
        h.append(
            header::SET_COOKIE,
            cookie(
                risk::DEVICE_COOKIE,
                d,
                risk::DEVICE_MAX_AGE,
                true,
                id.insecure_cookies(),
            ),
        );
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

#[derive(Debug, Deserialize)]
struct SecondStep {
    ticket: String,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    assertion: Option<serde_json::Value>,
}

async fn login_two_factor(
    State(id): State<Arc<Identity>>,
    ctx: RequestCtx,
    headers: HeaderMap,
    Json(b): Json<SecondStep>,
) -> PResult<Response> {
    let dev = risk::device_cookie(&headers);
    let s = two_factor::login_second_step(
        &id,
        &ctx,
        &b.ticket,
        b.code.as_deref(),
        b.assertion.as_ref(),
        dev.as_deref(),
    )
    .await?;
    Ok(session_response(&id, &s))
}

#[derive(Debug, Deserialize)]
struct TicketBody {
    ticket: String,
}

async fn two_factor_webauthn_options(ctx: RequestCtx, Json(b): Json<TicketBody>) -> PResult<Response> {
    Ok(no_store(webauthn::ticket_options(&ctx, &b.ticket).await?))
}

async fn passwordless_options(ctx: RequestCtx) -> PResult<Response> {
    Ok(no_store(webauthn::passwordless_options(&ctx).await?))
}

#[derive(Debug, Deserialize)]
struct PasswordlessBody {
    ceremony_id: String,
    assertion: serde_json::Value,
}

async fn passwordless_login(
    State(id): State<Arc<Identity>>,
    ctx: RequestCtx,
    headers: HeaderMap,
    Json(b): Json<PasswordlessBody>,
) -> PResult<Response> {
    let dev = risk::device_cookie(&headers);
    let s = webauthn::passwordless_login(&id, &ctx, &b.ceremony_id, &b.assertion, dev.as_deref()).await?;
    Ok(session_response(&id, &s))
}

async fn webauthn_registration_options(ctx: RequestCtx) -> PResult<Response> {
    Ok(no_store(webauthn::registration_options(&ctx).await?))
}

#[derive(Debug, Deserialize)]
struct NewCredential {
    ceremony_id: String,
    name: String,
    credential: serde_json::Value,
}

async fn webauthn_register(ctx: RequestCtx, Json(b): Json<NewCredential>) -> PResult<Response> {
    let r = webauthn::register(&ctx, &b.ceremony_id, &b.name, &b.credential).await?;
    Ok((StatusCode::CREATED, no_store(r)).into_response())
}

async fn webauthn_remove(ctx: RequestCtx, Path(cid): Path<i64>) -> PResult<Response> {
    webauthn::remove(&ctx, cid).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn logout(State(id): State<Arc<Identity>>, ctx: RequestCtx) -> PResult<Response> {
    auth::logout(&ctx).await?;
    let mut resp = StatusCode::NO_CONTENT.into_response();
    let h = resp.headers_mut();
    h.append(
        header::SET_COOKIE,
        cookie("__Host-pt_session", "", -1, true, id.insecure_cookies()),
    );
    h.append(
        header::SET_COOKIE,
        cookie("__Host-pt_csrf", "", -1, false, id.insecure_cookies()),
    );
    Ok(resp)
}

async fn register(
    State(id): State<Arc<Identity>>,
    ctx: RequestCtx,
    Json(req): Json<RegisterRequest>,
) -> PResult<Response> {
    risk::captcha_check(&ctx, "register", req.captcha_token.as_deref()).await?;
    let uid = accounts::register(&id, &ctx, req, Origin::Http).await?;
    let u = id
        .summary(&ctx, uid)
        .await?
        .ok_or_else(|| PluginError::internal("user vanished"))?;
    Ok((StatusCode::CREATED, Json(json!({ "user": u }))).into_response())
}

async fn me(State(id): State<Arc<Identity>>, ctx: RequestCtx) -> PResult<Response> {
    let uid = i64::try_from(ctx.require_user()?).map_err(|_| PluginError::bad_request("bad user id"))?;
    let u = id
        .summary(&ctx, uid)
        .await?
        .ok_or_else(|| PluginError::not_found("user not found"))?;
    let (_, roles) = rbac::effective_roles(&ctx, uid).await?;
    let mut roles: Vec<String> = roles.into_iter().collect();
    roles.sort();
    let verified: Option<bool> = {
        let mut conn = ctx.sql.conn().await?;
        sqlx::query_scalar("SELECT verified_at IS NOT NULL FROM user_emails WHERE user_id = $1")
            .bind(uid)
            .fetch_optional(&mut *conn)
            .await?
    };
    Ok(Json(json!({ "user": u, "roles": roles, "email_verified": verified.unwrap_or(false) })).into_response())
}

async fn my_permissions(ctx: RequestCtx) -> PResult<Response> {
    let uid = i64::try_from(ctx.require_user()?).map_err(|_| PluginError::bad_request("bad user id"))?;
    // 与网关同一规则：未通过 2FA 的会话不享有要求 2FA 的角色
    let sid = ctx
        .actor()
        .credential_id
        .as_deref()
        .and_then(|c| uuid::Uuid::parse_str(c).ok());
    let passed: Option<bool> = match sid {
        Some(sid) => {
            let mut conn = ctx.sql.conn().await?;
            sqlx::query_scalar("SELECT two_factor_at IS NOT NULL FROM sessions WHERE id = $1")
                .bind(sid)
                .fetch_optional(&mut *conn)
                .await?
        }
        None => None,
    };
    let cap = match passed {
        Some(false) => Policy::load(&ctx).await.two_factor_required_rank,
        _ => None,
    };
    let (_, roles) = rbac::effective_roles(&ctx, uid).await?;
    let needed = cap.is_some_and(|c| rbac::top_rank(&roles) >= c);
    Ok(Json(json!({ "permissions": rbac::permissions_capped(&ctx, uid, cap).await?, "two_factor_needed": needed })).into_response())
}

async fn my_passkey(State(id): State<Arc<Identity>>, ctx: RequestCtx) -> PResult<Response> {
    // 仅会话凭据（token 不能读取 passkey）
    let uid = ctx.require_user()?;
    let is_session = ctx
        .actor()
        .credential_id
        .as_deref()
        .is_some_and(|c| uuid::Uuid::parse_str(c).is_ok());
    if !is_session {
        return Err(PluginError::forbidden(
            "passkey is only available to browser sessions",
        ));
    }
    let pk = id
        .passkey(&ctx, uid)
        .await?
        .ok_or_else(|| PluginError::new("identity.passkey_revoked", "passkey has been revoked"))?;
    let mut resp = Json(json!({ "passkey": pk })).into_response();
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(resp)
}

async fn get_user(
    State(id): State<Arc<Identity>>,
    ctx: RequestCtx,
    Path(uid): Path<i64>,
) -> PResult<Response> {
    let u = id
        .summary(&ctx, uid)
        .await?
        .ok_or_else(|| PluginError::not_found("user not found"))?;
    Ok(Json(u).into_response())
}

#[derive(Debug, Deserialize)]
struct GrantBody {
    role: String,
}

async fn grant_role(ctx: RequestCtx, Path(uid): Path<i64>, Json(b): Json<GrantBody>) -> PResult<Response> {
    let by = ctx.require_user()?;
    // 管理员只能授予低于自己最高 rank 的内置角色（03 §1.10）
    let (_, mine) = rbac::effective_roles(&ctx, i64::try_from(by).unwrap_or_default()).await?;
    let rank = |r: &str| rbac::BUILTIN.iter().find(|(n, _)| *n == r).map(|(_, k)| *k);
    let my_rank = mine.iter().filter_map(|r| rank(r)).max().unwrap_or(0);
    match rank(&b.role) {
        Some(k) if k < my_rank => {}
        Some(_) => {
            return Err(PluginError::forbidden(
                "cannot grant a role at or above your own rank",
            ));
        }
        None => {
            return Err(PluginError::new(
                "identity.role_not_found",
                "only built-in roles can be granted in this version",
            ));
        }
    }
    rbac::grant_role(&ctx, uid, &b.role, &by.to_string()).await?;
    ctx.audit
        .write(pt_plugin_api::AuditEntry {
            action: "identity.roles.grant".into(),
            target: format!("user:{uid}"),
            details: json!({ "role": b.role }),
        })
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub(crate) async fn admin_rank_ok(ctx: &pt_plugin_api::Ctx, target: i64) -> PResult<i64> {
    let by = i64::try_from(ctx.require_user()?).unwrap_or_default();
    // 管理员只能限制 rank 低于自己的用户（03 §1.6），sysop 不受限
    let rank = |roles: &std::collections::HashSet<String>| {
        roles
            .iter()
            .filter_map(|r| rbac::BUILTIN.iter().find(|(n, _)| n == r).map(|(_, k)| *k))
            .max()
            .unwrap_or(0)
    };
    let (_, mine) = rbac::effective_roles(ctx, by).await?;
    let (_, theirs) = rbac::effective_roles(ctx, target).await?;
    if !mine.contains("sysop") && rank(&mine) <= rank(&theirs) {
        return Err(PluginError::new(
            "identity.insufficient_rank",
            "cannot restrict a user at or above your own rank",
        )
        .with_status(403));
    }
    Ok(by)
}

async fn list_restrictions(ctx: RequestCtx, Path(uid): Path<i64>) -> PResult<Response> {
    Ok(Json(status::list(&ctx, uid).await?).into_response())
}

#[derive(Debug, Deserialize)]
struct RestrictionBody {
    kind: String,
    reason_code: String,
    note: Option<String>,
    /// Unix 毫秒
    until: Option<i64>,
}

async fn apply_restriction(
    ctx: RequestCtx,
    Path(uid): Path<i64>,
    Json(b): Json<RestrictionBody>,
) -> PResult<Response> {
    let by = admin_rank_ok(&ctx, uid).await?;
    let rid = status::apply(
        &ctx,
        &status::Apply {
            user_id: uid,
            kind: &b.kind,
            source: "admin",
            reason_code: &b.reason_code,
            note: b.note.as_deref(),
            until_ms: b.until,
            by_user: Some(by),
        },
    )
    .await?;
    ctx.audit
        .write(pt_plugin_api::AuditEntry {
            action: "identity.restriction_apply".into(),
            target: format!("user:{uid}"),
            details: json!({ "restriction_id": rid, "kind": b.kind, "reason_code": b.reason_code, "until": b.until }),
        })
        .await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": rid }))).into_response())
}

#[derive(Debug, Default, Deserialize)]
struct LiftQuery {
    reason: Option<String>,
}

async fn lift_restriction(
    ctx: RequestCtx,
    Path((uid, rid)): Path<(i64, i64)>,
    axum::extract::Query(q): axum::extract::Query<LiftQuery>,
) -> PResult<Response> {
    let by = admin_rank_ok(&ctx, uid).await?;
    let reason = q.reason.unwrap_or_else(|| "admin".into());
    if !status::lift_one(&ctx, uid, rid, &reason, Some(by)).await? {
        return Err(PluginError::not_found("restriction not found or already lifted"));
    }
    ctx.audit
        .write(pt_plugin_api::AuditEntry {
            action: "identity.restriction_lift".into(),
            target: format!("user:{uid}"),
            details: json!({ "restriction_id": rid, "reason": reason }),
        })
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
