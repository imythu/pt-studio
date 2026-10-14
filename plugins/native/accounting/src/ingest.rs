//! 分区 worker（accounting 03 §1.1～§1.6）：读 `acct:<p>` 微批 → 解析 → 去重 → 聚合 → 落库 → 关闭窗口 → 游标 CAS。
//! 整个微批与游标在同一事务中提交，因此对数据库的效果恰好一次。
//!
//! 实现说明（与 03 的差异）：落库使用 `unnest` 多行语句代替 COPY + 暂存表；peers 不维护小时槽（快照在后续迭代）；
//! 基础信号、实时计数扣减（live）、流修剪在后续迭代实现。

// 批量写入以“列向量元组”组织参数（对应 unnest 的各列），类型较长是有意的
#![allow(clippy::type_complexity)]

use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::Mutex,
};

use chrono::{DateTime, Utc};
use pt_plugin_api::{Ctx, NewEvent, PResult, PluginError, PollOutcome, json};
use redis::Value;
use sqlx::{PgConnection, Row};

use crate::{Config, rates};

pub const FLAG_FIRST_SEEN: u32 = 1;
pub const FLAG_FIRST_COMPLETED: u32 = 1 << 4;

/// 需要写 snatch 的条目（03 §1.2）：纯做种续报只进入 peers 的小时槽
pub fn touches(e: &Entry) -> bool {
    e.dup > 0
        || e.ddown > 0
        || e.dleech > 0
        || e.flags & (FLAG_FIRST_SEEN | FLAG_FIRST_COMPLETED) != 0
        || matches!(e.ev, Ev::Started | Ev::Completed | Ev::Stopped)
}
const MAX_DELTA: u64 = 1 << 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct StreamId(pub u64, pub u64);

impl StreamId {
    pub fn parse(s: &str) -> Option<Self> {
        let (a, b) = s.split_once('-')?;
        Some(Self(a.parse().ok()?, b.parse().ok()?))
    }
}

impl std::fmt::Display for StreamId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.0, self.1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ev {
    Started,
    Stopped,
    Completed,
    None,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub id: StreamId,
    pub eid: String,
    pub ts: i64,
    pub uid: u64,
    pub tid: u64,
    pub pid: [u8; 20],
    pub ev: Ev,
    pub left: u64,
    pub dup: u64,
    pub ddown: u64,
    pub dseed: u64,
    pub dleech: u64,
    pub flags: u32,
    pub client: String,
}

/// 字段解析（§1.1.2）。返回拒收原因代号。
pub fn parse(
    id: StreamId,
    f: &HashMap<String, String>,
    partitions: u32,
    part: u32,
) -> Result<Entry, &'static str> {
    let get = |k: &str| f.get(k).map(String::as_str);
    let num = |k: &str| get(k).and_then(|v| v.parse::<u64>().ok());
    if get("v") != Some("1") {
        return Err("unsupported_version");
    }
    let uid = num("uid").ok_or("uid")?;
    if uid % u64::from(partitions.max(1)) != u64::from(part) {
        return Err("wrong_partition");
    }
    let tid = num("tid").filter(|t| *t > 0).ok_or("tid")?;
    let pid_hex = get("pid").filter(|p| p.len() == 40).ok_or("pid")?;
    let mut pid = [0u8; 20];
    hex::decode_to_slice(pid_hex, &mut pid).map_err(|_| "pid")?;
    let ev = match get("ev").map(str::to_ascii_lowercase).as_deref() {
        Some("started") => Ev::Started,
        Some("stopped") => Ev::Stopped,
        Some("completed") => Ev::Completed,
        Some("none" | "") => Ev::None,
        _ => return Err("ev"),
    };
    let (Some(left), Some(dup), Some(ddown), Some(dseed), Some(dleech)) =
        (num("left"), num("dup"), num("ddown"), num("dseed"), num("dleech"))
    else {
        return Err("number");
    };
    // 评审 R1 F-03：流量增量超界只丢弃流量字段，保留时长与完成标记
    let (dup, ddown) = if dup > MAX_DELTA || ddown > MAX_DELTA {
        (0, 0)
    } else {
        (dup, ddown)
    };
    let (dseed, dleech) = if dseed + dleech > 86_400 {
        (0, 0)
    } else {
        (dseed, dleech)
    };
    // 未知标志位忽略
    let flags = get("flags").and_then(|v| v.parse::<u32>().ok()).ok_or("flags")?;
    let ts = get("ts").and_then(|v| v.parse::<i64>().ok()).ok_or("ts")?;
    let eid = get("eid")
        .filter(|e| (1..=128).contains(&e.len()))
        .ok_or("eid")?
        .to_owned();
    let client: String = pid[..8]
        .iter()
        .map(|b| if b.is_ascii_graphic() { *b as char } else { '?' })
        .collect();
    Ok(Entry {
        id,
        eid,
        ts,
        uid,
        tid,
        pid,
        ev,
        left,
        dup,
        ddown,
        dseed,
        dleech,
        flags,
        client,
    })
}

fn bulk(v: &Value) -> Option<String> {
    match v {
        Value::BulkString(b) => String::from_utf8(b.clone()).ok(),
        Value::SimpleString(s) => Some(s.clone()),
        _ => None,
    }
}

/// XREAD 回复 → [(id, 字段)]
pub fn xread_entries(v: &Value) -> Vec<(StreamId, HashMap<String, String>)> {
    let mut out = Vec::new();
    let streams: Vec<&Value> = match v {
        Value::Array(a) => a.iter().collect(),
        Value::Map(m) => m.iter().map(|(_, v)| v).collect(),
        _ => vec![],
    };
    for st in streams {
        let list = match st {
            Value::Array(pair) if pair.len() == 2 => &pair[1],
            other => other,
        };
        let Value::Array(items) = list else { continue };
        for it in items {
            let Value::Array(parts) = it else { continue };
            if parts.len() != 2 {
                continue;
            }
            let (Some(id), Value::Array(kv)) = (bulk(&parts[0]).and_then(|s| StreamId::parse(&s)), &parts[1])
            else {
                continue;
            };
            let fields = kv
                .chunks(2)
                .filter_map(|c| Some((bulk(&c[0])?, bulk(c.get(1)?)?)))
                .collect();
            out.push((id, fields));
        }
    }
    out
}

/// 每分区最近 eid（进程内去重窗口）
#[derive(Debug, Default)]
pub struct EidCache {
    parts: Mutex<HashMap<u32, (HashSet<String>, VecDeque<String>)>>,
}

impl EidCache {
    const CAP: usize = 200_000;

    /// 返回 true 表示首次出现
    pub fn insert(&self, part: u32, eid: &str) -> bool {
        let mut g = self
            .parts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (set, q) = g.entry(part).or_default();
        if set.contains(eid) {
            return false;
        }
        set.insert(eid.to_owned());
        q.push_back(eid.to_owned());
        if q.len() > Self::CAP
            && let Some(old) = q.pop_front()
        {
            set.remove(&old);
        }
        true
    }

    /// 回滚的批次：从缓存中撤销（下次重读时不能被误判为重复）
    pub fn forget(&self, part: u32, eids: &[String]) {
        let mut g = self
            .parts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((set, _)) = g.get_mut(&part) {
            for e in eids {
                set.remove(e);
            }
        }
    }
}

pub fn win(ms: i64, window_secs: i64) -> i64 {
    (ms / 1000).div_euclid(window_secs) * window_secs
}

#[derive(Debug, Default, Clone)]
struct PairAgg {
    up_permille: u32,
    down_permille: u32,
    rule_ids: Vec<u64>,
    up_raw: u64,
    down_raw: u64,
    up: u64,
    down: u64,
    seed: u64,
    leech: u64,
    completed_at: Option<i64>,
    last_ts: i64,
    first_ts: i64,
}

#[derive(Debug, Clone)]
struct PeerAgg {
    is_seeder: bool,
    active: bool,
    client: String,
    last_ts: i64,
    /// 本微批的做种秒数（写入小时槽，由小时快照折叠进 snatches，03 §1.9）
    seed: u64,
}

fn ts_dt(secs: i64) -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(secs, 0).unwrap_or_else(Utc::now)
}

fn i(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// 空闲分区落库水位的最小间隔
const IDLE_PERSIST_MS: i64 = 5_000;

/// 一次轮询（kernel 在返回 Ok 时提交工作单元）。
pub async fn poll(state: &crate::State, ctx: &Ctx, part: u32, fence: u64) -> PResult<PollOutcome> {
    let cfg = state.cfg()?;
    let fence_i = i64::try_from(fence).unwrap_or(i64::MAX);
    let part_i = i16::try_from(part).map_err(|_| PluginError::internal("partition out of range"))?;
    let mut conn = ctx.redis.global("acct")?;
    let partitions: u32 = redis::cmd("HGET")
        .arg("acct:_meta")
        .arg("partitions")
        .query_async::<Option<u32>>(&mut conn)
        .await?
        .unwrap_or(cfg.default_partitions);
    state.refresh_rates(ctx).await?;

    let mut c = ctx.sql.conn().await?;
    sqlx::query("INSERT INTO stream_cursors (part) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(part_i)
        .execute(&mut *c)
        .await?;
    let cur =
        sqlx::query("SELECT last_id, watermark_ms, fence FROM stream_cursors WHERE part = $1 FOR UPDATE")
            .bind(part_i)
            .fetch_one(&mut *c)
            .await?;
    let last_id: String = cur.get(0);
    let watermark: i64 = cur.get(1);
    if cur.get::<i64, _>(2) > fence_i {
        return Err(PluginError::new(
            "accounting.lease_lost",
            "a newer lease holder owns this partition",
        )
        .retryable());
    }
    // 只在换租约时写：空闲分区每次轮询都 UPDATE 会把小机器的 Postgres 压满
    sqlx::query("UPDATE stream_cursors SET fence = $2 WHERE part = $1 AND fence <> $2")
        .bind(part_i)
        .bind(fence_i)
        .execute(&mut *c)
        .await?;

    let stream = format!("acct:{part}");
    let reply: Value = redis::cmd("XREAD")
        .arg("COUNT")
        .arg(cfg.batch_max_entries)
        .arg("STREAMS")
        .arg(&stream)
        .arg(&last_id)
        .query_async(&mut conn)
        .await?;
    let raw = xread_entries(&reply);
    let w = i64::from(cfg.window_secs);
    if raw.is_empty() {
        // 读空：以 Redis 时间推进水位并关闭已完整的窗口。
        // 落库的水位只供快照判断是否迟到，按 IDLE_PERSIST_MS 节流即可，窗口关闭用内存中的 wm
        let (secs, micros): (i64, i64) = redis::cmd("TIME").query_async(&mut conn).await?;
        let now_ms = secs * 1000 + micros / 1000;
        let wm = watermark.max(now_ms - 1);
        sqlx::query(
            "UPDATE stream_cursors SET watermark_ms = $2, idle_since = coalesce(idle_since, now()), updated_at = now()
              WHERE part = $1 AND (idle_since IS NULL OR watermark_ms <= $2 - $3)",
        )
        .bind(part_i)
        .bind(wm)
        .bind(IDLE_PERSIST_MS)
        .execute(&mut *c)
        .await?;
        drop(c);
        close_windows(ctx, cfg, part_i, wm).await?;
        return Ok(PollOutcome::Idle {
            sleep_ms: cfg.idle_sleep_ms,
        });
    }

    // 微批不跨结算窗口
    let first_win = win(i(raw[0].0.0), w);
    let mut entries = Vec::new();
    let mut rejected = Vec::new();
    let mut last = raw[0].0;
    for (id, fields) in &raw {
        if win(i(id.0), w) != first_win {
            break;
        }
        last = *id;
        match parse(*id, fields, partitions, part) {
            Ok(e) => entries.push(e),
            Err(why) => {
                let mut f = fields.clone();
                for k in ["ip", "port", "ua"] {
                    f.remove(k);
                }
                rejected.push((id.to_string(), why, serde_json::to_value(f)?));
            }
        }
    }
    let mut fresh = Vec::with_capacity(entries.len());
    let mut new_eids = Vec::new();
    for e in entries {
        if state.eids.insert(part, &e.eid) {
            new_eids.push(e.eid.clone());
            fresh.push(e);
        }
    }
    let r = store(ctx, &mut c, state, cfg, part_i, first_win, &fresh, &rejected).await;
    if let Err(e) = r {
        state.eids.forget(part, &new_eids);
        return Err(e);
    }
    let n = sqlx::query(
        "UPDATE stream_cursors SET last_id = $3, last_ms = $4, watermark_ms = greatest(watermark_ms, $4), idle_since = NULL,
                committed_at = now(), entries_total = entries_total + $5, updated_at = now()
          WHERE part = $1 AND last_id = $2 AND fence = $6",
    )
    .bind(part_i)
    .bind(&last_id)
    .bind(last.to_string())
    .bind(i(last.0))
    .bind(i64::try_from(fresh.len()).unwrap_or(0))
    .bind(fence_i)
    .execute(&mut *c)
    .await?
    .rows_affected();
    if n != 1 {
        state.eids.forget(part, &new_eids);
        return Err(PluginError::new("accounting.lease_lost", "cursor moved under us").retryable());
    }
    drop(c);
    close_windows(ctx, cfg, part_i, i(last.0)).await?;
    Ok(PollOutcome::Busy)
}

#[allow(clippy::too_many_arguments)]
async fn store(
    ctx: &Ctx,
    c: &mut PgConnection,
    state: &crate::State,
    cfg: &Config,
    part: i16,
    window_start: i64,
    entries: &[Entry],
    rejected: &[(String, &'static str, serde_json::Value)],
) -> PResult<()> {
    let idx = state.rate_index();
    // ---- 聚合（§1.2）：键有序，保证写入顺序一致 ----
    let mut pairs: BTreeMap<(u64, u64, i64), PairAgg> = BTreeMap::new();
    let mut peers: BTreeMap<(u64, u64, [u8; 20]), PeerAgg> = BTreeMap::new();
    for e in entries {
        let t_ms = i(e.id.0);
        let touch = touches(e);
        let (up_pm, down_pm, ids) = if touch {
            rates::effective(&idx, e.uid, e.tid, t_ms, cfg.max_up_permille)
        } else {
            (1000, 1000, vec![])
        };
        if touch {
            let key = rates::rate_key(up_pm, down_pm, &ids);
            let p = pairs.entry((e.uid, e.tid, key)).or_insert_with(|| PairAgg {
                up_permille: up_pm,
                down_permille: down_pm,
                rule_ids: ids.clone(),
                first_ts: e.ts,
                ..PairAgg::default()
            });
            p.up_raw += e.dup;
            p.down_raw += e.ddown;
            p.up += rates::credited(e.dup, up_pm);
            p.down += rates::credited(e.ddown, down_pm);
            p.seed += e.dseed;
            p.leech += e.dleech;
            p.last_ts = p.last_ts.max(e.ts);
            p.first_ts = p.first_ts.min(e.ts);
            if e.flags & FLAG_FIRST_COMPLETED != 0 {
                p.completed_at = Some(p.completed_at.map_or(e.ts, |c| c.min(e.ts)));
            }
        }
        let pe = peers.entry((e.uid, e.tid, e.pid)).or_insert_with(|| PeerAgg {
            is_seeder: e.left == 0,
            active: true,
            client: e.client.clone(),
            last_ts: e.ts,
            seed: 0,
        });
        pe.seed += e.dseed;
        pe.is_seeder = e.left == 0;
        pe.active = e.ev != Ev::Stopped;
        pe.client.clone_from(&e.client);
        pe.last_ts = pe.last_ts.max(e.ts);
    }

    // ---- peers ----
    if !peers.is_empty() {
        let (mut u, mut t, mut pid, mut seeder, mut active, mut client, mut at, mut secs) =
            (vec![], vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
        for ((uid, tid, p), a) in &peers {
            u.push(i(*uid));
            t.push(i(*tid));
            pid.push(p.to_vec());
            seeder.push(a.is_seeder);
            active.push(a.active);
            client.push(a.client.clone());
            at.push(ts_dt(a.last_ts));
            secs.push(i32::try_from(a.seed.min(86_400)).unwrap_or(0));
        }
        // 小时 = 窗口所在小时（窗口整除 1 小时，微批不跨窗口）；按 hour % 2 选槽，槽内同小时累加、换小时覆盖
        let hour = i32::try_from(window_start.div_euclid(3600)).unwrap_or(0);
        sqlx::query(
            "INSERT INTO peers AS p (user_id, torrent_id, peer_id, part, is_seeder, is_active, client_prefix, first_seen_at, last_announce_at,
                                    slot0_hour, slot0_secs, slot1_hour, slot1_secs)
             SELECT u, t, pid, $1, s, a, cl, at, at,
                    CASE WHEN $9 % 2 = 0 THEN $9 END, CASE WHEN $9 % 2 = 0 THEN sec ELSE 0 END,
                    CASE WHEN $9 % 2 = 1 THEN $9 END, CASE WHEN $9 % 2 = 1 THEN sec ELSE 0 END
               FROM unnest($2::bigint[], $3::bigint[], $4::bytea[], $5::bool[], $6::bool[], $7::text[], $8::timestamptz[], $10::int[])
                  AS x(u, t, pid, s, a, cl, at, sec)
             ORDER BY u, t, pid
             ON CONFLICT (user_id, torrent_id, peer_id) DO UPDATE SET
                is_seeder = EXCLUDED.is_seeder, is_active = EXCLUDED.is_active, client_prefix = EXCLUDED.client_prefix,
                last_announce_at = greatest(p.last_announce_at, EXCLUDED.last_announce_at),
                slot0_secs = CASE WHEN EXCLUDED.slot0_hour IS NULL THEN p.slot0_secs
                                  WHEN p.slot0_hour = EXCLUDED.slot0_hour THEN p.slot0_secs + EXCLUDED.slot0_secs
                                  ELSE EXCLUDED.slot0_secs END,
                slot0_hour = coalesce(EXCLUDED.slot0_hour, p.slot0_hour),
                slot1_secs = CASE WHEN EXCLUDED.slot1_hour IS NULL THEN p.slot1_secs
                                  WHEN p.slot1_hour = EXCLUDED.slot1_hour THEN p.slot1_secs + EXCLUDED.slot1_secs
                                  ELSE EXCLUDED.slot1_secs END,
                slot1_hour = coalesce(EXCLUDED.slot1_hour, p.slot1_hour)",
        )
        .bind(part)
        .bind(&u)
        .bind(&t)
        .bind(&pid)
        .bind(&seeder)
        .bind(&active)
        .bind(&client)
        .bind(&at)
        .bind(hour)
        .bind(&secs)
        .execute(&mut *c)
        .await?;
    }

    // ---- 首次完成判定（§1.5 第 3 步） ----
    let completing: Vec<(i64, i64)> = pairs
        .iter()
        .filter(|(_, a)| a.completed_at.is_some())
        .map(|((u, t, _), _)| (i(*u), i(*t)))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut first_completion: HashSet<(i64, i64)> = HashSet::new();
    if !completing.is_empty() {
        let (us, ts): (Vec<i64>, Vec<i64>) = completing.iter().copied().unzip();
        let done: HashSet<(i64, i64)> = sqlx::query(
            "SELECT s.user_id, s.torrent_id FROM snatches s JOIN unnest($1::bigint[], $2::bigint[]) AS x(u, t)
                ON s.user_id = x.u AND s.torrent_id = x.t
             WHERE s.completed_at IS NOT NULL ORDER BY 1, 2 FOR UPDATE OF s",
        )
        .bind(&us)
        .bind(&ts)
        .fetch_all(&mut *c)
        .await?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
        first_completion = completing.into_iter().filter(|k| !done.contains(k)).collect();
    }

    // ---- snatches（同一 (uid, tid) 的多个 rate_key 合并） ----
    let mut snatch: BTreeMap<(u64, u64), PairAgg> = BTreeMap::new();
    for ((u, t, _), a) in &pairs {
        let s = snatch.entry((*u, *t)).or_insert_with(|| PairAgg {
            first_ts: a.first_ts,
            ..PairAgg::default()
        });
        s.up_raw += a.up_raw;
        s.down_raw += a.down_raw;
        s.up += a.up;
        s.down += a.down;
        // 做种时长只由小时快照折叠（03 §1.9），此处不计入，避免与快照重复
        s.leech += a.leech;
        s.last_ts = s.last_ts.max(a.last_ts);
        s.first_ts = s.first_ts.min(a.first_ts);
        if a.completed_at.is_some() {
            s.completed_at = match (s.completed_at, a.completed_at) {
                (Some(x), Some(y)) => Some(x.min(y)),
                (x, y) => x.or(y),
            };
        }
    }
    {
        let mut cols: (
            Vec<i64>,
            Vec<i64>,
            Vec<i64>,
            Vec<i64>,
            Vec<i64>,
            Vec<i64>,
            Vec<i64>,
            Vec<i64>,
        ) = Default::default();
        let (mut first, mut lastv, mut comp, mut times) = (vec![], vec![], vec![], vec![]);
        for ((u, t), a) in &snatch {
            let fc = first_completion.contains(&(i(*u), i(*t)));
            cols.0.push(i(*u));
            cols.1.push(i(*t));
            cols.2.push(i(a.up_raw));
            cols.3.push(i(a.down_raw));
            cols.4.push(i(a.up));
            cols.5.push(i(a.down));
            cols.6.push(i(a.seed));
            cols.7.push(i(a.leech));
            first.push(ts_dt(a.first_ts));
            lastv.push(ts_dt(a.last_ts));
            comp.push(if fc { a.completed_at.map(ts_dt) } else { None });
            times.push(i32::from(fc));
        }
        sqlx::query(
            "INSERT INTO snatches AS s (user_id, torrent_id, uploaded_raw, downloaded_raw, uploaded, downloaded, seed_secs, leech_secs,
                                        first_action_at, last_action_at, completed_at, times_completed)
             SELECT * FROM unnest($1::bigint[], $2::bigint[], $3::bigint[], $4::bigint[], $5::bigint[], $6::bigint[], $7::bigint[],
                                  $8::bigint[], $9::timestamptz[], $10::timestamptz[], $11::timestamptz[], $12::int[])
             ORDER BY 1, 2
             ON CONFLICT (user_id, torrent_id) DO UPDATE SET
                uploaded_raw = s.uploaded_raw + EXCLUDED.uploaded_raw, downloaded_raw = s.downloaded_raw + EXCLUDED.downloaded_raw,
                uploaded = s.uploaded + EXCLUDED.uploaded, downloaded = s.downloaded + EXCLUDED.downloaded,
                seed_secs = s.seed_secs + EXCLUDED.seed_secs, leech_secs = s.leech_secs + EXCLUDED.leech_secs,
                last_action_at = greatest(s.last_action_at, EXCLUDED.last_action_at),
                completed_at = coalesce(s.completed_at, EXCLUDED.completed_at),
                times_completed = s.times_completed + EXCLUDED.times_completed, updated_at = now()",
        )
        .bind(&cols.0)
        .bind(&cols.1)
        .bind(&cols.2)
        .bind(&cols.3)
        .bind(&cols.4)
        .bind(&cols.5)
        .bind(&cols.6)
        .bind(&cols.7)
        .bind(&first)
        .bind(&lastv)
        .bind(&comp)
        .bind(&times)
        .execute(&mut *c)
        .await?;
    }

    // ---- torrent_counters 与首次完成事件 ----
    let mut completions: Vec<(i64, i64, i64)> = Vec::new();
    for ((u, t), a) in &snatch {
        if first_completion.contains(&(i(*u), i(*t))) {
            completions.push((i(*t), i(*u), a.completed_at.unwrap_or(a.last_ts)));
        }
    }
    completions.sort_unstable();
    if !completions.is_empty() {
        let ts: Vec<i64> = completions.iter().map(|c| c.0).collect();
        sqlx::query(
            "INSERT INTO torrent_counters (torrent_id, completed) SELECT t, count(*) FROM unnest($1::bigint[]) AS x(t) GROUP BY t ORDER BY t
             ON CONFLICT (torrent_id) DO UPDATE SET completed = torrent_counters.completed + EXCLUDED.completed, updated_at = now()",
        )
        .bind(&ts)
        .execute(&mut *c)
        .await?;
    }

    // ---- user_totals、user_daily ----
    let mut users: BTreeMap<u64, (u64, u64, u64, u64, u64, u64, i32)> = BTreeMap::new();
    for ((u, t), a) in &snatch {
        let x = users.entry(*u).or_default();
        x.0 += a.up_raw;
        x.1 += a.down_raw;
        x.2 += a.up;
        x.3 += a.down;
        x.4 += a.seed;
        x.5 += a.leech;
        x.6 += i32::from(first_completion.contains(&(i(*u), i(*t))));
    }
    if !users.is_empty() {
        let mut v: (
            Vec<i64>,
            Vec<i64>,
            Vec<i64>,
            Vec<i64>,
            Vec<i64>,
            Vec<i64>,
            Vec<i64>,
            Vec<i32>,
        ) = Default::default();
        for (u, x) in &users {
            v.0.push(i(*u));
            v.1.push(i(x.0));
            v.2.push(i(x.1));
            v.3.push(i(x.2));
            v.4.push(i(x.3));
            v.5.push(i(x.4));
            v.6.push(i(x.5));
            v.7.push(x.6);
        }
        sqlx::query(
            "INSERT INTO user_totals AS u (user_id, uploaded_raw, downloaded_raw, uploaded_traffic, downloaded_traffic, seed_secs, leech_secs, completed_count)
             SELECT * FROM unnest($1::bigint[], $2::bigint[], $3::bigint[], $4::bigint[], $5::bigint[], $6::bigint[], $7::bigint[], $8::int[]) ORDER BY 1
             ON CONFLICT (user_id) DO UPDATE SET
                uploaded_raw = u.uploaded_raw + EXCLUDED.uploaded_raw, downloaded_raw = u.downloaded_raw + EXCLUDED.downloaded_raw,
                uploaded_traffic = u.uploaded_traffic + EXCLUDED.uploaded_traffic,
                downloaded_traffic = u.downloaded_traffic + EXCLUDED.downloaded_traffic,
                seed_secs = u.seed_secs + EXCLUDED.seed_secs, leech_secs = u.leech_secs + EXCLUDED.leech_secs,
                completed_count = u.completed_count + EXCLUDED.completed_count, updated_at = now()",
        )
        .bind(&v.0)
        .bind(&v.1)
        .bind(&v.2)
        .bind(&v.3)
        .bind(&v.4)
        .bind(&v.5)
        .bind(&v.6)
        .bind(&v.7)
        .execute(&mut *c)
        .await?;
        // 日期按 UTC（站点时区在后续迭代接入）
        let day = ts_dt(window_start).date_naive();
        sqlx::query(
            "INSERT INTO user_daily AS d (day, user_id, uploaded_raw, downloaded_raw, uploaded, downloaded, seed_secs, leech_secs)
             SELECT $1, * FROM unnest($2::bigint[], $3::bigint[], $4::bigint[], $5::bigint[], $6::bigint[], $7::bigint[], $8::bigint[]) ORDER BY 1
             ON CONFLICT (day, user_id) DO UPDATE SET
                uploaded_raw = d.uploaded_raw + EXCLUDED.uploaded_raw, downloaded_raw = d.downloaded_raw + EXCLUDED.downloaded_raw,
                uploaded = d.uploaded + EXCLUDED.uploaded, downloaded = d.downloaded + EXCLUDED.downloaded,
                seed_secs = d.seed_secs + EXCLUDED.seed_secs, leech_secs = d.leech_secs + EXCLUDED.leech_secs",
        )
        .bind(day)
        .bind(&v.0)
        .bind(&v.1)
        .bind(&v.2)
        .bind(&v.3)
        .bind(&v.4)
        .bind(&v.5)
        .bind(&v.6)
        .execute(&mut *c)
        .await?;
    }

    // ---- settle_buffer（§1.5 第 7 步） ----
    if !pairs.is_empty() {
        #[derive(Default)]
        struct Cols {
            u: Vec<i64>,
            t: Vec<i64>,
            k: Vec<i64>,
            upm: Vec<i32>,
            dpm: Vec<i32>,
            rules: Vec<String>,
            ur: Vec<i64>,
            dr: Vec<i64>,
            cu: Vec<i64>,
            cd: Vec<i64>,
            s: Vec<i64>,
            l: Vec<i64>,
            c: Vec<bool>,
        }
        let mut v = Cols::default();
        for ((u, t, k), a) in &pairs {
            v.u.push(i(*u));
            v.t.push(i(*t));
            v.k.push(*k);
            v.upm.push(i32::try_from(a.up_permille).unwrap_or(i32::MAX));
            v.dpm.push(i32::try_from(a.down_permille).unwrap_or(i32::MAX));
            v.rules.push(format!(
                "{{{}}}",
                a.rule_ids
                    .iter()
                    .map(u64::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ));
            v.ur.push(i(a.up_raw));
            v.dr.push(i(a.down_raw));
            v.cu.push(i(a.up));
            v.cd.push(i(a.down));
            v.s.push(i(a.seed));
            v.l.push(i(a.leech));
            v.c.push(first_completion.contains(&(i(*u), i(*t))) && a.completed_at.is_some());
        }
        sqlx::query(
            "INSERT INTO settle_buffer AS b (window_start, part, user_id, torrent_id, rate_key, up_permille, down_permille, rule_ids,
                                            up_raw, down_raw, up, down, seed_secs, leech_secs, completed)
             SELECT $1, $2, u, t, k, upm, dpm, r::bigint[], ur, dr, cu, cd, s, l, c
               FROM unnest($3::bigint[], $4::bigint[], $5::bigint[], $6::int[], $7::int[], $8::text[], $9::bigint[], $10::bigint[],
                           $11::bigint[], $12::bigint[], $13::bigint[], $14::bigint[], $15::bool[])
                    AS x(u, t, k, upm, dpm, r, ur, dr, cu, cd, s, l, c)
             ORDER BY u, t, k
             ON CONFLICT (part, window_start, user_id, torrent_id, rate_key) DO UPDATE SET
                up_raw = b.up_raw + EXCLUDED.up_raw, down_raw = b.down_raw + EXCLUDED.down_raw,
                up = b.up + EXCLUDED.up, down = b.down + EXCLUDED.down,
                seed_secs = b.seed_secs + EXCLUDED.seed_secs, leech_secs = b.leech_secs + EXCLUDED.leech_secs,
                completed = b.completed OR EXCLUDED.completed",
        )
        .bind(window_start)
        .bind(part)
        .bind(&v.u)
        .bind(&v.t)
        .bind(&v.k)
        .bind(&v.upm)
        .bind(&v.dpm)
        .bind(&v.rules)
        .bind(&v.ur)
        .bind(&v.dr)
        .bind(&v.cu)
        .bind(&v.cd)
        .bind(&v.s)
        .bind(&v.l)
        .bind(&v.c)
        .execute(&mut *c)
        .await?;
    }

    for (id, why, fields) in rejected {
        sqlx::query("INSERT INTO rejected_entries (part, stream_id, reason, fields) VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING")
            .bind(part)
            .bind(id)
            .bind(*why)
            .bind(fields)
            .execute(&mut *c)
            .await?;
    }
    for (tid, uid, at) in &completions {
        ctx.events
            .publish(NewEvent::v1(
                "traffic.snatch_completed",
                format!("user:{uid}"),
                json!({
                "user_id": uid, "torrent_id": tid, "completed_at": ts_dt(*at).to_rfc3339() }),
            ))
            .await?;
    }
    Ok(())
}

/// 窗口关闭（§1.6）：结算缓冲 → 分段明细 + 每 (窗口, 用户) 一条 `traffic.settled`。
pub async fn close_windows(ctx: &Ctx, cfg: &Config, part: i16, watermark_ms: i64) -> PResult<()> {
    let w = i64::from(cfg.window_secs);
    let closable = win(watermark_ms - i64::from(cfg.settle_grace_secs) * 1000, w);
    let mut c = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "DELETE FROM settle_buffer WHERE part = $1 AND window_start < $2
         RETURNING window_start, user_id, torrent_id, rate_key, up_permille, down_permille, rule_ids,
                   up_raw, down_raw, up, down, seed_secs, leech_secs, completed",
    )
    .bind(part)
    .bind(closable)
    .fetch_all(&mut *c)
    .await?;
    if rows.is_empty() {
        return Ok(());
    }
    // 分段明细（重算依据）：按 (日, 用户, 种子, rate_key) 合并
    let mut seg: BTreeMap<(chrono::NaiveDate, i64, i64, i64), (i32, i32, String, i64, i64, i64, i64)> =
        BTreeMap::new();
    for r in &rows {
        let day = ts_dt(r.get::<i64, _>(0)).date_naive();
        let ids: Vec<i64> = r.get(6);
        let e = seg.entry((day, r.get(1), r.get(2), r.get(3))).or_insert_with(|| {
            (
                r.get(4),
                r.get(5),
                format!(
                    "{{{}}}",
                    ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
                ),
                0,
                0,
                0,
                0,
            )
        });
        e.3 += r.get::<i64, _>(7);
        e.4 += r.get::<i64, _>(8);
        e.5 += r.get::<i64, _>(9);
        e.6 += r.get::<i64, _>(10);
    }
    let mut v: (
        Vec<chrono::NaiveDate>,
        Vec<i64>,
        Vec<i64>,
        Vec<i64>,
        Vec<i32>,
        Vec<i32>,
        Vec<String>,
        Vec<i64>,
        Vec<i64>,
        Vec<i64>,
        Vec<i64>,
    ) = Default::default();
    for ((day, u, t, k), x) in seg {
        v.0.push(day);
        v.1.push(u);
        v.2.push(t);
        v.3.push(k);
        v.4.push(x.0);
        v.5.push(x.1);
        v.6.push(x.2);
        v.7.push(x.3);
        v.8.push(x.4);
        v.9.push(x.5);
        v.10.push(x.6);
    }
    sqlx::query(
        "INSERT INTO traffic_segments AS s (day, user_id, torrent_id, rate_key, up_permille, down_permille, rule_ids, up_raw, down_raw, up, down)
         SELECT d, u, t, k, upm, dpm, r::bigint[], ur, dr, cu, cd
           FROM unnest($1::date[], $2::bigint[], $3::bigint[], $4::bigint[], $5::int[], $6::int[], $7::text[], $8::bigint[], $9::bigint[], $10::bigint[], $11::bigint[])
                AS x(d, u, t, k, upm, dpm, r, ur, dr, cu, cd)
         ON CONFLICT (day, user_id, torrent_id, rate_key) DO UPDATE SET
            up_raw = s.up_raw + EXCLUDED.up_raw, down_raw = s.down_raw + EXCLUDED.down_raw,
            up = s.up + EXCLUDED.up, down = s.down + EXCLUDED.down",
    )
    .bind(&v.0)
    .bind(&v.1)
    .bind(&v.2)
    .bind(&v.3)
    .bind(&v.4)
    .bind(&v.5)
    .bind(&v.6)
    .bind(&v.7)
    .bind(&v.8)
    .bind(&v.9)
    .bind(&v.10)
    .execute(&mut *c)
    .await?;
    drop(c);

    // 事件：按 (窗口, 用户) 分组，同一种子多个 rate_key 合并
    #[derive(Default)]
    struct Item {
        up_raw: i64,
        down_raw: i64,
        up: i64,
        down: i64,
        seed: i64,
        leech: i64,
        rules: std::collections::BTreeSet<i64>,
        completed: bool,
    }
    let mut groups: BTreeMap<(i64, i64), BTreeMap<i64, Item>> = BTreeMap::new();
    for r in &rows {
        let it = groups
            .entry((r.get(0), r.get(1)))
            .or_default()
            .entry(r.get(2))
            .or_default();
        it.up_raw += r.get::<i64, _>(7);
        it.down_raw += r.get::<i64, _>(8);
        it.up += r.get::<i64, _>(9);
        it.down += r.get::<i64, _>(10);
        it.seed += r.get::<i64, _>(11);
        it.leech += r.get::<i64, _>(12);
        it.rules.extend(r.get::<Vec<i64>, _>(6));
        it.completed |= r.get::<bool, _>(13);
    }
    for ((ws, uid), items) in groups {
        let batch_id = uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_OID,
            format!("pt-studio:settle:{ws}:{uid}").as_bytes(),
        );
        let items: Vec<serde_json::Value> = items
            .into_iter()
            .map(|(tid, it)| {
                let mut v = json!({ "torrent_id": tid, "d_up_raw": it.up_raw, "d_down_raw": it.down_raw, "d_up": it.up,
                                    "d_down": it.down, "d_seed_secs": it.seed, "d_leech_secs": it.leech,
                                    "rate_rule_ids": it.rules.into_iter().collect::<Vec<_>>() });
                if it.completed {
                    v["completed"] = json!(true);
                }
                v
            })
            .collect();
        // 单条事件 ≤ 64 KiB：每 300 个种子一段
        let chunks: Vec<&[serde_json::Value]> = items.chunks(300).collect();
        let parts = chunks.len();
        for (n, chunk) in chunks.into_iter().enumerate() {
            ctx.events
                .publish(NewEvent::v1("traffic.settled", format!("user:{uid}"), json!({
                    "user_id": uid, "window_start": ts_dt(ws).to_rfc3339(), "window_end": ts_dt(ws + w).to_rfc3339(),
                    "batch_id": batch_id, "part": n + 1, "parts": parts, "items": chunk })))
                .await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(uid: u64) -> HashMap<String, String> {
        [
            ("v", "1"),
            ("eid", "n1:1:7"),
            ("ts", "1700000000"),
            ("tid", "5"),
            ("pid", "2d71423436333030aaaaaaaaaaaaaaaaaaaaaaaa"),
            ("ip", "1.2.3.4"),
            ("port", "1"),
            ("ev", "none"),
            ("up", "10"),
            ("down", "0"),
            ("left", "0"),
            ("dup", "10"),
            ("ddown", "0"),
            ("dseed", "60"),
            ("dleech", "0"),
            ("flags", "4294967295"),
            ("ua", "x"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .chain([("uid".to_owned(), uid.to_string())])
        .collect()
    }

    #[test]
    fn parses_and_validates() {
        let e = parse(StreamId(1, 0), &fields(9), 4, 1).ok();
        assert!(
            e.as_ref()
                .is_some_and(|e| e.uid == 9 && e.dup == 10 && e.client == "-qB46300")
        );
        assert_eq!(
            parse(StreamId(1, 0), &fields(9), 4, 2).err(),
            Some("wrong_partition")
        );
        let mut f = fields(9);
        f.insert("dup".into(), (MAX_DELTA + 1).to_string());
        f.insert("flags".into(), FLAG_FIRST_COMPLETED.to_string());
        let e = parse(StreamId(1, 0), &f, 4, 1).ok();
        assert!(
            e.is_some_and(|e| e.dup == 0 && e.dseed == 60 && e.flags & FLAG_FIRST_COMPLETED != 0),
            "out-of-range traffic only drops traffic fields"
        );
        f.insert("v".into(), "2".into());
        assert_eq!(parse(StreamId(1, 0), &f, 4, 1).err(), Some("unsupported_version"));
    }

    #[test]
    fn eid_cache_and_windows() {
        let c = EidCache::default();
        assert!(c.insert(0, "a") && !c.insert(0, "a") && c.insert(1, "a"));
        c.forget(0, &["a".into()]);
        assert!(c.insert(0, "a"));
        assert_eq!(win(1_700_000_123_456, 900), 1_700_000_100);
        assert_eq!(StreamId::parse("12-3"), Some(StreamId(12, 3)));
        assert!(StreamId(12, 3) < StreamId(13, 0));
    }
}
