//! 端到端：WASM 插件（bonus）——加载、init 中调用 Native 服务、事件消费、HTTP、Native 调用 WASM 服务。
//!
//! 需要 `rustup target add wasm32-wasip2`；测试会构建 plugins/wasm 工作区并打包到临时插件目录。

#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;

use axum::{Json, Router, body::Body, http::StatusCode, response::IntoResponse, routing::post};
use pt_kernel::{Kernel, config::SiteConfig};
use pt_plugin_api::{HttpRoutes, NativePlugin, PResult, RequestCtx, json};
use serde_json::Value;
use sqlx::{Connection, PgConnection};

use common::{call, env, reset, send};

const AWARDER: &str = r#"
id = "x-awarder"
name = "awarder"
version = "0.1.0"
kind = "native"
api = "pt:plugin@1"
license = "MIT OR Apache-2.0"
requires = ["ctx.bonus@^0.1"]

[capabilities.scopes]
"ctx.bonus" = ["award"]

[[http.routes]]
id = "award"
method = "POST"
path = "/v1/award"
auth = "public"
"#;

async fn award(ctx: RequestCtx, Json(b): Json<Value>) -> PResult<axum::response::Response> {
    let out = ctx.call_json("ctx.bonus", "award", b).await?;
    Ok(Json(out).into_response())
}

struct AwarderRoutes;
impl HttpRoutes for AwarderRoutes {
    fn router(&self) -> Router {
        Router::new().route("/v1/award", post(award))
    }
}

fn config(plugins_dir: &std::path::Path) -> SiteConfig {
    let mut text = common::config_text();
    text.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n",
        plugins_dir.display()
    ));
    SiteConfig::parse(&text).unwrap()
}

#[tokio::test]
async fn wasm_bonus_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter("info,sqlx=warn,cranelift=warn,wasmtime=warn")
        .try_init();
    let dir = common::package_wasm_plugins();
    reset().await;
    let mut plugins = pt_server::native_plugins();
    plugins.push(NativePlugin {
        manifest: AWARDER,
        http: Some(Arc::new(AwarderRoutes)),
        ..Default::default()
    });
    let kernel = Kernel::start(config(&dir), plugins)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();

    // init（WASM）→ ctx.ledger.register_currency（Native）
    let r = call(&app, "GET", "/api/kernel/v1/plugins", None, &[]).await;
    assert!(
        r.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "bonus" && p["kind"] == "wasm"),
        "{:?}",
        r.body
    );
    call(
        &app,
        "POST",
        "/api/p/identity/v1/users",
        Some(json!({"username": "uploader9", "email": "u9@example.com", "password": "correct horse"})),
        &[],
    )
    .await;
    let login = call(
        &app,
        "POST",
        "/api/p/identity/v1/sessions",
        Some(json!({"username": "uploader9", "password": "correct horse"})),
        &[],
    )
    .await;
    let uid = login.body["user"]["id"].as_u64().unwrap();
    let cookie = login.cookies.join("; ");
    let csrf = login.body["csrf_token"].as_str().unwrap().to_owned();
    let r = call(
        &app,
        "GET",
        "/api/p/ledger/v1/currencies",
        None,
        &[("cookie", &cookie)],
    )
    .await;
    let codes: Vec<&str> = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["code"].as_str())
        .collect();
    assert_eq!(codes, vec!["bonus", "seed_points"], "{:?}", r.body);

    // 上传（初始 approved）→ torrent.created → WASM on-event → ledger 发放 10.0 魔力值
    let file = pt_plugin_torrent::fixtures::sample_v1("Wasm.Test.2026", &[("a.bin", 40_000)], true);
    let boundary = "b0undary";
    let mut body = Vec::new();
    body.extend(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"meta\"\r\n\r\n{}\r\n",
            json!({"type_id": "generic", "title": "Wasm Test", "fields": {"kind": "software", "title": "Wasm Test", "platform": "windows"}})
        )
        .as_bytes(),
    );
    body.extend(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"torrent\"; filename=\"t.torrent\"\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend(&file);
    body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    let h = [
        ("cookie", cookie.as_str()),
        ("x-csrf-token", csrf.as_str()),
        ("sec-fetch-site", "same-origin"),
    ];
    let up = send(&app, "POST", "/api/p/torrent/v1/torrents", &h, |b| {
        b.header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body.clone()))
        .unwrap()
    })
    .await;
    assert_eq!(up.status, StatusCode::CREATED, "{:?}", up.body);

    // WASM HTTP：本人魔力值摘要
    let mut summary = Value::Null;
    for _ in 0..100 {
        summary = call(
            &app,
            "GET",
            "/api/p/bonus/v1/users/me/summary",
            None,
            &[("cookie", &cookie)],
        )
        .await
        .body;
        if summary["balances"][0]["balance"] == 100 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(summary["balances"][0]["currency"], "bonus", "{summary}");
    assert_eq!(summary["balances"][0]["balance"], 100, "{summary}");
    assert_eq!(summary["balances"][1]["currency"], "seed_points");

    // Native → WASM 服务：ctx.bonus.award（幂等、校验）
    let r = call(&app, "POST", "/api/p/x-awarder/v1/award",
        Some(json!({"user": uid, "currency": "bonus", "amount": 55, "reason": "x-awarder.contest", "idempotency_key": "k1"})), &[]).await;
    assert_eq!(r.body["duplicate"], false, "{:?}", r.body);
    let r2 = call(&app, "POST", "/api/p/x-awarder/v1/award",
        Some(json!({"user": uid, "currency": "bonus", "amount": 55, "reason": "x-awarder.contest", "idempotency_key": "k1"})), &[]).await;
    assert_eq!(r2.body["duplicate"], true, "{:?}", r2.body);
    let r3 = call(&app, "POST", "/api/p/x-awarder/v1/award",
        Some(json!({"user": uid, "currency": "seed_points", "amount": 5, "reason": "x-awarder.contest", "idempotency_key": "k2"})), &[]).await;
    assert_eq!(r3.body["code"], "bonus.currency_not_awardable", "{:?}", r3.body);
    let r4 = call(&app, "POST", "/api/p/x-awarder/v1/award",
        Some(json!({"user": uid, "currency": "bonus", "amount": 5, "reason": "other.contest", "idempotency_key": "k3"})), &[]).await;
    assert_eq!(r4.body["code"], "kernel.validation_failed", "{:?}", r4.body);
    let s = call(
        &app,
        "GET",
        "/api/p/bonus/v1/users/me/summary",
        None,
        &[("cookie", &cookie)],
    )
    .await
    .body;
    assert_eq!(s["balances"][0]["balance"], 155, "{s}");

    // 数据与事件
    let su = env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    );
    let mut db = PgConnection::connect(&su).await.unwrap();
    let states: Vec<String> = sqlx::query_scalar("SELECT state FROM bonus.action_awards")
        .fetch_all(&mut db)
        .await
        .unwrap();
    assert_eq!(states, vec!["awarded"]);
    let awarded: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM kernel.event_log WHERE event_type = 'bonus.awarded' AND source = 'bonus'",
    )
    .fetch_one(&mut db)
    .await
    .unwrap();
    assert_eq!(awarded, 2);
    let caller: String =
        sqlx::query_scalar("SELECT caller_plugin FROM ledger.entries WHERE reason = 'bonus.award'")
            .fetch_one(&mut db)
            .await
            .unwrap();
    assert_eq!(caller, "bonus");

    // 小时结算：tracker 流 → accounting 快照 → bonus 用户小时 → post_accruals → ledger
    let tid = up.body["id"].as_i64().unwrap();
    let mut rc = redis::Client::open(env("PT_TEST_REDIS_URL", "redis://127.0.0.1:56379/15"))
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap();
    let part = uid % 2;
    for (i, (ev, dseed)) in [("started", "0"), ("none", "1800")].into_iter().enumerate() {
        let _: String = redis::cmd("XADD")
            .arg(format!("acct:{part}"))
            .arg("*")
            .arg(&[
                ("v", "1".to_owned()),
                ("eid", format!("w1:1:{i}")),
                ("ts", chrono::Utc::now().timestamp().to_string()),
                ("uid", uid.to_string()),
                ("tid", tid.to_string()),
                ("ih", "ab".repeat(20)),
                ("pid", "2d71423436333030".to_owned() + &"22".repeat(12)),
                ("ip", "203.0.113.1".into()),
                ("port", "1".into()),
                ("ev", ev.into()),
                ("up", "0".into()),
                ("down", "0".into()),
                ("left", "0".into()),
                ("dup", "0".into()),
                ("ddown", "0".into()),
                ("dseed", dseed.into()),
                ("dleech", "0".into()),
                ("flags", "0".into()),
                ("ua", "x".into()),
            ])
            .query_async(&mut rc)
            .await
            .unwrap();
    }
    let su = env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    );
    let mut db = PgConnection::connect(&su).await.unwrap();
    let mut slotted = false;
    for _ in 0..100 {
        let n: i64 = sqlx::query_scalar("SELECT coalesce(sum(slot0_secs + slot1_secs), 0)::bigint FROM accounting.peers WHERE user_id = $1")
            .bind(i64::try_from(uid).unwrap()).fetch_one(&mut db).await.unwrap();
        if n == 1800 {
            slotted = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(slotted, "accounting did not record the seeding slot");
    // 新种子在 NexusPHP 公式中权重为 0：把种子事实的创建时间提前 4 周
    sqlx::query(
        "UPDATE bonus.torrent_facts SET source_created_at = now() - interval '28 days' WHERE torrent_id = $1",
    )
    .bind(tid)
    .execute(&mut db)
    .await
    .unwrap();
    // VIP（等级绑定或直接授予 `levels:vip` 角色）：做种魔力的基础部分 ×DONOR，做种积分不变
    sqlx::query(
        "INSERT INTO identity.roles (name, kind, owner_plugin, name_term) VALUES ('levels:vip', 'plugin', 'levels', 'levels.role.vip')
         ON CONFLICT (name) DO NOTHING",
    )
    .execute(&mut db)
    .await
    .unwrap();
    sqlx::query("INSERT INTO identity.user_roles (user_id, role, granted_by) VALUES ($1, 'levels:vip', 'test')")
        .bind(i64::try_from(uid).unwrap())
        .execute(&mut db)
        .await
        .unwrap();
    let hour = chrono::Utc::now().timestamp().div_euclid(3600);
    kernel
        .run_job_now(
            "accounting",
            "seeding_snapshot",
            json!({"hour": hour, "force": true}),
        )
        .await
        .unwrap();
    let mut amounts = Value::Null;
    for _ in 0..100 {
        if let Some(a) =
            sqlx::query_scalar::<_, Value>("SELECT amounts || jsonb_build_object('flags', flags) FROM bonus.user_hours WHERE user_id = $1")
                .bind(i64::try_from(uid).unwrap())
                .fetch_optional(&mut db)
                .await
                .unwrap()
        {
            amounts = a;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(
        amounts,
        json!({"bonus": 10, "seed_points": 5, "flags": {}}),
        "user hour amounts"
    );
    let next_hour = chrono::DateTime::from_timestamp((hour + 1) * 3600, 0)
        .unwrap()
        .to_rfc3339();
    kernel
        .run_job_now("bonus", "post_accruals", json!({"cutoff": next_hour}))
        .await
        .unwrap();
    kernel
        .run_job_now("bonus", "post_accruals", json!({"cutoff": next_hour}))
        .await
        .unwrap(); // 重复运行不重复入账
    let s = call(
        &app,
        "GET",
        "/api/p/bonus/v1/users/me/summary",
        None,
        &[("cookie", &cookie)],
    )
    .await
    .body;
    assert_eq!(
        (
            s["balances"][0]["balance"].as_i64(),
            s["balances"][1]["balance"].as_i64()
        ),
        (Some(165), Some(5)),
        "{s}"
    );
    let states: Vec<String> = sqlx::query_scalar("SELECT state FROM bonus.postings ORDER BY currency")
        .fetch_all(&mut db)
        .await
        .unwrap();
    assert_eq!(states, vec!["done", "done"]);
    let settled: i64 =
        sqlx::query_scalar("SELECT count(*) FROM kernel.event_log WHERE event_type = 'bonus.hourly_settled'")
            .fetch_one(&mut db)
            .await
            .unwrap();
    assert_eq!(settled, 1);

    // 沙箱：截止时间、陷入、内存上限、跨 schema 读取、Redis 白名单；越界后插件仍可正常服务
    for (op, code) in [
        ("loop", "kernel.timeout"),
        ("panic", "kernel.plugin_trapped"),
        ("alloc", "kernel.resource_exhausted"),
        ("peek", "kernel.internal"),
        ("eval", "kernel.redis_command_denied"),
        ("reset_role", "kernel.sql_rejected"),
        ("set_config", "kernel.sql_rejected"),
        ("mkfunc", "kernel.sql_rejected"),
    ] {
        let started = std::time::Instant::now();
        let r = call(&app, "GET", &format!("/api/p/x-chaos/v1/{op}"), None, &[]).await;
        assert_eq!(r.body["code"], code, "{op}: {:?}", r.body);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "{op} took too long"
        );
    }
    let r = call(&app, "GET", "/api/p/x-chaos/v1/fine", None, &[]).await;
    assert_eq!((r.status, r.bytes.as_slice()), (StatusCode::OK, &b"fine"[..]));

    kernel.shutdown().await;
}
