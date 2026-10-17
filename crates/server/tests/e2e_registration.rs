//! 端到端：注册策略（registration-policy，WASM）—— 同 IP 上限、邮箱域名黑名单、全站上限、开放注册关闭后只能邀请、
//! 限时开放窗口与名额、开放注册审核（待审核 → 管理员解除限制）、公开状态。
//!
//! 运行：`cargo test -p pt-server --test e2e_registration -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use std::net::SocketAddr;

use axum::{Router, body::Body, extract::ConnectInfo, http::StatusCode};
use pt_kernel::Kernel;
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{Resp, env, reset, send};

async fn req(app: &Router, method: &str, uri: &str, ip: &str, headers: &[(&str, &str)], body: Option<Value>) -> Resp {
    let addr: SocketAddr = format!("{ip}:40000").parse().unwrap();
    send(app, method, uri, headers, move |b| {
        let b = b.extension(ConnectInfo(addr));
        match body {
            Some(v) => b.header("content-type", "application/json").body(Body::from(v.to_string())).unwrap(),
            None => b.body(Body::empty()).unwrap(),
        }
    })
    .await
}

async fn settings(v: Value) {
    let mut c = PgConnection::connect(&env("PT_TEST_SUPERUSER_URL", "postgres://postgres@127.0.0.1:55432/pt_test"))
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('registration-policy', $1)
         ON CONFLICT (plugin_id) DO UPDATE SET settings = EXCLUDED.settings, version = kernel.plugin_settings.version + 1",
    )
    .bind(v)
    .execute(&mut c)
    .await
    .unwrap();
}

async fn register(app: &Router, ip: &str, name: &str, domain: &str) -> Resp {
    req(app, "POST", "/api/p/identity/v1/users", ip, &[],
        Some(json!({ "username": name, "email": format!("{name}@{domain}"), "password": "correct horse" }))).await
}

#[tokio::test]
async fn registration_policy_end_to_end() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!("\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n", dir.display()));
    let kernel = Kernel::start(pt_kernel::config::SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();

    // 管理员（第一个用户）
    let r = register(&app, "10.9.0.1", "root_admin", "example.com").await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    common::grant_role(1, "admin").await;
    let r = req(&app, "POST", "/api/p/identity/v1/sessions", "10.9.0.1", &[], Some(json!({ "username": "root_admin", "password": "correct horse" }))).await;
    let admin = [("cookie", r.cookies.join("; ")), ("x-csrf-token", r.body["csrf_token"].as_str().unwrap().to_owned()), ("sec-fetch-site", "same-origin".into())];
    let admin: Vec<(&str, &str)> = admin.iter().map(|(k, v)| (*k, v.as_str())).collect();

    // 同一 IP 24 小时内最多 2 个；邮箱域名黑名单（后缀规则）
    settings(json!({ "per_ip_max": 2, "email_domain_deny": ["*.trash.test", "mailinator.com"] })).await;
    assert_eq!(register(&app, "10.1.1.1", "ip_one", "example.com").await.status, StatusCode::CREATED);
    assert_eq!(register(&app, "10.1.1.1", "ip_two", "example.com").await.status, StatusCode::CREATED);
    let r = register(&app, "10.1.1.1", "ip_three", "example.com").await;
    assert_eq!((r.status, r.body["code"].clone()), (StatusCode::TOO_MANY_REQUESTS, json!("registration-policy.ip_limit_reached")), "{:?}", r.body);
    let r = register(&app, "10.1.1.2", "spammer", "x.trash.test").await;
    assert_eq!(r.body["code"], "registration-policy.email_domain_denied", "{:?}", r.body);
    let r = register(&app, "10.1.1.2", "spammer", "mailinator.com").await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);

    // 全站上限：现有 3 人，上限 4
    settings(json!({ "max_users": 4 })).await;
    assert_eq!(register(&app, "10.1.2.1", "cap_ok", "example.com").await.status, StatusCode::CREATED);
    let r = register(&app, "10.1.2.2", "cap_full", "example.com").await;
    assert_eq!((r.body["code"].clone(), r.body["details"]["reason"].clone()), (json!("registration-policy.closed"), json!("cap_reached")), "{:?}", r.body);

    // 关闭开放注册：只能邀请；状态接口
    settings(json!({ "open_enabled": false })).await;
    let r = register(&app, "10.1.3.1", "late", "example.com").await;
    assert_eq!((r.status, r.body["code"].clone()), (StatusCode::FORBIDDEN, json!("registration-policy.invite_required")), "{:?}", r.body);
    let r = req(&app, "GET", "/api/p/registration-policy/v1/status", "10.1.3.1", &[], None).await;
    assert_eq!((r.body["open"].clone(), r.body["invite"].clone()), (json!(false), json!(true)), "{:?}", r.body);
    settings(json!({ "open_enabled": false, "invite_enabled": false, "closed_message": "下次开放请关注公告" })).await;
    let r = register(&app, "10.1.3.1", "late", "example.com").await;
    assert_eq!(r.body["code"], "registration-policy.closed", "{:?}", r.body);

    // 限时开放窗口：1 个名额
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as i64;
    let r = req(&app, "POST", "/api/p/registration-policy/v1/admin/windows", "10.9.0.1", &admin,
        Some(json!({ "starts_at": now - 60_000, "ends_at": now + 3_600_000, "slots": 1, "note": "周年庆" }))).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let r = req(&app, "POST", "/api/p/registration-policy/v1/admin/windows", "10.9.0.1", &admin,
        Some(json!({ "starts_at": now, "ends_at": now + 60_000 }))).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "overlap: {:?}", r.body);
    let r = req(&app, "GET", "/api/p/registration-policy/v1/status", "10.1.4.1", &[], None).await;
    assert_eq!((r.body["open"].clone(), r.body["window"]["slots_left"].clone()), (json!(true), json!(1)), "{:?}", r.body);
    assert_eq!(register(&app, "10.1.4.1", "lucky", "example.com").await.status, StatusCode::CREATED);
    let r = register(&app, "10.1.4.2", "unlucky", "example.com").await;
    assert_eq!(r.body["details"]["reason"], "slots_exhausted", "{:?}", r.body);
    let r = req(&app, "GET", "/api/p/registration-policy/v1/admin/windows", "10.9.0.1", &admin, None).await;
    assert_eq!(r.body["items"][0]["note"], "周年庆", "{:?}", r.body);

    // 开放注册审核：新用户待审核，管理员解除限制后激活
    settings(json!({ "open_review": true })).await;
    let r = register(&app, "10.1.5.1", "reviewed", "example.com").await;
    assert_eq!((r.status, r.body["user"]["status"].clone()), (StatusCode::CREATED, json!("pending")), "{:?}", r.body);
    let uid = r.body["user"]["id"].as_i64().unwrap();
    let r = req(&app, "GET", &format!("/api/p/identity/v1/admin/users/{uid}/restrictions"), "10.9.0.1", &admin, None).await;
    let rid = r.body.as_array().or(r.body["items"].as_array()).unwrap()
        .iter().find(|x| x["kind"] == "pending" && x["lifted_at"].is_null()).unwrap()["id"].as_i64().unwrap();
    let r = req(&app, "DELETE", &format!("/api/p/identity/v1/admin/users/{uid}/restrictions/{rid}"), "10.9.0.1", &admin, None).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    let r = req(&app, "GET", &format!("/api/p/identity/v1/users/{uid}"), "10.9.0.1", &admin, None).await;
    assert_eq!(r.body["status"], "active", "{:?}", r.body);
}
