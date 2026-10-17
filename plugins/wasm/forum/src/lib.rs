//! forum 插件（WASM，docs/plugins/forum/）：论坛。
//!
//! 本版本实现：分区与版块（读 / 回复 / 发主题权限按最低等级或角色，版块版主）、主题与楼层（Markdown / BBCode 渲染、引用、
//! 编辑窗口与编辑历史、删除、感谢）、版主操作（置顶、锁定、全站公告、精华、移动）、投票（单选 / 多选、截止时间）、
//! 已读 / 未读与“全部标为已读”、订阅与汇总通知、首页公告、`forum.before_post` 钩子、防刷、讨论帖服务（`ctx.forum`）。
//! 合并 / 拆分主题、站内搜索贡献在后续迭代实现。

pt_plugin_sdk::world!("forum", http);

use std::collections::{HashMap, HashSet};

use exports::pt::plugin::{http_handler, on_cron, service_provider};
use pt_plugin_sdk::{audit, hook_runner, prelude::*, redis, serde_json, services, sql};

struct Forum;

// ---------------- 设置与通用 ----------------

fn setting(k: &str, d: i64) -> i64 {
    ctx::settings()[k].as_i64().unwrap_or(d)
}

fn publish(event: &str, key: &str, payload: &Value) -> R<()> {
    ctx::publish(event, key, payload)?;
    Ok(())
}

fn audit_write(action: &str, target: String, details: &Value) -> R<()> {
    audit::write(&audit::AuditEntry { action: action.into(), target, details: details.to_string() })
}

fn has(uid: i64, perm: &str) -> bool {
    services::call("ctx.rbac", "check", &json!({ "user": uid, "permission": perm }).to_string()).is_ok_and(|r| r == "true")
}

fn incr(key: &str, ttl: i64) -> R<i64> {
    let n = match redis::call(&redis::Command { name: "INCR".into(), args: vec![key.as_bytes().to_vec()] })? {
        redis::RedisReply::Int(n) => n,
        _ => 0,
    };
    if n == 1 {
        redis::call(&redis::Command { name: "EXPIRE".into(), args: vec![key.as_bytes().to_vec(), ttl.to_string().into_bytes()] })?;
    }
    Ok(n)
}

fn antispam(uid: i64) -> R<()> {
    let limited = || err("forum.rate_limited", "you are posting too fast");
    if incr(&format!("rl:30s:{uid}"), 30)? > setting("rate_per_30s", 3) {
        return Err(limited());
    }
    if incr(&format!("rl:1h:{uid}"), 3600)? > setting("rate_per_hour", 60) {
        return Err(limited());
    }
    Ok(())
}

fn render(body: &str, markup: pt_markup::Markup) -> pt_markup::Rendered {
    let hosts: Vec<String> = ctx::settings()["image_hosts"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    let opt = pt_markup::RenderOptions { image_hosts: &hosts, ..Default::default() };
    pt_markup::render(body, markup, &opt)
}

fn quote_prefix(markup: &str, name: &str, source: &str) -> String {
    let s: String = source.chars().take(500).collect();
    if markup == "bbcode" {
        format!("[quote={name}]{s}[/quote]\n")
    } else {
        let quoted: Vec<String> = s.lines().map(|l| format!("> {l}")).collect();
        format!("> @{name} 写道：\n{}\n\n", quoted.join("\n"))
    }
}

// ---------------- 访问者与权限 ----------------

struct Viewer {
    uid: Option<i64>,
    class: i64,
    roles: HashSet<String>,
    manage: bool,
    read: bool,
    post: bool,
    mod_boards: HashSet<i64>,
}

fn viewer() -> R<Viewer> {
    let Some(uid) = ctx::current_user_opt() else {
        return Ok(Viewer { uid: None, class: -1, roles: HashSet::new(), manage: false, read: false, post: false, mod_boards: HashSet::new() });
    };
    let class = sql::query("SELECT class_id FROM identity.v_user_public WHERE id = $1", &[SqlValue::Int(uid)])?
        .rows
        .first()
        .and_then(|r| r.int(0))
        .unwrap_or(0);
    let roles: HashSet<String> = services::call("ctx.rbac", "roles_of", &json!({ "user": uid }).to_string())
        .ok()
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default()
        .into_iter()
        .collect();
    let mod_boards = sql::query("SELECT board_id FROM board_moderators WHERE user_id = $1", &[SqlValue::Int(uid)])?
        .rows
        .iter()
        .filter_map(|r| r.int(0))
        .collect();
    Ok(Viewer { uid: Some(uid), class, roles, manage: has(uid, "forum.manage"), read: has(uid, "forum.read"), post: has(uid, "forum.post"), mod_boards })
}

impl Viewer {
    fn require(&self) -> R<i64> {
        self.uid.ok_or_else(|| err("kernel.unauthenticated", "login required"))
    }
    fn moderates(&self, board: i64) -> bool {
        self.manage || self.mod_boards.contains(&board)
    }
    /// action：read / reply / create
    fn allowed(&self, board: i64, acl: &Value, action: &str) -> bool {
        if self.moderates(board) {
            return true;
        }
        if !(if action == "read" { self.read } else { self.post }) {
            return false;
        }
        let rule = &acl[action];
        let min_ok = rule["min_class"].as_i64().is_none_or(|m| self.class >= m);
        let role_ok = rule["roles"].as_array().is_some_and(|rs| rs.iter().filter_map(Value::as_str).any(|r| self.roles.contains(r)));
        min_ok || role_ok
    }
}

struct Board {
    id: i64,
    acl: Value,
}

fn board(id: i64) -> R<Option<Board>> {
    let r = sql::query("SELECT id, acl::text FROM boards WHERE id = $1 AND active", &[SqlValue::Int(id)])?;
    Ok(r.rows.first().map(|row| Board {
        id: row.int(0).unwrap_or(0),
        acl: serde_json::from_str(&row.text(1).unwrap_or_default()).unwrap_or(Value::Null),
    }))
}

/// 版块存在且可读（不可读按不存在处理）
fn readable_board(v: &Viewer, id: i64) -> R<Board> {
    match board(id)? {
        Some(b) if v.allowed(b.id, &b.acl, "read") => Ok(b),
        _ => Err(err("forum.board_not_found", "board not found")),
    }
}

struct Topic {
    id: i64,
    board: Board,
    title: String,
    author: Option<i64>,
    locked: bool,
    next_floor: i64,
}

fn topic(v: &Viewer, id: i64) -> R<Topic> {
    let r = sql::query(
        "SELECT board_id, title, author_id, locked, next_floor FROM topics WHERE id = $1 AND state = 'visible'",
        &[SqlValue::Int(id)],
    )?;
    let row = r.rows.first().ok_or_else(|| err("forum.topic_not_found", "topic not found"))?;
    let b = readable_board(v, row.int(0).unwrap_or(0)).map_err(|_| err("forum.topic_not_found", "topic not found"))?;
    Ok(Topic { id, board: b, title: row.text(1).unwrap_or_default(), author: row.int(2), locked: row.bool(3).unwrap_or(false), next_floor: row.int(4).unwrap_or(1) })
}

fn usernames(ids: &[i64]) -> R<HashMap<i64, String>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    Ok(sql::query("SELECT id, username FROM identity.v_user_public WHERE id = ANY($1)", &[SqlValue::IntArray(ids.to_vec())])?
        .rows
        .iter()
        .filter_map(|r| Some((r.int(0)?, r.text(1)?)))
        .collect())
}

// ---------------- 版块 ----------------

fn catchup_at(uid: Option<i64>) -> R<Option<i64>> {
    let Some(uid) = uid else { return Ok(None) };
    Ok(sql::query("SELECT (extract(epoch FROM at) * 1000)::bigint FROM user_catchup WHERE user_id = $1", &[SqlValue::Int(uid)])?
        .rows
        .first()
        .and_then(|r| r.int(0)))
}

fn list_boards(v: &Viewer) -> R<Value> {
    let r = sql::query(
        "SELECT b.id, b.category_id, c.name, b.name, b.description, b.acl::text, b.topic_count, b.post_count,
                (extract(epoch FROM b.last_post_at) * 1000)::bigint, t.id, t.title,
                EXISTS (SELECT 1 FROM topics x LEFT JOIN topic_reads r ON r.topic_id = x.id AND r.user_id = $1
                         WHERE x.board_id = b.id AND x.state = 'visible' AND $1::bigint IS NOT NULL
                           AND x.last_post_at > coalesce((SELECT at FROM user_catchup WHERE user_id = $1), 'epoch')
                           AND (r.topic_id IS NULL OR r.last_read_floor < x.next_floor - 1))
           FROM boards b JOIN categories c ON c.id = b.category_id LEFT JOIN topics t ON t.id = b.last_topic_id AND t.state = 'visible'
          WHERE b.active ORDER BY c.sort, c.id, b.sort, b.id",
        &[v.uid.map_or(SqlValue::Null, SqlValue::Int)],
    )?;
    let mut cats: Vec<Value> = Vec::new();
    for row in &r.rows {
        let id = row.int(0).unwrap_or(0);
        let acl: Value = serde_json::from_str(&row.text(5).unwrap_or_default()).unwrap_or(Value::Null);
        if !v.allowed(id, &acl, "read") {
            continue;
        }
        let cid = row.int(1).unwrap_or(0);
        if cats.last().is_none_or(|c| c["id"] != cid) {
            cats.push(json!({ "id": cid, "name": row.text(2), "boards": [] }));
        }
        if let Some(Value::Array(bs)) = cats.last_mut().map(|c| &mut c["boards"]) {
            bs.push(json!({ "id": id, "name": row.text(3), "description": row.text(4), "topic_count": row.int(6), "post_count": row.int(7),
                            "last_post_at": row.int(8), "last_topic": row.int(9).map(|t| json!({ "id": t, "title": row.text(10) })),
                            "unread": row.bool(11).unwrap_or(false),
                            "can_create": v.allowed(id, &acl, "create") }));
        }
    }
    Ok(json!({ "categories": cats }))
}

fn list_topics(v: &Viewer, board_id: i64, page: i64) -> R<Value> {
    let b = readable_board(v, board_id)?;
    let per = setting("topics_per_page", 30).clamp(10, 100);
    let page = page.max(1);
    let catchup = catchup_at(v.uid)?;
    let r = sql::query(
        "SELECT t.id, t.title, t.author_id, t.post_count, (extract(epoch FROM t.last_post_at) * 1000)::bigint, t.last_poster_id,
                t.sticky, t.announcement, t.featured, t.locked, t.next_floor - 1, r.last_read_floor, EXISTS (SELECT 1 FROM polls p WHERE p.topic_id = t.id),
                (extract(epoch FROM t.created_at) * 1000)::bigint
           FROM topics t LEFT JOIN topic_reads r ON r.topic_id = t.id AND r.user_id = $4
          WHERE t.board_id = $1 AND t.state = 'visible'
          ORDER BY t.sticky DESC, t.last_post_at DESC LIMIT $2 OFFSET $3",
        &[SqlValue::Int(board_id), SqlValue::Int(per), SqlValue::Int((page - 1) * per), v.uid.map_or(SqlValue::Null, SqlValue::Int)],
    )?;
    let ids: Vec<i64> = r.rows.iter().flat_map(|row| [row.int(2), row.int(5)]).flatten().collect();
    let names = usernames(&ids)?;
    let total = sql::query("SELECT topic_count, name FROM boards WHERE id = $1", &[SqlValue::Int(board_id)])?;
    let topics: Vec<Value> = r
        .rows
        .iter()
        .map(|row| {
            let last_floor = row.int(10).unwrap_or(0);
            let last_at = row.int(4).unwrap_or(0);
            let unread = v.uid.is_some() && catchup.is_none_or(|c| last_at > c) && row.int(11).is_none_or(|f| f < last_floor);
            let name = |i: usize| row.int(i).and_then(|u| names.get(&u).cloned());
            json!({ "id": row.int(0), "title": row.text(1), "author": name(2), "author_id": row.int(2), "post_count": row.int(3),
                    "last_post_at": last_at, "last_poster": name(5), "sticky": row.bool(6), "announcement": row.bool(7), "featured": row.bool(8),
                    "locked": row.bool(9), "unread": unread, "poll": row.bool(12), "created_at": row.int(13) })
        })
        .collect();
    let trow = total.rows.first();
    Ok(json!({ "board": { "id": b.id, "name": trow.and_then(|r| r.text(1)), "can_create": v.allowed(b.id, &b.acl, "create"),
                          "moderator": v.moderates(b.id) },
               "topics": topics, "page": page, "per_page": per, "total": trow.and_then(|r| r.int(0)) }))
}

// ---------------- 发帖 ----------------

struct NewPost<'a> {
    author: i64,
    board: i64,
    topic: Option<i64>,
    title: Option<String>,
    body: String,
    markup: &'a str,
    idem: Option<String>,
}

/// 校验、钩子、渲染；返回 (标题, 正文, 渲染结果)
fn prepare(p: &NewPost<'_>) -> R<(Option<String>, String, pt_markup::Rendered, pt_markup::Markup)> {
    let markup = pt_markup::Markup::parse(p.markup).ok_or_else(|| err("kernel.validation_failed", "markup must be markdown or bbcode"))?;
    let mut body = p.body.trim().to_owned();
    let max_len = usize::try_from(setting("max_length", 20_000)).unwrap_or(20_000);
    if body.is_empty() || body.chars().count() > max_len {
        return Err(err("forum.body_invalid", "the post is empty or too long"));
    }
    let mut title = p.title.as_ref().map(|t| t.trim().to_owned());
    if let Some(t) = &title
        && (t.is_empty() || t.chars().count() > 120)
    {
        return Err(err("forum.title_invalid", "the title must be 1-120 characters"));
    }
    let hook = json!({ "version": 1, "author_id": p.author, "forum_id": p.board, "topic_id": p.topic, "title": title, "body": body,
                       "markup": p.markup, "is_topic": p.topic.is_none() });
    match hook_runner::run("forum.before_post", &hook.to_string())? {
        hook_runner::HookOutcome::Denied(d) => return Err(err(&d.code, &d.message)),
        hook_runner::HookOutcome::Allowed(out) => {
            let v: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
            if let Some(b) = v["body"].as_str() {
                body = b.to_owned();
            }
            if title.is_some()
                && let Some(t) = v["title"].as_str()
            {
                title = Some(t.to_owned());
            }
        }
    }
    let r = render(&body, markup);
    Ok((title, body, r, markup))
}

fn insert_post(topic: i64, floor: i64, author: i64, markup: &str, body: &str, r: &pt_markup::Rendered, idem: Option<String>) -> R<i64> {
    sql::query(
        "INSERT INTO posts (topic_id, floor, author_id, markup, source, rendered, plain, render_version, idempotency_key)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id",
        &[
            SqlValue::Int(topic),
            SqlValue::Int(floor),
            SqlValue::Int(author),
            SqlValue::Text(markup.into()),
            SqlValue::Text(body.into()),
            SqlValue::Text(r.html.clone()),
            SqlValue::Text(r.text.chars().take(20_000).collect()),
            SqlValue::Int(i64::from(pt_markup::RENDER_VERSION)),
            idem.map_or(SqlValue::Null, SqlValue::Text),
        ],
    )?
    .rows
    .first()
    .and_then(|row| row.int(0))
    .ok_or_else(|| err("forum.internal", "insert failed"))
}

fn create_topic(p: NewPost<'_>, poll: Option<&Value>, spam_check: bool) -> R<Value> {
    if let Some(k) = &p.idem {
        let r = sql::query("SELECT id FROM topics WHERE author_id = $1 AND idempotency_key = $2", &[SqlValue::Int(p.author), SqlValue::Text(k.clone())])?;
        if let Some(id) = r.rows.first().and_then(|row| row.int(0)) {
            return Ok(json!({ "id": id }));
        }
    }
    let poll = poll.filter(|v| !v.is_null()).map(parse_poll).transpose()?;
    let (title, body, r, _) = prepare(&p)?;
    if spam_check {
        antispam(p.author)?;
    }
    let tid = sql::query(
        "INSERT INTO topics (board_id, title, author_id, last_poster_id, post_count, next_floor, idempotency_key)
         VALUES ($1, $2, $3, $3, 1, 2, $4) RETURNING id",
        &[SqlValue::Int(p.board), SqlValue::Text(title.unwrap_or_default()), SqlValue::Int(p.author), p.idem.clone().map_or(SqlValue::Null, SqlValue::Text)],
    )?
    .rows
    .first()
    .and_then(|row| row.int(0))
    .ok_or_else(|| err("forum.internal", "insert failed"))?;
    let pid = insert_post(tid, 1, p.author, p.markup, &body, &r, None)?;
    sql::execute("UPDATE topics SET last_post_id = $2 WHERE id = $1", &[SqlValue::Int(tid), SqlValue::Int(pid)])?;
    sql::execute(
        "UPDATE boards SET topic_count = topic_count + 1, post_count = post_count + 1, last_topic_id = $2, last_post_at = now() WHERE id = $1",
        &[SqlValue::Int(p.board), SqlValue::Int(tid)],
    )?;
    if let Some((q, opts, max, closes)) = poll {
        let n = opts.len();
        sql::execute(
            "INSERT INTO polls (topic_id, question, options, counts, max_choices, closes_at)
             VALUES ($1, $2, $3, array_fill(0, ARRAY[$4::int]), $5, to_timestamp($6::float8 / 1000))",
            &[
                SqlValue::Int(tid),
                SqlValue::Text(q),
                SqlValue::TextArray(opts),
                SqlValue::Int(i64::try_from(n).unwrap_or(2)),
                SqlValue::Int(max),
                closes.map_or(SqlValue::Null, SqlValue::Int),
            ],
        )?;
    }
    sql::execute("INSERT INTO subscriptions (user_id, topic_id) VALUES ($1, $2) ON CONFLICT DO NOTHING", &[SqlValue::Int(p.author), SqlValue::Int(tid)])?;
    publish("forum.topic_created", &format!("topic:{tid}"), &json!({ "topic_id": tid, "forum_id": p.board, "author_id": p.author }))?;
    publish("forum.post_created", &format!("topic:{tid}"), &json!({ "post_id": pid, "topic_id": tid, "forum_id": p.board, "author_id": p.author, "mentions": r.mentions }))?;
    Ok(json!({ "id": tid, "post_id": pid }))
}

fn parse_poll(v: &Value) -> R<(String, Vec<String>, i64, Option<i64>)> {
    let bad = |m: &str| err("forum.poll_invalid", m);
    let q = v["question"].as_str().map(str::trim).filter(|q| !q.is_empty() && q.chars().count() <= 200).ok_or_else(|| bad("question"))?;
    let opts: Vec<String> = v["options"]
        .as_array()
        .ok_or_else(|| bad("options"))?
        .iter()
        .filter_map(Value::as_str)
        .map(|o| o.trim().chars().take(100).collect::<String>())
        .filter(|o| !o.is_empty())
        .collect();
    if !(2..=20).contains(&opts.len()) {
        return Err(bad("2 to 20 options"));
    }
    let max = v["max_choices"].as_i64().unwrap_or(1).clamp(1, i64::try_from(opts.len()).unwrap_or(1));
    Ok((q.to_owned(), opts, max, v["closes_at"].as_i64()))
}

fn reply(v: &Viewer, tid: i64, b: &Value) -> R<Value> {
    let uid = v.require()?;
    let t = topic(v, tid)?;
    if !v.allowed(t.board.id, &t.board.acl, "reply") {
        return Err(err("forum.permission_denied", "you cannot reply in this board"));
    }
    if t.locked && !v.moderates(t.board.id) {
        return Err(err("forum.topic_locked", "the topic is locked"));
    }
    let idem = b["idempotency_key"].as_str().map(str::to_owned);
    if let Some(k) = &idem {
        let r = sql::query("SELECT id, floor FROM posts WHERE author_id = $1 AND idempotency_key = $2", &[SqlValue::Int(uid), SqlValue::Text(k.clone())])?;
        if let Some(row) = r.rows.first() {
            return Ok(json!({ "id": row.int(0), "floor": row.int(1) }));
        }
    }
    let markup = b["markup"].as_str().unwrap_or("markdown");
    let mut body = b["body"].as_str().unwrap_or("").to_owned();
    if let Some(q) = b["quote_of"].as_i64() {
        let r = sql::query(
            "SELECT p.source, u.username FROM posts p LEFT JOIN identity.v_user_public u ON u.id = p.author_id
              WHERE p.id = $1 AND p.topic_id = $2 AND p.state = 'visible'",
            &[SqlValue::Int(q), SqlValue::Int(tid)],
        )?;
        let row = r.rows.first().ok_or_else(|| err("forum.post_not_found", "quoted post not found"))?;
        body = format!("{}{body}", quote_prefix(markup, &row.text(1).unwrap_or_else(|| "?".into()), &row.text(0).unwrap_or_default()));
    }
    let p = NewPost { author: uid, board: t.board.id, topic: Some(tid), title: None, body, markup, idem: idem.clone() };
    let (_, body, r, _) = prepare(&p)?;
    antispam(uid)?;
    let floor = sql::query(
        "UPDATE topics SET next_floor = next_floor + 1, post_count = post_count + 1, last_post_at = now(), last_poster_id = $2
          WHERE id = $1 RETURNING next_floor - 1",
        &[SqlValue::Int(tid), SqlValue::Int(uid)],
    )?
    .rows
    .first()
    .and_then(|row| row.int(0))
    .unwrap_or(t.next_floor);
    let pid = insert_post(tid, floor, uid, markup, &body, &r, idem)?;
    sql::execute("UPDATE topics SET last_post_id = $2 WHERE id = $1", &[SqlValue::Int(tid), SqlValue::Int(pid)])?;
    sql::execute(
        "UPDATE boards SET post_count = post_count + 1, last_topic_id = $2, last_post_at = now() WHERE id = $1",
        &[SqlValue::Int(t.board.id), SqlValue::Int(tid)],
    )?;
    sql::execute("INSERT INTO subs_dirty (topic_id) VALUES ($1) ON CONFLICT DO NOTHING", &[SqlValue::Int(tid)])?;
    // 自己的回复视为已读
    mark_read(uid, tid, floor)?;
    publish("forum.post_created", &format!("topic:{tid}"), &json!({ "post_id": pid, "topic_id": tid, "forum_id": t.board.id, "author_id": uid, "mentions": r.mentions }))?;
    Ok(json!({ "id": pid, "floor": floor }))
}

fn mark_read(uid: i64, tid: i64, floor: i64) -> R<()> {
    sql::execute(
        "INSERT INTO topic_reads (user_id, topic_id, last_read_floor) VALUES ($1, $2, $3)
         ON CONFLICT (user_id, topic_id) DO UPDATE SET last_read_floor = greatest(topic_reads.last_read_floor, EXCLUDED.last_read_floor), updated_at = now()",
        &[SqlValue::Int(uid), SqlValue::Int(tid), SqlValue::Int(floor)],
    )?;
    Ok(())
}

// ---------------- 阅读 ----------------

fn view_topic(v: &Viewer, tid: i64, page: Option<i64>, post: Option<i64>) -> R<Value> {
    let t = topic(v, tid)?;
    let per = setting("posts_per_page", 25).clamp(10, 100);
    let page = match (page, post) {
        (_, Some(pid)) => {
            let f = sql::query("SELECT floor FROM posts WHERE id = $1 AND topic_id = $2", &[SqlValue::Int(pid), SqlValue::Int(tid)])?
                .rows
                .first()
                .and_then(|r| r.int(0))
                .unwrap_or(1);
            (f - 1) / per + 1
        }
        (Some(p), None) => p.max(1),
        (None, None) => 1,
    };
    let me = v.uid;
    let window = setting("edit_window_minutes", 60);
    let r = sql::query(
        "SELECT p.id, p.floor, p.author_id, u.username, u.class_id, p.rendered, p.state, p.thanks_count, (extract(epoch FROM p.created_at) * 1000)::bigint,
                (extract(epoch FROM p.edited_at) * 1000)::bigint, EXISTS (SELECT 1 FROM thanks th WHERE th.post_id = p.id AND th.user_id = $4),
                p.created_at > now() - make_interval(mins => $5::int), p.markup, p.source
           FROM posts p LEFT JOIN identity.v_user_public u ON u.id = p.author_id
          WHERE p.topic_id = $1 ORDER BY p.floor LIMIT $2 OFFSET $3",
        &[SqlValue::Int(tid), SqlValue::Int(per), SqlValue::Int((page - 1) * per), me.map_or(SqlValue::Null, SqlValue::Int), SqlValue::Int(window)],
    )?;
    let moderator = v.moderates(t.board.id);
    let mut max_floor = 0;
    let posts: Vec<Value> = r
        .rows
        .iter()
        .map(|row| {
            let floor = row.int(1).unwrap_or(0);
            max_floor = max_floor.max(floor);
            let state = row.text(6).unwrap_or_default();
            let mine = me.is_some() && row.int(2) == me;
            let visible = state == "visible";
            let can_edit = visible && (moderator || (mine && row.bool(11).unwrap_or(false)));
            json!({ "id": row.int(0), "floor": floor, "author_id": row.int(2), "author": row.text(3), "author_class": row.int(4),
                    "html": if visible || moderator { row.text(5) } else { None }, "state": state, "thanks": row.int(7),
                    "created_at": row.int(8), "edited_at": row.int(9), "thanked": row.bool(10).unwrap_or(false), "markup": row.text(12),
                    "can_edit": can_edit, "source": if can_edit { row.text(13) } else { None },
                    "can_delete": visible && (moderator || (mine && floor > 1)) })
        })
        .collect();
    if let Some(uid) = me
        && max_floor > 0
    {
        mark_read(uid, tid, max_floor)?;
    }
    let info = sql::query(
        "SELECT t.post_count, t.sticky, t.announcement, t.featured, b.name,
                EXISTS (SELECT 1 FROM subscriptions s WHERE s.topic_id = t.id AND s.user_id = $2)
           FROM topics t JOIN boards b ON b.id = t.board_id WHERE t.id = $1",
        &[SqlValue::Int(tid), me.map_or(SqlValue::Null, SqlValue::Int)],
    )?;
    let i = info.rows.first();
    Ok(json!({
        "topic": { "id": tid, "title": t.title, "board_id": t.board.id, "board": i.and_then(|r| r.text(4)), "author_id": t.author,
                   "locked": t.locked, "sticky": i.and_then(|r| r.bool(1)), "announcement": i.and_then(|r| r.bool(2)), "featured": i.and_then(|r| r.bool(3)),
                   "post_count": i.and_then(|r| r.int(0)), "subscribed": i.and_then(|r| r.bool(5)).unwrap_or(false),
                   "can_reply": me.is_some() && v.allowed(t.board.id, &t.board.acl, "reply") && (!t.locked || moderator), "moderator": moderator },
        "poll": poll_json(tid, me)?,
        "posts": posts, "page": page, "per_page": per,
    }))
}

fn poll_json(tid: i64, me: Option<i64>) -> R<Value> {
    let r = sql::query(
        "SELECT question, options, counts::bigint[], max_choices, (extract(epoch FROM closes_at) * 1000)::bigint, closes_at IS NOT NULL AND closes_at <= now()
           FROM polls WHERE topic_id = $1",
        &[SqlValue::Int(tid)],
    )?;
    let Some(row) = r.rows.first() else { return Ok(Value::Null) };
    let mine: Vec<i64> = match me {
        Some(uid) => sql::query("SELECT option_idx FROM poll_votes WHERE topic_id = $1 AND user_id = $2 ORDER BY option_idx", &[SqlValue::Int(tid), SqlValue::Int(uid)])?
            .rows
            .iter()
            .filter_map(|r| r.int(0))
            .collect(),
        None => Vec::new(),
    };
    let arr = |i: usize| row.value(i).cloned();
    let options = match arr(1) {
        Some(SqlValue::TextArray(a)) => json!(a),
        _ => json!([]),
    };
    let counts = match arr(2) {
        Some(SqlValue::IntArray(a)) => json!(a),
        _ => json!([]),
    };
    Ok(json!({ "question": row.text(0), "options": options, "counts": counts, "max_choices": row.int(3), "closes_at": row.int(4),
               "closed": row.bool(5).unwrap_or(false), "my_votes": mine }))
}

fn vote(v: &Viewer, tid: i64, b: &Value) -> R<Value> {
    let uid = v.require()?;
    let t = topic(v, tid)?;
    if !v.allowed(t.board.id, &t.board.acl, "reply") {
        return Err(err("forum.permission_denied", "you cannot vote in this board"));
    }
    let r = sql::query(
        "SELECT array_length(options, 1), max_choices, closes_at IS NOT NULL AND closes_at <= now() FROM polls WHERE topic_id = $1 FOR UPDATE",
        &[SqlValue::Int(tid)],
    )?;
    let row = r.rows.first().ok_or_else(|| err("forum.poll_invalid", "the topic has no poll"))?;
    if row.bool(2).unwrap_or(false) {
        return Err(err("forum.poll_closed", "the poll is closed"));
    }
    let n = row.int(0).unwrap_or(0);
    let mut picks: Vec<i64> = b["options"].as_array().map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default();
    picks.sort_unstable();
    picks.dedup();
    if picks.is_empty() || picks.len() > usize::try_from(row.int(1).unwrap_or(1)).unwrap_or(1) || picks.iter().any(|p| *p < 0 || *p >= n) {
        return Err(err("forum.poll_invalid", "invalid choice"));
    }
    // 改票：先撤销旧票
    let old: Vec<i64> = sql::query("DELETE FROM poll_votes WHERE topic_id = $1 AND user_id = $2 RETURNING option_idx", &[SqlValue::Int(tid), SqlValue::Int(uid)])?
        .rows
        .iter()
        .filter_map(|r| r.int(0))
        .collect();
    for o in old {
        sql::execute("UPDATE polls SET counts[$2] = counts[$2] - 1 WHERE topic_id = $1", &[SqlValue::Int(tid), SqlValue::Int(o + 1)])?;
    }
    for p in &picks {
        sql::execute("INSERT INTO poll_votes (topic_id, user_id, option_idx) VALUES ($1, $2, $3)", &[SqlValue::Int(tid), SqlValue::Int(uid), SqlValue::Int(*p)])?;
        sql::execute("UPDATE polls SET counts[$2] = counts[$2] + 1 WHERE topic_id = $1", &[SqlValue::Int(tid), SqlValue::Int(p + 1)])?;
    }
    poll_json(tid, Some(uid))
}

// ---------------- 编辑与删除 ----------------

struct PostRow {
    topic: Topic,
    author: Option<i64>,
    floor: i64,
    source: String,
    markup: String,
    fresh: bool,
    state: String,
}

fn post_row(v: &Viewer, pid: i64) -> R<PostRow> {
    let window = setting("edit_window_minutes", 60);
    let r = sql::query(
        "SELECT topic_id, author_id, floor, source, markup, created_at > now() - make_interval(mins => $2::int), state FROM posts WHERE id = $1",
        &[SqlValue::Int(pid), SqlValue::Int(window)],
    )?;
    let row = r.rows.first().ok_or_else(|| err("forum.post_not_found", "post not found"))?;
    Ok(PostRow {
        topic: topic(v, row.int(0).unwrap_or(0))?,
        author: row.int(1),
        floor: row.int(2).unwrap_or(0),
        source: row.text(3).unwrap_or_default(),
        markup: row.text(4).unwrap_or_else(|| "markdown".into()),
        fresh: row.bool(5).unwrap_or(false),
        state: row.text(6).unwrap_or_default(),
    })
}

fn edit_post(v: &Viewer, pid: i64, b: &Value) -> R<Value> {
    let uid = v.require()?;
    let p = post_row(v, pid)?;
    let moderator = v.moderates(p.topic.board.id);
    if p.state != "visible" {
        return Err(err("forum.post_not_found", "post not found"));
    }
    if !(moderator || (p.author == Some(uid) && p.fresh)) {
        return Err(err("forum.edit_window_expired", "you can no longer edit this post"));
    }
    let title = if p.floor == 1 { b["title"].as_str().map(str::to_owned) } else { None };
    let np = NewPost {
        author: p.author.unwrap_or(uid),
        board: p.topic.board.id,
        topic: Some(p.topic.id),
        title: title.clone(),
        body: b["body"].as_str().unwrap_or("").to_owned(),
        markup: &p.markup,
        idem: None,
    };
    let (title, body, r, _) = prepare(&np)?;
    sql::execute("INSERT INTO post_edits (post_id, editor_id, old_source) VALUES ($1, $2, $3)", &[SqlValue::Int(pid), SqlValue::Int(uid), SqlValue::Text(p.source)])?;
    sql::execute(
        "UPDATE posts SET source = $2, rendered = $3, plain = $4, render_version = $5, edited_at = now() WHERE id = $1",
        &[
            SqlValue::Int(pid),
            SqlValue::Text(body),
            SqlValue::Text(r.html),
            SqlValue::Text(r.text.chars().take(20_000).collect()),
            SqlValue::Int(i64::from(pt_markup::RENDER_VERSION)),
        ],
    )?;
    if let Some(t) = title {
        sql::execute("UPDATE topics SET title = $2 WHERE id = $1", &[SqlValue::Int(p.topic.id), SqlValue::Text(t)])?;
    }
    if moderator && p.author != Some(uid) {
        audit_write("forum.post_edit", format!("post:{pid}"), &json!({ "topic_id": p.topic.id }))?;
    }
    publish("forum.post_updated", &format!("topic:{}", p.topic.id), &json!({ "post_id": pid, "topic_id": p.topic.id, "editor_id": uid }))?;
    Ok(json!({ "id": pid }))
}

fn delete_post(v: &Viewer, pid: i64) -> R<()> {
    let uid = v.require()?;
    let p = post_row(v, pid)?;
    let moderator = v.moderates(p.topic.board.id);
    if p.state != "visible" {
        return Err(err("forum.post_not_found", "post not found"));
    }
    if p.floor == 1 {
        // 主楼：删除整个主题（版主）
        if !moderator {
            return Err(err("forum.permission_denied", "only moderators can delete a topic"));
        }
        sql::execute("UPDATE topics SET state = 'deleted' WHERE id = $1", &[SqlValue::Int(p.topic.id)])?;
        sql::execute(
            "UPDATE boards SET topic_count = greatest(topic_count - 1, 0),
                    post_count = greatest(post_count - (SELECT count(*) FROM posts WHERE topic_id = $2 AND state = 'visible'), 0) WHERE id = $1",
            &[SqlValue::Int(p.topic.board.id), SqlValue::Int(p.topic.id)],
        )?;
        audit_write("forum.topic_delete", format!("topic:{}", p.topic.id), &json!({ "title": p.topic.title }))?;
        publish("forum.post_deleted", &format!("topic:{}", p.topic.id), &json!({ "post_id": pid, "topic_id": p.topic.id, "by": uid, "by_moderator": true }))?;
        return Ok(());
    }
    let by_author = p.author == Some(uid);
    if !(moderator || by_author) {
        return Err(err("forum.permission_denied", "you cannot delete this post"));
    }
    let state = if by_author && !moderator { "deleted_by_author" } else { "deleted_by_moderator" };
    sql::execute("UPDATE posts SET state = $2 WHERE id = $1", &[SqlValue::Int(pid), SqlValue::Text(state.into())])?;
    sql::execute("UPDATE topics SET post_count = greatest(post_count - 1, 0) WHERE id = $1", &[SqlValue::Int(p.topic.id)])?;
    sql::execute("UPDATE boards SET post_count = greatest(post_count - 1, 0) WHERE id = $1", &[SqlValue::Int(p.topic.board.id)])?;
    if state == "deleted_by_moderator" {
        audit_write("forum.post_delete", format!("post:{pid}"), &json!({ "topic_id": p.topic.id }))?;
    }
    publish("forum.post_deleted", &format!("topic:{}", p.topic.id), &json!({ "post_id": pid, "topic_id": p.topic.id, "by": uid, "by_moderator": state == "deleted_by_moderator" }))
}

fn history(v: &Viewer, pid: i64) -> R<Value> {
    let p = post_row(v, pid)?;
    if !v.moderates(p.topic.board.id) {
        return Err(err("forum.permission_denied", "moderators only"));
    }
    let r = sql::query(
        "SELECT e.editor_id, u.username, e.old_source, (extract(epoch FROM e.edited_at) * 1000)::bigint
           FROM post_edits e LEFT JOIN identity.v_user_public u ON u.id = e.editor_id WHERE e.post_id = $1 ORDER BY e.id DESC LIMIT 50",
        &[SqlValue::Int(pid)],
    )?;
    Ok(json!({ "items": r.rows.iter().map(|row| json!({ "editor_id": row.int(0), "editor": row.text(1), "source": row.text(2), "edited_at": row.int(3) })).collect::<Vec<_>>() }))
}

fn thank(v: &Viewer, pid: i64, on: bool) -> R<Value> {
    let uid = v.require()?;
    let p = post_row(v, pid)?;
    if p.author == Some(uid) {
        return Err(err("forum.permission_denied", "you cannot thank yourself"));
    }
    let n = if on {
        sql::execute("INSERT INTO thanks (post_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING", &[SqlValue::Int(pid), SqlValue::Int(uid)])?
    } else {
        sql::execute("DELETE FROM thanks WHERE post_id = $1 AND user_id = $2", &[SqlValue::Int(pid), SqlValue::Int(uid)])?
    };
    if n > 0 {
        sql::execute(
            &format!("UPDATE posts SET thanks_count = thanks_count {} 1 WHERE id = $1", if on { "+" } else { "-" }),
            &[SqlValue::Int(pid)],
        )?;
    }
    Ok(json!({ "thanked": on }))
}

// ---------------- 版主操作 ----------------

fn moderate(v: &Viewer, tid: i64, b: &Value) -> R<()> {
    let uid = v.require()?;
    let t = topic(v, tid)?;
    if !v.moderates(t.board.id) {
        return Err(err("forum.permission_denied", "moderators only"));
    }
    let action = b["action"].as_str().unwrap_or("");
    let set = |col: &str, val: bool| sql::execute(&format!("UPDATE topics SET {col} = $2 WHERE id = $1"), &[SqlValue::Int(tid), SqlValue::Boolean(val)]);
    match action {
        "sticky" | "unsticky" => set("sticky", action == "sticky")?,
        "lock" | "unlock" => {
            set("locked", action == "lock")?;
            publish("forum.topic_locked", &format!("topic:{tid}"), &json!({ "topic_id": tid, "locked": action == "lock", "by": uid }))?;
            0
        }
        "feature" | "unfeature" => set("featured", action == "feature")?,
        "announce" | "unannounce" => {
            // 全站公告只有论坛管理员能设置
            if !v.manage {
                return Err(err("forum.permission_denied", "only forum administrators can make announcements"));
            }
            set("announcement", action == "announce")?
        }
        "move" => {
            let to = b["to_board"].as_i64().ok_or_else(|| err("forum.moderation_invalid", "to_board is required"))?;
            let dest = board(to)?.ok_or_else(|| err("forum.moderation_invalid", "target board not found"))?;
            if !v.moderates(dest.id) {
                return Err(err("forum.permission_denied", "you do not moderate the target board"));
            }
            let posts = sql::query("SELECT count(*) FROM posts WHERE topic_id = $1 AND state = 'visible'", &[SqlValue::Int(tid)])?
                .rows
                .first()
                .and_then(|r| r.int(0))
                .unwrap_or(0);
            sql::execute("UPDATE topics SET board_id = $2 WHERE id = $1", &[SqlValue::Int(tid), SqlValue::Int(to)])?;
            sql::execute(
                "UPDATE boards SET topic_count = greatest(topic_count - 1, 0), post_count = greatest(post_count - $2, 0) WHERE id = $1",
                &[SqlValue::Int(t.board.id), SqlValue::Int(posts)],
            )?;
            sql::execute(
                "UPDATE boards SET topic_count = topic_count + 1, post_count = post_count + $2, last_post_at = greatest(last_post_at, now()) WHERE id = $1",
                &[SqlValue::Int(to), SqlValue::Int(posts)],
            )?;
            publish("forum.topic_moved", &format!("topic:{tid}"), &json!({ "topic_id": tid, "from_forum_id": t.board.id, "to_forum_id": to, "by": uid }))?;
            0
        }
        _ => return Err(err("forum.moderation_invalid", "unknown action")),
    };
    audit_write(&format!("forum.{action}"), format!("topic:{tid}"), &json!({ "to_board": b["to_board"] }))
}

fn subscribe(v: &Viewer, tid: i64, on: bool) -> R<()> {
    let uid = v.require()?;
    topic(v, tid)?;
    if on {
        sql::execute("INSERT INTO subscriptions (user_id, topic_id) VALUES ($1, $2) ON CONFLICT DO NOTHING", &[SqlValue::Int(uid), SqlValue::Int(tid)])?;
    } else {
        sql::execute("DELETE FROM subscriptions WHERE user_id = $1 AND topic_id = $2", &[SqlValue::Int(uid), SqlValue::Int(tid)])?;
    }
    Ok(())
}

fn catchup(v: &Viewer) -> R<()> {
    let uid = v.require()?;
    sql::execute(
        "INSERT INTO user_catchup (user_id, at) VALUES ($1, now()) ON CONFLICT (user_id) DO UPDATE SET at = now()",
        &[SqlValue::Int(uid)],
    )?;
    sql::execute("DELETE FROM topic_reads WHERE user_id = $1", &[SqlValue::Int(uid)])?;
    Ok(())
}

fn announcements(v: &Viewer) -> R<Value> {
    let r = sql::query(
        "SELECT t.id, t.title, t.board_id, b.acl::text, (extract(epoch FROM t.created_at) * 1000)::bigint
           FROM topics t JOIN boards b ON b.id = t.board_id
          WHERE t.announcement AND t.state = 'visible' AND b.active ORDER BY t.created_at DESC LIMIT 20",
        &[],
    )?;
    let items: Vec<Value> = r
        .rows
        .iter()
        .filter(|row| v.allowed(row.int(2).unwrap_or(0), &serde_json::from_str(&row.text(3).unwrap_or_default()).unwrap_or(Value::Null), "read"))
        .take(5)
        .map(|row| json!({ "id": row.int(0), "title": row.text(1), "created_at": row.int(4) }))
        .collect();
    Ok(json!({ "items": items }))
}

// ---------------- 后台 ----------------

fn acl_of(b: &Value) -> Value {
    let rule = |k: &str| {
        let r = &b["acl"][k];
        json!({ "min_class": r["min_class"].as_i64(), "roles": r["roles"].as_array().cloned().unwrap_or_default() })
    };
    json!({ "read": rule("read"), "reply": rule("reply"), "create": rule("create") })
}

fn admin(req: &http_handler::Request) -> R<http::Reply> {
    let b: Value = req.body_json::<Value>().unwrap_or(Value::Null);
    let text = |k: &str, max: usize| b[k].as_str().map(|s| s.trim().chars().take(max).collect::<String>()).filter(|s| !s.is_empty());
    let uid = ctx::current_user()?;
    let reply = match req.route() {
        "admin_list" => {
            let cats = sql::query("SELECT id, name, sort FROM categories ORDER BY sort, id", &[])?;
            let boards = sql::query(
                "SELECT b.id, b.category_id, b.name, b.description, b.sort, b.acl::text, b.active,
                        coalesce((SELECT array_to_json(array_agg(m.user_id))::text FROM board_moderators m WHERE m.board_id = b.id), '[]')
                   FROM boards b ORDER BY b.sort, b.id",
                &[],
            )?;
            http::Reply::ok(&json!({
                "categories": cats.rows.iter().map(|r| json!({ "id": r.int(0), "name": r.text(1), "sort": r.int(2) })).collect::<Vec<_>>(),
                "boards": boards.rows.iter().map(|r| json!({ "id": r.int(0), "category_id": r.int(1), "name": r.text(2), "description": r.text(3),
                    "sort": r.int(4), "acl": serde_json::from_str::<Value>(&r.text(5).unwrap_or_default()).unwrap_or(Value::Null), "active": r.bool(6),
                    "moderators": serde_json::from_str::<Value>(&r.text(7).unwrap_or_default()).unwrap_or(json!([])) })).collect::<Vec<_>>(),
            }))
        }
        "admin_create_category" => {
            let name = text("name", 60).ok_or_else(|| err("kernel.validation_failed", "name"))?;
            let id = sql::query("INSERT INTO categories (name, sort) VALUES ($1, $2) RETURNING id", &[SqlValue::Text(name), SqlValue::Int(b["sort"].as_i64().unwrap_or(100))])?
                .rows
                .first()
                .and_then(|r| r.int(0));
            http::Reply::created(&json!({ "id": id }))
        }
        "admin_create_board" => {
            let name = text("name", 60).ok_or_else(|| err("kernel.validation_failed", "name"))?;
            let cat = b["category_id"].as_i64().ok_or_else(|| err("kernel.validation_failed", "category_id"))?;
            let id = sql::query(
                "INSERT INTO boards (category_id, name, description, sort, acl) VALUES ($1, $2, $3, $4, $5::jsonb) RETURNING id",
                &[
                    SqlValue::Int(cat),
                    SqlValue::Text(name),
                    text("description", 300).map_or(SqlValue::Null, SqlValue::Text),
                    SqlValue::Int(b["sort"].as_i64().unwrap_or(100)),
                    SqlValue::Text(acl_of(&b).to_string()),
                ],
            )?
            .rows
            .first()
            .and_then(|r| r.int(0));
            audit_write("forum.board_create", format!("board:{}", id.unwrap_or(0)), &b)?;
            http::Reply::created(&json!({ "id": id }))
        }
        "admin_update_board" => {
            let id = req.param_i64("id")?;
            let n = sql::execute(
                "UPDATE boards SET name = coalesce($2, name), description = coalesce($3, description), sort = coalesce($4, sort),
                        acl = CASE WHEN $5 THEN $6::jsonb ELSE acl END, active = coalesce($7, active), category_id = coalesce($8, category_id)
                  WHERE id = $1",
                &[
                    SqlValue::Int(id),
                    text("name", 60).map_or(SqlValue::Null, SqlValue::Text),
                    text("description", 300).map_or(SqlValue::Null, SqlValue::Text),
                    b["sort"].as_i64().map_or(SqlValue::Null, SqlValue::Int),
                    SqlValue::Boolean(!b["acl"].is_null()),
                    SqlValue::Text(acl_of(&b).to_string()),
                    b["active"].as_bool().map_or(SqlValue::Null, SqlValue::Boolean),
                    b["category_id"].as_i64().map_or(SqlValue::Null, SqlValue::Int),
                ],
            )?;
            if n == 0 {
                return Err(err("forum.board_not_found", "board not found"));
            }
            audit_write("forum.board_update", format!("board:{id}"), &b)?;
            http::Reply::no_content()
        }
        "admin_add_moderator" => {
            let id = req.param_i64("id")?;
            let user = b["user_id"].as_i64().ok_or_else(|| err("kernel.validation_failed", "user_id"))?;
            sql::execute("INSERT INTO board_moderators (board_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING", &[SqlValue::Int(id), SqlValue::Int(user)])?;
            audit_write("forum.moderator_add", format!("board:{id}"), &json!({ "user_id": user }))?;
            http::Reply::no_content()
        }
        "admin_remove_moderator" => {
            let id = req.param_i64("id")?;
            let user = req.param_i64("uid")?;
            sql::execute("DELETE FROM board_moderators WHERE board_id = $1 AND user_id = $2", &[SqlValue::Int(id), SqlValue::Int(user)])?;
            audit_write("forum.moderator_remove", format!("board:{id}"), &json!({ "user_id": user }))?;
            http::Reply::no_content()
        }
        _ => return Err(err("kernel.not_found", "no such route")),
    };
    let _ = uid;
    Ok(reply)
}

// ---------------- 导出 ----------------

/// 订阅汇总：有新回复且超过汇总间隔的主题，通知订阅者（不含最后发帖者，且仍有读权限）
fn digest() -> R<()> {
    let mins = setting("digest_minutes", 10).max(0);
    let dirty = sql::query(
        "DELETE FROM subs_dirty WHERE topic_id IN (SELECT topic_id FROM subs_dirty WHERE since < now() - make_interval(mins => $1::int) LIMIT 200)
         RETURNING topic_id",
        &[SqlValue::Int(mins)],
    )?;
    for tid in dirty.rows.iter().filter_map(|r| r.int(0)) {
        let t = sql::query(
            "SELECT t.title, t.last_poster_id, t.next_floor - 1, t.last_post_id FROM topics t WHERE t.id = $1 AND t.state = 'visible'",
            &[SqlValue::Int(tid)],
        )?;
        let Some(row) = t.rows.first() else { continue };
        let (title, last_poster, last_floor, last_post) = (row.text(0).unwrap_or_default(), row.int(1), row.int(2).unwrap_or(0), row.int(3).unwrap_or(0));
        let subs = sql::query(
            "SELECT s.user_id FROM subscriptions s LEFT JOIN topic_reads r ON r.user_id = s.user_id AND r.topic_id = s.topic_id
              WHERE s.topic_id = $1 AND s.user_id IS DISTINCT FROM $2 AND (r.last_read_floor IS NULL OR r.last_read_floor < $3) LIMIT 1000",
            &[SqlValue::Int(tid), last_poster.map_or(SqlValue::Null, SqlValue::Int), SqlValue::Int(last_floor)],
        )?;
        for uid in subs.rows.iter().filter_map(|r| r.int(0)) {
            let _ = services::call(
                "ctx.notify",
                "send",
                &json!({ "user_id": uid, "template_key": "forum.topic_updated", "category": "social", "args": { "title": title, "topic_id": tid },
                         "title": format!("订阅的主题有新回复 / New replies: {title}"), "body": title,
                         "link": format!("/p/forum/topics/{tid}?post={last_post}"), "idempotency_key": format!("forum:sub:{tid}:{last_post}:{uid}") })
                .to_string(),
            );
        }
    }
    Ok(())
}

impl on_cron::Guest for Forum {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            "subs_digest" => digest(),
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

impl service_provider::Guest for Forum {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        match method.as_str() {
            // 代发主题（如种子讨论帖）：作用域 write，不做防刷与权限检查
            "create_topic" => {
                ctx::require_scope("write")?;
                let p = NewPost {
                    author: a["author_id"].as_i64().ok_or_else(|| err("kernel.validation_failed", "author_id"))?,
                    board: a["board_id"].as_i64().ok_or_else(|| err("kernel.validation_failed", "board_id"))?,
                    topic: None,
                    title: a["title"].as_str().map(str::to_owned),
                    body: a["body"].as_str().unwrap_or("").to_owned(),
                    markup: a["markup"].as_str().unwrap_or("markdown"),
                    idem: a["idempotency_key"].as_str().map(str::to_owned),
                };
                board(p.board)?.ok_or_else(|| err("forum.board_not_found", "board not found"))?;
                let r = create_topic(p, None, false)?;
                if let Some(target) = a["link_target"].as_str() {
                    sql::execute(
                        "INSERT INTO topic_links (target, topic_id) VALUES ($1, $2) ON CONFLICT (target) DO UPDATE SET topic_id = EXCLUDED.topic_id",
                        &[SqlValue::Text(target.chars().take(80).collect()), SqlValue::Int(r["id"].as_i64().unwrap_or(0))],
                    )?;
                }
                Ok(r.to_string())
            }
            "topic_for" => {
                let target = a["target"].as_str().ok_or_else(|| err("kernel.validation_failed", "target"))?;
                let r = sql::query(
                    "SELECT t.id, t.title, t.post_count FROM topic_links l JOIN topics t ON t.id = l.topic_id WHERE l.target = $1 AND t.state = 'visible'",
                    &[SqlValue::Text(target.into())],
                )?;
                Ok(r.rows.first().map_or(Value::Null, |row| json!({ "id": row.int(0), "title": row.text(1), "post_count": row.int(2), "url": format!("/p/forum/topics/{}", row.int(0).unwrap_or(0)) })).to_string())
            }
            other => Err(err("kernel.not_found", &format!("ctx.forum has no method `{other}`"))),
        }
    }
}

impl http_handler::Guest for Forum {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        if req.route().starts_with("admin_") {
            return Ok(admin(&req)?.into());
        }
        let v = viewer()?;
        let qi = |k: &str| req.query(k).and_then(|x| x.parse::<i64>().ok());
        let body = || req.body_json::<Value>().unwrap_or(Value::Null);
        let reply = match req.route() {
            "boards" => http::Reply::ok(&list_boards(&v)?),
            "topics" => http::Reply::ok(&list_topics(&v, req.param_i64("id")?, qi("page").unwrap_or(1))?),
            "create_topic" => {
                let uid = v.require()?;
                let bid = req.param_i64("id")?;
                let bd = readable_board(&v, bid)?;
                if !v.allowed(bid, &bd.acl, "create") {
                    return Err(err("forum.permission_denied", "you cannot start topics in this board"));
                }
                let b = body();
                let idem = req.header("idempotency-key").map(str::to_owned).or_else(|| b["idempotency_key"].as_str().map(str::to_owned));
                let p = NewPost {
                    author: uid,
                    board: bid,
                    topic: None,
                    title: Some(b["title"].as_str().unwrap_or("").to_owned()),
                    body: b["body"].as_str().unwrap_or("").to_owned(),
                    markup: b["markup"].as_str().unwrap_or("markdown"),
                    idem,
                };
                http::Reply::created(&create_topic(p, b.get("poll"), true)?)
            }
            "topic" => http::Reply::ok(&view_topic(&v, req.param_i64("id")?, qi("page"), qi("post"))?),
            "reply" => http::Reply::created(&reply(&v, req.param_i64("id")?, &body())?),
            "edit_post" => http::Reply::ok(&edit_post(&v, req.param_i64("id")?, &body())?),
            "delete_post" => {
                delete_post(&v, req.param_i64("id")?)?;
                http::Reply::no_content()
            }
            "history" => http::Reply::ok(&history(&v, req.param_i64("id")?)?),
            "thank" => http::Reply::ok(&thank(&v, req.param_i64("id")?, true)?),
            "unthank" => http::Reply::ok(&thank(&v, req.param_i64("id")?, false)?),
            "vote" => http::Reply::ok(&vote(&v, req.param_i64("id")?, &body())?),
            "subscribe" => {
                subscribe(&v, req.param_i64("id")?, true)?;
                http::Reply::no_content()
            }
            "unsubscribe" => {
                subscribe(&v, req.param_i64("id")?, false)?;
                http::Reply::no_content()
            }
            "moderate" => {
                moderate(&v, req.param_i64("id")?, &body())?;
                http::Reply::no_content()
            }
            "catchup" => {
                catchup(&v)?;
                http::Reply::no_content()
            }
            "announcements" => http::Reply::ok(&announcements(&v)?),
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(Forum);

#[cfg(test)]
mod tests {
    use super::*;

    fn viewer_with(class: i64, roles: &[&str]) -> Viewer {
        Viewer {
            uid: Some(1),
            class,
            roles: roles.iter().map(|r| (*r).to_owned()).collect(),
            manage: false,
            read: true,
            post: true,
            mod_boards: HashSet::new(),
        }
    }

    #[test]
    fn acl_rules() {
        let acl = json!({ "read": { "min_class": null }, "reply": { "min_class": 2 }, "create": { "min_class": 5, "roles": ["uploader"] } });
        let low = viewer_with(1, &[]);
        assert!(low.allowed(1, &acl, "read"));
        assert!(!low.allowed(1, &acl, "reply"));
        assert!(!low.allowed(1, &acl, "create"));
        let up = viewer_with(1, &["uploader"]);
        assert!(up.allowed(1, &acl, "create"), "a listed role is enough");
        let high = viewer_with(6, &[]);
        assert!(high.allowed(1, &acl, "create"));
        let mut m = viewer_with(0, &[]);
        m.mod_boards.insert(1);
        assert!(m.allowed(1, &acl, "create") && !m.allowed(2, &acl, "create"), "board moderators bypass their own board's ACL only");
        let mut guest = viewer_with(9, &[]);
        guest.read = false;
        assert!(!guest.allowed(1, &acl, "read"), "forum.read is required");
        assert_eq!(acl_of(&json!({})), json!({ "read": { "min_class": null, "roles": [] }, "reply": { "min_class": null, "roles": [] }, "create": { "min_class": null, "roles": [] } }));
    }
}
