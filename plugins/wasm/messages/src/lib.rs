//! messages 插件（WASM，docs/plugins/messages/）：站内信。
//!
//! 本版本实现：会话（发起：按用户名收件、去重、拉黑与“只接收员工”检查、频率限制、`message.before_send` 钩子；回复：重新进入
//! 其他参与者的收件箱、未读数；已读；移出）、收件箱 / 已发送 / 系统文件夹、会话详情（Markdown / BBCode 经 pt-markup）、
//! 系统消息（`ctx.messages.send_system`，作用域 `send`）、拉黑、接收设置、未读数、举报某条消息（快照经 `ctx.reports` 提交）、
//! MCP 工具 `list_my_messages`（不含正文）/ `send_message`（需确认）、群发公告（一行存储 + 用户已读游标，可按最低等级、
//! 到期时间投放，可撤回；`ctx.messages.broadcast`，作用域 `broadcast`）。星标、数据清理在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

pt_plugin_sdk::world!("messages");

use exports::pt::plugin::{http_handler, mcp_tool, service_provider};
use pt_plugin_sdk::{
    audit, config, events, hook_runner, invocation, redis, services,
    sql::{self, SqlValue},
    types::PluginError,
};
use serde_json::{Value, json};

struct Messages;

type R<T> = Result<T, PluginError>;

fn err(code: &str, message: &str) -> PluginError {
    PluginError {
        code: code.into(),
        message: message.into(),
        details: None,
        retryable: false,
    }
}

fn text(v: &SqlValue) -> Option<String> {
    match v {
        SqlValue::Text(s) | SqlValue::Json(s) => Some(s.clone()),
        _ => None,
    }
}

fn int(v: &SqlValue) -> Option<i64> {
    match v {
        SqlValue::Int(i) => Some(*i),
        _ => None,
    }
}

fn setting(k: &str, d: i64) -> i64 {
    let s: Value = serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null);
    s[k].as_i64().unwrap_or(d)
}

fn me() -> R<i64> {
    invocation::current()
        .actor
        .user_id
        .and_then(|u| i64::try_from(u).ok())
        .ok_or_else(|| err("kernel.unauthenticated", "login required"))
}

fn staff(uid: i64) -> bool {
    services::call(
        "ctx.rbac",
        "check",
        &json!({ "user": uid, "permission": "messages.broadcast" }).to_string(),
    )
    .is_ok_and(|r| r == "true")
}

fn publish(event: &str, key: &str, payload: Value) -> R<()> {
    events::publish(&events::NewEvent {
        event_type: event.into(),
        version: 1,
        key: key.into(),
        payload: payload.to_string(),
    })?;
    Ok(())
}

fn render(body: &str, markup: &str) -> pt_markup::Rendered {
    let m = pt_markup::Markup::parse(markup).unwrap_or(pt_markup::Markup::Markdown);
    pt_markup::render(body, m, &pt_markup::RenderOptions::default())
}

fn rate_limited(uid: i64) -> R<bool> {
    let key = format!("rl:{uid}");
    let n = match redis::call(&redis::Command {
        name: "INCR".into(),
        args: vec![key.as_bytes().to_vec()],
    })? {
        redis::RedisReply::Int(n) => n,
        _ => 0,
    };
    if n == 1 {
        redis::call(&redis::Command {
            name: "EXPIRE".into(),
            args: vec![key.as_bytes().to_vec(), b"3600".to_vec()],
        })?;
    }
    Ok(n > setting("rate_per_hour", 30))
}

/// 用户名 / ID → 用户 ID
fn resolve(r: &Value) -> R<Option<i64>> {
    let q = match r {
        Value::Number(n) => sql::query(
            "SELECT id FROM identity.v_user_public WHERE id = $1 AND status <> 'deleted'",
            &[SqlValue::Int(n.as_i64().unwrap_or(0))],
        )?,
        Value::String(s) => sql::query(
            "SELECT id FROM identity.v_user_public WHERE lower(username) = lower($1) AND status <> 'deleted'",
            &[SqlValue::Text(s.clone())],
        )?,
        _ => return Ok(None),
    };
    Ok(q.rows.first().and_then(|row| int(&row[0])))
}

fn insert_message(conv: i64, sender: Option<i64>, body: &str, markup: &str) -> R<i64> {
    let r = render(body, markup);
    let ins = sql::query(
        "INSERT INTO messages (conversation_id, sender_id, markup, source, rendered, plain, render_version) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        &[
            SqlValue::Int(conv),
            sender.map_or(SqlValue::Null, SqlValue::Int),
            SqlValue::Text(markup.into()),
            SqlValue::Text(body.into()),
            SqlValue::Text(r.html),
            SqlValue::Text(r.text.chars().take(20_000).collect()),
            SqlValue::Int(i64::from(pt_markup::RENDER_VERSION)),
        ],
    )?;
    let id = ins.rows.first().and_then(|row| int(&row[0])).unwrap_or(0);
    sql::execute(
        "UPDATE conversations SET last_message_id = $2, last_message_at = now() WHERE id = $1",
        &[SqlValue::Int(conv), SqlValue::Int(id)],
    )?;
    Ok(id)
}

fn check_body(subject: &str, body: &str) -> R<()> {
    if subject.trim().is_empty() || subject.chars().count() > 200 {
        return Err(err(
            "kernel.validation_failed",
            "subject must be 1..=200 characters",
        ));
    }
    if body.trim().is_empty()
        || body.chars().count() > usize::try_from(setting("max_length", 20_000)).unwrap_or(20_000)
    {
        return Err(err("messages.too_long", "message is empty or too long"));
    }
    Ok(())
}

/// 发起会话（LLD §1.1）
fn create(sender: i64, recipients: &[Value], subject: &str, body: &str, markup: &str) -> R<Value> {
    check_body(subject, body)?;
    let max = usize::try_from(setting("max_recipients", 5)).unwrap_or(5);
    if recipients.is_empty() || recipients.len() > max {
        return Err(err("kernel.validation_failed", &format!("1..={max} recipients")));
    }
    let is_staff = staff(sender);
    let mut ids: Vec<i64> = Vec::new();
    for (i, r) in recipients.iter().enumerate() {
        let Some(id) = resolve(r)? else {
            let mut e = err("messages.recipient_not_found", "recipient not found");
            e.details = Some(json!({ "recipient_index": i }).to_string());
            return Err(e);
        };
        if id == sender || ids.contains(&id) {
            continue;
        }
        if !is_staff {
            let blocked = sql::query(
                "SELECT 1 FROM blocks WHERE user_id = $1 AND blocked_id = $2",
                &[SqlValue::Int(id), SqlValue::Int(sender)],
            )?;
            if !blocked.rows.is_empty() {
                let mut e = err("messages.blocked", "the recipient does not accept your messages");
                e.details = Some(json!({ "recipient_index": i }).to_string());
                return Err(e);
            }
            let s = sql::query(
                "SELECT accept FROM settings WHERE user_id = $1",
                &[SqlValue::Int(id)],
            )?;
            if s.rows.first().and_then(|row| text(&row[0])).as_deref() == Some("staff_only") {
                let mut e = err(
                    "messages.recipient_not_accepting",
                    "the recipient only accepts messages from staff",
                );
                e.details = Some(json!({ "recipient_index": i }).to_string());
                return Err(e);
            }
        }
        ids.push(id);
    }
    if ids.is_empty() {
        return Err(err("kernel.validation_failed", "no valid recipients"));
    }
    if !is_staff && rate_limited(sender)? {
        return Err(err("messages.rate_limited", "you are sending messages too fast"));
    }
    let hook = json!({ "version": 1, "sender_id": sender, "recipient_ids": ids, "subject": subject, "body_length": body.chars().count() });
    if let hook_runner::HookOutcome::Denied(d) = hook_runner::run("message.before_send", &hook.to_string())? {
        return Err(err(&d.code, &d.message));
    }
    let c = sql::query(
        "INSERT INTO conversations (kind, subject, created_by) VALUES ('user', $1, $2) RETURNING id",
        &[SqlValue::Text(subject.trim().into()), SqlValue::Int(sender)],
    )?;
    let conv = c.rows.first().and_then(|row| int(&row[0])).unwrap_or(0);
    let mid = insert_message(conv, Some(sender), body, markup)?;
    sql::execute(
        "INSERT INTO participants (conversation_id, user_id, in_sent, last_read_message_id) VALUES ($1, $2, true, $3)",
        &[SqlValue::Int(conv), SqlValue::Int(sender), SqlValue::Int(mid)],
    )?;
    for r in &ids {
        sql::execute(
            "INSERT INTO participants (conversation_id, user_id, in_inbox, unread_count) VALUES ($1, $2, true, 1)",
            &[SqlValue::Int(conv), SqlValue::Int(*r)],
        )?;
        publish(
            "message.sent",
            &format!("user:{r}"),
            json!({ "message_id": mid, "conversation_id": conv, "sender_id": sender, "recipient_id": r, "kind": "user" }),
        )?;
    }
    Ok(json!({ "conversation_id": conv, "message_id": mid }))
}

fn participant(conv: i64, uid: i64) -> R<bool> {
    let r = sql::query(
        "SELECT 1 FROM participants WHERE conversation_id = $1 AND user_id = $2 AND removed_at IS NULL",
        &[SqlValue::Int(conv), SqlValue::Int(uid)],
    )?;
    Ok(!r.rows.is_empty())
}

fn reply(uid: i64, conv: i64, body: &str, markup: &str) -> R<Value> {
    if !participant(conv, uid)? {
        return Err(err("messages.not_found", "conversation not found"));
    }
    let kind = sql::query(
        "SELECT kind, subject FROM conversations WHERE id = $1",
        &[SqlValue::Int(conv)],
    )?;
    let row = kind
        .rows
        .first()
        .ok_or_else(|| err("messages.not_found", "conversation not found"))?;
    if text(&row[0]).as_deref() == Some("system") {
        return Err(err(
            "messages.reply_not_allowed",
            "system messages cannot be replied to",
        ));
    }
    check_body(&text(&row[1]).unwrap_or_default(), body)?;
    if !staff(uid) && rate_limited(uid)? {
        return Err(err("messages.rate_limited", "you are sending messages too fast"));
    }
    let mid = insert_message(conv, Some(uid), body, markup)?;
    sql::execute(
        "UPDATE participants SET in_inbox = CASE WHEN user_id = $2 THEN in_inbox ELSE true END, in_sent = CASE WHEN user_id = $2 THEN true ELSE in_sent END,
                unread_count = CASE WHEN user_id = $2 THEN 0 ELSE unread_count + 1 END,
                last_read_message_id = CASE WHEN user_id = $2 THEN $3 ELSE last_read_message_id END
          WHERE conversation_id = $1 AND removed_at IS NULL",
        &[SqlValue::Int(conv), SqlValue::Int(uid), SqlValue::Int(mid)],
    )?;
    let others = sql::query(
        "SELECT user_id FROM participants WHERE conversation_id = $1 AND user_id <> $2 AND removed_at IS NULL",
        &[SqlValue::Int(conv), SqlValue::Int(uid)],
    )?;
    for o in others.rows.iter().filter_map(|row| int(&row[0])) {
        publish(
            "message.sent",
            &format!("user:{o}"),
            json!({ "message_id": mid, "conversation_id": conv, "sender_id": uid, "recipient_id": o, "kind": "user" }),
        )?;
    }
    Ok(json!({ "conversation_id": conv, "message_id": mid }))
}

fn send_system(uid: i64, subject: &str, body: &str) -> R<Value> {
    check_body(subject, body)?;
    let c = sql::query(
        "INSERT INTO conversations (kind, subject, created_by_plugin) VALUES ('system', $1, $2) RETURNING id",
        &[
            SqlValue::Text(subject.into()),
            SqlValue::Text(invocation::current().caller_plugin),
        ],
    )?;
    let conv = c.rows.first().and_then(|row| int(&row[0])).unwrap_or(0);
    let mid = insert_message(conv, None, body, "markdown")?;
    sql::execute(
        "INSERT INTO participants (conversation_id, user_id, in_inbox, folder, unread_count) VALUES ($1, $2, true, 'system', 1)",
        &[SqlValue::Int(conv), SqlValue::Int(uid)],
    )?;
    publish(
        "message.sent",
        &format!("user:{uid}"),
        json!({ "message_id": mid, "conversation_id": conv, "sender_id": null, "recipient_id": uid, "kind": "system" }),
    )?;
    Ok(json!({ "conversation_id": conv, "message_id": mid }))
}

// ---------------- 群发公告 ----------------

/// 对该用户可见的公告条件（$1 = 用户 ID）
const VISIBLE: &str = "b.revoked_at IS NULL AND (b.expires_at IS NULL OR b.expires_at > now())
    AND (b.min_class IS NULL OR coalesce((SELECT class_id FROM identity.v_user_public WHERE id = $1), 0) >= b.min_class)
    AND NOT EXISTS (SELECT 1 FROM broadcast_hidden h WHERE h.user_id = $1 AND h.broadcast_id = b.id)";

fn broadcasts_for(uid: i64) -> R<Value> {
    let r = sql::query(
        &format!(
            "SELECT b.id, b.subject, b.body_html, (extract(epoch FROM b.created_at) * 1000)::bigint, b.id > coalesce((SELECT last_read FROM broadcast_cursors WHERE user_id = $1), 0)
               FROM broadcasts b WHERE {VISIBLE} ORDER BY b.id DESC LIMIT 50"
        ),
        &[SqlValue::Int(uid)],
    )?;
    Ok(json!({ "items": r.rows.iter().map(|row| json!({ "id": int(&row[0]), "subject": text(&row[1]), "html": text(&row[2]), "created_at": int(&row[3]),
        "unread": matches!(row[4], SqlValue::Boolean(true)) })).collect::<Vec<_>>() }))
}

fn unread_broadcasts(uid: i64) -> R<i64> {
    let r = sql::query(
        &format!("SELECT count(*) FROM broadcasts b WHERE {VISIBLE} AND b.id > coalesce((SELECT last_read FROM broadcast_cursors WHERE user_id = $1), 0)"),
        &[SqlValue::Int(uid)],
    )?;
    Ok(r.rows.first().and_then(|row| int(&row[0])).unwrap_or(0))
}

struct NewBroadcast<'a> {
    subject: &'a str,
    body: &'a str,
    markup: &'a str,
    min_class: Option<i64>,
    expires_ms: Option<i64>,
    created_by: Option<i64>,
    source: Option<(String, String)>,
}

fn broadcast(n: &NewBroadcast) -> R<Value> {
    check_body(n.subject, n.body)?;
    if let Some((plugin, key)) = &n.source {
        let dup = sql::query("SELECT id FROM broadcasts WHERE source_plugin = $1 AND idempotency_key = $2", &[SqlValue::Text(plugin.clone()), SqlValue::Text(key.clone())])?;
        if let Some(row) = dup.rows.first() {
            return Ok(json!({ "id": int(&row[0]) }));
        }
    }
    let html = render(n.body, n.markup).html;
    let r = sql::query(
        "INSERT INTO broadcasts (subject, body, body_html, markup, min_class, created_by, source_plugin, idempotency_key, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, CASE WHEN $9::bigint IS NULL THEN NULL ELSE to_timestamp($9::bigint / 1000.0) END) RETURNING id",
        &[
            SqlValue::Text(n.subject.into()),
            SqlValue::Text(n.body.into()),
            SqlValue::Text(html),
            SqlValue::Text(n.markup.into()),
            n.min_class.map_or(SqlValue::Null, SqlValue::Int),
            n.created_by.map_or(SqlValue::Null, SqlValue::Int),
            n.source.as_ref().map_or(SqlValue::Null, |s| SqlValue::Text(s.0.clone())),
            n.source.as_ref().map_or(SqlValue::Null, |s| SqlValue::Text(s.1.clone())),
            n.expires_ms.map_or(SqlValue::Null, SqlValue::Int),
        ],
    )?;
    let id = r.rows.first().and_then(|row| int(&row[0])).unwrap_or(0);
    publish("message.broadcast_sent", "global", json!({ "broadcast_id": id, "subject": n.subject, "min_class": n.min_class }))?;
    Ok(json!({ "id": id }))
}

fn admin_broadcasts() -> R<Value> {
    let r = sql::query(
        "SELECT b.id, b.subject, b.min_class, (extract(epoch FROM b.created_at) * 1000)::bigint, (extract(epoch FROM b.expires_at) * 1000)::bigint,
                b.revoked_at IS NOT NULL, u.username, b.source_plugin, (SELECT count(*) FROM broadcast_cursors c WHERE c.last_read >= b.id)
           FROM broadcasts b LEFT JOIN identity.v_user_public u ON u.id = b.created_by ORDER BY b.id DESC LIMIT 100",
        &[],
    )?;
    Ok(json!({ "items": r.rows.iter().map(|row| json!({ "id": int(&row[0]), "subject": text(&row[1]), "min_class": int(&row[2]), "created_at": int(&row[3]),
        "expires_at": int(&row[4]), "revoked": matches!(row[5], SqlValue::Boolean(true)), "created_by": text(&row[6]), "source_plugin": text(&row[7]), "read": int(&row[8]) })).collect::<Vec<_>>() }))
}

fn unread(uid: i64) -> R<i64> {
    let r = sql::query(
        "SELECT coalesce(sum(unread_count), 0)::bigint FROM participants WHERE user_id = $1 AND removed_at IS NULL AND in_inbox",
        &[SqlValue::Int(uid)],
    )?;
    Ok(r.rows.first().and_then(|row| int(&row[0])).unwrap_or(0))
}

fn list(uid: i64, folder: &str, unread_only: bool) -> R<Value> {
    let cond = match folder {
        "sent" => "p.in_sent",
        "system" => "p.in_inbox AND p.folder = 'system'",
        _ => "p.in_inbox AND p.folder = 'normal'",
    };
    let r = sql::query(
        &format!(
            "SELECT c.id, c.subject, c.kind, (extract(epoch FROM c.last_message_at) * 1000)::bigint, p.unread_count,
                    (SELECT json_agg(json_build_object('id', u.id, 'username', u.username))::text FROM participants o JOIN identity.v_user_public u ON u.id = o.user_id
                      WHERE o.conversation_id = c.id AND o.user_id <> $1)
               FROM participants p JOIN conversations c ON c.id = p.conversation_id
              WHERE p.user_id = $1 AND p.removed_at IS NULL AND {cond} AND (NOT $2 OR p.unread_count > 0)
              ORDER BY c.last_message_at DESC LIMIT 100"
        ),
        &[SqlValue::Int(uid), SqlValue::Boolean(unread_only)],
    )?;
    let items: Vec<Value> = r
        .rows
        .iter()
        .map(|row| {
            json!({ "id": int(&row[0]), "subject": text(&row[1]), "kind": text(&row[2]), "last_message_at": int(&row[3]), "unread": int(&row[4]),
                    "participants": text(&row[5]).and_then(|s| serde_json::from_str::<Value>(&s).ok()).unwrap_or(json!([])) })
        })
        .collect();
    Ok(json!({ "items": items }))
}

// ---------------- HTTP ----------------

fn resp(status: u16, v: &Value) -> http_handler::Response {
    http_handler::Response {
        status,
        headers: vec![http_handler::Header {
            name: "content-type".into(),
            value: "application/json".into(),
        }],
        body: v.to_string().into_bytes(),
    }
}

fn id_param(req: &http_handler::Request) -> R<i64> {
    req.params
        .iter()
        .find(|(n, _)| n == "id")
        .and_then(|(_, v)| v.parse().ok())
        .ok_or_else(|| err("kernel.validation_failed", "bad id"))
}

fn query<'a>(req: &'a http_handler::Request, k: &str) -> Option<&'a str> {
    req.query.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

fn no_content() -> http_handler::Response {
    http_handler::Response {
        status: 204,
        headers: vec![],
        body: vec![],
    }
}

impl http_handler::Guest for Messages {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let uid = me()?;
        let b: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
        match req.route_id.as_str() {
            "list" => Ok(resp(
                200,
                &list(
                    uid,
                    query(&req, "folder").unwrap_or("inbox"),
                    query(&req, "unread_only") == Some("1"),
                )?,
            )),
            "create" => {
                let recipients = b["recipients"].as_array().cloned().unwrap_or_default();
                Ok(resp(
                    201,
                    &create(
                        uid,
                        &recipients,
                        b["subject"].as_str().unwrap_or(""),
                        b["body"].as_str().unwrap_or(""),
                        b["markup"].as_str().unwrap_or("markdown"),
                    )?,
                ))
            }
            "get" => {
                let conv = id_param(&req)?;
                if !participant(conv, uid)? {
                    return Err(err("messages.not_found", "conversation not found"));
                }
                let c = sql::query(
                    "SELECT subject, kind FROM conversations WHERE id = $1",
                    &[SqlValue::Int(conv)],
                )?;
                let crow = c
                    .rows
                    .first()
                    .ok_or_else(|| err("messages.not_found", "conversation not found"))?;
                let m = sql::query(
                    "SELECT m.id, m.sender_id, u.username, m.rendered, (extract(epoch FROM m.created_at) * 1000)::bigint
                       FROM messages m LEFT JOIN identity.v_user_public u ON u.id = m.sender_id WHERE m.conversation_id = $1 ORDER BY m.id LIMIT 500",
                    &[SqlValue::Int(conv)],
                )?;
                let parts = sql::query(
                    "SELECT u.id, u.username FROM participants p JOIN identity.v_user_public u ON u.id = p.user_id WHERE p.conversation_id = $1",
                    &[SqlValue::Int(conv)],
                )?;
                // 打开即已读
                sql::execute(
                    "UPDATE participants SET unread_count = 0, last_read_message_id = coalesce((SELECT last_message_id FROM conversations WHERE id = $1), 0)
                      WHERE conversation_id = $1 AND user_id = $2",
                    &[SqlValue::Int(conv), SqlValue::Int(uid)],
                )?;
                Ok(resp(
                    200,
                    &json!({ "id": conv, "subject": text(&crow[0]), "kind": text(&crow[1]),
                             "participants": parts.rows.iter().map(|r| json!({ "id": int(&r[0]), "username": text(&r[1]) })).collect::<Vec<_>>(),
                             "messages": m.rows.iter().map(|r| json!({ "id": int(&r[0]), "sender": int(&r[1]).map(|s| json!({ "id": s, "username": text(&r[2]) })),
                                                                       "html": text(&r[3]), "created_at": int(&r[4]) })).collect::<Vec<_>>() }),
                ))
            }
            "reply" => Ok(resp(
                201,
                &reply(
                    uid,
                    id_param(&req)?,
                    b["body"].as_str().unwrap_or(""),
                    b["markup"].as_str().unwrap_or("markdown"),
                )?,
            )),
            "remove" => {
                let n = sql::execute(
                    "UPDATE participants SET in_inbox = false, in_sent = false, unread_count = 0, removed_at = now() WHERE conversation_id = $1 AND user_id = $2 AND removed_at IS NULL",
                    &[SqlValue::Int(id_param(&req)?), SqlValue::Int(uid)],
                )?;
                if n == 0 {
                    return Err(err("messages.not_found", "conversation not found"));
                }
                Ok(no_content())
            }
            "unread" => Ok(resp(200, &json!({ "private": unread(uid)?, "broadcasts": unread_broadcasts(uid)? }))),
            "broadcasts" => Ok(resp(200, &broadcasts_for(uid)?)),
            "broadcasts_read" => {
                sql::execute(
                    "INSERT INTO broadcast_cursors (user_id, last_read) VALUES ($1, coalesce((SELECT max(id) FROM broadcasts), 0))
                     ON CONFLICT (user_id) DO UPDATE SET last_read = EXCLUDED.last_read",
                    &[SqlValue::Int(uid)],
                )?;
                Ok(no_content())
            }
            "broadcast_hide" => {
                sql::execute(
                    "INSERT INTO broadcast_hidden (user_id, broadcast_id) SELECT $1, id FROM broadcasts WHERE id = $2 ON CONFLICT DO NOTHING",
                    &[SqlValue::Int(uid), SqlValue::Int(id_param(&req)?)],
                )?;
                Ok(no_content())
            }
            "admin_broadcasts" => Ok(resp(200, &admin_broadcasts()?)),
            "admin_broadcast_create" => {
                let expires = b["expires_days"].as_i64().filter(|d| (1..=365).contains(d)).map(|d| pt_plugin_sdk::time::now() + d * 86_400_000);
                let out = broadcast(&NewBroadcast {
                    subject: b["subject"].as_str().unwrap_or("").trim(),
                    body: b["body"].as_str().unwrap_or(""),
                    markup: b["markup"].as_str().unwrap_or("markdown"),
                    min_class: b["min_class"].as_i64().filter(|c| *c > 0),
                    expires_ms: expires,
                    created_by: Some(uid),
                    source: None,
                })?;
                audit::write(&audit::AuditEntry {
                    action: "messages.broadcast".into(),
                    target: format!("broadcast:{}", out["id"]),
                    details: json!({ "subject": b["subject"], "min_class": b["min_class"], "expires_days": b["expires_days"] }).to_string(),
                })?;
                Ok(resp(201, &out))
            }
            "admin_broadcast_revoke" => {
                let id = id_param(&req)?;
                let n = sql::execute("UPDATE broadcasts SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL", &[SqlValue::Int(id)])?;
                if n == 0 {
                    return Err(err("messages.not_found", "broadcast not found"));
                }
                audit::write(&audit::AuditEntry { action: "messages.broadcast_revoke".into(), target: format!("broadcast:{id}"), details: "{}".into() })?;
                Ok(no_content())
            }
            "blocks" => {
                let r = sql::query(
                    "SELECT b.blocked_id, u.username FROM blocks b LEFT JOIN identity.v_user_public u ON u.id = b.blocked_id WHERE b.user_id = $1 ORDER BY b.created_at DESC",
                    &[SqlValue::Int(uid)],
                )?;
                Ok(resp(
                    200,
                    &json!({ "items": r.rows.iter().map(|row| json!({ "id": int(&row[0]), "username": text(&row[1]) })).collect::<Vec<_>>() }),
                ))
            }
            "block" => {
                let target = resolve(&b["user"])?
                    .ok_or_else(|| err("messages.recipient_not_found", "user not found"))?;
                if target == uid {
                    return Err(err("kernel.validation_failed", "you cannot block yourself"));
                }
                sql::execute(
                    "INSERT INTO blocks (user_id, blocked_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                    &[SqlValue::Int(uid), SqlValue::Int(target)],
                )?;
                Ok(no_content())
            }
            "unblock" => {
                sql::execute(
                    "DELETE FROM blocks WHERE user_id = $1 AND blocked_id = $2",
                    &[SqlValue::Int(uid), SqlValue::Int(id_param(&req)?)],
                )?;
                Ok(no_content())
            }
            "get_settings" => {
                let r = sql::query(
                    "SELECT accept FROM settings WHERE user_id = $1",
                    &[SqlValue::Int(uid)],
                )?;
                Ok(resp(
                    200,
                    &json!({ "accept": r.rows.first().and_then(|row| text(&row[0])).unwrap_or_else(|| "all".into()) }),
                ))
            }
            "put_settings" => {
                let accept = b["accept"].as_str().unwrap_or("");
                if !matches!(accept, "all" | "staff_only") {
                    return Err(err(
                        "kernel.validation_failed",
                        "accept must be all or staff_only",
                    ));
                }
                sql::execute(
                    "INSERT INTO settings (user_id, accept) VALUES ($1, $2) ON CONFLICT (user_id) DO UPDATE SET accept = EXCLUDED.accept",
                    &[SqlValue::Int(uid), SqlValue::Text(accept.into())],
                )?;
                Ok(no_content())
            }
            // 举报某条消息：经 ctx.reports 提交快照（举报者须是会话参与者且同意提交内容）
            "report" => {
                let mid = id_param(&req)?;
                if b["consent"] != true {
                    return Err(err(
                        "kernel.validation_failed",
                        "consent is required to share the message with staff",
                    ));
                }
                let m = sql::query(
                    "SELECT conversation_id, sender_id, left(plain, 5000), (extract(epoch FROM created_at) * 1000)::bigint FROM messages WHERE id = $1",
                    &[SqlValue::Int(mid)],
                )?;
                let row = m
                    .rows
                    .first()
                    .ok_or_else(|| err("messages.not_found", "message not found"))?;
                let conv = int(&row[0]).unwrap_or(0);
                if !participant(conv, uid)? || int(&row[1]) == Some(uid) || int(&row[1]).is_none() {
                    return Err(err("messages.not_found", "message not found"));
                }
                let snapshot = json!({ "conversation_id": conv, "message_id": mid, "sender_id": int(&row[1]), "sent_at": int(&row[3]), "text": text(&row[2]) });
                let out = services::call(
                    "ctx.reports",
                    "create",
                    &json!({ "reporter_id": uid, "target": format!("message:{mid}"), "reason_code": b["reason_code"], "note": b["note"].as_str().unwrap_or(""), "snapshot": snapshot }).to_string(),
                )?;
                Ok(resp(
                    201,
                    &serde_json::from_str::<Value>(&out).unwrap_or(Value::Null),
                ))
            }
            _ => Err(err("kernel.not_found", "no such route")),
        }
    }
}

// ---------------- ctx.messages ----------------

impl service_provider::Guest for Messages {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        let out = match method.as_str() {
            "unread_count" => json!(unread(a["user_id"].as_i64().unwrap_or(0))?),
            "send_system" => {
                if !invocation::current().caller_scopes.iter().any(|s| s == "send") {
                    return Err(err("kernel.forbidden", "scope send required"));
                }
                send_system(
                    a["user_id"]
                        .as_i64()
                        .ok_or_else(|| err("kernel.validation_failed", "user_id"))?,
                    a["subject"].as_str().unwrap_or(""),
                    a["body"].as_str().unwrap_or(""),
                )?
            }
            "broadcast" => {
                let inv = invocation::current();
                if !inv.caller_scopes.iter().any(|s| s == "broadcast") {
                    return Err(err("kernel.forbidden", "scope broadcast required"));
                }
                let key = a["idempotency_key"]
                    .as_str()
                    .filter(|k| (1..=128).contains(&k.len()))
                    .ok_or_else(|| err("kernel.validation_failed", "idempotency_key"))?;
                broadcast(&NewBroadcast {
                    subject: a["subject"].as_str().unwrap_or(""),
                    body: a["body"].as_str().unwrap_or(""),
                    markup: "markdown",
                    min_class: a["min_class"].as_i64().filter(|c| *c > 0),
                    expires_ms: a["expires_at"].as_i64(),
                    created_by: None,
                    source: Some((inv.caller_plugin, key.to_owned())),
                })?
            }
            other => {
                return Err(err(
                    "kernel.not_found",
                    &format!("ctx.messages has no method `{other}`"),
                ));
            }
        };
        Ok(out.to_string())
    }
}

// ---------------- MCP ----------------

impl mcp_tool::Guest for Messages {
    fn call(tool: String, arguments: String) -> R<String> {
        let a: Value = serde_json::from_str(&arguments).unwrap_or(Value::Null);
        let uid = me()?;
        let out = match tool.as_str() {
            // 不含正文
            "list_my_messages" => list(
                uid,
                a["folder"].as_str().unwrap_or("inbox"),
                a["unread_only"] == true,
            )?,
            "send_message" => {
                let to = a["to"].as_array().cloned().unwrap_or_default();
                create(
                    uid,
                    &to,
                    a["subject"].as_str().unwrap_or(""),
                    a["body"].as_str().unwrap_or(""),
                    "markdown",
                )?
            }
            _ => return Err(err("kernel.not_found", "unknown tool")),
        };
        Ok(out.to_string())
    }
}

export!(Messages);
