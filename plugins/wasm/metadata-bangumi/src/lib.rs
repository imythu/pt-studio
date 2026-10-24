//! metadata-bangumi 插件（WASM，docs/plugins/metadata-bangumi/）：`ctx.metadata` 的 Bangumi（bgm.tv）提供方。
//!
//! - 公开 API `api.bgm.tv/v0`，无需申请凭证；User-Agent 为 `PT-Studio/<版本> (<project_url>)`（默认项目仓库地址，可在设置中改）。
//!   访问令牌可选：NSFW 条目需要令牌且开启 `allow_nsfw`，否则按不存在处理。
//! - `lookup`（条目编号 / `bangumi:<编号>` / 条目链接）、`search`（`POST /v0/search/subjects`）、`GET /v1/related`（关联条目）。
//! - 归一化、PG 缓存（30 天；连载中 7 天；负缓存 1 天；上游失败时返回过期数据）、封面转存、`metadata.fetched`、
//!   限速（默认每秒 5 次，可设置）与熔断（连续 10 次失败打开 5 分钟）、每小时 `refresh_stale`。

pt_plugin_sdk::world!("metadata-bangumi", http);

pub mod mapper;

use exports::pt::plugin::{http_handler, on_cron, service_provider};
use pt_plugin_sdk::{blob, http_client, prelude::*, redis, serde_json, sql, time, upstream::Upstream};
use sha2::{Digest, Sha256};

struct Bangumi;

const API: Upstream = Upstream::new("api", "metadata-bangumi.upstream_error").per_sec(5);
const REFRESH_BATCH: i64 = 200;
const REFRESH_BUDGET_MS: i64 = 8 * 60 * 1000;
const PROJECT_URL: &str = "https://github.com/imythu/pt-studio";

struct Cfg {
    api: String,
    ua: String,
    token: Option<String>,
    allow_nsfw: bool,
    rate: i64,
    cache_days: i64,
}

/// 插件设置优先，site.toml `[plugin.metadata-bangumi]` 为后备
fn cfg() -> Cfg {
    let (set, stat) = (ctx::settings(), ctx::static_config());
    let st = |k: &str| {
        [&set, &stat].iter().find_map(|c| {
            c[k].as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        })
    };
    let num = |k: &str| set[k].as_i64().or_else(|| stat[k].as_i64());
    let project = st("project_url").unwrap_or_else(|| PROJECT_URL.into());
    Cfg {
        api: st("api_base_url")
            .unwrap_or_else(|| "https://api.bgm.tv".into())
            .trim_end_matches('/')
            .to_owned(),
        ua: format!("PT-Studio/{} ({project})", env!("CARGO_PKG_VERSION")),
        token: st("access_token"),
        allow_nsfw: set["allow_nsfw"]
            .as_bool()
            .or_else(|| stat["allow_nsfw"].as_bool())
            .unwrap_or(false),
        rate: num("rate_per_sec").unwrap_or(5).clamp(1, 20),
        cache_days: num("cache_days").unwrap_or(30).clamp(1, 365),
    }
}

fn upstream(c: &Cfg) -> Upstream {
    Upstream {
        per_sec: c.rate,
        ..API
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn send(c: &Cfg, method: &str, url: &str, body: Option<Vec<u8>>) -> R<http_client::OutboundResponse> {
    let mut headers = vec![
        http_client::Header {
            name: "user-agent".into(),
            value: c.ua.clone(),
        },
        http_client::Header {
            name: "accept".into(),
            value: "application/json".into(),
        },
    ];
    if body.is_some() {
        headers.push(http_client::Header {
            name: "content-type".into(),
            value: "application/json".into(),
        });
    }
    if let Some(t) = &c.token {
        headers.push(http_client::Header {
            name: "authorization".into(),
            value: format!("Bearer {t}"),
        });
    }
    http_client::send(&http_client::OutboundRequest {
        method: method.into(),
        url: url.into(),
        headers,
        body,
        timeout_ms: 10_000,
    })
}

/// API 请求：限速与熔断（5xx、429、连接失败计为失败）；404 → None
fn api(c: &Cfg, method: &str, path: &str, body: Option<&Value>) -> R<Option<Value>> {
    let u = upstream(c);
    u.acquire()?;
    let r = send(
        c,
        method,
        &format!("{}{path}", c.api),
        body.map(|b| b.to_string().into_bytes()),
    );
    u.record(r.as_ref().is_ok_and(|r| r.status < 500 && r.status != 429));
    let r = r.map_err(|e| PluginError { retryable: true, ..e })?;
    match r.status {
        200 => serde_json::from_slice(&r.body)
            .map(Some)
            .map_err(|_| err("metadata-bangumi.upstream_error", "invalid JSON from Bangumi")),
        404 => Ok(None),
        s => Err(PluginError {
            retryable: s == 429 || s >= 500,
            ..err(
                "metadata-bangumi.upstream_error",
                &format!("Bangumi returned {s}"),
            )
        }),
    }
}

/// 封面转存（`images.large`）：只接受 jpeg / png / webp 且 ≤ 2 MiB；内容寻址
fn rehost(c: &Cfg, url: &str) -> Option<String> {
    let r = send(c, "GET", url, None).ok()?;
    let ct = r
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("content-type"))
        .map(|h| h.value.clone());
    let ext = match ct.as_deref().map(|t| t.split(';').next().unwrap_or("").trim()) {
        Some("image/jpeg") => "jpg",
        Some("image/png") => "png",
        Some("image/webp") => "webp",
        _ => return None,
    };
    if r.status != 200 || r.body.is_empty() || r.body.len() > 2 << 20 {
        return None;
    }
    let name = format!("images/{}.{ext}", hex(&Sha256::digest(&r.body)));
    blob::put(
        &format!("public/{name}"),
        ct.as_deref().unwrap_or("image/jpeg"),
        &r.body,
    )
    .ok()?;
    Some(format!("/media/metadata-bangumi/{name}"))
}

fn fetch(c: &Cfg, id: u64) -> R<Option<Value>> {
    let Some(v) = api(c, "GET", &format!("/v0/subjects/{id}"), None)? else {
        return Ok(None);
    };
    // 未允许的 NSFW 条目按不存在处理
    if v["nsfw"] == true && !(c.allow_nsfw && c.token.is_some()) {
        return Ok(None);
    }
    let mut data = mapper::map(&v);
    if let Some(p) = v["images"]["large"].as_str().and_then(|u| rehost(c, u)) {
        data["images"]["cover"] = json!(p);
        data["images"]["poster"] = json!(p);
    }
    Ok(Some(data))
}

/// 负缓存放在 Redis（1 天）：“不存在”以错误返回给 HTTP 调用方时工作单元会回滚，PG 中的记录会丢失
fn known_missing(key: &str) -> bool {
    matches!(
        redis::call(&redis::Command {
            name: "EXISTS".into(),
            args: vec![format!("nf:{key}").into_bytes()]
        }),
        Ok(redis::RedisReply::Int(1))
    )
}

fn mark_missing(key: &str) {
    let _ = redis::call(&redis::Command {
        name: "SET".into(),
        args: vec![
            format!("nf:{key}").into_bytes(),
            b"1".to_vec(),
            b"EX".to_vec(),
            b"86400".to_vec(),
        ],
    });
}

/// 返回 (编号, 数据, 是否过期降级)；`refresh` 为后台刷新：忽略缓存新鲜度，不更新 `last_used_at`
fn lookup(raw: &str, refresh: bool) -> R<Option<(String, Value, bool)>> {
    let id = mapper::parse_id(raw).ok_or_else(|| {
        err(
            "metadata-bangumi.id_invalid",
            "expected a Bangumi subject ID or link",
        )
    })?;
    let key = id.to_string();
    let c = cfg();
    if !refresh && known_missing(&key) {
        return Ok(None);
    }
    let cached = sql::query(
        "SELECT data::text, not_found,
                fetched_at > now() - make_interval(days => CASE WHEN not_found THEN 1 WHEN volatile THEN least(7, $2::int) ELSE $2::int END)
           FROM cache WHERE external_id = $1 AND locale = 'zh-CN'",
        &[SqlValue::Text(key.clone()), SqlValue::Int(c.cache_days)],
    )?;
    let hit = cached
        .rows
        .first()
        .map(|r| (r.text(0), r.bool(1).unwrap_or(false), r.bool(2).unwrap_or(false)));
    if let Some((data, not_found, true)) = &hit
        && !refresh
    {
        sql::execute(
            "UPDATE cache SET last_used_at = now() WHERE external_id = $1 AND locale = 'zh-CN'",
            &[SqlValue::Text(key.clone())],
        )?;
        if *not_found {
            return Ok(None);
        }
        return Ok(Some((
            key,
            serde_json::from_str(data.as_deref().unwrap_or("null")).unwrap_or(Value::Null),
            false,
        )));
    }
    let fetched = match fetch(&c, id) {
        Ok(f) => f,
        Err(e) => {
            if let Some((Some(data), false, _)) = hit {
                return Ok(Some((
                    key,
                    serde_json::from_str(&data).unwrap_or(Value::Null),
                    true,
                )));
            }
            return Err(e);
        }
    };
    let Some(data) = fetched else {
        mark_missing(&key);
        return Ok(None);
    };
    let hash = hex(&Sha256::digest(data.to_string().as_bytes()));
    let old = sql::query(
        "SELECT content_hash FROM cache WHERE external_id = $1 AND locale = 'zh-CN'",
        &[SqlValue::Text(key.clone())],
    )?;
    let changed = old
        .rows
        .first()
        .is_none_or(|r| r.text(0).as_deref() != Some(hash.as_str()));
    let volatile = mapper::volatile(&data, time::now().div_euclid(86_400_000));
    sql::execute(
        "INSERT INTO cache (external_id, locale, data, not_found, content_hash, volatile, fetched_at) VALUES ($1, 'zh-CN', $2, false, $3, $4, now())
         ON CONFLICT (external_id, locale) DO UPDATE SET data = EXCLUDED.data, not_found = false, content_hash = EXCLUDED.content_hash,
                volatile = EXCLUDED.volatile, fetched_at = now(), last_used_at = CASE WHEN $5 THEN cache.last_used_at ELSE now() END",
        &[SqlValue::Text(key.clone()), SqlValue::Json(data.to_string()), SqlValue::Text(hash), SqlValue::Boolean(volatile), SqlValue::Boolean(refresh)],
    )?;
    if changed {
        ctx::publish(
            "metadata.fetched",
            &format!("metadata:bangumi:{key}"),
            &json!({ "source": "bangumi", "external_id": key, "type_id": data["kind"], "group_id": null }),
        )?;
    }
    Ok(Some((key, data, false)))
}

/// Bangumi 条目类型：1 书籍、2 动画、3 音乐、4 游戏、6 三次元
fn subject_type(type_id: &str) -> i64 {
    match type_id {
        "book" | "ebook" => 1,
        "music" => 3,
        "game" => 4,
        "tv" => 6,
        _ => 2,
    }
}

fn search(query: &str, type_id: &str, limit: u32) -> R<Vec<Value>> {
    let q = query.trim();
    if q.is_empty() || q.chars().count() > 100 {
        return Err(err(
            "kernel.validation_failed",
            "query must be 1..=100 characters",
        ));
    }
    let c = cfg();
    let n = limit.clamp(1, 20);
    let body = json!({ "keyword": q, "sort": "match", "filter": { "type": [subject_type(type_id)], "nsfw": c.allow_nsfw && c.token.is_some() } });
    let r = api(&c, "POST", &format!("/v0/search/subjects?limit={n}"), Some(&body))?.unwrap_or(Value::Null);
    Ok(r["data"]
        .as_array()
        .map(|a| a.iter().take(n as usize).map(mapper::brief).collect())
        .unwrap_or_default())
}

/// 关联条目（前传 / 续集 / 番外篇……）
fn related(raw: &str) -> R<Vec<Value>> {
    let id = mapper::parse_id(raw).ok_or_else(|| {
        err(
            "metadata-bangumi.id_invalid",
            "expected a Bangumi subject ID or link",
        )
    })?;
    let Some(r) = api(&cfg(), "GET", &format!("/v0/subjects/{id}/subjects"), None)? else {
        return Err(err("metadata-bangumi.not_found", "subject not found"));
    };
    Ok(r.as_array()
        .map(|a| a.iter().take(50).map(mapper::brief).collect())
        .unwrap_or_default())
}

impl service_provider::Guest for Bangumi {
    fn call(service: String, method: String, args: String) -> R<String> {
        if service != "ctx.metadata" {
            return Err(err("kernel.not_found", "unknown service"));
        }
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        let out = match method.as_str() {
            "info" => json!({ "source": "bangumi", "type_ids": ["anime", "generic", "music"],
                              "id_pattern": "^\\d{1,10}$|^bangumi:\\d{1,10}$|(bgm|bangumi)\\.tv/subject/\\d+" }),
            "lookup" => match lookup(a["external_id"].as_str().unwrap_or(""), false)? {
                Some((id, data, stale)) => {
                    json!({ "source": "bangumi", "external_id": id, "data": data, "stale": stale })
                }
                None => Value::Null,
            },
            "search" => json!(search(
                a["query"].as_str().unwrap_or(""),
                a["type_id"].as_str().unwrap_or("anime"),
                u32::try_from(a["limit"].as_u64().unwrap_or(10)).unwrap_or(10)
            )?),
            other => {
                return Err(err(
                    "kernel.not_found",
                    &format!("ctx.metadata has no method `{other}`"),
                ));
            }
        };
        Ok(out.to_string())
    }
}

impl http_handler::Guest for Bangumi {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let reply = match req.route() {
            "lookup" => match lookup(req.query("id").unwrap_or(""), false)? {
                Some((id, data, stale)) => http::Reply::ok(
                    &json!({ "source": "bangumi", "external_id": id, "data": data, "stale": stale }),
                ),
                None => return Err(err("metadata-bangumi.not_found", "subject not found")),
            },
            "search" => http::Reply::ok(
                &json!({ "items": search(req.query("q").unwrap_or(""), req.query("type").unwrap_or("anime"), 10)? }),
            ),
            "related" => http::Reply::ok(&json!({ "items": related(req.query("id").unwrap_or(""))? })),
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

/// 过期且近 90 天用过的条目逐个重新获取；熔断打开或超出时长时停止，下次运行继续
fn refresh_stale() -> R<()> {
    let c = cfg();
    let started = time::now();
    let rows = sql::query(
        "SELECT external_id FROM cache
          WHERE NOT not_found AND locale = 'zh-CN' AND last_used_at > now() - interval '90 days'
            AND fetched_at < now() - make_interval(days => CASE WHEN volatile THEN least(7, $1::int) ELSE $1::int END)
          ORDER BY last_used_at DESC LIMIT $2",
        &[SqlValue::Int(c.cache_days), SqlValue::Int(REFRESH_BATCH)],
    )?;
    let (mut ok, mut failed) = (0u32, 0u32);
    for r in &rows.rows {
        if time::now() - started > REFRESH_BUDGET_MS || API.is_open() {
            break;
        }
        match lookup(r.text(0).as_deref().unwrap_or(""), true) {
            Ok(_) => ok += 1,
            Err(_) => failed += 1,
        }
        // 每条单独提交：中途超时也保留已刷新的结果
        sql::commit()?;
    }
    logging::info(
        "refresh_stale finished",
        &[
            ("refreshed", &ok.to_string()),
            ("failed", &failed.to_string()),
            ("stale", &rows.rows.len().to_string()),
        ],
    );
    Ok(())
}

impl on_cron::Guest for Bangumi {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            "refresh_stale" => refresh_stale(),
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

export!(Bangumi);
