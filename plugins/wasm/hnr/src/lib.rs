//! hnr 插件（WASM，docs/plugins/hnr/）。
//!
//! 本版本实现：下载完成时建档（豁免谓词规则、重下处理）、分层评估（临期每 10 分钟、其余每小时 1/6 轮转）、达标 / 计入 / 预警、
//! 有效计数与 identity 限制（施加 / 解除）、每日过期重算、免除、种子删除 / 下架时豁免、`torrent.before_download` 与
//! `invite.before_issue`、`ctx.hnr`、HTTP。规则编辑接口、批量免除、盒子判定、通知、MCP 在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

wit_bindgen::generate!({
    path: "wit",
    world: "hnr",
    generate_all,
});

use exports::pt::plugin::{hooks, http_handler, on_cron, on_event, service_provider};
use pt::plugin::{
    audit, config, events, invocation, services,
    sql::{self, SqlValue},
    time,
    types::PluginError,
};
use serde::Deserialize;
use serde_json::{Value, json};

struct Hnr;

type R<T> = Result<T, PluginError>;
type Row = Vec<SqlValue>;

fn err(code: &str, message: &str) -> PluginError {
    PluginError {
        code: code.into(),
        message: message.into(),
        details: None,
        retryable: false,
    }
}

fn q(stmt: &str, params: &[SqlValue]) -> R<Vec<Row>> {
    Ok(sql::query(stmt, params)?.rows)
}

fn x(stmt: &str, params: &[SqlValue]) -> R<u64> {
    sql::execute(stmt, params)
}

fn int(v: &SqlValue) -> i64 {
    match v {
        SqlValue::Int(i) => *i,
        SqlValue::Numeric(s) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

fn oint(v: &SqlValue) -> Option<i64> {
    match v {
        SqlValue::Int(i) => Some(*i),
        _ => None,
    }
}

fn num(v: &SqlValue) -> Option<f64> {
    match v {
        SqlValue::Float(f) => Some(*f),
        SqlValue::Int(i) => Some(*i as f64),
        _ => None,
    }
}

fn text(v: &SqlValue) -> String {
    match v {
        SqlValue::Text(s) => s.clone(),
        _ => String::new(),
    }
}

fn otext(v: &SqlValue) -> Option<String> {
    match v {
        SqlValue::Text(s) => Some(s.clone()),
        _ => None,
    }
}

fn t(s: &str) -> SqlValue {
    SqlValue::Text(s.into())
}

fn i(v: i64) -> SqlValue {
    SqlValue::Int(v)
}

fn ms(col: &str) -> String {
    format!("(extract(epoch FROM {col}) * 1000)::bigint")
}

fn publish(event_type: &str, user: i64, payload: Value) -> R<()> {
    events::publish(&events::NewEvent {
        event_type: event_type.into(),
        version: 1,
        key: format!("user:{user}"),
        payload: payload.to_string(),
    })?;
    Ok(())
}

fn svc(service: &str, method: &str, args: &Value) -> R<Value> {
    let out = services::call(service, method, &args.to_string())?;
    serde_json::from_str(&out).map_err(|_| err("hnr.bad_service_reply", "invalid JSON from service"))
}

// ---------------- 设置（SRS §5、LLD §1.1） ----------------

#[derive(Debug, Clone, Deserialize)]
struct Pred {
    field: String,
    op: String,
    value: Value,
}

#[derive(Debug, Clone, Deserialize)]
struct ExemptRule {
    id: String,
    when: Vec<Pred>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct Settings {
    window_days: i64,
    min_seed_hours: i64,
    min_torrent_ratio: f64,
    grace_hours: i64,
    prewarn_hours: i64,
    expire_days: i64,
    max_hnr: i64,
    invite_block_at: i64,
    exempt_rules: Vec<ExemptRule>,
    redownload_mode: String,
}

impl Default for Settings {
    fn default() -> Self {
        let rule = |id: &str, field: &str, op: &str, value: Value| ExemptRule {
            id: id.into(),
            when: vec![Pred {
                field: field.into(),
                op: op.into(),
                value,
            }],
        };
        Self {
            window_days: 14,
            min_seed_hours: 72,
            min_torrent_ratio: 1.0,
            grace_hours: 0,
            prewarn_hours: 48,
            expire_days: 90,
            max_hnr: 5,
            invite_block_at: 1,
            exempt_rules: vec![
                rule("small", "size", "lt", json!(524_288_000)),
                rule("own_upload", "is_uploader", "eq", json!(true)),
                rule("partial", "downloaded_pct", "lt", json!(0.1)),
            ],
            redownload_mode: "ignore".into(),
        }
    }
}

fn settings() -> Settings {
    let s: Settings = serde_json::from_str(&config::get_settings()).unwrap_or_default();
    Settings {
        window_days: s.window_days.clamp(1, 60),
        min_seed_hours: s.min_seed_hours.clamp(0, 720),
        expire_days: s.expire_days.clamp(1, 365),
        max_hnr: s.max_hnr.clamp(1, 1000),
        ..s
    }
}

fn cmp(a: &Value, op: &str, b: &Value) -> bool {
    match op {
        "eq" => a == b || (a.as_f64().is_some() && a.as_f64() == b.as_f64()),
        "ne" => a != b,
        "in" => b.as_array().is_some_and(|l| {
            l.contains(a) || a.as_array().is_some_and(|xs| xs.iter().any(|x| l.contains(x)))
        }),
        "contains" => a.as_array().is_some_and(|l| l.contains(b)),
        _ => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => match op {
                "lt" => x < y,
                "lte" => x <= y,
                "gt" => x > y,
                "gte" => x >= y,
                _ => false,
            },
            _ => false,
        },
    }
}

/// 第一条成立的豁免规则
fn exempt_by(rules: &[ExemptRule], ctx: &Value) -> Option<String> {
    rules
        .iter()
        .find(|r| !r.when.is_empty() && r.when.iter().all(|p| cmp(&ctx[&p.field], &p.op, &p.value)))
        .map(|r| r.id.clone())
}

// ---------------- 建档（LLD §1.2） ----------------

/// 豁免判定上下文（字段白名单见 LLD §1.1）
fn exempt_ctx(uid: i64, tid: i64) -> R<Option<Value>> {
    let tr = q(
        "SELECT size, type_id, uploader_id FROM torrent.v_torrent_public WHERE id = $1",
        &[i(tid)],
    )?;
    let Some(tr) = tr.first() else { return Ok(None) };
    let size = int(&tr[0]);
    let sn = q(
        "SELECT downloaded FROM accounting.v_snatch WHERE user_id = $1 AND torrent_id = $2",
        &[i(uid), i(tid)],
    )?;
    let downloaded = sn.first().map_or(0, |r| int(&r[0]));
    let u = q(
        "SELECT class_id, extract(epoch FROM now() - created_at)::float8 / 86400 FROM identity.v_user_public WHERE id = $1",
        &[i(uid)],
    )?;
    // 促销（可选依赖）：以下载完成时的状态为准
    let promo = svc("ctx.promotions", "current", &json!({ "torrent_id": tid }))
        .ok()
        .and_then(|v| v["primary"]["kind"].as_str().map(str::to_owned));
    Ok(Some(json!({
        "size": size, "type_id": text(&tr[1]), "tags": [], "promotion_kind": promo,
        "class_id": u.first().map(|r| int(&r[0])), "is_uploader": oint(&tr[2]) == Some(uid), "is_seedbox": false,
        "downloaded_pct": if size > 0 { downloaded as f64 / size as f64 } else { 0.0 },
        "account_age_days": u.first().and_then(|r| num(&r[1])).map(f64::floor),
    })))
}

fn bump_counts(uid: i64, d_active: i64, d_failed: i64) -> R<()> {
    x(
        "INSERT INTO user_counts (user_id, active, failed_effective) VALUES ($1, GREATEST($2, 0), GREATEST($3, 0))
         ON CONFLICT (user_id) DO UPDATE SET active = GREATEST(user_counts.active + $2, 0),
                failed_effective = GREATEST(user_counts.failed_effective + $3, 0), updated_at = now()",
        &[i(uid), i(d_active), i(d_failed)],
    )?;
    Ok(())
}

fn on_snatch_completed(p: &Value) -> R<()> {
    let (uid, tid) = (
        p["user_id"].as_i64().unwrap_or(0),
        p["torrent_id"].as_i64().unwrap_or(0),
    );
    let completed = p["completed_at"].as_str().unwrap_or("").to_owned();
    let s = settings();
    let last = q(
        "SELECT state FROM records WHERE user_id = $1 AND torrent_id = $2 ORDER BY completed_at DESC LIMIT 1",
        &[i(uid), i(tid)],
    )?;
    match last.first().map(|r| text(&r[0])).as_deref() {
        Some("active" | "satisfied" | "exempted" | "forgiven") => return Ok(()),
        Some("failed") if s.redownload_mode != "new_record" => return Ok(()),
        _ => {}
    }
    let Some(ctx) = exempt_ctx(uid, tid)? else {
        return Ok(());
    };
    let rule = exempt_by(&s.exempt_rules, &ctx);
    let r = q(
        "INSERT INTO records (user_id, torrent_id, completed_at, deadline, state, exempt_rule, resolved_at)
         VALUES ($1, $2, $3::timestamptz, $3::timestamptz + make_interval(days => $4::int), $5, $6,
                 CASE WHEN $5 = 'exempted' THEN now() END)
         ON CONFLICT (user_id, torrent_id, completed_at) DO NOTHING
         RETURNING id, (extract(epoch FROM deadline) * 1000)::bigint",
        &[
            i(uid),
            i(tid),
            t(&completed),
            i(s.window_days),
            t(if rule.is_some() { "exempted" } else { "active" }),
            rule.as_deref().map_or(SqlValue::Null, t),
        ],
    )?;
    let Some(row) = r.first() else { return Ok(()) };
    let id = int(&row[0]);
    match rule {
        Some(rule) => publish(
            "hnr.exempted",
            uid,
            json!({ "record_id": id, "user_id": uid, "torrent_id": tid, "rule": rule }),
        ),
        None => {
            bump_counts(uid, 1, 0)?;
            publish(
                "hnr.recorded",
                uid,
                json!({ "record_id": id, "user_id": uid, "torrent_id": tid, "deadline": int(&row[1]) }),
            )
        }
    }
}

// ---------------- 评估（LLD §1.3） ----------------

const BATCH: i64 = 5000;

fn evaluate(tier: &str) -> R<usize> {
    let s = settings();
    let hour = time::now().div_euclid(3_600_000) % 6;
    let filter = if tier == "hot" {
        "state = 'active' AND deadline < now() + interval '48 hours'"
    } else {
        "state = 'active' AND deadline >= now() + interval '48 hours' AND abs(hashtext(user_id::text)) % 6 = $2"
    };
    let mut done = 0;
    let mut after = 0i64;
    loop {
        let mut params = vec![i(after)];
        if tier != "hot" {
            params.push(i(hour));
        }
        params.push(i(BATCH));
        let lim = params.len();
        let rows = q(
            &format!(
                "SELECT r.id, r.user_id, r.torrent_id, r.completed_at::text, {}, r.prewarned, coalesce(tr.size, 0),
                        coalesce(sn.uploaded, 0), coalesce(sn.downloaded, 0), coalesce(sn.seed_secs, 0)
                   FROM records r
                   LEFT JOIN accounting.v_snatch sn ON sn.user_id = r.user_id AND sn.torrent_id = r.torrent_id
                   LEFT JOIN torrent.v_torrent_public tr ON tr.id = r.torrent_id
                  WHERE r.{filter} AND r.id > $1 ORDER BY r.id LIMIT ${lim} FOR UPDATE OF r SKIP LOCKED",
                ms("r.deadline")
            ),
            &params,
        )?;
        let Some(last) = rows.last().map(|r| int(&r[0])) else {
            return Ok(done);
        };
        for r in &rows {
            eval_one(r, &s)?;
        }
        done += rows.len();
        after = last;
        sql::commit()?;
    }
}

fn eval_one(r: &Row, s: &Settings) -> R<()> {
    let (id, uid, tid, completed, deadline) = (int(&r[0]), int(&r[1]), int(&r[2]), text(&r[3]), int(&r[4]));
    let (size, up, down, seed) = (int(&r[6]), int(&r[7]), int(&r[8]), int(&r[9]));
    // 分母取 max(下载量, 体积)，部分下载不能以小分母达标
    let base = down.max(size).max(1);
    let ratio = up as f64 / base as f64;
    let ok = seed >= s.min_seed_hours * 3600 || (s.min_torrent_ratio > 0.0 && ratio >= s.min_torrent_ratio);
    let now = time::now();
    let key = [t(&completed), i(id)];
    let snap = |state_sql: &str| -> R<()> {
        x(
            &format!(
                "UPDATE records SET seed_secs_snapshot = $3, ratio_snapshot = $4{state_sql} WHERE completed_at = $1::timestamptz AND id = $2"
            ),
            &[key[0].clone(), key[1].clone(), i(seed), SqlValue::Float(ratio)],
        )?;
        Ok(())
    };
    if ok {
        snap(", state = 'satisfied', resolved_at = now()")?;
        bump_counts(uid, -1, 0)?;
        publish(
            "hnr.satisfied",
            uid,
            json!({ "record_id": id, "user_id": uid, "torrent_id": tid }),
        )?;
    } else if now >= deadline + s.grace_hours * 3_600_000 {
        snap(", state = 'failed', failed_at = now(), resolved_at = now()")?;
        bump_counts(uid, -1, 0)?;
        let n = recount(uid, s)?;
        publish(
            "hnr.failed",
            uid,
            json!({ "record_id": id, "user_id": uid, "torrent_id": tid, "failed_effective": n }),
        )?;
    } else if deadline - now <= s.prewarn_hours * 3_600_000 && !matches!(r[5], SqlValue::Boolean(true)) {
        snap(", prewarned = true")?;
        publish(
            "hnr.prewarned",
            uid,
            json!({ "record_id": id, "user_id": uid, "torrent_id": tid, "deadline": deadline, "seed_secs": seed,
                    "required_seed_secs": s.min_seed_hours * 3600 }),
        )?;
    } else {
        snap("")?;
    }
    Ok(())
}

// ---------------- 处罚（LLD §1.4） ----------------

/// 重算有效计入数，越线施加 / 解除 identity 限制
fn recount(uid: i64, s: &Settings) -> R<i64> {
    let n = int(&q(
        "SELECT count(*) FROM records WHERE user_id = $1 AND state = 'failed' AND failed_at > now() - make_interval(days => $2::int)",
        &[i(uid), i(s.expire_days)],
    )?[0][0]);
    x(
        "INSERT INTO user_counts (user_id, failed_effective) VALUES ($1, $2)
         ON CONFLICT (user_id) DO UPDATE SET failed_effective = $2, updated_at = now()",
        &[i(uid), i(n)],
    )?;
    let restricted = q(
        "SELECT restricted FROM user_counts WHERE user_id = $1 FOR UPDATE",
        &[i(uid)],
    )?;
    let restricted = matches!(restricted.first().map(|r| &r[0]), Some(SqlValue::Boolean(true)));
    if n >= s.max_hnr && !restricted {
        match svc(
            "ctx.users",
            "apply_restriction",
            &json!({ "user": uid, "kind": "leech_disabled", "reason_code": "hnr.too_many" }),
        ) {
            Ok(_) => {
                x(
                    "UPDATE user_counts SET restricted = true, restricted_seq = restricted_seq + 1, restricted_skipped = false WHERE user_id = $1",
                    &[i(uid)],
                )?;
            }
            // 员工受保护：记录跳过，下载钩子仍会拦截
            Err(e) if e.code == "identity.target_protected" => {
                x(
                    "UPDATE user_counts SET restricted_skipped = true WHERE user_id = $1",
                    &[i(uid)],
                )?;
            }
            Err(e) => return Err(e),
        }
    } else if n < s.max_hnr && restricted {
        svc(
            "ctx.users",
            "lift_restriction",
            &json!({ "user": uid, "kind": "leech_disabled", "reason": "hnr.below_limit" }),
        )?;
        x(
            "UPDATE user_counts SET restricted = false WHERE user_id = $1",
            &[i(uid)],
        )?;
    }
    Ok(n)
}

fn expire_counts() -> R<()> {
    let s = settings();
    let mut after = 0i64;
    loop {
        let rows = q(
            "SELECT user_id FROM user_counts WHERE (failed_effective > 0 OR restricted) AND user_id > $1 ORDER BY user_id LIMIT 500",
            &[i(after)],
        )?;
        let Some(last) = rows.last().map(|r| int(&r[0])) else {
            return Ok(());
        };
        for r in &rows {
            recount(int(&r[0]), &s)?;
        }
        after = last;
        sql::commit()?;
    }
}

// ---------------- 免除（LLD §1.5） ----------------

fn forgive(id: i64, reason: &str, by: &str) -> R<()> {
    if reason.trim().is_empty() || reason.chars().count() > 500 {
        return Err(err(
            "kernel.validation_failed",
            "reason is required (≤ 500 characters)",
        ));
    }
    let r = q(
        "SELECT user_id, state, completed_at::text FROM records WHERE id = $1 FOR UPDATE",
        &[i(id)],
    )?;
    let row = r
        .first()
        .ok_or_else(|| err("hnr.record_not_found", "record not found"))?;
    let (uid, prev) = (int(&row[0]), text(&row[1]));
    if !matches!(prev.as_str(), "failed" | "active") {
        return Err(err("hnr.not_forgivable", &format!("record is {prev}")));
    }
    x(
        "UPDATE records SET state = 'forgiven', forgiven_by = $3, forgive_reason = $4, resolved_at = now()
          WHERE completed_at = $1::timestamptz AND id = $2",
        &[t(&text(&row[2])), i(id), t(by), t(reason)],
    )?;
    if prev == "active" {
        bump_counts(uid, -1, 0)?;
    }
    publish(
        "hnr.forgiven",
        uid,
        json!({ "record_id": id, "user_id": uid, "by": by, "reason": reason }),
    )?;
    if prev == "failed" {
        recount(uid, &settings())?;
    }
    Ok(())
}

// ---------------- 查询 ----------------

fn status(uid: i64) -> R<Value> {
    let s = settings();
    let r = q(
        "SELECT active, failed_effective FROM user_counts WHERE user_id = $1",
        &[i(uid)],
    )?;
    let (active, failed) = r.first().map_or((0, 0), |r| (int(&r[0]), int(&r[1])));
    Ok(
        json!({ "active": active, "failed": failed, "max": s.max_hnr, "can_download": failed < s.max_hnr,
               "rules": { "window_days": s.window_days, "min_seed_hours": s.min_seed_hours, "min_torrent_ratio": s.min_torrent_ratio } }),
    )
}

fn records(uid: i64, state: Option<&str>, cursor: Option<&str>) -> R<Value> {
    let s = settings();
    let before = cursor.and_then(|c| c.parse::<i64>().ok()).unwrap_or(i64::MAX);
    let rows = q(
        &format!(
            "SELECT id, torrent_id, state, {}, {}, seed_secs_snapshot, ratio_snapshot, exempt_rule
               FROM records WHERE user_id = $1 AND ($2::text IS NULL OR state = $2) AND id < $3 ORDER BY id DESC LIMIT 51",
            ms("completed_at"),
            ms("deadline")
        ),
        &[i(uid), state.map_or(SqlValue::Null, t), i(before)],
    )?;
    let items: Vec<Value> = rows
        .iter()
        .take(50)
        .map(|r| {
            json!({ "id": int(&r[0]), "torrent_id": int(&r[1]), "state": text(&r[2]), "completed_at": int(&r[3]),
                    "deadline": int(&r[4]), "seed_secs": int(&r[5]), "required_seed_secs": s.min_seed_hours * 3600,
                    "ratio": num(&r[6]), "required_ratio": (s.min_torrent_ratio > 0.0).then_some(s.min_torrent_ratio),
                    "exempt_rule": otext(&r[7]) })
        })
        .collect();
    let next = (rows.len() > 50)
        .then(|| items.last().map(|i| i["id"].to_string()))
        .flatten();
    Ok(json!({ "items": items, "next_cursor": next }))
}

fn exempt_torrent(tid: i64) -> R<()> {
    let rows = q(
        "UPDATE records SET state = 'exempted', exempt_rule = 'torrent_removed', resolved_at = now()
          WHERE torrent_id = $1 AND state = 'active' RETURNING user_id",
        &[i(tid)],
    )?;
    for r in rows {
        bump_counts(int(&r[0]), -1, 0)?;
    }
    Ok(())
}

// ---------------- 导出 ----------------

impl hooks::Guest for Hnr {
    fn handle(c: hooks::HookCall) -> R<hooks::HookDecision> {
        let input: Value = serde_json::from_str(&c.input).unwrap_or(Value::Null);
        let s = settings();
        let failed = |uid: i64| -> R<i64> {
            Ok(q(
                "SELECT failed_effective FROM user_counts WHERE user_id = $1",
                &[i(uid)],
            )?
            .first()
            .map_or(0, |r| int(&r[0])))
        };
        let deny = |code: &str, msg: &str, n: i64| {
            hooks::HookDecision::Deny(hooks::DenyInfo {
                code: code.into(),
                message: msg.into(),
                details: Some(json!({ "failed": n, "max": s.max_hnr }).to_string()),
            })
        };
        match c.hook.as_str() {
            "torrent.before_download" => {
                let n = failed(input["user_id"].as_i64().unwrap_or(0))?;
                if n >= s.max_hnr {
                    return Ok(deny(
                        "hnr.download_restricted",
                        "too many H&R violations; downloads are disabled",
                        n,
                    ));
                }
            }
            "invite.before_issue" => {
                let n = failed(input["inviter_id"].as_i64().unwrap_or(0))?;
                if s.invite_block_at > 0 && n >= s.invite_block_at {
                    return Ok(deny(
                        "hnr.restricted",
                        "you have unresolved H&R violations and cannot send invites",
                        n,
                    ));
                }
            }
            _ => {}
        }
        Ok(hooks::HookDecision::Allow)
    }
}

impl on_event::Guest for Hnr {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        let p: Value =
            serde_json::from_str(&evt.payload).map_err(|_| err("hnr.bad_event", "invalid payload"))?;
        let tid = p["torrent_id"].as_i64().unwrap_or(0);
        match evt.event_type.as_str() {
            "traffic.snatch_completed" => on_snatch_completed(&p),
            "torrent.deleted" => exempt_torrent(tid),
            "torrent.status_changed" if matches!(p["to"].as_str(), Some("deleted" | "disabled")) => {
                exempt_torrent(tid)
            }
            _ => Ok(()),
        }
    }
}

impl on_cron::Guest for Hnr {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            "evaluate_hot" => evaluate("hot").map(|_| ()),
            "evaluate_cold" => evaluate("cold").map(|_| ()),
            "expire_counts" => expire_counts(),
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

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

fn param(req: &http_handler::Request, k: &str) -> R<i64> {
    req.params
        .iter()
        .find(|(n, _)| n == k)
        .and_then(|(_, v)| v.parse().ok())
        .ok_or_else(|| err("kernel.bad_request", "bad path parameter"))
}

fn query<'a>(req: &'a http_handler::Request, k: &str) -> Option<&'a str> {
    req.query.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

fn me() -> R<i64> {
    invocation::current()
        .actor
        .user_id
        .and_then(|u| i64::try_from(u).ok())
        .ok_or_else(|| err("kernel.unauthenticated", "login required"))
}

impl http_handler::Guest for Hnr {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let v = match req.route_id.as_str() {
            "me" => status(me()?)?,
            "my_records" => records(me()?, query(&req, "state"), query(&req, "cursor"))?,
            "user_records" => {
                let uid = param(&req, "id")?;
                let mut v = records(uid, query(&req, "state"), query(&req, "cursor"))?;
                v["status"] = status(uid)?;
                v
            }
            "forgive" => {
                let id = param(&req, "id")?;
                let b: Value = serde_json::from_slice(&req.body)
                    .map_err(|_| err("kernel.bad_request", "invalid JSON body"))?;
                let reason = b["reason"].as_str().unwrap_or("");
                forgive(id, reason, &format!("user:{}", me()?))?;
                audit::write(&audit::AuditEntry {
                    action: "hnr.forgive".into(),
                    target: format!("hnr_record:{id}"),
                    details: json!({ "reason": reason }).to_string(),
                })?;
                return Ok(http_handler::Response {
                    status: 204,
                    headers: vec![],
                    body: vec![],
                });
            }
            "requirement" => {
                let tid = param(&req, "id")?;
                let s = settings();
                let ctx =
                    exempt_ctx(me()?, tid)?.ok_or_else(|| err("kernel.not_found", "torrent not found"))?;
                // 预判：按体积下载（downloaded_pct 视为 1）
                let mut c = ctx;
                c["downloaded_pct"] = json!(1.0);
                let rule = exempt_by(&s.exempt_rules, &c);
                json!({ "required": rule.is_none(), "exempt_rule": rule, "min_seed_hours": s.min_seed_hours,
                        "min_torrent_ratio": s.min_torrent_ratio, "window_days": s.window_days })
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(resp(200, &v))
    }
}

impl service_provider::Guest for Hnr {
    fn call(service: String, method: String, args: String) -> R<String> {
        if service != "ctx.hnr" {
            return Err(err("kernel.not_found", "unknown service"));
        }
        let a: Value =
            serde_json::from_str(&args).map_err(|_| err("kernel.bad_request", "invalid JSON arguments"))?;
        let user = a["user"].as_i64().or_else(|| a["user_id"].as_i64()).unwrap_or(0);
        let inv = invocation::current();
        let out = match method.as_str() {
            "status" => status(user)?,
            "records" => records(user, a["filter"]["state"].as_str(), a["cursor"].as_str())?,
            "forgive" => {
                if !inv.caller_scopes.iter().any(|s| s == "forgive") {
                    return Err(err("kernel.forbidden", "scope `forgive` required"));
                }
                let key = a["idempotency_key"]
                    .as_str()
                    .filter(|k| (1..=128).contains(&k.len()))
                    .ok_or_else(|| err("kernel.validation_failed", "idempotency_key"))?;
                let fresh = x(
                    "INSERT INTO idempotency (caller, key) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                    &[t(&inv.caller_plugin), t(key)],
                )?;
                if fresh == 1 {
                    forgive(
                        a["record_id"].as_i64().unwrap_or(0),
                        a["reason"].as_str().unwrap_or(""),
                        &format!("plugin:{}", inv.caller_plugin),
                    )?;
                }
                Value::Null
            }
            _ => return Err(err("kernel.not_found", "unknown method")),
        };
        Ok(out.to_string())
    }
}

export!(Hnr);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exemptions() {
        let s = Settings::default();
        let base = json!({ "size": 2_000_000_000_i64, "is_uploader": false, "downloaded_pct": 1.0, "promotion_kind": "free" });
        assert_eq!(exempt_by(&s.exempt_rules, &base), None);
        let mut small = base.clone();
        small["size"] = json!(100);
        assert_eq!(exempt_by(&s.exempt_rules, &small).as_deref(), Some("small"));
        let mut own = base.clone();
        own["is_uploader"] = json!(true);
        assert_eq!(exempt_by(&s.exempt_rules, &own).as_deref(), Some("own_upload"));
        let rules = vec![ExemptRule {
            id: "freeleech".into(),
            when: vec![Pred {
                field: "promotion_kind".into(),
                op: "in".into(),
                value: json!(["free", "double_up_free"]),
            }],
        }];
        assert_eq!(exempt_by(&rules, &base).as_deref(), Some("freeleech"));
        let none = json!({ "promotion_kind": null });
        assert_eq!(exempt_by(&rules, &none), None);
    }
}
