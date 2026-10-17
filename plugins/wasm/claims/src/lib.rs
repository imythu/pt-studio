//! claims 插件（WASM，docs/plugins/claims/README.md）：种子认领。
//!
//! - 认领自己下载完成过的种子（开关、每人 / 每种上限、发布时长、取消后的冷却），记录累计做种秒数为基线。
//! - 每月 1 日结算上个周期：做种时长（相对基线的增量）达到要求的发魔力值，周期中途认领的按覆盖天数折算要求；
//!   连续不达标若干次取消认领。按人记 `settlements`（幂等），奖励经 `ctx.ledger` 幂等发行。
//! - 种子删除时取消相关认领（不计失败）。

pt_plugin_sdk::world!("claims", http);

use exports::pt::plugin::{http_handler, on_cron, on_event, service_provider};
use pt_plugin_sdk::{prelude::*, serde_json, services, sql};

struct Claims;

const MONTH_SECS: f64 = 30.0 * 86_400.0;

struct Settings {
    enabled: bool,
    max_per_user: i64,
    max_per_torrent: i64,
    require_completed: bool,
    min_age_days: i64,
    monthly_seed_hours: i64,
    reward_bonus: i64,
    remove_after: i64,
    cooldown_days: i64,
}

fn settings() -> Settings {
    let s = ctx::settings();
    let int = |k: &str, d: i64| s[k].as_i64().unwrap_or(d).max(0);
    Settings {
        enabled: s["enabled"].as_bool().unwrap_or(true),
        max_per_user: int("max_claims_per_user", 1000),
        max_per_torrent: int("max_claimers_per_torrent", 0),
        require_completed: s["require_completed"].as_bool().unwrap_or(true),
        min_age_days: int("min_torrent_age_days", 0),
        monthly_seed_hours: int("monthly_seed_hours", 500),
        reward_bonus: int("reward_bonus", 300),
        remove_after: int("remove_after_failures", 2),
        cooldown_days: int("unclaim_cooldown_days", 0),
    }
}

/// 本周期的要求（秒）：按覆盖时长折算，最多一个月的要求
pub fn required_secs(monthly_hours: i64, covered_secs: f64) -> i64 {
    let full = monthly_hours as f64 * 3600.0;
    let frac = (covered_secs / MONTH_SECS).clamp(0.0, 1.0);
    // 有界：≤ 744 h × 3600
    #[allow(clippy::cast_possible_truncation)]
    let r = (full * frac).ceil() as i64;
    r
}

fn seed_secs(uid: i64, tid: i64) -> R<i64> {
    let r = sql::query(
        "SELECT seed_secs FROM accounting.v_snatch WHERE user_id = $1 AND torrent_id = $2",
        &[SqlValue::Int(uid), SqlValue::Int(tid)],
    )?;
    Ok(r.rows.first().and_then(|row| row.int(0)).unwrap_or(0))
}

fn publish(event: &str, uid: i64, payload: &Value) -> R<()> {
    ctx::publish(event, &format!("user:{uid}"), payload)?;
    Ok(())
}

// ---------------- 认领 ----------------

fn claim(uid: i64, tid: i64) -> R<Value> {
    let s = settings();
    if !s.enabled {
        return Err(err("claims.disabled", "claiming is turned off"));
    }
    let t = sql::query(
        "SELECT status, extract(epoch FROM now() - created_at)::bigint FROM torrent.v_torrent_public WHERE id = $1",
        &[SqlValue::Int(tid)],
    )?;
    let Some(row) = t.rows.first() else {
        return Err(err("kernel.not_found", "torrent not found"));
    };
    if row.text(0).as_deref() != Some("approved") {
        return Err(err("claims.not_allowed", "only published torrents can be claimed"));
    }
    if row.int(1).unwrap_or(0) < s.min_age_days * 86_400 {
        return Err(err("claims.not_allowed", "this torrent is too new to be claimed"));
    }
    if s.require_completed {
        let done = sql::query(
            "SELECT 1 FROM accounting.v_snatch WHERE user_id = $1 AND torrent_id = $2 AND completed_at IS NOT NULL",
            &[SqlValue::Int(uid), SqlValue::Int(tid)],
        )?;
        if done.rows.is_empty() {
            return Err(err("claims.not_completed", "you can only claim torrents you have downloaded completely"));
        }
    }
    let one = |q: &str, p: &[SqlValue]| -> R<i64> {
        Ok(sql::query(q, p)?.rows.first().and_then(|r| r.int(0)).unwrap_or(0))
    };
    if one("SELECT count(*) FROM claims WHERE user_id = $1 AND state = 'active'", &[SqlValue::Int(uid)])? >= s.max_per_user {
        return Err(err("claims.limit_reached", "you have reached the claim limit"));
    }
    if s.max_per_torrent > 0
        && one("SELECT count(*) FROM claims WHERE torrent_id = $1 AND state = 'active'", &[SqlValue::Int(tid)])? >= s.max_per_torrent
    {
        return Err(err("claims.torrent_full", "this torrent already has enough claimers"));
    }
    if s.cooldown_days > 0
        && one(
            "SELECT count(*) FROM claims WHERE user_id = $1 AND torrent_id = $2 AND state = 'removed'
                AND removed_at > now() - make_interval(days => $3::int)",
            &[SqlValue::Int(uid), SqlValue::Int(tid), SqlValue::Int(s.cooldown_days)],
        )? > 0
    {
        return Err(err("claims.cooldown", "you unclaimed this torrent recently"));
    }
    let base = seed_secs(uid, tid)?;
    let r = sql::query(
        "INSERT INTO claims (user_id, torrent_id, seed_secs_base) VALUES ($1, $2, $3)
         ON CONFLICT (user_id, torrent_id) WHERE state = 'active' DO NOTHING RETURNING id",
        &[SqlValue::Int(uid), SqlValue::Int(tid), SqlValue::Int(base)],
    )?;
    let Some(id) = r.rows.first().and_then(|row| row.int(0)) else {
        return Err(err("claims.already_claimed", "you already claimed this torrent"));
    };
    publish("claim.created", uid, &json!({ "user_id": uid, "torrent_id": tid, "claim_id": id }))?;
    Ok(json!({ "id": id }))
}

fn unclaim(uid: i64, tid: i64, reason: &str) -> R<bool> {
    let r = sql::query(
        "UPDATE claims SET state = 'removed', removed_at = now(), remove_reason = $3
          WHERE user_id = $1 AND torrent_id = $2 AND state = 'active' RETURNING id",
        &[SqlValue::Int(uid), SqlValue::Int(tid), SqlValue::Text(reason.into())],
    )?;
    let Some(id) = r.rows.first().and_then(|row| row.int(0)) else { return Ok(false) };
    publish("claim.removed", uid, &json!({ "user_id": uid, "torrent_id": tid, "claim_id": id, "reason": reason }))?;
    Ok(true)
}

fn torrent_info(tid: i64, me: Option<i64>) -> R<Value> {
    let r = sql::query(
        "SELECT count(*), bool_or(user_id = $2) FROM claims WHERE torrent_id = $1 AND state = 'active'",
        &[SqlValue::Int(tid), me.map_or(SqlValue::Null, SqlValue::Int)],
    )?;
    let row = r.rows.first();
    Ok(json!({ "claimers": row.and_then(|x| x.int(0)).unwrap_or(0), "mine": row.and_then(|x| x.bool(1)).unwrap_or(false),
               "enabled": settings().enabled }))
}

fn claimers(tid: i64, staff: bool) -> R<Value> {
    let r = sql::query(
        "SELECT c.user_id, u.username, (extract(epoch FROM c.claimed_at) * 1000)::bigint, c.last_result
           FROM claims c LEFT JOIN identity.v_user_public u ON u.id = c.user_id
          WHERE c.torrent_id = $1 AND c.state = 'active' ORDER BY c.claimed_at LIMIT 200",
        &[SqlValue::Int(tid)],
    )?;
    let items: Vec<Value> = r
        .rows
        .iter()
        .map(|row| {
            json!({ "user_id": if staff { row.int(0) } else { None }, "username": row.text(1), "claimed_at": row.int(2),
                    "last_result": row.text(3) })
        })
        .collect();
    Ok(json!({ "items": items }))
}

fn my_claims(uid: i64) -> R<Value> {
    let s = settings();
    let r = sql::query(
        "SELECT c.torrent_id, t.title, (extract(epoch FROM c.claimed_at) * 1000)::bigint, extract(epoch FROM now() - c.period_start)::float8,
                coalesce(sn.seed_secs, 0) - c.seed_secs_base, c.fails, c.last_result,
                extract(epoch FROM date_trunc('month', now()) + interval '1 month' - c.period_start)::float8
           FROM claims c
           LEFT JOIN torrent.v_torrent_public t ON t.id = c.torrent_id
           LEFT JOIN accounting.v_snatch sn ON sn.user_id = c.user_id AND sn.torrent_id = c.torrent_id
          WHERE c.user_id = $1 AND c.state = 'active' ORDER BY c.claimed_at DESC LIMIT 1000",
        &[SqlValue::Int(uid)],
    )?;
    let items: Vec<Value> = r
        .rows
        .iter()
        .map(|row| {
            // 本周期截至月底的覆盖时长决定本月要求
            let covered_to_month_end = row.float(7).unwrap_or(MONTH_SECS);
            json!({ "torrent_id": row.int(0), "title": row.text(1), "claimed_at": row.int(2),
                    "seed_secs": row.int(4).unwrap_or(0).max(0),
                    "required_secs": required_secs(s.monthly_seed_hours, covered_to_month_end),
                    "fails": row.int(5), "last_result": row.text(6) })
        })
        .collect();
    let last = sql::query(
        "SELECT period::text, claims, passed, failed, removed, reward FROM settlements WHERE user_id = $1 ORDER BY period DESC LIMIT 1",
        &[SqlValue::Int(uid)],
    )?;
    let last = last.rows.first().map(|row| {
        json!({ "period": row.text(0), "claims": row.int(1), "passed": row.int(2), "failed": row.int(3), "removed": row.int(4), "reward": row.int(5) })
    });
    Ok(json!({ "items": items, "max_claims": s.max_per_user, "monthly_seed_hours": s.monthly_seed_hours,
               "reward_bonus": s.reward_bonus, "last_settlement": last }))
}

// ---------------- 月结 ----------------

/// 结算上个周期：本月 1 日之前开始的有效认领（按用户分批、每批提交，已结算的用户跳过）
fn settle() -> R<()> {
    let s = settings();
    let period: String = sql::query("SELECT date_trunc('month', now())::date::text", &[])?
        .rows
        .first()
        .and_then(|r| r.text(0))
        .unwrap_or_default();
    let mut last_uid = 0i64;
    loop {
        let users = sql::query(
            "SELECT DISTINCT user_id FROM claims
              WHERE state = 'active' AND period_start < date_trunc('month', now()) AND user_id > $1
                AND NOT EXISTS (SELECT 1 FROM settlements st WHERE st.period = $2::date AND st.user_id = claims.user_id)
              ORDER BY user_id LIMIT 200",
            &[SqlValue::Int(last_uid), SqlValue::Text(period.clone())],
        )?;
        let ids: Vec<i64> = users.rows.iter().filter_map(|r| r.int(0)).collect();
        let Some(max) = ids.last().copied() else { break };
        for uid in ids {
            settle_user(uid, &period, &s)?;
        }
        sql::commit()?;
        last_uid = max;
    }
    Ok(())
}

fn settle_user(uid: i64, period: &str, s: &Settings) -> R<()> {
    let rows = sql::query(
        "SELECT c.id, c.torrent_id, coalesce(sn.seed_secs, 0), c.seed_secs_base, c.fails,
                extract(epoch FROM date_trunc('month', now()) - c.period_start)::float8
           FROM claims c LEFT JOIN accounting.v_snatch sn ON sn.user_id = c.user_id AND sn.torrent_id = c.torrent_id
          WHERE c.user_id = $1 AND c.state = 'active' AND c.period_start < date_trunc('month', now())
          FOR UPDATE OF c",
        &[SqlValue::Int(uid)],
    )?;
    let (mut passed, mut failed, mut removed) = (0i64, 0i64, 0i64);
    for row in &rows.rows {
        let (id, tid, cur, base, fails) =
            (row.int(0).unwrap_or(0), row.int(1).unwrap_or(0), row.int(2).unwrap_or(0), row.int(3).unwrap_or(0), row.int(4).unwrap_or(0));
        let need = required_secs(s.monthly_seed_hours, row.float(5).unwrap_or(MONTH_SECS));
        let ok = cur - base >= need;
        let fails = if ok { 0 } else { fails + 1 };
        let remove = !ok && s.remove_after > 0 && fails >= s.remove_after;
        if ok {
            passed += 1;
        } else {
            failed += 1;
        }
        sql::execute(
            "UPDATE claims SET seed_secs_base = $2, period_start = date_trunc('month', now()), fails = $3, last_result = $4,
                    state = CASE WHEN $5 THEN 'removed' ELSE state END,
                    removed_at = CASE WHEN $5 THEN now() ELSE removed_at END,
                    remove_reason = CASE WHEN $5 THEN 'seeding_requirement' ELSE remove_reason END
              WHERE id = $1",
            &[
                SqlValue::Int(id),
                SqlValue::Int(cur),
                SqlValue::Int(fails),
                SqlValue::Text(if ok { "passed" } else { "failed" }.into()),
                SqlValue::Boolean(remove),
            ],
        )?;
        if remove {
            removed += 1;
            publish("claim.removed", uid, &json!({ "user_id": uid, "torrent_id": tid, "claim_id": id, "reason": "seeding_requirement" }))?;
        }
    }
    let reward = passed * s.reward_bonus;
    sql::execute(
        "INSERT INTO settlements (period, user_id, claims, passed, failed, removed, reward) VALUES ($1::date, $2, $3, $4, $5, $6, $7)",
        &[
            SqlValue::Text(period.into()),
            SqlValue::Int(uid),
            SqlValue::Int(passed + failed),
            SqlValue::Int(passed),
            SqlValue::Int(failed),
            SqlValue::Int(removed),
            SqlValue::Int(reward),
        ],
    )?;
    if reward > 0 {
        ctx::call(
            "ctx.ledger",
            "transfer",
            &json!({ "currency": "bonus", "from_user": null, "to_user": uid, "amount": reward, "reason": "claims.monthly_reward",
                     "memo": period, "idempotency_key": format!("claims:{period}:{uid}") }),
        )?;
    }
    publish("claim.settled", uid, &json!({ "user_id": uid, "period": period, "passed": passed, "failed": failed, "removed": removed, "reward": reward }))?;
    let _ = services::call(
        "ctx.notify",
        "send",
        &json!({ "user_id": uid, "template_key": "claims.settled", "category": "economy",
                 "title": "认领月度结算 / Claims settled",
                 "body": format!("上月认领 {} 个：达标 {passed}，未达标 {failed}，取消 {removed}，奖励 {reward} 魔力值。\nLast month: {passed} passed, {failed} failed, {removed} removed, {reward} bonus.", passed + failed),
                 "link": "/p/claims/", "args": { "passed": passed, "failed": failed, "removed": removed, "reward": reward },
                 "idempotency_key": format!("claims:n:{period}:{uid}") })
        .to_string(),
    );
    Ok(())
}

// ---------------- 导出 ----------------

impl on_event::Guest for Claims {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        if evt.event_type != "torrent.deleted" {
            return Ok(());
        }
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        let Some(tid) = p["torrent_id"].as_i64() else { return Ok(()) };
        let r = sql::query(
            "UPDATE claims SET state = 'removed', removed_at = now(), remove_reason = 'torrent_deleted'
              WHERE torrent_id = $1 AND state = 'active' RETURNING user_id, id",
            &[SqlValue::Int(tid)],
        )?;
        for row in &r.rows {
            let uid = row.int(0).unwrap_or(0);
            publish("claim.removed", uid, &json!({ "user_id": uid, "torrent_id": tid, "claim_id": row.int(1), "reason": "torrent_deleted" }))?;
        }
        Ok(())
    }
}

impl on_cron::Guest for Claims {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            "settle" => settle(),
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

impl service_provider::Guest for Claims {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        match method.as_str() {
            // 其他插件查询某用户的有效认领数（如等级条件）
            "count_of" => {
                let uid = a["user"].as_i64().ok_or_else(|| err("kernel.validation_failed", "user"))?;
                let n = sql::query("SELECT count(*) FROM claims WHERE user_id = $1 AND state = 'active'", &[SqlValue::Int(uid)])?
                    .rows
                    .first()
                    .and_then(|r| r.int(0))
                    .unwrap_or(0);
                Ok(json!(n).to_string())
            }
            other => Err(err("kernel.not_found", &format!("ctx.claims has no method `{other}`"))),
        }
    }
}

fn is_staff() -> bool {
    ctx::call("ctx.rbac", "check", &json!({ "user": ctx::current_user_opt().unwrap_or(0), "permission": "claims.manage" }))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

impl http_handler::Guest for Claims {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let reply = match req.route() {
            "claim" => http::Reply::created(&claim(ctx::current_user()?, req.param_i64("id")?)?),
            "unclaim" => {
                if !unclaim(ctx::current_user()?, req.param_i64("id")?, "user")? {
                    return Err(err("kernel.not_found", "you have not claimed this torrent"));
                }
                http::Reply::no_content()
            }
            "torrent" => http::Reply::ok(&torrent_info(req.param_i64("id")?, ctx::current_user_opt())?),
            "claimers" => http::Reply::ok(&claimers(req.param_i64("id")?, is_staff())?),
            "me" => http::Reply::ok(&my_claims(ctx::current_user()?)?),
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(Claims);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prorated_requirement() {
        assert_eq!(required_secs(500, MONTH_SECS), 500 * 3600);
        assert_eq!(required_secs(500, 2.0 * MONTH_SECS), 500 * 3600);
        assert_eq!(required_secs(500, MONTH_SECS / 2.0), 250 * 3600);
        assert_eq!(required_secs(500, 0.0), 0);
    }
}
