//! 端到端：勋章与头衔（medals，WASM）—— 成就自动授予（事件计数、累计数据评估）、管理员授予与收回、限量、
//! 佩戴与头衔、他人勋章墙与批量展示、图标、shop 出售（有效期顺延）。
//!
//! 运行：`cargo test -p pt-server --test e2e_medals -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{User, call, env, grant_role, hv, register_login, reset, send};

async fn db() -> PgConnection {
    PgConnection::connect(&env("PT_TEST_SUPERUSER_URL", "postgres://postgres@127.0.0.1:55432/pt_test")).await.unwrap()
}

async fn get(app: &axum::Router, u: &User, path: &str) -> Value {
    let r = call(app, "GET", path, None, &hv(&u.headers())).await;
    assert_eq!(r.status, StatusCode::OK, "{path}: {:?}", r.body);
    r.body
}

fn holding<'a>(me: &'a Value, name: &str) -> Option<&'a Value> {
    me["holdings"].as_array().unwrap().iter().find(|h| h["name"] == name)
}

#[tokio::test]
async fn medals_end_to_end() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!("\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n", dir.display()));
    let kernel = Kernel::start(SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let alice = register_login(&app, "medal_alice").await;
    let bob = register_login(&app, "medal_bob").await;
    let admin = register_login(&app, "medal_admin").await;
    grant_role(admin.id, "admin").await;
    let post = |u: &User, path: String, body: Value| {
        let app = app.clone();
        let h = u.headers();
        async move { call(&app, "POST", &path, Some(body), &hv(&h)).await }
    };

    // 成就（事件计数）：首次发布即获得“初次发布”，“发种达人”显示进度
    common::upload(&app, &alice, "Medal.Test.2024", &[("a.mkv", 100 << 20)]).await;
    let mut me = Value::Null;
    for _ in 0..50 {
        me = get(&app, &alice, "/api/p/medals/v1/me").await;
        if holding(&me, "初次发布").is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(holding(&me, "初次发布").is_some(), "{me:?}");
    let ach: Vec<&Value> = me["achievements"].as_array().unwrap().iter().filter(|a| a["name"] == "发种达人").collect();
    assert_eq!((ach[0]["value"].as_i64(), ach[0]["held"].as_bool()), (Some(1), Some(false)), "{ach:?}");
    // 未设置佩戴时默认佩戴已持有的勋章
    assert_eq!(me["display"]["worn"][0]["name"], "初次发布");

    // 管理员：新建头衔与限量勋章；授予（含不存在的用户名）
    assert_eq!(post(&alice, "/api/p/medals/v1/admin/items".into(), json!({ "name": "x" })).await.status, StatusCode::FORBIDDEN);
    let r = post(&admin, "/api/p/medals/v1/admin/items".into(), json!({ "kind": "title", "name": "元老", "description": "开站元老" })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let elder = r.body["id"].as_i64().unwrap();
    let rare = post(&admin, "/api/p/medals/v1/admin/items".into(), json!({ "name": "限量纪念", "max_holders": 1 })).await.body["id"].as_i64().unwrap();
    let r = post(&admin, "/api/p/medals/v1/admin/grants".into(), json!({ "item_id": elder, "usernames": ["medal_alice", "nobody_here"], "reason": "开站元老" })).await;
    assert_eq!((r.status, r.body["granted"].clone(), r.body["missing"].clone()), (StatusCode::CREATED, json!(1), json!(["nobody_here"])), "{:?}", r.body);
    assert_eq!(post(&admin, "/api/p/medals/v1/admin/grants".into(), json!({ "item_id": rare, "usernames": ["medal_alice"], "reason": "r" })).await.status, StatusCode::CREATED);
    let r = post(&admin, "/api/p/medals/v1/admin/grants".into(), json!({ "item_id": rare, "usernames": ["medal_bob"], "reason": "r" })).await;
    assert_eq!(r.body["code"], "medals.limit_reached", "{:?}", r.body);

    // 佩戴与头衔
    let first = holding(&get(&app, &alice, "/api/p/medals/v1/me").await, "初次发布").unwrap()["id"].as_i64().unwrap();
    let put = |body: Value| {
        let app = app.clone();
        let h = alice.headers();
        async move { call(&app, "PUT", "/api/p/medals/v1/me/display", Some(body), &hv(&h)).await }
    };
    assert_eq!(put(json!({ "worn": [elder] })).await.body["code"], "medals.not_held");
    let r = put(json!({ "worn": [rare, first], "title": elder })).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!((r.body["worn"][0]["id"].as_i64(), r.body["title"]["name"].clone()), (Some(rare), json!("元老")));
    let wall = get(&app, &bob, &format!("/api/p/medals/v1/users/{}", alice.id)).await;
    assert_eq!(wall["display"]["title"]["name"], "元老");
    assert_eq!(wall["holdings"].as_array().unwrap().len(), 3, "{wall:?}");
    let show = get(&app, &bob, &format!("/api/p/medals/v1/show?user_ids={},{}", alice.id, bob.id)).await;
    assert_eq!(show[alice.id.to_string()]["worn"].as_array().unwrap().len(), 2, "{show:?}");
    assert_eq!(show[bob.id.to_string()], json!({ "worn": [], "title": null }));

    // 隐藏物品不出现在他人勋章墙
    let r = call(&app, "PATCH", &format!("/api/p/medals/v1/admin/items/{rare}"), Some(json!({ "hidden": true })), &hv(&admin.headers())).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(get(&app, &bob, &format!("/api/p/medals/v1/users/{}", alice.id)).await["holdings"].as_array().unwrap().len(), 2);

    // 收回：从持有和佩戴中消失
    let holders = get(&app, &admin, &format!("/api/p/medals/v1/admin/items/{rare}/holders")).await;
    let gid = holders["items"][0]["grant_id"].as_i64().unwrap();
    assert_eq!(post(&admin, format!("/api/p/medals/v1/admin/grants/{gid}/revoke"), json!({ "reason": "" })).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(post(&admin, format!("/api/p/medals/v1/admin/grants/{gid}/revoke"), json!({ "reason": "误发" })).await.status, StatusCode::NO_CONTENT);
    let me = get(&app, &alice, "/api/p/medals/v1/me").await;
    assert!(holding(&me, "限量纪念").is_none());
    assert_eq!(me["display"]["worn"].as_array().unwrap().len(), 1, "{me:?}");
    // 名额释放后可以授予别人
    assert_eq!(post(&admin, "/api/p/medals/v1/admin/grants".into(), json!({ "item_id": rare, "usernames": ["medal_bob"], "reason": "r" })).await.status, StatusCode::CREATED);

    // 累计数据成就：立即评估
    sqlx::query("INSERT INTO accounting.user_totals (user_id, uploaded_traffic) VALUES ($1, $2) ON CONFLICT (user_id) DO UPDATE SET uploaded_traffic = $2")
        .bind(i64::try_from(bob.id).unwrap())
        .bind(2_i64 << 40)
        .execute(&mut db().await)
        .await
        .unwrap();
    let rules = get(&app, &admin, "/api/p/medals/v1/admin/rules").await;
    let stat = rules["items"].as_array().unwrap().iter().find(|r| r["kind"] == "stat").unwrap()["id"].as_i64().unwrap();
    let r = post(&admin, format!("/api/p/medals/v1/admin/rules/{stat}/evaluate"), json!({})).await;
    assert_eq!(r.body["granted"], 1, "{:?}", r.body);
    assert_eq!(post(&admin, format!("/api/p/medals/v1/admin/rules/{stat}/evaluate"), json!({})).await.body["granted"], 0);
    assert!(holding(&get(&app, &bob, "/api/p/medals/v1/me").await, "1 TiB 上传").is_some());
    assert_eq!(post(&admin, "/api/p/medals/v1/admin/rules".into(), json!({ "item_id": rare, "kind": "event", "event": "nope", "threshold": 1 })).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = post(&admin, "/api/p/medals/v1/admin/rules".into(), json!({ "item_id": first, "kind": "event", "event": "checkin.checked_in", "measure": "max:streak", "threshold": 7 })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);

    // 图标
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01\x08\x06\0\0\0\x1f\x15\xc4\x89".to_vec();
    let r = send(&app, "POST", &format!("/api/p/medals/v1/admin/items/{first}/icon"), &hv(&admin.headers()), |b| {
        b.header("content-type", "image/png").body(axum::body::Body::from(png.clone())).unwrap()
    })
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let icon = r.body["icon"].as_str().unwrap().to_owned();
    assert!(icon.starts_with("/media/medals/icons/"), "{icon}");
    assert_eq!(call(&app, "GET", &icon, None, &[]).await.status, StatusCode::OK);
    let r = send(&app, "POST", &format!("/api/p/medals/v1/admin/items/{first}/icon"), &hv(&admin.headers()), |b| {
        b.header("content-type", "image/svg+xml").body(axum::body::Body::from("<svg/>")).unwrap()
    })
    .await;
    assert_eq!(r.body["code"], "medals.bad_icon");

    // shop 出售：有效期 30 天的勋章，再买一次顺延
    let vip = post(&admin, "/api/p/medals/v1/admin/items".into(), json!({ "name": "月度之星", "validity_days": 30 })).await.body["id"].as_i64().unwrap();
    let r = post(&admin, "/api/p/shop/v1/admin/items".into(), json!({ "kind": "medal", "title": "月度之星", "price": 10, "params": { "medal_id": vip }, "active": true })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let shop_item = r.body["id"].as_i64().unwrap();
    let r = call(
        &app,
        "POST",
        "/api/p/ledger/v1/admin/adjustments",
        Some(json!({ "user_id": alice.id, "currency": "bonus", "amount": 1000, "reason_note": "medal test" })),
        &hv(&[admin.headers(), vec![("idempotency-key", "medal-fund-1".to_owned())]].concat()),
    )
    .await;
    assert!(r.status.is_success(), "{:?}", r.body);
    for _ in 0..2 {
        let r = post(&alice, format!("/api/p/shop/v1/items/{shop_item}/purchase"), json!({})).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    }
    let me = get(&app, &alice, "/api/p/medals/v1/me").await;
    let exp = holding(&me, "月度之星").unwrap()["expires_at"].as_i64().unwrap();
    let days = (exp - chrono_now_ms()) / 86_400_000;
    assert!((59..=60).contains(&days), "{days}");

    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM kernel.audit_log WHERE action LIKE 'medals.%'").fetch_one(&mut db().await).await.unwrap();
    assert!(audit >= 6, "{audit}");
}

fn chrono_now_ms() -> i64 {
    i64::try_from(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()).unwrap()
}
