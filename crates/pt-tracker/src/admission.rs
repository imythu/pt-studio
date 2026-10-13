//! 准入数据的本地缓存（design/10 §5.5）：启动时从 tracker-bridge 写入的快照键全量加载（`trk:pk:*`、`trk:t:*`、
//! `trk:clients`、`trk:cfg`），之后消费 `trk:sync` 变更流；本地查不到时回源读一次 Redis 并做负缓存。

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use redis::AsyncCommands;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserCtx {
    pub uid: u64,
    /// 0 正常、1 禁止下载、2 不可用
    pub st: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TorrentCtx {
    pub tid: u64,
    /// 0 正常、1 不可用
    pub st: u8,
    /// 是否计账（hybrid 种子的 v2 hash 为 false）
    pub acct: bool,
    pub size: u64,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClientRules {
    #[serde(default)]
    pub default: String,
    #[serde(default)]
    pub rules: Vec<ClientRule>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClientRule {
    pub peer_id_prefix: String,
    #[serde(default = "allow")]
    pub action: String,
}

fn allow() -> String {
    "allow".into()
}

impl ClientRules {
    /// 第一条匹配 peer_id 前缀的规则决定；都不匹配时按 default（缺省拒绝）
    pub fn allows(&self, peer_id: &[u8]) -> bool {
        match self
            .rules
            .iter()
            .find(|r| !r.peer_id_prefix.is_empty() && peer_id.starts_with(r.peer_id_prefix.as_bytes()))
        {
            Some(r) => r.action == "allow",
            None => self.default == "allow",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Params {
    pub interval: u64,
    pub min_interval: u64,
    pub max_peers: usize,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            interval: 1800,
            min_interval: 900,
            max_peers: 50,
        }
    }
}

#[derive(Default)]
struct Inner {
    users: HashMap<String, UserCtx>,
    torrents: HashMap<[u8; 20], TorrentCtx>,
    clients: ClientRules,
    params: Params,
    /// 回源未命中的负缓存
    misses: HashMap<String, Instant>,
}

pub struct Admission {
    inner: RwLock<Inner>,
}

/// 回源未命中的负缓存时长：足够挡住穷举，又让刚注册 / 刚发布（同步尚未到达）的情况很快恢复
const NEGATIVE_TTL: Duration = Duration::from_secs(10);

fn field<T: std::str::FromStr>(h: &HashMap<String, String>, k: &str) -> Option<T> {
    h.get(k).and_then(|v| v.parse().ok())
}

fn user_of(h: &HashMap<String, String>) -> Option<UserCtx> {
    Some(UserCtx {
        uid: field(h, "uid")?,
        st: field(h, "st").unwrap_or(2),
    })
}

fn torrent_of(h: &HashMap<String, String>) -> Option<TorrentCtx> {
    Some(TorrentCtx {
        tid: field(h, "tid")?,
        st: field(h, "st").unwrap_or(1),
        acct: field::<u8>(h, "acct").unwrap_or(1) == 1,
        size: field(h, "size").unwrap_or(0),
    })
}

fn params_of(h: &HashMap<String, String>) -> Params {
    let d = Params::default();
    Params {
        interval: field(h, "interval").unwrap_or(d.interval),
        min_interval: field(h, "min_interval").unwrap_or(d.min_interval),
        max_peers: field(h, "max_peers").unwrap_or(d.max_peers),
    }
}

fn hash_key(hex_hash: &str) -> Option<[u8; 20]> {
    let mut h = [0u8; 20];
    hex::decode_to_slice(hex_hash, &mut h).ok()?;
    Some(h)
}

impl Admission {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Inner::default()),
        }
    }

    fn read(&self) -> std::sync::RwLockReadGuard<'_, Inner> {
        self.inner
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Inner> {
        self.inner
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn params(&self) -> Params {
        self.read().params
    }

    pub fn client_allowed(&self, peer_id: &[u8]) -> bool {
        self.read().clients.allows(peer_id)
    }

    pub fn counts(&self) -> (usize, usize) {
        let g = self.read();
        (g.users.len(), g.torrents.len())
    }

    /// passkey → 用户（本地缓存，未命中回源）
    pub async fn user(&self, conn: &mut redis::aio::ConnectionManager, passkey: &str) -> Option<UserCtx> {
        if let Some(u) = self.read().users.get(passkey) {
            return Some(*u);
        }
        let key = format!("pk:{passkey}");
        if self
            .read()
            .misses
            .get(&key)
            .is_some_and(|t| t.elapsed() < NEGATIVE_TTL)
        {
            return None;
        }
        let h: HashMap<String, String> = conn
            .hgetall(format!("trk:pk:{passkey}"))
            .await
            .unwrap_or_default();
        let u = user_of(&h);
        let mut g = self.write();
        match u {
            Some(u) => {
                g.users.insert(passkey.to_owned(), u);
            }
            None => {
                g.misses.insert(key, Instant::now());
            }
        }
        u
    }

    /// info_hash → 种子（本地缓存，未命中回源）
    pub async fn torrent(
        &self,
        conn: &mut redis::aio::ConnectionManager,
        ih: &[u8; 20],
    ) -> Option<TorrentCtx> {
        if let Some(t) = self.read().torrents.get(ih) {
            return Some(*t);
        }
        let hx = hex::encode(ih);
        let key = format!("t:{hx}");
        if self
            .read()
            .misses
            .get(&key)
            .is_some_and(|t| t.elapsed() < NEGATIVE_TTL)
        {
            return None;
        }
        let h: HashMap<String, String> = conn.hgetall(format!("trk:t:{hx}")).await.unwrap_or_default();
        let t = torrent_of(&h);
        let mut g = self.write();
        match t {
            Some(t) => {
                g.torrents.insert(*ih, t);
            }
            None => {
                g.misses.insert(key, Instant::now());
            }
        }
        t
    }

    /// 全量加载；返回加载前变更流的最新 ID（之后从这里开始消费）
    pub async fn load_all(&self, conn: &mut redis::aio::ConnectionManager) -> redis::RedisResult<String> {
        let last: Option<String> = redis::cmd("XREVRANGE")
            .arg("trk:sync")
            .arg("+")
            .arg("-")
            .arg("COUNT")
            .arg(1)
            .query_async::<Vec<(String, Vec<(String, String)>)>>(conn)
            .await
            .map(|v| v.into_iter().next().map(|e| e.0))?;
        let mut users = HashMap::new();
        let mut torrents = HashMap::new();
        for (pattern, is_user) in [("trk:pk:*", true), ("trk:t:*", false)] {
            let mut cursor: u64 = 0;
            loop {
                let (next, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                    .arg(cursor)
                    .arg("MATCH")
                    .arg(pattern)
                    .arg("COUNT")
                    .arg(1000)
                    .query_async(conn)
                    .await?;
                for k in keys {
                    let h: HashMap<String, String> = conn.hgetall(&k).await?;
                    if is_user {
                        if let (Some(pk), Some(u)) = (k.strip_prefix("trk:pk:"), user_of(&h)) {
                            users.insert(pk.to_owned(), u);
                        }
                    } else if let (Some(ih), Some(t)) =
                        (k.strip_prefix("trk:t:").and_then(hash_key), torrent_of(&h))
                    {
                        torrents.insert(ih, t);
                    }
                }
                if next == 0 {
                    break;
                }
                cursor = next;
            }
        }
        let clients = self.fetch_clients(conn).await;
        let cfg: HashMap<String, String> = conn.hgetall("trk:cfg").await.unwrap_or_default();
        let mut g = self.write();
        g.users = users;
        g.torrents = torrents;
        g.clients = clients;
        g.params = params_of(&cfg);
        g.misses.clear();
        Ok(last.unwrap_or_else(|| "0-0".into()))
    }

    async fn fetch_clients(&self, conn: &mut redis::aio::ConnectionManager) -> ClientRules {
        let s: Option<String> = conn.get("trk:clients").await.unwrap_or(None);
        s.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    /// 应用一条变更：重新读取对应的快照键（删除则移除）
    pub async fn apply(&self, conn: &mut redis::aio::ConnectionManager, kind: &str, key: &str, op: &str) {
        match kind {
            "pk" => {
                let u = if op == "delete" {
                    None
                } else {
                    let h: HashMap<String, String> =
                        conn.hgetall(format!("trk:pk:{key}")).await.unwrap_or_default();
                    user_of(&h)
                };
                let mut g = self.write();
                g.misses.remove(&format!("pk:{key}"));
                match u {
                    Some(u) => g.users.insert(key.to_owned(), u),
                    None => g.users.remove(key),
                };
            }
            "t" => {
                let Some(ih) = hash_key(key) else { return };
                let t = if op == "delete" {
                    None
                } else {
                    let h: HashMap<String, String> =
                        conn.hgetall(format!("trk:t:{key}")).await.unwrap_or_default();
                    torrent_of(&h)
                };
                let mut g = self.write();
                g.misses.remove(&format!("t:{key}"));
                match t {
                    Some(t) => g.torrents.insert(ih, t),
                    None => g.torrents.remove(&ih),
                };
            }
            "clients" => {
                let c = self.fetch_clients(conn).await;
                self.write().clients = c;
            }
            "cfg" => {
                let cfg: HashMap<String, String> = conn.hgetall("trk:cfg").await.unwrap_or_default();
                self.write().params = params_of(&cfg);
            }
            _ => {}
        }
    }

    /// 消费变更流（阻塞读）；游标落后于流的最早条目（被修剪）时重新全量加载
    pub async fn follow(&self, mut conn: redis::aio::ConnectionManager, mut last: String) {
        loop {
            let reply: redis::RedisResult<Option<Vec<StreamReply>>> = redis::cmd("XREAD")
                .arg("COUNT")
                .arg(500)
                .arg("BLOCK")
                .arg(5000)
                .arg("STREAMS")
                .arg("trk:sync")
                .arg(&last)
                .query_async(&mut conn)
                .await;
            match reply {
                Ok(Some(streams)) => {
                    for (_, entries) in streams {
                        for (id, fields) in entries {
                            let f: HashMap<String, String> = fields.into_iter().collect();
                            self.apply(
                                &mut conn,
                                f.get("kind").map_or("", String::as_str),
                                f.get("key").map_or("", String::as_str),
                                f.get("op").map_or("upsert", String::as_str),
                            )
                            .await;
                            last = id;
                        }
                    }
                }
                Ok(None) => {
                    // 超时：检查是否被修剪越过
                    let first: Option<String> = redis::cmd("XRANGE")
                        .arg("trk:sync")
                        .arg("-")
                        .arg("+")
                        .arg("COUNT")
                        .arg(1)
                        .query_async::<Vec<(String, Vec<(String, String)>)>>(&mut conn)
                        .await
                        .ok()
                        .and_then(|v| v.into_iter().next().map(|e| e.0));
                    if first.is_some_and(|f| stream_id_less(&last, &f) && last != "0-0") {
                        tracing::warn!("trk:sync was trimmed past our cursor; reloading");
                        if let Ok(l) = self.load_all(&mut conn).await {
                            last = l;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "reading trk:sync failed");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }
}

/// XREAD 回复：(流名, [(条目 ID, 字段)])
type StreamReply = (String, Vec<(String, Vec<(String, String)>)>);

/// Stream ID 比较（`ms-seq`）
pub fn stream_id_less(a: &str, b: &str) -> bool {
    let p = |s: &str| {
        let (ms, seq) = s.split_once('-').unwrap_or((s, "0"));
        (ms.parse::<u64>().unwrap_or(0), seq.parse::<u64>().unwrap_or(0))
    };
    p(a) < p(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_rules() {
        let r: ClientRules = serde_json::from_str(
            r#"{"default":"deny","rules":[{"peer_id_prefix":"-qB"},{"peer_id_prefix":"-XX","action":"deny"}]}"#,
        )
        .unwrap();
        assert!(r.allows(b"-qB4650-abcdefghijkl"));
        assert!(!r.allows(b"-XX0001-abcdefghijkl"));
        assert!(!r.allows(b"-ZZ0001-abcdefghijkl"));
        assert!(!ClientRules::default().allows(b"-qB"));
    }

    #[test]
    fn parsing() {
        let h: HashMap<String, String> = [("uid", "7"), ("st", "1")]
            .iter()
            .map(|(a, b)| ((*a).into(), (*b).into()))
            .collect();
        assert_eq!(user_of(&h), Some(UserCtx { uid: 7, st: 1 }));
        let h: HashMap<String, String> = [("tid", "9"), ("st", "0"), ("acct", "0"), ("size", "5")]
            .iter()
            .map(|(a, b)| ((*a).into(), (*b).into()))
            .collect();
        assert_eq!(
            torrent_of(&h),
            Some(TorrentCtx {
                tid: 9,
                st: 0,
                acct: false,
                size: 5
            })
        );
        assert!(user_of(&HashMap::new()).is_none());
        assert!(
            stream_id_less("1-5", "2-0") && stream_id_less("2-1", "2-3") && !stream_id_less("3-0", "2-9")
        );
    }
}
