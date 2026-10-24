//! 端到端：tracker-bridge（Native）—— 启动全量同步（参数、客户端规则从旧版白名单迁移）、注册后写入 passkey、发布后写入种子、
//! 换 passkey 删除旧键、封禁后状态变化、变更流、节点状态与健康检查、客户端规则的读取 / 替换 / 测试 / 预设、被拒客户端排行。
//! pt-tracker 只读这些键（见 crates/pt-tracker）。

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

    // 启动全量同步：参数与客户端规则（首次启动从旧版默认白名单迁移，默认拒绝未知客户端）
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
            .any(|x| x["pid_prefix"] == "-qB" && x["action"] == "allow")
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

    // 节点状态：读 pt-tracker 写出的 trk:stats:node:*；peer 超时小于 announce 间隔时告警
    let _: () = redis::cmd("HSET")
        .arg("trk:stats:node:e2e")
        .arg(&[
            ("ver", "0.1.0"),
            ("boot", "1700000000"),
            ("sid", "1-0"),
            ("lag_ms", "3"),
            ("pk", "2"),
            ("t", "1"),
            ("mpt", "1500"),
            ("rps", "0.5"),
            ("ts", "1700000010"),
        ])
        .query_async(&mut r)
        .await
        .unwrap();
    let bob = register_login(&app, "tb_bob").await;
    let res = call(
        &app,
        "GET",
        "/api/p/tracker-bridge/v1/admin/status",
        None,
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(res.status, 403, "{:?}", res.body);
    let res = call(
        &app,
        "GET",
        "/api/p/tracker-bridge/v1/admin/status",
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert!(res.status.is_success(), "{:?}", res.body);
    let n = &res.body["nodes"][0];
    assert_eq!(
        (
            n["node_id"].as_str(),
            n["passkeys"].as_u64(),
            n["max_peer_timeout"].as_u64()
        ),
        (Some("e2e"), Some(2), Some(1500))
    );
    // 节点停在 1-0，流里已有更新的条目
    assert!(n["behind_ms"].is_u64() && res.body["stream"]["length"].as_u64() > Some(0));
    assert!(
        res.body["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["code"] == "interval_exceeds_peer_timeout" && w["node_id"] == "e2e"),
        "{:?}",
        res.body
    );

    // 节点健康：落后告警发事件；曾经出现、超过 150 秒未上报的节点为失联
    let mut db2 = PgConnection::connect(&env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO tracker_bridge._kv (key, value)
         VALUES ('node_health', convert_to(json_build_object('nodes', json_build_object('gone', json_build_object(
                 'last_seen', (extract(epoch FROM now()) * 1000)::bigint - 200000)))::text, 'UTF8'))
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .execute(&mut db2)
    .await
    .unwrap();
    // 节点停在 1-0：流的最新条目写入超过 5 秒后算作落后
    tokio::time::sleep(std::time::Duration::from_millis(5500)).await;
    kernel
        .run_job_now("tracker-bridge", "node_health", json!({}))
        .await
        .unwrap();
    let kinds: Vec<String> = sqlx::query_scalar(
        "SELECT payload->>'kind' || ':' || (payload->>'node_id') FROM (SELECT event_type, envelope->'payload' AS payload FROM kernel.event_outbox
          UNION ALL SELECT event_type, payload FROM kernel.event_log) e WHERE event_type = 'tracker.inconsistency_detected' ORDER BY 1",
    )
    .fetch_all(&mut db2)
    .await
    .unwrap();
    assert!(
        kinds.contains(&"node_lag:e2e".to_owned()) && kinds.contains(&"node_lost:gone".to_owned()),
        "{kinds:?}"
    );
    // 告警只在进入时发一次
    kernel
        .run_job_now("tracker-bridge", "node_health", json!({}))
        .await
        .unwrap();
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (SELECT event_type FROM kernel.event_outbox UNION ALL SELECT event_type FROM kernel.event_log) e
          WHERE event_type = 'tracker.inconsistency_detected'",
    )
    .fetch_one(&mut db2)
    .await
    .unwrap();
    assert_eq!(usize::try_from(n).unwrap(), kinds.len());
    let res = call(
        &app,
        "GET",
        "/api/p/tracker-bridge/v1/admin/status",
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert!(
        res.body["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w["code"] == "node_lost" && w["node_id"] == "gone"),
        "{:?}",
        res.body
    );

    // 客户端规则：moderator 以下无权；读取带 ETag
    let base = "/api/p/tracker-bridge/v1/admin/client-rules";
    let res = call(&app, "GET", base, None, &hv(&bob.headers())).await;
    assert_eq!(res.status, 403, "{:?}", res.body);
    let res = call(&app, "GET", base, None, &hv(&admin.headers())).await;
    assert!(res.status.is_success(), "{:?}", res.body);
    assert_eq!(
        (res.body["version"].as_i64(), res.body["synced"].as_bool()),
        (Some(1), Some(true))
    );
    assert_eq!(
        res.headers.get("etag").and_then(|v| v.to_str().ok()),
        Some("\"1\"")
    );
    let qb = res.body["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["pid_prefix"] == "-qB")
        .unwrap()
        .clone();
    let ah = admin.headers();
    let with_match = |v: &'static str| {
        let mut h = hv(&ah);
        h.push(("if-match", v));
        h
    };
    // 缺 If-Match、规则无效（错误逐条定位）
    let presets = call(
        &app,
        "GET",
        "/api/p/tracker-bridge/v1/admin/client-rule-presets",
        None,
        &hv(&admin.headers()),
    )
    .await;
    let presets = presets.body["items"].as_array().unwrap().clone();
    assert!(presets.len() >= 5 && presets.iter().any(|p| p["min_ver"] == "4.3"));
    let res = call(
        &app,
        "PUT",
        base,
        Some(json!({ "default_action": "deny", "rules": presets })),
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(res.status, 400, "{:?}", res.body);
    let res = call(
        &app,
        "PUT",
        base,
        Some(json!({ "default_action": "deny", "rules": [
            { "name": "bad", "action": "allow", "pid_regex": "^-qB(" },
            { "name": "ver", "action": "allow", "pid_prefix": "-qB", "min_ver": "4.3" }
        ] })),
        &with_match("\"1\""),
    )
    .await;
    assert_eq!(res.status, 422, "{:?}", res.body);
    assert_eq!(res.body["code"], "tracker-bridge.rules_invalid");
    let paths: Vec<&str> = res.body["errors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["path"].as_str())
        .collect();
    assert_eq!(paths, ["/rules/0/pid_regex", "/rules/1/min_ver"]);
    // 替换为预设 + 保留原 qBittorrent 规则（ID 不变）并停用
    let mut rules = presets.clone();
    let mut kept = qb.clone();
    kept["enabled"] = json!(false);
    rules.push(kept);
    let res = call(
        &app,
        "PUT",
        base,
        Some(json!({ "default_action": "deny", "rules": rules })),
        &with_match("\"1\""),
    )
    .await;
    assert!(res.status.is_success(), "{:?}", res.body);
    assert_eq!(res.body["version"], 2);
    assert_eq!(
        res.headers.get("etag").and_then(|v| v.to_str().ok()),
        Some("\"2\"")
    );
    let saved = res.body["rules"].as_array().unwrap();
    assert_eq!(saved.len(), presets.len() + 1);
    assert_eq!(
        (
            saved.last().unwrap()["id"].clone(),
            saved.last().unwrap()["enabled"].clone()
        ),
        (qb["id"].clone(), json!(false))
    );
    // trk:clients 只导出启用的规则，并追加变更流
    let clients: String = redis::cmd("GET")
        .arg("trk:clients")
        .query_async(&mut r)
        .await
        .unwrap();
    let clients: serde_json::Value = serde_json::from_str(&clients).unwrap();
    assert_eq!(clients["rules"].as_array().unwrap().len(), presets.len());
    assert!(
        clients["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["pid_regex"] == "^-qB(?P<ver>[0-9A-Z]{4})-" && x["min_ver"] == "4.3")
    );
    assert!(clients["ver"].as_i64() > Some(0));
    // 并发修改：过期的 If-Match
    let res = call(
        &app,
        "PUT",
        base,
        Some(json!({ "default_action": "allow", "rules": [] })),
        &with_match("\"1\""),
    )
    .await;
    assert_eq!(res.status, 412, "{:?}", res.body);
    let audits: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM kernel.audit_log WHERE action = 'tracker-bridge.client_rules_set'",
    )
    .fetch_one(&mut db2)
    .await
    .unwrap();
    assert_eq!(audits, 1);
    let changed: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT payload FROM (SELECT event_type, envelope->'payload' AS payload FROM kernel.event_outbox UNION ALL SELECT event_type, payload FROM kernel.event_log) e
          WHERE event_type = 'tracker.client_rules_changed'",
    )
    .fetch_all(&mut db2)
    .await
    .unwrap();
    assert_eq!(
        changed,
        [json!({ "version": 2, "rule_count": presets.len() + 1 })]
    );

    // 规则测试：当前规则集、草稿、hex 形式的 peer_id、无效 peer_id
    let test = "/api/p/tracker-bridge/v1/admin/client-rules:test";
    let res = call(
        &app,
        "POST",
        test,
        Some(json!({ "peer_id": "-qB4630-a1b2c3d4e5f6", "user_agent": "qBittorrent/4.6.3" })),
        &hv(&admin.headers()),
    )
    .await;
    assert!(res.status.is_success(), "{:?}", res.body);
    assert_eq!(
        (res.body["action"].as_str(), res.body["rule_name"].as_str()),
        (Some("allow"), Some("qBittorrent ≥ 4.3"))
    );
    assert_eq!(res.body["extracted_ver"], json!([4, 6, 3, 0]));
    let res = call(
        &app,
        "POST",
        test,
        Some(json!({ "peer_id": hex::encode("-qB4600-a1b2c3d4e5f6") })),
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(
        (res.body["action"].as_str(), res.body["rule_index"].as_u64()),
        (Some("deny"), Some(0))
    );
    let res = call(
        &app,
        "POST",
        test,
        Some(json!({ "peer_id": "-ZZ0001-", "rules": { "default_action": "allow", "rules": [] } })),
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(
        (res.body["action"].as_str(), res.body["rule_index"].is_null()),
        (Some("allow"), true)
    );
    let res = call(
        &app,
        "POST",
        test,
        Some(json!({ "peer_id": "%G0" })),
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(
        (res.status.as_u16(), res.body["code"].as_str()),
        (422, Some("tracker-bridge.invalid_peer_id"))
    );

    // 被拒客户端排行（pt-tracker 写 trk:stats:denied:<UTC 日期>）
    let today = chrono::Utc::now().format("%Y%m%d").to_string();
    let _: () = redis::pipe()
        .cmd("ZINCRBY")
        .arg(format!("trk:stats:denied:{today}"))
        .arg(7)
        .arg("-XL0012-|Xunlei 0.0.1.2")
        .cmd("ZINCRBY")
        .arg(format!("trk:stats:denied:{today}"))
        .arg(2)
        .arg("-SD0100-|")
        .query_async(&mut r)
        .await
        .unwrap();
    let res = call(
        &app,
        "GET",
        "/api/p/tracker-bridge/v1/admin/denied-clients",
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert!(res.status.is_success(), "{:?}", res.body);
    assert_eq!(res.body["total"], 9);
    assert_eq!(
        (
            res.body["items"][0]["peer_id_prefix"].as_str(),
            res.body["items"][0]["user_agent"].as_str(),
            res.body["items"][0]["count"].as_u64()
        ),
        (Some("-XL0012-"), Some("Xunlei 0.0.1.2"), Some(7))
    );
    let res = call(
        &app,
        "GET",
        "/api/p/tracker-bridge/v1/admin/denied-clients?date=2020-01-01",
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(
        (
            res.body["total"].as_u64(),
            res.body["items"].as_array().map(Vec::len)
        ),
        (Some(0), Some(0))
    );
    let res = call(
        &app,
        "GET",
        "/api/p/tracker-bridge/v1/admin/denied-clients?date=x",
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(res.status, 422);
    kernel.shutdown().await;
}
