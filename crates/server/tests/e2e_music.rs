//! 端到端：音乐类型（type-music，WASM）—— 发布名补全、艺人与专辑分组、格式与编码组合、上传钩子的文件检查、
//! log 预检接口、上传后异步 log 评分写回种子字段（`ctx.torrents.update_fields`）、编辑后重新评分。

#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use axum::{body::Body, http::StatusCode};
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};

use common::{User, call, hv, register_login, reset, send};

const EAC_GOOD: &str = "Exact Audio Copy V1.6 from 23. October 2020\r\n\r\nEAC extraction logfile from 1. January 2021, 12:00\r\n\r\n\
Used drive  : PLEXTOR DVDR   PX-716A   Adapter: 1  ID: 0\r\n\r\nRead mode               : Secure\r\n\
Defeat audio cache      : Yes\r\n\r\nRead offset correction                      : 30\r\n\
Gap handling                                : Appended to previous track\r\n\r\n\
Track  1\r\n\r\n     Test CRC 1A2B3C4D\r\n     Copy CRC 1A2B3C4D\r\n     Accurately ripped (confidence 5)  [ABCDEF12]  (AR v2)\r\n     Copy OK\r\n\r\n\
No errors occurred\r\n\r\nEnd of status report\r\n\r\n==== Log checksum 0123456789ABCDEF ====\r\n";

fn config(dir: &std::path::Path) -> SiteConfig {
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n",
        dir.display()
    ));
    SiteConfig::parse(&t).unwrap()
}

async fn upload(
    app: &axum::Router,
    u: &User,
    meta: Value,
    name: &str,
    files: &[(&str, u64)],
) -> common::Resp {
    let file = pt_plugin_torrent::fixtures::sample_v1(name, files, true);
    let boundary = "b0undary-mu";
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
    send(app, "POST", "/api/p/torrent/v1/torrents", &hv(&h), |b| {
        b.header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body.clone()))
        .unwrap()
    })
    .await
}

async fn get(app: &axum::Router, u: &User, path: &str) -> Value {
    call(app, "GET", path, None, &hv(&u.headers())).await.body
}

/// 等待异步检查写回 `log_score`（满足 `pred`）
async fn wait_score(app: &axum::Router, u: &User, id: i64, pred: impl Fn(&Value) -> bool) -> Value {
    let mut d = Value::Null;
    for _ in 0..200 {
        d = get(app, u, &format!("/api/p/torrent/v1/torrents/{id}")).await;
        if pred(&d["fields"]["log_score"]) {
            return d;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("log_score not written: {d}");
}

#[tokio::test]
async fn music_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let dir = common::package_wasm_plugins();
    reset().await;
    let kernel = Kernel::start(config(&dir), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let u = register_login(&app, "mu_user").await;

    let types = get(&app, &u, "/api/p/torrent/v1/types").await;
    let mu = types["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["type_id"] == "music")
        .cloned()
        .unwrap();
    assert_eq!(mu["plugin"], "type-music");

    // WEB 24bit：发布名补全艺人 / 专辑 / 年份 / 格式；未填标题 → 规范标题
    let files: Vec<(&str, u64)> = vec![
        ("01 - Give Life Back to Music.flac", 30 << 20),
        ("02 - The Game of Love.flac", 30 << 20),
        ("cover.jpg", 1 << 20),
    ];
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "music", "title": "", "fields": { "release_name": "Daft Punk - Random Access Memories (2013) [FLAC 24bit WEB]" } }),
        "mu-web",
        &files,
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let g1 = r.body["group_id"].as_i64().unwrap();
    let d = get(&app, &u, &format!("/api/p/torrent/v1/torrents/{}", r.body["id"])).await;
    assert_eq!(
        d["title"],
        "Daft Punk - Random Access Memories [2013] [WEB FLAC 24bit Lossless]"
    );
    assert_eq!(d["fields"]["encoding"], "lossless_24");
    let g = get(&app, &u, &format!("/api/p/torrent/v1/groups/{g1}")).await;
    assert_eq!(
        (g["title"].as_str(), g["year"].as_i64()),
        (Some("Daft Punk - Random Access Memories"), Some(2013)),
        "{g}"
    );
    assert_eq!(g["fields"]["artists"], json!(["Daft Punk"]));

    // CD + log + cue：同一 Group；上传者填的 log_score 被忽略，异步检查写回 100
    let cd_files: Vec<(&str, u64)> = vec![
        ("01.flac", 30 << 20),
        ("Daft Punk.log", 4000),
        ("Daft Punk.cue", 2000),
    ];
    let cd = json!({ "type_id": "music", "title": "Daft Punk - Random Access Memories [2013] [CD FLAC Lossless Log]", "fields": {
        "artists": "Daft Punk", "album": "Random Access Memories", "original_year": 2013, "media": "CD", "format": "FLAC",
        "has_cue": true, "logs": [EAC_GOOD], "log_score": 42, "edition_label": "Columbia", "edition_catalog": "88883716861" } });
    let r = upload(&app, &u, cd.clone(), "mu-cd", &cd_files).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["group_id"].as_i64(), Some(g1));
    let cd_id = r.body["id"].as_i64().unwrap();
    let d = wait_score(&app, &u, cd_id, |s| s.as_i64() == Some(100)).await;
    assert_eq!(
        (d["fields"]["has_log"].as_bool(), d["fields"]["encoding"].as_str()),
        (Some(true), Some("lossless"))
    );

    // 编辑换成有问题的 log → 重新评分（非安全模式 −20、无校验和 −15）
    let bad = EAC_GOOD
        .replace(
            "Read mode               : Secure",
            "Read mode               : Burst",
        )
        .replace("==== Log checksum 0123456789ABCDEF ====\r\n", "");
    let mut f = d["fields"].clone();
    f["logs"] = json!([bad]);
    let r = call(
        &app,
        "PATCH",
        &format!("/api/p/torrent/v1/torrents/{cd_id}"),
        Some(json!({ "fields": f })),
        &hv(&u.headers()),
    )
    .await;
    assert!(r.status.is_success(), "{:?}", r.body);
    wait_score(&app, &u, cd_id, |s| s.as_i64() == Some(65)).await;

    // 文件检查
    let deny = |code: &'static str| {
        move |r: &common::Resp| {
            (r.status, r.body["code"].as_str().map(str::to_owned))
                == (StatusCode::UNPROCESSABLE_ENTITY, Some(code.to_owned()))
        }
    };
    let r = upload(
        &app,
        &u,
        cd.clone(),
        "mu-nolog",
        &[("01.flac", 30 << 20), ("a.cue", 100)],
    )
    .await;
    assert!(deny("type-music.log_missing")(&r), "{:?}", r.body);
    let r = upload(
        &app,
        &u,
        cd.clone(),
        "mu-mp3",
        &[("01.mp3", 30 << 20), ("a.log", 100), ("a.cue", 100)],
    )
    .await;
    assert!(deny("type-music.no_audio_files")(&r), "{:?}", r.body);
    let r = upload(
        &app,
        &u,
        cd.clone(),
        "mu-exe",
        &[
            ("01.flac", 30 << 20),
            ("a.log", 100),
            ("a.cue", 100),
            ("setup.exe", 100),
        ],
    )
    .await;
    assert!(deny("type-music.forbidden_file")(&r), "{:?}", r.body);
    // 格式与编码组合
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "music", "title": "X - Y", "fields": { "artists": "X", "album": "Y", "media": "web", "format": "mp3", "encoding": "24bit" } }),
        "mu-combo",
        &[("01.mp3", 30 << 20)],
    )
    .await;
    assert!(deny("type-music.format_encoding_invalid")(&r), "{:?}", r.body);

    // log 预检
    let r = call(
        &app,
        "POST",
        "/api/p/type-music/v1/log-check",
        Some(json!({ "log": EAC_GOOD })),
        &hv(&u.headers()),
    )
    .await;
    assert_eq!(
        (r.status, r.body["software"].as_str(), r.body["score"].as_i64()),
        (StatusCode::OK, Some("eac"), Some(100)),
        "{:?}",
        r.body
    );
    let r = call(
        &app,
        "POST",
        "/api/p/type-music/v1/log-check",
        Some(json!({ "log": "hello" })),
        &hv(&u.headers()),
    )
    .await;
    assert_eq!(
        (r.body["score"].clone(), r.body["issues"][0]["code"].as_str()),
        (Value::Null, Some("unknown_ripper"))
    );
    let r = call(
        &app,
        "POST",
        "/api/p/type-music/v1/log-check",
        Some(json!({ "log": "x".repeat(520 * 1024) })),
        &hv(&u.headers()),
    )
    .await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (StatusCode::PAYLOAD_TOO_LARGE, Some("type-music.log_too_large")),
        "{:?}",
        r.body
    );
    kernel.shutdown().await;
}
