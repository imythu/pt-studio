//! 处置与期望状态协调（LLD §1.8）、一次性外部操作（§1.9）、到期、外部覆盖检测。
//!
//! 决定事务只记录“想要什么”（sanctions → effects.desired + rev），外部调用在延迟任务 `run_effects` 中以
//! `ach:eff:<user>:<effect>:<rev>` 为幂等键执行并回写结果。

use pt_plugin_sdk::{
    R, ctx as pctx, err,
    serde_json::{Value, json},
};

use crate::db::{self, DAY, RowGet, i, j, ms, now, oi, q, q1, t, ts, x};

pub const EFFECTS: [&str; 4] = ["ban", "leech", "freeze", "wallet"];

pub fn kind_of(effect: &str) -> &'static str {
    match effect {
        "ban" => "ban",
        "leech" => "disable_download",
        "freeze" => "freeze_traffic",
        _ => "lock_wallet",
    }
}

pub fn effect_of(kind: &str) -> Option<&'static str> {
    match kind {
        "ban" => Some("ban"),
        "disable_download" => Some("leech"),
        "freeze_traffic" => Some("freeze"),
        "lock_wallet" => Some("wallet"),
        _ => None,
    }
}

pub fn severity_rank(kind: &str) -> i32 {
    match kind {
        "ban" => 7,
        "disable_download" => 6,
        "freeze_traffic" => 5,
        "lock_wallet" => 4,
        "adjust_traffic" => 3,
        "reset_passkey" => 2,
        "warn" => 1,
        _ => 0,
    }
}

pub fn sanction_changed(
    id: i64,
    user: i64,
    case_id: Option<i64>,
    kind: &str,
    op: &str,
    until: Option<i64>,
) -> R<()> {
    let mut p = json!({ "sanction_id": id, "user_id": user, "kind": kind, "op": op });
    if let Some(c) = case_id {
        p["case_id"] = json!(c);
    }
    if let Some(u) = until {
        p["until"] = json!(u);
    }
    db::publish("anticheat.sanction_changed", user, &p)?;
    if let Some(c) = case_id {
        crate::core::timeline(
            c,
            "sanction_changed",
            None,
            &json!({ "sanction_id": id, "kind": kind, "op": op }),
        )?;
    }
    db::counter("sanctions_changed_total", &[("kind", kind), ("op", op)]);
    Ok(())
}

/// 写入处置；一次性处置（警告、流量回收）登记操作；有期限的安排到期任务
#[allow(clippy::too_many_arguments)]
pub fn insert_sanction(
    user: i64,
    case_id: Option<i64>,
    kind: &str,
    state: &str,
    until: Option<i64>,
    params: &Value,
    policy_id: Option<&str>,
    decided_by: Option<i64>,
) -> R<i64> {
    let id = q1(
        &format!(
            "INSERT INTO sanctions (user_id, case_id, kind, state, until, params, policy_id, decided_by)
             VALUES ($1, $2, $3, $4, CASE WHEN $5::bigint IS NULL THEN NULL ELSE {} END, $6, $7, $8)
             ON CONFLICT DO NOTHING RETURNING id",
            ts(5)
        ),
        &[
            i(user),
            oi(case_id),
            t(kind),
            t(state),
            oi(until),
            j(params),
            db::ot(policy_id),
            oi(decided_by),
        ],
    )?
    .ok_or_else(|| err("kernel.conflict", "sanction already exists"))?
    .n(0);
    if let Some(u) = until {
        pctx::schedule(
            "sanction_expire",
            u,
            &json!({ "sanction_id": id }),
            Some(&format!("sx:{id}:{u}")),
        )?;
    }
    if state == "pending" {
        enqueue_for(id, user, kind, params)?;
    }
    db::counter(
        "sanctions_total",
        &[
            ("kind", kind),
            ("by", if policy_id.is_some() { "policy" } else { "user" }),
        ],
    );
    Ok(id)
}

/// 处置变为 pending 时（含批准后）登记一次性操作
pub fn enqueue_for(sanction_id: i64, user: i64, kind: &str, params: &Value) -> R<()> {
    match kind {
        "warn" => enqueue(user, Some(sanction_id), None, "warn_notice", &json!({}))?,
        "adjust_traffic" => enqueue(
            user,
            Some(sanction_id),
            None,
            "adjust",
            &json!({ "up_bytes": params["up_bytes"] }),
        )?,
        "reset_passkey" => enqueue(user, Some(sanction_id), None, "passkey_reset", &json!({}))?,
        _ => {}
    }
    Ok(())
}

pub fn enqueue(
    user: i64,
    sanction_id: Option<i64>,
    appeal_id: Option<i64>,
    op: &str,
    payload: &Value,
) -> R<()> {
    x(
        "INSERT INTO effect_ops (user_id, sanction_id, appeal_id, op, payload, state) VALUES ($1, $2, $3, $4, $5, 'pending')
         ON CONFLICT DO NOTHING",
        &[i(user), oi(sanction_id), oi(appeal_id), t(op), j(payload)],
    )?;
    Ok(())
}

/// 按生效处置合成期望效果；有变化时递增修订号并安排执行器（在调用方事务内）
pub fn bump(user: i64) -> R<()> {
    let mut sum = 0i64;
    let mut changed = false;
    for eff in EFFECTS {
        let live = q(
            &format!(
                "SELECT s.id, s.case_id, {}, {}, c.primary_rule FROM sanctions s LEFT JOIN cases c ON c.id = s.case_id
                  WHERE s.user_id = $1 AND s.kind = $2 AND s.state IN ('pending','active') ORDER BY s.id",
                ms("s.until"),
                ms("s.created_at")
            ),
            &[i(user), t(kind_of(eff))],
        )?;
        let desired = if live.is_empty() {
            json!({ "active": false })
        } else {
            let until = if live.iter().any(|r| r.on(2).is_none()) {
                Value::Null
            } else {
                json!(live.iter().map(|r| r.n(2)).max())
            };
            let last = live
                .iter()
                .max_by_key(|r| r.on(1).unwrap_or(0))
                .unwrap_or(&live[0]);
            json!({ "active": true, "until": until, "case_id": last.on(1),
                    "reason_code": format!("anticheat.{}", last.os(4).unwrap_or_else(|| "manual".into())),
                    "since": live.iter().map(|r| r.n(3)).min() })
        };
        let cur = q1(
            &format!(
                "SELECT desired, state, {}, rev FROM effects WHERE user_id = $1 AND effect = $2 FOR UPDATE",
                ms("overridden_at")
            ),
            &[i(user), t(eff)],
        )?;
        match &cur {
            None if desired["active"] != json!(true) => continue,
            Some(c) => {
                sum += c.n(3);
                if c.s(1) == "overridden" {
                    // 尊重管理员的外部解除：只有之后新建的处置才重新施加
                    let newer = live.iter().any(|r| r.n(3) > c.on(2).unwrap_or(0));
                    if !newer {
                        continue;
                    }
                } else if c.v(0) == desired {
                    continue;
                }
            }
            None => {}
        }
        x(
            "INSERT INTO effects (user_id, effect, desired, rev, state) VALUES ($1, $2, $3, 1, 'pending')
             ON CONFLICT (user_id, effect) DO UPDATE SET desired = EXCLUDED.desired, rev = effects.rev + 1, state = 'pending',
                attempts = 0, next_attempt_at = now(), overridden_at = NULL, updated_at = now()",
            &[i(user), t(eff), j(&desired)],
        )?;
        sum += 1;
        changed = true;
    }
    let ops = q1(
        "SELECT count(*) FROM effect_ops WHERE user_id = $1 AND state IN ('pending','retrying')",
        &[i(user)],
    )?
    .map_or(0, |r| r.n(0));
    if changed || ops > 0 {
        pctx::schedule(
            "run_effects",
            now(),
            &json!({ "user_id": user }),
            Some(&format!("eff:{user}:{sum}:{ops}:{}", now() / 1000)),
        )?;
    }
    Ok(())
}

fn backoff(attempts: i64) -> i64 {
    (30_000i64 << attempts.clamp(0, 7)).min(3_600_000)
}

/// 执行器（延迟任务 `run_effects`，tx manual）：逐个效果落地，每个效果一个事务
pub fn run(user: i64) -> R<()> {
    let rows = q(
        "SELECT effect, desired, rev, ext_ref, attempts FROM effects
          WHERE user_id = $1 AND applied_rev < rev AND state IN ('pending','retrying') AND next_attempt_at <= now()
          ORDER BY effect",
        &[i(user)],
    )?;
    for r in rows {
        let (eff, desired, rev, ext, attempts) = (r.s(0), r.v(1), r.n(2), r.os(3), r.n(4));
        let key = format!("ach:eff:{user}:{eff}:{rev}");
        match apply(user, &eff, &desired, ext.as_deref(), &key) {
            Ok(new_ext) => {
                let ext_ref = if desired["active"] == json!(true) {
                    new_ext.or(ext)
                } else {
                    None
                };
                x(
                    "UPDATE effects SET applied_rev = $3, applied = desired, ext_ref = $4, state = 'synced', last_error = NULL,
                            attempts = 0, updated_at = now()
                      WHERE user_id = $1 AND effect = $2 AND rev = $3",
                    &[i(user), t(&eff), i(rev), db::ot(ext_ref.as_deref())],
                )?;
                if desired["active"] == json!(true) {
                    let act = q(
                        &format!(
                            "UPDATE sanctions SET state = 'active', activated_at = now(), updated_at = now()
                              WHERE user_id = $1 AND kind = $2 AND state = 'pending' RETURNING id, case_id, {}",
                            ms("until")
                        ),
                        &[i(user), t(kind_of(&eff))],
                    )?;
                    for s in &act {
                        sanction_changed(s.n(0), user, s.on(1), kind_of(&eff), "applied", s.on(2))?;
                        let op = if eff == "ban" {
                            "ban_notice"
                        } else {
                            "sanction_notice"
                        };
                        enqueue(
                            user,
                            Some(s.n(0)),
                            None,
                            op,
                            &json!({ "kind": kind_of(&eff), "until": s.on(2), "case_id": s.on(1) }),
                        )?;
                    }
                }
            }
            Err(e) if db::retryable(&e) && attempts < 12 => {
                let next = now() + backoff(attempts);
                x(
                    &format!(
                        "UPDATE effects SET attempts = attempts + 1, state = 'retrying', next_attempt_at = {}, last_error = $3, updated_at = now()
                          WHERE user_id = $1 AND effect = $2",
                        ts(4)
                    ),
                    &[i(user), t(&eff), t(&e.code), i(next)],
                )?;
                pctx::schedule(
                    "run_effects",
                    next,
                    &json!({ "user_id": user }),
                    Some(&format!("eff:{user}:{eff}:{rev}:{attempts}")),
                )?;
            }
            Err(e) => {
                x(
                    "UPDATE effects SET state = 'failed', last_error = $3, updated_at = now() WHERE user_id = $1 AND effect = $2",
                    &[i(user), t(&eff), t(&e.code)],
                )?;
                let failed = q(
                    "UPDATE sanctions SET state = 'failed', updated_at = now()
                      WHERE user_id = $1 AND kind = $2 AND state = 'pending' RETURNING id, case_id",
                    &[i(user), t(kind_of(&eff))],
                )?;
                for s in &failed {
                    sanction_changed(s.n(0), user, s.on(1), kind_of(&eff), "failed", None)?;
                }
                db::audit(
                    "anticheat.effect_failed",
                    &format!("user:{user}"),
                    &json!({ "effect": eff, "code": e.code }),
                )?;
                db::counter("effect_failures_total", &[("effect", &eff), ("code", &e.code)]);
            }
        }
        db::commit()?;
    }
    run_ops(user)?;
    db::commit()?;
    Ok(())
}

fn apply(user: i64, eff: &str, d: &Value, ext: Option<&str>, key: &str) -> R<Option<String>> {
    let active = d["active"] == json!(true);
    let case = d["case_id"].as_i64().unwrap_or(0);
    match (eff, active) {
        ("ban" | "leech", true) => {
            let mut a = json!({ "user": user, "kind": if eff == "ban" { "ban" } else { "leech_disabled" },
                                "reason_code": d["reason_code"], "note": format!("case:{case}"), "idempotency_key": key });
            if let Some(u) = d["until"].as_i64().filter(|u| *u > now()) {
                a["until"] = json!(u);
            } else if d["until"].is_i64() {
                return Ok(None);
            }
            let r = db::call("ctx.users", "apply_restriction", &a)?;
            Ok(r["restriction_id"].as_i64().map(|x| x.to_string()))
        }
        ("ban" | "leech", false) => {
            db::call(
                "ctx.users",
                "lift_restriction",
                &json!({ "user": user, "kind": if eff == "ban" { "ban" } else { "leech_disabled" },
                         "reason_code": "anticheat.lifted", "idempotency_key": key }),
            )?;
            Ok(None)
        }
        ("freeze", true) => {
            let st = db::settings();
            let until = d["until"]
                .as_i64()
                .unwrap_or(now() + st.freeze_default_days * DAY);
            let since = d["since"].as_i64().unwrap_or(now()).min(now());
            let r = db::call(
                "ctx.accounting",
                "set_rate_rule",
                &json!({ "rule": { "external_ref": format!("freeze:{user}"), "scope": { "kind": "users", "ids": [user] },
                         "up_permille": 0, "down_permille": st.freeze_down_permille, "mode": "override", "priority": 1_000_000,
                         "starts_at": since, "ends_at": until.max(since + 1000), "reason_term": "anticheat.freeze_traffic" } }),
            )?;
            Ok(r.as_i64().map(|x| x.to_string()))
        }
        ("freeze", false) => {
            if let Some(id) = ext.and_then(|e| e.parse::<i64>().ok()) {
                match db::call("ctx.accounting", "remove_rate_rule", &json!({ "rule_id": id })) {
                    Ok(_) => {}
                    Err(e) if e.code.ends_with("not_found") => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(None)
        }
        ("wallet", true) => {
            if ext.is_some() {
                return Ok(None);
            }
            let r = db::call(
                "ctx.ledger",
                "lock_account",
                &json!({ "user": user, "currency": null, "reason": "anticheat.case", "reference": format!("case:{case}"),
                         "idempotency_key": key }),
            )?;
            Ok(r.as_str().map(str::to_owned))
        }
        ("wallet", false) => {
            if let Some(lock) = ext {
                match db::call(
                    "ctx.ledger",
                    "unlock_account",
                    &json!({ "lock_id": lock, "idempotency_key": key }),
                ) {
                    Ok(_) => {}
                    Err(e) if e.code.ends_with("not_found") => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(None)
        }
        _ => Err(err("kernel.internal", "unknown effect")),
    }
}

// ---------------- 一次性操作（§1.9） ----------------

#[allow(clippy::too_many_arguments)]
fn notice(
    user: i64,
    key: &str,
    args: &Value,
    title: &str,
    body: &str,
    link: Option<&str>,
    idem: &str,
    channels: Option<&[&str]>,
) -> R<Value> {
    let mut a = json!({ "user_id": user, "template_key": key, "category": "security", "args": args,
                        "title": title, "body": body, "idempotency_key": idem });
    if let Some(l) = link {
        a["link"] = json!(l);
    }
    if let Some(c) = channels {
        a["channels"] = json!(c);
    }
    match db::call("ctx.notify", "send", &a) {
        Ok(v) => Ok(v),
        Err(e) if db::retryable(&e) => Err(e),
        Err(_) => {
            // notify 不可用：退回站内信
            db::call(
                "ctx.messages",
                "send_system",
                &json!({ "user_id": user, "subject": title, "body": body }),
            )
        }
    }
}

fn kind_text(kind: &str) -> &'static str {
    match kind {
        "warn" => "警告",
        "freeze_traffic" => "冻结上传流量",
        "lock_wallet" => "锁定魔力值转出",
        "disable_download" => "禁止下载",
        "ban" => "封禁",
        "adjust_traffic" => "回收虚报流量",
        "reset_passkey" => "重置 passkey（旧种子文件失效，请重新下载）",
        _ => "处置",
    }
}

fn exec_op(id: i64, user: i64, op: &str, p: &Value, sanction: Option<i64>) -> R<Value> {
    let idem = format!("ach:op:{id}");
    let until = p["until"].as_i64().map(db::iso);
    match op {
        "warn_notice" | "sanction_notice" => {
            let kind = p["kind"].as_str().unwrap_or("warn");
            let body = format!(
                "你的账号因异常流量受到处置：{}{}。可在“我的案件”查看详情并在期限内申诉。",
                kind_text(kind),
                until.as_ref().map(|u| format!("（至 {u}）")).unwrap_or_default()
            );
            notice(
                user,
                &format!(
                    "anticheat.{}",
                    if op == "warn_notice" { "warn" } else { "sanction" }
                ),
                &json!({ "kind": kind, "until": until }),
                "账号处置通知",
                &body,
                Some("/p/anticheat/cases"),
                &idem,
                None,
            )
        }
        "ban_notice" => {
            let case_id = p["case_id"].as_i64().unwrap_or(0);
            let token = db::b64url(&db::hmac(&db::token_secret()?, format!("appeal:{id}").as_bytes()));
            let ttl = db::settings().appeal_token_ttl_days;
            x(
                &format!(
                    "INSERT INTO appeal_tokens (token_hash, op_id, case_id, user_id, expires_at) VALUES ($1, $2, $3, $4, {})
                     ON CONFLICT DO NOTHING",
                    ts(5)
                ),
                &[t(&db::hex(&db::sha256(token.as_bytes()))), i(id), i(case_id), i(user), i(now() + ttl * DAY)],
            )?;
            let link = format!("/p/anticheat/appeal#t={token}");
            notice(
                user,
                "anticheat.ban_notice",
                &json!({ "appeal_link": link, "until": until }),
                "账号已被封禁",
                &format!(
                    "你的账号因异常流量被封禁{}。如有异议，可通过以下链接申诉：{link}",
                    until.as_ref().map(|u| format!("（至 {u}）")).unwrap_or_default()
                ),
                Some(&link),
                &idem,
                Some(&["email"]),
            )
        }
        "appeal_result_notice" => notice(
            user,
            "anticheat.appeal_result",
            &json!({ "outcome": p["outcome"] }),
            "申诉结果",
            &format!(
                "你的申诉已处理：{}。",
                match p["outcome"].as_str() {
                    Some("overturned") => "撤销处置",
                    Some("reduced") => "减轻处置",
                    _ => "维持原决定",
                }
            ),
            Some("/p/anticheat/cases"),
            &idem,
            None,
        ),
        "adjust" | "adjust_revert" | "compensate" => {
            let bytes = p["up_bytes"].as_i64().unwrap_or(0);
            if bytes <= 0 {
                return Ok(json!({ "skipped": true }));
            }
            let (d_up, reason) = match op {
                "adjust" => (-bytes, "anticheat.fraud_revert"),
                "adjust_revert" => (bytes, "anticheat.fraud_revert_undo"),
                _ => (bytes, "anticheat.freeze_compensation"),
            };
            let r = db::call(
                "ctx.accounting",
                "adjust",
                &json!({ "user": user, "d_up": d_up, "d_down": 0, "reason": reason, "idempotency_key": idem }),
            )?;
            let _ = sanction;
            Ok(json!({ "applied_up": r["applied_up"], "adjustment_id": r["adjustment_id"] }))
        }
        "passkey_reset" => {
            let r = db::call(
                "ctx.users",
                "reset_passkey",
                &json!({ "user": user, "idempotency_key": idem }),
            )?;
            Ok(json!({ "version": r["version"] }))
        }
        _ => Err(err("kernel.internal", "unknown op")),
    }
}

pub fn run_ops(user: i64) -> R<()> {
    let rows = q(
        "SELECT id, op, payload, sanction_id, attempts FROM effect_ops
          WHERE user_id = $1 AND state IN ('pending','retrying') AND next_attempt_at <= now() ORDER BY id FOR UPDATE SKIP LOCKED",
        &[i(user)],
    )?;
    for r in rows {
        let (id, op, p, sid, attempts) = (r.n(0), r.s(1), r.v(2), r.on(3), r.n(4));
        match exec_op(id, user, &op, &p, sid) {
            Ok(v) => {
                x(
                    "UPDATE effect_ops SET state = 'done', result = $2, done_at = now(), last_error = NULL WHERE id = $1",
                    &[i(id), j(&v)],
                )?;
                if matches!(op.as_str(), "warn_notice" | "adjust" | "passkey_reset")
                    && let Some(s) = sid
                {
                    let act = q(
                        &format!(
                            "UPDATE sanctions SET state = 'active', activated_at = now(), updated_at = now()
                              WHERE id = $1 AND state = 'pending' RETURNING kind, case_id, {}",
                            ms("until")
                        ),
                        &[i(s)],
                    )?;
                    for a in &act {
                        sanction_changed(s, user, a.on(1), &a.s(0), "applied", a.on(2))?;
                        if op == "passkey_reset" {
                            enqueue(
                                user,
                                Some(s),
                                None,
                                "sanction_notice",
                                &json!({ "kind": "reset_passkey", "case_id": a.on(1) }),
                            )?;
                        }
                    }
                }
            }
            Err(e) if db::retryable(&e) && attempts < 7 => {
                let next = now() + backoff(attempts);
                x(
                    &format!(
                        "UPDATE effect_ops SET state = 'retrying', attempts = attempts + 1, next_attempt_at = {}, last_error = $2 WHERE id = $1",
                        ts(3)
                    ),
                    &[i(id), t(&e.code), i(next)],
                )?;
                pctx::schedule(
                    "run_effects",
                    next,
                    &json!({ "user_id": user }),
                    Some(&format!("op:{id}:{attempts}")),
                )?;
            }
            Err(e) => {
                x(
                    "UPDATE effect_ops SET state = 'failed', attempts = attempts + 1, last_error = $2 WHERE id = $1",
                    &[i(id), t(&e.code)],
                )?;
                if matches!(op.as_str(), "adjust")
                    && let Some(s) = sid
                {
                    let f = q(
                        "UPDATE sanctions SET state = 'failed', updated_at = now() WHERE id = $1 AND state = 'pending' RETURNING kind, case_id",
                        &[i(s)],
                    )?;
                    for a in &f {
                        sanction_changed(s, user, a.on(1), &a.s(0), "failed", None)?;
                    }
                }
                db::counter("op_failures_total", &[("op", &op), ("code", &e.code)]);
            }
        }
        db::commit()?;
    }
    Ok(())
}

// ---------------- 到期 ----------------

/// 单个处置到期（延迟任务 `sanction_expire`）
pub fn expire(sanction_id: i64) -> R<()> {
    let rows = q(
        "UPDATE sanctions SET state = 'expired', updated_at = now()
          WHERE id = $1 AND state IN ('active','pending') AND until IS NOT NULL AND until <= now()
          RETURNING user_id, case_id, kind",
        &[i(sanction_id)],
    )?;
    for r in &rows {
        sanction_changed(sanction_id, r.n(0), r.on(1), &r.s(2), "expired", None)?;
        bump(r.n(0))?;
        crate::core::refresh_blocked(r.n(0), &crate::core::active()?.rs)?;
    }
    Ok(())
}

/// 兜底：遗漏的到期（cron `sanction_sweep`）
pub fn sweep() -> R<usize> {
    let ids = q(
        "SELECT id FROM sanctions WHERE state IN ('active','pending') AND until IS NOT NULL AND until <= now()
          ORDER BY until LIMIT 500 FOR UPDATE SKIP LOCKED",
        &[],
    )?;
    for r in &ids {
        expire(r.n(0))?;
    }
    Ok(ids.len())
}

/// 巡检（cron `effect_reconcile`）：重新安排停滞的效果与操作；抽样比对 identity 的限制（事件丢失的兜底）
pub fn reconcile() -> R<()> {
    let users = q(
        "SELECT DISTINCT user_id FROM (
            SELECT user_id FROM effects WHERE state IN ('pending','retrying') AND next_attempt_at <= now() - interval '1 minute'
            UNION SELECT user_id FROM effect_ops WHERE state IN ('pending','retrying') AND next_attempt_at <= now() - interval '1 minute'
         ) u LIMIT 500",
        &[],
    )?;
    for u in &users {
        pctx::schedule(
            "run_effects",
            now(),
            &json!({ "user_id": u.n(0) }),
            Some(&format!("rec:{}:{}", u.n(0), now() / 60_000)),
        )?;
    }
    let sample = q(
        "SELECT user_id, effect, ext_ref FROM effects WHERE state = 'synced' AND effect IN ('ban','leech')
            AND (applied->>'active')::boolean ORDER BY random() LIMIT 200",
        &[],
    )?;
    for s in &sample {
        let Ok(list) = db::call("ctx.users", "restrictions", &json!({ "user": s.n(0) })) else {
            continue;
        };
        let alive = list["items"].as_array().is_some_and(|items| {
            items.iter().any(|r| {
                r["source"] == "anticheat"
                    && r["lifted_at"].is_null()
                    && Some(r["id"].as_i64().unwrap_or(-1).to_string()) == s.os(2)
            })
        });
        if !alive {
            overridden(s.n(0), &s.s(1), s.os(2).as_deref())?;
        }
    }
    Ok(())
}

/// 管理员在 identity 后台解除了本插件施加的限制：标记外部覆盖，不再重新施加
pub fn overridden(user: i64, eff: &str, ext: Option<&str>) -> R<()> {
    let n = x(
        "UPDATE effects SET state = 'overridden', overridden_at = now(), updated_at = now()
          WHERE user_id = $1 AND effect = $2 AND state = 'synced' AND (applied->>'active')::boolean
            AND ($3::text IS NULL OR ext_ref = $3)",
        &[i(user), t(eff), db::ot(ext)],
    )?;
    if n == 0 {
        return Ok(());
    }
    let rows = q(
        "UPDATE sanctions SET state = 'lifted', lift_reason = 'external_override', lifted_at = now(), updated_at = now()
          WHERE user_id = $1 AND kind = $2 AND state = 'active' RETURNING id, case_id",
        &[i(user), t(kind_of(eff))],
    )?;
    for r in &rows {
        sanction_changed(r.n(0), user, r.on(1), kind_of(eff), "lifted", None)?;
        if let Some(c) = r.on(1) {
            crate::core::timeline(c, "external_override", None, &json!({ "effect": eff }))?;
        }
    }
    db::counter("external_overrides_total", &[("effect", eff)]);
    Ok(())
}

/// `user.restriction_changed`
pub fn on_restriction_changed(p: &Value) -> R<()> {
    if p["source"] != "anticheat" || p["op"] != "lifted" || p["lifted_by"] == "anticheat" {
        return Ok(());
    }
    let user = p["user_id"].as_i64().ok_or_else(|| db::validation("user_id"))?;
    let eff = if p["kind"] == "ban" { "ban" } else { "leech" };
    let rid = p["restriction_id"].as_i64().map(|r| r.to_string());
    overridden(user, eff, rid.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapping_and_backoff() {
        for e in EFFECTS {
            assert_eq!(effect_of(kind_of(e)), Some(e));
        }
        assert_eq!(effect_of("warn"), None);
        assert!(severity_rank("ban") > severity_rank("freeze_traffic"));
        assert_eq!(backoff(0), 30_000);
        assert_eq!(backoff(20), 3_600_000);
    }
}
