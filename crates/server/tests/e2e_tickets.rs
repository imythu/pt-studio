//! 端到端：联系管理组（tickets，WASM）与群发公告（messages）—— 提交工单、数量上限、员工队列、回复与自动认领、内部备注不对用户可见、
//! 状态流转、关闭与重新打开、他人不可见；公告发布、按等级投放、未读数与已读游标、隐藏、撤回。
//!
//! 运行：`cargo test -p pt-server --test e2e_tickets -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{User, call, env, grant_role, hv, register_login, reset};

async fn db() -> PgConnection {
    PgConnection::connect(&env("PT_TEST_SUPERUSER_URL", "postgres://postgres@127.0.0.1:55432/pt_test")).await.unwrap()
}

async fn get(app: &axum::Router, u: &User, path: &str) -> common::Resp {
    call(app, "GET", path, None, &hv(&u.headers())).await
}

#[tokio::test]
async fn tickets_and_broadcasts_end_to_end() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!("\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n", dir.display()));
    let kernel = Kernel::start(SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let alice = register_login(&app, "tk_alice").await;
    let bob = register_login(&app, "tk_bob").await;
    let staff = register_login(&app, "tk_staff").await;
    grant_role(staff.id, "moderator").await;
    let post = |u: &User, path: String, body: Value| {
        let app = app.clone();
        let h = u.headers();
        async move { call(&app, "POST", &path, Some(body), &hv(&h)).await }
    };

    // 提交：校验、数量上限（默认 3）
    assert_eq!(post(&alice, "/api/p/tickets/v1/tickets".into(), json!({ "category": "nope", "subject": "x", "body": "y" })).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = post(&alice, "/api/p/tickets/v1/tickets".into(), json!({ "category": "account", "subject": "无法登录", "body": "提示密码错误" })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let tid = r.body["id"].as_i64().unwrap();
    for i in 0..2 {
        post(&alice, "/api/p/tickets/v1/tickets".into(), json!({ "category": "other", "subject": format!("t{i}"), "body": "b" })).await;
    }
    assert_eq!(post(&alice, "/api/p/tickets/v1/tickets".into(), json!({ "category": "other", "subject": "t4", "body": "b" })).await.body["code"], "tickets.too_many_open");

    // 他人不可见；普通用户进不了队列
    assert_eq!(get(&app, &bob, &format!("/api/p/tickets/v1/tickets/{tid}")).await.status, StatusCode::NOT_FOUND);
    assert_eq!(get(&app, &bob, "/api/p/tickets/v1/admin/tickets").await.status, StatusCode::FORBIDDEN);
    let q = get(&app, &staff, "/api/p/tickets/v1/admin/tickets?state=open").await.body;
    assert_eq!((q["items"].as_array().unwrap().len(), q["counts"]["open"].as_i64()), (3, Some(3)), "{q:?}");

    // 员工内部备注：不改变状态，用户看不到
    let r = post(&staff, format!("/api/p/tickets/v1/tickets/{tid}/messages"), json!({ "body": "疑似撞库", "internal": true })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let d = get(&app, &alice, &format!("/api/p/tickets/v1/tickets/{tid}")).await.body;
    assert_eq!((d["messages"].as_array().unwrap().len(), d["state"].clone()), (1, json!("open")), "{d:?}");
    // 员工回复：等待用户、自动认领、用户端不显示员工名
    post(&staff, format!("/api/p/tickets/v1/tickets/{tid}/messages"), json!({ "body": "已为你重置密码" })).await;
    let d = get(&app, &alice, &format!("/api/p/tickets/v1/tickets/{tid}")).await.body;
    assert_eq!(d["state"], "pending_user");
    assert_eq!((d["messages"][1]["staff"].clone(), d["messages"][1]["author"].clone()), (json!(true), Value::Null), "{d:?}");
    let ds = get(&app, &staff, &format!("/api/p/tickets/v1/tickets/{tid}")).await.body;
    assert_eq!((ds["messages"].as_array().unwrap().len(), ds["assignee"].clone()), (3, json!("tk_staff")), "{ds:?}");
    let notified: i64 = sqlx::query_scalar("SELECT count(*) FROM notify.notifications WHERE user_id = $1 AND template_key = 'tickets.update'")
        .bind(i64::try_from(alice.id).unwrap())
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert_eq!(notified, 1);
    // 用户回复回到待处理；只看我认领的
    post(&alice, format!("/api/p/tickets/v1/tickets/{tid}/messages"), json!({ "body": "可以登录了，谢谢" })).await;
    let mine = get(&app, &staff, "/api/p/tickets/v1/admin/tickets?state=open&mine=1").await.body;
    assert_eq!(mine["items"][0]["id"].as_i64(), Some(tid), "{mine:?}");
    assert_eq!(mine["items"][0]["last_reply_staff"], false);

    // 关闭后不能回复，可以重新打开
    assert_eq!(post(&alice, format!("/api/p/tickets/v1/tickets/{tid}/close"), json!({})).await.status, StatusCode::NO_CONTENT);
    assert_eq!(post(&alice, format!("/api/p/tickets/v1/tickets/{tid}/messages"), json!({ "body": "x" })).await.body["code"], "tickets.closed");
    assert_eq!(post(&alice, format!("/api/p/tickets/v1/tickets/{tid}/reopen"), json!({})).await.status, StatusCode::NO_CONTENT);
    assert_eq!(get(&app, &alice, &format!("/api/p/tickets/v1/tickets/{tid}")).await.body["state"], "open");
    // 指派与取消认领
    assert_eq!(post(&staff, format!("/api/p/tickets/v1/admin/tickets/{tid}/assign"), json!({ "user_id": bob.id })).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(post(&staff, format!("/api/p/tickets/v1/admin/tickets/{tid}/assign"), json!({ "user_id": 0 })).await.status, StatusCode::NO_CONTENT);
    assert_eq!(get(&app, &staff, &format!("/api/p/tickets/v1/tickets/{tid}")).await.body["assignee"], Value::Null);
    // 员工关闭：通知用户、审计
    post(&staff, format!("/api/p/tickets/v1/tickets/{tid}/close"), json!({})).await;
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM kernel.audit_log WHERE action LIKE 'tickets.%'").fetch_one(&mut db().await).await.unwrap();
    assert!(audit >= 2, "{audit}");

    // ---------------- 群发公告 ----------------
    let unread = |u: &User| {
        let app = app.clone();
        let h = u.headers();
        async move { call(&app, "GET", "/api/p/messages/v1/unread", None, &hv(&h)).await.body["broadcasts"].as_i64().unwrap() }
    };
    assert_eq!(post(&alice, "/api/p/messages/v1/admin/broadcasts".into(), json!({ "subject": "x", "body": "y" })).await.status, StatusCode::FORBIDDEN);
    let r = post(&staff, "/api/p/messages/v1/admin/broadcasts".into(), json!({ "subject": "维护通知", "body": "今晚 **22:00** 停机维护" })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let b1 = r.body["id"].as_i64().unwrap();
    // 按等级投放：bob 等级不够看不到
    sqlx::query("UPDATE identity.users SET class_id = CASE WHEN id = $1 THEN 50 ELSE 1 END WHERE id IN ($1, $2)")
        .bind(i64::try_from(alice.id).unwrap())
        .bind(i64::try_from(bob.id).unwrap())
        .execute(&mut db().await)
        .await
        .unwrap();
    post(&staff, "/api/p/messages/v1/admin/broadcasts".into(), json!({ "subject": "高级用户福利", "body": "b", "min_class": 50, "expires_days": 7 })).await;
    assert_eq!((unread(&alice).await, unread(&bob).await), (2, 1));
    let list = get(&app, &alice, "/api/p/messages/v1/broadcasts").await.body;
    assert!(list["items"][1]["html"].as_str().unwrap().contains("<strong>22:00</strong>"), "{list:?}");
    // 已读游标
    assert_eq!(post(&alice, "/api/p/messages/v1/broadcasts/read".into(), json!({})).await.status, StatusCode::NO_CONTENT);
    assert_eq!(unread(&alice).await, 0);
    assert_eq!(get(&app, &alice, "/api/p/messages/v1/broadcasts").await.body["items"][0]["unread"], false);
    // 隐藏
    call(&app, "DELETE", &format!("/api/p/messages/v1/broadcasts/{b1}"), None, &hv(&alice.headers())).await;
    assert_eq!(get(&app, &alice, "/api/p/messages/v1/broadcasts").await.body["items"].as_array().unwrap().len(), 1);
    // 撤回：bob 的未读消失
    assert_eq!(post(&staff, format!("/api/p/messages/v1/admin/broadcasts/{b1}/revoke"), json!({})).await.status, StatusCode::NO_CONTENT);
    assert_eq!(unread(&bob).await, 0);
    let admin_list = get(&app, &staff, "/api/p/messages/v1/admin/broadcasts").await.body;
    assert_eq!((admin_list["items"][1]["revoked"].clone(), admin_list["items"][0]["read"].clone()), (json!(true), json!(1)), "{admin_list:?}");
}
