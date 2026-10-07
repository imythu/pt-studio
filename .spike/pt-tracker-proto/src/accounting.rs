//! 增量计算（Δup / Δdown / Δseed_time）。
//!
//! torrust 的 swarm 以 `SocketAddr` 为键保存 peer，且 peer 中没有用户身份，
//! `handle_announcement` 也不返回旧值，所以“上一次累计值”必须由 pt-tracker
//! 自己保存，键为 (user_id, info_hash, peer_id)。生产实现：节点内存 + Redis 写穿
//! （`ps:{info_hash}:{user_id}:{peer_id}`，带 TTL），节点重启或分片迁移时从 Redis 读回。

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;

/// 一个 (user, torrent, peer_id) 会话上一次被接受的累计值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PeerState {
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
    /// 上一次 announce 的时间（unix 秒）
    pub last_announce: u64,
    /// 该会话是否已经上报过 completed（防止重复计 snatch）
    pub completed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct PeerKey {
    pub user_id: u64,
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    Started,
    Stopped,
    Completed,
    None,
}

/// 本次 announce 上报的原始累计值。
#[derive(Debug, Clone, Copy)]
pub struct Report {
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
    pub event: Event,
    pub now: u64,
}

/// 计算结果附带的“信号位”，tracker 不做判定，只如实上报给下游。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Flags {
    /// 节点上找不到上一次状态（首次出现）
    pub first_seen: bool,
    /// 找不到上一次状态且 event != started：可能是状态丢失（Redis 也丢）或客户端异常
    pub baseline_reset: bool,
    /// 累计值回退（客户端重启但没有发 started，或伪造）
    pub upload_regressed: bool,
    pub download_regressed: bool,
    /// 本次 completed 是该会话第一次 completed
    pub first_completed: bool,
    /// 距上次 announce 的间隔超过上限，做种时长被截断
    pub seed_time_capped: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Delta {
    pub up: u64,
    pub down: u64,
    pub seed_time: u64,
    pub leech_time: u64,
    pub flags: Flags,
}

#[derive(Debug, Clone, Copy)]
pub struct DeltaPolicy {
    /// 两次 announce 之间计入时长的上限（秒），通常取 max_peer_timeout
    pub max_interval_secs: u64,
}

/// 纯函数：给定上一次状态与本次上报，计算增量与新状态。
/// 返回的新状态为 `None` 表示应当删除（event=stopped）。
#[must_use]
pub fn compute(prev: Option<&PeerState>, r: &Report, policy: &DeltaPolicy) -> (Delta, Option<PeerState>) {
    let mut d = Delta::default();

    match prev {
        None => {
            d.flags.first_seen = true;
            if r.event == Event::Started {
                // 按 BEP 3，started 时的累计值是“本会话”的，正常应为 0；
                // 若非 0（部分客户端重启后续报），如实计入。
                d.up = r.uploaded;
                d.down = r.downloaded;
            } else {
                // 状态丢失：只建立基线，不计增量，避免重复计数。
                d.flags.baseline_reset = true;
            }
        }
        Some(p) => {
            if r.uploaded >= p.uploaded {
                d.up = r.uploaded - p.uploaded;
            } else {
                d.flags.upload_regressed = true;
            }
            if r.downloaded >= p.downloaded {
                d.down = r.downloaded - p.downloaded;
            } else {
                d.flags.download_regressed = true;
            }
            let elapsed = r.now.saturating_sub(p.last_announce);
            let counted = if elapsed > policy.max_interval_secs {
                d.flags.seed_time_capped = true;
                policy.max_interval_secs
            } else {
                elapsed
            };
            // 时长归属于“上一次”的角色：上次是做种者，这段时间就算做种时长。
            if p.left == 0 {
                d.seed_time = counted;
            } else {
                d.leech_time = counted;
            }
        }
    }

    let was_completed = prev.is_some_and(|p| p.completed);
    if r.event == Event::Completed && !was_completed {
        d.flags.first_completed = true;
    }

    let next = if r.event == Event::Stopped {
        None
    } else {
        Some(PeerState {
            uploaded: r.uploaded,
            downloaded: r.downloaded,
            left: r.left,
            last_announce: r.now,
            completed: was_completed || r.event == Event::Completed,
        })
    };
    (d, next)
}

/// 上一次累计值的存储。生产实现为 “本地分片 HashMap + Redis 写穿 / 读回”。
pub trait PeerStateStore: Send + Sync {
    /// 原子地读出旧状态、计算、写回新状态（同一 key 的并发 announce 必须串行）。
    fn update(&self, key: PeerKey, f: &mut dyn FnMut(Option<&PeerState>) -> Option<PeerState>);
    fn len(&self) -> usize;
}

/// 原型用的内存实现（按 key 哈希分 64 个锁分片）。
pub struct InMemoryPeerStateStore {
    shards: Vec<Mutex<HashMap<PeerKey, PeerState>>>,
}

impl Default for InMemoryPeerStateStore {
    fn default() -> Self {
        Self { shards: (0..64).map(|_| Mutex::new(HashMap::new())).collect() }
    }
}

impl InMemoryPeerStateStore {
    fn shard(&self, key: &PeerKey) -> &Mutex<HashMap<PeerKey, PeerState>> {
        let idx = (key.info_hash[0] as usize ^ key.peer_id[19] as usize ^ key.user_id as usize) % self.shards.len();
        &self.shards[idx]
    }
}

impl PeerStateStore for InMemoryPeerStateStore {
    fn update(&self, key: PeerKey, f: &mut dyn FnMut(Option<&PeerState>) -> Option<PeerState>) {
        let mut map = self.shard(&key).lock().expect("poisoned");
        let next = f(map.get(&key));
        match next {
            Some(s) => {
                map.insert(key, s);
            }
            None => {
                map.remove(&key);
            }
        }
    }

    fn len(&self) -> usize {
        self.shards.iter().map(|s| s.lock().expect("poisoned").len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: DeltaPolicy = DeltaPolicy { max_interval_secs: 3600 };

    fn rep(up: u64, down: u64, left: u64, event: Event, now: u64) -> Report {
        Report { uploaded: up, downloaded: down, left, event, now }
    }

    #[test]
    fn first_started_is_baseline_with_reported_values() {
        let (d, s) = compute(None, &rep(0, 0, 100, Event::Started, 1000), &P);
        assert_eq!((d.up, d.down), (0, 0));
        assert!(d.flags.first_seen && !d.flags.baseline_reset);
        assert_eq!(s.unwrap().last_announce, 1000);
    }

    #[test]
    fn unknown_peer_without_started_only_sets_baseline() {
        let (d, s) = compute(None, &rep(5000, 7000, 0, Event::None, 1000), &P);
        assert_eq!((d.up, d.down, d.seed_time), (0, 0, 0));
        assert!(d.flags.baseline_reset);
        assert_eq!(s.unwrap().uploaded, 5000);
    }

    #[test]
    fn regular_update_and_leech_time() {
        let prev = PeerState { uploaded: 100, downloaded: 200, left: 50, last_announce: 1000, completed: false };
        let (d, _) = compute(Some(&prev), &rep(150, 260, 0, Event::Completed, 1900), &P);
        assert_eq!((d.up, d.down, d.leech_time, d.seed_time), (50, 60, 900, 0));
        assert!(d.flags.first_completed);
    }

    #[test]
    fn seed_time_is_capped() {
        let prev = PeerState { uploaded: 0, downloaded: 0, left: 0, last_announce: 0, completed: true };
        let (d, _) = compute(Some(&prev), &rep(0, 0, 0, Event::None, 10_000), &P);
        assert_eq!(d.seed_time, 3600);
        assert!(d.flags.seed_time_capped);
        assert!(!d.flags.first_completed);
    }

    #[test]
    fn regression_yields_zero_and_flag_and_rebaselines() {
        let prev = PeerState { uploaded: 1000, downloaded: 1000, left: 0, last_announce: 0, completed: true };
        let (d, s) = compute(Some(&prev), &rep(10, 2000, 0, Event::None, 60), &P);
        assert_eq!((d.up, d.down), (0, 1000));
        assert!(d.flags.upload_regressed && !d.flags.download_regressed);
        assert_eq!(s.unwrap().uploaded, 10);
    }

    #[test]
    fn stopped_counts_then_removes() {
        let prev = PeerState { uploaded: 10, downloaded: 0, left: 0, last_announce: 0, completed: true };
        let (d, s) = compute(Some(&prev), &rep(30, 0, 0, Event::Stopped, 60), &P);
        assert_eq!((d.up, d.seed_time), (20, 60));
        assert!(s.is_none());
    }

    #[test]
    fn duplicate_completed_is_not_first() {
        let prev = PeerState { uploaded: 0, downloaded: 0, left: 0, last_announce: 0, completed: true };
        let (d, _) = compute(Some(&prev), &rep(0, 0, 0, Event::Completed, 1), &P);
        assert!(!d.flags.first_completed);
    }
}
