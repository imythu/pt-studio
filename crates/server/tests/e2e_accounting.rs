//! 端到端：accounting —— 流量流 → 分区 worker → 倍率 → 结算窗口与事件；eid 去重、拒收、首次完成、重启后恰好一次、调整。

#![allow(clippy::unwrap_used)]

mod common;

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use axum::{Json, Router, response::IntoResponse, routing::post};
use pt_kernel::{Kernel, config::SiteConfig};
use pt_plugin_accounting::{
    ACCOUNTING, AccountingService,
    rates::{RateMode, RateRule, RateScope},
};
use pt_plugin_api::{
    Ctx, DeliveryInfo, EventEnvelope, HttpRoutes, NativePlugin, OnEvent, PResult, PluginError, RequestCtx,
    json,
};
use serde_json::Value;
use sqlx::{Connection, PgConnection};

use common::{call, env, reset};

const PROMO_MANIFEST: &str = r#"
id = "x-promo"
name = "promo"
version = "0.1.0"
kind = "native"
api = "pt:plugin@0.1"
license = "MIT OR Apache-2.0"
requires = ["ctx.accounting@^0.1"]

[capabilities]
sql = true

[capabilities.scopes]
"ctx.accounting" = ["write:rates", "read", "write:adjust"]

[[subscriptions]]
name = "settled"
event = "traffic.settled"
start = "earliest"

[[subscriptions]]
name = "completed"
event = "traffic.snatch_completed"
start = "earliest"

[[http.routes]]
id = "op"
method = "POST"
path = "/v1/op"
auth = "public"

[database]
migrations = "migrations"
"#;

const PROMO_MIGRATIONS: &[(&str, &str)] = &[(
    "0001_init.sql",
    "CREATE TABLE seen (event_id uuid PRIMARY KEY, event_type text NOT NULL, payload jsonb NOT NULL);",
)];

struct Promo;

#[async_trait]
impl OnEvent for Promo {
    async fn handle(&self, ctx: &Ctx, evt: EventEnvelope, _d: DeliveryInfo) -> PResult<()> {
        let mut c = ctx.sql.conn().await?;
        sqlx::query("INSERT INTO seen (event_id, event_type, payload) VALUES ($1, $2, $3)")
            .bind(evt.id)
            .bind(&evt.event_type)
            .bind(&evt.payload)
            .execute(&mut *c)
            .await?;
        Ok(())
    }
}

async fn op(ctx: RequestCtx, Json(b): Json<Value>) -> PResult<axum::response::Response> {
    let acc = ctx.service::<dyn AccountingService>(ACCOUNTING)?;
    let out = match b["op"].as_str().unwrap_or_default() {
        "free2x" => {
            let tid = b["torrent_id"].as_u64().unwrap_or(0);
            let rule = RateRule {
                external_ref: format!("promo:{tid}"),
                scope: RateScope::Torrents(vec![tid]),
                up_permille: Some(2000),
                down_permille: Some(0),
                mode: RateMode::Favorable,
                priority: 0,
                starts_at: 0,
                ends_at: None,
                reason_term: Some("x-promo.free2x".into()),
            };
            json!({ "rule_id": acc.call(move |s, c| s.set_rate_rule(c, rule)).await? })
        }
        "rate" => {
            let (u, t) = (
                b["user"].as_u64().unwrap_or(0),
                b["torrent_id"].as_u64().unwrap_or(0),
            );
            serde_json::to_value(acc.call(move |s, c| s.effective_rate(c, u, t, None)).await?)?
        }
        "totals" => {
            let u = b["user"].as_u64().unwrap_or(0);
            serde_json::to_value(acc.call(move |s, c| s.user_totals(c, u)).await?)?
        }
        "adjust" => {
            let (u, d) = (b["user"].as_u64().unwrap_or(0), b["d_up"].as_i64().unwrap_or(0));
            let key = b["key"].as_str().unwrap_or_default().to_owned();
            serde_json::to_value(
                acc.call(move |s, c| {
                    Box::pin(async move { s.adjust(c, u, d, 0, "x-promo.fix", &key).await })
                })
                .await?,
            )?
        }
        "seen" => {
            let mut c = ctx.sql.conn().await?;
            let rows: Vec<(String, Value)> =
                sqlx::query_as("SELECT event_type, payload FROM seen ORDER BY event_type, event_id")
                    .fetch_all(&mut *c)
                    .await?;
            json!(rows)
        }
        _ => return Err(PluginError::bad_request("unknown op")),
    };
    Ok(Json(out).into_response())
}

struct PromoRoutes;
impl HttpRoutes for PromoRoutes {
    fn router(&self) -> Router {
        Router::new().route("/v1/op", post(op))
    }
}

fn promo() -> NativePlugin {
    let p = Arc::new(Promo);
    NativePlugin {
        manifest: PROMO_MANIFEST,
        migrations: PROMO_MIGRATIONS,
        on_event: Some(p),
        http: Some(Arc::new(PromoRoutes)),
        ..Default::default()
    }
}

fn config() -> SiteConfig {
    let mut text = common::config_text();
    text.push_str(&format!(
        "\n[redis.acct]\nurl = \"{}\"\n\n[plugin.accounting]\nwindow_secs = 2\nsettle_grace_secs = 0\ndefault_partitions = 2\nidle_sleep_ms = 50\n",
        env("PT_TEST_REDIS_URL", "redis://127.0.0.1:56379/15")
    ));
    SiteConfig::parse(&text).unwrap()
}

async fn redis() -> redis::aio::MultiplexedConnection {
    let client = redis::Client::open(env("PT_TEST_REDIS_URL", "redis://127.0.0.1:56379/15")).unwrap();
    client.get_multiplexed_async_connection().await.unwrap()
}

#[allow(clippy::too_many_arguments)]
async fn xadd(
    r: &mut redis::aio::MultiplexedConnection,
    uid: u64,
    tid: u64,
    eid: &str,
    ev: &str,
    dup: u64,
    ddown: u64,
    left: u64,
    flags: u32,
) {
    let part = uid % 2;
    let ts = chrono::Utc::now().timestamp();
    let _: String = redis::cmd("XADD")
        .arg(format!("acct:{part}"))
        .arg("*")
        .arg(&[
            ("v", "1".to_owned()),
            ("eid", eid.to_owned()),
            ("ts", ts.to_string()),
            ("uid", uid.to_string()),
            ("tid", tid.to_string()),
            ("ih", "ab".repeat(20)),
            ("pid", format!("2d7142343633302d{}", "11".repeat(12))),
            ("ip", "203.0.113.9".into()),
            ("port", "51413".into()),
            ("ev", ev.into()),
            ("up", dup.to_string()),
            ("down", ddown.to_string()),
            ("left", left.to_string()),
            ("dup", dup.to_string()),
            ("ddown", ddown.to_string()),
            ("dseed", "30".into()),
            ("dleech", "10".into()),
            ("flags", flags.to_string()),
            ("ua", "qBittorrent/4.6.3".into()),
        ])
        .query_async(r)
        .await
        .unwrap();
}

async fn wait_for<F: Fn(&Value) -> bool>(app: &Router, body: Value, f: F) -> Value {
    for _ in 0..150 {
        let r = call(app, "POST", "/api/p/x-promo/v1/op", Some(body.clone()), &[]).await;
        if f(&r.body) {
            return r.body;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("condition not met for {body}")
}

#[tokio::test]
async fn accounting_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter("info,sqlx=warn")
        .try_init();
    reset().await;
    let mut r = redis().await;
    let _: () = redis::cmd("HSET")
        .arg("acct:_meta")
        .arg("partitions")
        .arg(2)
        .query_async(&mut r)
        .await
        .unwrap();

    let mut plugins = pt_server::native_plugins();
    plugins.push(promo());
    let kernel = Kernel::start(config(), plugins)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();

    // 倍率：种子 5 免费 2x
    let resp = call(
        &app,
        "POST",
        "/api/p/x-promo/v1/op",
        Some(json!({"op": "free2x", "torrent_id": 5})),
        &[],
    )
    .await;
    assert!(resp.body["rule_id"].as_u64().is_some(), "{:?}", resp.body);
    let again = call(
        &app,
        "POST",
        "/api/p/x-promo/v1/op",
        Some(json!({"op": "free2x", "torrent_id": 5})),
        &[],
    )
    .await;
    assert_eq!(
        again.body["rule_id"], resp.body["rule_id"],
        "set_rate_rule must be idempotent"
    );
    wait_for(&app, json!({"op": "rate", "user": 7, "torrent_id": 5}), |b| {
        b["up_permille"] == 2000 && b["down_permille"] == 0
    })
    .await;

    // 用户 7（分区 1）：种子 5 上传 1000、下载 500（倍率后 2000 / 0）；种子 6 上传 300、下载 400
    xadd(&mut r, 7, 5, "n1:1:1", "started", 1000, 500, 100, 1).await;
    xadd(&mut r, 7, 5, "n1:1:1", "started", 1000, 500, 100, 1).await; // XADD 重试产生的重复条目
    xadd(&mut r, 7, 6, "n1:1:2", "none", 300, 400, 0, 0).await;
    xadd(&mut r, 7, 5, "n1:1:3", "completed", 0, 0, 0, 1 << 4).await; // 首次完成
    let _: String = redis::cmd("XADD")
        .arg("acct:1")
        .arg("*")
        .arg(&[("v", "1"), ("uid", "8")])
        .query_async(&mut r)
        .await
        .unwrap(); // 拒收

    let totals = wait_for(&app, json!({"op": "totals", "user": 7}), |b| {
        b["uploaded_raw"] == 1300
    })
    .await;
    assert_eq!(
        (totals["uploaded"].as_i64(), totals["downloaded"].as_i64()),
        (Some(2300), Some(400)),
        "{totals}"
    );
    assert_eq!(
        (
            totals["downloaded_raw"].as_i64(),
            totals["completed_count"].as_i64()
        ),
        (Some(900), Some(1))
    );

    // 窗口关闭后：每 (窗口, 用户) 一条 traffic.settled；首次完成事件一条
    let seen = wait_for(&app, json!({"op": "seen"}), |b| {
        b.as_array().is_some_and(|a| {
            a.iter().any(|e| e[0] == "traffic.settled")
                && a.iter().any(|e| e[0] == "traffic.snatch_completed")
        })
    })
    .await;
    let settled: Vec<&Value> = seen
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e[0] == "traffic.settled")
        .map(|e| &e[1])
        .collect();
    let items: Vec<&Value> = settled
        .iter()
        .flat_map(|p| p["items"].as_array().unwrap().iter())
        .collect();
    let t5: Vec<&&Value> = items.iter().filter(|i| i["torrent_id"] == 5).collect();
    let sum = |k: &str| t5.iter().map(|i| i[k].as_i64().unwrap()).sum::<i64>();
    assert_eq!(
        (sum("d_up_raw"), sum("d_up"), sum("d_down")),
        (1000, 2000, 0),
        "{settled:?}"
    );
    assert!(
        t5.iter()
            .all(|i| i["rate_rule_ids"] == json!([resp.body["rule_id"]]))
    );
    assert!(t5.iter().any(|i| i["completed"] == true));
    let completed: Vec<&Value> = seen
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e[0] == "traffic.snatch_completed")
        .collect();
    assert_eq!(completed.len(), 1);

    // 拒收记录、种子完成计数、分段明细
    let su = env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    );
    let mut db = PgConnection::connect(&su).await.unwrap();
    let rejected: Vec<String> = sqlx::query_scalar("SELECT reason FROM accounting.rejected_entries")
        .fetch_all(&mut db)
        .await
        .unwrap();
    assert_eq!(rejected, vec!["wrong_partition"]);
    let done: i32 =
        sqlx::query_scalar("SELECT completed FROM accounting.torrent_counters WHERE torrent_id = 5")
            .fetch_one(&mut db)
            .await
            .unwrap();
    assert_eq!(done, 1);
    let seg_up: i64 = sqlx::query_scalar(
        "SELECT coalesce(sum(up), 0)::bigint FROM accounting.traffic_segments WHERE user_id = 7",
    )
    .fetch_one(&mut db)
    .await
    .unwrap();
    assert_eq!(seg_up, 2300);

    // 调整：不能使总量为负；幂等
    let r1 = call(
        &app,
        "POST",
        "/api/p/x-promo/v1/op",
        Some(json!({"op": "adjust", "user": 7, "d_up": -5000, "key": "a1"})),
        &[],
    )
    .await;
    assert_eq!(
        r1.body["code"], "accounting.adjust_would_be_negative",
        "{:?}",
        r1.body
    );
    let r2 = call(
        &app,
        "POST",
        "/api/p/x-promo/v1/op",
        Some(json!({"op": "adjust", "user": 7, "d_up": -300, "key": "a2"})),
        &[],
    )
    .await;
    assert_eq!(r2.body["duplicate"], false, "{:?}", r2.body);
    let r3 = call(
        &app,
        "POST",
        "/api/p/x-promo/v1/op",
        Some(json!({"op": "adjust", "user": 7, "d_up": -300, "key": "a2"})),
        &[],
    )
    .await;
    assert_eq!(r3.body["duplicate"], true);

    // 重启：游标持久化，已处理条目不重复计入；重启后的新条目照常计入
    kernel.shutdown().await;
    let mut plugins = pt_server::native_plugins();
    plugins.push(promo());
    let kernel = Kernel::start(config(), plugins)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    xadd(&mut r, 7, 6, "n1:2:1", "none", 50, 0, 0, 0).await;
    let totals = wait_for(&app, json!({"op": "totals", "user": 7}), |b| {
        b["uploaded_raw"] == 1350
    })
    .await;
    assert_eq!(totals["uploaded"].as_i64(), Some(2300 - 300 + 50), "{totals}");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let totals = call(
        &app,
        "POST",
        "/api/p/x-promo/v1/op",
        Some(json!({"op": "totals", "user": 7})),
        &[],
    )
    .await
    .body;
    assert_eq!(
        totals["uploaded_raw"], 1350,
        "no double counting after restart: {totals}"
    );

    // 小时快照：把本小时 peers 槽中的做种秒数折叠进 snatches / 用户总量，并发布快照事件；重复运行不重复折叠
    let hour = chrono::Utc::now().timestamp().div_euclid(3600);
    kernel
        .run_job_now(
            "accounting",
            "seeding_snapshot",
            json!({"hour": hour, "force": true}),
        )
        .await
        .unwrap();
    kernel
        .run_job_now(
            "accounting",
            "seeding_snapshot",
            json!({"hour": hour, "force": true}),
        )
        .await
        .unwrap();
    let seed: i64 = sqlx::query_scalar("SELECT seed_secs FROM accounting.user_totals WHERE user_id = 7")
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(seed, 4 * 30, "dseed of the 4 distinct entries");
    let per_torrent: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT torrent_id, seed_secs FROM accounting.snatches WHERE user_id = 7 ORDER BY torrent_id",
    )
    .fetch_all(&mut db)
    .await
    .unwrap();
    assert_eq!(per_torrent, vec![(5, 60), (6, 60)]);
    let snaps: Vec<Value> = sqlx::query_scalar(
        "SELECT payload FROM kernel.event_log WHERE event_type = 'traffic.seeding_snapshot_taken'",
    )
    .fetch_all(&mut db)
    .await
    .unwrap();
    assert_eq!(snaps.len(), 1, "{snaps:?}");
    assert_eq!(snaps[0]["torrents"].as_array().unwrap().len(), 2);
    assert_eq!(snaps[0]["torrents"][0]["seed_secs"], 60);

    // HTTP：本人统计
    let reg = call(
        &app,
        "POST",
        "/api/p/identity/v1/users",
        Some(json!({"username": "seeder7", "email": "s7@example.com", "password": "correct horse"})),
        &[],
    )
    .await;
    let uid = reg.body["user"]["id"].as_i64().unwrap();
    let login = call(
        &app,
        "POST",
        "/api/p/identity/v1/sessions",
        Some(json!({"username": "seeder7", "password": "correct horse"})),
        &[],
    )
    .await;
    let cookie = login.cookies.join("; ");
    let me = call(
        &app,
        "GET",
        "/api/p/accounting/v1/users/me/stats",
        None,
        &[("cookie", &cookie)],
    )
    .await;
    assert_eq!(
        (me.body["user_id"].as_i64(), me.body["uploaded"].as_i64()),
        (Some(uid), Some(0)),
        "{:?}",
        me.body
    );
    let other = call(
        &app,
        "GET",
        "/api/p/accounting/v1/users/7/stats",
        None,
        &[("cookie", &cookie)],
    )
    .await;
    assert_eq!(other.body["ratio"], "5.1250");
    assert!(
        other.body.get("uploaded_raw").is_none(),
        "others only see public fields"
    );

    kernel.shutdown().await;
}
