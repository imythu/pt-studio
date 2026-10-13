//! pt-tracker：PT Studio 的 HTTP tracker（design/10 §5，单节点版本）。
//!
//! `GET /announce/<passkey>`、`GET /scrape/<passkey>`、`GET /health`。准入数据（passkey、种子、客户端白名单、参数）来自
//! tracker-bridge 写入的 Redis 快照与变更流；每次 announce 计算该会话的增量，写 `acct:<uid % N>` 流（accounting 消费）
//! 与 `u:<uid>:live` 实时计数。本版本：内存 swarm、会话状态写穿 Redis（重启可恢复基线）、IPv4 / IPv6 紧凑响应、
//! 单 hash scrape。分片、Prometheus 指标、UDP 在后续迭代实现。
//!
//! 用法：`pt-tracker [配置文件]`（默认 `tracker.toml`；字段见 tracker.example.toml）

mod accounting;
mod admission;
mod protocol;
mod sink;
mod swarm;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    Router,
    extract::{ConnectInfo, Path, RawQuery, State},
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;

use accounting::{Event, Report, compute};
use admission::Admission;
use protocol::{AnnounceReply, PeerAddr};
use sink::{Fact, Sink};
use swarm::Swarms;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(default = "d_node")]
    node_id: String,
    #[serde(default = "d_bind")]
    bind: String,
    redis_url: String,
    sync_url: Option<String>,
    acct_url: Option<String>,
    state_url: Option<String>,
    #[serde(default)]
    trust_forwarded: bool,
    #[serde(default = "d_timeout")]
    max_peer_timeout: u64,
    #[serde(default = "d_partitions")]
    partitions: u64,
}

fn d_node() -> String {
    "tracker-1".into()
}
fn d_bind() -> String {
    "0.0.0.0:8081".into()
}
fn d_timeout() -> u64 {
    2700
}
fn d_partitions() -> u64 {
    64
}

struct App {
    cfg: Config,
    admission: Admission,
    swarms: Swarms,
    sink: Sink,
    sync: redis::aio::ConnectionManager,
    acct: redis::aio::ConnectionManager,
    state: redis::aio::ConnectionManager,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn bencode(body: Vec<u8>) -> Response {
    let mut r = body.into_response();
    r.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    r
}

fn fail(msg: &str) -> Response {
    bencode(protocol::failure(msg))
}

fn client_ip(app: &App, headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
    if app.cfg.trust_forwarded
        && let Some(ip) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .and_then(|v| v.trim().parse().ok())
    {
        return ip;
    }
    peer.ip()
}

fn passkey_ok(pk: &str) -> bool {
    pk.len() == 32
        && pk
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// 日志中的 passkey 打码
fn masked(pk: &str) -> String {
    format!("{}…", pk.chars().take(4).collect::<String>())
}

async fn announce(
    State(app): State<Arc<App>>,
    Path(passkey): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    RawQuery(q): RawQuery,
) -> Response {
    let q = protocol::parse_query(q.as_deref().unwrap_or(""));
    // ① 解析
    let (Some(ih), Some(pid), Some(port)) = (
        protocol::get(&q, "info_hash").and_then(protocol::hash20),
        protocol::get(&q, "peer_id").and_then(protocol::hash20),
        protocol::get_u64(&q, "port")
            .and_then(|p| u16::try_from(p).ok())
            .filter(|p| *p > 0),
    ) else {
        return fail("bad request");
    };
    let (Some(up), Some(down), Some(left)) = (
        protocol::get_u64(&q, "uploaded"),
        protocol::get_u64(&q, "downloaded"),
        protocol::get_u64(&q, "left"),
    ) else {
        return fail("bad request");
    };
    let Some(event) = Event::parse(protocol::get_str(&q, "event")) else {
        return fail("bad request");
    };
    // ② 鉴权
    if !passkey_ok(&passkey) {
        return fail("invalid passkey");
    }
    let mut sync = app.sync.clone();
    let Some(user) = app.admission.user(&mut sync, &passkey).await else {
        return fail("unregistered passkey");
    };
    if user.st == 2 {
        return fail("account disabled");
    }
    // ③ 客户端白名单
    if !app.admission.client_allowed(&pid) {
        return fail("client not allowed");
    }
    // ④ 种子
    let Some(torrent) = app.admission.torrent(&mut sync, &ih).await else {
        return fail("torrent not registered");
    };
    if torrent.st != 0 {
        return fail("torrent not available");
    }
    if user.st == 1 && left > 0 && event != Event::Stopped {
        return fail("download privilege disabled");
    }
    let ts = now();
    let ip = client_ip(&app, &headers, peer);
    let addr = PeerAddr { ip, port };
    // ⑤ 增量与记账（先算，completed 计数据此判断首次）
    let key = (ih, user.uid, pid);
    let mut state = app.state.clone();
    let prev = app.sink.load(&mut state, &key).await;
    let report = Report {
        up,
        down,
        left,
        event,
        now: ts,
    };
    let (delta, next) = compute(prev.as_ref(), &report, app.cfg.max_peer_timeout);
    let first_completed = delta.flags & accounting::FIRST_COMPLETED != 0;
    // ⑥ swarm
    let params = app.admission.params();
    let numwant = protocol::get_u64(&q, "numwant")
        .map_or(params.max_peers, |n| {
            usize::try_from(n).unwrap_or(params.max_peers)
        })
        .min(params.max_peers);
    let r = app.swarms.announce(
        ih,
        user.uid,
        pid,
        addr,
        left == 0,
        event == Event::Stopped,
        first_completed,
        ts,
        numwant,
    );
    // ⑦ 输出（只对计账的 hash 写流量事件）
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let fact = Fact {
        ts,
        uid: user.uid,
        tid: torrent.tid,
        ih: &ih,
        pid: &pid,
        ip: ip.to_string(),
        port,
        event,
        up,
        down,
        left,
        ua,
    };
    let emit = torrent.acct && (!delta.is_zero() || event != Event::None);
    let mut acct = if same_conn(&app) {
        state.clone()
    } else {
        app.acct.clone()
    };
    let res = if same_conn(&app) {
        app.sink.commit(&mut acct, &key, next, &delta, &fact, emit).await
    } else {
        // 状态与流量在不同实例：先写状态，再写事件
        match app
            .sink
            .commit(&mut state, &key, next, &delta, &fact, false)
            .await
        {
            Ok(()) if emit => app.sink.commit(&mut acct, &key, next, &delta, &fact, true).await,
            other => other,
        }
    };
    if let Err(e) = res {
        tracing::error!(error = %e, passkey = %masked(&passkey), "recording announce failed");
        return fail("tracker busy, retry later");
    }
    bencode(protocol::announce(&AnnounceReply {
        complete: r.complete,
        incomplete: r.incomplete,
        interval: params.interval,
        min_interval: params.min_interval,
        peers: &r.peers,
    }))
}

fn same_conn(app: &App) -> bool {
    app.cfg.acct_url.is_none() && app.cfg.state_url.is_none()
}

async fn scrape(State(app): State<Arc<App>>, Path(passkey): Path<String>, RawQuery(q): RawQuery) -> Response {
    if !passkey_ok(&passkey) {
        return fail("invalid passkey");
    }
    let mut sync = app.sync.clone();
    match app.admission.user(&mut sync, &passkey).await {
        Some(u) if u.st != 2 => {}
        _ => return fail("unregistered passkey"),
    }
    let q = protocol::parse_query(q.as_deref().unwrap_or(""));
    let hashes: Vec<[u8; 20]> = q
        .iter()
        .filter(|(k, _)| k == "info_hash")
        .filter_map(|(_, v)| protocol::hash20(v))
        .take(10)
        .collect();
    if hashes.is_empty() {
        return fail("full scrape is not supported");
    }
    let mut files = Vec::new();
    for h in hashes {
        if app
            .admission
            .torrent(&mut sync, &h)
            .await
            .is_some_and(|t| t.st == 0)
        {
            let (c, d, i) = app.swarms.stats(&h);
            files.push((h, c, d, i));
        }
    }
    bencode(protocol::scrape(&files))
}

async fn health(State(app): State<Arc<App>>) -> impl IntoResponse {
    let (users, torrents) = app.admission.counts();
    axum::Json(
        serde_json::json!({ "status": "ok", "node": app.cfg.node_id, "passkeys": users, "torrents": torrents }),
    )
}

async fn connect(url: &str) -> redis::RedisResult<redis::aio::ConnectionManager> {
    redis::Client::open(url)?.get_connection_manager().await
}

/// 阻塞读（XREAD BLOCK）用的专用连接：不受默认响应超时限制，也不占用共享连接
async fn connect_blocking(url: &str) -> redis::RedisResult<redis::aio::ConnectionManager> {
    redis::Client::open(url)?
        .get_connection_manager_with_config(
            redis::aio::ConnectionManagerConfig::new().set_response_timeout(None),
        )
        .await
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let path = std::env::args().nth(1).unwrap_or_else(|| "tracker.toml".into());
    let cfg: Config = match std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|t| toml::from_str(&t).map_err(|e| e.to_string()))
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {path}: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    let urls = [
        cfg.sync_url.clone().unwrap_or_else(|| cfg.redis_url.clone()),
        cfg.acct_url.clone().unwrap_or_else(|| cfg.redis_url.clone()),
        cfg.state_url.clone().unwrap_or_else(|| cfg.redis_url.clone()),
    ];
    let mut conns = Vec::new();
    for u in &urls {
        match connect(u).await {
            Ok(c) => conns.push(c),
            Err(e) => {
                eprintln!("error: redis {u}: {e}");
                return std::process::ExitCode::from(3);
            }
        }
    }
    let (state, acct, mut sync) = (conns.remove(2), conns.remove(1), conns.remove(0));
    // 分区数与 accounting 一致：acct:_meta 中已有值时以它为准
    let mut a = acct.clone();
    let _: redis::RedisResult<i64> = redis::cmd("HSETNX")
        .arg("acct:_meta")
        .arg("partitions")
        .arg(cfg.partitions)
        .query_async(&mut a)
        .await;
    let partitions: u64 = redis::cmd("HGET")
        .arg("acct:_meta")
        .arg("partitions")
        .query_async::<Option<u64>>(&mut a)
        .await
        .ok()
        .flatten()
        .unwrap_or(cfg.partitions);
    let admission = Admission::new();
    let last = match admission.load_all(&mut sync).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: loading tracker state from redis: {e}");
            return std::process::ExitCode::from(3);
        }
    };
    let (u, t) = admission.counts();
    tracing::info!(passkeys = u, torrents = t, partitions, "admission state loaded");
    let app = Arc::new(App {
        sink: Sink::new(cfg.node_id.clone(), now(), partitions, cfg.max_peer_timeout * 2),
        admission,
        swarms: Swarms::default(),
        sync,
        acct,
        state,
        cfg: cfg.clone(),
    });
    let follower = match connect_blocking(&urls[0]).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: redis {}: {e}", urls[0]);
            return std::process::ExitCode::from(3);
        }
    };
    {
        let app = app.clone();
        tokio::spawn(async move { app.admission.follow(follower, last).await });
    }
    {
        let app = app.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(600));
            loop {
                tick.tick().await;
                let (removed, swarms) = app.swarms.cleanup(now(), app.cfg.max_peer_timeout);
                app.sink.cleanup(now(), app.cfg.max_peer_timeout);
                tracing::debug!(removed, swarms, "swarm cleanup");
            }
        });
    }
    let router = Router::new()
        .route("/announce/{passkey}", get(announce))
        .route("/scrape/{passkey}", get(scrape))
        .route("/health", get(health))
        .with_state(app);
    let listener = match tokio::net::TcpListener::bind(&cfg.bind).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: bind {}: {e}", cfg.bind);
            return std::process::ExitCode::from(4);
        }
    };
    tracing::info!(bind = %cfg.bind, "pt-tracker listening");
    let server = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    });
    if let Err(e) = server.await {
        tracing::error!(error = %e, "server error");
    }
    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passkeys() {
        assert!(passkey_ok("0123456789abcdef0123456789abcdef"));
        assert!(!passkey_ok("0123456789ABCDEF0123456789abcdef"));
        assert!(!passkey_ok("short"));
        assert_eq!(masked("abcdef"), "abcd…");
    }
}
