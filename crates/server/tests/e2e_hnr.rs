//! 端到端：hnr（WASM）—— tracker 流 → accounting 首次完成 → 建档（含豁免）→ 评估（计入 / 达标）→ identity 限制与下载拦截、
//! 邀请拦截 → 免除后解除限制。

#![allow(clippy::unwrap_used)]

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{User, call, env, hv, register_login, reset, upload};

const MIB: u64 = 1 << 20;

fn config(dir: &std::path::Path) -> SiteConfig {
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n",
        dir.display()
    ));
    SiteConfig::parse(&t).unwrap()
}

/// 一条带首次完成标记的 tracker 增量事件
async fn announce(r: &mut redis::aio::MultiplexedConnection, uid: u64, tid: i64, eid: &str, down: u64) {
    let _: String = redis::cmd("XADD")
        .arg(format!("acct:{}", uid % 2))
        .arg("*")
        .arg(&[
            ("v", "1".to_owned()),
            ("eid", eid.to_owned()),
            ("ts", chrono::Utc::now().timestamp().to_string()),
            ("uid", uid.to_string()),
            ("tid", tid.to_string()),
            ("ih", "cd".repeat(20)),
            ("pid", "2d71423436333030".to_owned() + &"44".repeat(12)),
            ("ip", "203.0.113.7".into()),
            ("port", "1".into()),
            ("ev", "completed".into()),
            ("up", "0".into()),
            ("down", down.to_string()),
            ("left", "0".into()),
            ("dup", "0".into()),
            ("ddown", down.to_string()),
            ("dseed", "30".into()),
            ("dleech", "0".into()),
            ("flags", (1 | (1 << 4)).to_string()),
            ("ua", "x".into()),
        ])
        .query_async(r)
        .await
        .unwrap();
}

async fn wait_for<F, Fut>(what: &str, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..200 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn me(app: &axum::Router, u: &User) -> Value {
    call(app, "GET", "/api/p/hnr/v1/me", None, &hv(&u.headers()))
        .await
        .body
}

#[tokio::test]
async fn hnr_end_to_end() {
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
    let mut db = PgConnection::connect(&env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap();
    sqlx::query("INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('hnr', $1), ('invite', $2)")
        .bind(json!({ "window_days": 1, "min_seed_hours": 1, "min_torrent_ratio": 0, "max_hnr": 1, "invite_block_at": 1 }))
        .bind(json!({ "min_account_days": 0, "initial_quota": { "amount": 1 } }))
        .execute(&mut db)
        .await
        .unwrap();

    let admin = register_login(&app, "hnr_admin").await;
    common::grant_role(admin.id, "admin").await;
    let alice = register_login(&app, "hnr_alice").await;
    let bob = register_login(&app, "hnr_bob").await;
    let big = upload(&app, &alice, "Big.Movie.2023.1080p", &[("a.mkv", 600 * MIB)]).await;
    let small = upload(&app, &alice, "Small.Clip", &[("b.mkv", 10 * MIB)]).await;
    let big2 = upload(&app, &alice, "Big.Show.S01", &[("c.mkv", 700 * MIB)]).await;
    // 等 accounting 镜像种子后再推 tracker 事件
    wait_for("torrent mirror", || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM accounting.torrents")
            .fetch_one(
                &mut PgConnection::connect(&env(
                    "PT_TEST_SUPERUSER_URL",
                    "postgres://postgres@127.0.0.1:55432/pt_test",
                ))
                .await
                .unwrap(),
            )
            .await
            .unwrap_or(0)
            == 3
    })
    .await;

    announce(&mut rc, bob.id, big, "h:1", 600 * MIB).await;
    announce(&mut rc, bob.id, small, "h:2", 10 * MIB).await;
    announce(&mut rc, bob.id, big2, "h:3", 700 * MIB).await;
    announce(&mut rc, alice.id, big, "h:4", 600 * MIB).await;
    wait_for("records", || async {
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM hnr.records")
            .fetch_one(
                &mut PgConnection::connect(&env(
                    "PT_TEST_SUPERUSER_URL",
                    "postgres://postgres@127.0.0.1:55432/pt_test",
                ))
                .await
                .unwrap(),
            )
            .await
            .unwrap_or(0)
            == 4
    })
    .await;
    let rows: Vec<(i64, i64, String, Option<String>)> = sqlx::query_as(
        "SELECT user_id, torrent_id, state, exempt_rule FROM hnr.records ORDER BY user_id, torrent_id",
    )
    .fetch_all(&mut db)
    .await
    .unwrap();
    let find = |u: u64, t: i64| {
        rows.iter()
            .find(|r| r.0 == u as i64 && r.1 == t)
            .cloned()
            .unwrap()
    };
    assert_eq!(find(bob.id, big).2, "active");
    assert_eq!(find(bob.id, small).3.as_deref(), Some("small"));
    assert_eq!(find(alice.id, big).3.as_deref(), Some("own_upload"));
    assert_eq!(me(&app, &bob).await["active"], 2);
    let r = call(
        &app,
        "GET",
        &format!("/api/p/hnr/v1/torrents/{small}/requirement"),
        None,
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(
        (r.body["required"].as_bool(), r.body["exempt_rule"].as_str()),
        (Some(false), Some("small"))
    );

    // big2 做种满 1 小时 → 达标；big 过期未达标 → 计入
    // 做种时长只由 accounting 小时快照折叠写入（见 e2e_accounting）；这里直接写入快照结果
    sqlx::query("UPDATE accounting.snatches SET seed_secs = 4000 WHERE user_id = $1 AND torrent_id = $2")
        .bind(bob.id as i64)
        .bind(big2)
        .execute(&mut db)
        .await
        .unwrap();
    sqlx::query("UPDATE hnr.records SET deadline = now() - interval '1 minute' WHERE torrent_id = $1")
        .bind(big)
        .execute(&mut db)
        .await
        .unwrap();
    kernel
        .run_job_now("hnr", "evaluate_hot", json!({}))
        .await
        .unwrap();
    let st: Vec<(i64, String)> =
        sqlx::query_as("SELECT torrent_id, state FROM hnr.records WHERE user_id = $1 ORDER BY torrent_id")
            .bind(bob.id as i64)
            .fetch_all(&mut db)
            .await
            .unwrap();
    assert!(st.contains(&(big, "failed".into())), "{st:?}");
    assert!(st.contains(&(big2, "satisfied".into())), "{st:?}");
    let m = me(&app, &bob).await;
    assert_eq!(
        (
            m["active"].as_i64(),
            m["failed"].as_i64(),
            m["can_download"].as_bool()
        ),
        (Some(0), Some(1), Some(false)),
        "{m}"
    );

    // 超限：identity 限制下载，下载被拒；邀请被 hnr 拦截
    let status: String = sqlx::query_scalar("SELECT status FROM identity.users WHERE id = $1")
        .bind(bob.id as i64)
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(status, "leech_disabled");
    let r = call(
        &app,
        "GET",
        &format!("/api/p/torrent/v1/torrents/{big2}/download"),
        None,
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.body["code"], "identity.download_disabled", "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        "/api/p/invite/v1/users/me/invites",
        Some(json!({})),
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(r.body["code"], "invite.account_inactive", "{:?}", r.body);

    // 免除：普通用户无权；管理员免除后解除限制
    let rid: i64 = sqlx::query_scalar("SELECT id FROM hnr.records WHERE user_id = $1 AND torrent_id = $2")
        .bind(bob.id as i64)
        .bind(big)
        .fetch_one(&mut db)
        .await
        .unwrap();
    let path = format!("/api/p/hnr/v1/records/{rid}/forgive");
    let r = call(
        &app,
        "POST",
        &path,
        Some(json!({ "reason": "x" })),
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = call(
        &app,
        "POST",
        &path,
        Some(json!({ "reason": "seeded elsewhere" })),
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    let r = call(
        &app,
        "POST",
        &path,
        Some(json!({ "reason": "again" })),
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.body["code"], "hnr.not_forgivable");
    let status: String = sqlx::query_scalar("SELECT status FROM identity.users WHERE id = $1")
        .bind(bob.id as i64)
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(status, "active");
    assert_eq!(me(&app, &bob).await["can_download"], true);
    let r = call(
        &app,
        "GET",
        &format!("/api/p/torrent/v1/torrents/{big2}/download"),
        None,
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    // 有计入但未达限制线的用户：hnr 拦截发邀（invite_block_at = 1）
    sqlx::query("INSERT INTO hnr.user_counts (user_id, failed_effective) VALUES ($1, 1)")
        .bind(alice.id as i64)
        .execute(&mut db)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE kernel.plugin_settings SET settings = settings || '{\"max_hnr\": 5}' WHERE plugin_id = 'hnr'",
    )
    .execute(&mut db)
    .await
    .unwrap();
    let r = call(
        &app,
        "POST",
        "/api/p/invite/v1/users/me/invites",
        Some(json!({})),
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(r.body["code"], "hnr.restricted", "{:?}", r.body);

    // 记录列表与他人查看权限
    let r = call(
        &app,
        "GET",
        "/api/p/hnr/v1/me/records?state=forgiven",
        None,
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(r.body["items"][0]["id"], rid);
    let r = call(
        &app,
        "GET",
        &format!("/api/p/hnr/v1/users/{}/records", bob.id),
        None,
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = call(
        &app,
        "GET",
        &format!("/api/p/hnr/v1/users/{}/records", bob.id),
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.body["items"].as_array().unwrap().len(), 3);

    kernel.shutdown().await;
}
