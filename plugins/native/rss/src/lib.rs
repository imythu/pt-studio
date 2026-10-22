//! rss 插件（Native，docs/plugins/rss/）：个人 feed key、保存的订阅、RSS 2.0 与 Torznab 接口、凭 key 下载种子。
//!
//! 本版本实现：feed key（经 `ctx.auth.issue_token` 签发 kind = feed，只对本插件有效、不过期；重置即吊销旧 key；最近使用时间
//! 与 UA 记在 Redis）、订阅增删改查（类型、关键词、字段条件、促销、体积、排除词；每人上限）、订阅 XML（RSS 2.0 + torznab 属性，
//! 60 秒缓存）、Torznab（caps / search / tvsearch / movie / music / book，分类映射、imdb / tmdb / tvdb / 季 / 集参数、Newznab
//! 错误 XML）、下载（以 key 认证出的用户身份调用 `ctx.torrents.build_download`，权限、账号状态与 `torrent.before_download`
//! 钩子与网页下载一致）、每个 key 每分钟请求上限。订阅条件经 ctx.search（相关度、可筛选字段校验）与 hnr 属性在后续迭代实现。

pub mod query;
pub mod xml;

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use base64::Engine as _;
use pt_plugin_api::{
    Credential, Ctx, HttpRoutes, NativePlugin, PResult, PluginError, Principal, RedisCommand, RequestCtx,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;

use query::Definition;

pub const MANIFEST: &str = include_str!("../plugin.toml");
pub const MIGRATIONS: &[(&str, &str)] = &[("0001_init.sql", include_str!("../migrations/0001_init.sql"))];

const CACHE_SECS: &str = "60";

fn e(code: &str, msg: &str, status: u16) -> PluginError {
    PluginError::new(format!("rss.{code}"), msg).with_status(status)
}

struct Site {
    name: String,
    url: String,
}

async fn site(ctx: &Ctx, headers: &HeaderMap) -> Site {
    let s = ctx.config.get_settings().await;
    let url = match s["site_url"].as_str().map(str::trim).filter(|u| !u.is_empty()) {
        Some(u) => u.trim_end_matches('/').to_owned(),
        None => {
            let host = headers
                .get(header::HOST)
                .and_then(|h| h.to_str().ok())
                .unwrap_or("localhost");
            let proto = headers
                .get("x-forwarded-proto")
                .and_then(|h| h.to_str().ok())
                .filter(|p| *p == "http" || *p == "https")
                .unwrap_or("https");
            format!("{proto}://{host}")
        }
    };
    Site {
        name: s["site_name"]
            .as_str()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_owned)
            .or_else(|| ctx.config.get_static()["_site"]["name"].as_str().map(str::to_owned))
            .unwrap_or_else(|| "PT Studio".into()),
        url,
    }
}

async fn setting(ctx: &Ctx, key: &str, default: i64) -> i64 {
    ctx.config.get_settings().await[key].as_i64().unwrap_or(default)
}

fn xml_resp(status: StatusCode, body: String) -> Response {
    (
        status,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/rss+xml; charset=utf-8"),
            ),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, max-age=60"),
            ),
        ],
        body,
    )
        .into_response()
}

fn text_resp(status: StatusCode, msg: &str) -> Response {
    (
        status,
        [(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/plain; charset=utf-8"),
        )],
        msg.to_owned(),
    )
        .into_response()
}

// ---------------- keys（LLD §1.1） ----------------

/// feed key → 主体（失败统一为 rss.key_invalid）；同时记录最近使用并做每 key 限流
async fn key_auth(ctx: &Ctx, k: Option<&str>, scope: &str, headers: &HeaderMap) -> PResult<Principal> {
    let invalid = || e("key_invalid", "feed key is invalid", 401);
    let k = k
        .map(str::trim)
        .filter(|k| k.starts_with("ptf_"))
        .ok_or_else(invalid)?;
    let p = ctx
        .authenticate(Credential::Feed(k.to_owned()))
        .await?
        .filter(|p| p.credential_kind == "feed" && p.scopes.iter().any(|s| s == scope))
        .ok_or_else(invalid)?;
    let tok = p.credential_id.trim_start_matches("tok:").to_owned();
    let minute = chrono::Utc::now().timestamp() / 60;
    let rl = format!("rl:{tok}:{minute}");
    let n = ctx
        .redis
        .call(RedisCommand::new("INCR").arg(&rl))
        .await?
        .as_int()
        .unwrap_or(0);
    if n == 1 {
        ctx.redis
            .call(RedisCommand::new("EXPIRE").arg(&rl).arg("120"))
            .await?;
    }
    if n > setting(ctx, "requests_per_minute", 60).await {
        return Err(e("rate_limited", "too many requests for this key", 429));
    }
    let ua: String = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .chars()
        .take(100)
        .collect();
    let use_key = format!("use:{tok}");
    ctx.redis
        .pipeline(
            vec![
                RedisCommand::new("HSET")
                    .arg(&use_key)
                    .arg("at")
                    .arg(chrono::Utc::now().timestamp_millis().to_string())
                    .arg("ua")
                    .arg(&ua),
                RedisCommand::new("EXPIRE").arg(&use_key).arg("2592000"),
            ],
            false,
        )
        .await?;
    Ok(p)
}

async fn current_key(ctx: &Ctx, uid: i64) -> PResult<Option<(String, String, i64)>> {
    let mut c = ctx.sql.conn().await?;
    let r = sqlx::query("SELECT token_id::text, prefix, (extract(epoch FROM created_at) * 1000)::bigint FROM keys WHERE user_id = $1")
        .bind(uid)
        .fetch_optional(&mut *c)
        .await?;
    Ok(r.map(|r| (r.get(0), r.get(1), r.get(2))))
}

fn uid_of(ctx: &Ctx) -> PResult<i64> {
    i64::try_from(ctx.require_user()?).map_err(|_| PluginError::bad_request("bad user"))
}

async fn http_key_get(ctx: RequestCtx) -> PResult<Response> {
    let uid = uid_of(&ctx)?;
    let Some((tok, prefix, created)) = current_key(&ctx, uid).await? else {
        return Ok(Json(json!({ "exists": false })).into_response());
    };
    let r = ctx
        .redis
        .call(
            RedisCommand::new("HMGET")
                .arg(format!("use:{tok}"))
                .arg("at")
                .arg("ua"),
        )
        .await?;
    let (at, ua) = match r {
        pt_plugin_api::RedisReply::Array(a) => (
            a.first().and_then(pt_plugin_api::RedisReply::as_int),
            a.get(1)
                .and_then(pt_plugin_api::RedisReply::as_bytes)
                .map(|b| String::from_utf8_lossy(b).into_owned()),
        ),
        _ => (None, None),
    };
    Ok(Json(json!({ "exists": true, "prefix": prefix, "created_at": created, "last_used_at": at, "last_user_agent": ua })).into_response())
}

async fn revoke_current(ctx: &Ctx, uid: i64) -> PResult<bool> {
    let Some((tok, _, _)) = current_key(ctx, uid).await? else {
        return Ok(false);
    };
    ctx.call_json("ctx.auth", "revoke_token", json!({ "token_id": tok }))
        .await?;
    let mut c = ctx.sql.conn().await?;
    sqlx::query("DELETE FROM keys WHERE user_id = $1")
        .bind(uid)
        .execute(&mut *c)
        .await?;
    Ok(true)
}

/// 生成 / 重置：吊销旧 key，签发新 key；明文只在此时返回
async fn http_key_reset(ctx: RequestCtx, headers: HeaderMap) -> PResult<Response> {
    let uid = uid_of(&ctx)?;
    revoke_current(&ctx, uid).await?;
    let r = ctx
        .call_json(
            "ctx.auth",
            "issue_token",
            json!({ "user_id": uid, "kind": "feed", "scopes": ["rss", "download"], "name": "RSS" }),
        )
        .await?;
    let secret = r["token"].as_str().unwrap_or_default().to_owned();
    let tok = r["token_id"].as_str().unwrap_or_default().to_owned();
    let prefix: String = secret.chars().take(8).collect();
    let mut c = ctx.sql.conn().await?;
    sqlx::query("INSERT INTO keys (user_id, token_id, prefix) VALUES ($1, $2::uuid, $3)")
        .bind(uid)
        .bind(&tok)
        .bind(&prefix)
        .execute(&mut *c)
        .await?;
    drop(c);
    let s = site(&ctx, &headers).await;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "key": secret, "prefix": prefix,
                     "torznab_url": format!("{}/api/p/rss/v1/torznab", s.url),
                     "feed_url_template": format!("{}/api/p/rss/v1/feeds/{{id}}.xml?k={secret}", s.url) })),
    )
        .into_response())
}

async fn http_key_revoke(ctx: RequestCtx) -> PResult<Response> {
    let uid = uid_of(&ctx)?;
    if !revoke_current(&ctx, uid).await? {
        return Err(PluginError::not_found("no feed key"));
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------------- 订阅管理（LLD §2） ----------------

#[derive(serde::Deserialize)]
struct FeedBody {
    name: String,
    #[serde(default)]
    definition: Value,
}

fn parse_body(b: FeedBody) -> PResult<(String, Definition)> {
    let name: String = b.name.trim().chars().take(60).collect();
    if name.is_empty() {
        return Err(PluginError::validation("name is required"));
    }
    let def = if b.definition.is_null() {
        Definition::default()
    } else {
        serde_json::from_value::<Definition>(b.definition)
            .map_err(|err| e("filter_invalid", &err.to_string(), 422))?
    };
    Ok((name, def.normalize()?))
}

fn feed_json(r: &sqlx::postgres::PgRow, url: &str) -> Value {
    let id: i64 = r.get(0);
    json!({ "id": id, "name": r.get::<String, _>(1), "definition": r.get::<Value, _>(2),
            "updated_at": r.get::<chrono::DateTime<chrono::Utc>, _>(3).timestamp_millis(),
            "url_template": format!("{url}/api/p/rss/v1/feeds/{id}.xml?k={{key}}") })
}

async fn http_feeds_list(ctx: RequestCtx, headers: HeaderMap) -> PResult<Response> {
    let uid = uid_of(&ctx)?;
    let s = site(&ctx, &headers).await;
    let mut c = ctx.sql.conn().await?;
    let rows =
        sqlx::query("SELECT id, name, definition, updated_at FROM feeds WHERE user_id = $1 ORDER BY id")
            .bind(uid)
            .fetch_all(&mut *c)
            .await?;
    let items: Vec<Value> = rows.iter().map(|r| feed_json(r, &s.url)).collect();
    Ok(
        Json(json!({ "items": items, "torznab_url": format!("{}/api/p/rss/v1/torznab", s.url) }))
            .into_response(),
    )
}

async fn http_feeds_create(
    ctx: RequestCtx,
    headers: HeaderMap,
    Json(b): Json<FeedBody>,
) -> PResult<Response> {
    let uid = uid_of(&ctx)?;
    let (name, def) = parse_body(b)?;
    let max = setting(&ctx, "max_feeds_per_user", 20).await;
    let mut c = ctx.sql.conn().await?;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM feeds WHERE user_id = $1")
        .bind(uid)
        .fetch_one(&mut *c)
        .await?;
    if n >= max {
        return Err(e("too_many_feeds", "too many feeds", 422));
    }
    let r = sqlx::query("INSERT INTO feeds (user_id, name, definition) VALUES ($1, $2, $3) RETURNING id, name, definition, updated_at")
        .bind(uid)
        .bind(&name)
        .bind(serde_json::to_value(&def)?)
        .fetch_one(&mut *c)
        .await?;
    drop(c);
    let s = site(&ctx, &headers).await;
    Ok((StatusCode::CREATED, Json(feed_json(&r, &s.url))).into_response())
}

async fn http_feeds_update(
    ctx: RequestCtx,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(b): Json<FeedBody>,
) -> PResult<Response> {
    let uid = uid_of(&ctx)?;
    let (name, def) = parse_body(b)?;
    let mut c = ctx.sql.conn().await?;
    let r = sqlx::query(
        "UPDATE feeds SET name = $3, definition = $4, updated_at = now() WHERE id = $1 AND user_id = $2
         RETURNING id, name, definition, updated_at",
    )
    .bind(id)
    .bind(uid)
    .bind(&name)
    .bind(serde_json::to_value(&def)?)
    .fetch_optional(&mut *c)
    .await?
    .ok_or_else(|| e("feed_not_found", "feed not found", 404))?;
    drop(c);
    let s = site(&ctx, &headers).await;
    Ok(Json(feed_json(&r, &s.url)).into_response())
}

async fn http_feeds_delete(ctx: RequestCtx, Path(id): Path<i64>) -> PResult<Response> {
    let uid = uid_of(&ctx)?;
    let mut c = ctx.sql.conn().await?;
    let n = sqlx::query("DELETE FROM feeds WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(uid)
        .execute(&mut *c)
        .await?
        .rows_affected();
    drop(c);
    if n == 0 {
        return Err(e("feed_not_found", "feed not found", 404));
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------------- 订阅 XML（LLD §1.3） ----------------

#[derive(serde::Deserialize, Default)]
struct KeyQuery {
    k: Option<String>,
}

/// 缓存：`fc:<feed>:<修改时间>:<token>`（订阅修改或 key 重置后自然失效）
async fn cached(ctx: &Ctx, key: &str) -> PResult<Option<String>> {
    let r = ctx.redis.call(RedisCommand::new("GET").arg(key)).await?;
    Ok(r.as_bytes().map(|b| String::from_utf8_lossy(b).into_owned()))
}

async fn store(ctx: &Ctx, key: &str, body: &str) -> PResult<()> {
    ctx.redis
        .call(
            RedisCommand::new("SET")
                .arg(key)
                .arg(body)
                .arg("EX")
                .arg(CACHE_SECS),
        )
        .await?;
    Ok(())
}

async fn http_feed_xml(
    ctx: RequestCtx,
    headers: HeaderMap,
    Path(file): Path<String>,
    Query(q): Query<KeyQuery>,
) -> PResult<Response> {
    let id: i64 = file
        .strip_suffix(".xml")
        .unwrap_or(&file)
        .parse()
        .map_err(|_| e("feed_not_found", "feed not found", 404))?;
    let p = key_auth(&ctx, q.k.as_deref(), "rss", &headers).await?;
    let uid = i64::try_from(p.user_id).unwrap_or_default();
    let mut c = ctx.sql.conn().await?;
    let row = sqlx::query("SELECT name, definition, (extract(epoch FROM updated_at) * 1000000)::bigint FROM feeds WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(uid)
        .fetch_optional(&mut *c)
        .await?
        .ok_or_else(|| e("feed_not_found", "feed not found", 404))?;
    drop(c);
    let name: String = row.get(0);
    let def: Definition = serde_json::from_value(row.get(1)).unwrap_or_default();
    let version: i64 = row.get(2);
    let ck = format!("fc:{id}:{version}:{}", p.credential_id);
    if let Some(x) = cached(&ctx, &ck).await? {
        return Ok(xml_resp(StatusCode::OK, x));
    }
    let limit = setting(&ctx, "items_per_feed", 50)
        .await
        .clamp(1, query::MAX_LIMIT);
    let fetch_n = if def.promotion.is_empty() {
        limit
    } else {
        limit * 4
    };
    let ctx_u = ctx.with_principal(&p)?;
    let mut items = query::fetch(&ctx_u, &def, fetch_n, 0).await?;
    items.retain(|it| query::promotion_ok(&def, it));
    items.truncate(usize::try_from(limit).unwrap_or(50));
    let s = site(&ctx, &headers).await;
    let k = q.k.unwrap_or_default();
    let body = xml::channel(
        &xml::Channel {
            site_name: &s.name,
            site_url: &s.url,
            title: &name,
            key: k.trim(),
        },
        &items,
    );
    store(&ctx, &ck, &body).await?;
    Ok(xml_resp(StatusCode::OK, body))
}

// ---------------- Torznab（LLD §1.4） ----------------

fn tz_error(status: StatusCode, code: u32, msg: &str) -> Response {
    xml_resp(status, xml::error(code, msg))
}

/// Torznab 参数 → 订阅定义 + 分类（取回后精确比对）；错误为说明文字（以 Newznab 错误 201 返回）
fn torznab_def(
    t: &str,
    q: &std::collections::HashMap<String, String>,
) -> Result<(Definition, Vec<u32>), String> {
    let bad = |m: &str| m.to_owned();
    let mut d = Definition::default();
    let mut words: Vec<String> = Vec::new();
    for k in ["q", "artist", "album", "author", "title"] {
        if let Some(v) = q.get(k).map(|v| v.trim()).filter(|v| !v.is_empty()) {
            words.push(v.to_owned());
        }
    }
    d.q = words.join(" ");
    let push = |d: &mut Definition, field: &str, value: Value| {
        d.filters.push(query::Filter {
            field: field.into(),
            op: "eq".into(),
            value,
        });
    };
    match t {
        "tvsearch" => d.type_ids = vec!["tv".into(), "anime".into()],
        "movie" => d.type_ids = vec!["movie".into()],
        "music" => d.type_ids = vec!["music".into()],
        "book" => {
            d.type_ids = vec!["generic".into()];
            push(&mut d, "kind", json!("ebook"));
        }
        _ => {}
    }
    if let Some(v) = q.get("imdbid").map(|v| v.trim()).filter(|v| !v.is_empty()) {
        let digits = v.trim_start_matches("tt");
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) || digits.len() > 10 {
            return Err(bad("imdbid must be digits"));
        }
        push(&mut d, "imdb_id", json!(format!("tt{digits:0>7}")));
    }
    for (param, field) in [
        ("tmdbid", "tmdb_id"),
        ("tvdbid", "tvdb_id"),
        ("season", "season"),
        ("ep", "episode"),
    ] {
        if let Some(v) = q.get(param).map(|v| v.trim()).filter(|v| !v.is_empty()) {
            let n: i64 = v
                .trim_start_matches(['S', 's', 'E', 'e'])
                .parse()
                .map_err(|_| bad(&format!("{param} must be a number")))?;
            push(&mut d, field, json!(n));
        }
    }
    let cats: Vec<u32> = match q.get("cat").map(|c| c.trim()).filter(|c| !c.is_empty()) {
        Some(c) => c
            .split(',')
            .map(|x| x.trim().parse::<u32>())
            .collect::<Result<_, _>>()
            .map_err(|_| bad("cat must be numbers"))?,
        None => Vec::new(),
    };
    if !cats.is_empty() {
        let mut types: Vec<String> = cats
            .iter()
            .flat_map(|c| xml::types_for_category(*c).iter().map(|s| (*s).to_owned()))
            .collect();
        types.sort();
        types.dedup();
        if !d.type_ids.is_empty() {
            types.retain(|t| d.type_ids.contains(t));
        }
        if types.is_empty() {
            // 请求的分类本站没有：返回空结果
            types.push("-".into());
        }
        d.type_ids = types;
    }
    Ok((d, cats))
}

async fn http_torznab(
    ctx: RequestCtx,
    headers: HeaderMap,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> PResult<Response> {
    let t = q.get("t").map_or("", String::as_str);
    let s = site(&ctx, &headers).await;
    if t == "caps" {
        return Ok(xml_resp(StatusCode::OK, xml::caps(&s.name)));
    }
    if !matches!(t, "search" | "tvsearch" | "movie" | "music" | "book") {
        return Ok(tz_error(StatusCode::BAD_REQUEST, 202, "No such function"));
    }
    let key = q.get("apikey").map(|k| k.trim().to_owned());
    let p = match key_auth(&ctx, key.as_deref(), "rss", &headers).await {
        Ok(p) => p,
        Err(err) if err.code == "rss.rate_limited" => {
            return Ok(tz_error(
                StatusCode::TOO_MANY_REQUESTS,
                500,
                "Request limit reached",
            ));
        }
        Err(err) if err.code == "rss.key_invalid" => {
            return Ok(tz_error(
                StatusCode::UNAUTHORIZED,
                100,
                "Incorrect user credentials",
            ));
        }
        Err(err) => return Err(err),
    };
    let (d, cats) = match torznab_def(t, &q) {
        Ok(v) => v,
        Err(m) => return Ok(tz_error(StatusCode::BAD_REQUEST, 201, &m)),
    };
    let limit = q
        .get("limit")
        .and_then(|l| l.parse::<i64>().ok())
        .unwrap_or(50)
        .clamp(1, query::MAX_LIMIT);
    let offset = q
        .get("offset")
        .and_then(|l| l.parse::<i64>().ok())
        .unwrap_or(0)
        .clamp(0, 1000);
    // 缓存键含查询与用户（可见性按用户一致）
    let mut keys: Vec<(&String, &String)> = q.iter().filter(|(k, _)| k.as_str() != "apikey").collect();
    keys.sort();
    let digest = hex::encode(Sha256::digest(format!("{}|{keys:?}", p.user_id).as_bytes()));
    let ck = format!("tz:{}", &digest[..32]);
    let key = key.unwrap_or_default();
    let body = if let Some(cached_items) = cached(&ctx, &ck).await? {
        cached_items
    } else {
        let ctx_u = ctx.with_principal(&p)?;
        let n = if cats.is_empty() { limit } else { limit * 4 };
        let mut items = query::fetch(&ctx_u, &d, n, offset).await?;
        items.retain(|it| xml::matches_categories(&xml::categories(&it.type_id, &it.fields), &cats));
        items.truncate(usize::try_from(limit).unwrap_or(50));
        let ch = xml::Channel {
            site_name: &s.name,
            site_url: &s.url,
            title: "Torznab",
            key: "{key}",
        };
        let body: String = items.iter().map(|it| xml::item(&ch, it)).collect();
        store(&ctx, &ck, &body).await?;
        body
    };
    // 缓存中不存放 key；输出时再填入（XML 转义后的形式）
    let items = body.replace("{key}", &xml::esc(&key));
    let x = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\" xmlns:torznab=\"http://torznab.com/schemas/2015/feed\"><channel><title>{}</title><link>{}</link>{items}</channel></rss>",
        xml::esc(&s.name),
        xml::esc(&s.url)
    );
    Ok(xml_resp(StatusCode::OK, x))
}

// ---------------- 下载（LLD §1.6） ----------------

async fn http_download(
    ctx: RequestCtx,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(q): Query<KeyQuery>,
) -> PResult<Response> {
    let p = match key_auth(&ctx, q.k.as_deref(), "download", &headers).await {
        Ok(p) => p,
        Err(err) => {
            let st = StatusCode::from_u16(err.status.unwrap_or(401)).unwrap_or(StatusCode::UNAUTHORIZED);
            return Ok(text_resp(st, &err.message));
        }
    };
    let ctx_u = ctx.with_principal(&p)?;
    let r = match ctx_u
        .call_json(
            "ctx.torrents",
            "build_download",
            json!({ "id": id, "via": "rss" }),
        )
        .await
    {
        Ok(r) => r,
        Err(err) => {
            // BT 客户端只显示纯文本
            let st = if err.code.ends_with("not_found") {
                StatusCode::NOT_FOUND
            } else if err.code.starts_with("kernel.") && !err.code.ends_with("forbidden") {
                return Err(err);
            } else {
                StatusCode::FORBIDDEN
            };
            return Ok(text_resp(st, &format!("{}: {}", err.code, err.message)));
        }
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(r["file_b64"].as_str().unwrap_or(""))
        .map_err(|_| PluginError::internal("bad torrent payload"))?;
    let filename = r["filename"].as_str().unwrap_or("download.torrent");
    let mut resp = bytes.into_response();
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-bittorrent"),
    );
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    Ok(resp)
}

struct Routes;

impl HttpRoutes for Routes {
    fn router(&self) -> Router {
        Router::new()
            .route(
                "/v1/me/key",
                get(http_key_get).post(http_key_reset).delete(http_key_revoke),
            )
            .route("/v1/me/feeds", get(http_feeds_list).post(http_feeds_create))
            .route(
                "/v1/me/feeds/{id}",
                axum::routing::put(http_feeds_update).delete(http_feeds_delete),
            )
            .route("/v1/feeds/{file}", get(http_feed_xml))
            .route("/v1/torznab/api", get(http_torznab))
            .route("/v1/dl/{id}", get(http_download))
    }
}

pub fn plugin() -> NativePlugin {
    NativePlugin {
        manifest: MANIFEST,
        migrations: MIGRATIONS,
        http: Some(Arc::new(Routes)),
        ..Default::default()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn q(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn torznab_params() {
        let (d, cats) = torznab_def(
            "tvsearch",
            &q(&[("q", "Show"), ("season", "S02"), ("ep", "3"), ("imdbid", "123")]),
        )
        .unwrap();
        assert_eq!(d.type_ids, vec!["tv", "anime"]);
        assert_eq!(d.q, "Show");
        assert!(cats.is_empty());
        let f: Vec<(String, Value)> = d.filters.into_iter().map(|f| (f.field, f.value)).collect();
        assert!(f.contains(&("imdb_id".into(), json!("tt0000123"))));
        assert!(f.contains(&("season".into(), json!(2))));
        assert!(f.contains(&("episode".into(), json!(3))));
        let (d, cats) = torznab_def("search", &q(&[("cat", "2045,5070")])).unwrap();
        assert_eq!(
            (d.type_ids, cats),
            (vec!["anime".to_owned(), "movie".to_owned()], vec![2045, 5070])
        );
        let (d, _) = torznab_def("movie", &q(&[("cat", "3000")])).unwrap();
        assert_eq!(d.type_ids, vec!["-"]);
        assert!(torznab_def("search", &q(&[("cat", "x")])).is_err());
        assert!(torznab_def("movie", &q(&[("imdbid", "tt12ab")])).is_err());
    }
}
