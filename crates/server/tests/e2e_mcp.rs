//! 端到端：MCP —— identity 个人 MCP token（签发、Bearer 鉴权、作用域、撤销、封禁后失效）、内核 MCP 工具注册（Native 与 WASM
//! 提供方、权限与作用域、输入校验）、根路由、mcp 网关（两个协议版本、initialize 与会话 ID、tools/list、tools/call、错误码）、
//! 写操作站内确认（待确认、确认后执行并保存结果、重放、拒绝）与事件。

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};

use common::{User, call, hv, register_login, reset, send};

fn config(dir: &std::path::Path) -> SiteConfig {
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n\n[plugin.mcp]\nsession_key = \"test-session-key-0123456789\"\nissuer = \"https://pt.example\"\n",
        dir.display()
    ));
    SiteConfig::parse(&t).unwrap()
}

async fn token(app: &axum::Router, u: &User, scopes: &[&str]) -> String {
    let r = call(
        app,
        "POST",
        "/api/p/identity/v1/users/me/tokens",
        Some(json!({ "kind": "mcp", "name": "Claude", "scopes": scopes, "ttl_days": 30 })),
        &hv(&u.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    r.body["token"].as_str().unwrap().to_owned()
}

/// JSON-RPC 调用：返回 (HTTP 状态, 响应头, 响应体)
async fn rpc(
    app: &axum::Router,
    tok: &str,
    version: &str,
    method: &str,
    params: Value,
    extra: &[(&str, &str)],
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    let auth = format!("Bearer {tok}");
    let r = send(app, "POST", "/mcp", &[], |b| {
        let mut b = b
            .header("authorization", &auth)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", version);
        if version == "2026-07-28" {
            b = b.header("mcp-method", method);
        }
        for (k, v) in extra {
            b = b.header(*k, *v);
        }
        b.body(axum::body::Body::from(body.to_string())).unwrap()
    })
    .await;
    (r.status, r.headers, r.body)
}

async fn tool(app: &axum::Router, tok: &str, name: &str, args: Value, meta: Option<Value>) -> Value {
    let mut params = json!({ "name": name, "arguments": args });
    if let Some(m) = meta {
        params["_meta"] = m;
    }
    let (st, _, b) = rpc(app, tok, "2026-07-28", "tools/call", params, &[]).await;
    assert_eq!(st, StatusCode::OK, "{b}");
    b
}

#[tokio::test]
async fn mcp_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let dir = common::package_wasm_plugins();
    reset().await;
    let kernel = Kernel::start(config(&dir), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let u = register_login(&app, "mcp_user").await;
    let admin = register_login(&app, "mcp_admin").await;
    common::grant_role(admin.id, "admin").await;
    let victim = register_login(&app, "mcp_victim").await;

    // token：签发（明文只出现一次）、列表、token 不能签发 token、admin 作用域需要权限
    let tok = token(&app, &u, &["read", "write"]).await;
    assert!(tok.starts_with("ptm_"));
    let r = call(
        &app,
        "GET",
        "/api/p/identity/v1/users/me/tokens",
        None,
        &hv(&u.headers()),
    )
    .await;
    assert_eq!(
        (
            r.body["items"].as_array().map(Vec::len),
            r.body["items"][0].get("token")
        ),
        (Some(1), None)
    );
    let bearer = format!("Bearer {tok}");
    let r = call(
        &app,
        "POST",
        "/api/p/identity/v1/users/me/tokens",
        Some(json!({ "kind": "mcp", "name": "x", "scopes": ["read"] })),
        &[("authorization", bearer.as_str())],
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        "/api/p/identity/v1/users/me/tokens",
        Some(json!({ "kind": "mcp", "name": "x", "scopes": ["admin"] })),
        &hv(&u.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{:?}", r.body);
    // token 用于普通 API（Bearer）
    let r = call(
        &app,
        "GET",
        "/api/p/identity/v1/users/me",
        None,
        &[("authorization", bearer.as_str())],
    )
    .await;
    assert_eq!(r.body["user"]["username"], "mcp_user", "{:?}", r.body);

    // 传输层：GET 405、无凭据 401 + WWW-Authenticate、受保护资源元数据
    assert_eq!(
        call(&app, "GET", "/mcp", None, &[]).await.status,
        StatusCode::METHOD_NOT_ALLOWED
    );
    let r = call(
        &app,
        "POST",
        "/mcp",
        Some(json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" })),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert!(
        r.headers["www-authenticate"]
            .to_str()
            .unwrap()
            .contains("https://pt.example/.well-known/oauth-protected-resource")
    );
    let (st, _, _) = rpc(&app, "ptm_bogus", "2026-07-28", "ping", json!({}), &[]).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    let r = call(&app, "GET", "/.well-known/oauth-protected-resource", None, &[]).await;
    assert_eq!(r.body["resource"], "https://pt.example/mcp");

    // 2025-11-25：initialize → 会话 ID；带会话 ID 的 tools/list
    let (st, h, b) = rpc(&app, &tok, "2025-11-25", "initialize", json!({ "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } }), &[]).await;
    assert_eq!(
        (st, b["result"]["protocolVersion"].as_str()),
        (StatusCode::OK, Some("2025-11-25")),
        "{b}"
    );
    let sid = h["mcp-session-id"].to_str().unwrap().to_owned();
    let (_, _, b) = rpc(
        &app,
        &tok,
        "2025-11-25",
        "tools/list",
        json!({}),
        &[("mcp-session-id", sid.as_str())],
    )
    .await;
    let names: Vec<&str> = b["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert!(
        names.contains(&"get_my_profile")
            && names.contains(&"get_torrent")
            && names.contains(&"get_my_level"),
        "{names:?}"
    );
    assert!(!names.contains(&"admin_set_user_status"));
    let (st, _, _) = rpc(
        &app,
        &tok,
        "2025-11-25",
        "tools/list",
        json!({}),
        &[("mcp-session-id", "00.1")],
    )
    .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let (st, _, _) = rpc(&app, &tok, "2024-01-01", "ping", json!({}), &[]).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);

    // 2026-07-28：工具调用（Native 与 WASM 提供方）、错误
    let b = tool(&app, &tok, "get_my_profile", json!({}), None).await;
    assert_eq!(
        (
            b["result"]["isError"].as_bool(),
            b["result"]["structuredContent"]["username"].as_str()
        ),
        (Some(false), Some("mcp_user")),
        "{b}"
    );
    let b = tool(&app, &tok, "get_my_level", json!({}), None).await;
    assert_eq!(b["result"]["isError"], false, "{b}");
    assert!(b["result"]["structuredContent"].is_object());
    let b = tool(&app, &tok, "get_torrent", json!({ "torrent_id": "x" }), None).await;
    assert_eq!(b["result"]["isError"], true, "argument validation: {b}");
    let b = tool(&app, &tok, "get_torrent", json!({ "torrent_id": 999 }), None).await;
    assert!(
        b["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("not_found"),
        "{b}"
    );
    let b = tool(&app, &tok, "admin_set_user_status", json!({}), None).await;
    assert_eq!(
        b["error"]["code"], -32602,
        "admin tools are invisible to user tokens: {b}"
    );
    let (_, _, b) = rpc(&app, &tok, "2026-07-28", "resources/list", json!({}), &[]).await;
    assert_eq!(b["error"]["code"], -32601);
    let auth = format!("Bearer {tok}");
    let r = send(&app, "POST", "/mcp", &[], |bld| {
        bld.header("authorization", &auth)
            .header("mcp-protocol-version", "2026-07-28")
            .header("mcp-method", "tools/list")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }).to_string(),
            ))
            .unwrap()
    })
    .await;
    assert_eq!(
        r.body["error"]["code"], -32600,
        "Mcp-Method mismatch: {:?}",
        r.body
    );

    // 管理员工具：写操作需要站内确认；只读 admin token 不能执行写工具
    let victim_tok = token(&app, &victim, &["read"]).await;
    let atok = token(&app, &admin, &["read", "write", "admin"]).await;
    let args = json!({ "user_id": victim.id, "status": "banned", "reason_code": "cheating" });
    let b = tool(&app, &atok, "admin_set_user_status", args.clone(), None).await;
    let sc = &b["result"]["structuredContent"];
    assert_eq!(sc["status"], "confirmation_required", "{b}");
    let cid = sc["confirmation_id"].as_str().unwrap().to_owned();
    assert!(
        sc["confirm_url"]
            .as_str()
            .unwrap()
            .starts_with("https://pt.example/p/mcp/confirmations/")
    );
    let b = tool(
        &app,
        &atok,
        "admin_set_user_status",
        args.clone(),
        Some(json!({ "confirmation_id": cid })),
    )
    .await;
    assert!(
        b["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("confirmation_pending"),
        "{b}"
    );
    let r = call(
        &app,
        "GET",
        "/api/p/mcp/v1/me/confirmations",
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.body["items"][0]["id"], cid.as_str(), "{:?}", r.body);
    // 别人不能确认；token 不能确认
    let r = call(
        &app,
        "POST",
        &format!("/api/p/mcp/v1/me/confirmations/{cid}/approve"),
        Some(json!({})),
        &hv(&u.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        &format!("/api/p/mcp/v1/me/confirmations/{cid}/approve"),
        Some(json!({})),
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(
        (r.status, r.body["state"].as_str()),
        (StatusCode::OK, Some("executed")),
        "{:?}",
        r.body
    );
    assert_eq!(r.body["result"]["structuredContent"]["status"], "banned");
    // 重放：返回已保存的结果；参数不同 → 无效
    let b = tool(
        &app,
        &atok,
        "admin_set_user_status",
        args.clone(),
        Some(json!({ "confirmation_id": cid })),
    )
    .await;
    assert_eq!(b["result"]["structuredContent"]["status"], "banned", "{b}");
    let mut other = args.clone();
    other["status"] = json!("leech_disabled");
    let b = tool(
        &app,
        &atok,
        "admin_set_user_status",
        other,
        Some(json!({ "confirmation_id": cid })),
    )
    .await;
    assert!(
        b["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("confirmation_invalid"),
        "{b}"
    );
    // 被封禁用户的 token 失效
    let (st, _, _) = rpc(&app, &victim_tok, "2026-07-28", "ping", json!({}), &[]).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    // 拒绝
    let b = tool(
        &app,
        &atok,
        "admin_set_user_status",
        json!({ "user_id": u.id, "status": "leech_disabled", "reason_code": "test" }),
        None,
    )
    .await;
    let cid2 = b["result"]["structuredContent"]["confirmation_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = call(
        &app,
        "POST",
        &format!("/api/p/mcp/v1/me/confirmations/{cid2}/reject"),
        Some(json!({})),
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.body["state"], "rejected");
    let b = tool(
        &app,
        &atok,
        "admin_set_user_status",
        json!({ "user_id": u.id, "status": "leech_disabled", "reason_code": "test" }),
        Some(json!({ "confirmation_id": cid2 })),
    )
    .await;
    assert!(
        b["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("confirmation_invalid")
    );
    let ro = token(&app, &admin, &["read", "admin"]).await;
    let b = tool(
        &app,
        &ro,
        "admin_set_user_status",
        json!({ "user_id": u.id, "status": "banned", "reason_code": "x" }),
        None,
    )
    .await;
    assert_eq!(
        b["result"]["isError"], true,
        "read-only token must not reach confirmation: {b}"
    );

    // 撤销 token → 401
    let r = call(
        &app,
        "GET",
        "/api/p/identity/v1/users/me/tokens",
        None,
        &hv(&u.headers()),
    )
    .await;
    let tid = r.body["items"][0]["id"].as_str().unwrap().to_owned();
    let r = call(
        &app,
        "DELETE",
        &format!("/api/p/identity/v1/users/me/tokens/{tid}"),
        None,
        &hv(&u.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let (st, _, _) = rpc(&app, &tok, "2026-07-28", "ping", json!({}), &[]).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);

    let mut db = sqlx::PgConnection::connect(&common::env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM kernel.event_outbox WHERE event_type IN ('mcp.tool_called', 'mcp.confirmation_resolved')")
        .fetch_one(&mut db)
        .await
        .unwrap_or(0);
    let m: i64 = sqlx::query_scalar("SELECT count(*) FROM kernel.event_log WHERE event_type IN ('mcp.tool_called', 'mcp.confirmation_resolved')")
        .fetch_one(&mut db)
        .await
        .unwrap_or(0);
    assert!(n + m >= 8, "mcp events must be published ({n} + {m})");
    kernel.shutdown().await;
}

use sqlx::Connection;
