//! 端到端：置顶（torrent）、收藏与合集（bookmarks，WASM）、排行榜（top10，WASM）。
//!
//! 运行：`cargo test -p pt-server --test e2e_collections -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{call, env, grant_role, hv, register_login, reset, upload};

async fn db() -> PgConnection {
    PgConnection::connect(&env("PT_TEST_SUPERUSER_URL", "postgres://postgres@127.0.0.1:55432/pt_test"))
        .await
        .unwrap()
}

async fn exec(q: &str) {
    sqlx::query(q).execute(&mut db().await).await.unwrap();
}

fn board<'a>(b: &'a Value, key: &str, period: &str) -> &'a Value {
    let board = b["boards"].as_array().unwrap().iter().find(|x| x["key"] == key).unwrap();
    let p = board["periods"].as_array().unwrap().iter().find(|x| x["period"] == period).unwrap();
    &p["items"]
}

#[tokio::test]
async fn sticky_bookmarks_collections_top10() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!("\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n", dir.display()));
    let kernel = Kernel::start(SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let alice = register_login(&app, "col_alice").await;
    let bob = register_login(&app, "col_bob").await;
    let staff = register_login(&app, "col_staff").await;
    grant_role(staff.id, "moderator").await;
    let t1 = upload(&app, &alice, "Col.One", &[("a.bin", 1 << 20)]).await;
    let t2 = upload(&app, &alice, "Col.Two", &[("b.bin", 1 << 20)]).await;
    let t3 = upload(&app, &bob, "Col.Three", &[("c.bin", 1 << 20)]).await;

    // ---- 置顶 ----
    let sticky = |tid: i64, body: Value| {
        let app = app.clone();
        let h = staff.headers();
        async move { call(&app, "POST", &format!("/api/p/torrent/v1/admin/torrents/{tid}/sticky"), Some(body), &hv(&h)).await }
    };
    let r = call(&app, "POST", &format!("/api/p/torrent/v1/admin/torrents/{t1}/sticky"), Some(json!({ "sticky": true })), &hv(&alice.headers())).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{:?}", r.body);
    let r = sticky(t1, json!({ "sticky": true, "days": 3 })).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["sticky_until"].is_string());
    let r = sticky(t2, json!({ "sticky": true })).await;
    assert!(r.body["sticky_until"].as_str().unwrap().starts_with("9999"), "{:?}", r.body);
    assert_eq!(sticky(t2, json!({ "sticky": true, "days": 99999 })).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let d = call(&app, "GET", &format!("/api/p/torrent/v1/torrents/{t1}"), None, &hv(&bob.headers())).await.body;
    assert!(d["sticky_until"].is_string(), "{d:?}");
    // 到期后由 unsticky 任务清除
    exec(&format!("UPDATE torrent.torrents SET sticky_until = now() - interval '1 minute' WHERE id = {t1}")).await;
    let d = call(&app, "GET", &format!("/api/p/torrent/v1/torrents/{t1}"), None, &hv(&bob.headers())).await.body;
    assert!(d["sticky_until"].is_null(), "expired sticky is not shown: {d:?}");
    kernel.run_job_now("torrent", "unsticky", json!({})).await.unwrap();
    let left: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar("SELECT sticky_until FROM torrent.torrents WHERE id = $1")
        .bind(t1)
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert!(left.is_none());
    assert!(sticky(t2, json!({ "sticky": false })).await.body["sticky_until"].is_null());

    // ---- 收藏 ----
    let bm = |u: &common::User, method: &'static str, tid: i64| {
        let app = app.clone();
        let h = u.headers();
        async move { call(&app, method, &format!("/api/p/bookmarks/v1/bookmarks/{tid}"), None, &hv(&h)).await }
    };
    assert_eq!(bm(&bob, "PUT", t1).await.status, StatusCode::NO_CONTENT);
    assert_eq!(bm(&bob, "PUT", t1).await.status, StatusCode::NO_CONTENT, "idempotent");
    assert_eq!(bm(&bob, "PUT", t2).await.status, StatusCode::NO_CONTENT);
    assert_eq!(bm(&alice, "PUT", t1).await.status, StatusCode::NO_CONTENT);
    assert_eq!(bm(&bob, "PUT", 999_999).await.status, StatusCode::NOT_FOUND);
    let info = call(&app, "GET", &format!("/api/p/bookmarks/v1/torrents/{t1}"), None, &hv(&bob.headers())).await.body;
    assert_eq!((info["bookmarks"].clone(), info["bookmarked"].clone()), (json!(2), json!(true)), "{info:?}");
    let mine = call(&app, "GET", "/api/p/bookmarks/v1/bookmarks", None, &hv(&bob.headers())).await.body;
    assert_eq!(mine["total"], 2);
    assert_eq!(mine["items"][0]["torrent_id"], json!(t2), "newest first");
    assert_eq!(bm(&bob, "DELETE", t2).await.status, StatusCode::NO_CONTENT);
    sqlx::query("INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('bookmarks', $1)")
        .bind(json!({ "max_bookmarks": 1, "max_items_per_collection": 2 }))
        .execute(&mut db().await)
        .await
        .unwrap();
    assert_eq!(bm(&bob, "PUT", t3).await.body["code"], "bookmarks.limit_reached");

    // ---- 合集 ----
    let r = call(&app, "POST", "/api/p/bookmarks/v1/collections", Some(json!({ "title": "  Best of  ", "description": "picks" })), &hv(&bob.headers())).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let cid = r.body["id"].as_i64().unwrap();
    let base = format!("/api/p/bookmarks/v1/collections/{cid}");
    assert_eq!(call(&app, "POST", "/api/p/bookmarks/v1/collections", Some(json!({ "title": "" })), &hv(&bob.headers())).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let add = |u: &common::User, body: Value| {
        let app = app.clone();
        let h = u.headers();
        let p = format!("{base}/items");
        async move { call(&app, "POST", &p, Some(body), &hv(&h)).await }
    };
    assert_eq!(add(&bob, json!({ "torrent_id": t1, "note": "first" })).await.status, StatusCode::NO_CONTENT);
    assert_eq!(add(&bob, json!({ "torrent_id": t3 })).await.status, StatusCode::NO_CONTENT);
    assert_eq!(add(&bob, json!({ "torrent_id": t2 })).await.body["code"], "bookmarks.collection_full");
    assert_eq!(add(&alice, json!({ "torrent_id": t2 })).await.status, StatusCode::FORBIDDEN, "only the owner edits");
    let c = call(&app, "GET", &base, None, &hv(&alice.headers())).await.body;
    assert_eq!((c["title"].clone(), c["item_count"].clone(), c["editable"].clone(), c["owner"].clone()), (json!("Best of"), json!(2), json!(false), json!("col_bob")), "{c:?}");
    assert_eq!(c["items"][0]["note"], "first");
    // 调整顺序
    let r = call(&app, "PUT", &format!("{base}/order"), Some(json!({ "torrent_ids": [t3, t1] })), &hv(&bob.headers())).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    let c = call(&app, "GET", &base, None, &hv(&bob.headers())).await.body;
    assert_eq!((c["items"][0]["torrent_id"].clone(), c["items"][1]["torrent_id"].clone()), (json!(t3), json!(t1)));
    // 种子页列出合集；公开列表与搜索
    let info = call(&app, "GET", &format!("/api/p/bookmarks/v1/torrents/{t3}"), None, &hv(&alice.headers())).await.body;
    assert_eq!(info["collections"][0]["id"], json!(cid), "{info:?}");
    let list = call(&app, "GET", "/api/p/bookmarks/v1/collections?q=best", None, &hv(&alice.headers())).await.body;
    assert_eq!(list["total"], 1, "{list:?}");
    // 私有后其他人看不到；管理员可以
    let r = call(&app, "PATCH", &base, Some(json!({ "is_public": false })), &hv(&bob.headers())).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    assert_eq!(call(&app, "GET", &base, None, &hv(&alice.headers())).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "GET", "/api/p/bookmarks/v1/collections", None, &hv(&alice.headers())).await.body["total"], 0);
    let c = call(&app, "GET", &base, None, &hv(&staff.headers())).await.body;
    assert_eq!(c["editable"], true, "{c:?}");
    let mine = call(&app, "GET", "/api/p/bookmarks/v1/collections?mine=1", None, &hv(&bob.headers())).await.body;
    assert_eq!(mine["total"], 1);
    // 移出
    let r = call(&app, "DELETE", &format!("{base}/items/{t1}"), None, &hv(&bob.headers())).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(call(&app, "DELETE", &format!("{base}/items/{t1}"), None, &hv(&bob.headers())).await.status, StatusCode::NOT_FOUND);

    // 种子删除：从收藏与合集中移除
    let r = call(&app, "DELETE", &format!("/api/p/torrent/v1/admin/torrents/{t3}?reason=dupe"), None, &hv(&staff.headers())).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let mut ok = false;
    for _ in 0..50 {
        let n: i32 = sqlx::query_scalar("SELECT item_count FROM bookmarks.collections WHERE id = $1")
            .bind(cid)
            .fetch_one(&mut db().await)
            .await
            .unwrap();
        if n == 0 {
            ok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(ok, "deleted torrents leave collections");

    // ---- 排行榜 ----
    for (uid, tid) in [(bob.id, t1), (staff.id, t1), (bob.id, t2)] {
        sqlx::query(
            "INSERT INTO accounting.snatches (user_id, torrent_id, first_action_at, last_action_at, completed_at)
             VALUES ($1, $2, now() - interval '2 days', now(), now() - interval '2 days')",
        )
        .bind(i64::try_from(uid).unwrap())
        .bind(tid)
        .execute(&mut db().await)
        .await
        .unwrap();
    }
    for (uid, up) in [(alice.id, 5_000_i64), (bob.id, 9_000)] {
        sqlx::query("INSERT INTO accounting.user_daily (day, user_id, uploaded, seed_secs) VALUES (current_date - 1, $1, $2, $2)")
            .bind(i64::try_from(uid).unwrap())
            .bind(up)
            .execute(&mut db().await)
            .await
            .unwrap();
    }
    // 启动时的 catch_up 已经跑过一次（数据为空），这里手动重算
    kernel.run_job_now("top10", "refresh", json!({})).await.unwrap();
    let b = call(&app, "GET", "/api/p/top10/v1/boards", None, &hv(&alice.headers())).await.body;
    assert!(b["refreshed_at"].is_i64(), "{b:?}");
    let snatched = board(&b, "torrents.snatched", "week");
    assert_eq!((snatched[0]["id"].clone(), snatched[0]["value"].clone(), snatched[0]["name"].clone()), (json!(t1), json!(2), json!("Col.One")), "{b:?}");
    assert!(board(&b, "torrents.snatched", "day").as_array().unwrap().is_empty());
    let up = board(&b, "users.uploaded", "week");
    assert_eq!((up[0]["name"].clone(), up[1]["name"].clone()), (json!("col_bob"), json!("col_alice")), "{up:?}");
    let uploads = board(&b, "users.uploads", "all");
    assert_eq!((uploads[0]["name"].clone(), uploads[0]["value"].clone()), (json!("col_alice"), json!(2)), "{uploads:?}");
    // 退出用户榜立即生效
    let r = call(&app, "PUT", "/api/p/top10/v1/me", Some(json!({ "opted_out": true })), &hv(&bob.headers())).await;
    assert_eq!(r.body["opted_out"], true);
    let b = call(&app, "GET", "/api/p/top10/v1/boards", None, &hv(&alice.headers())).await.body;
    let up = board(&b, "users.uploaded", "week");
    assert_eq!(up.as_array().unwrap().len(), 1, "{up:?}");
    assert_eq!((up[0]["name"].clone(), up[0]["rank"].clone()), (json!("col_alice"), json!(1)));
    // 关闭用户榜
    exec("INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('top10', '{\"user_boards\": false}')").await;
    let b = call(&app, "GET", "/api/p/top10/v1/boards", None, &hv(&alice.headers())).await.body;
    assert!(b["boards"].as_array().unwrap().iter().all(|x| x["kind"] == "torrent"), "{b:?}");
}
