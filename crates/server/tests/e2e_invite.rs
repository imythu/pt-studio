//! 端到端：invite（WASM）—— 初始额度、发邀与重放、公开校验、注册预留与确认、邀请树、撤销返还、过期、
//! levels 的 `invite.before_issue`、identity 限制 → `user.status_changed` → 连坐（扣额度、暂停、复核）与解封撤销、活动码。

#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{User, call, env, hv, reset};

fn config(dir: &std::path::Path) -> SiteConfig {
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n",
        dir.display()
    ));
    SiteConfig::parse(&t).unwrap()
}

async fn wait_for<F, Fut>(what: &str, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..200 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn register(app: &axum::Router, name: &str, code: Option<&str>) -> common::Resp {
    let mut body =
        json!({ "username": name, "email": format!("{name}@example.com"), "password": "correct horse" });
    if let Some(c) = code {
        body["invite_code"] = json!(c);
    }
    call(app, "POST", "/api/p/identity/v1/users", Some(body), &[]).await
}

async fn login(app: &axum::Router, name: &str) -> User {
    let r = call(
        app,
        "POST",
        "/api/p/identity/v1/sessions",
        Some(json!({ "username": name, "password": "correct horse" })),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    User {
        id: r.body["user"]["id"].as_u64().unwrap(),
        cookie: r.cookies.join("; "),
        csrf: r.body["csrf_token"].as_str().unwrap().to_owned(),
    }
}

async fn req(app: &axum::Router, u: &User, method: &str, path: &str, body: Option<Value>) -> common::Resp {
    call(
        app,
        method,
        &format!("/api/p/invite{path}"),
        body,
        &hv(&u.headers()),
    )
    .await
}

async fn balance(app: &axum::Router, u: &User) -> i64 {
    req(app, u, "GET", "/v1/users/me/quota", None).await.body["balance"]
        .as_i64()
        .unwrap()
}

#[tokio::test]
async fn invite_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter("info,sqlx=warn,cranelift=warn,wasmtime=warn")
        .try_init();
    let dir = common::package_wasm_plugins();
    reset().await;
    let kernel = Kernel::start(config(&dir), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let mut db = PgConnection::connect(&env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap();
    let settings = json!({
        "min_account_days": 0, "daily_limit": 10,
        "class_rules": { "*": { "max_outstanding": 3 } },
        "initial_quota": { "amount": 1 },
        "penalty_rules": [ {
            "id": "ban_early", "group": "ban", "trigger": "invitee_banned", "within_days": 30, "max_depth": 2,
            "levels": [ { "depth": 1, "actions": [ { "type": "deduct_quota", "amount": 1 }, { "type": "suspend_invite", "days": 30 } ] },
                        { "depth": 2, "actions": [ { "type": "flag_review" }, { "type": "deduct_quota", "amount": 1 } ] } ],
            "reverse_on_lift": true } ],
    });
    sqlx::query("INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('invite', $1)")
        .bind(&settings)
        .execute(&mut db)
        .await
        .unwrap();

    let r = register(&app, "inv_admin", None).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let admin = login(&app, "inv_admin").await;
    common::grant_role(admin.id, "admin").await;
    assert_eq!(
        register(&app, "inv_alice", None).await.status,
        StatusCode::CREATED
    );
    let alice = login(&app, "inv_alice").await;

    // 初始额度（user.registered 订阅）
    wait_for("initial quota", || async { balance(&app, &alice).await == 1 }).await;
    let r = req(&app, &alice, "GET", "/v1/users/me/eligibility", None).await;
    assert_eq!(r.body["eligible"], true, "{:?}", r.body);

    // 发邀与 request_id 重放
    let r = req(
        &app,
        &alice,
        "POST",
        "/v1/users/me/invites",
        Some(json!({ "request_id": "r1", "note": "friend" })),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.headers["cache-control"], "no-store");
    let code1 = r.body["code"].as_str().unwrap().to_owned();
    assert!(code1.starts_with("INV-") && code1.len() == 30);
    let r2 = req(
        &app,
        &alice,
        "POST",
        "/v1/users/me/invites",
        Some(json!({ "request_id": "r1" })),
    )
    .await;
    assert_eq!(
        (r2.status, r2.body["code"].as_str()),
        (StatusCode::OK, Some(code1.as_str()))
    );
    assert_eq!(balance(&app, &alice).await, 0);
    let r = req(&app, &alice, "POST", "/v1/users/me/invites", Some(json!({}))).await;
    assert_eq!(r.body["code"], "invite.quota_exhausted", "{:?}", r.body);
    // 库中不存明文
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM invite.invites WHERE note = 'friend' AND code_hint = $1")
            .bind(&code1[code1.len() - 4..])
            .fetch_one(&mut db)
            .await
            .unwrap();
    assert_eq!(n, 1);

    // 管理员调额度
    let mut h = admin.headers();
    h.push(("idempotency-key", "adj1".into()));
    let r = call(
        &app,
        "POST",
        &format!("/api/p/invite/v1/admin/users/{}/quota-adjustments", alice.id),
        Some(json!({ "delta": 2, "reason": "test" })),
        &hv(&h),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(balance(&app, &alice).await, 2);

    // 公开校验（码可带空格、小写）
    let messy = format!(" {} ", code1.to_lowercase());
    let r = call(
        &app,
        "POST",
        "/api/p/invite/v1/invite-checks",
        Some(json!({ "code": messy })),
        &[],
    )
    .await;
    assert_eq!(
        (r.status, r.body["inviter_username"].as_str()),
        (StatusCode::OK, Some("inv_alice")),
        "{:?}",
        r.body
    );
    let r = call(
        &app,
        "POST",
        "/api/p/invite/v1/invite-checks",
        Some(json!({ "code": "INV-NOPE" })),
        &[],
    )
    .await;
    assert_eq!(
        (r.status, r.body["details"]["reason"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("not_found"))
    );

    // 注册：钩子预留 + 事件确认 → 邀请树
    let r = register(&app, "inv_bob", Some(&code1)).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let bob = login(&app, "inv_bob").await;
    wait_for("redemption", || async {
        req(&app, &alice, "GET", "/v1/users/me/invitees", None).await.body["items"][0]["user_id"].as_u64()
            == Some(bob.id)
    })
    .await;
    let inviter: Option<i64> = sqlx::query_scalar("SELECT inviter_id FROM identity.users WHERE id = $1")
        .bind(bob.id as i64)
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(inviter, Some(alice.id as i64));
    let r = register(&app, "inv_mallory", Some(&code1)).await;
    assert_eq!(
        (r.status, r.body["details"]["reason"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("exhausted")),
        "{:?}",
        r.body
    );

    // 撤销返还原批次
    let r = req(&app, &alice, "POST", "/v1/users/me/invites", Some(json!({}))).await;
    let id = r.body["invite"]["id"].as_i64().unwrap();
    assert_eq!(balance(&app, &alice).await, 1);
    let r = req(
        &app,
        &alice,
        "DELETE",
        &format!("/v1/users/me/invites/{id}"),
        None,
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    assert_eq!(balance(&app, &alice).await, 2);
    let r = req(&app, &bob, "DELETE", &format!("/v1/users/me/invites/{id}"), None).await;
    assert_eq!(r.body["code"], "invite.not_owner");

    // 第二层：bob（初始额度 1）邀请 carol
    wait_for("bob initial quota", || async { balance(&app, &bob).await == 1 }).await;
    let r = req(
        &app,
        &bob,
        "POST",
        "/v1/users/me/invites",
        Some(json!({ "username": "Inv_Carol" })),
    )
    .await;
    let code2 = r.body["code"].as_str().unwrap().to_owned();
    let r = register(&app, "inv_dave", Some(&code2)).await;
    assert_eq!(r.body["code"], "invite.binding_invalid", "{:?}", r.body);
    assert_eq!(
        register(&app, "inv_carol", Some(&code2)).await.status,
        StatusCode::CREATED
    );
    let carol = login(&app, "inv_carol").await;
    wait_for("second level", || async {
        req(
            &app,
            &alice,
            "GET",
            &format!("/v1/users/{}/tree?depth=2", alice.id),
            None,
        )
        .await
        .body["level_counts"]
            == json!([1, 1])
    })
    .await;
    let r = req(
        &app,
        &carol,
        "GET",
        &format!("/v1/users/{}/ancestors", carol.id),
        None,
    )
    .await;
    let anc: Vec<u64> = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["user_id"].as_u64().unwrap())
        .collect();
    assert_eq!(anc, vec![bob.id, alice.id]);
    let r = req(&app, &carol, "GET", &format!("/v1/users/{}/tree", alice.id), None).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // 封禁 carol（identity 限制）→ user.status_changed → 连坐
    let mut h = admin.headers();
    h.push(("idempotency-key", "ban1".into()));
    let r = call(
        &app,
        "POST",
        &format!("/api/p/identity/v1/admin/users/{}/restrictions", carol.id),
        Some(json!({ "kind": "ban", "reason_code": "cheating" })),
        &hv(&h),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let rid = r.body["id"].as_i64().unwrap();
    let r = call(
        &app,
        "GET",
        "/api/p/identity/v1/users/me",
        None,
        &hv(&carol.headers()),
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::UNAUTHORIZED,
        "banned user's session must be revoked"
    );
    wait_for("penalties", || async {
        let r = call(
            &app,
            "GET",
            "/api/p/invite/v1/admin/penalties",
            None,
            &hv(&admin.headers()),
        )
        .await;
        r.body["items"].as_array().is_some_and(|a| a.len() == 2)
    })
    .await;
    let r = req(&app, &bob, "GET", "/v1/users/me/penalties", None).await;
    assert_eq!(r.body["items"][0]["state"], "applied", "{:?}", r.body);
    assert_eq!(r.body["items"][0]["applied"]["deducted"], 0); // bob 的额度已用完
    let r = req(&app, &bob, "POST", "/v1/users/me/invites", Some(json!({}))).await;
    assert_eq!(r.body["code"], "invite.privilege_suspended", "{:?}", r.body);
    let r = req(&app, &alice, "GET", "/v1/users/me/penalties", None).await;
    let alice_pen = r.body["items"][0].clone();
    assert_eq!(alice_pen["state"], "pending_review");
    assert_eq!(balance(&app, &alice).await, 2, "actions wait for review");

    // 复核：确认 → 执行延后动作（扣 1）
    let mut h = admin.headers();
    h.push(("idempotency-key", "rv1".into()));
    let r = call(
        &app,
        "POST",
        &format!("/api/p/invite/v1/admin/penalties/{}/reviews", alice_pen["id"]),
        Some(json!({ "decision": "confirm", "version": alice_pen["version"] })),
        &hv(&h),
    )
    .await;
    assert_eq!(
        (r.status, r.body["state"].as_str()),
        (StatusCode::OK, Some("confirmed")),
        "{:?}",
        r.body
    );
    assert_eq!(balance(&app, &alice).await, 1);

    // 解封 → 撤销连坐：返还、解除暂停
    let r = call(
        &app,
        "DELETE",
        &format!("/api/p/identity/v1/admin/users/{}/restrictions/{rid}", carol.id),
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    wait_for("penalties reversed", || async {
        balance(&app, &alice).await == 2
    })
    .await;
    let r = req(&app, &bob, "GET", "/v1/users/me/eligibility", None).await;
    assert!(
        r.body["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "not_suspended" && c["ok"] == true),
        "{:?}",
        r.body
    );
    let states: Vec<String> = sqlx::query_scalar("SELECT state FROM invite.penalties ORDER BY id")
        .fetch_all(&mut db)
        .await
        .unwrap();
    assert_eq!(states, vec!["reversed", "reversed"]);

    // levels 的 invite.before_issue：农民不能发邀
    sqlx::query("UPDATE levels.user_levels SET class_id = 10 WHERE user_id = $1")
        .bind(alice.id as i64)
        .execute(&mut db)
        .await
        .unwrap();
    let r = req(&app, &alice, "POST", "/v1/users/me/invites", Some(json!({}))).await;
    assert_eq!(r.body["code"], "levels.class_required", "{:?}", r.body);
    assert_eq!(
        balance(&app, &alice).await,
        2,
        "a denied issue must not consume quota"
    );
    sqlx::query("UPDATE levels.user_levels SET class_id = 20 WHERE user_id = $1")
        .bind(alice.id as i64)
        .execute(&mut db)
        .await
        .unwrap();

    // 过期（refund_on_expire 默认 false）
    let r = req(&app, &alice, "POST", "/v1/users/me/invites", Some(json!({}))).await;
    let id = r.body["invite"]["id"].as_i64().unwrap();
    sqlx::query("UPDATE invite.invites SET expires_at = now() - interval '1 second' WHERE id = $1")
        .bind(id)
        .execute(&mut db)
        .await
        .unwrap();
    kernel
        .run_job_now("invite", "expire_invites", json!({}))
        .await
        .unwrap();
    let st: String = sqlx::query_scalar("SELECT status FROM invite.invites WHERE id = $1")
        .bind(id)
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(st, "expired");
    assert_eq!(balance(&app, &alice).await, 1);

    // 活动码：两次可用，第三次用完
    let r = req(
        &app,
        &admin,
        "POST",
        "/v1/admin/invites",
        Some(json!({ "kind": "campaign", "max_uses": 2, "expires_days": 3 })),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let camp = r.body["code"].as_str().unwrap().to_owned();
    assert_eq!(
        register(&app, "inv_c1", Some(&camp)).await.status,
        StatusCode::CREATED
    );
    assert_eq!(
        register(&app, "inv_c2", Some(&camp)).await.status,
        StatusCode::CREATED
    );
    wait_for("campaign exhausted", || async {
        call(
            &app,
            "POST",
            "/api/p/invite/v1/invite-checks",
            Some(json!({ "code": camp })),
            &[],
        )
        .await
        .body["details"]["reason"]
            == "exhausted"
    })
    .await;
    let edges: i64 = sqlx::query_scalar("SELECT count(*) FROM invite.tree_edges")
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(edges, 2, "campaign codes have no inviter by default");

    kernel.shutdown().await;
}
