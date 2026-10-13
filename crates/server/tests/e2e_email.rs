//! 端到端：通知的邮件渠道（notify，WASM）—— 按类别与渠道的偏好、投递时读取已验证邮箱（user.email 授权）、http_relay
//! 提供方（本地模拟）、未验证邮箱跳过、4xx 永久失败、一键退订（签名令牌）后不再发送。

#![allow(clippy::unwrap_used)]

mod common;

use std::sync::{Arc, Mutex};

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{User, call, env, hv, register_login, reset};

type Inbox = Arc<Mutex<Vec<(String, Value)>>>;

/// 模拟 HTTP→SMTP 中继：记录 (Authorization, 请求体)；收件人含 bounce 时返回 422
async fn mock_relay(inbox: Inbox) -> String {
    let app = axum::Router::new().route(
        "/send",
        axum::routing::post(
            move |headers: axum::http::HeaderMap, axum::Json(b): axum::Json<Value>| {
                let inbox = inbox.clone();
                async move {
                    let auth = headers
                        .get("authorization")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_owned();
                    let bounce = b["to"].as_str().unwrap_or("").contains("bounce");
                    inbox.lock().unwrap().push((auth, b));
                    if bounce {
                        StatusCode::UNPROCESSABLE_ENTITY
                    } else {
                        StatusCode::ACCEPTED
                    }
                }
            },
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}/send")
}

fn config(dir: &std::path::Path, relay: &str) -> SiteConfig {
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n\n[grants.notify]\nsensitive = [\"user.email\"]\n\n[plugin.notify]\n\
         email_provider = \"http_relay\"\nemail_endpoint = \"{relay}\"\nemail_token = \"relay-token\"\nemail_from = \"noreply@pt.test\"\n\
         site_name = \"PT Test\"\nsite_url = \"https://pt.test\"\nunsubscribe_secret = \"unsub-secret\"\n",
        dir.display()
    ));
    SiteConfig::parse(&t).unwrap()
}

async fn post(app: &axum::Router, u: &User, path: &str, body: Value) -> common::Resp {
    call(app, "POST", path, Some(body), &hv(&u.headers())).await
}

async fn db() -> PgConnection {
    PgConnection::connect(&env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap()
}

/// 发布求种（签到得到的魔力值做悬赏），由 filler 应求 → 求种者收到 content 类通知
async fn request_and_fill(app: &axum::Router, owner: &User, filler: &User, title: &str) {
    post(app, owner, "/api/p/checkin/v1/check-ins", json!({})).await;
    let r = post(
        app,
        owner,
        "/api/p/requests/v1/requests",
        json!({ "type_id": "generic", "title": title, "bounty": 1 }),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let rid = r.body["id"].as_i64().unwrap();
    let tid = common::upload(
        app,
        filler,
        &format!("Fill.{rid}.{}", owner.id),
        &[("a.bin", 4 << 20)],
    )
    .await;
    let r = post(
        app,
        filler,
        &format!("/api/p/requests/v1/requests/{rid}/fills"),
        json!({ "torrent_id": tid }),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
}

async fn outbox_state(uid: u64) -> Vec<String> {
    let mut c = db().await;
    sqlx::query_scalar("SELECT state FROM notify.outbox WHERE user_id = $1 ORDER BY id")
        .bind(i64::try_from(uid).unwrap())
        .fetch_all(&mut c)
        .await
        .unwrap()
}

/// 等待某用户的邮件投递任务结束（不再是 pending）
async fn wait_done(uid: u64, n: usize) -> Vec<String> {
    for _ in 0..200 {
        let s = outbox_state(uid).await;
        if s.len() >= n && s.iter().all(|x| x != "pending") {
            return s;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!(
        "outbox of user {uid} not delivered: {:?}",
        outbox_state(uid).await
    );
}

#[tokio::test]
async fn email_channel_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let dir = common::package_wasm_plugins();
    reset().await;
    let inbox: Inbox = Arc::new(Mutex::new(Vec::new()));
    let relay = mock_relay(inbox.clone()).await;
    let kernel = Kernel::start(config(&dir, &relay), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let alice = register_login(&app, "em_alice").await;
    let carol = register_login(&app, "em_carol").await;
    let bounce = register_login(&app, "em_bounce").await;
    let bob = register_login(&app, "em_bob").await;
    // alice 与 bounce 的邮箱已验证，carol 未验证
    let mut c = db().await;
    sqlx::query("UPDATE identity.user_emails SET verified_at = now() WHERE user_id = ANY($1)")
        .bind(vec![
            i64::try_from(alice.id).unwrap(),
            i64::try_from(bounce.id).unwrap(),
        ])
        .execute(&mut c)
        .await
        .unwrap();

    // 偏好：邮件渠道可用；content 类默认不发邮件，account 默认发
    let p = call(
        &app,
        "GET",
        "/api/p/notify/v1/preferences",
        None,
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(
        (
            p.body["content"]["email"].as_bool(),
            p.body["account"]["email"].as_bool(),
            p.body["content"]["email_available"].as_bool()
        ),
        (Some(false), Some(true), Some(true)),
        "{:?}",
        p.body
    );
    // 默认不发：应求通知只进站内
    request_and_fill(&app, &alice, &bob, "求 A0").await;
    assert!(outbox_state(alice.id).await.is_empty());
    for u in [&alice, &carol, &bounce] {
        let r = call(
            &app,
            "PUT",
            "/api/p/notify/v1/preferences",
            Some(json!({ "content": { "email": true } })),
            &hv(&u.headers()),
        )
        .await;
        assert_eq!(r.status, StatusCode::NO_CONTENT);
    }

    // 开启后：alice 收到邮件；carol 未验证 → 跳过；bounce 被提供方拒收 → 永久失败
    request_and_fill(&app, &alice, &bob, "求 A1").await;
    request_and_fill(&app, &carol, &bob, "求 C1").await;
    request_and_fill(&app, &bounce, &bob, "求 B1").await;
    assert_eq!(wait_done(alice.id, 1).await, vec!["sent"]);
    assert_eq!(wait_done(carol.id, 1).await, vec!["skipped"]);
    assert_eq!(wait_done(bounce.id, 1).await, vec!["failed"]);
    let mails = inbox.lock().unwrap().clone();
    assert_eq!(mails.len(), 2, "alice + bounce attempt: {mails:?}");
    let (auth, m) = mails
        .iter()
        .find(|(_, m)| m["to"] == "em_alice@example.com")
        .cloned()
        .unwrap();
    assert_eq!(auth, "Bearer relay-token");
    assert_eq!(m["from"], "noreply@pt.test");
    assert!(m["subject"].as_str().unwrap().starts_with("[PT Test] "), "{m}");
    assert!(
        m["text"]
            .as_str()
            .unwrap()
            .contains("https://pt.test/p/requests/"),
        "{m}"
    );
    assert_eq!(
        m["headers"]["List-Unsubscribe-Post"],
        "List-Unsubscribe=One-Click"
    );
    assert!(
        m["text"]
            .as_str()
            .unwrap()
            .contains("Unsubscribe: https://pt.test/p/notify/unsubscribe?t="),
        "{m}"
    );
    // 站内通知照常
    let n = call(&app, "GET", "/api/p/notify/v1/inbox", None, &hv(&alice.headers())).await;
    assert_eq!(n.body["items"].as_array().map(Vec::len), Some(2), "{:?}", n.body);

    // 一键退订（公开路由，凭签名令牌）
    let link = m["headers"]["List-Unsubscribe"].as_str().unwrap();
    let path = link
        .trim_start_matches("<https://pt.test")
        .trim_end_matches('>')
        .to_owned();
    let r = call(&app, "GET", &path, None, &[]).await;
    assert_eq!(
        (
            r.status,
            r.body["category"].as_str(),
            r.body["unsubscribed"].as_bool()
        ),
        (StatusCode::OK, Some("content"), Some(false)),
        "{:?}",
        r.body
    );
    let r = call(&app, "POST", &path, Some(json!({})), &[]).await;
    assert_eq!(r.body["unsubscribed"], true, "{:?}", r.body);
    let forged = path.replacen(&format!("t={}", alice.id), &format!("t={}", bob.id), 1);
    let r = call(&app, "POST", &forged, Some(json!({})), &[]).await;
    assert_eq!(r.body["code"], "notify.unsubscribe_invalid", "{:?}", r.body);
    let p = call(
        &app,
        "GET",
        "/api/p/notify/v1/preferences",
        None,
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(p.body["content"]["email"], false);
    request_and_fill(&app, &alice, &bob, "求 A2").await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert_eq!(
        outbox_state(alice.id).await,
        vec!["sent"],
        "no mail after unsubscribe"
    );
    kernel.shutdown().await;
}
