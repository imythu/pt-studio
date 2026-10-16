//! 端到端：anticheat（WASM）—— tracker 流 → accounting 基础信号 → 证据与评分 → 结论性规则开案并自动冻结流量 + 锁定钱包
//! → 复核（认领、四眼封禁）→ 凭令牌申诉 → 撤销回滚；伪造 started 的计数异常；规则集校验 / 模拟 / 激活；转出钩子。

#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Connection, PgConnection};

use common::{User, call, env, hv, register_login, reset, upload};

const GIB: u64 = 1 << 30;
const SECRET: &str = "e2e-anticheat-appeal-token-secret-0123456789";

fn config(dir: &std::path::Path) -> SiteConfig {
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugin.anticheat]\nappeal_token_secret = \"{SECRET}\"\n\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n",
        dir.display()
    ));
    SiteConfig::parse(&t).unwrap()
}

async fn db() -> PgConnection {
    PgConnection::connect(&env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap()
}

#[allow(clippy::too_many_arguments)]
async fn announce(
    r: &mut redis::aio::MultiplexedConnection,
    uid: u64,
    tid: i64,
    eid: &str,
    ev: &str,
    up: u64,
    dup: u64,
    flags: u32,
    xup: Option<u64>,
) {
    let mut f = vec![
        ("v", "1".to_owned()),
        ("eid", eid.to_owned()),
        ("ts", chrono::Utc::now().timestamp().to_string()),
        ("uid", uid.to_string()),
        ("tid", tid.to_string()),
        ("ih", "ab".repeat(20)),
        ("pid", "2d5452343033302d".to_owned() + &"61".repeat(12)),
        ("ip", "203.0.113.9".into()),
        ("port", "1".into()),
        ("ev", ev.into()),
        ("up", up.to_string()),
        ("down", "0".into()),
        ("left", "0".into()),
        ("dup", dup.to_string()),
        ("ddown", "0".into()),
        ("dseed", "30".into()),
        ("dleech", "0".into()),
        ("flags", flags.to_string()),
        ("ua", "Transmission/4.0.3".into()),
    ];
    if let Some(x) = xup {
        f.push(("xup", x.to_string()));
        f.push(("xdown", "0".into()));
    }
    let _: String = redis::cmd("XADD")
        .arg(format!("acct:{}", uid % 2))
        .arg("*")
        .arg(&f)
        .query_async(r)
        .await
        .unwrap();
}

async fn wait_for<F, Fut>(what: &str, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..300 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn scalar(sql: &str, uid: u64) -> i64 {
    sqlx::query_scalar::<_, i64>(sql)
        .bind(i64::try_from(uid).unwrap())
        .fetch_one(&mut db().await)
        .await
        .unwrap()
}

fn hmac(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    k[..key.len()].copy_from_slice(key);
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(msg);
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

fn b64url(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for k in 0..=c.len() {
            out.push(A[((n >> (18 - 6 * k)) & 63) as usize] as char);
        }
    }
    out
}

async fn get(app: &axum::Router, u: &User, path: &str) -> common::Resp {
    call(app, "GET", path, None, &hv(&u.headers())).await
}

async fn post(app: &axum::Router, u: &User, path: &str, body: Value) -> common::Resp {
    call(app, "POST", path, Some(body), &hv(&u.headers())).await
}

#[tokio::test]
async fn anticheat_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter("info,sqlx=warn,cranelift=warn,wasmtime=warn")
        .try_init();
    let dir = common::package_wasm_plugins();
    reset().await;
    let mut rc = redis::Client::open(env("PT_TEST_REDIS_URL", "redis://127.0.0.1:56379/15"))
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap();
    let _: () = redis::cmd("HSET")
        .arg("acct:_meta")
        .arg("partitions")
        .arg(2)
        .query_async(&mut rc)
        .await
        .unwrap();
    let kernel = Kernel::start(config(&dir), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();

    let admin = register_login(&app, "ac_admin").await;
    common::grant_role(admin.id, "admin").await;
    let admin2 = register_login(&app, "ac_admin2").await;
    common::grant_role(admin2.id, "admin").await;
    let admin3 = register_login(&app, "ac_admin3").await;
    common::grant_role(admin3.id, "admin").await;
    let alice = register_login(&app, "ac_alice").await;
    let carol = register_login(&app, "ac_carol").await;
    let tid = upload(&app, &admin, "Some.Linux.Iso.2026", &[("a.iso", 600 << 20)]).await;
    wait_for("torrent mirror", || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM accounting.torrents")
            .fetch_one(&mut db().await)
            .await
            .unwrap()
            > 0
    })
    .await;

    // ---- 伪造 started（首报带 50 GiB）：只建基线，产生 counter_jump/started_nonzero，分数低、不开案 ----
    announce(
        &mut rc,
        carol.id,
        tid,
        "n1:1:1",
        "started",
        50 * GIB,
        0,
        1 | (1 << 1),
        None,
    )
    .await;
    wait_for("started_nonzero evidence", || async {
        scalar("SELECT count(*) FROM anticheat.evidence WHERE user_id = $1 AND details->>'kind' = 'started_nonzero'", carol.id).await == 1
    })
    .await;
    assert_eq!(
        scalar(
            "SELECT count(*) FROM anticheat.cases WHERE user_id = $1",
            carol.id
        )
        .await,
        0
    );
    assert_eq!(
        scalar(
            "SELECT score::bigint FROM anticheat.user_risk WHERE user_id = $1",
            carol.id
        )
        .await,
        12
    );

    // ---- pt-tracker 截断的超速段：20 GiB / 30 s → speed_extreme（结论性）→ 开案 + 自动冻结流量 + 锁定钱包 ----
    announce(
        &mut rc,
        alice.id,
        tid,
        "n1:1:2",
        "none",
        0,
        0,
        1 << 7,
        Some(20 * GIB),
    )
    .await;
    wait_for("accounting signal", || async {
        scalar(
            "SELECT count(*) FROM accounting.signals WHERE user_id = $1 AND signal = 'speed_exceeded'",
            alice.id,
        )
        .await
            == 1
    })
    .await;
    wait_for("open case", || async {
        scalar(
            "SELECT count(*) FROM anticheat.cases WHERE user_id = $1 AND state = 'open'",
            alice.id,
        )
        .await
            == 1
    })
    .await;
    wait_for("auto freeze applied", || async {
        scalar("SELECT count(*) FROM anticheat.sanctions WHERE user_id = $1 AND state = 'active' AND policy_id = 'conclusive_freeze'", alice.id).await == 2
    })
    .await;
    assert_eq!(
        scalar("SELECT count(*) FROM accounting.rate_rules WHERE external_ref = 'freeze:' || $1::text AND is_current AND up_permille = 0", alice.id).await,
        1
    );
    assert_eq!(
        scalar(
            "SELECT count(*) FROM ledger.account_locks WHERE user_id = $1 AND released_at IS NULL",
            alice.id
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            "SELECT transfer_blocked::int::bigint FROM anticheat.user_risk WHERE user_id = $1",
            alice.id
        )
        .await,
        1
    );

    // ---- 后台：队列与详情 ----
    let r = get(&app, &admin, "/api/p/anticheat/v1/admin/cases").await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let case = r.body["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["user"]["id"] == json!(alice.id))
        .unwrap()
        .clone();
    let cid = case["id"].as_i64().unwrap();
    assert_eq!(case["primary_rule"], "speed_extreme");
    assert_eq!(case["band"], "critical");
    let d = get(&app, &admin, &format!("/api/p/anticheat/v1/admin/cases/{cid}")).await;
    assert_eq!(d.status, StatusCode::OK, "{:?}", d.body);
    assert_eq!(d.body["evidence"][0]["kind"], "speed_exceeded");
    assert_eq!(d.body["evidence"][0]["torrent"]["id"], json!(tid));
    assert!(
        d.body["timeline"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "auto_action")
    );
    let r = get(
        &app,
        &admin,
        &format!("/api/p/anticheat/v1/admin/users/{}/risk", alice.id),
    )
    .await;
    assert_eq!(r.body["under_review"], true);
    // 普通用户看不到后台
    assert_eq!(
        get(&app, &alice, "/api/p/anticheat/v1/admin/cases").await.status,
        StatusCode::FORBIDDEN
    );

    // ---- 复核：admin2 认领后 admin 不能决定；admin 决定封禁 → 提议（四眼）→ 自批被拒 → admin2 批准 ----
    let r = post(
        &app,
        &admin2,
        &format!("/api/p/anticheat/v1/admin/cases/{cid}/claims"),
        json!({}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let res = |note: &str| json!({ "decision": "sanction", "actions": [{ "kind": "ban", "days": 30 }, { "kind": "warn" }], "note": note });
    let r = post(
        &app,
        &admin,
        &format!("/api/p/anticheat/v1/admin/cases/{cid}/resolutions"),
        res("单次 20 GiB / 30 s"),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{:?}", r.body);
    assert_eq!(r.body["code"], "anticheat.claim_required");
    call(
        &app,
        "DELETE",
        &format!("/api/p/anticheat/v1/admin/cases/{cid}/claims"),
        None,
        &hv(&admin2.headers()),
    )
    .await;
    let r = post(
        &app,
        &admin,
        &format!("/api/p/anticheat/v1/admin/cases/{cid}/resolutions"),
        res("单次 20 GiB / 30 s"),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["state"], "resolved");
    let ban = r.body["sanctions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["kind"] == "ban")
        .unwrap()["id"]
        .as_i64()
        .unwrap();
    assert_eq!(
        r.body["sanctions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["kind"] == "ban")
            .unwrap()["state"],
        "proposed"
    );
    let r = post(
        &app,
        &admin,
        &format!("/api/p/anticheat/v1/admin/sanctions/{ban}/approvals"),
        json!({}),
    )
    .await;
    assert_eq!(r.body["code"], "anticheat.four_eyes_required", "{:?}", r.body);
    let r = post(
        &app,
        &admin2,
        &format!("/api/p/anticheat/v1/admin/sanctions/{ban}/approvals"),
        json!({}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    wait_for("ban applied", || async {
        sqlx::query_scalar::<_, String>("SELECT status FROM identity.users WHERE id = $1")
            .bind(i64::try_from(alice.id).unwrap())
            .fetch_one(&mut db().await)
            .await
            .unwrap()
            == "banned"
    })
    .await;
    wait_for("ban notice op", || async {
        scalar("SELECT count(*) FROM anticheat.effect_ops WHERE user_id = $1 AND op = 'ban_notice' AND state = 'done'", alice.id).await == 1
    })
    .await;

    // ---- 凭令牌申诉（被封禁用户无法登录）----
    let op_id: i64 = sqlx::query_scalar("SELECT op_id FROM anticheat.appeal_tokens WHERE user_id = $1")
        .bind(i64::try_from(alice.id).unwrap())
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    let token = b64url(&hmac(SECRET.as_bytes(), format!("appeal:{op_id}").as_bytes()));
    let r = call(
        &app,
        "POST",
        "/api/p/anticheat/v1/appeals:lookup",
        Some(json!({ "token": token })),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["case"]["appeal"]["allowed"], true);
    assert!(
        r.body["case"].get("score").is_none(),
        "user view must not expose the score"
    );
    let bad = call(
        &app,
        "POST",
        "/api/p/anticheat/v1/appeals:by-token",
        Some(json!({ "token": "nope", "body": "x" })),
        &[],
    )
    .await;
    assert_eq!(bad.body["code"], "anticheat.appeal_token_invalid");
    let r = call(
        &app,
        "POST",
        "/api/p/anticheat/v1/appeals:by-token",
        Some(json!({ "token": token, "body": "我用的是千兆盒子" })),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let aid = r.body["appeal_id"].as_i64().unwrap();
    // 原决定人与批准人都不能复核（四眼）
    for who in [&admin, &admin2] {
        let r = post(
            &app,
            who,
            &format!("/api/p/anticheat/v1/admin/appeals/{aid}/decisions"),
            json!({ "outcome": "overturned", "note": "x" }),
        )
        .await;
        assert_eq!(r.body["code"], "anticheat.four_eyes_required", "{:?}", r.body);
    }
    let r = post(
        &app,
        &admin3,
        &format!("/api/p/anticheat/v1/admin/appeals/{aid}/decisions"),
        json!({ "outcome": "overturned", "note": "核实为盒子，误判" }),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["case_state"], "overturned");
    wait_for("ban lifted", || async {
        sqlx::query_scalar::<_, String>("SELECT status FROM identity.users WHERE id = $1")
            .bind(i64::try_from(alice.id).unwrap())
            .fetch_one(&mut db().await)
            .await
            .unwrap()
            == "active"
    })
    .await;
    wait_for("freeze removed and wallet unlocked", || async {
        scalar("SELECT count(*) FROM ledger.account_locks WHERE user_id = $1 AND released_at IS NULL", alice.id).await == 0
            && scalar("SELECT count(*) FROM accounting.rate_rules WHERE external_ref = 'freeze:' || $1::text AND is_current AND removed_at IS NULL", alice.id).await == 0
    })
    .await;
    assert_eq!(
        scalar(
            "SELECT count(*) FROM anticheat.evidence WHERE user_id = $1 AND verdict = 'false_positive'",
            alice.id
        )
        .await,
        1
    );
    assert_eq!(
        scalar(
            "SELECT score::bigint FROM anticheat.user_risk WHERE user_id = $1",
            alice.id
        )
        .await,
        0
    );
    assert_eq!(
        scalar(
            "SELECT transfer_blocked::int::bigint FROM anticheat.user_risk WHERE user_id = $1",
            alice.id
        )
        .await,
        0
    );

    // ---- 规则集：校验、草案、未模拟不能激活、模拟、激活 ----
    let r = get(&app, &admin, "/api/p/anticheat/v1/admin/rulesets/active").await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let mut def = r.body["definition"].clone();
    let mut broken = def.clone();
    broken["thresholds"]["case"] = json!(5);
    let r = post(
        &app,
        &admin,
        "/api/p/anticheat/v1/admin/rulesets:validate",
        json!({ "definition": broken }),
    )
    .await;
    assert_eq!(r.body["valid"], false, "{:?}", r.body);
    assert!(
        r.body["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["path"] == "/thresholds/case")
    );
    def["thresholds"]["watch"] = json!(25);
    let r = post(
        &app,
        &admin,
        "/api/p/anticheat/v1/admin/rulesets",
        json!({ "definition": def, "based_on": 1, "note": "watch 25" }),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let v = r.body["version"].as_i64().unwrap();
    let r = post(
        &app,
        &admin,
        &format!("/api/p/anticheat/v1/admin/rulesets/{v}/activations"),
        json!({}),
    )
    .await;
    assert_eq!(r.body["code"], "anticheat.simulation_required", "{:?}", r.body);
    let r = post(
        &app,
        &admin,
        &format!("/api/p/anticheat/v1/admin/rulesets/{v}/simulations"),
        json!({ "window_days": 30 }),
    )
    .await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{:?}", r.body);
    let sim = r.body["id"].as_str().unwrap().to_owned();
    let mut done = Value::Null;
    for _ in 0..200 {
        let r = get(
            &app,
            &admin,
            &format!("/api/p/anticheat/v1/admin/simulations/{sim}"),
        )
        .await;
        if r.body["status"] == "done" {
            done = r.body;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(done["status"], "done", "simulation did not finish");
    assert!(done["result"]["users"].as_i64().unwrap() >= 2, "{done:?}");
    let r = post(
        &app,
        &admin,
        &format!("/api/p/anticheat/v1/admin/rulesets/{v}/activations"),
        json!({ "note": "ok" }),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["active_version"], json!(v));
    let r = get(&app, &admin, "/api/p/anticheat/v1/admin/dashboard").await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["open_cases"], 0);

    // ---- 用户“我的案件” ----
    let r = get(&app, &carol, "/api/p/anticheat/v1/users/me/cases").await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["items"].as_array().unwrap().len(), 0);
    drop(kernel);
}
