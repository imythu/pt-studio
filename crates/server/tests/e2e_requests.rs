//! 端到端：签到与求种（checkin、requests，WASM）—— 签到（连续、奖励经 ctx.ledger 到账、重复签到、日历与排行、MCP 免确认）、
//! 求种（发布并托管悬赏、追加、投票、规格要求检查、自我应求、应求通知、确认后发放、撤回退款、求种下的评论）。

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

async fn balance(app: &axum::Router, u: &User) -> i64 {
    let r = call(
        app,
        "GET",
        "/api/p/bonus/v1/users/me/summary",
        None,
        &hv(&u.headers()),
    )
    .await;
    r.body["balances"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|b| b["currency"] == "bonus")
        .and_then(|b| b["balance"].as_i64())
        .unwrap_or(0)
}

async fn post(app: &axum::Router, u: &User, path: &str, body: Value) -> common::Resp {
    call(app, "POST", path, Some(body), &hv(&u.headers())).await
}

#[tokio::test]
async fn requests_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let dir = common::package_wasm_plugins();
    reset().await;
    let kernel = Kernel::start(config(&dir), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let alice = register_login(&app, "rq_alice").await;
    let bob = register_login(&app, "rq_bob").await;
    let carol = register_login(&app, "rq_carol").await;

    // 签到：奖励到账；同日重复签到被拒；日历与排行
    let r = post(&app, &alice, "/api/p/checkin/v1/check-ins", json!({})).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(
        (
            r.body["streak"].as_i64(),
            r.body["rank"].as_i64(),
            r.body["reward"]["total"].as_i64(),
            r.body["reward"]["status"].as_str()
        ),
        (Some(1), Some(1), Some(10), Some("paid")),
        "{:?}",
        r.body
    );
    assert_eq!(balance(&app, &alice).await, 10);
    let r = post(&app, &alice, "/api/p/checkin/v1/check-ins", json!({})).await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (StatusCode::CONFLICT, Some("checkin.already_checked_in")),
        "{:?}",
        r.body
    );
    assert_eq!(balance(&app, &alice).await, 10, "no double payout");
    let s = call(&app, "GET", "/api/p/checkin/v1/me", None, &hv(&alice.headers())).await;
    let today: i64 = s.body["date"].as_str().unwrap()[8..10].parse().unwrap();
    assert_eq!(
        (
            s.body["checked_in_today"].as_bool(),
            s.body["streak"].as_i64(),
            s.body["month_days"].clone()
        ),
        (Some(true), Some(1), json!([today])),
        "{:?}",
        s.body
    );
    assert_eq!(s.body["next_reward"]["daily"], 12);
    // MCP：签到免确认
    let t = post(
        &app,
        &bob,
        "/api/p/identity/v1/users/me/tokens",
        json!({ "kind": "mcp", "name": "c", "scopes": ["read", "write"] }),
    )
    .await;
    let auth = format!("Bearer {}", t.body["token"].as_str().unwrap());
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": { "name": "check_in", "arguments": {} } });
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
        (
            r.body["result"]["structuredContent"]["streak"].as_i64(),
            r.body["result"]["structuredContent"]["rank"].as_i64()
        ),
        (Some(1), Some(2)),
        "{:?}",
        r.body
    );
    assert_eq!(balance(&app, &bob).await, 10);
    let l = call(
        &app,
        "GET",
        "/api/p/checkin/v1/leaderboard",
        None,
        &hv(&carol.headers()),
    )
    .await;
    assert_eq!(l.body["items"].as_array().map(Vec::len), Some(2));

    // 求种：发布（托管初始悬赏）、余额不足被拒、追加、投票
    let req = json!({ "type_id": "generic", "title": "求 Windows 版工具", "description": "**急需**", "bounty": 6,
                      "requirements": [{ "field": "platform", "op": "in", "value": ["windows"] }] });
    let r = post(&app, &alice, "/api/p/requests/v1/requests", req.clone()).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let rid = r.body["id"].as_i64().unwrap();
    assert_eq!(balance(&app, &alice).await, 4);
    let mut too_much = req.clone();
    too_much["bounty"] = json!(1000);
    let r = post(&app, &alice, "/api/p/requests/v1/requests", too_much).await;
    assert_eq!(r.body["code"], "ledger.insufficient_balance", "{:?}", r.body);
    let r = post(
        &app,
        &bob,
        &format!("/api/p/requests/v1/requests/{rid}/bounty"),
        json!({ "amount": 3 }),
    )
    .await;
    assert_eq!(r.body["bounty_total"], 9, "{:?}", r.body);
    assert_eq!(balance(&app, &bob).await, 7);
    assert_eq!(
        post(
            &app,
            &carol,
            &format!("/api/p/requests/v1/requests/{rid}/vote"),
            json!({})
        )
        .await
        .status,
        StatusCode::OK
    );
    let d = call(
        &app,
        "GET",
        &format!("/api/p/requests/v1/requests/{rid}"),
        None,
        &hv(&carol.headers()),
    )
    .await;
    assert_eq!(
        (
            d.body["votes"].as_i64(),
            d.body["voted"].as_bool(),
            d.body["description_html"].as_str()
        ),
        (Some(1), Some(true), Some("<p><strong>急需</strong></p>"))
    );
    assert_eq!(d.body["contributors"].as_array().map(Vec::len), Some(2));

    // 应求：自我应求被拒；规格不符被拒；符合 → 待确认并通知求种者
    let own = common::upload(&app, &alice, "Alice.Own.Upload", &[("a.bin", 4 << 20)]).await;
    assert_eq!(
        post(
            &app,
            &alice,
            &format!("/api/p/requests/v1/requests/{rid}/fills"),
            json!({ "torrent_id": own })
        )
        .await
        .body["code"],
        "requests.self_fill"
    );
    let strict = post(&app, &alice, "/api/p/requests/v1/requests", json!({ "type_id": "generic", "title": "求游戏", "bounty": 1, "requirements": [{ "field": "kind", "op": "eq", "value": "game" }] })).await;
    let sid = strict.body["id"].as_i64().unwrap();
    let tb = common::upload(&app, &bob, "Bob.Windows.Tool", &[("a.bin", 4 << 20)]).await;
    let r = post(
        &app,
        &bob,
        &format!("/api/p/requests/v1/requests/{sid}/fills"),
        json!({ "torrent_id": tb }),
    )
    .await;
    assert_eq!(r.body["code"], "requests.requirements_not_met", "{:?}", r.body);
    let r = post(
        &app,
        &bob,
        &format!("/api/p/requests/v1/requests/{rid}/fills"),
        json!({ "torrent_id": tb }),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let fid = r.body["fill_id"].as_i64().unwrap();
    let n = call(&app, "GET", "/api/p/notify/v1/inbox", None, &hv(&alice.headers())).await;
    assert_eq!(
        n.body["items"][0]["template_key"], "requests.fill_submitted",
        "{:?}",
        n.body
    );
    // 只有求种者 / 版主能确认；确认后悬赏发给应求者
    assert_eq!(
        post(
            &app,
            &carol,
            &format!("/api/p/requests/v1/fills/{fid}/confirm"),
            json!({})
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    // 上传本身也会得到魔力值奖励（bonus 插件），按差值核对
    let bob_before = balance(&app, &bob).await;
    let r = post(
        &app,
        &alice,
        &format!("/api/p/requests/v1/fills/{fid}/confirm"),
        json!({}),
    )
    .await;
    assert_eq!(
        (r.status, r.body["payout"].as_i64()),
        (StatusCode::OK, Some(9)),
        "{:?}",
        r.body
    );
    assert_eq!(balance(&app, &bob).await, bob_before + 9);
    let d = call(
        &app,
        "GET",
        &format!("/api/p/requests/v1/requests/{rid}"),
        None,
        &hv(&carol.headers()),
    )
    .await;
    assert_eq!(
        (d.body["status"].as_str(), d.body["filled_torrent_id"].as_i64()),
        (Some("filled"), Some(tb))
    );
    let n = call(&app, "GET", "/api/p/notify/v1/inbox", None, &hv(&bob.headers())).await;
    assert!(
        n.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["template_key"] == "requests.fill_confirmed" && x["args"]["payout"] == 9)
    );

    // 撤回：退还托管的悬赏
    let before = balance(&app, &alice).await;
    let r = post(
        &app,
        &alice,
        &format!("/api/p/requests/v1/requests/{sid}/withdraw"),
        json!({}),
    )
    .await;
    assert_eq!(r.body["state"], "closed", "{:?}", r.body);
    assert_eq!(balance(&app, &alice).await, before + 1);
    // 列表默认只含未结的求种
    let l = call(
        &app,
        "GET",
        "/api/p/requests/v1/requests",
        None,
        &hv(&carol.headers()),
    )
    .await;
    assert_eq!(l.body["items"].as_array().map(Vec::len), Some(0), "{:?}", l.body);
    let l = call(
        &app,
        "GET",
        "/api/p/requests/v1/requests?status=filled",
        None,
        &hv(&carol.headers()),
    )
    .await;
    assert_eq!(l.body["items"][0]["id"].as_i64(), Some(rid));

    // 求种下的评论：已应求的可以评论，已关闭的不行
    let r = post(
        &app,
        &carol,
        &format!("/api/p/comments/v1/targets/request:{rid}/comments"),
        json!({ "body": "感谢应求" }),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let r = post(
        &app,
        &carol,
        &format!("/api/p/comments/v1/targets/request:{sid}/comments"),
        json!({ "body": "已经关了" }),
    )
    .await;
    assert_eq!(r.body["code"], "comments.target_locked", "{:?}", r.body);
    kernel.shutdown().await;
}
