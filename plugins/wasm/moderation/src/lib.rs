//! moderation 插件（WASM，docs/plugins/moderation/）：候选区与审核。
//!
//! 本版本实现：上传初始状态规则（`torrent.before_upload`：按角色、等级、已通过 / 被拒数、账号天数、类型、体积等匹配第一条规则，
//! 只能把状态变严）、审核队列（由 `torrent.created` / `torrent.status_changed` / `ai.upload_reviewed` 维护，优先级 = 等待小时 + AI
//! 结论权重）、认领、决定（通过 / 拒绝经 `ctx.torrents.set_status`，需修改则等待上传者）、决定日志、上传者统计、候选区投票
//! （达到票数即通过 / 拒绝）、敏感词（`comment.before_create`：拦截 / 遮蔽 / 复核）、`ctx.moderation`、事件、MCP 工具。
//! 状态变更钩子（torrent.before_status_change）、搜索可见性钩子、理由模板与规则编辑器在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

wit_bindgen::generate!({
    path: "wit",
    world: "moderation",
    generate_all,
});

pub mod rules;

use exports::pt::plugin::{hooks, http_handler, mcp_tool, on_event, service_provider};
use pt::plugin::{
    config, events, invocation, services,
    sql::{self, SqlValue},
    types::PluginError,
};
use serde_json::{Value, json};

struct Moderation;

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

fn me() -> R<i64> {
    invocation::current()
        .actor
        .user_id
        .and_then(|u| i64::try_from(u).ok())
        .ok_or_else(|| err("kernel.unauthenticated", "login required"))
}

fn svc(service: &str, method: &str, args: &Value) -> R<Value> {
    let out = services::call(service, method, &args.to_string())?;
    Ok(serde_json::from_str(&out).unwrap_or(Value::Null))
}

fn has(uid: i64, perm: &str) -> bool {
    svc("ctx.rbac", "check", &json!({ "user": uid, "permission": perm })).is_ok_and(|v| v == true)
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

// ---------------- 钩子 ----------------

/// 上传初始状态（LLD §1.1）
fn before_upload(input: &Value) -> R<hooks::HookDecision> {
    let s = settings();
    let Some(rules_v) = s
        .get("initial_rules")
        .filter(|r| r.as_array().is_some_and(|a| !a.is_empty()))
    else {
        return Ok(hooks::HookDecision::Allow);
    };
    let uid = input["uploader_id"].as_i64().unwrap_or(0);
    let stats = sql::query(
        "SELECT approved, rejected FROM uploader_stats WHERE user_id = $1",
        &[SqlValue::Int(uid)],
    )?;
    let (ok, bad) = stats
        .rows
        .first()
        .map_or((0, 0), |r| (int(&r[0]).unwrap_or(0), int(&r[1]).unwrap_or(0)));
    let user = sql::query(
        "SELECT class_id, (extract(epoch FROM now() - created_at) / 86400)::bigint FROM identity.v_user_public WHERE id = $1",
        &[SqlValue::Int(uid)],
    )?;
    let (class_id, age) = user
        .rows
        .first()
        .map_or((0, 0), |r| (int(&r[0]).unwrap_or(0), int(&r[1]).unwrap_or(0)));
    let roles = svc("ctx.rbac", "roles_of", &json!({ "user": uid })).unwrap_or(json!([]));
    let facts = json!({
        "roles": roles, "class_id": class_id, "approved_uploads": ok, "rejected_uploads": bad,
        "rejected_ratio": if ok + bad > 0 { bad as f64 / (ok + bad) as f64 } else { 0.0 },
        "account_age_days": age, "type_id": input["type_id"], "size": input["size"], "anonymous": input["anonymous"],
    });
    let Some((_, mut status)) = rules::evaluate(rules_v, &facts) else {
        let d = s["default_status"].as_str().unwrap_or("");
        return Ok(if matches!(d, "pending" | "candidate") {
            hooks::HookDecision::Modify(json!({ "initial_status": d }).to_string())
        } else {
            hooks::HookDecision::Allow
        });
    };
    if status == "candidate" && s["candidate_enabled"].as_bool() != Some(true) {
        status = "pending".into();
    }
    // 只能变严：其他处理者已给出更严的状态时不覆盖
    let current = input["initial_status"].as_str().unwrap_or("approved");
    if rules::strictness(&status) > rules::strictness(current) {
        return Ok(hooks::HookDecision::Modify(
            json!({ "initial_status": status }).to_string(),
        ));
    }
    Ok(hooks::HookDecision::Allow)
}

fn words(scope: &str) -> R<Vec<rules::Word>> {
    let r = sql::query(
        "SELECT pattern, action FROM words WHERE active AND $1 = ANY(scopes)",
        &[SqlValue::Text(scope.into())],
    )?;
    Ok(r.rows
        .iter()
        .filter_map(|row| {
            Some(rules::Word {
                pattern: text(&row[0])?,
                action: text(&row[1])?,
            })
        })
        .collect())
}

/// 敏感词（LLD §1.5）：拦截不回显命中词
fn before_comment(input: &Value) -> R<hooks::HookDecision> {
    let body = input["body"].as_str().unwrap_or("");
    let w = words("comment")?;
    let Some((action, spans)) = rules::find(&w, body) else {
        return Ok(hooks::HookDecision::Allow);
    };
    Ok(match action.as_str() {
        "block" => hooks::HookDecision::Deny(hooks::DenyInfo {
            code: "moderation.blocked_word".into(),
            message: "the text contains words that are not allowed".into(),
            details: Some(json!({ "count": spans.len() }).to_string()),
        }),
        "mask" => hooks::HookDecision::Modify(json!({ "body": rules::mask(body, &spans) }).to_string()),
        _ => {
            sql::execute(
                "INSERT INTO word_hits (scope, author_id, excerpt) VALUES ('comment', $1, $2)",
                &[
                    input["author_id"].as_i64().map_or(SqlValue::Null, SqlValue::Int),
                    SqlValue::Text(body.chars().take(200).collect()),
                ],
            )?;
            hooks::HookDecision::Allow
        }
    })
}

impl hooks::Guest for Moderation {
    fn handle(c: hooks::HookCall) -> R<hooks::HookDecision> {
        let input: Value = serde_json::from_str(&c.input).unwrap_or(Value::Null);
        match c.hook.as_str() {
            "torrent.before_upload" => before_upload(&input),
            "comment.before_create" => before_comment(&input),
            _ => Ok(hooks::HookDecision::Allow),
        }
    }
}

// ---------------- 队列（LLD §1.2） ----------------

fn ai_weight(v: Option<&str>) -> i64 {
    match v {
        Some("fail") => 50,
        Some("warn") => 20,
        _ => 0,
    }
}

impl on_event::Guest for Moderation {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        let id = p["torrent_id"].as_i64().unwrap_or(0);
        let target = format!("torrent:{id}");
        match evt.event_type.as_str() {
            "torrent.created" => {
                let st = p["status"].as_str().unwrap_or("");
                if matches!(st, "pending" | "candidate") {
                    sql::execute(
                        "INSERT INTO queue (target, kind, state, type_id, uploader_id, submitted_at) VALUES ($1, 'torrent', $2, $3, $4, now())
                         ON CONFLICT (target) DO UPDATE SET state = EXCLUDED.state",
                        &[
                            SqlValue::Text(target),
                            SqlValue::Text(st.into()),
                            p["type_id"].as_str().map_or(SqlValue::Null, |t| SqlValue::Text(t.into())),
                            p["uploader_id"].as_i64().map_or(SqlValue::Null, SqlValue::Int),
                        ],
                    )?;
                }
            }
            "torrent.status_changed" => {
                let to = p["to"].as_str().unwrap_or("");
                if matches!(to, "pending" | "candidate") {
                    let t = sql::query(
                        "SELECT type_id, uploader_id FROM torrent.v_torrent_public WHERE id = $1",
                        &[SqlValue::Int(id)],
                    )?;
                    let (ty, up) = t.rows.first().map_or((None, None), |r| (text(&r[0]), int(&r[1])));
                    sql::execute(
                        "INSERT INTO queue (target, kind, state, type_id, uploader_id, submitted_at) VALUES ($1, 'torrent', $2, $3, $4, now())
                         ON CONFLICT (target) DO UPDATE SET state = EXCLUDED.state",
                        &[SqlValue::Text(target), SqlValue::Text(to.into()), ty.map_or(SqlValue::Null, SqlValue::Text), up.map_or(SqlValue::Null, SqlValue::Int)],
                    )?;
                } else {
                    sql::execute("DELETE FROM queue WHERE target = $1", &[SqlValue::Text(target)])?;
                }
                // 上传者统计（规则用）
                if matches!(to, "approved" | "rejected")
                    && matches!(p["from"].as_str(), Some("pending" | "candidate"))
                {
                    let up = sql::query(
                        "SELECT uploader_id FROM torrent.v_torrent_public WHERE id = $1",
                        &[SqlValue::Int(id)],
                    )?;
                    if let Some(uid) = up.rows.first().and_then(|r| int(&r[0])) {
                        let col = if to == "approved" { "approved" } else { "rejected" };
                        sql::execute(
                            &format!(
                                "INSERT INTO uploader_stats (user_id, {col}) VALUES ($1, 1) ON CONFLICT (user_id) DO UPDATE SET {col} = uploader_stats.{col} + 1"
                            ),
                            &[SqlValue::Int(uid)],
                        )?;
                    }
                }
            }
            "torrent.updated" => {
                // 上传者按要求修改后回到待审
                sql::execute(
                    "UPDATE queue SET state = 'pending' WHERE target = $1 AND state = 'awaiting_uploader'",
                    &[SqlValue::Text(target)],
                )?;
            }
            "ai.upload_reviewed" => {
                let n = p["findings"].as_array().map_or(0, Vec::len);
                sql::execute(
                    "UPDATE queue SET ai_verdict = $2, ai_findings = $3 WHERE target = $1",
                    &[
                        SqlValue::Text(target),
                        p["verdict"]
                            .as_str()
                            .map_or(SqlValue::Null, |v| SqlValue::Text(v.into())),
                        SqlValue::Int(i64::try_from(n).unwrap_or(0)),
                    ],
                )?;
            }
            _ => {}
        }
        Ok(())
    }
}

fn queue_list(state: Option<&str>, type_id: Option<&str>, limit: i64) -> R<Value> {
    let r = sql::query(
        "SELECT q.target, q.state, q.type_id, q.uploader_id, (extract(epoch FROM q.submitted_at) * 1000)::bigint, q.ai_verdict, q.ai_findings,
                q.claimed_by, (q.claim_until > now()), t.title, (extract(epoch FROM now() - q.submitted_at) / 3600)::bigint
           FROM queue q LEFT JOIN torrent.v_torrent_public t ON q.kind = 'torrent' AND t.id = split_part(q.target, ':', 2)::bigint
          WHERE ($1::text IS NULL OR q.state = $1) AND ($2::text IS NULL OR q.type_id = $2)
          LIMIT 500",
        &[state.map_or(SqlValue::Null, |s| SqlValue::Text(s.into())), type_id.map_or(SqlValue::Null, |s| SqlValue::Text(s.into()))],
    )?;
    let mut items: Vec<(i64, Value)> = r
        .rows
        .iter()
        .map(|row| {
            let ai = text(&row[5]);
            let hours = int(&row[10]).unwrap_or(0);
            let priority = hours + ai_weight(ai.as_deref());
            let claimed = matches!(row[8], SqlValue::Boolean(true));
            (
                priority,
                json!({ "target": text(&row[0]), "state": text(&row[1]), "type_id": text(&row[2]), "uploader_id": int(&row[3]), "submitted_at": int(&row[4]),
                        "ai_verdict": ai, "ai_findings": int(&row[6]), "claimed_by": if claimed { json!(int(&row[7])) } else { Value::Null },
                        "title": text(&row[9]), "waiting_hours": hours, "priority": priority }),
            )
        })
        .collect();
    items.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1["submitted_at"].as_i64().cmp(&b.1["submitted_at"].as_i64()))
    });
    Ok(
        json!({ "items": items.into_iter().take(usize::try_from(limit).unwrap_or(50)).map(|x| x.1).collect::<Vec<_>>() }),
    )
}

fn claim(target: &str, uid: i64, release: bool) -> R<()> {
    if release {
        sql::execute(
            "UPDATE queue SET claimed_by = NULL, claim_until = NULL WHERE target = $1 AND claimed_by = $2",
            &[SqlValue::Text(target.into()), SqlValue::Int(uid)],
        )?;
        return Ok(());
    }
    let mins = settings()["claim_minutes"].as_i64().unwrap_or(30);
    let n = sql::execute(
        "UPDATE queue SET claimed_by = $2, claim_until = now() + make_interval(mins => $3::int)
          WHERE target = $1 AND (claimed_by IS NULL OR claim_until < now() OR claimed_by = $2)",
        &[
            SqlValue::Text(target.into()),
            SqlValue::Int(uid),
            SqlValue::Int(mins),
        ],
    )?;
    if n == 0 {
        let exists = sql::query(
            "SELECT claimed_by FROM queue WHERE target = $1",
            &[SqlValue::Text(target.into())],
        )?;
        return Err(match exists.rows.first() {
            None => err("moderation.not_in_queue", "target is not in the queue"),
            Some(_) => err(
                "moderation.claimed_by_other",
                "another moderator is reviewing this",
            ),
        });
    }
    Ok(())
}

/// 决定（LLD §1.3）
fn decide(target: &str, uid: Option<i64>, decision: &str, note: &str, actor: &str) -> R<Value> {
    if !matches!(decision, "approve" | "reject" | "needs_edit") {
        return Err(err(
            "kernel.validation_failed",
            "decision must be approve, reject or needs_edit",
        ));
    }
    let q = sql::query(
        "SELECT state, claimed_by, (claim_until > now()) FROM queue WHERE target = $1 FOR UPDATE",
        &[SqlValue::Text(target.into())],
    )?;
    let row = q
        .rows
        .first()
        .ok_or_else(|| err("moderation.not_in_queue", "target is not in the queue"))?;
    let claimed_by_other =
        matches!(row[2], SqlValue::Boolean(true)) && int(&row[1]).is_some_and(|c| Some(c) != uid);
    if claimed_by_other && !uid.is_some_and(|u| has(u, "moderation.manage")) {
        return Err(err(
            "moderation.claimed_by_other",
            "another moderator is reviewing this",
        ));
    }
    let (kind, id) = target
        .split_once(':')
        .ok_or_else(|| err("kernel.validation_failed", "bad target"))?;
    let id: i64 = id
        .parse()
        .map_err(|_| err("kernel.validation_failed", "bad target"))?;
    let snap = if kind == "torrent" {
        let t = sql::query(
            "SELECT title, uploader_id, status FROM torrent.v_torrent_public WHERE id = $1",
            &[SqlValue::Int(id)],
        )?;
        t.rows.first().map_or(
            Value::Null,
            |r| json!({ "title": text(&r[0]), "uploader_id": int(&r[1]), "status_before": text(&r[2]) }),
        )
    } else {
        Value::Null
    };
    if kind == "torrent" && decision != "needs_edit" {
        let to = if decision == "approve" {
            "approved"
        } else {
            "rejected"
        };
        match svc(
            "ctx.torrents",
            "set_status",
            &json!({ "id": id, "status": to, "reason": if note.is_empty() { format!("moderation.{decision}") } else { note.chars().take(500).collect() } }),
        ) {
            Ok(_) => {}
            Err(e) if e.code == "kernel.not_found" || e.code.ends_with(".not_found") => {
                return Err(err("moderation.stale", "the torrent no longer exists"));
            }
            Err(e) => return Err(e),
        }
    }
    sql::execute(
        "INSERT INTO decisions (target, decision, note, moderator_id, actor, snapshot) VALUES ($1, $2, $3, $4, $5, $6::jsonb)",
        &[
            SqlValue::Text(target.into()),
            SqlValue::Text(decision.into()),
            SqlValue::Text(note.chars().take(2000).collect()),
            uid.map_or(SqlValue::Null, SqlValue::Int),
            SqlValue::Text(actor.into()),
            SqlValue::Text(snap.to_string()),
        ],
    )?;
    if decision == "needs_edit" {
        sql::execute(
            "UPDATE queue SET state = 'awaiting_uploader', claimed_by = NULL, claim_until = NULL WHERE target = $1",
            &[SqlValue::Text(target.into())],
        )?;
    } else {
        sql::execute(
            "DELETE FROM queue WHERE target = $1",
            &[SqlValue::Text(target.into())],
        )?;
    }
    publish(
        "moderation.decided",
        target,
        json!({ "target": target, "decision": decision, "moderator_id": uid, "by_ai": false, "note": note }),
    )?;
    // 通知上传者
    if let Some(up) = snap["uploader_id"].as_i64() {
        let title = snap["title"].as_str().unwrap_or("").to_owned();
        let zh = match decision {
            "approve" => format!("你的种子已通过审核：{title}"),
            "reject" => format!("你的种子未通过审核：{title}"),
            _ => format!("你的种子需要修改：{title}"),
        };
        notify(
            up,
            &format!("moderation.decision.{decision}"),
            "content",
            json!({ "title": title, "note": note }),
            &zh,
            note,
            &format!("/torrents/{id}"),
            &format!("{target}:{decision}:{}", pt::plugin::time::now()),
        );
    }
    Ok(json!({ "target": target, "decision": decision }))
}

// ---------------- 候选区（LLD §1.4） ----------------

fn vote(uid: i64, torrent_id: i64, value: i64) -> R<Value> {
    let s = settings();
    if s["candidate_enabled"].as_bool() != Some(true) || !matches!(value, -1 | 1) {
        return Err(err("moderation.vote_not_allowed", "voting is not available"));
    }
    let t = sql::query(
        "SELECT status, uploader_id FROM torrent.v_torrent_public WHERE id = $1",
        &[SqlValue::Int(torrent_id)],
    )?;
    let row = t
        .rows
        .first()
        .ok_or_else(|| err("moderation.vote_not_allowed", "torrent not found"))?;
    if text(&row[0]).as_deref() != Some("candidate") || int(&row[1]) == Some(uid) {
        return Err(err(
            "moderation.vote_not_allowed",
            "you cannot vote on this torrent",
        ));
    }
    let age = sql::query(
        "SELECT (extract(epoch FROM now() - created_at) / 86400)::bigint FROM identity.v_user_public WHERE id = $1",
        &[SqlValue::Int(uid)],
    )?;
    if age.rows.first().and_then(|r| int(&r[0])).unwrap_or(0)
        < s["vote_min_account_days"].as_i64().unwrap_or(7)
    {
        return Err(err(
            "moderation.vote_not_allowed",
            "your account is too new to vote",
        ));
    }
    sql::execute(
        "INSERT INTO candidate_votes (torrent_id, user_id, value) VALUES ($1, $2, $3) ON CONFLICT (torrent_id, user_id) DO UPDATE SET value = EXCLUDED.value",
        &[
            SqlValue::Int(torrent_id),
            SqlValue::Int(uid),
            SqlValue::Int(value),
        ],
    )?;
    let tally = sql::query(
        "SELECT count(*) FILTER (WHERE value = 1), count(*) FILTER (WHERE value = -1) FROM candidate_votes WHERE torrent_id = $1",
        &[SqlValue::Int(torrent_id)],
    )?;
    let (up, down) = tally
        .rows
        .first()
        .map_or((0, 0), |r| (int(&r[0]).unwrap_or(0), int(&r[1]).unwrap_or(0)));
    publish(
        "moderation.candidate_voted",
        &format!("torrent:{torrent_id}"),
        json!({ "torrent_id": torrent_id, "up": up, "down": down }),
    )?;
    let target = format!("torrent:{torrent_id}");
    let resolved = if up - down >= s["approve_votes"].as_i64().unwrap_or(3) {
        decide(
            &target,
            None,
            "approve",
            "moderation.candidate_votes",
            "plugin:moderation",
        )
        .ok()
        .map(|_| "approved")
    } else if down - up >= s["reject_votes"].as_i64().unwrap_or(3) {
        decide(
            &target,
            None,
            "reject",
            "moderation.candidate_votes",
            "plugin:moderation",
        )
        .ok()
        .map(|_| "rejected")
    } else {
        None
    };
    Ok(json!({ "up": up, "down": down, "resolved": resolved }))
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

fn no_content() -> http_handler::Response {
    http_handler::Response {
        status: 204,
        headers: vec![],
        body: vec![],
    }
}

impl http_handler::Guest for Moderation {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let b: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
        let target = param(&req, "target").unwrap_or("").to_owned();
        match req.route_id.as_str() {
            "queue" => Ok(resp(
                200,
                &queue_list(
                    query(&req, "state"),
                    query(&req, "type_id"),
                    query(&req, "limit").and_then(|l| l.parse().ok()).unwrap_or(50),
                )?,
            )),
            "queue_item" => {
                let r = sql::query(
                    "SELECT state, ai_verdict, ai_findings, claimed_by, (claim_until > now()) FROM queue WHERE target = $1",
                    &[SqlValue::Text(target.clone())],
                )?;
                let row = r
                    .rows
                    .first()
                    .ok_or_else(|| err("moderation.not_in_queue", "target is not in the queue"))?;
                let hist = sql::query(
                    "SELECT decision, note, moderator_id, (extract(epoch FROM created_at) * 1000)::bigint FROM decisions WHERE target = $1 ORDER BY id DESC LIMIT 20",
                    &[SqlValue::Text(target.clone())],
                )?;
                Ok(resp(
                    200,
                    &json!({ "target": target, "state": text(&row[0]), "ai_verdict": text(&row[1]), "ai_findings": int(&row[2]),
                             "claimed_by": if matches!(row[4], SqlValue::Boolean(true)) { json!(int(&row[3])) } else { Value::Null },
                             "history": hist.rows.iter().map(|h| json!({ "decision": text(&h[0]), "note": text(&h[1]), "moderator_id": int(&h[2]), "created_at": int(&h[3]) })).collect::<Vec<_>>() }),
                ))
            }
            "claim" => {
                claim(&target, me()?, false)?;
                Ok(no_content())
            }
            "release" => {
                claim(&target, me()?, true)?;
                Ok(no_content())
            }
            "decision" => {
                let uid = me()?;
                Ok(resp(
                    200,
                    &decide(
                        &target,
                        Some(uid),
                        b["decision"].as_str().unwrap_or(""),
                        b["note"].as_str().unwrap_or(""),
                        &format!("user:{uid}"),
                    )?,
                ))
            }
            "history" => {
                let r = sql::query(
                    "SELECT target, decision, note, moderator_id, actor, snapshot::text, (extract(epoch FROM created_at) * 1000)::bigint FROM decisions
                      WHERE ($1::text IS NULL OR target = $1) AND ($2::bigint IS NULL OR moderator_id = $2) ORDER BY id DESC LIMIT 100",
                    &[
                        query(&req, "target").map_or(SqlValue::Null, |t| SqlValue::Text(t.into())),
                        query(&req, "moderator_id").and_then(|m| m.parse().ok()).map_or(SqlValue::Null, SqlValue::Int),
                    ],
                )?;
                let items: Vec<Value> = r
                    .rows
                    .iter()
                    .map(|row| {
                        json!({ "target": text(&row[0]), "decision": text(&row[1]), "note": text(&row[2]), "moderator_id": int(&row[3]), "actor": text(&row[4]),
                                "snapshot": text(&row[5]).and_then(|s| serde_json::from_str::<Value>(&s).ok()), "created_at": int(&row[6]) })
                    })
                    .collect();
                Ok(resp(200, &json!({ "items": items })))
            }
            "candidates" => {
                let uid = me().ok();
                let r = sql::query(
                    "SELECT t.id, t.title, t.type_id, (extract(epoch FROM t.created_at) * 1000)::bigint,
                            (SELECT count(*) FILTER (WHERE value = 1) FROM candidate_votes v WHERE v.torrent_id = t.id),
                            (SELECT count(*) FILTER (WHERE value = -1) FROM candidate_votes v WHERE v.torrent_id = t.id),
                            (SELECT value FROM candidate_votes v WHERE v.torrent_id = t.id AND v.user_id = $1)
                       FROM torrent.v_torrent_public t WHERE t.status = 'candidate' ORDER BY t.created_at DESC LIMIT 100",
                    &[uid.map_or(SqlValue::Null, SqlValue::Int)],
                )?;
                let items: Vec<Value> = r
                    .rows
                    .iter()
                    .map(|row| json!({ "torrent_id": int(&row[0]), "title": text(&row[1]), "type_id": text(&row[2]), "created_at": int(&row[3]), "up": int(&row[4]), "down": int(&row[5]), "my_vote": int(&row[6]) }))
                    .collect();
                Ok(resp(
                    200,
                    &json!({ "items": items, "approve_votes": settings()["approve_votes"].as_i64().unwrap_or(3) }),
                ))
            }
            "vote" => {
                let id: i64 = param(&req, "id")
                    .and_then(|v| v.parse().ok())
                    .ok_or_else(|| err("kernel.validation_failed", "bad id"))?;
                Ok(resp(200, &vote(me()?, id, b["value"].as_i64().unwrap_or(0))?))
            }
            "list_words" => {
                let r = sql::query(
                    "SELECT id, pattern, action, scopes::text, active FROM words ORDER BY id",
                    &[],
                )?;
                let items: Vec<Value> = r
                    .rows
                    .iter()
                    .map(|row| json!({ "id": int(&row[0]), "pattern": text(&row[1]), "action": text(&row[2]), "scopes": text(&row[3]), "active": matches!(row[4], SqlValue::Boolean(true)) }))
                    .collect();
                Ok(resp(200, &json!({ "items": items })))
            }
            "add_word" => {
                let pattern = b["pattern"].as_str().unwrap_or("").trim();
                let action = b["action"].as_str().unwrap_or("review");
                if pattern.is_empty()
                    || pattern.chars().count() > 100
                    || !matches!(action, "block" | "mask" | "review")
                {
                    return Err(err(
                        "kernel.validation_failed",
                        "pattern (1..=100 chars) and action (block / mask / review) are required",
                    ));
                }
                let scopes: Vec<String> = b["scopes"]
                    .as_array()
                    .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                    .unwrap_or_else(|| vec!["comment".into()]);
                let r = sql::query(
                    "INSERT INTO words (pattern, kind, action, scopes) VALUES ($1, 'exact', $2, ARRAY(SELECT jsonb_array_elements_text($3::jsonb))) RETURNING id",
                    &[
                        SqlValue::Text(pattern.into()),
                        SqlValue::Text(action.into()),
                        SqlValue::Text(json!(scopes).to_string()),
                    ],
                )?;
                Ok(resp(
                    201,
                    &json!({ "id": r.rows.first().and_then(|row| int(&row[0])) }),
                ))
            }
            "delete_word" => {
                let id: i64 = param(&req, "id")
                    .and_then(|v| v.parse().ok())
                    .ok_or_else(|| err("kernel.validation_failed", "bad id"))?;
                sql::execute("DELETE FROM words WHERE id = $1", &[SqlValue::Int(id)])?;
                Ok(no_content())
            }
            "word_hits" => {
                let r = sql::query(
                    "SELECT id, scope, author_id, excerpt, (extract(epoch FROM created_at) * 1000)::bigint FROM word_hits WHERE state = 'open' ORDER BY id DESC LIMIT 100",
                    &[],
                )?;
                let items: Vec<Value> = r
                    .rows
                    .iter()
                    .map(|row| json!({ "id": int(&row[0]), "scope": text(&row[1]), "author_id": int(&row[2]), "excerpt": text(&row[3]), "created_at": int(&row[4]) }))
                    .collect();
                Ok(resp(200, &json!({ "items": items })))
            }
            _ => Err(err("kernel.not_found", "no such route")),
        }
    }
}

// ---------------- ctx.moderation ----------------

fn queue_stats() -> R<Value> {
    let r = sql::query(
        "SELECT count(*) FILTER (WHERE state = 'pending'), count(*) FILTER (WHERE state = 'candidate'), count(*) FILTER (WHERE state = 'awaiting_uploader'),
                coalesce(extract(epoch FROM now() - min(submitted_at))::bigint, 0) FROM queue",
        &[],
    )?;
    let row = r.rows.first().cloned().unwrap_or_default();
    let g = |i: usize| row.get(i).and_then(int).unwrap_or(0);
    Ok(json!({ "pending": g(0), "candidate": g(1), "awaiting_uploader": g(2), "oldest_age_secs": g(3) }))
}

impl service_provider::Guest for Moderation {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        let scoped = invocation::current().caller_scopes.iter().any(|s| s == "write");
        let out = match method.as_str() {
            "queue_stats" => queue_stats()?,
            "submit" => {
                if !scoped {
                    return Err(err("kernel.forbidden", "scope write required"));
                }
                let target = a["target"].as_str().unwrap_or("");
                sql::execute(
                    "INSERT INTO queue (target, kind, state, submitted_at) VALUES ($1, split_part($1, ':', 1), 'pending', now()) ON CONFLICT (target) DO NOTHING",
                    &[SqlValue::Text(target.into())],
                )?;
                publish(
                    "moderation.submitted",
                    target,
                    json!({ "target": target, "submitted_by_plugin": invocation::current().caller_plugin, "reason": a["reason"] }),
                )?;
                json!({})
            }
            "decide" => {
                let uid = me()?;
                if !scoped || !has(uid, "moderation.decide") {
                    return Err(err(
                        "kernel.forbidden",
                        "scope write and moderation.decide required",
                    ));
                }
                decide(
                    a["target"].as_str().unwrap_or(""),
                    Some(uid),
                    a["decision"].as_str().unwrap_or(""),
                    a["note"].as_str().unwrap_or(""),
                    &format!("user:{uid}"),
                )?
            }
            other => {
                return Err(err(
                    "kernel.not_found",
                    &format!("ctx.moderation has no method `{other}`"),
                ));
            }
        };
        Ok(out.to_string())
    }
}

// ---------------- MCP ----------------

impl mcp_tool::Guest for Moderation {
    fn call(tool: String, arguments: String) -> R<String> {
        let a: Value = serde_json::from_str(&arguments).unwrap_or(Value::Null);
        let out = match tool.as_str() {
            "admin_list_moderation_queue" => queue_list(
                a["state"].as_str(),
                a["type_id"].as_str(),
                a["limit"].as_i64().unwrap_or(20).clamp(1, 100),
            )?,
            "admin_decide_moderation" => {
                let uid = me()?;
                decide(
                    a["target"].as_str().unwrap_or(""),
                    Some(uid),
                    a["decision"].as_str().unwrap_or(""),
                    a["note"].as_str().unwrap_or(""),
                    &format!("mcp:{uid}"),
                )?
            }
            _ => return Err(err("kernel.not_found", "unknown tool")),
        };
        Ok(out.to_string())
    }
}

export!(Moderation);
