//! newbie-exam 插件（WASM，docs/plugins/newbie-exam/）：新手考核。
//!
//! - 考核定义在插件设置中：1～5 个阶段（距分配的天数严格递增），每阶段 1～8 个目标；指标为考核期间的增量
//!   （`uploaded_gib`、`downloaded_gib`、`seed_hours`、`uploads_count`、`bonus`），`ratio` 取当前值。
//! - 新用户按注册方式自动分配（`user.registered`，默认不分配，站点在设置中开启），管理员也可批量分配；豁免等级不参加。
//! - 评估：`traffic.settled` 标脏 + 每 20 分钟（脏用户与即将到期的考核）+ 每小时全量。阶段截止前预警；
//!   未完成按 `on_fail` 处理：不处理 / 经 identity 禁止下载 / 封禁；禁止下载的可在补救期内完成最后阶段目标后自动解除。
//! - 通过可奖励魔力值（`ctx.ledger` 发行，幂等）。勋章奖励等 medals 插件实现后接入。
//! - 钩子 `invite.before_issue`：考核中或考核失败的用户不能发邀请（可选）。

pt_plugin_sdk::world!("newbie-exam", http);

use std::collections::BTreeMap;

use exports::pt::plugin::{hooks, http_handler, on_cron, on_event, service_provider};
use pt_plugin_sdk::{audit, prelude::*, serde_json, services, sql, time};

struct Exam;

const GIB: f64 = 1_073_741_824.0;
const DAY_MS: i64 = 86_400_000;
pub const METRICS: &[&str] = &["uploaded_gib", "downloaded_gib", "seed_hours", "uploads_count", "bonus", "ratio"];

// ---------------- 定义 ----------------

#[derive(Debug, Clone, PartialEq)]
pub struct Goal {
    pub metric: String,
    pub gte: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stage {
    pub days: i64,
    pub goals: Vec<Goal>,
}

#[derive(Debug, Clone)]
pub struct Def {
    pub name: String,
    pub stages: Vec<Stage>,
    pub on_fail: String,
    pub remedy_days: i64,
    pub reward_bonus: i64,
}

/// 校验并解析考核定义（设置中的 `stages` 等字段，或分配时的快照）
pub fn parse_def(v: &Value) -> Result<Def, String> {
    let stages: Vec<Stage> = v["stages"]
        .as_array()
        .ok_or("stages is required")?
        .iter()
        .map(|s| {
            let goals = s["goals"]
                .as_array()
                .ok_or("each stage needs goals")?
                .iter()
                .map(|g| {
                    let metric = g["metric"].as_str().ok_or("goal metric")?.to_owned();
                    if !METRICS.contains(&metric.as_str()) {
                        return Err(format!("unknown metric `{metric}`"));
                    }
                    let gte = g["gte"].as_f64().filter(|x| x.is_finite() && *x >= 0.0).ok_or("goal gte")?;
                    Ok(Goal { metric, gte })
                })
                .collect::<Result<Vec<_>, String>>()?;
            if goals.is_empty() || goals.len() > 8 {
                return Err("each stage needs 1 to 8 goals".to_owned());
            }
            Ok(Stage { days: s["days"].as_i64().ok_or("stage days")?, goals })
        })
        .collect::<Result<_, String>>()?;
    if stages.is_empty() || stages.len() > 5 {
        return Err("1 to 5 stages".into());
    }
    let mut prev = 0;
    for s in &stages {
        if s.days <= prev || s.days > 365 {
            return Err("stage days must increase and be at most 365".into());
        }
        prev = s.days;
    }
    let on_fail = v["on_fail"].as_str().unwrap_or("restrict").to_owned();
    if !matches!(on_fail.as_str(), "none" | "restrict" | "ban") {
        return Err("on_fail must be none, restrict or ban".into());
    }
    Ok(Def {
        name: v["name"].as_str().unwrap_or("新手考核").to_owned(),
        stages,
        on_fail,
        remedy_days: v["remedy_days"].as_i64().unwrap_or(7).max(0),
        reward_bonus: v["reward_bonus"].as_i64().unwrap_or(0).max(0),
    })
}

fn def_json(d: &Def) -> Value {
    json!({ "name": d.name, "on_fail": d.on_fail, "remedy_days": d.remedy_days, "reward_bonus": d.reward_bonus,
            "stages": d.stages.iter().map(|s| json!({ "days": s.days,
                "goals": s.goals.iter().map(|g| json!({ "metric": g.metric, "gte": g.gte })).collect::<Vec<_>>() })).collect::<Vec<_>>() })
}

struct Settings {
    def: Result<Def, String>,
    auto_modes: Vec<String>,
    exempt_class_ids: Vec<i64>,
    prewarn_hours: i64,
    block_invite: bool,
    notify: bool,
    allow_ban: bool,
}

fn settings() -> Settings {
    let s = ctx::settings();
    let mut d = s.clone();
    if d["stages"].is_null() {
        // 默认：30 天内上传 10 GiB、做种 72 小时
        d["stages"] = json!([{ "days": 30, "goals": [{ "metric": "uploaded_gib", "gte": 10 }, { "metric": "seed_hours", "gte": 72 }] }]);
    }
    let allow_ban = s["allow_ban"].as_bool().unwrap_or(false);
    let def = parse_def(&d).and_then(|x| {
        if x.on_fail == "ban" && !allow_ban { Err("on_fail = ban requires allow_ban".into()) } else { Ok(x) }
    });
    Settings {
        def,
        // 默认不自动分配：站点在设置中选择注册方式后才生效
        auto_modes: s["auto_assign_modes"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default(),
        exempt_class_ids: s["exempt_class_ids"].as_array().map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default(),
        prewarn_hours: s["prewarn_hours"].as_i64().unwrap_or(48).max(0),
        block_invite: s["block_invite_during_exam"].as_bool().unwrap_or(true),
        notify: s["notify"].as_bool().unwrap_or(true),
        allow_ban,
    }
}

// ---------------- 指标 ----------------

type Metrics = BTreeMap<String, f64>;

fn snapshot(uid: i64) -> R<Metrics> {
    let r = sql::query(
        "SELECT coalesce(t.uploaded, 0), coalesce(t.downloaded, 0), coalesce(t.seed_secs, 0), coalesce(c.approved_count, 0)
           FROM identity.v_user_public u
           LEFT JOIN accounting.v_user_totals t ON t.user_id = u.id
           LEFT JOIN torrent.v_uploader_counts c ON c.user_id = u.id
          WHERE u.id = $1",
        &[SqlValue::Int(uid)],
    )?;
    let row = r.rows.first();
    #[allow(clippy::cast_precision_loss)]
    let num = |i: usize| row.and_then(|x| x.int(i)).unwrap_or(0) as f64;
    let (up, down) = (num(0), num(1));
    let mut m = Metrics::new();
    m.insert("uploaded_gib".into(), up / GIB);
    m.insert("downloaded_gib".into(), down / GIB);
    m.insert("seed_hours".into(), num(2) / 3600.0);
    m.insert("uploads_count".into(), num(3));
    m.insert("ratio".into(), if down > 0.0 { up / down } else { f64::INFINITY });
    let bonus = services::call("ctx.ledger", "balance", &json!({ "currency": "bonus", "user": uid }).to_string())
        .ok()
        .and_then(|v| serde_json::from_str::<Value>(&v).ok())
        .and_then(|v| v.as_f64().or_else(|| v["available"].as_f64()).or_else(|| v["balance"].as_f64()))
        .unwrap_or(0.0);
    #[allow(clippy::cast_precision_loss)]
    m.insert("bonus".into(), bonus / ctx::currency_unit("bonus") as f64);
    Ok(m)
}

/// 考核期间的进度：ratio 取当前值，其余为相对基线的增量（下限 0）
pub fn progress(metric: &str, cur: &Metrics, base: &Value) -> f64 {
    let c = cur.get(metric).copied().unwrap_or(0.0);
    if metric == "ratio" {
        return c;
    }
    (c - base[metric].as_f64().unwrap_or(0.0)).max(0.0)
}

fn baseline_json(m: &Metrics) -> Value {
    let mut o = serde_json::Map::new();
    for (k, v) in m {
        if v.is_finite() {
            o.insert(k.clone(), json!(v));
        }
    }
    Value::Object(o)
}

// ---------------- 分配 ----------------

fn publish(event: &str, uid: i64, payload: &Value) -> R<()> {
    ctx::publish(event, &format!("user:{uid}"), payload)?;
    Ok(())
}

fn notify(s: &Settings, uid: i64, key: &str, title: &str, body: &str, idem: &str) {
    if !s.notify {
        return;
    }
    let _ = services::call(
        "ctx.notify",
        "send",
        &json!({ "user_id": uid, "template_key": format!("newbie-exam.{key}"), "category": "account", "title": title,
                 "body": body, "link": "/p/newbie-exam/", "idempotency_key": idem })
        .to_string(),
    );
}

/// 分配考核；返回分配 ID（豁免时为 None）
fn assign(uid: i64, by: &str, s: &Settings) -> R<Option<i64>> {
    let def = s.def.clone().map_err(|e| err("newbie-exam.definition_invalid", &e))?;
    let class: Option<i64> = sql::query("SELECT class_id FROM identity.v_user_public WHERE id = $1", &[SqlValue::Int(uid)])?
        .rows
        .first()
        .and_then(|r| r.int(0));
    let Some(class) = class else {
        return Err(err("kernel.not_found", "user not found"));
    };
    let open = sql::query(
        "SELECT 1 FROM assignments WHERE user_id = $1 AND state IN ('active', 'failed_restricted')",
        &[SqlValue::Int(uid)],
    )?;
    if !open.rows.is_empty() {
        return Err(err("newbie-exam.already_assigned", "the user already has an exam"));
    }
    if s.exempt_class_ids.contains(&class) {
        sql::execute(
            "INSERT INTO assignments (user_id, state, definition_snapshot, assigned_by, reason, result_at) VALUES ($1, 'exempted', $2, $3, 'class', now())",
            &[SqlValue::Int(uid), SqlValue::Json(def_json(&def).to_string()), SqlValue::Text(by.into())],
        )?;
        publish("exam.exempted", uid, &json!({ "user_id": uid, "reason": "class" }))?;
        return Ok(None);
    }
    let base = baseline_json(&snapshot(uid)?);
    let deadline = time::now() + def.stages[0].days * DAY_MS;
    let id = sql::query(
        "INSERT INTO assignments (user_id, state, baseline, definition_snapshot, stage_deadline, assigned_by)
         VALUES ($1, 'active', $2, $3, to_timestamp($4::float8 / 1000), $5) RETURNING id",
        &[
            SqlValue::Int(uid),
            SqlValue::Json(base.to_string()),
            SqlValue::Json(def_json(&def).to_string()),
            SqlValue::Int(deadline),
            SqlValue::Text(by.into()),
        ],
    )?
    .rows
    .first()
    .and_then(|r| r.int(0))
    .ok_or_else(|| err("newbie-exam.internal", "insert failed"))?;
    let last = time::now() + def.stages.last().map_or(0, |x| x.days) * DAY_MS;
    publish("exam.assigned", uid, &json!({ "user_id": uid, "assignment_id": id, "deadline": last }))?;
    notify(
        s,
        uid,
        "assigned",
        "新手考核 / Newbie exam",
        &format!("欢迎加入！请在 {} 天内完成新手考核目标，进度见“新手考核”页面。\nWelcome! Complete the newbie exam within {} days; see the exam page for your progress.",
            def.stages.last().map_or(0, |x| x.days), def.stages.last().map_or(0, |x| x.days)),
        &format!("exam:n:{id}:assigned"),
    );
    Ok(Some(id))
}

// ---------------- 评估 ----------------

struct Assignment {
    id: i64,
    uid: i64,
    state: String,
    assigned_ms: i64,
    baseline: Value,
    def: Def,
    stage: usize,
    deadline_ms: i64,
    prewarned: bool,
    result_ms: Option<i64>,
}

fn load(where_sql: &str, params: &[SqlValue]) -> R<Vec<Assignment>> {
    let r = sql::query(
        &format!(
            "SELECT id, user_id, state, (extract(epoch FROM assigned_at) * 1000)::bigint, baseline::text, definition_snapshot::text,
                    stage_index, (extract(epoch FROM stage_deadline) * 1000)::bigint, prewarned, (extract(epoch FROM result_at) * 1000)::bigint
               FROM assignments WHERE {where_sql} ORDER BY id LIMIT 500 FOR UPDATE SKIP LOCKED"
        ),
        params,
    )?;
    Ok(r.rows
        .iter()
        .filter_map(|row| {
            let def = parse_def(&serde_json::from_str(&row.text(5)?).ok()?).ok()?;
            Some(Assignment {
                id: row.int(0)?,
                uid: row.int(1)?,
                state: row.text(2)?,
                assigned_ms: row.int(3).unwrap_or(0),
                baseline: serde_json::from_str(&row.text(4).unwrap_or_default()).unwrap_or(Value::Null),
                def,
                stage: usize::try_from(row.int(6).unwrap_or(0)).unwrap_or(0),
                deadline_ms: row.int(7).unwrap_or(i64::MAX),
                prewarned: row.bool(8).unwrap_or(false),
                result_ms: row.int(9),
            })
        })
        .collect())
}

fn stage_done(st: &Stage, cur: &Metrics, base: &Value) -> bool {
    st.goals.iter().all(|g| progress(&g.metric, cur, base) >= g.gte)
}

fn evaluate(a: &Assignment, s: &Settings, now: i64) -> R<()> {
    let cur = snapshot(a.uid)?;
    let Some(stage) = a.def.stages.get(a.stage) else { return Ok(()) };
    match a.state.as_str() {
        "active" => {
            if stage_done(stage, &cur, &a.baseline) {
                publish("exam.stage_passed", a.uid, &json!({ "user_id": a.uid, "assignment_id": a.id, "stage": a.stage }))?;
                if let Some(next) = a.def.stages.get(a.stage + 1) {
                    sql::execute(
                        "UPDATE assignments SET stage_index = stage_index + 1, stage_deadline = to_timestamp($2::float8 / 1000), prewarned = false,
                                updated_at = now() WHERE id = $1",
                        &[SqlValue::Int(a.id), SqlValue::Int(a.assigned_ms + next.days * DAY_MS)],
                    )?;
                    // 下一阶段可能已经达成：同一轮继续评估由下次调度处理
                } else {
                    sql::execute(
                        "UPDATE assignments SET state = 'passed', result_at = now(), updated_at = now() WHERE id = $1",
                        &[SqlValue::Int(a.id)],
                    )?;
                    publish("exam.passed", a.uid, &json!({ "user_id": a.uid, "assignment_id": a.id }))?;
                    reward(a)?;
                    notify(s, a.uid, "passed", "新手考核已通过 / Exam passed", "恭喜，你已通过新手考核！\nCongratulations, you passed the newbie exam!", &format!("exam:n:{}:passed", a.id));
                }
            } else if now >= a.deadline_ms {
                fail(a, s)?;
            } else if !a.prewarned && s.prewarn_hours > 0 && a.deadline_ms - now <= s.prewarn_hours * 3_600_000 {
                sql::execute("UPDATE assignments SET prewarned = true WHERE id = $1", &[SqlValue::Int(a.id)])?;
                notify(
                    s,
                    a.uid,
                    "prewarn",
                    "新手考核即将到期 / Exam deadline approaching",
                    "新手考核的当前阶段即将到期，请尽快完成目标。\nThe current stage of your newbie exam ends soon.",
                    &format!("exam:n:{}:prewarn:{}", a.id, a.stage),
                );
            }
        }
        "failed_restricted" => {
            // 补救期内完成最后阶段的全部目标 → 解除
            let in_remedy = a.result_ms.is_some_and(|r| now <= r + a.def.remedy_days * DAY_MS);
            let last = a.def.stages.last().unwrap_or(stage);
            if in_remedy && stage_done(last, &cur, &a.baseline) {
                ctx::call("ctx.users", "lift_restriction", &json!({ "user": a.uid, "kind": "leech_disabled", "reason": "newbie-exam.remedied" }))?;
                sql::execute(
                    "UPDATE assignments SET state = 'remedied', updated_at = now() WHERE id = $1",
                    &[SqlValue::Int(a.id)],
                )?;
                publish("exam.remedied", a.uid, &json!({ "user_id": a.uid, "assignment_id": a.id }))?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn fail(a: &Assignment, s: &Settings) -> R<()> {
    let action = a.def.on_fail.as_str();
    let state = match action {
        "restrict" | "ban" => {
            let kind = if action == "ban" && s.allow_ban { "ban" } else { "leech_disabled" };
            match ctx::call(
                "ctx.users",
                "apply_restriction",
                &json!({ "user": a.uid, "kind": kind, "reason_code": "newbie-exam.failed" }),
            ) {
                Ok(_) => if kind == "ban" { "failed_banned" } else { "failed_restricted" },
                // 员工受保护：只记失败
                Err(e) if e.code == "identity.target_protected" => "failed",
                Err(e) => return Err(e),
            }
        }
        _ => "failed",
    };
    sql::execute(
        "UPDATE assignments SET state = $2, result_at = now(), updated_at = now() WHERE id = $1",
        &[SqlValue::Int(a.id), SqlValue::Text(state.into())],
    )?;
    publish("exam.failed", a.uid, &json!({ "user_id": a.uid, "assignment_id": a.id, "stage": a.stage, "action": action }))?;
    let remedy = if state == "failed_restricted" && a.def.remedy_days > 0 {
        format!("在 {} 天内完成全部目标可自动恢复。", a.def.remedy_days)
    } else {
        String::new()
    };
    notify(
        s,
        a.uid,
        "failed",
        "新手考核未通过 / Exam failed",
        &format!("很遗憾，你没有在期限内完成新手考核。{remedy}\nYou did not complete the newbie exam in time."),
        &format!("exam:n:{}:failed", a.id),
    );
    Ok(())
}

fn reward(a: &Assignment) -> R<()> {
    if a.def.reward_bonus <= 0 {
        return Ok(());
    }
    ctx::call(
        "ctx.ledger",
        "transfer",
        &json!({ "currency": "bonus", "from_user": null, "to_user": a.uid, "amount": a.def.reward_bonus.saturating_mul(ctx::currency_unit("bonus")),
                 "reason": "newbie-exam.reward", "idempotency_key": format!("exam:c:{}", a.id) }),
    )?;
    sql::execute("UPDATE assignments SET reward_given = true WHERE id = $1", &[SqlValue::Int(a.id)])?;
    Ok(())
}

fn evaluate_set(list: &[Assignment]) -> R<()> {
    let s = settings();
    let now = time::now();
    for a in list {
        evaluate(a, &s, now)?;
    }
    Ok(())
}

/// 脏用户与一小时内到期的考核
fn evaluate_due() -> R<()> {
    let dirty = sql::query(
        "DELETE FROM dirty_users WHERE user_id IN (SELECT user_id FROM dirty_users ORDER BY since LIMIT 2000 FOR UPDATE SKIP LOCKED) RETURNING user_id",
        &[],
    )?;
    let ids: Vec<i64> = dirty.rows.iter().filter_map(|r| r.int(0)).collect();
    let list = load(
        "(state = 'active' AND (user_id = ANY($1) OR stage_deadline < now() + interval '1 hour')) OR (state = 'failed_restricted' AND user_id = ANY($1))",
        &[SqlValue::IntArray(ids)],
    )?;
    evaluate_set(&list)?;
    sql::commit()?;
    Ok(())
}

/// 全部进行中与补救期内的考核（键集分页）
fn evaluate_all() -> R<()> {
    let mut last = 0i64;
    loop {
        let list = load("id > $1 AND state IN ('active', 'failed_restricted')", &[SqlValue::Int(last)])?;
        let Some(max) = list.last().map(|a| a.id) else { break };
        evaluate_set(&list)?;
        sql::commit()?;
        last = max;
    }
    // 补发失败的奖励
    let pending = load("state = 'passed' AND NOT reward_given AND definition_snapshot->>'reward_bonus' <> '0'", &[])?;
    for a in &pending {
        reward(a)?;
    }
    sql::commit()?;
    Ok(())
}

// ---------------- 状态 ----------------

fn status(uid: i64) -> R<Option<Value>> {
    let r = sql::query(
        "SELECT id, state, (extract(epoch FROM assigned_at) * 1000)::bigint, baseline::text, definition_snapshot::text, stage_index,
                (extract(epoch FROM stage_deadline) * 1000)::bigint, (extract(epoch FROM result_at) * 1000)::bigint
           FROM assignments WHERE user_id = $1 ORDER BY assigned_at DESC LIMIT 1",
        &[SqlValue::Int(uid)],
    )?;
    let Some(row) = r.rows.first() else { return Ok(None) };
    let def = parse_def(&serde_json::from_str(&row.text(4).unwrap_or_default()).unwrap_or(Value::Null))
        .map_err(|e| err("newbie-exam.internal", &e))?;
    let base: Value = serde_json::from_str(&row.text(3).unwrap_or_default()).unwrap_or(Value::Null);
    let assigned = row.int(2).unwrap_or(0);
    let cur_stage = usize::try_from(row.int(5).unwrap_or(0)).unwrap_or(0);
    let state = row.text(1).unwrap_or_default();
    let cur = snapshot(uid)?;
    let stages: Vec<Value> = def
        .stages
        .iter()
        .enumerate()
        .map(|(i, st)| {
            json!({
                "index": i, "deadline": assigned + st.days * DAY_MS,
                "passed": i < cur_stage || state == "passed" || state == "remedied",
                "goals": st.goals.iter().map(|g| {
                    let p = progress(&g.metric, &cur, &base);
                    json!({ "metric": g.metric, "target": g.gte, "current": if p.is_finite() { json!(p) } else { Value::Null }, "met": p >= g.gte })
                }).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Some(json!({ "assignment_id": row.int(0), "exam": def.name, "state": state, "current_stage": cur_stage,
                    "action_on_fail": def.on_fail, "remedy_days": def.remedy_days, "reward_bonus": def.reward_bonus,
                    "result_at": row.int(7), "stages": stages })))
}

// ---------------- 钩子、事件、cron、服务 ----------------

impl hooks::Guest for Exam {
    fn handle(c: hooks::HookCall) -> R<hooks::HookDecision> {
        let input: Value = serde_json::from_str(&c.input).unwrap_or(Value::Null);
        if c.hook == "invite.before_issue"
            && let Some(uid) = input["inviter_id"].as_i64()
            && settings().block_invite
        {
            let r = sql::query(
                "SELECT 1 FROM assignments WHERE user_id = $1 AND state IN ('active', 'failed', 'failed_restricted', 'failed_banned')
                  ORDER BY assigned_at DESC LIMIT 1",
                &[SqlValue::Int(uid)],
            )?;
            if !r.rows.is_empty() {
                return Ok(hooks::HookDecision::Deny(hooks::DenyInfo {
                    code: "newbie-exam.restricted".into(),
                    message: "members who have not passed the newbie exam cannot send invites".into(),
                    details: None,
                }));
            }
        }
        Ok(hooks::HookDecision::Allow)
    }
}

impl on_event::Guest for Exam {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        let Some(uid) = p["user_id"].as_i64() else { return Ok(()) };
        match evt.event_type.as_str() {
            "user.registered" => {
                let s = settings();
                let mode = p["registration_mode"].as_str().unwrap_or("open");
                if s.def.is_ok() && s.auto_modes.iter().any(|m| m == mode) {
                    match assign(uid, "auto", &s) {
                        Ok(_) => {}
                        Err(e) if e.code == "newbie-exam.already_assigned" => {}
                        Err(e) => return Err(e),
                    }
                }
                Ok(())
            }
            "traffic.settled" => {
                sql::execute("INSERT INTO dirty_users (user_id) VALUES ($1) ON CONFLICT DO NOTHING", &[SqlValue::Int(uid)])?;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

impl on_cron::Guest for Exam {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            "evaluate" => evaluate_due(),
            "evaluate_all" => evaluate_all(),
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

impl service_provider::Guest for Exam {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        let uid = a["user"].as_i64().ok_or_else(|| err("kernel.validation_failed", "user"))?;
        match method.as_str() {
            "status" => Ok(status(uid)?.unwrap_or(Value::Null).to_string()),
            other => Err(err("kernel.not_found", &format!("ctx.exams has no method `{other}`"))),
        }
    }
}

// ---------------- 后台 ----------------

fn audit_write(action: &str, uid: i64, details: &Value) -> R<()> {
    audit::write(&audit::AuditEntry { action: action.into(), target: format!("user:{uid}"), details: details.to_string() })
}

fn admin_assign(b: &Value) -> R<Value> {
    let ids: Vec<i64> = b["user_ids"].as_array().map(|a| a.iter().filter_map(Value::as_i64).take(1000).collect()).unwrap_or_default();
    if ids.is_empty() {
        return Err(err("kernel.validation_failed", "user_ids is required"));
    }
    let s = settings();
    let by = format!("user:{}", ctx::current_user()?);
    let mut results = Vec::new();
    for uid in ids {
        results.push(match assign(uid, &by, &s) {
            Ok(id) => json!({ "user_id": uid, "assignment_id": id }),
            Err(e) => json!({ "user_id": uid, "error": e.code }),
        });
    }
    audit::write(&audit::AuditEntry { action: "newbie-exam.assign".into(), target: "users".into(), details: json!({ "results": results }).to_string() })?;
    Ok(json!({ "results": results }))
}

fn admin_exempt(uid: i64, b: &Value) -> R<()> {
    let reason = b["reason"].as_str().unwrap_or("").chars().take(500).collect::<String>();
    let r = sql::query(
        "UPDATE assignments SET state = 'exempted', reason = $2, result_at = now(), updated_at = now()
          WHERE user_id = $1 AND state IN ('active', 'failed_restricted') RETURNING state",
        &[SqlValue::Int(uid), SqlValue::Text(reason.clone())],
    )?;
    if r.rows.is_empty() {
        return Err(err("newbie-exam.not_assigned", "the user has no open exam"));
    }
    // 失败限制中被豁免：解除本插件施加的限制
    let _ = ctx::call("ctx.users", "lift_restriction", &json!({ "user": uid, "kind": "leech_disabled", "reason": "newbie-exam.exempted" }));
    publish("exam.exempted", uid, &json!({ "user_id": uid, "reason": reason }))?;
    audit_write("newbie-exam.exempt", uid, &json!({ "reason": reason }))
}

fn admin_extend(uid: i64, b: &Value) -> R<()> {
    let days = b["days"].as_i64().filter(|d| (1..=365).contains(d)).ok_or_else(|| err("kernel.validation_failed", "days must be 1-365"))?;
    // 顺延：分配时间后移，当前与后续阶段的截止时间一起顺延
    let n = sql::execute(
        "UPDATE assignments SET assigned_at = assigned_at + make_interval(days => $2::int), stage_deadline = stage_deadline + make_interval(days => $2::int),
                prewarned = false, updated_at = now()
          WHERE user_id = $1 AND state = 'active'",
        &[SqlValue::Int(uid), SqlValue::Int(days)],
    )?;
    if n == 0 {
        return Err(err("newbie-exam.not_assigned", "the user has no active exam"));
    }
    audit_write("newbie-exam.extend", uid, &json!({ "days": days }))
}

impl http_handler::Guest for Exam {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let reply = match req.route() {
            "me" => match status(ctx::current_user()?)? {
                Some(v) => http::Reply::ok(&v),
                None => http::Reply::no_content(),
            },
            "admin_status" => http::Reply::ok(&status(req.param_i64("id")?)?.unwrap_or(Value::Null)),
            "admin_assign" => http::Reply::ok(&admin_assign(&req.body_json::<Value>()?)?),
            "admin_exempt" => {
                admin_exempt(req.param_i64("id")?, &req.body_json::<Value>().unwrap_or(Value::Null))?;
                http::Reply::no_content()
            }
            "admin_extend" => {
                admin_extend(req.param_i64("id")?, &req.body_json::<Value>()?)?;
                http::Reply::no_content()
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(Exam);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions() {
        let ok = json!({ "stages": [
            { "days": 7, "goals": [{ "metric": "uploaded_gib", "gte": 5 }] },
            { "days": 30, "goals": [{ "metric": "seed_hours", "gte": 100 }, { "metric": "ratio", "gte": 1 }] } ] });
        let d = parse_def(&ok).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!((d.stages.len(), d.on_fail.as_str(), d.remedy_days), (2, "restrict", 7));
        assert!(parse_def(&json!({ "stages": [{ "days": 30, "goals": [] }] })).is_err());
        assert!(parse_def(&json!({ "stages": [{ "days": 30, "goals": [{ "metric": "nope", "gte": 1 }] }] })).is_err());
        assert!(parse_def(&json!({ "stages": [{ "days": 30, "goals": [{ "metric": "ratio", "gte": 1 }] }, { "days": 30, "goals": [{ "metric": "ratio", "gte": 1 }] }] })).is_err());
        assert!(parse_def(&json!({ "on_fail": "x", "stages": [{ "days": 3, "goals": [{ "metric": "ratio", "gte": 1 }] }] })).is_err());
        assert_eq!(parse_def(&def_json(&d)).map(|x| x.stages), Ok(d.stages));
    }

    #[test]
    fn progress_is_delta_except_ratio() {
        let mut cur = Metrics::new();
        cur.insert("uploaded_gib".into(), 12.0);
        cur.insert("ratio".into(), 0.8);
        let base = json!({ "uploaded_gib": 10.0, "ratio": 2.0 });
        assert!((progress("uploaded_gib", &cur, &base) - 2.0).abs() < 1e-9);
        assert!((progress("ratio", &cur, &base) - 0.8).abs() < 1e-9);
        assert!((progress("seed_hours", &cur, &base)).abs() < 1e-9);
    }
}
