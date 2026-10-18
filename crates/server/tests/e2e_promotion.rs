//! 端到端：promotion（WASM）—— 单种促销、自动规则、全站活动、个人卡、到期；倍率经 accounting 生效。

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{call, env, hv, register_login, reset, upload};

fn config(dir: &std::path::Path) -> SiteConfig {
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n",
        dir.display()
    ));
    SiteConfig::parse(&t).unwrap()
}

#[tokio::test]
async fn promotion_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter("info,sqlx=warn,cranelift=warn,wasmtime=warn")
        .try_init();
    let dir = common::package_wasm_plugins();
    reset().await;
    let mut rc = redis::Client::open(env("PT_TEST_REDIS_URL", "redis://127.0.0.1:56379/15"))
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap();
    let _: () = redis::cmd("HSET")
        .arg("acct:_meta")
        .arg("partitions")
        .arg(2)
        .query_async(&mut rc)
        .await
        .unwrap();
    let kernel = Kernel::start(config(&dir), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();

    let admin = register_login(&app, "promo_admin").await;
    common::grant_role(admin.id, "admin").await;
    let user = register_login(&app, "promo_user").await;
    let ta = upload(&app, &user, "Promo.A", &[("a.bin", 30_000)]).await;
    let mut db = PgConnection::connect(&env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap();
    let wait_mirror = |tid: i64| {
        let su = env(
            "PT_TEST_SUPERUSER_URL",
            "postgres://postgres@127.0.0.1:55432/pt_test",
        );
        async move {
            let mut c = PgConnection::connect(&su).await.unwrap();
            for _ in 0..100 {
                let n: i64 =
                    sqlx::query_scalar("SELECT count(*) FROM promotion.torrent_mirror WHERE torrent_id = $1")
                        .bind(tid)
                        .fetch_one(&mut c)
                        .await
                        .unwrap();
                if n == 1 {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            panic!("torrent {tid} was not mirrored");
        }
    };
    wait_mirror(ta).await;

    // 单种促销：2x 免费；普通用户无权设置
    let mut h = admin.headers();
    h.push(("idempotency-key", "p1".into()));
    let r = call(&app, "POST", "/api/p/promotion/v1/torrents/promotions",
                 Some(json!({"torrent_ids": [ta, 999_999], "kind": "double_up_free", "duration_secs": 86400, "reason": "test"})), &hv(&h)).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(
        r.body["items"][0]["promotion_id"].as_i64().is_some(),
        "{:?}",
        r.body
    );
    assert_eq!(r.body["items"][1]["error"], "promotion.torrent_not_found");
    let mut uh = user.headers();
    uh.push(("idempotency-key", "p2".into()));
    let r = call(
        &app,
        "POST",
        "/api/p/promotion/v1/torrents/promotions",
        Some(json!({"torrent_ids": [ta], "kind": "free", "reason": "x"})),
        &hv(&uh),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // 展示状态与本人有效倍率（accounting 规则经事件异步下发，负载高时稍有延迟）
    let mut r = call(
        &app,
        "GET",
        &format!("/api/p/promotion/v1/torrents/{ta}"),
        None,
        &hv(&user.headers()),
    )
    .await;
    for _ in 0..100 {
        if r.body["mine"]["up_permille"].as_i64() == Some(2000) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        r = call(
            &app,
            "GET",
            &format!("/api/p/promotion/v1/torrents/{ta}"),
            None,
            &hv(&user.headers()),
        )
        .await;
    }
    assert_eq!(r.body["primary"]["kind"], "double_up_free", "{:?}", r.body);
    assert_eq!(
        (
            r.body["combined_up_permille"].as_i64(),
            r.body["combined_down_permille"].as_i64()
        ),
        (Some(2000), Some(0))
    );
    assert_eq!(
        (
            r.body["mine"]["up_permille"].as_i64(),
            r.body["mine"]["down_permille"].as_i64()
        ),
        (Some(2000), Some(0)),
        "{:?}",
        r.body
    );

    // 倍率实际作用于记账：上传 1000 → 2000，下载 500 → 0
    let part = user.id % 2;
    let _: String = redis::cmd("XADD")
        .arg(format!("acct:{part}"))
        .arg("*")
        .arg(&[
            ("v", "1".to_owned()),
            ("eid", "pr:1:1".into()),
            ("ts", chrono::Utc::now().timestamp().to_string()),
            ("uid", user.id.to_string()),
            ("tid", ta.to_string()),
            ("ih", "ab".repeat(20)),
            ("pid", "2d71423436333030".to_owned() + &"33".repeat(12)),
            ("ip", "203.0.113.2".into()),
            ("port", "1".into()),
            ("ev", "started".into()),
            ("up", "1000".into()),
            ("down", "500".into()),
            ("left", "10".into()),
            ("dup", "1000".into()),
            ("ddown", "500".into()),
            ("dseed", "0".into()),
            ("dleech", "10".into()),
            ("flags", "1".into()),
            ("ua", "x".into()),
        ])
        .query_async(&mut rc)
        .await
        .unwrap();
    let mut totals = (0i64, 0i64);
    for _ in 0..100 {
        if let Some(t) = sqlx::query_as::<_, (i64, i64)>(
            "SELECT uploaded_traffic, downloaded_traffic FROM accounting.user_totals WHERE user_id = $1",
        )
        .bind(i64::try_from(user.id).unwrap())
        .fetch_optional(&mut db)
        .await
        .unwrap()
        {
            totals = t;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(totals, (2000, 0));

    // 自动规则：新种 50% 下载（fixed 24h）
    let r = call(&app, "POST", "/api/p/promotion/v1/admin/rules", Some(json!({
        "schema": 1, "priority": 50, "kind": "half_down",
        "enter": [ { "field": "size", "op": "gte", "value": 1 }, { "field": "status", "op": "eq", "value": "approved" } ],
        "exit": null, "duration": { "mode": "fixed", "hours": 24 }, "min_hold_minutes": 0, "type_ids": null })), &hv(&admin.headers())).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let tb = upload(&app, &user, "Promo.B", &[("b.bin", 20_000)]).await;
    wait_mirror(tb).await;
    let r = call(
        &app,
        "GET",
        &format!("/api/p/promotion/v1/torrents?ids={ta},{tb}"),
        None,
        &hv(&user.headers()),
    )
    .await;
    let items = r.body["items"].as_array().unwrap();
    let b = items.iter().find(|i| i["torrent_id"] == tb).unwrap();
    assert_eq!(
        (b["primary"]["kind"].as_str(), b["primary"]["source"].as_str()),
        (Some("half_down"), Some("auto")),
        "{:?}",
        r.body
    );

    // 全站活动：免费 → 种子 B 的合并倍率下载为 0
    let mut h = admin.headers();
    h.push(("idempotency-key", "e1".into()));
    let ends = chrono::Utc::now().timestamp_millis() + 86_400_000;
    let r = call(&app, "POST", "/api/p/promotion/v1/admin/events",
                 Some(json!({"kind": "free", "ends_at_ms": ends, "type_ids": null, "title_zh": "测试全站免费", "title_en": "Test FL"})), &hv(&h)).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let r = call(
        &app,
        "GET",
        &format!("/api/p/promotion/v1/torrents/{tb}"),
        None,
        &hv(&user.headers()),
    )
    .await;
    assert_eq!(
        (
            r.body["combined_down_permille"].as_i64(),
            r.body["global"].as_array().map(Vec::len)
        ),
        (Some(0), Some(1)),
        "{:?}",
        r.body
    );

    // 个人卡：发放 1 张；全站免费时使用免费卡提示冗余，确认后使用
    let mut h = admin.headers();
    h.push(("idempotency-key", "c1".into()));
    let r = call(
        &app,
        "POST",
        &format!("/api/p/promotion/v1/admin/users/{}/cards", user.id),
        Some(json!({"card_kind": "free", "delta": 1, "reason": "test"})),
        &hv(&h),
    )
    .await;
    assert_eq!(r.body["balance"], 1, "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        "/api/p/promotion/v1/me/cards/apply",
        Some(json!({"torrent_id": tb, "card_kind": "free"})),
        &hv(&user.headers()),
    )
    .await;
    assert_eq!(r.body["code"], "promotion.card_redundant", "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        "/api/p/promotion/v1/me/cards/apply",
        Some(json!({"torrent_id": tb, "card_kind": "free", "confirm": true})),
        &hv(&user.headers()),
    )
    .await;
    assert_eq!(r.body["balance"], 0, "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        "/api/p/promotion/v1/me/cards/apply",
        Some(json!({"torrent_id": ta, "card_kind": "free", "confirm": true})),
        &hv(&user.headers()),
    )
    .await;
    assert_eq!(r.body["code"], "promotion.no_card");
    let r = call(
        &app,
        "GET",
        "/api/p/promotion/v1/me/cards",
        None,
        &hv(&user.headers()),
    )
    .await;
    assert_eq!(r.body["active"][0]["torrent_id"], tb, "{:?}", r.body);

    // 结束单种促销：accounting 规则移除；到期任务结束过期促销
    let r = call(
        &app,
        "DELETE",
        &format!("/api/p/promotion/v1/torrents/{ta}/promotion"),
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    let st: String = sqlx::query_scalar(
        "SELECT sync_state FROM promotion.promotions WHERE torrent_id = $1 AND source = 'manual'",
    )
    .bind(ta)
    .fetch_one(&mut db)
    .await
    .unwrap();
    assert_eq!(st, "removed");
    let removed: bool = sqlx::query_scalar(
        "SELECT removed_at IS NOT NULL FROM accounting.rate_rules WHERE external_ref LIKE 'm:%'",
    )
    .fetch_one(&mut db)
    .await
    .unwrap();
    assert!(removed);
    // 单种促销结束后立即重新评估：种子 A 进入自动规则（50% 下载）
    let r = call(
        &app,
        "GET",
        &format!("/api/p/promotion/v1/torrents/{ta}"),
        None,
        &hv(&user.headers()),
    )
    .await;
    assert_eq!(
        (
            r.body["primary"]["kind"].as_str(),
            r.body["primary"]["source"].as_str()
        ),
        (Some("half_down"), Some("auto")),
        "{:?}",
        r.body
    );
    sqlx::query(
        "UPDATE promotion.promotions SET ends_at = now() - interval '1 minute' WHERE source = 'auto'",
    )
    .execute(&mut db)
    .await
    .unwrap();
    kernel
        .run_job_now("promotion", "expire", json!({}))
        .await
        .unwrap();
    let causes: Vec<String> =
        sqlx::query_scalar("SELECT end_cause FROM promotion.promotions WHERE source = 'auto'")
            .fetch_all(&mut db)
            .await
            .unwrap();
    assert_eq!(causes, vec!["expired", "expired"]);
    let ended: Vec<Value> = sqlx::query_scalar(
        "SELECT payload FROM kernel.event_log WHERE event_type = 'promotion.ended' ORDER BY seq",
    )
    .fetch_all(&mut db)
    .await
    .unwrap();
    let causes: Vec<&str> = ended.iter().filter_map(|p| p["cause"].as_str()).collect();
    assert_eq!(causes, vec!["cancelled", "expired", "expired"]);

    // 后台列表：全站活动（含已结束）、提前结束；自动规则停用 / 启用
    let admin_h = admin.headers();
    let ah = hv(&admin_h);
    let ev = call(&app, "GET", "/api/p/promotion/v1/admin/events", None, &ah).await.body;
    assert_eq!(ev["items"][0]["title"]["zh-CN"], "测试全站免费", "{ev:?}");
    let r = call(&app, "POST", "/api/p/promotion/v1/admin/events",
                 Some(json!({"kind": "double_up", "ends_at_ms": chrono::Utc::now().timestamp_millis() + 3_600_000})), &ah).await;
    let eid = r.body["id"].as_i64().unwrap();
    assert_eq!(call(&app, "DELETE", &format!("/api/p/promotion/v1/admin/events/{eid}"), None, &ah).await.status, StatusCode::NO_CONTENT);
    assert_eq!(call(&app, "DELETE", &format!("/api/p/promotion/v1/admin/events/{eid}"), None, &ah).await.body["code"], "promotion.not_found");
    let ev = call(&app, "GET", "/api/p/promotion/v1/admin/events", None, &ah).await.body;
    assert_eq!((ev["items"][0]["id"].as_i64(), ev["items"][0]["state"].as_str()), (Some(eid), Some("ended")), "{ev:?}");
    let rules = call(&app, "GET", "/api/p/promotion/v1/admin/rules", None, &ah).await.body;
    let rid = rules["items"][0]["id"].as_i64().unwrap();
    assert_eq!((rules["items"][0]["active"].as_bool(), rules["items"][0]["definition"]["kind"].as_str()), (Some(true), Some("half_down")));
    assert_eq!(call(&app, "PATCH", &format!("/api/p/promotion/v1/admin/rules/{rid}"), Some(json!({"active": false})), &ah).await.status, StatusCode::NO_CONTENT);
    assert_eq!(call(&app, "GET", "/api/p/promotion/v1/admin/rules", None, &ah).await.body["items"][0]["active"], false);
    assert_eq!(call(&app, "GET", "/api/p/promotion/v1/admin/rules", None, &hv(&user.headers())).await.status, StatusCode::FORBIDDEN);

    kernel.shutdown().await;
}
