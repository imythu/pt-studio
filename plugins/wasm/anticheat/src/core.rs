//! 规则集加载（§1.2）、评分上下文（§1.3）、重算（§1.4）、案件开立与更新（§1.5）、自动处置与熔断（§1.7）、规则统计（§1.13）。

use std::collections::BTreeSet;

use pt_plugin_sdk::{
    PluginError, R, ctx as pctx, err, kv,
    serde_json::{Value, json},
};

use crate::db::{self, DAY, RowGet, i, j, ms, now, oi, q, q1, t, ts, x};
use crate::effects;
use crate::rules::{self, Band, Ctx, Evidence, Ruleset, Score, Sev};

// ---------------- 规则集 ----------------

pub struct Active {
    pub version: i64,
    pub rs: Ruleset,
}

pub fn checksum(def: &Value) -> String {
    format!("sha256:{}", db::hex(&db::sha256(def.to_string().as_bytes())))
}

/// 生效规则集；从未有过规则集时写入默认 v1
pub fn active() -> R<Active> {
    let row = match q1(
        "SELECT version, definition FROM rulesets WHERE status = 'active'",
        &[],
    )? {
        Some(r) => r,
        None => {
            let def = rules::default_ruleset();
            x(
                "INSERT INTO rulesets (version, status, definition, checksum, note, activated_at)
                 VALUES (1, 'active', $1, $2, 'default', now()) ON CONFLICT DO NOTHING",
                &[j(&def), t(&checksum(&def))],
            )?;
            q1(
                "SELECT version, definition FROM rulesets WHERE status = 'active'",
                &[],
            )?
            .ok_or_else(|| err("anticheat.ruleset_not_found", "no active ruleset"))?
        }
    };
    let rs = serde_json::from_value(row.v(1)).map_err(|e| {
        err(
            "anticheat.ruleset_invalid",
            &format!("active ruleset is invalid: {e}"),
        )
    })?;
    Ok(Active {
        version: row.n(0),
        rs,
    })
}

/// 内置默认规则集升级：生效版本仍是未改动过的内置默认（`note = 'default'`、无创建人）且与当前内置定义不同时，
/// 写入新版本并激活、旧版本退役、全量重算。管理员建过的版本（有创建人）不受影响。`queue_maintenance` 调用。
pub fn upgrade_default() -> R<Option<i64>> {
    let def = rules::default_ruleset();
    let sum = checksum(&def);
    let Some(r) = q1(
        "SELECT version, checksum FROM rulesets WHERE status = 'active' AND note = 'default' AND created_by IS NULL",
        &[],
    )?
    else {
        return Ok(None);
    };
    if r.s(1) == sum {
        return Ok(None);
    }
    let old = r.n(0);
    let Some(v) = q1(
        "INSERT INTO rulesets (version, status, definition, checksum, based_on, note)
         SELECT max(version) + 1, 'draft', $1, $2, $3, 'default' FROM rulesets ON CONFLICT DO NOTHING RETURNING version",
        &[j(&def), t(&sum), i(old)],
    )?
    else {
        return Ok(None);
    };
    let v = v.n(0);
    x(
        "UPDATE rulesets SET status = 'retired', updated_at = now() WHERE version = $1 AND status = 'active'",
        &[i(old)],
    )?;
    x(
        "UPDATE rulesets SET status = 'active', activated_at = now(), updated_at = now() WHERE version = $1",
        &[i(v)],
    )?;
    db::audit(
        "anticheat.ruleset_activate",
        "plugin:anticheat",
        &json!({ "from": old, "to": v, "note": "built-in default upgraded", "checksum": sum }),
    )?;
    start_rescore(v)?;
    Ok(Some(v))
}

/// 激活后全量重算（分批任务）
pub fn start_rescore(version: i64) -> R<()> {
    kv::set("rescoring", version.to_string().as_bytes(), Some(now() + DAY))?;
    pctx::schedule(
        "rescore_batch",
        now(),
        &json!({ "ruleset_version": version, "cursor": 0 }),
        Some(&format!("rs:{version}:0")),
    )?;
    Ok(())
}

pub fn ruleset(version: i64) -> R<(Ruleset, String, String)> {
    let r = q1(
        "SELECT definition, status, checksum FROM rulesets WHERE version = $1",
        &[i(version)],
    )?
    .ok_or_else(|| err("anticheat.ruleset_not_found", "ruleset not found"))?;
    let rs = serde_json::from_value(r.v(0))
        .map_err(|e| err("anticheat.ruleset_invalid", &format!("ruleset is invalid: {e}")))?;
    Ok((rs, r.s(1), r.s(2)))
}

// ---------------- 上下文 ----------------

/// 用户上下文（24 h 缓存）。外部服务失败时用旧缓存；无缓存时按“受保护”保守计算（不会被自动处置）
pub fn context(user: i64) -> R<Ctx> {
    let cached = q1(
        &format!(
            "SELECT {}, class_id, is_protected, confirmed_cases_365d, warnings_90d, refreshed_at > now() - interval '24 hours'
               FROM user_context WHERE user_id = $1",
            ms("account_created_at")
        ),
        &[i(user)],
    )?;
    if let Some(c) = &cached
        && c.b(5)
    {
        return Ok(ctx_of(c));
    }
    let s = db::settings();
    let u = q1(
        &format!(
            "SELECT {}, class_id FROM identity.v_user_public WHERE id = $1",
            ms("created_at")
        ),
        &[i(user)],
    )?;
    let roles = db::call("ctx.rbac", "roles_of", &json!({ "user": user }));
    let (Some(u), Ok(roles)) = (u, roles) else {
        db::counter("context_stale_total", &[("service", "identity")]);
        return Ok(cached.map_or_else(
            || Ctx {
                account_age_days: 0,
                is_protected: true,
                ..Ctx::default()
            },
            |c| ctx_of(&c),
        ));
    };
    let rank = roles
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(db::role_rank)
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    let protected = rank >= db::role_rank(&s.protected_min_role).max(1);
    let confirmed = q1(
        "SELECT count(*) FROM cases WHERE user_id = $1 AND resolution = 'sanctioned' AND resolved_at > now() - interval '365 days'",
        &[i(user)],
    )?
    .map_or(0, |r| r.n(0));
    let warnings = q1(
        "SELECT count(*) FROM sanctions WHERE user_id = $1 AND kind = 'warn' AND created_at > now() - interval '90 days'
            AND state <> 'overturned'",
        &[i(user)],
    )?
    .map_or(0, |r| r.n(0));
    let row = q1(
        &format!(
            "INSERT INTO user_context (user_id, account_created_at, class_id, is_protected, confirmed_cases_365d, warnings_90d, refreshed_at)
             VALUES ($1, {}, $3, $4, $5, $6, now())
             ON CONFLICT (user_id) DO UPDATE SET account_created_at = EXCLUDED.account_created_at, class_id = EXCLUDED.class_id,
                is_protected = EXCLUDED.is_protected, confirmed_cases_365d = EXCLUDED.confirmed_cases_365d,
                warnings_90d = EXCLUDED.warnings_90d, refreshed_at = now()
             RETURNING {}, class_id, is_protected, confirmed_cases_365d, warnings_90d, true",
            ts(2),
            ms("account_created_at")
        ),
        &[i(user), i(u.n(0)), i(u.n(1)), pt_plugin_sdk::SqlValue::Boolean(protected), i(confirmed), i(warnings)],
    )?
    .ok_or_else(|| err("kernel.internal", "user_context upsert"))?;
    Ok(ctx_of(&row))
}

fn ctx_of(r: &db::Row) -> Ctx {
    Ctx {
        account_age_days: (now() - r.n(0)).max(0) / DAY,
        class_id: r.n(1),
        is_protected: r.b(2),
        confirmed_cases_365d: r.n(3),
        warnings_90d: r.n(4),
        seedbox: BTreeSet::new(),
    }
}

/// 上下文失效（决定、处置后，确认次数 / 警告数会变化）
pub fn invalidate_context(user: i64) -> R<()> {
    x("DELETE FROM user_context WHERE user_id = $1", &[i(user)])?;
    Ok(())
}

// ---------------- 评分 ----------------

pub fn load_evidence(user: i64, window_days: f64, max: i64, at: i64) -> R<(Vec<Evidence>, bool)> {
    let from = at - (window_days * DAY as f64) as i64;
    let rows = q(
        &format!(
            "SELECT id, kind, severity, hits, torrent_id, details, {} FROM evidence
              WHERE user_id = $1 AND occurred_at >= {} AND occurred_at <= {}
                AND verdict IS DISTINCT FROM 'false_positive'
              ORDER BY occurred_at DESC, id DESC LIMIT $4",
            ms("occurred_at"),
            ts(2),
            ts(3)
        ),
        &[i(user), i(from), i(at), i(max.clamp(1, 5000) + 1)],
    )?;
    let truncated = rows.len() as i64 > max;
    let ev = rows
        .iter()
        .take(usize::try_from(max).unwrap_or(0))
        .map(|r| Evidence {
            id: r.n(0),
            kind: r.s(1),
            severity: r.os(2).and_then(|s| Sev::parse(&s)),
            hits: r.n(3),
            torrent_id: r.on(4),
            details: r.v(5),
            occurred_at: r.n(6),
        })
        .collect();
    Ok((ev, truncated))
}

/// 盒子种子（§1.3 `ctx.is_seedbox`）：证据产生时 peer IP 命中已批准盒子网段（accounting 在信号 details 记
/// `seedbox: true`），或该用户当前在该种子上有盒子 peer（`accounting.v_swarm_peers.is_seedbox`）。
/// seedbox 插件未安装时两者都为空。
pub fn seedbox_torrents(user: i64, ev: &[Evidence]) -> R<BTreeSet<i64>> {
    let mut out = rules::seedbox_from_evidence(ev);
    let tids: BTreeSet<i64> = ev.iter().filter_map(|e| e.torrent_id).collect();
    if !tids.is_empty() {
        let rows = q(
            "SELECT DISTINCT torrent_id FROM accounting.v_swarm_peers WHERE user_id = $1 AND is_seedbox AND torrent_id = ANY($2)",
            &[i(user), db::ints(tids.into_iter().collect())],
        )?;
        out.extend(rows.iter().map(|r| r.n(0)));
    }
    Ok(out)
}

pub fn score_user(user: i64, rs: &Ruleset, at: i64) -> R<(Score, Vec<Evidence>, Ctx)> {
    let (ev, truncated) = load_evidence(user, rs.max_window_days(), rs.defaults.max_evidence, at)?;
    let mut ctx = context(user)?;
    ctx.seedbox = seedbox_torrents(user, &ev)?;
    let s = rules::compute(rs, &ev, &ctx, at, truncated);
    Ok((s, ev, ctx))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Evidence,
    Decay,
    Activation,
    Manual,
}

/// 重算（在调用方事务内）：分数 → user_risk → 案件 → 自动策略
pub fn rescore_user(user: i64, trigger: Trigger, auto_policies: bool) -> R<Score> {
    let a = active()?;
    let at = now();
    let (s, ev, ctx) = score_user(user, &a.rs, at)?;
    let old = q1(
        "SELECT band, peak_score FROM user_risk WHERE user_id = $1 FOR UPDATE",
        &[i(user)],
    )?;
    x(
        "INSERT INTO user_risk (user_id, score, shadow_score, peak_score, band, tags, breakdown, ruleset_version,
                                evidence_truncated, computed_at)
         VALUES ($1, $2, $3, $2, $4, $5, $6, $7, $8, now())
         ON CONFLICT (user_id) DO UPDATE SET score = EXCLUDED.score, shadow_score = EXCLUDED.shadow_score,
            peak_score = greatest(user_risk.peak_score, EXCLUDED.score), band = EXCLUDED.band, tags = EXCLUDED.tags,
            breakdown = EXCLUDED.breakdown, ruleset_version = EXCLUDED.ruleset_version,
            evidence_truncated = EXCLUDED.evidence_truncated, computed_at = now(), updated_at = now()",
        &[
            i(user),
            i(s.score),
            i(s.shadow_score),
            t(s.band.as_str()),
            db::texts(s.tags.clone()),
            j(&serde_json::to_value(&s.breakdown).unwrap_or_default()),
            i(a.version),
            pt_plugin_sdk::SqlValue::Boolean(s.truncated),
        ],
    )?;
    let from = old.as_ref().map_or_else(|| "none".to_owned(), |r| r.s(0));
    if from != s.band.as_str() {
        x(
            "INSERT INTO risk_history (user_id, at, from_band, to_band, score, ruleset_version) VALUES ($1, clock_timestamp(), $2, $3, $4, $5)
             ON CONFLICT DO NOTHING",
            &[i(user), t(&from), t(s.band.as_str()), i(s.score), i(a.version)],
        )?;
    }
    let case = on_score(user, &s, &ev, &a, trigger, &from)?;
    if auto_policies && let Some(c) = case {
        evaluate_policies(user, c, &s, &a, &ctx)?;
    }
    refresh_blocked(user, &a.rs)?;
    Ok(s)
}

/// 禁止转出：进行中案件且分数 ≥ transfer_block，或存在冻结 / 钱包处置
pub fn refresh_blocked(user: i64, rs: &Ruleset) -> R<()> {
    x(
        "UPDATE user_risk SET transfer_blocked =
            (open_case_id IS NOT NULL AND score >= $2)
            OR EXISTS (SELECT 1 FROM sanctions WHERE user_id = $1 AND kind IN ('freeze_traffic','lock_wallet')
                       AND state IN ('pending','active'))
          WHERE user_id = $1",
        &[i(user), i(rs.thresholds.transfer_block.ceil() as i64)],
    )?;
    Ok(())
}

// ---------------- 案件 ----------------

pub fn timeline(case_id: i64, kind: &str, actor: Option<i64>, data: &Value) -> R<()> {
    x(
        "INSERT INTO case_events (case_id, kind, actor_user_id, actor_plugin, data) VALUES ($1, $2, $3, $4, $5)",
        &[
            i(case_id),
            t(kind),
            oi(actor),
            if actor.is_none() {
                t("anticheat")
            } else {
                pt_plugin_sdk::SqlValue::Null
            },
            j(data),
        ],
    )?;
    Ok(())
}

pub fn priority(score: i64, band: Band, waiting_hours: i64) -> i64 {
    score + if band == Band::Critical { 20 } else { 0 } + (waiting_hours / 4).min(30)
}

fn snapshot(e: &Evidence) -> Value {
    json!({ "kind": e.kind, "severity": e.severity.map(Sev::as_str), "hits": e.hits, "torrent_id": e.torrent_id,
            "details": e.details, "occurred_at": e.occurred_at })
}

fn primary_rule(s: &Score) -> String {
    s.breakdown
        .iter()
        .find(|b| b.mode == rules::Mode::Enforce && b.score.is_some())
        .map_or_else(|| "manual".to_owned(), |b| b.rule.clone())
}

/// 开立案件（自动 / 手动），返回 (案件 ID, 是否新开)
pub fn open_case(user: i64, s: &Score, version: i64, reason: &str, by: Option<i64>) -> R<(i64, bool)> {
    let prior = q1(
        "SELECT id FROM cases WHERE user_id = $1 AND state <> 'open' ORDER BY opened_at DESC LIMIT 1",
        &[i(user)],
    )?
    .map(|r| r.n(0));
    let rule = if reason == "score" {
        primary_rule(s)
    } else {
        reason.to_owned()
    };
    let row = q1(
        "INSERT INTO cases (user_id, state, opened_reason, score_at_open, score, peak_score, band, primary_rule, ruleset_version,
                            prior_case_id, priority, opened_by)
         VALUES ($1, 'open', $2, $3, $3, $3, $4, $5, $6, $7, $8, $9)
         ON CONFLICT (user_id) WHERE state = 'open' DO NOTHING RETURNING id",
        &[
            i(user),
            t(reason),
            i(s.score),
            t(s.band.as_str()),
            t(&rule),
            i(version),
            oi(prior),
            i(priority(s.score, s.band, 0)),
            oi(by),
        ],
    )?;
    if let Some(r) = row {
        let id = r.n(0);
        timeline(
            id,
            "opened",
            by,
            &json!({ "score": s.score, "band": s.band.as_str(), "reason": reason }),
        )?;
        db::publish(
            "anticheat.case_opened",
            user,
            &json!({ "case_id": id, "user_id": user, "rule": rule, "score": s.score, "band": s.band.as_str(),
                     "opened_reason": reason, "ruleset_version": version,
                     "evidence_count": s.matched.values().map(Vec::len).sum::<usize>() }),
        )?;
        pctx::schedule(
            "enrich_case",
            now(),
            &json!({ "case_id": id }),
            Some(&format!("enrich:{id}")),
        )?;
        stats_bump(version, &s.rules_hit, "cases_opened")?;
        db::counter("cases_opened_total", &[("reason", reason)]);
        x(
            "UPDATE user_risk SET open_case_id = $2 WHERE user_id = $1",
            &[i(user), i(id)],
        )?;
        return Ok((id, true));
    }
    let id = q1(
        "SELECT id FROM cases WHERE user_id = $1 AND state = 'open'",
        &[i(user)],
    )?
    .ok_or_else(|| err("kernel.conflict", "case state changed concurrently").retryable())?
    .n(0);
    Ok((id, false))
}

/// 分数变化后的案件处理（§1.5 on_score）：band ≥ high 或命中自动策略（结论性规则）时开案；有进行中案件时追加证据与分数
fn on_score(
    user: i64,
    s: &Score,
    ev: &[Evidence],
    a: &Active,
    _trigger: Trigger,
    from_band: &str,
) -> R<Option<i64>> {
    let open = q1(
        "SELECT id, band FROM cases WHERE user_id = $1 AND state = 'open' FOR UPDATE",
        &[i(user)],
    )?;
    let conclusive =
        a.rs.policies
            .iter()
            .any(|p| p.auto && rules::policy_applies(p, s));
    let case = match open {
        Some(r) => Some(r.n(0)),
        None if s.band >= Band::High || conclusive => Some(open_case(user, s, a.version, "score", None)?.0),
        None => None,
    };
    let Some(c) = case else { return Ok(None) };
    let ids: BTreeSet<i64> = s.matched.values().flatten().copied().collect();
    let mut added = 0;
    for e in ev.iter().filter(|e| ids.contains(&e.id)) {
        added += x(
            "INSERT INTO case_evidence (case_id, evidence_id, snapshot) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
            &[i(c), i(e.id), j(&snapshot(e))],
        )?;
    }
    let row = q1(
        &format!("SELECT band, {} FROM cases WHERE id = $1", ms("opened_at")),
        &[i(c)],
    )?
    .ok_or_else(|| err("anticheat.case_not_found", "case not found"))?;
    let waiting = (now() - row.n(1)) / 3_600_000;
    x(
        "UPDATE cases SET score = $2, peak_score = greatest(peak_score, $2), band = $3, primary_rule = CASE WHEN opened_reason = 'score' THEN $4 ELSE primary_rule END,
                priority = $5, updated_at = now() WHERE id = $1",
        &[i(c), i(s.score), t(s.band.as_str()), t(&primary_rule(s)), i(priority(s.score, s.band, waiting))],
    )?;
    if added > 0 {
        timeline(
            c,
            "evidence_added",
            None,
            &json!({ "count": added, "score": s.score }),
        )?;
    }
    if row.s(0) != s.band.as_str() && from_band != s.band.as_str() {
        timeline(
            c,
            "band_changed",
            None,
            &json!({ "from": row.s(0), "to": s.band.as_str(), "score": s.score }),
        )?;
    }
    x(
        "UPDATE user_risk SET open_case_id = $2 WHERE user_id = $1",
        &[i(user), i(c)],
    )?;
    Ok(Some(c))
}

// ---------------- 自动处置与熔断（§1.7） ----------------

pub fn breaker_tripped() -> R<bool> {
    Ok(db::settings().breaker_tripped || kv::get("breaker")?.is_some())
}

fn evaluate_policies(user: i64, case_id: i64, s: &Score, a: &Active, ctx: &Ctx) -> R<()> {
    let st = db::settings();
    let fired: Vec<&rules::Policy> =
        a.rs.policies
            .iter()
            .filter(|p| p.auto && rules::policy_applies(p, s))
            .collect();
    if fired.is_empty() {
        return Ok(());
    }
    let blocked_reason = if !st.auto_actions_enabled {
        Some("auto_actions_disabled")
    } else if breaker_tripped()? {
        Some("breaker_tripped")
    } else if ctx.is_protected {
        Some("protected")
    } else {
        None
    };
    for p in fired {
        let done = q1(
            "SELECT 1 FROM sanctions WHERE case_id = $1 AND policy_id = $2 LIMIT 1",
            &[i(case_id), t(&p.id)],
        )?;
        if done.is_some() {
            continue;
        }
        if let Some(why) = blocked_reason {
            suggest(case_id, &p.id, &p.actions, why)?;
            continue;
        }
        let n = q1("SELECT coalesce(sum(count), 0)::bigint FROM auto_action_log WHERE at > now() - interval '1 hour'", &[])?
            .map_or(0, |r| r.n(0));
        if n + p.actions.len() as i64 > st.breaker_max_auto_per_hour {
            kv::set(
                "breaker",
                json!({ "tripped_at": now(), "count": n }).to_string().as_bytes(),
                None,
            )?;
            db::gauge("breaker_tripped", 1.0, &[]);
            pt_plugin_sdk::logging::error("anticheat breaker tripped", &[("count", &n.to_string())]);
            suggest(case_id, &p.id, &p.actions, "breaker_tripped")?;
            continue;
        }
        for act in &p.actions {
            let days = act.days.map(i64::from).or(match act.kind.as_str() {
                "freeze_traffic" => Some(st.freeze_default_days),
                "warn" => Some(st.warn_default_days),
                _ => None,
            });
            let until = days.map(|d| now() + d * DAY);
            effects::insert_sanction(
                user,
                Some(case_id),
                &act.kind,
                "pending",
                until,
                &json!({}),
                Some(&p.id),
                None,
            )?;
        }
        x(
            "INSERT INTO auto_action_log (user_id, case_id, policy_id, count) VALUES ($1, $2, $3, $4)",
            &[i(user), i(case_id), t(&p.id), i(p.actions.len() as i64)],
        )?;
        timeline(
            case_id,
            "auto_action",
            None,
            &json!({ "policy_id": p.id, "actions": p.actions }),
        )?;
        db::counter("auto_actions_total", &[("policy", &p.id)]);
    }
    effects::bump(user)?;
    Ok(())
}

fn suggest(case_id: i64, policy: &str, actions: &[rules::ActionSpec], why: &str) -> R<()> {
    timeline(
        case_id,
        "suggestion",
        None,
        &json!({ "policy_id": policy, "actions": actions, "reason": why }),
    )
}

// ---------------- 规则统计（§1.13） ----------------

pub fn stats_bump(version: i64, rules: &[String], field: &str) -> R<()> {
    if !matches!(
        field,
        "hits" | "cases_opened" | "confirmed" | "dismissed" | "overturned"
    ) {
        return Err(PluginError {
            code: "kernel.internal".into(),
            message: "bad stats field".into(),
            details: None,
            retryable: false,
        });
    }
    for r in rules {
        x(
            &format!(
                "INSERT INTO rule_stats (ruleset_version, rule_id, day, {field}) VALUES ($1, $2, current_date, 1)
                 ON CONFLICT (ruleset_version, rule_id, day) DO UPDATE SET {field} = rule_stats.{field} + 1"
            ),
            &[i(version), t(r)],
        )?;
    }
    Ok(())
}
