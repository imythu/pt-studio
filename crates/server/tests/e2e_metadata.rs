//! 端到端：元数据（metadata-tmdb，WASM）—— 内核出站 HTTP（allowlist、config 主机、allow_private）、ctx.metadata 多实现路由、
//! lookup（imdb 换算、归一化、缓存）、search、海报转存与 /media 访问、type-movie 新 Group 自动补全元数据。
//! 上游为本地模拟的 TMDB。

#![allow(clippy::unwrap_used)]

mod common;

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{call, env, hv, register_login, reset, send};

fn config(dir: &std::path::Path, base: &str) -> SiteConfig {
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n\n[plugin.metadata-tmdb]\napi_key = \"testkey\"\napi_base_url = \"{base}\"\nimage_base_url = \"{base}\"\n",
        dir.display()
    ));
    SiteConfig::parse(&t).unwrap()
}

#[tokio::test]
async fn metadata_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let dir = common::package_wasm_plugins();
    let hits = Arc::new(AtomicUsize::new(0));
    let base = common::mock_tmdb(hits.clone()).await;
    reset().await;
    let kernel = Kernel::start(config(&dir, &base), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let mut db = PgConnection::connect(&env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap();
    let u = register_login(&app, "md_user").await;

    // lookup：imdb → movie:603，归一化 + 海报转存
    let r = call(
        &app,
        "GET",
        "/api/p/metadata-tmdb/v1/lookup?id=imdb:tt0133093&locale=zh-CN",
        None,
        &hv(&u.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let d = &r.body["data"];
    assert_eq!(r.body["external_id"], "movie:603");
    assert_eq!(d["titles"]["localized"]["zh-CN"], "黑客帝国");
    assert_eq!(d["ids"], json!({ "tmdb": 603, "imdb": "tt0133093" }));
    assert_eq!(d["genres"], json!(["action", "scifi"]));
    assert_eq!(d["attribution"]["source"], "tmdb");
    let poster = d["images"]["poster"].as_str().unwrap().to_owned();
    assert!(
        poster.starts_with("/media/metadata-tmdb/images/") && poster.ends_with(".jpg"),
        "{poster}"
    );
    let img = call(&app, "GET", &poster, None, &[]).await;
    assert_eq!(
        (img.status, img.headers["content-type"].to_str().unwrap()),
        (StatusCode::OK, "image/jpeg")
    );
    assert_eq!(img.bytes, common::JPEG);
    assert_eq!(
        call(&app, "GET", "/media/metadata-tmdb/../../etc/passwd", None, &[])
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // 缓存：再次 lookup 不请求上游
    let before = hits.load(Ordering::SeqCst);
    let r = call(
        &app,
        "GET",
        "/api/p/metadata-tmdb/v1/lookup?id=imdb:tt0133093&locale=zh-CN",
        None,
        &hv(&u.headers()),
    )
    .await;
    assert_eq!(r.body["data"]["ids"]["tmdb"], 603);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        before,
        "second lookup must hit the cache"
    );
    // 不存在 / 格式错误
    let r = call(
        &app,
        "GET",
        "/api/p/metadata-tmdb/v1/lookup?id=movie:999",
        None,
        &hv(&u.headers()),
    )
    .await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("metadata-tmdb.not_found"))
    );
    let r = call(
        &app,
        "GET",
        "/api/p/metadata-tmdb/v1/lookup?id=xyz",
        None,
        &hv(&u.headers()),
    )
    .await;
    assert_eq!(r.body["code"], "metadata-tmdb.id_invalid");
    // search
    let r = call(
        &app,
        "GET",
        "/api/p/metadata-tmdb/v1/search?q=matrix&type=movie",
        None,
        &hv(&u.headers()),
    )
    .await;
    assert_eq!(r.body["items"][0]["external_id"], "movie:603", "{:?}", r.body);

    // type-movie：新 Group 自动补全元数据
    let file = pt_plugin_torrent::fixtures::sample_v1("Matrix", &[("m.mkv", 600 << 20)], true);
    let boundary = "b0undary-md";
    let meta = json!({ "type_id": "movie", "title": "The.Matrix.1999.1080p.BluRay.x264-GRP", "fields": { "imdb_id": "tt0133093" } });
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
    let h = u.headers();
    let r = send(&app, "POST", "/api/p/torrent/v1/torrents", &hv(&h), |b| {
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
            &hv(&u.headers()),
        )
        .await
        .body;
        if g["fields"]["poster"].is_string() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(g["fields"]["poster"], poster, "{g}");
    assert_eq!(g["fields"]["tmdb_id"], 603);
    assert_eq!(g["fields"]["overview"], "一名黑客发现了世界的真相。");
    assert_eq!(g["fields"]["genres"], json!(["action", "scifi"]));
    assert_eq!(g["fields"]["runtime_min"], 136);
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM kernel.event_log WHERE event_type IN ('metadata.fetched', 'group.updated')",
    )
    .fetch_one(&mut db)
    .await
    .unwrap();
    assert!(n >= 2, "metadata.fetched and group.updated must be published");
    kernel.shutdown().await;
}
