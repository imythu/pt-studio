//! requests 插件（WASM，docs/plugins/requests/）：求种与悬赏。
//!
//! 本版本实现：发布（类型、标题、描述（pt-markup）、规格要求谓词、初始悬赏，`request.before_create` 钩子）、悬赏追加与托管
//! （经 `ctx.ledger` 从贡献者回收，幂等键按贡献）、投票、应求（种子须已通过且类型一致、规格要求逐条检查，版主可放宽；同时只有
//! 一个待确认应求）、确认（求种者或版主；悬赏扣除手续费后发给应求者）、驳回、撤回求种（退还全部托管悬赏）、到期自动关闭并退款、
//! 待确认超时自动确认、通知（应求待确认、确认、驳回）、`ctx.requests`、事件、MCP `list_requests` / `create_request`。
//! 认领、撤销已发放的悬赏、对账在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

pt_plugin_sdk::world!("requests");

use exports::pt::plugin::{http_handler, mcp_tool, on_cron, service_provider};
use pt_plugin_sdk::{
    config, events, hook_runner, invocation, services,
    sql::{self, SqlValue},
    types::PluginError,
};
use serde_json::{Value, json};

struct Requests;

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

fn setting(k: &str, d: i64) -> i64 {
    settings()[k].as_i64().unwrap_or(d)
}

fn currency() -> String {
    settings()["currency"].as_str().unwrap_or("bonus").to_owned()
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

fn publish(event: &str, key: &str, payload: Value) -> R<()> {
    events::publish(&events::NewEvent {
        event_type: event.into(),
        version: 1,
        key: key.into(),
        payload: payload.to_string(),
    })?;
    Ok(())
}

/// 发送站内通知（notify 未安装或失败时忽略）
fn notify(user: i64, key: &str, args: Value, title: &str, link: &str, idem: &str) {
    let _ = services::call(
        "ctx.notify",
        "send",
        &json!({ "user_id": user, "template_key": key, "category": "content", "args": args, "title": title, "body": "", "link": link, "idempotency_key": idem }).to_string(),
    );
}

/// 经 ledger 转账：from / to 为空表示系统回收 / 发行
fn ledger(from: Option<i64>, to: Option<i64>, amount: i64, reason: &str, memo: &str, key: &str) -> R<()> {
    services::call(
        "ctx.ledger",
        "transfer",
        &json!({ "currency": currency(), "from_user": from, "to_user": to, "amount": amount, "reason": reason, "memo": memo, "idempotency_key": key }).to_string(),
    )?;
    Ok(())
}

// ---------------- 规格要求（谓词） ----------------

/// `[{field, op, value}]`；op ∈ eq / ne / in / not_in / gte / lte / gt / lt；返回不满足的条目
pub fn failed_requirements(reqs: &Value, fields: &Value) -> Vec<Value> {
    let num = |v: &Value| v.as_f64();
    reqs.as_array()
        .into_iter()
        .flatten()
        .filter(|r| {
            let f = &fields[r["field"].as_str().unwrap_or("")];
            let v = &r["value"];
            let ok = match r["op"].as_str().unwrap_or("eq") {
                "eq" => f == v,
                "ne" => f != v,
                "in" | "not_in" => {
                    let list = v.as_array().cloned().unwrap_or_default();
                    let hit = match f {
                        Value::Array(a) => a.iter().any(|x| list.contains(x)),
                        x => list.contains(x),
                    };
                    hit == (r["op"] == "in")
                }
                "gte" => num(f).zip(num(v)).is_some_and(|(a, b)| a >= b),
                "lte" => num(f).zip(num(v)).is_some_and(|(a, b)| a <= b),
                "gt" => num(f).zip(num(v)).is_some_and(|(a, b)| a > b),
                "lt" => num(f).zip(num(v)).is_some_and(|(a, b)| a < b),
                _ => false,
            };
            !ok
        })
        .cloned()
        .collect()
}

// ---------------- 核心流程 ----------------

fn contribute(uid: i64, rid: i64, amount: i64) -> R<i64> {
    if amount < setting("min_contribution", 1) {
        return Err(err(
            "requests.bounty_too_small",
            "the amount is below the minimum",
        ));
    }
    let r = sql::query(
        "SELECT state FROM requests WHERE id = $1 FOR UPDATE",
        &[SqlValue::Int(rid)],
    )?;
    if !matches!(
        r.rows.first().and_then(|row| text(&row[0])).as_deref(),
        Some("open" | "fill_pending")
    ) {
        return Err(err("requests.not_open", "the request is not open"));
    }
    let c = sql::query(
        "INSERT INTO contributions (request_id, user_id, amount, state) VALUES ($1, $2, $3, 'pending') RETURNING id",
        &[SqlValue::Int(rid), SqlValue::Int(uid), SqlValue::Int(amount)],
    )?;
    let cid = c.rows.first().and_then(|row| int(&row[0])).unwrap_or(0);
    // 托管：从贡献者回收（ledger 的工作单元独立提交；幂等键防重复扣款）
    ledger(
        Some(uid),
        None,
        amount,
        "requests.bounty_escrow",
        &format!("request:{rid}"),
        &format!("req:c:{cid}"),
    )?;
    sql::execute(
        "UPDATE contributions SET state = 'escrowed' WHERE id = $1",
        &[SqlValue::Int(cid)],
    )?;
    let t = sql::query(
        "UPDATE requests SET bounty_total = bounty_total + $2, updated_at = now() WHERE id = $1 RETURNING bounty_total",
        &[SqlValue::Int(rid), SqlValue::Int(amount)],
    )?;
    let total = t.rows.first().and_then(|row| int(&row[0])).unwrap_or(0);
    publish(
        "request.bounty_added",
        &format!("request:{rid}"),
        json!({ "request_id": rid, "user_id": uid, "amount": amount, "currency": currency(), "total": total }),
    )?;
    Ok(total)
}

fn create(uid: i64, b: &Value) -> R<Value> {
    let type_id = b["type_id"].as_str().unwrap_or("").trim().to_owned();
    let mut title: String = b["title"]
        .as_str()
        .unwrap_or("")
        .trim()
        .chars()
        .take(200)
        .collect();
    let desc = b["description"].as_str().unwrap_or("").to_owned();
    let bounty = b["bounty"].as_i64().unwrap_or(0);
    if type_id.is_empty() || title.is_empty() || desc.chars().count() > 20_000 {
        return Err(err(
            "kernel.validation_failed",
            "type_id and title are required; description ≤ 20000 characters",
        ));
    }
    if bounty < setting("min_bounty", 1) {
        return Err(err(
            "requests.bounty_too_small",
            "the initial bounty is below the minimum",
        ));
    }
    let reqs = b.get("requirements").cloned().unwrap_or(json!([]));
    if !reqs.is_array()
        || reqs
            .as_array()
            .is_some_and(|a| a.len() > 20 || a.iter().any(|r| !r["field"].is_string()))
    {
        return Err(err(
            "kernel.validation_failed",
            "requirements must be a list of {field, op, value}",
        ));
    }
    let hook = json!({ "version": 1, "user_id": uid, "type_id": type_id, "title": title, "bounty": bounty, "currency": currency() });
    match hook_runner::run("request.before_create", &hook.to_string())? {
        hook_runner::HookOutcome::Denied(d) => return Err(err(&d.code, &d.message)),
        hook_runner::HookOutcome::Allowed(out) => {
            if let Some(t) = serde_json::from_str::<Value>(&out)
                .ok()
                .and_then(|v| v["title"].as_str().map(str::to_owned))
            {
                title = t.chars().take(200).collect();
            }
        }
    }
    let html = pt_markup::render(
        &desc,
        pt_markup::Markup::Markdown,
        &pt_markup::RenderOptions::default(),
    )
    .html;
    let r = sql::query(
        "INSERT INTO requests (user_id, type_id, title, description_src, description_html, requirements, currency, state, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7, 'open', now() + make_interval(days => $8::int)) RETURNING id",
        &[
            SqlValue::Int(uid),
            SqlValue::Text(type_id.clone()),
            SqlValue::Text(title),
            SqlValue::Text(desc),
            SqlValue::Text(html),
            SqlValue::Text(reqs.to_string()),
            SqlValue::Text(currency()),
            SqlValue::Int(setting("expire_days", 30)),
        ],
    )?;
    let rid = r.rows.first().and_then(|row| int(&row[0])).unwrap_or(0);
    contribute(uid, rid, bounty)?;
    publish(
        "request.created",
        &format!("request:{rid}"),
        json!({ "request_id": rid, "user_id": uid, "type_id": type_id, "bounty": bounty, "currency": currency() }),
    )?;
    Ok(json!({ "id": rid }))
}

/// 退还全部托管悬赏并关闭
fn close_with_refunds(rid: i64, reason: &str) -> R<()> {
    let cs = sql::query(
        "SELECT id, user_id, amount FROM contributions WHERE request_id = $1 AND state = 'escrowed'",
        &[SqlValue::Int(rid)],
    )?;
    for row in &cs.rows {
        let (cid, u, amt) = (
            int(&row[0]).unwrap_or(0),
            int(&row[1]).unwrap_or(0),
            int(&row[2]).unwrap_or(0),
        );
        ledger(
            None,
            Some(u),
            amt,
            "requests.bounty_refund",
            &format!("request:{rid}"),
            &format!("req:r:{cid}"),
        )?;
        sql::execute(
            "UPDATE contributions SET state = 'refunded' WHERE id = $1",
            &[SqlValue::Int(cid)],
        )?;
    }
    sql::execute(
        "UPDATE requests SET state = 'closed', close_reason = $2, updated_at = now() WHERE id = $1",
        &[SqlValue::Int(rid), SqlValue::Text(reason.into())],
    )?;
    publish(
        "request.closed",
        &format!("request:{rid}"),
        json!({ "request_id": rid, "reason": reason }),
    )
}

fn submit_fill(uid: i64, rid: i64, tid: i64) -> R<Value> {
    let r = sql::query(
        "SELECT user_id, type_id, state, requirements::text, title FROM requests WHERE id = $1 FOR UPDATE",
        &[SqlValue::Int(rid)],
    )?;
    let row = r
        .rows
        .first()
        .ok_or_else(|| err("requests.not_found", "request not found"))?;
    if text(&row[2]).as_deref() != Some("open") {
        return Err(err("requests.not_open", "the request is not open for fills"));
    }
    let owner = int(&row[0]);
    if owner == Some(uid) && settings()["allow_self_fill"].as_bool() != Some(true) {
        return Err(err("requests.self_fill", "you cannot fill your own request"));
    }
    let t = sql::query(
        "SELECT f.type_id, f.fields::text, p.status FROM torrent.v_torrent_fields f JOIN torrent.v_torrent_public p ON p.id = f.id WHERE f.id = $1",
        &[SqlValue::Int(tid)],
    )?;
    let trow = t
        .rows
        .first()
        .ok_or_else(|| err("requests.torrent_invalid", "torrent not found"))?;
    if text(&trow[2]).as_deref() != Some("approved") || text(&trow[0]) != text(&row[1]) {
        return Err(err(
            "requests.torrent_invalid",
            "the torrent must be approved and of the requested type",
        ));
    }
    let fields: Value = text(&trow[1])
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    let reqs: Value = text(&row[3])
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(json!([]));
    let failed = failed_requirements(&reqs, &fields);
    if !failed.is_empty() && !has(uid, "requests.moderate") {
        let mut e = err(
            "requests.requirements_not_met",
            "the torrent does not meet the request's requirements",
        );
        e.details = Some(json!({ "failed": failed }).to_string());
        return Err(e);
    }
    let f = sql::query(
        "INSERT INTO fills (request_id, torrent_id, filler_id, state, auto_confirm_at) VALUES ($1, $2, $3, 'pending', now() + make_interval(days => $4::int)) RETURNING id",
        &[
            SqlValue::Int(rid),
            SqlValue::Int(tid),
            SqlValue::Int(uid),
            SqlValue::Int(setting("auto_confirm_days", 3)),
        ],
    )?;
    let fid = f.rows.first().and_then(|row| int(&row[0])).unwrap_or(0);
    sql::execute(
        "UPDATE requests SET state = 'fill_pending', updated_at = now() WHERE id = $1",
        &[SqlValue::Int(rid)],
    )?;
    publish(
        "request.fill_submitted",
        &format!("request:{rid}"),
        json!({ "request_id": rid, "fill_id": fid, "torrent_id": tid, "filler_id": uid }),
    )?;
    if let Some(o) = owner {
        let title = text(&row[4]).unwrap_or_default();
        notify(
            o,
            "requests.fill_submitted",
            json!({ "title": title }),
            &format!("你的求种有人应求了：{title}"),
            &format!("/p/requests/{rid}"),
            &format!("fill:{fid}"),
        );
    }
    Ok(json!({ "fill_id": fid }))
}

/// 确认应求：扣除手续费后发放给应求者
fn confirm(fid: i64, by: &str) -> R<Value> {
    let f = sql::query(
        "SELECT f.request_id, f.torrent_id, f.filler_id, f.state, r.title FROM fills f JOIN requests r ON r.id = f.request_id WHERE f.id = $1 FOR UPDATE",
        &[SqlValue::Int(fid)],
    )?;
    let row = f
        .rows
        .first()
        .ok_or_else(|| err("requests.fill_not_found", "fill not found"))?;
    if text(&row[3]).as_deref() != Some("pending") {
        return Err(err("requests.fill_not_pending", "the fill is not pending"));
    }
    let (rid, tid, filler) = (
        int(&row[0]).unwrap_or(0),
        int(&row[1]).unwrap_or(0),
        int(&row[2]).unwrap_or(0),
    );
    let total = sql::query("SELECT coalesce(sum(amount), 0)::bigint FROM contributions WHERE request_id = $1 AND state = 'escrowed'", &[SqlValue::Int(rid)])?
        .rows
        .first()
        .and_then(|r| int(&r[0]))
        .unwrap_or(0);
    let fee = total * setting("fee_permille", 0).clamp(0, 1000) / 1000;
    let pay = total - fee;
    if pay > 0 {
        ledger(
            None,
            Some(filler),
            pay,
            "requests.bounty_payout",
            &format!("request:{rid}"),
            &format!("req:p:{fid}"),
        )?;
    }
    sql::execute(
        "UPDATE contributions SET state = 'paid', paid_fill_id = $2 WHERE request_id = $1 AND state = 'escrowed'",
        &[SqlValue::Int(rid), SqlValue::Int(fid)],
    )?;
    sql::execute(
        "UPDATE fills SET state = 'paid_out', confirmed_at = now(), confirmed_by = $2, payout_amount = $3 WHERE id = $1",
        &[SqlValue::Int(fid), SqlValue::Text(by.into()), SqlValue::Int(pay)],
    )?;
    sql::execute(
        "UPDATE requests SET state = 'filled', filled_torrent_id = $2, updated_at = now() WHERE id = $1",
        &[SqlValue::Int(rid), SqlValue::Int(tid)],
    )?;
    publish(
        "request.filled",
        &format!("request:{rid}"),
        json!({ "request_id": rid, "torrent_id": tid, "filler_id": filler, "payout": pay, "currency": currency() }),
    )?;
    let title = text(&row[4]).unwrap_or_default();
    notify(
        filler,
        "requests.fill_confirmed",
        json!({ "title": title, "payout": pay }),
        &format!("你的应求已确认：{title}"),
        &format!("/p/requests/{rid}"),
        &format!("fill:{fid}:ok"),
    );
    Ok(json!({ "state": "filled", "payout": pay, "fee": fee }))
}

fn owner_or_mod(rid: i64, uid: i64) -> R<bool> {
    let r = sql::query(
        "SELECT user_id FROM requests WHERE id = $1",
        &[SqlValue::Int(rid)],
    )?;
    let owner = r
        .rows
        .first()
        .ok_or_else(|| err("requests.not_found", "request not found"))?
        .first()
        .and_then(int);
    Ok(owner == Some(uid) || has(uid, "requests.moderate"))
}

fn fill_request(fid: i64) -> R<i64> {
    let r = sql::query(
        "SELECT request_id FROM fills WHERE id = $1",
        &[SqlValue::Int(fid)],
    )?;
    r.rows
        .first()
        .and_then(|row| int(&row[0]))
        .ok_or_else(|| err("requests.fill_not_found", "fill not found"))
}

fn info(rid: i64) -> R<Option<Value>> {
    let r = sql::query(
        "SELECT r.id, r.user_id, u.username, r.type_id, r.title, r.description_html, r.requirements::text, r.currency, r.bounty_total, r.votes, r.state,
                r.close_reason, (extract(epoch FROM r.expires_at) * 1000)::bigint, r.filled_torrent_id, (extract(epoch FROM r.created_at) * 1000)::bigint
           FROM requests r LEFT JOIN identity.v_user_public u ON u.id = r.user_id WHERE r.id = $1",
        &[SqlValue::Int(rid)],
    )?;
    Ok(r.rows.first().map(|row| {
        json!({ "id": int(&row[0]), "user": { "id": int(&row[1]), "username": text(&row[2]) }, "type_id": text(&row[3]), "title": text(&row[4]),
                "description_html": text(&row[5]), "requirements": text(&row[6]).and_then(|s| serde_json::from_str::<Value>(&s).ok()), "currency": text(&row[7]),
                "bounty_total": int(&row[8]), "votes": int(&row[9]), "status": text(&row[10]), "close_reason": text(&row[11]), "expires_at": int(&row[12]),
                "filled_torrent_id": int(&row[13]), "created_at": int(&row[14]) })
    }))
}

fn list(status: Option<&str>, type_id: Option<&str>, sort: &str) -> R<Value> {
    let order = match sort {
        "bounty" => "bounty_total DESC, id DESC",
        "votes" => "votes DESC, id DESC",
        _ => "id DESC",
    };
    let r = sql::query(
        &format!(
            "SELECT id, type_id, title, bounty_total, votes, state, (extract(epoch FROM created_at) * 1000)::bigint FROM requests
              WHERE ($1::text IS NULL AND state IN ('open', 'fill_pending') OR state = $1) AND ($2::text IS NULL OR type_id = $2) ORDER BY {order} LIMIT 100"
        ),
        &[status.map_or(SqlValue::Null, |s| SqlValue::Text(s.into())), type_id.map_or(SqlValue::Null, |s| SqlValue::Text(s.into()))],
    )?;
    let items: Vec<Value> = r
        .rows
        .iter()
        .map(|row| json!({ "id": int(&row[0]), "type_id": text(&row[1]), "title": text(&row[2]), "bounty_total": int(&row[3]), "votes": int(&row[4]), "status": text(&row[5]), "created_at": int(&row[6]) }))
        .collect();
    Ok(json!({ "items": items, "currency": currency() }))
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

impl http_handler::Guest for Requests {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let b: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
        match req.route_id.as_str() {
            "list" => Ok(resp(
                200,
                &list(
                    query(&req, "status"),
                    query(&req, "type_id"),
                    query(&req, "sort").unwrap_or("new"),
                )?,
            )),
            "create" => Ok(resp(201, &create(me()?, &b)?)),
            "get" => {
                let rid = id_param(&req)?;
                let mut v = info(rid)?.ok_or_else(|| err("requests.not_found", "request not found"))?;
                let uid = me().ok();
                let cs = sql::query(
                    "SELECT c.user_id, u.username, sum(c.amount)::bigint FROM contributions c LEFT JOIN identity.v_user_public u ON u.id = c.user_id
                      WHERE c.request_id = $1 AND c.state IN ('escrowed', 'paid') GROUP BY 1, 2 ORDER BY 3 DESC",
                    &[SqlValue::Int(rid)],
                )?;
                v["contributors"] = Value::Array(cs.rows.iter().map(|r| json!({ "user_id": int(&r[0]), "username": text(&r[1]), "amount": int(&r[2]) })).collect());
                let fs = sql::query(
                    "SELECT f.id, f.torrent_id, f.filler_id, u.username, f.state, f.reason, f.payout_amount FROM fills f LEFT JOIN identity.v_user_public u ON u.id = f.filler_id
                      WHERE f.request_id = $1 ORDER BY f.id DESC",
                    &[SqlValue::Int(rid)],
                )?;
                v["fills"] = Value::Array(
                    fs.rows
                        .iter()
                        .map(|r| json!({ "id": int(&r[0]), "torrent_id": int(&r[1]), "filler": { "id": int(&r[2]), "username": text(&r[3]) }, "state": text(&r[4]), "reason": text(&r[5]), "payout": int(&r[6]) }))
                        .collect(),
                );
                v["voted"] = json!(uid.is_some_and(|u| {
                    sql::query(
                        "SELECT 1 FROM votes WHERE request_id = $1 AND user_id = $2",
                        &[SqlValue::Int(rid), SqlValue::Int(u)],
                    )
                    .is_ok_and(|r| !r.rows.is_empty())
                }));
                v["can_manage"] = json!(uid.is_some_and(|u| owner_or_mod(rid, u).unwrap_or(false)));
                Ok(resp(200, &v))
            }
            "bounty" => {
                let total = contribute(me()?, id_param(&req)?, b["amount"].as_i64().unwrap_or(0))?;
                Ok(resp(200, &json!({ "bounty_total": total })))
            }
            "vote" | "unvote" => {
                let (rid, uid) = (id_param(&req)?, me()?);
                let n = if req.route_id == "vote" {
                    sql::execute(
                        "INSERT INTO votes (request_id, user_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                        &[SqlValue::Int(rid), SqlValue::Int(uid)],
                    )?
                } else {
                    sql::execute(
                        "DELETE FROM votes WHERE request_id = $1 AND user_id = $2",
                        &[SqlValue::Int(rid), SqlValue::Int(uid)],
                    )?
                };
                if n > 0 {
                    let delta = if req.route_id == "vote" { 1 } else { -1 };
                    sql::execute(
                        "UPDATE requests SET votes = greatest(votes + $2, 0) WHERE id = $1",
                        &[SqlValue::Int(rid), SqlValue::Int(delta)],
                    )?;
                }
                Ok(resp(200, &json!({ "voted": req.route_id == "vote" })))
            }
            "fill" => Ok(resp(
                201,
                &submit_fill(me()?, id_param(&req)?, b["torrent_id"].as_i64().unwrap_or(0))?,
            )),
            "confirm" => {
                let (fid, uid) = (id_param(&req)?, me()?);
                if !owner_or_mod(fill_request(fid)?, uid)? {
                    return Err(err(
                        "kernel.forbidden",
                        "only the requester or a moderator can confirm",
                    ));
                }
                Ok(resp(200, &confirm(fid, &format!("user:{uid}"))?))
            }
            "reject" => {
                let (fid, uid) = (id_param(&req)?, me()?);
                let rid = fill_request(fid)?;
                if !owner_or_mod(rid, uid)? {
                    return Err(err(
                        "kernel.forbidden",
                        "only the requester or a moderator can reject",
                    ));
                }
                let reason: String = b["reason"].as_str().unwrap_or("").chars().take(500).collect();
                let n = sql::execute(
                    "UPDATE fills SET state = 'rejected', reason = $2 WHERE id = $1 AND state = 'pending'",
                    &[SqlValue::Int(fid), SqlValue::Text(reason.clone())],
                )?;
                if n == 0 {
                    return Err(err("requests.fill_not_pending", "the fill is not pending"));
                }
                sql::execute(
                    "UPDATE requests SET state = 'open', updated_at = now() WHERE id = $1 AND state = 'fill_pending'",
                    &[SqlValue::Int(rid)],
                )?;
                publish(
                    "request.fill_rejected",
                    &format!("request:{rid}"),
                    json!({ "request_id": rid, "fill_id": fid, "by": uid, "reason": reason }),
                )?;
                if let Some(filler) =
                    sql::query("SELECT filler_id FROM fills WHERE id = $1", &[SqlValue::Int(fid)])?
                        .rows
                        .first()
                        .and_then(|r| int(&r[0]))
                {
                    notify(
                        filler,
                        "requests.fill_rejected",
                        json!({ "reason": reason }),
                        "你的应求被驳回",
                        &format!("/p/requests/{rid}"),
                        &format!("fill:{fid}:rej"),
                    );
                }
                Ok(resp(200, &json!({ "state": "rejected" })))
            }
            "withdraw" => {
                let (rid, uid) = (id_param(&req)?, me()?);
                let r = sql::query(
                    "SELECT user_id, state FROM requests WHERE id = $1 FOR UPDATE",
                    &[SqlValue::Int(rid)],
                )?;
                let row = r
                    .rows
                    .first()
                    .ok_or_else(|| err("requests.not_found", "request not found"))?;
                if int(&row[0]) != Some(uid) && !has(uid, "requests.moderate") {
                    return Err(err("kernel.forbidden", "only the requester can withdraw"));
                }
                if text(&row[1]).as_deref() != Some("open") {
                    return Err(err(
                        "requests.not_open",
                        "only open requests without a pending fill can be withdrawn",
                    ));
                }
                close_with_refunds(rid, "withdrawn")?;
                Ok(resp(200, &json!({ "state": "closed" })))
            }
            _ => Err(err("kernel.not_found", "no such route")),
        }
    }
}

// ---------------- 定时任务 ----------------

impl on_cron::Guest for Requests {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            "expire" => {
                let r = sql::query(
                    "SELECT id FROM requests WHERE state = 'open' AND expires_at <= now() LIMIT 100",
                    &[],
                )?;
                for rid in r.rows.iter().filter_map(|row| int(&row[0])) {
                    close_with_refunds(rid, "expired")?;
                }
                Ok(())
            }
            "auto_confirm" => {
                let r = sql::query(
                    "SELECT id FROM fills WHERE state = 'pending' AND auto_confirm_at <= now() LIMIT 100",
                    &[],
                )?;
                for fid in r.rows.iter().filter_map(|row| int(&row[0])) {
                    confirm(fid, "system")?;
                }
                Ok(())
            }
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

// ---------------- ctx.requests ----------------

impl service_provider::Guest for Requests {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        let out = match method.as_str() {
            "get" => info(a["id"].as_i64().unwrap_or(0))?.unwrap_or(Value::Null),
            "list_open_by_type" => list(None, a["type_id"].as_str(), "new")?,
            "fill" => {
                if !invocation::current().caller_scopes.iter().any(|s| s == "write") {
                    return Err(err("kernel.forbidden", "scope write required"));
                }
                submit_fill(
                    a["filler_id"].as_i64().unwrap_or(0),
                    a["request_id"].as_i64().unwrap_or(0),
                    a["torrent_id"].as_i64().unwrap_or(0),
                )?
            }
            other => {
                return Err(err(
                    "kernel.not_found",
                    &format!("ctx.requests has no method `{other}`"),
                ));
            }
        };
        Ok(out.to_string())
    }
}

impl mcp_tool::Guest for Requests {
    fn call(tool: String, arguments: String) -> R<String> {
        let a: Value = serde_json::from_str(&arguments).unwrap_or(Value::Null);
        let out = match tool.as_str() {
            "list_requests" => list(
                a["status"].as_str(),
                a["type_id"].as_str(),
                a["sort"].as_str().unwrap_or("new"),
            )?,
            "create_request" => create(me()?, &a)?,
            _ => return Err(err("kernel.not_found", "unknown tool")),
        };
        Ok(out.to_string())
    }
}

export!(Requests);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requirements() {
        let reqs = json!([{ "field": "resolution", "op": "in", "value": ["2160p", "1080p"] }, { "field": "hdr", "op": "in", "value": ["dv"] }, { "field": "year", "op": "gte", "value": 2020 }]);
        let ok = json!({ "resolution": "2160p", "hdr": ["dv", "hdr10"], "year": 2023 });
        assert!(failed_requirements(&reqs, &ok).is_empty());
        let bad = json!({ "resolution": "720p", "hdr": [], "year": 2019 });
        assert_eq!(failed_requirements(&reqs, &bad).len(), 3);
    }
}
