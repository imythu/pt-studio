//! 端到端：豆瓣元数据（metadata-douban，WASM）—— 未启用时拒绝、插件设置中启用（密钥字段不回显 / 不进审计）、代理模式查询 / 海报转存 / 缓存 / 负缓存 / 搜索、电影 Group 用豆瓣补全、
//! PT-Gen 版式简介、熔断、refresh_stale、PT-Gen 兼容端点模式。上游为本地模拟服务。
//!
//! 运行：`cargo test -p pt-server --test e2e_douban -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::{extract::Query, http::StatusCode, response::IntoResponse};
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};

use sqlx::{Connection, PgConnection};

use common::{call, env, hv, register_login, reset};

async fn db() -> PgConnection {
    PgConnection::connect(&env("PT_TEST_SUPERUSER_URL", "postgres://postgres@127.0.0.1:55432/pt_test")).await.unwrap()
}

/// 模拟上游：代理接口（/subject/{id}、/search）、PT-Gen（/ptgen/?url=）与海报（/img/p.jpg）
async fn mock(hits: Arc<AtomicUsize>) -> String {
    let jpeg: Vec<u8> = [&[0xFF, 0xD8, 0xFF, 0xE0][..], &[0u8; 64], &[0xFF, 0xD9]].concat();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let poster = format!("{base}/img/p.jpg");
    let h1 = hits.clone();
    let p1 = poster.clone();
    let p2 = poster.clone();
    let app = axum::Router::new()
        .route(
            "/subject/{id}",
            axum::routing::get(move |axum::extract::Path(id): axum::extract::Path<String>| {
                let hits = h1.clone();
                let poster = p1.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
                    // 500：上游故障（熔断）
                    if id == "500" {
                        return StatusCode::SERVICE_UNAVAILABLE.into_response();
                    }
                    // 图书（`?type=book`，代理返回图书字段）
                    if id == "1000001" {
                        return axum::Json(book(&poster)).into_response();
                    }
                    if id != "1292052" {
                        return StatusCode::NOT_FOUND.into_response();
                    }
                    axum::Json(json!({ "id": "1292052", "type": "movie", "title": "肖申克的救赎", "original_title": "The Shawshank Redemption",
                        "aka": ["刺激1995"], "year": 1994, "pubdate": "1994-09-10", "imdb": "tt0111161", "genres": ["剧情", "犯罪"],
                        "countries": ["美国"], "languages": ["英语"], "directors": ["弗兰克·德拉邦特"], "casts": ["蒂姆·罗宾斯", "摩根·弗里曼"],
                        "durations_min": 142, "rating": { "value": 9.7, "count": 3000000 }, "poster_url": poster, "summary": "二十世纪四十年代末……" }))
                    .into_response()
                }
            }),
        )
        .route(
            "/isbn/{isbn}",
            axum::routing::get(move |axum::extract::Path(isbn): axum::extract::Path<String>| {
                let poster = p2.clone();
                async move {
                    if isbn != "9787111111115" {
                        return StatusCode::NOT_FOUND.into_response();
                    }
                    axum::Json(book(&poster)).into_response()
                }
            }),
        )
        .route(
            "/search",
            axum::routing::get(|Query(q): Query<std::collections::HashMap<String, String>>| async move {
                axum::Json(json!({ "items": [{ "id": "1292052", "type": "movie", "title": format!("{} 结果", q.get("q").cloned().unwrap_or_default()), "year": 1994 }] }))
            }),
        )
        .route(
            "/ptgen/",
            axum::routing::get(|Query(q): Query<std::collections::HashMap<String, String>>| async move {
                if !q.get("url").is_some_and(|u| u.contains("/subject/26794435/")) {
                    return axum::Json(json!({ "success": false, "error": "subject not exist" }));
                }
                axum::Json(json!({ "success": true, "chinese_title": "哪吒之魔童降世", "foreign_title": "", "aka": ["哪吒"], "year": "2019",
                    "region": ["中国大陆"], "genre": ["动画", "奇幻"], "language": ["汉语普通话"], "playdate": ["2019-07-26(中国大陆)"],
                    "douban_rating_average": "8.4", "douban_votes": "1900000", "episodes": "", "duration": "110分钟",
                    "director": [{ "name": "饺子" }], "cast": [{ "name": "吕艳婷" }], "introduction": "天地灵气孕育出一颗能量巨大的混元珠……" }))
            }),
        )
        .route(
            "/img/p.jpg",
            axum::routing::get(move || {
                let b = jpeg.clone();
                async move { ([("content-type", "image/jpeg")], b) }
            }),
        );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

fn book(poster: &str) -> Value {
    json!({ "id": "1000001", "type": "book", "title": "示例之书", "original_title": "An Example Book", "authors": ["[英] 某作者"],
        "translators": ["某译者"], "publisher": "示例出版社", "pubdate": "2011-6", "pages": 360, "isbn": "9787111111115",
        "rating": { "value": 9.1, "count": 1234 }, "poster_url": poster, "summary": "第一段。", "author_intro": "某作者。" })
}

fn config(dir: &std::path::Path, extra: &str) -> SiteConfig {
    let mut t = common::config_text();
    t.push_str(&format!("\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n\n[plugin.metadata-douban]\n{extra}", dir.display()));
    SiteConfig::parse(&t).unwrap()
}

#[tokio::test]
async fn douban_proxy_and_ptgen() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let hits = Arc::new(AtomicUsize::new(0));
    let base = mock(hits.clone()).await;
    let dir = common::package_wasm_plugins();
    let kernel = Kernel::start(
        config(&dir, &format!("mode = \"proxy\"\nproxy_base_url = \"{base}\"\nimage_hosts = [\"127.0.0.1\"]\n")),
        pt_server::native_plugins(),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let admin = register_login(&app, "douban_admin").await;
    let user = register_login(&app, "douban_user").await;
    common::grant_role(admin.id, "admin").await;
    let get = |path: String| {
        let app = app.clone();
        let h = user.headers();
        async move { call(&app, "GET", &format!("/api/p/metadata-douban{path}"), None, &hv(&h)).await }
    };

    // 未启用：拒绝；状态接口告诉前端是否显示“生成简介”
    let r = get("/v1/lookup?id=1292052".into()).await;
    assert_eq!((r.status, r.body["code"].clone()), (StatusCode::SERVICE_UNAVAILABLE, json!("metadata-douban.not_enabled")), "{:?}", r.body);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    assert_eq!(get("/v1/status".into()).await.body["enabled"], false);
    // 启用与配置都在插件设置中（只有管理员能改）；密钥字段不回显，提交占位符时保留原值
    let path = "/api/kernel/v1/admin/plugins/metadata-douban/settings";
    let body = json!({ "settings": { "enabled": true, "proxy_token": "tok-secret" }, "version": 0 });
    assert_eq!(call(&app, "PUT", path, Some(body.clone()), &hv(&user.headers())).await.status, StatusCode::FORBIDDEN);
    let r = call(&app, "PUT", path, Some(body), &hv(&admin.headers())).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(&app, "GET", path, None, &hv(&admin.headers())).await;
    assert_eq!((r.body["settings"]["proxy_token"].clone(), r.body["schema"]["properties"]["cookie"]["x-pt-secret"].clone()), (json!("********"), json!(true)));
    let r = call(&app, "PUT", path, Some(json!({ "settings": r.body["settings"], "version": r.body["version"] })), &hv(&admin.headers())).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let (stored,): (Value,) = sqlx::query_as("SELECT settings FROM kernel.plugin_settings WHERE plugin_id = 'metadata-douban'")
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert_eq!(stored["proxy_token"], "tok-secret");
    let (audit,): (String,) = sqlx::query_as("SELECT details::text FROM kernel.audit_log WHERE action = 'kernel.settings_update' AND target = 'plugin:metadata-douban' ORDER BY id DESC LIMIT 1")
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert!(!audit.contains("tok-secret"), "{audit}");
    assert_eq!(get("/v1/status".into()).await.body["enabled"], true);

    // 查询（链接形式）：归一化、海报转存到站内
    let r = get(format!("/v1/lookup?id={}", "https%3A%2F%2Fmovie.douban.com%2Fsubject%2F1292052%2F")).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let d = &r.body["data"];
    assert_eq!((d["ids"]["imdb"].clone(), d["genres"].clone(), d["countries"].clone()), (json!("tt0111161"), json!(["drama", "crime"]), json!(["US"])));
    let poster = d["images"]["poster"].as_str().unwrap().to_owned();
    assert!(poster.starts_with("/media/metadata-douban/images/"), "{poster}");
    let img = call(&app, "GET", &poster, None, &[]).await;
    assert_eq!(img.status, StatusCode::OK);
    assert_eq!(&img.bytes[..2], &[0xFF, 0xD8]);
    // 缓存命中：上游不再被请求
    let before = hits.load(Ordering::SeqCst);
    assert_eq!(get("/v1/lookup?id=1292052".into()).await.status, StatusCode::OK);
    assert_eq!(hits.load(Ordering::SeqCst), before);
    // 不存在 + 负缓存
    assert_eq!(get("/v1/lookup?id=999".into()).await.status, StatusCode::NOT_FOUND);
    let before = hits.load(Ordering::SeqCst);
    assert_eq!(get("/v1/lookup?id=999".into()).await.status, StatusCode::NOT_FOUND);
    assert_eq!(hits.load(Ordering::SeqCst), before, "negative cache");
    assert_eq!(get("/v1/lookup?id=abc".into()).await.body["code"], "metadata-douban.id_invalid");
    // 搜索
    let r = get("/v1/search?q=%E8%82%96%E7%94%B3%E5%85%8B".into()).await;
    assert_eq!(r.body["items"][0]["title"], "肖申克 结果", "{:?}", r.body);
    // PT-Gen 版式（BBCode 带站内海报；纯文本不带）
    let r = get("/v1/ptgen?id=1292052".into()).await;
    let f = r.body["format"].as_str().unwrap();
    assert!(f.starts_with(&format!("[img]{poster}[/img]")), "{f}");
    assert!(f.contains("◎片　　名　The Shawshank Redemption") && f.contains("◎豆瓣评分　9.7/10 from 3000000 users"), "{f}");
    let r = get("/v1/ptgen?id=1292052&markup=text".into()).await;
    assert!(!r.body["format"].as_str().unwrap().contains("[img]"));

    // 资源类型自动补全：只有豆瓣编号的电影 Group 用豆瓣条目补全
    let file = pt_plugin_torrent::fixtures::sample_v1("Shawshank", &[("s.mkv", 600 << 20)], true);
    let boundary = "b0undary-db";
    let meta = json!({ "type_id": "movie", "title": "The.Shawshank.Redemption.1994.1080p.BluRay.x264-GRP", "fields": { "douban_id": "1292052" } });
    let mut body = Vec::new();
    body.extend(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"meta\"\r\n\r\n{meta}\r\n").as_bytes(),
    );
    body.extend(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"torrent\"; filename=\"t.torrent\"\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend(&file);
    body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    let h = user.headers();
    let r = common::send(&app, "POST", "/api/p/torrent/v1/torrents", &hv(&h), |b| {
        b.header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(axum::body::Body::from(body.clone()))
        .unwrap()
    })
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let gid = r.body["group_id"].as_i64().unwrap();
    let mut g = Value::Null;
    for _ in 0..200 {
        g = call(
            &app,
            "GET",
            &format!("/api/p/torrent/v1/groups/{gid}"),
            None,
            &hv(&user.headers()),
        )
        .await
        .body;
        if g["fields"]["poster"].is_string() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(
        (g["fields"]["poster"].as_str(), g["fields"]["rating"].as_f64()),
        (Some(poster.as_str()), Some(9.7)),
        "{g}"
    );
    assert_eq!(
        (
            g["fields"]["imdb_id"].as_str(),
            g["fields"]["runtime_min"].as_i64()
        ),
        (Some("tt0111161"), Some(142)),
        "{g}"
    );

    // 图书：book: 前缀 / 图书链接 / ISBN
    let r = get(format!(
        "/v1/lookup?id={}",
        "https%3A%2F%2Fbook.douban.com%2Fsubject%2F1000001%2F"
    ))
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["external_id"], "book:1000001");
    let d = &r.body["data"];
    assert_eq!(
        (d["kind"].clone(), d["ids"].clone(), d["year"].clone()),
        (
            json!("book"),
            json!({ "douban": "1000001", "isbn": "9787111111115" }),
            json!(2011)
        )
    );
    assert_eq!(d["people"][0], json!({ "name": "[英] 某作者", "role": "writer" }));
    assert_eq!(d["images"]["poster"], json!(poster));
    let r = get("/v1/lookup?id=isbn:9787111111115".into()).await;
    assert_eq!(r.body["data"]["ids"]["douban"], "1000001", "{:?}", r.body);
    assert_eq!(
        get("/v1/lookup?id=isbn:9787111111116".into()).await.body["code"],
        "metadata-douban.id_invalid"
    );
    let f = get("/v1/ptgen?id=book:1000001".into()).await.body["format"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        f.contains("◎书　　名　示例之书")
            && f.contains("◎ＩＳＢＮ　9787111111115")
            && f.contains("◎作者简介"),
        "{f}"
    );

    // 通用类型的电子书：只填 ISBN，用豆瓣图书补全作品
    let file = pt_plugin_torrent::fixtures::sample_v1("Example Book", &[("b.epub", 5 << 20)], true);
    let meta = json!({ "type_id": "generic", "title": "Example Book", "fields": { "kind": "ebook", "title": "示例之书", "isbn": "978-7-111-11111-5" } });
    let mut body = Vec::new();
    body.extend(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"meta\"\r\n\r\n{meta}\r\n").as_bytes(),
    );
    body.extend(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"torrent\"; filename=\"b.torrent\"\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend(&file);
    body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    let r = common::send(&app, "POST", "/api/p/torrent/v1/torrents", &hv(&h), |b| {
        b.header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(axum::body::Body::from(body.clone()))
        .unwrap()
    })
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let gid = r.body["group_id"].as_i64().unwrap();
    for _ in 0..200 {
        g = call(
            &app,
            "GET",
            &format!("/api/p/torrent/v1/groups/{gid}"),
            None,
            &hv(&user.headers()),
        )
        .await
        .body;
        if g["fields"]["poster"].is_string() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let gf = &g["fields"];
    assert_eq!(
        (
            gf["poster"].as_str(),
            gf["rating"].as_f64(),
            gf["douban_id"].as_str()
        ),
        (Some(poster.as_str()), Some(9.1), Some("1000001")),
        "{g}"
    );
    assert_eq!(
        (
            gf["authors"].as_str(),
            gf["publisher"].as_str(),
            gf["overview"].as_str()
        ),
        (Some("[英] 某作者"), Some("示例出版社"), Some("第一段。")),
        "{g}"
    );

    // 熔断：连续 5 次上游失败后不再请求上游
    for _ in 0..5 {
        assert_eq!(
            get("/v1/lookup?id=500".into()).await.body["code"],
            "metadata-douban.upstream_error"
        );
    }
    let before = hits.load(Ordering::SeqCst);
    assert_eq!(
        get("/v1/lookup?id=500".into()).await.body["code"],
        "metadata-douban.upstream_error"
    );
    assert_eq!(hits.load(Ordering::SeqCst), before, "circuit must be open");
    // refresh_stale：熔断打开时不动；恢复后重新获取过期条目，不改变 last_used_at
    let mut pg = db().await;
    sqlx::query(
        "UPDATE metadata_douban.cache SET fetched_at = now() - interval '40 days', last_used_at = now() - interval '1 day'
          WHERE external_id = '1292052'",
    )
    .execute(&mut pg)
    .await
    .unwrap();
    kernel
        .run_job_now("metadata-douban", "refresh_stale", json!({}))
        .await
        .unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), before);
    let mut rc = redis::Client::open(env("PT_TEST_REDIS_URL", "redis://127.0.0.1:56379/15"))
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap();
    let keys: Vec<String> = redis::cmd("KEYS")
        .arg("p:metadata-douban:cb:*")
        .query_async(&mut rc)
        .await
        .unwrap();
    for k in keys {
        let _: i64 = redis::cmd("DEL").arg(k).query_async(&mut rc).await.unwrap();
    }
    kernel
        .run_job_now("metadata-douban", "refresh_stale", json!({}))
        .await
        .unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), before + 1);
    let (fresh, used_recently): (bool, bool) = sqlx::query_as(
        "SELECT fetched_at > now() - interval '1 minute', last_used_at > now() - interval '1 hour'
           FROM metadata_douban.cache WHERE external_id = '1292052'",
    )
    .fetch_one(&mut pg)
    .await
    .unwrap();
    assert_eq!((fresh, used_recently), (true, false));
    drop(kernel);

    // PT-Gen 兼容端点模式（启用状态保留在插件设置）
    let kernel = Kernel::start(config(&dir, &format!("mode = \"ptgen\"\nptgen_url = \"{base}/ptgen\"\n")), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let r = call(&app, "GET", "/api/p/metadata-douban/v1/ptgen?id=26794435", None, &hv(&user.headers())).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let f = r.body["format"].as_str().unwrap();
    assert!(f.contains("◎译　　名　哪吒") && f.contains("◎片　　长　110 分钟") && f.contains("◎产　　地　中国大陆"), "{f}");
    let r = call(&app, "GET", "/api/p/metadata-douban/v1/lookup?id=1", None, &hv(&user.headers())).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{:?}", r.body);
    let r = call(&app, "GET", "/api/p/metadata-douban/v1/search?q=x", None, &hv(&user.headers())).await;
    assert_eq!(r.body["code"], "metadata-douban.search_unsupported");
    let r = call(
        &app,
        "GET",
        "/api/p/metadata-douban/v1/lookup?id=book:1000002",
        None,
        &hv(&user.headers()),
    )
    .await;
    assert_eq!(
        (r.status, r.body["code"].clone()),
        (StatusCode::NOT_IMPLEMENTED, json!("metadata-douban.unsupported"))
    );
    let _: &Value = &r.body;
}
