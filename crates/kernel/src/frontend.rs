//! 前端支持（web HLD §8）：前端清单、术语快照、插件设置接口、SPA 与插件前端静态文件（CSP nonce 注入 import map）。

use std::{collections::BTreeMap, path::PathBuf, sync::Arc};

use axum::{
    Json as AxJson, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use pt_plugin_api::{
    Actor, InvocationKind, Json, PResult, PluginError, Principal,
    services::{RBAC, RbacService},
};
use serde::Deserialize;
use serde_json::json;

use crate::{
    gateway::{credential, csrf_ok, locale},
    runtime::{CallSpec, Shared},
};

/// 内核自身的权限点（catalog/permissions.md §2）
pub const KERNEL_PERMISSIONS: &[(&str, &str)] = &[
    ("kernel.plugins.read", "admin"),
    ("kernel.settings.manage", "admin"),
    ("kernel.audit.read", "admin"),
];

pub fn routes(shared: Arc<Shared>) -> Router {
    Router::new()
        .route("/api/kernel/v1/frontend/manifest", get(manifest))
        .route("/api/kernel/v1/terms", get(terms))
        .route(
            "/api/kernel/v1/admin/plugins/{id}/settings",
            get(get_settings).put(put_settings),
        )
        .route("/api/kernel/v1/admin/audit", get(audit_log))
        .route("/api/kernel/v1/markup/preview", post(markup_preview))
        .with_state(shared)
}

fn web_dir(sh: &Shared) -> Option<PathBuf> {
    sh.cfg.http.web_dir.as_ref().map(PathBuf::from)
}

/// 插件前端入口 URL：`/static/plugins/<id>/<version>/<entry>`（版本进路径，长期缓存）
fn entry_url(id: &str, version: &str, entry: &str) -> String {
    format!(
        "/static/plugins/{id}/{version}/{}",
        entry.trim_start_matches("./")
    )
}

async fn manifest(State(sh): State<Arc<Shared>>) -> Response {
    let plugins: Vec<Json> = sh
        .order
        .iter()
        .filter_map(|id| sh.plugins.get(id))
        .filter_map(|rt| {
            let f = rt.manifest.frontend.as_ref()?;
            let version = rt.manifest.version.to_string();
            Some(json!({
                "id": rt.id, "name": rt.manifest.name, "version": version,
                "entry": f.entry.as_deref().map(|e| entry_url(&rt.id, &version, e)),
                "slots": f.slots, "routes": f.routes, "settings_ui": f.settings_ui.as_deref().unwrap_or("schema"),
            }))
        })
        .collect();
    let mut locales = sh.cfg.site.locales.clone();
    if locales.is_empty() {
        locales.push(sh.cfg.site.default_locale.clone());
    }
    let mut resp = AxJson(json!({
        "site": { "name": sh.cfg.site.name, "default_locale": sh.cfg.site.default_locale, "locales": locales,
                  "themes": ["default", "dark"] },
        "plugins": plugins,
        "installed": sh.order,
        "features": {},
    }))
    .into_response();
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    resp
}

#[derive(Deserialize)]
struct TermsQuery {
    locale: Option<String>,
}

/// 术语快照：清单 `[terms]` 合并（terms 插件接入前的实现）
async fn terms(State(sh): State<Arc<Shared>>, Query(q): Query<TermsQuery>, headers: HeaderMap) -> Response {
    let default = sh.cfg.site.default_locale.clone();
    let loc = q
        .locale
        .or_else(|| locale(&headers))
        .unwrap_or_else(|| default.clone());
    let mut out: BTreeMap<String, Json> = BTreeMap::new();
    for rt in sh.plugins.values() {
        for (k, v) in &rt.manifest.terms {
            let entry = v
                .get(&loc)
                .or_else(|| v.get(&default))
                .or_else(|| v.as_object().and_then(|o| o.values().next()));
            if let Some(e) = entry {
                let term = match e {
                    Json::String(s) => json!({ "name": s }),
                    o => o.clone(),
                };
                out.insert(k.clone(), term);
            }
        }
    }
    let mut resp = AxJson(json!({ "locale": loc, "version": 1, "terms": out })).into_response();
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=300"),
    );
    resp
}

/// 内核接口的会话鉴权与权限检查（与插件路由相同的规则：会话写请求需 CSRF）
#[derive(Deserialize)]
struct PreviewBody {
    body: String,
    #[serde(default)]
    markup: Option<String>,
}

/// 用户内容预览（编辑器“预览”页签）：与插件保存时相同的 pt-markup 渲染；需登录，每用户每分钟 60 次，正文 ≤ 100000 字
async fn markup_preview(
    State(sh): State<Arc<Shared>>,
    method: Method,
    headers: HeaderMap,
    AxJson(b): AxJson<PreviewBody>,
) -> PResult<Response> {
    let (kctx, kuow) = sh.make_ctx("kernel", CallSpec::new(InvocationKind::Http, Actor::anonymous(), 5_000))?;
    let cred = credential(&headers).ok_or_else(PluginError::unauthenticated)?;
    let session = matches!(cred, pt_plugin_api::Credential::Session(_));
    let principal = kctx.authenticate(cred).await?.ok_or_else(PluginError::unauthenticated)?;
    kuow.finish(true).await?;
    if session && !matches!(method, Method::GET | Method::HEAD) && !csrf_ok(&headers, &sh.cfg.site.base_url) {
        return Err(PluginError::new("kernel.csrf_failed", "CSRF check failed"));
    }
    if let Some(redis) = sh.redis.get("main") {
        let rules = crate::limits::parse("user:60/m").map_err(PluginError::internal)?;
        crate::limits::check(redis, "kernel.markup_preview", &rules, None, Some(principal.user_id), None).await?;
    }
    if b.body.chars().count() > 100_000 {
        return Err(PluginError::validation("body is too long"));
    }
    let markup = pt_markup::Markup::parse(b.markup.as_deref().unwrap_or("markdown")).unwrap_or(pt_markup::Markup::Markdown);
    let r = pt_markup::render(&b.body, markup, &pt_markup::RenderOptions::default());
    Ok(AxJson(json!({ "html": r.html })).into_response())
}

async fn authorize(
    sh: &Shared,
    headers: &HeaderMap,
    method: &Method,
    permission: &str,
) -> PResult<Principal> {
    let (kctx, kuow) = sh.make_ctx(
        "kernel",
        CallSpec::new(InvocationKind::Http, Actor::anonymous(), 5_000),
    )?;
    let cred = credential(headers).ok_or_else(PluginError::unauthenticated)?;
    let session = matches!(cred, pt_plugin_api::Credential::Session(_));
    let principal = kctx
        .authenticate(cred)
        .await?
        .ok_or_else(PluginError::unauthenticated)?;
    if session && !matches!(*method, Method::GET | Method::HEAD) && !csrf_ok(headers, &sh.cfg.site.base_url) {
        return Err(PluginError::new("kernel.csrf_failed", "CSRF check failed"));
    }
    let (rbac_id, obj) = sh
        .services
        .get(RBAC)
        .and_then(|v| v.first())
        .ok_or_else(|| PluginError::unavailable("no ctx.rbac provider"))?;
    let rbac = obj
        .downcast_ref::<Arc<dyn RbacService>>()
        .cloned()
        .ok_or_else(|| PluginError::internal("ctx.rbac has wrong type"))?;
    let (rctx, ruow) = sh.kernel_service_ctx(&kctx, rbac_id).await?;
    let ok = rbac
        .check_principal(&rctx, &principal, Some(permission), method.as_str())
        .await;
    ruow.finish(ok.is_ok()).await?;
    kuow.finish(true).await?;
    if !ok? {
        return Err(
            PluginError::forbidden("permission denied").with_details(json!({ "permission": permission }))
        );
    }
    Ok(principal)
}

#[derive(Debug, Deserialize)]
struct AuditQuery {
    plugin: Option<String>,
    /// 前缀匹配（如 `identity.` 或 `identity.restriction_apply`）
    action: Option<String>,
    target: Option<String>,
    /// 操作者用户 ID
    actor: Option<i64>,
    /// 游标：上一页最后一条的 id
    before: Option<i64>,
    limit: Option<i64>,
}

/// 管理操作日志（kernel.audit_log）：按插件、动作前缀、目标、操作者筛选，按 id 倒序游标分页
async fn audit_log(
    State(sh): State<Arc<Shared>>,
    method: Method,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<AuditQuery>,
) -> PResult<Response> {
    authorize(&sh, &headers, &method, "kernel.audit.read").await?;
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    type Row = (i64, chrono::DateTime<chrono::Utc>, Json, String, String, Option<String>, String, Option<String>, Json);
    let rows: Vec<Row> =
        sqlx::query_as(
            "SELECT id, occurred_at, actor, plugin_id, action, target, outcome, request_id, details FROM kernel.audit_log
              WHERE ($1::text IS NULL OR plugin_id = $1)
                AND ($2::text IS NULL OR action LIKE replace(replace($2, '%', '\\%'), '_', '\\_') || '%')
                AND ($3::text IS NULL OR target = $3)
                AND ($4::bigint IS NULL OR (actor->>'user_id')::bigint = $4)
                AND ($5::bigint IS NULL OR id < $5)
              ORDER BY id DESC LIMIT $6",
        )
        .bind(q.plugin.as_deref().filter(|s| !s.is_empty()))
        .bind(q.action.as_deref().filter(|s| !s.is_empty()))
        .bind(q.target.as_deref().filter(|s| !s.is_empty()))
        .bind(q.actor)
        .bind(q.before)
        .bind(limit)
        .fetch_all(&sh.pool)
        .await?;
    let next = (rows.len() == usize::try_from(limit).unwrap_or(50)).then(|| rows.last().map(|r| r.0)).flatten();
    let items: Vec<Json> = rows
        .into_iter()
        .map(|(id, at, actor, plugin, action, target, outcome, request_id, details)| {
            json!({ "id": id, "occurred_at": at.timestamp_millis(), "actor": actor, "plugin_id": plugin, "action": action,
                    "target": target, "outcome": outcome, "request_id": request_id, "details": details })
        })
        .collect();
    Ok(AxJson(json!({ "items": items, "next_before": next })).into_response())
}

async fn get_settings(
    State(sh): State<Arc<Shared>>,
    Path(id): Path<String>,
    method: Method,
    headers: HeaderMap,
) -> PResult<Response> {
    authorize(&sh, &headers, &method, "kernel.settings.manage").await?;
    let rt = sh
        .plugins
        .get(&id)
        .ok_or_else(|| PluginError::not_found("plugin not found"))?;
    let row: Option<(Json, i64)> =
        sqlx::query_as("SELECT settings, version FROM kernel.plugin_settings WHERE plugin_id = $1")
            .bind(&id)
            .fetch_optional(&sh.pool)
            .await?;
    let (settings, version) = row.unwrap_or_else(|| (json!({}), 0));
    Ok(AxJson(json!({
        "plugin_id": id, "settings": settings, "version": version, "schema": rt.manifest.settings_schema,
        "settings_ui": rt.manifest.frontend.as_ref().and_then(|f| f.settings_ui.clone()).unwrap_or_else(|| "schema".into()),
    }))
    .into_response())
}

#[derive(Deserialize)]
struct PutSettings {
    settings: Json,
    version: i64,
}

async fn put_settings(
    State(sh): State<Arc<Shared>>,
    Path(id): Path<String>,
    method: Method,
    headers: HeaderMap,
    AxJson(b): AxJson<PutSettings>,
) -> PResult<Response> {
    let p = authorize(&sh, &headers, &method, "kernel.settings.manage").await?;
    if !sh.plugins.contains_key(&id) {
        return Err(PluginError::not_found("plugin not found"));
    }
    if !b.settings.is_object() || b.settings.to_string().len() > 256 * 1024 {
        return Err(PluginError::validation(
            "settings must be a JSON object (≤ 256 KiB)",
        ));
    }
    let by = i64::try_from(p.user_id).unwrap_or_default();
    let mut tx = sh.pool.begin().await?;
    let cur: Option<(i64, Json)> = sqlx::query_as(
        "SELECT version, settings FROM kernel.plugin_settings WHERE plugin_id = $1 FOR UPDATE",
    )
    .bind(&id)
    .fetch_optional(&mut *tx)
    .await?;
    let (cur_version, before) = cur.clone().unwrap_or((0, json!({})));
    if cur_version != b.version {
        return Err(
            PluginError::new("kernel.conflict", "settings were modified; reload and retry")
                .with_details(json!({ "version": cur_version })),
        );
    }
    let version: i64 = sqlx::query_scalar(
        "INSERT INTO kernel.plugin_settings (plugin_id, settings, version, updated_by) VALUES ($1, $2, 1, $3)
         ON CONFLICT (plugin_id) DO UPDATE SET settings = EXCLUDED.settings, version = kernel.plugin_settings.version + 1,
                updated_by = EXCLUDED.updated_by, updated_at = now()
         RETURNING version",
    )
    .bind(&id)
    .bind(&b.settings)
    .bind(by)
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO kernel.audit_log (actor, plugin_id, action, target, outcome, request_id, details)
         VALUES ($1, 'kernel', 'kernel.settings_update', $2, 'ok', $3, $4)",
    )
    .bind(json!({ "kind": "user", "user_id": p.user_id }))
    .bind(format!("plugin:{id}"))
    .bind(uuid::Uuid::now_v7().to_string())
    .bind(json!({ "before": before, "after": b.settings, "version": version }))
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(AxJson(json!({ "plugin_id": id, "version": version })).into_response())
}

// ---------------- 静态文件 ----------------

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// 只允许普通路径段（拒绝 `..`、空段、反斜杠、隐藏文件）
fn safe_rel(path: &str) -> Option<PathBuf> {
    let mut p = PathBuf::new();
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        if seg == ".." || seg.starts_with('.') || seg.contains('\\') || seg.contains('\0') {
            return None;
        }
        p.push(seg);
    }
    Some(p)
}

/// 插件清单 `[frontend.csp]` 中的外部来源：只接受 `https://` 加主机名（可带路径），其余忽略
fn valid_source(s: &str) -> bool {
    s.strip_prefix("https://").is_some_and(|rest| {
        let host = rest.split('/').next().unwrap_or("");
        !host.is_empty()
            && host.split('.').count() >= 2
            && host
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.' || b == b'*')
            && rest.bytes().all(|b| b.is_ascii_graphic() && !matches!(b, b';' | b',' | b'\'' | b'"'))
    })
}

/// 已加载插件声明的 CSP 来源：(script, frame, connect, style)
#[derive(Debug, Default)]
pub struct CspExtra {
    script: Vec<String>,
    frame: Vec<String>,
    connect: Vec<String>,
    style: Vec<String>,
}

impl CspExtra {
    pub fn from_plugins<'a>(decls: impl Iterator<Item = &'a pt_plugin_api::manifest::CspDecl>) -> Self {
        let mut x = Self::default();
        for d in decls {
            for (dst, src) in [
                (&mut x.script, &d.script_src),
                (&mut x.frame, &d.frame_src),
                (&mut x.connect, &d.connect_src),
                (&mut x.style, &d.style_src),
            ] {
                for s in src.iter().filter(|s| valid_source(s)) {
                    if !dst.contains(s) {
                        dst.push(s.clone());
                    }
                }
            }
        }
        x
    }
}

fn csp(nonce: &str, x: &CspExtra) -> String {
    let more = |v: &[String]| v.iter().map(|s| format!(" {s}")).collect::<String>();
    let frame = if x.frame.is_empty() {
        String::new()
    } else {
        format!(" frame-src{};", more(&x.frame))
    };
    format!(
        "default-src 'self'; script-src 'self' 'nonce-{nonce}'{}; style-src 'self'{}; img-src 'self' data:; connect-src 'self'{};{frame} \
         font-src 'self'; frame-ancestors 'none'; base-uri 'none'; object-src 'none'; form-action 'self'",
        more(&x.script),
        more(&x.style),
        more(&x.connect),
    )
}

fn security_headers(h: &mut HeaderMap) {
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("referrer-policy", HeaderValue::from_static("same-origin"));
}

async fn serve_index(dir: &std::path::Path, extra: &CspExtra) -> Response {
    let Ok(html) = tokio::fs::read_to_string(dir.join("index.html")).await else {
        return PluginError::not_found("web frontend is not built").into_response();
    };
    let nonce = {
        use rand::RngCore;
        let mut b = [0u8; 16];
        rand::rng().fill_bytes(&mut b);
        hex::encode(b)
    };
    let body = html.replace("__CSP_NONCE__", &nonce);
    let mut resp = Response::new(Body::from(body));
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(v) = HeaderValue::from_str(&csp(&nonce, extra)) {
        h.insert(header::CONTENT_SECURITY_POLICY, v);
    }
    security_headers(h);
    resp
}

async fn serve_file(path: PathBuf, immutable: bool) -> Option<Response> {
    let meta = tokio::fs::metadata(&path).await.ok()?;
    if !meta.is_file() {
        return None;
    }
    let bytes = tokio::fs::read(&path).await.ok()?;
    let mut resp = Response::new(Body::from(bytes));
    let h = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(content_type(&path.to_string_lossy())) {
        h.insert(header::CONTENT_TYPE, v);
    }
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(if immutable {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        }),
    );
    security_headers(h);
    Some(resp)
}

/// 非 /api 路径：静态文件或 SPA 首页
pub async fn fallback(sh: Arc<Shared>, method: Method, uri: Uri) -> Response {
    let path = uri.path();
    if path.starts_with("/api/") || !matches!(method, Method::GET | Method::HEAD) {
        return PluginError::not_found("no such route").into_response();
    }
    // 插件公开媒体（如元数据插件转存的海报）：/media/<plugin>/<key> → blob `plugins/<plugin>/public/<key>`
    if let Some(rest) = path.strip_prefix("/media/") {
        let Some((id, key)) = rest.split_once('/') else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let root = sh.cfg.blob.dir.as_deref().map(PathBuf::from);
        return match (root, safe_rel(key), sh.plugins.contains_key(id)) {
            (Some(root), Some(rel), true) => {
                serve_file(root.join("plugins").join(id).join("public").join(rel), true)
                    .await
                    .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
            }
            _ => StatusCode::NOT_FOUND.into_response(),
        };
    }
    let Some(dir) = web_dir(&sh) else {
        return PluginError::not_found("no such route").into_response();
    };
    // 插件前端：/static/plugins/<id>/<version>/<file> → <web_dir>/plugins/<id>/<file>
    if let Some(rest) = path.strip_prefix("/static/plugins/") {
        let mut it = rest.splitn(3, '/');
        let (Some(id), Some(_ver), Some(file)) = (it.next(), it.next(), it.next()) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        if !sh.plugins.contains_key(id) {
            return StatusCode::NOT_FOUND.into_response();
        }
        return match safe_rel(file) {
            Some(rel) => serve_file(dir.join("plugins").join(id).join(rel), true)
                .await
                .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response()),
            None => StatusCode::NOT_FOUND.into_response(),
        };
    }
    if let Some(rest) = path.strip_prefix("/static/") {
        return match safe_rel(rest) {
            Some(rel) => serve_file(dir.join("static").join(rel), true)
                .await
                .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response()),
            None => StatusCode::NOT_FOUND.into_response(),
        };
    }
    if let Some(rel) = safe_rel(path).filter(|r| r.extension().is_some() && r.components().count() == 1)
        && let Some(r) = serve_file(dir.join(rel), false).await
    {
        return r;
    }
    serve_index(&dir, &sh.csp_extra).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        assert!(safe_rel("a/b.js").is_some());
        assert!(safe_rel("../etc/passwd").is_none());
        assert!(safe_rel("a/.env").is_none());
        assert_eq!(
            entry_url("levels", "0.1.0", "./index.js"),
            "/static/plugins/levels/0.1.0/index.js"
        );
        assert!(csp("n", &CspExtra::default()).contains("'nonce-n'"));
        let d = pt_plugin_api::manifest::CspDecl {
            script_src: vec!["https://challenges.cloudflare.com".into(), "http://evil.test".into(), "https://x.test; script-src *".into()],
            frame_src: vec!["https://challenges.cloudflare.com".into()],
            ..Default::default()
        };
        let c = csp("n", &CspExtra::from_plugins([&d].into_iter()));
        assert!(c.contains("script-src 'self' 'nonce-n' https://challenges.cloudflare.com;"), "{c}");
        assert!(c.contains("frame-src https://challenges.cloudflare.com;"), "{c}");
        assert!(!c.contains("evil") && !c.contains("x.test"), "{c}");
    }
}
