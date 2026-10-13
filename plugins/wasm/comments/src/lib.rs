//! comments 插件（WASM，docs/plugins/comments/）：种子与作品的评论。
//!
//! 本版本实现：目标（`torrent:<id>`、`group:<id>`，按视图判断存在与可见性；待审种子只对发布者与版主可见）、发表（Markdown /
//! BBCode 经 pt-markup 渲染、@提醒解析、楼层号、单层回复、引用、`comment.before_create` 钩子、幂等键）、防刷（30 秒 / 每小时 /
//! 新用户每日次数、近 1 小时 simhash 重复检测、链接数）、列表（楼层分页、回复聚合、感谢数与本人是否感谢）、编辑（作者时限内或
//! 版主，保留历史）、删除（作者 / 版主，占位显示）、感谢、预览、用户评论历史、`ctx.comments`（计数、列表、取原文、代发、删除）、
//! 事件。图片上传、表情、@提醒通知（ctx.notify）、重新渲染任务在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

wit_bindgen::generate!({
    path: "wit",
    world: "comments",
    generate_all,
});

pub mod logic;

use exports::pt::plugin::{http_handler, service_provider};
use pt::plugin::{
    config, events, hook_runner, invocation, redis, services,
    sql::{self, SqlValue},
    types::PluginError,
};
use serde_json::{Value, json};

struct Comments;

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

fn settings() -> Value {
    serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null)
}

fn setting(k: &str, default: i64) -> i64 {
    settings()[k].as_i64().unwrap_or(default)
}

fn me() -> R<i64> {
    invocation::current()
        .actor
        .user_id
        .and_then(|u| i64::try_from(u).ok())
        .ok_or_else(|| err("kernel.unauthenticated", "login required"))
}

fn has(uid: i64, perm: &str) -> bool {
    services::call(
        "ctx.rbac",
        "check",
        &json!({ "user": uid, "permission": perm }).to_string(),
    )
    .is_ok_and(|r| r == "true")
}

fn moderator(uid: i64) -> bool {
    has(uid, "comments.moderate")
}

/// 目标是否存在且对 uid 可见；返回 (可见, 可评论)
fn target_state(kind: &str, id: i64, uid: Option<i64>) -> R<(bool, bool)> {
    match kind {
        "torrent" => {
            let r = sql::query(
                "SELECT status, uploader_id FROM torrent.v_torrent_public WHERE id = $1",
                &[SqlValue::Int(id)],
            )?;
            let Some(row) = r.rows.first() else {
                return Ok((false, false));
            };
            let status = text(&row[0]).unwrap_or_default();
            let commentable = matches!(status.as_str(), "approved" | "candidate");
            let visible = commentable
                || (status == "pending" && uid.is_some_and(|u| int(&row[1]) == Some(u) || moderator(u)));
            Ok((visible, commentable))
        }
        "group" => {
            let r = sql::query(
                "SELECT 1 FROM torrent.v_group_public WHERE id = $1",
                &[SqlValue::Int(id)],
            )?;
            Ok((!r.rows.is_empty(), !r.rows.is_empty()))
        }
        _ => Ok((false, false)),
    }
}

fn check_target(target: &str, uid: Option<i64>) -> R<bool> {
    let (kind, id) =
        logic::parse_target(target).ok_or_else(|| err("comments.target_not_found", "unknown target"))?;
    let disabled: Vec<String> = settings()["disabled_targets"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    if disabled.iter().any(|d| d == kind) {
        return Err(err(
            "comments.target_disabled",
            "comments are disabled for this kind of target",
        ));
    }
    let (visible, commentable) = target_state(kind, id, uid)?;
    if !visible {
        return Err(err("comments.target_not_found", "target not found"));
    }
    Ok(commentable)
}

fn username_id(name: &str) -> Option<u64> {
    let r = sql::query(
        "SELECT id FROM identity.v_user_public WHERE lower(username) = lower($1) AND status <> 'deleted'",
        &[SqlValue::Text(name.to_owned())],
    )
    .ok()?;
    r.rows
        .first()
        .and_then(|row| int(&row[0]))
        .and_then(|i| u64::try_from(i).ok())
}

fn render(body: &str, markup: pt_markup::Markup) -> pt_markup::Rendered {
    let hosts: Vec<String> = settings()["image_hosts"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    let resolver = |n: &str| username_id(n);
    let opt = pt_markup::RenderOptions {
        image_hosts: &hosts,
        mention_resolver: Some(&resolver),
        ..Default::default()
    };
    pt_markup::render(body, markup, &opt)
}

fn incr(key: &str, ttl: i64) -> R<i64> {
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
            args: vec![key.as_bytes().to_vec(), ttl.to_string().into_bytes()],
        })?;
    }
    Ok(n)
}

/// 防刷（LLD §1.3）
fn antispam(uid: i64, plain: &str, links: u32) -> R<()> {
    let limited = || err("comments.rate_limited", "you are posting too fast");
    if incr(&format!("rl:30s:{uid}"), 30)? > setting("rate_per_30s", 3) {
        return Err(limited());
    }
    if incr(&format!("rl:1h:{uid}"), 3600)? > setting("rate_per_hour", 60) {
        return Err(limited());
    }
    let new_days = setting("new_user_days", 7);
    let age = sql::query(
        "SELECT (extract(epoch FROM now() - created_at) / 86400)::bigint FROM identity.v_user_public WHERE id = $1",
        &[SqlValue::Int(uid)],
    )?;
    if age
        .rows
        .first()
        .and_then(|r| int(&r[0]))
        .is_some_and(|d| d < new_days)
        && incr(&format!("rl:day:{uid}"), 86_400)? > setting("new_user_daily_limit", 20)
    {
        return Err(limited());
    }
    if i64::from(links) > setting("max_links", 10) {
        return Err(err("comments.too_many_links", "too many links"));
    }
    // 近 1 小时的内容指纹
    let fp = logic::simhash(plain);
    let key = format!("fp:{uid}");
    let list = redis::call(&redis::Command {
        name: "LRANGE".into(),
        args: vec![key.as_bytes().to_vec(), b"0".to_vec(), b"19".to_vec()],
    })?;
    if let redis::RedisReply::Array(items) = list {
        for it in items {
            if let redis::RedisScalar::Bulk(b) = it
                && let Ok(prev) = String::from_utf8_lossy(&b).parse::<u64>()
                && fp != 0
                && logic::near_duplicate(fp, prev)
            {
                return Err(err(
                    "comments.duplicate",
                    "you posted almost the same text recently",
                ));
            }
        }
    }
    redis::pipeline(
        &[
            redis::Command {
                name: "LPUSH".into(),
                args: vec![key.as_bytes().to_vec(), fp.to_string().into_bytes()],
            },
            redis::Command {
                name: "LTRIM".into(),
                args: vec![key.as_bytes().to_vec(), b"0".to_vec(), b"19".to_vec()],
            },
            redis::Command {
                name: "EXPIRE".into(),
                args: vec![key.as_bytes().to_vec(), b"3600".to_vec()],
            },
        ],
        true,
    )?;
    Ok(())
}

/// 发送站内通知（notify 未安装或失败时忽略）
#[allow(clippy::too_many_arguments)]
fn notify(
    user: i64,
    key: &str,
    category: &str,
    args: Value,
    title: &str,
    body: &str,
    link: &str,
    idem: &str,
) {
    let _ = services::call(
        "ctx.notify",
        "send",
        &json!({ "user_id": user, "template_key": key, "category": category, "args": args, "title": title, "body": body, "link": link, "idempotency_key": idem }).to_string(),
    );
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

struct NewComment<'a> {
    author: i64,
    target: &'a str,
    body: String,
    markup: &'a str,
    reply_to: Option<i64>,
    quote_of: Option<i64>,
    idem: Option<String>,
    /// 代发（服务调用）不做防刷
    antispam: bool,
}

fn create(c: NewComment<'_>) -> R<Value> {
    if let Some(k) = &c.idem {
        let r = sql::query(
            "SELECT id, floor FROM comments WHERE author_id = $1 AND idempotency_key = $2",
            &[SqlValue::Int(c.author), SqlValue::Text(k.clone())],
        )?;
        if let Some(row) = r.rows.first() {
            return Ok(json!({ "id": int(&row[0]), "floor": int(&row[1]) }));
        }
    }
    if !check_target(c.target, Some(c.author))? {
        return Err(err(
            "comments.target_locked",
            "this target does not accept comments",
        ));
    }
    let markup = pt_markup::Markup::parse(c.markup)
        .ok_or_else(|| err("kernel.validation_failed", "markup must be markdown or bbcode"))?;
    let mut body = c.body.trim().to_owned();
    let max_len = usize::try_from(setting("max_length", 10_000)).unwrap_or(10_000);
    if body.is_empty() || body.chars().count() > max_len {
        return Err(err("comments.too_long", "comment is empty or too long"));
    }
    if let Some(r) = c.reply_to {
        let p = sql::query(
            "SELECT 1 FROM comments WHERE id = $1 AND target = $2 AND reply_to IS NULL AND state = 'visible'",
            &[SqlValue::Int(r), SqlValue::Text(c.target.to_owned())],
        )?;
        if p.rows.is_empty() {
            return Err(err(
                "comments.reply_invalid",
                "can only reply to a visible top-level comment of the same target",
            ));
        }
    }
    if let Some(q) = c.quote_of {
        let p = sql::query(
            "SELECT c.source, u.username FROM comments c LEFT JOIN identity.v_user_public u ON u.id = c.author_id WHERE c.id = $1 AND c.target = $2 AND c.state = 'visible'",
            &[SqlValue::Int(q), SqlValue::Text(c.target.to_owned())],
        )?;
        let row = p
            .rows
            .first()
            .ok_or_else(|| err("comments.reply_invalid", "quoted comment not found"))?;
        body = format!(
            "{}{body}",
            logic::quote_prefix(
                c.markup,
                &text(&row[1]).unwrap_or_else(|| "?".into()),
                &text(&row[0]).unwrap_or_default()
            )
        );
    }
    let mut r = render(&body, markup);
    let hook = json!({ "version": 1, "author_id": c.author, "target": c.target, "body": body, "markup": c.markup, "reply_to": c.reply_to, "mentions": r.mentions });
    match hook_runner::run("comment.before_create", &hook.to_string())? {
        hook_runner::HookOutcome::Denied(d) => return Err(err(&d.code, &d.message)),
        hook_runner::HookOutcome::Allowed(out) => {
            let v: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
            if let Some(b) = v["body"].as_str().filter(|b| *b != body) {
                body = b.to_owned();
                r = render(&body, markup);
            }
        }
    }
    if c.antispam {
        antispam(c.author, &r.text, r.links)?;
    }
    let floor = sql::query(
        "INSERT INTO target_counters (target, next_floor, comment_count) VALUES ($1, 2, 1)
         ON CONFLICT (target) DO UPDATE SET next_floor = target_counters.next_floor + 1, comment_count = target_counters.comment_count + 1
         RETURNING next_floor - 1",
        &[SqlValue::Text(c.target.to_owned())],
    )?;
    let floor = floor.rows.first().and_then(|row| int(&row[0])).unwrap_or(1);
    let opt = |v: Option<i64>| v.map_or(SqlValue::Null, SqlValue::Int);
    let ins = sql::query(
        "INSERT INTO comments (target, floor, author_id, reply_to, markup, source, rendered, plain, render_version, idempotency_key)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING id",
        &[
            SqlValue::Text(c.target.to_owned()),
            SqlValue::Int(floor),
            SqlValue::Int(c.author),
            opt(c.reply_to),
            SqlValue::Text(c.markup.to_owned()),
            SqlValue::Text(body),
            SqlValue::Text(r.html),
            SqlValue::Text(r.text.chars().take(20_000).collect()),
            SqlValue::Int(i64::from(pt_markup::RENDER_VERSION)),
            c.idem.map_or(SqlValue::Null, SqlValue::Text),
        ],
    )?;
    let id = ins.rows.first().and_then(|row| int(&row[0])).unwrap_or(0);
    let mentions: Vec<u64> = r
        .mentions
        .into_iter()
        .filter(|m| i64::try_from(*m).ok() != Some(c.author))
        .take(10)
        .collect();
    // 通知：@提醒与回复（不通知自己）
    let (kind, tid) = logic::parse_target(c.target).unwrap_or(("torrent", 0));
    let link = format!("/{}s/{tid}#comment-{id}", kind);
    let author_name = sql::query(
        "SELECT username FROM identity.v_user_public WHERE id = $1",
        &[SqlValue::Int(c.author)],
    )?
    .rows
    .first()
    .and_then(|r| text(&r[0]))
    .unwrap_or_default();
    let excerpt: String = r.text.chars().take(100).collect();
    for m in &mentions {
        let uid = i64::try_from(*m).unwrap_or(0);
        notify(
            uid,
            "comments.mentioned",
            "social",
            json!({ "user": author_name, "excerpt": excerpt }),
            &format!("{author_name} 在评论中提到了你"),
            &excerpt,
            &link,
            &format!("{id}:m:{uid}"),
        );
    }
    if let Some(parent) = c.reply_to {
        let p = sql::query(
            "SELECT author_id FROM comments WHERE id = $1",
            &[SqlValue::Int(parent)],
        )?;
        if let Some(pa) = p
            .rows
            .first()
            .and_then(|row| int(&row[0]))
            .filter(|pa| *pa != c.author && !mentions.contains(&u64::try_from(*pa).unwrap_or(0)))
        {
            notify(
                pa,
                "comments.replied",
                "social",
                json!({ "user": author_name, "excerpt": excerpt }),
                &format!("{author_name} 回复了你的评论"),
                &excerpt,
                &link,
                &format!("{id}:r"),
            );
        }
    }
    publish(
        "comment.created",
        c.target,
        json!({ "comment_id": id, "target": c.target, "author_id": c.author, "reply_to": c.reply_to, "mentions": mentions, "length": r.text.chars().count() }),
    )?;
    Ok(json!({ "id": id, "floor": floor }))
}

fn authors(ids: &[i64]) -> R<std::collections::HashMap<i64, String>> {
    let mut out = std::collections::HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    let r = sql::query(
        "SELECT id, username FROM identity.v_user_public WHERE id IN (SELECT jsonb_array_elements_text($1::jsonb)::bigint)",
        &[SqlValue::Text(json!(ids).to_string())],
    )?;
    for row in r.rows {
        if let (Some(i), Some(n)) = (int(&row[0]), text(&row[1])) {
            out.insert(i, n);
        }
    }
    Ok(out)
}

const COLS: &str = "id, floor, author_id, reply_to, rendered, state, thanks_count, (extract(epoch FROM created_at) * 1000)::bigint,
    (extract(epoch FROM edited_at) * 1000)::bigint";

fn row_json(
    row: &[SqlValue],
    names: &std::collections::HashMap<i64, String>,
    viewer: Option<i64>,
    thanked: &[i64],
    is_mod: bool,
) -> Value {
    let id = int(&row[0]).unwrap_or(0);
    let author = int(&row[2]);
    let state = text(&row[5]).unwrap_or_default();
    let deleted = state != "visible";
    let window = setting("edit_window_minutes", 60) * 60_000;
    let created = int(&row[7]).unwrap_or(0);
    let now = pt::plugin::time::now();
    json!({
        "id": id, "floor": int(&row[1]), "reply_to": int(&row[3]),
        "author": author.map(|a| json!({ "id": a, "username": names.get(&a) })),
        // 已删除：只给占位（版主仍可看到内容）
        "html": if deleted && !is_mod { Value::Null } else { json!(text(&row[4])) },
        "deleted": deleted, "state": state, "thanks": int(&row[6]).unwrap_or(0), "thanked_by_me": thanked.contains(&id),
        "created_at": created, "edited_at": int(&row[8]),
        "can_edit": !deleted && (is_mod || (viewer.is_some() && viewer == author && now - created <= window)),
        "can_delete": !deleted && (is_mod || (viewer.is_some() && viewer == author)),
    })
}

/// 列表：楼层游标分页，回复按父评论聚合（每条最多 5 条）
fn list(target: &str, viewer: Option<i64>, cursor: Option<i64>, limit: i64) -> R<Value> {
    check_target(target, viewer)?;
    let is_mod = viewer.is_some_and(moderator);
    let r = sql::query(
        &format!(
            "SELECT {COLS} FROM comments WHERE target = $1 AND reply_to IS NULL AND floor > $2 AND state <> 'purged' ORDER BY floor LIMIT $3"
        ),
        &[
            SqlValue::Text(target.to_owned()),
            SqlValue::Int(cursor.unwrap_or(0)),
            SqlValue::Int(limit + 1),
        ],
    )?;
    let mut rows = r.rows;
    let more = rows.len() as i64 > limit;
    rows.truncate(usize::try_from(limit).unwrap_or(50));
    let ids: Vec<i64> = rows.iter().filter_map(|row| int(&row[0])).collect();
    let replies = if ids.is_empty() {
        vec![]
    } else {
        sql::query(
            &format!(
                "SELECT * FROM (SELECT {COLS}, row_number() OVER (PARTITION BY reply_to ORDER BY floor) AS rn FROM comments
                  WHERE reply_to IN (SELECT jsonb_array_elements_text($1::jsonb)::bigint) AND state <> 'purged') x WHERE rn <= 5 ORDER BY floor"
            ),
            &[SqlValue::Text(json!(ids).to_string())],
        )?
        .rows
    };
    let counts = if ids.is_empty() {
        vec![]
    } else {
        sql::query(
            "SELECT reply_to, count(*) FROM comments WHERE reply_to IN (SELECT jsonb_array_elements_text($1::jsonb)::bigint) AND state = 'visible' GROUP BY reply_to",
            &[SqlValue::Text(json!(ids).to_string())],
        )?
        .rows
    };
    let all_ids: Vec<i64> = rows
        .iter()
        .chain(replies.iter())
        .filter_map(|row| int(&row[0]))
        .collect();
    let thanked: Vec<i64> = match viewer {
        Some(v) if !all_ids.is_empty() => sql::query(
            "SELECT comment_id FROM thanks WHERE user_id = $1 AND comment_id IN (SELECT jsonb_array_elements_text($2::jsonb)::bigint)",
            &[SqlValue::Int(v), SqlValue::Text(json!(all_ids).to_string())],
        )?
        .rows
        .iter()
        .filter_map(|row| int(&row[0]))
        .collect(),
        _ => vec![],
    };
    let author_ids: Vec<i64> = rows
        .iter()
        .chain(replies.iter())
        .filter_map(|row| int(&row[2]))
        .collect();
    let names = authors(&author_ids)?;
    let items: Vec<Value> = rows
        .iter()
        .map(|row| {
            let id = int(&row[0]);
            let mut v = row_json(row, &names, viewer, &thanked, is_mod);
            v["replies"] = Value::Array(
                replies
                    .iter()
                    .filter(|x| int(&x[3]) == id)
                    .map(|x| row_json(x, &names, viewer, &thanked, is_mod))
                    .collect(),
            );
            v["reply_count"] = json!(
                counts
                    .iter()
                    .find(|c| int(&c[0]) == id)
                    .and_then(|c| int(&c[1]))
                    .unwrap_or(0)
            );
            v
        })
        .collect();
    let next = if more {
        items
            .last()
            .and_then(|v| v["floor"].as_i64())
            .map(|f| f.to_string())
    } else {
        None
    };
    let total = sql::query(
        "SELECT comment_count FROM target_counters WHERE target = $1",
        &[SqlValue::Text(target.to_owned())],
    )?;
    Ok(
        json!({ "items": items, "next_cursor": next, "total": total.rows.first().and_then(|r| int(&r[0])).unwrap_or(0) }),
    )
}

fn load(id: i64) -> R<(String, Option<i64>, String, String, i64, String)> {
    let r = sql::query(
        "SELECT target, author_id, state, markup, (extract(epoch FROM created_at) * 1000)::bigint, source FROM comments WHERE id = $1",
        &[SqlValue::Int(id)],
    )?;
    let row = r
        .rows
        .first()
        .ok_or_else(|| err("comments.not_found", "comment not found"))?;
    Ok((
        text(&row[0]).unwrap_or_default(),
        int(&row[1]),
        text(&row[2]).unwrap_or_default(),
        text(&row[3]).unwrap_or_default(),
        int(&row[4]).unwrap_or(0),
        text(&row[5]).unwrap_or_default(),
    ))
}

fn delete(id: i64, by: Option<i64>, reason: &str, as_mod: bool) -> R<()> {
    let (target, author, state, ..) = load(id)?;
    if state != "visible" {
        return Err(err("comments.not_found", "comment not found"));
    }
    let by_mod = as_mod || by.is_some_and(moderator);
    if !by_mod && (by.is_none() || by != author) {
        return Err(err("kernel.forbidden", "not allowed to delete this comment"));
    }
    let new_state = if by_mod && by != author {
        "deleted_by_moderator"
    } else {
        "deleted_by_author"
    };
    sql::execute(
        "UPDATE comments SET state = $2, deleted_reason = $3 WHERE id = $1",
        &[
            SqlValue::Int(id),
            SqlValue::Text(new_state.into()),
            SqlValue::Text(reason.chars().take(200).collect()),
        ],
    )?;
    sql::execute(
        "UPDATE target_counters SET comment_count = greatest(comment_count - 1, 0) WHERE target = $1",
        &[SqlValue::Text(target.clone())],
    )?;
    publish(
        "comment.deleted",
        &target,
        json!({ "comment_id": id, "target": target, "by": by, "reason": reason, "by_moderator": new_state == "deleted_by_moderator" }),
    )
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

fn param<'a>(req: &'a http_handler::Request, k: &str) -> Option<&'a str> {
    req.params.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

fn query<'a>(req: &'a http_handler::Request, k: &str) -> Option<&'a str> {
    req.query.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

fn id_param(req: &http_handler::Request) -> R<i64> {
    param(req, "id")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| err("kernel.validation_failed", "bad id"))
}

fn header<'a>(req: &'a http_handler::Request, k: &str) -> Option<&'a str> {
    req.headers
        .iter()
        .find(|h| h.name.eq_ignore_ascii_case(k))
        .map(|h| h.value.as_str())
}

fn body(req: &http_handler::Request) -> Value {
    serde_json::from_slice(&req.body).unwrap_or(Value::Null)
}

impl http_handler::Guest for Comments {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let viewer = me().ok();
        match req.route_id.as_str() {
            "list" => {
                let target = param(&req, "target").unwrap_or("");
                let limit = query(&req, "limit")
                    .and_then(|l| l.parse().ok())
                    .unwrap_or(30i64)
                    .clamp(1, 100);
                Ok(resp(
                    200,
                    &list(
                        target,
                        viewer,
                        query(&req, "cursor").and_then(|c| c.parse().ok()),
                        limit,
                    )?,
                ))
            }
            "create" => {
                let uid = me()?;
                let b = body(&req);
                let out = create(NewComment {
                    author: uid,
                    target: param(&req, "target").unwrap_or(""),
                    body: b["body"].as_str().unwrap_or("").to_owned(),
                    markup: b["markup"].as_str().unwrap_or("markdown"),
                    reply_to: b["reply_to"].as_i64(),
                    quote_of: b["quote_of"].as_i64(),
                    idem: header(&req, "idempotency-key").map(|k| k.chars().take(128).collect()),
                    antispam: true,
                })?;
                Ok(resp(201, &out))
            }
            "get" => {
                let id = id_param(&req)?;
                let (target, author, state, markup, _, source) = load(id)?;
                check_target(&target, viewer)?;
                let is_mod = viewer.is_some_and(moderator);
                if state != "visible" && !is_mod {
                    return Err(err("comments.not_found", "comment not found"));
                }
                let r = sql::query(
                    &format!("SELECT {COLS} FROM comments WHERE id = $1"),
                    &[SqlValue::Int(id)],
                )?;
                let names = authors(&author.into_iter().collect::<Vec<_>>())?;
                let mut v = r
                    .rows
                    .first()
                    .map(|row| row_json(row, &names, viewer, &[], is_mod))
                    .unwrap_or(Value::Null);
                v["target"] = json!(target);
                // 原文：作者本人（编辑用）或版主
                if query(&req, "raw") == Some("1") && (is_mod || (viewer.is_some() && viewer == author)) {
                    v["source"] = json!(source);
                    v["markup"] = json!(markup);
                }
                Ok(resp(200, &v))
            }
            "update" => {
                let uid = me()?;
                let id = id_param(&req)?;
                let (target, author, state, markup, created, source) = load(id)?;
                let is_mod = moderator(uid);
                if state != "visible" {
                    return Err(err("comments.not_found", "comment not found"));
                }
                if !is_mod {
                    if author != Some(uid) {
                        return Err(err("kernel.forbidden", "not allowed to edit this comment"));
                    }
                    if pt::plugin::time::now() - created > setting("edit_window_minutes", 60) * 60_000 {
                        return Err(err("comments.edit_window_expired", "the edit window has passed"));
                    }
                }
                let b = body(&req);
                let new_body = b["body"].as_str().unwrap_or("").trim().to_owned();
                if new_body.is_empty()
                    || new_body.chars().count()
                        > usize::try_from(setting("max_length", 10_000)).unwrap_or(10_000)
                {
                    return Err(err("comments.too_long", "comment is empty or too long"));
                }
                let m = pt_markup::Markup::parse(&markup).unwrap_or(pt_markup::Markup::Markdown);
                let r = render(&new_body, m);
                sql::execute(
                    "INSERT INTO edits (comment_id, editor_id, old_source) VALUES ($1, $2, $3)",
                    &[SqlValue::Int(id), SqlValue::Int(uid), SqlValue::Text(source)],
                )?;
                sql::execute(
                    "UPDATE comments SET source = $2, rendered = $3, plain = $4, render_version = $5, edited_at = now() WHERE id = $1",
                    &[
                        SqlValue::Int(id),
                        SqlValue::Text(new_body),
                        SqlValue::Text(r.html),
                        SqlValue::Text(r.text),
                        SqlValue::Int(i64::from(pt_markup::RENDER_VERSION)),
                    ],
                )?;
                publish(
                    "comment.updated",
                    &target,
                    json!({ "comment_id": id, "target": target, "editor_id": uid }),
                )?;
                Ok(http_handler::Response {
                    status: 204,
                    headers: vec![],
                    body: vec![],
                })
            }
            "delete" => {
                let uid = me()?;
                delete(
                    id_param(&req)?,
                    Some(uid),
                    query(&req, "reason").unwrap_or(""),
                    false,
                )?;
                Ok(http_handler::Response {
                    status: 204,
                    headers: vec![],
                    body: vec![],
                })
            }
            "thanks" => {
                let uid = me()?;
                let id = id_param(&req)?;
                let (target, author, state, ..) = load(id)?;
                check_target(&target, Some(uid))?;
                if state != "visible" || author == Some(uid) {
                    return Err(err("kernel.validation_failed", "cannot thank this comment"));
                }
                let n = sql::execute(
                    "INSERT INTO thanks (comment_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                    &[SqlValue::Int(id), SqlValue::Int(uid)],
                )?;
                if n > 0 {
                    sql::execute(
                        "UPDATE comments SET thanks_count = thanks_count + 1 WHERE id = $1",
                        &[SqlValue::Int(id)],
                    )?;
                }
                Ok(resp(200, &json!({ "thanked": true })))
            }
            "preview" => {
                let b = body(&req);
                let m = pt_markup::Markup::parse(b["markup"].as_str().unwrap_or("markdown"))
                    .unwrap_or(pt_markup::Markup::Markdown);
                Ok(resp(
                    200,
                    &json!({ "html": render(b["body"].as_str().unwrap_or(""), m).html }),
                ))
            }
            "edits" => {
                let id = id_param(&req)?;
                let r = sql::query(
                    "SELECT editor_id, old_source, (extract(epoch FROM edited_at) * 1000)::bigint FROM edits WHERE comment_id = $1 ORDER BY id DESC",
                    &[SqlValue::Int(id)],
                )?;
                let items: Vec<Value> = r.rows.iter().map(|row| json!({ "editor_id": int(&row[0]), "old_source": text(&row[1]), "edited_at": int(&row[2]) })).collect();
                Ok(resp(200, &json!({ "items": items })))
            }
            "user_comments" => {
                let uid = id_param(&req)?;
                let r = sql::query(
                    "SELECT id, target, floor, left(plain, 200), (extract(epoch FROM created_at) * 1000)::bigint FROM comments
                      WHERE author_id = $1 AND state = 'visible' ORDER BY id DESC LIMIT 50",
                    &[SqlValue::Int(uid)],
                )?;
                // 只列出查看者可见的目标
                let items: Vec<Value> = r
                    .rows
                    .iter()
                    .filter(|row| text(&row[1]).is_some_and(|t| check_target(&t, viewer).is_ok()))
                    .map(|row| json!({ "id": int(&row[0]), "target": text(&row[1]), "floor": int(&row[2]), "excerpt": text(&row[3]), "created_at": int(&row[4]) }))
                    .collect();
                Ok(resp(200, &json!({ "items": items })))
            }
            _ => Err(err("kernel.not_found", "no such route")),
        }
    }
}

// ---------------- ctx.comments（LLD §2.1，JSON 分派） ----------------

fn scope(s: &str) -> bool {
    invocation::current().caller_scopes.iter().any(|x| x == s)
}

impl service_provider::Guest for Comments {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        let out = match method.as_str() {
            "count" => {
                let r = sql::query(
                    "SELECT comment_count FROM target_counters WHERE target = $1",
                    &[SqlValue::Text(a["target"].as_str().unwrap_or("").to_owned())],
                )?;
                json!(r.rows.first().and_then(|row| int(&row[0])).unwrap_or(0))
            }
            "count_many" => {
                let targets: Vec<String> = a["targets"]
                    .as_array()
                    .map(|x| x.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                    .unwrap_or_default();
                let r = sql::query(
                    "SELECT target, comment_count FROM target_counters WHERE target IN (SELECT jsonb_array_elements_text($1::jsonb))",
                    &[SqlValue::Text(json!(targets).to_string())],
                )?;
                let m: std::collections::HashMap<String, i64> = r
                    .rows
                    .iter()
                    .filter_map(|row| Some((text(&row[0])?, int(&row[1])?)))
                    .collect();
                json!(
                    targets
                        .iter()
                        .map(|t| m.get(t).copied().unwrap_or(0))
                        .collect::<Vec<_>>()
                )
            }
            // 是否存在（举报、审核等定位目标；不含正文）
            "exists" => {
                let id = a["id"]
                    .as_i64()
                    .ok_or_else(|| err("kernel.validation_failed", "id"))?;
                let r = sql::query(
                    "SELECT target, author_id, state, left(plain, 200) FROM comments WHERE id = $1",
                    &[SqlValue::Int(id)],
                )?;
                match r.rows.first() {
                    Some(row) => {
                        json!({ "id": id, "target": text(&row[0]), "author_id": int(&row[1]), "state": text(&row[2]), "excerpt": text(&row[3]) })
                    }
                    None => Value::Null,
                }
            }
            // 原文与纯文本（ai-moderation 等，作用域 read:raw）
            "get" => {
                if !scope("read:raw") {
                    return Err(err("kernel.forbidden", "scope read:raw required"));
                }
                let id = a["id"]
                    .as_i64()
                    .ok_or_else(|| err("kernel.validation_failed", "id"))?;
                let r = sql::query(
                    "SELECT target, author_id, state, source, plain, (extract(epoch FROM created_at) * 1000)::bigint FROM comments WHERE id = $1",
                    &[SqlValue::Int(id)],
                )?;
                match r.rows.first() {
                    Some(row) => {
                        json!({ "id": id, "target": text(&row[0]), "author_id": int(&row[1]), "state": text(&row[2]), "source": text(&row[3]),
                                         "plain": text(&row[4]), "created_at": int(&row[5]) })
                    }
                    None => Value::Null,
                }
            }
            "create" => {
                if !scope("write") {
                    return Err(err("kernel.forbidden", "scope write required"));
                }
                create(NewComment {
                    author: a["author_id"]
                        .as_i64()
                        .ok_or_else(|| err("kernel.validation_failed", "author_id"))?,
                    target: a["target"].as_str().unwrap_or(""),
                    body: a["body"].as_str().unwrap_or("").to_owned(),
                    markup: a["markup"].as_str().unwrap_or("markdown"),
                    reply_to: a["reply_to"].as_i64(),
                    quote_of: None,
                    idem: a["idempotency_key"].as_str().map(str::to_owned),
                    antispam: false,
                })?
            }
            "delete" => {
                if !scope("write") {
                    return Err(err("kernel.forbidden", "scope write required"));
                }
                let id = a["id"]
                    .as_i64()
                    .ok_or_else(|| err("kernel.validation_failed", "id"))?;
                let by = invocation::current()
                    .actor
                    .user_id
                    .and_then(|u| i64::try_from(u).ok());
                delete(id, by, a["reason"].as_str().unwrap_or(""), true)?;
                json!({ "deleted": true })
            }
            other => {
                return Err(err(
                    "kernel.not_found",
                    &format!("ctx.comments has no method `{other}`"),
                ));
            }
        };
        Ok(out.to_string())
    }
}

export!(Comments);
