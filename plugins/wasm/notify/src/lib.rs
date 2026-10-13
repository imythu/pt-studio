//! notify 插件（WASM，docs/plugins/notify/）：站内通知中心。
//!
//! 本版本实现：`ctx.notify.send` / `send_many`（作用域 `send`；模板键前缀必须是调用方插件 ID；按类别的用户偏好；
//! security 类不可关闭；幂等键）、通知中心（列表、未读数、标记已读）、偏好读写、过期清理。通知的标题与正文由前端按模板术语
//! （`<key>.title` / `<key>.body`，参数插值）渲染，存储的 `title` / `body` 为调用方给出的回退文本。邮件、Telegram、Discord
//! 渠道、合并通知、退订链接在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

wit_bindgen::generate!({
    path: "wit",
    world: "notify",
    generate_all,
});

use exports::pt::plugin::{http_handler, on_cron, service_provider};
use pt::plugin::{
    invocation,
    sql::{self, SqlValue},
    types::PluginError,
};
use serde_json::{Value, json};

struct Notify;

type R<T> = Result<T, PluginError>;

pub const CATEGORIES: &[&str] = &["security", "account", "social", "content", "economy", "system"];

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

fn me() -> R<i64> {
    invocation::current()
        .actor
        .user_id
        .and_then(|u| i64::try_from(u).ok())
        .ok_or_else(|| err("kernel.unauthenticated", "login required"))
}

/// 用户关闭了该类别的站内通知？
fn muted(uid: i64, category: &str) -> R<bool> {
    if category == "security" {
        return Ok(false);
    }
    let r = sql::query(
        "SELECT enabled FROM prefs WHERE user_id = $1 AND category = $2 AND channel = 'center'",
        &[SqlValue::Int(uid), SqlValue::Text(category.into())],
    )?;
    Ok(matches!(
        r.rows.first().map(|row| &row[0]),
        Some(SqlValue::Boolean(false))
    ))
}

struct Send<'a> {
    users: Vec<i64>,
    key: &'a str,
    category: &'a str,
    args: &'a Value,
    title: String,
    body: String,
    link: Option<String>,
    idem: Option<String>,
}

/// 发送；返回 (已投递, 跳过)
fn send(s: Send<'_>) -> R<(u32, u32)> {
    let owner = invocation::current().caller_plugin;
    if !s.key.starts_with(&format!("{owner}.")) {
        return Err(err(
            "notify.template_not_owned",
            "template keys must start with the caller plugin id",
        ));
    }
    if !CATEGORIES.contains(&s.category) {
        return Err(err("notify.args_invalid", "unknown category"));
    }
    if s.args.to_string().len() > 8192 || s.title.chars().count() > 300 || s.body.chars().count() > 2000 {
        return Err(err(
            "notify.args_invalid",
            "arguments, title or body are too large",
        ));
    }
    let link = s.link.filter(|l| l.starts_with('/') && !l.starts_with("//"));
    let (mut sent, mut skipped) = (0u32, 0u32);
    for (i, uid) in s.users.iter().enumerate() {
        if let Some(k) = &s.idem {
            let key = if s.users.len() > 1 {
                format!("{k}:{i}")
            } else {
                k.clone()
            };
            let dup = sql::query(
                "SELECT 1 FROM notifications WHERE owner_plugin = $1 AND idempotency_key = $2",
                &[SqlValue::Text(owner.clone()), SqlValue::Text(key)],
            )?;
            if !dup.rows.is_empty() {
                skipped += 1;
                continue;
            }
        }
        let exists = sql::query(
            "SELECT 1 FROM identity.v_user_public WHERE id = $1 AND status <> 'deleted'",
            &[SqlValue::Int(*uid)],
        )?;
        if exists.rows.is_empty() || muted(*uid, s.category)? {
            skipped += 1;
            continue;
        }
        sql::execute(
            "INSERT INTO notifications (user_id, owner_plugin, template_key, category, args, title, body, link, idempotency_key)
             VALUES ($1, $2, $3, $4, $5::jsonb, $6, $7, $8, $9)",
            &[
                SqlValue::Int(*uid),
                SqlValue::Text(owner.clone()),
                SqlValue::Text(s.key.into()),
                SqlValue::Text(s.category.into()),
                SqlValue::Text(s.args.to_string()),
                SqlValue::Text(s.title.clone()),
                SqlValue::Text(s.body.clone()),
                link.clone().map_or(SqlValue::Null, SqlValue::Text),
                s.idem.as_ref().map_or(SqlValue::Null, |k| SqlValue::Text(if s.users.len() > 1 { format!("{k}:{i}") } else { k.clone() })),
            ],
        )?;
        sent += 1;
    }
    Ok((sent, skipped))
}

impl service_provider::Guest for Notify {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        if !invocation::current().caller_scopes.iter().any(|s| s == "send") {
            return Err(err("kernel.forbidden", "scope send required"));
        }
        let users: Vec<i64> = match method.as_str() {
            "send" => vec![
                a["user_id"]
                    .as_i64()
                    .ok_or_else(|| err("notify.args_invalid", "user_id"))?,
            ],
            "send_many" => a["user_ids"]
                .as_array()
                .map(|x| x.iter().filter_map(Value::as_i64).take(1000).collect())
                .unwrap_or_default(),
            "channels_of" => return Ok(json!(["center"]).to_string()),
            other => {
                return Err(err(
                    "kernel.not_found",
                    &format!("ctx.notify has no method `{other}`"),
                ));
            }
        };
        let args_v = a.get("args").cloned().unwrap_or(json!({}));
        let (sent, skipped) = send(Send {
            users,
            key: a["template_key"].as_str().unwrap_or(""),
            category: a["category"].as_str().unwrap_or("system"),
            args: &args_v,
            title: a["title"].as_str().unwrap_or("").to_owned(),
            body: a["body"].as_str().unwrap_or("").to_owned(),
            link: a["link"].as_str().map(str::to_owned),
            idem: a["idempotency_key"]
                .as_str()
                .filter(|k| !k.is_empty())
                .map(|k| k.chars().take(200).collect()),
        })?;
        Ok(json!({ "sent": sent, "skipped": skipped, "channels": ["center"] }).to_string())
    }
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

fn query<'a>(req: &'a http_handler::Request, k: &str) -> Option<&'a str> {
    req.query.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

fn unread(uid: i64) -> R<i64> {
    let r = sql::query(
        "SELECT count(*) FROM notifications WHERE user_id = $1 AND read_at IS NULL",
        &[SqlValue::Int(uid)],
    )?;
    Ok(r.rows.first().and_then(|row| int(&row[0])).unwrap_or(0))
}

impl http_handler::Guest for Notify {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let uid = me()?;
        let b: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
        match req.route_id.as_str() {
            "inbox" => {
                let cursor: i64 = query(&req, "cursor")
                    .and_then(|c| c.parse().ok())
                    .unwrap_or(i64::MAX);
                let unread_only = query(&req, "unread_only") == Some("1");
                let r = sql::query(
                    "SELECT id, template_key, category, args::text, title, body, link, read_at IS NOT NULL, (extract(epoch FROM created_at) * 1000)::bigint
                       FROM notifications WHERE user_id = $1 AND id < $2 AND (NOT $3 OR read_at IS NULL) ORDER BY id DESC LIMIT 51",
                    &[SqlValue::Int(uid), SqlValue::Int(cursor), SqlValue::Boolean(unread_only)],
                )?;
                let mut items: Vec<Value> = r
                    .rows
                    .iter()
                    .map(|row| {
                        json!({ "id": int(&row[0]), "template_key": text(&row[1]), "category": text(&row[2]),
                                "args": text(&row[3]).and_then(|s| serde_json::from_str::<Value>(&s).ok()), "title": text(&row[4]), "body": text(&row[5]),
                                "link": text(&row[6]), "read": matches!(row[7], SqlValue::Boolean(true)), "created_at": int(&row[8]) })
                    })
                    .collect();
                let next = (items.len() > 50).then(|| items[49]["id"].as_i64()).flatten();
                items.truncate(50);
                Ok(resp(
                    200,
                    &json!({ "items": items, "next_cursor": next.map(|n| n.to_string()), "unread": unread(uid)? }),
                ))
            }
            "unread" => Ok(resp(200, &json!({ "count": unread(uid)? }))),
            "read" => {
                if b["all"] == true {
                    sql::execute(
                        "UPDATE notifications SET read_at = now() WHERE user_id = $1 AND read_at IS NULL",
                        &[SqlValue::Int(uid)],
                    )?;
                } else {
                    let ids: Vec<i64> = b["ids"]
                        .as_array()
                        .map(|a| a.iter().filter_map(Value::as_i64).take(500).collect())
                        .unwrap_or_default();
                    sql::execute(
                        "UPDATE notifications SET read_at = now() WHERE user_id = $1 AND read_at IS NULL AND id IN (SELECT jsonb_array_elements_text($2::jsonb)::bigint)",
                        &[SqlValue::Int(uid), SqlValue::Text(json!(ids).to_string())],
                    )?;
                }
                Ok(resp(200, &json!({ "unread": unread(uid)? })))
            }
            "get_prefs" => {
                let r = sql::query(
                    "SELECT category, enabled FROM prefs WHERE user_id = $1 AND channel = 'center'",
                    &[SqlValue::Int(uid)],
                )?;
                let mut out = serde_json::Map::new();
                for c in CATEGORIES {
                    let enabled = r
                        .rows
                        .iter()
                        .find(|row| text(&row[0]).as_deref() == Some(c))
                        .is_none_or(|row| matches!(row[1], SqlValue::Boolean(true)));
                    out.insert(
                        (*c).into(),
                        json!({ "center": enabled, "locked": *c == "security" }),
                    );
                }
                Ok(resp(200, &Value::Object(out)))
            }
            "put_prefs" => {
                for c in CATEGORIES.iter().filter(|c| **c != "security") {
                    if let Some(enabled) = b[*c]["center"].as_bool() {
                        sql::execute(
                            "INSERT INTO prefs (user_id, category, channel, enabled) VALUES ($1, $2, 'center', $3)
                             ON CONFLICT (user_id, category, channel) DO UPDATE SET enabled = EXCLUDED.enabled",
                            &[SqlValue::Int(uid), SqlValue::Text((*c).into()), SqlValue::Boolean(enabled)],
                        )?;
                    }
                }
                Ok(http_handler::Response {
                    status: 204,
                    headers: vec![],
                    body: vec![],
                })
            }
            _ => Err(err("kernel.not_found", "no such route")),
        }
    }
}

impl on_cron::Guest for Notify {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        if job.name != "cleanup" {
            return Err(err("kernel.not_found", "unknown job"));
        }
        sql::execute(
            "DELETE FROM notifications WHERE created_at < now() - interval '180 days'",
            &[],
        )?;
        Ok(())
    }
}

export!(Notify);
