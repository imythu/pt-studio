//! 端到端：盒子登记（seedbox，WASM）+ accounting 标记 —— 网段校验、审核、来自已批准网段的 peer 记为盒子、
//! `v_swarm_peers.is_seedbox`、删除登记后网段立即删除。
//!
//! 运行：`cargo test -p pt-server --test e2e_seedbox -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{User, call, env, hv, register_login, reset};

async fn db() -> PgConnection {
    PgConnection::connect(&env("PT_TEST_SUPERUSER_URL", "postgres://postgres@127.0.0.1:55432/pt_test"))
        .await
        .unwrap()
}

async fn redis() -> redis::aio::MultiplexedConnection {
    redis::Client::open(env("PT_TEST_REDIS_URL", "redis://127.0.0.1:56379/15"))
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap()
}

/// 模拟 pt-tracker 写入一条记账流（`ip` 为 announce 的来源地址）
async fn announce(uid: u64, tid: u64, peer: u8, ip: &str, eid: &str) {
    let _: String = redis::cmd("XADD")
        .arg(format!("acct:{}", uid % 2))
        .arg("*")
        .arg(&[
            ("v", "1".to_owned()),
            ("eid", eid.to_owned()),
            ("ts", chrono::Utc::now().timestamp().to_string()),
            ("uid", uid.to_string()),
            ("tid", tid.to_string()),
            ("ih", "ab".repeat(20)),
            ("pid", format!("2d7142343633302d{}", format!("{peer:02x}").repeat(12))),
            ("ip", ip.into()),
            ("port", "51413".into()),
            ("ev", "started".into()),
            ("up", "0".into()),
            ("down", "0".into()),
            ("left", "0".into()),
            ("dup", "0".into()),
            ("ddown", "0".into()),
            ("dseed", "30".into()),
            ("dleech", "0".into()),
            ("flags", "0".into()),
            ("ua", "qBittorrent/5.1.0".into()),
        ])
        .query_async(&mut redis().await)
        .await
        .unwrap();
}

/// (peer 字节, is_seedbox)，等到出现 n 个 peer
async fn peers(uid: u64, n: usize) -> Vec<(i32, bool)> {
    for _ in 0..100 {
        let rows: Vec<(Vec<u8>, bool)> = sqlx::query_as(
            "SELECT peer_id, is_seedbox FROM accounting.peers WHERE user_id = $1 ORDER BY peer_id",
        )
        .bind(i64::try_from(uid).unwrap())
        .fetch_all(&mut db().await)
        .await
        .unwrap();
        if rows.len() >= n {
            return rows.into_iter().map(|(p, s)| (i32::from(p[19]), s)).collect();
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    panic!("peers did not arrive");
}

async fn post(app: &axum::Router, u: &User, path: &str, body: Value) -> common::Resp {
    call(app, "POST", path, Some(body), &hv(&u.headers())).await
}

#[tokio::test]
async fn seedbox_registry_and_peer_marking() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let _: () = redis::cmd("HSET").arg("acct:_meta").arg("partitions").arg(2).query_async(&mut redis().await).await.unwrap();
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n\n[redis.acct]\nurl = \"{}\"\n\n[plugin.accounting]\nwindow_secs = 2\nsettle_grace_secs = 0\ndefault_partitions = 2\nidle_sleep_ms = 50\n",
        dir.display(),
        env("PT_TEST_REDIS_URL", "redis://127.0.0.1:56379/15")
    ));
    let kernel = Kernel::start(SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let alice = register_login(&app, "box_owner").await;
    let staff = register_login(&app, "box_mod").await;
    common::grant_role(staff.id, "moderator").await;

    // 网段校验：私网、过宽被拒；合法网段待审核
    let r = post(&app, &alice, "/api/p/seedbox/v1/boxes", json!({ "ranges": ["10.0.0.0/24"] })).await;
    assert_eq!(r.body["code"], "seedbox.range_invalid", "{:?}", r.body);
    let r = post(&app, &alice, "/api/p/seedbox/v1/boxes", json!({ "ranges": ["198.51.0.0/16"] })).await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    let r = post(&app, &alice, "/api/p/seedbox/v1/boxes", json!({ "ranges": ["198.51.100.77/24", "2001:db8:7::/48"], "provider": "Hetzner" })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!(r.body["state"], "pending");
    assert_eq!(r.body["ranges"], json!(["198.51.100.0/24", "2001:db8:7::/48"]));
    let box_id = r.body["id"].as_i64().unwrap();

    // 审核前：不标记
    announce(alice.id, 5, 1, "198.51.100.20", "s:1:1").await;
    assert_eq!(peers(alice.id, 1).await, vec![(1, false)]);

    // 普通用户不能审核；版主批准
    let r = post(&app, &alice, &format!("/api/p/seedbox/v1/admin/boxes/{box_id}/review"), json!({ "approve": true })).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let list = call(&app, "GET", "/api/p/seedbox/v1/admin/boxes?state=pending", None, &hv(&staff.headers())).await;
    assert_eq!(list.body["items"][0]["username"], "box_owner", "{:?}", list.body);
    let r = post(&app, &staff, &format!("/api/p/seedbox/v1/admin/boxes/{box_id}/review"), json!({ "approve": true, "reason": "ok" })).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    let r = post(&app, &staff, &format!("/api/p/seedbox/v1/admin/boxes/{box_id}/review"), json!({ "approve": true })).await;
    assert_eq!(r.body["code"], "seedbox.not_pending");

    // 批准后（accounting 每分钟重载网段，测试中等待重载）：盒子网段内的 peer 标记，家宽不标记
    tokio::time::sleep(std::time::Duration::from_secs(61)).await;
    announce(alice.id, 6, 2, "198.51.100.99", "s:1:2").await;
    announce(alice.id, 6, 3, "203.0.113.5", "s:1:3").await;
    announce(alice.id, 7, 4, "2001:db8:7:1::9", "s:1:4").await;
    let got = peers(alice.id, 4).await;
    assert_eq!(got, vec![(1, false), (2, true), (3, false), (4, true)], "{got:?}");
    let swarm: Vec<(i64, bool)> = sqlx::query_as("SELECT torrent_id, is_seedbox FROM accounting.v_swarm_peers WHERE user_id = $1 ORDER BY torrent_id, is_seedbox")
        .bind(i64::try_from(alice.id).unwrap())
        .fetch_all(&mut db().await)
        .await
        .unwrap();
    assert!(swarm.contains(&(6, true)) && swarm.contains(&(7, true)), "{swarm:?}");
    let r = call(&app, "GET", "/api/p/seedbox/v1/boxes", None, &hv(&alice.headers())).await;
    assert_eq!(r.body["items"][0]["state"], "approved", "{:?}", r.body);

    // 删除登记：网段立即删除
    let r = call(&app, "DELETE", &format!("/api/p/seedbox/v1/boxes/{box_id}"), None, &hv(&alice.headers())).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM seedbox.box_ranges")
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert_eq!(left, 0);
}
