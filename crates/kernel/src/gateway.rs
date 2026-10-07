//! HTTP 网关（kernel/03 §8）。

use std::{collections::HashMap, sync::Arc};

use axum::{
    Json as AxJson, Router,
    extract::{DefaultBodyLimit, MatchedPath, Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use pt_plugin_api::{
    Actor, Credential, ErrorMarker, InvocationKind, PluginError, Principal, RequestCtx,
    manifest::{AuthLevel, RouteDecl},
    services::{RBAC, RbacService},
};
use serde_json::json;

use crate::runtime::{CallSpec, Shared};

pub const SESSION_COOKIE: &str = "__Host-pt_session";
pub const CSRF_COOKIE: &str = "__Host-pt_csrf";

#[derive(Clone)]
struct PluginState {
    shared: Arc<Shared>,
    plugin: String,
    /// (方法, 路由模板) → 声明
    routes: Arc<HashMap<(String, String), RouteDecl>>,
}

pub fn router(shared: Arc<Shared>) -> Router {
    let mut app = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(readyz))
        .route("/api/kernel/v1/plugins", get(list_plugins))
        .with_state(shared.clone());
    for (id, rt) in &shared.plugins {
        let Some(http) = &rt.http else { continue };
        let routes: HashMap<(String, String), RouteDecl> = rt
            .manifest
            .http
            .as_ref()
            .map(|h| {
                h.routes
                    .iter()
                    .map(|r| ((r.method.to_ascii_uppercase(), r.path.clone()), r.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let state = PluginState {
            shared: shared.clone(),
            plugin: id.clone(),
            routes: Arc::new(routes),
        };
        let max_kb = usize::try_from(shared.cfg.http.max_body_kb).unwrap_or(1024);
        let r = http
            .router()
            .route_layer(middleware::from_fn_with_state(state, plugin_mw))
            .layer(DefaultBodyLimit::max(max_kb * 1024));
        app = app.nest(&format!("/api/p/{id}"), r);
    }
    app.fallback(|| async { PluginError::not_found("no such route") })
}

async fn readyz(State(sh): State<Arc<Shared>>) -> Response {
    match sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(&sh.pool).await {
        Ok(_) => "ready".into_response(),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "database unavailable").into_response(),
    }
}

async fn list_plugins(State(sh): State<Arc<Shared>>) -> Response {
    let items: Vec<_> = sh
        .order
        .iter()
        .filter_map(|id| sh.plugins.get(id))
        .map(|rt| {
            json!({ "id": rt.id, "name": rt.manifest.name, "version": rt.manifest.version.to_string(),
                          "kind": rt.manifest.kind, "provides": rt.manifest.provides })
        })
        .collect();
    AxJson(json!({ "items": items })).into_response()
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|c| {
            let (k, v) = c.trim().split_once('=')?;
            (k == name).then_some(v)
        })
}

fn credential(headers: &HeaderMap) -> Option<Credential> {
    if let Some(auth) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok())
        && let Some(tok) = auth.strip_prefix("Bearer ")
    {
        return Some(Credential::Bearer(tok.trim().to_owned()));
    }
    cookie(headers, SESSION_COOKIE).map(|s| Credential::Session(s.to_owned()))
}

fn locale(headers: &HeaderMap) -> Option<String> {
    let v = headers.get(header::ACCEPT_LANGUAGE)?.to_str().ok()?;
    let first = v.split(',').next()?.split(';').next()?.trim();
    (!first.is_empty() && first.len() <= 16).then(|| first.to_owned())
}

/// 客户端 IP：直连地址在 trusted_proxies 内时，取 X-Forwarded-For 中最右侧的不可信地址。
fn client_ip(req: &Request, headers: &HeaderMap, trusted: &[String]) -> Option<String> {
    let peer = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|c| c.0.ip());
    let is_trusted = |ip: &std::net::IpAddr| trusted.iter().any(|cidr| cidr_contains(cidr, ip));
    let mut ip = peer?;
    if is_trusted(&ip)
        && let Some(xff) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
    {
        for hop in xff.rsplit(',') {
            let Ok(h) = hop.trim().parse::<std::net::IpAddr>() else {
                break;
            };
            ip = h;
            if !is_trusted(&h) {
                break;
            }
        }
    }
    Some(ip.to_string())
}

fn cidr_contains(cidr: &str, ip: &std::net::IpAddr) -> bool {
    let (net, bits) = match cidr.split_once('/') {
        Some((n, b)) => (n, b.parse::<u32>().ok()),
        None => (cidr, None),
    };
    let Ok(net) = net.parse::<std::net::IpAddr>() else {
        return false;
    };
    match (net, ip) {
        (std::net::IpAddr::V4(n), std::net::IpAddr::V4(i)) => {
            let b = bits.unwrap_or(32).min(32);
            let mask = if b == 0 { 0 } else { u32::MAX << (32 - b) };
            (u32::from(n) & mask) == (u32::from(*i) & mask)
        }
        (std::net::IpAddr::V6(n), std::net::IpAddr::V6(i)) => {
            let b = bits.unwrap_or(128).min(128);
            let mask = if b == 0 { 0 } else { u128::MAX << (128 - b) };
            (u128::from(n) & mask) == (u128::from(*i) & mask)
        }
        _ => false,
    }
}

fn origin_of(url: &str) -> &str {
    // scheme://host[:port]
    match url.find("://") {
        Some(i) => match url[i + 3..].find('/') {
            Some(j) => &url[..i + 3 + j],
            None => url,
        },
        None => url,
    }
}

/// CSRF（§8.2 第 5 步）：仅会话凭据的非安全方法。
fn csrf_ok(headers: &HeaderMap, base_url: &str) -> bool {
    let token = headers.get("x-csrf-token").and_then(|v| v.to_str().ok());
    let cookie_token = cookie(headers, CSRF_COOKIE);
    let tokens_ok = matches!((token, cookie_token), (Some(a), Some(b)) if !a.is_empty() && a == b);
    let site_ok = match headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) {
        Some(s) => s == "same-origin" || s == "none",
        None => headers
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .is_none_or(|o| o == origin_of(base_url)),
    };
    tokens_ok && site_ok
}

fn finish_problem(mut resp: Response, request_id: &str) -> Response {
    resp.headers_mut().insert(
        "x-request-id",
        request_id
            .parse()
            .unwrap_or_else(|_| header::HeaderValue::from_static("-")),
    );
    resp
}

async fn plugin_mw(
    State(st): State<PluginState>,
    matched: Option<MatchedPath>,
    mut req: Request,
    next: Next,
) -> Response {
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.len() <= 64)
        .map_or_else(|| uuid::Uuid::now_v7().to_string(), str::to_owned);
    let resp = match prepare(&st, matched, &mut req, &request_id).await {
        Err(e) => e.into_response(),
        Ok(p) => run_prepared(&st, p, req, next).await,
    };
    // 限流超限：Retry-After
    let resp = match resp.extensions().get::<ErrorMarker>() {
        Some(m) if m.0.code == "kernel.rate_limited" => {
            let retry =
                m.0.details
                    .as_ref()
                    .and_then(|d| d["retry_after"].as_i64())
                    .unwrap_or(1);
            let mut r = resp;
            if let Ok(v) = header::HeaderValue::from_str(&retry.to_string()) {
                r.headers_mut().insert(header::RETRY_AFTER, v);
            }
            r
        }
        _ => resp,
    };
    // 插件未显式给出状态时，使用 error-codes 目录登记的状态（conventions/api.md §7）
    let resp = match resp.extensions().get::<ErrorMarker>() {
        Some(m) if m.0.status.is_none() => match crate::catalog::error_status(&m.0.code) {
            Some(st) if st != resp.status().as_u16() => m.0.clone().with_status(st).into_response(),
            _ => resp,
        },
        _ => resp,
    };
    if let Some(m) = resp.extensions().get::<ErrorMarker>()
        && resp.status().is_server_error()
    {
        tracing::error!(plugin = %st.plugin, %request_id, code = %m.0.code, message = %m.0.message, "request failed");
    }
    finish_problem(resp, &request_id)
}

/// 调用插件 handler：幂等键处理（§8.4）、工作单元提交 / 回滚、限流响应头。
async fn run_prepared(st: &PluginState, p: Prepared, req: Request, next: Next) -> Response {
    let Prepared {
        ctx,
        route,
        principal_key,
        verdict,
    } = p;
    let uow = ctx.unit.clone();
    let idem = match (
        route.idempotent,
        req.method().clone(),
        req.headers().get("idempotency-key").and_then(|v| v.to_str().ok()),
    ) {
        (true, Method::POST | Method::PATCH, Some(k)) => {
            if !crate::limits::valid_key(k) {
                return PluginError::bad_request("Idempotency-Key must be 1-64 visible ASCII characters")
                    .into_response();
            }
            principal_key.as_ref().map(|pk| format!("k:idem:{pk}:{k}"))
        }
        _ => None,
    };
    let redis = st.shared.redis.get("main").cloned();
    let (req, idem) = match (idem, redis) {
        (Some(key), Some(redis)) => {
            let limit = usize::try_from(route.max_body_kb.unwrap_or(st.shared.cfg.http.max_body_kb))
                .unwrap_or(1024)
                * 1024;
            let (parts, body) = req.into_parts();
            let bytes = match axum::body::to_bytes(body, limit).await {
                Ok(b) => b,
                Err(_) => {
                    return PluginError::new("kernel.payload_too_large", "request body too large")
                        .into_response();
                }
            };
            let mut h = <sha2::Sha256 as sha2::Digest>::new();
            sha2::Digest::update(&mut h, parts.method.as_str().as_bytes());
            sha2::Digest::update(&mut h, parts.uri.path().as_bytes());
            sha2::Digest::update(&mut h, &bytes);
            let fp = hex::encode(sha2::Digest::finalize(h));
            match crate::limits::begin(&redis, &key, &fp).await {
                Err(e) => return e.into_response(),
                Ok(crate::limits::IdemState::Done {
                    status,
                    content_type,
                    body,
                }) => {
                    let mut r =
                        (StatusCode::from_u16(status).unwrap_or(StatusCode::OK), body).into_response();
                    if let Ok(v) = header::HeaderValue::from_str(&content_type) {
                        r.headers_mut().insert(header::CONTENT_TYPE, v);
                    }
                    r.headers_mut()
                        .insert("idempotent-replayed", header::HeaderValue::from_static("true"));
                    return r;
                }
                Ok(crate::limits::IdemState::Fresh) => {}
            }
            (
                Request::from_parts(parts, axum::body::Body::from(bytes)),
                Some((redis, key)),
            )
        }
        (_, _) => (req, None),
    };
    let mut req = req;
    req.extensions_mut().insert(RequestCtx(Arc::new(ctx)));
    let resp = next.run(req).await;
    let failed = resp.extensions().get::<ErrorMarker>().is_some() || resp.status().is_server_error();
    let finish = match uow.downcast::<crate::storage::uow::Uow>() {
        Ok(u) => u.finish(!failed).await,
        Err(_) => Err(PluginError::internal("missing unit of work")),
    };
    let mut resp = match finish {
        Ok(()) => resp,
        Err(e) => {
            tracing::error!(error = %e, "commit failed");
            e.into_response()
        }
    };
    if let Some((redis, key)) = idem {
        if resp.status().is_server_error() {
            crate::limits::abandon(&redis, &key).await;
        } else {
            let (parts, body) = resp.into_parts();
            let bytes = axum::body::to_bytes(body, 16 << 20).await.unwrap_or_default();
            let ct = parts
                .headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/json")
                .to_owned();
            crate::limits::complete(&redis, &key, parts.status.as_u16(), &ct, &bytes).await;
            resp = Response::from_parts(parts, axum::body::Body::from(bytes));
        }
    }
    if let Some(v) = &verdict {
        rate_headers(&mut resp, v);
    }
    resp
}

async fn prepare(
    st: &PluginState,
    matched: Option<MatchedPath>,
    req: &mut Request,
    request_id: &str,
) -> Result<Prepared, PluginError> {
    let sh = &st.shared;
    let prefix = format!("/api/p/{}", st.plugin);
    let pattern = matched.as_ref().map(MatchedPath::as_str).unwrap_or_default();
    let pattern = pattern.strip_prefix(&prefix).unwrap_or(pattern);
    let method = req.method().as_str().to_owned();
    let route = st
        .routes
        .get(&(method.clone(), pattern.to_owned()))
        .or_else(|| {
            (req.method() == Method::HEAD)
                .then(|| st.routes.get(&("GET".to_owned(), pattern.to_owned())))
                .flatten()
        })
        .ok_or_else(|| PluginError::not_found("route not declared"))?
        .clone();
    let headers = req.headers().clone();
    let timeout_ms = i64::from(route.timeout_ms.unwrap_or(sh.cfg.http.request_timeout_ms));

    // 鉴权（§8.2 第 3、4 步）
    let mut spec = CallSpec::new(InvocationKind::Http, Actor::anonymous(), timeout_ms);
    spec.request_id = request_id.to_owned();
    spec.locale = locale(&headers);
    spec.client_ip = client_ip(req, &headers, &sh.cfg.http.trusted_proxies);
    spec.user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.chars().take(256).collect());
    let (kctx, kuow) = sh.make_ctx("kernel", spec.clone())?;
    let cred = credential(&headers);
    let principal: Option<Principal> = match &cred {
        Some(c) => kctx.authenticate(c.clone()).await?,
        None => None,
    };
    if cred.is_some() && principal.is_none() && route.auth != AuthLevel::Public {
        return Err(PluginError::unauthenticated());
    }
    if route.auth != AuthLevel::Public && principal.is_none() {
        return Err(PluginError::unauthenticated());
    }
    // CSRF（第 5 步）
    let unsafe_method = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if unsafe_method
        && matches!(cred, Some(Credential::Session(_)))
        && principal.is_some()
        && !csrf_ok(&headers, &sh.cfg.site.base_url)
    {
        return Err(PluginError::new("kernel.csrf_failed", "CSRF check failed"));
    }
    // 权限（第 6 步，评审 R1 B-06）
    if let Some(p) = &principal {
        if let Some((rbac_id, obj)) = sh.services.get(RBAC).and_then(|v| v.first()) {
            let rbac = obj
                .downcast_ref::<Arc<dyn RbacService>>()
                .cloned()
                .ok_or_else(|| PluginError::internal("ctx.rbac has wrong type"))?;
            let (rctx, ruow) = sh.kernel_service_ctx(&kctx, rbac_id).await?;
            let r = rbac
                .check_principal(&rctx, p, route.permission.as_deref(), &method)
                .await;
            ruow.finish(r.is_ok()).await?;
            if !r? {
                return Err(PluginError::forbidden("permission denied")
                    .with_details(json!({ "permission": route.permission })));
            }
        } else if route.permission.is_some() {
            return Err(PluginError::unavailable("no ctx.rbac provider"));
        }
        if let Some(secs) = route.reauth_secs {
            let fresh = p
                .reauth_at
                .is_some_and(|t| crate::runtime::now_ms() - t <= i64::from(secs) * 1000);
            if !fresh {
                return Err(PluginError::new(
                    "identity.reauth_required",
                    "please re-authenticate",
                ));
            }
        }
    }
    kuow.finish(true).await?;

    // 限流（第 7 步）：全站默认规则 + 路由规则
    let mut verdict = None;
    if let Some(redis) = sh.redis.get("main") {
        let ip = spec.client_ip.as_deref();
        let user = principal.as_ref().map(|p| p.user_id);
        let token = principal
            .as_ref()
            .filter(|p| p.credential_kind != "session")
            .map(|p| p.credential_id.as_str());
        let site = crate::limits::parse(&sh.cfg.http.rate_limit.default).unwrap_or_default();
        crate::limits::check(redis, "_site", &site, ip, user, token).await?;
        if let Some(spec_rl) = &route.rate_limit {
            let rules = crate::limits::parse(spec_rl).map_err(PluginError::internal)?;
            verdict = crate::limits::check(
                redis,
                &format!("{}.{}", st.plugin, route.id),
                &rules,
                ip,
                user,
                token,
            )
            .await?;
        }
    }
    let principal_key = principal.as_ref().map(|p| p.credential_id.clone());

    // 插件调用上下文（第 9 步）
    spec.actor = match &principal {
        Some(p) => Actor::user(p.user_id, Some(p.credential_id.clone())),
        None => Actor::anonymous(),
    };
    if !sh.has_sensitive(&st.plugin, "user.ip") {
        spec.client_ip = None;
    }
    let (ctx, _) = sh.make_ctx(&st.plugin, spec)?;
    if let Some(p) = &principal
        && let Ok(mut l) = ctx.authenticated.lock()
    {
        l.push(p.user_id);
    }
    Ok(Prepared {
        ctx,
        route,
        principal_key,
        verdict,
    })
}

struct Prepared {
    ctx: pt_plugin_api::Ctx,
    route: RouteDecl,
    /// 幂等键的主体（凭据 ID；匿名请求不支持幂等键）
    principal_key: Option<String>,
    verdict: Option<crate::limits::Verdict>,
}

fn rate_headers(resp: &mut Response, v: &crate::limits::Verdict) {
    let h = resp.headers_mut();
    if let Ok(x) = header::HeaderValue::from_str(&format!("\"{}\"", v.policy)) {
        h.insert("ratelimit-policy", x);
    }
    if let Ok(x) = header::HeaderValue::from_str(&format!(
        "remaining={}, reset={}",
        v.remaining.max(0),
        v.reset_secs
    )) {
        h.insert("ratelimit", x);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csrf() {
        let mut h = HeaderMap::new();
        h.insert(
            header::COOKIE,
            "a=1; __Host-pt_csrf=tok"
                .parse()
                .unwrap_or_else(|_| unreachable!()),
        );
        assert!(!csrf_ok(&h, "https://pt.example"));
        h.insert("x-csrf-token", "tok".parse().unwrap_or_else(|_| unreachable!()));
        assert!(csrf_ok(&h, "https://pt.example"));
        h.insert(
            header::ORIGIN,
            "https://evil.example".parse().unwrap_or_else(|_| unreachable!()),
        );
        assert!(!csrf_ok(&h, "https://pt.example/"));
        h.insert(
            "sec-fetch-site",
            "same-origin".parse().unwrap_or_else(|_| unreachable!()),
        );
        assert!(csrf_ok(&h, "https://pt.example"));
        assert_eq!(origin_of("https://pt.example/x/y"), "https://pt.example");
        let ip = |s: &str| s.parse::<std::net::IpAddr>().unwrap_or_else(|_| unreachable!());
        assert!(cidr_contains("10.0.0.0/8", &ip("10.2.3.4")));
        assert!(!cidr_contains("10.0.0.0/8", &ip("11.2.3.4")));
        assert!(cidr_contains("::1", &ip("::1")));
    }
}
