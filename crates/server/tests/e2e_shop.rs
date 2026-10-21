//! 端到端：魔力值商城（shop，WASM）—— 上下架、购买上传量 / 邀请名额 / 促销卡 / 消除 H&R、余额不足、限购与库存、
//! 履约失败退回魔力值、订单列表与权限。
//!
//! 运行：`cargo test -p pt-server --test e2e_shop -- --test-threads=1`

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
    r.body["balances"].as_array().into_iter().flatten().find(|b| b["currency"] == "bonus").and_then(|b| Some(b["balance"].as_i64()? / 10_i64.pow(b["scale"].as_u64()? as u32))).unwrap_or(0)
}

async fn item_id(app: &axum::Router, admin: &User, kind: &str) -> i64 {
    let r = call(app, "GET", "/api/p/shop/v1/admin/items", None, &hv(&admin.headers())).await;
    r.body["items"].as_array().unwrap().iter().find(|i| i["kind"] == kind).unwrap()["id"].as_i64().unwrap()
}

#[tokio::test]
async fn shop_end_to_end() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!("\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n", dir.display()));
    let kernel = Kernel::start(SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let alice = register_login(&app, "shop_alice").await;
    let admin = register_login(&app, "shop_admin").await;
    grant_role(admin.id, "admin").await;
    let alice_h = alice.headers();
    let ah = hv(&alice_h);
    let buy = |id: i64, body: Value| {
        let app = app.clone();
        let h = alice.headers();
        async move { call(&app, "POST", &format!("/api/p/shop/v1/items/{id}/purchase"), Some(body), &hv(&h)).await }
    };
    let patch = |id: i64, body: Value| {
        let app = app.clone();
        let h = admin.headers();
        async move { call(&app, "PATCH", &format!("/api/p/shop/v1/admin/items/{id}"), Some(body), &hv(&h)).await }
    };

    // 预置商品首次加载即上架（含 VIP）；下架后用户看不到，也买不了
    let items = call(&app, "GET", "/api/p/shop/v1/items", None, &ah).await.body;
    let kinds: Vec<&str> = items["items"].as_array().unwrap().iter().map(|i| i["kind"].as_str().unwrap()).collect();
    assert!(kinds.contains(&"upload_credit") && kinds.contains(&"vip"), "{items:?}");
    let upload = item_id(&app, &admin, "upload_credit").await;
    assert_eq!(patch(upload, json!({ "active": false })).await.status, StatusCode::OK);
    let items = call(&app, "GET", "/api/p/shop/v1/items", None, &ah).await.body;
    assert!(items["items"].as_array().unwrap().iter().all(|i| i["id"].as_i64() != Some(upload)), "{items:?}");
    assert_eq!(buy(upload, json!({})).await.body["code"], "shop.unavailable");
    assert_eq!(call(&app, "GET", "/api/p/shop/v1/admin/items", None, &ah).await.status, StatusCode::FORBIDDEN);

    // 上架；参数校验
    assert_eq!(patch(upload, json!({ "params": { "bytes": 0 } })).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = patch(upload, json!({ "active": true, "price": 100 })).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let items = call(&app, "GET", "/api/p/shop/v1/items", None, &ah).await.body;
    assert_eq!((items["items"][0]["id"].as_i64(), items["items"][0]["price"].as_i64()), (Some(upload), Some(100)), "{items:?}");

    // 余额不足
    let start = balance(&app, &alice).await;
    let r = buy(upload, json!({ "quantity": 10 })).await;
    if start < 1000 {
        assert_eq!((r.status, r.body["code"].clone()), (StatusCode::PAYMENT_REQUIRED, json!("shop.insufficient_funds")), "{:?}", r.body);
    }
    let r = call(
        &app,
        "POST",
        "/api/p/ledger/v1/admin/adjustments",
        Some(json!({ "user_id": alice.id, "currency": "bonus", "amount": 500_000, "reason_note": "shop test" })),
        &hv(&[admin.headers(), vec![("idempotency-key", "shop-fund-1".to_owned())]].concat()),
    )
    .await;
    assert!(r.status.is_success(), "{:?}", r.body);
    let funded = balance(&app, &alice).await;

    // 上传量：扣款、加上传量
    let r = buy(upload, json!({ "quantity": 2 })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["total"], 200);
    assert_eq!(balance(&app, &alice).await, funded - 200);
    let d_up: i64 = sqlx::query_scalar("SELECT d_up FROM accounting.adjustments WHERE user_id = $1 AND caller_plugin = 'shop'")
        .bind(i64::try_from(alice.id).unwrap())
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert_eq!(d_up, 2 * (10_i64 << 30));

    // 邀请名额：每月限购 1
    let invite = item_id(&app, &admin, "invite").await;
    patch(invite, json!({ "active": true })).await;
    assert_eq!(buy(invite, json!({})).await.status, StatusCode::CREATED);
    assert_eq!(buy(invite, json!({})).await.body["code"], "shop.limit_reached");
    let lots: i64 = sqlx::query_scalar("SELECT count(*) FROM invite.quota_lots WHERE user_id = $1 AND source = 'shop' AND expires_at IS NOT NULL")
        .bind(i64::try_from(alice.id).unwrap())
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert_eq!(lots, 1);

    // 促销卡：库存
    let card = item_id(&app, &admin, "promo_card").await;
    patch(card, json!({ "active": true, "stock": 1 })).await;
    assert_eq!(buy(card, json!({ "quantity": 2 })).await.body["code"], "shop.sold_out");
    assert_eq!(buy(card, json!({})).await.status, StatusCode::CREATED);
    assert_eq!(buy(card, json!({})).await.body["code"], "shop.sold_out");
    let cards: i32 = sqlx::query_scalar("SELECT balance FROM promotion.card_balances WHERE user_id = $1 AND card_kind = 'free'")
        .bind(i64::try_from(alice.id).unwrap())
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert_eq!(cards, 1);

    // 履约失败：退回魔力值、不留订单
    let before = balance(&app, &alice).await;
    sqlx::query("UPDATE shop.items SET params = '{\"card_kind\": \"gold\", \"count\": 1}', stock = NULL WHERE id = $1")
        .bind(card)
        .execute(&mut db().await)
        .await
        .unwrap();
    let r = buy(card, json!({})).await;
    assert_eq!((r.status, r.body["code"].clone()), (StatusCode::BAD_GATEWAY, json!("shop.fulfillment_failed")), "{:?}", r.body);
    assert_eq!(balance(&app, &alice).await, before);

    // 消除 H&R：必须是自己的进行中 / 已失败记录
    let hnr = item_id(&app, &admin, "hnr_removal").await;
    patch(hnr, json!({ "active": true })).await;
    let rid: i64 = sqlx::query_scalar(
        "INSERT INTO hnr.records (user_id, torrent_id, completed_at, deadline, state) VALUES ($1, 1, now(), now() + interval '3 days', 'active') RETURNING id",
    )
    .bind(i64::try_from(alice.id).unwrap())
    .fetch_one(&mut db().await)
    .await
    .unwrap();
    assert_eq!(buy(hnr, json!({ "record_id": rid + 1000 })).await.body["code"], "shop.invalid_target");
    let r = buy(hnr, json!({ "record_id": rid })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let state: String = sqlx::query_scalar("SELECT state FROM hnr.records WHERE id = $1").bind(rid).fetch_one(&mut db().await).await.unwrap();
    assert_eq!(state, "forgiven");

    // 订单
    let mine = call(&app, "GET", "/api/p/shop/v1/orders", None, &ah).await.body;
    assert_eq!(mine["items"].as_array().unwrap().len(), 4, "{mine:?}");
    let all = call(&app, "GET", "/api/p/shop/v1/admin/orders", None, &hv(&admin.headers())).await.body;
    assert_eq!(all["items"][0]["username"], "shop_alice");
    assert_eq!(balance(&app, &alice).await, funded - 200 - 8000 - 3000 - 5000);

    // 管理员新建商品
    let r = call(
        &app,
        "POST",
        "/api/p/shop/v1/admin/items",
        Some(json!({ "kind": "promo_card", "title": "双倍卡", "price": 1500, "params": { "card_kind": "double_up", "count": 1 }, "active": true })),
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM kernel.audit_log WHERE action = 'shop.item_create'").fetch_one(&mut db().await).await.unwrap();
    assert_eq!(audit, 1);
}
