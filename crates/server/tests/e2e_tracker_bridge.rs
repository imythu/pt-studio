//! 端到端：tracker-bridge（Native）—— 启动全量同步（参数、客户端白名单）、注册后写入 passkey、发布后写入种子、
//! 换 passkey 删除旧键、封禁后状态变化、变更流。pt-tracker 只读这些键（见 crates/pt-tracker）。

#![allow(clippy::unwrap_used)]

mod common;

use std::collections::HashMap;

use pt_kernel::Kernel;
use serde_json::json;
use sqlx::{Connection, PgConnection};

use common::{call, env, hv, register_login, reset, upload};

async fn redis() -> redis::aio::MultiplexedConnection {
    redis::Client::open(env("PT_TEST_REDIS_URL", "redis://127.0.0.1:56379/15"))
        .unwrap()
        .get_multiplexed_async_connection()
        .await
        .unwrap()
}

async fn hash(c: &mut redis::aio::MultiplexedConnection, key: &str) -> HashMap<String, String> {
    redis::cmd("HGETALL").arg(key).query_async(c).await.unwrap()
}

/// 事件投递是异步的：等到条件成立
async fn wait_hash(
    c: &mut redis::aio::MultiplexedConnection,
    key: &str,
    ok: impl Fn(&HashMap<String, String>) -> bool,
) -> HashMap<String, String> {
    for _ in 0..100 {
        let h = hash(c, key).await;
        if ok(&h) {
            return h;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("{key} never satisfied the condition: {:?}", hash(c, key).await);
}

async fn passkey(app: &axum::Router, u: &common::User) -> String {
    let r = call(
        app,
        "GET",
        "/api/p/identity/v1/users/me/passkey",
        None,
        &hv(&u.headers()),
    )
    .await;
    r.body["passkey"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn tracker_bridge_end_to_end() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let kernel = Kernel::start(common::config(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let mut r = redis().await;

    // 启动全量同步：参数与客户端白名单（默认拒绝未知客户端）
    let cfg = hash(&mut r, "trk:cfg").await;
    assert_eq!(
        (
            cfg.get("interval").map(String::as_str),
            cfg.get("max_peers").map(String::as_str)
        ),
        (Some("1800"), Some("50"))
    );
    let clients: String = redis::cmd("GET")
        .arg("trk:clients")
        .query_async(&mut r)
        .await
        .unwrap();
    let clients: serde_json::Value = serde_json::from_str(&clients).unwrap();
    assert_eq!(clients["default"], "deny");
    assert!(
        clients["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["peer_id_prefix"] == "-qB")
    );

    // 注册 → passkey 键
    let alice = register_login(&app, "tb_alice").await;
    let admin = register_login(&app, "tb_admin").await;
    common::grant_role(admin.id, "admin").await;
    let pk = passkey(&app, &alice).await;
    let h = wait_hash(&mut r, &format!("trk:pk:{pk}"), |h| h.contains_key("uid")).await;
    assert_eq!(
        (h["uid"].clone(), h["st"].clone()),
        (alice.id.to_string(), "0".to_owned())
    );

    // 发布 → 种子键（v1 hash，计账）
    let tid = upload(&app, &alice, "Bridge.Test", &[("a.bin", 1 << 20)]).await;
    let mut db = PgConnection::connect(&env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap();
    let ih: Vec<u8> = sqlx::query_scalar("SELECT info_hash_v1 FROM torrent.torrents WHERE id = $1")
        .bind(tid)
        .fetch_one(&mut db)
        .await
        .unwrap();
    let h = wait_hash(&mut r, &format!("trk:t:{}", hex::encode(&ih)), |h| {
        h.contains_key("tid")
    })
    .await;
    assert_eq!(
        (h["tid"].clone(), h["st"].clone(), h["acct"].clone()),
        (tid.to_string(), "0".to_owned(), "1".to_owned())
    );
    assert_eq!(h["size"], (1 << 20).to_string());

    // 换 passkey：旧键删除、新键写入、变更流有 delete
    let res = call(
        &app,
        "POST",
        "/api/p/identity/v1/users/me/passkey-resets",
        Some(json!({})),
        &hv(&alice.headers()),
    )
    .await;
    assert!(res.status.is_success(), "{:?}", res.body);
    let pk2 = passkey(&app, &alice).await;
    assert_ne!(pk, pk2);
    wait_hash(&mut r, &format!("trk:pk:{pk2}"), |h| h.contains_key("uid")).await;
    let old: HashMap<String, String> = hash(&mut r, &format!("trk:pk:{pk}")).await;
    assert!(old.is_empty(), "old passkey must be removed: {old:?}");
    let entries: Vec<(String, Vec<(String, String)>)> = redis::cmd("XRANGE")
        .arg("trk:sync")
        .arg("-")
        .arg("+")
        .query_async(&mut r)
        .await
        .unwrap();
    assert!(
        entries
            .iter()
            .any(|(_, f)| f.contains(&("op".into(), "delete".into()))
                && f.contains(&("key".into(), pk.clone())))
    );

    // 封禁 → st = 2
    let res = call(
        &app,
        "POST",
        &format!("/api/p/identity/v1/admin/users/{}/restrictions", alice.id),
        Some(json!({ "kind": "ban", "reason_code": "cheating" })),
        &hv(&admin.headers()),
    )
    .await;
    assert!(res.status.is_success(), "{:?}", res.body);
    wait_hash(&mut r, &format!("trk:pk:{pk2}"), |h| {
        h.get("st").map(String::as_str) == Some("2")
    })
    .await;

    // 种子禁用 → st = 1
    let res = call(
        &app,
        "POST",
        &format!("/api/p/torrent/v1/admin/torrents/{tid}/status"),
        Some(json!({ "to": "disabled", "reason": "test" })),
        &hv(&admin.headers()),
    )
    .await;
    assert!(res.status.is_success(), "{:?}", res.body);
    wait_hash(&mut r, &format!("trk:t:{}", hex::encode(&ih)), |h| {
        h.get("st").map(String::as_str) == Some("1")
    })
    .await;
    kernel.shutdown().await;
}
