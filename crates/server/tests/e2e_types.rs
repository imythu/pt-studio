//! 端到端：资源类型（type-movie、type-generic，WASM）—— 类型列表与 schema、上传时规范化、按外部 ID 分组、
//! 规范标题、字段校验错误、通用 kind（预设与新增）、Group 页、搜索字段贡献（Meilisearch 可达时）。

#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use axum::{body::Body, http::StatusCode};
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};

use common::{User, call, env, hv, register_login, reset, send};

fn config(dir: &std::path::Path, meili: Option<&str>) -> SiteConfig {
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n",
        dir.display()
    ));
    if let Some(url) = meili {
        t.push_str(&format!(
            "\n[plugin.search-meilisearch]\nindex_prefix = \"pttypes\"\nflush_window_ms = 200\ncache_ttl_secs = 0\n\
             instances = [{{ id = \"m1\", url = \"{url}\", api_key = \"{}\" }}]\n",
            env("PT_TEST_MEILI_KEY", "pt-test-master-key-0123456789")
        ));
    }
    SiteConfig::parse(&t).unwrap()
}

/// 上传：meta 原样（含 type_id、fields），文件名区分 info_hash
async fn upload(app: &axum::Router, u: &User, meta: Value, name: &str) -> common::Resp {
    let file = pt_plugin_torrent::fixtures::sample_v1(name, &[("a.mkv", 600 << 20)], true);
    let boundary = "b0undary-t";
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

async fn torrent(app: &axum::Router, u: &User, id: i64) -> Value {
    call(
        app,
        "GET",
        &format!("/api/p/torrent/v1/torrents/{id}"),
        None,
        &hv(&u.headers()),
    )
    .await
    .body
}

#[tokio::test]
async fn content_types_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let dir = common::package_wasm_plugins();
    let url = env("PT_TEST_MEILI_URL", "http://127.0.0.1:57700");
    let meili = pt_plugin_search_meilisearch::meili::Meili::new(
        "m1",
        &url,
        &env("PT_TEST_MEILI_KEY", "pt-test-master-key-0123456789"),
        2000,
        10_000,
    )
    .unwrap();
    let with_search = meili.version().await.is_ok();
    if with_search {
        for uid in ["pttypes_torrents", "pttypes_torrents__next"] {
            if meili.index_exists(uid).await.unwrap() {
                let t = meili.delete_index(uid).await.unwrap();
                meili.wait(t, Duration::from_secs(30)).await.unwrap();
            }
        }
    }
    reset().await;
    let kernel = Kernel::start(
        config(&dir, with_search.then_some(url.as_str())),
        pt_server::native_plugins(),
    )
    .await
    .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let admin = register_login(&app, "ty_admin").await;
    common::grant_role(admin.id, "admin").await;
    let u = register_login(&app, "ty_user").await;

    // 类型列表：movie 与 generic（oneOf 按 kind 判别）
    let r = call(&app, "GET", "/api/p/torrent/v1/types", None, &hv(&u.headers())).await;
    let items = r.body["items"].as_array().unwrap().clone();
    let movie = items.iter().find(|i| i["type_id"] == "movie").unwrap();
    assert_eq!(movie["plugin"], "type-movie");
    assert_eq!(
        movie["field_schema"]["properties"]["resolution"]["x-pt-filter"],
        "facet"
    );
    assert!(movie["group_schema"]["properties"]["imdb_id"].is_object());
    let generic = items.iter().find(|i| i["type_id"] == "generic").unwrap();
    let kinds: Vec<&str> = generic["field_schema"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|k| k["properties"]["kind"]["const"].as_str())
        .collect();
    assert_eq!(kinds, vec!["course", "ebook", "game", "software"]);

    // 电影：发布名补全 + IMDb 链接 → 分组
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "movie", "title": "The.Wandering.Earth.II.2023.2160p.WEB-DL.H265.DDP5.1-ABC",
                                      "fields": { "imdb_id": "https://www.imdb.com/title/tt13539646/" } }),
        "m1",
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let (t1, g1) = (
        r.body["id"].as_i64().unwrap(),
        r.body["group_id"].as_i64().unwrap(),
    );
    let d = torrent(&app, &u, t1).await;
    assert_eq!(d["fields"]["imdb_id"], "tt13539646");
    assert_eq!(d["fields"]["resolution"], "2160p");
    assert_eq!(d["fields"]["video_codec"], "hevc");
    assert_eq!(d["fields"]["release_group"], "ABC");
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "movie", "title": "The.Wandering.Earth.II.2023.1080p.BluRay.x264.DTS-XYZ",
                                      "fields": { "imdb_id": "tt13539646" } }),
        "m2",
    )
    .await;
    assert_eq!(r.body["group_id"].as_i64(), Some(g1), "{:?}", r.body);
    let g = call(
        &app,
        "GET",
        &format!("/api/p/torrent/v1/groups/{g1}"),
        None,
        &hv(&u.headers()),
    )
    .await;
    assert_eq!(g.status, StatusCode::OK, "{:?}", g.body);
    assert_eq!(g.body["group_key"], "movie:imdb:tt13539646");
    assert_eq!(g.body["title"], "The Wandering Earth II");
    assert_eq!(g.body["year"], 2023);
    assert_eq!(
        g.body["fields"],
        json!({ "title": "The Wandering Earth II", "year": 2023, "imdb_id": "tt13539646" })
    );
    assert_eq!(g.body["torrents"].as_array().unwrap().len(), 2);

    // 标题为空：规范标题
    let r = upload(&app, &u, json!({ "type_id": "movie", "title": "",
        "fields": { "title": "Heat", "year": 1995, "resolution": "1080p", "source": "BluRay", "video_codec": "x264", "release_group": "FGT" } }), "m3").await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(
        torrent(&app, &u, r.body["id"].as_i64().unwrap()).await["title"],
        "Heat 1995 1080p BluRay AVC -FGT"
    );

    // 校验错误：错误码 + JSON Pointer
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "movie", "title": "Something", "fields": { "resolution": "999p" } }),
        "m4",
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    assert!(
        r.body["code"].as_str().unwrap().starts_with("type-movie."),
        "{:?}",
        r.body
    );
    let paths: Vec<&str> = r.body["details"]["errors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["path"].as_str())
        .collect();
    assert!(
        paths.contains(&"/resolution") && paths.contains(&"/year"),
        "{paths:?}"
    );

    // 通用类型：预设 game
    let r = upload(&app, &u, json!({ "type_id": "generic", "title": "",
        "fields": { "kind": "game", "title": "Hades", "platform": "windows", "release_kind": "repack", "version": "1.38", "year": 2020 } }), "g1").await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let d = torrent(&app, &u, r.body["id"].as_i64().unwrap()).await;
    assert_eq!(d["title"], "Hades v1.38 [Windows] [Repack]");
    let r = upload(
        &app,
        &u,
        json!({ "type_id": "generic", "title": "x", "fields": { "kind": "nope" } }),
        "g2",
    )
    .await;
    assert_eq!(r.body["code"], "type-generic.kind_invalid", "{:?}", r.body);

    // 新增 kind：普通用户无权；管理员定义后立即可用
    let def = json!({ "kind": "comic", "name_zh": "漫画", "name_en": "Comics",
        "fields": [ { "key": "title", "type": "string", "required": true, "group_level": true, "search": "searchable" },
                    { "key": "volume", "type": "integer", "min": 1, "max": 999 },
                    { "key": "status", "type": "enum", "filter": "facet", "options": [ { "value": "ongoing" }, { "value": "completed" } ] } ],
        "group_fallback_fields": ["title"], "title_template": "{title} {volume?Vol.| }{status?[|]}" });
    let r = call(
        &app,
        "POST",
        "/api/p/type-generic/v1/admin/kinds",
        Some(def.clone()),
        &hv(&u.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let mut bad = def.clone();
    bad["title_template"] = json!("{missing}");
    let r = call(
        &app,
        "POST",
        "/api/p/type-generic/v1/admin/kinds",
        Some(bad),
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(
        r.body["code"], "type-generic.kind_definition_invalid",
        "{:?}",
        r.body
    );
    let r = call(
        &app,
        "POST",
        "/api/p/type-generic/v1/admin/kinds",
        Some(def),
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let r = upload(&app, &u, json!({ "type_id": "generic", "title": "", "fields": { "kind": "comic", "title": "Blame!", "volume": 3, "status": "completed" } }), "c1").await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(
        torrent(&app, &u, r.body["id"].as_i64().unwrap()).await["title"],
        "Blame! Vol.3 [completed]"
    );
    let r = call(
        &app,
        "DELETE",
        "/api/p/type-generic/v1/admin/kinds/comic",
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = upload(&app, &u, json!({ "type_id": "generic", "title": "", "fields": { "kind": "comic", "title": "Blame!", "volume": 4 } }), "c2").await;
    assert_eq!(r.body["code"], "type-generic.kind_invalid");

    // 搜索：类型插件贡献的 movie_* 字段可筛选
    if with_search {
        let mut found = Vec::new();
        for _ in 0..300 {
            let r = call(&app, "POST", "/api/p/search-meilisearch/v1/search",
                Some(json!({ "index": "torrents", "filters": [ { "field": "movie_imdb_id", "op": "eq", "values": ["tt13539646"] } ],
                             "facets": ["movie_resolution"] })), &hv(&u.headers())).await;
            found = r.body["hits"].as_array().cloned().unwrap_or_default();
            if found.len() == 2 {
                assert_eq!(r.body["facets"][0]["field"], "movie_resolution", "{:?}", r.body);
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(found.len(), 2, "movie fields were not indexed");
        assert!(
            found
                .iter()
                .all(|h| h["document"]["group_id"].as_i64() == Some(g1))
        );
    } else {
        eprintln!("search part SKIPPED: Meilisearch is not reachable");
    }
    kernel.shutdown().await;
}
