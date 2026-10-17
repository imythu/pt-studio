//! 端到端：豆瓣元数据（metadata-douban，WASM）—— 未确认风险时拒绝、代理模式查询 / 海报转存 / 缓存 / 负缓存 / 搜索、
//! PT-Gen 版式简介、PT-Gen 兼容端点模式。上游为本地模拟服务。
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

use common::{call, hv, register_login, reset};

/// 模拟上游：代理接口（/subject/{id}、/search）、PT-Gen（/ptgen/?url=）与海报（/img/p.jpg）
async fn mock(hits: Arc<AtomicUsize>) -> String {
    let jpeg: Vec<u8> = [&[0xFF, 0xD8, 0xFF, 0xE0][..], &[0u8; 64], &[0xFF, 0xD9]].concat();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let poster = format!("{base}/img/p.jpg");
    let h1 = hits.clone();
    let p1 = poster.clone();
    let app = axum::Router::new()
        .route(
            "/subject/{id}",
            axum::routing::get(move |axum::extract::Path(id): axum::extract::Path<String>| {
                let hits = h1.clone();
                let poster = p1.clone();
                async move {
                    hits.fetch_add(1, Ordering::SeqCst);
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

    // 未确认风险：拒绝
    let r = get("/v1/lookup?id=1292052".into()).await;
    assert_eq!((r.status, r.body["code"].clone()), (StatusCode::SERVICE_UNAVAILABLE, json!("metadata-douban.not_enabled")), "{:?}", r.body);
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    let r = call(&app, "POST", "/api/p/metadata-douban/v1/admin/enable", Some(json!({ "ack": true, "statement_version": 1 })), &hv(&user.headers())).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = call(&app, "POST", "/api/p/metadata-douban/v1/admin/enable", Some(json!({ "ack": true, "statement_version": 1 })), &hv(&admin.headers())).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);

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
    drop(kernel);

    // PT-Gen 兼容端点模式（启用状态保留在插件 KV）
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
    let _: &Value = &r.body;
}
