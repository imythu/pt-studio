//! 个人 API token 与 MCP token（identity 03 §1.5）：格式 `<前缀><base62(32 字节随机)><base62(CRC32, 6 位)>`，
//! 前缀 `pts_`（api）、`ptm_`（mcp）、`ptf_`（feed，rss 等插件签发、只对签发插件有效、不过期）；只存 SHA-256。Principal 的 credential_id 为 `tok:<id>`，与会话 ID 区分。

use axum::{
    Json,
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::{DateTime, Duration, Utc};
use pt_plugin_api::{Ctx, PResult, PluginError, Principal, RequestCtx, json};
use serde::Deserialize;
use sqlx::Row;

use crate::{crypto, rbac};

const B62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
/// 每个用户每种 token 的上限
pub const MAX_PER_USER: i64 = 20;
const TOUCH_SECS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Api,
    Mcp,
    Feed,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "api" => Some(Self::Api),
            "mcp" => Some(Self::Mcp),
            "feed" => Some(Self::Feed),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Api => "api",
            Self::Mcp => "mcp",
            Self::Feed => "feed",
        }
    }
    fn prefix(self) -> &'static str {
        match self {
            Self::Api => "pts_",
            Self::Mcp => "ptm_",
            Self::Feed => "ptf_",
        }
    }
    /// 允许的作用域
    fn allowed(self, scope: &str) -> bool {
        match self {
            Self::Api => scope == "read" || scope == "write",
            Self::Mcp => matches!(scope, "read" | "write" | "admin"),
            Self::Feed => matches!(scope, "rss" | "download"),
        }
    }
    /// 最长有效期（天）
    fn max_days(self) -> i64 {
        match self {
            Self::Api => 365,
            Self::Mcp => 90,
            Self::Feed => 0,
        }
    }
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for b in data {
        c ^= u32::from(*b);
        for _ in 0..8 {
            c = if c & 1 == 1 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// 任意字节 → base62（大端整数）
fn base62(bytes: &[u8]) -> String {
    let mut num: Vec<u8> = bytes.to_vec();
    let mut out = Vec::new();
    while num.iter().any(|b| *b != 0) {
        let mut rem = 0u32;
        for d in &mut num {
            let cur = (rem << 8) | u32::from(*d);
            *d = u8::try_from(cur / 62).unwrap_or(0);
            rem = cur % 62;
        }
        out.push(B62[rem as usize]);
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

fn checksum(prefix: &str, body: &str) -> String {
    let c = base62(&crc32(format!("{prefix}{body}").as_bytes()).to_be_bytes());
    format!("{c:0>6}")
}

pub fn format_secret(kind: Kind, random: &[u8]) -> String {
    let body = base62(random);
    let sum = checksum(kind.prefix(), &body);
    format!("{}{body}{sum}", kind.prefix())
}

/// 前缀与校验和（不查库即可拒绝打错 / 伪造的 token）
pub fn parse_secret(s: &str) -> Option<Kind> {
    let kind = [Kind::Api, Kind::Mcp, Kind::Feed]
        .into_iter()
        .find(|k| s.starts_with(k.prefix()))?;
    let rest = &s[kind.prefix().len()..];
    if rest.len() < 7 || !rest.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    let (body, sum) = rest.split_at(rest.len() - 6);
    (checksum(kind.prefix(), body) == sum).then_some(kind)
}

/// 签发（返回 token ID 与明文；明文只在此时出现）
pub async fn issue(
    ctx: &Ctx,
    user_id: i64,
    kind: Kind,
    name: &str,
    scopes: &[String],
    ttl_secs: i64,
    issued_by: &str,
) -> PResult<(uuid::Uuid, String, DateTime<Utc>)> {
    let name: String = name.trim().chars().take(64).collect();
    if name.is_empty() {
        return Err(PluginError::validation("name is required"));
    }
    if scopes.is_empty() || scopes.iter().any(|s| !kind.allowed(s)) {
        return Err(PluginError::validation(format!(
            "invalid scopes for a {} token",
            kind.as_str()
        )));
    }
    // feed key 不过期（ttl 忽略），靠重置 / 撤销失效
    if kind != Kind::Feed && !(60..=kind.max_days() * 86_400).contains(&ttl_secs) {
        return Err(PluginError::validation(format!(
            "ttl must be 60 s .. {} days",
            kind.max_days()
        )));
    }
    if kind == Kind::Mcp
        && scopes.iter().any(|s| s == "admin")
        && !rbac::permissions_of(ctx, user_id)
            .await?
            .iter()
            .any(|p| p == "mcp.admin_tools.use")
    {
        return Err(PluginError::forbidden(
            "the admin scope requires mcp.admin_tools.use",
        ));
    }
    let mut conn = ctx.sql.conn().await?;
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM tokens WHERE user_id = $1 AND kind = $2 AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > now())",
    )
    .bind(user_id)
    .bind(kind.as_str())
    .fetch_one(&mut *conn)
    .await?;
    if n >= MAX_PER_USER {
        return Err(PluginError::new(
            "identity.token_limit_exceeded",
            "too many active tokens",
        ));
    }
    let id = ctx.random.uuid_v7();
    let secret = format_secret(kind, &ctx.random.bytes(32));
    let expires = Utc::now() + Duration::seconds(ttl_secs);
    let feed = kind == Kind::Feed;
    sqlx::query(
        "INSERT INTO tokens (id, user_id, kind, name, scopes, token_hash, issued_by, expires_at, audience_plugin) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(id)
    .bind(user_id)
    .bind(kind.as_str())
    .bind(&name)
    .bind(scopes)
    .bind(crypto::sha256(secret.as_bytes()))
    .bind(issued_by)
    .bind((!feed).then_some(expires))
    .bind(feed.then_some(issued_by))
    .execute(&mut *conn)
    .await?;
    drop(conn);
    ctx.audit
        .write(pt_plugin_api::AuditEntry {
            action: "identity.token_issue".into(),
            target: format!("user:{user_id}"),
            details: json!({ "token_id": id, "kind": kind.as_str(), "name": name, "scopes": scopes, "on_behalf_of": user_id }),
        })
        .await?;
    Ok((id, secret, expires))
}

/// Bearer / feed 凭据 → Principal（无效 / 过期 / 撤销 / 用户封禁或待验证 → None）。
/// `audience` 为 Some 时只接受由该插件签发的 feed key；为 None 时只接受 api / mcp token。
pub async fn authenticate(ctx: &Ctx, secret: &str, audience: Option<&str>) -> PResult<Option<Principal>> {
    let Some(kind) = parse_secret(secret) else {
        return Ok(None);
    };
    if (kind == Kind::Feed) != audience.is_some() {
        return Ok(None);
    }
    let mut conn = ctx.sql.conn().await?;
    let row = sqlx::query(
        "SELECT t.id, t.user_id, t.kind, t.scopes, t.expires_at, t.revoked_at, t.last_used_at, u.status, t.audience_plugin, u.class_id
           FROM tokens t JOIN users u ON u.id = t.user_id WHERE t.token_hash = $1",
    )
    .bind(crypto::sha256(secret.as_bytes()))
    .fetch_optional(&mut *conn)
    .await?;
    let Some(r) = row else { return Ok(None) };
    let id: uuid::Uuid = r.get(0);
    let uid: i64 = r.get(1);
    let expires: Option<DateTime<Utc>> = r.get(4);
    let revoked: Option<DateTime<Utc>> = r.get(5);
    let last_used: Option<DateTime<Utc>> = r.get(6);
    let status: String = r.get(7);
    let now = Utc::now();
    if r.get::<String, _>(2) != kind.as_str()
        || revoked.is_some()
        || expires.is_some_and(|e| now > e)
        || r.get::<Option<String>, _>(8).as_deref() != audience
    {
        return Ok(None);
    }
    if matches!(status.as_str(), "banned" | "deleted" | "pending") {
        return Ok(None);
    }
    if last_used.is_none_or(|t| (now - t).num_seconds() > TOUCH_SECS) {
        sqlx::query("UPDATE tokens SET last_used_at = now() WHERE id = $1")
            .bind(id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(Some(Principal {
        user_id: u64::try_from(uid).unwrap_or_default(),
        credential_id: format!("tok:{id}"),
        credential_kind: match kind {
            Kind::Mcp => "mcp".into(),
            Kind::Feed => "feed".into(),
            Kind::Api => "token".into(),
        },
        scopes: r.get(3),
        reauth_at: None,
        status,
        class_id: Some(r.get::<i32, _>(9)),
    }))
}

/// 按明文撤销（OAuth 吊销、刷新时撤销旧访问令牌）；`user_id` 给出时只撤销该用户的
pub async fn revoke_secret(ctx: &Ctx, secret: &str, user_id: Option<i64>) -> PResult<bool> {
    let mut conn = ctx.sql.conn().await?;
    let n = sqlx::query("UPDATE tokens SET revoked_at = now() WHERE token_hash = $1 AND revoked_at IS NULL AND ($2::bigint IS NULL OR user_id = $2)")
        .bind(crypto::sha256(secret.as_bytes()))
        .bind(user_id)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    Ok(n > 0)
}

pub async fn revoke_id(ctx: &Ctx, id: uuid::Uuid) -> PResult<bool> {
    let mut conn = ctx.sql.conn().await?;
    let n = sqlx::query("UPDATE tokens SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL")
        .bind(id)
        .execute(&mut *conn)
        .await?
        .rows_affected();
    Ok(n > 0)
}

// ---------------- HTTP：/v1/users/me/tokens ----------------

/// 只有浏览器会话能管理 token（token 不能签发 token）
fn session_user(ctx: &RequestCtx) -> PResult<i64> {
    let uid = i64::try_from(ctx.require_user()?).map_err(|_| PluginError::bad_request("bad user id"))?;
    let is_session = ctx
        .actor()
        .credential_id
        .as_deref()
        .is_some_and(|c| uuid::Uuid::parse_str(c).is_ok());
    if !is_session {
        return Err(PluginError::forbidden(
            "tokens can only be managed from a browser session",
        ));
    }
    Ok(uid)
}

pub async fn http_list(ctx: RequestCtx) -> PResult<Response> {
    let uid = session_user(&ctx)?;
    let mut conn = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT id, kind, name, scopes, issued_by, expires_at, last_used_at, created_at FROM tokens
          WHERE user_id = $1 AND kind IN ('api', 'mcp') AND revoked_at IS NULL AND (expires_at IS NULL OR expires_at > now())
          ORDER BY created_at DESC",
    )
    .bind(uid)
    .fetch_all(&mut *conn)
    .await?;
    let ms = |t: Option<DateTime<Utc>>| t.map(|t| t.timestamp_millis());
    let items: Vec<_> = rows
        .iter()
        .map(|r| {
            json!({ "id": r.get::<uuid::Uuid, _>(0), "kind": r.get::<String, _>(1), "name": r.get::<String, _>(2),
                    "scopes": r.get::<Vec<String>, _>(3), "issued_by": r.get::<String, _>(4), "expires_at": ms(r.get(5)),
                    "last_used_at": ms(r.get(6)), "created_at": r.get::<DateTime<Utc>, _>(7).timestamp_millis() })
        })
        .collect();
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    kind: String,
    name: String,
    scopes: Vec<String>,
    #[serde(default = "default_days")]
    ttl_days: i64,
}

fn default_days() -> i64 {
    30
}

pub async fn http_create(ctx: RequestCtx, Json(b): Json<CreateBody>) -> PResult<Response> {
    let uid = session_user(&ctx)?;
    let kind = Kind::parse(&b.kind)
        .filter(|k| *k != Kind::Feed)
        .ok_or_else(|| PluginError::validation("kind must be api or mcp"))?;
    if !(1..=kind.max_days()).contains(&b.ttl_days) {
        return Err(PluginError::validation(format!(
            "ttl_days must be 1..={}",
            kind.max_days()
        )));
    }
    let (id, secret, expires) =
        issue(&ctx, uid, kind, &b.name, &b.scopes, b.ttl_days * 86_400, "user").await?;
    let mut resp = (
        StatusCode::CREATED,
        Json(json!({ "id": id, "kind": kind.as_str(), "token": secret, "scopes": b.scopes, "expires_at": expires.timestamp_millis() })),
    )
        .into_response();
    resp.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    Ok(resp)
}

pub async fn http_revoke(ctx: RequestCtx, Path(id): Path<uuid::Uuid>) -> PResult<Response> {
    let uid = session_user(&ctx)?;
    let mut conn = ctx.sql.conn().await?;
    let n = sqlx::query(
        "UPDATE tokens SET revoked_at = now() WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
    )
    .bind(id)
    .bind(uid)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if n == 0 {
        return Err(PluginError::not_found("token not found"));
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_and_parse() {
        let s = format_secret(Kind::Mcp, &[7u8; 32]);
        assert!(s.starts_with("ptm_") && s.len() > 40, "{s}");
        assert_eq!(parse_secret(&s), Some(Kind::Mcp));
        let mut bad = s.clone();
        bad.replace_range(10..11, if &s[10..11] == "A" { "B" } else { "A" });
        assert_eq!(parse_secret(&bad), None);
        assert_eq!(parse_secret("pts_short"), None);
        let f = format_secret(Kind::Feed, &[9u8; 32]);
        assert!(f.starts_with("ptf_"), "{f}");
        assert_eq!(parse_secret(&f), Some(Kind::Feed));
        assert_eq!(
            parse_secret(&format_secret(Kind::Api, &[255u8; 32])),
            Some(Kind::Api)
        );
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
