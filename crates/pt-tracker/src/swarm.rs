//! 内存 swarm：每个 info_hash 的 peer 集合（键为 (用户, peer_id)），超时清理，完成数统计。

use std::collections::HashMap;
use std::sync::Mutex;

use crate::protocol::PeerAddr;

#[derive(Debug, Clone, Copy)]
struct Peer {
    addr: PeerAddr,
    seeder: bool,
    updated: u64,
}

#[derive(Default)]
struct Swarm {
    peers: HashMap<(u64, [u8; 20]), Peer>,
    /// 完成次数（scrape 的 downloaded）
    downloaded: u64,
}

#[derive(Default)]
pub struct Swarms {
    inner: Mutex<HashMap<[u8; 20], Swarm>>,
}

pub struct AnnounceResult {
    pub complete: u64,
    pub incomplete: u64,
    pub peers: Vec<PeerAddr>,
}

impl Swarms {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<[u8; 20], Swarm>> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 更新本 peer 并返回其他 peer（做种者只拿下载者；下载者优先拿做种者）
    #[allow(clippy::too_many_arguments)]
    pub fn announce(
        &self,
        ih: [u8; 20],
        uid: u64,
        peer_id: [u8; 20],
        addr: PeerAddr,
        seeder: bool,
        stopped: bool,
        completed: bool,
        now: u64,
        numwant: usize,
    ) -> AnnounceResult {
        let mut g = self.lock();
        let sw = g.entry(ih).or_default();
        let me = (uid, peer_id);
        if stopped {
            sw.peers.remove(&me);
        } else {
            sw.peers.insert(
                me,
                Peer {
                    addr,
                    seeder,
                    updated: now,
                },
            );
        }
        if completed {
            sw.downloaded += 1;
        }
        let complete = sw.peers.values().filter(|p| p.seeder).count() as u64;
        let incomplete = sw.peers.len() as u64 - complete;
        let mut peers: Vec<&Peer> = sw
            .peers
            .iter()
            .filter(|(k, p)| **k != me && p.addr != addr && !(seeder && p.seeder))
            .map(|(_, p)| p)
            .collect();
        // 下载者优先连做种者；同类按最近活跃排序
        peers.sort_by_key(|p| (!p.seeder, std::cmp::Reverse(p.updated)));
        let peers = if stopped {
            vec![]
        } else {
            peers.into_iter().take(numwant).map(|p| p.addr).collect()
        };
        AnnounceResult {
            complete,
            incomplete,
            peers,
        }
    }

    /// (做种, 完成次数, 下载中)
    pub fn stats(&self, ih: &[u8; 20]) -> (u64, u64, u64) {
        let g = self.lock();
        g.get(ih).map_or((0, 0, 0), |sw| {
            let c = sw.peers.values().filter(|p| p.seeder).count() as u64;
            (c, sw.downloaded, sw.peers.len() as u64 - c)
        })
    }

    /// 清理超时 peer；返回 (删除的 peer 数, 剩余 swarm 数)
    pub fn cleanup(&self, now: u64, timeout: u64) -> (usize, usize) {
        let mut g = self.lock();
        let mut removed = 0;
        for sw in g.values_mut() {
            let before = sw.peers.len();
            sw.peers.retain(|_, p| now.saturating_sub(p.updated) <= timeout);
            removed += before - sw.peers.len();
        }
        g.retain(|_, sw| !sw.peers.is_empty() || sw.downloaded > 0);
        (removed, g.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(n: u8) -> PeerAddr {
        PeerAddr {
            ip: std::net::IpAddr::from([10, 0, 0, n]),
            port: 6881,
        }
    }

    #[test]
    fn peers_and_stats() {
        let s = Swarms::default();
        let ih = [1u8; 20];
        let r = s.announce(ih, 1, [1; 20], addr(1), true, false, false, 100, 50);
        assert_eq!((r.complete, r.incomplete, r.peers.len()), (1, 0, 0));
        let r = s.announce(ih, 2, [2; 20], addr(2), false, false, false, 101, 50);
        assert_eq!((r.complete, r.incomplete, r.peers), (1, 1, vec![addr(1)]));
        // 做种者不拿别的做种者
        let r = s.announce(ih, 3, [3; 20], addr(3), true, false, false, 102, 50);
        assert_eq!(r.peers, vec![addr(2)]);
        // 完成：计数
        s.announce(ih, 2, [2; 20], addr(2), true, false, true, 103, 50);
        assert_eq!(s.stats(&ih), (3, 1, 0));
        // 停止：移出
        s.announce(ih, 3, [3; 20], addr(3), true, true, false, 104, 50);
        assert_eq!(s.stats(&ih).0, 2);
        // 超时清理
        assert_eq!(s.cleanup(100 + 3000, 2700).0, 2);
    }
}
