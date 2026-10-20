//! 端到端：论坛（forum，WASM）—— 版块权限（读 / 发主题按等级 ID）、发主题与投票、引用回复、未读标记、感谢、编辑与历史、
//! 版主操作（锁定、置顶、公告、移动、删除主题）、订阅汇总通知。
//!
//! 运行：`cargo test -p pt-server --test e2e_forum -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{User, call, env, hv, register_login, reset};

async fn db() -> PgConnection {
    PgConnection::connect(&env("PT_TEST_SUPERUSER_URL", "postgres://postgres@127.0.0.1:55432/pt_test"))
        .await
        .unwrap()
}

#[tokio::test]
async fn forum_end_to_end() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!("\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n", dir.display()));
    let kernel = Kernel::start(SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let admin = register_login(&app, "forum_admin").await;
    let alice = register_login(&app, "forum_alice").await;
    let bob = register_login(&app, "forum_bob").await;
    common::grant_role(admin.id, "admin").await;
    sqlx::query("INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('forum', $1)")
        .bind(json!({ "digest_minutes": 0, "rate_per_30s": 50 }))
        .execute(&mut db().await)
        .await
        .unwrap();
    let get = |u: &User, path: String| {
        let app = app.clone();
        let h = u.headers();
        async move { call(&app, "GET", &format!("/api/p/forum{path}"), None, &hv(&h)).await }
    };
    let send = |u: &User, method: &'static str, path: String, body: Value| {
        let app = app.clone();
        let h = u.headers();
        async move { call(&app, method, &format!("/api/p/forum{path}"), Some(body), &hv(&h)).await }
    };

    // 后台：分区与三个版块（公告：发主题需等级 5；VIP：阅读需等级 3）
    // 门槛按 alice 当前等级 + 1（levels 插件给新用户的默认等级 ID 不一定是 0）
    let class: i32 = sqlx::query_scalar("SELECT class_id FROM identity.users WHERE username = 'forum_alice'")
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    let above = class + 1;
    let r = send(&admin, "POST", "/v1/admin/categories".into(), json!({ "name": "站务" })).await;
    let cat = r.body["id"].as_i64().unwrap();
    let mk = |name: &str, acl: Value| send(&admin, "POST", "/v1/admin/boards".into(), json!({ "category_id": cat, "name": name, "acl": acl }));
    let notice = mk("公告", json!({ "create": { "min_class": above } })).await.body["id"].as_i64().unwrap();
    let general = mk("综合讨论", json!({})).await.body["id"].as_i64().unwrap();
    let vip = mk("VIP", json!({ "read": { "min_class": above } })).await.body["id"].as_i64().unwrap();
    let r = send(&alice, "POST", "/v1/admin/boards".into(), json!({ "category_id": cat, "name": "x" })).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    let boards = get(&alice, "/v1/boards".into()).await.body;
    // 首次加载预置的分区与版块（站务公告仅管理组可发帖）
    let preset = &boards["categories"][0];
    assert_eq!(preset["name"], "站务", "{boards:?}");
    assert_eq!(preset["boards"][0]["can_create"], json!(false), "{boards:?}");
    let ours = |b: &Value| b["categories"].as_array().unwrap().iter().find(|c| c["id"] == cat).unwrap()["boards"].clone();
    let names: Vec<String> = ours(&boards).as_array().unwrap().iter().map(|b| b["name"].as_str().unwrap().to_owned()).collect();
    assert_eq!(names, vec!["公告", "综合讨论"], "VIP is hidden: {boards:?}");
    assert_eq!(get(&alice, format!("/v1/boards/{vip}/topics")).await.status, StatusCode::NOT_FOUND);

    // 发主题：公告区等级不够；综合区带投票
    let r = send(&alice, "POST", format!("/v1/boards/{notice}/topics"), json!({ "title": "t", "body": "b" })).await;
    assert_eq!(r.body["code"], "forum.permission_denied", "{:?}", r.body);
    let r = send(&alice, "POST", format!("/v1/boards/{general}/topics"), json!({
        "title": "周末看什么", "body": "推荐一下 **电影**", "poll": { "question": "选一个", "options": ["A", "B", "C"], "max_choices": 2 }
    })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let topic = r.body["id"].as_i64().unwrap();
    let first_post = r.body["post_id"].as_i64().unwrap();

    // bob 引用回复；alice 看到版块未读，打开主题后已读
    let r = send(&bob, "POST", format!("/v1/topics/{topic}/posts"), json!({ "body": "我投 B", "quote_of": first_post })).await;
    assert_eq!((r.status, r.body["floor"].clone()), (StatusCode::CREATED, json!(2)), "{:?}", r.body);
    let reply_id = r.body["id"].as_i64().unwrap();
    let unread = |b: &Value| ours(b).as_array().unwrap().iter().find(|x| x["id"] == general).unwrap()["unread"].clone();
    assert_eq!(unread(&get(&alice, "/v1/boards".into()).await.body), json!(true));
    let view = get(&alice, format!("/v1/topics/{topic}")).await.body;
    assert_eq!(view["posts"].as_array().unwrap().len(), 2);
    assert!(view["posts"][0]["html"].as_str().unwrap().contains("<strong>电影</strong>"), "{view:?}");
    assert!(view["posts"][1]["html"].as_str().unwrap().contains("forum_alice"), "quote: {view:?}");
    assert_eq!(unread(&get(&alice, "/v1/boards".into()).await.body), json!(false));

    // 投票（多选上限 2）、改票
    let r = send(&bob, "POST", format!("/v1/topics/{topic}/vote"), json!({ "options": [0, 1, 2] })).await;
    assert_eq!(r.body["code"], "forum.poll_invalid");
    let r = send(&bob, "POST", format!("/v1/topics/{topic}/vote"), json!({ "options": [1] })).await;
    assert_eq!(r.body["counts"], json!([0, 1, 0]), "{:?}", r.body);
    let r = send(&bob, "POST", format!("/v1/topics/{topic}/vote"), json!({ "options": [0, 2] })).await;
    assert_eq!((r.body["counts"].clone(), r.body["my_votes"].clone()), (json!([1, 0, 1]), json!([0, 2])));

    // 感谢；不能感谢自己
    assert_eq!(send(&bob, "POST", format!("/v1/posts/{first_post}/thanks"), json!({})).await.status, StatusCode::OK);
    assert_eq!(send(&alice, "POST", format!("/v1/posts/{first_post}/thanks"), json!({})).await.body["code"], "forum.permission_denied");
    // 编辑：作者可以（含标题），他人不行；版主看历史
    let r = send(&alice, "PATCH", format!("/v1/posts/{first_post}"), json!({ "body": "改过了", "title": "周末看什么（已更新）" })).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(send(&bob, "PATCH", format!("/v1/posts/{first_post}"), json!({ "body": "x" })).await.body["code"], "forum.edit_window_expired");
    let h = get(&admin, format!("/v1/posts/{first_post}/history")).await.body;
    assert_eq!(h["items"][0]["source"], "推荐一下 **电影**");
    let view = get(&bob, format!("/v1/topics/{topic}")).await.body;
    assert_eq!((view["topic"]["title"].clone(), view["posts"][0]["thanks"].clone()), (json!("周末看什么（已更新）"), json!(1)));

    // 订阅汇总：alice 是作者（自动订阅）；她已读到第 2 楼，bob 再回复后才通知
    let r = send(&bob, "POST", format!("/v1/topics/{topic}/posts"), json!({ "body": "再补充一句" })).await;
    assert_eq!(r.body["floor"], 3, "{:?}", r.body);
    kernel.run_job_now("forum", "subs_digest", json!({})).await.unwrap();
    let inbox = call(&app, "GET", "/api/p/notify/v1/inbox", None, &hv(&alice.headers())).await.body;
    assert!(inbox["items"].as_array().unwrap().iter().any(|n| n["template_key"] == "forum.topic_updated"), "{inbox:?}");

    // 版主：锁定后普通用户不能回复；置顶、公告、移动
    let modr = |action: &str, extra: Value| {
        let mut b = json!({ "action": action });
        if let (Some(o), Some(e)) = (b.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        send(&admin, "POST", format!("/v1/topics/{topic}/moderate"), b)
    };
    assert_eq!(send(&bob, "POST", format!("/v1/topics/{topic}/moderate"), json!({ "action": "lock" })).await.status, StatusCode::FORBIDDEN);
    assert_eq!(modr("lock", json!({})).await.status, StatusCode::NO_CONTENT);
    assert_eq!(send(&bob, "POST", format!("/v1/topics/{topic}/posts"), json!({ "body": "还能回吗" })).await.body["code"], "forum.topic_locked");
    assert_eq!(modr("sticky", json!({})).await.status, StatusCode::NO_CONTENT);
    assert_eq!(modr("announce", json!({})).await.status, StatusCode::NO_CONTENT);
    let ann = get(&bob, "/v1/announcements".into()).await.body;
    assert_eq!(ann["items"][0]["id"], topic, "{ann:?}");
    assert_eq!(modr("move", json!({ "to_board": notice })).await.status, StatusCode::NO_CONTENT);
    let list = get(&bob, format!("/v1/boards/{notice}/topics")).await.body;
    assert_eq!((list["topics"][0]["id"].clone(), list["topics"][0]["sticky"].clone()), (json!(topic), json!(true)), "{list:?}");

    // 删除：作者删自己的回复；版主删主楼 = 删除主题
    assert_eq!(call(&app, "DELETE", &format!("/api/p/forum/v1/posts/{reply_id}"), None, &hv(&bob.headers())).await.status, StatusCode::NO_CONTENT);
    assert_eq!(call(&app, "DELETE", &format!("/api/p/forum/v1/posts/{first_post}"), None, &hv(&alice.headers())).await.status, StatusCode::FORBIDDEN);
    assert_eq!(call(&app, "DELETE", &format!("/api/p/forum/v1/posts/{first_post}"), None, &hv(&admin.headers())).await.status, StatusCode::NO_CONTENT);
    assert_eq!(get(&bob, format!("/v1/topics/{topic}")).await.status, StatusCode::NOT_FOUND);
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM kernel.audit_log WHERE action LIKE 'forum.%'")
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert!(audits >= 7, "{audits}");
}
