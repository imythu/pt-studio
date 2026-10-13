//! 端到端：审核（moderation、ai-moderation，WASM）—— 上传初始状态规则、审核队列（AI 预审结论）、认领冲突、退回修改与重新待审、
//! 通过（经 ctx.torrents.set_status）、决定日志、上传者统计影响规则、候选区投票、敏感词（遮蔽 / 拦截）、ai-moderation 本地规则
//! 命中后以系统身份建举报工单、工单结论反馈。

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

async fn put_settings(app: &axum::Router, admin: &User, plugin: &str, settings: Value) {
    let cur = call(
        app,
        "GET",
        &format!("/api/kernel/v1/admin/plugins/{plugin}/settings"),
        None,
        &hv(&admin.headers()),
    )
    .await;
    let version = cur.body["version"].as_i64().unwrap_or(0);
    let r = call(
        app,
        "PUT",
        &format!("/api/kernel/v1/admin/plugins/{plugin}/settings"),
        Some(json!({ "settings": settings, "version": version })),
        &hv(&admin.headers()),
    )
    .await;
    assert!(r.status.is_success(), "{:?}", r.body);
}

async fn status(app: &axum::Router, u: &User, tid: i64) -> String {
    call(
        app,
        "GET",
        &format!("/api/p/torrent/v1/torrents/{tid}"),
        None,
        &hv(&u.headers()),
    )
    .await
    .body["status"]
        .as_str()
        .unwrap_or("")
        .to_owned()
}

async fn wait_for(mut f: impl AsyncFnMut() -> bool) -> bool {
    for _ in 0..200 {
        if f().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    false
}

#[tokio::test]
async fn moderation_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let dir = common::package_wasm_plugins();
    reset().await;
    let kernel = Kernel::start(config(&dir), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let admin = register_login(&app, "md_admin").await;
    common::grant_role(admin.id, "admin").await;
    let m1 = register_login(&app, "md_mod1").await;
    common::grant_role(m1.id, "moderator").await;
    let m2 = register_login(&app, "md_mod2").await;
    common::grant_role(m2.id, "moderator").await;
    let alice = register_login(&app, "md_alice").await;
    let bob = register_login(&app, "md_bob").await;
    let carol = register_login(&app, "md_carol").await;

    // 规则：版主直接通过；没有通过过的上传者先待审
    put_settings(&app, &admin, "moderation", json!({
        "initial_rules": [
            { "id": "staff", "when": [{ "field": "roles", "op": "in", "value": ["moderator", "admin"] }], "status": "approved" },
            { "id": "new_uploader", "when": [{ "field": "approved_uploads", "op": "lt", "value": 1 }], "status": "pending" },
        ],
        "candidate_enabled": true, "approve_votes": 1, "vote_min_account_days": 0,
    }))
    .await;
    let t1 = common::upload(&app, &alice, "Moderation.First.2026", &[("a.bin", 4 << 20)]).await;
    assert_eq!(status(&app, &alice, t1).await, "pending");
    let tm = common::upload(&app, &m1, "Moderation.Staff.2026", &[("a.bin", 4 << 20)]).await;
    assert_eq!(status(&app, &m1, tm).await, "approved");

    // 队列：AI 预审结论随事件写入（未配置模型 → 只有规则结果）
    let target = format!("torrent:{t1}");
    assert!(
        wait_for(async || call(
            &app,
            "GET",
            &format!("/api/p/moderation/v1/queue/{target}"),
            None,
            &hv(&m1.headers())
        )
        .await
        .body["ai_verdict"]
            .is_string())
        .await,
        "queue entry with AI verdict"
    );
    let q = call(
        &app,
        "GET",
        "/api/p/moderation/v1/queue",
        None,
        &hv(&m1.headers()),
    )
    .await;
    assert_eq!(
        (
            q.body["items"].as_array().map(Vec::len),
            q.body["items"][0]["title"].as_str()
        ),
        (Some(1), Some("Moderation.First.2026")),
        "{:?}",
        q.body
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/p/moderation/v1/queue",
            None,
            &hv(&alice.headers())
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    // 认领冲突
    assert_eq!(
        call(
            &app,
            "POST",
            &format!("/api/p/moderation/v1/queue/{target}/claim"),
            Some(json!({})),
            &hv(&m1.headers())
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    let r = call(
        &app,
        "POST",
        &format!("/api/p/moderation/v1/queue/{target}/decision"),
        Some(json!({ "decision": "approve" })),
        &hv(&m2.headers()),
    )
    .await;
    assert_eq!(r.body["code"], "moderation.claimed_by_other", "{:?}", r.body);

    // 退回修改 → 上传者修改 → 重新待审 → 通过
    let r = call(
        &app,
        "POST",
        &format!("/api/p/moderation/v1/queue/{target}/decision"),
        Some(json!({ "decision": "needs_edit", "note": "请补充简介" })),
        &hv(&m1.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let q = call(
        &app,
        "GET",
        &format!("/api/p/moderation/v1/queue/{target}"),
        None,
        &hv(&m1.headers()),
    )
    .await;
    assert_eq!(q.body["state"], "awaiting_uploader");
    let r = call(
        &app,
        "PATCH",
        &format!("/api/p/torrent/v1/torrents/{t1}"),
        Some(json!({ "description": "补充的简介" })),
        &hv(&alice.headers()),
    )
    .await;
    assert!(r.status.is_success(), "{:?}", r.body);
    assert!(
        wait_for(async || call(
            &app,
            "GET",
            &format!("/api/p/moderation/v1/queue/{target}"),
            None,
            &hv(&m1.headers())
        )
        .await
        .body["state"]
            == "pending")
        .await,
        "back to pending after the uploader edits"
    );
    let r = call(
        &app,
        "POST",
        &format!("/api/p/moderation/v1/queue/{target}/decision"),
        Some(json!({ "decision": "approve" })),
        &hv(&m1.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(status(&app, &alice, t1).await, "approved");
    let h = call(
        &app,
        "GET",
        &format!("/api/p/moderation/v1/history?target={target}"),
        None,
        &hv(&m1.headers()),
    )
    .await;
    let decisions: Vec<&str> = h.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|x| x["decision"].as_str())
        .collect();
    assert_eq!(decisions, vec!["approve", "needs_edit"]);
    assert_eq!(h.body["items"][0]["snapshot"]["status_before"], "pending");
    // 通过过一次后不再命中“新上传者”规则
    assert!(
        wait_for(async || {
            let t = common::upload(
                &app,
                &alice,
                &format!("Moderation.Second.{}", rand_suffix()),
                &[("a.bin", 4 << 20)],
            )
            .await;
            status(&app, &alice, t).await == "approved"
        })
        .await
    );

    // 候选区：规则改为候选；上传者不能给自己投票；净赞成达到阈值即通过
    put_settings(&app, &admin, "moderation", json!({
        "initial_rules": [{ "id": "new_uploader", "when": [{ "field": "approved_uploads", "op": "lt", "value": 1 }], "status": "candidate" }],
        "candidate_enabled": true, "approve_votes": 1, "vote_min_account_days": 0,
    }))
    .await;
    let tb = common::upload(&app, &bob, "Moderation.Candidate.2026", &[("a.bin", 4 << 20)]).await;
    assert_eq!(status(&app, &bob, tb).await, "candidate");
    let c = call(
        &app,
        "GET",
        "/api/p/moderation/v1/candidates",
        None,
        &hv(&carol.headers()),
    )
    .await;
    assert_eq!(c.body["items"][0]["torrent_id"].as_i64(), Some(tb));
    let r = call(
        &app,
        "POST",
        &format!("/api/p/moderation/v1/candidates/{tb}/vote"),
        Some(json!({ "value": 1 })),
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(r.body["code"], "moderation.vote_not_allowed");
    let r = call(
        &app,
        "POST",
        &format!("/api/p/moderation/v1/candidates/{tb}/vote"),
        Some(json!({ "value": 1 })),
        &hv(&carol.headers()),
    )
    .await;
    assert_eq!(
        (r.body["up"].as_i64(), r.body["resolved"].as_str()),
        (Some(1), Some("approved")),
        "{:?}",
        r.body
    );
    assert_eq!(status(&app, &bob, tb).await, "approved");

    // 敏感词：遮蔽与拦截（不回显命中词）
    for (p, a) in [("坏词", "mask"), ("spamsite", "block")] {
        let r = call(
            &app,
            "POST",
            "/api/p/moderation/v1/admin/words",
            Some(json!({ "pattern": p, "action": a, "scopes": ["comment"] })),
            &hv(&admin.headers()),
        )
        .await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    }
    let r = call(
        &app,
        "POST",
        &format!("/api/p/comments/v1/targets/torrent:{tm}/comments"),
        Some(json!({ "body": "这里有坏词" })),
        &hv(&carol.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let g = call(
        &app,
        "GET",
        &format!("/api/p/comments/v1/comments/{}", r.body["id"]),
        None,
        &hv(&carol.headers()),
    )
    .await;
    assert_eq!(g.body["html"], "<p>这里有**</p>");
    let r = call(
        &app,
        "POST",
        &format!("/api/p/comments/v1/targets/torrent:{tm}/comments"),
        Some(json!({ "body": "去 SpamSite 看看" })),
        &hv(&carol.headers()),
    )
    .await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("moderation.blocked_word")),
        "{:?}",
        r.body
    );
    assert!(!r.body.to_string().to_lowercase().contains("spamsite"));

    // ai-moderation：新用户的评论全部审核；本地规则（泄露 passkey）命中 → 系统举报工单
    let r = call(
        &app,
        "POST",
        &format!("/api/p/comments/v1/targets/torrent:{tm}/comments"),
        Some(json!({ "body": "我的 passkey 是 0123456789abcdef0123456789abcdef 大家随便用" })),
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let cid = r.body["id"].as_i64().unwrap();
    let mut case = Value::Null;
    assert!(
        wait_for(async || {
            let r = call(&app, "GET", "/api/p/reports/v1/cases", None, &hv(&m1.headers())).await;
            case = r.body["items"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|c| c["target"] == format!("comment:{cid}"))
                .cloned()
                .unwrap_or(Value::Null);
            !case.is_null()
        })
        .await,
        "AI moderation must file a report"
    );
    let case_id = case["id"].as_i64().unwrap();
    let d = call(
        &app,
        "GET",
        &format!("/api/p/reports/v1/cases/{case_id}"),
        None,
        &hv(&m1.headers()),
    )
    .await;
    assert_eq!(
        (
            d.body["reports"][0]["reason_code"].as_str(),
            d.body["reports"][0]["reporter"]["id"].as_i64()
        ),
        (Some("personal_info"), Some(0)),
        "{:?}",
        d.body
    );
    assert!(
        d.body["reports"][0]["note"]
            .as_str()
            .unwrap()
            .starts_with("AI 判定：personal_info")
    );
    let r = call(
        &app,
        "POST",
        &format!("/api/p/reports/v1/cases/{case_id}/resolve"),
        Some(json!({ "code": "resolved", "actions": [{ "kind": "delete_content" }] })),
        &hv(&m1.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(
        wait_for(async || {
            let f = call(
                &app,
                "GET",
                "/api/p/ai-moderation/v1/admin/flags",
                None,
                &hv(&admin.headers()),
            )
            .await;
            f.body["items"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|x| x["target"] == format!("comment:{cid}") && x["outcome"] == "true_positive")
        })
        .await,
        "report outcome must be fed back to the flag"
    );
    let s = call(
        &app,
        "GET",
        "/api/p/ai-moderation/v1/admin/stats",
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(s.body["items"][0]["precision"], 1.0, "{:?}", s.body);
    kernel.shutdown().await;
}

fn rand_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}
