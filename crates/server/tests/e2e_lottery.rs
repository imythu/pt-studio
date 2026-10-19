//! 端到端：幸运转盘（lottery，WASM）—— 默认关闭、注册天数限制、奖品与概率校验、各类奖品发放（魔力值、上传量、
//! VIP 天数、永久 VIP）、库存、每日次数、魔力值不足、未中奖、删除奖品。
//!
//! 运行：`cargo test -p pt-server --test e2e_lottery -- --test-threads=1`

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
    let alice = register_login(&app, "lot_alice").await;
    let carol = register_login(&app, "lot_carol").await;
    let admin = register_login(&app, "lot_admin").await;
    grant_role(admin.id, "admin").await;
    let r = call(
        &app,
        "POST",
        "/api/p/ledger/v1/admin/adjustments",
        Some(json!({ "user_id": alice.id, "currency": "bonus", "amount": 1000, "reason_note": "lottery test" })),
        &hv(&[admin.headers(), vec![("idempotency-key", "lot-fund-alice".to_owned())]].concat()),
    )
    .await;
    assert!(r.status.is_success(), "{:?}", r.body);

    let spin = |u: &User| {
        let app = app.clone();
        let h = u.headers();
        async move { call(&app, "POST", "/api/p/lottery/v1/wheel/spin", Some(json!({})), &hv(&h)).await }
    };
    let save = |u: &User, prizes: Value| {
        let app = app.clone();
        let h = u.headers();
        async move { call(&app, "PUT", "/api/p/lottery/v1/admin/wheel/prizes", Some(json!({ "prizes": prizes })), &hv(&h)).await }
    };

    // 迁移带默认奖品；用户看到的转盘不含库存等后台字段
    let w = call(&app, "GET", "/api/p/lottery/v1/wheel", None, &hv(&alice.headers())).await.body;
    assert_eq!(w["enabled"], false);
    assert_eq!(w["prizes"].as_array().unwrap().len(), 7, "{w:?}");
    assert!(w["prizes"][0].get("stock").is_none());
    assert!(w["none_chance_ppm"].as_i64().unwrap() > 0);

    // 默认关闭；开启后仍有注册天数限制
    assert_eq!(spin(&alice).await.body["code"], "lottery.disabled");
    put_settings(&app, &admin, json!({ "enabled": true, "spin_cost": 50, "daily_limit": 10 })).await;
    assert_eq!(spin(&alice).await.body["code"], "lottery.not_eligible");
    put_settings(&app, &admin, json!({ "enabled": true, "spin_cost": 50, "daily_limit": 10, "min_account_days": 0 })).await;

    // 奖品校验：概率之和不超过 100%、数量范围、仅管理员
    let over = json!([{ "kind": "bonus", "amount": 1, "chance_ppm": 600_000 }, { "kind": "bonus", "amount": 2, "chance_ppm": 500_000 }]);
    assert_eq!(save(&admin, over).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let bad = json!([{ "kind": "vip_days", "amount": 0, "chance_ppm": 1 }]);
    assert_eq!(save(&admin, bad).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(save(&alice, json!([])).await.status, StatusCode::FORBIDDEN);

    // 魔力值：扣 50、得 300
    let r = save(&admin, json!([{ "kind": "bonus", "amount": 300, "chance_ppm": 1_000_000 }])).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["prizes"].as_array().unwrap().len(), 1, "old prizes are removed");
    let b0 = balance(&app, &alice).await;
    let r = spin(&alice).await;
    assert_eq!((r.status, r.body["kind"].clone(), r.body["amount"].clone()), (StatusCode::CREATED, json!("bonus"), json!(300)), "{:?}", r.body);
    assert_eq!(balance(&app, &alice).await, b0 + 250);

    // 上传量
    let gib = 1_i64 << 30;
    let bonus_id = r.body["prize_id"].as_i64().unwrap();
    save(&admin, json!([{ "id": bonus_id, "kind": "bonus", "amount": 300, "chance_ppm": 0 }, { "kind": "upload", "amount": gib, "chance_ppm": 1_000_000 }])).await;
    let r = spin(&alice).await;
    assert_eq!(r.body["kind"], "upload", "{:?}", r.body);
    let up: i64 = sqlx::query_scalar("SELECT uploaded_adjust FROM accounting.user_totals WHERE user_id = $1")
        .bind(i64::try_from(alice.id).unwrap())
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert_eq!(up, gib);

    // VIP 天数（顺延），之后永久 VIP（限量 1）
    save(&admin, json!([{ "kind": "vip_days", "amount": 7, "chance_ppm": 1_000_000 }])).await;
    assert_eq!(spin(&alice).await.body["kind"], "vip_days");
    let days: f64 = sqlx::query_scalar(
        "SELECT extract(epoch FROM until - now())::float8 / 86400 FROM levels.overrides WHERE user_id = $1 AND kind = 'vip' AND ended_at IS NULL",
    )
    .bind(i64::try_from(alice.id).unwrap())
    .fetch_one(&mut db().await)
    .await
    .unwrap();
    assert!((6.9..7.1).contains(&days), "{days}");
    let r = save(&admin, json!([{ "kind": "vip_permanent", "amount": 0, "chance_ppm": 1_000_000, "stock": 1 }])).await;
    let perm_id = r.body["prizes"][0]["id"].as_i64().unwrap();
    assert_eq!(spin(&alice).await.body["kind"], "vip_permanent");
    let open: Vec<Option<f64>> = sqlx::query_scalar(
        "SELECT extract(epoch FROM until)::float8 FROM levels.overrides WHERE user_id = $1 AND kind = 'vip' AND ended_at IS NULL",
    )
    .bind(i64::try_from(alice.id).unwrap())
    .fetch_all(&mut db().await)
    .await
    .unwrap();
    assert_eq!(open, vec![None], "the timed VIP is replaced by a permanent one");
    let a = call(&app, "GET", "/api/p/lottery/v1/admin/wheel", None, &hv(&admin.headers())).await.body;
    assert_eq!(a["prizes"][0]["stock"], 0, "{a:?}");
    // 库存用完：转盘上不再出现，抽到的是“谢谢参与”，仍然扣费
    let w = call(&app, "GET", "/api/p/lottery/v1/wheel", None, &hv(&alice.headers())).await.body;
    assert_eq!(w["prizes"].as_array().unwrap().len(), 0);
    assert_eq!(w["none_chance_ppm"], 1_000_000);
    let b1 = balance(&app, &alice).await;
    let r = spin(&alice).await;
    assert_eq!((r.status, r.body["prize_id"].clone()), (StatusCode::CREATED, Value::Null), "{:?}", r.body);
    assert_eq!(balance(&app, &alice).await, b1 - 50);

    // 每日次数
    let today = r.body["spins_today"].as_i64().unwrap();
    assert_eq!(today, 5);
    put_settings(&app, &admin, json!({ "enabled": true, "spin_cost": 50, "daily_limit": 5, "min_account_days": 0 })).await;
    assert_eq!(spin(&alice).await.body["code"], "lottery.limit_reached");

    // 魔力值不足：不扣费、无记录
    assert_eq!(spin(&carol).await.body["code"], "lottery.insufficient_funds");
    let w = call(&app, "GET", "/api/p/lottery/v1/wheel", None, &hv(&carol.headers())).await.body;
    assert_eq!(w["spins_today"], 0);

    // 我的记录、最近中奖、删除奖品
    let me = call(&app, "GET", "/api/p/lottery/v1/me", None, &hv(&alice.headers())).await.body;
    assert_eq!(me["items"].as_array().unwrap().len(), 5);
    let w = call(&app, "GET", "/api/p/lottery/v1/wheel", None, &hv(&carol.headers())).await.body;
    assert_eq!(w["recent_wins"].as_array().unwrap().len(), 4);
    assert_eq!(w["recent_wins"][0]["username"], "lot_alice");
    let r = save(&admin, json!([])).await;
    assert_eq!(r.body["prizes"], json!([]));
    let deleted: Option<bool> = sqlx::query_scalar("SELECT deleted_at IS NOT NULL FROM lottery.wheel_prizes WHERE id = $1")
        .bind(perm_id)
        .fetch_optional(&mut db().await)
        .await
        .unwrap();
    assert_eq!(deleted, Some(true));
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM kernel.audit_log WHERE action = 'lottery.prizes_update'")
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert!(audit >= 5, "{audit}");
}
