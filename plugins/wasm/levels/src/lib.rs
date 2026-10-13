//! levels 插件（WASM，docs/plugins/levels/）。
//!
//! 本版本实现：等级定义（NexusPHP 预设）、角色同步（每个等级一个 `levels:class_<id>` 角色并绑定到等级）、
//! 指标（注册天数、上传 / 下载 / 分享率、做种时长、发布数、做种体积）、评估（升级、保级警告、到期降级、
//! 升级保护期、手动覆盖与保号）、增量评估（流量结算后标记）与每日全量评估、`invite.before_issue` 钩子、
//! `ctx.levels` 服务、HTTP。邀请奖励、不活跃处理、VIP / 荣誉覆盖、模拟评估在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

pt_plugin_sdk::world!("levels");

use std::collections::BTreeMap;

use exports::pt::plugin::{hooks, http_handler, lifecycle, mcp_tool, on_cron, on_event, service_provider};
use pt_plugin_sdk::{
    events, invocation, services,
    sql::{self, SqlValue},
    types::PluginError,
};
use serde_json::{Value, json};

struct Levels;

type R<T> = Result<T, PluginError>;

fn err(code: &str, message: &str) -> PluginError {
    PluginError {
        code: code.into(),
        message: message.into(),
        details: None,
        retryable: false,
    }
}

fn int(v: &SqlValue) -> i64 {
    match v {
        SqlValue::Int(i) => *i,
        _ => 0,
    }
}

fn num(v: &SqlValue) -> f64 {
    match v {
        SqlValue::Int(i) => *i as f64,
        SqlValue::Float(f) => *f,
        SqlValue::Numeric(s) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn text(v: &SqlValue) -> String {
    match v {
        SqlValue::Text(s) => s.clone(),
        _ => String::new(),
    }
}

fn call(service: &str, method: &str, args: &Value) -> R<Value> {
    let out = services::call(service, method, &args.to_string())?;
    serde_json::from_str(&out).map_err(|_| err("levels.bad_service_reply", "invalid JSON from service"))
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

fn cfg() -> Value {
    serde_json::from_str(&pt_plugin_sdk::config::get_static()).unwrap_or(Value::Null)
}

const GIB: f64 = 1_073_741_824.0;
const TIB: f64 = 1_099_511_627_776.0;

/// NexusPHP 风格预设（03 §1.1）。“农民”为降级落点：用户 keep = ratio ≥ 0.5，回到用户需 ratio ≥ 0.6（滞回）。
fn preset_nexus() -> Vec<Value> {
    let step = |id: i64, slug: &str, days: f64, up: f64, ratio: f64, keep: f64, protected: bool| {
        json!({ "id": id, "slug": slug, "rank": id, "type": "normal", "protected": protected, "is_default": false,
                "promote": [ { "field": "account_age_days", "op": "gte", "value": days },
                             { "field": "uploaded", "op": "gte", "value": up },
                             { "field": "ratio", "op": "gte", "value": ratio } ],
                "keep": [ { "field": "ratio", "op": "gte", "value": keep } ] })
    };
    vec![
        json!({ "id": 10, "slug": "peasant", "rank": 10, "type": "normal", "promote": [], "keep": [], "can_invite": false, "is_default": false }),
        json!({ "id": 20, "slug": "user", "rank": 20, "type": "normal", "is_default": true,
                "promote": [ { "field": "ratio", "op": "gte", "value": 0.6 } ], "keep": [ { "field": "ratio", "op": "gte", "value": 0.5 } ] }),
        step(30, "power_user", 28.0, 50.0 * GIB, 1.05, 0.95, false),
        step(40, "elite_user", 56.0, 120.0 * GIB, 1.55, 1.45, false),
        step(50, "crazy_user", 84.0, 300.0 * GIB, 2.05, 1.95, false),
        step(60, "insane_user", 112.0, 500.0 * GIB, 2.55, 2.45, false),
        step(70, "veteran_user", 168.0, 750.0 * GIB, 3.05, 2.95, true),
        step(80, "extreme_user", 224.0, TIB, 3.55, 3.45, true),
        step(90, "ultimate_user", 280.0, 1.5 * TIB, 4.05, 3.95, true),
        step(100, "nexus_master", 364.0, 3.0 * TIB, 4.55, 4.45, true),
        json!({ "id": 200, "slug": "vip", "rank": 200, "type": "vip", "promote": [], "keep": [], "protected": true, "is_default": false }),
    ]
}

#[derive(Debug, Clone)]
struct Class {
    id: i64,
    rank: i64,
    kind: String,
    promote: Vec<Value>,
    /// None = 与 promote 相同；Some(空) = 永不降级
    keep: Option<Vec<Value>>,
    can_invite: bool,
    protected: bool,
    is_default: bool,
}

fn classes() -> R<Vec<Class>> {
    let r = sql::query(
        "SELECT id, rank, type, promote::text, keep::text, can_invite, protected, is_default FROM classes WHERE active ORDER BY rank, id",
        &[],
    )?;
    Ok(r.rows
        .iter()
        .map(|row| Class {
            id: int(&row[0]),
            rank: int(&row[1]),
            kind: text(&row[2]),
            promote: serde_json::from_str(&text(&row[3])).unwrap_or_default(),
            keep: match &row[4] {
                SqlValue::Text(t) => serde_json::from_str(t).ok(),
                _ => None,
            },
            can_invite: matches!(row[5], SqlValue::Boolean(true)),
            protected: matches!(row[6], SqlValue::Boolean(true)),
            is_default: matches!(row[7], SqlValue::Boolean(true)),
        })
        .collect())
}

// ---------------- 指标与谓词（03 §1.2） ----------------

/// 用户指标；ratio 在下载为 0 时为 +∞
type Metrics = BTreeMap<String, f64>;

fn metrics_of(ids: &[i64]) -> R<BTreeMap<i64, Metrics>> {
    let r = sql::query(
        "SELECT u.id, extract(epoch FROM now() - u.created_at)::float8 / 86400,
                coalesce(t.uploaded, 0), coalesce(t.downloaded, 0), coalesce(t.seed_secs, 0),
                coalesce(t.completed_count, 0), coalesce(t.seeding_size, 0), coalesce(c.approved_count, 0)
           FROM identity.v_user_public u
           LEFT JOIN accounting.v_user_totals t ON t.user_id = u.id
           LEFT JOIN torrent.v_uploader_counts c ON c.user_id = u.id
          WHERE u.id = ANY($1) AND u.status <> 'deleted'",
        &[SqlValue::IntArray(ids.to_vec())],
    )?;
    Ok(r.rows
        .iter()
        .map(|row| {
            let (up, down) = (num(&row[2]), num(&row[3]));
            let seed = num(&row[4]);
            let mut m = Metrics::new();
            m.insert("account_age_days".into(), num(&row[1]).floor());
            m.insert("uploaded".into(), up);
            m.insert("downloaded".into(), down);
            m.insert("ratio".into(), if down > 0.0 { up / down } else { f64::INFINITY });
            m.insert("seed_secs".into(), seed);
            m.insert("avg_seed_secs".into(), seed / num(&row[5]).max(1.0));
            m.insert("seeding_size".into(), num(&row[6]));
            m.insert("uploads_count".into(), num(&row[7]));
            (int(&row[0]), m)
        })
        .collect())
}

fn holds(m: &Metrics, p: &Value) -> bool {
    let Some(cur) = p["field"].as_str().and_then(|f| m.get(f)).copied() else {
        return false;
    };
    let Some(v) = p["value"].as_f64() else {
        return false;
    };
    match p["op"].as_str() {
        Some("gte") => cur >= v,
        Some("gt") => cur > v,
        Some("lte") => cur <= v,
        Some("lt") => cur < v,
        Some("eq") => (cur - v).abs() < f64::EPSILON,
        _ => false,
    }
}

fn all(m: &Metrics, ps: &[Value]) -> bool {
    ps.iter().all(|p| holds(m, p))
}

fn keep_list(c: &Class) -> &[Value] {
    c.keep.as_deref().unwrap_or(&c.promote)
}

fn fmt_metric(field: &str, v: f64) -> String {
    if v.is_infinite() {
        "inf".into()
    } else if field == "ratio" {
        format!("{v:.2}")
    } else {
        format!("{}", v.round() as i64)
    }
}

fn progress(m: &Metrics, ps: &[Value]) -> Vec<Value> {
    ps.iter()
        .map(|p| {
            let f = p["field"].as_str().unwrap_or("");
            json!({ "metric": f, "op": p["op"], "target": fmt_metric(f, p["value"].as_f64().unwrap_or(0.0)),
                    "current": fmt_metric(f, m.get(f).copied().unwrap_or(0.0)), "met": holds(m, p) })
        })
        .collect()
}

// ---------------- 评估（HLD §5.1、03 §1.3） ----------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Full,
    PromoteOnly,
}

struct State {
    class_id: i64,
    base_class_id: i64,
    protected: bool,
    warning_deadline_passed: Option<bool>,
}

fn ensure_row(user: i64, cls: &[Class]) -> R<State> {
    let default = cls.iter().find(|c| c.is_default).map_or(0, |c| c.id);
    sql::execute(
        "INSERT INTO user_levels (user_id, class_id, base_class_id, source) VALUES ($1, $2, $2, 'default') ON CONFLICT DO NOTHING",
        &[SqlValue::Int(user), SqlValue::Int(default)],
    )?;
    let r = sql::query(
        "SELECT class_id, base_class_id,
                (coalesce(protect_until, '-infinity') > now() OR coalesce(protected_until, '-infinity') > now()),
                CASE WHEN warning_deadline IS NULL THEN NULL ELSE warning_deadline <= now() END
           FROM user_levels WHERE user_id = $1 FOR UPDATE",
        &[SqlValue::Int(user)],
    )?;
    let row = &r.rows[0];
    Ok(State {
        class_id: int(&row[0]),
        base_class_id: int(&row[1]),
        protected: matches!(row[2], SqlValue::Boolean(true)),
        warning_deadline_passed: match row[3] {
            SqlValue::Boolean(b) => Some(b),
            _ => None,
        },
    })
}

fn active_override(user: i64) -> R<Option<i64>> {
    let r = sql::query(
        "SELECT class_id FROM overrides WHERE user_id = $1 AND ended_at IS NULL AND (until IS NULL OR until > now())
          ORDER BY CASE kind WHEN 'manual' THEN 300 WHEN 'honor' THEN 200 ELSE 100 END DESC, id DESC LIMIT 1",
        &[SqlValue::Int(user)],
    )?;
    Ok(r.rows.first().map(|row| int(&row[0])))
}

/// 设置等级（经 identity），写历史与事件
fn apply(user: i64, from: i64, to: i64, base: i64, source: &str, cause: &str) -> R<()> {
    call(
        "ctx.users",
        "set_class",
        &json!({ "id": user, "class_id": to, "reason": format!("levels.{cause}") }),
    )?;
    sql::execute(
        "UPDATE user_levels SET class_id = $2, base_class_id = $3, source = $4, evaluated_at = now(), updated_at = now() WHERE user_id = $1",
        &[
            SqlValue::Int(user),
            SqlValue::Int(to),
            SqlValue::Int(base),
            SqlValue::Text(source.into()),
        ],
    )?;
    if from != to {
        sql::execute(
            "INSERT INTO history (user_id, from_class, to_class, cause, actor) VALUES ($1, $2, $3, $4, 'levels')",
            &[
                SqlValue::Int(user),
                SqlValue::Int(from),
                SqlValue::Int(to),
                SqlValue::Text(cause.into()),
            ],
        )?;
    }
    Ok(())
}

fn evaluate(user: i64, m: &Metrics, mode: Mode, cls: &[Class]) -> R<()> {
    let st = ensure_row(user, cls)?;
    let normal: Vec<&Class> = cls.iter().filter(|c| c.kind == "normal").collect();
    if let Some(ov) = active_override(user)? {
        if st.class_id != ov {
            apply(user, st.class_id, ov, st.base_class_id, "manual", "override")?;
        }
        return Ok(());
    }
    let cur = normal
        .iter()
        .find(|c| c.id == st.base_class_id)
        .copied()
        .or_else(|| normal.iter().find(|c| c.is_default).copied());
    let Some(cur) = cur else { return Ok(()) };
    // 覆盖到期：回到基线等级
    if st.class_id != cur.id {
        apply(user, st.class_id, cur.id, cur.id, "auto", "override_expired")?;
    }
    let one_step = cfg()["promote_mode"].as_str() == Some("one_step");
    let mut cands: Vec<&Class> = normal
        .iter()
        .filter(|c| c.rank > cur.rank && !c.promote.is_empty() && all(m, &c.promote))
        .copied()
        .collect();
    if one_step {
        let next_rank = normal.iter().filter(|c| c.rank > cur.rank).map(|c| c.rank).min();
        cands.retain(|c| Some(c.rank) == next_rank);
    }
    if let Some(target) = cands.iter().max_by_key(|c| c.rank) {
        let protect_days = cfg()["protect_days"].as_i64().unwrap_or(7);
        apply(user, cur.id, target.id, target.id, "auto", "promoted")?;
        sql::execute(
            "UPDATE user_levels SET protect_until = now() + make_interval(days => $2::int), warning_class_id = NULL,
                    warning_deadline = NULL, warning_failed = NULL WHERE user_id = $1",
            &[SqlValue::Int(user), SqlValue::Int(protect_days)],
        )?;
        publish(
            "level.promoted",
            user,
            json!({ "user_id": user, "from_class": cur.id, "to_class": target.id, "cause": "auto" }),
        )?;
        return Ok(());
    }
    if mode == Mode::PromoteOnly || st.protected || cur.protected {
        return Ok(());
    }
    if all(m, keep_list(cur)) {
        if st.warning_deadline_passed.is_some() {
            sql::execute(
                "UPDATE user_levels SET warning_class_id = NULL, warning_deadline = NULL, warning_failed = NULL WHERE user_id = $1",
                &[SqlValue::Int(user)],
            )?;
            publish(
                "level.warning_cleared",
                user,
                json!({ "user_id": user, "class_id": cur.id }),
            )?;
        }
        return Ok(());
    }
    let failed: Vec<Value> = keep_list(cur).iter().filter(|p| !holds(m, p)).cloned().collect();
    match st.warning_deadline_passed {
        None => {
            let days = cfg()["warning_days"].as_i64().unwrap_or(7);
            let r = sql::query(
                "UPDATE user_levels SET warning_class_id = $2, warning_deadline = now() + make_interval(days => $3::int), warning_failed = $4
                  WHERE user_id = $1 RETURNING (extract(epoch FROM warning_deadline) * 1000)::bigint",
                &[SqlValue::Int(user), SqlValue::Int(cur.id), SqlValue::Int(days), SqlValue::Json(json!(failed).to_string())],
            )?;
            publish(
                "level.warned",
                user,
                json!({ "user_id": user, "class_id": cur.id, "deadline": int(&r.rows[0][0]),
                                                  "failed_conditions": failed }),
            )?;
        }
        Some(false) => {}
        Some(true) => {
            // 降到较低等级中保级条件满足的最高者，没有则最低等级
            let target = normal
                .iter()
                .filter(|c| c.rank < cur.rank && all(m, keep_list(c)))
                .max_by_key(|c| c.rank)
                .or_else(|| normal.first())
                .copied()
                .unwrap_or(cur);
            apply(user, cur.id, target.id, target.id, "auto", "demoted")?;
            sql::execute(
                "UPDATE user_levels SET warning_class_id = NULL, warning_deadline = NULL, warning_failed = NULL WHERE user_id = $1",
                &[SqlValue::Int(user)],
            )?;
            publish(
                "level.demoted",
                user,
                json!({ "user_id": user, "from_class": cur.id, "to_class": target.id, "reason": "keep_failed" }),
            )?;
        }
    }
    Ok(())
}

fn evaluate_batch(ids: &[i64], mode: Mode) -> R<usize> {
    let cls = classes()?;
    let ms = metrics_of(ids)?;
    let mut sorted = ids.to_vec();
    sorted.sort_unstable();
    for uid in &sorted {
        if let Some(m) = ms.get(uid) {
            evaluate(*uid, m, mode, &cls)?;
        }
    }
    Ok(sorted.len())
}

fn level_info(user: i64) -> R<Value> {
    let cls = classes()?;
    let st = ensure_row(user, &cls)?;
    let m = metrics_of(&[user])?.remove(&user).unwrap_or_default();
    let cur = cls.iter().find(|c| c.id == st.base_class_id);
    let next = cur.and_then(|c| {
        cls.iter()
            .filter(|n| n.kind == "normal" && n.rank > c.rank && !n.promote.is_empty())
            .min_by_key(|n| n.rank)
    });
    let w = sql::query(
        "SELECT (extract(epoch FROM warning_deadline) * 1000)::bigint, source FROM user_levels WHERE user_id = $1",
        &[SqlValue::Int(user)],
    )?;
    let src = w.rows.first().map(|r| text(&r[1])).unwrap_or_default();
    Ok(json!({
        "class": { "id": st.class_id, "name_key": format!("levels.class.{}.name", st.class_id), "source": src },
        "next": next.map(|n| json!({ "id": n.id, "name_key": format!("levels.class.{}.name", n.id), "progress": progress(&m, &n.promote) })),
        "warning": w.rows.first().and_then(|r| match r[0] { SqlValue::Int(d) => Some(json!({ "deadline": d })), _ => None }),
        "protected": st.protected || cls.iter().any(|c| c.id == st.class_id && c.protected),
    }))
}

// ---------------- 导出 ----------------

impl lifecycle::Guest for Levels {
    fn init() -> R<()> {
        let has = sql::query("SELECT 1 FROM classes LIMIT 1", &[])?;
        if has.rows.is_empty() {
            for c in preset_nexus() {
                sql::execute(
                    "INSERT INTO classes (id, slug, rank, type, promote, keep, can_invite, protected, is_default, preset)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'nexus')",
                    &[SqlValue::Int(c["id"].as_i64().unwrap_or(0)), SqlValue::Text(c["slug"].as_str().unwrap_or("").into()),
                      SqlValue::Int(c["rank"].as_i64().unwrap_or(0)), SqlValue::Text(c["type"].as_str().unwrap_or("normal").into()),
                      SqlValue::Json(c["promote"].to_string()),
                      if c["keep"].is_null() { SqlValue::Null } else { SqlValue::Json(c["keep"].to_string()) },
                      SqlValue::Boolean(c["can_invite"].as_bool().unwrap_or(true)), SqlValue::Boolean(c["protected"].as_bool().unwrap_or(false)),
                      SqlValue::Boolean(c["is_default"].as_bool().unwrap_or(false))],
                )?;
            }
        }
        // 角色同步：每个等级一个角色，绑定到该等级（权限由管理员在 identity 中为角色配置）
        for c in classes()? {
            let role = format!("levels:class_{}", c.id);
            call("ctx.rbac", "define_role", &json!({ "role": role }))?;
            call(
                "ctx.rbac",
                "set_class_roles",
                &json!({ "class_id": c.id, "roles": [role] }),
            )?;
        }
        Ok(())
    }
}

impl hooks::Guest for Levels {
    fn handle(c: hooks::HookCall) -> R<hooks::HookDecision> {
        if c.hook == "invite.before_issue" {
            let input: Value = serde_json::from_str(&c.input).unwrap_or(Value::Null);
            let inviter = input["inviter_id"].as_i64().unwrap_or(0);
            let r = sql::query(
                "SELECT c.can_invite FROM user_levels u JOIN classes c ON c.id = u.class_id WHERE u.user_id = $1",
                &[SqlValue::Int(inviter)],
            )?;
            if matches!(r.rows.first().map(|row| &row[0]), Some(SqlValue::Boolean(false))) {
                return Ok(hooks::HookDecision::Deny(hooks::DenyInfo {
                    code: "levels.class_required".into(),
                    message: "your class cannot send invites".into(),
                    details: None,
                }));
            }
        }
        Ok(hooks::HookDecision::Allow)
    }
}

impl on_event::Guest for Levels {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        let p: Value =
            serde_json::from_str(&evt.payload).map_err(|_| err("levels.bad_event", "invalid payload"))?;
        let uid = p["user_id"].as_i64().unwrap_or(0);
        match evt.event_type.as_str() {
            // 新用户：写入默认等级并同步到 identity
            "user.registered" => {
                let cls = classes()?;
                let st = ensure_row(uid, &cls)?;
                if p["class_id"].as_i64() != Some(st.class_id) {
                    call(
                        "ctx.users",
                        "set_class",
                        &json!({ "id": uid, "class_id": st.class_id, "reason": "levels.default" }),
                    )?;
                }
            }
            "traffic.settled" => {
                sql::execute(
                    "INSERT INTO dirty_users (user_id, since) VALUES ($1, now()) ON CONFLICT DO NOTHING",
                    &[SqlValue::Int(uid)],
                )?;
            }
            "user.logged_in" => {
                sql::execute(
                    "UPDATE user_levels SET last_active_day = current_date WHERE user_id = $1 AND last_active_day IS DISTINCT FROM current_date",
                    &[SqlValue::Int(uid)],
                )?;
            }
            _ => {}
        }
        Ok(())
    }
}

impl on_cron::Guest for Levels {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            // 增量评估：只升级（03 §1.4）
            "incr_evaluate" => loop {
                let r = sql::query(
                    "DELETE FROM dirty_users WHERE user_id IN (SELECT user_id FROM dirty_users ORDER BY since LIMIT 2000 FOR UPDATE SKIP LOCKED)
                     RETURNING user_id",
                    &[],
                )?;
                let ids: Vec<i64> = r.rows.iter().map(|row| int(&row[0])).collect();
                if ids.is_empty() {
                    return Ok(());
                }
                evaluate_batch(&ids, Mode::PromoteOnly)?;
                sql::commit()?;
            },
            // 每日全量评估（键集分页，每批提交）
            "daily_evaluate" => {
                let mut last = 0i64;
                loop {
                    let r = sql::query(
                        "SELECT id FROM identity.v_user_public WHERE id > $1 AND status <> 'deleted' ORDER BY id LIMIT 2000",
                        &[SqlValue::Int(last)],
                    )?;
                    let ids: Vec<i64> = r.rows.iter().map(|row| int(&row[0])).collect();
                    let Some(max) = ids.last().copied() else {
                        return Ok(());
                    };
                    evaluate_batch(&ids, Mode::Full)?;
                    sql::commit()?;
                    last = max;
                }
            }
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

fn json_response(status: u16, v: &Value) -> http_handler::Response {
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

fn me() -> R<i64> {
    invocation::current()
        .actor
        .user_id
        .and_then(|u| i64::try_from(u).ok())
        .ok_or_else(|| err("kernel.unauthenticated", "login required"))
}

impl http_handler::Guest for Levels {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let body = || -> R<Value> {
            serde_json::from_slice(&req.body).map_err(|_| err("kernel.bad_request", "invalid JSON body"))
        };
        let v = match req.route_id.as_str() {
            "me" => level_info(me()?)?,
            "classes" => {
                json!({ "items": classes()?.iter().map(|c| json!({ "id": c.id, "rank": c.rank, "type": c.kind,
                "name_key": format!("levels.class.{}.name", c.id), "promote": c.promote, "keep": keep_list(c),
                "can_invite": c.can_invite, "protected": c.protected })).collect::<Vec<_>>() })
            }
            "set_user_class" => {
                let (uid, b) = (param(&req, "user_id")?, body()?);
                let class = b["class_id"]
                    .as_i64()
                    .ok_or_else(|| err("kernel.validation_failed", "class_id is required"))?;
                if !classes()?.iter().any(|c| c.id == class) {
                    return Err(err("levels.class_not_found", "class not found"));
                }
                let until = b["lock_until_ms"].as_i64();
                sql::execute(
                    "UPDATE overrides SET ended_at = now() WHERE user_id = $1 AND kind = 'manual' AND ended_at IS NULL",
                    &[SqlValue::Int(uid)],
                )?;
                sql::execute(
                    "INSERT INTO overrides (user_id, kind, class_id, until, reason, created_by) VALUES ($1, 'manual', $2, to_timestamp($3::float8 / 1000), $4, $5)",
                    &[
                        SqlValue::Int(uid),
                        SqlValue::Int(class),
                        until.map_or(SqlValue::Null, SqlValue::Int),
                        SqlValue::Text(b["reason"].as_str().unwrap_or("").into()),
                        SqlValue::Text(format!("user:{}", me()?)),
                    ],
                )?;
                evaluate_batch(&[uid], Mode::Full)?;
                return Ok(http_handler::Response {
                    status: 204,
                    headers: vec![],
                    body: vec![],
                });
            }
            "unlock_user_class" => {
                let uid = param(&req, "user_id")?;
                sql::execute(
                    "UPDATE overrides SET ended_at = now() WHERE user_id = $1 AND kind = 'manual' AND ended_at IS NULL",
                    &[SqlValue::Int(uid)],
                )?;
                evaluate_batch(&[uid], Mode::Full)?;
                return Ok(http_handler::Response {
                    status: 204,
                    headers: vec![],
                    body: vec![],
                });
            }
            "protect_user" => {
                let (uid, b) = (param(&req, "user_id")?, body()?);
                let cls = classes()?;
                ensure_row(uid, &cls)?;
                sql::execute(
                    "UPDATE user_levels SET protected_until = to_timestamp($2::float8 / 1000) WHERE user_id = $1",
                    &[
                        SqlValue::Int(uid),
                        b["until_ms"].as_i64().map_or(SqlValue::Null, SqlValue::Int),
                    ],
                )?;
                return Ok(http_handler::Response {
                    status: 204,
                    headers: vec![],
                    body: vec![],
                });
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(json_response(200, &v))
    }
}

impl service_provider::Guest for Levels {
    fn call(service: String, method: String, args: String) -> R<String> {
        if service != "ctx.levels" {
            return Err(err("kernel.not_found", "unknown service"));
        }
        let a: Value =
            serde_json::from_str(&args).map_err(|_| err("kernel.bad_request", "invalid JSON arguments"))?;
        let user = a["user"].as_i64().unwrap_or(0);
        let inv = invocation::current();
        let out = match method.as_str() {
            "level_of" => level_info(user)?,
            "classes" => json!(
                classes()?
                    .iter()
                    .map(
                        |c| json!({ "id": c.id, "rank": c.rank, "type": c.kind, "can_invite": c.can_invite,
                                                                 "protected": c.protected })
                    )
                    .collect::<Vec<_>>()
            ),
            "evaluate" => {
                if !inv.caller_scopes.iter().any(|s| s == "evaluate") {
                    return Err(err("levels.scope_required", "scope `evaluate` required"));
                }
                evaluate_batch(&[user], Mode::Full)?;
                level_info(user)?
            }
            "is_protected" => level_info(user)?["protected"].clone(),
            _ => return Err(err("kernel.not_found", "unknown method")),
        };
        Ok(out.to_string())
    }
}

// ---------------- MCP 工具（catalog/mcp-tools.md） ----------------

impl mcp_tool::Guest for Levels {
    fn call(tool: String, _arguments: String) -> R<String> {
        match tool.as_str() {
            "get_my_level" => Ok(level_info(me()?)?.to_string()),
            _ => Err(err("kernel.not_found", "unknown tool")),
        }
    }
}

export!(Levels);

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pairs: &[(&str, f64)]) -> Metrics {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
    }

    #[test]
    fn predicates_and_progress() {
        let x = m(&[
            ("ratio", f64::INFINITY),
            ("uploaded", 60.0 * GIB),
            ("account_age_days", 30.0),
        ]);
        let pu = &preset_nexus()[2];
        assert!(all(&x, pu["promote"].as_array().unwrap_or(&vec![])));
        let y = m(&[("ratio", 0.4)]);
        assert!(!holds(&y, &json!({"field": "ratio", "op": "gte", "value": 0.5})));
        assert!(!holds(&y, &json!({"field": "missing", "op": "gte", "value": 0})));
        let p = progress(&x, &[json!({"field": "ratio", "op": "gte", "value": 1.05})]);
        assert_eq!(p[0]["current"], "inf");
        assert_eq!(p[0]["target"], "1.05");
        assert_eq!(
            preset_nexus().iter().filter(|c| c["is_default"] == true).count(),
            1
        );
    }
}
