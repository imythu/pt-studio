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

impl Session {
    fn headers(&self) -> [(&str, &str); 3] {
        [("cookie", self.cookie.as_str()), ("x-csrf-token", self.csrf.as_str()), ("sec-fetch-site", "same-origin")]
    }
    async fn send(&self, app: &Router, method: &str, uri: &str, body: Value) -> Resp {
        req(app, method, uri, "10.0.0.1", &self.headers(), Some(body)).await
    }
}

fn code_at(secret: &str, offset_secs: i64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    pt_plugin_identity::two_factor::totp_at(secret, i64::try_from(now).unwrap() + offset_secs).unwrap()
}

#[tokio::test]
async fn totp_two_factor_and_reauth() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let kernel = Kernel::start(common::config(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    for name in ["erin", "frank"] {
        let r = req(&app, "POST", "/api/p/identity/v1/users", "10.0.0.1", &[],
            Some(json!({ "username": name, "email": format!("{name}@example.com"), "password": "correct horse" }))).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    }
    let erin = Session::of(&login(&app, "10.0.0.1", "erin", "correct horse", None).await);

    // 绑定：生成密钥与二维码 → 错误码被拒 → 正确码开启并返回 10 个恢复码
    let r = erin.send(&app, "POST", "/api/p/identity/v1/users/me/totp", json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let secret = r.body["secret"].as_str().unwrap().to_owned();
    assert!(r.body["otpauth_uri"].as_str().unwrap().starts_with("otpauth://totp/test:erin?secret="));
    assert!(r.body["qr_svg"].as_str().unwrap().contains("<svg"));
    let r = erin.send(&app, "POST", "/api/p/identity/v1/users/me/totp/confirm", json!({ "code": "000000" })).await;
    assert_eq!(r.body["code"], "identity.two_factor_invalid", "{:?}", r.body);
    let first_code = code_at(&secret, 0);
    let r = erin.send(&app, "POST", "/api/p/identity/v1/users/me/totp/confirm", json!({ "code": first_code })).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let recovery: Vec<String> = serde_json::from_value(r.body["recovery_codes"].clone()).unwrap();
    assert_eq!(recovery.len(), 10);
    let r = erin.get(&app, "10.0.0.1", "/api/p/identity/v1/users/me/two-factor").await;
    assert_eq!((r.body["totp"].clone(), r.body["recovery_remaining"].clone()), (json!(true), json!(10)));

    // 敏感操作要求近期重新验证（登录后 10 分钟内有效）
    sqlx::query("UPDATE identity.sessions SET reauth_at = now() - interval '1 hour'")
        .execute(&mut db().await)
        .await
        .unwrap();
    let r = erin.send(&app, "POST", "/api/p/identity/v1/users/me/recovery-codes", json!({})).await;
    assert_eq!(r.body["code"], "identity.reauth_required", "{:?}", r.body);
    let r = erin.send(&app, "POST", "/api/p/identity/v1/sessions/current/reauth", json!({ "password": "wrong" })).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = erin.send(&app, "POST", "/api/p/identity/v1/sessions/current/reauth", json!({ "password": "correct horse" })).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = erin.send(&app, "POST", "/api/p/identity/v1/users/me/recovery-codes", json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let recovery: Vec<String> = serde_json::from_value(r.body["recovery_codes"].clone()).unwrap();

    // 登录：密码正确只得到票据（没有会话 cookie）
    let two_step = |ticket: String, code: String| {
        let app = app.clone();
        async move {
            req(&app, "POST", "/api/p/identity/v1/sessions/two-factor", "10.0.0.1", &[],
                Some(json!({ "ticket": ticket, "code": code }))).await
        }
    };
    let r = login(&app, "10.0.0.1", "erin", "correct horse", None).await;
    assert_eq!(r.body["two_factor_required"], true, "{:?}", r.body);
    assert!(cookie_named(&r, "__Host-pt_session").is_none());
    let ticket = r.body["ticket"].as_str().unwrap().to_owned();
    // 绑定时用过的码不能重放
    let r = two_step(ticket.clone(), first_code.clone()).await;
    assert_eq!(r.body["code"], "identity.two_factor_invalid", "replayed code: {:?}", r.body);
    // 下一个时间步的码（服务端接受 ±1 步）
    let next = (1..=2).map(|k| code_at(&secret, 30 * k)).find(|c| *c != first_code).unwrap();
    let r = two_step(ticket.clone(), next).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["two_factor_passed"], true);
    let r = two_step(ticket.clone(), recovery[0].clone()).await;
    assert_eq!(r.body["code"], "identity.ticket_invalid", "tickets work once");

    // 恢复码：可用一次（大小写与连字符无关）
    let r = login(&app, "10.0.0.1", "erin", "correct horse", None).await;
    let r = two_step(r.body["ticket"].as_str().unwrap().to_owned(), recovery[1].to_uppercase().replace('-', " ")).await;
    let erin = Session::of(&r);
    let r = login(&app, "10.0.0.1", "erin", "correct horse", None).await;
    let t = r.body["ticket"].as_str().unwrap().to_owned();
    let r = two_step(t.clone(), recovery[1].clone()).await;
    assert_eq!(r.body["code"], "identity.two_factor_invalid");
    // 票据最多尝试 5 次
    for _ in 0..4 {
        two_step(t.clone(), "nope-nope0".into()).await;
    }
    let r = two_step(t, recovery[2].clone()).await;
    assert_eq!(r.body["code"], "identity.ticket_invalid", "{:?}", r.body);

    // 强制 2FA：moderator 及以上的会话未通过两步验证时不享有员工权限
    common::grant_role(1, "moderator").await;
    common::grant_role(2, "moderator").await;
    set_settings(json!({ "two_factor_required_role": "moderator" })).await;
    let frank = Session::of(&login(&app, "10.0.0.1", "frank", "correct horse", None).await);
    let r = frank.get(&app, "10.0.0.1", "/api/p/identity/v1/users/me/permissions").await;
    assert_eq!(r.body["two_factor_needed"], true, "{:?}", r.body);
    assert!(!r.body["permissions"].as_array().unwrap().iter().any(|p| p == "identity.users.read"));
    let r = frank.get(&app, "10.0.0.1", "/api/p/identity/v1/admin/users").await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = erin.get(&app, "10.0.0.1", "/api/p/identity/v1/admin/users").await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = erin.send(&app, "DELETE", "/api/p/identity/v1/users/me/totp", json!({})).await;
    assert_eq!(r.body["code"], "identity.two_factor_required", "{:?}", r.body);
    // 不强制时可以关闭
    set_settings(json!({})).await;
    let r = erin.send(&app, "DELETE", "/api/p/identity/v1/users/me/totp", json!({})).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    let r = login(&app, "10.0.0.1", "erin", "correct horse", None).await;
    assert!(r.body["csrf_token"].is_string(), "password-only again: {:?}", r.body);
}

/// 软件认证器（ES256）：生成 attestationObject（fmt = none）与断言
struct SoftKey {
    sk: p256::ecdsa::SigningKey,
    cred_id: Vec<u8>,
    counter: u32,
    handle: Vec<u8>,
}

fn b64(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b)
}

fn b64d(s: &str) -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s).unwrap()
}

fn sha256(b: &[u8]) -> Vec<u8> {
    use sha2::Digest;
    sha2::Sha256::digest(b).to_vec()
}

fn cbor_bytes(b: &[u8]) -> Vec<u8> {
    let mut out = vec![0x59];
    out.extend_from_slice(&u16::try_from(b.len()).unwrap().to_be_bytes());
    out.extend_from_slice(b);
    out
}

fn cbor_text(s: &str) -> Vec<u8> {
    let mut out = vec![0x60 | u8::try_from(s.len()).unwrap()];
    out.extend_from_slice(s.as_bytes());
    out
}

impl SoftKey {
    fn new(seed: u8) -> Self {
        Self {
            sk: p256::ecdsa::SigningKey::from_slice(&[seed; 32]).unwrap(),
            cred_id: vec![seed; 16],
            counter: 0,
            handle: Vec::new(),
        }
    }
    fn cose(&self) -> Vec<u8> {
        let pt = self.sk.verifying_key().to_encoded_point(false);
        let mut k = vec![0xa5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01, 0x21, 0x58, 0x20];
        k.extend_from_slice(pt.x().unwrap());
        k.extend_from_slice(&[0x22, 0x58, 0x20]);
        k.extend_from_slice(pt.y().unwrap());
        k
    }
    fn auth_data(&self, rp_id: &str, flags: u8, attested: bool) -> Vec<u8> {
        let mut d = sha256(rp_id.as_bytes());
        d.push(flags | if attested { 0x40 } else { 0 });
        d.extend_from_slice(&self.counter.to_be_bytes());
        if attested {
            d.extend_from_slice(&[0u8; 16]);
            d.extend_from_slice(&u16::try_from(self.cred_id.len()).unwrap().to_be_bytes());
            d.extend_from_slice(&self.cred_id);
            d.extend_from_slice(&self.cose());
        }
        d
    }
    /// navigator.credentials.create 的结果
    fn create(&mut self, opts: &Value, origin: &str) -> Value {
        let pk = &opts["public_key"];
        self.handle = b64d(pk["user"]["id"].as_str().unwrap());
        let cd = json!({ "type": "webauthn.create", "challenge": pk["challenge"], "origin": origin }).to_string();
        let mut att = vec![0xa3];
        att.extend(cbor_text("fmt"));
        att.extend(cbor_text("none"));
        att.extend(cbor_text("attStmt"));
        att.push(0xa0);
        att.extend(cbor_text("authData"));
        att.extend(cbor_bytes(&self.auth_data(pk["rp"]["id"].as_str().unwrap(), 0x05, true)));
        json!({ "id": b64(&self.cred_id), "rawId": b64(&self.cred_id), "type": "public-key",
                "response": { "clientDataJSON": b64(cd.as_bytes()), "attestationObject": b64(&att) },
                "clientExtensionResults": { "credProps": { "rk": true } } })
    }
    /// navigator.credentials.get 的结果；`flags`：0x01 UP，0x04 UV
    fn get(&mut self, opts: &Value, origin: &str, flags: u8, bump: bool) -> Value {
        use p256::ecdsa::signature::Signer;
        let pk = &opts["public_key"];
        if bump {
            self.counter += 1;
        }
        let cd = json!({ "type": "webauthn.get", "challenge": pk["challenge"], "origin": origin }).to_string();
        let ad = self.auth_data(pk["rpId"].as_str().unwrap(), flags, false);
        let mut msg = ad.clone();
        msg.extend_from_slice(&sha256(cd.as_bytes()));
        let sig: p256::ecdsa::Signature = self.sk.sign(&msg);
        json!({ "id": b64(&self.cred_id), "rawId": b64(&self.cred_id), "type": "public-key",
                "response": { "clientDataJSON": b64(cd.as_bytes()), "authenticatorData": b64(&ad),
                              "signature": b64(sig.to_der().as_bytes()), "userHandle": b64(&self.handle) } })
    }
}

#[tokio::test]
async fn webauthn_second_factor_and_passwordless() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let kernel = Kernel::start(common::config(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let r = req(&app, "POST", "/api/p/identity/v1/users", "10.0.0.1", &[],
        Some(json!({ "username": "gina", "email": "gina@example.com", "password": "correct horse" }))).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let gina = Session::of(&login(&app, "10.0.0.1", "gina", "correct horse", None).await);
    const ORIGIN: &str = "https://pt.test";
    let post = |path: &'static str, body: Value| {
        let app = app.clone();
        async move { req(&app, "POST", path, "10.0.0.1", &[], Some(body)).await }
    };

    // 登记安全密钥：rp 为站点主机名；第一个第二因素同时给出恢复码
    let mut key = SoftKey::new(7);
    let opts = gina.send(&app, "POST", "/api/p/identity/v1/users/me/webauthn/registration-options", json!({})).await;
    assert_eq!(opts.body["public_key"]["rp"]["id"], "pt.test", "{:?}", opts.body);
    let cred = key.create(&opts.body, "https://evil.test");
    let r = gina.send(&app, "POST", "/api/p/identity/v1/users/me/webauthn-credentials",
        json!({ "ceremony_id": opts.body["ceremony_id"], "name": "YubiKey", "credential": cred })).await;
    assert_eq!(r.body["code"], "identity.webauthn_invalid", "wrong origin: {:?}", r.body);
    let opts = gina.send(&app, "POST", "/api/p/identity/v1/users/me/webauthn/registration-options", json!({})).await;
    let cred = key.create(&opts.body, ORIGIN);
    let r = gina.send(&app, "POST", "/api/p/identity/v1/users/me/webauthn-credentials",
        json!({ "ceremony_id": opts.body["ceremony_id"], "name": "YubiKey", "credential": cred.clone() })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["recovery_codes"].as_array().unwrap().len(), 10);
    let r = gina.send(&app, "POST", "/api/p/identity/v1/users/me/webauthn-credentials",
        json!({ "ceremony_id": opts.body["ceremony_id"], "name": "again", "credential": cred })).await;
    assert_eq!(r.body["code"], "identity.webauthn_invalid", "ceremonies work once: {:?}", r.body);
    let r = gina.get(&app, "10.0.0.1", "/api/p/identity/v1/users/me/two-factor").await;
    assert_eq!(r.body["webauthn"][0]["name"], "YubiKey", "{:?}", r.body);

    // 第二因素：票据 → 断言选项 → 断言
    let r = login(&app, "10.0.0.1", "gina", "correct horse", None).await;
    assert_eq!(r.body["methods"], json!(["webauthn", "recovery"]), "{:?}", r.body);
    let ticket = r.body["ticket"].as_str().unwrap().to_owned();
    let o = post("/api/p/identity/v1/sessions/two-factor/webauthn-options", json!({ "ticket": ticket })).await;
    assert_eq!(o.body["public_key"]["allowCredentials"][0]["id"], b64(&key.cred_id), "{:?}", o.body);
    let r = post("/api/p/identity/v1/sessions/two-factor", json!({ "ticket": ticket, "assertion": key.get(&o.body, ORIGIN, 0x01, true) })).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["two_factor_passed"], true);

    // 签名计数器没有递增（疑似克隆）→ 拒绝
    let r = login(&app, "10.0.0.1", "gina", "correct horse", None).await;
    let ticket = r.body["ticket"].as_str().unwrap().to_owned();
    let o = post("/api/p/identity/v1/sessions/two-factor/webauthn-options", json!({ "ticket": ticket })).await;
    let r = post("/api/p/identity/v1/sessions/two-factor", json!({ "ticket": ticket, "assertion": key.get(&o.body, ORIGIN, 0x01, false) })).await;
    assert_eq!(r.body["code"], "identity.two_factor_invalid", "{:?}", r.body);

    // 免密码登录：要求用户验证（UV）
    let o = post("/api/p/identity/v1/webauthn/assertion-options", json!({})).await;
    let r = post("/api/p/identity/v1/sessions/webauthn", json!({ "ceremony_id": o.body["ceremony_id"], "assertion": key.get(&o.body, ORIGIN, 0x01, true) })).await;
    assert_eq!(r.body["code"], "identity.webauthn_invalid", "UV required: {:?}", r.body);
    let o = post("/api/p/identity/v1/webauthn/assertion-options", json!({})).await;
    let r = post("/api/p/identity/v1/sessions/webauthn", json!({ "ceremony_id": o.body["ceremony_id"], "assertion": key.get(&o.body, ORIGIN, 0x05, true) })).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["user"]["username"], "gina");
    let fresh = Session::of(&r);
    let r = fresh.get(&app, "10.0.0.1", "/api/p/identity/v1/users/me/security-events").await;
    assert!(r.body["items"].as_array().unwrap().iter().any(|e| e["kind"] == "webauthn_counter_regression"), "{:?}", r.body);

    // 未知凭据（另一把钥匙）
    let mut other = SoftKey::new(9);
    other.handle = key.handle.clone();
    let o = post("/api/p/identity/v1/webauthn/assertion-options", json!({})).await;
    let r = post("/api/p/identity/v1/sessions/webauthn", json!({ "ceremony_id": o.body["ceremony_id"], "assertion": other.get(&o.body, ORIGIN, 0x05, true) })).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{:?}", r.body);

    // 删除后回到只用密码登录（恢复码随之作废）
    let id = fresh.get(&app, "10.0.0.1", "/api/p/identity/v1/users/me/two-factor").await.body["webauthn"][0]["id"].as_i64().unwrap();
    let r = fresh.send(&app, "DELETE", &format!("/api/p/identity/v1/users/me/webauthn-credentials/{id}"), json!({})).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    let r = login(&app, "10.0.0.1", "gina", "correct horse", None).await;
    assert!(r.body["csrf_token"].is_string(), "{:?}", r.body);
    let r = fresh.get(&app, "10.0.0.1", "/api/p/identity/v1/users/me/two-factor").await;
    assert_eq!(r.body["recovery_remaining"], 0);
}

/// 模拟 siteverify：表单中 response=good-token 时成功；记录收到的请求体
async fn mock_siteverify(log: std::sync::Arc<std::sync::Mutex<Vec<String>>>) -> String {
    let app = axum::Router::new().route(
        "/siteverify",
        axum::routing::post(move |body: String| {
            let log = log.clone();
            async move {
                let ok = body.split('&').any(|kv| kv == "response=good-token");
                log.lock().unwrap().push(body);
                axum::Json(json!({ "success": ok, "error-codes": if ok { json!([]) } else { json!(["invalid-input-response"]) } }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/siteverify")
}

#[tokio::test]
async fn captcha_guards_login_register_and_reset() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let log: std::sync::Arc<std::sync::Mutex<Vec<String>>> = std::sync::Arc::default();
    let verify = mock_siteverify(log.clone()).await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n\n[plugin.captcha]\nprovider = \"turnstile\"\nsite_key = \"site-key\"\n\
         secret = \"s3cret\"\nverify_url = \"{verify}\"\n",
        dir.display()
    ));
    let kernel = Kernel::start(pt_kernel::config::SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();

    // 前端配置：不含密钥；默认只验证注册与找回密码
    let r = req(&app, "GET", "/api/p/captcha/v1/config", "10.0.0.1", &[], None).await;
    assert_eq!(r.body, json!({ "enabled": true, "provider": "turnstile", "site_key": "site-key", "actions": ["register", "password_reset"] }));

    let register = |token: Option<&str>| {
        let mut body = json!({ "username": "hank", "email": "hank@example.com", "password": "correct horse" });
        if let Some(t) = token {
            body["captcha_token"] = json!(t);
        }
        let app = app.clone();
        async move { req(&app, "POST", "/api/p/identity/v1/users", "10.0.0.7", &[], Some(body)).await }
    };
    let r = register(None).await;
    assert_eq!(r.body["code"], "identity.captcha_failed", "{:?}", r.body);
    let r = register(Some("bad-token")).await;
    assert_eq!(r.body["code"], "identity.captcha_failed", "{:?}", r.body);
    let r = register(Some("good-token")).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let sent = log.lock().unwrap().last().cloned().unwrap();
    assert!(sent.contains("secret=s3cret") && sent.contains("remoteip=10.0.0.7"), "{sent}");

    // 登录默认不验证；后台开启后需要令牌
    let r = login(&app, "10.0.0.7", "hank", "correct horse", None).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    sqlx::query("INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('captcha', $1)")
        .bind(json!({ "actions": ["login"] }))
        .execute(&mut db().await)
        .await
        .unwrap();
    let r = login(&app, "10.0.0.7", "hank", "correct horse", None).await;
    assert_eq!(r.body["code"], "identity.captcha_failed", "{:?}", r.body);
    let r = req(&app, "POST", "/api/p/identity/v1/sessions", "10.0.0.7", &[],
        Some(json!({ "username": "hank", "password": "correct horse", "captcha_token": "good-token" }))).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    // 找回密码不再在列表中：不需要令牌
    let r = req(&app, "POST", "/api/p/identity/v1/password-resets", "10.0.0.7", &[], Some(json!({ "login": "hank" }))).await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{:?}", r.body);
}
