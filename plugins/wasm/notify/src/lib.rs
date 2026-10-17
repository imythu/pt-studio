//! notify 插件（WASM，docs/plugins/notify/）：站内通知中心。
//!
//! 本版本实现：`ctx.notify.send` / `send_many`（作用域 `send`；模板键前缀必须是调用方插件 ID；按类别的用户偏好；
//! security 类不可关闭；幂等键）、通知中心（列表、未读数、标记已读）、偏好读写、过期清理。通知的标题与正文由前端按模板术语
//! （`<key>.title` / `<key>.body`，参数插值）渲染，存储的 `title` / `body` 为调用方给出的回退文本。
//! 邮件渠道：按类别与渠道的偏好（security、account 默认开启邮件）、每人每小时上限、待投递记录 + 延迟任务 `deliver`
//! （投递时经 `ctx.users.get_sensitive` 读取已验证的邮箱，需 `user.email` 授权；4xx 永久失败、429 / 5xx 由内核重试）、
//! 提供方 http_relay / postmark / sendgrid、一键退订（签名令牌，List-Unsubscribe）。Telegram、Discord、合并通知在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

pub mod email;

pt_plugin_sdk::world!("notify");

use exports::pt::plugin::{http_handler, on_cron, on_event, service_provider};
use pt_plugin_sdk::{
    config, http_client, invocation, jobs, services,
    sql::{self, SqlValue},
    time,
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

pub const CHANNELS: &[&str] = &["center", "email"];

/// 未设置时的默认值：站内全开；邮件只开 security 与 account
pub fn default_enabled(category: &str, channel: &str) -> bool {
    channel == "center" || matches!(category, "security" | "account")
}

/// 用户是否接收该类别在该渠道的通知（security 类不可关闭）
fn enabled(uid: i64, category: &str, channel: &str) -> R<bool> {
    if category == "security" {
        return Ok(true);
    }
    let r = sql::query(
        "SELECT enabled FROM prefs WHERE user_id = $1 AND category = $2 AND channel = $3",
        &[
            SqlValue::Int(uid),
            SqlValue::Text(category.into()),
            SqlValue::Text(channel.into()),
        ],
    )?;
    Ok(match r.rows.first().map(|row| &row[0]) {
        Some(SqlValue::Boolean(b)) => *b,
        _ => default_enabled(category, channel),
    })
}

fn static_config() -> Value {
    serde_json::from_str(&config::get_static()).unwrap_or(Value::Null)
}

fn email_config() -> Option<email::Config> {
    email::Config::from_static(&static_config())
}

fn set_pref(uid: i64, category: &str, channel: &str, on: bool) -> R<()> {
    sql::execute(
        "INSERT INTO prefs (user_id, category, channel, enabled) VALUES ($1, $2, $3, $4)
         ON CONFLICT (user_id, category, channel) DO UPDATE SET enabled = EXCLUDED.enabled",
        &[
            SqlValue::Int(uid),
            SqlValue::Text(category.into()),
            SqlValue::Text(channel.into()),
            SqlValue::Boolean(on),
        ],
    )?;
    Ok(())
}

struct Send<'a> {
    /// 模板键所属插件；None = 服务调用方
    owner: Option<String>,
    users: Vec<i64>,
    key: &'a str,
    category: &'a str,
    args: &'a Value,
    title: String,
    body: String,
    link: Option<String>,
    idem: Option<String>,
    channels: Vec<String>,
}

/// 发送；返回 (已投递, 跳过)
fn send(s: Send<'_>) -> R<(u32, u32)> {
    let owner = s.owner.clone().unwrap_or_else(|| invocation::current().caller_plugin);
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
    let mail = email_config();
    let per_hour = static_config()["email_per_hour"].as_i64().unwrap_or(20);
    let (mut sent, mut skipped) = (0u32, 0u32);
    for (i, uid) in s.users.iter().enumerate() {
        let idem = s.idem.as_ref().map(|k| {
            if s.users.len() > 1 {
                format!("{k}:{i}")
            } else {
                k.clone()
            }
        });
        if let Some(key) = &idem {
            let dup = sql::query(
                "SELECT 1 FROM notifications WHERE owner_plugin = $1 AND idempotency_key = $2
                 UNION ALL SELECT 1 FROM outbox WHERE owner_plugin = $1 AND idempotency_key = $2",
                &[SqlValue::Text(owner.clone()), SqlValue::Text(key.clone())],
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
        if exists.rows.is_empty() {
            skipped += 1;
            continue;
        }
        let mut delivered = false;
        // 邮件：渠道已配置、用户接收；每人每小时上限
        if mail.is_some() && s.channels.iter().any(|c| c == "email") && enabled(*uid, s.category, "email")? {
            let n = sql::query(
                "SELECT count(*) FROM outbox WHERE user_id = $1 AND channel = 'email' AND created_at > now() - interval '1 hour'",
                &[SqlValue::Int(*uid)],
            )?;
            if n.rows.first().and_then(|r| int(&r[0])).unwrap_or(0) < per_hour {
                let r = sql::query(
                    "INSERT INTO outbox (user_id, channel, owner_plugin, template_key, category, title, body, link, idempotency_key)
                     VALUES ($1, 'email', $2, $3, $4, $5, $6, $7, $8) RETURNING id",
                    &[
                        SqlValue::Int(*uid),
                        SqlValue::Text(owner.clone()),
                        SqlValue::Text(s.key.into()),
                        SqlValue::Text(s.category.into()),
                        SqlValue::Text(s.title.clone()),
                        SqlValue::Text(s.body.clone()),
                        link.clone().map_or(SqlValue::Null, SqlValue::Text),
                        idem.clone().map_or(SqlValue::Null, SqlValue::Text),
                    ],
                )?;
                if let Some(id) = r.rows.first().and_then(|r| int(&r[0])) {
                    jobs::schedule_once(
                        "deliver",
                        time::now(),
                        &json!({ "outbox_id": id }).to_string(),
                        Some(&format!("outbox:{id}")),
                    )?;
                    delivered = true;
                }
            }
        }
        if !s.channels.iter().any(|c| c == "center") || !enabled(*uid, s.category, "center")? {
            if delivered {
                sent += 1;
            } else {
                skipped += 1;
            }
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
                idem.map_or(SqlValue::Null, SqlValue::Text),
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
            "channels_of" => {
                let mut chs = vec!["center"];
                if email_config().is_some() {
                    chs.push("email");
                }
                return Ok(json!(chs).to_string());
            }
            other => {
                return Err(err(
                    "kernel.not_found",
                    &format!("ctx.notify has no method `{other}`"),
                ));
            }
        };
        let args_v = a.get("args").cloned().unwrap_or(json!({}));
        // 渠道：调用方可限定；缺省为全部（再按用户偏好过滤）
        let channels: Vec<String> = match a.get("channels").and_then(Value::as_array) {
            Some(list) => list
                .iter()
                .filter_map(Value::as_str)
                .filter(|c| CHANNELS.contains(c))
                .map(str::to_owned)
                .collect(),
            None => CHANNELS.iter().map(|c| (*c).to_owned()).collect(),
        };
        let (sent, skipped) = send(Send {
            owner: None,
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
            channels: channels.clone(),
        })?;
        Ok(json!({ "sent": sent, "skipped": skipped, "channels": channels }).to_string())
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

/// 退订（LLD §1.7，公开路由，凭签名令牌）：GET 只校验，POST 关闭该类别的邮件
fn unsubscribe(req: &http_handler::Request, apply: bool) -> R<http_handler::Response> {
    let secret = static_config()["unsubscribe_secret"]
        .as_str()
        .unwrap_or("")
        .to_owned();
    let t = query(req, "t").unwrap_or("");
    let Some((uid, category)) = email::verify_token(&secret, t, time::now() / 1000)
        .filter(|(_, c)| CATEGORIES.contains(&c.as_str()) && c != "security")
    else {
        return Err(err(
            "notify.unsubscribe_invalid",
            "unsubscribe link is invalid or expired",
        ));
    };
    if apply {
        set_pref(uid, &category, "email", false)?;
    }
    Ok(resp(200, &json!({ "category": category, "unsubscribed": apply })))
}

impl http_handler::Guest for Notify {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        match req.route_id.as_str() {
            "unsubscribe_check" => return unsubscribe(&req, false),
            "unsubscribe" => return unsubscribe(&req, true),
            _ => {}
        }
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
                    "SELECT category, channel, enabled FROM prefs WHERE user_id = $1",
                    &[SqlValue::Int(uid)],
                )?;
                let email_available = email_config().is_some();
                let mut out = serde_json::Map::new();
                for c in CATEGORIES {
                    let get = |ch: &str| {
                        *c == "security"
                            || r.rows
                                .iter()
                                .find(|row| {
                                    text(&row[0]).as_deref() == Some(c)
                                        && text(&row[1]).as_deref() == Some(ch)
                                })
                                .map_or(default_enabled(c, ch), |row| {
                                    matches!(row[2], SqlValue::Boolean(true))
                                })
                    };
                    out.insert(
                        (*c).into(),
                        json!({ "center": get("center"), "email": get("email"), "email_available": email_available, "locked": *c == "security" }),
                    );
                }
                Ok(resp(200, &Value::Object(out)))
            }
            "put_prefs" => {
                for c in CATEGORIES.iter().filter(|c| **c != "security") {
                    for ch in CHANNELS {
                        if let Some(on) = b[*c][*ch].as_bool() {
                            set_pref(uid, c, ch, on)?;
                        }
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

fn finish(id: i64, state: &str, error: Option<&str>) -> R<()> {
    sql::execute(
        "UPDATE outbox SET state = $2, last_error = $3, sent_at = CASE WHEN $2 = 'sent' THEN now() END WHERE id = $1",
        &[
            SqlValue::Int(id),
            SqlValue::Text(state.into()),
            error.map_or(SqlValue::Null, |e| SqlValue::Text(e.chars().take(300).collect())),
        ],
    )?;
    Ok(())
}

/// 投递一封邮件（延迟任务 `deliver`）：临时失败返回错误，由内核重试
fn deliver(id: i64) -> R<()> {
    let r = sql::query(
        "SELECT user_id, category, title, body, link, state, challenge_id::text FROM outbox WHERE id = $1",
        &[SqlValue::Int(id)],
    )?;
    let Some(row) = r.rows.first() else {
        return Ok(());
    };
    if text(&row[5]).as_deref() != Some("pending") {
        return Ok(());
    }
    let uid = int(&row[0]).unwrap_or(0);
    let category = text(&row[1]).unwrap_or_default();
    sql::execute(
        "UPDATE outbox SET attempts = attempts + 1 WHERE id = $1",
        &[SqlValue::Int(id)],
    )?;
    let Some(cfg) = email_config() else {
        return finish(id, "skipped", Some("email channel is not configured"));
    };
    let (to, link) = if let Some(cid) = text(&row[6]) {
        // 邮件挑战：收件人与链接由 identity 给出（邮箱可以尚未验证）
        let r = services::call(
            "ctx.auth",
            "challenge_delivery",
            &json!({ "challenge_id": cid, "idempotency_key": format!("outbox:{id}") }).to_string(),
        );
        match r {
            Ok(v) => {
                let v: Value = serde_json::from_str(&v).unwrap_or(Value::Null);
                match (v["to"].as_str(), v["link"].as_str()) {
                    (Some(to), Some(link)) => (to.to_owned(), Some(link.to_owned())),
                    _ => return finish(id, "skipped", Some("challenge has no recipient")),
                }
            }
            Err(e) if e.code.starts_with("identity.challenge_") || e.code == "kernel.capability_denied" => {
                return finish(id, "skipped", Some(&e.code));
            }
            Err(e) => return Err(e),
        }
    } else {
        // 投递前再确认偏好（排队期间可能已退订）
        if !enabled(uid, &category, "email")? {
            return finish(id, "skipped", Some("unsubscribed"));
        }
        let addr = match services::call(
            "ctx.users",
            "get_sensitive",
            &json!({ "id": uid, "field": "email" }).to_string(),
        ) {
            Ok(v) => serde_json::from_str::<Value>(&v).unwrap_or(Value::Null),
            Err(e) if e.code == "kernel.capability_denied" => {
                return finish(id, "skipped", Some("user.email is not granted to notify"));
            }
            Err(e) => return Err(e),
        };
        let Some(to) = addr["value"].as_str().filter(|_| addr["verified"] == true) else {
            return finish(id, "skipped", Some("no verified email address"));
        };
        (to.to_owned(), text(&row[4]))
    };
    let unsub = (!cfg.unsubscribe_secret.is_empty() && category != "security" && !cfg.site_url.is_empty())
        .then(|| {
            let t = email::unsubscribe_token(
                &cfg.unsubscribe_secret,
                uid,
                &category,
                time::now() / 1000 + 90 * 86_400,
            );
            email::Unsubscribe {
                page: format!("{}/p/notify/unsubscribe?t={t}", cfg.site_url),
                one_click: format!("{}/api/p/notify/v1/unsubscribe?t={t}", cfg.site_url),
            }
        });
    let m = email::compose(
        &cfg,
        &to,
        &text(&row[2]).unwrap_or_default(),
        &text(&row[3]).unwrap_or_default(),
        link.as_deref(),
        unsub,
    );
    let req = email::request(&cfg, &m);
    let resp = http_client::send(&http_client::OutboundRequest {
        method: "POST".into(),
        url: req.url,
        headers: req
            .headers
            .into_iter()
            .map(|(name, value)| http_client::Header { name, value })
            .collect(),
        body: Some(req.body),
        timeout_ms: 15_000,
    });
    match resp {
        Ok(r) => match email::classify(r.status) {
            email::Outcome::Sent => finish(id, "sent", None),
            email::Outcome::Permanent => {
                finish(id, "failed", Some(&format!("provider returned {}", r.status)))
            }
            email::Outcome::Retry => {
                let msg = format!("provider returned {}", r.status);
                sql::execute(
                    "UPDATE outbox SET last_error = $2 WHERE id = $1",
                    &[SqlValue::Int(id), SqlValue::Text(msg.clone())],
                )?;
                Err(PluginError {
                    code: "notify.delivery_failed".into(),
                    message: msg,
                    details: None,
                    retryable: true,
                })
            }
        },
        Err(e) => Err(PluginError { retryable: true, ..e }),
    }
}

// ---------------- 账号安全事件（identity 发布，notify 投递） ----------------

impl on_event::Guest for Notify {
    fn handle(evt: on_event::EventEnvelope, d: on_event::DeliveryInfo) -> R<()> {
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        match d.subscription.as_str() {
            "on_login" => on_login(&p, &evt.id),
            "on_challenge" => on_challenge(&p),
            _ => Ok(()),
        }
    }
}

/// 新设备登录提醒：identity 在 `user.logged_in` 中标记 `alert = true`（新设备且站点开启提醒）
fn on_login(p: &Value, event_id: &str) -> R<()> {
    let Some(uid) = p["user_id"].as_i64().filter(|_| p["alert"] == true) else {
        return Ok(());
    };
    let ua = p["ua"].as_str().unwrap_or("-");
    let at = p["at"].as_str().unwrap_or("");
    send(Send {
        owner: Some("notify".into()),
        users: vec![uid],
        key: "notify.login_alert",
        category: "security",
        args: &json!({ "ua": ua, "at": at }),
        title: "新设备登录提醒 / New sign-in to your account".into(),
        body: format!(
            "你的账号刚刚在一台新设备上登录（{ua}）。如果不是你本人操作，请立即修改密码并在设置中撤销该会话。\n\
             Your account was just signed in from a new device ({ua}). If this wasn't you, change your password and revoke the session in settings."
        ),
        link: Some("/settings".into()),
        idem: Some(format!("login:{event_id}")),
        channels: CHANNELS.iter().map(|c| (*c).to_owned()).collect(),
    })?;
    Ok(())
}

/// 邮箱验证 / 找回密码邮件：只记挑战 ID，投递时取链接（不受每小时上限约束，identity 自己限流）
fn on_challenge(p: &Value) -> R<()> {
    let (Some(uid), Some(cid)) = (p["user_id"].as_i64(), p["challenge_id"].as_str()) else {
        return Ok(());
    };
    if email_config().is_none() {
        return Ok(());
    }
    let zh = p["locale"].as_str().unwrap_or("zh-CN").starts_with("zh");
    let purpose = p["purpose"].as_str().unwrap_or("");
    let (title, body) = match (purpose, zh) {
        ("password_reset", true) => ("找回密码", "有人（希望是你）申请重置这个账号的密码。点击下面的链接设置新密码，链接 1 小时内有效、只能使用一次。\n如果不是你本人操作，请忽略这封邮件，密码不会改变。"),
        ("password_reset", false) => ("Reset your password", "Someone (hopefully you) asked to reset the password for this account. Open the link below to choose a new one; it works once and expires in an hour.\nIf this wasn't you, ignore this email and your password stays the same."),
        ("email_verify", true) => ("验证邮箱", "请点击下面的链接验证你的邮箱地址，链接 48 小时内有效。"),
        ("email_verify", false) => ("Verify your email address", "Open the link below to verify your email address. It expires in 48 hours."),
        _ => return Ok(()),
    };
    let r = sql::query(
        "INSERT INTO outbox (user_id, channel, owner_plugin, template_key, category, title, body, idempotency_key, challenge_id)
         VALUES ($1, 'email', 'notify', $2, 'security', $3, $4, $5, $6::uuid)
         ON CONFLICT DO NOTHING RETURNING id",
        &[
            SqlValue::Int(uid),
            SqlValue::Text(format!("notify.challenge.{purpose}")),
            SqlValue::Text(title.into()),
            SqlValue::Text(body.into()),
            SqlValue::Text(format!("challenge:{cid}")),
            SqlValue::Text(cid.into()),
        ],
    )?;
    if let Some(id) = r.rows.first().and_then(|r| int(&r[0])) {
        jobs::schedule_once(
            "deliver",
            time::now(),
            &json!({ "outbox_id": id }).to_string(),
            Some(&format!("outbox:{id}")),
        )?;
    }
    Ok(())
}

impl on_cron::Guest for Notify {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            "deliver" => {
                let p: Value = serde_json::from_str(&job.payload).unwrap_or(Value::Null);
                deliver(p["outbox_id"].as_i64().unwrap_or(0))
            }
            "cleanup" => {
                sql::execute(
                    "DELETE FROM notifications WHERE created_at < now() - interval '180 days'",
                    &[],
                )?;
                sql::execute(
                    "DELETE FROM outbox WHERE created_at < now() - interval '30 days'",
                    &[],
                )?;
                Ok(())
            }
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

export!(Notify);
