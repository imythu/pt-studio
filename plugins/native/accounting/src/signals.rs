//! 基础信号（03 §1.4、§1.10）：条目级判定（纯函数）、去重与 `traffic.signal_raised`、每小时交叉校验、查询。
//!
//! accounting 只输出信号，不做判定（anticheat 订阅后评分）。信号与流量结算在同一事务中提交。

use std::collections::{BTreeMap, HashMap, HashSet};

use pt_plugin_api::{Ctx, NewEvent, PResult, PluginError, json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgConnection, Row};

use crate::ingest::{Entry, Ev};

/// pt-tracker 的信号位（pt-tracker accounting.rs）
pub const FLAG_BASELINE_RESET: u32 = 1 << 1;
pub const FLAG_UPLOAD_REGRESSED: u32 = 1 << 2;
pub const FLAG_DOWNLOAD_REGRESSED: u32 = 1 << 3;
pub const FLAG_SPEED_EXCEEDED: u32 = 1 << 7;

/// 静态配置 `[plugin.accounting.signals]`（SRS §5 `signals.*`）
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SignalCfg {
    pub enabled: bool,
    pub speed_limit_bps: u64,
    pub speed_min_bytes: u64,
    pub jump_size_factor: u64,
    pub no_peer_min_bytes: u64,
    pub cross_check_tolerance_pct: u64,
    pub cross_check_slack_bytes: u64,
    pub cross_check_top_k: i64,
    pub dedupe_secs: i64,
}

impl Default for SignalCfg {
    fn default() -> Self {
        Self {
            enabled: true,
            speed_limit_bps: 125_000_000,
            speed_min_bytes: 1 << 30,
            jump_size_factor: 3,
            no_peer_min_bytes: 100 << 20,
            cross_check_tolerance_pct: 20,
            cross_check_slack_bytes: 1 << 30,
            cross_check_top_k: 3,
            dedupe_secs: 86_400,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Low,
    Medium,
    High,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "high" => Self::High,
            "medium" => Self::Medium,
            _ => Self::Low,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sig {
    pub uid: u64,
    pub tid: u64,
    pub signal: &'static str,
    pub severity: Severity,
    pub details: Value,
    /// Unix 秒
    pub at: i64,
}

/// 条目级上下文：种子体积（镜像缺失为 None）、有其他用户在下载的种子
pub struct Swarm<'a> {
    pub sizes: &'a HashMap<u64, u64>,
    /// (种子, 下载者用户) —— 活跃 leecher
    pub leechers: &'a HashSet<(u64, u64)>,
}

impl Swarm<'_> {
    fn has_other_leecher(&self, tid: u64, uid: u64) -> bool {
        self.leechers.iter().any(|(t, u)| *t == tid && *u != uid)
    }
}

fn speed(bytes: u64, secs: u64) -> u64 {
    bytes / secs.max(1)
}

/// 条目级规则（§1.4 表格）。`started_nonzero` 是 pt-tracker 对首报 started 带累计值的处理（只建基线）。
pub fn evaluate(e: &Entry, cfg: &SignalCfg, sw: &Swarm<'_>) -> Vec<Sig> {
    let mut out = Vec::new();
    let sig = |signal, severity, details| Sig {
        uid: e.uid,
        tid: e.tid,
        signal,
        severity,
        details,
        at: e.ts,
    };
    let interval = e.dseed + e.dleech;
    let limit = cfg.speed_limit_bps.max(1);
    // 速度：pt-tracker 截断的段（xup）一律 high；其余按超出倍数
    if e.flags & FLAG_SPEED_EXCEEDED != 0 && e.xup > 0 {
        let sp = speed(e.xup, interval);
        out.push(sig(
            "speed_exceeded",
            Severity::High,
            json!({ "bytes": e.xup, "interval_secs": interval, "speed_bps": sp, "limit_bps": limit,
                    "client": e.client, "kind": "tracker_cap" }),
        ));
    } else if e.dup >= cfg.speed_min_bytes && interval > 0 && speed(e.dup, interval) > limit {
        let sp = speed(e.dup, interval);
        let sev = if sp >= limit.saturating_mul(2) {
            Severity::High
        } else {
            Severity::Medium
        };
        out.push(sig(
            "speed_exceeded",
            sev,
            json!({ "bytes": e.dup, "interval_secs": interval, "speed_bps": sp, "limit_bps": limit, "client": e.client }),
        ));
    }
    for (bit, kind) in [
        (FLAG_UPLOAD_REGRESSED, "upload_regressed"),
        (FLAG_DOWNLOAD_REGRESSED, "download_regressed"),
    ] {
        if e.flags & bit != 0 {
            out.push(sig(
                "negative_delta",
                Severity::Low,
                json!({ "kind": kind, "reported_up": e.rep_up, "reported_down": e.rep_down }),
            ));
        }
    }
    if e.flags & FLAG_BASELINE_RESET != 0 {
        let (kind, sev) = if e.ev == Ev::Started {
            ("started_nonzero", Severity::Medium)
        } else {
            ("baseline_reset_nonstart", Severity::Low)
        };
        out.push(sig(
            "counter_jump",
            sev,
            json!({ "kind": kind, "reported_up": e.rep_up, "reported_down": e.rep_down }),
        ));
    }
    if e.out_of_range {
        out.push(sig(
            "counter_jump",
            Severity::Medium,
            json!({ "kind": "out_of_range" }),
        ));
    }
    if let Some(&size) = sw.sizes.get(&e.tid).filter(|s| **s > 0) {
        if e.dup > size.saturating_mul(cfg.jump_size_factor.max(1)) {
            out.push(sig(
                "counter_jump",
                Severity::Medium,
                json!({ "kind": "delta_exceeds_size", "bytes": e.dup, "size": size }),
            ));
        }
        if e.left > size {
            out.push(sig(
                "counter_jump",
                Severity::Medium,
                json!({ "kind": "left_exceeds_size", "left": e.left, "size": size }),
            ));
        }
    }
    if e.dup >= cfg.no_peer_min_bytes.max(1) && !sw.has_other_leecher(e.tid, e.uid) {
        let sev = if e.dup >= cfg.no_peer_min_bytes.saturating_mul(10) {
            Severity::Medium
        } else {
            Severity::Low
        };
        out.push(sig(
            "peer_not_in_swarm",
            sev,
            json!({ "kind": "no_leechers", "bytes": e.dup, "leechers": 0 }),
        ));
    }
    out
}

/// 同一微批中 (用户, 种子, 信号) 合并：hits = 条数，严重度取最高，details 取最高严重度的第一条
pub fn merge(sigs: Vec<Sig>) -> Vec<(Sig, i32)> {
    let mut m: BTreeMap<(u64, u64, &'static str), (Sig, i32)> = BTreeMap::new();
    for s in sigs {
        match m.get_mut(&(s.uid, s.tid, s.signal)) {
            Some((cur, n)) => {
                *n += 1;
                if s.severity > cur.severity {
                    let at = cur.at.min(s.at);
                    *cur = Sig { at, ..s };
                } else {
                    cur.at = cur.at.min(s.at);
                }
            }
            None => {
                m.insert((s.uid, s.tid, s.signal), (s, 1));
            }
        }
    }
    m.into_values().collect()
}

fn i(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

fn rfc3339(secs: i64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp(secs, 0)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// 去重后写入（与调用方同一事务）：首次、去重期已过或严重度升级时新建信号并发布事件，否则累加 hits
pub async fn record(
    ctx: &Ctx,
    c: &mut PgConnection,
    cfg: &SignalCfg,
    sigs: Vec<(Sig, i32)>,
) -> PResult<usize> {
    let mut published = 0;
    for (s, n) in sigs {
        let (u, t) = (i(s.uid), i(s.tid));
        let cur = sqlx::query(
            "SELECT severity, until > now(), signal_id FROM signal_dedupe
              WHERE user_id = $1 AND torrent_id = $2 AND signal = $3 FOR UPDATE",
        )
        .bind(u)
        .bind(t)
        .bind(s.signal)
        .fetch_optional(&mut *c)
        .await?;
        let fresh = match &cur {
            None => true,
            Some(r) => !r.get::<bool, _>(1) || s.severity > Severity::parse(&r.get::<String, _>(0)),
        };
        if !fresh {
            let id: i64 = cur.as_ref().map(|r| r.get(2)).unwrap_or_default();
            sqlx::query("UPDATE signals SET hits = hits + $2, last_at = greatest(last_at, to_timestamp($3)) WHERE id = $1")
                .bind(id)
                .bind(n)
                .bind(s.at as f64)
                .execute(&mut *c)
                .await?;
            sqlx::query(
                "UPDATE signal_dedupe SET hits = hits + $4 WHERE user_id = $1 AND torrent_id = $2 AND signal = $3",
            )
            .bind(u)
            .bind(t)
            .bind(s.signal)
            .bind(n)
            .execute(&mut *c)
            .await?;
            continue;
        }
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO signals (user_id, torrent_id, signal, severity, hits, details, first_at, last_at)
             VALUES ($1, $2, $3, $4, $5, $6, to_timestamp($7), to_timestamp($7)) RETURNING id",
        )
        .bind(u)
        .bind(t)
        .bind(s.signal)
        .bind(s.severity.as_str())
        .bind(n)
        .bind(&s.details)
        .bind(s.at as f64)
        .fetch_one(&mut *c)
        .await?;
        sqlx::query(
            "INSERT INTO signal_dedupe (user_id, torrent_id, signal, severity, until, signal_id, hits)
             VALUES ($1, $2, $3, $4, now() + make_interval(secs => $5), $6, $7)
             ON CONFLICT (user_id, torrent_id, signal) DO UPDATE SET
                severity = EXCLUDED.severity, until = EXCLUDED.until, signal_id = EXCLUDED.signal_id, hits = EXCLUDED.hits",
        )
        .bind(u)
        .bind(t)
        .bind(s.signal)
        .bind(s.severity.as_str())
        .bind(cfg.dedupe_secs.max(60) as f64)
        .bind(id)
        .bind(n)
        .execute(&mut *c)
        .await?;
        let mut payload = json!({ "signal_id": id, "user_id": u, "signal": s.signal, "severity": s.severity.as_str(),
                                  "hits": n, "details": s.details, "first_at": rfc3339(s.at) });
        if t > 0 {
            payload["torrent_id"] = json!(t);
        }
        ctx.events
            .publish(NewEvent::v1(
                "traffic.signal_raised",
                format!("user:{u}"),
                payload,
            ))
            .await?;
        published += 1;
    }
    Ok(published)
}

/// 微批的条目级信号：种子体积与活跃 leecher 各一次查询（peers 已在本事务中写入本批）
pub async fn for_batch(ctx: &Ctx, c: &mut PgConnection, cfg: &SignalCfg, entries: &[Entry]) -> PResult<()> {
    if !cfg.enabled || entries.is_empty() {
        return Ok(());
    }
    let tids: Vec<i64> = entries
        .iter()
        .map(|e| i(e.tid))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let sizes: HashMap<u64, u64> = sqlx::query("SELECT id, size FROM torrents WHERE id = ANY($1)")
        .bind(&tids)
        .fetch_all(&mut *c)
        .await?
        .iter()
        .map(|r| {
            (
                u64::try_from(r.get::<i64, _>(0)).unwrap_or(0),
                u64::try_from(r.get::<i64, _>(1)).unwrap_or(0),
            )
        })
        .collect();
    let upload_tids: Vec<i64> = entries
        .iter()
        .filter(|e| e.dup >= cfg.no_peer_min_bytes.max(1))
        .map(|e| i(e.tid))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let leechers: HashSet<(u64, u64)> = if upload_tids.is_empty() {
        HashSet::new()
    } else {
        sqlx::query(
            "SELECT DISTINCT torrent_id, user_id FROM peers
              WHERE torrent_id = ANY($1) AND is_active AND NOT is_seeder AND last_announce_at > now() - interval '1 hour'",
        )
        .bind(&upload_tids)
        .fetch_all(&mut *c)
        .await?
        .iter()
        .map(|r| {
            (
                u64::try_from(r.get::<i64, _>(0)).unwrap_or(0),
                u64::try_from(r.get::<i64, _>(1)).unwrap_or(0),
            )
        })
        .collect()
    };
    let sw = Swarm {
        sizes: &sizes,
        leechers: &leechers,
    };
    let sigs: Vec<Sig> = entries.iter().flat_map(|e| evaluate(e, cfg, &sw)).collect();
    if sigs.is_empty() {
        return Ok(());
    }
    record(ctx, c, cfg, merge(sigs)).await?;
    Ok(())
}

/// 交叉校验（§1.10，cron `cross_check`）：处理上次之后、全部分区水位都已越过的整点小时（一次最多 48 个）
pub async fn cross_check(ctx: &Ctx, cfg: &SignalCfg) -> PResult<usize> {
    if !cfg.enabled {
        return Ok(0);
    }
    let mut total = 0;
    let mut c = ctx.sql.conn().await?;
    let wm: Option<i64> = sqlx::query_scalar("SELECT min(watermark_ms) FROM stream_cursors")
        .fetch_one(&mut *c)
        .await?;
    let Some(wm) = wm else { return Ok(0) };
    // 已完整的最后一个小时
    let ready = wm.div_euclid(3_600_000) - 1;
    let last: Option<i32> = sqlx::query_scalar("SELECT max(hour) FROM cross_check_runs")
        .fetch_one(&mut *c)
        .await?;
    let first = match last {
        Some(h) => i64::from(h) + 1,
        None => {
            let min: Option<i32> = sqlx::query_scalar("SELECT min(hour) FROM torrent_user_hourly")
                .fetch_one(&mut *c)
                .await?;
            min.map_or(ready + 1, i64::from)
        }
    };
    drop(c);
    let tol = cfg.cross_check_tolerance_pct.min(500);
    let slack = cfg.cross_check_slack_bytes;
    for h in first.max(ready - 47)..=ready {
        let hour = i32::try_from(h).map_err(|_| PluginError::internal("hour out of range"))?;
        let mut c = ctx.sql.conn().await?;
        let agg = sqlx::query(
            "SELECT torrent_id, sum(up_raw)::bigint, sum(down_raw)::bigint, count(*) FILTER (WHERE up_raw > 0)
               FROM torrent_user_hourly WHERE hour = $1 GROUP BY torrent_id
             HAVING sum(up_raw) > sum(down_raw) * (100 + $2) / 100 + $3 ORDER BY torrent_id",
        )
        .bind(hour)
        .bind(i(tol))
        .bind(i(slack))
        .fetch_all(&mut *c)
        .await?;
        let mut sigs = Vec::new();
        for r in &agg {
            let (t, up, down, n_up): (i64, i64, i64, i64) = (r.get(0), r.get(1), r.get(2), r.get(3));
            let excess = up - down;
            let top = sqlx::query(
                "SELECT user_id, up_raw FROM torrent_user_hourly WHERE hour = $1 AND torrent_id = $2 AND up_raw > 0
                  ORDER BY up_raw DESC, user_id LIMIT $3",
            )
            .bind(hour)
            .bind(t)
            .bind(cfg.cross_check_top_k.clamp(1, 20))
            .fetch_all(&mut *c)
            .await?;
            let sev = if excess >= i(slack).saturating_mul(10) {
                Severity::High
            } else {
                Severity::Medium
            };
            for u in &top {
                let (uid, uup): (i64, i64) = (u.get(0), u.get(1));
                // 只归因到“单人就足以解释大部分超额”的用户
                if uup.saturating_mul(2) < excess && uup < i(slack) {
                    continue;
                }
                sigs.push(Sig {
                    uid: u64::try_from(uid).unwrap_or(0),
                    tid: u64::try_from(t).unwrap_or(0),
                    signal: "cross_check_mismatch",
                    severity: sev,
                    details: json!({ "hour_start": rfc3339(h * 3600), "swarm_uploaded": up, "swarm_downloaded": down,
                                     "user_uploaded": uup, "user_share_pct": uup * 100 / up.max(1), "uploaders": n_up }),
                    at: (h + 1) * 3600 - 1,
                });
            }
        }
        let n = sigs.len();
        record(ctx, &mut c, cfg, merge(sigs)).await?;
        sqlx::query(
            "INSERT INTO cross_check_runs (hour, torrents, signals) VALUES ($1, $2, $3) ON CONFLICT (hour) DO NOTHING",
        )
        .bind(hour)
        .bind(i32::try_from(agg.len()).unwrap_or(i32::MAX))
        .bind(i32::try_from(n).unwrap_or(i32::MAX))
        .execute(&mut *c)
        .await?;
        sqlx::query("DELETE FROM torrent_user_hourly WHERE hour < $1")
            .bind(hour - 48)
            .execute(&mut *c)
            .await?;
        drop(c);
        ctx.sql.commit().await?;
        total += n;
    }
    Ok(total)
}

/// 到期的去重行（cron 顺带清理）
pub async fn purge_dedupe(ctx: &Ctx) -> PResult<()> {
    let mut c = ctx.sql.conn().await?;
    sqlx::query("DELETE FROM signal_dedupe WHERE until < now() - interval '1 day'")
        .execute(&mut *c)
        .await?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalItem {
    pub id: u64,
    pub user_id: u64,
    pub torrent_id: Option<u64>,
    pub kind: String,
    pub severity: String,
    pub hits: i64,
    pub details: Value,
    pub first_at: i64,
    pub last_at: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignalPage {
    pub items: Vec<SignalItem>,
    pub next_cursor: Option<String>,
}

/// 查询（`ctx.accounting.signals`、后台）：按创建时间倒序，游标为 `<created_ms>:<id>`；区间 ≤ 366 天
pub async fn list(
    ctx: &Ctx,
    user: Option<i64>,
    signal: Option<&str>,
    from_ms: i64,
    to_ms: i64,
    cursor: Option<&str>,
    limit: i64,
) -> PResult<SignalPage> {
    if to_ms < from_ms || to_ms - from_ms > 366 * 86_400_000 {
        return Err(PluginError::validation("range must be 0..=366 days"));
    }
    let cur = match cursor {
        Some(s) => {
            let (a, b) = s
                .split_once(':')
                .and_then(|(a, b)| Some((a.parse::<i64>().ok()?, b.parse::<i64>().ok()?)))
                .ok_or_else(|| PluginError::bad_request("bad cursor"))?;
            Some((a, b))
        }
        None => None,
    };
    let limit = limit.clamp(1, 200);
    let mut c = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT id, user_id, torrent_id, signal, severity, hits, details,
                (extract(epoch FROM first_at) * 1000)::bigint, (extract(epoch FROM last_at) * 1000)::bigint,
                (extract(epoch FROM created_at) * 1000)::bigint
           FROM signals
          WHERE ($1::bigint IS NULL OR user_id = $1) AND ($2::text IS NULL OR signal = $2)
            AND created_at >= to_timestamp($3::float8 / 1000) AND created_at <= to_timestamp($4::float8 / 1000)
            AND ($5::bigint IS NULL OR (created_at, id) < (to_timestamp($5::float8 / 1000), $6))
          ORDER BY created_at DESC, id DESC LIMIT $7",
    )
    .bind(user)
    .bind(signal)
    .bind(from_ms)
    .bind(to_ms)
    .bind(cur.map(|c| c.0))
    .bind(cur.map_or(0, |c| c.1))
    .bind(limit + 1)
    .fetch_all(&mut *c)
    .await?;
    let more = rows.len() > usize::try_from(limit).unwrap_or(0);
    let items: Vec<SignalItem> = rows
        .iter()
        .take(usize::try_from(limit).unwrap_or(0))
        .map(|r| SignalItem {
            id: u64::try_from(r.get::<i64, _>(0)).unwrap_or(0),
            user_id: u64::try_from(r.get::<i64, _>(1)).unwrap_or(0),
            torrent_id: u64::try_from(r.get::<i64, _>(2)).ok().filter(|t| *t > 0),
            kind: r.get(3),
            severity: r.get(4),
            hits: i64::from(r.get::<i32, _>(5)),
            details: r.get(6),
            first_at: r.get(7),
            last_at: r.get(8),
            created_at: r.get(9),
        })
        .collect();
    let next_cursor = more
        .then(|| items.last().map(|x| format!("{}:{}", x.created_at, x.id)))
        .flatten();
    Ok(SignalPage { items, next_cursor })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::StreamId;

    fn entry(uid: u64, tid: u64) -> Entry {
        Entry {
            id: StreamId(1, 0),
            eid: "e".into(),
            ts: 1_700_000_000,
            uid,
            tid,
            pid: [b'p'; 20],
            ev: Ev::None,
            left: 0,
            dup: 0,
            ddown: 0,
            dseed: 0,
            dleech: 0,
            flags: 0,
            client: "-qB5120-".into(),
            rep_up: Some(0),
            rep_down: Some(0),
            xup: 0,
            xdown: 0,
            out_of_range: false,
            ip: None,
        }
    }

    fn kinds(v: &[Sig]) -> Vec<(&'static str, Severity, String)> {
        v.iter()
            .map(|s| {
                (
                    s.signal,
                    s.severity,
                    s.details["kind"].as_str().unwrap_or("").to_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn entry_rules() {
        let cfg = SignalCfg::default();
        let sizes = HashMap::from([(5, 4u64 << 30)]);
        let leech = HashSet::from([(5, 9), (6, 1)]);
        let sw = Swarm {
            sizes: &sizes,
            leechers: &leech,
        };
        // 正常：有别人在下载，速度正常
        let mut e = entry(1, 5);
        e.dup = 2 << 30;
        e.dseed = 900;
        assert!(evaluate(&e, &cfg, &sw).is_empty());
        // 2 GiB / 10 s ≈ 1.7 倍上限 → medium；2 GiB / 5 s ≈ 3.4 倍 → high
        e.dseed = 10;
        assert_eq!(
            kinds(&evaluate(&e, &cfg, &sw)),
            [("speed_exceeded", Severity::Medium, String::new())]
        );
        e.dseed = 5;
        assert_eq!(
            kinds(&evaluate(&e, &cfg, &sw)),
            [("speed_exceeded", Severity::High, String::new())]
        );
        // tracker 截断：dup = 0、xup 给出被丢弃的量
        let mut t = entry(1, 5);
        t.flags = FLAG_SPEED_EXCEEDED;
        t.xup = 50 << 30;
        t.dseed = 900;
        let s = evaluate(&t, &cfg, &sw);
        assert_eq!(s[0].details["kind"], "tracker_cap");
        assert_eq!(s[0].details["bytes"], 50u64 << 30);
        // 首报 started 带累计值（伪造）与普通状态丢失
        let mut f = entry(1, 5);
        f.ev = Ev::Started;
        f.flags = FLAG_BASELINE_RESET;
        f.rep_up = Some(50 << 30);
        assert_eq!(
            kinds(&evaluate(&f, &cfg, &sw)),
            [("counter_jump", Severity::Medium, "started_nonzero".into())]
        );
        f.ev = Ev::None;
        assert_eq!(
            kinds(&evaluate(&f, &cfg, &sw)),
            [("counter_jump", Severity::Low, "baseline_reset_nonstart".into())]
        );
        // 回退、超体积、无下载者
        let mut g = entry(1, 5);
        g.flags = FLAG_UPLOAD_REGRESSED;
        g.left = 5 << 30;
        g.dup = 13 << 30;
        g.dseed = 1800;
        let k = kinds(&evaluate(
            &g,
            &cfg,
            &Swarm {
                sizes: &sizes,
                leechers: &HashSet::new(),
            },
        ));
        assert!(k.contains(&("negative_delta", Severity::Low, "upload_regressed".into())));
        assert!(k.contains(&("counter_jump", Severity::Medium, "delta_exceeds_size".into())));
        assert!(k.contains(&("counter_jump", Severity::Medium, "left_exceeds_size".into())));
        assert!(k.contains(&("peer_not_in_swarm", Severity::Medium, "no_leechers".into())));
        // 只有自己在下载：仍算无对端
        let only_me = HashSet::from([(5, 1)]);
        let mut h = entry(1, 5);
        h.dup = 200 << 20;
        h.dseed = 1800;
        assert_eq!(
            kinds(&evaluate(
                &h,
                &cfg,
                &Swarm {
                    sizes: &sizes,
                    leechers: &only_me
                }
            )),
            [("peer_not_in_swarm", Severity::Low, "no_leechers".into())]
        );
    }

    #[test]
    fn merging() {
        let s = |sev, at| Sig {
            uid: 1,
            tid: 2,
            signal: "speed_exceeded",
            severity: sev,
            details: json!({ "sev": format!("{sev:?}") }),
            at,
        };
        let m = merge(vec![
            s(Severity::Medium, 5),
            s(Severity::High, 9),
            s(Severity::Low, 3),
        ]);
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].1, m[0].0.severity, m[0].0.at), (3, Severity::High, 3));
        assert_eq!(m[0].0.details["sev"], "High");
    }
}
