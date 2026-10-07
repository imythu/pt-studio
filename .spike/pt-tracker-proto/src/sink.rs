//! 流量事件出口。生产实现是 Redis（同一 pipeline 中 XADD + HINCRBY），
//! 原型环境没有 redis-server，因此用 trait 抽象 + 日志 / 内存实现，
//! 并把“将要执行的 Redis 命令”原样渲染出来，便于核对键名与分区规则。

use std::sync::Mutex;

use serde::Serialize;

use crate::accounting::{Delta, Event};

/// 一次 announce 产生的流量事件（tracker 只报事实，不含任何倍率 / 促销规则）。
#[derive(Debug, Clone, Serialize)]
pub struct TrafficEvent {
    /// 格式版本，下游按版本解析
    pub v: u8,
    /// 产生事件的节点 + 节点内单调序号，用于排查与去重
    pub node: String,
    pub seq: u64,
    pub ts: u64,
    pub user_id: u64,
    pub torrent_id: u64,
    pub info_hash: String,
    pub peer_id: String,
    pub ip: String,
    pub port: u16,
    pub event: Event,
    /// 本次上报的原始累计值（便于下游复核 / 反作弊）
    pub uploaded: u64,
    pub downloaded: u64,
    pub left: u64,
    pub delta: Delta,
    pub user_agent: String,
}

pub trait TrafficSink: Send + Sync {
    fn emit(&self, ev: &TrafficEvent);
}

/// 根据分区数计算 Redis Stream 键：`acct:{user_id % N}`
#[must_use]
pub fn stream_key(user_id: u64, partitions: u64) -> String {
    format!("acct:{}", user_id % partitions)
}

/// 渲染生产环境中会在同一个 pipeline 里执行的 Redis 命令。
#[must_use]
pub fn render_redis_commands(ev: &TrafficEvent, partitions: u64) -> Vec<String> {
    let mut cmds = vec![format!(
        "XADD {} MAXLEN ~ 10000000 * v {} node {} seq {} ts {} uid {} tid {} ih {} pid {} ev {:?} up {} down {} left {} dup {} ddown {} dseed {} dleech {} flags {}",
        stream_key(ev.user_id, partitions),
        ev.v,
        ev.node,
        ev.seq,
        ev.ts,
        ev.user_id,
        ev.torrent_id,
        ev.info_hash,
        ev.peer_id,
        ev.event,
        ev.uploaded,
        ev.downloaded,
        ev.left,
        ev.delta.up,
        ev.delta.down,
        ev.delta.seed_time,
        ev.delta.leech_time,
        serde_json::to_string(&ev.delta.flags).unwrap_or_default(),
    )];
    if ev.delta.up > 0 {
        cmds.push(format!("HINCRBY u:{}:live up {}", ev.user_id, ev.delta.up));
    }
    if ev.delta.down > 0 {
        cmds.push(format!("HINCRBY u:{}:live down {}", ev.user_id, ev.delta.down));
    }
    if ev.delta.seed_time > 0 {
        cmds.push(format!("HINCRBY u:{}:live seed_time {}", ev.user_id, ev.delta.seed_time));
    }
    cmds
}

/// 写日志的实现：每个事件一行 JSON + 渲染出的 Redis 命令。
pub struct LogSink {
    pub partitions: u64,
}

impl TrafficSink for LogSink {
    fn emit(&self, ev: &TrafficEvent) {
        tracing::info!(target: "traffic", event = %serde_json::to_string(ev).unwrap_or_default(), "traffic event");
        for c in render_redis_commands(ev, self.partitions) {
            tracing::info!(target: "traffic", "redis> {c}");
        }
    }
}

/// 内存实现：保存所有事件，供 /debug/events 查看。
#[derive(Default)]
pub struct MemorySink {
    pub events: Mutex<Vec<TrafficEvent>>,
}

impl TrafficSink for MemorySink {
    fn emit(&self, ev: &TrafficEvent) {
        self.events.lock().expect("poisoned").push(ev.clone());
    }
}

/// 组合多个 sink。
pub struct Fanout(pub Vec<std::sync::Arc<dyn TrafficSink>>);

impl TrafficSink for Fanout {
    fn emit(&self, ev: &TrafficEvent) {
        for s in &self.0 {
            s.emit(ev);
        }
    }
}
