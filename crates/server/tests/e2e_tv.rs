//! 端到端：剧集与动漫类型（type-tv、type-anime，WASM）—— 发布名推断季集与范围、按季分组与作品标题、规范标题、
//! 范围组合校验、上传钩子按文件列表检查范围、剧与季元数据补全（模拟 TMDB）；动漫的字幕组风格发布名、字幕组别名
//! （后台设置）、字幕标签、按 Bangumi 分组、剧场版。

#![allow(clippy::unwrap_used)]

mod common;

use std::{
    sync::{Arc, atomic::AtomicUsize},
    time::Duration,
};

use axum::{body::Body, http::StatusCode};
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};

use common::{User, call, hv, register_login, reset, send};

fn config(dir: &std::path::Path, base: &str) -> SiteConfig {
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n\n[plugin.metadata-tmdb]\napi_key = \"testkey\"\napi_base_url = \"{base}\"\nimage_base_url = \"{base}\"\n",
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
    let boundary = "b0undary-tv";
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

#[tokio::test]
async fn tv_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let dir = common::package_wasm_plugins();
    let base = common::mock_tmdb(Arc::new(AtomicUsize::new(0))).await;
    reset().await;
    let kernel = Kernel::start(config(&dir, &base), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let u = register_login(&app, "tv_user").await;

    let types = get(&app, &u, "/api/p/torrent/v1/types").await;
    let tv = types["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["type_id"] == "tv")
        .cloned()
        .unwrap();
    assert_eq!(tv["plugin"], "type-tv");
    assert_eq!(
        tv["field_schema"]["properties"]["scope"]["enum"],
        json!(["season_pack", "episode", "episodes", "complete"])
    );

    // 季包：发布名推断季与范围；10 集（≥ 50 MiB）+ sample 与 nfo 不计入
    let eps: Vec<String> = (1..=10)
        .map(|e| format!("Game.of.Thrones.S01E{e:02}.1080p.BluRay.x264-GRP.mkv"))
        .collect();
    let mut files: Vec<(&str, u64)> = eps.iter().map(|e| (e.as_str(), 55 << 20)).collect();
    files.push(("Sample/sample.mkv", 55 << 20));
    files.push(("info.nfo", 2000));
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "tv", "title": "Game.of.Thrones.S01.1080p.BluRay.x264-GRP", "fields": { "tmdb_tv_id": "https://www.themoviedb.org/tv/1399" } }),
        "got-s01",
        &files,
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let (t1, g1) = (
        r.body["id"].as_i64().unwrap(),
        r.body["group_id"].as_i64().unwrap(),
    );
    let d = get(&app, &u, &format!("/api/p/torrent/v1/torrents/{t1}")).await;
    assert_eq!(
        (
            d["fields"]["scope"].as_str(),
            d["fields"]["season"].as_i64(),
            d["fields"]["tmdb_tv_id"].as_u64()
        ),
        (Some("season_pack"), Some(1), Some(1399))
    );
    let g = get(&app, &u, &format!("/api/p/torrent/v1/groups/{g1}")).await;
    assert_eq!(g["title"], "Game of Thrones 第1季 / Season 1", "{g}");

    // 元数据：剧 + 季（季海报、季简介、季集数优先）
    let mut g = Value::Null;
    for _ in 0..200 {
        g = get(&app, &u, &format!("/api/p/torrent/v1/groups/{g1}")).await;
        if g["fields"]["poster"].is_string() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let f = &g["fields"];
    assert!(
        f["poster"]
            .as_str()
            .unwrap()
            .starts_with("/media/metadata-tmdb/images/"),
        "{g}"
    );
    assert_eq!(
        (f["overview"].as_str(), f["episode_count"].as_i64()),
        (Some("第一季简介。"), Some(10)),
        "{g}"
    );
    assert_eq!(
        (
            f["title_zh"].as_str(),
            f["status"].as_str(),
            f["imdb_id"].as_str()
        ),
        (Some("权力的游戏"), Some("ended"), Some("tt0944947"))
    );
    assert_eq!(f["genres"], json!(["drama"]));

    // 单集进同一季 Group；未填标题时用规范标题（含 S01E02）
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "tv", "title": "", "fields": { "release_name": "Game.of.Thrones.S01E02.720p.WEB-DL.DDP5.1.H.264-NTb", "tmdb_tv_id": 1399 } }),
        "got-s01e02",
        &[("Game.of.Thrones.S01E02.720p.WEB-DL.mkv", 60 << 20)],
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["group_id"].as_i64(), Some(g1));
    let d = get(&app, &u, &format!("/api/p/torrent/v1/torrents/{}", r.body["id"])).await;
    assert_eq!(d["title"], "Game of Thrones S01E02 720p WEB-DL AVC DDP -NTb");

    // 范围与文件不符：声明单集但含两集
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "tv", "title": "Game.of.Thrones.S01E03.720p.WEB-DL.H.264-NTb", "fields": { "tmdb_tv_id": 1399 } }),
        "got-bad",
        &[("S01E03.mkv", 60 << 20), ("S01E04.mkv", 60 << 20)],
    )
    .await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("type-tv.scope_mismatch")),
        "{:?}",
        r.body
    );

    // 已有 Group 的季包缺集（10 集只有 5 集，比例 0.8）
    let few: Vec<(&str, u64)> = eps.iter().take(5).map(|e| (e.as_str(), 55 << 20)).collect();
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "tv", "title": "Game.of.Thrones.S01.720p.BluRay.x264-OTHER", "fields": { "tmdb_tv_id": 1399 } }),
        "got-few",
        &few,
    )
    .await;
    assert_eq!(r.body["code"], "type-tv.scope_mismatch", "{:?}", r.body);
    // 连载中的季包不检查完整度
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "tv", "title": "Game.of.Thrones.S01.720p.BluRay.x264-OTHER", "fields": { "tmdb_tv_id": 1399, "ongoing": true } }),
        "got-ongoing",
        &few,
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);

    // 第 2 季是另一个 Group；按标题分组（无外部 ID）
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "tv", "title": "Game.of.Thrones.S02E01.1080p.HDTV.x264-GRP", "fields": { "tmdb_tv_id": 1399 } }),
        "got-s02e01",
        &[("Game.of.Thrones.S02E01.mkv", 60 << 20)],
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_ne!(r.body["group_id"].as_i64(), Some(g1));
    let g = get(
        &app,
        &u,
        &format!("/api/p/torrent/v1/groups/{}", r.body["group_id"]),
    )
    .await;
    assert_eq!(g["title"], "Game of Thrones 第2季 / Season 2");

    // 组合校验：多集缺结束集；未知范围
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "tv", "title": "Some.Show.1080p.WEB-DL.H.264-GRP", "fields": { "scope": "episodes", "episode": 3 } }),
        "bad-1",
        &[("a.mkv", 60 << 20)],
    )
    .await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("type-tv.scope_invalid")),
        "{:?}",
        r.body
    );
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "tv", "title": "Some.Show.S01.1080p.WEB-DL.H.264-GRP", "fields": { "scope": "weekly" } }),
        "bad-2",
        &[("a.mkv", 60 << 20)],
    )
    .await;
    assert_eq!(r.body["code"], "type-tv.scope_invalid", "{:?}", r.body);
    kernel.shutdown().await;
}

#[tokio::test]
async fn anime_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let dir = common::package_wasm_plugins();
    let base = common::mock_tmdb(Arc::new(AtomicUsize::new(0))).await;
    reset().await;
    let kernel = Kernel::start(config(&dir, &base), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let admin = register_login(&app, "an_admin").await;
    common::grant_role(admin.id, "admin").await;
    let u = register_login(&app, "an_user").await;
    let r = call(
        &app,
        "PUT",
        "/api/kernel/v1/admin/plugins/type-anime/settings",
        Some(json!({ "settings": { "fansub_aliases": { "ani": "ANi", "Nekomoe kissaten": "喵萌奶茶屋" } }, "version": 0 })),
        &hv(&admin.headers()),
    )
    .await;
    assert!(r.status.is_success(), "{:?}", r.body);

    // 单集：字幕组别名、中文名 / 罗马音、字幕标签；未填标题 → 规范标题
    let rel = "[ani] 葬送的芙莉莲 / Sousou no Frieren - 05 [1080P][Baha][WEB-DL][AAC AVC][CHT].mp4";
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "anime", "title": "", "fields": { "release_name": rel } }),
        "an-1",
        &[(
            "[ANi] Sousou no Frieren - 05 [1080P][Baha][WEB-DL][AAC AVC][CHT].mp4",
            60 << 20,
        )],
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let d = get(&app, &u, &format!("/api/p/torrent/v1/torrents/{}", r.body["id"])).await;
    let f = &d["fields"];
    assert_eq!(
        (
            f["fansub"].as_str(),
            f["episode"].as_i64(),
            f["subtitle_langs"].clone()
        ),
        (Some("ANi"), Some(5), json!(["cht"])),
        "{d}"
    );
    assert_eq!(
        d["title"],
        "[ANi] Sousou no Frieren S01E05 [1080p][WEB-DL][AVC][CHT]"
    );
    let g = get(
        &app,
        &u,
        &format!("/api/p/torrent/v1/groups/{}", r.body["group_id"]),
    )
    .await;
    assert_eq!(g["title"], "葬送的芙莉莲 / Sousou no Frieren");

    // 文件与单集不符
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "anime", "title": "[ANi] Sousou no Frieren - 06 [1080P][WEB-DL][CHT]" }),
        "an-bad",
        &[
            ("[ANi] Sousou no Frieren - 06 [1080P].mp4", 60 << 20),
            ("[ANi] Sousou no Frieren - 07 [1080P].mp4", 60 << 20),
        ],
    )
    .await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("type-anime.scope_mismatch")
        ),
        "{:?}",
        r.body
    );

    // 合集：区间 + “合集” → 季包；Bangumi 链接分组
    let eps: Vec<String> = (1..=12)
        .map(|e| format!("[Nekomoe kissaten][Kimetsu no Yaiba][{e:02}][BDRip][1080p][JPSC].mkv"))
        .collect();
    let files: Vec<(&str, u64)> = eps.iter().map(|e| (e.as_str(), 55 << 20)).collect();
    let meta = json!({ "type_id": "anime", "title": "[Nekomoe kissaten][Kimetsu no Yaiba][01-12][BDRip][合集][1080p][JPSC]",
                       "fields": { "bangumi_id": "https://bgm.tv/subject/206382" } });
    let r = upload(&app, &u, meta, "an-batch", &files).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let d = get(&app, &u, &format!("/api/p/torrent/v1/torrents/{}", r.body["id"])).await;
    assert_eq!(
        (
            d["fields"]["scope"].as_str(),
            d["fields"]["fansub"].as_str(),
            d["fields"]["bangumi_id"].as_u64()
        ),
        (Some("season_pack"), Some("喵萌奶茶屋"), Some(206_382))
    );
    let g1 = r.body["group_id"].as_i64();
    // 同一 Bangumi 条目的另一个发布进同一 Group
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "anime", "title": "[Other] Kimetsu no Yaiba - 03 [720p]", "fields": { "bangumi_id": 206_382 } }),
        "an-other",
        &[("[Other] Kimetsu no Yaiba - 03 [720p].mkv", 60 << 20)],
    )
    .await;
    assert_eq!(
        (r.status, r.body["group_id"].as_i64()),
        (StatusCode::CREATED, g1),
        "{:?}",
        r.body
    );

    // 剧场版：不分季集、不检查范围
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "anime", "title": "[Group] Suzume no Tojimari [BDRip 1080p HEVC-10bit FLAC][简繁内封]",
                "fields": { "media": "movie", "title_zh": "铃芽之旅", "year": 2022 } }),
        "an-movie",
        &[("Suzume.mkv", 60 << 20)],
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let d = get(&app, &u, &format!("/api/p/torrent/v1/torrents/{}", r.body["id"])).await;
    assert_eq!(
        (
            d["fields"].get("scope"),
            d["fields"]["bit_depth"].as_i64(),
            d["fields"]["subtitle_kind"].as_str()
        ),
        (None, Some(10), Some("softsub")),
        "{d}"
    );
    let g = get(
        &app,
        &u,
        &format!("/api/p/torrent/v1/groups/{}", r.body["group_id"]),
    )
    .await;
    assert_eq!(g["title"], "铃芽之旅 / Suzume no Tojimari 剧场版");

    // 字幕标签无法识别
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "anime", "title": "[G] X - 01 [1080p]", "fields": { "subtitle_langs": ["klingon"] } }),
        "an-sub",
        &[("x.mkv", 60 << 20)],
    )
    .await;
    assert_eq!(r.body["code"], "type-anime.subtitle_invalid", "{:?}", r.body);
    kernel.shutdown().await;
}
