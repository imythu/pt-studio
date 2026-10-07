//! 端到端：identity + torrent —— 上传、重复检测、详情、下载（passkey 注入、info hash 不变）。

#![allow(clippy::unwrap_used)]

mod common;

use axum::{body::Body, http::StatusCode};
use pt_kernel::Kernel;
use pt_plugin_torrent::{
    bencode::{self, BValue, Limits},
    fixtures::sample_v1,
};
use serde_json::json;
use sha1::{Digest, Sha1};

use common::{call, config, reset, send};

fn multipart(meta: &serde_json::Value, file: &[u8]) -> (String, Vec<u8>) {
    let boundary = "ptstudio-boundary-7f3a";
    let mut body = Vec::new();
    body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"meta\"\r\nContent-Type: application/json\r\n\r\n{meta}\r\n").as_bytes());
    body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"torrent\"; filename=\"x.torrent\"\r\nContent-Type: application/x-bittorrent\r\n\r\n").as_bytes());
    body.extend(file);
    body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

#[tokio::test]
async fn torrent_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter("info,sqlx=warn")
        .try_init();
    reset().await;
    let kernel = Kernel::start(config(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();

    let r = call(
        &app,
        "POST",
        "/api/p/identity/v1/users",
        Some(json!({"username": "uploader1", "email": "u1@example.com", "password": "correct horse"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        "/api/p/identity/v1/sessions",
        Some(json!({"username": "uploader1", "password": "correct horse"})),
        &[],
    )
    .await;
    let csrf = r.body["csrf_token"].as_str().unwrap().to_owned();
    let cookie = r.cookies.join("; ");
    let auth = [
        ("cookie", cookie.as_str()),
        ("x-csrf-token", csrf.as_str()),
        ("sec-fetch-site", "same-origin"),
    ];

    // 上传
    let file = sample_v1(
        "Some.Movie.2024",
        &[("movie.mkv", 50_000), ("info.nfo", 120)],
        false,
    );
    let meta =
        json!({ "type_id": "movie", "title": "Some Movie 2024 1080p", "fields": {"resolution": "1080p"} });
    let (ct, body) = multipart(&meta, &file);
    let up = send(&app, "POST", "/api/p/torrent/v1/torrents", &auth, |b| {
        b.header("content-type", &ct)
            .body(Body::from(body.clone()))
            .unwrap()
    })
    .await;
    assert_eq!(up.status, StatusCode::CREATED, "{:?}", up.body);
    let tid = up.body["id"].as_i64().unwrap();
    let ih = up.body["info_hash_v1"].as_str().unwrap().to_owned();
    assert_eq!(
        up.headers["location"],
        format!("/api/p/torrent/v1/torrents/{tid}")
    );

    // 重复（规范化后的同一种子）
    let dup = send(&app, "POST", "/api/p/torrent/v1/torrents", &auth, |b| {
        b.header("content-type", &ct).body(Body::from(body)).unwrap()
    })
    .await;
    assert_eq!(
        (dup.status, dup.body["code"].as_str()),
        (StatusCode::CONFLICT, Some("torrent.duplicate")),
        "{:?}",
        dup.body
    );

    // 坏文件
    let (ct2, bad) = multipart(&meta, b"d4:infod4:name1:xee");
    let r = send(&app, "POST", "/api/p/torrent/v1/torrents", &auth, |b| {
        b.header("content-type", &ct2).body(Body::from(bad)).unwrap()
    })
    .await;
    assert_eq!(
        (
            r.status,
            r.body["code"].as_str(),
            r.body["details"]["reason"].as_str()
        ),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("torrent.invalid_file"),
            Some("no_pieces")
        ),
        "{:?}",
        r.body
    );

    // 未知类型
    let (ct3, b3) = multipart(
        &json!({"type_id": "game", "title": "x"}),
        &sample_v1("Other", &[("a", 10)], true),
    );
    let r = send(&app, "POST", "/api/p/torrent/v1/torrents", &auth, |b| {
        b.header("content-type", &ct3).body(Body::from(b3)).unwrap()
    })
    .await;
    assert_eq!(r.body["code"], "torrent.type_unknown");

    // 详情与文件列表
    let r = call(
        &app,
        "GET",
        &format!("/api/p/torrent/v1/torrents/{tid}"),
        None,
        &auth,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        (
            r.body["size"].as_i64(),
            r.body["status"].as_str(),
            r.body["can_download"].as_bool()
        ),
        (Some(50_120), Some("approved"), Some(true))
    );
    let r = call(
        &app,
        "GET",
        &format!("/api/p/torrent/v1/torrents/{tid}/files"),
        None,
        &auth,
    )
    .await;
    assert_eq!(
        r.body["items"][0]["path"],
        json!(["Some.Movie.2024", "movie.mkv"])
    );

    // 下载：announce 带本人 passkey，info hash 与上传结果一致，private = 1
    let pk = call(&app, "GET", "/api/p/identity/v1/users/me/passkey", None, &auth)
        .await
        .body["passkey"]
        .as_str()
        .unwrap()
        .to_owned();
    let dl = call(
        &app,
        "GET",
        &format!("/api/p/torrent/v1/torrents/{tid}/download"),
        None,
        &auth,
    )
    .await;
    assert_eq!(dl.status, StatusCode::OK, "{:?}", dl.body);
    assert_eq!(dl.headers["content-type"], "application/x-bittorrent");
    let root = bencode::decode(&dl.bytes, &Limits::default()).unwrap();
    assert_eq!(
        root.get("announce").and_then(BValue::as_bytes),
        Some(format!("https://tracker.test/announce/{pk}").as_bytes())
    );
    let info = root.get("info").unwrap();
    assert_eq!(hex::encode(Sha1::digest(info.raw().unwrap())), ih);
    assert_eq!(info.get("private").and_then(BValue::as_int), Some(1));

    // 未登录、无此种子
    let r = call(
        &app,
        "GET",
        &format!("/api/p/torrent/v1/torrents/{tid}/download"),
        None,
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = call(&app, "GET", "/api/p/torrent/v1/torrents/999999", None, &auth).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    kernel.shutdown().await;
}
