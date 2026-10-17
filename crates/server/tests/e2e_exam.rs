//! 端到端：新手考核（newbie-exam，WASM）—— 注册自动分配、阶段推进、通过奖励魔力值、超期禁止下载、补救期内完成后自动恢复、
//! 后台分配（重复报错）/ 延期 / 豁免。
//!
//! 运行：`cargo test -p pt-server --test e2e_exam -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{User, call, env, hv, register_login, reset};

const GIB: i64 = 1 << 30;

async fn db() -> PgConnection {
    PgConnection::connect(&env("PT_TEST_SUPERUSER_URL", "postgres://postgres@127.0.0.1:55432/pt_test"))
        .await
        .unwrap()
}

async fn traffic(uid: u64, up: i64, down: i64, seed_secs: i64) {
    sqlx::query(
        "INSERT INTO accounting.user_totals (user_id, uploaded_adjust, downloaded_adjust, seed_secs) VALUES ($1, $2, $3, $4)
         ON CONFLICT (user_id) DO UPDATE SET uploaded_adjust = $2, downloaded_adjust = $3, seed_secs = $4",
    )
    .bind(i64::try_from(uid).unwrap())
    .bind(up)
    .bind(down)
    .bind(seed_secs)
    .execute(&mut db().await)
    .await
    .unwrap();
}

async fn me(app: &axum::Router, u: &User) -> Value {
    call(app, "GET", "/api/p/newbie-exam/v1/me", None, &hv(&u.headers())).await.body
}

async fn status(uid: u64) -> String {
    sqlx::query_scalar("SELECT status FROM identity.users WHERE id = $1")
        .bind(i64::try_from(uid).unwrap())
        .fetch_one(&mut db().await)
        .await
        .unwrap()
}

#[tokio::test]
async fn newbie_exam_end_to_end() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!("\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n", dir.display()));
    let kernel = Kernel::start(SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let staff = register_login(&app, "exam_staff").await;
    common::grant_role(staff.id, "moderator").await;
    sqlx::query("INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('newbie-exam', $1)")
        .bind(json!({
            "stages": [
                { "days": 7, "goals": [{ "metric": "uploaded_gib", "gte": 1 }] },
                { "days": 30, "goals": [{ "metric": "seed_hours", "gte": 10 }, { "metric": "ratio", "gte": 0.5 }] }
            ],
            "reward_bonus": 100, "remedy_days": 7, "auto_assign_modes": ["open"], "notify": false
        }))
        .execute(&mut db().await)
        .await
        .unwrap();
    let pass = register_login(&app, "exam_pass").await;
    let fail = register_login(&app, "exam_fail").await;
    // 注册事件异步投递：等到自动分配
    let mut m = Value::Null;
    for _ in 0..50 {
        m = me(&app, &pass).await;
        if m["state"] == "active" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert_eq!((m["state"].clone(), m["current_stage"].clone()), (json!("active"), json!(0)), "{m:?}");
    assert_eq!(m["stages"][0]["goals"][0], json!({ "metric": "uploaded_gib", "target": 1.0, "current": 0.0, "met": false }));

    // 第一阶段：考核期间上传 2 GiB
    traffic(pass.id, 2 * GIB, 2 * GIB, 0).await;
    kernel.run_job_now("newbie-exam", "evaluate_all", json!({})).await.unwrap();
    let m = me(&app, &pass).await;
    assert_eq!((m["current_stage"].clone(), m["stages"][0]["passed"].clone()), (json!(1), json!(true)), "{m:?}");
    // 第二阶段：做种 11 小时、分享率 1.0 → 通过，奖励 100 魔力值
    traffic(pass.id, 2 * GIB, 2 * GIB, 11 * 3600).await;
    kernel.run_job_now("newbie-exam", "evaluate_all", json!({})).await.unwrap();
    assert_eq!(me(&app, &pass).await["state"], "passed");
    let bonus = call(&app, "GET", "/api/p/ledger/v1/users/me/balances", None, &hv(&pass.headers())).await.body;
    let got = bonus.as_array().or(bonus["items"].as_array()).unwrap().iter().find(|b| b["currency"] == "bonus").unwrap()["balance"].clone();
    assert_eq!(got, json!(100), "{bonus:?}");

    // 超期未完成 → 禁止下载；补救期内完成最后阶段目标 → 自动恢复
    sqlx::query("UPDATE newbie_exam.assignments SET stage_deadline = now() - interval '1 minute' WHERE state = 'active'")
        .execute(&mut db().await)
        .await
        .unwrap();
    kernel.run_job_now("newbie-exam", "evaluate_all", json!({})).await.unwrap();
    assert_eq!(me(&app, &fail).await["state"], "failed_restricted");
    assert_eq!(status(fail.id).await, "leech_disabled");
    traffic(fail.id, GIB, GIB, 20 * 3600).await;
    kernel.run_job_now("newbie-exam", "evaluate_all", json!({})).await.unwrap();
    assert_eq!(me(&app, &fail).await["state"], "remedied");
    assert_eq!(status(fail.id).await, "active");

    // 后台：重复分配报错；给 staff 分配后延期、豁免
    let post = |path: String, body: Value| {
        let app = app.clone();
        let h = staff.headers();
        async move { call(&app, "POST", &path, Some(body), &hv(&h)).await }
    };
    let r = post("/api/p/newbie-exam/v1/admin/assign".into(), json!({ "user_ids": [staff.id, fail.id] })).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(r.body["results"][0]["assignment_id"].as_i64().is_some(), "{:?}", r.body);
    let r = post("/api/p/newbie-exam/v1/admin/assign".into(), json!({ "user_ids": [staff.id] })).await;
    assert_eq!(r.body["results"][0]["error"], "newbie-exam.already_assigned", "{:?}", r.body);
    let before = me(&app, &staff).await["stages"][0]["deadline"].as_i64().unwrap();
    let r = post(format!("/api/p/newbie-exam/v1/admin/users/{}/extend", staff.id), json!({ "days": 3 })).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    let after = me(&app, &staff).await["stages"][0]["deadline"].as_i64().unwrap();
    assert_eq!(after - before, 3 * 86_400_000);
    let r = post(format!("/api/p/newbie-exam/v1/admin/users/{}/exempt", staff.id), json!({ "reason": "staff" })).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    assert_eq!(me(&app, &staff).await["state"], "exempted");
}
