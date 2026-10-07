//! Swarm 后端抽象。原型的实现直接包装 torrust `swarm-coordination-registry` 的 `Registry`，
//! 不经过 `tracker-core`（从而不引入 sqlx / MySQL / PostgreSQL / SQLite 依赖）。
//!
//! 之所以加一层 trait：torrust 的这些包未发布、API 变动频繁、且为 AGPL，
//! 将来若改为自研 swarm（见 10 号文档方案 D），只需替换本模块的实现。

use std::sync::Arc;
use std::time::Duration;

use torrust_info_hash::InfoHash;
use torrust_tracker_primitives::TrackerPolicy;
use torrust_tracker_primitives::peer::Peer;
use torrust_tracker_primitives::swarm_metadata::SwarmMetadata;
use torrust_tracker_swarm_coordination_registry::Registry;

pub struct SwarmReply {
    pub peers: Vec<Arc<Peer>>,
    pub stats: SwarmMetadata,
}

pub struct TorrustSwarm {
    pub registry: Arc<Registry>,
    pub max_peers: usize,
}

impl TorrustSwarm {
    /// upsert / remove 该 peer，并返回给它的 peer 列表与 swarm 统计。
    pub async fn announce(&self, info_hash: &InfoHash, peer: &Peer, numwant: Option<u32>) -> Result<SwarmReply, String> {
        self.registry
            .handle_announcement(info_hash, peer, None)
            .await
            .map_err(|e| e.to_string())?;

        let limit = numwant.map_or(self.max_peers, |n| (n as usize).min(self.max_peers));
        // torrust 的 peers_excluding 按 SocketAddr 排序取前 N 个，不随机、不区分做种者。
        // 原型在此取全部后自行筛选：做种者不需要其他做种者，再做轻量随机化。
        let all = self
            .registry
            .get_peers_peers_excluding(info_hash, peer, usize::MAX)
            .await
            .map_err(|e| e.to_string())?;
        let me_seeder = peer.is_seeder();
        let mut candidates: Vec<Arc<Peer>> = all.into_iter().filter(|p| !(me_seeder && p.is_seeder())).collect();
        rotate_pseudo_random(&mut candidates, peer);
        candidates.truncate(limit);

        let stats = self
            .registry
            .get_swarm_metadata_or_default(info_hash)
            .await
            .map_err(|e| e.to_string())?;
        Ok(SwarmReply { peers: candidates, stats })
    }

    /// 清理不活跃 peer 与空 swarm（替代 tracker-core 的 TorrentsManager）。
    pub async fn cleanup(&self, now: Duration, policy: &TrackerPolicy) {
        let cutoff = now.saturating_sub(Duration::from_secs(u64::from(policy.max_peer_timeout)));
        let _ = self.registry.remove_inactive_peers(cutoff).await;
        let _ = self.registry.remove_peerless_torrents(policy).await;
    }
}

/// 用请求方的端口和时间做一个廉价的轮转起点，避免所有人都拿到同一批 peer。
fn rotate_pseudo_random(v: &mut [Arc<Peer>], me: &Peer) {
    if v.len() > 1 {
        let seed = u64::from(me.peer_addr.port()) ^ (me.updated.subsec_nanos() as u64);
        let k = (seed as usize) % v.len();
        v.rotate_left(k);
    }
}
