//! peer 会话状态（`ps:<info_hash>`，design/10 §5.6）与流量事件（`acct:<user_id % N>`、`u:<uid>:live`，§5.4）。
//! 状态先写、事件后写：两步之间崩溃最多少记一个间隔的流量，不会重复记账。

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::accounting::{Delta, Event, PeerState};

/// 本地状态缓存的键：(info_hash, 用户, peer_id)
pub type SessionKey = ([u8; 20], u64, [u8; 20]);

pub struct Sink {
    pub node: String,
    pub boot: u64,
    pub partitions: u64,
    pub state_ttl: u64,
    seq: AtomicU64,
    states: Mutex<HashMap<SessionKey, PeerState>>,
}

pub struct Fact<'a> {
    pub ts: u64,
    pub uid: u64,
    pub tid: u64,
    pub ih: &'a [u8; 20],
    pub pid: &'a [u8; 20],
    pub ip: String,
    pub port: u16,
    pub event: Event,
    pub up: u64,
    pub down: u64,
    pub left: u64,
    pub ua: &'a str,
}

fn field(k: &SessionKey) -> String {
    format!("{}:{}", k.1, hex::encode(k.2))
}

impl Sink {
    pub fn new(node: String, boot: u64, partitions: u64, state_ttl: u64) -> Self {
        Self {
            node,
            boot,
            partitions: partitions.max(1),
            state_ttl,
            seq: AtomicU64::new(0),
            states: Mutex::new(HashMap::new()),
        }
    }

    fn states(&self) -> std::sync::MutexGuard<'_, HashMap<SessionKey, PeerState>> {
        self.states
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 取上一次状态：本地缓存，未命中读 Redis（重启后恢复）
    pub async fn load(&self, conn: &mut redis::aio::ConnectionManager, k: &SessionKey) -> Option<PeerState> {
        if let Some(s) = self.states().get(k) {
            return Some(*s);
        }
        let v: Option<String> = redis::cmd("HGET")
            .arg(format!("ps:{}", hex::encode(k.0)))
            .arg(field(k))
            .query_async(conn)
            .await
            .unwrap_or(None);
        let s: Option<PeerState> = v.and_then(|v| serde_json::from_str(&v).ok());
        if let Some(s) = s {
            self.states().insert(*k, s);
        }
        s
    }

    /// 写回状态并输出事件（一个 pipeline）
    pub async fn commit(
        &self,
        conn: &mut redis::aio::ConnectionManager,
        k: &SessionKey,
        next: Option<PeerState>,
        delta: &Delta,
        fact: &Fact<'_>,
        emit: bool,
    ) -> redis::RedisResult<()> {
        let ps = format!("ps:{}", hex::encode(k.0));
        let mut p = redis::pipe();
        match next {
            Some(s) => {
                self.states().insert(*k, s);
                p.cmd("HSET")
                    .arg(&ps)
                    .arg(field(k))
                    .arg(serde_json::to_string(&s).unwrap_or_default())
                    .ignore();
                p.cmd("EXPIRE").arg(&ps).arg(self.state_ttl).ignore();
            }
            None => {
                self.states().remove(k);
                p.cmd("HDEL").arg(&ps).arg(field(k)).ignore();
            }
        }
        if emit {
            let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
            let ua: String = fact.ua.chars().take(128).collect();
            p.cmd("XADD")
                .arg(format!("acct:{}", fact.uid % self.partitions))
                .arg("MAXLEN")
                .arg("~")
                .arg(10_000_000)
                .arg("*")
                .arg("v")
                .arg(1)
                .arg("eid")
                .arg(format!("{}:{}:{seq}", self.node, self.boot))
                .arg("ts")
                .arg(fact.ts)
                .arg("uid")
                .arg(fact.uid)
                .arg("tid")
                .arg(fact.tid)
                .arg("ih")
                .arg(hex::encode(fact.ih))
                .arg("pid")
                .arg(hex::encode(fact.pid))
                .arg("ip")
                .arg(&fact.ip)
                .arg("port")
                .arg(fact.port)
                .arg("ev")
                .arg(fact.event.as_str())
                .arg("up")
                .arg(fact.up)
                .arg("down")
                .arg(fact.down)
                .arg("left")
                .arg(fact.left)
                .arg("dup")
                .arg(delta.up)
                .arg("ddown")
                .arg(delta.down)
                .arg("dseed")
                .arg(delta.seed)
                .arg("dleech")
                .arg(delta.leech)
                .arg("flags")
                .arg(delta.flags)
                .arg("ua")
                .arg(ua)
                .ignore();
            let live = format!("u:{}:live", fact.uid);
            for (f, v) in [("up", delta.up), ("down", delta.down), ("seed_time", delta.seed)] {
                if v > 0 {
                    p.cmd("HINCRBY").arg(&live).arg(f).arg(v).ignore();
                }
            }
            if delta.up > 0 || delta.down > 0 || delta.seed > 0 {
                p.cmd("EXPIRE").arg(&live).arg(7 * 86_400).ignore();
            }
        }
        p.query_async::<()>(conn).await
    }

    /// 清理超时的本地状态（Redis 中由 key TTL 回收）
    pub fn cleanup(&self, now: u64, timeout: u64) {
        self.states().retain(|_, s| now.saturating_sub(s.ts) <= timeout);
    }
}
