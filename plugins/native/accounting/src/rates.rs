//! 倍率规则（accounting 03 §1.3）：模型、索引、有效倍率计算与规则写入。

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use pt_plugin_api::{Ctx, PResult, PluginError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{PgConnection, Row};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "ids", rename_all = "snake_case")]
pub enum RateScope {
    Global,
    Torrents(Vec<u64>),
    Users(Vec<u64>),
    UserTorrent((u64, u64)),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RateMode {
    Favorable,
    Override,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateRule {
    pub external_ref: String,
    pub scope: RateScope,
    pub up_permille: Option<u32>,
    pub down_permille: Option<u32>,
    pub mode: RateMode,
    #[serde(default)]
    pub priority: i32,
    pub starts_at: i64,
    pub ends_at: Option<i64>,
    #[serde(default)]
    pub reason_term: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveRate {
    pub up_permille: u32,
    pub down_permille: u32,
}

/// 索引中的规则（时间为毫秒，有效结束 = min(ends_at, removed_at)）
#[derive(Debug, Clone)]
pub struct Loaded {
    pub id: u64,
    pub up: Option<u32>,
    pub down: Option<u32>,
    pub mode: RateMode,
    pub priority: i32,
    pub starts_at: i64,
    pub end: Option<i64>,
}

#[derive(Debug, Default, Clone)]
pub struct RateIndex {
    pub global: Vec<Loaded>,
    pub by_torrent: HashMap<u64, Vec<Loaded>>,
    pub by_user: HashMap<u64, Vec<Loaded>>,
    pub by_pair: HashMap<(u64, u64), Vec<Loaded>>,
}

/// 有效倍率与命中规则 ID（升序）。
pub fn effective(idx: &RateIndex, uid: u64, tid: u64, t: i64, max_up: u32) -> (u32, u32, Vec<u64>) {
    let active = |r: &&Loaded| r.starts_at <= t && r.end.is_none_or(|e| t < e);
    let mut cands: Vec<&Loaded> = idx
        .global
        .iter()
        .chain(idx.by_torrent.get(&tid).into_iter().flatten())
        .chain(idx.by_user.get(&uid).into_iter().flatten())
        .chain(idx.by_pair.get(&(uid, tid)).into_iter().flatten())
        .filter(active)
        .collect();
    cands.sort_by_key(|r| r.id);
    cands.dedup_by_key(|r| r.id);
    let mut ov: Vec<&&Loaded> = cands.iter().filter(|r| r.mode == RateMode::Override).collect();
    ov.sort_by(|a, b| b.priority.cmp(&a.priority).then(b.id.cmp(&a.id)));
    let mut up = None;
    let mut down = None;
    for r in ov {
        if up.is_none() {
            up = r.up;
        }
        if down.is_none() {
            down = r.down;
        }
    }
    let fav = || cands.iter().filter(|r| r.mode == RateMode::Favorable);
    let up = up.unwrap_or_else(|| fav().filter_map(|r| r.up).fold(1000, u32::max));
    let down = down.unwrap_or_else(|| fav().filter_map(|r| r.down).fold(1000, u32::min));
    (up.min(max_up), down, cands.iter().map(|r| r.id).collect())
}

pub fn rate_key(up: u32, down: u32, ids: &[u64]) -> i64 {
    let mut buf = Vec::with_capacity(8 + ids.len() * 8);
    buf.extend_from_slice(&up.to_le_bytes());
    buf.extend_from_slice(&down.to_le_bytes());
    for id in ids {
        buf.extend_from_slice(&id.to_le_bytes());
    }
    i64::from_le_bytes(xxhash_rust::xxh3::xxh3_64(&buf).to_le_bytes())
}

/// 计入量 = 原始量 × 千分比 / 1000（向下取整）
pub fn credited(raw: u64, permille: u32) -> u64 {
    u64::try_from(u128::from(raw) * u128::from(permille) / 1000).unwrap_or(u64::MAX)
}

/// 从数据库装载未结束（或结束于 2 h 内）的规则。
pub async fn load_index(c: &mut PgConnection) -> PResult<RateIndex> {
    let rows = sqlx::query(
        "SELECT r.id, r.scope_kind, r.up_permille, r.down_permille, r.mode, r.priority, r.starts_at,
                least(r.ends_at, r.removed_at) AS eff_end, t.torrent_id, t.user_id
           FROM rate_rules r LEFT JOIN rate_rule_targets t ON t.rule_id = r.id
          WHERE least(r.ends_at, r.removed_at) IS NULL OR least(r.ends_at, r.removed_at) > now() - interval '2 hours'",
    )
    .fetch_all(&mut *c)
    .await?;
    let mut idx = RateIndex::default();
    for r in rows {
        let l = Loaded {
            id: u64::try_from(r.get::<i64, _>(0)).unwrap_or(0),
            up: r.get::<Option<i32>, _>(2).and_then(|v| u32::try_from(v).ok()),
            down: r.get::<Option<i32>, _>(3).and_then(|v| u32::try_from(v).ok()),
            mode: if r.get::<String, _>(4) == "override" {
                RateMode::Override
            } else {
                RateMode::Favorable
            },
            priority: r.get(5),
            starts_at: r.get::<DateTime<Utc>, _>(6).timestamp_millis(),
            end: r.get::<Option<DateTime<Utc>>, _>(7).map(|d| d.timestamp_millis()),
        };
        let tid = u64::try_from(r.get::<Option<i64>, _>(8).unwrap_or(0)).unwrap_or(0);
        let uid = u64::try_from(r.get::<Option<i64>, _>(9).unwrap_or(0)).unwrap_or(0);
        match r.get::<String, _>(1).as_str() {
            "global" => idx.global.push(l),
            "torrents" => idx.by_torrent.entry(tid).or_default().push(l),
            "users" => idx.by_user.entry(uid).or_default().push(l),
            _ => idx.by_pair.entry((uid, tid)).or_default().push(l),
        }
    }
    Ok(idx)
}

fn invalid(msg: &str) -> PluginError {
    PluginError::new("accounting.rate_rule_invalid", msg.to_owned())
}

/// `set_rate_rule`：以 (调用方, external_ref) 幂等；内容不同则结束旧规则并新建（supersedes）。
pub async fn set_rate_rule(ctx: &Ctx, owner: &str, rule: RateRule, max_up: u32) -> PResult<u64> {
    let ref_ok = (1..=128).contains(&rule.external_ref.len())
        && rule
            .external_ref
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b":._-".contains(&b));
    if !ref_ok {
        return Err(invalid("external_ref must match [A-Za-z0-9:._-]{1,128}"));
    }
    if rule.up_permille.is_none() && rule.down_permille.is_none() {
        return Err(invalid("at least one of up_permille / down_permille is required"));
    }
    if rule.up_permille.is_some_and(|u| u > max_up) || rule.down_permille.is_some_and(|d| d > 10_000) {
        return Err(invalid("rate out of range"));
    }
    if rule.mode == RateMode::Favorable
        && (rule.up_permille.is_some_and(|u| u < 1000) || rule.down_permille.is_some_and(|d| d > 1000))
    {
        return Err(invalid(
            "favorable rules must have up >= 1000 and down <= 1000; use override for penalties",
        ));
    }
    if rule.ends_at.is_some_and(|e| e <= rule.starts_at) {
        return Err(invalid("ends_at must be after starts_at"));
    }
    let targets: Vec<(i64, i64)> = match &rule.scope {
        RateScope::Global => vec![],
        RateScope::Torrents(ids) | RateScope::Users(ids) if ids.is_empty() || ids.len() > 10_000 => {
            return Err(invalid("scope must list 1..=10000 ids"));
        }
        RateScope::Torrents(ids) => ids.iter().map(|t| (i64::try_from(*t).unwrap_or(0), 0)).collect(),
        RateScope::Users(ids) => ids.iter().map(|u| (0, i64::try_from(*u).unwrap_or(0))).collect(),
        RateScope::UserTorrent((u, t)) => {
            vec![(i64::try_from(*t).unwrap_or(0), i64::try_from(*u).unwrap_or(0))]
        }
    };
    let kind = match rule.scope {
        RateScope::Global => "global",
        RateScope::Torrents(_) => "torrents",
        RateScope::Users(_) => "users",
        RateScope::UserTorrent(_) => "user_torrent",
    };
    let hash = Sha256::digest(serde_json::to_vec(&rule)?).to_vec();
    // 不允许追溯（追溯走重算）：开始时间最早为 now − 5 min
    let starts = rule.starts_at.max(Utc::now().timestamp_millis() - 300_000);
    let mut c = ctx.sql.conn().await?;
    let cur = sqlx::query("SELECT id, content_hash FROM rate_rules WHERE owner_plugin = $1 AND external_ref = $2 AND is_current FOR UPDATE")
        .bind(owner)
        .bind(&rule.external_ref)
        .fetch_optional(&mut *c)
        .await?;
    let mut supersedes = None;
    if let Some(r) = cur {
        let id: i64 = r.get(0);
        if r.get::<Vec<u8>, _>(1) == hash {
            return Ok(u64::try_from(id).unwrap_or(0));
        }
        sqlx::query("UPDATE rate_rules SET is_current = false, removed_at = coalesce(removed_at, now()), updated_at = now() WHERE id = $1")
            .bind(id)
            .execute(&mut *c)
            .await?;
        supersedes = Some(id);
    }
    let mode = if rule.mode == RateMode::Override {
        "override"
    } else {
        "favorable"
    };
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO rate_rules (owner_plugin, external_ref, scope_kind, up_permille, down_permille, mode, priority,
                                 starts_at, ends_at, supersedes, reason_term, content_hash)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) RETURNING id",
    )
    .bind(owner)
    .bind(&rule.external_ref)
    .bind(kind)
    .bind(rule.up_permille.and_then(|v| i32::try_from(v).ok()))
    .bind(rule.down_permille.and_then(|v| i32::try_from(v).ok()))
    .bind(mode)
    .bind(rule.priority)
    .bind(DateTime::<Utc>::from_timestamp_millis(starts))
    .bind(rule.ends_at.and_then(DateTime::<Utc>::from_timestamp_millis))
    .bind(supersedes)
    .bind(&rule.reason_term)
    .bind(&hash)
    .fetch_one(&mut *c)
    .await?;
    if !targets.is_empty() {
        let (ts, us): (Vec<i64>, Vec<i64>) = targets.into_iter().unzip();
        sqlx::query("INSERT INTO rate_rule_targets (rule_id, torrent_id, user_id) SELECT $1, t, u FROM unnest($2::bigint[], $3::bigint[]) AS x(t, u) ON CONFLICT DO NOTHING")
            .bind(id)
            .bind(&ts)
            .bind(&us)
            .execute(&mut *c)
            .await?;
    }
    drop(c);
    bump_version(ctx).await;
    Ok(u64::try_from(id).unwrap_or(0))
}

pub async fn remove_rate_rule(ctx: &Ctx, owner: Option<&str>, rule_id: u64) -> PResult<()> {
    let id = i64::try_from(rule_id).map_err(|_| PluginError::bad_request("bad rule id"))?;
    let mut c = ctx.sql.conn().await?;
    let r: Option<String> = sqlx::query_scalar("SELECT owner_plugin FROM rate_rules WHERE id = $1")
        .bind(id)
        .fetch_optional(&mut *c)
        .await?;
    match (r, owner) {
        (None, _) => {
            return Err(
                PluginError::new("accounting.rate_rule_not_found", "rate rule not found").with_status(404),
            );
        }
        (Some(o), Some(caller)) if o != caller => {
            return Err(PluginError::forbidden("rule belongs to another plugin"));
        }
        _ => {}
    }
    sqlx::query(
        "UPDATE rate_rules SET removed_at = coalesce(removed_at, now()), updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .execute(&mut *c)
    .await?;
    drop(c);
    bump_version(ctx).await;
    Ok(())
}

/// 规则版本号（私有 Redis 键）：worker 发现变化即重载索引。
pub async fn bump_version(ctx: &Ctx) {
    let r = ctx
        .redis
        .call(pt_plugin_api::RedisCommand::new("INCR").arg("rate:ver"))
        .await;
    if let Err(e) = r {
        tracing::warn!(error = %e, "rate:ver INCR failed; workers refresh within 10 s");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: u64, up: Option<u32>, down: Option<u32>, mode: RateMode, priority: i32) -> Loaded {
        Loaded {
            id,
            up,
            down,
            mode,
            priority,
            starts_at: 0,
            end: None,
        }
    }

    #[test]
    fn favorable_takes_best_override_wins() {
        let mut idx = RateIndex::default();
        idx.global.push(rule(1, None, Some(500), RateMode::Favorable, 0)); // 全站 50% 下载
        idx.by_torrent
            .insert(7, vec![rule(2, Some(2000), Some(0), RateMode::Favorable, 0)]); // 2x 免费
        assert_eq!(effective(&idx, 1, 7, 10, 10_000), (2000, 0, vec![1, 2]));
        assert_eq!(effective(&idx, 1, 8, 10, 10_000), (1000, 500, vec![1]));
        // 个人惩罚 override：上传 0x、下载 2x，覆盖免费
        idx.by_user
            .insert(1, vec![rule(3, Some(0), Some(2000), RateMode::Override, 10)]);
        assert_eq!(effective(&idx, 1, 7, 10, 10_000), (0, 2000, vec![1, 2, 3]));
        // 上限
        assert_eq!(effective(&idx, 2, 7, 10, 1500).0, 1500);
        // 过期
        idx.global[0].end = Some(5);
        assert_eq!(effective(&idx, 2, 8, 10, 10_000), (1000, 1000, vec![]));
        assert_eq!(credited(1001, 1500), 1501);
        assert_ne!(rate_key(1000, 1000, &[]), rate_key(1000, 1000, &[1]));
    }
}
