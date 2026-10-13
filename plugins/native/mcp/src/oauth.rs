//! OAuth 2.1（mcp LLD §1.6）：授权服务器元数据、动态客户端注册（RFC 7591）、授权码 + PKCE（S256）、令牌端点（授权码 / 刷新，
//! 刷新令牌轮换）、吊销（RFC 7009）、站内同意。访问令牌由 identity 签发（MCP token，1 小时）；刷新令牌与授权记录存在本插件。

use std::collections::HashMap;

use axum::{
    Form, Json,
    extract::{Path, Query},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use pt_plugin_api::{Ctx, PResult, PluginError, RequestCtx};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;

use crate::issuer;

pub const ACCESS_TTL_SECS: i64 = 3600;
const REFRESH_TTL_DAYS: i32 = 30;
const CODE_TTL_MINUTES: i32 = 10;
pub const SCOPES: &[&str] = &["read", "write", "admin"];
const DEFAULT_SCOPES: &str = "read write";

fn sha(s: &str) -> Vec<u8> {
    Sha256::digest(s.as_bytes()).to_vec()
}

fn random_token(ctx: &Ctx, prefix: &str) -> String {
    format!("{prefix}{}", URL_SAFE_NO_PAD.encode(ctx.random.bytes(32)))
}

/// RFC 6749 §5.2 错误响应
fn oauth_error(status: StatusCode, error: &str, description: &str) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(json!({ "error": error, "error_description": description })),
    )
        .into_response()
}

fn pct(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn with_query(base: &str, params: &[(&str, &str)]) -> String {
    let q: Vec<String> = params.iter().map(|(k, v)| format!("{k}={}", pct(v))).collect();
    format!(
        "{base}{}{}",
        if base.contains('?') { "&" } else { "?" },
        q.join("&")
    )
}

/// 允许的回调：https；回环地址的 http；原生应用的自定义 scheme（排除危险 scheme）
pub fn redirect_ok(uri: &str, native: bool) -> bool {
    let Some((scheme, rest)) = uri.split_once("://") else {
        return false;
    };
    if uri.contains('#') || uri.len() > 512 {
        return false;
    }
    let host = rest.split(['/', '?']).next().unwrap_or("");
    let host = host.rsplit_once(':').map_or(host, |(h, p)| {
        if p.chars().all(|c| c.is_ascii_digit()) {
            h
        } else {
            host
        }
    });
    match scheme {
        "https" => !host.is_empty(),
        "http" => matches!(host, "127.0.0.1" | "localhost" | "[::1]"),
        "javascript" | "data" | "file" | "vbscript" | "blob" => false,
        s => {
            native
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-'))
        }
    }
}

/// 请求的 scope → 规范列表（空 → 默认；未知 → None）
pub fn parse_scopes(s: Option<&str>) -> Option<Vec<String>> {
    let s = s
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_SCOPES);
    let mut out: Vec<String> = Vec::new();
    for x in s.split_whitespace() {
        // 兼容 mcp 设计稿中的写法
        let x = match x {
            "mcp:user" => "read",
            "mcp:admin" => "admin",
            o => o,
        };
        if !SCOPES.contains(&x) {
            return None;
        }
        if !out.iter().any(|y| y == x) {
            out.push(x.to_owned());
        }
    }
    Some(out)
}

pub fn pkce_ok(verifier: &str, challenge: &str) -> bool {
    (43..=128).contains(&verifier.len())
        && URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) == challenge
}

// ---------------- 元数据 ----------------

pub async fn http_metadata(ctx: RequestCtx, headers: HeaderMap) -> PResult<Response> {
    let iss = issuer(&ctx, &headers);
    Ok(Json(json!({
        "issuer": iss, "authorization_endpoint": format!("{iss}/oauth/authorize"), "token_endpoint": format!("{iss}/oauth/token"),
        "registration_endpoint": format!("{iss}/oauth/register"), "revocation_endpoint": format!("{iss}/oauth/revoke"),
        "response_types_supported": ["code"], "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"], "token_endpoint_auth_methods_supported": ["none", "client_secret_basic", "client_secret_post"],
        "scopes_supported": SCOPES, "authorization_response_iss_parameter_supported": true,
    }))
    .into_response())
}

// ---------------- 动态注册 ----------------

pub async fn http_register(ctx: RequestCtx, Json(b): Json<Value>) -> PResult<Response> {
    let name: String = b["client_name"]
        .as_str()
        .unwrap_or("")
        .trim()
        .chars()
        .take(100)
        .collect();
    let app_type = b["application_type"].as_str().unwrap_or("web");
    if !matches!(app_type, "web" | "native") {
        return Ok(oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_client_metadata",
            "application_type must be web or native",
        ));
    }
    let uris: Vec<String> = b["redirect_uris"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    if name.is_empty() || uris.is_empty() || uris.len() > 10 {
        return Ok(oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_client_metadata",
            "client_name and 1..=10 redirect_uris are required",
        ));
    }
    if let Some(bad) = uris.iter().find(|u| !redirect_ok(u, app_type == "native")) {
        return Ok(oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_redirect_uri",
            &format!("redirect uri not allowed: {bad}"),
        ));
    }
    let method = b["token_endpoint_auth_method"]
        .as_str()
        .unwrap_or("client_secret_basic");
    if !matches!(method, "none" | "client_secret_basic" | "client_secret_post") {
        return Ok(oauth_error(
            StatusCode::BAD_REQUEST,
            "invalid_client_metadata",
            "unsupported token_endpoint_auth_method",
        ));
    }
    let client_id = format!("mcpc_{}", URL_SAFE_NO_PAD.encode(ctx.random.bytes(16)));
    let secret = (method != "none").then(|| random_token(&ctx, "mcps_"));
    let mut c = ctx.sql.conn().await?;
    sqlx::query(
        "INSERT INTO oauth_clients (client_id, client_secret_hash, client_name, redirect_uris, application_type, auth_method) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&client_id)
    .bind(secret.as_deref().map(sha))
    .bind(&name)
    .bind(&uris)
    .bind(app_type)
    .bind(method)
    .execute(&mut *c)
    .await?;
    let mut out = json!({ "client_id": client_id, "client_name": name, "redirect_uris": uris, "application_type": app_type,
                          "token_endpoint_auth_method": method, "grant_types": ["authorization_code", "refresh_token"], "response_types": ["code"],
                          "client_id_issued_at": chrono::Utc::now().timestamp() });
    if let Some(s) = secret {
        out["client_secret"] = json!(s);
        out["client_secret_expires_at"] = json!(0);
    }
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "no-store")],
        Json(out),
    )
        .into_response())
}

// ---------------- 授权 ----------------

struct Client {
    name: String,
    redirect_uris: Vec<String>,
    secret_hash: Option<Vec<u8>>,
    auth_method: String,
}

async fn client(ctx: &Ctx, id: &str) -> PResult<Option<Client>> {
    let mut c = ctx.sql.conn().await?;
    let r = sqlx::query("SELECT client_name, redirect_uris, client_secret_hash, auth_method FROM oauth_clients WHERE client_id = $1")
        .bind(id)
        .fetch_optional(&mut *c)
        .await?;
    Ok(r.map(|r| Client {
        name: r.get(0),
        redirect_uris: r.get(1),
        secret_hash: r.get(2),
        auth_method: r.get(3),
    }))
}

/// 授权请求的校验结果：Ok(客户端名, scopes) 或 Err(是否可以重定向回客户端, error, 说明)
async fn check_authorize(
    ctx: &Ctx,
    q: &HashMap<String, String>,
    iss: &str,
) -> PResult<Result<(String, Vec<String>), (bool, &'static str, String)>> {
    let get = |k: &str| q.get(k).map(String::as_str);
    let Some(cl) = client(ctx, get("client_id").unwrap_or("")).await? else {
        return Ok(Err((false, "invalid_client", "unknown client_id".into())));
    };
    let redirect = get("redirect_uri").unwrap_or("");
    if !cl.redirect_uris.iter().any(|u| u == redirect) {
        return Ok(Err((
            false,
            "invalid_request",
            "redirect_uri does not match a registered uri".into(),
        )));
    }
    if get("response_type") != Some("code") {
        return Ok(Err((
            true,
            "unsupported_response_type",
            "response_type must be code".into(),
        )));
    }
    if get("code_challenge").is_none_or(|c| c.len() < 43) || get("code_challenge_method") != Some("S256") {
        return Ok(Err((
            true,
            "invalid_request",
            "PKCE with S256 is required".into(),
        )));
    }
    if let Some(res) = get("resource")
        && res.trim_end_matches('/') != format!("{iss}/mcp")
    {
        return Ok(Err((
            true,
            "invalid_target",
            "resource must be the MCP endpoint".into(),
        )));
    }
    let Some(scopes) = parse_scopes(get("scope")) else {
        return Ok(Err((true, "invalid_scope", "unknown scope".into())));
    };
    Ok(Ok((cl.name, scopes)))
}

/// GET /oauth/authorize：校验后转到站内同意页（不能信任的参数错误不重定向回客户端）
pub async fn http_authorize(
    ctx: RequestCtx,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> PResult<Response> {
    let iss = issuer(&ctx, &headers);
    match check_authorize(&ctx, &q, &iss).await? {
        Ok(_) => {
            let pairs: Vec<(&str, &str)> = q.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            Ok(Redirect::to(&with_query(&format!("{iss}/p/mcp/consent"), &pairs)).into_response())
        }
        Err((false, e, d)) => Ok(oauth_error(StatusCode::BAD_REQUEST, e, &d)),
        Err((true, e, d)) => {
            let mut p = vec![
                ("error", e),
                ("error_description", d.as_str()),
                ("iss", iss.as_str()),
            ];
            if let Some(s) = q.get("state") {
                p.push(("state", s));
            }
            Ok(
                Redirect::to(&with_query(q.get("redirect_uri").map_or("", String::as_str), &p))
                    .into_response(),
            )
        }
    }
}

/// GET /v1/oauth/authorize-info：同意页展示用（客户端名由客户端自报，未经验证）
pub async fn http_authorize_info(
    ctx: RequestCtx,
    headers: HeaderMap,
    Query(q): Query<HashMap<String, String>>,
) -> PResult<Response> {
    ctx.require_user()?;
    match check_authorize(&ctx, &q, &issuer(&ctx, &headers)).await? {
        Ok((name, scopes)) => Ok(Json(
            json!({ "client_name": name, "scopes": scopes, "redirect_uri": q.get("redirect_uri") }),
        )
        .into_response()),
        Err((_, e, d)) => {
            Err(PluginError::new("mcp.oauth_invalid_request", d).with_details(json!({ "error": e })))
        }
    }
}

/// POST /v1/oauth/consent：同意 → 授权码；拒绝 → access_denied（均返回要跳转的地址）
pub async fn http_consent(ctx: RequestCtx, headers: HeaderMap, Json(b): Json<Value>) -> PResult<Response> {
    let uid = i64::try_from(ctx.require_user()?).unwrap_or(0);
    if uuid::Uuid::parse_str(ctx.actor().credential_id.as_deref().unwrap_or("")).is_err() {
        return Err(PluginError::forbidden("consent requires a browser session"));
    }
    let iss = issuer(&ctx, &headers);
    let q: HashMap<String, String> = b["params"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let (_, scopes) = match check_authorize(&ctx, &q, &iss).await? {
        Ok(v) => v,
        Err((_, e, d)) => {
            return Err(PluginError::new("mcp.oauth_invalid_request", d).with_details(json!({ "error": e })));
        }
    };
    let redirect = q.get("redirect_uri").cloned().unwrap_or_default();
    let state = q.get("state").cloned();
    let mut params: Vec<(&str, String)> = Vec::new();
    if b["approve"] == true {
        let code = random_token(&ctx, "mcpa_");
        let mut c = ctx.sql.conn().await?;
        sqlx::query(
            "INSERT INTO auth_codes (code_hash, client_id, user_id, scopes, redirect_uri, code_challenge, expires_at)
             VALUES ($1, $2, $3, $4, $5, $6, now() + make_interval(mins => $7))",
        )
        .bind(sha(&code))
        .bind(q.get("client_id"))
        .bind(uid)
        .bind(&scopes)
        .bind(&redirect)
        .bind(q.get("code_challenge"))
        .bind(CODE_TTL_MINUTES)
        .execute(&mut *c)
        .await?;
        params.push(("code", code));
    } else {
        params.push(("error", "access_denied".into()));
    }
    if let Some(s) = state {
        params.push(("state", s));
    }
    params.push(("iss", iss));
    let pairs: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    Ok(Json(json!({ "redirect": with_query(&redirect, &pairs) })).into_response())
}

// ---------------- 令牌 ----------------

/// 客户端认证：client_secret_basic / client_secret_post / none
fn client_credentials(headers: &HeaderMap, f: &HashMap<String, String>) -> (Option<String>, Option<String>) {
    if let Some(basic) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
        && let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(basic.trim())
        && let Ok(s) = String::from_utf8(raw)
        && let Some((id, secret)) = s.split_once(':')
    {
        return (Some(id.to_owned()), Some(secret.to_owned()));
    }
    (f.get("client_id").cloned(), f.get("client_secret").cloned())
}

async fn issue_pair(
    ctx: &Ctx,
    user_id: i64,
    client_id: &str,
    client_name: &str,
    scopes: &[String],
) -> PResult<Value> {
    let r = ctx
        .call_json(
            "ctx.auth",
            "issue_token",
            json!({ "user_id": user_id, "kind": "mcp", "scopes": scopes, "ttl_secs": ACCESS_TTL_SECS, "name": client_name.chars().take(64).collect::<String>() }),
        )
        .await?;
    let refresh = random_token(ctx, "mcpr_");
    let mut c = ctx.sql.conn().await?;
    // 同一用户与客户端只保留一份授权；旧的访问令牌随之撤销
    let old: Option<String> =
        sqlx::query_scalar("SELECT access_token_id FROM grants WHERE user_id = $1 AND client_id = $2")
            .bind(user_id)
            .bind(client_id)
            .fetch_optional(&mut *c)
            .await?
            .flatten();
    sqlx::query(
        "INSERT INTO grants (user_id, client_id, scopes, refresh_hash, refresh_expires_at, access_token_id, last_used_at)
         VALUES ($1, $2, $3, $4, now() + make_interval(days => $5), $6, now())
         ON CONFLICT (user_id, client_id) DO UPDATE SET scopes = EXCLUDED.scopes, refresh_hash = EXCLUDED.refresh_hash,
             refresh_expires_at = EXCLUDED.refresh_expires_at, access_token_id = EXCLUDED.access_token_id, last_used_at = now()",
    )
    .bind(user_id)
    .bind(client_id)
    .bind(scopes)
    .bind(sha(&refresh))
    .bind(REFRESH_TTL_DAYS)
    .bind(r["token_id"].as_str())
    .execute(&mut *c)
    .await?;
    sqlx::query("UPDATE oauth_clients SET last_used_at = now() WHERE client_id = $1")
        .bind(client_id)
        .execute(&mut *c)
        .await?;
    drop(c);
    if let Some(o) = old {
        ctx.call_json("ctx.auth", "revoke_token", json!({ "token_id": o }))
            .await?;
    }
    Ok(
        json!({ "access_token": r["token"], "token_type": "Bearer", "expires_in": ACCESS_TTL_SECS, "refresh_token": refresh, "scope": scopes.join(" ") }),
    )
}

pub async fn http_token(
    ctx: RequestCtx,
    headers: HeaderMap,
    Form(f): Form<HashMap<String, String>>,
) -> PResult<Response> {
    let (cid, secret) = client_credentials(&headers, &f);
    let Some(cid) = cid else {
        return Ok(oauth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "client authentication failed",
        ));
    };
    let Some(cl) = client(&ctx, &cid).await? else {
        return Ok(oauth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "client authentication failed",
        ));
    };
    if cl.auth_method != "none"
        && (secret.is_none() || cl.secret_hash.as_deref() != secret.as_deref().map(sha).as_deref())
    {
        return Ok(oauth_error(
            StatusCode::UNAUTHORIZED,
            "invalid_client",
            "client authentication failed",
        ));
    }
    let mut c = ctx.sql.conn().await?;
    let body = match f.get("grant_type").map(String::as_str) {
        Some("authorization_code") => {
            let code = f.get("code").map_or("", String::as_str);
            let row = sqlx::query(
                "UPDATE auth_codes SET used = true WHERE code_hash = $1 AND client_id = $2 AND NOT used AND expires_at > now()
                 RETURNING user_id, scopes, redirect_uri, code_challenge",
            )
            .bind(sha(code))
            .bind(&cid)
            .fetch_optional(&mut *c)
            .await?;
            drop(c);
            let Some(r) = row else {
                return Ok(oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "authorization code is invalid, used or expired",
                ));
            };
            if f.get("redirect_uri").map(String::as_str) != Some(r.get::<String, _>(2).as_str()) {
                return Ok(oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "redirect_uri mismatch",
                ));
            }
            if !pkce_ok(
                f.get("code_verifier").map_or("", String::as_str),
                &r.get::<String, _>(3),
            ) {
                return Ok(oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "PKCE verification failed",
                ));
            }
            let scopes: Vec<String> = r.get(1);
            match issue_pair(&ctx, r.get(0), &cid, &cl.name, &scopes).await {
                Ok(b) => b,
                // 用户被封禁、作用域不再允许等
                Err(e) => return Ok(oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", &e.message)),
            }
        }
        Some("refresh_token") => {
            let rt = f.get("refresh_token").map_or("", String::as_str);
            let row = sqlx::query("SELECT user_id, scopes FROM grants WHERE refresh_hash = $1 AND client_id = $2 AND refresh_expires_at > now()")
                .bind(sha(rt))
                .bind(&cid)
                .fetch_optional(&mut *c)
                .await?;
            drop(c);
            let Some(r) = row else {
                return Ok(oauth_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    "refresh token is invalid or expired",
                ));
            };
            let mut scopes: Vec<String> = r.get(1);
            // 可以缩小作用域，不能扩大
            if let Some(req) = f.get("scope") {
                match parse_scopes(Some(req)) {
                    Some(s) if s.iter().all(|x| scopes.contains(x)) => scopes = s,
                    _ => {
                        return Ok(oauth_error(
                            StatusCode::BAD_REQUEST,
                            "invalid_scope",
                            "scope exceeds the original grant",
                        ));
                    }
                }
            }
            match issue_pair(&ctx, r.get(0), &cid, &cl.name, &scopes).await {
                Ok(b) => b,
                Err(e) => return Ok(oauth_error(StatusCode::BAD_REQUEST, "invalid_grant", &e.message)),
            }
        }
        _ => {
            return Ok(oauth_error(
                StatusCode::BAD_REQUEST,
                "unsupported_grant_type",
                "grant_type must be authorization_code or refresh_token",
            ));
        }
    };
    let mut resp = Json(body).into_response();
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(resp)
}

/// RFC 7009：未知令牌也返回 200
pub async fn http_revoke(
    ctx: RequestCtx,
    headers: HeaderMap,
    Form(f): Form<HashMap<String, String>>,
) -> PResult<Response> {
    let (cid, _) = client_credentials(&headers, &f);
    let token = f.get("token").map_or("", String::as_str);
    if token.starts_with("mcpr_") {
        let mut c = ctx.sql.conn().await?;
        let old: Option<Option<String>> = sqlx::query_scalar("DELETE FROM grants WHERE refresh_hash = $1 AND ($2::text IS NULL OR client_id = $2) RETURNING access_token_id")
            .bind(sha(token))
            .bind(cid)
            .fetch_optional(&mut *c)
            .await?;
        drop(c);
        if let Some(Some(id)) = old {
            ctx.call_json("ctx.auth", "revoke_token", json!({ "token_id": id }))
                .await?;
        }
    } else if token.starts_with("ptm_") {
        ctx.call_json("ctx.auth", "revoke_token", json!({ "token": token }))
            .await?;
    }
    Ok(StatusCode::OK.into_response())
}

// ---------------- 已授权的客户端（设置页） ----------------

pub async fn http_list_grants(ctx: RequestCtx) -> PResult<Response> {
    let uid = i64::try_from(ctx.require_user()?).unwrap_or(0);
    let mut c = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT g.client_id, c.client_name, g.scopes, g.created_at, g.last_used_at FROM grants g JOIN oauth_clients c USING (client_id)
          WHERE g.user_id = $1 ORDER BY g.created_at DESC",
    )
    .bind(uid)
    .fetch_all(&mut *c)
    .await?;
    let ms = |t: Option<chrono::DateTime<chrono::Utc>>| t.map(|t| t.timestamp_millis());
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({ "client_id": r.get::<String, _>(0), "client_name": r.get::<String, _>(1), "scopes": r.get::<Vec<String>, _>(2),
                    "created_at": ms(r.get(3)), "last_used_at": ms(r.get(4)) })
        })
        .collect();
    Ok(Json(json!({ "items": items })).into_response())
}

pub async fn http_revoke_grant(ctx: RequestCtx, Path(client_id): Path<String>) -> PResult<Response> {
    let uid = i64::try_from(ctx.require_user()?).unwrap_or(0);
    let mut c = ctx.sql.conn().await?;
    let old: Option<Option<String>> = sqlx::query_scalar(
        "DELETE FROM grants WHERE user_id = $1 AND client_id = $2 RETURNING access_token_id",
    )
    .bind(uid)
    .bind(&client_id)
    .fetch_optional(&mut *c)
    .await?;
    drop(c);
    match old {
        None => Err(PluginError::not_found("grant not found")),
        Some(id) => {
            if let Some(id) = id {
                ctx.call_json("ctx.auth", "revoke_token", json!({ "token_id": id }))
                    .await?;
            }
            Ok(StatusCode::NO_CONTENT.into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirects_scopes_pkce() {
        assert!(redirect_ok("https://claude.ai/api/mcp/auth_callback", false));
        assert!(redirect_ok("http://127.0.0.1:33418/callback", false));
        assert!(redirect_ok("http://localhost/cb", false));
        assert!(!redirect_ok("http://evil.example/cb", false));
        assert!(!redirect_ok("javascript://x", true));
        assert!(redirect_ok("cursor://anysphere.cursor-retrieval/oauth", true));
        assert!(!redirect_ok("cursor://x/oauth", false));
        assert!(!redirect_ok("https://a.example/cb#frag", false));
        assert_eq!(parse_scopes(None), Some(vec!["read".into(), "write".into()]));
        assert_eq!(
            parse_scopes(Some("mcp:user admin read")),
            Some(vec!["read".into(), "admin".into()])
        );
        assert_eq!(parse_scopes(Some("openid")), None);
        // RFC 7636 附录 B 的测试向量
        assert!(pkce_ok(
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        ));
        assert!(!pkce_ok(
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk",
            "x".repeat(43).as_str()
        ));
        assert_eq!(
            with_query("https://a/cb", &[("code", "a b"), ("state", "x&y")]),
            "https://a/cb?code=a%20b&state=x%26y"
        );
    }
}
