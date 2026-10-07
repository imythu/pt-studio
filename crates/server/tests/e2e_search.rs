//! 端到端：search-meilisearch —— 首次建索引（swap 重建）、增量同步、发布名解析、过滤 / 排序 / facet / 游标 / 高亮、
//! 可见性（待审种子仅上传者与有 restricted 权限者可见）、缓存、手动重建。
//!
//! 需要可访问的 Meilisearch（`PT_TEST_MEILI_URL`，默认 http://127.0.0.1:57700，master key `PT_TEST_MEILI_KEY`）；
//! 不可达时跳过。

#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use pt_plugin_search_meilisearch::meili::Meili;
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{call, env, hv, register_login, reset, upload};

const PREFIX: &str = "pttest";

fn config(url: &str, key: &str) -> SiteConfig {
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugin.search-meilisearch]\nindex_prefix = \"{PREFIX}\"\nflush_window_ms = 200\ncache_ttl_secs = 3\n\
         instances = [{{ id = \"m1\", url = \"{url}\", api_key = \"{key}\" }}]\n"
    ));
    SiteConfig::parse(&t).unwrap()
}

async fn wait_for<F, Fut>(what: &str, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..300 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn search(app: &axum::Router, u: &common::User, body: Value) -> common::Resp {
    let mut h = u.headers();
    h.push(("content-type", "application/json".into()));
    call(
        app,
        "POST",
        "/api/p/search-meilisearch/v1/search",
        Some(body),
        &hv(&h),
    )
    .await
}

fn ids(r: &common::Resp) -> Vec<i64> {
    let mut v: Vec<i64> = r.body["hits"]
        .as_array()
        .map(|a| a.iter().filter_map(|h| h["id"].as_i64()).collect())
        .unwrap_or_default();
    v.sort_unstable();
    v
}

#[tokio::test]
async fn search_end_to_end() {
    let url = env("PT_TEST_MEILI_URL", "http://127.0.0.1:57700");
    let key = env("PT_TEST_MEILI_KEY", "pt-test-master-key-0123456789");
    let meili = Meili::new("m1", &url, &key, 2000, 10_000).unwrap();
    if meili.version().await.is_err() {
        eprintln!("SKIPPED: Meilisearch is not reachable at {url}");
        return;
    }
    let _guard = common::LOCK.lock().await;
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter("info,sqlx=warn")
        .try_init();
    for uid in [format!("{PREFIX}_torrents"), format!("{PREFIX}_torrents__next")] {
        if meili.index_exists(&uid).await.unwrap() {
            let t = meili.delete_index(&uid).await.unwrap();
            meili.wait(t, Duration::from_secs(30)).await.unwrap();
        }
    }
    reset().await;
    let mut db = PgConnection::connect(&env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap();
    let kernel = Kernel::start(config(&url, &key), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let admin = register_login(&app, "srch_admin").await;
    common::grant_role(admin.id, "admin").await;
    let alice = register_login(&app, "srch_alice").await;
    let bob = register_login(&app, "srch_bob").await;

    // 种子在索引建好之前上传：首次重建枚举时一并写入
    let t1 = upload(
        &app,
        &alice,
        "The.Wandering.Earth.II.2023.2160p.WEB-DL.H265.DDP5.1-ABC",
        &[("a.mkv", 40_000)],
    )
    .await;
    let get_admin_index = || {
        let app = app.clone();
        let h = admin.headers();
        async move {
            let r = call(
                &app,
                "GET",
                "/api/p/search-meilisearch/v1/admin/indexes",
                None,
                &hv(&h),
            )
            .await;
            r.body["items"][0].clone()
        }
    };
    wait_for("initial rebuild", || async {
        let ix = get_admin_index().await;
        ix["status"] == "ready" && ix["generation"].as_i64() >= Some(1) && ix["documents"] == 1
    })
    .await;
    let ix = get_admin_index().await;
    assert_eq!(ix["contributors"][0]["plugin"], "torrent", "{ix}");
    assert_eq!(ix["documents"], 1, "{ix}");

    // 增量：上传后经事件 → 待写表 → flush 写入
    let t2 = upload(
        &app,
        &bob,
        "Some.Show.S01.1080p.AMZN.WEB-DL.DDP5.1.H.264-GRP",
        &[("b.mkv", 90_000)],
    )
    .await;
    let t3 = upload(
        &app,
        &bob,
        "流浪地球2.The.Wandering.Earth.II.2023.UHD.BluRay.2160p.x265.10bit.DoVi.TrueHD.Atmos.7.1-XYZ@HDSky",
        &[("c.mkv", 70_000)],
    )
    .await;
    wait_for("incremental indexing", || async {
        ids(&search(&app, &alice, json!({ "index": "torrents" })).await).len() == 3
    })
    .await;

    // 关键词 + 高亮 + 发布名字段
    let r = search(
        &app,
        &alice,
        json!({ "index": "torrents", "query": "wandering earth" }),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(ids(&r), vec![t1, t3]);
    let hit = r.body["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["id"] == t3)
        .unwrap()
        .clone();
    assert_eq!(hit["document"]["rel_resolution"], "2160p");
    assert_eq!(hit["document"]["rel_source"], "bluray");
    assert_eq!(hit["document"]["rel_hdr"], json!(["dv"]));
    assert_eq!(hit["document"]["rel_group"], "XYZ");
    assert!(
        hit["highlights"]["title"].as_str().unwrap().contains('\u{E000}'),
        "{hit}"
    );
    assert!(hit["document"].get("vis_level").is_some());

    // 结构化过滤、排序、facet、游标
    let r = search(
        &app,
        &alice,
        json!({ "index": "torrents",
        "filters": [ { "field": "rel_video_codec", "op": "eq", "values": ["hevc"] } ],
        "sort": [ { "field": "size", "descending": true } ], "facets": ["rel_resolution"] }),
    )
    .await;
    let order: Vec<i64> = r.body["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["id"].as_i64().unwrap())
        .collect();
    assert_eq!(order, vec![t3, t1], "{:?}", r.body);
    assert_eq!(
        r.body["facets"][0]["buckets"][0],
        json!({ "value": "2160p", "count": 2 })
    );
    let r = search(
        &app,
        &alice,
        json!({ "index": "torrents", "sort": [ { "field": "id", "descending": false } ], "limit": 2 }),
    )
    .await;
    assert_eq!(r.body["hits"].as_array().unwrap().len(), 2);
    let cursor = r.body["next_cursor"].as_str().unwrap().to_owned();
    let r2 = search(&app, &alice, json!({ "index": "torrents", "sort": [ { "field": "id", "descending": false } ], "limit": 2, "cursor": cursor })).await;
    assert_eq!(ids(&r2), vec![t3]);
    assert_eq!(r2.body["next_cursor"], Value::Null);
    let r3 = search(
        &app,
        &alice,
        json!({ "index": "torrents", "limit": 2, "cursor": cursor }),
    )
    .await;
    assert_eq!(r3.status, StatusCode::BAD_REQUEST);
    // GET 形式
    let r = call(
        &app,
        "GET",
        "/api/p/search-meilisearch/v1/search?index=torrents&rel_resolution=1080p&sort=-created_at",
        None,
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(ids(&r), vec![t2], "{:?}", r.body);
    // 错误
    let r = search(
        &app,
        &alice,
        json!({ "index": "torrents", "filters": [ { "field": "title", "op": "eq", "values": ["x"] } ] }),
    )
    .await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("search-meilisearch.field_not_filterable")
        )
    );
    let r = search(
        &app,
        &alice,
        json!({ "index": "torrents", "sort": [ { "field": "title" } ] }),
    )
    .await;
    assert_eq!(r.body["code"], "search-meilisearch.invalid_sort");
    let r = search(&app, &alice, json!({ "index": "nope" })).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    // 缓存：相同请求第二次命中
    let q = json!({ "index": "torrents", "query": "show" });
    assert_eq!(search(&app, &alice, q.clone()).await.body["cached"], false);
    assert_eq!(search(&app, &alice, q).await.body["cached"], true);

    // 联想与 schema
    let r = call(
        &app,
        "GET",
        "/api/p/search-meilisearch/v1/suggest?index=torrents&prefix=some",
        None,
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(r.body["items"][0]["id"], t2, "{:?}", r.body);
    let r = call(
        &app,
        "GET",
        "/api/p/search-meilisearch/v1/indexes/torrents/schema",
        None,
        &hv(&alice.headers()),
    )
    .await;
    let fields = r.body["fields"].as_array().unwrap();
    assert!(
        fields
            .iter()
            .any(|f| f["name"] == "rel_resolution" && f["generic"] == true && f["facetable"] == true)
    );
    assert!(
        fields
            .iter()
            .any(|f| f["name"] == "title" && f["searchable"] == true)
    );

    // 可见性：t2 变为待审 → 只有上传者 bob 与管理员（moderator 权限 restricted.read）可见
    sqlx::query("UPDATE torrent.torrents SET status = 'pending' WHERE id = $1")
        .bind(t2)
        .execute(&mut db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO search_meilisearch.pending_ops (index_name, entity_id, op, lane, enqueued_at) VALUES ('torrents', $1, 'upsert', 'priority', now() - interval '1 minute')")
        .bind(t2)
        .execute(&mut db)
        .await
        .unwrap();
    kernel
        .run_job_now("search-meilisearch", "flush_sweeper", json!({}))
        .await
        .unwrap();
    wait_for("pending torrent hidden", || async {
        !ids(&search(&app, &alice, json!({ "index": "torrents" })).await).contains(&t2)
    })
    .await;
    assert!(ids(&search(&app, &bob, json!({ "index": "torrents" })).await).contains(&t2));
    assert!(ids(&search(&app, &admin, json!({ "index": "torrents" })).await).contains(&t2));
    // 拒绝 → 从索引删除
    sqlx::query("UPDATE torrent.torrents SET status = 'rejected' WHERE id = $1")
        .bind(t2)
        .execute(&mut db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO search_meilisearch.pending_ops (index_name, entity_id, op, lane, enqueued_at) VALUES ('torrents', $1, 'upsert', 'priority', now() - interval '1 minute')")
        .bind(t2)
        .execute(&mut db)
        .await
        .unwrap();
    kernel
        .run_job_now("search-meilisearch", "flush_sweeper", json!({}))
        .await
        .unwrap();
    wait_for("rejected torrent removed", || async {
        !ids(&search(&app, &admin, json!({ "index": "torrents" })).await).contains(&t2)
    })
    .await;

    // 手动重建：普通用户无权；管理员 202，完成后代数 +1，结果不变
    let mut h = alice.headers();
    h.push(("idempotency-key", "rb0".into()));
    let r = call(
        &app,
        "POST",
        "/api/p/search-meilisearch/v1/admin/indexes/torrents/rebuilds",
        Some(json!({})),
        &hv(&h),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let gen_before = get_admin_index().await["generation"].as_i64().unwrap();
    let mut h = admin.headers();
    h.push(("idempotency-key", "rb1".into()));
    let r = call(
        &app,
        "POST",
        "/api/p/search-meilisearch/v1/admin/indexes/torrents/rebuilds",
        Some(json!({ "reason": "test" })),
        &hv(&h),
    )
    .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{:?}", r.body);
    let rid = r.body["id"].as_str().unwrap().to_owned();
    wait_for("manual rebuild", || async {
        let r = call(
            &app,
            "GET",
            &format!("/api/p/search-meilisearch/v1/admin/rebuilds/{rid}"),
            None,
            &hv(&admin.headers()),
        )
        .await;
        r.body["state"] == "done"
    })
    .await;
    assert_eq!(
        get_admin_index().await["generation"].as_i64().unwrap(),
        gen_before + 1
    );
    assert_eq!(
        ids(&search(&app, &alice, json!({ "index": "torrents" })).await),
        vec![t1, t3]
    );
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM kernel.event_log WHERE event_type = 'search.index_rebuilt'")
            .fetch_one(&mut db)
            .await
            .unwrap();
    assert_eq!(n, 2);

    kernel.shutdown().await;
}
