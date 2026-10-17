//! dupe 插件（WASM，docs/plugins/dupe/README.md）：去重与替代（trump）。
//!
//! - 发布钩子：与已有种子内容相同（piece 指纹一致，常见于改了 source 标记重新制种）时拒绝；
//!   槽位策略为 `block` 时，同一作品下已有相同规格（分辨率 / 来源 / 编码 / HDR / 版本）的已发布种子则拒绝。
//! - 槽位策略为 `review`（默认）时，发布后把同槽位的种子记入审核队列，由管理员判定保留、替代或忽略。
//! - 替代：新种子的发布者（或管理员）申请用新种子替代同一作品下的旧种子，管理员批准后旧种子下架（disabled），
//!   通知完成过旧种子的用户；两边种子页互相显示链接。

pt_plugin_sdk::world!("dupe", http);

use exports::pt::plugin::{hooks, http_handler, on_event};
use pt_plugin_sdk::{audit, prelude::*, serde_json, services, sql};

struct Dupe;

const DEFAULT_SLOT_FIELDS: &[&str] = &["resolution", "source", "video_codec", "hdr", "edition"];

struct Settings {
    content_check: bool,
    slot_policy: String,
    slot_fields: Vec<String>,
    notify_snatchers: bool,
}

fn settings() -> Settings {
    let s = ctx::settings();
    Settings {
        content_check: s["content_check"].as_bool().unwrap_or(true),
        slot_policy: s["slot_policy"].as_str().unwrap_or("review").to_owned(),
        slot_fields: s["slot_fields"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_else(|| DEFAULT_SLOT_FIELDS.iter().map(|x| (*x).to_owned()).collect()),
        notify_snatchers: s["notify_snatchers"].as_bool().unwrap_or(true),
    }
}

/// 规格槽位：按设置的字段取值（数组排序后拼接，缺失记为 `-`）；所有字段都缺失时没有槽位
pub fn slot_of(fields: &Value, keys: &[String]) -> Option<String> {
    let mut any = false;
    let parts: Vec<String> = keys
        .iter()
        .map(|k| match &fields[k] {
            Value::Null => "-".to_owned(),
            Value::Array(a) if a.is_empty() => "-".to_owned(),
            Value::Array(a) => {
                any = true;
                let mut v: Vec<String> = a.iter().map(|x| x.as_str().map_or_else(|| x.to_string(), str::to_lowercase)).collect();
                v.sort();
                v.join("+")
            }
            Value::String(s) if s.trim().is_empty() => "-".to_owned(),
            Value::String(s) => {
                any = true;
                s.trim().to_lowercase()
            }
            other => {
                any = true;
                other.to_string()
            }
        })
        .collect();
    any.then(|| parts.join("|"))
}

/// 同一作品下与给定槽位相同的已发布种子（排除自身）
fn slot_peers(group_id: i64, slot: &str, keys: &[String], exclude: i64) -> R<Vec<i64>> {
    let r = sql::query(
        "SELECT f.id, f.fields FROM torrent.v_torrent_fields f JOIN torrent.v_torrent_public t ON t.id = f.id AND t.status = 'approved'
          WHERE f.group_id = $1 AND f.id <> $2 ORDER BY f.id LIMIT 500",
        &[SqlValue::Int(group_id), SqlValue::Int(exclude)],
    )?;
    Ok(r.rows
        .iter()
        .filter(|row| slot_of(&row.json(1).unwrap_or(Value::Null), keys).as_deref() == Some(slot))
        .filter_map(|row| row.int(0))
        .collect())
}

fn deny(code: &str, message: &str, details: Value) -> hooks::HookDecision {
    hooks::HookDecision::Deny(hooks::DenyInfo { code: code.into(), message: message.into(), details: Some(details.to_string()) })
}

fn before_upload(input: &Value) -> R<hooks::HookDecision> {
    let s = settings();
    if s.content_check
        && let Some(fp) = input["content_fp"].as_str().filter(|f| !f.is_empty())
    {
        let r = sql::query(
            "SELECT id FROM torrent.v_torrent_fingerprint WHERE content_fp = decode($1, 'hex') AND status NOT IN ('rejected', 'disabled') LIMIT 1",
            &[SqlValue::Text(fp.to_owned())],
        )?;
        if let Some(id) = r.rows.first().and_then(|r| r.int(0)) {
            return Ok(deny("dupe.same_content", "a torrent with the same content already exists", json!({ "torrent_id": id })));
        }
    }
    if s.slot_policy == "block"
        && let Some(gid) = input["group_id"].as_i64()
        && let Some(slot) = slot_of(&input["fields"], &s.slot_fields)
    {
        let peers = slot_peers(gid, &slot, &s.slot_fields, 0)?;
        if !peers.is_empty() {
            return Ok(deny(
                "dupe.slot_taken",
                "this work already has a torrent with the same specs; ask staff to trump it instead",
                json!({ "torrent_ids": peers, "slot": slot }),
            ));
        }
    }
    Ok(hooks::HookDecision::Allow)
}

/// 发布后：review 策略下记录同槽位的疑似重复
fn on_created(tid: i64) -> R<()> {
    let s = settings();
    if s.slot_policy != "review" {
        return Ok(());
    }
    let r = sql::query("SELECT group_id, fields FROM torrent.v_torrent_fields WHERE id = $1", &[SqlValue::Int(tid)])?;
    let Some(row) = r.rows.first() else { return Ok(()) };
    let (Some(gid), Some(slot)) = (row.int(0), slot_of(&row.json(1).unwrap_or(Value::Null), &s.slot_fields)) else { return Ok(()) };
    let peers = slot_peers(gid, &slot, &s.slot_fields, tid)?;
    if !peers.is_empty() {
        sql::execute(
            "INSERT INTO candidates (torrent_id, matches, slot) VALUES ($1, $2, $3) ON CONFLICT (torrent_id) DO NOTHING",
            &[SqlValue::Int(tid), SqlValue::IntArray(peers), SqlValue::Text(slot)],
        )?;
    }
    Ok(())
}

fn is_staff(uid: i64) -> bool {
    ctx::call("ctx.rbac", "check", &json!({ "user": uid, "permission": "dupe.manage" }))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

struct TorrentRow {
    group_id: Option<i64>,
    status: String,
    uploader: Option<i64>,
}

fn torrent(id: i64) -> R<Option<TorrentRow>> {
    let r = sql::query(
        "SELECT p.group_id, p.status, t.uploader_id FROM torrent.v_torrent_public p JOIN torrent.v_torrent_tracker t ON t.id = p.id WHERE p.id = $1",
        &[SqlValue::Int(id)],
    )?;
    Ok(r.rows.first().map(|row| TorrentRow { group_id: row.int(0), status: row.text(1).unwrap_or_default(), uploader: row.int(2) }))
}

// ---------------- 替代 ----------------

fn request_trump(uid: i64, b: &Value) -> R<Value> {
    let old_id = b["old_id"].as_i64().ok_or_else(|| err("kernel.validation_failed", "old_id is required"))?;
    let new_id = b["new_id"].as_i64().ok_or_else(|| err("kernel.validation_failed", "new_id is required"))?;
    let reason: String = b["reason"].as_str().unwrap_or("").trim().chars().take(500).collect();
    if reason.is_empty() {
        return Err(err("kernel.validation_failed", "a reason is required"));
    }
    if old_id == new_id {
        return Err(err("kernel.validation_failed", "a torrent cannot trump itself"));
    }
    let old = torrent(old_id)?.ok_or_else(|| err("kernel.not_found", "torrent not found"))?;
    let new = torrent(new_id)?.ok_or_else(|| err("kernel.not_found", "torrent not found"))?;
    let staff = is_staff(uid);
    if new.uploader != Some(uid) && !staff {
        return Err(err("kernel.forbidden", "only the uploader of the new torrent or staff can request a trump"));
    }
    if old.group_id.is_none() || old.group_id != new.group_id {
        return Err(err("dupe.not_same_work", "both torrents must belong to the same work"));
    }
    if old.status != "approved" || !matches!(new.status.as_str(), "approved" | "pending" | "candidate") {
        return Err(err("dupe.not_trumpable", "the old torrent must be published"));
    }
    let r = sql::query(
        "INSERT INTO trumps (old_id, new_id, reason, requested_by) VALUES ($1, $2, $3, $4)
         ON CONFLICT (old_id) WHERE state IN ('pending', 'approved') DO NOTHING RETURNING id",
        &[SqlValue::Int(old_id), SqlValue::Int(new_id), SqlValue::Text(reason), SqlValue::Int(uid)],
    )?;
    let id = r.rows.first().and_then(|r| r.int(0)).ok_or_else(|| err("dupe.already_requested", "a trump for this torrent is already pending"))?;
    // 管理员可以直接批准
    if staff && b["approve"].as_bool().unwrap_or(false) {
        decide(uid, id, true, "")?;
    }
    Ok(json!({ "id": id }))
}

fn decide(uid: i64, id: i64, approve: bool, note: &str) -> R<()> {
    let r = sql::query("SELECT old_id, new_id, reason FROM trumps WHERE id = $1 AND state = 'pending' FOR UPDATE", &[SqlValue::Int(id)])?;
    let Some(row) = r.rows.first() else { return Err(err("kernel.not_found", "no pending trump with this id")) };
    let (old_id, new_id, reason) = (row.int(0).unwrap_or(0), row.int(1).unwrap_or(0), row.text(2).unwrap_or_default());
    sql::execute(
        "UPDATE trumps SET state = $2, decided_by = $3, decide_note = $4, decided_at = now() WHERE id = $1",
        &[SqlValue::Int(id), SqlValue::Text(if approve { "approved" } else { "rejected" }.into()), SqlValue::Int(uid), SqlValue::Text(note.chars().take(500).collect())],
    )?;
    audit::write(&audit::AuditEntry {
        action: if approve { "dupe.trump_approved" } else { "dupe.trump_rejected" }.into(),
        target: format!("torrent:{old_id}"),
        details: json!({ "trump_id": id, "new_id": new_id, "reason": reason, "note": note }).to_string(),
    })?;
    if !approve {
        return Ok(());
    }
    ctx::call("ctx.torrents", "set_status", &json!({ "id": old_id, "status": "disabled", "reason": format!("trumped by #{new_id}: {reason}") }))?;
    sql::execute(
        "UPDATE candidates SET state = 'trumped', resolved_by = $2, resolved_at = now()
          WHERE state = 'open' AND (torrent_id = $1 OR torrent_id = $3 OR $1 = ANY(matches))",
        &[SqlValue::Int(old_id), SqlValue::Int(uid), SqlValue::Int(new_id)],
    )?;
    ctx::publish("torrent.trumped", &format!("torrent:{old_id}"), &json!({ "old_id": old_id, "new_id": new_id, "trump_id": id, "reason": reason }))?;
    if settings().notify_snatchers {
        let users = sql::query(
            "SELECT DISTINCT user_id FROM accounting.v_snatch WHERE torrent_id = $1 AND completed_at IS NOT NULL LIMIT 5000",
            &[SqlValue::Int(old_id)],
        )?;
        let mut n = 0i64;
        for u in users.rows.iter().filter_map(|r| r.int(0)) {
            let sent = services::call(
                "ctx.notify",
                "send",
                &json!({ "user_id": u, "template_key": "dupe.trumped", "category": "content",
                         "title": "你下载过的种子已被替代 / A torrent you downloaded was replaced",
                         "body": format!("种子 #{old_id} 已被 #{new_id} 替代：{reason}\nTorrent #{old_id} was replaced by #{new_id}: {reason}"),
                         "link": format!("/torrents/{new_id}"), "args": { "old_id": old_id, "new_id": new_id },
                         "idempotency_key": format!("dupe:trump:{id}:{u}") })
                .to_string(),
            );
            if sent.is_ok() {
                n += 1;
            }
        }
        sql::execute("UPDATE trumps SET notified = $2 WHERE id = $1", &[SqlValue::Int(id), SqlValue::Int(n)])?;
    }
    Ok(())
}

fn torrent_info(tid: i64, uid: i64) -> R<Value> {
    let link = |q: &str| -> R<Vec<Value>> {
        let r = sql::query(q, &[SqlValue::Int(tid)])?;
        Ok(r.rows
            .iter()
            .map(|row| json!({ "trump_id": row.int(0), "torrent_id": row.int(1), "title": row.text(2), "reason": row.text(3), "state": row.text(4) }))
            .collect())
    };
    let trumped_by = link(
        "SELECT x.id, x.new_id, t.title, x.reason, x.state FROM trumps x LEFT JOIN torrent.v_torrent_public t ON t.id = x.new_id
          WHERE x.old_id = $1 AND x.state IN ('pending', 'approved') ORDER BY x.id DESC LIMIT 1",
    )?;
    let replaces = link(
        "SELECT x.id, x.old_id, t.title, x.reason, x.state FROM trumps x LEFT JOIN torrent.v_torrent_public t ON t.id = x.old_id
          WHERE x.new_id = $1 AND x.state IN ('pending', 'approved') ORDER BY x.id",
    )?;
    let s = settings();
    let me = torrent(tid)?;
    let staff = is_staff(uid);
    // 同作品同槽位的其他已发布种子（替代候选）
    let mut peers = Vec::new();
    if let Some(TorrentRow { group_id: Some(gid), .. }) = &me {
        let f = sql::query("SELECT fields FROM torrent.v_torrent_fields WHERE id = $1", &[SqlValue::Int(tid)])?;
        if let Some(slot) = f.rows.first().and_then(|r| slot_of(&r.json(0).unwrap_or(Value::Null), &s.slot_fields)) {
            for id in slot_peers(*gid, &slot, &s.slot_fields, tid)? {
                let t = sql::query("SELECT title FROM torrent.v_torrent_public WHERE id = $1", &[SqlValue::Int(id)])?;
                peers.push(json!({ "torrent_id": id, "title": t.rows.first().and_then(|r| r.text(0)) }));
            }
        }
    }
    // 其他同作品的已发布种子：发布者 / 管理员可从中选择被替代者
    let mut others = Vec::new();
    let can_request = me.as_ref().is_some_and(|t| t.uploader == Some(uid)) || staff;
    if can_request && let Some(TorrentRow { group_id: Some(gid), .. }) = &me {
        let r = sql::query(
            "SELECT id, title FROM torrent.v_torrent_public WHERE group_id = $1 AND id <> $2 AND status = 'approved' ORDER BY id DESC LIMIT 100",
            &[SqlValue::Int(*gid), SqlValue::Int(tid)],
        )?;
        others = r.rows.iter().map(|row| json!({ "torrent_id": row.int(0), "title": row.text(1) })).collect();
    }
    Ok(json!({ "trumped_by": trumped_by.first(), "replaces": replaces, "slot_peers": peers, "can_request": can_request && !others.is_empty(),
               "candidates": others, "staff": staff }))
}

fn queue() -> R<Value> {
    let t = sql::query(
        "SELECT x.id, x.old_id, o.title, x.new_id, n.title, x.reason, u.username, (extract(epoch FROM x.created_at) * 1000)::bigint
           FROM trumps x LEFT JOIN torrent.v_torrent_public o ON o.id = x.old_id LEFT JOIN torrent.v_torrent_public n ON n.id = x.new_id
           LEFT JOIN identity.v_user_public u ON u.id = x.requested_by
          WHERE x.state = 'pending' ORDER BY x.created_at LIMIT 200",
        &[],
    )?;
    let trumps: Vec<Value> = t
        .rows
        .iter()
        .map(|r| json!({ "id": r.int(0), "old_id": r.int(1), "old_title": r.text(2), "new_id": r.int(3), "new_title": r.text(4),
                         "reason": r.text(5), "requested_by": r.text(6), "created_at": r.int(7) }))
        .collect();
    let c = sql::query(
        "SELECT c.id, c.torrent_id, t.title, c.matches, c.slot, (extract(epoch FROM c.created_at) * 1000)::bigint
           FROM candidates c LEFT JOIN torrent.v_torrent_public t ON t.id = c.torrent_id
          WHERE c.state = 'open' ORDER BY c.created_at LIMIT 200",
        &[],
    )?;
    let candidates: Vec<Value> = c
        .rows
        .iter()
        .map(|r| {
            let matches = match r.value(3) {
                Some(SqlValue::IntArray(v)) => v.clone(),
                _ => vec![],
            };
            json!({ "id": r.int(0), "torrent_id": r.int(1), "title": r.text(2), "matches": matches, "slot": r.text(4), "created_at": r.int(5) })
        })
        .collect();
    Ok(json!({ "trumps": trumps, "candidates": candidates }))
}

// ---------------- 导出 ----------------

impl hooks::Guest for Dupe {
    fn handle(c: hooks::HookCall) -> R<hooks::HookDecision> {
        let input: Value = serde_json::from_str(&c.input).unwrap_or(Value::Null);
        match c.hook.as_str() {
            "torrent.before_upload" => before_upload(&input),
            _ => Ok(hooks::HookDecision::Allow),
        }
    }
}

impl on_event::Guest for Dupe {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        let Some(tid) = p["torrent_id"].as_i64() else { return Ok(()) };
        match evt.event_type.as_str() {
            "torrent.created" => on_created(tid),
            "torrent.deleted" => {
                sql::execute("UPDATE candidates SET state = 'dismissed', resolved_at = now() WHERE torrent_id = $1 AND state = 'open'", &[SqlValue::Int(tid)])?;
                sql::execute(
                    "UPDATE trumps SET state = 'rejected', decide_note = 'torrent deleted', decided_at = now()
                      WHERE state = 'pending' AND (old_id = $1 OR new_id = $1)",
                    &[SqlValue::Int(tid)],
                )?;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

impl http_handler::Guest for Dupe {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let uid = ctx::current_user()?;
        let reply = match req.route() {
            "torrent" => http::Reply::ok(&torrent_info(req.param_i64("id")?, uid)?),
            "request" => http::Reply::created(&request_trump(uid, &req.body_value())?),
            "queue" => http::Reply::ok(&queue()?),
            "decide" => {
                let b = req.body_value();
                let approve = match b["decision"].as_str() {
                    Some("approve") => true,
                    Some("reject") => false,
                    _ => return Err(err("kernel.validation_failed", "decision must be approve or reject")),
                };
                decide(uid, req.param_i64("id")?, approve, b["note"].as_str().unwrap_or(""))?;
                http::Reply::no_content()
            }
            "dismiss" => {
                let n = sql::execute(
                    "UPDATE candidates SET state = 'dismissed', resolved_by = $2, resolved_at = now() WHERE id = $1 AND state = 'open'",
                    &[SqlValue::Int(req.param_i64("id")?), SqlValue::Int(uid)],
                )?;
                if n == 0 {
                    return Err(err("kernel.not_found", "no open candidate with this id"));
                }
                http::Reply::no_content()
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(Dupe);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots() {
        let keys: Vec<String> = DEFAULT_SLOT_FIELDS.iter().map(|x| (*x).to_owned()).collect();
        let a = json!({ "resolution": "2160p", "source": "bluray", "video_codec": "hevc", "hdr": ["hdr10", "dv"] });
        let b = json!({ "resolution": "2160P", "source": "BluRay", "video_codec": "hevc", "hdr": ["dv", "hdr10"], "edition": "" });
        assert_eq!(slot_of(&a, &keys), slot_of(&b, &keys));
        assert_eq!(slot_of(&a, &keys).as_deref(), Some("2160p|bluray|hevc|dv+hdr10|-"));
        let c = json!({ "resolution": "2160p", "source": "web", "video_codec": "hevc" });
        assert_ne!(slot_of(&a, &keys), slot_of(&c, &keys));
        assert!(slot_of(&json!({ "title": "x" }), &keys).is_none(), "no spec fields, no slot");
    }
}
