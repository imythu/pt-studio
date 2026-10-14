//! 集成测试（真实 Redis；`PT_TRACKER_TEST_REDIS`，默认 redis://127.0.0.1:56379/13，测试会清空该库；不可达时跳过）：
//! 两个 tracker 节点共用 Redis（无状态）、swarm 内容与人数、做种者只拿下载者、完成计数与 scrape、准入拒绝、
//! 非紧凑格式、流量事件、CAS 冲突、变更流实时生效。

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::HashMap;
use std::net::SocketAddr;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

const PK_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const PK_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const IH: [u8; 20] = [0x11; 20];

fn url() -> String {
    std::env::var("PT_TRACKER_TEST_REDIS").unwrap_or_else(|_| "redis://127.0.0.1:56379/13".into())
}

async fn redis() -> Option<redis::aio::MultiplexedConnection> {
    let c = redis::Client::open(url()).ok()?;
    c.get_multiplexed_async_connection().await.ok()
}

async fn cmd(c: &mut redis::aio::MultiplexedConnection, args: &[&str]) {
    let mut x = redis::cmd(args[0]);
    for a in &args[1..] {
        x.arg(*a);
    }
    let _: redis::Value = x.query_async(c).await.unwrap();
}

/// 模拟 tracker-bridge 写入准入数据
async fn seed(c: &mut redis::aio::MultiplexedConnection) {
    cmd(c, &["FLUSHDB"]).await;
    cmd(
        c,
        &[
            "HSET",
            &format!("trk:pk:{PK_A}"),
            "uid",
            "1",
            "st",
            "0",
            "ver",
            "1",
        ],
    )
    .await;
    cmd(
        c,
        &[
            "HSET",
            &format!("trk:pk:{PK_B}"),
            "uid",
            "2",
            "st",
            "0",
            "ver",
            "2",
        ],
    )
    .await;
    cmd(
        c,
        &[
            "HSET",
            &format!("trk:t:{}", hex::encode(IH)),
            "tid",
            "9",
            "st",
            "0",
            "acct",
            "1",
            "size",
            "100",
            "ver",
            "3",
        ],
    )
    .await;
    cmd(
        c,
        &[
            "SET",
            "trk:clients",
            r#"{"default":"deny","rules":[{"peer_id_prefix":"-qB"},{"peer_id_prefix":"-TR"}]}"#,
        ],
    )
    .await;
    cmd(
        c,
        &[
            "HSET",
            "trk:cfg",
            "interval",
            "1800",
            "min_interval",
            "900",
            "max_peers",
            "50",
            "ver",
            "4",
        ],
    )
    .await;
    cmd(
        c,
        &[
            "XADD", "trk:sync", "*", "kind", "cfg", "key", "cfg", "op", "upsert", "ver", "4",
        ],
    )
    .await;
}

async fn node(name: &str) -> SocketAddr {
    let cfg: pt_tracker::Config =
        toml::from_str(&format!("node_id = \"{name}\"\nredis_url = \"{}\"\n", url())).unwrap();
    let router = pt_tracker::start(cfg).await.unwrap();
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(l, router.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .unwrap();
    });
    addr
}

/// 最简 HTTP/1.1 GET，返回响应体（原始字节）
async fn get(addr: SocketAddr, path: &str) -> Vec<u8> {
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    s.write_all(
        format!(
            "GET {path} HTTP/1.1\r\nHost: t\r\nUser-Agent: qBittorrent/5.1.0\r\nConnection: close\r\n\r\n"
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let i = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    buf[i + 4..].to_vec()
}

fn enc(b: &[u8]) -> String {
    b.iter().map(|x| format!("%{x:02x}")).collect()
}

fn pid(tag: &str) -> Vec<u8> {
    let mut p = tag.as_bytes().to_vec();
    p.resize(20, b'x');
    p
}

async fn announce(addr: SocketAddr, pk: &str, peer: &[u8], q: &str) -> String {
    let body = get(
        addr,
        &format!("/announce/{pk}?info_hash={}&peer_id={}&{q}", enc(&IH), enc(peer)),
    )
    .await;
    // 逐字节映射为字符（无损，紧凑 peers 是二进制）
    body.iter().map(|&b| char::from(b)).collect()
}

/// 紧凑 peers 中的端口列表（IPv4，6 字节一组）
fn ports(body: &str) -> Vec<u16> {
    let b: Vec<u8> = body
        .chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap())
        .collect();
    let Some(i) = b.windows(7).position(|w| w == b"5:peers") else {
        return vec![];
    };
    let rest = &b[i + 7..];
    let colon = rest.iter().position(|&x| x == b':').unwrap();
    let len: usize = std::str::from_utf8(&rest[..colon]).unwrap().parse().unwrap();
    rest[colon + 1..colon + 1 + len]
        .chunks(6)
        .map(|c| u16::from_be_bytes([c[4], c[5]]))
        .collect()
}

#[tokio::test]
async fn tracker_against_redis() {
    let Some(mut r) = redis().await else {
        eprintln!("SKIPPED: redis not reachable at {}", url());
        return;
    };
    seed(&mut r).await;
    let n1 = node("n1").await;
    let n2 = node("n2").await;
    let (a, b) = (pid("-qB5100-a"), pid("-TR4100-b"));

    // A 做种
    let x = announce(
        n1,
        PK_A,
        &a,
        "port=6881&uploaded=0&downloaded=0&left=0&event=started",
    )
    .await;
    assert!(x.contains("8:completei1e10:incompletei0e"), "{x}");
    assert!(ports(&x).is_empty());
    // B 下载（在另一个节点上）：看得到 A
    let x = announce(
        n2,
        PK_B,
        &b,
        "port=6882&uploaded=0&downloaded=0&left=100&event=started",
    )
    .await;
    assert!(x.contains("8:completei1e10:incompletei1e"), "{x}");
    assert_eq!(ports(&x), vec![6881]);
    // A 再 announce（节点 1）：做种者只拿下载者，看得到 B
    let x = announce(n1, PK_A, &a, "port=6881&uploaded=40&downloaded=0&left=0").await;
    assert_eq!(ports(&x), vec![6882], "{x}");
    // 非紧凑格式
    let x = announce(
        n1,
        PK_A,
        &a,
        "port=6881&uploaded=40&downloaded=0&left=0&compact=0",
    )
    .await;
    assert!(x.contains("5:peersld2:ip9:127.0.0.17:peer id20:-TR4100-b"), "{x}");
    let x = announce(
        n1,
        PK_A,
        &a,
        "port=6881&uploaded=40&downloaded=0&left=0&compact=0&no_peer_id=1",
    )
    .await;
    assert!(!x.contains("peer id"), "{x}");
    // B 完成
    let x = announce(
        n2,
        PK_B,
        &b,
        "port=6882&uploaded=0&downloaded=100&left=0&event=completed",
    )
    .await;
    assert!(x.contains("8:completei2e10:incompletei0e"), "{x}");
    let s = get(n1, &format!("/scrape/{PK_A}?info_hash={}", enc(&IH))).await;
    assert!(String::from_utf8_lossy(&s).ends_with("d8:completei2e10:downloadedi1e10:incompletei0eeee"));

    // 准入拒绝
    assert!(
        announce(
            n1,
            PK_A,
            &pid("-ZZ0001-z"),
            "port=1&uploaded=0&downloaded=0&left=0"
        )
        .await
        .contains("client not allowed")
    );
    assert!(
        announce(n1, &"c".repeat(32), &a, "port=1&uploaded=0&downloaded=0&left=0")
            .await
            .contains("unregistered passkey")
    );
    assert!(
        announce(n1, "short", &a, "port=1&uploaded=0&downloaded=0&left=0")
            .await
            .contains("invalid passkey")
    );
    assert!(
        announce(n1, PK_A, &a, "port=1&left=0")
            .await
            .contains("bad request")
    );

    // 流量事件：A 上传 40，B 下载 100 且首次完成
    let entries: Vec<(String, Vec<(String, String)>)> = redis::cmd("XRANGE")
        .arg("acct:1")
        .arg("-")
        .arg("+")
        .query_async(&mut r)
        .await
        .unwrap();
    let a_up: u64 = entries
        .iter()
        .map(|(_, f)| {
            f.iter()
                .find(|(k, _)| k == "dup")
                .map_or(0, |(_, v)| v.parse::<u64>().unwrap())
        })
        .sum();
    assert_eq!(a_up, 40);
    let entries: Vec<(String, Vec<(String, String)>)> = redis::cmd("XRANGE")
        .arg("acct:2")
        .arg("-")
        .arg("+")
        .query_async(&mut r)
        .await
        .unwrap();
    let last: HashMap<String, String> = entries.last().unwrap().1.iter().cloned().collect();
    assert_eq!(
        (last["ev"].as_str(), last["ddown"].as_str(), last["tid"].as_str()),
        ("completed", "100", "9")
    );
    assert_eq!(
        last["flags"].parse::<u32>().unwrap() & pt_tracker::accounting::FIRST_COMPLETED,
        pt_tracker::accounting::FIRST_COMPLETED
    );
    let live: HashMap<String, String> = redis::cmd("HGETALL")
        .arg("u:1:live")
        .query_async(&mut r)
        .await
        .unwrap();
    assert_eq!(live.get("up").map(String::as_str), Some("40"));

    // CAS：用过期的原值写入会被拒绝
    let store = pt_tracker::store::Store::new("t".into(), 0, 64, 2700);
    let mut cm = pt_tracker::connect(&url()).await.unwrap();
    let w = pt_tracker::store::Write {
        member: &pt_tracker::store::member(1, &a.clone().try_into().unwrap()),
        expected: "{\"stale\":true}",
        next: None,
        seeder: true,
        now: pt_tracker::now(),
        numwant: 0,
        first_completed: false,
    };
    assert!(matches!(
        store.announce(&mut cm, &IH, &w).await.unwrap(),
        pt_tracker::store::Outcome::Conflict
    ));

    // B 停止：移出 swarm
    let x = announce(
        n2,
        PK_B,
        &b,
        "port=6882&uploaded=0&downloaded=100&left=0&event=stopped",
    )
    .await;
    assert!(x.contains("8:completei1e") && ports(&x).is_empty(), "{x}");

    // 变更流实时生效：封禁 A
    cmd(
        &mut r,
        &["HSET", &format!("trk:pk:{PK_A}"), "st", "2", "ver", "9"],
    )
    .await;
    cmd(
        &mut r,
        &[
            "XADD", "trk:sync", "*", "kind", "pk", "key", PK_A, "op", "upsert", "ver", "9",
        ],
    )
    .await;
    let mut banned = false;
    for _ in 0..50 {
        if announce(n1, PK_A, &a, "port=6881&uploaded=40&downloaded=0&left=0")
            .await
            .contains("account disabled")
        {
            banned = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(banned, "ban did not propagate through trk:sync");
}
