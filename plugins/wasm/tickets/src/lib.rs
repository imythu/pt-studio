//! tickets 插件（WASM）：联系管理组（工单）。
//!
//! - 用户按类别（账号、种子、申诉、建议、其他）提交工单，同时未关闭的数量有上限；可回复、关闭，关闭后一段时间内可重新打开。
//! - 员工（`tickets.handle`）在队列中按状态筛选、认领 / 指派、回复（工单转为“等待用户”）、写内部备注（用户看不到）、关闭。
//! - 正文经 pt-markup 渲染（Markdown）；员工回复通知用户，用户回复通知被指派的员工。

pt_plugin_sdk::world!("tickets", http);

use exports::pt::plugin::{http_handler, on_event};
use pt_plugin_sdk::{audit, prelude::*, serde_json, services, sql};

struct Tickets;

pub const CATEGORIES: &[&str] = &["account", "torrent", "appeal", "suggestion", "other"];

fn one(q: &str, p: &[SqlValue]) -> R<i64> {
    Ok(sql::query(q, p)?.rows.first().and_then(|r| r.int(0)).unwrap_or(0))
}

fn invalid(m: &str) -> PluginError {
    err("kernel.validation_failed", m)
}

fn is_staff(uid: i64) -> bool {
    ctx::call("ctx.rbac", "check", &json!({ "user": uid, "permission": "tickets.handle" }))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn render(body: &str) -> String {
    pt_markup::render(body, pt_markup::Markup::Markdown, &pt_markup::RenderOptions::default()).html
}

/// 正文校验：1～20000 字
pub fn check_body(body: &str) -> Result<(), &'static str> {
    let n = body.trim().chars().count();
    if n == 0 || n > 20_000 { Err("body must be 1 to 20000 characters") } else { Ok(()) }
}

fn notify(uid: i64, tid: i64, title: &str, key: &str) {
    let _ = services::call(
        "ctx.notify",
        "send",
        &json!({ "user_id": uid, "template_key": "tickets.update", "category": "account", "title": title, "body": "",
                 "link": format!("/p/tickets/{tid}"), "args": { "ticket_id": tid }, "idempotency_key": key })
        .to_string(),
    );
}

fn add_message(tid: i64, author: i64, staff: bool, internal: bool, body: &str) -> R<i64> {
    check_body(body).map_err(invalid)?;
    one(
        "INSERT INTO ticket_messages (ticket_id, author_id, staff, internal, body, body_html) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
        &[SqlValue::Int(tid), SqlValue::Int(author), SqlValue::Boolean(staff), SqlValue::Boolean(internal), SqlValue::Text(body.to_owned()), SqlValue::Text(render(body))],
    )
}

struct Row {
    user: i64,
    state: String,
    assignee: Option<i64>,
    subject: String,
    closed_days: Option<i64>,
}

fn load(tid: i64, lock: bool) -> R<Row> {
    let r = sql::query(
        &format!(
            "SELECT user_id, state, assignee_id, subject, (extract(epoch FROM now() - closed_at) / 86400)::bigint FROM tickets WHERE id = $1{}",
            if lock { " FOR UPDATE" } else { "" }
        ),
        &[SqlValue::Int(tid)],
    )?;
    let row = r.rows.first().ok_or_else(|| err("kernel.not_found", "ticket not found"))?;
    Ok(Row { user: row.int(0).unwrap_or(0), state: row.text(1).unwrap_or_default(), assignee: row.int(2), subject: row.text(3).unwrap_or_default(), closed_days: row.int(4) })
}

/// 本人或员工可见；否则按不存在处理
fn access(uid: i64, t: &Row) -> R<bool> {
    let staff = is_staff(uid);
    if t.user != uid && !staff {
        return Err(err("kernel.not_found", "ticket not found"));
    }
    Ok(staff)
}

const LIST_COLS: &str = "t.id, t.category, t.subject, t.state, (extract(epoch FROM t.created_at) * 1000)::bigint, (extract(epoch FROM t.updated_at) * 1000)::bigint,
    u.username, a.username, t.assignee_id,
    (SELECT m.staff FROM ticket_messages m WHERE m.ticket_id = t.id AND NOT m.internal ORDER BY m.id DESC LIMIT 1)";

fn list_json(r: &[Vec<SqlValue>]) -> Vec<Value> {
    r.iter()
        .map(|r| {
            json!({ "id": r.int(0), "category": r.text(1), "subject": r.text(2), "state": r.text(3), "created_at": r.int(4), "updated_at": r.int(5),
                    "username": r.text(6), "assignee": r.text(7), "assignee_id": r.int(8), "last_reply_staff": r.bool(9) })
        })
        .collect()
}

fn create(uid: i64, b: &Value) -> R<Value> {
    let category = b["category"].as_str().filter(|c| CATEGORIES.contains(c)).ok_or_else(|| invalid("unknown category"))?;
    let subject = b["subject"].as_str().unwrap_or("").trim().to_owned();
    if subject.is_empty() || subject.chars().count() > 200 {
        return Err(invalid("subject is required (≤ 200 characters)"));
    }
    let body = b["body"].as_str().unwrap_or("");
    check_body(body).map_err(invalid)?;
    let max = ctx::settings()["max_open"].as_i64().unwrap_or(3).clamp(1, 20);
    if one("SELECT count(*) FROM tickets WHERE user_id = $1 AND state <> 'closed'", &[SqlValue::Int(uid)])? >= max {
        return Err(err("tickets.too_many_open", &format!("you can have at most {max} open tickets")));
    }
    let tid = one(
        "INSERT INTO tickets (user_id, category, subject) VALUES ($1, $2, $3) RETURNING id",
        &[SqlValue::Int(uid), SqlValue::Text(category.to_owned()), SqlValue::Text(subject)],
    )?;
    add_message(tid, uid, false, false, body)?;
    ctx::publish("ticket.created", &format!("user:{uid}"), &json!({ "ticket_id": tid, "user_id": uid, "category": category }))?;
    Ok(json!({ "id": tid }))
}

fn detail(uid: i64, tid: i64) -> R<Value> {
    let t = load(tid, false)?;
    let staff = access(uid, &t)?;
    let r = sql::query(&format!("SELECT {LIST_COLS} FROM tickets t LEFT JOIN identity.v_user_public u ON u.id = t.user_id LEFT JOIN identity.v_user_public a ON a.id = t.assignee_id WHERE t.id = $1"), &[SqlValue::Int(tid)])?;
    let mut v = list_json(&r.rows).into_iter().next().unwrap_or(Value::Null);
    let m = sql::query(
        "SELECT m.id, m.staff, m.internal, m.body_html, (extract(epoch FROM m.created_at) * 1000)::bigint, u.username
           FROM ticket_messages m LEFT JOIN identity.v_user_public u ON u.id = m.author_id
          WHERE m.ticket_id = $1 AND ($2 OR NOT m.internal) ORDER BY m.id",
        &[SqlValue::Int(tid), SqlValue::Boolean(staff)],
    )?;
    // 员工姓名只对员工显示，用户看到的是“管理组”
    v["messages"] = json!(m
        .rows
        .iter()
        .map(|r| {
            let is_staff_msg = r.bool(1).unwrap_or(false);
            json!({ "id": r.int(0), "staff": is_staff_msg, "internal": r.bool(2), "html": r.text(3), "created_at": r.int(4),
                    "author": if is_staff_msg && !staff { Value::Null } else { json!(r.text(5)) } })
        })
        .collect::<Vec<_>>());
    v["viewer_staff"] = json!(staff);
    if !staff {
        v["assignee"] = Value::Null;
        v["assignee_id"] = Value::Null;
    }
    Ok(v)
}

fn reply(uid: i64, tid: i64, b: &Value) -> R<Value> {
    let t = load(tid, true)?;
    let staff = access(uid, &t)?;
    let own = t.user == uid;
    let internal = staff && b["internal"].as_bool().unwrap_or(false);
    if t.state == "closed" && !internal {
        return Err(err("tickets.closed", "this ticket is closed"));
    }
    let body = b["body"].as_str().unwrap_or("");
    // 员工回复自己提交的工单视作用户回复
    let as_staff = staff && !own;
    let mid = add_message(tid, uid, as_staff, internal, body)?;
    if !internal {
        let state = if as_staff { "pending_user" } else { "open" };
        // 员工首次回复时自动认领
        sql::execute(
            "UPDATE tickets SET state = $2, updated_at = now(), assignee_id = CASE WHEN $3 THEN coalesce(assignee_id, $4) ELSE assignee_id END WHERE id = $1",
            &[SqlValue::Int(tid), SqlValue::Text(state.into()), SqlValue::Boolean(as_staff), SqlValue::Int(uid)],
        )?;
        if as_staff {
            notify(t.user, tid, &format!("管理组回复了你的工单「{0}」 / Staff replied to \"{0}\"", t.subject), &format!("tickets:reply:{mid}"));
        } else if let Some(a) = t.assignee {
            notify(a, tid, &format!("工单「{0}」有新回复 / New reply on \"{0}\"", t.subject), &format!("tickets:reply:{mid}"));
        }
    }
    Ok(json!({ "id": mid }))
}

fn close(uid: i64, tid: i64) -> R<()> {
    let t = load(tid, true)?;
    let staff = access(uid, &t)?;
    if t.state == "closed" {
        return Ok(());
    }
    sql::execute("UPDATE tickets SET state = 'closed', closed_at = now(), closed_by = $2, updated_at = now() WHERE id = $1", &[SqlValue::Int(tid), SqlValue::Int(uid)])?;
    if staff && t.user != uid {
        audit::write(&audit::AuditEntry { action: "tickets.close".into(), target: format!("ticket:{tid}"), details: "{}".into() })?;
        notify(t.user, tid, &format!("你的工单「{0}」已关闭 / Your ticket \"{0}\" was closed", t.subject), &format!("tickets:close:{tid}:{}", pt_plugin_sdk::time::now()));
    }
    ctx::publish("ticket.closed", &format!("user:{}", t.user), &json!({ "ticket_id": tid, "user_id": t.user, "by": uid }))?;
    Ok(())
}

fn reopen(uid: i64, tid: i64) -> R<()> {
    let t = load(tid, true)?;
    let staff = access(uid, &t)?;
    if t.state != "closed" {
        return Ok(());
    }
    let days = ctx::settings()["reopen_days"].as_i64().unwrap_or(14);
    if !staff && t.closed_days.unwrap_or(0) >= days {
        return Err(err("tickets.reopen_expired", "this ticket was closed too long ago; please open a new one"));
    }
    if !staff {
        let max = ctx::settings()["max_open"].as_i64().unwrap_or(3).clamp(1, 20);
        if one("SELECT count(*) FROM tickets WHERE user_id = $1 AND state <> 'closed'", &[SqlValue::Int(uid)])? >= max {
            return Err(err("tickets.too_many_open", &format!("you can have at most {max} open tickets")));
        }
    }
    sql::execute("UPDATE tickets SET state = 'open', closed_at = NULL, closed_by = NULL, updated_at = now() WHERE id = $1", &[SqlValue::Int(tid)])?;
    Ok(())
}

fn queue(uid: i64, state: &str, mine: bool) -> R<Value> {
    let state = if matches!(state, "open" | "pending_user" | "closed") { state } else { "open" };
    let r = sql::query(
        &format!(
            "SELECT {LIST_COLS} FROM tickets t LEFT JOIN identity.v_user_public u ON u.id = t.user_id LEFT JOIN identity.v_user_public a ON a.id = t.assignee_id
              WHERE t.state = $1 AND (NOT $2 OR t.assignee_id = $3) ORDER BY t.updated_at {} LIMIT 200",
            if state == "closed" { "DESC" } else { "ASC" }
        ),
        &[SqlValue::Text(state.into()), SqlValue::Boolean(mine), SqlValue::Int(uid)],
    )?;
    let counts = sql::query("SELECT state, count(*) FROM tickets WHERE state <> 'closed' GROUP BY state", &[])?;
    let c = |s: &str| counts.rows.iter().find(|r| r.text(0).as_deref() == Some(s)).and_then(|r| r.int(1)).unwrap_or(0);
    Ok(json!({ "items": list_json(&r.rows), "counts": { "open": c("open"), "pending_user": c("pending_user") } }))
}

fn assign(actor: i64, tid: i64, b: &Value) -> R<()> {
    load(tid, true)?;
    let to = match &b["user_id"] {
        Value::Null => Some(actor),
        v if v.as_i64() == Some(0) => None,
        v => {
            let id = v.as_i64().ok_or_else(|| invalid("user_id"))?;
            if !is_staff(id) {
                return Err(invalid("the assignee must be able to handle tickets"));
            }
            Some(id)
        }
    };
    sql::execute("UPDATE tickets SET assignee_id = $2, updated_at = now() WHERE id = $1", &[SqlValue::Int(tid), to.map_or(SqlValue::Null, SqlValue::Int)])?;
    audit::write(&audit::AuditEntry { action: "tickets.assign".into(), target: format!("ticket:{tid}"), details: json!({ "assignee": to }).to_string() })?;
    Ok(())
}

impl on_event::Guest for Tickets {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        if evt.event_type == "user.deleted" {
            let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
            if let Some(uid) = p["user_id"].as_i64() {
                sql::execute("UPDATE tickets SET state = 'closed', closed_at = coalesce(closed_at, now()) WHERE user_id = $1", &[SqlValue::Int(uid)])?;
            }
        }
        Ok(())
    }
}

impl http_handler::Guest for Tickets {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let uid = ctx::current_user()?;
        let reply_ = match req.route() {
            "mine" => {
                let r = sql::query(
                    &format!("SELECT {LIST_COLS} FROM tickets t LEFT JOIN identity.v_user_public u ON u.id = t.user_id LEFT JOIN identity.v_user_public a ON a.id = t.assignee_id WHERE t.user_id = $1 ORDER BY t.id DESC LIMIT 100"),
                    &[SqlValue::Int(uid)],
                )?;
                let mut items = list_json(&r.rows);
                for i in &mut items {
                    i["assignee"] = Value::Null;
                    i["assignee_id"] = Value::Null;
                }
                http::Reply::ok(&json!({ "items": items }))
            }
            "create" => http::Reply::created(&create(uid, &req.body_value())?),
            "get" => http::Reply::ok(&detail(uid, req.param_i64("id")?)?),
            "reply" => http::Reply::created(&reply(uid, req.param_i64("id")?, &req.body_value())?),
            "close" => {
                close(uid, req.param_i64("id")?)?;
                http::Reply::no_content()
            }
            "reopen" => {
                reopen(uid, req.param_i64("id")?)?;
                http::Reply::no_content()
            }
            "queue" => http::Reply::ok(&queue(uid, req.query("state").unwrap_or("open"), req.query("mine") == Some("1"))?),
            "assign" => {
                assign(uid, req.param_i64("id")?, &req.body_value())?;
                http::Reply::no_content()
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply_.into())
    }
}

export!(Tickets);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bodies() {
        assert!(check_body("  ").is_err());
        assert!(check_body("你好").is_ok());
        assert!(check_body(&"a".repeat(20_001)).is_err());
    }
}
