//! 复核（LLD §1.5 认领、§1.6 决定 / 批准 / 解除）、手动开案、手工证据、备注。

use std::collections::HashSet;

use pt_plugin_sdk::{
    R, err,
    serde_json::{Value, json},
};
use serde::Deserialize;

use crate::core;
use crate::db::{self, DAY, RowGet, i, ints, j, ms, now, oi, q, q1, t, ts, x};
use crate::effects;
use crate::rules::{Band, Score};

pub fn rbac(user: i64, perm: &str) -> R<bool> {
    Ok(db::call("ctx.rbac", "check", &json!({ "user": user, "permission": perm }))?.as_bool() == Some(true))
}

pub fn require(user: i64, perm: &str) -> R<()> {
    if rbac(user, perm)? {
        Ok(())
    } else {
        Err(err("anticheat.action_not_allowed", "permission required")
            .with_details(&json!({ "permission": perm })))
    }
}

// ---------------- 认领 ----------------

pub fn claim(case_id: i64, me: i64) -> R<Value> {
    let ttl = db::settings().claim_ttl_secs;
    let r = q1(
        &format!(
            "UPDATE cases SET assigned_to = $2, claim_expires_at = now() + make_interval(secs => $3), version = version + 1
              WHERE id = $1 AND state = 'open' AND (assigned_to IS NULL OR claim_expires_at < now() OR assigned_to = $2)
              RETURNING {}",
            ms("claim_expires_at")
        ),
        &[i(case_id), i(me), SqlF(ttl as f64).into()],
    )?;
    match r {
        Some(r) => {
            core::timeline(case_id, "claimed", Some(me), &json!({}))?;
            Ok(json!({ "assigned_to": me, "claim_expires_at": db::iso(r.n(0)) }))
        }
        None => {
            let c = q1(
                &format!(
                    "SELECT state, assigned_to, {} FROM cases WHERE id = $1",
                    ms("claim_expires_at")
                ),
                &[i(case_id)],
            )?
            .ok_or_else(|| err("anticheat.case_not_found", "case not found"))?;
            if c.s(0) != "open" {
                return Err(err("anticheat.case_state_invalid", "case is not open"));
            }
            Err(err("anticheat.case_claimed", "case is claimed by someone else")
                .with_details(&json!({ "assigned_to": c.on(1), "claim_expires_at": db::iso_opt(c.on(2)) })))
        }
    }
}

struct SqlF(f64);
impl From<SqlF> for pt_plugin_sdk::SqlValue {
    fn from(v: SqlF) -> Self {
        Self::Float(v.0)
    }
}

pub fn release(case_id: i64, me: i64) -> R<()> {
    if x(
        "UPDATE cases SET assigned_to = NULL, claim_expires_at = NULL, version = version + 1 WHERE id = $1 AND assigned_to = $2",
        &[i(case_id), i(me)],
    )? > 0
    {
        core::timeline(case_id, "released", Some(me), &json!({}))?;
    }
    Ok(())
}

pub fn add_note(case_id: i64, me: i64, text: &str) -> R<Value> {
    let text = db::clean_text(text, 2000, true)?;
    q1("SELECT 1 FROM cases WHERE id = $1", &[i(case_id)])?
        .ok_or_else(|| err("anticheat.case_not_found", "case not found"))?;
    let r = q1(
        "INSERT INTO case_notes (case_id, author_id, text) VALUES ($1, $2, $3) RETURNING id",
        &[i(case_id), i(me), t(&text)],
    )?;
    Ok(json!({ "id": r.map(|r| r.n(0)) }))
}

// ---------------- 决定 ----------------

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct Action {
    pub kind: String,
    #[serde(default)]
    pub days: Option<u32>,
    #[serde(default)]
    pub up_bytes: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct Decision {
    pub decision: String,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub false_positive_evidence_ids: Vec<i64>,
    pub note: String,
}

fn perm_for(kind: &str, protected: bool) -> &'static str {
    match kind {
        "disable_download" | "ban" | "reset_passkey" => "anticheat.sanctions.manage",
        _ if protected => "anticheat.sanctions.manage",
        _ => "anticheat.cases.moderate",
    }
}

/// 幂等（服务 / MCP 入口）：同键同请求返回首次结果，不同请求报错
pub fn idem_begin(actor_key: &str, key: &str, req: &Value) -> R<Option<Value>> {
    let hash = db::hex(&db::sha256(req.to_string().as_bytes()));
    let ins = x(
        "INSERT INTO decision_requests (actor_key, key, request_hash) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        &[t(actor_key), t(key), t(&hash)],
    )?;
    if ins == 1 {
        return Ok(None);
    }
    let r = q1(
        "SELECT request_hash, result FROM decision_requests WHERE actor_key = $1 AND key = $2",
        &[t(actor_key), t(key)],
    )?
    .ok_or_else(|| err("kernel.conflict", "idempotency state changed").retryable())?;
    if r.s(0) != hash {
        return Err(err(
            "kernel.idempotency_key_reused",
            "idempotency key was used for a different request",
        ));
    }
    match r.v(1) {
        Value::Null => Err(err(
            "kernel.idempotency_in_progress",
            "a request with this key is in progress",
        )
        .retryable()),
        mut v => {
            v["duplicate"] = json!(true);
            Ok(Some(v))
        }
    }
}

pub fn idem_end(actor_key: &str, key: &str, result: &Value) -> R<()> {
    x(
        "UPDATE decision_requests SET result = $3 WHERE actor_key = $1 AND key = $2",
        &[t(actor_key), t(key), j(result)],
    )?;
    Ok(())
}

fn warn_chain_until(user: i64, days: i64) -> R<i64> {
    let last = q1(
        &format!(
            "SELECT max({}) FROM sanctions WHERE user_id = $1 AND kind = 'warn' AND state IN ('pending','active') AND until > now()",
            ms("until")
        ),
        &[i(user)],
    )?
    .and_then(|r| r.on(0));
    Ok(last.unwrap_or(0).max(now()) + days * DAY)
}

fn sanction_json(id: i64) -> R<Value> {
    let r = q1(
        &format!(
            "SELECT id, kind, state, {}, policy_id FROM sanctions WHERE id = $1",
            ms("until")
        ),
        &[i(id)],
    )?
    .ok_or_else(|| err("anticheat.sanction_not_found", "sanction not found"))?;
    Ok(
        json!({ "id": r.n(0), "kind": r.s(1), "state": r.s(2), "until": db::iso_opt(r.on(3)), "policy_id": r.os(4) }),
    )
}

/// 复核决定（§1.6）。`actor` 为用户（HTTP / MCP / 服务调用者的用户）
pub fn resolve(case_id: i64, d: &Decision, actor: i64) -> R<Value> {
    let note = db::clean_text(&d.note, 2000, true)?;
    let st = db::settings();
    let c = q1(
        "SELECT user_id, state, assigned_to, coalesce(claim_expires_at < now(), true), ruleset_version FROM cases WHERE id = $1 FOR UPDATE",
        &[i(case_id)],
    )?
    .ok_or_else(|| err("anticheat.case_not_found", "case not found"))?;
    let user = c.n(0);
    if c.s(1) != "open" {
        return Err(err("anticheat.case_state_invalid", "case is not open"));
    }
    match c.on(2) {
        Some(a) if a != actor && !c.b(3) => {
            return Err(err("anticheat.claim_required", "case is claimed by someone else"));
        }
        Some(a) if a == actor => {}
        _ => {
            claim(case_id, actor)?;
        }
    }
    let ctx = core::context(user)?;
    let a = core::active()?;
    let (score, _, _) = core::score_user(user, &a.rs, now())?;
    let mut created = Vec::new();
    let (state, resolution) = match d.decision.as_str() {
        "dismiss" => {
            if !d.false_positive_evidence_ids.is_empty() {
                let valid: HashSet<i64> = q(
                    "SELECT evidence_id FROM case_evidence WHERE case_id = $1 AND evidence_id = ANY($2)",
                    &[i(case_id), ints(d.false_positive_evidence_ids.clone())],
                )?
                .iter()
                .map(|r| r.n(0))
                .collect();
                if valid.len() != d.false_positive_evidence_ids.iter().collect::<HashSet<_>>().len() {
                    return Err(db::validation(
                        "false_positive_evidence_ids must belong to this case",
                    ));
                }
                x(
                    "UPDATE evidence SET verdict = 'false_positive' WHERE user_id = $1 AND id = ANY($2)",
                    &[i(user), ints(d.false_positive_evidence_ids.clone())],
                )?;
            }
            // 误判：撤回本案件的自动处置
            let lifted = q(
                "UPDATE sanctions SET state = 'lifted', lift_reason = 'dismissed', lifted_by = $2, lifted_at = now(), updated_at = now()
                  WHERE case_id = $1 AND policy_id IS NOT NULL AND state IN ('proposed','pending','active') RETURNING id, kind",
                &[i(case_id), i(actor)],
            )?;
            for s in &lifted {
                effects::sanction_changed(s.n(0), user, Some(case_id), &s.s(1), "lifted", None)?;
            }
            ("dismissed", "dismissed")
        }
        "no_action" => ("resolved", "no_action"),
        "sanction" => {
            if !(1..=6).contains(&d.actions.len()) {
                return Err(db::validation("1..=6 actions required"));
            }
            let kinds: HashSet<&str> = d.actions.iter().map(|a| a.kind.as_str()).collect();
            if kinds.len() != d.actions.len() {
                return Err(db::validation("duplicate action kinds"));
            }
            for act in &d.actions {
                if !crate::rules::ACTION_KINDS.contains(&act.kind.as_str()) {
                    return Err(db::validation("unknown action kind"));
                }
                require(actor, perm_for(&act.kind, ctx.is_protected))?;
                if ctx.is_protected
                    && matches!(act.kind.as_str(), "ban" | "disable_download" | "reset_passkey")
                {
                    return Err(err(
                        "anticheat.target_protected",
                        "protected users cannot receive this sanction",
                    ));
                }
                if act.kind == "reset_passkey" {
                    return Err(err(
                        "anticheat.action_unavailable",
                        "passkey reset is not available",
                    ));
                }
                if act.kind == "lock_wallet" && db::call("ctx.ledger", "currencies", &json!({})).is_err() {
                    return Err(err("anticheat.action_unavailable", "ledger is not available"));
                }
                if act.days.is_some_and(|d| !(1..=3650).contains(&d)) {
                    return Err(db::validation("days must be 1..=3650"));
                }
                if act.kind == "adjust_traffic" {
                    let up = act.up_bytes.unwrap_or(0);
                    let tot = db::call("ctx.accounting", "user_totals", &json!({ "user": user }))?;
                    if up <= 0 || up > tot["uploaded"].as_i64().unwrap_or(0) {
                        return Err(err(
                            "anticheat.adjust_invalid",
                            "up_bytes must be positive and ≤ current uploaded",
                        ));
                    }
                }
            }
            for act in &d.actions {
                let days = act.days.map(i64::from);
                let until = match act.kind.as_str() {
                    "warn" => Some(warn_chain_until(user, days.unwrap_or(st.warn_default_days))?),
                    "freeze_traffic" => Some(now() + days.unwrap_or(st.freeze_default_days) * DAY),
                    "adjust_traffic" | "reset_passkey" => None,
                    _ => days.map(|d| now() + d * DAY),
                };
                let state = if act.kind == "ban" && st.four_eyes_for_ban {
                    "proposed"
                } else {
                    "pending"
                };
                let params = act
                    .up_bytes
                    .map_or_else(|| json!({}), |b| json!({ "up_bytes": b }));
                let id = effects::insert_sanction(
                    user,
                    Some(case_id),
                    &act.kind,
                    state,
                    until,
                    &params,
                    None,
                    Some(actor),
                )?;
                created.push(id);
            }
            ("resolved", "sanctioned")
        }
        _ => return Err(db::validation("decision must be dismiss, no_action or sanction")),
    };
    let contributing = score.contributing();
    x(
        &format!(
            "UPDATE cases SET state = $2, resolution = $3, resolution_note = $4, resolved_by = $5, resolved_at = now(),
                    appeal_deadline = CASE WHEN $3 = 'sanctioned' THEN {} END, contributing_rules = $7,
                    assigned_to = NULL, claim_expires_at = NULL, version = version + 1, updated_at = now()
              WHERE id = $1",
            ts(6)
        ),
        &[
            i(case_id),
            t(state),
            t(resolution),
            t(&note),
            i(actor),
            i(now() + st.appeal_window_days * DAY),
            db::texts(contributing.clone()),
        ],
    )?;
    x(
        "UPDATE user_risk SET open_case_id = NULL WHERE user_id = $1 AND open_case_id = $2",
        &[i(user), i(case_id)],
    )?;
    let field = match resolution {
        "sanctioned" => "confirmed",
        "dismissed" => "dismissed",
        _ => "",
    };
    if !field.is_empty() {
        core::stats_bump(c.n(4), &contributing, field)?;
    }
    if resolution == "sanctioned" {
        x(
            "UPDATE evidence SET verdict = 'confirmed' WHERE user_id = $1 AND verdict IS NULL AND id IN (SELECT evidence_id FROM case_evidence WHERE case_id = $2)",
            &[i(user), i(case_id)],
        )?;
    }
    let mut actions: Vec<&str> = d.actions.iter().map(|a| a.kind.as_str()).collect();
    actions.sort_by_key(|k| -effects::severity_rank(k));
    core::timeline(
        case_id,
        "resolved",
        Some(actor),
        &json!({ "decision": d.decision, "actions": d.actions }),
    )?;
    db::audit(
        "anticheat.resolve",
        &format!("user:{user}"),
        &json!({ "case_id": case_id, "decision": d.decision, "actions": d.actions, "sanctions": created }),
    )?;
    db::publish(
        "anticheat.case_resolved",
        user,
        &json!({ "case_id": case_id, "user_id": user, "action": if resolution == "sanctioned" { actions.first().copied().unwrap_or("none") } else { "none" },
                 "actions": if resolution == "sanctioned" { json!(actions) } else { json!([]) }, "resolution": resolution,
                 "false_positive": d.decision == "dismiss", "by": { "kind": "user", "user_id": actor } }),
    )?;
    db::counter("cases_resolved_total", &[("resolution", resolution)]);
    core::invalidate_context(user)?;
    effects::bump(user)?;
    if d.decision == "dismiss" {
        core::rescore_user(user, core::Trigger::Manual, false)?;
    } else {
        core::refresh_blocked(user, &a.rs)?;
    }
    let sanctions = created
        .iter()
        .map(|id| sanction_json(*id))
        .collect::<R<Vec<_>>>()?;
    Ok(json!({ "case_id": case_id, "state": state, "sanctions": sanctions, "duplicate": false }))
}

/// 批准提议中的封禁（四眼）
pub fn approve(sanction_id: i64, actor: i64) -> R<Value> {
    require(actor, "anticheat.sanctions.manage")?;
    let s = q1(
        "SELECT user_id, state, decided_by, kind, params, case_id FROM sanctions WHERE id = $1 FOR UPDATE",
        &[i(sanction_id)],
    )?
    .ok_or_else(|| err("anticheat.sanction_not_found", "sanction not found"))?;
    if s.s(1) != "proposed" {
        return Err(err(
            "anticheat.sanction_state_invalid",
            "sanction is not awaiting approval",
        ));
    }
    if s.on(2) == Some(actor) {
        return Err(err("anticheat.four_eyes_required", "the proposer cannot approve"));
    }
    x(
        "UPDATE sanctions SET state = 'pending', approved_by = $2, approved_at = now(), updated_at = now() WHERE id = $1",
        &[i(sanction_id), i(actor)],
    )?;
    effects::enqueue_for(sanction_id, s.n(0), &s.s(3), &s.v(4))?;
    if let Some(c) = s.on(5) {
        core::timeline(
            c,
            "sanction_changed",
            Some(actor),
            &json!({ "sanction_id": sanction_id, "op": "approved" }),
        )?;
    }
    db::audit(
        "anticheat.approve",
        &format!("user:{}", s.n(0)),
        &json!({ "sanction_id": sanction_id }),
    )?;
    effects::bump(s.n(0))?;
    Ok(json!({ "id": sanction_id, "state": "pending" }))
}

/// 手动解除处置
pub fn lift(sanction_id: i64, reason: &str, actor: i64) -> R<Value> {
    let reason = db::clean_text(reason, 500, false)?;
    let s = q1(
        "SELECT user_id, state, kind, case_id FROM sanctions WHERE id = $1 FOR UPDATE",
        &[i(sanction_id)],
    )?
    .ok_or_else(|| err("anticheat.sanction_not_found", "sanction not found"))?;
    if !matches!(s.s(1).as_str(), "proposed" | "pending" | "active" | "suspended") {
        return Err(err("anticheat.sanction_state_invalid", "sanction is not live"));
    }
    let perm = if matches!(s.s(2).as_str(), "ban" | "disable_download") {
        "anticheat.sanctions.manage"
    } else {
        "anticheat.cases.moderate"
    };
    require(actor, perm)?;
    x(
        "UPDATE sanctions SET state = 'lifted', lift_reason = 'manual', lifted_by = $2, lifted_at = now(), updated_at = now() WHERE id = $1",
        &[i(sanction_id), i(actor)],
    )?;
    effects::sanction_changed(sanction_id, s.n(0), s.on(3), &s.s(2), "lifted", None)?;
    db::audit(
        "anticheat.lift",
        &format!("user:{}", s.n(0)),
        &json!({ "sanction_id": sanction_id, "reason": reason }),
    )?;
    effects::bump(s.n(0))?;
    core::refresh_blocked(s.n(0), &core::active()?.rs)?;
    Ok(json!({ "id": sanction_id, "state": "lifted" }))
}

/// 手动开案（版主）
pub fn manual_case(user: i64, reason: &str, actor: i64) -> R<Value> {
    let reason = db::clean_text(reason, 2000, true)?;
    q1("SELECT 1 FROM identity.v_user_public WHERE id = $1", &[i(user)])?
        .ok_or_else(|| err("anticheat.user_not_found", "user not found"))?;
    let a = core::active()?;
    let (s, _, _) = core::score_user(user, &a.rs, now())?;
    let s = Score {
        band: s.band.max(Band::High),
        ..s
    };
    let (id, fresh) = core::open_case(user, &s, a.version, "manual", Some(actor))?;
    add_note(id, actor, &reason)?;
    Ok(json!({ "id": id, "created": fresh }))
}

/// 手工证据（`anticheat.manual`）
pub fn manual_evidence(user: i64, b: &Value, actor: i64, key: &str) -> R<Value> {
    q1("SELECT 1 FROM identity.v_user_public WHERE id = $1", &[i(user)])?
        .ok_or_else(|| err("anticheat.user_not_found", "user not found"))?;
    let note = db::clean_text(b["note"].as_str().unwrap_or(""), 1000, true)?;
    let sev = b["severity"]
        .as_str()
        .filter(|s| crate::rules::Sev::parse(s).is_some())
        .unwrap_or("medium");
    let r = q1(
        &format!(
            "INSERT INTO evidence (occurred_at, user_id, torrent_id, source, kind, severity, details, external_ref, note, created_by)
             VALUES ({}, $2, $3, 'manual', 'anticheat.manual', $4, '{{}}', $5, $6, $7)
             ON CONFLICT (source, external_ref, occurred_at) DO NOTHING RETURNING id",
            ts(1)
        ),
        &[i(now()), i(user), oi(b["torrent_id"].as_i64()), t(sev), t(&format!("manual:{key}")), t(&note), i(actor)],
    )?;
    db::audit(
        "anticheat.evidence_manual",
        &format!("user:{user}"),
        &json!({ "severity": sev, "torrent_id": b["torrent_id"] }),
    )?;
    core::rescore_user(user, core::Trigger::Manual, false)?;
    Ok(json!({ "id": r.map(|r| r.n(0)) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_matrix() {
        assert_eq!(perm_for("warn", false), "anticheat.cases.moderate");
        assert_eq!(perm_for("warn", true), "anticheat.sanctions.manage");
        assert_eq!(perm_for("ban", false), "anticheat.sanctions.manage");
        assert_eq!(perm_for("freeze_traffic", false), "anticheat.cases.moderate");
    }
}
