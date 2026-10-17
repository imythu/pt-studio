//! 端到端：账号安全（identity 03 §1.9）—— 登录限流（指数延迟、账号软锁与已知设备豁免、IP 封锁）、设备 cookie、
//! IP 记录（登录与会话 IP 变化）、本人 IP 历史（截断）、管理员按 IP / CIDR 查账户、共用 IP、安全事件。
//!
//! 运行：`cargo test -p pt-server --test e2e_security -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use std::net::SocketAddr;

use axum::{Router, body::Body, extract::ConnectInfo, http::StatusCode};
use pt_kernel::Kernel;
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{Resp, env, reset, send};

async fn db() -> PgConnection {
    PgConnection::connect(&env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap()
}

/// 以客户端 IP `ip` 发请求（网关从 ConnectInfo 取直连地址）
async fn req(app: &Router, method: &str, uri: &str, ip: &str, headers: &[(&str, &str)], body: Option<Value>) -> Resp {
    let addr: SocketAddr = format!("{ip}:40000").parse().unwrap();
    send(app, method, uri, headers, move |b| {
        let b = b.extension(ConnectInfo(addr));
        match body {
            Some(v) => b
                .header("content-type", "application/json")
                .body(Body::from(v.to_string()))
                .unwrap(),
            None => b.body(Body::empty()).unwrap(),
        }
    })
    .await
}

async fn login(app: &Router, ip: &str, name: &str, pw: &str, cookie: Option<&str>) -> Resp {
    let h: Vec<(&str, &str)> = cookie.map(|c| vec![("cookie", c)]).unwrap_or_default();
    req(
        app,
        "POST",
        "/api/p/identity/v1/sessions",
        ip,
        &h,
        Some(json!({ "username": name, "password": pw })),
    )
    .await
}

fn cookie_named<'a>(r: &'a Resp, name: &str) -> Option<&'a str> {
    r.cookies
        .iter()
        .find(|c| c.starts_with(&format!("{name}=")))
        .map(String::as_str)
}

async fn set_settings(v: Value) {
    sqlx::query(
        "INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('identity', $1)
         ON CONFLICT (plugin_id) DO UPDATE SET settings = EXCLUDED.settings, version = kernel.plugin_settings.version + 1",
    )
    .bind(v)
    .execute(&mut db().await)
    .await
    .unwrap();
}

struct Session {
    cookie: String,
    csrf: String,
}

impl Session {
    fn of(r: &Resp) -> Self {
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
        Self {
            cookie: r.cookies.join("; "),
            csrf: r.body["csrf_token"].as_str().unwrap().to_owned(),
        }
    }
    async fn get(&self, app: &Router, ip: &str, uri: &str) -> Resp {
        req(app, "GET", uri, ip, &[("cookie", &self.cookie)], None).await
    }
}

#[tokio::test]
async fn login_throttle_devices_and_ip_log() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!("\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n", dir.display()));
    let kernel = Kernel::start(pt_kernel::config::SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    for name in ["alice", "bob", "carol"] {
        let r = req(
            &app,
            "POST",
            "/api/p/identity/v1/users",
            "10.0.0.1",
            &[],
            Some(json!({ "username": name, "email": format!("{name}@example.com"), "password": "correct horse" })),
        )
        .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    }

    // 首次登录：下发设备 cookie，记录 IP
    let r = login(&app, "10.0.0.1", "alice", "correct horse", None).await;
    let alice = Session::of(&r);
    let dev = cookie_named(&r, "__Host-pt_dev").expect("device cookie").to_owned();

    // 第 3 次失败起有指数延迟：第 4 次立即尝试被拒（429，retry_after）
    for _ in 0..3 {
        let r = login(&app, "10.0.0.2", "alice", "wrong", None).await;
        assert_eq!(r.body["code"], "identity.invalid_credentials", "{:?}", r.body);
    }
    let r = login(&app, "10.0.0.2", "alice", "correct horse", None).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "{:?}", r.body);
    assert_eq!(r.body["code"], "identity.login_throttled");
    assert!(r.body["details"]["retry_after"].as_i64().unwrap() >= 1);
    // 成功登录清除用户名维度：取消延迟后可以登录
    set_settings(json!({ "login_max_delay_secs": 0, "login_user_lock_after": 3, "login_ip_block_after": 5 })).await;
    let r = login(&app, "10.0.0.2", "alice", "correct horse", Some(&dev)).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // 连续 3 次失败 → 账号软锁：新设备即使密码正确也被拒，已知设备不受影响
    for _ in 0..3 {
        login(&app, "10.0.0.3", "alice", "wrong", None).await;
    }
    let r = login(&app, "10.0.0.3", "alice", "correct horse", None).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "{:?}", r.body);
    let r = login(&app, "10.0.0.3", "alice", "correct horse", Some(&dev)).await;
    assert_eq!(r.status, StatusCode::OK, "known device bypasses the soft lock: {:?}", r.body);

    // 同一 IP 失败 5 次 → 该 IP 的登录全部被封（不论账号）
    for i in 0..5 {
        login(&app, "10.0.0.9", &format!("nobody{i}"), "wrong", None).await;
    }
    let r = login(&app, "10.0.0.9", "bob", "correct horse", None).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS, "{:?}", r.body);
    let r = login(&app, "10.0.0.8", "bob", "correct horse", None).await;
    let bob = Session::of(&r);

    // 会话 IP 变化记入 IP 历史；本人只看到网段
    let r = alice.get(&app, "10.1.2.3", "/api/p/identity/v1/users/me").await;
    assert_eq!(r.status, StatusCode::OK);
    let r = alice.get(&app, "10.1.2.3", "/api/p/identity/v1/users/me/ip-history").await;
    let items = r.body["items"].as_array().unwrap();
    let seen: Vec<(&str, &str)> = items
        .iter()
        .map(|i| (i["ip"].as_str().unwrap(), i["source"].as_str().unwrap()))
        .collect();
    assert!(seen.contains(&("10.1.2.0/24", "session")), "{seen:?}");
    assert!(seen.contains(&("10.0.0.0/24", "login")), "{seen:?}");
    let r = alice.get(&app, "10.1.2.3", "/api/p/identity/v1/users/me/security-events").await;
    let kinds: Vec<&str> = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"login_locked"), "{kinds:?}");
    assert!(kinds.contains(&"login"), "{kinds:?}");
    let ev = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "login")
        .unwrap();
    assert!(ev["details"]["ip_prefix"].as_str().unwrap().ends_with("/24"));
    assert!(ev["details"].get("ip").is_none(), "security events never hold full IPs");

    // 普通用户不能查 IP
    let r = bob.get(&app, "10.0.0.8", "/api/p/identity/v1/admin/ip-lookups?ip=10.0.0.1").await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // 管理员：按 IP / CIDR 查账户、共用 IP、IP 历史（完整 IP）
    common::grant_role(3, "admin").await;
    let r = login(&app, "10.0.0.1", "carol", "correct horse", None).await;
    let carol = Session::of(&r);
    let r = carol.get(&app, "10.0.0.1", "/api/p/identity/v1/admin/ip-lookups?ip=10.0.0.1").await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let names: Vec<&str> = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["username"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"alice") && names.contains(&"carol"), "{names:?}");
    let r = carol.get(&app, "10.0.0.1", "/api/p/identity/v1/admin/ip-lookups?ip=10.1.0.0/16").await;
    assert_eq!(r.body["items"].as_array().unwrap().len(), 1, "{:?}", r.body);
    let r = carol.get(&app, "10.0.0.1", "/api/p/identity/v1/admin/ip-lookups?ip=10.0.0.0/8").await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    let r = carol.get(&app, "10.0.0.1", "/api/p/identity/v1/admin/users/1/shared-ips").await;
    assert_eq!(r.body["items"][0]["username"], "carol", "{:?}", r.body);
    let r = carol.get(&app, "10.0.0.1", "/api/p/identity/v1/admin/users/1/ip-history").await;
    assert!(
        r.body["items"].as_array().unwrap().iter().any(|i| i["ip"] == "10.1.2.3"),
        "{:?}",
        r.body
    );
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM kernel.audit_log WHERE action = 'identity.sensitive_read'",
    )
    .fetch_one(&mut db().await)
    .await
    .unwrap();
    assert!(audits >= 3, "sensitive reads are audited");

    // 保留期：截断超期 IP 后，按单个地址仍能经 HMAC 查到
    let mut c = db().await;
    sqlx::query("UPDATE identity.ip_log SET last_seen_at = now() - interval '100 days' WHERE user_id = 1")
        .execute(&mut c)
        .await
        .unwrap();
    kernel.run_job_now("identity", "ip_retention", json!({})).await.unwrap();
    let full: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM identity.ip_log WHERE user_id = 1 AND (NOT truncated OR masklen(ip) = 32)",
    )
    .fetch_one(&mut c)
    .await
    .unwrap();
    assert_eq!(full, 0);
    let r = carol.get(&app, "10.0.0.1", "/api/p/identity/v1/admin/ip-lookups?ip=10.1.2.3").await;
    assert_eq!(r.body["items"][0]["username"], "alice", "{:?}", r.body);

    // 新设备登录 → notify 订阅 user.logged_in 发安全提醒；已知设备不提醒
    set_settings(json!({ "login_max_delay_secs": 0 })).await;
    let r = login(&app, "10.0.0.4", "alice", "correct horse", Some(&dev)).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = login(&app, "10.0.0.4", "alice", "correct horse", None).await;
    let alice2 = Session::of(&r);
    let mut alerts = 0;
    for _ in 0..50 {
        let r = alice2.get(&app, "10.0.0.4", "/api/p/notify/v1/inbox").await;
        alerts = r.body["items"]
            .as_array()
            .map_or(0, |a| a.iter().filter(|n| n["template_key"] == "notify.login_alert").count());
        if alerts > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert_eq!(alerts, 1, "exactly one alert for the one new device");
}

type Inbox = std::sync::Arc<std::sync::Mutex<Vec<Value>>>;

/// 模拟 HTTP→SMTP 中继：记录请求体
async fn mock_relay(inbox: Inbox) -> String {
    let app = axum::Router::new().route(
        "/send",
        axum::routing::post(move |axum::Json(b): axum::Json<Value>| {
            let inbox = inbox.clone();
            async move {
                inbox.lock().unwrap().push(b);
                StatusCode::ACCEPTED
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/send")
}

/// 等待发给 `to`、正文含 `path` 的第 `nth` 封邮件，返回 (challenge_id, token)
async fn mail_link(inbox: &Inbox, to: &str, path: &str, nth: usize) -> (String, String) {
    for _ in 0..100 {
        let found = inbox
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m["to"] == to)
            .filter_map(|m| m["text"].as_str().map(str::to_owned))
            .filter(|t| t.contains(&format!("https://pt.test{path}?c=")))
            .nth(nth);
        if let Some(text) = found {
            let url = text
                .split_whitespace()
                .find(|w| w.starts_with("https://pt.test"))
                .unwrap()
                .to_owned();
            let q = url.split_once('?').unwrap().1;
            let get = |k: &str| {
                q.split('&')
                    .find_map(|kv| kv.strip_prefix(&format!("{k}=")))
                    .unwrap()
                    .to_owned()
            };
            return (get("c"), get("t"));
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    panic!("no mail to {to} with {path}: {:?}", inbox.lock().unwrap());
}

#[tokio::test]
async fn email_verification_and_password_reset() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let inbox: Inbox = std::sync::Arc::default();
    let relay = mock_relay(inbox.clone()).await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n\n[grants.notify]\nsensitive = [\"user.email\"]\n\n[plugin.notify]\n\
         email_provider = \"http_relay\"\nemail_endpoint = \"{relay}\"\nemail_token = \"t\"\nemail_from = \"noreply@pt.test\"\n\
         site_name = \"PT Test\"\nsite_url = \"https://pt.test\"\nunsubscribe_secret = \"s\"\n",
        dir.display()
    ));
    let kernel = Kernel::start(pt_kernel::config::SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    set_settings(json!({ "require_email_verification": true })).await;

    // 注册后为待验证：只有访客权限
    let r = req(
        &app,
        "POST",
        "/api/p/identity/v1/users",
        "10.0.0.1",
        &[],
        Some(json!({ "username": "dave", "email": "Dave@Example.com", "password": "correct horse" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["user"]["status"], "pending");
    let dave = Session::of(&login(&app, "10.0.0.1", "dave", "correct horse", None).await);
    let r = dave.get(&app, "10.0.0.1", "/api/p/identity/v1/users/me").await;
    assert_eq!(r.body["email_verified"], false);
    let r = dave.get(&app, "10.0.0.1", "/api/p/identity/v1/users/me/tokens").await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "pending users only have guest permissions");

    // 验证邮件：链接只在邮件里，通知库不保存
    let (c, tok) = mail_link(&inbox, "dave@example.com", "/verify-email", 0).await;
    let links: i64 = sqlx::query_scalar("SELECT count(*) FROM notify.outbox WHERE link IS NOT NULL OR body LIKE '%t=%'")
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert_eq!(links, 0);
    let confirm = |c: &str, t: &str| {
        let body = json!({ "challenge_id": c, "token": t });
        let app = app.clone();
        async move { req(&app, "POST", "/api/p/identity/v1/email-verifications/confirm", "10.0.0.1", &[], Some(body)).await }
    };
    let r = confirm(&c, "wrong-token").await;
    assert_eq!(r.body["code"], "identity.challenge_invalid", "{:?}", r.body);
    let r = confirm(&c, &tok).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = confirm(&c, &tok).await;
    assert_eq!(r.body["code"], "identity.challenge_invalid", "links work once");
    let r = dave.get(&app, "10.0.0.1", "/api/p/identity/v1/users/me").await;
    assert_eq!((r.body["email_verified"].clone(), r.body["user"]["status"].clone()), (json!(true), json!("active")));
    let r = dave.get(&app, "10.0.0.1", "/api/p/identity/v1/users/me/tokens").await;
    assert_eq!(r.status, StatusCode::OK);

    // 找回密码：不存在的账号也返回 202；第二次申请使第一封的链接作废
    let reset_req = |login: &str| {
        let body = json!({ "login": login });
        let app = app.clone();
        async move { req(&app, "POST", "/api/p/identity/v1/password-resets", "10.0.0.5", &[], Some(body)).await }
    };
    assert_eq!(reset_req("ghost").await.status, StatusCode::ACCEPTED);
    assert_eq!(reset_req("dave@example.com").await.status, StatusCode::ACCEPTED);
    let (c1, t1) = mail_link(&inbox, "dave@example.com", "/reset-password", 0).await;
    assert_eq!(reset_req("DAVE").await.status, StatusCode::ACCEPTED);
    let (c2, t2) = mail_link(&inbox, "dave@example.com", "/reset-password", 1).await;
    let reset = |c: &str, t: &str, pw: &str| {
        let body = json!({ "challenge_id": c, "token": t, "new_password": pw });
        let app = app.clone();
        async move { req(&app, "POST", "/api/p/identity/v1/password-resets/confirm", "10.0.0.5", &[], Some(body)).await }
    };
    let r = reset(&c1, &t1, "brand new pass").await;
    assert_eq!(r.body["code"], "identity.challenge_invalid", "superseded: {:?}", r.body);
    let r = reset(&c2, &t2, "short").await;
    assert_eq!(r.body["code"], "identity.password_weak", "{:?}", r.body);
    let r = reset(&c2, &t2, "brand new pass").await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "a weak password does not consume the link: {:?}", r.body);
    // 全部会话失效，旧密码不能用，新密码可以
    let r = dave.get(&app, "10.0.0.1", "/api/p/identity/v1/users/me").await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = login(&app, "10.0.0.1", "dave", "correct horse", None).await;
    assert_eq!(r.body["code"], "identity.invalid_credentials");
    let dave = Session::of(&login(&app, "10.0.0.1", "dave", "brand new pass", None).await);
    let r = dave.get(&app, "10.0.0.1", "/api/p/identity/v1/users/me/security-events").await;
    let kinds: Vec<&str> = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"password_reset") && kinds.contains(&"email_verified"), "{kinds:?}");
    let h = [("cookie", dave.cookie.as_str()), ("x-csrf-token", dave.csrf.as_str()), ("sec-fetch-site", "same-origin")];
    let r = req(&app, "POST", "/api/p/identity/v1/email-verifications", "10.0.0.1", &h, Some(json!({}))).await;
    assert_eq!(r.body["code"], "identity.email_already_verified", "{:?}", r.body);
    let mails_to_ghost = inbox.lock().unwrap().iter().filter(|m| m["to"].as_str().unwrap_or("").contains("ghost")).count();
    assert_eq!(mails_to_ghost, 0);
}
