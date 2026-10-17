//! pt-tracker：PT Studio 的 HTTP tracker（design/10 §5；本实现为自研，见 docs/design/README.md 决策 D20）。
//!
//! `GET /announce/<passkey>`、`GET /scrape/<passkey>`、`GET /health`。准入数据（passkey、种子、客户端白名单、参数）来自
//! tracker-bridge 写入的 Redis 快照与变更流，缓存在本地内存；swarm 与会话状态都在 Redis 中（见 [`store`]），因此 tracker
//! 是无状态的：可以同时运行多个节点、随意重启，负载均衡不需要按 info_hash 分片。每次 announce 计算该会话的增量，
//! 写 `acct:<uid % N>` 流（accounting 消费）与 `u:<uid>:live` 实时计数。

pub mod accounting;
pub mod admission;
pub mod metrics;
pub mod protocol;
pub mod store;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

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
use store::{Fact, Outcome, Store, Stored, Write};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "d_node")]
    pub node_id: String,
    #[serde(default = "d_bind")]
    pub bind: String,
    pub redis_url: String,
    /// `trk:*`（只读）；默认同 redis_url
    pub sync_url: Option<String>,
    /// `acct:*`、`u:*:live`；默认同 redis_url
    pub acct_url: Option<String>,
    /// swarm 与会话状态（`ps:*`、`sw:*`）；默认同 redis_url
    pub state_url: Option<String>,
    #[serde(default)]
    pub trust_forwarded: bool,
    /// peer 超时（秒）：超过这么久没有 announce 的 peer 被移出 swarm；同时是两次 announce 之间计入时长的上限
    #[serde(default = "d_timeout")]
    pub max_peer_timeout: u64,
    /// 单个会话上传 / 下载的平均速度上限（字节 / 秒），超过时该段流量不计并打 speed_exceeded 标；0 不限
    #[serde(default = "d_max_rate")]
    pub max_rate: u64,
    #[serde(default = "d_partitions")]
    pub partitions: u64,
    /// Prometheus 指标的独立监听地址（如 `127.0.0.1:9465`）；不设置时不提供 `/metrics`
    #[serde(default)]
    pub metrics_bind: Option<String>,
}

fn d_node() -> String {
    "tracker-1".into()
}
fn d_bind() -> String {
    "0.0.0.0:31252".into()
}
fn d_timeout() -> u64 {
    2700
}
fn d_max_rate() -> u64 {
    // 10 Gbit/s
    1_250_000_000
}
fn d_partitions() -> u64 {
    64
}

impl Config {
    pub fn sync_url(&self) -> &str {
        self.sync_url.as_deref().unwrap_or(&self.redis_url)
    }
    pub fn acct_url(&self) -> &str {
        self.acct_url.as_deref().unwrap_or(&self.redis_url)
    }
    pub fn state_url(&self) -> &str {
        self.state_url.as_deref().unwrap_or(&self.redis_url)
    }
}

pub struct App {
    pub cfg: Config,
    pub admission: Admission,
    pub store: Store,
    pub sync: redis::aio::ConnectionManager,
    pub acct: redis::aio::ConnectionManager,
    pub state: redis::aio::ConnectionManager,
}

pub fn now() -> u64 {
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

/// 失败原因（固定字符串，指标标签）
#[derive(Clone, Copy)]
struct Failure(&'static str);

fn fail(msg: &'static str) -> Response {
    let mut r = bencode(protocol::failure(msg));
    r.extensions_mut().insert(Failure(msg));
    r
}

fn failure_of(r: &Response) -> Option<&'static str> {
    r.extensions().get::<Failure>().map(|f| f.0)
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

pub fn passkey_ok(pk: &str) -> bool {
    pk.len() == 32
        && pk
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// 日志中的 passkey 打码
fn masked(pk: &str) -> String {
    format!("{}…", pk.chars().take(4).collect::<String>())
}

/// CAS 冲突时重新读取再算的次数
const MAX_RETRIES: usize = 3;

async fn announce(
    State(app): State<Arc<App>>,
    Path(passkey): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    RawQuery(q): RawQuery,
) -> Response {
    let started = std::time::Instant::now();
    let r = announce_inner(app, passkey, peer, headers, q).await;
    metrics::request("announce", failure_of(&r));
    metrics::announce_latency(started.elapsed().as_secs_f64());
    r
}

async fn announce_inner(
    app: Arc<App>,
    passkey: String,
    peer: SocketAddr,
    headers: HeaderMap,
    q: Option<String>,
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
    // BEP 7：连接走 IPv4 时客户端用 `ipv6=` 报 IPv6 地址（反之 `ipv4=`）；双栈客户端靠它在只有单栈的 tracker 上互相找到对方
    let alt = protocol::get_str(
        &q,
        if ip.to_canonical().is_ipv4() {
            "ipv6"
        } else {
            "ipv4"
        },
    )
    .and_then(|v| protocol::alt_addr(v, port))
    .filter(|a| a.is_ipv4() != ip.to_canonical().is_ipv4());
    let params = app.admission.params();
    let numwant = protocol::get_u64(&q, "numwant")
        .map_or(params.max_peers, |n| {
            usize::try_from(n).unwrap_or(params.max_peers)
        })
        .min(params.max_peers);
    // ⑤ 增量计算 + swarm 更新（同一脚本中 CAS 会话状态）
    let member = store::member(user.uid, &pid);
    let mut state = app.state.clone();
    let mut done = None;
    for attempt in 0..MAX_RETRIES {
        let (raw, prev) = match app.store.load_cached(&mut state, &ih, &member, attempt > 0).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(error = %e, "reading peer state failed");
                return fail("tracker busy, retry later");
            }
        };
        let report = Report {
            up,
            down,
            left,
            event,
            now: ts,
        };
        let (delta, next) = compute(prev.as_ref(), &report, app.cfg.max_peer_timeout, app.cfg.max_rate);
        let w = Write {
            member: &member,
            expected: &raw,
            next: next.map(|s| Stored {
                state: s,
                ip: ip.to_string(),
                port,
                alt: alt.map(|a| a.to_string()),
            }),
            seeder: left == 0,
            now: ts,
            numwant,
            first_completed: delta.flags & accounting::FIRST_COMPLETED != 0,
        };
        match app.store.announce(&mut state, &ih, &w).await {
            Ok(Outcome::Done(view)) => {
                done = Some((delta, view));
                break;
            }
            Ok(Outcome::Conflict) => continue,
            Err(e) => {
                tracing::error!(error = %e, passkey = %masked(&passkey), "recording announce failed");
                return fail("tracker busy, retry later");
            }
        }
    }
    let Some((delta, view)) = done else {
        return fail("tracker busy, retry later");
    };
    // ⑥ 流量事件（只对计账的 hash；没有变化的普通 announce 不写）
    if torrent.acct && (!delta.is_zero() || event != Event::None) {
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
        let mut acct = app.acct.clone();
        if let Err(e) = app.store.emit(&mut acct, &delta, &fact).await {
            // 状态已经前移：这一段流量会少记（宁可少记、不多记），客户端照常继续
            tracing::error!(error = %e, uid = user.uid, "writing traffic event failed");
        }
    }
    // ⑦ 响应：每个 peer 给出连接地址与 BEP 7 地址；去掉与自己同地址的（同一台机器的多个会话）
    let me = |a: SocketAddr| (a.ip() == ip && a.port() == port) || Some(a) == alt;
    let peers: Vec<PeerAddr> = view
        .peers
        .iter()
        .flat_map(|p| {
            std::iter::once(SocketAddr::new(p.ip, p.port))
                .chain(p.alt)
                .filter(|a| !me(*a))
                .map(|a| PeerAddr {
                    ip: a.ip(),
                    port: a.port(),
                    peer_id: p.peer_id,
                })
        })
        .collect();
    metrics::event(event.as_str());
    let compact = protocol::get_str(&q, "compact") != Some("0");
    let no_peer_id = protocol::get_str(&q, "no_peer_id") == Some("1");
    bencode(protocol::announce(&AnnounceReply {
        complete: view.seeders,
        incomplete: view.leechers,
        interval: params.interval,
        min_interval: params.min_interval,
        peers: &peers,
        compact,
        no_peer_id,
    }))
}

async fn scrape(State(app): State<Arc<App>>, Path(passkey): Path<String>, RawQuery(q): RawQuery) -> Response {
    let r = scrape_inner(app, passkey, q).await;
    metrics::request("scrape", failure_of(&r));
    r
}

async fn scrape_inner(app: Arc<App>, passkey: String, q: Option<String>) -> Response {
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
    let mut state = app.state.clone();
    let mut files = Vec::new();
    for h in hashes {
        if app
            .admission
            .torrent(&mut sync, &h)
            .await
            .is_some_and(|t| t.st == 0)
        {
            match app.store.stats(&mut state, &h).await {
                Ok((c, d, i)) => files.push((h, c, d, i)),
                Err(e) => {
                    tracing::error!(error = %e, "scrape failed");
                    return fail("tracker busy, retry later");
                }
            }
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

pub async fn connect(url: &str) -> redis::RedisResult<redis::aio::ConnectionManager> {
    redis::Client::open(url)?.get_connection_manager().await
}

/// 阻塞读（XREAD BLOCK）用的专用连接：不受默认响应超时限制，也不占用共享连接
pub async fn connect_blocking(url: &str) -> redis::RedisResult<redis::aio::ConnectionManager> {
    redis::Client::open(url)?
        .get_connection_manager_with_config(
            redis::aio::ConnectionManagerConfig::new().set_response_timeout(None),
        )
        .await
}

/// 连接 Redis、加载准入数据、启动变更流跟随；返回路由（`into_make_service_with_connect_info::<SocketAddr>()` 提供服务）
pub async fn start(cfg: Config) -> Result<Router, String> {
    let sync = connect(cfg.sync_url())
        .await
        .map_err(|e| format!("redis {}: {e}", cfg.sync_url()))?;
    let acct = connect(cfg.acct_url())
        .await
        .map_err(|e| format!("redis {}: {e}", cfg.acct_url()))?;
    let state = connect(cfg.state_url())
        .await
        .map_err(|e| format!("redis {}: {e}", cfg.state_url()))?;
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
    let mut s = sync.clone();
    let last = admission
        .load_all(&mut s)
        .await
        .map_err(|e| format!("loading tracker state from redis: {e}"))?;
    let (u, t) = admission.counts();
    tracing::info!(passkeys = u, torrents = t, partitions, "admission state loaded");
    let follower = connect_blocking(cfg.sync_url())
        .await
        .map_err(|e| format!("redis {}: {e}", cfg.sync_url()))?;
    let app = Arc::new(App {
        store: Store::new(cfg.node_id.clone(), now(), partitions, cfg.max_peer_timeout),
        admission,
        sync,
        acct,
        state,
        cfg,
    });
    {
        let app = app.clone();
        tokio::spawn(async move { app.admission.follow(follower, last).await });
    }
    if let Some(mb) = app.cfg.metrics_bind.clone() {
        let a = app.clone();
        match tokio::net::TcpListener::bind(&mb).await {
            Ok(l) => {
                tracing::info!(bind = %mb, "metrics listening");
                let r = Router::new().route(
                    "/metrics",
                    get(move || {
                        let (u, t) = a.admission.counts();
                        async move { ([(header::CONTENT_TYPE, "text/plain; version=0.0.4")], metrics::render(u, t)) }
                    }),
                );
                tokio::spawn(async move {
                    let _ = axum::serve(l, r).await;
                });
            }
            Err(e) => tracing::error!(bind = %mb, error = %e, "metrics listener not started"),
        }
    }
    Ok(Router::new()
        .route("/announce/{passkey}", get(announce))
        .route("/scrape/{passkey}", get(scrape))
        .route("/health", get(health))
        .with_state(app))
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
