//! 端到端：ledger 批量发放 —— 预览（按条件 / 名单、找不到的用户名、封禁用户不计入）、创建（幂等键重放、审计）、
//! 后台分块发放到账、逐人系统站内信（模板变量）、详情与列表、已完成的批不能继续、无权限用户被拒；
//! 批量扣除：预览余额不足人数与预计总额、余额不足跳过、有多少扣多少、扣除通知按实际数量、不形成负余额。

#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{User, call, env, hv, register_login, reset};

fn config(dir: &std::path::Path) -> SiteConfig {
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n",
        dir.display()
    ));
    SiteConfig::parse(&t).unwrap()
}

async fn db() -> PgConnection {
    PgConnection::connect(&env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap()
}

async fn post(app: &axum::Router, u: &User, path: &str, body: Value, key: Option<&str>) -> common::Resp {
    let mut h = u.headers();
    if let Some(k) = key {
        h.push(("idempotency-key", k.to_owned()));
    }
    call(app, "POST", path, Some(body), &hv(&h)).await
}

async fn wait_done(app: &axum::Router, admin: &User, id: i64) -> Value {
    let mut d = Value::Null;
    for _ in 0..300 {
        d = call(app, "GET", &format!("/api/p/ledger/v1/admin/grants/{id}"), None, &hv(&admin.headers())).await.body;
        if d["state"] == "done" {
            return d;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("grant {id} did not finish: {d:?}");
}

async fn bonus_of(app: &axum::Router, admin: &User, uid: u64) -> i64 {
    let r = call(
        app,
        "GET",
        &format!("/api/p/ledger/v1/admin/users/{uid}/balances"),
        None,
        &hv(&admin.headers()),
    )
    .await;
    r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["currency"] == "bonus")
        .map_or(0, |b| b["balance"].as_i64().unwrap())
}

#[tokio::test]
async fn grants_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let dir = common::package_wasm_plugins();
    reset().await;
    let kernel = Kernel::start(config(&dir), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let admin = register_login(&app, "gr_admin").await;
    common::grant_role(admin.id, "admin").await;
    let alice = register_login(&app, "gr_alice").await;
    let bob = register_login(&app, "gr_bob").await;
    let carol = register_login(&app, "gr_carol").await;
    sqlx::query("UPDATE identity.users SET status = 'banned' WHERE id = $1")
        .bind(i64::try_from(carol.id).unwrap())
        .execute(&mut db().await)
        .await
        .unwrap();
    let base_alice = bonus_of(&app, &admin, alice.id).await;
    let base_carol = bonus_of(&app, &admin, carol.id).await;

    // ---- 预览：默认只含正常状态用户（封禁的 carol 不计入）；名单模式报告找不到的用户名 ----
    let all = json!({ "kind": "filter", "statuses": ["active"] });
    let r = post(&app, &admin, "/api/p/ledger/v1/admin/grants/preview", json!({ "currency": "bonus", "amount": 1000, "target": all }), None).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["count"], 3, "{:?}", r.body);
    assert_eq!(r.body["total_amount"], 3000);
    assert_eq!(r.body["scale"], 1);
    assert_eq!(r.body["notify_available"], true);
    let named = json!({ "kind": "users", "usernames": ["GR_Alice", "gr_carol", "nobody"] });
    let r = post(&app, &admin, "/api/p/ledger/v1/admin/grants/preview", json!({ "currency": "bonus", "amount": 10, "target": named }), None).await;
    assert_eq!(r.body["count"], 1, "{:?}", r.body);
    assert_eq!(r.body["missing"], json!(["gr_carol", "nobody"]));
    let r = post(&app, &admin, "/api/p/ledger/v1/admin/grants/preview", json!({ "currency": "bonus", "amount": 10, "target": { "kind": "filter", "statuses": ["banned"] } }), None).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);

    // ---- 无权限 ----
    let r = post(&app, &alice, "/api/p/ledger/v1/admin/grants", json!({ "currency": "bonus", "amount": 1000, "memo": "x", "target": all }), Some("g-alice")).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{:?}", r.body);

    // ---- 创建：发放 100 魔力（最小单位 1000）并通知 ----
    let body = json!({ "currency": "bonus", "amount": 1000, "memo": "国庆福利", "target": all,
                       "notify": { "subject": "福利：{amount} {currency}", "body": "{username} 收到 {amount}，余额 {balance}。{memo}" } });
    let r = post(&app, &admin, "/api/p/ledger/v1/admin/grants", body.clone(), Some("g-1")).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let id = r.body["id"].as_i64().unwrap();
    assert_eq!(r.body["total"], 3);
    assert_eq!(r.body["state"], "granting");
    // 同一幂等键重放：同一批
    let r = post(&app, &admin, "/api/p/ledger/v1/admin/grants", body, Some("g-1")).await;
    assert_eq!(r.body["id"], id, "{:?}", r.body);

    let mut d = Value::Null;
    for _ in 0..300 {
        d = call(&app, "GET", &format!("/api/p/ledger/v1/admin/grants/{id}"), None, &hv(&admin.headers())).await.body;
        if d["state"] == "done" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(d["state"], "done", "{d:?}");
    assert_eq!((d["granted"].as_i64(), d["failed"].as_i64()), (Some(3), Some(0)), "{d:?}");
    assert_eq!((d["notified"].as_i64(), d["notify_failed"].as_i64()), (Some(3), Some(0)), "{d:?}");
    assert_eq!(d["failures"], json!([]));
    assert_eq!(bonus_of(&app, &admin, alice.id).await, base_alice + 1000);
    assert_eq!(bonus_of(&app, &admin, carol.id).await, base_carol);

    // 流水：原因 ledger.admin_grant、备注为原因
    let e = call(&app, "GET", "/api/p/ledger/v1/users/me/entries?limit=5", None, &hv(&bob.headers())).await.body;
    let it = e["items"].as_array().unwrap().iter().find(|i| i["reason"] == "ledger.admin_grant").cloned().unwrap();
    assert_eq!((it["amount"].as_i64(), it["memo"].as_str()), (Some(1000), Some("国庆福利")));

    // 站内信：系统文件夹一封，标题按模板渲染（金额按小数位格式化、货币显示名）
    let m = call(&app, "GET", "/api/p/messages/v1/conversations?folder=system", None, &hv(&alice.headers())).await.body;
    let items = m["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{m:?}");
    let subject = items[0]["subject"].as_str().unwrap();
    assert!(subject.starts_with("福利：100 "), "{subject}");
    let conv = call(&app, "GET", &format!("/api/p/messages/v1/conversations/{}", items[0]["id"]), None, &hv(&alice.headers())).await.body;
    let text = conv.to_string();
    assert!(text.contains("gr_alice 收到 100") && text.contains("国庆福利"), "{text}");
    let carol_msgs = call(&app, "GET", "/api/p/messages/v1/conversations?folder=system", None, &hv(&carol.headers())).await;
    assert!(carol_msgs.body["items"].as_array().is_none_or(Vec::is_empty), "{:?}", carol_msgs.body);

    // ---- 列表、审计、已完成的批不能继续 ----
    let l = call(&app, "GET", "/api/p/ledger/v1/admin/grants", None, &hv(&admin.headers())).await.body;
    assert_eq!(l["items"][0]["id"], id);
    assert_eq!(l["items"][0]["created_by_name"], "gr_admin");
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM kernel.audit_log WHERE action = 'ledger.grant'")
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert_eq!(audits, 1);
    let r = post(&app, &admin, &format!("/api/p/ledger/v1/admin/grants/{id}/resume"), json!({}), None).await;
    assert_eq!(r.body["code"], "ledger.grant_finished", "{:?}", r.body);

    // ---- 不通知；没有匹配的用户 ----
    let r = post(&app, &admin, "/api/p/ledger/v1/admin/grants",
        json!({ "currency": "bonus", "amount": 5, "memo": "补偿", "target": { "kind": "users", "usernames": ["gr_bob"] } }), Some("g-2")).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let id2 = r.body["id"].as_i64().unwrap();
    for _ in 0..300 {
        d = call(&app, "GET", &format!("/api/p/ledger/v1/admin/grants/{id2}"), None, &hv(&admin.headers())).await.body;
        if d["state"] == "done" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!((d["granted"].as_i64(), d["notified"].as_i64()), (Some(1), Some(0)), "{d:?}");
    let r = post(&app, &admin, "/api/p/ledger/v1/admin/grants",
        json!({ "currency": "bonus", "amount": 5, "memo": "x", "target": { "kind": "users", "usernames": ["nobody"] } }), Some("g-3")).await;
    assert_eq!(r.body["code"], "ledger.grant_no_recipients", "{:?}", r.body);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM ledger.grant_batches")
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert_eq!(n, 2, "failed creation must roll back");

    // ---- 批量扣除 ----
    let (a0, b0, m0) = (
        bonus_of(&app, &admin, alice.id).await,
        bonus_of(&app, &admin, bob.id).await,
        bonus_of(&app, &admin, admin.id).await,
    );
    assert!(a0 > 300 && b0 > 300 && m0 > 300, "{a0} {b0} {m0}");
    let three = json!({ "kind": "users", "usernames": ["gr_alice", "gr_bob", "gr_admin"] });
    // 预览：每人扣 300 → 无人不足；扣比三人余额都多 → 全部不足，partial 预计总额为三人余额之和
    let r = post(&app, &admin, "/api/p/ledger/v1/admin/grants/preview",
        json!({ "currency": "bonus", "amount": 300, "direction": "deduct", "target": three }), None).await;
    assert_eq!((r.body["count"].as_i64(), r.body["insufficient"].as_i64(), r.body["total_amount"].as_i64()), (Some(3), Some(0), Some(900)), "{:?}", r.body);
    assert!(r.body["default_subject"].as_str().unwrap().contains("扣除"), "{:?}", r.body);
    let huge = a0.max(b0).max(m0) + 1;
    let r = post(&app, &admin, "/api/p/ledger/v1/admin/grants/preview",
        json!({ "currency": "bonus", "amount": huge, "direction": "deduct", "shortfall": "partial", "target": three }), None).await;
    assert_eq!((r.body["insufficient"].as_i64(), r.body["total_amount"].as_i64()), (Some(3), Some(a0 + b0 + m0)), "{:?}", r.body);

    // 正常扣除 300，带通知
    let r = post(&app, &admin, "/api/p/ledger/v1/admin/grants",
        json!({ "currency": "bonus", "amount": 300, "memo": "违规回收", "direction": "deduct", "target": three,
                "notify": { "subject": "", "body": "" } }), Some("d-1")).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["direction"], "deduct");
    let d = wait_done(&app, &admin, r.body["id"].as_i64().unwrap()).await;
    assert_eq!((d["granted"].as_i64(), d["failed"].as_i64(), d["moved"].as_i64(), d["notified"].as_i64()), (Some(3), Some(0), Some(900), Some(3)), "{d:?}");
    assert_eq!(bonus_of(&app, &admin, alice.id).await, a0 - 300);
    let e = call(&app, "GET", "/api/p/ledger/v1/users/me/entries?limit=1", None, &hv(&alice.headers())).await.body;
    assert_eq!((e["items"][0]["reason"].as_str(), e["items"][0]["amount"].as_i64()), (Some("ledger.admin_deduct"), Some(-300)), "{e:?}");
    let m = call(&app, "GET", "/api/p/messages/v1/conversations?folder=system", None, &hv(&alice.headers())).await.body;
    let subject = m["items"][0]["subject"].as_str().unwrap();
    assert!(subject.starts_with("你被扣除了 30 "), "{subject}");

    // 余额不足：skip 跳过（不记账、不通知）；partial 扣到 0（通知里是实际数量）
    let b1 = bonus_of(&app, &admin, bob.id).await;
    let r = post(&app, &admin, "/api/p/ledger/v1/admin/grants",
        json!({ "currency": "bonus", "amount": b1 + 1, "memo": "x", "direction": "deduct",
                "target": { "kind": "users", "usernames": ["gr_bob"] }, "notify": {} }), Some("d-2")).await;
    let d = wait_done(&app, &admin, r.body["id"].as_i64().unwrap()).await;
    assert_eq!((d["granted"].as_i64(), d["failed"].as_i64(), d["notified"].as_i64()), (Some(0), Some(1), Some(0)), "{d:?}");
    assert_eq!(d["failures"][0]["error"], "ledger.insufficient_balance");
    assert_eq!(bonus_of(&app, &admin, bob.id).await, b1);
    let r = post(&app, &admin, "/api/p/ledger/v1/admin/grants",
        json!({ "currency": "bonus", "amount": b1 + 1, "memo": "清零", "direction": "deduct", "shortfall": "partial",
                "target": { "kind": "users", "usernames": ["gr_bob"] }, "notify": { "subject": "扣 {amount}", "body": "余额 {balance}" } }), Some("d-3")).await;
    let d = wait_done(&app, &admin, r.body["id"].as_i64().unwrap()).await;
    assert_eq!((d["granted"].as_i64(), d["moved"].as_i64(), d["notified"].as_i64()), (Some(1), Some(b1), Some(1)), "{d:?}");
    assert_eq!(d["failures"][0]["amount"].as_i64(), Some(b1), "partial deductions are listed: {d:?}");
    assert_eq!(bonus_of(&app, &admin, bob.id).await, 0);
    let m = call(&app, "GET", "/api/p/messages/v1/conversations?folder=system", None, &hv(&bob.headers())).await.body;
    let want = format!("扣 {}", grants_fmt(b1));
    assert!(m["items"].as_array().unwrap().iter().any(|i| i["subject"] == want.as_str()), "{want} in {m:?}");
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM kernel.audit_log WHERE action = 'ledger.deduct'")
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert_eq!(audits, 3);
    kernel.shutdown().await;
}

/// scale = 1 的金额显示
fn grants_fmt(v: i64) -> String {
    if v % 10 == 0 { (v / 10).to_string() } else { format!("{}.{}", v / 10, v % 10) }
}
