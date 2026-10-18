//! medals 插件（WASM，docs/plugins/medals/）：勋章与头衔。
//!
//! - 物品：勋章（可佩戴多枚，个人页展示）或头衔（同时激活一个）；可限量、可设有效期、可隐藏。
//! - 授予：管理员批量授予、成就规则自动授予、其他插件经 `ctx.medals.grant`（如 shop 出售）。每次授予有确定性的
//!   `source_ref`（唯一），重复事件 / 重复调用不会重复授予；已持有时再次授予会按有效期顺延。
//! - 成就规则：`event` 类订阅事件累计（计数 / 求和 / 取最大值），达到阈值即授予；`stat` 类每日按
//!   `accounting.v_user_totals` 的累计数据评估（上传量、做种时长等）。
//! - 展示：用户设置佩戴的勋章与激活的头衔；未设置时默认佩戴已持有的前几枚。

pt_plugin_sdk::world!("medals", http);

use exports::pt::plugin::{http_handler, on_cron, on_event, service_provider};
use pt_plugin_sdk::{audit, blob, prelude::*, serde_json, services, sql, time};

struct Medals;

/// 可用于成就的事件及其中表示“用户”的字段
pub const EVENTS: &[(&str, &str)] = &[
    ("torrent.created", "uploader_id"),
    ("traffic.snatch_completed", "user_id"),
    ("checkin.checked_in", "user_id"),
    ("comment.created", "author_id"),
    ("forum.post_created", "author_id"),
    ("subtitle.uploaded", "uploader_id"),
    ("request.filled", "filler_id"),
    ("invite.redeemed", "inviter_id"),
];
/// 可用于统计类成就的累计字段（`accounting.v_user_totals`）
pub const STAT_FIELDS: &[&str] = &["uploaded", "downloaded", "seed_secs", "completed_count", "seeding_count"];

const ACTIVE: &str = "g.revoked_at IS NULL AND (g.expires_at IS NULL OR g.expires_at > now())";
const ITEM_COLS: &str = "id, kind, name, description, icon_key, max_holders, validity_days, hidden, active, sort";

struct Settings {
    max_worn: usize,
    notify: bool,
}

fn settings() -> Settings {
    let s = ctx::settings();
    Settings {
        max_worn: s["max_worn"].as_u64().map_or(5, |n| usize::try_from(n.clamp(1, 10)).unwrap_or(5)),
        notify: s["notify_on_grant"].as_bool().unwrap_or(true),
    }
}

fn one(q: &str, p: &[SqlValue]) -> R<i64> {
    Ok(sql::query(q, p)?.rows.first().and_then(|r| r.int(0)).unwrap_or(0))
}

fn invalid(m: &str) -> PluginError {
    err("kernel.validation_failed", m)
}

/// 成就计量：`count`、`sum:<字段>`、`max:<字段>`
pub fn parse_measure(m: &str) -> Option<(&str, Option<&str>)> {
    let ident = |f: &str| !f.is_empty() && f.len() <= 40 && f.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    match m.split_once(':') {
        None if m == "count" => Some(("count", None)),
        Some((op @ ("sum" | "max"), f)) if ident(f) => Some((op, Some(f))),
        _ => None,
    }
}

/// 图标格式（按文件头）：png / jpg / gif / webp
pub fn icon_ext(b: &[u8]) -> Option<(&'static str, &'static str)> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("png", "image/png"))
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(("jpg", "image/jpeg"))
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some(("gif", "image/gif"))
    } else if b.len() > 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some(("webp", "image/webp"))
    } else {
        None
    }
}

fn icon_url(key: Option<String>) -> Value {
    key.and_then(|k| k.strip_prefix("public/").map(|k| format!("/media/medals/{k}"))).map_or(Value::Null, Value::String)
}

fn item_json(r: &[SqlValue]) -> Value {
    json!({ "id": r.int(0), "kind": r.text(1), "name": r.text(2), "description": r.text(3), "icon": icon_url(r.text(4)),
            "max_holders": r.int(5), "validity_days": r.int(6), "hidden": r.bool(7), "active": r.bool(8), "sort": r.int(9) })
}

// ---------------- 授予与收回 ----------------

struct Grant<'a> {
    user: i64,
    item: i64,
    source: &'a str,
    source_plugin: Option<String>,
    source_ref: String,
    reason: &'a str,
    by: Option<i64>,
    expires_ms: Option<i64>,
}

fn grant(g: &Grant) -> R<Value> {
    if let Some(r) = sql::query("SELECT id FROM grants WHERE source_ref = $1", &[SqlValue::Text(g.source_ref.clone())])?.rows.first() {
        return Ok(json!({ "grant_id": r.int(0), "created": false }));
    }
    let it = sql::query("SELECT kind, name, max_holders, validity_days, active, description FROM items WHERE id = $1 FOR UPDATE", &[SqlValue::Int(g.item)])?;
    let Some(it) = it.rows.first() else { return Err(err("kernel.not_found", "medal not found")) };
    if !it.bool(4).unwrap_or(false) {
        return Err(err("medals.inactive", "this medal is disabled"));
    }
    let name = it.text(1).unwrap_or_default();
    let held = sql::query(
        &format!("SELECT count(*), bool_or(g.expires_at IS NULL), (extract(epoch FROM max(g.expires_at)) * 1000)::bigint FROM grants g WHERE g.user_id = $1 AND g.item_id = $2 AND {ACTIVE}"),
        &[SqlValue::Int(g.user), SqlValue::Int(g.item)],
    )?;
    let h = held.rows.first();
    let holding = h.and_then(|r| r.int(0)).unwrap_or(0) > 0;
    if !holding && let Some(max) = it.int(2) {
        let n = one(&format!("SELECT count(DISTINCT g.user_id) FROM grants g WHERE g.item_id = $1 AND {ACTIVE}"), &[SqlValue::Int(g.item)])?;
        if n >= max {
            return Err(err("medals.limit_reached", "this medal has reached its holder limit"));
        }
    }
    // 有效期：显式给出的优先；否则按物品有效期，已持有（且非永久）时从当前到期时间顺延
    let now = time::now();
    let expires = g.expires_ms.or_else(|| {
        it.int(3).map(|d| {
            let base = if holding && !h.and_then(|r| r.bool(1)).unwrap_or(false) { h.and_then(|r| r.int(2)).unwrap_or(now).max(now) } else { now };
            base + d * 86_400_000
        })
    });
    let id = one(
        "INSERT INTO grants (user_id, item_id, source, source_plugin, source_ref, reason, granted_by, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, CASE WHEN $8::bigint IS NULL THEN NULL ELSE to_timestamp($8::bigint / 1000.0) END) RETURNING id",
        &[
            SqlValue::Int(g.user),
            SqlValue::Int(g.item),
            SqlValue::Text(g.source.to_owned()),
            g.source_plugin.clone().map_or(SqlValue::Null, SqlValue::Text),
            SqlValue::Text(g.source_ref.clone()),
            SqlValue::Text(g.reason.chars().take(300).collect()),
            g.by.map_or(SqlValue::Null, SqlValue::Int),
            expires.map_or(SqlValue::Null, SqlValue::Int),
        ],
    )?;
    ctx::publish(
        "medal.granted",
        &format!("user:{}", g.user),
        &json!({ "user_id": g.user, "medal_id": g.item, "reason": g.reason, "grant_id": id, "kind": it.text(0), "source": g.source, "expires_at": expires }),
    )?;
    if !holding && settings().notify {
        let _ = services::call(
            "ctx.notify",
            "send",
            &json!({ "user_id": g.user, "template_key": "medals.granted", "category": "account",
                     "title": format!("你获得了「{name}」 / You earned \"{name}\""),
                     "body": if g.source == "achievement" { it.text(5).unwrap_or_default() } else { g.reason.to_owned() }, "link": "/p/medals/",
                     "args": { "medal_id": g.item, "name": name }, "idempotency_key": format!("medals:grant:{id}") })
            .to_string(),
        );
    }
    Ok(json!({ "grant_id": id, "created": true, "expires_at": expires }))
}

fn revoke_where(cond: &str, p: &[SqlValue], reason: &str) -> R<i64> {
    let mut params = p.to_vec();
    params.push(SqlValue::Text(reason.chars().take(300).collect()));
    let n = params.len();
    let r = sql::query(
        &format!("UPDATE grants g SET revoked_at = now(), revoke_reason = ${n} WHERE {cond} AND {ACTIVE} RETURNING g.id, g.user_id, g.item_id"),
        &params,
    )?;
    for row in &r.rows {
        let uid = row.int(1).unwrap_or(0);
        ctx::publish(
            "medal.revoked",
            &format!("user:{uid}"),
            &json!({ "user_id": uid, "medal_id": row.int(2), "reason": reason, "grant_id": row.int(0) }),
        )?;
    }
    Ok(i64::try_from(r.rows.len()).unwrap_or(0))
}

// ---------------- 展示 ----------------

fn holdings(uid: i64, include_hidden: bool) -> R<Vec<Value>> {
    let r = sql::query(
        &format!(
            "SELECT i.id, i.kind, i.name, i.description, i.icon_key, i.hidden, (extract(epoch FROM min(g.granted_at)) * 1000)::bigint,
                    CASE WHEN bool_or(g.expires_at IS NULL) THEN NULL ELSE (extract(epoch FROM max(g.expires_at)) * 1000)::bigint END,
                    (array_agg(g.reason ORDER BY g.id DESC))[1]
               FROM grants g JOIN items i ON i.id = g.item_id
              WHERE g.user_id = $1 AND {ACTIVE} AND i.active AND ($2 OR NOT i.hidden)
              GROUP BY i.id ORDER BY i.sort, i.id"
        ),
        &[SqlValue::Int(uid), SqlValue::Boolean(include_hidden)],
    )?;
    Ok(r.rows
        .iter()
        .map(|r| {
            json!({ "id": r.int(0), "kind": r.text(1), "name": r.text(2), "description": r.text(3), "icon": icon_url(r.text(4)),
                    "hidden": r.bool(5), "granted_at": r.int(6), "expires_at": r.int(7), "reason": r.text(8) })
        })
        .collect())
}

/// 批量展示：`{ "<uid>": { worn: [...], title: {...}|null } }`
fn display_many(uids: &[i64]) -> R<serde_json::Map<String, Value>> {
    let max = settings().max_worn;
    let held = sql::query(
        &format!(
            "SELECT DISTINCT g.user_id, i.id, i.kind, i.name, i.icon_key, i.sort FROM grants g JOIN items i ON i.id = g.item_id
              WHERE g.user_id = ANY($1) AND {ACTIVE} AND i.active ORDER BY i.sort, i.id"
        ),
        &[SqlValue::IntArray(uids.to_vec())],
    )?;
    let disp = sql::query("SELECT user_id, to_jsonb(worn), title_item FROM display WHERE user_id = ANY($1)", &[SqlValue::IntArray(uids.to_vec())])?;
    let mut out = serde_json::Map::new();
    for &uid in uids {
        let mine: Vec<&Vec<SqlValue>> = held.rows.iter().filter(|r| r.int(0) == Some(uid)).collect();
        let entry = |r: &Vec<SqlValue>| json!({ "id": r.int(1), "name": r.text(3), "icon": icon_url(r.text(4)) });
        let d = disp.rows.iter().find(|r| r.int(0) == Some(uid));
        let medals: Vec<&&Vec<SqlValue>> = mine.iter().filter(|r| r.text(2).as_deref() == Some("medal")).collect();
        let worn: Vec<Value> = match d.and_then(|d| d.json(1)) {
            Some(Value::Array(ids)) => ids.iter().filter_map(Value::as_i64).filter_map(|id| medals.iter().find(|r| r.int(1) == Some(id)).map(|r| entry(r))).take(max).collect(),
            _ => medals.iter().take(max).map(|r| entry(r)).collect(),
        };
        let title = d
            .and_then(|d| d.int(2))
            .and_then(|t| mine.iter().find(|r| r.int(1) == Some(t) && r.text(2).as_deref() == Some("title")))
            .map_or(Value::Null, |r| entry(r));
        out.insert(uid.to_string(), json!({ "worn": worn, "title": title }));
    }
    Ok(out)
}

fn display_of(uid: i64) -> R<Value> {
    Ok(display_many(&[uid])?.remove(&uid.to_string()).unwrap_or(Value::Null))
}

fn set_display(uid: i64, b: &Value) -> R<Value> {
    let max = settings().max_worn;
    let held = sql::query(
        &format!("SELECT DISTINCT i.id, i.kind FROM grants g JOIN items i ON i.id = g.item_id WHERE g.user_id = $1 AND {ACTIVE} AND i.active"),
        &[SqlValue::Int(uid)],
    )?;
    let kind_of = |id: i64| held.rows.iter().find(|r| r.int(0) == Some(id)).and_then(|r| r.text(1));
    let mut worn: Vec<i64> = Vec::new();
    for id in b["worn"].as_array().into_iter().flatten().filter_map(Value::as_i64) {
        if kind_of(id).as_deref() != Some("medal") {
            return Err(err("medals.not_held", "you can only wear medals you hold"));
        }
        if !worn.contains(&id) {
            worn.push(id);
        }
    }
    if worn.len() > max {
        return Err(invalid(&format!("you can wear at most {max} medals")));
    }
    let title = b["title"].as_i64();
    if let Some(t) = title
        && kind_of(t).as_deref() != Some("title")
    {
        return Err(err("medals.not_held", "you can only use titles you hold"));
    }
    sql::execute(
        "INSERT INTO display (user_id, worn, title_item) VALUES ($1, $2, $3)
         ON CONFLICT (user_id) DO UPDATE SET worn = EXCLUDED.worn, title_item = EXCLUDED.title_item, updated_at = now()",
        &[SqlValue::Int(uid), SqlValue::IntArray(worn), title.map_or(SqlValue::Null, SqlValue::Int)],
    )?;
    display_of(uid)
}

fn progress(uid: i64) -> R<Vec<Value>> {
    let r = sql::query(
        &format!(
            "SELECT r.id, r.item_id, i.name, i.icon_key, r.kind, r.stat_field, r.threshold, r.description, coalesce(c.value, 0),
                    EXISTS (SELECT 1 FROM grants g WHERE g.item_id = r.item_id AND g.user_id = $1 AND {ACTIVE})
               FROM rules r JOIN items i ON i.id = r.item_id LEFT JOIN counters c ON c.rule_id = r.id AND c.user_id = $1
              WHERE r.active AND i.active AND NOT i.hidden ORDER BY i.sort, r.id"
        ),
        &[SqlValue::Int(uid)],
    )?;
    let totals = sql::query(
        &format!("SELECT {} FROM accounting.v_user_totals WHERE user_id = $1", STAT_FIELDS.iter().map(|f| format!("{f}::bigint")).collect::<Vec<_>>().join(", ")),
        &[SqlValue::Int(uid)],
    )
    .ok();
    let stat = |f: &str| {
        let i = STAT_FIELDS.iter().position(|x| *x == f)?;
        totals.as_ref()?.rows.first()?.int(i)
    };
    Ok(r.rows
        .iter()
        .map(|r| {
            let value = if r.text(4).as_deref() == Some("stat") { r.text(5).and_then(|f| stat(&f)).unwrap_or(0) } else { r.int(8).unwrap_or(0) };
            json!({ "rule_id": r.int(0), "medal_id": r.int(1), "name": r.text(2), "icon": icon_url(r.text(3)), "kind": r.text(4),
                    "stat_field": r.text(5), "threshold": r.int(6), "description": r.text(7), "value": value, "held": r.bool(9) })
        })
        .collect())
}

// ---------------- 成就 ----------------

fn on_achievement_event(event: &str, p: &Value) -> R<()> {
    let Some((_, field)) = EVENTS.iter().find(|(e, _)| *e == event) else { return Ok(()) };
    let Some(uid) = p[*field].as_i64() else { return Ok(()) };
    // 匿名发布不计入（勋章会暴露发布者）
    if event == "torrent.created" && p["anonymous"].as_bool() == Some(true) {
        return Ok(());
    }
    let rules = sql::query("SELECT id, item_id, measure, threshold FROM rules WHERE active AND kind = 'event' AND event = $1", &[SqlValue::Text(event.to_owned())])?;
    for r in &rules.rows {
        let (rid, item, threshold) = (r.int(0).unwrap_or(0), r.int(1).unwrap_or(0), r.int(3).unwrap_or(i64::MAX));
        let measure = r.text(2).unwrap_or_default();
        let Some((op, f)) = parse_measure(&measure) else { continue };
        let x = f.map_or(1, |f| p[f].as_i64().unwrap_or(0));
        let upd = if op == "max" { "GREATEST(counters.value, EXCLUDED.value)" } else { "counters.value + EXCLUDED.value" };
        let v = one(
            &format!("INSERT INTO counters (rule_id, user_id, value) VALUES ($1, $2, $3) ON CONFLICT (rule_id, user_id) DO UPDATE SET value = {upd} RETURNING value"),
            &[SqlValue::Int(rid), SqlValue::Int(uid), SqlValue::Int(x)],
        )?;
        if v >= threshold {
            achieve(rid, item, uid)?;
        }
    }
    Ok(())
}

fn achieve(rule: i64, item: i64, uid: i64) -> R<()> {
    let g = Grant {
        user: uid,
        item,
        source: "achievement",
        source_plugin: None,
        source_ref: format!("ach:{rule}:{uid}"),
        reason: "achievement",
        by: None,
        expires_ms: None,
    };
    match grant(&g) {
        // 物品停用或限量已满：跳过，不让事件处理失败重试
        Err(e) if e.code == "medals.inactive" || e.code == "medals.limit_reached" => Ok(()),
        r => r.map(|_| ()),
    }
}

/// 统计类成就：一批最多 5000 人，返回本批授予数
fn eval_stat_batch(rule: i64, item: i64, field: &str, threshold: i64) -> R<usize> {
    if !STAT_FIELDS.contains(&field) {
        return Ok(0);
    }
    let r = sql::query(
        &format!(
            "SELECT t.user_id FROM accounting.v_user_totals t JOIN identity.v_user_public u ON u.id = t.user_id
              WHERE t.{field} >= $1 AND NOT EXISTS (SELECT 1 FROM grants g WHERE g.source_ref = 'ach:' || $2::bigint || ':' || t.user_id)
              ORDER BY t.user_id LIMIT 5000"
        ),
        &[SqlValue::Int(threshold), SqlValue::Int(rule)],
    )?;
    for uid in r.rows.iter().filter_map(|r| r.int(0)) {
        achieve(rule, item, uid)?;
    }
    Ok(r.rows.len())
}

fn eval_stats(commit: bool) -> R<usize> {
    let rules = sql::query("SELECT r.id, r.item_id, r.stat_field, r.threshold FROM rules r JOIN items i ON i.id = r.item_id WHERE r.active AND i.active AND r.kind = 'stat'", &[])?;
    let mut total = 0;
    for r in &rules.rows {
        let (rid, item, field, th) = (r.int(0).unwrap_or(0), r.int(1).unwrap_or(0), r.text(2).unwrap_or_default(), r.int(3).unwrap_or(i64::MAX));
        loop {
            let n = eval_stat_batch(rid, item, &field, th)?;
            total += n;
            if commit {
                sql::commit()?;
            }
            if n < 5000 || !commit {
                break;
            }
        }
    }
    Ok(total)
}

// ---------------- 管理 ----------------

fn save_item(id: Option<i64>, b: &Value) -> R<Value> {
    let old = match id {
        Some(id) => Some(
            sql::query(&format!("SELECT {ITEM_COLS} FROM items WHERE id = $1"), &[SqlValue::Int(id)])?
                .rows
                .first()
                .map(|r| item_json(r))
                .ok_or_else(|| err("kernel.not_found", "medal not found"))?,
        ),
        None => None,
    };
    let pick = |k: &str| if b.get(k).is_some() { b[k].clone() } else { old.as_ref().map_or(Value::Null, |o| o[k].clone()) };
    let kind = old.as_ref().map_or_else(|| b["kind"].as_str().unwrap_or("medal").to_owned(), |o| o["kind"].as_str().unwrap_or("").to_owned());
    if !matches!(kind.as_str(), "medal" | "title") {
        return Err(invalid("kind must be medal or title"));
    }
    let name = pick("name").as_str().unwrap_or("").trim().to_owned();
    if name.is_empty() || name.chars().count() > 40 {
        return Err(invalid("name is required (≤ 40 characters)"));
    }
    let opt_pos = |k: &str, max: i64| -> R<SqlValue> {
        match pick(k) {
            Value::Null => Ok(SqlValue::Null),
            v => v.as_i64().filter(|n| (1..=max).contains(n)).map(SqlValue::Int).ok_or_else(|| invalid(&format!("{k} must be 1..{max} or empty"))),
        }
    };
    let vals = vec![
        SqlValue::Text(name),
        SqlValue::Text(pick("description").as_str().unwrap_or("").chars().take(300).collect()),
        opt_pos("max_holders", 1_000_000)?,
        opt_pos("validity_days", 3650)?,
        SqlValue::Boolean(pick("hidden").as_bool().unwrap_or(false)),
        SqlValue::Boolean(pick("active").as_bool().unwrap_or(true)),
        SqlValue::Int(pick("sort").as_i64().unwrap_or(0)),
    ];
    let new_id = if let Some(id) = id {
        let mut p = vec![SqlValue::Int(id)];
        p.extend(vals);
        sql::execute(
            "UPDATE items SET name = $2, description = $3, max_holders = $4, validity_days = $5, hidden = $6, active = $7, sort = $8, updated_at = now() WHERE id = $1",
            &p,
        )?;
        id
    } else {
        let mut p = vec![SqlValue::Text(kind)];
        p.extend(vals);
        one(
            "INSERT INTO items (kind, name, description, max_holders, validity_days, hidden, active, sort) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
            &p,
        )?
    };
    audit::write(&audit::AuditEntry {
        action: if id.is_some() { "medals.item_update" } else { "medals.item_create" }.into(),
        target: format!("medal:{new_id}"),
        details: b.to_string(),
    })?;
    Ok(json!({ "id": new_id }))
}

fn set_icon(id: i64, data: &[u8]) -> R<Value> {
    let (ext, mime) = icon_ext(data).ok_or_else(|| err("medals.bad_icon", "the icon must be a PNG, JPEG, GIF or WebP image"))?;
    let old = sql::query("SELECT icon_key FROM items WHERE id = $1", &[SqlValue::Int(id)])?;
    let Some(old) = old.rows.first() else { return Err(err("kernel.not_found", "medal not found")) };
    let key = format!("public/icons/{id}-{}.{ext}", time::now());
    blob::put(&key, mime, data)?;
    sql::execute("UPDATE items SET icon_key = $2, updated_at = now() WHERE id = $1", &[SqlValue::Int(id), SqlValue::Text(key.clone())])?;
    if let Some(k) = old.text(0) {
        let _ = blob::delete(&k);
    }
    Ok(json!({ "icon": icon_url(Some(key)) }))
}

fn admin_grant(actor: i64, b: &Value) -> R<Value> {
    let item = b["item_id"].as_i64().ok_or_else(|| invalid("item_id is required"))?;
    let reason = b["reason"].as_str().unwrap_or("").trim().to_owned();
    if reason.is_empty() {
        return Err(invalid("reason is required"));
    }
    let names: Vec<String> = b["usernames"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).take(500).collect();
    if names.is_empty() {
        return Err(invalid("usernames is required"));
    }
    let users = sql::query("SELECT id, username FROM identity.v_user_public WHERE username = ANY(string_to_array($1, E'\\n'))", &[SqlValue::Text(names.join("\n"))])?;
    let missing: Vec<&String> = names.iter().filter(|n| !users.rows.iter().any(|r| r.text(1).as_deref() == Some(n.as_str()))).collect();
    let expires = b["expires_days"].as_i64().filter(|d| (1..=3650).contains(d)).map(|d| time::now() + d * 86_400_000);
    let batch = time::now();
    let mut granted = 0;
    for uid in users.rows.iter().filter_map(|r| r.int(0)) {
        let g = Grant {
            user: uid,
            item,
            source: "admin",
            source_plugin: None,
            source_ref: format!("admin:{actor}:{batch}:{uid}"),
            reason: &reason,
            by: Some(actor),
            expires_ms: expires,
        };
        grant(&g)?;
        granted += 1;
    }
    audit::write(&audit::AuditEntry {
        action: "medals.grant".into(),
        target: format!("medal:{item}"),
        details: json!({ "users": names, "reason": reason, "expires_at": expires }).to_string(),
    })?;
    Ok(json!({ "granted": granted, "missing": missing }))
}

fn holders(item: i64) -> R<Value> {
    let r = sql::query(
        &format!(
            "SELECT g.id, g.user_id, u.username, g.source, g.reason, (extract(epoch FROM g.granted_at) * 1000)::bigint, (extract(epoch FROM g.expires_at) * 1000)::bigint
               FROM grants g LEFT JOIN identity.v_user_public u ON u.id = g.user_id WHERE g.item_id = $1 AND {ACTIVE} ORDER BY g.id DESC LIMIT 500"
        ),
        &[SqlValue::Int(item)],
    )?;
    Ok(json!({ "items": r.rows.iter().map(|r| json!({ "grant_id": r.int(0), "user_id": r.int(1), "username": r.text(2), "source": r.text(3),
        "reason": r.text(4), "granted_at": r.int(5), "expires_at": r.int(6) })).collect::<Vec<_>>() }))
}

fn rules() -> R<Value> {
    let r = sql::query(
        "SELECT r.id, r.item_id, i.name, r.kind, r.event, r.measure, r.stat_field, r.threshold, r.description, r.active,
                (SELECT count(*) FROM grants g WHERE g.source_ref LIKE 'ach:' || r.id || ':%')
           FROM rules r JOIN items i ON i.id = r.item_id ORDER BY r.id",
        &[],
    )?;
    Ok(json!({
        "items": r.rows.iter().map(|r| json!({ "id": r.int(0), "item_id": r.int(1), "item_name": r.text(2), "kind": r.text(3), "event": r.text(4),
            "measure": r.text(5), "stat_field": r.text(6), "threshold": r.int(7), "description": r.text(8), "active": r.bool(9), "granted": r.int(10) })).collect::<Vec<_>>(),
        "events": EVENTS.iter().map(|(e, _)| *e).collect::<Vec<_>>(),
        "stat_fields": STAT_FIELDS,
    }))
}

fn create_rule(b: &Value) -> R<Value> {
    let item = b["item_id"].as_i64().ok_or_else(|| invalid("item_id is required"))?;
    if one("SELECT count(*) FROM items WHERE id = $1", &[SqlValue::Int(item)])? == 0 {
        return Err(err("kernel.not_found", "medal not found"));
    }
    let kind = b["kind"].as_str().unwrap_or("");
    let threshold = b["threshold"].as_i64().filter(|t| *t > 0).ok_or_else(|| invalid("threshold must be positive"))?;
    let (event, measure, field) = match kind {
        "event" => {
            let e = b["event"].as_str().filter(|e| EVENTS.iter().any(|(x, _)| x == e)).ok_or_else(|| invalid("unsupported event"))?;
            let m = b["measure"].as_str().unwrap_or("count");
            parse_measure(m).ok_or_else(|| invalid("measure must be count, sum:<field> or max:<field>"))?;
            (SqlValue::Text(e.to_owned()), m.to_owned(), SqlValue::Null)
        }
        "stat" => {
            let f = b["stat_field"].as_str().filter(|f| STAT_FIELDS.contains(f)).ok_or_else(|| invalid("unsupported stat_field"))?;
            (SqlValue::Null, "count".to_owned(), SqlValue::Text(f.to_owned()))
        }
        _ => return Err(invalid("kind must be event or stat")),
    };
    let id = one(
        "INSERT INTO rules (item_id, kind, event, measure, stat_field, threshold, description, active) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
        &[
            SqlValue::Int(item),
            SqlValue::Text(kind.to_owned()),
            event,
            SqlValue::Text(measure),
            field,
            SqlValue::Int(threshold),
            SqlValue::Text(b["description"].as_str().unwrap_or("").chars().take(300).collect()),
            SqlValue::Boolean(b["active"].as_bool().unwrap_or(true)),
        ],
    )?;
    audit::write(&audit::AuditEntry { action: "medals.rule_create".into(), target: format!("medal_rule:{id}"), details: b.to_string() })?;
    Ok(json!({ "id": id }))
}

fn update_rule(id: i64, b: &Value) -> R<()> {
    let threshold = match &b["threshold"] {
        Value::Null => SqlValue::Null,
        v => SqlValue::Int(v.as_i64().filter(|t| *t > 0).ok_or_else(|| invalid("threshold must be positive"))?),
    };
    let n = sql::execute(
        "UPDATE rules SET threshold = coalesce($2, threshold), active = coalesce($3, active), description = coalesce($4, description) WHERE id = $1",
        &[
            SqlValue::Int(id),
            threshold,
            b["active"].as_bool().map_or(SqlValue::Null, SqlValue::Boolean),
            b["description"].as_str().map_or(SqlValue::Null, |d| SqlValue::Text(d.chars().take(300).collect())),
        ],
    )?;
    if n == 0 {
        return Err(err("kernel.not_found", "rule not found"));
    }
    audit::write(&audit::AuditEntry { action: "medals.rule_update".into(), target: format!("medal_rule:{id}"), details: b.to_string() })?;
    Ok(())
}

fn evaluate_rule(id: i64) -> R<Value> {
    let r = sql::query("SELECT item_id, kind, stat_field, threshold, active FROM rules WHERE id = $1", &[SqlValue::Int(id)])?;
    let Some(r) = r.rows.first() else { return Err(err("kernel.not_found", "rule not found")) };
    if r.text(1).as_deref() != Some("stat") {
        return Err(invalid("only stat rules can be evaluated on demand"));
    }
    let n = eval_stat_batch(id, r.int(0).unwrap_or(0), &r.text(2).unwrap_or_default(), r.int(3).unwrap_or(i64::MAX))?;
    Ok(json!({ "granted": n }))
}

fn user_arg(a: &Value) -> R<i64> {
    a["user"].as_i64().or_else(|| a["user_id"].as_i64()).ok_or_else(|| invalid("user_id is required"))
}

fn ids_param(s: &str) -> Vec<i64> {
    s.split(',').filter_map(|x| x.trim().parse().ok()).take(100).collect()
}

// ---------------- 导出 ----------------

impl on_event::Guest for Medals {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        if evt.event_type == "user.deleted" {
            let Some(uid) = p["user_id"].as_i64() else { return Ok(()) };
            revoke_where("g.user_id = $1", &[SqlValue::Int(uid)], "user_deleted")?;
            sql::execute("DELETE FROM display WHERE user_id = $1", &[SqlValue::Int(uid)])?;
            sql::execute("DELETE FROM counters WHERE user_id = $1", &[SqlValue::Int(uid)])?;
            return Ok(());
        }
        on_achievement_event(&evt.event_type, &p)
    }
}

impl on_cron::Guest for Medals {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            "eval_stats" => eval_stats(true).map(|_| ()),
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

impl service_provider::Guest for Medals {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        let out = match method.as_str() {
            "of_user" => json!(holdings(user_arg(&a)?, a["include_hidden"].as_bool().unwrap_or(false))?),
            "worn_many" => {
                let ids: Vec<i64> = a["user_ids"].as_array().into_iter().flatten().filter_map(Value::as_i64).take(500).collect();
                Value::Object(display_many(&ids)?)
            }
            "definitions" => {
                let r = sql::query(&format!("SELECT {ITEM_COLS} FROM items WHERE active ORDER BY sort, id"), &[])?;
                json!(r.rows.iter().map(|r| item_json(r)).collect::<Vec<_>>())
            }
            "grant" => {
                ctx::require_scope("grant")?;
                let key = a["idempotency_key"].as_str().filter(|k| (1..=128).contains(&k.len())).ok_or_else(|| invalid("idempotency_key"))?;
                let caller = ctx::caller();
                grant(&Grant {
                    user: user_arg(&a)?,
                    item: a["medal_id"].as_i64().ok_or_else(|| invalid("medal_id is required"))?,
                    source: "plugin",
                    source_ref: format!("{caller}:{key}"),
                    source_plugin: Some(caller),
                    reason: a["reason"].as_str().unwrap_or(""),
                    by: None,
                    expires_ms: a["expires_at"].as_i64(),
                })?
            }
            "revoke" => {
                ctx::require_scope("grant")?;
                let key = a["idempotency_key"].as_str().filter(|k| (1..=128).contains(&k.len())).ok_or_else(|| invalid("idempotency_key"))?;
                let fresh = sql::execute("INSERT INTO idempotency (caller, key) VALUES ($1, $2) ON CONFLICT DO NOTHING", &[SqlValue::Text(ctx::caller()), SqlValue::Text(key.to_owned())])?;
                let n = if fresh == 1 {
                    let item = a["medal_id"].as_i64().ok_or_else(|| invalid("medal_id is required"))?;
                    revoke_where("g.user_id = $1 AND g.item_id = $2", &[SqlValue::Int(user_arg(&a)?), SqlValue::Int(item)], a["reason"].as_str().unwrap_or(""))?
                } else {
                    0
                };
                json!({ "revoked": n })
            }
            other => return Err(err("kernel.not_found", &format!("ctx.medals has no method `{other}`"))),
        };
        Ok(out.to_string())
    }
}

impl http_handler::Guest for Medals {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let uid = ctx::current_user()?;
        let reply = match req.route() {
            "me" => http::Reply::ok(&json!({ "holdings": holdings(uid, true)?, "display": display_of(uid)?, "achievements": progress(uid)?,
                                             "max_worn": settings().max_worn })),
            "set_display" => http::Reply::ok(&set_display(uid, &req.body_value())?),
            "user_wall" => {
                let id = req.param_i64("id")?;
                http::Reply::ok(&json!({ "holdings": holdings(id, false)?, "display": display_of(id)? }))
            }
            "show" => http::Reply::ok(&Value::Object(display_many(&ids_param(req.query("user_ids").unwrap_or("")))?)),
            "admin_items" => {
                let r = sql::query(
                    &format!("SELECT {ITEM_COLS}, (SELECT count(DISTINCT g.user_id) FROM grants g WHERE g.item_id = items.id AND {ACTIVE}) FROM items ORDER BY sort, id"),
                    &[],
                )?;
                http::Reply::ok(&json!({ "items": r.rows.iter().map(|r| { let mut v = item_json(r); v["holders"] = json!(r.int(10)); v }).collect::<Vec<_>>() }))
            }
            "admin_create" => http::Reply::created(&save_item(None, &req.body_value())?),
            "admin_update" => http::Reply::ok(&save_item(Some(req.param_i64("id")?), &req.body_value())?),
            "admin_icon" => http::Reply::ok(&set_icon(req.param_i64("id")?, req.body_bytes())?),
            "admin_holders" => http::Reply::ok(&holders(req.param_i64("id")?)?),
            "admin_grant" => http::Reply::created(&admin_grant(uid, &req.body_value())?),
            "admin_revoke" => {
                let b = req.body_value();
                let reason = b["reason"].as_str().unwrap_or("").trim().to_owned();
                if reason.is_empty() {
                    return Err(invalid("reason is required"));
                }
                let id = req.param_i64("id")?;
                if revoke_where("g.id = $1", &[SqlValue::Int(id)], &reason)? == 0 {
                    return Err(err("kernel.not_found", "no active grant with this id"));
                }
                audit::write(&audit::AuditEntry { action: "medals.revoke".into(), target: format!("medal_grant:{id}"), details: json!({ "reason": reason }).to_string() })?;
                http::Reply::no_content()
            }
            "admin_rules" => http::Reply::ok(&rules()?),
            "admin_rule_create" => http::Reply::created(&create_rule(&req.body_value())?),
            "admin_rule_update" => {
                update_rule(req.param_i64("id")?, &req.body_value())?;
                http::Reply::no_content()
            }
            "admin_rule_eval" => http::Reply::ok(&evaluate_rule(req.param_i64("id")?)?),
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(Medals);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measures() {
        assert_eq!(parse_measure("count"), Some(("count", None)));
        assert_eq!(parse_measure("max:streak"), Some(("max", Some("streak"))));
        assert_eq!(parse_measure("sum:size"), Some(("sum", Some("size"))));
        assert_eq!(parse_measure("sum:"), None);
        assert_eq!(parse_measure("avg:size"), None);
        assert_eq!(parse_measure("sum:a;drop"), None);
    }

    #[test]
    fn icons() {
        assert_eq!(icon_ext(b"\x89PNG\r\n\x1a\nxxxx").map(|x| x.0), Some("png"));
        assert_eq!(icon_ext(b"RIFF\0\0\0\0WEBPVP8 ").map(|x| x.0), Some("webp"));
        assert_eq!(icon_ext(b"<svg>"), None);
        assert_eq!(icon_url(Some("public/icons/1-2.png".into())), json!("/media/medals/icons/1-2.png"));
    }
}
