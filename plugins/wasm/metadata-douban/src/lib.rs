//! metadata-douban 插件（WASM，docs/plugins/metadata-douban/）：`ctx.metadata` 的豆瓣提供方。
//!
//! - 三种上游（插件设置 `mode`）：`direct`（默认，直接解析豆瓣条目页，做法参考 PT-Gen；可填 Cookie 缓解限流）、
//!   `ptgen`（PT-Gen 兼容端点，如自建的 pt-gen-cfworker）、`proxy`（站点自建代理，LLD §1 的接口）。
//!   配置都在后台插件设置中（site.toml `[plugin.metadata-douban]` 的同名键仍作为后备）；默认关闭，启用开关即确认风险说明（设置变更有审计）。
//! - `lookup` / `search`、归一化到统一模型、PG 缓存（30 天、负缓存 1 天、上游失败时返回过期数据）、海报转存（主机在
//!   `image_hosts` 中，类型与大小校验）、`metadata.fetched`。
//! - `GET /v1/ptgen?id=&markup=`：按 PT-Gen 版式生成简介（BBCode / Markdown / 纯文本），海报使用转存后的站内地址。
//!
//! - 上游保护：条目获取与搜索每秒最多 1 次（LLD §1）；连续 5 次失败（5xx、429、被豆瓣拦截、连接失败）熔断 10 分钟，期间有缓存就降级返回。
//! - 每小时 `refresh_stale`：重新获取近 90 天用过的过期条目（每次最多 60 个，熔断打开时停止）。

pt_plugin_sdk::world!("metadata-douban", http);

pub mod mapper;
pub mod scrape;

use exports::pt::plugin::{http_handler, on_cron, service_provider};
use pt_plugin_sdk::{
    blob, config, http_client, kv, prelude::*, redis, serde_json, sql, time, upstream::Upstream,
};
use sha2::{Digest, Sha256};

struct Douban;

/// 豆瓣（或所配置的 PT-Gen / 代理）：按“一次条目获取 / 一次搜索”计数，图片不计
const UPSTREAM: Upstream = Upstream::new("douban", "metadata-douban.upstream_error")
    .per_sec(1)
    .breaker(5, 600);
const REFRESH_BATCH: i64 = 60;
const REFRESH_BUDGET_MS: i64 = 8 * 60 * 1000;

/// 经限速与熔断执行一次上游操作；可重试的错误（5xx、429、被拦截、连接失败）计为失败
fn guarded<T>(f: impl FnOnce() -> R<T>) -> R<T> {
    UPSTREAM.acquire()?;
    let r = f();
    match &r {
        Ok(_) => UPSTREAM.record(true),
        Err(e) if e.retryable => UPSTREAM.record(false),
        Err(_) => {}
    }
    r
}

struct Cfg {
    mode: String,
    base: String,
    token: Option<String>,
    cookie: Option<String>,
    image_hosts: Vec<String>,
    cache_days: i64,
}

/// 插件设置优先，site.toml 静态配置为后备
fn cfg() -> Cfg {
    let set: Value = serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null);
    let stat: Value = serde_json::from_str(&config::get_static()).unwrap_or(Value::Null);
    let st = |k: &str| {
        [&set, &stat]
            .iter()
            .find_map(|c| c[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned))
    };
    let mode = st("mode").unwrap_or_else(|| {
        if st("ptgen_url").is_some() {
            "ptgen".into()
        } else if st("proxy_base_url").is_some() {
            "proxy".into()
        } else {
            "direct".into()
        }
    });
    let base = match mode.as_str() {
        "ptgen" => st("ptgen_url"),
        "proxy" => st("proxy_base_url"),
        _ => Some(DOUBAN.into()),
    };
    let token = if mode == "ptgen" { st("ptgen_apikey") } else { st("proxy_token") };
    let hosts = [&set, &stat].iter().find_map(|c| c["image_hosts"].as_array().filter(|a| !a.is_empty()).cloned());
    Cfg {
        base: base.unwrap_or_default().trim_end_matches('/').to_owned(),
        mode,
        token,
        cookie: st("cookie"),
        image_hosts: hosts.map_or_else(
            || vec!["*.doubanio.com".into()],
            |a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
        ),
        cache_days: set["cache_days"].as_i64().or_else(|| stat["cache_days"].as_i64()).unwrap_or(30).clamp(1, 365),
    }
}

const DOUBAN: &str = "https://movie.douban.com";
const BOOK: &str = "https://book.douban.com";
const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

/// 设置中的启用开关；旧版本在后台页确认过的风险声明（kv `consent`）在设置未写入该键前继续有效
fn enabled() -> bool {
    let set: Value = serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null);
    if let Some(b) = set["enabled"].as_bool() {
        return b;
    }
    kv::get("consent")
        .ok()
        .flatten()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .is_some_and(|v| v["ack"] == true)
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

fn http_get(url: &str, token: Option<&str>, referer: Option<&str>) -> R<(u16, Vec<u8>, Option<String>)> {
    send(url, vec![http_client::Header { name: "accept".into(), value: "application/json".into() }], token, referer)
}

fn send(url: &str, mut headers: Vec<http_client::Header>, token: Option<&str>, referer: Option<&str>) -> R<(u16, Vec<u8>, Option<String>)> {
    if let Some(t) = token {
        headers.push(http_client::Header { name: "authorization".into(), value: format!("Bearer {t}") });
    }
    if let Some(r) = referer {
        headers.push(http_client::Header { name: "referer".into(), value: r.into() });
    }
    let r = http_client::send(&http_client::OutboundRequest { method: "GET".into(), url: url.into(), headers, body: None, timeout_ms: 10_000 })?;
    let ct = r.headers.iter().find(|h| h.name.eq_ignore_ascii_case("content-type")).map(|h| h.value.clone());
    Ok((r.status, r.body, ct))
}

fn upstream(c: &Cfg, url: &str) -> R<Option<Value>> {
    let (status, body, _) = http_get(url, c.token.as_deref(), None)?;
    match status {
        200 => serde_json::from_slice(&body).map(Some).map_err(|_| err("metadata-douban.upstream_error", "invalid JSON from upstream")),
        404 => Ok(None),
        s => Err(PluginError { retryable: s == 429 || s >= 500, ..err("metadata-douban.upstream_error", &format!("upstream returned {s}")) }),
    }
}

fn host_allowed(c: &Cfg, url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")) else { return false };
    let host = rest.split(['/', ':', '?']).next().unwrap_or("").to_ascii_lowercase();
    c.image_hosts.iter().any(|p| match p.strip_prefix("*.") {
        Some(s) => host.ends_with(&format!(".{s}")),
        None => host == *p,
    })
}

/// 海报转存：只接受 jpeg / png / webp 且 ≤ 2 MiB；内容寻址
fn rehost(c: &Cfg, url: &str) -> Option<String> {
    if !host_allowed(c, url) {
        return None;
    }
    let (status, body, ct) = http_get(url, None, Some("https://movie.douban.com/")).ok()?;
    let ext = match ct.as_deref().map(|t| t.split(';').next().unwrap_or("").trim()) {
        Some("image/jpeg") => "jpg",
        Some("image/png") => "png",
        Some("image/webp") => "webp",
        _ => return None,
    };
    if status != 200 || body.is_empty() || body.len() > 2 << 20 {
        return None;
    }
    let hex: String = Sha256::digest(&body).iter().map(|b| format!("{b:02x}")).collect();
    let name = format!("images/{hex}.{ext}");
    blob::put(&format!("public/{name}"), ct.as_deref().unwrap_or("image/jpeg"), &body).ok()?;
    Some(format!("/media/metadata-douban/{name}"))
}

/// 直连豆瓣：浏览器 UA（与可选 Cookie）请求页面；被拦截时提示在设置中填 Cookie
fn douban_get(c: &Cfg, url: &str, html: bool) -> R<(u16, Vec<u8>)> {
    douban_req(c, url, html).map(|(s, b, _)| (s, b))
}

/// 同 `douban_get`，另返回 Location（不跟随重定向）
fn douban_req(c: &Cfg, url: &str, html: bool) -> R<(u16, Vec<u8>, Option<String>)> {
    let mut headers = vec![
        http_client::Header { name: "user-agent".into(), value: UA.into() },
        http_client::Header {
            name: "accept".into(),
            value: if html { "text/html,application/xhtml+xml;q=0.9,*/*;q=0.8" } else { "application/json, text/javascript, */*;q=0.01" }.into(),
        },
        http_client::Header { name: "accept-language".into(), value: "zh-CN,zh;q=0.9".into() },
    ];
    if let Some(k) = &c.cookie {
        headers.push(http_client::Header { name: "cookie".into(), value: k.clone() });
    }
    let referer = if url.starts_with(BOOK) {
        "https://book.douban.com/"
    } else {
        "https://movie.douban.com/"
    };
    headers.push(http_client::Header {
        name: "referer".into(),
        value: referer.into(),
    });
    let r = http_client::send(&http_client::OutboundRequest {
        method: "GET".into(),
        url: url.into(),
        headers,
        body: None,
        timeout_ms: 10_000,
    })?;
    let loc = r
        .headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case("location"))
        .map(|h| h.value.clone());
    Ok((r.status, r.body, loc))
}

/// 条目页的状态：200 → true，404 → false，跳到验证页（sec.douban.com）等 → 被拦截
fn page_status(status: u16) -> R<bool> {
    match status {
        200 => Ok(true),
        404 => Ok(false),
        301 | 302 | 403 | 418 | 429 => Err(blocked()),
        s => Err(PluginError {
            retryable: s >= 500,
            ..err("metadata-douban.upstream_error", &format!("Douban returned {s}"))
        }),
    }
}

fn blocked() -> PluginError {
    PluginError {
        retryable: true,
        ..err("metadata-douban.blocked", "Douban rejected the request (rate limited); set a cookie in the plugin settings or retry later")
    }
}

const MOBILE_UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1";
const REXXAR: &str = "https://m.douban.com/rexxar/api/v2";

/// 移动端接口（要求 Referer 为 m.douban.com）；返回 (状态, 正文, Location)
fn rexxar_get(c: &Cfg, url: &str) -> R<(u16, Vec<u8>, Option<String>)> {
    let mut headers = vec![
        http_client::Header { name: "user-agent".into(), value: MOBILE_UA.into() },
        http_client::Header { name: "accept".into(), value: "application/json".into() },
        http_client::Header { name: "referer".into(), value: "https://m.douban.com/movie/".into() },
    ];
    if let Some(k) = &c.cookie {
        headers.push(http_client::Header { name: "cookie".into(), value: k.clone() });
    }
    let r = http_client::send(&http_client::OutboundRequest {
        method: "GET".into(),
        url: url.into(),
        headers,
        body: None,
        timeout_ms: 10_000,
    })?;
    let loc = r.headers.iter().find(|h| h.name.eq_ignore_ascii_case("location")).map(|h| h.value.clone());
    Ok((r.status, r.body, loc))
}

fn rexxar_json(c: &Cfg, url: &str) -> Value {
    match rexxar_get(c, url) {
        Ok((200, b, _)) => serde_json::from_slice(&b).unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

/// 移动端接口：`movie/<id>` 对剧集返回 301 到 `tv/<id>`。Err(None) 表示不可用，交给桌面页解析
fn via_rexxar(c: &Cfg, id: &str) -> Result<Option<Value>, Option<PluginError>> {
    let mut path = format!("{REXXAR}/movie/{id}");
    for _ in 0..2 {
        let (status, body, loc) = rexxar_get(c, &path).map_err(Some)?;
        match status {
            200 => {
                let v: Value = serde_json::from_slice(&body).map_err(|_| None)?;
                let credits = rexxar_json(c, &format!("{path}/credits"));
                let awards = rexxar_json(c, &format!("{path}/awards"));
                let kind = if path.contains("/tv/") { "tv" } else { "movie" };
                let imdb = match douban_get(c, &format!("https://www.douban.com/doubanapp//h5/{kind}/{id}/desc"), true) {
                    Ok((200, b)) => scrape::h5_imdb(&String::from_utf8_lossy(&b)),
                    _ => None,
                };
                return Ok(Some(scrape::from_rexxar(&v, &credits, &awards, imdb)));
            }
            301 | 302 => match loc.filter(|l| l.starts_with(REXXAR)) {
                Some(l) => path = l,
                None => return Err(None),
            },
            404 => return Ok(None),
            _ => return Err(None),
        }
    }
    Err(None)
}

fn direct(c: &Cfg, id: &str) -> R<Option<Value>> {
    match via_rexxar(c, id) {
        Ok(v) => return Ok(v),
        Err(Some(e)) if e.code != "kernel.capability_denied" => return Err(e),
        Err(_) => {}
    }
    let (status, body) = douban_get(c, &format!("{DOUBAN}/subject/{id}/"), true)?;
    if !page_status(status)? {
        return Ok(None);
    }
    let html = String::from_utf8_lossy(&body);
    let mut v = match scrape::subject(&html) {
        scrape::Page::Ok(v) => v,
        scrape::Page::NotFound => return Ok(None),
        scrape::Page::Blocked => return Err(blocked()),
    };
    // 获奖情况在单独页面，失败不影响主体
    if let Ok((200, b)) = douban_get(c, &format!("{DOUBAN}/subject/{id}/awards/"), true) {
        let a = scrape::awards(&String::from_utf8_lossy(&b));
        if !a.is_empty() {
            v["awards"] = json!(a);
        }
    }
    Ok(Some(v))
}

/// 图书：移动端接口 + h5 简介页（ISBN、原作名、装帧），不可用时解析桌面条目页
fn book_direct(c: &Cfg, id: &str) -> R<Option<Value>> {
    match rexxar_get(c, &format!("{REXXAR}/book/{id}")) {
        Ok((200, body, _)) => {
            if let Ok(v) = serde_json::from_slice::<Value>(&body) {
                let desc = match douban_get(
                    c,
                    &format!("https://www.douban.com/doubanapp//h5/book/{id}/desc"),
                    true,
                ) {
                    Ok((200, b)) => scrape::h5_table(&String::from_utf8_lossy(&b)),
                    _ => Vec::new(),
                };
                return Ok(Some(scrape::book_from_rexxar(&v, &desc)));
            }
        }
        Ok((404, _, _)) => return Ok(None),
        Err(e) if e.code != "kernel.capability_denied" => return Err(e),
        _ => {}
    }
    let (status, body) = douban_get(c, &format!("{BOOK}/subject/{id}/"), true)?;
    if !page_status(status)? {
        return Ok(None);
    }
    match scrape::book_page(&String::from_utf8_lossy(&body)) {
        scrape::Page::Ok(v) => Ok(Some(v)),
        scrape::Page::NotFound => Ok(None),
        scrape::Page::Blocked => Err(blocked()),
    }
}

/// ISBN → 豆瓣图书 ID：直连时 `book.douban.com/isbn/<ISBN>/` 重定向到条目页；代理为 `/isbn/<ISBN>`（返回图书字段与 `id`）
fn fetch_book(c: &Cfg, key: &str) -> R<Option<(String, Value)>> {
    let (num, isbn) = match key.strip_prefix("isbn:") {
        Some(isbn) => (None, Some(isbn)),
        None => (key.strip_prefix("book:"), None),
    };
    match c.mode.as_str() {
        "direct" => {
            let id = match (num, isbn) {
                (Some(n), _) => n.to_owned(),
                (None, Some(i)) => {
                    let (status, _, loc) = douban_req(c, &format!("{BOOK}/isbn/{i}/"), true)?;
                    let found = loc
                        .filter(|_| matches!(status, 301 | 302))
                        .and_then(|l| mapper::parse_id(&l))
                        .and_then(|k| k.strip_prefix("book:").map(str::to_owned));
                    match found {
                        Some(n) => n,
                        // 不认识的 ISBN：404 或停留在搜索页
                        None if matches!(status, 200 | 404) => return Ok(None),
                        None => return Err(page_status(status).err().unwrap_or_else(blocked)),
                    }
                }
                (None, None) => return Ok(None),
            };
            Ok(book_direct(c, &id)?.map(|v| (id, v)))
        }
        "proxy" => {
            let url = match (num, isbn) {
                (Some(n), _) => format!("{}/subject/{n}?type=book", c.base),
                (None, Some(i)) => format!("{}/isbn/{i}", c.base),
                (None, None) => return Ok(None),
            };
            let Some(mut v) = upstream(c, &url)? else {
                return Ok(None);
            };
            let id = num
                .map(str::to_owned)
                .or_else(|| v["id"].as_str().map(str::to_owned))
                .unwrap_or_default();
            if v["poster"].is_null() {
                v["poster"] = v["poster_url"].clone();
            }
            Ok(Some((id, v)))
        }
        _ => Err(err(
            "metadata-douban.unsupported",
            "the PT-Gen upstream does not provide Douban books; use direct or proxy mode",
        )),
    }
}

fn fetch(c: &Cfg, id: &str) -> R<Option<Value>> {
    if c.base.is_empty() {
        return Err(err("metadata-douban.not_configured", "no Douban proxy or PT-Gen endpoint is configured"));
    }
    if id.starts_with("book:") || id.starts_with("isbn:") {
        let Some((num, v)) = fetch_book(c, id)? else {
            return Ok(None);
        };
        let mut data = mapper::from_book(&num, &v);
        if data["ids"]["isbn"].is_null()
            && let Some(i) = id.strip_prefix("isbn:")
        {
            data["ids"]["isbn"] = json!(i);
        }
        if let Some(p) = v["poster"].as_str().and_then(|u| rehost(c, u)) {
            data["images"]["poster"] = json!(p);
        }
        return Ok(Some(data));
    }
    let (mut data, poster) = if c.mode == "direct" {
        let Some(v) = direct(c, id)? else { return Ok(None) };
        (mapper::from_ptgen(id, &v), v["poster"].as_str().map(str::to_owned))
    } else if c.mode == "ptgen" {
        let url = format!("{}/?url={}", c.base, enc(&format!("https://movie.douban.com/subject/{id}/")));
        let Some(v) = upstream(c, &url)? else { return Ok(None) };
        if v["success"] == false {
            let msg = v["error"].as_str().unwrap_or("");
            return if msg.contains("not exist") || msg.contains("不存在") || msg.contains("404") {
                Ok(None)
            } else {
                Err(err("metadata-douban.upstream_error", "PT-Gen returned an error"))
            };
        }
        (mapper::from_ptgen(id, &v), v["poster"].as_str().map(str::to_owned))
    } else {
        let Some(v) = upstream(c, &format!("{}/subject/{id}", c.base))? else { return Ok(None) };
        (mapper::from_proxy(id, &v), v["poster_url"].as_str().map(str::to_owned))
    };
    if let Some(p) = poster.as_deref().and_then(|u| rehost(c, u)) {
        data["images"]["poster"] = json!(p);
    }
    Ok(Some(data))
}

/// 返回 (数据, 是否过期降级)；`refresh` 为后台刷新：忽略缓存新鲜度，不更新 `last_used_at`
fn lookup(raw: &str, refresh: bool) -> R<Option<(String, Value, bool)>> {
    if !enabled() {
        return Err(err("metadata-douban.not_enabled", "the Douban source is not enabled"));
    }
    let id = mapper::parse_id(raw).ok_or_else(|| err("metadata-douban.id_invalid", "expected a Douban subject ID or link"))?;
    let c = cfg();
    // 负缓存放在 Redis：“不存在”会以错误返回，工作单元回滚后 PG 中的记录会丢失
    let nf_key = format!("nf:{id}");
    if matches!(redis::call(&redis::Command { name: "EXISTS".into(), args: vec![nf_key.as_bytes().to_vec()] }), Ok(redis::RedisReply::Int(1))) {
        return Ok(None);
    }
    let cached = sql::query(
        "SELECT data::text, not_found, fetched_at > now() - make_interval(days => CASE WHEN not_found THEN 1 ELSE $2::int END)
           FROM cache WHERE external_id = $1 AND locale = 'zh-CN'",
        &[SqlValue::Text(id.clone()), SqlValue::Int(c.cache_days)],
    )?;
    let hit = cached.rows.first().map(|r| (r.text(0), r.bool(1).unwrap_or(false), r.bool(2).unwrap_or(false)));
    if let Some((data, not_found, true)) = &hit
        && !refresh
    {
        if *not_found {
            return Ok(None);
        }
        sql::execute("UPDATE cache SET last_used_at = now() WHERE external_id = $1 AND locale = 'zh-CN'", &[SqlValue::Text(id.clone())])?;
        return Ok(Some((id, serde_json::from_str(data.as_deref().unwrap_or("null")).unwrap_or(Value::Null), false)));
    }
    let fetched = match guarded(|| fetch(&c, &id)) {
        Ok(f) => f,
        Err(e) => {
            if let Some((Some(data), false, _)) = hit {
                return Ok(Some((id, serde_json::from_str(&data).unwrap_or(Value::Null), true)));
            }
            return Err(e);
        }
    };
    match fetched {
        None => {
            let _ = redis::call(&redis::Command {
                name: "SET".into(),
                args: vec![nf_key.into_bytes(), b"1".to_vec(), b"EX".to_vec(), b"86400".to_vec()],
            });
            Ok(None)
        }
        Some(data) => {
            let hash: String = Sha256::digest(data.to_string().as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
            let old = sql::query("SELECT content_hash FROM cache WHERE external_id = $1 AND locale = 'zh-CN'", &[SqlValue::Text(id.clone())])?;
            let changed = old.rows.first().is_none_or(|r| r.text(0).as_deref() != Some(hash.as_str()));
            sql::execute(
                "INSERT INTO cache (external_id, locale, data, not_found, content_hash, fetched_at) VALUES ($1, 'zh-CN', $2, false, $3, now())
                 ON CONFLICT (external_id, locale) DO UPDATE SET data = EXCLUDED.data, not_found = false, content_hash = EXCLUDED.content_hash,
                        fetched_at = now(), last_used_at = CASE WHEN $4 THEN cache.last_used_at ELSE now() END",
                &[SqlValue::Text(id.clone()), SqlValue::Json(data.to_string()), SqlValue::Text(hash), SqlValue::Boolean(refresh)],
            )?;
            if changed {
                ctx::publish(
                    "metadata.fetched",
                    &format!("metadata:douban:{id}"),
                    &json!({ "source": "douban", "external_id": id, "type_id": data["kind"], "group_id": null }),
                )?;
            }
            Ok(Some((id, data, false)))
        }
    }
}

fn search(query: &str, type_id: &str, limit: u32) -> R<Vec<Value>> {
    if !enabled() {
        return Err(err("metadata-douban.not_enabled", "the Douban source is not enabled"));
    }
    let c = cfg();
    let q = query.trim();
    if q.is_empty() || q.chars().count() > 100 {
        return Err(err("kernel.validation_failed", "query must be 1..=100 characters"));
    }
    guarded(|| search_upstream(&c, q, type_id, limit))
}

fn search_upstream(c: &Cfg, q: &str, type_id: &str, limit: u32) -> R<Vec<Value>> {
    if c.mode == "direct" {
        let n = usize::try_from(limit.clamp(1, 20)).unwrap_or(10);
        if type_id == "book" {
            if let Ok((200, body, _)) = rexxar_get(c, &format!("{REXXAR}/search/book?q={}&count={n}", enc(q)))
            {
                let items = scrape::rexxar_book_search(&body);
                if !items.is_empty() {
                    return Ok(items.into_iter().take(n).collect());
                }
            }
            let (status, body) = douban_get(c, &format!("{BOOK}/j/subject_suggest?q={}", enc(q)), false)?;
            page_status(status)?;
            return Ok(scrape::suggest(&body)
                .into_iter()
                .filter(|x| x["kind"] == "book")
                .take(n)
                .collect());
        }
        if let Ok((200, body, _)) = rexxar_get(c, &format!("{REXXAR}/search/movie?q={}&count={n}", enc(q))) {
            let items = scrape::rexxar_search(&body);
            if !items.is_empty() {
                return Ok(items.into_iter().take(n).collect());
            }
        }
        let (status, body) = douban_get(c, &format!("{DOUBAN}/j/subject_suggest?q={}", enc(q)), false)?;
        return match status {
            200 => Ok(scrape::suggest(&body).into_iter().take(n).collect()),
            301 | 302 | 403 | 418 | 429 => Err(blocked()),
            s => Err(PluginError { retryable: s >= 500, ..err("metadata-douban.upstream_error", &format!("Douban returned {s}")) }),
        };
    }
    if c.mode == "ptgen" || c.base.is_empty() {
        return Err(err("metadata-douban.search_unsupported", "the configured upstream does not support search"));
    }
    let kind = match type_id {
        "tv" | "anime" => "tv",
        "book" => "book",
        _ => "movie",
    };
    let url = format!("{}/search?q={}&type={kind}&limit={}", c.base, enc(q), limit.clamp(1, 20));
    match upstream(c, &url)? {
        None => Err(err("metadata-douban.search_unsupported", "the proxy does not support search")),
        Some(v) => Ok(v["items"]
            .as_array()
            .map(|a| {
                a.iter()
                    .take(usize::try_from(limit.clamp(1, 20)).unwrap_or(10))
                    .map(|x| {
                        let id = if x["type"] == "book" { json!(format!("book:{}", x["id"].as_str().unwrap_or(""))) } else { x["id"].clone() };
                        json!({ "external_id": id, "title": x["title"], "year": x["year"], "kind": x["type"] })
                    })
                    .collect()
            })
            .unwrap_or_default()),
    }
}

impl service_provider::Guest for Douban {
    fn call(service: String, method: String, args: String) -> R<String> {
        if service != "ctx.metadata" {
            return Err(err("kernel.not_found", "unknown service"));
        }
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        let out = match method.as_str() {
            "info" => json!({ "source": "douban", "type_ids": ["movie", "tv", "generic"],
                              "id_pattern": "^\\d{1,12}$|^book:\\d{1,12}$|^isbn:[0-9Xx-]{10,17}$|^97[89]\\d{10}$|douban\\.com/subject/\\d+", "enabled": enabled() }),
            "lookup" => match lookup(a["external_id"].as_str().unwrap_or(""), false)? {
                Some((id, data, stale)) => json!({ "source": "douban", "external_id": id, "data": data, "stale": stale }),
                None => Value::Null,
            },
            "search" => json!(search(
                a["query"].as_str().unwrap_or(""),
                a["type_id"].as_str().unwrap_or("movie"),
                u32::try_from(a["limit"].as_u64().unwrap_or(10)).unwrap_or(10)
            )?),
            other => return Err(err("kernel.not_found", &format!("ctx.metadata has no method `{other}`"))),
        };
        Ok(out.to_string())
    }
}

impl http_handler::Guest for Douban {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let reply = match req.route() {
            "lookup" => match lookup(req.query("id").unwrap_or(""), false)? {
                Some((id, data, stale)) => http::Reply::ok(&json!({ "external_id": id, "data": data, "stale": stale })),
                None => return Err(err("metadata-douban.not_found", "subject not found")),
            },
            "search" => http::Reply::ok(&json!({ "items": search(req.query("q").unwrap_or(""), req.query("type").unwrap_or("movie"), 10)? })),
            "ptgen" => {
                let Some((id, data, stale)) = lookup(req.query("id").unwrap_or(""), false)? else {
                    return Err(err("metadata-douban.not_found", "subject not found"));
                };
                // markup：bbcode（默认）/ markdown / text（纯文本，不含海报图片）
                let markup = match req.query("markup") {
                    Some("markdown") => "markdown",
                    Some("text") => "text",
                    _ => "bbcode",
                };
                let poster = if markup == "text" { None } else { data["images"]["poster"].as_str() };
                let text = mapper::intro(&data, poster, markup == "markdown");
                http::Reply::ok(&json!({ "external_id": id, "format": text, "markup": markup,
                                         "title": data["titles"]["localized"]["zh-CN"], "imdb": data["ids"]["imdb"], "stale": stale }))
            }
            "status" => {
                let c = cfg();
                http::Reply::ok(&json!({ "enabled": enabled(), "mode": c.mode, "search": c.mode != "ptgen" }))
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

/// 过期且近 90 天用过的条目逐个重新获取；未启用、熔断打开或超出时长时停止，下次运行继续
fn refresh_stale() -> R<()> {
    if !enabled() {
        return Ok(());
    }
    let started = time::now();
    let rows = sql::query(
        "SELECT external_id FROM cache
          WHERE NOT not_found AND locale = 'zh-CN' AND last_used_at > now() - interval '90 days'
            AND fetched_at < now() - make_interval(days => $1::int)
          ORDER BY last_used_at DESC LIMIT $2",
        &[SqlValue::Int(cfg().cache_days), SqlValue::Int(REFRESH_BATCH)],
    )?;
    let (mut ok, mut failed) = (0u32, 0u32);
    for r in &rows.rows {
        if time::now() - started > REFRESH_BUDGET_MS || UPSTREAM.is_open() {
            break;
        }
        match lookup(r.text(0).as_deref().unwrap_or(""), true) {
            Ok(_) => ok += 1,
            Err(e) if e.code == "metadata-douban.not_configured" => return Ok(()),
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

impl on_cron::Guest for Douban {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            "refresh_stale" => refresh_stale(),
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

export!(Douban);
