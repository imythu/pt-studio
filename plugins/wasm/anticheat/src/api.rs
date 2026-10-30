//! 接口层：HTTP（LLD §2.4）、`ctx.anticheat` 服务（§2.1）、MCP 工具（§2.5）、钩子（§1.14）。

use pt_plugin_sdk::{
    R, ctx as pctx, err,
    http::{Reply, RequestExt},
    kv,
    serde_json::{Value, json},
};

use crate::core;
use crate::db::{self, RowGet, i, ints, j, ms, now, q, q1, t, x};
use crate::decisions::{self, Decision};
use crate::exports::pt::plugin::http_handler::Request;
use crate::{appeals, jobs, links, rules};

fn me() -> R<i64> {
    pctx::current_user()
}

pub fn usernames(ids: &[i64]) -> R<std::collections::HashMap<i64, String>> {
    if ids.is_empty() {
        return Ok(Default::default());
    }
    Ok(q(
        "SELECT id, username FROM identity.v_user_public WHERE id = ANY($1)",
        &[ints(ids.to_vec())],
    )?
    .iter()
    .map(|r| (r.n(0), r.s(1)))
    .collect())
}

fn titles(ids: &[i64]) -> std::collections::HashMap<i64, String> {
    if ids.is_empty() {
        return Default::default();
    }
    q(
        "SELECT id, title FROM torrent.v_torrent_public WHERE id = ANY($1)",
        &[ints(ids.to_vec())],
    )
    .map(|rows| rows.iter().map(|r| (r.n(0), r.s(1))).collect())
    .unwrap_or_default()
}

/// 种子视角（FR-ACH-042）：最近 `days` 天（基线确认之后）交叉校验异常的种子，按异常小时数、超额排序。
/// 同一小时可能归因多名上传者，超额（swarm 上传 − 下载）按种子 + 小时去重后累加。
fn suspicious_torrents(days: i64, limit: i64) -> R<Value> {
    if !(1..=90).contains(&days) {
        return Err(db::validation("days must be 1..=90"));
    }
    let rows = q(
        &format!(
            "SELECT e.torrent_id, e.user_id, e.details, {}, {} FROM evidence e
               LEFT JOIN torrent_baselines b ON b.torrent_id = e.torrent_id
              WHERE e.kind = 'cross_check_mismatch' AND e.torrent_id IS NOT NULL
                AND e.verdict IS DISTINCT FROM 'false_positive'
                AND e.occurred_at > now() - make_interval(days => $1::int)
                AND (b.confirmed_at IS NULL OR e.occurred_at > b.confirmed_at)
              ORDER BY e.occurred_at DESC LIMIT 20000",
            ms("e.occurred_at"),
            ms("b.confirmed_at")
        ),
        &[i(days)],
    )?;
    #[derive(Default)]
    struct Agg {
        hours: std::collections::BTreeMap<String, i64>,
        users: std::collections::HashMap<i64, (i64, i64)>,
        last_at: i64,
        baseline_at: Option<i64>,
    }
    let mut by: std::collections::HashMap<i64, Agg> = Default::default();
    for r in &rows {
        let d = r.v(2);
        let a = by.entry(r.n(0)).or_default();
        let hour = d["hour_start"]
            .as_str()
            .map_or_else(|| r.n(3).to_string(), str::to_owned);
        let excess =
            (d["swarm_uploaded"].as_i64().unwrap_or(0) - d["swarm_downloaded"].as_i64().unwrap_or(0)).max(0);
        a.hours.insert(hour, excess);
        let u = a.users.entry(r.n(1)).or_default();
        u.0 += 1;
        u.1 += d["user_uploaded"].as_i64().unwrap_or(0);
        a.last_at = a.last_at.max(r.n(3));
        a.baseline_at = r.on(4);
    }
    let mut list: Vec<(i64, Agg)> = by.into_iter().collect();
    list.sort_by_key(|(tid, a)| {
        (
            std::cmp::Reverse(a.hours.len()),
            std::cmp::Reverse(a.hours.values().sum::<i64>()),
            *tid,
        )
    });
    list.truncate(usize::try_from(limit.clamp(1, 200)).unwrap_or(50));
    let tids: Vec<i64> = list.iter().map(|(t, _)| *t).collect();
    let uids: Vec<i64> = list
        .iter()
        .flat_map(|(_, a)| a.users.keys().copied())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let (names, tt) = (usernames(&uids)?, titles(&tids));
    let items: Vec<Value> = list
        .iter()
        .map(|(tid, a)| {
            let mut users: Vec<(&i64, &(i64, i64))> = a.users.iter().collect();
            users.sort_by_key(|(id, (hits, up))| (std::cmp::Reverse(*hits), std::cmp::Reverse(*up), **id));
            json!({
                "torrent": { "id": tid, "title": tt.get(tid) },
                "mismatch_hours": a.hours.len(),
                "excess_bytes": a.hours.values().sum::<i64>(),
                "last_at": db::iso(a.last_at),
                "baseline_at": db::iso_opt(a.baseline_at),
                "user_count": users.len(),
                "users": users.iter().take(20).map(|(id, (hits, up))| json!({
                    "id": id, "username": names.get(id), "hits": hits, "uploaded": up,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({ "days": days, "items": items, "truncated": rows.len() >= 20000 }))
}

/// 确认种子基线：复核员看过后，之前的异常不再计入种子视角
fn confirm_baseline(tid: i64, note: Option<&str>, actor: i64) -> R<Value> {
    let note = note
        .map(|n| db::clean_text(n, 500, true))
        .transpose()?
        .filter(|n| !n.is_empty());
    let r = q1(
        &format!(
            "INSERT INTO torrent_baselines (torrent_id, confirmed_at, confirmed_by, note) VALUES ($1, now(), $2, $3)
             ON CONFLICT (torrent_id) DO UPDATE SET confirmed_at = now(), confirmed_by = $2, note = $3
             RETURNING {}",
            ms("confirmed_at")
        ),
        &[i(tid), i(actor), db::ot(note.as_deref())],
    )?
    .ok_or_else(|| err("kernel.internal", "baseline upsert"))?;
    db::audit(
        "anticheat.torrent_baseline_confirmed",
        &format!("torrent:{tid}"),
        &json!({ "note": note }),
    )?;
    Ok(json!({ "torrent_id": tid, "confirmed_at": db::iso(r.n(0)) }))
}

fn qi(req: &Request, k: &str) -> Option<i64> {
    req.query(k).and_then(|v| v.parse().ok())
}

fn list_param(req: &Request, k: &str) -> Vec<String> {
    req.query(k)
        .map(|v| {
            v.split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn idem_key(req: &Request) -> String {
    req.header("idempotency-key")
        .unwrap_or("")
        .chars()
        .take(128)
        .collect()
}

// ---------------- 案件 ----------------

#[allow(clippy::too_many_arguments)]
pub fn list_cases(
    states: &[String],
    bands: &[String],
    user: Option<i64>,
    rule: Option<&str>,
    assigned: Option<i64>,
    sort: &str,
    cursor: Option<i64>,
    limit: i64,
) -> R<Value> {
    let order = match sort {
        "opened_at" | "+opened_at" => "opened_at, id",
        "-opened_at" => "opened_at DESC, id DESC",
        "-score" | "score" => "score DESC, id",
        _ => "priority DESC, opened_at, id",
    };
    let states = if states.is_empty() {
        vec!["open".to_owned()]
    } else {
        states.to_vec()
    };
    let off = cursor.unwrap_or(0).max(0);
    let limit = limit.clamp(1, 100);
    let rows = q(
        &format!(
            "SELECT id, user_id, state, score, peak_score, band, primary_rule, priority, assigned_to, {}, {}, resolution,
                    (SELECT array_agg(DISTINCT kind) FROM sanctions s WHERE s.case_id = cases.id AND s.policy_id IS NOT NULL AND s.state IN ('pending','active'))
               FROM cases
              WHERE state = ANY($1) AND (cardinality($2::text[]) = 0 OR band = ANY($2)) AND ($3::bigint IS NULL OR user_id = $3)
                AND ($4::text IS NULL OR primary_rule = $4) AND ($5::bigint IS NULL OR assigned_to = $5)
              ORDER BY {order} OFFSET $6 LIMIT $7",
            ms("opened_at"),
            ms("updated_at")
        ),
        &[db::texts(states), db::texts(bands.to_vec()), db::oi(user), db::ot(rule), db::oi(assigned), i(off), i(limit + 1)],
    )?;
    let more = rows.len() as i64 > limit;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();
    let names = usernames(&rows.iter().map(|r| r.n(1)).collect::<Vec<_>>())?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({ "id": r.n(0), "user": { "id": r.n(1), "username": names.get(&r.n(1)) }, "state": r.s(2), "score": r.n(3),
                    "peak_score": r.n(4), "band": r.s(5), "primary_rule": r.s(6), "priority": r.n(7), "assigned_to": r.on(8),
                    "opened_at": db::iso(r.n(9)), "updated_at": db::iso(r.n(10)), "waiting_hours": (now() - r.n(9)) / 3_600_000,
                    "resolution": r.os(11), "auto_sanctions": r.arr(12) })
        })
        .collect();
    Ok(json!({ "items": items, "next_cursor": more.then(|| (off + limit).to_string()), "has_more": more }))
}

fn suggestions(user: i64, case_id: i64) -> R<Value> {
    let a = core::active()?;
    let Some(r) = q1(
        "SELECT breakdown, score, band, tags FROM user_risk WHERE user_id = $1",
        &[i(user)],
    )?
    else {
        return Ok(json!([]));
    };
    let (s, _, _) = core::score_user(user, &a.rs, now())?;
    let _ = r;
    // 回收建议值：交叉校验超额归因 + 跳变超出部分，上限为当前计入上传
    let sugg_bytes: i64 = q(
        "SELECT snapshot FROM case_evidence WHERE case_id = $1",
        &[i(case_id)],
    )?
    .iter()
    .map(|r| {
        let s = r.v(0);
        let d = &s["details"];
        match s["kind"].as_str() {
            Some("cross_check_mismatch") => d["user_uploaded"]
                .as_i64()
                .unwrap_or(0)
                .min(d["swarm_uploaded"].as_i64().unwrap_or(0) - d["swarm_downloaded"].as_i64().unwrap_or(0))
                .max(0),
            Some("anticheat.daily_upload_spike") => (d["uploaded_raw"].as_i64().unwrap_or(0)
                - db::settings()
                    .spike_min_bytes
                    .max(d["factor"].as_i64().unwrap_or(5) * d["p95"].as_i64().unwrap_or(0)))
            .max(0),
            _ => 0,
        }
    })
    .sum();
    let uploaded = db::call("ctx.accounting", "user_totals", &json!({ "user": user }))
        .map(|v| v["uploaded"].as_i64().unwrap_or(0))
        .unwrap_or(0);
    Ok(json!(
        a.rs.policies
            .iter()
            .filter(|p| !p.auto && rules::policy_applies(p, &s))
            .map(
                |p| json!({ "policy_id": p.id, "actions": p.actions.iter().map(|act| {
            let mut v = json!({ "kind": act.kind, "days": act.days });
            if act.kind == "adjust_traffic" { v["up_bytes"] = json!(sugg_bytes.min(uploaded)); }
            v
        }).collect::<Vec<_>>() })
            )
            .collect::<Vec<_>>()
    ))
}

pub fn sanctions_of(filter: &str, id: i64) -> R<Vec<Value>> {
    Ok(q(
        &format!(
            "SELECT s.id, s.kind, s.state, s.case_id, {}, s.decided_by, s.approved_by, s.policy_id, {}, s.params, s.lift_reason,
                    e.state = 'overridden'
               FROM sanctions s LEFT JOIN effects e ON e.user_id = s.user_id AND e.effect = CASE s.kind
                    WHEN 'ban' THEN 'ban' WHEN 'disable_download' THEN 'leech' WHEN 'freeze_traffic' THEN 'freeze' WHEN 'lock_wallet' THEN 'wallet' END
              WHERE s.{filter} = $1 ORDER BY s.id DESC LIMIT 200",
            ms("s.until"),
            ms("s.created_at")
        ),
        &[i(id)],
    )?
    .iter()
    .map(|r| {
        json!({ "id": r.n(0), "kind": r.s(1), "state": r.s(2), "case_id": r.on(3), "until": db::iso_opt(r.on(4)),
                "decided_by": r.on(5), "approved_by": r.on(6), "policy_id": r.os(7), "created_at": db::iso(r.n(8)),
                "params": r.v(9), "lift_reason": r.os(10), "external_override": r.b(11) })
    })
    .collect())
}

pub fn case_detail(case_id: i64, staff: bool) -> R<Value> {
    let c = q1(
        &format!(
            "SELECT id, user_id, state, score, peak_score, band, primary_rule, ruleset_version, priority, assigned_to, {}, {},
                    resolution, resolution_note, {}, {}, version, opened_reason, contributing_rules, {}, resolved_by
               FROM cases WHERE id = $1",
            ms("claim_expires_at"),
            ms("opened_at"),
            ms("resolved_at"),
            ms("appeal_deadline"),
            ms("updated_at")
        ),
        &[i(case_id)],
    )?
    .ok_or_else(|| err("anticheat.case_not_found", "case not found"))?;
    let user = c.n(1);
    let u = q1(
        &format!(
            "SELECT username, class_id, {} FROM identity.v_user_public WHERE id = $1",
            ms("created_at")
        ),
        &[i(user)],
    )?;
    let fp = jobs::false_positive_ids(case_id)?;
    let ev_rows = q(
        "SELECT evidence_id, snapshot FROM case_evidence WHERE case_id = $1 ORDER BY (snapshot->>'occurred_at')::bigint DESC NULLS LAST, evidence_id DESC LIMIT 200",
        &[i(case_id)],
    )?;
    let tids: Vec<i64> = ev_rows
        .iter()
        .filter_map(|r| r.v(1)["torrent_id"].as_i64())
        .collect();
    let tt = titles(&tids);
    let evidence: Vec<Value> = ev_rows
        .iter()
        .map(|r| {
            let s = r.v(1);
            let tid = s["torrent_id"].as_i64();
            json!({ "id": r.n(0), "kind": s["kind"], "severity": s["severity"], "hits": s["hits"], "details": s["details"],
                    "torrent": tid.map(|t| json!({ "id": t, "title": tt.get(&t) })),
                    "occurred_at": s["occurred_at"].as_i64().map(db::iso), "false_positive": fp.contains(&r.n(0)) })
        })
        .collect();
    let breakdown =
        q1("SELECT breakdown FROM user_risk WHERE user_id = $1", &[i(user)])?.map_or(json!([]), |r| r.v(0));
    let events = q(
        &format!(
            "SELECT {}, kind, actor_user_id, data FROM case_events WHERE case_id = $1 ORDER BY at, id LIMIT 500",
            ms("at")
        ),
        &[i(case_id)],
    )?;
    let timeline: Vec<Value> = events
        .iter()
        .filter(|e| e.s(1) != "swarm_snapshot")
        .map(|e| json!({ "at": db::iso(e.n(0)), "kind": e.s(1), "actor": e.on(2), "data": e.v(3) }))
        .collect();
    let swarm: Vec<Value> = events
        .iter()
        .filter(|e| e.s(1) == "swarm_snapshot")
        .map(|e| e.v(3))
        .collect();
    let traffic = db::call("ctx.accounting", "user_totals", &json!({ "user": user })).unwrap_or(Value::Null);
    let prior: Vec<Value> = q(
        &format!("SELECT id, state, resolution, {} FROM cases WHERE user_id = $1 AND id <> $2 ORDER BY opened_at DESC LIMIT 20", ms("opened_at")),
        &[i(user), i(case_id)],
    )?
    .iter()
    .map(|r| json!({ "id": r.n(0), "state": r.s(1), "resolution": r.os(2), "opened_at": db::iso(r.n(3)) }))
    .collect();
    let notes: Vec<Value> = if staff {
        q(
            &format!(
                "SELECT author_id, {}, text FROM case_notes WHERE case_id = $1 ORDER BY created_at",
                ms("created_at")
            ),
            &[i(case_id)],
        )?
        .iter()
        .map(|r| json!({ "by": r.n(0), "at": db::iso(r.n(1)), "text": r.s(2) }))
        .collect()
    } else {
        vec![]
    };
    let appeals: Vec<Value> = q(
        &format!("SELECT id, state, via, body, decided_by, decision_note, {} FROM appeals WHERE case_id = $1 ORDER BY id", ms("created_at")),
        &[i(case_id)],
    )?
    .iter()
    .map(|r| json!({ "id": r.n(0), "state": r.s(1), "via": r.s(2), "body": r.os(3), "decided_by": r.on(4), "decision_note": r.os(5), "created_at": db::iso(r.n(6)) }))
    .collect();
    Ok(json!({
        "id": c.n(0),
        "user": { "id": user, "username": u.as_ref().map(|u| u.s(0)), "class_id": u.as_ref().map(|u| u.n(1)),
                  "account_age_days": u.as_ref().map(|u| (now() - u.n(2)) / db::DAY) },
        "state": c.s(2), "score": c.n(3), "peak_score": c.n(4), "band": c.s(5), "primary_rule": c.s(6), "ruleset_version": c.n(7),
        "priority": c.n(8), "assigned_to": c.on(9), "claim_expires_at": db::iso_opt(c.on(10)), "opened_at": db::iso(c.n(11)),
        "resolution": c.os(12), "resolution_note": if staff { json!(c.os(13)) } else { Value::Null }, "resolved_at": db::iso_opt(c.on(14)),
        "appeal_deadline": db::iso_opt(c.on(15)), "version": c.n(16), "opened_reason": c.s(17), "contributing_rules": c.arr(18),
        "updated_at": db::iso(c.n(19)), "resolved_by": c.on(20),
        "breakdown": breakdown, "evidence": evidence, "sanctions": sanctions_of("case_id", case_id)?,
        "suggestions": if c.s(2) == "open" { suggestions(user, case_id)? } else { json!([]) },
        "traffic": { "uploaded": traffic["uploaded"], "downloaded": traffic["downloaded"], "seed_secs": traffic["seed_secs"] },
        "timeline": timeline, "swarm_snapshots": swarm, "prior_cases": prior, "notes": notes, "appeals": appeals,
    }))
}

/// 用户视图：不含分数、分段、规则、员工备注
pub fn user_case_json(case_id: i64, user: i64) -> R<Value> {
    let c = q1(
        &format!("SELECT id, state, {}, {}, resolution, {}, ruleset_version, contributing_rules FROM cases WHERE id = $1 AND user_id = $2",
                 ms("opened_at"), ms("resolved_at"), ms("appeal_deadline")),
        &[i(case_id), i(user)],
    )?
    .ok_or_else(|| err("kernel.not_found", "case not found"))?;
    let st = db::settings();
    let cats: Vec<String> = core::ruleset(c.n(6))
        .map(|(rs, _, _)| {
            let mut v: Vec<String> = c
                .arr(7)
                .iter()
                .filter_map(|r| rs.rule(r))
                .flat_map(|r| r.tags().to_vec())
                .filter(|t| t != "conclusive")
                .collect();
            v.sort();
            v.dedup();
            v.into_iter().map(|t| format!("anticheat.category.{t}")).collect()
        })
        .unwrap_or_default();
    let torrents: Vec<Value> = if st.user_view_show_torrents {
        let ids: Vec<i64> = q(
            "SELECT DISTINCT (snapshot->>'torrent_id')::bigint FROM case_evidence WHERE case_id = $1 AND snapshot->>'torrent_id' IS NOT NULL LIMIT 20",
            &[i(case_id)],
        )?
        .iter()
        .map(|r| r.n(0))
        .collect();
        let tt = titles(&ids);
        ids.iter()
            .map(|t| json!({ "id": t, "title": tt.get(t) }))
            .collect()
    } else {
        vec![]
    };
    let sanctions: Vec<Value> = q(
        &format!(
            "SELECT kind, state, {} FROM sanctions WHERE case_id = $1 AND state <> 'proposed' ORDER BY id",
            ms("until")
        ),
        &[i(case_id)],
    )?
    .iter()
    .map(|r| json!({ "kind": r.s(0), "state": r.s(1), "until": db::iso_opt(r.on(2)) }))
    .collect();
    let ap = q1(
        "SELECT state FROM appeals WHERE case_id = $1 ORDER BY id DESC LIMIT 1",
        &[i(case_id)],
    )?
    .map(|r| r.s(0));
    let used = q1(
        "SELECT count(*) FROM appeals WHERE case_id = $1 AND state <> 'withdrawn'",
        &[i(case_id)],
    )?
    .map_or(0, |r| r.n(0));
    let allowed = c.os(4).as_deref() == Some("sanctioned")
        && c.on(5).is_some_and(|d| now() <= d)
        && used < st.appeal_max_per_case
        && !matches!(ap.as_deref(), Some("submitted" | "in_review"));
    Ok(
        json!({ "id": c.n(0), "state": c.s(1), "opened_at": db::iso(c.n(2)), "resolved_at": db::iso_opt(c.on(3)),
               "categories": cats, "torrents": torrents, "sanctions": sanctions,
               "appeal": { "allowed": allowed, "deadline": db::iso_opt(c.on(5)), "state": ap } }),
    )
}

pub fn risk_json(user: i64) -> R<Value> {
    let r = q1(
        &format!(
            "SELECT score, band, open_case_id, transfer_blocked, ruleset_version, {}, shadow_score, peak_score, breakdown, tags, evidence_truncated FROM user_risk WHERE user_id = $1",
            ms("computed_at")
        ),
        &[i(user)],
    )?;
    let active: Vec<String> = q(
        "SELECT DISTINCT kind FROM sanctions WHERE user_id = $1 AND state IN ('pending','active') ORDER BY kind",
        &[i(user)],
    )?
    .iter()
    .map(|r| r.s(0))
    .collect();
    Ok(match r {
        Some(r) => {
            json!({ "user_id": user, "score": r.n(0), "band": r.s(1), "under_review": r.on(2).is_some(), "open_case_id": r.on(2),
                           "transfer_blocked": r.b(3), "active_sanctions": active, "ruleset_version": r.n(4), "computed_at": db::iso_opt(r.on(5)),
                           "shadow_score": r.n(6), "peak_score": r.n(7), "breakdown": r.v(8), "tags": r.arr(9), "evidence_truncated": r.b(10) })
        }
        None => {
            json!({ "user_id": user, "score": 0, "band": "none", "under_review": false, "open_case_id": null, "transfer_blocked": false,
                        "active_sanctions": active, "ruleset_version": core::active()?.version, "computed_at": null })
        }
    })
}

fn evidence_list(user: i64, kind: Option<&str>, limit: i64) -> R<Value> {
    let rows = q(
        &format!(
            "SELECT id, kind, source, severity, hits, torrent_id, details, {}, verdict, note FROM evidence
              WHERE user_id = $1 AND ($2::text IS NULL OR kind = $2) ORDER BY occurred_at DESC, id DESC LIMIT $3",
            ms("occurred_at")
        ),
        &[i(user), db::ot(kind), i(limit.clamp(1, 500))],
    )?;
    let tt = titles(&rows.iter().filter_map(|r| r.on(5)).collect::<Vec<_>>());
    Ok(
        json!({ "items": rows.iter().map(|r| json!({ "id": r.n(0), "kind": r.s(1), "source": r.s(2), "severity": r.os(3), "hits": r.n(4),
        "torrent": r.on(5).map(|t| json!({ "id": t, "title": tt.get(&t) })), "details": r.v(6), "occurred_at": db::iso(r.n(7)),
        "verdict": r.os(8), "note": r.os(9) })).collect::<Vec<_>>() }),
    )
}

// ---------------- 规则集 ----------------

fn ruleset_json(version: i64) -> R<Value> {
    let r = q1(
        &format!("SELECT version, status, definition, checksum, based_on, note, created_by, {}, {}, {}, activated_by FROM rulesets WHERE version = $1",
                 ms("created_at"), ms("updated_at"), ms("activated_at")),
        &[i(version)],
    )?
    .ok_or_else(|| err("anticheat.ruleset_not_found", "ruleset not found"))?;
    Ok(
        json!({ "version": r.n(0), "status": r.s(1), "definition": r.v(2), "checksum": r.s(3), "based_on": r.on(4), "note": r.os(5),
               "created_by": r.on(6), "created_at": db::iso(r.n(7)), "updated_at": db::iso(r.n(8)), "activated_at": db::iso_opt(r.on(9)),
               "activated_by": r.on(10) }),
    )
}

fn invalid(errs: Vec<rules::VErr>) -> pt_plugin_sdk::PluginError {
    err("anticheat.ruleset_invalid", "ruleset failed validation").with_details(&json!({ "errors": errs }))
}

fn create_ruleset(b: &Value, actor: i64) -> R<Value> {
    let def = b["definition"].clone();
    rules::parse(&def, db::settings().auto_ban_enabled).map_err(invalid)?;
    let note = b["note"]
        .as_str()
        .map(|s| s.chars().take(500).collect::<String>());
    let v = q1("SELECT coalesce(max(version), 0) + 1 FROM rulesets", &[])?.map_or(1, |r| r.n(0));
    let sum = core::checksum(&def);
    x(
        "INSERT INTO rulesets (version, status, definition, checksum, based_on, note, created_by) VALUES ($1, 'draft', $2, $3, $4, $5, $6)",
        &[
            i(v),
            j(&def),
            t(&sum),
            db::oi(b["based_on"].as_i64()),
            db::ot(note.as_deref()),
            i(actor),
        ],
    )?;
    db::audit(
        "anticheat.ruleset_create",
        "plugin:anticheat",
        &json!({ "version": v, "checksum": sum }),
    )?;
    Ok(json!({ "version": v, "status": "draft", "checksum": sum }))
}

fn update_ruleset(version: i64, b: &Value, if_match: Option<&str>) -> R<Value> {
    let (_, status, sum) = core::ruleset(version)?;
    if status != "draft" {
        return Err(err("anticheat.ruleset_not_draft", "only drafts can be modified"));
    }
    if let Some(m) = if_match
        && m.trim_matches('"') != sum
    {
        return Err(err("kernel.precondition_failed", "ruleset was modified")
            .with_details(&json!({ "checksum": sum })));
    }
    let def = b["definition"].clone();
    rules::parse(&def, db::settings().auto_ban_enabled).map_err(invalid)?;
    let new = core::checksum(&def);
    x(
        "UPDATE rulesets SET definition = $2, checksum = $3, note = coalesce($4, note), updated_at = now() WHERE version = $1",
        &[i(version), j(&def), t(&new), db::ot(b["note"].as_str())],
    )?;
    Ok(json!({ "version": version, "status": "draft", "checksum": new }))
}

fn activate(version: i64, actor: i64, note: &str) -> R<Value> {
    let (_, status, sum) = core::ruleset(version)?;
    if status == "active" {
        return Ok(json!({ "active_version": version, "rescore": "none" }));
    }
    if db::settings().require_simulation_before_activate && status == "draft" {
        let ok = q1(
            "SELECT 1 FROM simulations WHERE ruleset_version = $1 AND status = 'done' AND ruleset_checksum = $2",
            &[i(version), t(&sum)],
        )?;
        if ok.is_none() {
            return Err(err(
                "anticheat.simulation_required",
                "run a simulation of this draft before activating it",
            ));
        }
    }
    let old = core::active()?.version;
    // 回滚到已退役版本：复制为新版本，保证版本号单调
    let target = if status == "retired" {
        let r = create_ruleset(
            &json!({ "definition": core::ruleset(version)?.0, "based_on": version, "note": format!("rollback to v{version}") }),
            actor,
        )?;
        r["version"].as_i64().unwrap_or(version)
    } else {
        version
    };
    x(
        "UPDATE rulesets SET status = 'retired', updated_at = now() WHERE status = 'active'",
        &[],
    )?;
    x(
        "UPDATE rulesets SET status = 'active', activated_at = now(), activated_by = $2, updated_at = now() WHERE version = $1",
        &[i(target), i(actor)],
    )?;
    db::audit(
        "anticheat.ruleset_activate",
        "plugin:anticheat",
        &json!({ "from": old, "to": target, "note": note, "checksum": core::ruleset(target)?.2 }),
    )?;
    core::start_rescore(target)?;
    Ok(json!({ "active_version": target, "rescore": "scheduled" }))
}

fn dashboard() -> R<Value> {
    let sla = db::settings().review_sla_hours;
    let r = q1(
        &format!(
            "SELECT count(*) FILTER (WHERE state = 'open'),
                    count(*) FILTER (WHERE state = 'open' AND opened_at < now() - make_interval(hours => $1::int)),
                    coalesce(min({}) FILTER (WHERE state = 'open'), 0),
                    count(*) FILTER (WHERE opened_at > now() - interval '24 hours'),
                    count(*) FILTER (WHERE resolved_at > now() - interval '24 hours')
               FROM cases",
            ms("opened_at")
        ),
        &[i(sla)],
    )?
    .unwrap_or_default();
    let auto = q1(
        "SELECT coalesce(sum(count), 0)::bigint FROM auto_action_log WHERE at > now() - interval '1 hour'",
        &[],
    )?
    .map_or(0, |r| r.n(0));
    let appeals = q1(
        "SELECT count(*) FROM appeals WHERE state IN ('submitted','in_review')",
        &[],
    )?
    .map_or(0, |r| r.n(0));
    let failed = q1("SELECT count(*) FROM effects WHERE state = 'failed'", &[])?.map_or(0, |r| r.n(0));
    let breaker = kv::get("breaker")?.and_then(|v| serde_json::from_slice::<Value>(&v).ok());
    Ok(
        json!({ "open_cases": r.n(0), "over_sla": r.n(1), "oldest_waiting_hours": if r.n(2) > 0 { (now() - r.n(2)) / 3_600_000 } else { 0 },
               "opened_24h": r.n(3), "resolved_24h": r.n(4), "auto_actions_1h": auto,
               "breaker": { "tripped": breaker.is_some() || db::settings().breaker_tripped, "since": breaker.as_ref().and_then(|b| b["tripped_at"].as_i64()).map(db::iso) },
               "appeals_pending": appeals, "effects_failed": failed }),
    )
}

// ---------------- HTTP ----------------

pub fn http(req: &Request) -> R<Reply> {
    let pid = |k: &str| req.param_i64(k);
    let b = req.body_value();
    Ok(match req.route() {
        // 用户
        "list_my_cases" => {
            let uid = me()?;
            let ids = q(
                "SELECT id FROM cases WHERE user_id = $1 AND state <> 'open' ORDER BY opened_at DESC LIMIT 50",
                &[i(uid)],
            )?;
            let items = ids
                .iter()
                .map(|r| user_case_json(r.n(0), uid))
                .collect::<R<Vec<_>>>()?;
            Reply::ok(&json!({ "items": items, "next_cursor": null, "has_more": false }))
        }
        "get_my_case" => {
            let uid = me()?;
            let id = pid("id")?;
            if q1(
                "SELECT 1 FROM cases WHERE id = $1 AND user_id = $2 AND state <> 'open'",
                &[i(id), i(uid)],
            )?
            .is_none()
            {
                return Err(err("kernel.not_found", "case not found"));
            }
            Reply::ok(&user_case_json(id, uid)?)
        }
        "create_appeal" => Reply::created(&appeals::submit_session(
            pid("id")?,
            me()?,
            b["body"].as_str().unwrap_or(""),
        )?),
        "create_appeal_by_token" => Reply::created(&appeals::submit_token(
            b["token"].as_str().unwrap_or(""),
            b["body"].as_str().unwrap_or(""),
        )?),
        "get_appeal_by_token" => Reply::ok(&appeals::lookup_token(b["token"].as_str().unwrap_or(""))?),
        // 后台：案件
        "admin_list_cases" => Reply::ok(&list_cases(
            &list_param(req, "states"),
            &list_param(req, "bands"),
            qi(req, "user_id"),
            req.query("rule"),
            qi(req, "assigned_to"),
            req.query("sort").unwrap_or("-priority"),
            qi(req, "cursor"),
            qi(req, "limit").unwrap_or(50),
        )?),
        "admin_get_case" => Reply::ok(&case_detail(pid("id")?, true)?),
        "admin_create_case" => Reply::created(&decisions::manual_case(
            b["user_id"].as_i64().ok_or_else(|| db::validation("user_id"))?,
            b["reason_note"].as_str().unwrap_or(""),
            me()?,
        )?),
        "admin_claim_case" => Reply::ok(&decisions::claim(pid("id")?, me()?)?),
        "admin_release_claim" => {
            decisions::release(pid("id")?, me()?)?;
            Reply::no_content()
        }
        "admin_add_note" => Reply::created(&decisions::add_note(
            pid("id")?,
            me()?,
            b["text"].as_str().unwrap_or(""),
        )?),
        "admin_resolve_case" => {
            let d: Decision = req.body_json()?;
            Reply::ok(&decisions::resolve(pid("id")?, &d, me()?)?)
        }
        "admin_approve_sanction" => Reply::ok(&decisions::approve(pid("id")?, me()?)?),
        "admin_lift_sanction" => Reply::ok(&decisions::lift(
            pid("id")?,
            b["reason_note"].as_str().unwrap_or(""),
            me()?,
        )?),
        "admin_list_user_sanctions" => Reply::ok(&json!({ "items": sanctions_of("user_id", pid("id")?)? })),
        "admin_get_user_risk" => Reply::ok(&risk_json(pid("id")?)?),
        "admin_list_user_evidence" => Reply::ok(&evidence_list(
            pid("id")?,
            req.query("kind"),
            qi(req, "limit").unwrap_or(100),
        )?),
        "admin_create_evidence" => Reply::created(&decisions::manual_evidence(
            pid("id")?,
            &b,
            me()?,
            &idem_key(req),
        )?),
        "admin_list_watchlist" => {
            let rows = q(
                "SELECT user_id, score, band FROM user_risk WHERE band = 'watch' ORDER BY score DESC, user_id LIMIT 200",
                &[],
            )?;
            let names = usernames(&rows.iter().map(|r| r.n(0)).collect::<Vec<_>>())?;
            Reply::ok(
                &json!({ "items": rows.iter().map(|r| json!({ "user": { "id": r.n(0), "username": names.get(&r.n(0)) }, "score": r.n(1), "band": r.s(2) })).collect::<Vec<_>>() }),
            )
        }
        // 后台：种子视角
        "admin_list_suspicious_torrents" => Reply::ok(&suspicious_torrents(
            qi(req, "days").unwrap_or(30),
            qi(req, "limit").unwrap_or(50),
        )?),
        "admin_confirm_torrent_baseline" => {
            Reply::ok(&confirm_baseline(pid("id")?, b["note"].as_str(), me()?)?)
        }
        // 后台：多账号关联
        "admin_list_account_clusters" => Reply::ok(&links::list(
            qi(req, "days").unwrap_or(90),
            qi(req, "min_torrents").unwrap_or(1),
            req.query("source").unwrap_or("all"),
            qi(req, "limit").unwrap_or(50),
        )?),
        "admin_list_user_links" => Reply::ok(&links::of_user(pid("id")?, qi(req, "days").unwrap_or(90))?),
        "admin_review_account_link" => Reply::ok(&links::review(
            pid("a")?,
            pid("b")?,
            b["verdict"].as_str().unwrap_or(""),
            b["note"].as_str(),
            me()?,
        )?),
        "admin_clear_account_link" => {
            links::clear(pid("a")?, pid("b")?)?;
            Reply::no_content()
        }
        // 后台：申诉
        "admin_list_appeals" => {
            let states = list_param(req, "states");
            let states = if states.is_empty() {
                vec!["submitted".into(), "in_review".into()]
            } else {
                states
            };
            let rows = q(
                &format!("SELECT a.id, a.case_id, a.user_id, a.state, a.via, a.body, a.assigned_to, {}, c.resolved_by FROM appeals a JOIN cases c ON c.id = a.case_id
                           WHERE a.state = ANY($1) ORDER BY a.created_at LIMIT 200", ms("a.created_at")),
                &[db::texts(states)],
            )?;
            let names = usernames(&rows.iter().map(|r| r.n(2)).collect::<Vec<_>>())?;
            Reply::ok(
                &json!({ "items": rows.iter().map(|r| json!({ "id": r.n(0), "case_id": r.n(1), "user": { "id": r.n(2), "username": names.get(&r.n(2)) },
                "state": r.s(3), "via": r.s(4), "body": r.os(5), "assigned_to": r.on(6), "created_at": db::iso(r.n(7)), "decided_by_case": r.on(8) })).collect::<Vec<_>>() }),
            )
        }
        "admin_claim_appeal" => Reply::ok(&appeals::claim(pid("id")?, me()?)?),
        "admin_decide_appeal" => Reply::ok(&appeals::decide(pid("id")?, &b, me()?)?),
        // 后台：规则集
        "admin_list_rulesets" => {
            let rows = q(
                &format!(
                    "SELECT version, status, checksum, based_on, note, {}, {} FROM rulesets ORDER BY version DESC LIMIT 200",
                    ms("created_at"),
                    ms("activated_at")
                ),
                &[],
            )?;
            Reply::ok(
                &json!({ "items": rows.iter().map(|r| json!({ "version": r.n(0), "status": r.s(1), "checksum": r.s(2), "based_on": r.on(3),
                "note": r.os(4), "created_at": db::iso(r.n(5)), "activated_at": db::iso_opt(r.on(6)) })).collect::<Vec<_>>() }),
            )
        }
        "admin_get_ruleset" => {
            let v = req.param("version").unwrap_or("");
            let version = if v == "active" {
                core::active()?.version
            } else {
                pid("version")?
            };
            Reply::ok(&ruleset_json(version)?).header("etag", &format!("\"{}\"", core::ruleset(version)?.2))
        }
        "admin_create_ruleset" => Reply::created(&create_ruleset(&b, me()?)?),
        "admin_update_ruleset" => Reply::ok(&update_ruleset(pid("version")?, &b, req.header("if-match"))?),
        "admin_validate_ruleset" => {
            let errs = rules::validate(&b["definition"], db::settings().auto_ban_enabled);
            Reply::ok(&json!({ "valid": errs.is_empty(), "errors": errs }))
        }
        "admin_activate_ruleset" => Reply::ok(&activate(
            pid("version")?,
            me()?,
            b["note"].as_str().unwrap_or(""),
        )?),
        "admin_create_simulation" => {
            let r = jobs::create_simulation(pid("version")?, &b, me()?)?;
            Reply::json(202, &r).header(
                "location",
                &format!(
                    "/api/p/anticheat/v1/admin/simulations/{}",
                    r["id"].as_str().unwrap_or("")
                ),
            )
        }
        "admin_get_simulation" => {
            let id = req.param("id").unwrap_or("");
            let r = q1(
                &format!("SELECT id::text, status, ruleset_version, baseline_version, params, result, totals, {} FROM simulations WHERE id::text = $1", ms("finished_at")),
                &[t(id)],
            )?
            .ok_or_else(|| err("kernel.not_found", "simulation not found"))?;
            Reply::ok(
                &json!({ "id": r.s(0), "status": r.s(1), "ruleset_version": r.n(2), "baseline_version": r.n(3), "window_days": r.v(4)["window_days"],
                "result": r.v(5), "progress": r.v(6)["users"], "finished_at": db::iso_opt(r.on(7)),
                "note": "上下文按当前缓存计算，可能与历史时点略有差异" }),
            )
        }
        "admin_rule_stats" => Reply::ok(
            &json!({ "items": jobs::stats_rows(qi(req, "days").unwrap_or(90).clamp(1, 365), qi(req, "version"))? }),
        ),
        "admin_dashboard" => Reply::ok(&dashboard()?),
        "admin_reset_breaker" => {
            kv::delete("breaker")?;
            db::gauge("breaker_tripped", 0.0, &[]);
            db::audit(
                "anticheat.breaker_reset",
                "plugin:anticheat",
                &json!({ "note": b["note"] }),
            )?;
            Reply::ok(&json!({ "tripped": db::settings().breaker_tripped }))
        }
        "admin_create_explanation" => {
            let id = pid("id")?;
            let (_, version) = jobs::explain_data(id)?;
            if q1(
                "SELECT 1 FROM explanations WHERE case_id = $1 AND case_version = $2",
                &[i(id), i(version)],
            )?
            .is_none()
            {
                pctx::schedule(
                    "explain_case",
                    now(),
                    &json!({ "case_id": id, "requested_by": me()? }),
                    Some(&format!("explain:{id}:{version}")),
                )?;
            }
            Reply::json(202, &json!({ "status": "queued" }))
        }
        "admin_get_explanation" => {
            let id = pid("id")?;
            let v = q1("SELECT version FROM cases WHERE id = $1", &[i(id)])?
                .ok_or_else(|| err("anticheat.case_not_found", "case not found"))?
                .n(0);
            let r = q1(&format!("SELECT text, model, {}, case_version FROM explanations WHERE case_id = $1 ORDER BY case_version DESC LIMIT 1", ms("created_at")), &[i(id)])?
                .ok_or_else(|| err("kernel.not_found", "no explanation yet"))?;
            Reply::ok(
                &json!({ "text": r.s(0), "model": r.s(1), "created_at": db::iso(r.n(2)), "case_version": r.n(3), "stale": r.n(3) != v }),
            )
        }
        other => return Err(err("kernel.not_found", &format!("unknown route {other}"))),
    })
}

// ---------------- 服务 ----------------

fn actor_user() -> R<i64> {
    let inv = pctx::current();
    match inv.actor.kind {
        pt_plugin_sdk::types::ActorKind::User | pt_plugin_sdk::types::ActorKind::Mcp => inv
            .actor
            .user_id
            .and_then(|u| i64::try_from(u).ok())
            .ok_or_else(|| err("kernel.unauthenticated", "user required")),
        _ => Err(err(
            "anticheat.action_not_allowed",
            "resolve requires a user actor",
        )),
    }
}

pub fn service(method: &str, a: &Value) -> R<Value> {
    let need = |s: &str| {
        if pctx::has_scope(s) {
            Ok(())
        } else {
            Err(err("anticheat.scope_required", &format!("scope `{s}` required")))
        }
    };
    match method {
        "risk" => {
            need("read")?;
            risk_json(
                a["user"]
                    .as_i64()
                    .or_else(|| a["user_id"].as_i64())
                    .ok_or_else(|| db::validation("user"))?,
            )
        }
        "cases" => {
            need("read")?;
            let f = &a["filter"];
            let strs = |k: &str| {
                f[k].as_array()
                    .map(|l| {
                        l.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            };
            let mut states = strs("states");
            if states.is_empty() {
                states = [
                    "open",
                    "resolved",
                    "dismissed",
                    "overturned",
                    "closed_stale",
                    "closed_user_deleted",
                ]
                .map(String::from)
                .to_vec();
            }
            list_cases(
                &states,
                &strs("bands"),
                f["user"].as_i64(),
                f["primary_rule"].as_str(),
                f["assigned_to"].as_i64(),
                "-priority",
                a["cursor"].as_str().and_then(|c| c.parse().ok()),
                a["limit"].as_i64().unwrap_or(50),
            )
        }
        "case" => {
            need("read")?;
            let mut d = case_detail(a["id"].as_i64().ok_or_else(|| db::validation("id"))?, false)?;
            d["notes"] = json!([]);
            Ok(d)
        }
        "resolve" => {
            need("write")?;
            let actor = actor_user()?;
            let key = a["idempotency_key"]
                .as_str()
                .filter(|k| (1..=128).contains(&k.len()))
                .ok_or_else(|| db::validation("idempotency_key"))?;
            let ak = format!("{}:{actor}", pctx::caller());
            if let Some(prev) = decisions::idem_begin(&ak, key, a)? {
                return Ok(prev);
            }
            let d: Decision =
                serde_json::from_value(json!({ "decision": a["decision"], "actions": a["actions"],
                "false_positive_evidence_ids": a["false_positive_evidence_ids"], "note": a["note"] }))
                .map_err(|e| db::validation(&e.to_string()))?;
            let r = decisions::resolve(
                a["case_id"].as_i64().ok_or_else(|| db::validation("case_id"))?,
                &d,
                actor,
            )?;
            decisions::idem_end(&ak, key, &r)?;
            Ok(r)
        }
        "report" => Ok(json!(jobs::report(a)?)),
        other => Err(err(
            "kernel.not_found",
            &format!("ctx.anticheat has no method `{other}`"),
        )),
    }
}

// ---------------- MCP ----------------

pub fn mcp(tool: &str, a: &Value) -> R<Value> {
    let uid = me()?;
    match tool {
        "admin_list_anticheat_cases" => {
            decisions::require(uid, "anticheat.cases.read")?;
            let strs = |k: &str| {
                a[k].as_array()
                    .map(|l| {
                        l.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            };
            let r = list_cases(
                &strs("states"),
                &strs("bands"),
                None,
                None,
                None,
                "-priority",
                None,
                a["limit"].as_i64().unwrap_or(20).clamp(1, 50),
            )?;
            Ok(
                json!({ "items": r["items"].as_array().cloned().unwrap_or_default().iter().map(|c| json!({
                "case_id": c["id"], "user_id": c["user"]["id"], "username": c["user"]["username"], "score": c["score"], "band": c["band"],
                "primary_rule": c["primary_rule"], "opened_at": c["opened_at"], "waiting_hours": c["waiting_hours"], "assigned_to": c["assigned_to"] })).collect::<Vec<_>>() }),
            )
        }
        "admin_explain_anticheat_case" => {
            decisions::require(uid, "anticheat.cases.read")?;
            let id = a["case_id"].as_i64().ok_or_else(|| db::validation("case_id"))?;
            let deadline = pctx::current().deadline;
            if deadline - now() < 10_000 {
                let (_, v) = jobs::explain_data(id)?;
                pctx::schedule(
                    "explain_case",
                    now(),
                    &json!({ "case_id": id, "requested_by": uid }),
                    Some(&format!("explain:{id}:{v}")),
                )?;
                return Ok(json!({ "case_id": id, "status": "queued" }));
            }
            match jobs::explain(id, Some(uid)) {
                Ok((text, model)) => {
                    Ok(json!({ "case_id": id, "status": "ready", "explanation": text, "model": model }))
                }
                Err(e) if db::retryable(&e) => {
                    let (_, v) = jobs::explain_data(id)?;
                    pctx::schedule(
                        "explain_case",
                        now(),
                        &json!({ "case_id": id, "requested_by": uid }),
                        Some(&format!("explain:{id}:{v}")),
                    )?;
                    Ok(json!({ "case_id": id, "status": "queued" }))
                }
                Err(e) => Err(e),
            }
        }
        "admin_resolve_anticheat_case" => {
            decisions::require(uid, "anticheat.cases.moderate")?;
            let d: Decision = serde_json::from_value(json!({ "decision": a["decision"], "actions": a["actions"].as_array().cloned().unwrap_or_default(),
                "false_positive_evidence_ids": a["false_positive_evidence_ids"].as_array().cloned().unwrap_or_default(), "note": a["note"] }))
                .map_err(|e| db::validation(&e.to_string()))?;
            let r = decisions::resolve(
                a["case_id"].as_i64().ok_or_else(|| db::validation("case_id"))?,
                &d,
                uid,
            )?;
            Ok(json!({ "case_id": r["case_id"], "state": r["state"], "sanctions": r["sanctions"] }))
        }
        other => Err(err("kernel.not_found", &format!("unknown tool `{other}`"))),
    }
}

// ---------------- 钩子 ----------------

pub enum Hook {
    Allow,
    Deny(&'static str, &'static str),
}

pub fn hook(name: &str, input: &Value) -> R<Hook> {
    let st = db::settings();
    match name {
        "ledger.before_transfer" => {
            let op = input["operation"].as_str().unwrap_or("transfer");
            if !st.hooks_block_operations.iter().any(|o| o == op) {
                return Ok(Hook::Allow);
            }
            let Some(user) = input["from_user"].as_i64() else {
                return Ok(Hook::Allow);
            };
            let blocked = q1(
                "SELECT transfer_blocked FROM user_risk WHERE user_id = $1",
                &[i(user)],
            )?
            .is_some_and(|r| r.b(0));
            db::counter(
                "hook_decisions_total",
                &[
                    ("hook", name),
                    ("decision", if blocked { "deny" } else { "allow" }),
                ],
            );
            Ok(if blocked {
                Hook::Deny("anticheat.transfer_blocked", "账户正在接受审核，暂时无法转出")
            } else {
                Hook::Allow
            })
        }
        "checkin.before_check_in" => {
            if !st.hooks_block_checkin_when_frozen {
                return Ok(Hook::Allow);
            }
            let Some(user) = input["user_id"].as_i64() else {
                return Ok(Hook::Allow);
            };
            let frozen = q1(
                "SELECT 1 FROM effects WHERE user_id = $1 AND effect = 'freeze' AND state = 'synced' AND (applied->>'active')::boolean",
                &[i(user)],
            )?
            .is_some();
            Ok(if frozen {
                Hook::Deny("anticheat.checkin_blocked", "账户流量冻结期间不能签到")
            } else {
                Hook::Allow
            })
        }
        _ => Ok(Hook::Allow),
    }
}
