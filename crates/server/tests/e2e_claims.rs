//! 端到端：种子认领（claims，WASM）—— 认领条件（下载完成、重复）、种子页信息、月结（折算要求、达标奖励、不达标取消、幂等）、取消认领。
//!
//! 运行：`cargo test -p pt-server --test e2e_claims -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{call, env, hv, register_login, reset, upload};

async fn db() -> PgConnection {
    PgConnection::connect(&env("PT_TEST_SUPERUSER_URL", "postgres://postgres@127.0.0.1:55432/pt_test"))
        .await
        .unwrap()
}

async fn snatch(uid: u64, tid: i64, seed_secs: i64) {
    sqlx::query(
        "INSERT INTO accounting.snatches (user_id, torrent_id, first_action_at, last_action_at, completed_at, seed_secs)
         VALUES ($1, $2, now() - interval '50 days', now(), now() - interval '50 days', $3)
         ON CONFLICT (user_id, torrent_id) DO UPDATE SET seed_secs = EXCLUDED.seed_secs",
    )
    .bind(i64::try_from(uid).unwrap())
    .bind(tid)
    .bind(seed_secs)
    .execute(&mut db().await)
    .await
    .unwrap();
}

async fn bonus(app: &axum::Router, u: &common::User) -> Value {
    let b = call(app, "GET", "/api/p/ledger/v1/users/me/balances", None, &hv(&u.headers())).await.body;
    b.as_array().or(b["items"].as_array()).unwrap().iter().find(|x| x["currency"] == "bonus").map_or(json!(0), |x| x["balance"].clone())
}

#[tokio::test]
async fn claims_end_to_end() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!("\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n", dir.display()));
    let kernel = Kernel::start(SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let uploader = register_login(&app, "claim_up").await;
    let keeper = register_login(&app, "claim_keeper").await;
    sqlx::query("INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('claims', $1)")
        .bind(json!({ "monthly_seed_hours": 10, "reward_bonus": 50, "remove_after_failures": 1 }))
        .execute(&mut db().await)
        .await
        .unwrap();
    let t1 = upload(&app, &uploader, "Claim.One", &[("a.bin", 1 << 20)]).await;
    let t2 = upload(&app, &uploader, "Claim.Two", &[("b.bin", 1 << 20)]).await;
    let claim = |tid: i64| {
        let app = app.clone();
        let h = keeper.headers();
        async move { call(&app, "POST", &format!("/api/p/claims/v1/torrents/{tid}/claims"), Some(json!({})), &hv(&h)).await }
    };

    // 只能认领下载完成过的种子；不能重复认领
    let r = claim(t1).await;
    assert_eq!(r.body["code"], "claims.not_completed", "{:?}", r.body);
    snatch(keeper.id, t1, 3600).await;
    snatch(keeper.id, t2, 3600).await;
    assert_eq!(claim(t1).await.status, StatusCode::CREATED);
    assert_eq!(claim(t1).await.body["code"], "claims.already_claimed");
    assert_eq!(claim(t2).await.status, StatusCode::CREATED);
    let info = call(&app, "GET", &format!("/api/p/claims/v1/torrents/{t1}"), None, &hv(&keeper.headers())).await.body;
    assert_eq!((info["claimers"].clone(), info["mine"].clone()), (json!(1), json!(true)), "{info:?}");
    let list = call(&app, "GET", &format!("/api/p/claims/v1/torrents/{t1}/claimers"), None, &hv(&uploader.headers())).await.body;
    assert_eq!(list["items"][0]["username"], "claim_keeper");
    assert!(list["items"][0]["user_id"].is_null(), "user ids are staff-only");

    // 当月新认领不参加结算
    kernel.run_job_now("claims", "settle", json!({})).await.unwrap();
    assert_eq!(bonus(&app, &keeper).await, json!(0));

    // 认领周期挪到上个月（整月）：t1 做种 +11 h 达标，t2 +1 h 不达标 → 取消
    sqlx::query("UPDATE claims.claims SET period_start = date_trunc('month', now()) - interval '1 month'")
        .execute(&mut db().await)
        .await
        .unwrap();
    snatch(keeper.id, t1, 3600 + 11 * 3600).await;
    snatch(keeper.id, t2, 3600 + 3600).await;
    kernel.run_job_now("claims", "settle", json!({})).await.unwrap();
    assert_eq!(bonus(&app, &keeper).await, json!(50));
    let me = call(&app, "GET", "/api/p/claims/v1/me", None, &hv(&keeper.headers())).await.body;
    assert_eq!(me["items"].as_array().unwrap().len(), 1, "{me:?}");
    assert_eq!(me["items"][0]["torrent_id"], t1);
    assert_eq!(me["items"][0]["last_result"], "passed");
    assert_eq!(me["last_settlement"]["passed"], 1);
    assert_eq!(me["last_settlement"]["removed"], 1);
    // 重跑不重复发奖
    kernel.run_job_now("claims", "settle", json!({})).await.unwrap();
    assert_eq!(bonus(&app, &keeper).await, json!(50));

    // 取消认领
    let r = call(&app, "DELETE", &format!("/api/p/claims/v1/torrents/{t1}/claims"), None, &hv(&keeper.headers())).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let me = call(&app, "GET", "/api/p/claims/v1/me", None, &hv(&keeper.headers())).await.body;
    assert!(me["items"].as_array().unwrap().is_empty());
}
