//! 申诉（LLD §1.10）：会话申诉、令牌申诉（被封禁用户）、认领、复核与结果回滚。

use pt_plugin_sdk::{
    R, err,
    serde_json::{Value, json},
};

use crate::core;
use crate::db::{self, RowGet, i, ms, now, q, q1, t, ts, x};
use crate::decisions::{rbac, require};
use crate::effects;

fn submit(case_id: i64, user: i64, body: &str, via: &str, check_deadline: bool) -> R<Value> {
    let st = db::settings();
    let c = q1(
        &format!(
            "SELECT user_id, resolution, {} FROM cases WHERE id = $1 FOR UPDATE",
            ms("appeal_deadline")
        ),
        &[i(case_id)],
    )?
    .filter(|c| c.n(0) == user)
    .ok_or_else(|| err("kernel.not_found", "case not found"))?;
    if c.os(1).as_deref() != Some("sanctioned") {
        return Err(err(
            "anticheat.appeal_not_allowed",
            "this case cannot be appealed",
        ));
    }
    if check_deadline && c.on(2).is_none_or(|d| now() > d) {
        return Err(err(
            "anticheat.appeal_window_closed",
            "the appeal window has closed",
        ));
    }
    let used = q1(
        "SELECT count(*) FROM appeals WHERE case_id = $1 AND state <> 'withdrawn'",
        &[i(case_id)],
    )?
    .map_or(0, |r| r.n(0));
    if q1(
        "SELECT 1 FROM appeals WHERE case_id = $1 AND state IN ('submitted','in_review')",
        &[i(case_id)],
    )?
    .is_some()
    {
        return Err(err(
            "anticheat.appeal_exists",
            "an appeal is already being reviewed",
        ));
    }
    if used >= st.appeal_max_per_case {
        return Err(err("anticheat.appeal_limit_reached", "appeal limit reached"));
    }
    let body = db::clean_text(body, 4000, true)?;
    let id = q1(
        "INSERT INTO appeals (case_id, user_id, state, via, body) VALUES ($1, $2, 'submitted', $3, $4) RETURNING id",
        &[i(case_id), i(user), t(via), t(&body)],
    )?
    .map_or(0, |r| r.n(0));
    core::timeline(
        case_id,
        "appeal",
        Some(user),
        &json!({ "appeal_id": id, "op": "submitted", "via": via }),
    )?;
    db::publish(
        "anticheat.appeal_submitted",
        user,
        &json!({ "appeal_id": id, "case_id": case_id, "user_id": user, "via": via }),
    )?;
    db::counter("appeals_total", &[("via", via), ("outcome", "submitted")]);
    if st.appeal_suspend_sanctions {
        x(
            "UPDATE sanctions SET state = 'suspended', updated_at = now() WHERE case_id = $1 AND kind <> 'ban' AND state IN ('pending','active')",
            &[i(case_id)],
        )?;
        effects::bump(user)?;
    }
    Ok(json!({ "appeal_id": id, "state": "submitted" }))
}

pub fn submit_session(case_id: i64, user: i64, body: &str) -> R<Value> {
    submit(case_id, user, body, "session", true)
}

fn token_row(token: &str) -> R<db::Row> {
    let h = db::hex(&db::sha256(token.trim().as_bytes()));
    q1(
        &format!("SELECT case_id, user_id, used_at IS NOT NULL, expires_at < now(), {} FROM appeal_tokens WHERE token_hash = $1", ms("expires_at")),
        &[t(&h)],
    )?
    .ok_or_else(|| err("anticheat.appeal_token_invalid", "invalid or expired token"))
}

pub fn submit_token(token: &str, body: &str) -> R<Value> {
    let r = token_row(token)?;
    if r.b(2) || r.b(3) {
        return Err(err("anticheat.appeal_token_invalid", "invalid or expired token"));
    }
    x(
        "UPDATE appeal_tokens SET used_at = now() WHERE token_hash = $1",
        &[t(&db::hex(&db::sha256(token.trim().as_bytes())))],
    )?;
    submit(r.n(0), r.n(1), body, "token", false)
}

pub fn lookup_token(token: &str) -> R<Value> {
    let r = token_row(token)?;
    if r.b(3) {
        return Err(err("anticheat.appeal_token_invalid", "invalid or expired token"));
    }
    let case = crate::api::user_case_json(r.n(0), r.n(1))?;
    let a = q1(
        &format!(
            "SELECT state, {} FROM appeals WHERE case_id = $1 ORDER BY id DESC LIMIT 1",
            ms("decided_at")
        ),
        &[i(r.n(0))],
    )?;
    Ok(
        json!({ "case": case, "token_used": r.b(2), "appeal": a.map(|a| json!({ "state": a.s(0), "decided_at": db::iso_opt(a.on(1)) })) }),
    )
}

pub fn claim(appeal_id: i64, me: i64) -> R<Value> {
    let ttl = db::settings().claim_ttl_secs;
    let r = q1(
        &format!(
            "UPDATE appeals SET assigned_to = $2, claim_expires_at = now() + make_interval(secs => $3::float8), state = 'in_review',
                    version = version + 1, updated_at = now()
              WHERE id = $1 AND state IN ('submitted','in_review') AND (assigned_to IS NULL OR claim_expires_at < now() OR assigned_to = $2)
              RETURNING {}",
            ms("claim_expires_at")
        ),
        &[i(appeal_id), i(me), i(ttl)],
    )?;
    match r {
        Some(r) => Ok(json!({ "assigned_to": me, "claim_expires_at": db::iso(r.n(0)) })),
        None => Err(err("anticheat.appeal_state_invalid", "appeal cannot be claimed")),
    }
}

/// 申诉决定：维持 / 减轻 / 撤销
pub fn decide(appeal_id: i64, b: &Value, actor: i64) -> R<Value> {
    let st = db::settings();
    let note = db::clean_text(b["note"].as_str().unwrap_or(""), 2000, true)?;
    let outcome = b["outcome"].as_str().unwrap_or("");
    if !matches!(outcome, "upheld" | "reduced" | "overturned") {
        return Err(db::validation("outcome must be upheld, reduced or overturned"));
    }
    let a = q1(
        "SELECT case_id, user_id, state FROM appeals WHERE id = $1 FOR UPDATE",
        &[i(appeal_id)],
    )?
    .ok_or_else(|| err("kernel.not_found", "appeal not found"))?;
    if !matches!(a.s(2).as_str(), "submitted" | "in_review") {
        return Err(err("anticheat.appeal_state_invalid", "appeal is already decided"));
    }
    let (case_id, user) = (a.n(0), a.n(1));
    require(actor, "anticheat.appeals.moderate")?;
    let c = q1(
        "SELECT resolved_by, ruleset_version, contributing_rules FROM cases WHERE id = $1 FOR UPDATE",
        &[i(case_id)],
    )?
    .ok_or_else(|| err("anticheat.case_not_found", "case not found"))?;
    if st.four_eyes_for_appeal {
        let approvers: Vec<i64> = q(
            "SELECT approved_by FROM sanctions WHERE case_id = $1 AND approved_by IS NOT NULL",
            &[i(case_id)],
        )?
        .iter()
        .map(|r| r.n(0))
        .collect();
        let involved = c.on(0) == Some(actor) || approvers.contains(&actor);
        if involved && !rbac_is_sysop(actor)? {
            return Err(err(
                "anticheat.four_eyes_required",
                "the original decider cannot review the appeal",
            ));
        }
    }
    let has_ban = q1(
        "SELECT 1 FROM sanctions WHERE case_id = $1 AND kind = 'ban' LIMIT 1",
        &[i(case_id)],
    )?
    .is_some();
    if outcome == "overturned" && has_ban {
        require(actor, "anticheat.sanctions.manage")?;
    }
    match outcome {
        "reduced" => {
            let changes = b["changes"].as_array().cloned().unwrap_or_default();
            if changes.is_empty() {
                return Err(err("anticheat.appeal_change_invalid", "changes are required"));
            }
            for ch in &changes {
                let sid = ch["sanction_id"]
                    .as_i64()
                    .ok_or_else(|| err("anticheat.appeal_change_invalid", "sanction_id"))?;
                let s = q1(
                    &format!(
                        "SELECT kind, {}, state FROM sanctions WHERE id = $1 AND case_id = $2 FOR UPDATE",
                        ms("until")
                    ),
                    &[i(sid), i(case_id)],
                )?
                .ok_or_else(|| {
                    err(
                        "anticheat.appeal_change_invalid",
                        "sanction is not part of this case",
                    )
                })?;
                if !matches!(s.s(2).as_str(), "proposed" | "pending" | "active" | "suspended") {
                    return Err(err("anticheat.appeal_change_invalid", "sanction is not live"));
                }
                if ch["lift"] == json!(true) {
                    x(
                        "UPDATE sanctions SET state = 'lifted', lift_reason = 'appeal', lifted_by = $2, lifted_at = now(), updated_at = now() WHERE id = $1",
                        &[i(sid), i(actor)],
                    )?;
                    effects::sanction_changed(sid, user, Some(case_id), &s.s(0), "lifted", None)?;
                } else if let Some(u) = ch["until"].as_i64() {
                    // 只能缩短
                    if s.on(1).is_some_and(|cur| u >= cur) || u <= now() {
                        return Err(err(
                            "anticheat.appeal_change_invalid",
                            "until can only be shortened",
                        ));
                    }
                    x(
                        &format!(
                            "UPDATE sanctions SET until = {}, updated_at = now() WHERE id = $1",
                            ts(2)
                        ),
                        &[i(sid), i(u)],
                    )?;
                    pt_plugin_sdk::ctx::schedule(
                        "sanction_expire",
                        u,
                        &json!({ "sanction_id": sid }),
                        Some(&format!("sx:{sid}:{u}")),
                    )?;
                } else {
                    return Err(err(
                        "anticheat.appeal_change_invalid",
                        "each change needs `lift` or `until`",
                    ));
                }
            }
        }
        "overturned" => {
            let rows = q(
                "UPDATE sanctions SET state = 'overturned', updated_at = now()
                  WHERE case_id = $1 AND state IN ('proposed','pending','active','suspended','expired') RETURNING id, kind",
                &[i(case_id)],
            )?;
            for s in &rows {
                effects::sanction_changed(s.n(0), user, Some(case_id), &s.s(1), "overturned", None)?;
            }
            // 已完成的流量回收：撤销
            for r in q(
                "SELECT sanction_id, result FROM effect_ops WHERE op = 'adjust' AND state = 'done' AND sanction_id IN (SELECT id FROM sanctions WHERE case_id = $1)",
                &[i(case_id)],
            )? {
                let applied = -r.v(1)["applied_up"].as_i64().unwrap_or(0);
                if applied > 0 {
                    effects::enqueue(
                        user,
                        r.on(0),
                        Some(appeal_id),
                        "adjust_revert",
                        &json!({ "up_bytes": applied }),
                    )?;
                }
            }
            // 未执行的回收：取消
            x(
                "UPDATE effect_ops SET state = 'cancelled' WHERE op = 'adjust' AND state IN ('pending','retrying')
                    AND sanction_id IN (SELECT id FROM sanctions WHERE case_id = $1)",
                &[i(case_id)],
            )?;
            if let Some(bytes) = b["compensate_bytes"].as_i64().filter(|b| *b > 0) {
                effects::enqueue(
                    user,
                    None,
                    Some(appeal_id),
                    "compensate",
                    &json!({ "up_bytes": bytes }),
                )?;
            }
            x(
                "UPDATE evidence SET verdict = 'false_positive' WHERE user_id = $1 AND id IN (SELECT evidence_id FROM case_evidence WHERE case_id = $2)",
                &[i(user), i(case_id)],
            )?;
            x(
                "UPDATE cases SET state = 'overturned', resolution = 'overturned', version = version + 1, updated_at = now() WHERE id = $1",
                &[i(case_id)],
            )?;
            core::stats_bump(c.n(1), &c.arr(2), "overturned")?;
            db::publish(
                "anticheat.case_resolved",
                user,
                &json!({ "case_id": case_id, "user_id": user, "action": "none", "actions": [], "resolution": "overturned",
                         "false_positive": true, "by": { "kind": "user", "user_id": actor } }),
            )?;
        }
        _ => {}
    }
    x(
        "UPDATE appeals SET state = $2, decided_by = $3, decided_at = now(), decision_note = $4, version = version + 1, updated_at = now() WHERE id = $1",
        &[i(appeal_id), t(outcome), i(actor), t(&note)],
    )?;
    // 暂停的处置：维持 / 减轻时恢复
    x(
        "UPDATE sanctions SET state = 'active', updated_at = now() WHERE case_id = $1 AND state = 'suspended'",
        &[i(case_id)],
    )?;
    core::timeline(
        case_id,
        "appeal",
        Some(actor),
        &json!({ "appeal_id": appeal_id, "op": outcome }),
    )?;
    effects::enqueue(
        user,
        None,
        Some(appeal_id),
        "appeal_result_notice",
        &json!({ "outcome": outcome }),
    )?;
    db::publish(
        "anticheat.appeal_decided",
        user,
        &json!({ "appeal_id": appeal_id, "case_id": case_id, "user_id": user, "outcome": outcome, "by": { "kind": "user", "user_id": actor } }),
    )?;
    db::audit(
        "anticheat.appeal_decide",
        &format!("user:{user}"),
        &json!({ "appeal_id": appeal_id, "case_id": case_id, "outcome": outcome }),
    )?;
    db::counter("appeals_total", &[("via", "review"), ("outcome", outcome)]);
    core::invalidate_context(user)?;
    effects::bump(user)?;
    if outcome == "overturned" {
        core::rescore_user(user, core::Trigger::Manual, false)?;
    }
    let case_state = q1("SELECT state FROM cases WHERE id = $1", &[i(case_id)])?.map(|r| r.s(0));
    Ok(json!({ "appeal_id": appeal_id, "state": outcome, "case_state": case_state }))
}

fn rbac_is_sysop(user: i64) -> R<bool> {
    let roles = db::call("ctx.rbac", "roles_of", &json!({ "user": user }))?;
    Ok(roles.as_array().is_some_and(|r| r.iter().any(|x| x == "sysop"))
        && rbac(user, "anticheat.appeals.moderate")?)
}
