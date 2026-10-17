//! ratio-policy 插件（WASM，docs/plugins/ratio-policy/）：分享率考核。
//!
//! - 要求分享率按下载量分档（Gazelle 风格），可按“做种中的已完成种子比例”减免（`seeding_reduction`）；
//!   豁免：下载量低于阈值、注册宽限期、豁免等级、管理员手动豁免。
//! - 状态机：ok → warned（警告期 `warning_days`）→ restricted（经 identity 限制下载 `leech_disabled`）
//!   →（可选）banned；分享率恢复后自动解除本插件施加的限制。手动警告不因分享率恢复而提前解除。
//! - 评估：`traffic.settled` 标脏 → 每 15 分钟增量评估；每日全量评估（同时处理警告到期）。
//! - 钩子：警告期禁止下载新种子（可选，已下载过的种子不受影响）；警告 / 限制中不能发邀请。
//! - `ctx.ratio.status`、`GET /v1/me`、后台警告 / 解除 / 豁免与列表（审计）。

pt_plugin_sdk::world!("ratio-policy", http);

use exports::pt::plugin::{hooks, http_handler, on_cron, on_event, service_provider};
use pt_plugin_sdk::{audit, prelude::*, serde_json, services, sql, time};

struct Ratio;

const GIB: f64 = 1_073_741_824.0;
const DAY_MS: i64 = 86_400_000;

// ---------------- 设置与纯函数 ----------------

#[derive(Debug, Clone, PartialEq)]
pub struct Tier {
    pub min_bytes: u64,
    pub required: f64,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub table: Vec<Tier>,
    pub seeding_reduction: f64,
    pub exempt_below_bytes: u64,
    pub grace_days: i64,
    pub exempt_class_ids: Vec<i64>,
    pub warning_days: i64,
    pub ban_after_days: i64,
    pub block_new_downloads: bool,
    pub notify: bool,
}

/// Gazelle 默认要求表（GiB → 要求分享率）
const DEFAULT_TABLE: &[(f64, f64)] = &[(0.0, 0.0), (5.0, 0.15), (10.0, 0.2), (20.0, 0.3), (30.0, 0.4), (40.0, 0.5), (50.0, 0.6)];

fn gib(v: f64) -> u64 {
    // 设置值有界（≤ 1 PiB），截断到字节
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let b = (v.max(0.0) * GIB) as u64;
    b
}

impl Settings {
    pub fn from(s: &Value) -> Self {
        let mut table: Vec<Tier> = s["table"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|t| {
                        Some(Tier {
                            min_bytes: gib(t["min_downloaded_gib"].as_f64()?),
                            required: t["required"].as_f64()?.clamp(0.0, 10.0),
                        })
                    })
                    .collect()
            })
            .filter(|t: &Vec<Tier>| !t.is_empty())
            .unwrap_or_else(|| DEFAULT_TABLE.iter().map(|(g, r)| Tier { min_bytes: gib(*g), required: *r }).collect());
        table.sort_by_key(|t| t.min_bytes);
        let int = |k: &str, d: i64| s[k].as_i64().unwrap_or(d).max(0);
        Self {
            table,
            seeding_reduction: s["seeding_reduction"].as_f64().unwrap_or(1.0).clamp(0.0, 1.0),
            exempt_below_bytes: gib(s["exempt_below_gib"].as_f64().unwrap_or(5.0)),
            grace_days: int("grace_days", 7),
            exempt_class_ids: s["exempt_class_ids"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_i64).collect())
                .unwrap_or_default(),
            warning_days: int("warning_days", 14).max(1),
            ban_after_days: int("ban_after_days", 0),
            block_new_downloads: s["block_new_downloads_during_warning"].as_bool().unwrap_or(false),
            notify: s["notify"].as_bool().unwrap_or(true),
        }
    }
}

/// 要求分享率：所在档位 × (1 − 减免 × 做种比例)，保留 3 位小数
pub fn required(downloaded: u64, seeding_ratio: f64, s: &Settings) -> f64 {
    let base = s.table.iter().rev().find(|t| downloaded >= t.min_bytes).map_or(0.0, |t| t.required);
    let r = base * (1.0 - s.seeding_reduction * seeding_ratio.clamp(0.0, 1.0));
    (r * 1000.0).round() / 1000.0
}

/// 分享率；没有下载为 None（无限）
pub fn ratio(up: u64, down: u64) -> Option<f64> {
    #[allow(clippy::cast_precision_loss)]
    (down > 0).then(|| up as f64 / down as f64)
}

/// 达到要求还需上传的字节数
pub fn upload_needed(up: u64, down: u64, req: f64) -> u64 {
    #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let need = (req * down as f64).ceil() as u64;
    need.saturating_sub(up)
}

#[derive(Debug, Clone, Default)]
pub struct Metrics {
    pub class_id: i64,
    pub created_ms: i64,
    pub uploaded: u64,
    pub downloaded: u64,
    pub completed: u64,
    pub seeding_completed: u64,
}

impl Metrics {
    fn seeding_ratio(&self) -> f64 {
        #[allow(clippy::cast_precision_loss)]
        if self.completed == 0 { 0.0 } else { self.seeding_completed as f64 / self.completed as f64 }
    }
}

pub fn exempt(m: &Metrics, exempt_until_ms: Option<i64>, now_ms: i64, s: &Settings) -> Option<&'static str> {
    if exempt_until_ms.is_some_and(|t| t > now_ms) {
        return Some("manual");
    }
    if m.downloaded < s.exempt_below_bytes {
        return Some("low_download");
    }
    if now_ms - m.created_ms < s.grace_days * DAY_MS {
        return Some("grace");
    }
    if s.exempt_class_ids.contains(&m.class_id) {
        return Some("class");
    }
    None
}

// ---------------- 数据 ----------------

fn settings() -> Settings {
    Settings::from(&ctx::settings())
}

fn u(v: Option<i64>) -> u64 {
    u64::try_from(v.unwrap_or(0)).unwrap_or(0)
}

fn metrics_of(ids: &[i64]) -> R<Vec<(i64, Metrics)>> {
    let r = sql::query(
        "SELECT u.id, u.class_id, (extract(epoch FROM u.created_at) * 1000)::bigint,
                coalesce(t.uploaded, 0), coalesce(t.downloaded, 0), coalesce(t.completed_count, 0), coalesce(t.seeding_completed_count, 0)
           FROM identity.v_user_public u LEFT JOIN accounting.v_user_totals t ON t.user_id = u.id
          WHERE u.id = ANY($1) AND u.status <> 'deleted'",
        &[SqlValue::IntArray(ids.to_vec())],
    )?;
    Ok(r.rows
        .iter()
        .map(|row| {
            (
                row.int(0).unwrap_or(0),
                Metrics {
                    class_id: row.int(1).unwrap_or(0),
                    created_ms: row.int(2).unwrap_or(0),
                    uploaded: u(row.int(3)),
                    downloaded: u(row.int(4)),
                    completed: u(row.int(5)),
                    seeding_completed: u(row.int(6)),
                },
            )
        })
        .collect())
}

#[derive(Debug, Clone, Default)]
struct State {
    state: String,
    manual: bool,
    deadline_ms: Option<i64>,
    restricted_ms: Option<i64>,
    seq: i64,
    exempt_until_ms: Option<i64>,
}

fn load_state(uid: i64) -> R<State> {
    sql::execute("INSERT INTO states (user_id) VALUES ($1) ON CONFLICT DO NOTHING", &[SqlValue::Int(uid)])?;
    let r = sql::query(
        "SELECT state, manual, (extract(epoch FROM deadline) * 1000)::bigint, (extract(epoch FROM restricted_at) * 1000)::bigint,
                restricted_seq, (extract(epoch FROM exempt_until) * 1000)::bigint
           FROM states WHERE user_id = $1 FOR UPDATE",
        &[SqlValue::Int(uid)],
    )?;
    let row = r.rows.first().ok_or_else(|| err("ratio-policy.internal", "state row missing"))?;
    Ok(State {
        state: row.text(0).unwrap_or_else(|| "ok".into()),
        manual: row.bool(1).unwrap_or(false),
        deadline_ms: row.int(2),
        restricted_ms: row.int(3),
        seq: row.int(4).unwrap_or(0),
        exempt_until_ms: row.int(5),
    })
}

fn ts(ms: Option<i64>) -> SqlValue {
    // SqlValue::Timestamp 为 Unix 微秒
    ms.map_or(SqlValue::Null, |m| SqlValue::Timestamp(m * 1000))
}

fn f(v: Option<f64>) -> SqlValue {
    v.filter(|x| x.is_finite()).map_or(SqlValue::Null, SqlValue::Float)
}

#[allow(clippy::too_many_arguments)]
fn transition(uid: i64, from: &str, to: &str, ratio: Option<f64>, req: f64, actor: &str, reason: &str) -> R<()> {
    sql::execute(
        "INSERT INTO history (user_id, from_state, to_state, ratio, required, actor, reason) VALUES ($1, $2, $3, $4, $5, $6, $7)",
        &[
            SqlValue::Int(uid),
            SqlValue::Text(from.into()),
            SqlValue::Text(to.into()),
            f(ratio),
            SqlValue::Float(req),
            SqlValue::Text(actor.into()),
            SqlValue::Text(reason.into()),
        ],
    )?;
    Ok(())
}

fn publish(event: &str, uid: i64, payload: &Value) -> R<()> {
    ctx::publish(event, &format!("user:{uid}"), payload)?;
    Ok(())
}

fn notify(s: &Settings, uid: i64, key: &str, title: &str, body: &str, idem: &str) {
    if !s.notify {
        return;
    }
    let r = services::call(
        "ctx.notify",
        "send",
        &json!({ "user_id": uid, "template_key": format!("ratio-policy.{key}"), "category": "account", "title": title,
                 "body": body, "link": "/p/ratio-policy/", "idempotency_key": idem })
        .to_string(),
    );
    if let Err(e) = r {
        logging::debug("ratio notice not sent", &[("error", &e.message)]);
    }
}

fn fmt_ratio(r: Option<f64>) -> String {
    r.map_or_else(|| "∞".into(), |r| format!("{r:.3}"))
}

/// 经 identity 施加 / 解除本插件的限制；员工受保护时返回 false（记为 restricted_skipped）
fn restrict(uid: i64, kind: &str) -> R<bool> {
    match ctx::call(
        "ctx.users",
        "apply_restriction",
        &json!({ "user": uid, "kind": kind, "reason_code": "ratio-policy.below_required" }),
    ) {
        Ok(_) => Ok(true),
        Err(e) if e.code == "identity.target_protected" => Ok(false),
        Err(e) => Err(e),
    }
}

fn lift(uid: i64, kind: Option<&str>) -> R<()> {
    let mut a = json!({ "user": uid, "reason": "ratio-policy.recovered" });
    if let Some(k) = kind {
        a["kind"] = json!(k);
    }
    ctx::call("ctx.users", "lift_restriction", &a)?;
    Ok(())
}

/// 评估一个用户（03 §1.3 / 02 §5 状态机）
fn evaluate(uid: i64, m: &Metrics, s: &Settings, now: i64) -> R<()> {
    let st = load_state(uid)?;
    let req = required(m.downloaded, m.seeding_ratio(), s);
    let r = ratio(m.uploaded, m.downloaded);
    let ex = exempt(m, st.exempt_until_ms, now, s);
    let good = ex.is_some() || r.is_none_or(|r| r >= req);
    let set = |sql_text: &str, extra: &[SqlValue]| -> R<()> {
        let mut params = vec![SqlValue::Int(uid), f(r), SqlValue::Float(req)];
        params.extend_from_slice(extra);
        sql::execute(sql_text, &params)?;
        Ok(())
    };
    let cur = st.state.as_str();
    match (cur, good) {
        ("ok", false) => {
            let deadline = now + s.warning_days * DAY_MS;
            set(
                "UPDATE states SET state = 'warned', manual = false, warned_at = now(), deadline = $4, last_ratio = $2, last_required = $3,
                        evaluated_at = now(), updated_at = now() WHERE user_id = $1",
                &[ts(Some(deadline))],
            )?;
            transition(uid, cur, "warned", r, req, "system", "below_required")?;
            publish("ratio.warned", uid, &json!({ "user_id": uid, "ratio": r, "required": req, "deadline": deadline, "manual": false }))?;
            notify(
                s,
                uid,
                "warned",
                "分享率警告 / Ratio warning",
                &format!(
                    "你的分享率 {} 低于要求 {req:.3}，请在 {} 天内上传 {} GiB 以上，否则将被禁止下载。\nYour ratio {} is below the required {req:.3}; upload at least {} GiB within {} days or downloading will be disabled.",
                    fmt_ratio(r),
                    s.warning_days,
                    upload_gib(m, req),
                    fmt_ratio(r),
                    upload_gib(m, req),
                    s.warning_days
                ),
                &format!("ratio:n:{uid}:warned:{}", st.seq),
            );
        }
        ("warned", true) if !st.manual || st.deadline_ms.is_some_and(|d| now >= d) => {
            set(
                "UPDATE states SET state = 'ok', manual = false, deadline = NULL, last_ratio = $2, last_required = $3, evaluated_at = now(), updated_at = now()
                  WHERE user_id = $1",
                &[],
            )?;
            transition(uid, cur, "ok", r, req, "system", "recovered")?;
            publish("ratio.warning_lifted", uid, &json!({ "user_id": uid, "ratio": r, "cause": "recovered" }))?;
        }
        ("warned", false) if st.deadline_ms.is_some_and(|d| now >= d) => {
            let ok = restrict(uid, "leech_disabled")?;
            let to = if ok { "restricted" } else { "restricted_skipped" };
            set(
                &format!(
                    "UPDATE states SET state = '{to}', restricted_at = now(), restricted_seq = restricted_seq + 1, last_ratio = $2, last_required = $3,
                            evaluated_at = now(), updated_at = now() WHERE user_id = $1"
                ),
                &[],
            )?;
            transition(uid, cur, to, r, req, "system", "deadline_passed")?;
            if ok {
                publish("ratio.restricted", uid, &json!({ "user_id": uid, "ratio": r, "required": req, "kind": "leech_disabled" }))?;
                notify(
                    s,
                    uid,
                    "restricted",
                    "已因分享率不足被禁止下载 / Downloading disabled",
                    &format!(
                        "警告期已过，你的分享率 {} 仍低于要求 {req:.3}，下载已被禁止。上传达标后会自动恢复。\nYour ratio {} is still below {req:.3}; downloading is disabled until it recovers.",
                        fmt_ratio(r),
                        fmt_ratio(r)
                    ),
                    &format!("ratio:n:{uid}:restricted:{}", st.seq + 1),
                );
            }
        }
        ("restricted" | "restricted_skipped", true) => {
            if cur == "restricted" {
                lift(uid, Some("leech_disabled"))?;
            }
            set(
                "UPDATE states SET state = 'ok', manual = false, deadline = NULL, restricted_at = NULL, last_ratio = $2, last_required = $3,
                        evaluated_at = now(), updated_at = now() WHERE user_id = $1",
                &[],
            )?;
            transition(uid, cur, "ok", r, req, "system", "recovered")?;
            publish("ratio.restriction_lifted", uid, &json!({ "user_id": uid, "ratio": r, "cause": "recovered" }))?;
        }
        ("restricted", false) if s.ban_after_days > 0 && st.restricted_ms.is_some_and(|t| now >= t + s.ban_after_days * DAY_MS) => {
            if restrict(uid, "ban")? {
                set(
                    "UPDATE states SET state = 'banned', last_ratio = $2, last_required = $3, evaluated_at = now(), updated_at = now() WHERE user_id = $1",
                    &[],
                )?;
                transition(uid, cur, "banned", r, req, "system", "ban_after_days")?;
                publish("ratio.restricted", uid, &json!({ "user_id": uid, "ratio": r, "required": req, "kind": "banned" }))?;
            }
        }
        ("banned", true) => {
            lift(uid, None)?;
            set(
                "UPDATE states SET state = 'ok', manual = false, deadline = NULL, restricted_at = NULL, last_ratio = $2, last_required = $3,
                        evaluated_at = now(), updated_at = now() WHERE user_id = $1",
                &[],
            )?;
            transition(uid, cur, "ok", r, req, "system", "recovered")?;
            publish("ratio.restriction_lifted", uid, &json!({ "user_id": uid, "ratio": r, "cause": "recovered" }))?;
        }
        _ => set(
            "UPDATE states SET last_ratio = $2, last_required = $3, evaluated_at = now() WHERE user_id = $1",
            &[],
        )?,
    }
    Ok(())
}

fn upload_gib(m: &Metrics, req: f64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let g = upload_needed(m.uploaded, m.downloaded, req) as f64 / GIB;
    format!("{g:.2}")
}

fn evaluate_batch(ids: &[i64]) -> R<()> {
    let s = settings();
    let now = time::now();
    let mut ms = metrics_of(ids)?;
    ms.sort_by_key(|(id, _)| *id);
    for (uid, m) in &ms {
        evaluate(*uid, m, &s, now)?;
    }
    Ok(())
}

fn incremental() -> R<()> {
    loop {
        let r = sql::query(
            "DELETE FROM dirty_users WHERE user_id IN (SELECT user_id FROM dirty_users ORDER BY since LIMIT 2000 FOR UPDATE SKIP LOCKED)
             RETURNING user_id",
            &[],
        )?;
        let ids: Vec<i64> = r.rows.iter().filter_map(|row| row.int(0)).collect();
        if ids.is_empty() {
            return Ok(());
        }
        evaluate_batch(&ids)?;
        sql::commit()?;
    }
}

/// 全量：所有用户（键集分页）；状态非 ok 的用户另外再评估一遍以处理警告到期
fn full() -> R<()> {
    let mut last = 0i64;
    loop {
        let r = sql::query(
            "SELECT id FROM identity.v_user_public WHERE id > $1 AND status <> 'deleted' ORDER BY id LIMIT 2000",
            &[SqlValue::Int(last)],
        )?;
        let ids: Vec<i64> = r.rows.iter().filter_map(|row| row.int(0)).collect();
        let Some(max) = ids.last().copied() else { break };
        evaluate_batch(&ids)?;
        sql::commit()?;
        last = max;
    }
    sql::execute("DELETE FROM history WHERE at < now() - interval '2 years'", &[])?;
    sql::commit()?;
    Ok(())
}

// ---------------- 状态查询 ----------------

fn status(uid: i64) -> R<Value> {
    let s = settings();
    let m = metrics_of(&[uid])?.pop().map(|(_, m)| m).unwrap_or_default();
    let r = sql::query(
        "SELECT state, (extract(epoch FROM deadline) * 1000)::bigint, (extract(epoch FROM exempt_until) * 1000)::bigint, exempt_reason
           FROM states WHERE user_id = $1",
        &[SqlValue::Int(uid)],
    )?;
    let row = r.rows.first();
    let state = row.and_then(|x| x.text(0)).unwrap_or_else(|| "ok".into());
    let exempt_until = row.and_then(|x| x.int(2));
    let req = required(m.downloaded, m.seeding_ratio(), &s);
    let ex = exempt(&m, exempt_until, time::now(), &s);
    let shown_state = if state == "restricted_skipped" { "ok" } else { state.as_str() };
    Ok(json!({
        "ratio": ratio(m.uploaded, m.downloaded),
        "required": req,
        "upload_needed": if ex.is_some() { 0 } else { upload_needed(m.uploaded, m.downloaded, req) },
        "state": shown_state,
        "warned_until": if state == "warned" { row.and_then(|x| x.int(1)) } else { None },
        "exempt_reason": ex,
        "can_download": !matches!(state.as_str(), "restricted" | "banned"),
        "table": s.table.iter().map(|t| {
            #[allow(clippy::cast_precision_loss)]
            let g = t.min_bytes as f64 / GIB;
            json!({ "min_downloaded_gib": g, "required": t.required })
        }).collect::<Vec<_>>(),
        "seeding_reduction_applied": ((s.seeding_reduction * m.seeding_ratio()) * 1000.0).round() / 1000.0,
    }))
}

// ---------------- 钩子 ----------------

fn state_of(uid: i64) -> R<String> {
    let r = sql::query("SELECT state FROM states WHERE user_id = $1", &[SqlValue::Int(uid)])?;
    Ok(r.rows.first().and_then(|row| row.text(0)).unwrap_or_else(|| "ok".into()))
}

fn deny(code: &str, message: &str) -> hooks::HookDecision {
    hooks::HookDecision::Deny(hooks::DenyInfo {
        code: code.into(),
        message: message.into(),
        details: None,
    })
}

impl hooks::Guest for Ratio {
    fn handle(c: hooks::HookCall) -> R<hooks::HookDecision> {
        let input: Value = serde_json::from_str(&c.input).unwrap_or(Value::Null);
        match c.hook.as_str() {
            "torrent.before_download" => {
                let (Some(uid), Some(tid)) = (input["user_id"].as_i64(), input["torrent_id"].as_i64()) else {
                    return Ok(hooks::HookDecision::Allow);
                };
                if !settings().block_new_downloads || state_of(uid)? != "warned" {
                    return Ok(hooks::HookDecision::Allow);
                }
                let seen = sql::query(
                    "SELECT 1 FROM accounting.v_snatch WHERE user_id = $1 AND torrent_id = $2",
                    &[SqlValue::Int(uid), SqlValue::Int(tid)],
                )?;
                if seen.rows.is_empty() {
                    return Ok(deny(
                        "ratio-policy.download_restricted",
                        "new downloads are disabled during a ratio warning",
                    ));
                }
                Ok(hooks::HookDecision::Allow)
            }
            "invite.before_issue" => {
                let Some(uid) = input["inviter_id"].as_i64() else {
                    return Ok(hooks::HookDecision::Allow);
                };
                if matches!(state_of(uid)?.as_str(), "warned" | "restricted" | "banned") {
                    return Ok(deny("ratio-policy.restricted", "users under a ratio warning cannot send invites"));
                }
                Ok(hooks::HookDecision::Allow)
            }
            _ => Ok(hooks::HookDecision::Allow),
        }
    }
}

impl on_event::Guest for Ratio {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        if evt.event_type == "traffic.settled"
            && let Some(uid) = p["user_id"].as_i64()
        {
            sql::execute(
                "INSERT INTO dirty_users (user_id) VALUES ($1) ON CONFLICT DO NOTHING",
                &[SqlValue::Int(uid)],
            )?;
        }
        Ok(())
    }
}

impl on_cron::Guest for Ratio {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            "incr_evaluate" => incremental(),
            "daily_evaluate" => full(),
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

impl service_provider::Guest for Ratio {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        let uid = a["user"].as_i64().ok_or_else(|| err("kernel.validation_failed", "user"))?;
        match method.as_str() {
            "status" => Ok(status(uid)?.to_string()),
            other => Err(err("kernel.not_found", &format!("ctx.ratio has no method `{other}`"))),
        }
    }
}

// ---------------- 后台 ----------------

fn ms_of(v: &Value) -> Option<i64> {
    v.as_i64()
}

fn admin_warn(uid: i64, b: &Value) -> R<()> {
    let deadline = ms_of(&b["deadline"]).ok_or_else(|| err("kernel.validation_failed", "deadline (Unix ms) is required"))?;
    if deadline <= time::now() {
        return Err(err("kernel.validation_failed", "deadline must be in the future"));
    }
    let reason = b["reason"].as_str().unwrap_or("").chars().take(500).collect::<String>();
    let st = load_state(uid)?;
    sql::execute(
        "UPDATE states SET state = 'warned', manual = true, warned_at = now(), deadline = $2, updated_at = now() WHERE user_id = $1",
        &[SqlValue::Int(uid), ts(Some(deadline))],
    )?;
    transition(uid, &st.state, "warned", None, 0.0, &format!("user:{}", ctx::current_user()?), &reason)?;
    publish("ratio.warned", uid, &json!({ "user_id": uid, "ratio": null, "required": null, "deadline": deadline, "manual": true }))?;
    notify(
        &settings(),
        uid,
        "warned",
        "分享率警告 / Ratio warning",
        &format!("管理员对你发出了分享率警告：{reason}\nStaff issued a ratio warning: {reason}"),
        &format!("ratio:n:{uid}:manual:{deadline}"),
    );
    audit_write("ratio-policy.warn", uid, &json!({ "deadline": deadline, "reason": reason }))
}

fn admin_lift(uid: i64, b: &Value) -> R<()> {
    let st = load_state(uid)?;
    match st.state.as_str() {
        "warned" | "restricted_skipped" => {}
        "restricted" => lift(uid, Some("leech_disabled"))?,
        "banned" => lift(uid, None)?,
        _ => return Err(err("ratio-policy.not_warned", "the user is not under a ratio warning")),
    }
    sql::execute(
        "UPDATE states SET state = 'ok', manual = false, deadline = NULL, restricted_at = NULL, updated_at = now() WHERE user_id = $1",
        &[SqlValue::Int(uid)],
    )?;
    let reason = b["reason"].as_str().unwrap_or("").chars().take(500).collect::<String>();
    transition(uid, &st.state, "ok", None, 0.0, &format!("user:{}", ctx::current_user()?), &reason)?;
    publish(
        if st.state == "warned" { "ratio.warning_lifted" } else { "ratio.restriction_lifted" },
        uid,
        &json!({ "user_id": uid, "ratio": null, "cause": "manual" }),
    )?;
    audit_write("ratio-policy.lift", uid, &json!({ "from": st.state, "reason": reason }))
}

fn admin_exempt(uid: i64, b: &Value) -> R<()> {
    let until = ms_of(&b["until"]).ok_or_else(|| err("kernel.validation_failed", "until (Unix ms) is required"))?;
    let reason = b["reason"].as_str().unwrap_or("").chars().take(500).collect::<String>();
    let st = load_state(uid)?;
    sql::execute(
        "UPDATE states SET exempt_until = $2, exempt_reason = $3, updated_at = now() WHERE user_id = $1",
        &[SqlValue::Int(uid), ts(Some(until)), SqlValue::Text(reason.clone())],
    )?;
    audit_write("ratio-policy.exempt", uid, &json!({ "until": until, "reason": reason }))?;
    // 正在警告 / 限制中：按新的豁免立即重新评估
    if st.state != "ok" {
        evaluate_batch(&[uid])?;
    }
    Ok(())
}

fn audit_write(action: &str, uid: i64, details: &Value) -> R<()> {
    audit::write(&audit::AuditEntry {
        action: action.into(),
        target: format!("user:{uid}"),
        details: details.to_string(),
    })
}

fn admin_list(state: &str) -> R<Value> {
    let r = sql::query(
        "SELECT s.user_id, u.username, s.state, s.last_ratio, s.last_required, (extract(epoch FROM s.deadline) * 1000)::bigint, s.manual
           FROM states s JOIN identity.v_user_public u ON u.id = s.user_id
          WHERE s.state = $1 ORDER BY s.deadline NULLS LAST, s.user_id LIMIT 500",
        &[SqlValue::Text(state.into())],
    )?;
    let items: Vec<Value> = r
        .rows
        .iter()
        .map(|row| {
            json!({ "user_id": row.int(0), "username": row.text(1), "state": row.text(2), "ratio": row.float(3),
                    "required": row.float(4), "deadline": row.int(5), "manual": row.bool(6) })
        })
        .collect();
    Ok(json!({ "items": items }))
}

impl http_handler::Guest for Ratio {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let reply = match req.route() {
            "me" => http::Reply::ok(&status(ctx::current_user()?)?),
            "admin_status" => http::Reply::ok(&status(req.param_i64("id")?)?),
            "admin_list" => {
                let st = req.query("state").unwrap_or("warned");
                if !matches!(st, "warned" | "restricted" | "banned" | "restricted_skipped") {
                    return Err(err("kernel.validation_failed", "state"));
                }
                http::Reply::ok(&admin_list(st)?)
            }
            "admin_warn" => {
                admin_warn(req.param_i64("id")?, &req.body_json::<Value>()?)?;
                http::Reply::no_content()
            }
            "admin_lift" => {
                admin_lift(req.param_i64("id")?, &req.body_json::<Value>().unwrap_or(Value::Null))?;
                http::Reply::no_content()
            }
            "admin_exempt" => {
                admin_exempt(req.param_i64("id")?, &req.body_json::<Value>()?)?;
                http::Reply::no_content()
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(Ratio);

#[cfg(test)]
mod tests {
    use super::*;

    fn gb(g: f64) -> u64 {
        gib(g)
    }

    #[test]
    fn requirements() {
        let s = Settings::from(&json!({}));
        assert!((required(gb(3.0), 0.0, &s) - 0.0).abs() < 1e-9);
        assert!((required(gb(12.0), 0.0, &s) - 0.2).abs() < 1e-9);
        assert!((required(gb(500.0), 0.0, &s) - 0.6).abs() < 1e-9);
        // 一半已完成的种子仍在做种：要求减半
        assert!((required(gb(12.0), 0.5, &s) - 0.1).abs() < 1e-9);
        let s2 = Settings::from(&json!({ "seeding_reduction": 0, "table": [{ "min_downloaded_gib": 0, "required": 0.3 }] }));
        assert!((required(gb(1.0), 1.0, &s2) - 0.3).abs() < 1e-9);
        assert_eq!(ratio(5, 0), None);
        assert_eq!(upload_needed(10, 100, 0.3), 20);
        assert_eq!(upload_needed(40, 100, 0.3), 0);
    }

    #[test]
    fn exemptions() {
        let s = Settings::from(&json!({ "exempt_class_ids": [8] }));
        let now = 100 * DAY_MS;
        let m = Metrics { downloaded: gb(10.0), created_ms: 0, ..Default::default() };
        assert_eq!(exempt(&m, None, now, &s), None);
        assert_eq!(exempt(&m, Some(now + 1), now, &s), Some("manual"));
        assert_eq!(exempt(&Metrics { downloaded: gb(1.0), ..m.clone() }, None, now, &s), Some("low_download"));
        assert_eq!(exempt(&Metrics { created_ms: now - DAY_MS, ..m.clone() }, None, now, &s), Some("grace"));
        assert_eq!(exempt(&Metrics { class_id: 8, ..m }, None, now, &s), Some("class"));
    }
}
