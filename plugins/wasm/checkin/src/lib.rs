//! checkin 插件（WASM，docs/plugins/checkin/）：每日签到。也是 `pt-plugin-sdk` 的参考插件。
//!
//! 本版本实现：签到（每日一次、连续天数、`checkin.before_check_in` 钩子、幂等键）、奖励（基础 + 每连续一天递增、上限、连续
//! 天数里程碑，经 `ctx.ledger` 发行，幂等键按用户与日期）、状态（今天是否已签、连续 / 最长 / 累计天数、本月日历、当日名次）、
//! 排行（连续天数）、`checkin.checked_in` 事件、MCP 工具 `check_in`（免确认）。补签卡、随机奖励、站点时区（当前按 UTC 计日）
//! 在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

pt_plugin_sdk::world!("checkin", http);

use exports::pt::plugin::{http_handler, mcp_tool};
use pt_plugin_sdk::{prelude::*, sql};

struct Checkin;

/// 奖励（LLD §1.3，纯函数）：返回 (每日, 里程碑天数与金额)
pub fn reward(s: &Value, streak: i64) -> (i64, Option<(i64, i64)>) {
    let base = s["reward_base"].as_i64().unwrap_or(10).max(0);
    let step = s["reward_step"].as_i64().unwrap_or(2).max(0);
    let cap = s["reward_cap"].as_i64().unwrap_or(50).max(base);
    let daily = base.saturating_add(step.saturating_mul(streak - 1)).min(cap);
    let milestone = s["milestones"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|m| m["days"].as_i64() == Some(streak))
        .and_then(|m| Some((streak, m["amount"].as_i64()?.max(0))));
    (daily, milestone)
}

fn today() -> R<String> {
    let r = sql::query("SELECT (now() AT TIME ZONE 'UTC')::date::text", &[])?;
    Ok(r.rows.first().and_then(|row| row.text(0)).unwrap_or_default())
}

fn pay(uid: i64, currency: &str, amount: i64, reason: &str, day: &str) -> R<()> {
    if amount <= 0 {
        return Ok(());
    }
    ctx::call(
        "ctx.ledger",
        "transfer",
        &json!({ "currency": currency, "from_user": null, "to_user": uid, "amount": amount.saturating_mul(ctx::currency_unit(currency)), "reason": reason, "memo": day,
                 "idempotency_key": format!("{reason}:{uid}:{day}") }),
    )?;
    Ok(())
}

/// 签到（LLD §1.2）
fn check_in(uid: i64, via: &str) -> R<Value> {
    let s = ctx::settings();
    if s["enabled"].as_bool() == Some(false) {
        return Err(err("checkin.disabled", "check-in is disabled"));
    }
    let u = sql::query(
        "SELECT status FROM identity.v_user_public WHERE id = $1",
        &[uid.into()],
    )?;
    if !matches!(
        u.rows.first().and_then(|r| r.text(0)).as_deref(),
        Some("active" | "leech_disabled")
    ) {
        return Err(err("checkin.not_allowed", "your account cannot check in"));
    }
    let day = today()?;
    ctx::run_hook(
        "checkin.before_check_in",
        &json!({ "version": 1, "user_id": uid, "kind": "daily", "date": day }),
    )?;
    // 锁定用户状态行
    sql::execute(
        "INSERT INTO user_state (user_id) VALUES ($1) ON CONFLICT DO NOTHING",
        &[uid.into()],
    )?;
    let st = sql::query(
        "SELECT streak, last_day::text, ($2::date - last_day) FROM user_state WHERE user_id = $1 FOR UPDATE",
        &[uid.into(), (&day).into()],
    )?;
    let row = st.rows.first().cloned().unwrap_or_default();
    if row.text(1).as_deref() == Some(day.as_str()) {
        return Err(err(
            "checkin.already_checked_in",
            "you have already checked in today",
        ));
    }
    let streak = if row.int(2) == Some(1) {
        row.int(0).unwrap_or(0) + 1
    } else {
        1
    };
    let (daily, milestone) = reward(&s, streak);
    let currency = s["currency"].as_str().unwrap_or("bonus").to_owned();
    let ins = sql::query(
        "INSERT INTO records (user_id, day, streak_after, reward_daily, reward_milestone, milestone_days, currency, via)
         VALUES ($1, $2::date, $3, $4, $5, $6, $7, $8) ON CONFLICT (user_id, day) DO NOTHING RETURNING id",
        &[
            uid.into(),
            (&day).into(),
            streak.into(),
            daily.into(),
            milestone.map_or(0, |m| m.1).into(),
            milestone.map(|m| m.0).into(),
            (&currency).into(),
            via.into(),
        ],
    )?;
    if ins.rows.is_empty() {
        return Err(err(
            "checkin.already_checked_in",
            "you have already checked in today",
        ));
    }
    sql::execute(
        "UPDATE user_state SET streak = $2, longest = greatest(longest, $2), total_days = total_days + 1, last_day = $3::date, updated_at = now() WHERE user_id = $1",
        &[uid.into(), streak.into(), (&day).into()],
    )?;
    // 奖励（ledger 幂等键按用户与日期；失败时签到仍成立，记录为待发放）
    let paid = pay(uid, &currency, daily, "checkin.daily", &day).and_then(|()| {
        pay(
            uid,
            &currency,
            milestone.map_or(0, |m| m.1),
            "checkin.milestone",
            &day,
        )
    });
    let status = match &paid {
        Ok(()) if daily + milestone.map_or(0, |m| m.1) > 0 => "paid",
        Ok(()) => "none",
        Err(e) => {
            logging::warn("check-in reward failed", &[("code", &e.code)]);
            "failed"
        }
    };
    sql::execute(
        "UPDATE records SET payout_status = $3 WHERE user_id = $1 AND day = $2::date",
        &[uid.into(), (&day).into(), status.into()],
    )?;
    let rank = sql::query(
        "SELECT count(*) FROM records WHERE day = $1::date",
        &[(&day).into()],
    )?
    .rows
    .first()
    .and_then(|r| r.int(0))
    .unwrap_or(1);
    let total = daily + milestone.map_or(0, |m| m.1);
    ctx::publish(
        "checkin.checked_in",
        &format!("user:{uid}"),
        &json!({ "user_id": uid, "date": day, "streak": streak, "reward": total, "currency": currency, "milestone_days": milestone.map(|m| m.0), "made_up": false }),
    )?;
    Ok(json!({ "date": day, "streak": streak, "rank": rank,
               "reward": { "total": total, "daily": daily, "milestone": milestone.map_or(0, |m| m.1), "milestone_days": milestone.map(|m| m.0), "currency": currency, "status": status } }))
}

fn status(uid: i64) -> R<Value> {
    let day = today()?;
    let st = sql::query(
        "SELECT streak, longest, total_days, last_day::text FROM user_state WHERE user_id = $1",
        &[uid.into()],
    )?;
    let row = st.rows.first().cloned().unwrap_or_default();
    let last = row.text(3);
    // 连续天数在断签后显示为 0（昨天之前的最后一次签到）
    let gap = sql::query(
        "SELECT ($1::date - $2::date)",
        &[(&day).into(), last.clone().into()],
    )?;
    let gap = gap.rows.first().and_then(|r| r.int(0));
    let streak = if gap.is_some_and(|g| g <= 1) {
        row.int(0).unwrap_or(0)
    } else {
        0
    };
    let month = sql::query(
        "SELECT extract(day FROM day)::bigint FROM records WHERE user_id = $1 AND date_trunc('month', day) = date_trunc('month', $2::date) ORDER BY day",
        &[uid.into(), (&day).into()],
    )?;
    let checked = last.as_deref() == Some(day.as_str());
    let (next_daily, next_milestone) = reward(&ctx::settings(), streak + 1);
    Ok(
        json!({ "date": day, "checked_in_today": checked, "streak": streak, "longest": row.int(1).unwrap_or(0),
               "total_days": row.int(2).unwrap_or(0),
               "month_days": month.rows.iter().filter_map(|r| r.int(0)).collect::<Vec<_>>(),
               "next_reward": { "daily": next_daily, "milestone": next_milestone.map(|m| json!({ "days": m.0, "amount": m.1 })) } }),
    )
}

fn leaderboard() -> R<Value> {
    let r = sql::query(
        "SELECT s.user_id, u.username, s.streak, s.total_days FROM user_state s JOIN identity.v_user_public u ON u.id = s.user_id
          WHERE s.last_day >= (now() AT TIME ZONE 'UTC')::date - 1 ORDER BY s.streak DESC, s.total_days DESC LIMIT 50",
        &[],
    )?;
    let items: Vec<Value> = r
        .rows
        .iter()
        .map(|row| json!({ "user_id": row.int(0), "username": row.text(1), "streak": row.int(2), "total_days": row.int(3) }))
        .collect();
    Ok(json!({ "items": items }))
}

impl http_handler::Guest for Checkin {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let reply = match req.route() {
            "check_in" => http::Reply::created(&check_in(ctx::current_user()?, "web")?),
            "status" => http::Reply::ok(&status(ctx::current_user()?)?),
            "leaderboard" => http::Reply::ok(&leaderboard()?),
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

impl mcp_tool::Guest for Checkin {
    fn call(tool: String, _arguments: String) -> R<String> {
        match tool.as_str() {
            "check_in" => Ok(check_in(ctx::current_user()?, "mcp")?.to_string()),
            _ => Err(err("kernel.not_found", "unknown tool")),
        }
    }
}

export!(Checkin);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewards() {
        let s = json!({ "reward_base": 10, "reward_step": 5, "reward_cap": 30, "milestones": [{ "days": 7, "amount": 100 }] });
        assert_eq!(reward(&s, 1), (10, None));
        assert_eq!(reward(&s, 3), (20, None));
        assert_eq!(reward(&s, 7), (30, Some((7, 100))));
        assert_eq!(reward(&json!({}), 1).0, 10);
    }
}
