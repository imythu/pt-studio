//! HTTP 路由（identity 03 §2.4 子集）。

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use pt_plugin_api::{PResult, PluginError, RequestCtx, json};
use serde::Deserialize;

use crate::{
    Identity, accounts,
    accounts::{Origin, RegisterRequest},
    auth, rbac, settings, status,
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
        .with_state(id)
}

#[derive(Debug, Deserialize)]
struct LoginBody {
    username: String,
    password: String,
    #[serde(default)]
    remember: bool,
}

fn cookie(name: &str, value: &str, max_age: i64, http_only: bool) -> HeaderValue {
    let age = if max_age > 0 {
        format!("; Max-Age={max_age}")
    } else {
        String::new()
    };
    let ho = if http_only { "; HttpOnly" } else { "" };
    HeaderValue::from_str(&format!("{name}={value}; Path=/; Secure{ho}; SameSite=Lax{age}"))
        .unwrap_or_else(|_| HeaderValue::from_static(""))
}

async fn login(
    State(id): State<Arc<Identity>>,
    ctx: RequestCtx,
    Json(b): Json<LoginBody>,
) -> PResult<Response> {
    let s = auth::login_password(&id, &ctx, &b.username, &b.password, b.remember).await?;
    let body = json!({
        "session_id": s.session_id,
        "user": { "id": s.user_id, "username": s.username, "class_id": s.class_id, "status": s.status },
        "two_factor_passed": false,
        "csrf_token": s.csrf,
    });
    let mut resp = (StatusCode::OK, Json(body)).into_response();
    let h = resp.headers_mut();
    h.append(
        header::SET_COOKIE,
        cookie("__Host-pt_session", &s.token, s.max_age_secs, true),
    );
    h.append(
        header::SET_COOKIE,
        cookie("__Host-pt_csrf", &s.csrf, s.max_age_secs, false),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(resp)
}

async fn logout(ctx: RequestCtx) -> PResult<Response> {
    auth::logout(&ctx).await?;
    let mut resp = StatusCode::NO_CONTENT.into_response();
    let h = resp.headers_mut();
    h.append(header::SET_COOKIE, cookie("__Host-pt_session", "", -1, true));
    h.append(header::SET_COOKIE, cookie("__Host-pt_csrf", "", -1, false));
    Ok(resp)
}

async fn register(
    State(id): State<Arc<Identity>>,
    ctx: RequestCtx,
    Json(req): Json<RegisterRequest>,
) -> PResult<Response> {
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
    Ok(Json(json!({ "user": u, "roles": roles })).into_response())
}

async fn my_permissions(ctx: RequestCtx) -> PResult<Response> {
    let uid = i64::try_from(ctx.require_user()?).map_err(|_| PluginError::bad_request("bad user id"))?;
    Ok(Json(json!({ "permissions": rbac::permissions_of(&ctx, uid).await? })).into_response())
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

async fn admin_rank_ok(ctx: &RequestCtx, target: i64) -> PResult<i64> {
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
