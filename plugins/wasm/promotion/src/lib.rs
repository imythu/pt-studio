//! promotion 插件（WASM，docs/plugins/promotion/）。
//!
//! 本版本实现：促销类型、单种促销（设置 / 结束 / 替换）、自动规则（`torrent.created` 时评估）、全站活动、
//! 个人促销卡（发放 / 使用）、下发到 accounting 倍率规则（先落库后下发，失败由巡检补齐）、到期、展示状态、
//! `ctx.promotions` 服务。做种数触发的自动规则重评估、年龄阈值巡检、搜索贡献、全量对账在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

wit_bindgen::generate!({
    path: "wit",
    world: "promotion",
    generate_all,
});

use exports::pt::plugin::{http_handler, on_cron, on_event, service_provider};
use pt::plugin::{
    events, invocation, services,
    sql::{self, SqlValue},
    types::PluginError,
};
use serde_json::{Value, json};

struct Promotion;

type R<T> = Result<T, PluginError>;

fn err(code: &str, message: &str) -> PluginError {
    PluginError {
        code: code.into(),
        message: message.into(),
        details: None,
        retryable: false,
    }
}

fn int(v: &SqlValue) -> i64 {
    match v {
        SqlValue::Int(i) => *i,
        _ => 0,
    }
}

fn opt_int(v: &SqlValue) -> Option<i64> {
    match v {
        SqlValue::Int(i) => Some(*i),
        _ => None,
    }
}

fn text(v: &SqlValue) -> String {
    match v {
        SqlValue::Text(s) => s.clone(),
        _ => String::new(),
    }
}

fn opt_text(v: &SqlValue) -> Option<String> {
    match v {
        SqlValue::Text(s) => Some(s.clone()),
        _ => None,
    }
}

fn accounting(method: &str, args: &Value) -> R<Value> {
    let out = services::call("ctx.accounting", method, &args.to_string())?;
    serde_json::from_str(&out).map_err(|_| err("promotion.bad_service_reply", "invalid JSON from accounting"))
}

fn ival(v: Option<i64>) -> SqlValue {
    v.map_or(SqlValue::Null, SqlValue::Int)
}

// ---------------- 类型（03 §1.1） ----------------

/// (up‰, down‰, override)
fn kind_rates(
    kind: &str,
    up: Option<i64>,
    down: Option<i64>,
    max_up: i64,
) -> R<(Option<i64>, Option<i64>, bool)> {
    Ok(match kind {
        "free" => (None, Some(0), false),
        "double_up" => (Some(2000), None, false),
        "double_up_free" => (Some(2000), Some(0), false),
        "half_down" => (None, Some(500), false),
        "thirty_down" => (None, Some(300), false),
        "double_up_half_down" => (Some(2000), Some(500), false),
        "neutral" => (Some(0), Some(0), true),
        "custom" => {
            // custom 只允许“更有利”方向；不利方向只能用 neutral
            if up.is_some_and(|u| !(1000..=max_up).contains(&u))
                || down.is_some_and(|d| !(0..=1000).contains(&d))
                || (up.is_none() && down.is_none())
            {
                return Err(err(
                    "promotion.rate_invalid",
                    "custom promotions need up in 1000..=max and/or down in 0..=1000",
                ));
            }
            (up, down, false)
        }
        _ => {
            return Err(err(
                "promotion.kind_disabled",
                "unknown or disabled promotion kind",
            ));
        }
    })
}

fn max_custom_up() -> i64 {
    let cfg: Value = serde_json::from_str(&pt::plugin::config::get_static()).unwrap_or(Value::Null);
    cfg["max_custom_up_permille"].as_i64().unwrap_or(5000)
}

// ---------------- 下发（03 §1.6） ----------------

struct Promo {
    id: i64,
    source: String,
    torrent_id: Option<i64>,
    user_id: Option<i64>,
    kind: String,
    up: Option<i64>,
    down: Option<i64>,
    starts_ms: i64,
    ends_ms: Option<i64>,
    external_ref: String,
    rate_rule_id: Option<i64>,
}

fn load(id: i64) -> R<Option<Promo>> {
    let r = sql::query(
        "SELECT id, source, torrent_id, user_id, kind, up_permille, down_permille,
                (extract(epoch FROM starts_at) * 1000)::bigint, (extract(epoch FROM ends_at) * 1000)::bigint,
                external_ref, rate_rule_id
           FROM promotions WHERE id = $1",
        &[SqlValue::Int(id)],
    )?;
    Ok(r.rows.first().map(|row| Promo {
        id: int(&row[0]),
        source: text(&row[1]),
        torrent_id: opt_int(&row[2]),
        user_id: opt_int(&row[3]),
        kind: text(&row[4]),
        up: opt_int(&row[5]),
        down: opt_int(&row[6]),
        starts_ms: int(&row[7]),
        ends_ms: opt_int(&row[8]),
        external_ref: text(&row[9]),
        rate_rule_id: opt_int(&row[10]),
    }))
}

fn push(p: &Promo) -> R<()> {
    let scope = match (p.source.as_str(), p.torrent_id, p.user_id) {
        ("card", Some(t), Some(u)) => json!({ "kind": "user_torrent", "ids": [u, t] }),
        (_, Some(t), _) => json!({ "kind": "torrents", "ids": [t] }),
        _ => json!({ "kind": "global" }),
    };
    let neutral = p.kind == "neutral";
    let rule = json!({
        "external_ref": p.external_ref, "scope": scope, "up_permille": p.up, "down_permille": p.down,
        "mode": if neutral { "override" } else { "favorable" }, "priority": if neutral { 100 } else { 0 },
        "starts_at": p.starts_ms, "ends_at": p.ends_ms, "reason_term": format!("promotion.{}", p.kind),
    });
    match accounting("set_rate_rule", &json!({ "rule": rule })) {
        Ok(v) => sql::execute(
            "UPDATE promotions SET rate_rule_id = $2, sync_state = 'synced', sync_error = NULL, updated_at = now() WHERE id = $1",
            &[SqlValue::Int(p.id), SqlValue::Int(v.as_i64().unwrap_or(0))],
        )
        .map(|_| ()),
        Err(e) => {
            // 先落库后下发：失败不回滚本插件事务，由巡检重试
            sql::execute(
                "UPDATE promotions SET sync_state = 'pending', sync_error = $2, sync_attempts = sync_attempts + 1, updated_at = now() WHERE id = $1",
                &[SqlValue::Int(p.id), SqlValue::Text(e.code)],
            )?;
            Ok(())
        }
    }
}

fn pull(p: &Promo) -> R<()> {
    let Some(rid) = p.rate_rule_id else {
        sql::execute(
            "UPDATE promotions SET sync_state = 'removed' WHERE id = $1",
            &[SqlValue::Int(p.id)],
        )?;
        return Ok(());
    };
    let state = match accounting("remove_rate_rule", &json!({ "rule_id": rid })) {
        Ok(_) => "removed",
        Err(_) => "remove_pending",
    };
    sql::execute(
        "UPDATE promotions SET sync_state = $2, updated_at = now() WHERE id = $1",
        &[SqlValue::Int(p.id), SqlValue::Text(state.into())],
    )?;
    Ok(())
}

fn history(p: &Promo, action: &str, actor: &str, reason: &str) -> R<()> {
    sql::execute(
        "INSERT INTO history (promotion_id, torrent_id, action, kind, source, actor, reason) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        &[
            SqlValue::Int(p.id),
            ival(p.torrent_id),
            SqlValue::Text(action.into()),
            SqlValue::Text(p.kind.clone()),
            SqlValue::Text(p.source.clone()),
            SqlValue::Text(actor.into()),
            SqlValue::Text(reason.into()),
        ],
    )?;
    Ok(())
}

fn publish(event_type: &str, key: String, payload: Value) -> R<()> {
    events::publish(&events::NewEvent {
        event_type: event_type.into(),
        version: 1,
        key,
        payload: payload.to_string(),
    })?;
    Ok(())
}

fn started(p: &Promo) -> R<()> {
    if p.source == "card" {
        return Ok(());
    }
    let (scope, key) = match p.torrent_id {
        Some(t) => ("torrent", format!("torrent:{t}")),
        None => ("global", "global".to_owned()),
    };
    publish(
        "promotion.started",
        key,
        json!({
        "promotion_id": p.id, "scope": scope, "source": p.source, "kind": p.kind,
        "torrent_ids": p.torrent_id.map_or(vec![], |t| vec![t]), "up_rate": p.up, "down_rate": p.down,
        "starts_at": p.starts_ms, "ends_at": p.ends_ms }),
    )
}

/// 结束一条促销（03 §1.2 end_internal）
fn end_internal(id: i64, cause: &str, actor: &str) -> R<bool> {
    let n = sql::execute(
        "UPDATE promotions SET state = 'ended', end_cause = $2, ended_at = now(), updated_at = now() WHERE id = $1 AND state = 'active'",
        &[SqlValue::Int(id), SqlValue::Text(cause.into())],
    )?;
    if n == 0 {
        return Ok(false);
    }
    let Some(p) = load(id)? else { return Ok(false) };
    pull(&p)?;
    history(&p, "end", actor, cause)?;
    if p.source != "card" {
        let (scope, key) = match p.torrent_id {
            Some(t) => ("torrent", format!("torrent:{t}")),
            None => ("global", "global".to_owned()),
        };
        publish(
            "promotion.ended",
            key,
            json!({ "promotion_id": p.id, "scope": scope, "source": p.source,
            "torrent_ids": p.torrent_id.map_or(vec![], |t| vec![t]), "cause": cause }),
        )?;
    }
    Ok(true)
}

struct NewPromo<'a> {
    source: &'a str,
    torrent_id: Option<i64>,
    user_id: Option<i64>,
    rule_id: Option<i64>,
    kind: &'a str,
    up: Option<i64>,
    down: Option<i64>,
    priority: i64,
    starts_ms: Option<i64>,
    ends_ms: Option<i64>,
    reason: &'a str,
    created_by: &'a str,
    title: Option<Value>,
}

fn create(n: &NewPromo<'_>) -> R<Promo> {
    let (up, down, _) = kind_rates(n.kind, n.up, n.down, max_custom_up())?;
    let prefix = match n.source {
        "manual" => "m",
        "auto" => "a",
        "global" => "g",
        _ => "c",
    };
    let id = int(&sql::query(
        "INSERT INTO promotions (source, torrent_id, user_id, rule_id, kind, up_permille, down_permille, priority, starts_at, ends_at,
                                 reason, created_by, state, sync_state, title)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8,
                 greatest(now(), coalesce(to_timestamp($9::float8 / 1000), now())), to_timestamp($10::float8 / 1000),
                 $11, $12, 'active', 'pending', $13)
         RETURNING id",
        &[SqlValue::Text(n.source.into()), ival(n.torrent_id), ival(n.user_id), ival(n.rule_id), SqlValue::Text(n.kind.into()),
          ival(up), ival(down), SqlValue::Int(n.priority), ival(n.starts_ms), ival(n.ends_ms), SqlValue::Text(n.reason.into()),
          SqlValue::Text(n.created_by.into()), n.title.as_ref().map_or(SqlValue::Null, |t| SqlValue::Json(t.to_string()))],
    )?
    .rows[0][0]);
    sql::execute(
        "UPDATE promotions SET external_ref = $2 WHERE id = $1",
        &[SqlValue::Int(id), SqlValue::Text(format!("{prefix}:{id}"))],
    )?;
    let p = load(id)?.ok_or_else(|| err("promotion.not_found", "promotion vanished"))?;
    history(&p, "start", n.created_by, n.reason)?;
    push(&p)?;
    started(&p)?;
    Ok(p)
}

// ---------------- 镜像与单种（03 §1.2、§1.7） ----------------

fn mirror_status(tid: i64) -> R<Option<(String, String, i64)>> {
    let r = sql::query(
        "SELECT coalesce(status, ''), coalesce(type_id, ''), coalesce(size, 0) FROM torrent_mirror WHERE torrent_id = $1",
        &[SqlValue::Int(tid)],
    )?;
    Ok(r.rows
        .first()
        .map(|row| (text(&row[0]), text(&row[1]), int(&row[2]))))
}

#[allow(clippy::too_many_arguments)]
fn set_single(
    tids: &[i64],
    kind: &str,
    up: Option<i64>,
    down: Option<i64>,
    ends_ms: Option<i64>,
    reason: &str,
    actor: &str,
) -> R<Vec<Value>> {
    let mut ids = tids.to_vec();
    ids.sort_unstable();
    ids.dedup();
    if ids.is_empty() || ids.len() > 1000 {
        return Err(err(
            "kernel.validation_failed",
            "torrent_ids must contain 1..=1000 ids",
        ));
    }
    kind_rates(kind, up, down, max_custom_up())?;
    let mut out = Vec::new();
    for tid in ids {
        match mirror_status(tid)? {
            Some((st, _, _)) if st != "deleted" => {}
            _ => {
                out.push(json!({ "torrent_id": tid, "error": "promotion.torrent_not_found" }));
                continue;
            }
        }
        let old = sql::query(
            "SELECT id FROM promotions WHERE torrent_id = $1 AND source = 'manual' AND state = 'active' FOR UPDATE",
            &[SqlValue::Int(tid)],
        )?;
        for row in &old.rows {
            end_internal(int(&row[0]), "replaced", actor)?;
        }
        // manual 优先于 auto：结束自动促销，避免倍率叠加
        let auto = sql::query(
            "SELECT id FROM promotions WHERE torrent_id = $1 AND source = 'auto' AND state = 'active'",
            &[SqlValue::Int(tid)],
        )?;
        for row in &auto.rows {
            end_internal(int(&row[0]), "replaced", actor)?;
        }
        let p = create(&NewPromo {
            source: "manual",
            torrent_id: Some(tid),
            user_id: None,
            rule_id: None,
            kind,
            up,
            down,
            priority: 0,
            starts_ms: None,
            ends_ms,
            reason,
            created_by: actor,
            title: None,
        })?;
        out.push(json!({ "torrent_id": tid, "promotion_id": p.id }));
    }
    Ok(out)
}

// ---------------- 自动规则（03 §1.3） ----------------

fn predicate(m: &Value, pr: &Value) -> bool {
    let field = pr["field"].as_str().unwrap_or("");
    let op = pr["op"].as_str().unwrap_or("");
    let v = &pr["value"];
    let actual = &m[field];
    if actual.is_null() {
        return false;
    }
    match (op, actual.as_f64(), v.as_f64()) {
        ("gte", Some(a), Some(b)) => a >= b,
        ("gt", Some(a), Some(b)) => a > b,
        ("lte", Some(a), Some(b)) => a <= b,
        ("lt", Some(a), Some(b)) => a < b,
        ("eq", _, _) => actual == v,
        ("ne", _, _) => actual != v,
        ("in", _, _) => match (actual.as_array(), v.as_array()) {
            (Some(a), Some(b)) => a.iter().any(|x| b.contains(x)),
            (None, Some(b)) => b.contains(actual),
            _ => false,
        },
        ("nin", _, _) => match (actual.as_array(), v.as_array()) {
            (Some(a), Some(b)) => !a.iter().any(|x| b.contains(x)),
            (None, Some(b)) => !b.contains(actual),
            _ => false,
        },
        _ => false,
    }
}

fn evaluate(tid: i64) -> R<()> {
    let m = sql::query(
        "SELECT row_to_json(x)::text FROM (SELECT type_id, size, status, tags, file_count, seeders, leechers, completed,
                extract(epoch FROM now() - created_at) / 3600 AS age_hours FROM torrent_mirror WHERE torrent_id = $1) x",
        &[SqlValue::Int(tid)],
    )?;
    let Some(row) = m.rows.first() else { return Ok(()) };
    let m: Value = serde_json::from_str(&text(&row[0])).unwrap_or(Value::Null);
    let current = sql::query(
        "SELECT p.id, p.rule_id FROM promotions p WHERE p.torrent_id = $1 AND p.source = 'auto' AND p.state = 'active' FOR UPDATE",
        &[SqlValue::Int(tid)],
    )?;
    if m["status"] != "approved" {
        for r in &current.rows {
            end_internal(int(&r[0]), "condition_lost", "system")?;
        }
        return Ok(());
    }
    // manual 生效时不评估自动规则
    let manual = sql::query(
        "SELECT 1 FROM promotions WHERE torrent_id = $1 AND source = 'manual' AND state = 'active'",
        &[SqlValue::Int(tid)],
    )?;
    if !manual.rows.is_empty() {
        return Ok(());
    }
    let rules = sql::query(
        "SELECT r.id, r.definition::text, coalesce(s.fired_once, false) FROM auto_rules r
           LEFT JOIN auto_state s ON s.rule_id = r.id AND s.torrent_id = $1
          WHERE r.active ORDER BY r.priority DESC, r.id",
        &[SqlValue::Int(tid)],
    )?;
    let mut best: Option<(i64, Value)> = None;
    for r in &rules.rows {
        let def: Value = serde_json::from_str(&text(&r[1])).unwrap_or(Value::Null);
        if let Some(types) = def["type_ids"].as_array()
            && !types.contains(&m["type_id"])
        {
            continue;
        }
        if def["duration"]["mode"] == "fixed" && matches!(r[2], SqlValue::Boolean(true)) {
            continue;
        }
        let enter = def["enter"].as_array().cloned().unwrap_or_default();
        if !enter.is_empty() && enter.iter().all(|p| predicate(&m, p)) {
            best = Some((int(&r[0]), def));
            break;
        }
    }
    if let Some(cur) = current.rows.first() {
        let cur_rule = opt_int(&cur[1]);
        if best.as_ref().is_some_and(|(id, _)| Some(*id) == cur_rule) {
            return Ok(());
        }
        // 当前规则仍满足（或为 fixed）且没有更高优先级规则抢占时保留
        let cur_def: Option<Value> = rules
            .rows
            .iter()
            .find(|r| Some(int(&r[0])) == cur_rule)
            .and_then(|r| serde_json::from_str(&text(&r[1])).ok());
        let keep = match &cur_def {
            Some(d) if d["duration"]["mode"] == "fixed" => best
                .as_ref()
                .is_none_or(|(_, b)| b["priority"].as_i64() <= d["priority"].as_i64()),
            Some(d) => {
                let still = match d["exit"].as_array() {
                    Some(exit) => !exit.iter().any(|p| predicate(&m, p)),
                    None => d["enter"]
                        .as_array()
                        .is_some_and(|e| e.iter().all(|p| predicate(&m, p))),
                };
                still
                    && best
                        .as_ref()
                        .is_none_or(|(_, b)| b["priority"].as_i64() <= d["priority"].as_i64())
            }
            None => false,
        };
        if keep {
            return Ok(());
        }
        end_internal(
            int(&cur[0]),
            if best.is_some() {
                "replaced"
            } else {
                "condition_lost"
            },
            "system",
        )?;
    }
    if let Some((rid, def)) = best {
        let now = pt::plugin::time::now();
        let hours = match def["duration"]["mode"].as_str() {
            Some("fixed") => def["duration"]["hours"].as_i64(),
            _ => def["duration"]["max_hours"].as_i64().filter(|h| *h > 0),
        };
        let kind = def["kind"].as_str().unwrap_or("free");
        create(&NewPromo {
            source: "auto",
            torrent_id: Some(tid),
            user_id: None,
            rule_id: Some(rid),
            kind,
            up: def["up_permille"].as_i64(),
            down: def["down_permille"].as_i64(),
            priority: def["priority"].as_i64().unwrap_or(0),
            starts_ms: None,
            ends_ms: hours.map(|h| now + h * 3_600_000),
            reason: "auto",
            created_by: &format!("auto:{rid}"),
            title: None,
        })?;
        sql::execute(
            "INSERT INTO auto_state (rule_id, torrent_id, fired_once, entered_at) VALUES ($1, $2, true, now())
             ON CONFLICT (rule_id, torrent_id) DO UPDATE SET fired_once = true, entered_at = now()",
            &[SqlValue::Int(rid), SqlValue::Int(tid)],
        )?;
    }
    Ok(())
}

fn validate_rule(def: &Value) -> R<i64> {
    let bad = |m: &str| err("kernel.validation_failed", m);
    let enter = def["enter"]
        .as_array()
        .ok_or_else(|| bad("enter must be an array"))?;
    if enter.is_empty() || enter.len() > 10 {
        return Err(bad("enter must have 1..=10 predicates"));
    }
    const FIELDS: &[&str] = &[
        "size",
        "age_hours",
        "type_id",
        "status",
        "tags",
        "seeders",
        "leechers",
        "completed",
        "file_count",
    ];
    const OPS: &[&str] = &["gte", "gt", "lte", "lt", "eq", "ne", "in", "nin"];
    for p in enter.iter().chain(def["exit"].as_array().into_iter().flatten()) {
        if !FIELDS.contains(&p["field"].as_str().unwrap_or(""))
            || !OPS.contains(&p["op"].as_str().unwrap_or(""))
        {
            return Err(bad("unknown predicate field or op"));
        }
    }
    kind_rates(
        def["kind"].as_str().unwrap_or(""),
        def["up_permille"].as_i64(),
        def["down_permille"].as_i64(),
        max_custom_up(),
    )?;
    if !matches!(def["duration"]["mode"].as_str(), Some("fixed" | "while_matched")) {
        return Err(bad("duration.mode must be fixed or while_matched"));
    }
    let pr = def["priority"].as_i64().unwrap_or(0);
    if !(0..=1000).contains(&pr) {
        return Err(bad("priority must be 0..=1000"));
    }
    Ok(pr)
}

// ---------------- 展示状态（03 §1.8） ----------------

fn brief(row: &[SqlValue]) -> Value {
    json!({ "promotion_id": int(&row[0]), "kind": text(&row[1]), "source": text(&row[2]),
            "name_key": format!("promotion.{}", text(&row[1])), "up_permille": opt_int(&row[3]), "down_permille": opt_int(&row[4]),
            "starts_at": opt_text(&row[5]), "ends_at": opt_text(&row[6]) })
}

fn active_global() -> R<Vec<Value>> {
    let r = sql::query(
        "SELECT id, kind, source, up_permille, down_permille, starts_at::text, ends_at::text FROM promotions
          WHERE source = 'global' AND state = 'active' AND starts_at <= now() AND (ends_at IS NULL OR ends_at > now()) ORDER BY id",
        &[],
    )?;
    Ok(r.rows.iter().map(|row| brief(row)).collect())
}

/// favorable 合并（与 accounting 同算法，不含个人卡）；neutral 覆盖
fn combine(items: &[&Value]) -> (i64, i64) {
    if items.iter().any(|b| b["kind"] == "neutral") {
        return (0, 0);
    }
    let up = items
        .iter()
        .filter_map(|b| b["up_permille"].as_i64())
        .fold(1000, i64::max);
    let down = items
        .iter()
        .filter_map(|b| b["down_permille"].as_i64())
        .fold(1000, i64::min);
    (up, down)
}

fn current_many(ids: &[i64]) -> R<Vec<Value>> {
    if ids.len() > 500 {
        return Err(err("promotion.too_many_ids", "at most 500 ids"));
    }
    let r = sql::query(
        "SELECT DISTINCT ON (torrent_id) torrent_id, id, kind, source, up_permille, down_permille, starts_at::text, ends_at::text
           FROM promotions
          WHERE torrent_id = ANY($1) AND source IN ('manual', 'auto') AND state = 'active'
            AND starts_at <= now() AND (ends_at IS NULL OR ends_at > now())
          ORDER BY torrent_id, (source = 'manual') DESC, priority DESC, id DESC",
        &[SqlValue::IntArray(ids.to_vec())],
    )?;
    let global = active_global()?;
    let mut out = Vec::new();
    for tid in ids {
        let primary = r
            .rows
            .iter()
            .find(|row| int(&row[0]) == *tid)
            .map(|row| brief(&row[1..]));
        if primary.is_none() && global.is_empty() {
            continue;
        }
        let mut all: Vec<&Value> = global.iter().collect();
        if let Some(p) = &primary {
            all.push(p);
        }
        let (up, down) = combine(&all);
        out.push(json!({ "torrent_id": tid, "primary": primary, "global": global,
                         "combined_up_permille": up, "combined_down_permille": down }));
    }
    Ok(out)
}

// ---------------- 个人卡（03 §1.5） ----------------

fn grant_cards(user: i64, card_kind: &str, n: i64, reason: &str) -> R<i64> {
    if !matches!(card_kind, "free" | "double_up") {
        return Err(err(
            "promotion.card_kind_invalid",
            "card_kind must be free or double_up",
        ));
    }
    if !(-100..=100).contains(&n) || n == 0 {
        return Err(err(
            "kernel.validation_failed",
            "delta must be in -100..=100 and non-zero",
        ));
    }
    let bal = sql::query(
        "INSERT INTO card_balances (user_id, card_kind, balance) VALUES ($1, $2, greatest($3, 0))
         ON CONFLICT (user_id, card_kind) DO UPDATE SET balance = greatest(card_balances.balance + $3, 0) RETURNING balance",
        &[SqlValue::Int(user), SqlValue::Text(card_kind.into()), SqlValue::Int(n)],
    )?;
    let balance = int(&bal.rows[0][0]);
    sql::execute(
        "INSERT INTO card_log (user_id, card_kind, delta, balance_after, reason) VALUES ($1, $2, $3, $4, $5)",
        &[
            SqlValue::Int(user),
            SqlValue::Text(card_kind.into()),
            SqlValue::Int(n),
            SqlValue::Int(balance),
            SqlValue::Text(reason.into()),
        ],
    )?;
    publish(
        "promotion.cards_changed",
        format!("user:{user}"),
        json!({ "user_id": user, "card_kind": card_kind, "delta": n, "balance": balance, "reason": reason }),
    )?;
    Ok(balance)
}

fn apply_card(user: i64, tid: i64, card_kind: &str, confirm: bool) -> R<Value> {
    if !matches!(card_kind, "free" | "double_up") {
        return Err(err(
            "promotion.card_kind_invalid",
            "card_kind must be free or double_up",
        ));
    }
    match mirror_status(tid)? {
        Some((st, _, _)) if st == "approved" => {}
        _ => {
            return Err(err(
                "promotion.card_not_applicable",
                "the torrent is not eligible for cards",
            ));
        }
    }
    let bal = sql::query(
        "SELECT balance FROM card_balances WHERE user_id = $1 AND card_kind = $2 FOR UPDATE",
        &[SqlValue::Int(user), SqlValue::Text(card_kind.into())],
    )?;
    if bal.rows.first().is_none_or(|r| int(&r[0]) < 1) {
        return Err(PluginError {
            code: "promotion.no_card".into(),
            message: "you have no card of this kind".into(),
            details: None,
            retryable: false,
        });
    }
    let dup = sql::query(
        "SELECT 1 FROM promotions WHERE source = 'card' AND user_id = $1 AND torrent_id = $2 AND kind = $3 AND state = 'active'",
        &[
            SqlValue::Int(user),
            SqlValue::Int(tid),
            SqlValue::Text(card_kind.into()),
        ],
    )?;
    if !dup.rows.is_empty() {
        return Err(err(
            "promotion.card_already_applied",
            "this card is already active on the torrent",
        ));
    }
    let eff = accounting(
        "effective_rate",
        &json!({ "user": user, "torrent_id": tid, "at": null }),
    )?;
    let redundant = match card_kind {
        "free" => eff["down_permille"].as_i64() == Some(0),
        _ => eff["up_permille"].as_i64().is_some_and(|u| u >= 2000),
    };
    if redundant && !confirm {
        return Err(PluginError {
            code: "promotion.card_redundant".into(),
            message: "the current rates already cover this card".into(),
            details: Some(json!({ "current": eff }).to_string()),
            retryable: false,
        });
    }
    let balance = grant_cards(user, card_kind, -1, &format!("apply:{tid}"))?;
    let days: i64 = serde_json::from_str::<Value>(&pt::plugin::config::get_static())
        .ok()
        .and_then(|c| c["card_duration_days"].as_i64())
        .unwrap_or(7);
    let now = pt::plugin::time::now();
    let p = create(&NewPromo {
        source: "card",
        torrent_id: Some(tid),
        user_id: Some(user),
        rule_id: None,
        kind: card_kind,
        up: None,
        down: None,
        priority: 0,
        starts_ms: None,
        ends_ms: Some(now + days * 86_400_000),
        reason: "card",
        created_by: &format!("user:{user}"),
        title: None,
    })?;
    publish(
        "promotion.card_applied",
        format!("user:{user}"),
        json!({ "promotion_id": p.id, "user_id": user, "torrent_id": tid, "card_kind": card_kind, "ends_at": p.ends_ms }),
    )?;
    Ok(json!({ "promotion_id": p.id, "ends_at": p.ends_ms, "balance": balance }))
}

// ---------------- 导出 ----------------

impl on_event::Guest for Promotion {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        let p: Value =
            serde_json::from_str(&evt.payload).map_err(|_| err("promotion.bad_event", "invalid payload"))?;
        if evt.event_type == "torrent.created" {
            let tid = p["torrent_id"]
                .as_i64()
                .ok_or_else(|| err("promotion.bad_event", "torrent_id missing"))?;
            sql::execute(
                "INSERT INTO torrent_mirror (torrent_id, type_id, size, status, file_count, created_at)
                 VALUES ($1, $2, $3, $4, $5, to_timestamp($6::float8 / 1000))
                 ON CONFLICT (torrent_id) DO UPDATE SET type_id = EXCLUDED.type_id, size = EXCLUDED.size, status = EXCLUDED.status,
                     file_count = EXCLUDED.file_count, updated_at = now()",
                &[SqlValue::Int(tid), p["type_id"].as_str().map_or(SqlValue::Null, |t| SqlValue::Text(t.into())),
                  ival(p["size"].as_i64()), p["status"].as_str().map_or(SqlValue::Null, |t| SqlValue::Text(t.into())),
                  ival(p["file_count"].as_i64()), SqlValue::Int(p["created_at"].as_i64().unwrap_or(evt.occurred_at))],
            )?;
            if p["status"] == "approved" {
                evaluate(tid)?;
            }
        }
        Ok(())
    }
}

impl on_cron::Guest for Promotion {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            // 到期（03 §1.10）：accounting 规则已按 ends_at 失效，这里只更新状态与发事件
            "expire" => loop {
                let r = sql::query(
                    "UPDATE promotions SET state = 'ended', end_cause = 'expired', ended_at = ends_at, updated_at = now()
                      WHERE id IN (SELECT id FROM promotions WHERE state = 'active' AND ends_at <= now() ORDER BY ends_at LIMIT 1000)
                      RETURNING id, source, torrent_id",
                    &[],
                )?;
                for row in &r.rows {
                    if text(&row[1]) != "card" {
                        let tid = opt_int(&row[2]);
                        publish(
                            "promotion.ended",
                            tid.map_or("global".into(), |t| format!("torrent:{t}")),
                            json!({ "promotion_id": int(&row[0]), "scope": if tid.is_some() { "torrent" } else { "global" },
                                        "source": text(&row[1]), "torrent_ids": tid.map_or(vec![], |t| vec![t]), "cause": "expired" }),
                        )?;
                    }
                }
                sql::commit()?;
                if r.rows.len() < 1000 {
                    return Ok(());
                }
            },
            // 巡检（03 §1.6 第 1 步）：补齐下发失败的规则
            "reconcile" => {
                let r = sql::query(
                    "SELECT id, sync_state FROM promotions WHERE sync_state IN ('pending', 'remove_pending') ORDER BY id LIMIT 500",
                    &[],
                )?;
                for row in &r.rows {
                    if let Some(p) = load(int(&row[0]))? {
                        if text(&row[1]) == "pending" {
                            push(&p)?
                        } else {
                            pull(&p)?
                        }
                    }
                }
                Ok(())
            }
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

fn json_response(status: u16, v: &Value) -> http_handler::Response {
    http_handler::Response {
        status,
        headers: vec![http_handler::Header {
            name: "content-type".into(),
            value: "application/json".into(),
        }],
        body: v.to_string().into_bytes(),
    }
}

fn body(req: &http_handler::Request) -> R<Value> {
    serde_json::from_slice(&req.body).map_err(|_| err("kernel.bad_request", "invalid JSON body"))
}

fn param(req: &http_handler::Request, k: &str) -> R<i64> {
    req.params
        .iter()
        .find(|(n, _)| n == k)
        .and_then(|(_, v)| v.parse().ok())
        .ok_or_else(|| err("kernel.bad_request", "bad path parameter"))
}

fn me() -> R<i64> {
    invocation::current()
        .actor
        .user_id
        .and_then(|u| i64::try_from(u).ok())
        .ok_or_else(|| err("kernel.unauthenticated", "login required"))
}

impl http_handler::Guest for Promotion {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let v = match req.route_id.as_str() {
            "torrent_state" => {
                let tid = param(&req, "torrent_id")?;
                let mut st = current_many(&[tid])?.pop().unwrap_or_else(|| json!({ "torrent_id": tid, "primary": null, "global": [],
                                                                                    "combined_up_permille": 1000, "combined_down_permille": 1000 }));
                st["mine"] = accounting(
                    "effective_rate",
                    &json!({ "user": me()?, "torrent_id": tid, "at": null }),
                )?;
                st
            }
            "torrent_states" => {
                let ids: Vec<i64> = req
                    .query
                    .iter()
                    .find(|(k, _)| k == "ids")
                    .map(|(_, v)| v.split(',').filter_map(|x| x.trim().parse().ok()).collect())
                    .unwrap_or_default();
                json!({ "items": current_many(&ids)? })
            }
            "set_single" => {
                let b = body(&req)?;
                let tids: Vec<i64> = b["torrent_ids"]
                    .as_array()
                    .map(|a| a.iter().filter_map(Value::as_i64).collect())
                    .unwrap_or_default();
                let ends = b["duration_secs"]
                    .as_i64()
                    .map(|d| pt::plugin::time::now() + d * 1000)
                    .or_else(|| b["ends_at_ms"].as_i64());
                let items = set_single(
                    &tids,
                    b["kind"].as_str().unwrap_or(""),
                    b["up_permille"].as_i64(),
                    b["down_permille"].as_i64(),
                    ends,
                    b["reason"].as_str().unwrap_or(""),
                    &format!("user:{}", me()?),
                )?;
                json!({ "items": items })
            }
            "end_single" => {
                let tid = param(&req, "torrent_id")?;
                let r = sql::query(
                    "SELECT id FROM promotions WHERE torrent_id = $1 AND source = 'manual' AND state = 'active'",
                    &[SqlValue::Int(tid)],
                )?;
                let Some(row) = r.rows.first() else {
                    return Err(err("promotion.not_found", "no manual promotion on this torrent"));
                };
                end_internal(int(&row[0]), "cancelled", &format!("user:{}", me()?))?;
                evaluate(tid)?;
                return Ok(http_handler::Response {
                    status: 204,
                    headers: vec![],
                    body: vec![],
                });
            }
            "create_event" => {
                let b = body(&req)?;
                let (starts, ends) = (
                    b["starts_at_ms"].as_i64(),
                    b["ends_at_ms"]
                        .as_i64()
                        .ok_or_else(|| err("kernel.validation_failed", "ends_at_ms is required"))?,
                );
                if ends - starts.unwrap_or_else(pt::plugin::time::now) > 31 * 86_400_000 {
                    return Err(err(
                        "promotion.duration_invalid",
                        "site events last at most 31 days",
                    ));
                }
                if !b["type_ids"].is_null() {
                    return Err(err(
                        "promotion.type_scope_unsupported",
                        "type-limited site events are not supported yet",
                    ));
                }
                let p = create(&NewPromo {
                    source: "global",
                    torrent_id: None,
                    user_id: None,
                    rule_id: None,
                    kind: b["kind"].as_str().unwrap_or(""),
                    up: b["up_permille"].as_i64(),
                    down: b["down_permille"].as_i64(),
                    priority: 0,
                    starts_ms: starts,
                    ends_ms: Some(ends),
                    reason: "event",
                    created_by: &format!("user:{}", me()?),
                    title: Some(json!({ "zh-CN": b["title_zh"], "en": b["title_en"] })),
                })?;
                return Ok(json_response(201, &json!({ "id": p.id })));
            }
            "create_rule" => {
                let def = body(&req)?;
                let pr = validate_rule(&def)?;
                let id = int(&sql::query("INSERT INTO auto_rules (definition, priority, created_by) VALUES ($1, $2, $3) RETURNING id",
                                         &[SqlValue::Json(def.to_string()), SqlValue::Int(pr), SqlValue::Int(me()?)])?.rows[0][0]);
                return Ok(json_response(201, &json!({ "id": id })));
            }
            "grant_cards" => {
                let b = body(&req)?;
                let bal = grant_cards(
                    param(&req, "user_id")?,
                    b["card_kind"].as_str().unwrap_or(""),
                    b["delta"].as_i64().unwrap_or(0),
                    b["reason"].as_str().unwrap_or("admin"),
                )?;
                json!({ "balance": bal })
            }
            "my_cards" => {
                let uid = me()?;
                let b = sql::query(
                    "SELECT card_kind, balance FROM card_balances WHERE user_id = $1 ORDER BY card_kind",
                    &[SqlValue::Int(uid)],
                )?;
                let a = sql::query(
                    "SELECT torrent_id, kind, ends_at::text FROM promotions WHERE source = 'card' AND user_id = $1 AND state = 'active' ORDER BY id",
                    &[SqlValue::Int(uid)],
                )?;
                json!({ "balances": b.rows.iter().map(|r| json!({ "card_kind": text(&r[0]), "balance": int(&r[1]) })).collect::<Vec<_>>(),
                        "active": a.rows.iter().map(|r| json!({ "torrent_id": int(&r[0]), "card_kind": text(&r[1]), "ends_at": opt_text(&r[2]) })).collect::<Vec<_>>() })
            }
            "apply_card" => {
                let b = body(&req)?;
                apply_card(
                    me()?,
                    b["torrent_id"].as_i64().unwrap_or(0),
                    b["card_kind"].as_str().unwrap_or(""),
                    b["confirm"].as_bool().unwrap_or(false),
                )?
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(json_response(200, &v))
    }
}

impl service_provider::Guest for Promotion {
    fn call(service: String, method: String, args: String) -> R<String> {
        if service != "ctx.promotions" {
            return Err(err("kernel.not_found", "unknown service"));
        }
        let a: Value =
            serde_json::from_str(&args).map_err(|_| err("kernel.bad_request", "invalid JSON arguments"))?;
        let inv = invocation::current();
        let caller = inv.caller_plugin.clone();
        let has = |s: &str| inv.caller_scopes.iter().any(|x| x == s);
        let out = match method.as_str() {
            "current" => json!(current_many(&[a["torrent_id"].as_i64().unwrap_or(0)])?.pop()),
            "current_many" => json!(current_many(
                &a["ids"]
                    .as_array()
                    .map(|x| x.iter().filter_map(Value::as_i64).collect::<Vec<_>>())
                    .unwrap_or_default()
            )?),
            "active_global" => json!(active_global()?),
            "create" => {
                if !has("write") {
                    return Err(err("promotion.scope_required", "scope `write` required"));
                }
                let items = set_single(
                    &[a["torrent_id"].as_i64().unwrap_or(0)],
                    a["kind"].as_str().unwrap_or(""),
                    a["up_permille"].as_i64(),
                    a["down_permille"].as_i64(),
                    a["ends_at"].as_i64(),
                    a["reason"].as_str().unwrap_or(""),
                    &format!("plugin:{caller}"),
                )?;
                match items.first() {
                    Some(i) if i.get("promotion_id").is_some() => i["promotion_id"].clone(),
                    Some(i) => {
                        return Err(err(
                            i["error"].as_str().unwrap_or("promotion.torrent_not_found"),
                            "promotion could not be created",
                        ));
                    }
                    None => Value::Null,
                }
            }
            "end" => {
                if !has("write") {
                    return Err(err("promotion.scope_required", "scope `write` required"));
                }
                let id = a["promotion_id"].as_i64().unwrap_or(0);
                let owner = sql::query(
                    "SELECT created_by FROM promotions WHERE id = $1",
                    &[SqlValue::Int(id)],
                )?;
                match owner.rows.first() {
                    None => return Err(err("promotion.not_found", "promotion not found")),
                    Some(r) if text(&r[0]) != format!("plugin:{caller}") => {
                        return Err(err("kernel.forbidden", "not your promotion"));
                    }
                    _ => {}
                }
                json!(end_internal(id, "cancelled", &format!("plugin:{caller}"))?)
            }
            "grant_cards" => {
                if !has("write:cards") {
                    return Err(err("promotion.scope_required", "scope `write:cards` required"));
                }
                json!(grant_cards(
                    a["user"].as_i64().unwrap_or(0),
                    a["card_kind"].as_str().unwrap_or(""),
                    a["n"].as_i64().unwrap_or(0),
                    a["reason"].as_str().unwrap_or("")
                )?)
            }
            "cards_of" => {
                let b = sql::query(
                    "SELECT card_kind, balance FROM card_balances WHERE user_id = $1 ORDER BY card_kind",
                    &[SqlValue::Int(a["user"].as_i64().unwrap_or(0))],
                )?;
                json!(
                    b.rows
                        .iter()
                        .map(|r| json!({ "card_kind": text(&r[0]), "balance": int(&r[1]) }))
                        .collect::<Vec<_>>()
                )
            }
            _ => return Err(err("kernel.not_found", "unknown method")),
        };
        Ok(out.to_string())
    }
}

export!(Promotion);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicates() {
        let m = json!({ "size": 20, "status": "approved", "tags": ["official", "4k"], "type_id": "movie", "seeders": 1 });
        assert!(predicate(&m, &json!({"field": "size", "op": "gte", "value": 10})));
        assert!(!predicate(&m, &json!({"field": "size", "op": "lt", "value": 10})));
        assert!(predicate(
            &m,
            &json!({"field": "status", "op": "eq", "value": "approved"})
        ));
        assert!(predicate(
            &m,
            &json!({"field": "tags", "op": "in", "value": ["4k"]})
        ));
        assert!(predicate(
            &m,
            &json!({"field": "type_id", "op": "nin", "value": ["tv"]})
        ));
        assert!(
            !predicate(&m, &json!({"field": "file_count", "op": "gte", "value": 1})),
            "null fields never match"
        );
        assert_eq!(
            combine(&[
                &json!({"kind": "free", "down_permille": 0}),
                &json!({"kind": "double_up", "up_permille": 2000})
            ]),
            (2000, 0)
        );
        assert_eq!(
            combine(&[
                &json!({"kind": "neutral", "up_permille": 0, "down_permille": 0}),
                &json!({"kind": "free", "down_permille": 0})
            ]),
            (0, 0)
        );
    }
}
