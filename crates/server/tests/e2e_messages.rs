//! 端到端：通知与站内信（notify、messages，WASM）—— 评论 @提醒 / 回复、审核决定、举报结论经 ctx.notify 进入通知中心；
//! 按类别关闭通知；已读；站内信会话（按用户名收件、未读、回复回到收件箱、拉黑、只接收员工、员工不受限）、举报私信（快照）、MCP。

#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

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

async fn inbox(app: &axum::Router, u: &User) -> Value {
    call(app, "GET", "/api/p/notify/v1/inbox", None, &hv(&u.headers()))
        .await
        .body
}

async fn comment(app: &axum::Router, u: &User, target: &str, body: Value) -> Value {
    let r = call(
        app,
        "POST",
        &format!("/api/p/comments/v1/targets/{target}/comments"),
        Some(body),
        &hv(&u.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    r.body
}

#[tokio::test]
async fn messages_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let dir = common::package_wasm_plugins();
    reset().await;
    let kernel = Kernel::start(config(&dir), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let admin = register_login(&app, "ms_admin").await;
    common::grant_role(admin.id, "admin").await;
    let modr = register_login(&app, "ms_mod").await;
    common::grant_role(modr.id, "moderator").await;
    let alice = register_login(&app, "ms_alice").await;
    let bob = register_login(&app, "ms_bob").await;
    let carol = register_login(&app, "ms_carol").await;
    let tid = common::upload(&app, &bob, "Messages.Torrent.2026", &[("a.bin", 4 << 20)]).await;
    let target = format!("torrent:{tid}");

    // 评论 @提醒 → 通知（带链接与模板参数）
    let c1 = comment(&app, &alice, &target, json!({ "body": "@ms_bob 感谢分享" })).await;
    let n = inbox(&app, &bob).await;
    assert_eq!(
        (n["unread"].as_i64(), n["items"][0]["template_key"].as_str()),
        (Some(1), Some("comments.mentioned")),
        "{n}"
    );
    assert_eq!(n["items"][0]["args"]["user"], "ms_alice");
    assert_eq!(
        n["items"][0]["link"],
        format!("/torrents/{tid}#comment-{}", c1["id"])
    );
    // 回复 → 被回复者收到；不通知自己
    comment(
        &app,
        &bob,
        &target,
        json!({ "body": "不客气", "reply_to": c1["id"] }),
    )
    .await;
    let n = inbox(&app, &alice).await;
    assert_eq!(n["items"][0]["template_key"], "comments.replied", "{n}");
    // 关闭“互动”类通知后不再收到
    let r = call(
        &app,
        "PUT",
        "/api/p/notify/v1/preferences",
        Some(json!({ "social": { "center": false } })),
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let p = call(
        &app,
        "GET",
        "/api/p/notify/v1/preferences",
        None,
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(
        (
            p.body["social"]["center"].as_bool(),
            p.body["security"]["locked"].as_bool()
        ),
        (Some(false), Some(true))
    );
    comment(&app, &carol, &target, json!({ "body": "@ms_alice 你好" })).await;
    assert_eq!(
        inbox(&app, &alice).await["items"].as_array().map(Vec::len),
        Some(1)
    );
    // 已读
    let r = call(
        &app,
        "POST",
        "/api/p/notify/v1/inbox/read",
        Some(json!({ "all": true })),
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(r.body["unread"], 0);
    assert_eq!(
        call(&app, "GET", "/api/p/notify/v1/unread", None, &hv(&bob.headers()))
            .await
            .body["count"],
        0
    );

    // 审核决定 → 上传者收到（含说明）
    let put = |plugin: &'static str, settings: Value| {
        let h = admin.headers();
        let app = app.clone();
        async move {
            let cur = call(
                &app,
                "GET",
                &format!("/api/kernel/v1/admin/plugins/{plugin}/settings"),
                None,
                &hv(&h),
            )
            .await;
            let r = call(
                &app,
                "PUT",
                &format!("/api/kernel/v1/admin/plugins/{plugin}/settings"),
                Some(json!({ "settings": settings, "version": cur.body["version"] })),
                &hv(&h),
            )
            .await;
            assert!(r.status.is_success(), "{:?}", r.body);
        }
    };
    put("moderation", json!({ "initial_rules": [{ "id": "all", "when": [{ "field": "approved_uploads", "op": "lt", "value": 100 }], "status": "pending" }] })).await;
    let tc = common::upload(&app, &carol, "Messages.Pending.2026", &[("a.bin", 4 << 20)]).await;
    let qt = format!("torrent:{tc}");
    let mut queued = false;
    for _ in 0..100 {
        if call(
            &app,
            "GET",
            &format!("/api/p/moderation/v1/queue/{qt}"),
            None,
            &hv(&modr.headers()),
        )
        .await
        .status
            == StatusCode::OK
        {
            queued = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(queued);
    let r = call(
        &app,
        "POST",
        &format!("/api/p/moderation/v1/queue/{qt}/decision"),
        Some(json!({ "decision": "reject", "note": "缺少简介" })),
        &hv(&modr.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let n = inbox(&app, &carol).await;
    let d = n["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["template_key"] == "moderation.decision.reject")
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(
        (d["args"]["note"].as_str(), d["args"]["title"].as_str()),
        (Some("缺少简介"), Some("Messages.Pending.2026")),
        "{n}"
    );

    // 举报结论 → 举报者收到
    let rep = call(
        &app,
        "POST",
        "/api/p/reports/v1/reports",
        Some(json!({ "target": format!("comment:{}", c1["id"]), "reason_code": "spam", "note": "" })),
        &hv(&carol.headers()),
    )
    .await;
    let case = rep.body["case_id"].as_i64().unwrap();
    let r = call(
        &app,
        "POST",
        &format!("/api/p/reports/v1/cases/{case}/resolve"),
        Some(json!({ "code": "invalid", "note_public": "未发现问题" })),
        &hv(&modr.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let n = inbox(&app, &carol).await;
    assert!(
        n["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["template_key"] == "reports.resolved.invalid" && x["args"]["note"] == "未发现问题"),
        "{n}"
    );

    // 站内信：按用户名收件、未读、打开即已读、回复回到对方收件箱
    let r = call(
        &app,
        "POST",
        "/api/p/messages/v1/conversations",
        Some(json!({ "recipients": ["ms_bob"], "subject": "你好", "body": "**想请教**一个问题" })),
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let conv = r.body["conversation_id"].as_i64().unwrap();
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/p/messages/v1/unread",
            None,
            &hv(&bob.headers())
        )
        .await
        .body["private"],
        1
    );
    let l = call(
        &app,
        "GET",
        "/api/p/messages/v1/conversations?folder=inbox",
        None,
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(
        (
            l.body["items"][0]["subject"].as_str(),
            l.body["items"][0]["participants"][0]["username"].as_str()
        ),
        (Some("你好"), Some("ms_alice")),
        "{:?}",
        l.body
    );
    let g = call(
        &app,
        "GET",
        &format!("/api/p/messages/v1/conversations/{conv}"),
        None,
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(
        g.body["messages"][0]["html"],
        "<p><strong>想请教</strong>一个问题</p>"
    );
    let first_msg = g.body["messages"][0]["id"].as_i64().unwrap();
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/p/messages/v1/unread",
            None,
            &hv(&bob.headers())
        )
        .await
        .body["private"],
        0
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/p/messages/v1/conversations/{conv}"),
            None,
            &hv(&carol.headers())
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    let r = call(
        &app,
        "POST",
        &format!("/api/p/messages/v1/conversations/{conv}/messages"),
        Some(json!({ "body": "请说" })),
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/p/messages/v1/unread",
            None,
            &hv(&alice.headers())
        )
        .await
        .body["private"],
        1
    );
    let s = call(
        &app,
        "GET",
        "/api/p/messages/v1/conversations?folder=sent",
        None,
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(s.body["items"][0]["id"].as_i64(), Some(conv));
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/p/messages/v1/conversations",
            Some(json!({ "recipients": ["nobody_here"], "subject": "x", "body": "x" })),
            &hv(&alice.headers())
        )
        .await
        .body["code"],
        "messages.recipient_not_found"
    );

    // 举报私信：快照随举报提交，工单预览可见原文
    let r = call(
        &app,
        "POST",
        &format!("/api/p/messages/v1/messages/{first_msg}/report"),
        Some(json!({ "reason_code": "spam", "note": "", "consent": false })),
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);
    let r = call(
        &app,
        "POST",
        &format!("/api/p/messages/v1/messages/{first_msg}/report"),
        Some(json!({ "reason_code": "spam", "note": "", "consent": true })),
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let d = call(
        &app,
        "GET",
        &format!("/api/p/reports/v1/cases/{}", r.body["case_id"]),
        None,
        &hv(&modr.headers()),
    )
    .await;
    assert_eq!(
        (
            d.body["kind"].as_str(),
            d.body["target_preview"]["sender_id"].as_i64()
        ),
        (Some("message"), Some(i64::try_from(alice.id).unwrap())),
        "{:?}",
        d.body
    );
    assert!(
        d.body["target_preview"]["text"]
            .as_str()
            .unwrap()
            .contains("想请教")
    );

    // 拉黑与只接收员工；员工不受限
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/p/messages/v1/blocks",
            Some(json!({ "user": "ms_alice" })),
            &hv(&bob.headers())
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let r = call(
        &app,
        "POST",
        "/api/p/messages/v1/conversations",
        Some(json!({ "recipients": ["ms_bob"], "subject": "再问", "body": "在吗" })),
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(r.body["code"], "messages.blocked", "{:?}", r.body);
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/p/messages/v1/settings",
            Some(json!({ "accept": "staff_only" })),
            &hv(&carol.headers())
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let r = call(
        &app,
        "POST",
        "/api/p/messages/v1/conversations",
        Some(json!({ "recipients": ["ms_carol"], "subject": "x", "body": "x" })),
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(r.body["code"], "messages.recipient_not_accepting");
    let r = call(
        &app,
        "POST",
        "/api/p/messages/v1/conversations",
        Some(json!({ "recipients": ["ms_carol"], "subject": "审核说明", "body": "请补充简介后重新提交" })),
        &hv(&modr.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);

    // MCP：列出站内信（不含正文）
    let t = call(
        &app,
        "POST",
        "/api/p/identity/v1/users/me/tokens",
        Some(json!({ "kind": "mcp", "name": "c", "scopes": ["read"] })),
        &hv(&alice.headers()),
    )
    .await;
    let auth = format!("Bearer {}", t.body["token"].as_str().unwrap());
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "list_my_messages", "arguments": {} } });
    let r = common::send(&app, "POST", "/mcp", &[], |b| {
        b.header("authorization", &auth)
            .header("content-type", "application/json")
            .header("mcp-protocol-version", "2026-07-28")
            .header("mcp-method", "tools/call")
            .body(axum::body::Body::from(body.to_string()))
            .unwrap()
    })
    .await;
    let sc = &r.body["result"]["structuredContent"];
    assert_eq!(sc["items"][0]["subject"], "你好", "{:?}", r.body);
    assert!(
        !sc.to_string().contains("想请教"),
        "MCP listing must not include message bodies"
    );
    kernel.shutdown().await;
}
