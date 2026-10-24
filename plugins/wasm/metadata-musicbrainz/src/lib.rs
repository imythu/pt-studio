//! metadata-musicbrainz 插件（WASM，docs/plugins/metadata-musicbrainz/）：`ctx.metadata` 的 MusicBrainz 提供方。
//!
//! - 公开 API `musicbrainz.org/ws/2`（核心数据 CC0），无需凭证；User-Agent 为 `PT-Studio/<版本> ( <contact> )`，
//!   联系方式默认是项目地址，可在设置中改为本站地址或邮箱。
//! - `lookup`：`release-group:<mbid>`（默认）、`release:<mbid>`、`artist:<mbid>` 或对应链接；`search`：发行组（Lucene 转义）。
//! - 封面：Cover Art Archive `front-500`（手动跟随到 archive.org 的重定向），类型与大小校验后转存。
//! - 全站每秒 1 次（设置可调高，供自建镜像）、熔断（连续 10 次失败打开 5 分钟）；PG 缓存 30 天（发行组至少 90 天）、负缓存 1 天、
//!   上游失败时返回过期数据；`metadata.fetched`；每小时 `refresh_stale`。

pt_plugin_sdk::world!("metadata-musicbrainz", http);

pub mod mapper;

use exports::pt::plugin::{http_handler, on_cron, service_provider};
use mapper::Id;
use pt_plugin_sdk::{blob, http_client, prelude::*, redis, serde_json, sql, time, upstream::Upstream};
use sha2::{Digest, Sha256};

struct MusicBrainz;

const API: Upstream = Upstream::new("api", "metadata-musicbrainz.upstream_error").per_sec(1);
const REFRESH_BATCH: i64 = 120;
const REFRESH_BUDGET_MS: i64 = 8 * 60 * 1000;
const PROJECT_URL: &str = "https://github.com/imythu/pt-studio";

struct Cfg {
    api: String,
    caa: String,
    ua: String,
    rate: i64,
    cache_days: i64,
}

/// 插件设置优先，site.toml `[plugin.metadata-musicbrainz]` 为后备
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
    let base = |k: &str, d: &str| st(k).unwrap_or_else(|| d.into()).trim_end_matches('/').to_owned();
    Cfg {
        api: base("api_base_url", "https://musicbrainz.org"),
        caa: base("caa_base_url", "https://coverartarchive.org"),
        ua: format!(
            "PT-Studio/{} ( {} )",
            env!("CARGO_PKG_VERSION"),
            st("contact").unwrap_or_else(|| PROJECT_URL.into())
        ),
        rate: num("rate_per_sec").unwrap_or(1).clamp(1, 50),
        cache_days: num("cache_days").unwrap_or(30).clamp(1, 365),
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

fn get(c: &Cfg, url: &str) -> R<http_client::OutboundResponse> {
    http_client::send(&http_client::OutboundRequest {
        method: "GET".into(),
        url: url.into(),
        headers: vec![
            http_client::Header {
                name: "user-agent".into(),
                value: c.ua.clone(),
            },
            http_client::Header {
                name: "accept".into(),
                value: "application/json".into(),
            },
        ],
        body: None,
        timeout_ms: 10_000,
    })
}

/// API 请求：全站限速与熔断（5xx——MusicBrainz 超限时返回 503——与连接失败计为失败）；404 / 400 → None
fn api(c: &Cfg, path: &str) -> R<Option<Value>> {
    let u = Upstream {
        per_sec: c.rate,
        ..API
    };
    u.acquire()?;
    let r = get(c, &format!("{}/ws/2/{path}", c.api));
    u.record(r.as_ref().is_ok_and(|r| r.status < 500 && r.status != 429));
    let r = r.map_err(|e| PluginError { retryable: true, ..e })?;
    match r.status {
        200 => serde_json::from_slice(&r.body).map(Some).map_err(|_| {
            err(
                "metadata-musicbrainz.upstream_error",
                "invalid JSON from MusicBrainz",
            )
        }),
        400 | 404 => Ok(None),
        s => Err(PluginError {
            retryable: s == 429 || s >= 500,
            ..err(
                "metadata-musicbrainz.upstream_error",
                &format!("MusicBrainz returned {s}"),
            )
        }),
    }
}

/// Cover Art Archive 正面封面（500px）：重定向到 archive.org，最多跟随 4 次；只接受 jpeg / png / webp 且 ≤ 2 MiB
fn cover(c: &Cfg, id: &Id) -> Option<String> {
    let entity = match id.entity {
        "release-group" | "release" => id.entity,
        _ => return None,
    };
    let mut url = format!("{}/{entity}/{}/front-500", c.caa, id.mbid);
    let r = (0..5).find_map(|_| {
        let r = get(c, &url).ok()?;
        match r.status {
            301 | 302 | 303 | 307 | 308 => {
                url = r
                    .headers
                    .iter()
                    .find(|h| h.name.eq_ignore_ascii_case("location"))
                    .map(|h| h.value.clone())?;
                None
            }
            _ => Some(r),
        }
    })?;
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
    Some(format!("/media/metadata-musicbrainz/{name}"))
}

fn fetch(c: &Cfg, id: &Id) -> R<Option<Value>> {
    let inc = match id.entity {
        "release-group" => "artist-credits+genres",
        "release" => "artist-credits+labels+media+genres+release-groups",
        _ => "aliases+genres",
    };
    let Some(v) = api(c, &format!("{}/{}?inc={inc}&fmt=json", id.entity, id.mbid))? else {
        return Ok(None);
    };
    let mut data = match id.entity {
        "release-group" => mapper::release_group(id, &v),
        "release" => mapper::release(id, &v),
        _ => mapper::artist(id, &v),
    };
    if let Some(p) = cover(c, id) {
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
            "metadata-musicbrainz.id_invalid",
            "expected release-group:<mbid>, release:<mbid>, artist:<mbid> or a MusicBrainz link",
        )
    })?;
    let key = id.key();
    let c = cfg();
    if !refresh && known_missing(&key) {
        return Ok(None);
    }
    // 发行组变化少，至少缓存 90 天
    let cached = sql::query(
        "SELECT data::text, not_found,
                fetched_at > now() - make_interval(days => CASE WHEN not_found THEN 1
                                                              WHEN external_id LIKE 'release-group:%' THEN greatest($2::int, 90) ELSE $2::int END)
           FROM cache WHERE external_id = $1 AND locale = 'en'",
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
            "UPDATE cache SET last_used_at = now() WHERE external_id = $1 AND locale = 'en'",
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
    let fetched = match fetch(&c, &id) {
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
        "SELECT content_hash FROM cache WHERE external_id = $1 AND locale = 'en'",
        &[SqlValue::Text(key.clone())],
    )?;
    let changed = old
        .rows
        .first()
        .is_none_or(|r| r.text(0).as_deref() != Some(hash.as_str()));
    sql::execute(
        "INSERT INTO cache (external_id, locale, data, not_found, content_hash, fetched_at) VALUES ($1, 'en', $2, false, $3, now())
         ON CONFLICT (external_id, locale) DO UPDATE SET data = EXCLUDED.data, not_found = false, content_hash = EXCLUDED.content_hash,
                fetched_at = now(), last_used_at = CASE WHEN $4 THEN cache.last_used_at ELSE now() END",
        &[SqlValue::Text(key.clone()), SqlValue::Json(data.to_string()), SqlValue::Text(hash), SqlValue::Boolean(refresh)],
    )?;
    if changed {
        ctx::publish(
            "metadata.fetched",
            &format!("metadata:musicbrainz:{key}"),
            &json!({ "source": "musicbrainz", "external_id": key, "type_id": "music", "group_id": null }),
        )?;
    }
    Ok(Some((key, data, false)))
}

fn search(query: &str, limit: u32) -> R<Vec<Value>> {
    let q = query.trim();
    if q.is_empty() || q.chars().count() > 100 {
        return Err(err(
            "kernel.validation_failed",
            "query must be 1..=100 characters",
        ));
    }
    let n = limit.clamp(1, 20);
    let r = api(
        &cfg(),
        &format!(
            "release-group/?query={}&limit={n}&fmt=json",
            enc(&mapper::escape(q))
        ),
    )?
    .unwrap_or(Value::Null);
    Ok(r["release-groups"]
        .as_array()
        .map(|a| a.iter().take(n as usize).map(mapper::brief).collect())
        .unwrap_or_default())
}

impl service_provider::Guest for MusicBrainz {
    fn call(service: String, method: String, args: String) -> R<String> {
        if service != "ctx.metadata" {
            return Err(err("kernel.not_found", "unknown service"));
        }
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        let out = match method.as_str() {
            "info" => json!({ "source": "musicbrainz", "type_ids": ["music"],
                              "id_pattern": "^(release-group|release|artist):[0-9a-fA-F-]{36}$|musicbrainz\\.org/(release-group|release|artist)/" }),
            "lookup" => match lookup(a["external_id"].as_str().unwrap_or(""), false)? {
                Some((id, data, stale)) => {
                    json!({ "source": "musicbrainz", "external_id": id, "data": data, "stale": stale })
                }
                None => Value::Null,
            },
            "search" => json!(search(
                a["query"].as_str().unwrap_or(""),
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

impl http_handler::Guest for MusicBrainz {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let reply = match req.route() {
            "lookup" => match lookup(req.query("id").unwrap_or(""), false)? {
                Some((id, data, stale)) => http::Reply::ok(
                    &json!({ "source": "musicbrainz", "external_id": id, "data": data, "stale": stale }),
                ),
                None => return Err(err("metadata-musicbrainz.not_found", "no such MusicBrainz entry")),
            },
            "search" => http::Reply::ok(&json!({ "items": search(req.query("q").unwrap_or(""), 10)? })),
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
          WHERE NOT not_found AND locale = 'en' AND last_used_at > now() - interval '90 days'
            AND fetched_at < now() - make_interval(days => CASE WHEN external_id LIKE 'release-group:%' THEN greatest($1::int, 90) ELSE $1::int END)
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

impl on_cron::Guest for MusicBrainz {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            "refresh_stale" => refresh_stale(),
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

export!(MusicBrainz);
