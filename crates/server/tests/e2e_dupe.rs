//! 端到端：去重与替代（dupe，WASM）—— 内容相同拒绝、同作品同规格进入审核队列 / 直接拒绝、替代申请与批准（旧种子下架、通知）、驳回。
//!
//! 运行：`cargo test -p pt-server --test e2e_dupe -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use axum::{body::Body, http::StatusCode};
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{User, call, env, grant_role, hv, register_login, reset, send};

async fn db() -> PgConnection {
    PgConnection::connect(&env("PT_TEST_SUPERUSER_URL", "postgres://postgres@127.0.0.1:55432/pt_test")).await.unwrap()
}

async fn post_torrent(app: &axum::Router, u: &User, meta: Value, file: Vec<u8>) -> common::Resp {
    let boundary = "b0undary-d";
    let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"meta\"\r\n\r\n{meta}\r\n").into_bytes();
    body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"torrent\"; filename=\"t.torrent\"\r\n\r\n").as_bytes());
    body.extend(&file);
    body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    send(app, "POST", "/api/p/torrent/v1/torrents", &hv(&u.headers()), |b| {
        b.header("content-type", format!("multipart/form-data; boundary={boundary}")).body(Body::from(body)).unwrap()
    })
    .await
}

async fn movie(app: &axum::Router, u: &User, title: &str, name: &str) -> common::Resp {
    let file = pt_plugin_torrent::fixtures::sample_v1(name, &[("m.mkv", 300 << 20)], true);
    post_torrent(app, u, json!({ "type_id": "movie", "title": title, "fields": { "imdb_id": "tt0133093" } }), file).await
}

async fn status_of(tid: i64) -> String {
    sqlx::query_scalar("SELECT status FROM torrent.torrents WHERE id = $1").bind(tid).fetch_one(&mut db().await).await.unwrap()
}

#[tokio::test]
async fn dupe_end_to_end() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!("\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n", dir.display()));
    let kernel = Kernel::start(SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let alice = register_login(&app, "dupe_alice").await;
    let bob = register_login(&app, "dupe_bob").await;
    let carol = register_login(&app, "dupe_carol").await;
    let staff = register_login(&app, "dupe_staff").await;
    grant_role(staff.id, "moderator").await;

    let r = movie(&app, &alice, "The.Matrix.1999.2160p.BluRay.x265-AAA", "Matrix.One").await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let (a, gid) = (r.body["id"].as_i64().unwrap(), r.body["group_id"].as_i64().unwrap());
    assert_eq!(status_of(a).await, "approved");

    // 内容相同（只改了种子名，piece 一致）→ 拒绝
    let same = pt_plugin_torrent::fixtures::sample_v1("Matrix.One", &[("m.mkv", 300 << 20)], true);
    let renamed: Vec<u8> = {
        let (needle, repl) = (b"10:Matrix.One".as_slice(), b"10:Matrix.Uno".as_slice());
        let i = same.windows(needle.len()).position(|w| w == needle).unwrap();
        [&same[..i], repl, &same[i + needle.len()..]].concat()
    };
    let r = post_torrent(&app, &bob, json!({ "type_id": "movie", "title": "The.Matrix.1999.2160p.BluRay.x265-ZZZ", "fields": { "imdb_id": "tt0133093" } }), renamed).await;
    assert_eq!((r.status, r.body["code"].clone()), (StatusCode::CONFLICT, json!("dupe.same_content")), "{:?}", r.body);
    assert_eq!(r.body["details"]["torrent_id"], json!(a), "{:?}", r.body);

    // review（默认）：同规格的进入审核队列，不同规格的不进
    let r = movie(&app, &bob, "The.Matrix.1999.2160p.BluRay.x265-BBB", "Matrix.Two").await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let b = r.body["id"].as_i64().unwrap();
    assert_eq!(r.body["group_id"].as_i64(), Some(gid));
    let c = movie(&app, &bob, "The.Matrix.1999.1080p.WEB-DL.H264-CCC", "Matrix.Three").await.body["id"].as_i64().unwrap();
    let mut q = Value::Null;
    for _ in 0..50 {
        q = call(&app, "GET", "/api/p/dupe/v1/admin/queue", None, &hv(&staff.headers())).await.body;
        if !q["candidates"].as_array().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let cands = q["candidates"].as_array().unwrap();
    assert_eq!(cands.len(), 1, "{q:?}");
    assert_eq!((cands[0]["torrent_id"].clone(), cands[0]["matches"].clone()), (json!(b), json!([a])));
    assert_eq!(call(&app, "GET", "/api/p/dupe/v1/admin/queue", None, &hv(&bob.headers())).await.status, StatusCode::FORBIDDEN);

    // 种子页信息：同规格版本、可申请替代
    let info = call(&app, "GET", &format!("/api/p/dupe/v1/torrents/{b}"), None, &hv(&bob.headers())).await.body;
    assert_eq!(info["slot_peers"][0]["torrent_id"], json!(a), "{info:?}");
    assert_eq!(info["can_request"], true);
    let ids: Vec<i64> = info["candidates"].as_array().unwrap().iter().filter_map(|x| x["torrent_id"].as_i64()).collect();
    assert!(ids.contains(&a) && ids.contains(&c), "{ids:?}");
    assert_eq!(call(&app, "GET", &format!("/api/p/dupe/v1/torrents/{b}"), None, &hv(&carol.headers())).await.body["can_request"], false);

    // 替代申请：只有新种子的发布者或管理员；重复申请冲突
    let req = |u: &User, body: Value| {
        let app = app.clone();
        let h = u.headers();
        async move { call(&app, "POST", "/api/p/dupe/v1/trumps", Some(body), &hv(&h)).await }
    };
    assert_eq!(req(&alice, json!({ "old_id": a, "new_id": b, "reason": "x" })).await.status, StatusCode::FORBIDDEN);
    assert_eq!(req(&bob, json!({ "old_id": a, "new_id": b, "reason": "" })).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = req(&bob, json!({ "old_id": a, "new_id": b, "reason": "修正音画不同步" })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let trump = r.body["id"].as_i64().unwrap();
    assert_eq!(req(&bob, json!({ "old_id": a, "new_id": b, "reason": "again" })).await.body["code"], "dupe.already_requested");
    let info = call(&app, "GET", &format!("/api/p/dupe/v1/torrents/{a}"), None, &hv(&carol.headers())).await.body;
    assert_eq!((info["trumped_by"]["torrent_id"].clone(), info["trumped_by"]["state"].clone()), (json!(b), json!("pending")), "{info:?}");

    // 批准：旧种子下架、下载过的用户收到通知、审核队列清空
    sqlx::query(
        "INSERT INTO accounting.snatches (user_id, torrent_id, first_action_at, last_action_at, completed_at) VALUES ($1, $2, now(), now(), now())",
    )
    .bind(i64::try_from(carol.id).unwrap())
    .bind(a)
    .execute(&mut db().await)
    .await
    .unwrap();
    let r = call(&app, "POST", &format!("/api/p/dupe/v1/admin/trumps/{trump}/decision"), Some(json!({ "decision": "approve" })), &hv(&staff.headers())).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    assert_eq!(status_of(a).await, "disabled");
    let notified: i32 = sqlx::query_scalar("SELECT notified FROM dupe.trumps WHERE id = $1").bind(trump).fetch_one(&mut db().await).await.unwrap();
    assert_eq!(notified, 1);
    let q = call(&app, "GET", "/api/p/dupe/v1/admin/queue", None, &hv(&staff.headers())).await.body;
    assert!(q["trumps"].as_array().unwrap().is_empty() && q["candidates"].as_array().unwrap().is_empty(), "{q:?}");
    let info = call(&app, "GET", &format!("/api/p/dupe/v1/torrents/{b}"), None, &hv(&carol.headers())).await.body;
    assert_eq!(info["replaces"][0]["torrent_id"], json!(a), "{info:?}");
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM kernel.audit_log WHERE action = 'dupe.trump_approved'").fetch_one(&mut db().await).await.unwrap();
    assert_eq!(audit, 1);

    // 驳回：管理员直接为 C 申请替代 B 后驳回，B 不受影响
    let r = req(&staff, json!({ "old_id": b, "new_id": c, "reason": "test" })).await;
    let t2 = r.body["id"].as_i64().unwrap();
    let r = call(&app, "POST", &format!("/api/p/dupe/v1/admin/trumps/{t2}/decision"), Some(json!({ "decision": "reject", "note": "规格不同" })), &hv(&staff.headers())).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(status_of(b).await, "approved");

    // block：同规格直接拒绝
    sqlx::query("INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('dupe', $1)")
        .bind(json!({ "slot_policy": "block" }))
        .execute(&mut db().await)
        .await
        .unwrap();
    let r = movie(&app, &carol, "The.Matrix.1999.2160p.BluRay.x265-DDD", "Matrix.Four").await;
    assert_eq!((r.status, r.body["code"].clone()), (StatusCode::CONFLICT, json!("dupe.slot_taken")), "{:?}", r.body);
    assert_eq!(r.body["details"]["torrent_ids"], json!([b]));
    assert_eq!(movie(&app, &carol, "The.Matrix.1999.720p.WEB-DL.H264-EEE", "Matrix.Five").await.status, StatusCode::CREATED);
}
