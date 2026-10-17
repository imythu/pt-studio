//! 端到端：分享率考核（ratio-policy，WASM）—— 分档要求与豁免、警告、警告期禁止下载新种子、期满禁止下载（identity 限制）、
//! 恢复后自动解除、后台手动警告 / 解除 / 豁免、`/v1/me`。
//!
//! 运行：`cargo test -p pt-server --test e2e_ratio -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{User, call, env, hv, register_login, reset, upload};

const GIB: i64 = 1 << 30;

async fn db() -> PgConnection {
    PgConnection::connect(&env("PT_TEST_SUPERUSER_URL", "postgres://postgres@127.0.0.1:55432/pt_test"))
        .await
        .unwrap()
}

async fn set_traffic(uid: u64, up: i64, down: i64) {
    sqlx::query(
        "INSERT INTO accounting.user_totals (user_id, uploaded_adjust, downloaded_adjust) VALUES ($1, $2, $3)
         ON CONFLICT (user_id) DO UPDATE SET uploaded_adjust = $2, downloaded_adjust = $3",
    )
    .bind(i64::try_from(uid).unwrap())
    .bind(up)
    .bind(down)
    .execute(&mut db().await)
    .await
    .unwrap();
}

async fn get(app: &axum::Router, u: &User, path: &str) -> Value {
    call(app, "GET", path, None, &hv(&u.headers())).await.body
}

async fn post(app: &axum::Router, u: &User, path: &str, body: Value) -> common::Resp {
    call(app, "POST", path, Some(body), &hv(&u.headers())).await
}

async fn status(uid: u64) -> String {
    sqlx::query_scalar("SELECT status FROM identity.users WHERE id = $1")
        .bind(i64::try_from(uid).unwrap())
        .fetch_one(&mut db().await)
        .await
        .unwrap()
}

#[tokio::test]
async fn ratio_policy_end_to_end() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!("\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n", dir.display()));
    let kernel = Kernel::start(SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let low = register_login(&app, "low_ratio").await;
    let good = register_login(&app, "good_ratio").await;
    let newbie = register_login(&app, "newbie").await;
    let staff = register_login(&app, "staff_mod").await;
    common::grant_role(staff.id, "moderator").await;
    // 前三人注册于 30 天前（过了宽限期）；newbie 刚注册
    sqlx::query("UPDATE identity.users SET created_at = now() - interval '30 days' WHERE username IN ('low_ratio', 'good_ratio', 'staff_mod')")
        .execute(&mut db().await)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('ratio-policy', $1)",
    )
    .bind(json!({ "warning_days": 3, "block_new_downloads_during_warning": true, "notify": false }))
    .execute(&mut db().await)
    .await
    .unwrap();
    // 下载 10 GiB 的要求是 0.2：low 上传 1 GiB（0.1），good 上传 3 GiB（0.3），newbie 在宽限期
    set_traffic(low.id, GIB, 10 * GIB).await;
    set_traffic(good.id, 3 * GIB, 10 * GIB).await;
    set_traffic(newbie.id, 0, 10 * GIB).await;
    kernel.run_job_now("ratio-policy", "daily_evaluate", json!({})).await.unwrap();

    let me = get(&app, &low, "/api/p/ratio-policy/v1/me").await;
    assert_eq!((me["state"].clone(), me["required"].clone()), (json!("warned"), json!(0.2)), "{me:?}");
    assert_eq!(me["upload_needed"], json!(GIB), "0.2 × 10 GiB − 1 GiB: {me:?}");
    assert!(me["warned_until"].as_i64().unwrap() > 0);
    assert_eq!(get(&app, &good, "/api/p/ratio-policy/v1/me").await["state"], "ok");
    let nb = get(&app, &newbie, "/api/p/ratio-policy/v1/me").await;
    assert_eq!((nb["state"].clone(), nb["exempt_reason"].clone()), (json!("ok"), json!("grace")), "{nb:?}");

    // 警告期：不能下载新种子
    let tid = upload(&app, &good, "Ratio.Test", &[("a.bin", 1 << 20)]).await;
    let r = call(&app, "GET", &format!("/api/p/torrent/v1/torrents/{tid}/download"), None, &hv(&low.headers())).await;
    assert_eq!(r.body["code"], "ratio-policy.download_restricted", "{:?}", r.body);
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = call(&app, "GET", &format!("/api/p/torrent/v1/torrents/{tid}/download"), None, &hv(&good.headers())).await;
    assert_eq!(r.status, StatusCode::OK);

    // 警告期满仍不达标 → 禁止下载（identity 限制）
    sqlx::query("UPDATE ratio_policy.states SET deadline = now() - interval '1 minute' WHERE state = 'warned'")
        .execute(&mut db().await)
        .await
        .unwrap();
    kernel.run_job_now("ratio-policy", "daily_evaluate", json!({})).await.unwrap();
    assert_eq!(get(&app, &low, "/api/p/ratio-policy/v1/me").await["state"], "restricted");
    assert_eq!(status(low.id).await, "leech_disabled");

    // 上传达标（增量评估：标脏后由 incr_evaluate 处理）→ 自动解除
    set_traffic(low.id, 3 * GIB, 10 * GIB).await;
    sqlx::query("INSERT INTO ratio_policy.dirty_users (user_id) VALUES ($1)")
        .bind(i64::try_from(low.id).unwrap())
        .execute(&mut db().await)
        .await
        .unwrap();
    kernel.run_job_now("ratio-policy", "incr_evaluate", json!({})).await.unwrap();
    assert_eq!(get(&app, &low, "/api/p/ratio-policy/v1/me").await["state"], "ok");
    assert_eq!(status(low.id).await, "active");

    // 后台：普通用户无权；版主手动警告（分享率达标也保持警告）→ 列表 → 解除；豁免
    let uri = |id: u64, op: &str| format!("/api/p/ratio-policy/v1/admin/users/{id}/{op}");
    let r = post(&app, &good, &uri(low.id, "warn"), json!({ "deadline": 4_000_000_000_000i64, "reason": "x" })).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = post(&app, &staff, &uri(low.id, "warn"), json!({ "deadline": 4_000_000_000_000i64, "reason": "hit and run" })).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    kernel.run_job_now("ratio-policy", "daily_evaluate", json!({})).await.unwrap();
    assert_eq!(get(&app, &low, "/api/p/ratio-policy/v1/me").await["state"], "warned", "manual warnings stay until lifted");
    let list = get(&app, &staff, "/api/p/ratio-policy/v1/admin/warned?state=warned").await;
    assert_eq!(list["items"][0]["username"], "low_ratio", "{list:?}");
    let r = post(&app, &staff, &uri(low.id, "lift"), json!({ "reason": "ok" })).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    let r = post(&app, &staff, &uri(low.id, "lift"), json!({})).await;
    assert_eq!(r.body["code"], "ratio-policy.not_warned");
    set_traffic(good.id, 0, 10 * GIB).await;
    let r = post(&app, &staff, &uri(good.id, "exempt"), json!({ "until": 4_000_000_000_000i64, "reason": "donor" })).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    kernel.run_job_now("ratio-policy", "daily_evaluate", json!({})).await.unwrap();
    let g = get(&app, &good, "/api/p/ratio-policy/v1/me").await;
    assert_eq!((g["state"].clone(), g["exempt_reason"].clone()), (json!("ok"), json!("manual")), "{g:?}");
    let audits: i64 = sqlx::query_scalar("SELECT count(*) FROM kernel.audit_log WHERE action LIKE 'ratio-policy.%'")
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert_eq!(audits, 3);
}
