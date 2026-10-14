//! 压测：在进程内启动 tracker（连接给定的 Redis 库，**会清空该库**），造 U 个用户、T 个种子，用 C 条长连接发送 N 次
//! announce（随机用户 / 种子 / 进度），输出吞吐与延迟分位数。
//!
//!   pt-tracker-bench <redis_url> [并发 C=64] [请求数 N=200000] [用户 U=20000] [种子 T=5000]

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

fn arg<T: std::str::FromStr>(i: usize, d: T) -> T {
    std::env::args().nth(i).and_then(|v| v.parse().ok()).unwrap_or(d)
}

fn pk(u: u64) -> String {
    format!("{u:032x}")
}

fn ih(t: u64) -> [u8; 20] {
    let mut h = [0u8; 20];
    h[..8].copy_from_slice(&t.to_be_bytes());
    h[19] = 1;
    h
}

fn enc(b: &[u8]) -> String {
    b.iter().map(|x| format!("%{x:02x}")).collect()
}

/// 简单的 xorshift，避免引入 rand
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[tokio::main]
async fn main() {
    let Some(url) = std::env::args().nth(1) else {
        eprintln!("usage: pt-tracker-bench <redis_url> [concurrency] [requests] [users] [torrents]");
        std::process::exit(2);
    };
    let (c, n, users, torrents): (u64, u64, u64, u64) =
        (arg(2, 64), arg(3, 200_000), arg(4, 20_000), arg(5, 5_000));
    let client = redis::Client::open(url.as_str()).expect("redis url");
    let mut r = client.get_multiplexed_async_connection().await.expect("redis");
    let _: () = redis::cmd("FLUSHDB").query_async(&mut r).await.expect("flushdb");
    eprintln!("seeding {users} users, {torrents} torrents …");
    for chunk in (0..users).collect::<Vec<_>>().chunks(1000) {
        let mut p = redis::pipe();
        for u in chunk {
            p.cmd("HSET")
                .arg(format!("trk:pk:{}", pk(*u)))
                .arg("uid")
                .arg(u + 1)
                .arg("st")
                .arg(0)
                .ignore();
        }
        let _: () = p.query_async(&mut r).await.expect("seed users");
    }
    for chunk in (0..torrents).collect::<Vec<_>>().chunks(1000) {
        let mut p = redis::pipe();
        for t in chunk {
            p.cmd("HSET")
                .arg(format!("trk:t:{}", hex::encode(ih(*t))))
                .arg("tid")
                .arg(t + 1)
                .arg("st")
                .arg(0)
                .arg("acct")
                .arg(1)
                .arg("size")
                .arg(1u64 << 30)
                .ignore();
        }
        let _: () = p.query_async(&mut r).await.expect("seed torrents");
    }
    let _: () = redis::cmd("SET")
        .arg("trk:clients")
        .arg(r#"{"default":"deny","rules":[{"peer_id_prefix":"-qB"}]}"#)
        .query_async(&mut r)
        .await
        .expect("clients");
    let cfg: pt_tracker::Config = toml::from_str(&format!("redis_url = \"{url}\"\n")).expect("cfg");
    let router = pt_tracker::start(cfg).await.expect("start");
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = l.local_addr().expect("addr");
    tokio::spawn(async move {
        axum::serve(l, router.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .expect("serve");
    });

    let done = Arc::new(AtomicU64::new(0));
    let failures = Arc::new(AtomicU64::new(0));
    let start = Instant::now();
    let mut tasks = Vec::new();
    for w in 0..c {
        let (done, failures) = (done.clone(), failures.clone());
        let per = n / c;
        tasks.push(tokio::spawn(async move {
            let mut lat = Vec::with_capacity(usize::try_from(per).unwrap_or(0));
            let s = tokio::net::TcpStream::connect(addr).await.expect("connect");
            let (rd, mut wr) = s.into_split();
            let mut rd = BufReader::new(rd);
            let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (w + 1));
            for _ in 0..per {
                let u = rng.next() % users;
                let t = rng.next() % torrents;
                let mut peer = *b"-qB5100-000000000000";
                peer[8..].copy_from_slice(format!("{:012}", u % 1_000_000_000_000).as_bytes());
                let left = if rng.next().is_multiple_of(3) { 0 } else { 1u64 << 29 };
                let req = format!(
                    "GET /announce/{}?info_hash={}&peer_id={}&port={}&uploaded={}&downloaded={}&left={left}&compact=1 HTTP/1.1\r\nHost: t\r\nUser-Agent: qBittorrent/5.1.0\r\n\r\n",
                    pk(u),
                    enc(&ih(t)),
                    enc(&peer),
                    10_000 + u % 50_000,
                    rng.next() % (1 << 30),
                    rng.next() % (1 << 30),
                );
                let t0 = Instant::now();
                wr.write_all(req.as_bytes()).await.expect("write");
                // 读响应头，按 content-length 读体
                let mut len = 0usize;
                loop {
                    let mut line = String::new();
                    rd.read_line(&mut line).await.expect("read");
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                }
                let mut body = vec![0u8; len];
                rd.read_exact(&mut body).await.expect("body");
                lat.push(t0.elapsed());
                if body.windows(14).any(|x| x == b"failure reason") {
                    failures.fetch_add(1, Ordering::Relaxed);
                }
                done.fetch_add(1, Ordering::Relaxed);
            }
            lat
        }));
    }
    let mut all: Vec<Duration> = Vec::new();
    for t in tasks {
        all.extend(t.await.expect("task"));
    }
    let secs = start.elapsed().as_secs_f64();
    all.sort();
    let pct = |p: f64| {
        let i = ((all.len() as f64 * p) as usize).min(all.len().saturating_sub(1));
        all.get(i).map_or(0.0, |d| d.as_secs_f64() * 1000.0)
    };
    let total = done.load(Ordering::Relaxed);
    println!(
        "{total} announces in {secs:.1}s = {:.0}/s (concurrency {c}); latency ms p50 {:.2} p90 {:.2} p99 {:.2} max {:.2}; failures {}",
        total as f64 / secs,
        pct(0.5),
        pct(0.9),
        pct(0.99),
        pct(1.0),
        failures.load(Ordering::Relaxed)
    );
}
