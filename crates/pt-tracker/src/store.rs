//! Redis 中的 swarm 与会话状态（无状态 tracker：任何节点都能处理任何种子）。
//!
//! 每个种子（`H` = info_hash hex，`{H}` 是 Redis Cluster 哈希标签，保证同一种子的键在同一个 slot）：
//! - `ps:{H}` HASH：field `<uid>:<peer_id_hex>` → 会话状态 JSON（累计值、left、最近时间、是否完成、IP、端口、BEP 7 另一族地址）
//! - `sw:{H}:s` / `sw:{H}:l` ZSET：做种者 / 下载者，member 同上，score 为最近 announce 时间
//! - `sw:{H}:n` STRING：完成次数（scrape 的 downloaded）
//!
//! 每次 announce 一个 Lua 脚本：比较会话状态（CAS，防止两个节点同时处理同一会话造成重复记账）→ 清理超时 peer →
//! 更新状态与集合 → 返回做种 / 下载人数与随机抽取的 peer。流量事件（`acct:*`、`u:*:live`）在脚本成功后写入。

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::accounting::{Delta, Event, PeerState};

/// 存储的会话：增量计算用的状态 + 对外的地址
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stored {
    #[serde(flatten)]
    pub state: PeerState,
    pub ip: String,
    pub port: u16,
    /// BEP 7 `ipv6=` / `ipv4=` 自报的另一族地址（`[v6]:port` / `v4:port`）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
}

/// 抽取到的 peer：地址与 peer_id（非紧凑格式需要）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub ip: IpAddr,
    pub port: u16,
    pub alt: Option<SocketAddr>,
    pub peer_id: [u8; 20],
}

pub struct SwarmView {
    pub seeders: u64,
    pub leechers: u64,
    pub peers: Vec<Peer>,
}

const ANNOUNCE: &str = r"
local cur = redis.call('HGET', KEYS[1], ARGV[1])
if (cur or '') ~= ARGV[2] then return {-1} end
local now = tonumber(ARGV[5])
local cutoff = now - tonumber(ARGV[6])
for _, z in ipairs({KEYS[2], KEYS[3]}) do
  local old = redis.call('ZRANGEBYSCORE', z, '-inf', '(' .. cutoff, 'LIMIT', 0, 200)
  if #old > 0 then
    redis.call('ZREM', z, unpack(old))
    redis.call('HDEL', KEYS[1], unpack(old))
  end
end
if ARGV[3] == '' then
  redis.call('HDEL', KEYS[1], ARGV[1])
  redis.call('ZREM', KEYS[2], ARGV[1])
  redis.call('ZREM', KEYS[3], ARGV[1])
else
  redis.call('HSET', KEYS[1], ARGV[1], ARGV[3])
  if ARGV[4] == '1' then
    redis.call('ZADD', KEYS[2], now, ARGV[1])
    redis.call('ZREM', KEYS[3], ARGV[1])
  else
    redis.call('ZADD', KEYS[3], now, ARGV[1])
    redis.call('ZREM', KEYS[2], ARGV[1])
  end
end
if ARGV[8] == '1' then redis.call('INCR', KEYS[4]) end
local ttl = tonumber(ARGV[9])
for _, k in ipairs(KEYS) do redis.call('EXPIRE', k, ttl) end
local seeders = redis.call('ZCARD', KEYS[2])
local leechers = redis.call('ZCARD', KEYS[3])
local want = tonumber(ARGV[7])
local picked = {}
if ARGV[3] ~= '' and want > 0 then
  local sources = {KEYS[2], KEYS[3]}
  if ARGV[4] == '1' then sources = {KEYS[3]} end
  for _, z in ipairs(sources) do
    if #picked < want then
      local m = redis.call('ZRANDMEMBER', z, want - #picked + 1)
      for _, x in ipairs(m) do
        if x ~= ARGV[1] and #picked < want then picked[#picked + 1] = x end
      end
    end
  end
end
local out = {seeders, leechers}
if #picked > 0 then
  local states = redis.call('HMGET', KEYS[1], unpack(picked))
  for i, x in ipairs(picked) do
    if states[i] then
      out[#out + 1] = x
      out[#out + 1] = states[i]
    end
  end
end
return out
";

pub struct Store {
    pub node: String,
    pub boot: u64,
    pub partitions: u64,
    /// 超时：超过这么久没有 announce 的 peer 被清理
    pub timeout: u64,
    seq: AtomicU64,
    script: redis::Script,
    /// 本节点最近写入的会话原值（省掉一次 HGET；脚本中的 CAS 保证正确，不一致时回源重算）
    recent: std::sync::Mutex<std::collections::HashMap<([u8; 20], String), String>>,
}

/// 本地缓存上限（约 150 B / 条）；超过时整体清空
const RECENT_MAX: usize = 1_000_000;

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

pub struct Write<'a> {
    pub member: &'a str,
    /// 读到的原值（CAS 比较；没有为空串）
    pub expected: &'a str,
    /// 新值；`None` 表示删除（stopped）
    pub next: Option<Stored>,
    pub seeder: bool,
    pub now: u64,
    pub numwant: usize,
    pub first_completed: bool,
}

pub enum Outcome {
    Done(SwarmView),
    /// 会话状态在读与写之间被别处修改：重新读取再算
    Conflict,
}

pub fn member(uid: u64, pid: &[u8; 20]) -> String {
    format!("{uid}:{}", hex::encode(pid))
}

fn keys(ih: &[u8; 20]) -> [String; 4] {
    let h = hex::encode(ih);
    [
        format!("ps:{{{h}}}"),
        format!("sw:{{{h}}}:s"),
        format!("sw:{{{h}}}:l"),
        format!("sw:{{{h}}}:n"),
    ]
}

fn parse_peer(member: &str, state: &str) -> Option<Peer> {
    let (_, pid_hex) = member.split_once(':')?;
    let mut peer_id = [0u8; 20];
    hex::decode_to_slice(pid_hex, &mut peer_id).ok()?;
    let s: Stored = serde_json::from_str(state).ok()?;
    Some(Peer {
        ip: s.ip.parse().ok()?,
        port: s.port,
        alt: s.alt.and_then(|a| a.parse().ok()),
        peer_id,
    })
}

impl Store {
    pub fn new(node: String, boot: u64, partitions: u64, timeout: u64) -> Self {
        Self {
            node,
            boot,
            partitions: partitions.max(1),
            timeout,
            seq: AtomicU64::new(0),
            script: redis::Script::new(ANNOUNCE),
            recent: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    fn recent(&self) -> std::sync::MutexGuard<'_, std::collections::HashMap<([u8; 20], String), String>> {
        self.recent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn parse_state(&self, raw: &str) -> Option<PeerState> {
        serde_json::from_str::<Stored>(raw)
            .ok()
            .map(|s| s.state)
            .filter(|s| self.timeout == 0 || crate::now().saturating_sub(s.ts) <= self.timeout)
    }

    /// 读取会话：先用本地缓存（`fresh = false`），CAS 冲突后用 `fresh = true` 回源
    pub async fn load_cached(
        &self,
        conn: &mut redis::aio::ConnectionManager,
        ih: &[u8; 20],
        member: &str,
        fresh: bool,
    ) -> redis::RedisResult<(String, Option<PeerState>)> {
        if !fresh && let Some(raw) = self.recent().get(&(*ih, member.to_owned())).cloned() {
            let state = self.parse_state(&raw);
            return Ok((raw, state));
        }
        self.load(conn, ih, member).await
    }

    /// 读取会话：(原始值, 解析后的状态)
    pub async fn load(
        &self,
        conn: &mut redis::aio::ConnectionManager,
        ih: &[u8; 20],
        member: &str,
    ) -> redis::RedisResult<(String, Option<PeerState>)> {
        let raw: Option<String> = redis::cmd("HGET")
            .arg(&keys(ih)[0])
            .arg(member)
            .query_async(conn)
            .await?;
        let raw = raw.unwrap_or_default();
        let state = self.parse_state(&raw);
        Ok((raw, state))
    }

    pub async fn announce(
        &self,
        conn: &mut redis::aio::ConnectionManager,
        ih: &[u8; 20],
        w: &Write<'_>,
    ) -> redis::RedisResult<Outcome> {
        let k = keys(ih);
        let next = w
            .next
            .as_ref()
            .map(|s| serde_json::to_string(s).unwrap_or_default())
            .unwrap_or_default();
        let mut inv = self.script.prepare_invoke();
        for key in &k {
            inv.key(key);
        }
        inv.arg(w.member)
            .arg(w.expected)
            .arg(&next)
            .arg(u8::from(w.seeder))
            .arg(w.now)
            .arg(self.timeout)
            .arg(w.numwant)
            .arg(u8::from(w.first_completed))
            .arg(self.timeout * 2);
        let v: Vec<redis::Value> = inv.invoke_async(conn).await?;
        let int = |x: &redis::Value| match x {
            redis::Value::Int(i) => Some(*i),
            _ => None,
        };
        let text = |x: &redis::Value| match x {
            redis::Value::BulkString(b) => String::from_utf8(b.clone()).ok(),
            redis::Value::SimpleString(s) => Some(s.clone()),
            _ => None,
        };
        if v.first().and_then(int) == Some(-1) {
            self.recent().remove(&(*ih, w.member.to_owned()));
            return Ok(Outcome::Conflict);
        }
        {
            let mut recent = self.recent();
            if next.is_empty() {
                recent.remove(&(*ih, w.member.to_owned()));
            } else {
                if recent.len() >= RECENT_MAX {
                    recent.clear();
                }
                recent.insert((*ih, w.member.to_owned()), next.clone());
            }
        }
        let seeders = v.first().and_then(int).unwrap_or(0).max(0).unsigned_abs();
        let leechers = v.get(1).and_then(int).unwrap_or(0).max(0).unsigned_abs();
        let peers = v[2.min(v.len())..]
            .chunks(2)
            .filter_map(|c| parse_peer(&text(c.first()?)?, &text(c.get(1)?)?))
            .collect();
        Ok(Outcome::Done(SwarmView {
            seeders,
            leechers,
            peers,
        }))
    }

    /// (做种, 完成次数, 下载中)，只计未超时的 peer
    pub async fn stats(
        &self,
        conn: &mut redis::aio::ConnectionManager,
        ih: &[u8; 20],
    ) -> redis::RedisResult<(u64, u64, u64)> {
        let k = keys(ih);
        let cutoff = crate::now().saturating_sub(self.timeout);
        let (s, n, l): (u64, Option<u64>, u64) = redis::pipe()
            .cmd("ZCOUNT")
            .arg(&k[1])
            .arg(cutoff)
            .arg("+inf")
            .cmd("GET")
            .arg(&k[3])
            .cmd("ZCOUNT")
            .arg(&k[2])
            .arg(cutoff)
            .arg("+inf")
            .query_async(conn)
            .await?;
        Ok((s, n.unwrap_or(0), l))
    }

    /// 流量事件：`acct:<uid % N>` 与实时计数
    pub async fn emit(
        &self,
        conn: &mut redis::aio::ConnectionManager,
        delta: &Delta,
        fact: &Fact<'_>,
    ) -> redis::RedisResult<()> {
        let seq = self.seq.fetch_add(1, Ordering::Relaxed) + 1;
        let ua: String = fact.ua.chars().take(128).collect();
        let mut p = redis::pipe();
        let xadd = p.cmd("XADD");
        xadd
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
            .arg(ua);
        if delta.dropped_up > 0 || delta.dropped_down > 0 {
            // 超速未计入的量（v1 的可选字段，旧版 accounting 忽略）
            xadd.arg("xup").arg(delta.dropped_up).arg("xdown").arg(delta.dropped_down);
        }
        xadd.ignore();
        let live = format!("u:{}:live", fact.uid);
        for (f, v) in [("up", delta.up), ("down", delta.down), ("seed_time", delta.seed)] {
            if v > 0 {
                p.cmd("HINCRBY").arg(&live).arg(f).arg(v).ignore();
            }
        }
        if delta.up > 0 || delta.down > 0 || delta.seed > 0 {
            p.cmd("EXPIRE").arg(&live).arg(7 * 86_400).ignore();
        }
        p.query_async::<()>(conn).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_roundtrip_and_peer_parse() {
        let s = Stored {
            state: PeerState {
                up: 1,
                down: 2,
                left: 3,
                ts: 4,
                completed: false,
            },
            ip: "10.0.0.1".into(),
            port: 6881,
            alt: Some("[2001:4860::1]:6882".into()),
        };
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Stored>(&j).unwrap(), s);
        let pid = [b'a'; 20];
        let p = parse_peer(&member(7, &pid), &j).unwrap();
        assert_eq!(
            (p.ip.to_string(), p.port, p.peer_id, p.alt.map(|a| a.to_string())),
            (
                "10.0.0.1".to_owned(),
                6881,
                pid,
                Some("[2001:4860::1]:6882".to_owned())
            )
        );
        // 旧格式（没有 alt）照常解析
        let old = r#"{"up":0,"down":0,"left":0,"ts":1,"completed":false,"ip":"1.2.3.4","port":1}"#;
        assert_eq!(parse_peer(&member(7, &pid), old).unwrap().alt, None);
        assert!(parse_peer("bad", &j).is_none());
        assert_eq!(keys(&[0xab; 20])[1], format!("sw:{{{}}}:s", "ab".repeat(20)));
    }
}
