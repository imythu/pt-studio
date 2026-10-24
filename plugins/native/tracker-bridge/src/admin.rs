//! 后台接口：客户端规则（LLD §1.5、§2.4）、被拒客户端排行、节点健康检查（`node_health`，LLD §1.9）。

use std::collections::HashMap;

use axum::{
    Json,
    extract::Query,
    http::{HeaderMap, HeaderValue, header},
    response::{IntoResponse, Response},
};
use pt_plugin_api::{AuditEntry, Ctx, NewEvent, PResult, PluginError, RequestCtx, json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;

use crate::clients::{self, Rule};
use crate::{DEFAULT_CLIENTS, NODE_LAG_ALERT_MS, PUT_JSON, ROLE, eval, nodes, redis_err};

/// 当前规则集（数据库）
pub struct RuleSet {
    pub version: i32,
    pub default_action: String,
    pub rules: Vec<Rule>,
}

pub async fn load(ctx: &Ctx) -> PResult<RuleSet> {
    let mut c = ctx.sql.conn().await?;
    let head = sqlx::query("SELECT version, default_action FROM client_ruleset WHERE id = 1")
        .fetch_one(&mut *c)
        .await?;
    let rules = sqlx::query(
        "SELECT id, name, action, pid_prefix, pid_regex, min_ver, max_ver, ua_prefix, ua_regex, enabled, note
           FROM client_rules ORDER BY position",
    )
    .fetch_all(&mut *c)
    .await?
    .iter()
    .map(|r| Rule {
        id: Some(r.get(0)),
        name: r.get(1),
        action: r.get(2),
        pid_prefix: r.get(3),
        pid_regex: r.get(4),
        min_ver: r.get(5),
        max_ver: r.get(6),
        ua_prefix: r.get(7),
        ua_regex: r.get(8),
        enabled: r.get(9),
        note: r.get(10),
    })
    .collect();
    Ok(RuleSet {
        version: head.get(0),
        default_action: head.get(1),
        rules,
    })
}

/// 整体替换规则（新规则重新分配 ID 之外保留原 ID）；返回新版本号
async fn replace(ctx: &Ctx, default_action: &str, rules: &[Rule], expected: Option<i32>) -> PResult<i32> {
    let by = ctx.user_id().and_then(|u| i64::try_from(u).ok());
    let mut c = ctx.sql.conn().await?;
    let version: Option<i32> = sqlx::query_scalar(
        "UPDATE client_ruleset SET version = version + 1, default_action = $1, updated_at = now(), updated_by = $2
          WHERE id = 1 AND ($3::int IS NULL OR version = $3) RETURNING version",
    )
    .bind(default_action)
    .bind(by)
    .bind(expected)
    .fetch_optional(&mut *c)
    .await?;
    let Some(version) = version else {
        let cur: i32 = sqlx::query_scalar("SELECT version FROM client_ruleset WHERE id = 1")
            .fetch_one(&mut *c)
            .await?;
        return Err(PluginError::new("kernel.precondition_failed", "client rules were changed by someone else")
            .with_details(json!({ "current_rev": cur.to_string() })));
    };
    sqlx::query("DELETE FROM client_rules").execute(&mut *c).await?;
    for (i, r) in rules.iter().enumerate() {
        sqlx::query(
            "INSERT INTO client_rules (id, position, name, action, pid_prefix, pid_regex, min_ver, max_ver, ua_prefix, ua_regex,
                                       enabled, note, updated_by)
             VALUES (COALESCE($1, nextval(pg_get_serial_sequence('client_rules', 'id'))), $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
        )
        .bind(r.id)
        .bind(i32::try_from(i).unwrap_or(i32::MAX))
        .bind(&r.name)
        .bind(&r.action)
        .bind(&r.pid_prefix)
        .bind(&r.pid_regex)
        .bind(&r.min_ver)
        .bind(&r.max_ver)
        .bind(&r.ua_prefix)
        .bind(&r.ua_regex)
        .bind(r.enabled)
        .bind(&r.note)
        .bind(by)
        .execute(&mut *c)
        .await?;
    }
    // 显式 ID 插入后让序列跟上
    sqlx::query(
        "SELECT setval(pg_get_serial_sequence('client_rules', 'id'), GREATEST((SELECT max(id) FROM client_rules), 1))",
    )
    .execute(&mut *c)
    .await?;
    Ok(version)
}

/// 规则集为空（版本 0）时从旧版设置迁移：`clients` 白名单 + `allow_unknown_clients`
pub async fn ensure(ctx: &Ctx) -> PResult<RuleSet> {
    let set = load(ctx).await?;
    if set.version > 0 {
        return Ok(set);
    }
    let (default_action, rules) = clients::from_legacy(&ctx.config.get_settings().await, DEFAULT_CLIENTS);
    match replace(ctx, &default_action, &rules, Some(0)).await {
        Ok(_) => load(ctx).await,
        // 另一个副本已经迁移：用它的结果
        Err(e) if e.code == "kernel.precondition_failed" => load(ctx).await,
        Err(e) => Err(e),
    }
}

/// 写 `trk:clients` 并记录导出的 ver
pub async fn export(ctx: &Ctx, set: &RuleSet) -> PResult<i64> {
    let doc = clients::export(&set.default_action, &set.rules).to_string();
    let ver = eval(ctx, PUT_JSON, "trk:clients", "clients", "clients", &[("json", doc)]).await?;
    let mut c = ctx.sql.conn().await?;
    sqlx::query("UPDATE client_ruleset SET exported_ver = $1 WHERE id = 1")
        .bind(ver)
        .execute(&mut *c)
        .await?;
    Ok(ver)
}

fn etag(version: i32) -> HeaderValue {
    HeaderValue::from_str(&format!("\"{version}\"")).unwrap_or_else(|_| HeaderValue::from_static("\"0\""))
}

async fn rules_json(ctx: &Ctx, set: &RuleSet) -> PResult<Response> {
    let mut conn = ctx.redis.global(ROLE)?;
    let cur: Option<String> = redis::cmd("GET")
        .arg("trk:clients")
        .query_async(&mut conn)
        .await
        .map_err(|e| redis_err(&e))?;
    let redis_ver = cur
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v["ver"].as_i64());
    let mut c = ctx.sql.conn().await?;
    let exported: Option<i64> = sqlx::query_scalar("SELECT exported_ver FROM client_ruleset WHERE id = 1")
        .fetch_one(&mut *c)
        .await?;
    let body = json!({
        "version": set.version,
        "default_action": set.default_action,
        "rules": set.rules,
        "redis_ver": redis_ver,
        "synced": redis_ver.is_some() && redis_ver >= exported,
    });
    let mut r = Json(body).into_response();
    r.headers_mut().insert(header::ETAG, etag(set.version));
    Ok(r)
}

/// `GET /v1/admin/client-rules`
pub async fn get_rules(ctx: RequestCtx) -> PResult<Response> {
    let set = ensure(&ctx).await?;
    rules_json(&ctx, &set).await
}

#[derive(Deserialize)]
pub struct RulesBody {
    default_action: String,
    rules: Vec<Rule>,
}

fn invalid(errs: &[clients::FieldError]) -> PluginError {
    PluginError::new("tracker-bridge.rules_invalid", "client rules are invalid")
        .with_status(422)
        .with_details(json!({ "errors": errs }))
}

/// `PUT /v1/admin/client-rules`（`If-Match: "<version>"`）
pub async fn put_rules(ctx: RequestCtx, headers: HeaderMap, Json(b): Json<RulesBody>) -> PResult<Response> {
    let expected = headers
        .get(header::IF_MATCH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().trim_start_matches("W/").trim_matches('"').parse::<i32>().ok())
        .ok_or_else(|| PluginError::bad_request("If-Match header with the rule set version is required"))?;
    let rules: Vec<Rule> = b.rules.into_iter().map(Rule::normalized).collect();
    let errs = clients::validate(&b.default_action, &rules);
    if !errs.is_empty() {
        return Err(invalid(&errs));
    }
    let before = ensure(&ctx).await?;
    // 只保留属于当前规则集的 ID（未知或重复的 ID 当作新规则）
    let known: Vec<i64> = before.rules.iter().filter_map(|r| r.id).collect();
    let mut used = Vec::new();
    let rules: Vec<Rule> = rules
        .into_iter()
        .map(|mut r| {
            r.id = r.id.filter(|id| known.contains(id) && !used.contains(id));
            used.extend(r.id);
            r
        })
        .collect();
    let version = replace(&ctx, &b.default_action, &rules, Some(expected)).await?;
    let set = load(&ctx).await?;
    export(&ctx, &set).await?;
    let ids = |s: &[Rule]| s.iter().filter_map(|r| r.id).collect::<Vec<_>>();
    let (old, new) = (ids(&before.rules), ids(&set.rules));
    let changed = set
        .rules
        .iter()
        .filter(|r| before.rules.iter().any(|o| o.id == r.id && *o != **r))
        .count();
    ctx.audit
        .write(AuditEntry {
            action: "tracker-bridge.client_rules_set".into(),
            target: "global".into(),
            details: json!({
                "version": version,
                "default_action": set.default_action,
                "added": new.iter().filter(|i| !old.contains(i)).count(),
                "removed": old.iter().filter(|i| !new.contains(i)).count(),
                "changed": changed,
            }),
        })
        .await?;
    ctx.events
        .publish(NewEvent::v1(
            "tracker.client_rules_changed",
            "global",
            json!({ "version": version, "rule_count": set.rules.len() }),
        ))
        .await?;
    rules_json(&ctx, &set).await
}

#[derive(Deserialize)]
pub struct TestBody {
    peer_id: String,
    #[serde(default)]
    user_agent: String,
    /// 测试草稿；为空时用当前规则集
    rules: Option<RulesBody>,
}

/// `POST /v1/admin/client-rules:test`
pub async fn test_rules(ctx: RequestCtx, Json(b): Json<TestBody>) -> PResult<Json<Value>> {
    let pid = clients::parse_peer_id(b.peer_id.trim()).ok_or_else(|| {
        PluginError::new(
            "tracker-bridge.invalid_peer_id",
            "peer_id must be up to 20 bytes: text, %XX escapes or 40 hex digits",
        )
        .with_status(422)
    })?;
    let (default_action, rules) = match b.rules {
        Some(d) => {
            let rules: Vec<Rule> = d.rules.into_iter().map(Rule::normalized).collect();
            let errs = clients::validate(&d.default_action, &rules);
            if !errs.is_empty() {
                return Err(invalid(&errs));
            }
            (d.default_action, rules)
        }
        None => {
            let s = ensure(&ctx).await?;
            (s.default_action, s.rules)
        }
    };
    let d = clients::decide(&default_action, &rules, &pid, b.user_agent.as_bytes());
    let hit = d.index.and_then(|i| rules.get(i));
    Ok(Json(json!({
        "action": d.action,
        "rule_index": d.index,
        "rule_id": hit.and_then(|r| r.id),
        "rule_name": hit.map(|r| r.name.clone()),
        "extracted_ver": d.version,
        "peer_id": clients::escape(&pid),
    })))
}

/// `GET /v1/admin/client-rule-presets`
pub async fn presets(_ctx: RequestCtx) -> Json<Value> {
    Json(json!({ "items": clients::presets() }))
}

#[derive(Deserialize)]
pub struct DeniedQuery {
    /// `YYYY-MM-DD`（UTC），缺省为今天
    date: Option<String>,
    limit: Option<usize>,
}

/// `2026-10-05` → `20261005`
fn day_key(date: &str) -> Option<String> {
    let b = date.as_bytes();
    let ok = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    ok.then(|| date.replace('-', ""))
}

/// ZSET member（`<peer_id 前 8 字节转义>|<UA>`）→ 前缀与 UA
fn denied_item(member: &str, count: f64) -> Value {
    let (pid, ua) = member.split_once('|').unwrap_or((member, ""));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let count = count.max(0.0).round() as u64;
    json!({ "peer_id_prefix": pid, "user_agent": ua, "count": count, "overflow": pid == "~overflow" })
}

/// `GET /v1/admin/denied-clients?date=&limit=`（pt-tracker 写的 `trk:stats:denied:<yyyymmdd>`，保留 8 天）
pub async fn denied(ctx: RequestCtx, Query(q): Query<DeniedQuery>) -> PResult<Json<Value>> {
    let date = match q.date.as_deref().filter(|d| !d.is_empty()) {
        Some(d) => d.to_owned(),
        None => chrono::DateTime::from_timestamp_millis(ctx.time.now())
            .unwrap_or_default()
            .format("%Y-%m-%d")
            .to_string(),
    };
    let key = day_key(&date).ok_or_else(|| PluginError::validation("date must be YYYY-MM-DD"))?;
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let mut conn = ctx.redis.global(ROLE)?;
    let rows: Vec<(String, f64)> = redis::cmd("ZREVRANGE")
        .arg(format!("trk:stats:denied:{key}"))
        .arg(0)
        .arg(limit - 1)
        .arg("WITHSCORES")
        .query_async(&mut conn)
        .await
        .map_err(|e| redis_err(&e))?;
    let total: f64 = redis::cmd("EVAL")
        .arg("local s = 0 for _, v in ipairs(redis.call('ZRANGE', KEYS[1], 0, -1, 'WITHSCORES')) do s = s + (tonumber(v) or 0) end return tostring(s)")
        .arg(1)
        .arg(format!("trk:stats:denied:{key}"))
        .query_async::<String>(&mut conn)
        .await
        .map_err(|e| redis_err(&e))?
        .parse()
        .unwrap_or(0.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let total = total.max(0.0).round() as u64;
    Ok(Json(json!({
        "date": date,
        "total": total,
        "items": rows.iter().map(|(m, n)| denied_item(m, *n)).collect::<Vec<_>>(),
    })))
}

/// 节点健康状态（KV `node_health`）：见过的节点与当前告警
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Health {
    #[serde(default)]
    pub nodes: HashMap<String, NodeSeen>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeSeen {
    /// 最近一次看到节点状态键（毫秒）
    pub last_seen: u64,
    #[serde(default)]
    pub lost: bool,
    #[serde(default)]
    pub lagging: bool,
}

/// 节点状态键过期（30 秒）后再等 2 分钟仍未出现视为失联
pub const NODE_LOST_AFTER_MS: u64 = 150_000;
/// 24 小时未出现的节点从列表中移除（视为已下线）
const NODE_FORGET_AFTER_MS: u64 = 86_400_000;

/// 一次检查产生的告警
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Alert {
    Lag { node: String, behind_ms: u64 },
    Lost { node: String },
    Recovered { node: String },
}

/// 纯函数：根据本次看到的节点（ID → 落后毫秒）更新健康状态，返回新出现的告警
pub fn check(h: &mut Health, seen: &[(String, u64)], now_ms: u64) -> Vec<Alert> {
    let mut out = Vec::new();
    for (id, behind) in seen {
        let n = h.nodes.entry(id.clone()).or_default();
        if n.lost {
            out.push(Alert::Recovered { node: id.clone() });
        }
        n.last_seen = now_ms;
        n.lost = false;
        let lagging = *behind > NODE_LAG_ALERT_MS;
        if lagging && !n.lagging {
            out.push(Alert::Lag {
                node: id.clone(),
                behind_ms: *behind,
            });
        }
        n.lagging = lagging;
    }
    h.nodes.retain(|id, n| {
        if seen.iter().any(|(s, _)| s == id) {
            return true;
        }
        let gone = now_ms.saturating_sub(n.last_seen);
        if gone > NODE_FORGET_AFTER_MS {
            return false;
        }
        if gone > NODE_LOST_AFTER_MS && !n.lost {
            n.lost = true;
            n.lagging = false;
            out.push(Alert::Lost { node: id.clone() });
        }
        true
    });
    out
}

const HEALTH_KEY: &str = "node_health";

pub async fn health(ctx: &Ctx) -> PResult<Health> {
    Ok(ctx
        .kv
        .get(HEALTH_KEY)
        .await?
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default())
}

/// cron `node_health`（每分钟）：节点延迟 / 失联告警（指标、日志、`tracker.inconsistency_detected` 事件）
pub async fn node_health(ctx: &Ctx) -> PResult<()> {
    let snap = nodes(ctx).await?;
    let now_ms = u64::try_from(ctx.time.now()).unwrap_or(0);
    let seen: Vec<(String, u64)> = snap
        .nodes
        .iter()
        .filter_map(|n| Some((n["node_id"].as_str()?.to_owned(), n["behind_ms"].as_u64().unwrap_or(0))))
        .collect();
    let mut h = health(ctx).await?;
    let before = h.clone();
    let alerts = check(&mut h, &seen, now_ms);
    #[allow(clippy::cast_precision_loss)]
    {
        ctx.metrics.gauge_set("nodes_online", seen.len() as f64, &[]);
        for (id, behind) in &seen {
            ctx.metrics.gauge_set("node_lag_ms", *behind as f64, &[("node", id)]);
        }
    }
    for a in &alerts {
        let (kind, node, extra) = match a {
            Alert::Lag { node, behind_ms } => {
                tracing::warn!(node = %node, behind_ms, "tracker node is lagging behind trk:sync");
                ("node_lag", node, json!({ "behind_ms": behind_ms }))
            }
            Alert::Lost { node } => {
                tracing::warn!(node = %node, "tracker node stopped reporting");
                ("node_lost", node, json!({}))
            }
            Alert::Recovered { node } => {
                tracing::info!(node = %node, "tracker node is reporting again");
                continue;
            }
        };
        let mut payload = json!({ "kind": kind, "count": 1, "repaired": 0, "node_id": node });
        if let (Some(p), Some(e)) = (payload.as_object_mut(), extra.as_object()) {
            p.extend(e.clone());
        }
        ctx.events
            .publish(NewEvent::v1("tracker.inconsistency_detected", "global", payload))
            .await?;
    }
    if h != before {
        let b = serde_json::to_vec(&h).map_err(|e| PluginError::internal(e.to_string()))?;
        ctx.kv.set(HEALTH_KEY, &b, None).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_alerts() {
        let mut h = Health::default();
        let s = |id: &str, b: u64| vec![(id.to_owned(), b)];
        assert!(check(&mut h, &s("n1", 0), 1_000).is_empty());
        // 延迟告警只在进入时发一次
        assert_eq!(
            check(&mut h, &s("n1", 9_000), 61_000),
            vec![Alert::Lag { node: "n1".into(), behind_ms: 9_000 }]
        );
        assert!(check(&mut h, &s("n1", 9_000), 121_000).is_empty());
        assert!(check(&mut h, &s("n1", 0), 181_000).is_empty());
        // 失联：最后一次看到之后超过 150 秒
        assert!(check(&mut h, &[], 300_000).is_empty());
        assert_eq!(check(&mut h, &[], 332_000), vec![Alert::Lost { node: "n1".into() }]);
        assert!(check(&mut h, &[], 392_000).is_empty());
        assert_eq!(check(&mut h, &s("n1", 0), 452_000), vec![Alert::Recovered { node: "n1".into() }]);
        // 24 小时未出现则忘记
        check(&mut h, &[], 452_000 + NODE_LOST_AFTER_MS + 1);
        assert!(h.nodes["n1"].lost);
        check(&mut h, &[], 452_000 + 86_400_001);
        assert!(h.nodes.is_empty());
    }

    #[test]
    fn denied_items() {
        assert_eq!(day_key("2026-10-05").as_deref(), Some("20261005"));
        assert!(day_key("2026-1-05").is_none() && day_key("20261005").is_none() && day_key("2026-10-0x").is_none());
        let v = denied_item("-XL0012-|Xunlei 0.0.1.2", 1834.0);
        assert_eq!((v["peer_id_prefix"].as_str(), v["user_agent"].as_str(), v["count"].as_u64()), (Some("-XL0012-"), Some("Xunlei 0.0.1.2"), Some(1834)));
        assert_eq!(denied_item("~overflow|", 3.0)["overflow"], true);
    }
}
