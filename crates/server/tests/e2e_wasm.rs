//! 端到端：WASM 插件（bonus）——加载、init 中调用 Native 服务、事件消费、HTTP、Native 调用 WASM 服务。
//!
//! 需要 `rustup target add wasm32-wasip2`；测试会构建 plugins/wasm 工作区并打包到临时插件目录。

#![allow(clippy::unwrap_used)]

mod common;

use std::{path::PathBuf, process::Command, sync::Arc};

use axum::{Json, Router, body::Body, http::StatusCode, response::IntoResponse, routing::post};
use pt_kernel::{Kernel, config::SiteConfig};
use pt_plugin_api::{HttpRoutes, NativePlugin, PResult, RequestCtx, json};
use serde_json::Value;
use sqlx::{Connection, PgConnection};

use common::{call, env, reset, send};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 构建 WASM 插件并组装插件目录：<dir>/bonus/{plugin.toml, plugin.wasm, migrations/}
fn package_plugins() -> PathBuf {
    let ws = repo().join("plugins/wasm");
    let st = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .current_dir(&ws)
        .status()
        .unwrap();
    assert!(st.success(), "building plugins/wasm failed");
    let dir = PathBuf::from(common::env("PT_TEST_PLUGINS_DIR", "/tmp/pt-studio-test-plugins"));
    let _ = std::fs::remove_dir_all(&dir);
    for (id, artifact) in [("bonus", "pt_wasm_bonus"), ("x-chaos", "pt_wasm_x_chaos")] {
        let dst = dir.join(id);
        std::fs::create_dir_all(dst.join("migrations")).unwrap();
        std::fs::copy(ws.join(id).join("plugin.toml"), dst.join("plugin.toml")).unwrap();
        std::fs::copy(
            ws.join(format!("target/wasm32-wasip2/release/{artifact}.wasm")),
            dst.join("plugin.wasm"),
        )
        .unwrap();
        if let Ok(rd) = std::fs::read_dir(ws.join(id).join("migrations")) {
            for f in rd {
                let f = f.unwrap().path();
                std::fs::copy(&f, dst.join("migrations").join(f.file_name().unwrap())).unwrap();
            }
        }
    }
    dir
}

const AWARDER: &str = r#"
id = "x-awarder"
name = "awarder"
version = "0.1.0"
kind = "native"
api = "pt:plugin@0.1"
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
    let dir = package_plugins();
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
            json!({"type_id": "generic", "title": "Wasm Test"})
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

    // 沙箱：截止时间、陷入、内存上限、跨 schema 读取、Redis 白名单；越界后插件仍可正常服务
    for (op, code) in [
        ("loop", "kernel.timeout"),
        ("panic", "kernel.plugin_trapped"),
        ("alloc", "kernel.resource_exhausted"),
        ("peek", "kernel.internal"),
        ("eval", "kernel.redis_command_denied"),
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
