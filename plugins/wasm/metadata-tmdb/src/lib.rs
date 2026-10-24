//! metadata-tmdb 插件（WASM，docs/plugins/metadata-tmdb/）：`ctx.metadata` 的 TMDB 提供方。
//!
//! 本版本实现：`lookup`（movie / tv / imdb 换算）、`search`、归一化、PG 结果缓存（含负缓存、过期降级）、海报转存（类型与大小校验、
//! 内容寻址）、`metadata.fetched`（数据变化时）、HTTP `/v1/lookup` 与 `/v1/search`；API 限速（每秒 30 次）与熔断（连续 10 次失败
//! 打开 5 分钟，期间有缓存就降级返回）、429 / 5xx 重试；翻译标题、背景图转存；近期上映或连载中的条目缓存 7 天；
//! 每小时 `refresh_stale` 刷新近 90 天用过的过期条目。

#![allow(clippy::missing_errors_doc)]

pt_plugin_sdk::world!("metadata-tmdb");

pub mod mapper;

use std::time::Duration;

use exports::pt::plugin::{http_handler, on_cron, service_provider};
use mapper::Id;
use pt_plugin_sdk::{
    blob, config, events, http_client,
    sql::{self, SqlValue},
    time,
    types::PluginError,
    upstream::{self, Upstream},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

struct Tmdb;

/// TMDB API（图片 CDN 不计入）
const API: Upstream = Upstream::new("api", "metadata-tmdb.upstream_error");
/// 单次 `refresh_stale` 最多刷新的条目数与时长
const REFRESH_BATCH: i64 = 300;
const REFRESH_BUDGET_MS: i64 = 8 * 60 * 1000;

type R<T> = Result<T, PluginError>;

fn err(code: &str, message: &str) -> PluginError {
    PluginError {
        code: code.into(),
        message: message.into(),
        details: None,
        retryable: false,
    }
}

struct Cfg {
    api: String,
    image: String,
    key: Option<String>,
    token: Option<String>,
    cache_days: i64,
}

/// 后台插件设置优先，site.toml `[plugin.metadata-tmdb]` 为后备
fn cfg() -> Cfg {
    let set: Value = serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null);
    let stat: Value = serde_json::from_str(&config::get_static()).unwrap_or(Value::Null);
    let st = |k: &str| {
        [&set, &stat]
            .iter()
            .find_map(|c| c[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned))
    };
    Cfg {
        api: st("api_base_url")
            .unwrap_or_else(|| "https://api.themoviedb.org".into())
            .trim_end_matches('/')
            .to_owned(),
        image: st("image_base_url")
            .unwrap_or_else(|| "https://image.tmdb.org".into())
            .trim_end_matches('/')
            .to_owned(),
        key: st("api_key"),
        token: st("access_token"),
        cache_days: set["cache_days"]
            .as_i64()
            .or_else(|| stat["cache_days"].as_i64())
            .unwrap_or(30)
            .clamp(1, 365),
    }
}

fn get(c: &Cfg, path: &str, query: &[(&str, String)]) -> R<http_client::OutboundResponse> {
    let mut q: Vec<String> = query.iter().map(|(k, v)| format!("{k}={}", enc(v))).collect();
    let mut headers = vec![http_client::Header {
        name: "accept".into(),
        value: "application/json".into(),
    }];
    match (&c.token, &c.key) {
        (Some(t), _) => headers.push(http_client::Header {
            name: "authorization".into(),
            value: format!("Bearer {t}"),
        }),
        (None, Some(k)) => q.push(format!("api_key={}", enc(k))),
        (None, None) => {
            return Err(err(
                "metadata-tmdb.not_configured",
                "TMDB API key is not configured",
            ));
        }
    }
    let url = if path.starts_with("http") {
        path.to_owned()
    } else {
        format!("{}{path}?{}", c.api, q.join("&"))
    };
    http_client::send(&http_client::OutboundRequest {
        method: "GET".into(),
        url,
        headers,
        body: None,
        timeout_ms: 8000,
    })
}

fn header<'a>(r: &'a http_client::OutboundResponse, name: &str) -> Option<&'a str> {
    r.headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

/// API 请求：限速、熔断；429 按 `Retry-After`（≤ 2 秒）、5xx 退避 200 / 800 ms，各最多重试 2 次
fn api_json(c: &Cfg, path: &str, query: &[(&str, String)]) -> R<Option<Value>> {
    let mut attempt = 0;
    let r = loop {
        API.acquire()?;
        let r = match get(c, path, query) {
            Ok(r) => r,
            Err(e) => {
                // 未配置不算上游故障
                API.record(e.code == "metadata-tmdb.not_configured");
                return Err(e);
            }
        };
        // 每次上游应答都计入熔断（重试中的失败也算）
        API.record(r.status < 500 && r.status != 429);
        let retry = match r.status {
            429 => Some(upstream::retry_after(header(&r, "retry-after"), 2)),
            s if s >= 500 => Some(Duration::from_millis(if attempt == 0 { 200 } else { 800 })),
            _ => None,
        };
        match retry {
            Some(wait) if attempt < 2 => {
                attempt += 1;
                std::thread::sleep(wait);
            }
            _ => break r,
        }
    };
    let (status, body) = (r.status, r.body);
    match status {
        200 => serde_json::from_slice(&body)
            .map(Some)
            .map_err(|_| err("metadata-tmdb.upstream_error", "invalid JSON from TMDB")),
        404 => Ok(None),
        401 => Err(err("metadata-tmdb.not_configured", "TMDB rejected the API key")),
        s => Err(PluginError {
            retryable: s == 429 || s >= 500,
            ..err("metadata-tmdb.upstream_error", &format!("TMDB returned {s}"))
        }),
    }
}

/// 图片转存（海报 `w500`、背景图 `w1280`）：只接受 jpeg / png / webp 且 ≤ 2 MiB；内容寻址，返回公开路径
fn rehost(c: &Cfg, size: &str, path: &str) -> Option<String> {
    let r = get(c, &format!("{}/t/p/{size}{path}", c.image), &[]).ok()?;
    let ct = header(&r, "content-type").map(str::to_owned);
    let (status, body) = (r.status, r.body);
    let ext = match ct.as_deref().map(|t| t.split(';').next().unwrap_or("").trim()) {
        Some("image/jpeg") => "jpg",
        Some("image/png") => "png",
        Some("image/webp") => "webp",
        _ => return None,
    };
    if status != 200 || body.is_empty() || body.len() > 2 << 20 {
        return None;
    }
    let name = format!("images/{}.{ext}", hex(&Sha256::digest(&body)));
    blob::put(
        &format!("public/{name}"),
        ct.as_deref().unwrap_or("image/jpeg"),
        &body,
    )
    .ok()?;
    Some(format!("/media/metadata-tmdb/{name}"))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn text(v: &SqlValue) -> String {
    match v {
        SqlValue::Text(s) | SqlValue::Json(s) => s.clone(),
        _ => String::new(),
    }
}

/// imdb:tt… → movie:… / tv:…
fn resolve(c: &Cfg, id: Id) -> R<Option<Id>> {
    let Id::Imdb(tt) = id else { return Ok(Some(id)) };
    let Some(r) = api_json(
        c,
        &format!("/3/find/{tt}"),
        &[("external_source", "imdb_id".into())],
    )?
    else {
        return Ok(None);
    };
    if let Some(m) = r["movie_results"][0]["id"].as_u64() {
        return Ok(Some(Id::Movie(m)));
    }
    Ok(r["tv_results"][0]["id"].as_u64().map(|t| Id::Tv(t, None)))
}

/// 返回 (规范 ID, 数据, 是否过期降级)；`refresh` 为后台刷新：忽略缓存新鲜度，不更新 `last_used_at`
fn lookup(external_id: &str, locale: &str, refresh: bool) -> R<Option<(String, Value, bool)>> {
    let c = cfg();
    let id = mapper::parse_id(external_id).ok_or_else(|| {
        err(
            "metadata-tmdb.id_invalid",
            "expected movie:<id>, tv:<id>[:s<n>] or imdb:tt<digits>",
        )
    })?;
    let raw_key = id.key();
    let cached = sql::query(
        "SELECT data::text, not_found,
                fetched_at > now() - make_interval(days => CASE WHEN not_found THEN 1 WHEN volatile THEN least(7, $3::int) ELSE $3::int END)
           FROM cache
          WHERE external_id = $1 AND locale = $2",
        &[SqlValue::Text(raw_key.clone()), SqlValue::Text(locale.into()), SqlValue::Int(c.cache_days)],
    )?;
    let hit = cached.rows.first().map(|r| {
        (
            text(&r[0]),
            matches!(r[1], SqlValue::Boolean(true)),
            matches!(r[2], SqlValue::Boolean(true)),
        )
    });
    if let Some((data, not_found, fresh)) = &hit
        && *fresh
        && !refresh
    {
        sql::execute(
            "UPDATE cache SET last_used_at = now() WHERE external_id = $1 AND locale = $2",
            &[SqlValue::Text(raw_key.clone()), SqlValue::Text(locale.into())],
        )?;
        if *not_found {
            return Ok(None);
        }
        let v: Value = serde_json::from_str(data).unwrap_or(Value::Null);
        return Ok(Some((
            v["_external_id"].as_str().unwrap_or(&raw_key).to_owned(),
            v["data"].clone(),
            false,
        )));
    }
    let fetched = (|| -> R<Option<(String, Value)>> {
        let Some(id) = resolve(&c, id.clone())? else {
            return Ok(None);
        };
        let (kind, path) = match &id {
            Id::Movie(m) => ("movie", format!("/3/movie/{m}")),
            Id::Tv(t, None) => ("tv", format!("/3/tv/{t}")),
            Id::Tv(t, Some(s)) => ("tv", format!("/3/tv/{t}/season/{s}")),
            Id::Imdb(_) => return Ok(None),
        };
        let Some(r) = api_json(
            &c,
            &path,
            &[
                ("language", locale.into()),
                ("append_to_response", "credits,external_ids,translations".into()),
            ],
        )?
        else {
            return Ok(None);
        };
        let mut data = mapper::map(kind, &r, locale);
        if let Some(p) = r["poster_path"].as_str().and_then(|p| rehost(&c, "w500", p)) {
            data["images"]["poster"] = json!(p);
        }
        if let Some(p) = r["backdrop_path"].as_str().and_then(|p| rehost(&c, "w1280", p)) {
            data["images"]["backdrop"] = json!(p);
        }
        Ok(Some((id.key(), data)))
    })();
    let fetched = match fetched {
        Ok(f) => f,
        // 上游失败：有过期缓存就降级返回
        Err(e) => {
            if let Some((data, false, _)) = hit {
                let v: Value = serde_json::from_str(&data).unwrap_or(Value::Null);
                return Ok(Some((
                    v["_external_id"].as_str().unwrap_or(&raw_key).to_owned(),
                    v["data"].clone(),
                    true,
                )));
            }
            return Err(e);
        }
    };
    match fetched {
        None => {
            sql::execute(
                "INSERT INTO cache (external_id, locale, data, not_found, fetched_at) VALUES ($1, $2, NULL, true, now())
                 ON CONFLICT (external_id, locale) DO UPDATE SET not_found = true, data = NULL, fetched_at = now()",
                &[SqlValue::Text(raw_key), SqlValue::Text(locale.into())],
            )?;
            Ok(None)
        }
        Some((key, data)) => {
            let hash = hex(&Sha256::digest(data.to_string().as_bytes()));
            let stored = json!({ "_external_id": key, "data": data });
            let old = sql::query(
                "SELECT content_hash FROM cache WHERE external_id = $1 AND locale = $2",
                &[SqlValue::Text(raw_key.clone()), SqlValue::Text(locale.into())],
            )?;
            let changed = old.rows.first().is_none_or(|r| text(&r[0]) != hash);
            let volatile = mapper::volatile(&data, mapper::day_of(time::now()));
            sql::execute(
                "INSERT INTO cache (external_id, locale, data, not_found, content_hash, volatile, fetched_at) VALUES ($1, $2, $3, false, $4, $5, now())
                 ON CONFLICT (external_id, locale) DO UPDATE SET data = EXCLUDED.data, not_found = false, content_hash = EXCLUDED.content_hash,
                        volatile = EXCLUDED.volatile, fetched_at = now(),
                        last_used_at = CASE WHEN $6 THEN cache.last_used_at ELSE now() END",
                &[
                    SqlValue::Text(raw_key),
                    SqlValue::Text(locale.into()),
                    SqlValue::Json(stored.to_string()),
                    SqlValue::Text(hash),
                    SqlValue::Boolean(volatile),
                    SqlValue::Boolean(refresh),
                ],
            )?;
            // 首次或内容变化时通知
            if changed {
                events::publish(&events::NewEvent {
                    event_type: "metadata.fetched".into(),
                    version: 1,
                    key: format!("metadata:tmdb:{key}"),
                    payload: json!({ "source": "tmdb", "external_id": key, "type_id": data["kind"], "group_id": null }).to_string(),
                })?;
            }
            Ok(Some((key, data, false)))
        }
    }
}

fn search(query: &str, type_id: &str, limit: u32, language: &str, year: Option<u32>) -> R<Vec<Value>> {
    let c = cfg();
    let q = query.trim();
    if q.is_empty() || q.chars().count() > 100 {
        return Err(err(
            "kernel.validation_failed",
            "query must be 1..=100 characters",
        ));
    }
    let kind = if type_id == "movie" { "movie" } else { "tv" };
    let mut params: Vec<(&str, String)> = vec![("query", q.into()), ("language", language.into())];
    if let Some(y) = year {
        params.push((
            if kind == "movie" {
                "year"
            } else {
                "first_air_date_year"
            },
            y.to_string(),
        ));
    }
    let Some(r) = api_json(&c, &format!("/3/search/{kind}"), &params)? else {
        return Ok(vec![]);
    };
    Ok(r["results"]
        .as_array()
        .map(|a| {
            a.iter()
                .take(usize::try_from(limit.clamp(1, 20)).unwrap_or(10))
                .map(|x| mapper::brief(kind, x))
                .collect()
        })
        .unwrap_or_default())
}

impl service_provider::Guest for Tmdb {
    fn call(service: String, method: String, args: String) -> R<String> {
        if service != "ctx.metadata" {
            return Err(err("kernel.not_found", "unknown service"));
        }
        let a: Value =
            serde_json::from_str(&args).map_err(|_| err("kernel.bad_request", "invalid JSON arguments"))?;
        let out = match method.as_str() {
            "info" => json!({ "source": "tmdb", "type_ids": ["movie", "tv", "anime"],
                              "id_pattern": "^(movie|tv):\\d+(:s\\d+)?$|^imdb:tt\\d{7,9}$" }),
            "lookup" => {
                let locale = a["locale"].as_str().unwrap_or("en-US");
                match lookup(a["external_id"].as_str().unwrap_or(""), locale, false)? {
                    Some((id, data, stale)) => {
                        json!({ "source": "tmdb", "external_id": id, "data": data, "stale": stale })
                    }
                    None => Value::Null,
                }
            }
            "search" => json!(search(
                a["query"].as_str().unwrap_or(""),
                a["type_id"].as_str().unwrap_or("movie"),
                u32::try_from(a["limit"].as_u64().unwrap_or(10)).unwrap_or(10),
                a["language"].as_str().unwrap_or("zh-CN"),
                a["year"].as_u64().and_then(|y| u32::try_from(y).ok())
            )?),
            _ => return Err(err("kernel.not_found", "unknown method")),
        };
        Ok(out.to_string())
    }
}

fn q<'a>(req: &'a http_handler::Request, k: &str) -> Option<&'a str> {
    req.query.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

fn resp(status: u16, v: &Value) -> http_handler::Response {
    http_handler::Response {
        status,
        headers: vec![http_handler::Header {
            name: "content-type".into(),
            value: "application/json".into(),
        }],
        body: v.to_string().into_bytes(),
    }
}

impl http_handler::Guest for Tmdb {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        match req.route_id.as_str() {
            "lookup" => {
                let id = q(&req, "id").unwrap_or("");
                match lookup(id, q(&req, "locale").unwrap_or("zh-CN"), false)? {
                    Some((id, data, stale)) => Ok(resp(
                        200,
                        &json!({ "source": "tmdb", "external_id": id, "stale": stale, "data": data }),
                    )),
                    None => Err(err("metadata-tmdb.not_found", "no such TMDB entry")),
                }
            }
            "search" => Ok(resp(
                200,
                &json!({ "items": search(
                    q(&req, "q").unwrap_or(""),
                    q(&req, "type").unwrap_or("movie"),
                    10,
                    q(&req, "language").filter(|l| l.len() <= 10).unwrap_or("zh-CN"),
                    q(&req, "year").and_then(|y| y.parse().ok()),
                )? }),
            )),
            _ => Err(err("kernel.not_found", "no such route")),
        }
    }
}

/// 过期且近 90 天用过的条目逐个重新获取（LLD §1.4）；熔断打开或超出时长时停在当前位置，下次运行继续
fn refresh_stale() -> R<()> {
    let c = cfg();
    let started = time::now();
    let rows = sql::query(
        "SELECT external_id, locale FROM cache
          WHERE NOT not_found AND last_used_at > now() - interval '90 days'
            AND fetched_at < now() - make_interval(days => CASE WHEN volatile THEN least(7, $1::int) ELSE $1::int END)
          ORDER BY last_used_at DESC LIMIT $2",
        &[SqlValue::Int(c.cache_days), SqlValue::Int(REFRESH_BATCH)],
    )?;
    let (mut ok, mut failed) = (0u32, 0u32);
    for r in &rows.rows {
        if time::now() - started > REFRESH_BUDGET_MS || API.is_open() {
            break;
        }
        match lookup(&text(&r[0]), &text(&r[1]), true) {
            Ok(_) => ok += 1,
            Err(e) if e.code == "metadata-tmdb.not_configured" => return Ok(()),
            Err(_) => failed += 1,
        }
        // 每条单独提交：中途超时也保留已刷新的结果
        sql::commit()?;
    }
    pt_plugin_sdk::logging::info(
        "refresh_stale finished",
        &[
            ("refreshed", &ok.to_string()),
            ("failed", &failed.to_string()),
            ("stale", &rows.rows.len().to_string()),
        ],
    );
    Ok(())
}

impl on_cron::Guest for Tmdb {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            "refresh_stale" => refresh_stale(),
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

export!(Tmdb);
