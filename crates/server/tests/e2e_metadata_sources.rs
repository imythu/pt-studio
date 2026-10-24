//! 端到端：无需凭证的元数据源——metadata-bangumi（查询、归一化、封面转存、NSFW 过滤、搜索、关联条目、缓存与负缓存、User-Agent）与
//! metadata-musicbrainz（发行组查询、Cover Art Archive 重定向封面、搜索的 Lucene 转义、缓存），以及 type-anime（Bangumi 优先）/
//! type-music（发行组）新 Group 自动补全。上游为本地模拟服务。
//!
//! 运行：`cargo test -p pt-server --test e2e_metadata_sources -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use axum::{
    body::Body,
    extract::{Path, Query},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};

use common::{User, call, hv, register_login, reset, send};

const RG: &str = "b1392450-e666-3926-a536-22c65f834433";

#[derive(Clone, Default)]
struct Seen {
    hits: Arc<AtomicUsize>,
    ua: Arc<Mutex<Vec<String>>>,
    queries: Arc<Mutex<Vec<String>>>,
}

impl Seen {
    fn hit(&self, h: &HeaderMap) {
        self.hits.fetch_add(1, Ordering::SeqCst);
        let ua = h
            .get("user-agent")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        self.ua.lock().unwrap().push(ua);
    }
}

/// Bangumi（/v0/…）、MusicBrainz（/ws/2/…）、Cover Art Archive（/release-group/…/front-500 → 307）与图片（/img/c.jpg）
async fn mock(seen: Seen) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let img = format!("{base}/img/c.jpg");
    let (s1, s2, s3, s4) = (seen.clone(), seen.clone(), seen.clone(), seen.clone());
    let (i1, i2) = (img.clone(), img.clone());
    let app = axum::Router::new()
        .route(
            "/v0/subjects/{id}",
            axum::routing::get(move |Path(id): Path<u64>, h: HeaderMap| {
                let img = i1.clone();
                s1.hit(&h);
                async move {
                    match id {
                        245_665 => axum::Json(json!({ "id": 245_665, "type": 2, "name": "鬼滅の刃", "name_cn": "鬼灭之刃", "date": "2019-04-06",
                            "platform": "TV", "summary": "大正时期。", "nsfw": false, "eps": 26, "total_episodes": 26,
                            "rating": { "rank": 1566, "total": 22246, "score": 7.3 }, "tags": [{ "name": "热血" }],
                            "images": { "large": img }, "infobox": [{ "key": "导演", "value": "外崎春雄" }, { "key": "播放结束", "value": "2019年9月28日" }] }))
                        .into_response(),
                        777 => axum::Json(json!({ "id": 777, "type": 2, "name": "x", "nsfw": true })).into_response(),
                        _ => StatusCode::NOT_FOUND.into_response(),
                    }
                }
            }),
        )
        .route(
            "/v0/subjects/{id}/subjects",
            axum::routing::get(|| async { axum::Json(json!([{ "id": 283_643, "type": 2, "name": "鬼滅の刃 遊郭編", "name_cn": "鬼灭之刃 游郭篇", "relation": "续集" }])) }),
        )
        .route(
            "/v0/search/subjects",
            axum::routing::post(move |h: HeaderMap, axum::Json(b): axum::Json<Value>| {
                s2.hit(&h);
                async move {
                    assert_eq!(b["filter"], json!({ "type": [2], "nsfw": false }), "{b}");
                    axum::Json(json!({ "data": [{ "id": 245_665, "type": 2, "name": "鬼滅の刃", "name_cn": format!("{} 结果", b["keyword"].as_str().unwrap_or("")), "date": "2019-04-06" }], "total": 1 }))
                }
            }),
        )
        .route(
            "/ws/2/release-group/{id}",
            axum::routing::get(move |Path(id): Path<String>, h: HeaderMap| {
                s3.hit(&h);
                async move {
                    if id != RG {
                        return StatusCode::NOT_FOUND.into_response();
                    }
                    axum::Json(json!({ "id": RG, "title": "OK Computer", "first-release-date": "1997-05-21", "primary-type": "Album", "secondary-types": [],
                        "artist-credit": [{ "name": "Radiohead", "joinphrase": "", "artist": { "id": "a74b1b7f-71a5-4011-9441-d0b5e4122711", "name": "Radiohead" } }],
                        "genres": [{ "name": "alternative rock", "count": 27 }, { "name": "rock", "count": 13 }] }))
                    .into_response()
                }
            }),
        )
        .route(
            "/ws/2/release-group/",
            axum::routing::get(move |Query(q): Query<std::collections::HashMap<String, String>>, h: HeaderMap| {
                s4.hit(&h);
                s4.queries.lock().unwrap().push(q.get("query").cloned().unwrap_or_default());
                async move {
                    axum::Json(json!({ "release-groups": [{ "id": RG, "title": "OK Computer", "first-release-date": "1997-05-21",
                        "primary-type": "Album", "artist-credit": [{ "name": "Radiohead" }] }] }))
                }
            }),
        )
        .route(
            "/release-group/{id}/front-500",
            axum::routing::get(move |Path(id): Path<String>| {
                let img = i2.clone();
                async move {
                    if id != RG {
                        return StatusCode::NOT_FOUND.into_response();
                    }
                    (StatusCode::TEMPORARY_REDIRECT, [("location", img)]).into_response()
                }
            }),
        )
        .route(
            "/img/c.jpg",
            axum::routing::get(|| async { ([("content-type", "image/jpeg")], common::JPEG.to_vec()) }),
        );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

fn config(dir: &std::path::Path, base: &str) -> SiteConfig {
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n\n[plugin.metadata-bangumi]\napi_base_url = \"{base}\"\n\n\
         [plugin.metadata-musicbrainz]\napi_base_url = \"{base}\"\ncaa_base_url = \"{base}\"\ncontact = \"admin@example.test\"\nrate_per_sec = 20\n",
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
    let boundary = "b0undary-ms";
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

/// 等待 Group 的某个字段被补全
async fn wait_group(app: &axum::Router, u: &User, gid: i64, key: &str) -> Value {
    let mut g = Value::Null;
    for _ in 0..200 {
        g = call(
            app,
            "GET",
            &format!("/api/p/torrent/v1/groups/{gid}"),
            None,
            &hv(&u.headers()),
        )
        .await
        .body;
        if !g["fields"][key].is_null() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    g
}

#[tokio::test]
async fn bangumi_and_musicbrainz() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let seen = Seen::default();
    let base = mock(seen.clone()).await;
    let dir = common::package_wasm_plugins();
    let kernel = Kernel::start(config(&dir, &base), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let user = register_login(&app, "meta_src").await;
    let get = |path: String| {
        let (app, h) = (app.clone(), user.headers());
        async move { call(&app, "GET", &path, None, &hv(&h)).await }
    };

    // ---- Bangumi ----
    let r = get("/api/p/metadata-bangumi/v1/lookup?id=https%3A%2F%2Fbgm.tv%2Fsubject%2F245665".into()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let d = &r.body["data"];
    assert_eq!(r.body["external_id"], "245665");
    assert_eq!(
        (
            d["kind"].clone(),
            d["titles"]["localized"]["zh-CN"].clone(),
            d["episode_count"].clone()
        ),
        (json!("anime"), json!("鬼灭之刃"), json!(26))
    );
    assert_eq!(d["ratings"]["bangumi"], json!({ "value": 7.3, "votes": 22246 }));
    let cover = d["images"]["cover"].as_str().unwrap().to_owned();
    assert!(cover.starts_with("/media/metadata-bangumi/images/"), "{cover}");
    let img = call(&app, "GET", &cover, None, &[]).await;
    assert_eq!((img.status, &img.bytes[..2]), (StatusCode::OK, &[0xFF, 0xD8][..]));
    let ua = seen.ua.lock().unwrap().last().cloned().unwrap();
    assert!(
        ua.starts_with("PT-Studio/") && ua.contains("(https://github.com/"),
        "{ua}"
    );
    // 缓存命中
    let before = seen.hits.load(Ordering::SeqCst);
    assert_eq!(
        get("/api/p/metadata-bangumi/v1/lookup?id=bangumi:245665".into())
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(seen.hits.load(Ordering::SeqCst), before);
    // NSFW（未配置令牌）按不存在处理；不存在 + 负缓存；编号格式错误
    assert_eq!(
        get("/api/p/metadata-bangumi/v1/lookup?id=777".into())
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get("/api/p/metadata-bangumi/v1/lookup?id=5".into()).await.status,
        StatusCode::NOT_FOUND
    );
    let before = seen.hits.load(Ordering::SeqCst);
    assert_eq!(
        get("/api/p/metadata-bangumi/v1/lookup?id=5".into()).await.status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(seen.hits.load(Ordering::SeqCst), before, "negative cache");
    assert_eq!(
        get("/api/p/metadata-bangumi/v1/lookup?id=abc".into()).await.body["code"],
        "metadata-bangumi.id_invalid"
    );
    // 搜索、关联条目
    let r = get("/api/p/metadata-bangumi/v1/search?q=%E9%AC%BC%E7%81%AD".into()).await;
    assert_eq!(
        r.body["items"][0],
        json!({ "external_id": "245665", "title": "鬼灭 结果", "original_title": "鬼滅の刃", "year": 2019, "kind": "anime", "relation": null }),
        "{:?}",
        r.body
    );
    let r = get("/api/p/metadata-bangumi/v1/related?id=245665".into()).await;
    assert_eq!(
        (
            r.body["items"][0]["relation"].clone(),
            r.body["items"][0]["external_id"].clone()
        ),
        (json!("续集"), json!("283643")),
        "{:?}",
        r.body
    );

    // ---- MusicBrainz ----
    let r = get(format!(
        "/api/p/metadata-musicbrainz/v1/lookup?id=https%3A%2F%2Fmusicbrainz.org%2Frelease-group%2F{RG}"
    ))
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["external_id"], format!("release-group:{RG}"));
    let d = &r.body["data"];
    assert_eq!(
        (
            d["titles"]["original"].clone(),
            d["year"].clone(),
            d["genres"].clone()
        ),
        (
            json!("OK Computer"),
            json!(1997),
            json!(["alternative rock", "rock"])
        )
    );
    assert_eq!(
        (
            d["extra"]["artist_credit_string"].clone(),
            d["extra"]["release_type"].clone()
        ),
        (json!("Radiohead"), json!("album"))
    );
    let mb_cover = d["images"]["cover"].as_str().unwrap().to_owned();
    assert!(
        mb_cover.starts_with("/media/metadata-musicbrainz/images/"),
        "{mb_cover}"
    );
    let ua = seen.ua.lock().unwrap().last().cloned().unwrap();
    assert!(ua.ends_with("( admin@example.test )"), "{ua}");
    let before = seen.hits.load(Ordering::SeqCst);
    assert_eq!(
        get(format!(
            "/api/p/metadata-musicbrainz/v1/lookup?id=release-group:{RG}"
        ))
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(seen.hits.load(Ordering::SeqCst), before);
    assert_eq!(
        get(
            "/api/p/metadata-musicbrainz/v1/lookup?id=release-group:00000000-0000-0000-0000-000000000000"
                .into()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let r = get("/api/p/metadata-musicbrainz/v1/search?q=AC%2FDC%3A%20Back".into()).await;
    assert_eq!(
        r.body["items"][0]["external_id"],
        format!("release-group:{RG}"),
        "{:?}",
        r.body
    );
    assert_eq!(
        seen.queries.lock().unwrap().last().map(String::as_str),
        Some("AC\\/DC\\: Back")
    );

    // ---- type-anime：Bangumi 编号补全作品 ----
    let r = upload(
        &app,
        &user,
        json!({ "type_id": "anime", "title": "", "fields": { "release_name": "[ANi] Kimetsu no Yaiba - 05 [1080P][WEB-DL][CHT]", "bangumi_id": "https://bgm.tv/subject/245665" } }),
        "ms-anime",
        &[("[ANi] Kimetsu no Yaiba - 05 [1080P][WEB-DL][CHT].mp4", 60 << 20)],
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let g = wait_group(&app, &user, r.body["group_id"].as_i64().unwrap(), "poster").await;
    let f = &g["fields"];
    assert_eq!(
        (f["poster"].as_str(), f["title_zh"].as_str(), f["rating"].as_f64()),
        (Some(cover.as_str()), Some("鬼灭之刃"), Some(7.3)),
        "{g}"
    );
    assert_eq!(
        (f["episode_count"].as_i64(), f["overview"].as_str()),
        (Some(26), Some("大正时期。")),
        "{g}"
    );

    // ---- type-music：发行组补全封面、流派 ----
    let r = upload(
        &app,
        &user,
        json!({ "type_id": "music", "title": "Radiohead - OK Computer [1997] [WEB FLAC]", "fields": {
            "artists": "Radiohead", "album": "OK Computer", "media": "web", "format": "flac", "encoding": "lossless",
            "mb_release_group_id": format!("https://musicbrainz.org/release-group/{RG}") } }),
        "ms-music",
        &[("01 Airbag.flac", 30 << 20), ("02 Paranoid Android.flac", 40 << 20)],
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let g = wait_group(&app, &user, r.body["group_id"].as_i64().unwrap(), "cover").await;
    let f = &g["fields"];
    assert_eq!(
        (
            f["cover"].as_str(),
            f["genres"].clone(),
            f["original_year"].as_i64()
        ),
        (
            Some(mb_cover.as_str()),
            json!(["alternative rock", "rock"]),
            Some(1997)
        ),
        "{g}"
    );
    drop(kernel);
}
