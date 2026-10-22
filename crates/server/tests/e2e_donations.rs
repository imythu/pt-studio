//! 端到端：捐赠（donations，WASM）与 VIP 授予（levels.grant_vip）—— 说明页与开放开关、登记捐赠（按设置计算默认奖励、
//! VIP 顺延、魔力值、勋章）、公开名单与本月进度、权限、撤销、商城 VIP 商品顺延、VIP 到期自动回到原等级、到期提醒；
//! 魔力值流水接口能看到捐赠奖励。
//!
//! 运行：`cargo test -p pt-server --test e2e_donations -- --test-threads=1`

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

async fn put_settings(app: &axum::Router, admin: &User, plugin: &str, settings: Value) {
    let path = format!("/api/kernel/v1/admin/plugins/{plugin}/settings");
    let version = call(app, "GET", &path, None, &hv(&admin.headers())).await.body["version"].as_i64().unwrap_or(0);
    let r = call(app, "PUT", &path, Some(json!({ "settings": settings, "version": version })), &hv(&admin.headers())).await;
    assert!(r.status.is_success(), "{:?}", r.body);
}

async fn class_key(app: &axum::Router, u: &User) -> String {
    call(app, "GET", "/api/p/levels/v1/me", None, &hv(&u.headers())).await.body["class"]["key"].as_str().unwrap_or("").to_owned()
}

const DAY: i64 = 86_400_000;

fn now_ms() -> i64 {
    i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()).unwrap()
}

#[tokio::test]
async fn donations_end_to_end() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!("\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n", dir.display()));
    let kernel = Kernel::start(SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let alice = register_login(&app, "dn_alice").await;
    let bob = register_login(&app, "dn_bob").await;
    let admin = register_login(&app, "dn_admin").await;
    grant_role(admin.id, "admin").await;
    let post = |u: &User, path: String, body: Value| {
        let app = app.clone();
        let h = u.headers();
        async move { call(&app, "POST", &path, Some(body), &hv(&h)).await }
    };
    let summary = |u: &User| {
        let app = app.clone();
        let h = u.headers();
        async move { call(&app, "GET", "/api/p/donations/v1/summary", None, &hv(&h)).await.body }
    };

    // 默认未开放；开放并设置说明、目标与默认奖励比例
    let s = summary(&alice).await;
    assert_eq!(s["enabled"], false, "{s:?}");
    put_settings(
        &app,
        &admin,
        "donations",
        json!({ "enabled": true, "instructions": "扫码捐赠，**备注用户名**", "monthly_goal": 500, "vip_days_per_unit": 1, "bonus_per_unit": 10 }),
    )
    .await;
    let s = summary(&alice).await;
    assert!(s["instructions_html"].as_str().unwrap().contains("<strong>备注用户名</strong>"), "{s:?}");
    assert_eq!((s["monthly_goal"].as_i64(), s["month_total"].as_i64(), s["vip_until"].clone()), (Some(50_000), Some(0), Value::Null));

    // 权限与校验
    assert_eq!(post(&alice, "/api/p/donations/v1/admin/donations".into(), json!({ "username": "dn_alice", "amount": 100 })).await.status, StatusCode::FORBIDDEN);
    assert_eq!(post(&admin, "/api/p/donations/v1/admin/donations".into(), json!({ "username": "nobody", "amount": 100 })).await.body["code"], "donations.user_not_found");
    assert_eq!(post(&admin, "/api/p/donations/v1/admin/donations".into(), json!({ "username": "dn_alice", "amount": 0 })).await.status, StatusCode::UNPROCESSABLE_ENTITY);

    // 登记：¥30 → 默认 30 天 VIP、300 魔力值；公开
    assert_eq!(class_key(&app, &alice).await, "user");
    let t0 = now_ms();
    let r = post(&admin, "/api/p/donations/v1/admin/donations".into(), json!({ "username": "DN_Alice", "amount": 3000, "method": "支付宝", "public": true })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!((r.body["vip_days"].as_i64(), r.body["bonus"].as_i64(), r.body["failed"].clone()), (Some(30), Some(300), json!([])), "{:?}", r.body);
    let until1 = r.body["vip_until"].as_i64().unwrap();
    assert!((until1 - t0 - 30 * DAY).abs() < 60_000, "{until1}");
    assert_eq!(class_key(&app, &alice).await, "vip");
    // 第二笔：显式指定 10 天、不给魔力值、不公开 → 在已有 VIP 之后顺延
    let r = post(&admin, "/api/p/donations/v1/admin/donations".into(), json!({ "user_id": alice.id, "amount": 1000, "vip_days": 10, "bonus": 0 })).await;
    assert_eq!(r.body["vip_until"].as_i64(), Some(until1 + 10 * DAY), "{:?}", r.body);
    // 等级接口带上 VIP 到期时间（个人页、等级说明页显示）
    let me = call(&app, "GET", "/api/p/levels/v1/me", None, &hv(&alice.headers())).await.body;
    assert_eq!(me["class"]["until"].as_i64(), Some(until1 + 10 * DAY), "{me:?}");
    let d2 = r.body["id"].as_i64().unwrap();

    // 说明页：本月进度、公开名单、我的记录、VIP 到期时间
    let s = summary(&bob).await;
    assert_eq!((s["month_total"].as_i64(), s["donors"].clone(), s["mine"].clone()), (Some(4000), json!(["dn_alice"]), json!([])), "{s:?}");
    let s = summary(&alice).await;
    assert_eq!((s["mine"].as_array().unwrap().len(), s["vip_until"].as_i64()), (2, Some(until1 + 10 * DAY)), "{s:?}");

    // 魔力值流水：捐赠奖励
    let e = call(&app, "GET", "/api/p/ledger/v1/users/me/entries?currency=bonus", None, &hv(&alice.headers())).await.body;
    assert!(e["items"].as_array().unwrap().iter().any(|x| x["reason"] == "donations.reward" && x["amount"] == 3000), "{e:?}");
    // 按页码分页（魔力值页「魔力明细」）：每页 1 条，第 2 页与列表第 2 条一致，并带总数
    let n = e["items"].as_array().unwrap().len();
    let p2 = call(&app, "GET", "/api/p/ledger/v1/users/me/entries?currency=bonus&page=2&limit=1", None, &hv(&alice.headers())).await.body;
    assert_eq!(p2["total"].as_u64(), Some(n as u64), "{p2:?}");
    assert_eq!(p2["items"].as_array().unwrap().len(), usize::from(n >= 2), "{p2:?}");
    if n >= 2 {
        assert_eq!(p2["items"][0]["entry_id"], e["items"][1]["entry_id"], "{p2:?}");
    }
    let notified: i64 = sqlx::query_scalar("SELECT count(*) FROM notify.notifications WHERE user_id = $1 AND template_key = 'donations.thanks'")
        .bind(i64::try_from(alice.id).unwrap())
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert_eq!(notified, 2);

    // 撤销：排除出统计，奖励保留
    assert_eq!(post(&admin, format!("/api/p/donations/v1/admin/donations/{d2}/revoke"), json!({ "reason": "重复登记" })).await.status, StatusCode::NO_CONTENT);
    assert_eq!(post(&admin, format!("/api/p/donations/v1/admin/donations/{d2}/revoke"), json!({})).await.status, StatusCode::NOT_FOUND);
    let a = call(&app, "GET", "/api/p/donations/v1/admin/donations", None, &hv(&admin.headers())).await.body;
    assert_eq!((a["month_total"].as_i64(), a["items"][0]["revoked_at"].is_i64(), a["items"][1]["created_by"].clone()), (Some(3000), true, json!("dn_admin")), "{a:?}");

    // 商城 VIP 商品（预置之外再上架一个）：bob 购买 2 × 7 天
    let r = post(&admin, "/api/p/shop/v1/admin/items".into(), json!({ "kind": "vip", "title": "VIP 7 天", "price": 100, "params": { "days": 7 }, "active": true })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let vip_item = r.body["id"].as_i64().unwrap();
    let r = call(
        &app,
        "POST",
        "/api/p/ledger/v1/admin/adjustments",
        Some(json!({ "user_id": bob.id, "currency": "bonus", "amount": 10_000, "reason_note": "donations test" })),
        &hv(&[admin.headers(), vec![("idempotency-key", "dn-fund-1".to_owned())]].concat()),
    )
    .await;
    assert!(r.status.is_success(), "{:?}", r.body);
    let r = post(&bob, format!("/api/p/shop/v1/items/{vip_item}/purchase"), json!({ "quantity": 2 })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(class_key(&app, &bob).await, "vip");
    let until: i64 = sqlx::query_scalar("SELECT (extract(epoch FROM until) * 1000)::bigint FROM levels.overrides WHERE user_id = $1 AND ended_at IS NULL")
        .bind(i64::try_from(bob.id).unwrap())
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert!((until - now_ms() - 14 * DAY).abs() < 60_000, "{until}");

    // 到期提醒：bob 的 VIP 还剩 2 天（不是捐赠用户，不提醒）；alice 的改成 2 天后到期 → 提醒一次
    sqlx::query("UPDATE levels.overrides SET until = now() + interval '2 days' WHERE ended_at IS NULL AND kind = 'vip'")
        .execute(&mut db().await)
        .await
        .unwrap();
    kernel.run_job_now("donations", "remind", json!({})).await.unwrap();
    kernel.run_job_now("donations", "remind", json!({})).await.unwrap();
    let reminded: Vec<i64> = sqlx::query_scalar("SELECT user_id FROM notify.notifications WHERE template_key = 'donations.vip_expiring'")
        .fetch_all(&mut db().await)
        .await
        .unwrap();
    assert_eq!(reminded, vec![i64::try_from(alice.id).unwrap()]);

    // 到期：incr_evaluate 结束过期覆盖，回到原等级
    sqlx::query("UPDATE levels.overrides SET until = now() - interval '1 minute' WHERE ended_at IS NULL AND kind = 'vip'")
        .execute(&mut db().await)
        .await
        .unwrap();
    kernel.run_job_now("levels", "incr_evaluate", json!({})).await.unwrap();
    assert_eq!((class_key(&app, &alice).await, class_key(&app, &bob).await), ("user".to_owned(), "user".to_owned()));
    let open: i64 = sqlx::query_scalar("SELECT count(*) FROM levels.overrides WHERE ended_at IS NULL").fetch_one(&mut db().await).await.unwrap();
    assert_eq!(open, 0);
    assert_eq!(summary(&alice).await["vip_until"], Value::Null);
}
