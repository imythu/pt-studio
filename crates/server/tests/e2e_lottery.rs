//! 端到端：抽奖（lottery，WASM）—— 默认关闭、注册天数限制、开期、购票（扣款、限购、票号连续）、立即开奖（奖金与抽水、
//! commit 校验）、管理员取消退款、到期票数不足自动取消、停售后不能买。
//!
//! 运行：`cargo test -p pt-server --test e2e_lottery -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Connection, PgConnection};

use common::{User, call, env, grant_role, hv, register_login, reset};

async fn db() -> PgConnection {
    PgConnection::connect(&env("PT_TEST_SUPERUSER_URL", "postgres://postgres@127.0.0.1:55432/pt_test")).await.unwrap()
}

async fn balance(app: &axum::Router, u: &User) -> i64 {
    let r = call(app, "GET", "/api/p/bonus/v1/users/me/summary", None, &hv(&u.headers())).await;
    r.body["balances"].as_array().into_iter().flatten().find(|b| b["currency"] == "bonus").and_then(|b| b["balance"].as_i64()).unwrap_or(0)
}

async fn put_settings(app: &axum::Router, admin: &User, settings: Value) {
    let path = "/api/kernel/v1/admin/plugins/lottery/settings";
    let version = call(app, "GET", path, None, &hv(&admin.headers())).await.body["version"].as_i64().unwrap_or(0);
    let r = call(app, "PUT", path, Some(json!({ "settings": settings, "version": version })), &hv(&admin.headers())).await;
    assert!(r.status.is_success(), "{:?}", r.body);
}

fn now_ms() -> i64 {
    i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()).unwrap()
}

#[tokio::test]
async fn lottery_end_to_end() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!("\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n", dir.display()));
    let kernel = Kernel::start(SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let users = [register_login(&app, "lot_alice").await, register_login(&app, "lot_bob").await, register_login(&app, "lot_carol").await];
    let admin = register_login(&app, "lot_admin").await;
    grant_role(admin.id, "admin").await;
    let post = |u: &User, path: String, body: Value| {
        let app = app.clone();
        let h = u.headers();
        async move { call(&app, "POST", &path, Some(body), &hv(&h)).await }
    };
    for (i, u) in users.iter().enumerate() {
        let r = call(
            &app,
            "POST",
            "/api/p/ledger/v1/admin/adjustments",
            Some(json!({ "user_id": u.id, "currency": "bonus", "amount": 1000, "reason_note": "lottery test" })),
            &hv(&[admin.headers(), vec![("idempotency-key", format!("lot-fund-{i}"))]].concat()),
        )
        .await;
        assert!(r.status.is_success(), "{:?}", r.body);
    }
    let [alice, bob, carol] = &users;

    // 开期：参数校验
    let mk = |body: Value| post(&admin, "/api/p/lottery/v1/admin/rounds".into(), body);
    let base = json!({ "title": "第一期", "ticket_price": 10, "max_per_user": 5, "min_tickets": 3, "house_cut_pct": 10,
                       "tiers": [{ "count": 1, "pct": 50 }, { "count": 1, "pct": 20 }], "closes_at": now_ms() + 3_600_000 });
    let mut bad = base.clone();
    bad["tiers"] = json!([{ "count": 1, "pct": 80 }, { "count": 1, "pct": 30 }]);
    assert_eq!(mk(bad).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(post(alice, "/api/p/lottery/v1/admin/rounds".into(), base.clone()).await.status, StatusCode::FORBIDDEN);
    let r = mk(base.clone()).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let rid = r.body["id"].as_i64().unwrap();
    let commit = r.body["commit"].as_str().unwrap().to_owned();
    let buy = |u: &User, rid: i64, count: i64| post(u, format!("/api/p/lottery/v1/rounds/{rid}/tickets"), json!({ "count": count }));

    // 默认关闭；开启后仍有注册天数限制
    assert_eq!(buy(alice, rid, 1).await.body["code"], "lottery.disabled");
    put_settings(&app, &admin, json!({ "enabled": true })).await;
    assert_eq!(buy(alice, rid, 1).await.body["code"], "lottery.not_eligible");
    put_settings(&app, &admin, json!({ "enabled": true, "min_account_days": 0 })).await;

    // 购票：扣款、票号连续、限购
    let start = balance(&app, alice).await;
    let r = buy(alice, rid, 2).await;
    assert_eq!((r.status, r.body["first_no"].clone(), r.body["cost"].clone()), (StatusCode::CREATED, json!(1), json!(20)), "{:?}", r.body);
    assert_eq!(balance(&app, alice).await, start - 20);
    assert_eq!(buy(bob, rid, 3).await.body["first_no"], 3);
    assert_eq!(buy(bob, rid, 3).await.body["code"], "lottery.limit_reached");
    assert_eq!(buy(carol, rid, 1).await.body["first_no"], 6);
    let d = call(&app, "GET", &format!("/api/p/lottery/v1/rounds/{rid}"), None, &hv(&bob.headers())).await.body;
    assert_eq!((d["total_tickets"].as_i64(), d["pool"].as_i64()), (Some(6), Some(60)), "{d:?}");
    assert_eq!(d["my_tickets"], json!([{ "first_no": 3, "count": 3, "cost": 30, "refunded": false }]));
    assert_eq!(d["tiers"][0]["amount"], 27);
    // 开奖前不公开 seed
    let v = call(&app, "GET", &format!("/api/p/lottery/v1/rounds/{rid}/verify"), None, &hv(&bob.headers())).await.body;
    assert_eq!((v["seed"].clone(), v["commit"].clone()), (Value::Null, json!(commit)));

    // 立即开奖：两个不同的中奖票号，奖金 ⌊60 × 90% × 50%⌋ = 27、⌊60 × 90% × 20%⌋ = 10
    let before: Vec<i64> = balances_of(&app, &users).await;
    let r = post(&admin, format!("/api/p/lottery/v1/admin/rounds/{rid}/draw"), json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let winners = r.body["winners"].as_array().unwrap().clone();
    assert_eq!(winners.len(), 2, "{winners:?}");
    assert_ne!(winners[0]["ticket_no"], winners[1]["ticket_no"]);
    assert_eq!((winners[0]["prize"].as_i64(), winners[1]["prize"].as_i64()), (Some(27), Some(10)));
    let after: Vec<i64> = balances_of(&app, &users).await;
    let gained: i64 = before.iter().zip(&after).map(|(b, a)| a - b).sum();
    assert_eq!(gained, 37);
    let v = call(&app, "GET", &format!("/api/p/lottery/v1/rounds/{rid}/verify"), None, &hv(&bob.headers())).await.body;
    let seed: Vec<u8> = (0..32).map(|i| u8::from_str_radix(&v["seed"].as_str().unwrap()[i * 2..i * 2 + 2], 16).unwrap()).collect();
    let digest: String = Sha256::digest(&seed).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(digest, commit);
    assert_eq!(v["batches"].as_array().unwrap().len(), 3);
    assert_eq!(buy(alice, rid, 1).await.body["code"], "lottery.closed");
    assert_eq!(post(&admin, format!("/api/p/lottery/v1/admin/rounds/{rid}/draw"), json!({})).await.body["code"], "lottery.closed");

    // 管理员取消：退款
    let mut second = base.clone();
    second["title"] = json!("第二期");
    let r2 = mk(second).await.body["id"].as_i64().unwrap();
    let b0 = balance(&app, alice).await;
    buy(alice, r2, 4).await;
    assert_eq!(balance(&app, alice).await, b0 - 40);
    assert_eq!(post(&admin, format!("/api/p/lottery/v1/admin/rounds/{r2}/cancel"), json!({ "reason": "" })).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = post(&admin, format!("/api/p/lottery/v1/admin/rounds/{r2}/cancel"), json!({ "reason": "测试取消" })).await;
    assert_eq!(r.body["refunded_batches"], 1, "{:?}", r.body);
    assert_eq!(balance(&app, alice).await, b0);

    // 到期票数不足：定时任务取消并退款
    let mut third = base.clone();
    third["title"] = json!("第三期");
    let r3 = mk(third).await.body["id"].as_i64().unwrap();
    buy(carol, r3, 1).await;
    sqlx::query("UPDATE lottery.rounds SET closes_at = now() - interval '1 second', opens_at = now() - interval '1 hour' WHERE id = $1")
        .bind(r3)
        .execute(&mut db().await)
        .await
        .unwrap();
    assert_eq!(buy(alice, r3, 1).await.body["code"], "lottery.closed");
    kernel.run_job_now("lottery", "tick", json!({})).await.unwrap();
    let d = call(&app, "GET", &format!("/api/p/lottery/v1/rounds/{r3}"), None, &hv(&carol.headers())).await.body;
    assert_eq!((d["state"].clone(), d["cancel_reason"].clone()), (json!("cancelled"), json!("not_enough_tickets")), "{d:?}");
    assert_eq!(d["my_tickets"][0]["refunded"], true);

    // 我的中奖与列表
    let rounds = call(&app, "GET", "/api/p/lottery/v1/rounds", None, &hv(&alice.headers())).await.body;
    assert_eq!(rounds["items"].as_array().unwrap().len(), 3);
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM kernel.audit_log WHERE action LIKE 'lottery.round_%'").fetch_one(&mut db().await).await.unwrap();
    assert!(audit >= 5, "{audit}");
}

async fn balances_of(app: &axum::Router, users: &[User]) -> Vec<i64> {
    let mut v = Vec::new();
    for u in users {
        v.push(balance(app, u).await);
    }
    v
}
