//! 端到端：插件模板（templates/wasm-plugin，基于 pt-plugin-sdk）—— 独立构建、`pt_server::package` 打包与检查、
//! 内核加载；第三方钩子点（清单定义的 `notes.before_create`，由测试中的 Native 插件处理：修改 title、拒绝）、
//! 清单 `[error_codes]` 的 HTTP 状态、后台设置、事件、MCP 工具。

#![allow(clippy::unwrap_used)]

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use pt_plugin_api::{Ctx, HookCall, HookDecision, Hooks, NativePlugin, PResult, json};
use serde_json::Value;
use sqlx::{Connection, PgConnection};

use common::{call, env, hv, register_login, reset};

const GUARD: &str = r#"
id = "x-notes-guard"
name = "test guard"
version = "0.1.0"
kind = "native"
api = "pt:plugin@1"
license = "MIT"

[[hooks]]
hook = "notes.before_create"
"#;

/// 处理第三方钩子：标题含 spam 拒绝；否则在标题后加标记
struct Guard;

#[async_trait]
impl Hooks for Guard {
    async fn handle(&self, _ctx: &Ctx, call: HookCall) -> PResult<HookDecision> {
        let title = call.input["title"].as_str().unwrap_or("");
        if title.contains("spam") {
            return Ok(HookDecision::Deny {
                code: "x-notes-guard.spam".into(),
                message: "no spam".into(),
                details: None,
            });
        }
        Ok(HookDecision::Modify(json!({ "title": format!("{title} [ok]") })))
    }
}

fn build_template() -> std::path::PathBuf {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tpl = root.join("templates/wasm-plugin");
    let st = std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .current_dir(&tpl)
        .status()
        .unwrap();
    assert!(st.success(), "building the template failed");
    let out = std::path::PathBuf::from(env("PT_TEST_TEMPLATE_DIR", "/tmp/pt-studio-test-template"));
    let _ = std::fs::remove_dir_all(&out);
    let pkg = pt_server::package(
        &tpl,
        &tpl.join("target/wasm32-wasip2/release/pt_wasm_notes.wasm"),
        &out,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        pkg.join("checksums.txt").exists() && pkg.join("schemas/hooks/notes.before_create.json").exists()
    );
    out
}

#[tokio::test]
async fn template_plugin_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let dir = build_template();
    reset().await;
    let mut db = PgConnection::connect(&env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap();
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n",
        dir.display()
    ));
    let mut plugins = pt_server::native_plugins();
    plugins.push(NativePlugin {
        manifest: GUARD,
        hooks: Some(Arc::new(Guard)),
        ..Default::default()
    });
    let kernel = Kernel::start(SiteConfig::parse(&t).unwrap(), plugins)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    sqlx::query("INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('notes', $1)")
        .bind(json!({ "max_notes": 2 }))
        .execute(&mut db)
        .await
        .unwrap();
    let alice = register_login(&app, "tpl_alice").await;
    let h = alice.headers();

    // 创建：第三方钩子改写标题；拒绝时返回处理者的错误码
    let r = call(
        &app,
        "POST",
        "/api/p/notes/v1/notes",
        Some(json!({ "title": " Hello ", "body": "b" })),
        &hv(&h),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["title"], "Hello [ok]");
    let first = r.body["id"].as_i64().unwrap();
    let r = call(
        &app,
        "POST",
        "/api/p/notes/v1/notes",
        Some(json!({ "title": "buy spam" })),
        &hv(&h),
    )
    .await;
    assert_eq!(r.body["code"], "x-notes-guard.spam", "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        "/api/p/notes/v1/notes",
        Some(json!({ "title": "  " })),
        &hv(&h),
    )
    .await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("notes.title_invalid"))
    );
    // 设置 max_notes = 2；清单 [error_codes] 给出 422
    let r = call(
        &app,
        "POST",
        "/api/p/notes/v1/notes",
        Some(json!({ "title": "second" })),
        &hv(&h),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let r = call(
        &app,
        "POST",
        "/api/p/notes/v1/notes",
        Some(json!({ "title": "third" })),
        &hv(&h),
    )
    .await;
    assert_eq!(
        (
            r.status,
            r.body["code"].as_str(),
            r.body["details"]["max"].as_i64()
        ),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("notes.too_many"), Some(2)),
        "{:?}",
        r.body
    );
    // 列表、事件
    let l = call(&app, "GET", "/api/p/notes/v1/notes", None, &hv(&h)).await;
    assert_eq!(l.body["items"].as_array().map(Vec::len), Some(2), "{:?}", l.body);
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM kernel.event_log WHERE event_type = 'notes.created'")
            .fetch_one(&mut db)
            .await
            .unwrap();
    assert_eq!(n, 2);

    // MCP 工具（只读，不含正文）
    let t = call(
        &app,
        "POST",
        "/api/p/identity/v1/users/me/tokens",
        Some(json!({ "kind": "mcp", "name": "c", "scopes": ["read"] })),
        &hv(&h),
    )
    .await;
    let auth = format!("Bearer {}", t.body["token"].as_str().unwrap());
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "list_my_notes", "arguments": { "limit": 5 } } });
    let r = common::send(&app, "POST", "/mcp", &[], |b| {
        b.header("authorization", &auth)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2026-07-28")
            .header("mcp-method", "tools/call")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap()
    })
    .await;
    let items: Vec<Value> = r.body["result"]["structuredContent"]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(items.len(), 2, "{:?}", r.body);
    assert!(
        items
            .iter()
            .all(|i| i.get("body").is_none() && i["title"].is_string())
    );

    // 删除：本人的可删；不存在 → 404（后缀规则）
    let r = call(
        &app,
        "DELETE",
        &format!("/api/p/notes/v1/notes/{first}"),
        None,
        &hv(&h),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = call(
        &app,
        "DELETE",
        &format!("/api/p/notes/v1/notes/{first}"),
        None,
        &hv(&h),
    )
    .await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (StatusCode::NOT_FOUND, Some("notes.not_found"))
    );
    kernel.shutdown().await;
}
