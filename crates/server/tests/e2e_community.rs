//! 端到端：社区（comments、reports，WASM）—— 评论（渲染安全、@提醒、楼层、回复与引用、防刷、编辑、感谢、目标校验）、
//! 举报（理由、合并工单、重复与自举报、认领、处理动作：删除评论 / 禁用种子、我的举报）、事件与 MCP 管理工具。

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};

use common::{User, call, hv, register_login, reset};

fn config(dir: &std::path::Path) -> SiteConfig {
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n",
        dir.display()
    ));
    SiteConfig::parse(&t).unwrap()
}

async fn post(app: &axum::Router, u: &User, target: &str, body: Value) -> common::Resp {
    call(
        app,
        "POST",
        &format!("/api/p/comments/v1/targets/{target}/comments"),
        Some(body),
        &hv(&u.headers()),
    )
    .await
}

#[tokio::test]
async fn community_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let dir = common::package_wasm_plugins();
    reset().await;
    let kernel = Kernel::start(config(&dir), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let alice = register_login(&app, "cm_alice").await;
    let bob = register_login(&app, "cm_bob").await;
    let modr = register_login(&app, "cm_mod").await;
    common::grant_role(modr.id, "moderator").await;
    let tid = common::upload(&app, &alice, "Community.Test.2026", &[("a.bin", 4 << 20)]).await;
    let target = format!("torrent:{tid}");

    // 发表：原始 HTML 被转义、@提醒解析为链接
    let r = post(
        &app,
        &alice,
        &target,
        json!({ "body": "**感谢分享** <script>alert(1)</script> @cm_bob @nobody", "markup": "markdown" }),
    )
    .await;
    assert_eq!(
        (r.status, r.body["floor"].as_i64()),
        (StatusCode::CREATED, Some(1)),
        "{:?}",
        r.body
    );
    let c1 = r.body["id"].as_i64().unwrap();
    let l = call(
        &app,
        "GET",
        &format!("/api/p/comments/v1/targets/{target}/comments"),
        None,
        &hv(&bob.headers()),
    )
    .await;
    let html = l.body["items"][0]["html"].as_str().unwrap().to_owned();
    assert!(
        html.contains("<strong>感谢分享</strong>")
            && html.contains("&lt;script&gt;")
            && !html.contains("<script"),
        "{html}"
    );
    assert!(
        html.contains(&format!(
            "<a href=\"/users/{}\" class=\"mention\">@cm_bob</a>",
            bob.id
        )) && html.contains("@nobody"),
        "{html}"
    );
    assert_eq!(
        (
            l.body["total"].as_i64(),
            l.body["items"][0]["author"]["username"].as_str()
        ),
        (Some(1), Some("cm_alice"))
    );

    // 回复（单层）、引用、BBCode
    let r = post(
        &app,
        &bob,
        &target,
        json!({ "body": "[b]同意[/b]", "markup": "bbcode", "reply_to": c1 }),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let c2 = r.body["id"].as_i64().unwrap();
    let r = post(
        &app,
        &alice,
        &target,
        json!({ "body": "回复的回复", "reply_to": c2 }),
    )
    .await;
    assert_eq!(r.body["code"], "comments.reply_invalid");
    let r = post(&app, &bob, &target, json!({ "body": "说得对", "quote_of": c1 })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let l = call(
        &app,
        "GET",
        &format!("/api/p/comments/v1/targets/{target}/comments"),
        None,
        &hv(&bob.headers()),
    )
    .await;
    let items = l.body["items"].as_array().unwrap();
    assert_eq!(
        items[0]["replies"][0]["html"].as_str(),
        Some("<strong>同意</strong>")
    );
    assert!(
        items[1]["html"].as_str().unwrap().contains("<blockquote>"),
        "{:?}",
        items[1]
    );

    // 防刷：近似重复、30 秒内超过 3 条
    let r = post(&app, &bob, &target, json!({ "body": "同意！" })).await;
    assert_eq!(r.body["code"], "comments.duplicate", "{:?}", r.body);
    let r = post(
        &app,
        &bob,
        &target,
        json!({ "body": "完全不同的另一段内容，讨论画质与字幕" }),
    )
    .await;
    assert_eq!(r.body["code"], "comments.rate_limited", "{:?}", r.body);

    // 编辑：作者可以，别人不行；感谢：不能感谢自己
    let r = call(
        &app,
        "PATCH",
        &format!("/api/p/comments/v1/comments/{c1}"),
        Some(json!({ "body": "改过的内容" })),
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = call(
        &app,
        "PATCH",
        &format!("/api/p/comments/v1/comments/{c1}"),
        Some(json!({ "body": "改过的内容" })),
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        &format!("/api/p/comments/v1/comments/{c1}/thanks"),
        Some(json!({})),
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = call(
        &app,
        "POST",
        &format!("/api/p/comments/v1/comments/{c1}/thanks"),
        Some(json!({})),
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let g = call(
        &app,
        "GET",
        &format!("/api/p/comments/v1/comments/{c1}"),
        None,
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(
        (
            g.body["thanks"].as_i64(),
            g.body["html"].as_str(),
            g.body["edited_at"].is_i64()
        ),
        (Some(1), Some("<p>改过的内容</p>"), true),
        "{:?}",
        g.body
    );
    let r = call(
        &app,
        "GET",
        &format!("/api/p/comments/v1/comments/{c1}/edits"),
        None,
        &hv(&modr.headers()),
    )
    .await;
    assert!(
        r.body["items"][0]["old_source"]
            .as_str()
            .unwrap()
            .contains("感谢分享")
    );
    assert_eq!(
        post(&app, &alice, "torrent:999999", json!({ "body": "x" }))
            .await
            .body["code"],
        "comments.target_not_found"
    );
    let r = call(
        &app,
        "POST",
        "/api/p/comments/v1/preview",
        Some(json!({ "body": "[url=javascript:x]a[/url]", "markup": "bbcode" })),
        &hv(&alice.headers()),
    )
    .await;
    assert!(!r.body["html"].as_str().unwrap().contains("href"), "{:?}", r.body);

    // 举报：理由、合并工单、重复、自举报
    let r = call(
        &app,
        "GET",
        "/api/p/reports/v1/reasons?kind=comment",
        None,
        &hv(&alice.headers()),
    )
    .await;
    assert!(
        r.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["code"] == "abuse")
    );
    let rep = |u: &User, target: String, code: &'static str| {
        let h = u.headers();
        let app = app.clone();
        async move {
            call(
                &app,
                "POST",
                "/api/p/reports/v1/reports",
                Some(json!({ "target": target, "reason_code": code, "note": "" })),
                &hv(&h),
            )
            .await
        }
    };
    let r = rep(&alice, format!("comment:{c2}"), "abuse").await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let case = r.body["case_id"].as_i64().unwrap();
    let r = rep(&modr, format!("comment:{c2}"), "spam").await;
    assert_eq!(
        r.body["case_id"].as_i64(),
        Some(case),
        "reports on the same target share a case"
    );
    assert_eq!(
        rep(&alice, format!("comment:{c2}"), "spam").await.body["code"],
        "reports.already_reported"
    );
    assert_eq!(
        rep(&alice, format!("user:{}", alice.id), "spam").await.body["code"],
        "reports.self_report"
    );
    assert_eq!(
        rep(&alice, format!("comment:{c2}"), "dupe").await.body["code"],
        "reports.reason_invalid"
    );
    assert_eq!(
        rep(&alice, "comment:999999".into(), "spam").await.body["code"],
        "reports.target_not_found"
    );

    // 工单：普通用户不可见；版主查看、认领、处理（删除评论）
    assert_eq!(
        call(&app, "GET", "/api/p/reports/v1/cases", None, &hv(&bob.headers()))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "GET",
        &format!("/api/p/reports/v1/cases/{case}"),
        None,
        &hv(&modr.headers()),
    )
    .await;
    assert_eq!(
        (
            r.body["report_count"].as_i64(),
            r.body["reports"].as_array().map(Vec::len)
        ),
        (Some(2), Some(2)),
        "{:?}",
        r.body
    );
    assert_eq!(r.body["target_preview"]["excerpt"], "同意");
    assert!(
        r.body["available_actions"]
            .as_array()
            .unwrap()
            .contains(&json!("delete_content"))
    );
    let r = call(
        &app,
        "POST",
        &format!("/api/p/reports/v1/cases/{case}/claim"),
        Some(json!({})),
        &hv(&modr.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        &format!("/api/p/reports/v1/cases/{case}/resolve"),
        Some(
            json!({ "code": "resolved", "note_public": "已删除", "actions": [{ "kind": "delete_content" }] }),
        ),
        &hv(&modr.headers()),
    )
    .await;
    assert_eq!(
        (r.status, r.body["state"].as_str()),
        (StatusCode::OK, Some("closed")),
        "{:?}",
        r.body
    );
    let g = call(
        &app,
        "GET",
        &format!("/api/p/comments/v1/targets/{target}/comments"),
        None,
        &hv(&bob.headers()),
    )
    .await;
    let reply = &g.body["items"][0]["replies"][0];
    assert_eq!(
        (reply["deleted"].as_bool(), reply["html"].clone()),
        (Some(true), Value::Null),
        "{:?}",
        g.body
    );
    let r = call(
        &app,
        "GET",
        "/api/p/reports/v1/my-reports",
        None,
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(
        (
            r.body["items"][0]["resolution"].as_str(),
            r.body["items"][0]["note_public"].as_str()
        ),
        (Some("resolved"), Some("已删除"))
    );

    // 种子举报 → 禁用种子
    let r = rep(&bob, target.clone(), "bad_file").await;
    let case2 = r.body["case_id"].as_i64().unwrap();
    let r = call(
        &app,
        "POST",
        &format!("/api/p/reports/v1/cases/{case2}/resolve"),
        Some(json!({ "code": "resolved", "actions": [{ "kind": "disable_torrent" }] })),
        &hv(&modr.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let t = call(
        &app,
        "GET",
        &format!("/api/p/torrent/v1/torrents/{tid}"),
        None,
        &hv(&modr.headers()),
    )
    .await;
    assert_eq!(t.body["status"], "disabled", "{:?}", t.body);
    // 不适用的动作 → 207 且不结案
    let r = rep(&alice, format!("user:{}", bob.id), "spam").await;
    let case3 = r.body["case_id"].as_i64().unwrap();
    let r = call(
        &app,
        "POST",
        &format!("/api/p/reports/v1/cases/{case3}/resolve"),
        Some(json!({ "code": "resolved", "actions": [{ "kind": "delete_content" }] })),
        &hv(&modr.headers()),
    )
    .await;
    assert_eq!(
        (r.status.as_u16(), r.body["results"][0]["error"].as_str()),
        (207, Some("reports.action_unavailable")),
        "{:?}",
        r.body
    );
    let r = call(
        &app,
        "POST",
        &format!("/api/p/reports/v1/cases/{case3}/resolve"),
        Some(json!({ "code": "invalid" })),
        &hv(&modr.headers()),
    )
    .await;
    assert_eq!(r.body["state"], "closed");

    // MCP：管理员工具列出工单
    let t = call(
        &app,
        "POST",
        "/api/p/identity/v1/users/me/tokens",
        Some(json!({ "kind": "mcp", "name": "m", "scopes": ["read", "admin"] })),
        &hv(&modr.headers()),
    )
    .await;
    let auth = format!("Bearer {}", t.body["token"].as_str().unwrap());
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "admin_list_reports", "arguments": { "state": "closed" } } });
    let r = common::send(&app, "POST", "/mcp", &[], |b| {
        b.header("authorization", &auth)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2026-07-28")
            .header("mcp-method", "tools/call")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap()
    })
    .await;
    assert_eq!(
        r.body["result"]["structuredContent"]["items"]
            .as_array()
            .map(Vec::len),
        Some(3),
        "{:?}",
        r.body
    );

    let mut db = sqlx::PgConnection::connect(&common::env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap();
    let n: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT event_type) FROM (SELECT event_type FROM kernel.event_outbox UNION ALL SELECT event_type FROM kernel.event_log) e
          WHERE event_type IN ('comment.created', 'comment.updated', 'comment.deleted', 'report.created', 'report.resolved')",
    )
    .fetch_one(&mut db)
    .await
    .unwrap();
    assert_eq!(n, 5);
    kernel.shutdown().await;
}

use sqlx::Connection;
