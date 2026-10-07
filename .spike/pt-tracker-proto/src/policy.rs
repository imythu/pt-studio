//! 准入策略：passkey 鉴权、用户状态、种子白名单、客户端白名单。
//! 生产实现由 tracker-bridge 插件经 Redis 下发（快照 + 变更流），节点本地缓存；
//! 原型用内存实现。

use std::collections::HashMap;
use std::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserStatus {
    Active,
    /// 只允许做种（例如分享率不足被禁止下载）
    LeechDisabled,
    Banned,
}

#[derive(Debug, Clone, Copy)]
pub struct UserCtx {
    pub user_id: u64,
    pub status: UserStatus,
}

#[derive(Debug, Clone, Copy)]
pub struct TorrentCtx {
    pub torrent_id: u64,
}

/// passkey 只接受 32 位小写十六进制。
#[must_use]
pub fn parse_passkey(raw: &str) -> Option<[u8; 32]> {
    let b = raw.as_bytes();
    if b.len() != 32 || !b.iter().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c)) {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(b);
    Some(out)
}

pub trait PasskeyStore: Send + Sync {
    fn lookup(&self, passkey: &[u8; 32]) -> Option<UserCtx>;
}

pub trait TorrentStore: Send + Sync {
    fn lookup(&self, info_hash: &[u8; 20]) -> Option<TorrentCtx>;
}

#[derive(Default)]
pub struct InMemoryPasskeys(pub RwLock<HashMap<[u8; 32], UserCtx>>);

impl PasskeyStore for InMemoryPasskeys {
    fn lookup(&self, passkey: &[u8; 32]) -> Option<UserCtx> {
        self.0.read().expect("poisoned").get(passkey).copied()
    }
}

#[derive(Default)]
pub struct InMemoryTorrents(pub RwLock<HashMap<[u8; 20], TorrentCtx>>);

impl TorrentStore for InMemoryTorrents {
    fn lookup(&self, info_hash: &[u8; 20]) -> Option<TorrentCtx> {
        self.0.read().expect("poisoned").get(info_hash).copied()
    }
}

/// 客户端白名单：peer_id 前缀 + 可选 User-Agent 前缀，二者都需匹配同一条规则。
#[derive(Debug, Clone)]
pub struct ClientRule {
    pub name: String,
    pub peer_id_prefix: Vec<u8>,
    pub user_agent_prefix: Option<String>,
}

#[derive(Default)]
pub struct ClientAllowlist {
    pub rules: Vec<ClientRule>,
}

impl ClientAllowlist {
    #[must_use]
    pub fn check(&self, peer_id: &[u8; 20], user_agent: &str) -> Option<&ClientRule> {
        self.rules.iter().find(|r| {
            peer_id.starts_with(&r.peer_id_prefix)
                && r.user_agent_prefix.as_deref().is_none_or(|p| user_agent.starts_with(p))
        })
    }
}
