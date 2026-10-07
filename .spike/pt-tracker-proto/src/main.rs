//! pt-tracker 原型：直接组装 torrust 的 swarm-coordination-registry / http-protocol /
//! primitives / events（不依赖 tracker-core，从而不引入 sqlx），自己实现 HTTP 入口与 PT 专用流水线：
//!
//!   解析 → passkey 鉴权 → 用户状态 → 客户端白名单 → 种子白名单
//!   → torrust swarm Registry（swarm 更新 + 取 peers）→ 增量计算 → 流量事件 → bencode 响应
//!
//! torrust 自带的私有模式 / 白名单 / axum 路由都不使用（见 docs/design/10-torrust-integration.md）。

mod accounting;
mod policy;
mod sink;
mod swarm;

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::extract::{ConnectInfo, Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use torrust_info_hash::InfoHash;
use torrust_tracker_events::bus::SenderStatus;
use torrust_tracker_http_protocol::v1::query::Query;
use torrust_tracker_http_protocol::v1::requests::announce::{Announce, Compact, Event as HttpEvent};
use torrust_tracker_http_protocol::v1::responses;
use torrust_tracker_primitives::peer::Peer;
use torrust_tracker_primitives::{AnnounceEvent, AnnouncePolicy, NumberOfBytes, TrackerPolicy};
use torrust_tracker_swarm_coordination_registry::container::SwarmCoordinationRegistryContainer;
use torrust_tracker_swarm_coordination_registry::event::Event as SwarmEvent;

use crate::accounting::{DeltaPolicy, InMemoryPeerStateStore, PeerKey, PeerStateStore, Report};
use crate::policy::{
    ClientAllowlist, ClientRule, InMemoryPasskeys, InMemoryTorrents, PasskeyStore, TorrentCtx, TorrentStore, UserCtx,
    UserStatus, parse_passkey,
};
use crate::sink::{Fanout, LogSink, MemorySink, TrafficEvent, TrafficSink};
use crate::swarm::TorrustSwarm;

struct App {
    node_id: String,
    seq: AtomicU64,
    swarm: TorrustSwarm,
    announce_policy: AnnouncePolicy,
    passkeys: Arc<dyn PasskeyStore>,
    torrents: Arc<dyn TorrentStore>,
    clients: ClientAllowlist,
    peer_states: Arc<dyn PeerStateStore>,
    delta_policy: DeltaPolicy,
    sink: Arc<dyn TrafficSink>,
    memory_sink: Arc<MemorySink>,
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn failure(reason: &str) -> Response {
    // BT 约定：业务失败也返回 200 + bencode 的 failure reason
    let body = responses::error::Error { failure_reason: reason.to_owned() }.write();
    (StatusCode::OK, body).into_response()
}

#[allow(clippy::too_many_lines)]
async fn announce(
    State(app): State<Arc<App>>,
    Path(raw_passkey): Path<String>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    RawQuery(raw_query): RawQuery,
) -> Response {
    // 1. 鉴权：passkey 格式 → 本地缓存查找（生产：miss 时回源 Redis，带负缓存）
    let Some(passkey) = parse_passkey(&raw_passkey) else {
        return failure("invalid passkey");
    };
    let Some(user): Option<UserCtx> = app.passkeys.lookup(&passkey) else {
        return failure("unregistered passkey");
    };
    if user.status == UserStatus::Banned {
        return failure("account disabled");
    }

    // 2. 解析 announce 参数（复用 torrust http-protocol 的解析器）
    let Some(raw_query) = raw_query else {
        return failure("missing query");
    };
    let announce: Announce = match raw_query.parse::<Query>().map_err(|e| e.to_string()).and_then(|q| {
        Announce::try_from(q).map_err(|e| e.to_string())
    }) {
        Ok(a) => a,
        Err(e) => return failure(&format!("bad announce request: {e}")),
    };

    // 3. 客户端白名单（peer_id 前缀 + User-Agent）
    let user_agent = headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("");
    let Some(rule) = app.clients.check(&announce.peer_id.0, user_agent) else {
        return failure("client not allowed");
    };
    tracing::debug!(client_rule = %rule.name, "client allowed");

    // 4. 种子白名单
    let info_hash_bytes: [u8; 20] = announce.info_hash.0;
    let Some(torrent): Option<TorrentCtx> = app.torrents.lookup(&info_hash_bytes) else {
        return failure("torrent not registered");
    };

    let uploaded = announce.uploaded.map_or(0, |n| n.0.max(0) as u64);
    let downloaded = announce.downloaded.map_or(0, |n| n.0.max(0) as u64);
    let left = announce.left.map_or(0, |n| n.0.max(0) as u64);
    let event = match announce.event {
        Some(HttpEvent::Started) => accounting::Event::Started,
        Some(HttpEvent::Stopped) => accounting::Event::Stopped,
        Some(HttpEvent::Completed) => accounting::Event::Completed,
        Some(HttpEvent::Empty) | None => accounting::Event::None,
    };

    // 5. 用户状态：禁止下载的用户只能做种
    if user.status == UserStatus::LeechDisabled && left > 0 && event != accounting::Event::Stopped {
        return failure("download privilege disabled");
    }

    // 6. swarm 更新 + 取 peers：直接调用 torrust swarm Registry
    //    （torrust 自带的 key 鉴权 / 白名单在 tracker-core 与 http-core 中，这里都不使用）
    let peer_ip: IpAddr = remote.ip(); // 生产：反向代理时用 http-protocol 的 peer_ip_resolver 取 X-Forwarded-For
    let peer = Peer {
        peer_id: announce.peer_id,
        peer_addr: SocketAddr::new(peer_ip, announce.port),
        updated: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default(),
        uploaded: NumberOfBytes::new(uploaded as i64),
        downloaded: NumberOfBytes::new(downloaded as i64),
        left: NumberOfBytes::new(left as i64),
        event: match event {
            accounting::Event::Started => AnnounceEvent::Started,
            accounting::Event::Stopped => AnnounceEvent::Stopped,
            accounting::Event::Completed => AnnounceEvent::Completed,
            accounting::Event::None => AnnounceEvent::None,
        },
    };
    let data = match app.swarm.announce(&announce.info_hash, &peer, announce.numwant).await {
        Ok(d) => d,
        Err(e) => return failure(&format!("tracker error: {e}")),
    };

    // 7. 增量计算（自有状态，键 = user_id + info_hash + peer_id）
    let now = now_secs();
    let report = Report { uploaded, downloaded, left, event, now };
    let key = PeerKey { user_id: user.user_id, info_hash: info_hash_bytes, peer_id: announce.peer_id.0 };
    let mut delta = accounting::Delta::default();
    app.peer_states.update(key, &mut |prev| {
        let (d, next) = accounting::compute(prev, &report, &app.delta_policy);
        delta = d;
        next
    });

    // 8. 流量事件
    let ev = TrafficEvent {
        v: 1,
        node: app.node_id.clone(),
        seq: app.seq.fetch_add(1, Ordering::Relaxed),
        ts: now,
        user_id: user.user_id,
        torrent_id: torrent.torrent_id,
        info_hash: hex(&info_hash_bytes),
        peer_id: hex(&announce.peer_id.0),
        ip: peer_ip.to_string(),
        port: announce.port,
        event,
        uploaded,
        downloaded,
        left,
        delta,
        user_agent: user_agent.to_owned(),
    };
    app.sink.emit(&ev);

    // 9. 响应（复用 torrust http-protocol 的 bencode 编码）
    let protocol_data = responses::announce::AnnounceData {
        peers: data
            .peers
            .into_iter()
            .map(|p| responses::announce::Peer { peer_id: p.peer_id, peer_addr: p.peer_addr })
            .collect(),
        stats: responses::announce::SwarmMetadata {
            complete: data.stats.complete,
            downloaded: data.stats.downloaded,
            incomplete: data.stats.incomplete,
        },
        policy: responses::announce::AnnouncePolicy {
            interval: app.announce_policy.interval,
            interval_min: app.announce_policy.interval_min,
        },
    };
    let bytes: Vec<u8> = if announce.compact == Some(Compact::NotAccepted) {
        let r: responses::Announce<responses::Normal> = protocol_data.into();
        r.data.into()
    } else {
        let r: responses::Announce<responses::Compact> = protocol_data.into();
        r.data.into()
    };
    (StatusCode::OK, bytes).into_response()
}

async fn debug_events(State(app): State<Arc<App>>) -> Response {
    let evs = app.memory_sink.events.lock().expect("poisoned").clone();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string_pretty(&serde_json::json!({
            "peer_states": app.peer_states.len(),
            "events": evs,
        }))
        .unwrap_or_default(),
    )
        .into_response()
}

fn demo_data() -> (InMemoryPasskeys, InMemoryTorrents, ClientAllowlist) {
    let passkeys = InMemoryPasskeys::default();
    {
        let mut m = passkeys.0.write().expect("poisoned");
        let pk = |s: &str| parse_passkey(s).expect("valid demo passkey");
        m.insert(pk("0123456789abcdef0123456789abcdef"), UserCtx { user_id: 42, status: UserStatus::Active });
        m.insert(pk("ffffffffffffffffffffffffffffffff"), UserCtx { user_id: 7, status: UserStatus::Banned });
        m.insert(pk("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"), UserCtx { user_id: 9, status: UserStatus::LeechDisabled });
    }
    let torrents = InMemoryTorrents::default();
    torrents.0.write().expect("poisoned").insert(
        "3b245504cf5f11bbdbe1201cea6a6bf45aee1bc0".parse::<InfoHash>().expect("valid").0,
        TorrentCtx { torrent_id: 1001 },
    );
    let clients = ClientAllowlist {
        rules: vec![
            ClientRule {
                name: "qBittorrent 4.x/5.x".into(),
                peer_id_prefix: b"-qB".to_vec(),
                user_agent_prefix: Some("qBittorrent/".into()),
            },
            ClientRule { name: "Transmission 4.x".into(), peer_id_prefix: b"-TR4".to_vec(), user_agent_prefix: None },
        ],
    };
    (passkeys, torrents, clients)
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let bind: SocketAddr = std::env::var("PT_TRACKER_BIND").unwrap_or_else(|_| "127.0.0.1:7070".into()).parse().expect("bind");
    let partitions: u64 = std::env::var("PT_ACCT_PARTITIONS").ok().and_then(|v| v.parse().ok()).unwrap_or(16);

    // torrust swarm 注册表（只用这一层；不初始化 tracker-core，不需要数据库）
    let swarm = Arc::new(SwarmCoordinationRegistryContainer::initialize(SenderStatus::Enabled));
    // max_peer_timeout 必须大于 announce 间隔，否则 peer 会在两次 announce 之间被清理
    let tracker_policy = TrackerPolicy { max_peer_timeout: 2700, ..TrackerPolicy::default() };
    let max_peer_timeout = u64::from(tracker_policy.max_peer_timeout);
    let announce_policy = AnnouncePolicy::new(1800, 900, 50); // PT 常用：30 分钟间隔
    let cleanup_every = Duration::from_secs(600);

    // 订阅 torrust swarm 事件总线，验证事件机制与字段（只是旁路观察，不用于记账）
    let mut rx = swarm.event_bus.receiver();
    tokio::spawn(async move {
        while let Ok(ev) = rx.recv().await {
            match ev {
                SwarmEvent::PeerUpdated { info_hash, old_peer, new_peer } => tracing::info!(
                    target: "swarm_event",
                    "PeerUpdated ih={info_hash} addr={} old_up={} new_up={} (无用户身份字段)",
                    new_peer.peer_addr, old_peer.uploaded.0, new_peer.uploaded.0
                ),
                other => tracing::info!(target: "swarm_event", "{other:?}"),
            }
        }
    });

    let app_swarm = TorrustSwarm { registry: swarm.swarms.clone(), max_peers: announce_policy.max_peers_per_announce };
    {
        // 不活跃 peer 清理
        let cleaner = TorrustSwarm { registry: swarm.swarms.clone(), max_peers: 0 };
        tokio::spawn(async move {
            let mut t = tokio::time::interval(cleanup_every);
            t.tick().await;
            loop {
                t.tick().await;
                cleaner.cleanup(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default(), &tracker_policy).await;
            }
        });
    }

    let (passkeys, torrents, clients) = demo_data();
    let memory_sink = Arc::new(MemorySink::default());
    let app = Arc::new(App {
        node_id: std::env::var("PT_NODE_ID").unwrap_or_else(|_| "node-1".into()),
        seq: AtomicU64::new(0),
        swarm: app_swarm,
        announce_policy,
        passkeys: Arc::new(passkeys),
        torrents: Arc::new(torrents),
        clients,
        peer_states: Arc::new(InMemoryPeerStateStore::default()),
        delta_policy: DeltaPolicy { max_interval_secs: max_peer_timeout },
        sink: Arc::new(Fanout(vec![Arc::new(LogSink { partitions }), memory_sink.clone()])),
        memory_sink,
    });

    let router = Router::new()
        .route("/announce/{passkey}", get(announce))
        .route("/announce", get(|| async { failure("passkey required: use /announce/{passkey}") }))
        .route("/debug/events", get(debug_events))
        .with_state(app);

    let listener = tokio::net::TcpListener::bind(bind).await.expect("bind");
    tracing::info!("pt-tracker-proto listening on http://{bind}");
    axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>()).await.expect("serve");
}
