//! 端到端：ledger —— 发行与幂等、用户转账（手续费、钩子拒绝、余额不足）、冻结 / 扣款 / 到期、锁定、冲正、供给一致性。

#![allow(clippy::unwrap_used)]

mod common;

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::post};
use pt_kernel::Kernel;
use pt_plugin_api::{
    Ctx, HookCall, HookDecision, Hooks, HttpRoutes, Lifecycle, NativePlugin, PResult, PluginError,
    RequestCtx, json,
};
use pt_plugin_ledger::api::{
    CaptureRequest, CurrencyDef, CurrencyOptions, HoldRequest, LEDGER, LedgerService, LockRequest,
    TransferRequest,
};
use serde_json::Value;
use sqlx::{Connection, PgConnection};

use common::{call, config, env, reset};

const COIN_MANIFEST: &str = r#"
id = "x-coin"
name = "coin"
version = "0.1.0"
kind = "native"
api = "pt:plugin@0.1"
license = "MIT OR Apache-2.0"
requires = ["ctx.ledger@^0.1"]

[capabilities.scopes]
"ctx.ledger" = ["currency:coin", "lock", "read"]

[[hooks]]
hook = "ledger.before_transfer"

[[http.routes]]
id = "op"
method = "POST"
path = "/v1/op"
auth = "public"
"#;

struct Coin;

fn ledger(ctx: &Ctx) -> PResult<pt_plugin_api::Svc<dyn LedgerService>> {
    ctx.service::<dyn LedgerService>(LEDGER)
}

#[async_trait]
impl Lifecycle for Coin {
    async fn init(&self, ctx: &Ctx) -> PResult<()> {
        let def = CurrencyDef {
            code: "coin".into(),
            scale: 0,
            name_term: "x-coin.currency".into(),
            options: CurrencyOptions {
                transferable: Some(true),
                fee_bps: Some(500),
                min_transfer: Some(10),
                daily_out_limit: None,
            },
        };
        ledger(ctx)?.call(move |s, c| s.register_currency(c, def)).await
    }
}

#[async_trait]
impl Hooks for Coin {
    async fn handle(&self, _ctx: &Ctx, call: HookCall) -> PResult<HookDecision> {
        if call.input["amount"] == 666 {
            return Ok(HookDecision::Deny {
                code: "x-coin.unlucky_amount".into(),
                message: "nope".into(),
                details: None,
            });
        }
        Ok(HookDecision::Allow)
    }
}

/// 测试入口：`{op, ...}` 分派到 ctx.ledger 的各方法。
async fn op(ctx: RequestCtx, Json(b): Json<Value>) -> PResult<axum::response::Response> {
    let l = ledger(&ctx)?;
    let s = |k: &str| b[k].as_str().unwrap_or_default().to_owned();
    let u = |k: &str| b[k].as_u64();
    let out: Value = match b["op"].as_str().unwrap_or_default() {
        "issue" => {
            let req = TransferRequest {
                currency: "coin".into(),
                from_user: None,
                to_user: u("user"),
                amount: b["amount"].as_i64().unwrap_or(0),
                reason: "x-coin.grant".into(),
                memo: String::new(),
                idempotency_key: s("key"),
            };
            serde_json::to_value(l.call(move |s, c| s.transfer(c, req)).await?)?
        }
        "hold" => {
            let req = HoldRequest {
                currency: "coin".into(),
                user: u("user").unwrap_or(0),
                amount: b["amount"].as_i64().unwrap_or(0),
                reason: "x-coin.stake".into(),
                expires_at: b["expires_at"].as_i64(),
                idempotency_key: s("key"),
            };
            json!({ "hold_id": l.call(move |s, c| s.hold(c, req)).await? })
        }
        "capture" => {
            let req = CaptureRequest {
                hold_id: s("hold_id"),
                to_user: u("to"),
                amount: b["amount"].as_i64().unwrap_or(0),
                reason: "x-coin.win".into(),
                memo: String::new(),
                idempotency_key: s("key"),
            };
            serde_json::to_value(l.call(move |s, c| s.capture_hold(c, req)).await?)?
        }
        "lock" => {
            let req = LockRequest {
                user: u("user").unwrap_or(0),
                currency: None,
                reason: "test".into(),
                reference: "t".into(),
                idempotency_key: s("key"),
            };
            json!({ "lock_id": l.call(move |s, c| s.lock_account(c, req)).await? })
        }
        "unlock" => {
            let (id, key) = (s("lock_id"), s("key"));
            l.call(move |s, c| Box::pin(async move { s.unlock_account(c, &id, &key).await }))
                .await?;
            json!({})
        }
        "supply" => serde_json::to_value(l.call(|s, c| s.supply(c, "coin")).await?)?,
        "balances" => {
            let uid = u("user").unwrap_or(0);
            serde_json::to_value(l.call(move |s, c| s.balances(c, uid)).await?)?
        }
        _ => return Err(PluginError::bad_request("unknown op")),
    };
    Ok(Json(out).into_response())
}

struct CoinRoutes;
impl HttpRoutes for CoinRoutes {
    fn router(&self) -> Router {
        Router::new().route("/v1/op", post(op))
    }
}

fn coin() -> NativePlugin {
    let c = Arc::new(Coin);
    NativePlugin {
        manifest: COIN_MANIFEST,
        lifecycle: Some(c.clone()),
        hooks: Some(c),
        http: Some(Arc::new(CoinRoutes)),
        ..Default::default()
    }
}

async fn coin_op(app: &Router, body: Value) -> common::Resp {
    call(app, "POST", "/api/p/x-coin/v1/op", Some(body), &[]).await
}

async fn login(app: &Router, name: &str) -> (u64, String, String) {
    let r = call(
        app,
        "POST",
        "/api/p/identity/v1/users",
        Some(json!({"username": name, "email": format!("{name}@example.com"), "password": "correct horse"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let id = r.body["user"]["id"].as_u64().unwrap();
    let r = call(
        app,
        "POST",
        "/api/p/identity/v1/sessions",
        Some(json!({"username": name, "password": "correct horse"})),
        &[],
    )
    .await;
    (
        id,
        r.cookies.join("; "),
        r.body["csrf_token"].as_str().unwrap().to_owned(),
    )
}

async fn xfer(app: &Router, cookie: &str, csrf: &str, key: &str, body: Value) -> common::Resp {
    let h = [
        ("cookie", cookie),
        ("x-csrf-token", csrf),
        ("sec-fetch-site", "same-origin"),
        ("idempotency-key", key),
    ];
    call(app, "POST", "/api/p/ledger/v1/transfers", Some(body), &h).await
}

async fn balance_of(app: &Router, user: u64) -> (i64, i64) {
    let r = coin_op(app, json!({"op": "balances", "user": user})).await;
    let b = r
        .body
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["currency"] == "coin")
        .cloned()
        .unwrap_or(json!({"balance": 0, "held": 0}));
    (b["balance"].as_i64().unwrap(), b["held"].as_i64().unwrap())
}

#[tokio::test]
async fn ledger_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter("info,sqlx=warn")
        .try_init();
    reset().await;
    let mut plugins = pt_server::native_plugins();
    plugins.push(coin());
    let kernel = Kernel::start(config(), plugins)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();

    let (alice, a_cookie, a_csrf) = login(&app, "alice").await;
    let (bob, _, _) = login(&app, "bob").await;
    let (admin, m_cookie, m_csrf) = login(&app, "admin_1").await;
    let su = env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    );
    let mut db = PgConnection::connect(&su).await.unwrap();
    sqlx::query("INSERT INTO identity.user_roles (user_id, role, granted_by) VALUES ($1, 'admin', 'test')")
        .bind(i64::try_from(admin).unwrap())
        .execute(&mut db)
        .await
        .unwrap();
    let xf = |key: &'static str, body: Value| xfer(&app, &a_cookie, &a_csrf, key, body);

    // 发行与幂等
    let r = coin_op(
        &app,
        json!({"op": "issue", "user": alice, "amount": 1000, "key": "grant-1"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        (r.body["to_balance_after"].as_i64(), r.body["duplicate"].as_bool()),
        (Some(1000), Some(false))
    );
    let r = coin_op(
        &app,
        json!({"op": "issue", "user": alice, "amount": 1000, "key": "grant-1"}),
    )
    .await;
    assert_eq!(r.body["duplicate"], true);
    let r = coin_op(
        &app,
        json!({"op": "issue", "user": alice, "amount": 5, "key": "grant-1"}),
    )
    .await;
    assert_eq!(r.body["code"], "kernel.idempotency_key_reused");
    assert_eq!(balance_of(&app, alice).await, (1000, 0));

    // 用户转账：5% 手续费；重放同一 Idempotency-Key 不重复扣款
    let r = xf(
        "t-1",
        json!({"currency": "coin", "to_username": "Bob", "amount": 100}),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(
        (r.body["fee"].as_i64(), r.body["balance_after"].as_i64()),
        (Some(5), Some(895))
    );
    let r = xf(
        "t-1",
        json!({"currency": "coin", "to_username": "Bob", "amount": 100}),
    )
    .await;
    // 网关幂等键（kernel/03 §8.4）直接返回首次响应，不再进入 ledger
    assert_eq!((r.status, r.body["duplicate"].as_bool()), (StatusCode::CREATED, Some(false)));
    assert_eq!(r.headers["idempotent-replayed"], "true");
    let transfer_entry = r.body["entry_id"].as_str().unwrap().to_owned();
    assert_eq!(balance_of(&app, bob).await, (100, 0));

    // 钩子拒绝、余额不足、低于最小额、无效收款方
    for (key, body, code) in [
        (
            "t-2",
            json!({"currency": "coin", "to_username": "bob", "amount": 666}),
            "x-coin.unlucky_amount",
        ),
        (
            "t-3",
            json!({"currency": "coin", "to_username": "bob", "amount": 10_000}),
            "ledger.insufficient_balance",
        ),
        (
            "t-4",
            json!({"currency": "coin", "to_username": "bob", "amount": 5}),
            "ledger.amount_below_minimum",
        ),
        (
            "t-5",
            json!({"currency": "coin", "to_username": "nobody", "amount": 50}),
            "ledger.recipient_invalid",
        ),
        (
            "t-6",
            json!({"currency": "coin", "to_username": "alice", "amount": 50}),
            "ledger.self_transfer",
        ),
    ] {
        let r = xf(key, body).await;
        assert_eq!(r.body["code"], code, "{:?}", r.body);
    }
    assert_eq!(balance_of(&app, alice).await, (895, 0));

    // 冻结 500 后可用余额只有 395；扣款 200（系统回收），剩余冻结额释放
    let r = coin_op(
        &app,
        json!({"op": "hold", "user": alice, "amount": 500, "key": "h-1"}),
    )
    .await;
    let hold = r.body["hold_id"].as_str().unwrap().to_owned();
    assert_eq!(balance_of(&app, alice).await, (895, 500));
    let r = xf(
        "t-7",
        json!({"currency": "coin", "to_username": "bob", "amount": 400}),
    )
    .await;
    assert_eq!(r.body["code"], "ledger.insufficient_balance");
    assert_eq!(r.body["details"]["available"], 395);
    let r = coin_op(
        &app,
        json!({"op": "capture", "hold_id": hold, "amount": 200, "key": "c-1"}),
    )
    .await;
    assert_eq!(r.body["from_balance_after"], 695, "{:?}", r.body);
    assert_eq!(balance_of(&app, alice).await, (695, 0));
    let r = coin_op(
        &app,
        json!({"op": "capture", "hold_id": hold, "amount": 1, "key": "c-2"}),
    )
    .await;
    assert_eq!(r.body["code"], "ledger.hold_not_active");

    // 到期自动释放（内核延迟任务）
    let exp = chrono::Utc::now().timestamp_millis() + 1000;
    coin_op(
        &app,
        json!({"op": "hold", "user": alice, "amount": 100, "key": "h-2", "expires_at": exp}),
    )
    .await;
    assert_eq!(balance_of(&app, alice).await, (695, 100));
    let mut released = false;
    for _ in 0..80 {
        if balance_of(&app, alice).await == (695, 0) {
            released = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(released, "expired hold was not released");

    // 锁定：转账被拒；解锁后恢复
    let r = coin_op(&app, json!({"op": "lock", "user": alice, "key": "l-1"})).await;
    let lock = r.body["lock_id"].as_str().unwrap().to_owned();
    let r = xf(
        "t-8",
        json!({"currency": "coin", "to_username": "bob", "amount": 10}),
    )
    .await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("ledger.account_locked"))
    );
    coin_op(&app, json!({"op": "unlock", "lock_id": lock, "key": "u-1"})).await;
    let r = xf(
        "t-9",
        json!({"currency": "coin", "to_username": "bob", "amount": 10}),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    // 10 × 5% = 0.5，手续费向下取整为 0
    assert_eq!(balance_of(&app, alice).await, (685, 0));

    // 管理员冲正第一笔转账：alice +105，bob −100，手续费从回收中退回
    let m = [
        ("cookie", m_cookie.as_str()),
        ("x-csrf-token", m_csrf.as_str()),
        ("sec-fetch-site", "same-origin"),
        ("idempotency-key", "rev-1"),
    ];
    let r = call(
        &app,
        "POST",
        &format!("/api/p/ledger/v1/admin/entries/{transfer_entry}/reversals"),
        Some(json!({"reason_note": "test"})),
        &m,
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let m2 = [
        ("cookie", m_cookie.as_str()),
        ("x-csrf-token", m_csrf.as_str()),
        ("sec-fetch-site", "same-origin"),
        ("idempotency-key", "rev-2"),
    ];
    let r = call(
        &app,
        "POST",
        &format!("/api/p/ledger/v1/admin/entries/{transfer_entry}/reversals"),
        Some(json!({"reason_note": "again"})),
        &m2,
    )
    .await;
    assert_eq!(r.body["code"], "ledger.already_reversed");
    // 普通用户不能调用管理接口
    let h = [
        ("cookie", a_cookie.as_str()),
        ("x-csrf-token", a_csrf.as_str()),
        ("sec-fetch-site", "same-origin"),
        ("idempotency-key", "adj-1"),
    ];
    let r = call(
        &app,
        "POST",
        "/api/p/ledger/v1/admin/adjustments",
        Some(json!({"user_id": alice, "currency": "coin", "amount": 5, "reason_note": "x"})),
        &h,
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // 供给一致性：Σ余额 = 发行 − 回收
    let (ab, _) = balance_of(&app, alice).await;
    let (bb, _) = balance_of(&app, bob).await;
    let s = coin_op(&app, json!({"op": "supply"})).await.body;
    assert_eq!(
        s["circulating"].as_i64(),
        Some(ab + bb),
        "supply {s:?}, alice {ab}, bob {bb}"
    );
    assert_eq!(s["issued"], 1000);

    // 分录平衡：每笔流水的分录之和为 0
    let unbalanced: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (SELECT entry_id FROM ledger.lines GROUP BY entry_id HAVING sum(amount) <> 0) x")
        .fetch_one(&mut db).await.unwrap();
    assert_eq!(unbalanced, 0);
    let posted: i64 =
        sqlx::query_scalar("SELECT count(*) FROM kernel.event_log WHERE event_type = 'ledger.entry_posted'")
            .fetch_one(&mut db)
            .await
            .unwrap();
    assert!(posted >= 8, "entry_posted events: {posted}");

    kernel.shutdown().await;
}
