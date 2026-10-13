//! reports 插件（WASM，docs/plugins/reports/）：举报与工单。
//!
//! 本版本实现：理由目录（按目标类型，默认目录随迁移写入）、举报（目标存在性：种子、作品、评论、用户；不能举报自己；同一目标
//! 未结工单合并、同一人只举报一次、优先级取最高；每小时次数；多次无效举报暂时禁止；快照只接受服务调用）、工单队列（优先级、
//! 举报数）、认领、处理（动作：删除评论、禁用种子、限制用户、无；部分失败返回 207 且不结案；结论无效时记入举报者滥用计数）、
//! 我的举报、`ctx.reports`（代为举报、处理）、事件、MCP 工具 `admin_list_reports` / `admin_resolve_report`。
//! 通知举报者与被举报者（ctx.notify）在 notify 插件实现后接入。

#![allow(clippy::missing_errors_doc)]

pt_plugin_sdk::world!("reports");

use exports::pt::plugin::{http_handler, mcp_tool, service_provider};
use pt_plugin_sdk::{
    audit, config, events, invocation, redis, services,
    sql::{self, SqlValue},
    types::PluginError,
};
use serde_json::{Value, json};

struct Reports;

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

fn setting(k: &str, default: i64) -> i64 {
    let s: Value = serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null);
    s[k].as_i64().unwrap_or(default)
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

fn svc(service: &str, method: &str, args: &Value) -> R<Value> {
    let out = services::call(service, method, &args.to_string())?;
    Ok(serde_json::from_str(&out).unwrap_or(Value::Null))
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

/// 目标 → (类型, ID, 预览, 目标所属用户)
/// (类型, ID, 预览, 目标所属用户)
type Located = (String, i64, Value, Option<i64>);

fn locate(target: &str, snapshot: Option<&Value>) -> R<Option<Located>> {
    let Some((kind, id)) = target.split_once(':') else {
        return Ok(None);
    };
    let Ok(id) = id.parse::<i64>() else {
        return Ok(None);
    };
    let found = match kind {
        "torrent" => sql::query(
            "SELECT title, status, uploader_id FROM torrent.v_torrent_public WHERE id = $1",
            &[SqlValue::Int(id)],
        )?
        .rows
        .first()
        .map(|r| (json!({ "title": text(&r[0]), "status": text(&r[1]) }), int(&r[2]))),
        "group" => sql::query(
            "SELECT title FROM torrent.v_group_public WHERE id = $1",
            &[SqlValue::Int(id)],
        )?
        .rows
        .first()
        .map(|r| (json!({ "title": text(&r[0]) }), None)),
        "user" => sql::query(
            "SELECT username, status FROM identity.v_user_public WHERE id = $1",
            &[SqlValue::Int(id)],
        )?
        .rows
        .first()
        .map(|r| {
            (
                json!({ "username": text(&r[0]), "status": text(&r[1]) }),
                Some(id),
            )
        }),
        // 私信：只能经服务调用附快照举报；之后的预览取已保存的快照
        "message" => {
            let snap = match snapshot {
                Some(s) => Some(s.clone()),
                None => sql::query(
                    "SELECT r.snapshot::text FROM reports r JOIN cases c ON c.id = r.case_id WHERE c.target = $1 AND r.snapshot IS NOT NULL ORDER BY r.id LIMIT 1",
                    &[SqlValue::Text(target.to_owned())],
                )?
                .rows
                .first()
                .and_then(|r| text(&r[0]))
                .and_then(|t| serde_json::from_str(&t).ok()),
            };
            snap.map(|s| {
                (
                    json!({ "text": s["text"], "sender_id": s["sender_id"] }),
                    s["sender_id"].as_i64(),
                )
            })
        }
        "comment" => match svc("ctx.comments", "exists", &json!({ "id": id })) {
            Ok(v) if !v.is_null() => Some((
                json!({ "target": v["target"], "excerpt": v["excerpt"], "state": v["state"] }),
                v["author_id"].as_i64(),
            )),
            _ => None,
        },
        _ => None,
    };
    Ok(found.map(|(p, owner)| (kind.to_owned(), id, p, owner)))
}

struct NewReport<'a> {
    reporter: i64,
    target: &'a str,
    reason: &'a str,
    note: String,
    snapshot: Option<Value>,
}

fn create(r: NewReport<'_>) -> R<Value> {
    let ban = sql::query(
        "SELECT 1 FROM abuse WHERE user_id = $1 AND banned_until > now()",
        &[SqlValue::Int(r.reporter)],
    )?;
    if !ban.rows.is_empty() {
        return Err(err(
            "reports.temporarily_banned",
            "you cannot report for a while because of repeated invalid reports",
        ));
    }
    let key = format!("rl:{}", r.reporter);
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
    if n > setting("rate_per_hour", 20) {
        return Err(err("reports.rate_limited", "too many reports, try again later"));
    }
    let Some((kind, ..)) = locate(r.target, r.snapshot.as_ref())? else {
        return Err(err("reports.target_not_found", "target not found"));
    };
    if r.target == format!("user:{}", r.reporter) {
        return Err(err("reports.self_report", "you cannot report yourself"));
    }
    let reason = sql::query(
        "SELECT priority, requires_note FROM reasons WHERE kind = $1 AND code = $2 AND active",
        &[SqlValue::Text(kind.clone()), SqlValue::Text(r.reason.to_owned())],
    )?;
    let row = reason.rows.first().ok_or_else(|| {
        err(
            "reports.reason_invalid",
            "this reason does not apply to the target",
        )
    })?;
    let priority = int(&row[0]).unwrap_or(20);
    if matches!(row[1], SqlValue::Boolean(true)) && r.note.trim().is_empty() {
        return Err(err("reports.note_required", "this reason requires a note"));
    }
    // 同一目标的未结工单（并发时由部分唯一索引兜底）
    let c = sql::query(
        "INSERT INTO cases (target, kind, state, priority) VALUES ($1, $2, 'open', $3)
         ON CONFLICT (target) WHERE state IN ('open', 'claimed', 'escalated') DO UPDATE SET last_report_at = now()
         RETURNING id",
        &[SqlValue::Text(r.target.to_owned()), SqlValue::Text(kind), SqlValue::Int(priority)],
    )?;
    let case_id = c.rows.first().and_then(|row| int(&row[0])).unwrap_or(0);
    let ins = sql::query(
        "INSERT INTO reports (case_id, reporter_id, reason_code, note, snapshot) VALUES ($1, $2, $3, $4, $5::jsonb)
         ON CONFLICT (case_id, reporter_id) DO NOTHING RETURNING id",
        &[
            SqlValue::Int(case_id),
            SqlValue::Int(r.reporter),
            SqlValue::Text(r.reason.to_owned()),
            SqlValue::Text(r.note.chars().take(2000).collect()),
            r.snapshot.map_or(SqlValue::Null, |s| SqlValue::Text(s.to_string())),
        ],
    )?;
    let Some(report_id) = ins.rows.first().and_then(|row| int(&row[0])) else {
        return Err(err("reports.already_reported", "you already reported this"));
    };
    sql::execute(
        "UPDATE cases SET report_count = report_count + 1, priority = greatest(priority, $2), last_report_at = now() WHERE id = $1",
        &[SqlValue::Int(case_id), SqlValue::Int(priority)],
    )?;
    publish(
        "report.created",
        r.target,
        json!({ "report_id": report_id, "case_id": case_id, "target": r.target, "reporter_id": r.reporter, "reason_code": r.reason }),
    )?;
    Ok(json!({ "report_id": report_id, "case_id": case_id }))
}

fn case_json(id: i64, staff: bool) -> R<Option<Value>> {
    let r = sql::query(
        "SELECT target, kind, state, priority, report_count, claimed_by, resolution, note_public, note_internal, action_results,
                (extract(epoch FROM created_at) * 1000)::bigint, (extract(epoch FROM resolved_at) * 1000)::bigint
           FROM cases WHERE id = $1",
        &[SqlValue::Int(id)],
    )?;
    let Some(row) = r.rows.first() else {
        return Ok(None);
    };
    let target = text(&row[0]).unwrap_or_default();
    let kind = text(&row[1]).unwrap_or_default();
    let mut v = json!({
        "id": id, "target": target, "kind": kind, "state": text(&row[2]), "priority": int(&row[3]), "report_count": int(&row[4]),
        "claimed_by": int(&row[5]), "resolution": text(&row[6]), "note_public": text(&row[7]), "created_at": int(&row[10]), "resolved_at": int(&row[11]),
    });
    if staff {
        v["note_internal"] = json!(text(&row[8]));
        v["action_results"] = text(&row[9])
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        v["target_preview"] = locate(&target, None)?.map_or(Value::Null, |l| l.2);
        let reps = sql::query(
            "SELECT r.reporter_id, u.username, r.reason_code, r.note, r.snapshot::text, (extract(epoch FROM r.created_at) * 1000)::bigint
               FROM reports r LEFT JOIN identity.v_user_public u ON u.id = r.reporter_id WHERE r.case_id = $1 ORDER BY r.id",
            &[SqlValue::Int(id)],
        )?;
        v["reports"] = Value::Array(
            reps.rows
                .iter()
                .map(|r| {
                    json!({ "reporter": { "id": int(&r[0]), "username": text(&r[1]) }, "reason_code": text(&r[2]), "note": text(&r[3]),
                            "snapshot": text(&r[4]).and_then(|s| serde_json::from_str::<Value>(&s).ok()), "created_at": int(&r[5]) })
                })
                .collect(),
        );
        v["available_actions"] = json!(actions_for(&kind));
    }
    Ok(Some(v))
}

fn actions_for(kind: &str) -> Vec<&'static str> {
    match kind {
        "comment" => vec!["delete_content", "restrict_user", "none"],
        "torrent" => vec!["disable_torrent", "restrict_user", "none"],
        "user" | "message" => vec!["restrict_user", "none"],
        _ => vec!["none"],
    }
}

/// 执行一个处理动作（LLD §1.4）
fn run_action(a: &Value, kind: &str, id: i64, owner: Option<i64>, case_id: i64, i: usize) -> R<()> {
    let k = a["kind"].as_str().unwrap_or("none");
    if !actions_for(kind).contains(&k) {
        return Err(err(
            "reports.action_unavailable",
            "action is not available for this target",
        ));
    }
    let reason = format!("reports.case_{case_id}");
    match k {
        "delete_content" => {
            svc("ctx.comments", "delete", &json!({ "id": id, "reason": reason }))?;
        }
        "disable_torrent" => {
            svc(
                "ctx.torrents",
                "set_status",
                &json!({ "id": id, "status": "disabled", "reason": reason }),
            )?;
        }
        "restrict_user" => {
            let uid =
                owner.ok_or_else(|| err("reports.action_unavailable", "the target has no known owner"))?;
            let me = me()?;
            if !has(me, "identity.users.ban") {
                return Err(err(
                    "kernel.forbidden",
                    "restricting users requires identity.users.ban",
                ));
            }
            let restriction = match a["restriction"].as_str() {
                Some("banned") => "ban",
                _ => "leech_disabled",
            };
            let until = a["days"]
                .as_i64()
                .filter(|d| *d > 0)
                .map(|d| pt_plugin_sdk::time::now() + d * 86_400_000);
            svc(
                "ctx.users",
                "apply_restriction",
                &json!({ "user": uid, "kind": restriction, "reason_code": reason, "until": until, "idempotency_key": format!("rpt:{case_id}:{i}") }),
            )?;
        }
        _ => {}
    }
    Ok(())
}

/// 处理工单；返回 (HTTP 状态, 结果)
fn resolve(case_id: i64, staff: i64, b: &Value) -> R<(u16, Value)> {
    let code = b["code"].as_str().unwrap_or("");
    if !matches!(code, "resolved" | "invalid" | "escalated") {
        return Err(err(
            "kernel.validation_failed",
            "code must be resolved, invalid or escalated",
        ));
    }
    let r = sql::query(
        "SELECT target, kind, state, claimed_by, (claim_until > now()) FROM cases WHERE id = $1 FOR UPDATE",
        &[SqlValue::Int(case_id)],
    )?;
    let row = r
        .rows
        .first()
        .ok_or_else(|| err("reports.case_not_found", "case not found"))?;
    let (target, kind, state) = (
        text(&row[0]).unwrap_or_default(),
        text(&row[1]).unwrap_or_default(),
        text(&row[2]).unwrap_or_default(),
    );
    if state == "closed" {
        return Err(err("reports.case_not_found", "case is already closed"));
    }
    if matches!(row[4], SqlValue::Boolean(true)) && int(&row[3]).is_some_and(|c| c != staff) {
        return Err(err(
            "reports.claimed_by_other",
            "another moderator is handling this case",
        ));
    }
    if state == "escalated" && !has(staff, "identity.users.ban") {
        return Err(err(
            "reports.escalation_required",
            "escalated cases need an administrator",
        ));
    }
    let id: i64 = target
        .split_once(':')
        .and_then(|(_, i)| i.parse().ok())
        .unwrap_or(0);
    let owner = locate(&target, None)?.and_then(|l| l.3);
    let mut results = Vec::new();
    let mut ok_all = true;
    if code == "resolved" {
        for (i, a) in b["actions"].as_array().into_iter().flatten().enumerate() {
            match run_action(a, &kind, id, owner, case_id, i) {
                Ok(()) => results.push(json!({ "kind": a["kind"], "ok": true })),
                Err(e) => {
                    ok_all = false;
                    results.push(json!({ "kind": a["kind"], "ok": false, "error": e.code }));
                }
            }
        }
    }
    let note = |k: &str| SqlValue::Text(b[k].as_str().unwrap_or("").chars().take(2000).collect());
    if !ok_all {
        sql::execute(
            "UPDATE cases SET action_results = $2::jsonb WHERE id = $1",
            &[SqlValue::Int(case_id), SqlValue::Text(json!(results).to_string())],
        )?;
        return Ok((207, json!({ "results": results })));
    }
    let new_state = if code == "escalated" {
        "escalated"
    } else {
        "closed"
    };
    sql::execute(
        "UPDATE cases SET state = $2, resolution = $3, note_internal = $4, note_public = $5, action_results = $6::jsonb, resolved_by = $7,
                resolved_at = CASE WHEN $2 = 'closed' THEN now() END, claimed_by = NULL, claim_until = NULL WHERE id = $1",
        &[
            SqlValue::Int(case_id),
            SqlValue::Text(new_state.into()),
            SqlValue::Text(code.into()),
            note("note_internal"),
            note("note_public"),
            SqlValue::Text(json!(results).to_string()),
            SqlValue::Int(staff),
        ],
    )?;
    // 无效举报：举报者滥用计数（窗口 30 天，达到阈值暂时禁止举报）
    if code == "invalid" {
        let threshold = setting("abuse_threshold", 5);
        let days = setting("abuse_ban_days", 7);
        sql::execute(
            "INSERT INTO abuse (user_id, invalid_count, window_start)
             SELECT reporter_id, 1, now() FROM reports WHERE case_id = $1 AND reporter_id IS NOT NULL
             ON CONFLICT (user_id) DO UPDATE SET
               invalid_count = CASE WHEN abuse.window_start < now() - interval '30 days' THEN 1 ELSE abuse.invalid_count + 1 END,
               window_start = CASE WHEN abuse.window_start < now() - interval '30 days' THEN now() ELSE abuse.window_start END",
            &[SqlValue::Int(case_id)],
        )?;
        sql::execute(
            "UPDATE abuse SET banned_until = now() + make_interval(days => $2::int)
              WHERE invalid_count >= $1 AND (banned_until IS NULL OR banned_until < now())
                AND user_id IN (SELECT reporter_id FROM reports WHERE case_id = $3)",
            &[
                SqlValue::Int(threshold),
                SqlValue::Int(days),
                SqlValue::Int(case_id),
            ],
        )?;
    }
    audit::write(&audit::AuditEntry {
        action: "reports.resolve".into(),
        target: format!("case:{case_id}"),
        details: json!({ "code": code, "results": results, "target": target }).to_string(),
    })?;
    // 通知举报者（系统举报者 0 除外；升级不算结案）
    if new_state == "closed" {
        let public = b["note_public"].as_str().unwrap_or("").to_owned();
        let reporters = sql::query(
            "SELECT DISTINCT reporter_id FROM reports WHERE case_id = $1 AND reporter_id > 0",
            &[SqlValue::Int(case_id)],
        )?;
        for rid in reporters.rows.iter().filter_map(|r| int(&r[0])) {
            let zh = if code == "resolved" {
                "你的举报已处理"
            } else {
                "你的举报经核实不成立"
            };
            notify(
                rid,
                &format!("reports.resolved.{code}"),
                "content",
                json!({ "note": public }),
                zh,
                &public,
                "/p/reports/",
                &format!("case:{case_id}:{rid}"),
            );
        }
    }
    let first = sql::query(
        "SELECT min(id) FROM reports WHERE case_id = $1",
        &[SqlValue::Int(case_id)],
    )?;
    publish(
        "report.resolved",
        &target,
        json!({ "report_id": first.rows.first().and_then(|r| int(&r[0])), "case_id": case_id, "target": target, "resolution": code, "by": staff }),
    )?;
    Ok((200, json!({ "state": new_state, "results": results })))
}

fn list_cases(state: Option<&str>, kind: Option<&str>) -> R<Value> {
    let r = sql::query(
        "SELECT id, target, kind, state, priority, report_count, claimed_by, (extract(epoch FROM created_at) * 1000)::bigint FROM cases
          WHERE ($1::text IS NULL AND state IN ('open', 'claimed', 'escalated') OR state = $1) AND ($2::text IS NULL OR kind = $2)
          ORDER BY priority DESC, report_count DESC, created_at LIMIT 100",
        &[state.map_or(SqlValue::Null, |s| SqlValue::Text(s.into())), kind.map_or(SqlValue::Null, |s| SqlValue::Text(s.into()))],
    )?;
    let items: Vec<Value> = r
        .rows
        .iter()
        .map(|row| {
            json!({ "id": int(&row[0]), "target": text(&row[1]), "kind": text(&row[2]), "state": text(&row[3]), "priority": int(&row[4]),
                    "report_count": int(&row[5]), "claimed_by": int(&row[6]), "created_at": int(&row[7]) })
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

impl http_handler::Guest for Reports {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let b: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
        match req.route_id.as_str() {
            "reasons" => {
                let r = sql::query(
                    "SELECT kind, code, requires_note FROM reasons WHERE active AND ($1::text IS NULL OR kind = $1) ORDER BY kind, priority DESC, code",
                    &[query(&req, "kind").map_or(SqlValue::Null, |k| SqlValue::Text(k.into()))],
                )?;
                let items: Vec<Value> = r
                    .rows
                    .iter()
                    .map(|row| json!({ "kind": text(&row[0]), "code": text(&row[1]), "requires_note": matches!(row[2], SqlValue::Boolean(true)) }))
                    .collect();
                Ok(resp(200, &json!({ "items": items })))
            }
            "create" => {
                // HTTP 提交的快照被忽略（防伪造）
                let out = create(NewReport {
                    reporter: me()?,
                    target: b["target"].as_str().unwrap_or(""),
                    reason: b["reason_code"].as_str().unwrap_or(""),
                    note: b["note"].as_str().unwrap_or("").to_owned(),
                    snapshot: None,
                })?;
                Ok(resp(201, &out))
            }
            "my_reports" => {
                let r = sql::query(
                    "SELECT r.id, c.target, r.reason_code, c.state, c.resolution, c.note_public, (extract(epoch FROM r.created_at) * 1000)::bigint
                       FROM reports r JOIN cases c ON c.id = r.case_id WHERE r.reporter_id = $1 ORDER BY r.id DESC LIMIT 100",
                    &[SqlValue::Int(me()?)],
                )?;
                let items: Vec<Value> = r
                    .rows
                    .iter()
                    .map(|row| {
                        json!({ "id": int(&row[0]), "target": text(&row[1]), "reason_code": text(&row[2]), "state": text(&row[3]),
                                "resolution": text(&row[4]), "note_public": text(&row[5]), "created_at": int(&row[6]) })
                    })
                    .collect();
                Ok(resp(200, &json!({ "items": items })))
            }
            "list_cases" => Ok(resp(200, &list_cases(query(&req, "state"), query(&req, "kind"))?)),
            "get_case" => case_json(id_param(&req)?, true)?
                .map(|v| resp(200, &v))
                .ok_or_else(|| err("reports.case_not_found", "case not found")),
            "claim" => {
                let uid = me()?;
                let mins = setting("claim_minutes", 30);
                let n = sql::execute(
                    "UPDATE cases SET claimed_by = $2, claim_until = now() + make_interval(mins => $3::int), state = CASE WHEN state = 'open' THEN 'claimed' ELSE state END
                      WHERE id = $1 AND state <> 'closed' AND (claimed_by IS NULL OR claimed_by = $2 OR claim_until < now())",
                    &[SqlValue::Int(id_param(&req)?), SqlValue::Int(uid), SqlValue::Int(mins)],
                )?;
                if n == 0 {
                    return Err(err(
                        "reports.claimed_by_other",
                        "the case is closed or claimed by another moderator",
                    ));
                }
                Ok(resp(200, &json!({ "claimed": true })))
            }
            "resolve" => {
                let (status, v) = resolve(id_param(&req)?, me()?, &b)?;
                Ok(resp(status, &v))
            }
            _ => Err(err("kernel.not_found", "no such route")),
        }
    }
}

// ---------------- ctx.reports ----------------

impl service_provider::Guest for Reports {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        let inv = invocation::current();
        if !inv.caller_scopes.iter().any(|s| s == "write") {
            return Err(err("kernel.forbidden", "scope write required"));
        }
        let out = match method.as_str() {
            // 系统举报（如 ai-moderation）：reporter 为空时记为 0（系统）
            "create" => create(NewReport {
                reporter: a["reporter_id"].as_i64().unwrap_or(0),
                target: a["target"].as_str().unwrap_or(""),
                reason: a["reason_code"].as_str().unwrap_or(""),
                note: a["note"].as_str().unwrap_or("").to_owned(),
                snapshot: a
                    .get("snapshot")
                    .filter(|s| !s.is_null() && s.to_string().len() <= 65_536)
                    .cloned(),
            })?,
            "resolve" => {
                let uid = me()?;
                if !has(uid, "reports.moderate") {
                    return Err(err("kernel.forbidden", "reports.moderate required"));
                }
                resolve(a["case_id"].as_i64().unwrap_or(0), uid, &a)?.1
            }
            other => {
                return Err(err(
                    "kernel.not_found",
                    &format!("ctx.reports has no method `{other}`"),
                ));
            }
        };
        Ok(out.to_string())
    }
}

// ---------------- MCP ----------------

impl mcp_tool::Guest for Reports {
    fn call(tool: String, arguments: String) -> R<String> {
        let a: Value = serde_json::from_str(&arguments).unwrap_or(Value::Null);
        let out = match tool.as_str() {
            "admin_list_reports" => list_cases(a["state"].as_str(), a["kind"].as_str())?,
            "admin_resolve_report" => {
                let case_id = a["case_id"]
                    .as_i64()
                    .ok_or_else(|| err("kernel.validation_failed", "case_id"))?;
                resolve(case_id, me()?, &a)?.1
            }
            _ => return Err(err("kernel.not_found", "unknown tool")),
        };
        Ok(out.to_string())
    }
}

export!(Reports);
