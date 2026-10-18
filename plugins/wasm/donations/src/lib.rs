//! donations 插件（WASM，docs/plugins/donations/）：捐赠。
//!
//! - 站点不接入支付：用户按说明页付款并告知管理组，管理员核实后登记一笔捐赠。
//! - 登记即发放奖励：VIP 天数（`ctx.levels.grant_vip`，在已有 VIP 之后顺延）、魔力值（`ctx.ledger.transfer`，原因
//!   `donations.reward`）、勋章（`ctx.medals.grant`）。幂等键由捐赠 ID 派生；某项奖励失败时捐赠仍然记录，
//!   返回失败项供管理员手动补发（服务调用各自提交，不能整体回滚）。
//! - 说明页：Markdown 说明、本月进度、公开捐赠名单（只列出同意公开的用户名，不显示金额）、本人捐赠记录与 VIP 到期时间。
//! - 每日任务：VIP 到期前 N 天提醒捐赠过的用户（每个到期时间只提醒一次）。

pt_plugin_sdk::world!("donations", http);

use exports::pt::plugin::{http_handler, on_cron};
use pt_plugin_sdk::{audit, prelude::*, services, sql, time};

struct Donations;

fn one(q: &str, p: &[SqlValue]) -> R<Option<i64>> {
    Ok(sql::query(q, p)?.rows.first().and_then(|r| r.int(0)))
}

fn notify(uid: i64, template: &str, title: &str, key: &str) {
    let _ = services::call(
        "ctx.notify",
        "send",
        &json!({ "user_id": uid, "template_key": template, "category": "account", "title": title, "body": "",
                 "link": "/p/donations/", "args": {}, "idempotency_key": key })
        .to_string(),
    );
}

fn vip_until(uid: i64) -> Option<i64> {
    ctx::call("ctx.levels", "vip_until", &json!({ "user": uid })).ok().and_then(|v| v["until_ms"].as_i64())
}

/// 未指定时按设置的每元比例计算默认奖励（amount 以分计）
pub fn default_reward(amount_cents: i64, per_unit: f64) -> i64 {
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let v = (amount_cents as f64 / 100.0 * per_unit).floor() as i64;
    v.max(0)
}

fn summary(uid: i64) -> R<Value> {
    let s = ctx::settings();
    let instructions = s["instructions"].as_str().unwrap_or("");
    let html = pt_markup::render(instructions, pt_markup::Markup::Markdown, &pt_markup::RenderOptions::default()).html;
    let month_total = one(
        "SELECT coalesce(sum(amount), 0)::bigint FROM donations WHERE revoked_at IS NULL AND created_at >= date_trunc('month', now())",
        &[],
    )?
    .unwrap_or(0);
    let donors = if s["show_donors"].as_bool().unwrap_or(true) {
        let r = sql::query(
            "SELECT u.username FROM donations d JOIN identity.v_user_public u ON u.id = d.user_id AND u.status <> 'deleted'
              WHERE d.public AND d.revoked_at IS NULL AND d.created_at > now() - interval '1 year'
              GROUP BY u.username ORDER BY max(d.created_at) DESC LIMIT 100",
            &[],
        )?;
        r.rows.iter().filter_map(|row| row.text(0)).collect::<Vec<_>>()
    } else {
        vec![]
    };
    let mine = sql::query(
        "SELECT id, amount, vip_days, bonus, revoked_at IS NOT NULL, (extract(epoch FROM created_at) * 1000)::bigint
           FROM donations WHERE user_id = $1 ORDER BY id DESC LIMIT 50",
        &[SqlValue::Int(uid)],
    )?;
    Ok(json!({
        "enabled": s["enabled"].as_bool().unwrap_or(false),
        "instructions_html": html,
        "currency_symbol": s["currency_symbol"].as_str().unwrap_or("¥"),
        "monthly_goal": s["monthly_goal"].as_i64().unwrap_or(0) * 100,
        "month_total": month_total,
        "donors": donors,
        "mine": mine.rows.iter().map(|r| json!({ "id": r.int(0), "amount": r.int(1), "vip_days": r.int(2), "bonus": r.int(3),
            "revoked": r.bool(4), "created_at": r.int(5) })).collect::<Vec<_>>(),
        "vip_until": vip_until(uid),
    }))
}

fn admin_list() -> R<Value> {
    let r = sql::query(
        "SELECT d.id, d.user_id, u.username, d.amount, d.method, d.note, d.public, d.vip_days, d.bonus, d.medal_id,
                (extract(epoch FROM d.vip_until) * 1000)::bigint, c.username, (extract(epoch FROM d.revoked_at) * 1000)::bigint,
                (extract(epoch FROM d.created_at) * 1000)::bigint
           FROM donations d LEFT JOIN identity.v_user_public u ON u.id = d.user_id LEFT JOIN identity.v_user_public c ON c.id = d.created_by
          ORDER BY d.id DESC LIMIT 200",
        &[],
    )?;
    let totals = sql::query(
        "SELECT coalesce(sum(amount) FILTER (WHERE created_at >= date_trunc('month', now())), 0)::bigint, coalesce(sum(amount), 0)::bigint
           FROM donations WHERE revoked_at IS NULL",
        &[],
    )?;
    let t = totals.rows.first();
    Ok(json!({
        "items": r.rows.iter().map(|row| json!({ "id": row.int(0), "user_id": row.int(1), "username": row.text(2), "amount": row.int(3),
            "method": row.text(4), "note": row.text(5), "public": row.bool(6), "vip_days": row.int(7), "bonus": row.int(8),
            "medal_id": row.int(9), "vip_until": row.int(10), "created_by": row.text(11), "revoked_at": row.int(12),
            "created_at": row.int(13) })).collect::<Vec<_>>(),
        "month_total": t.and_then(|r| r.int(0)).unwrap_or(0),
        "total": t.and_then(|r| r.int(1)).unwrap_or(0),
    }))
}

fn resolve_user(b: &Value) -> R<i64> {
    let found = match (b["user_id"].as_i64(), b["username"].as_str()) {
        (Some(id), _) => one(
            "SELECT id FROM identity.v_user_public WHERE id = $1 AND status <> 'deleted'",
            &[SqlValue::Int(id)],
        )?,
        (None, Some(name)) => one(
            "SELECT id FROM identity.v_user_public WHERE lower(username) = lower($1) AND status <> 'deleted'",
            &[SqlValue::Text(name.trim().into())],
        )?,
        _ => None,
    };
    found.ok_or_else(|| err("donations.user_not_found", "user not found"))
}

fn record(staff: i64, b: &Value) -> R<Value> {
    let s = ctx::settings();
    let uid = resolve_user(b)?;
    let amount = b["amount"]
        .as_i64()
        .filter(|a| (1..=1_000_000_000).contains(a))
        .ok_or_else(|| err("kernel.validation_failed", "amount (in cents) must be positive"))?;
    let vip_days = b["vip_days"]
        .as_i64()
        .unwrap_or_else(|| default_reward(amount, s["vip_days_per_unit"].as_f64().unwrap_or(0.0)));
    let bonus = b["bonus"]
        .as_i64()
        .unwrap_or_else(|| default_reward(amount, s["bonus_per_unit"].as_f64().unwrap_or(0.0)));
    if !(0..=3650).contains(&vip_days) || !(0..=100_000_000).contains(&bonus) {
        return Err(err("kernel.validation_failed", "vip_days must be 0..3650 and bonus 0..100000000"));
    }
    let medal = b["medal_id"].as_i64().filter(|m| *m > 0);
    let id = one(
        "INSERT INTO donations (user_id, amount, method, note, public, vip_days, bonus, medal_id, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id",
        &[
            SqlValue::Int(uid),
            SqlValue::Int(amount),
            SqlValue::Text(b["method"].as_str().unwrap_or("").chars().take(50).collect()),
            SqlValue::Text(b["note"].as_str().unwrap_or("").chars().take(500).collect()),
            SqlValue::Boolean(b["public"].as_bool().unwrap_or(false)),
            SqlValue::Int(vip_days),
            SqlValue::Int(bonus),
            medal.map_or(SqlValue::Null, SqlValue::Int),
            SqlValue::Int(staff),
        ],
    )?
    .unwrap_or(0);
    // 发放奖励：逐项执行，失败项记录下来（已成功的服务调用不会回滚，捐赠记录必须保留）
    let mut failed = Vec::new();
    let mut warn = |what: &str, e: &PluginError| {
        logging::warn("donation reward failed", &[("donation", &id.to_string()), ("reward", what), ("code", &e.code)]);
        failed.push(json!({ "reward": what, "code": e.code }));
    };
    let mut until = None;
    if vip_days > 0 {
        match ctx::call(
            "ctx.levels",
            "grant_vip",
            &json!({ "user": uid, "days": vip_days, "reason": format!("donation {id}"), "source_ref": format!("donations:vip:{id}") }),
        ) {
            Ok(v) => until = v["until_ms"].as_i64(),
            Err(e) => warn("vip", &e),
        }
    }
    if bonus > 0
        && let Err(e) = ctx::call(
            "ctx.ledger",
            "transfer",
            &json!({ "currency": "bonus", "from_user": null, "to_user": uid, "amount": bonus, "reason": "donations.reward",
                     "memo": format!("donation {id}"), "idempotency_key": format!("donations:bonus:{id}") }),
        )
    {
        warn("bonus", &e);
    }
    if let Some(m) = medal
        && let Err(e) = ctx::call(
            "ctx.medals",
            "grant",
            &json!({ "user": uid, "medal_id": m, "reason": "donation", "idempotency_key": format!("donations:medal:{id}") }),
        )
    {
        warn("medal", &e);
    }
    sql::execute(
        "UPDATE donations SET vip_until = to_timestamp($2::float8 / 1000) WHERE id = $1",
        &[SqlValue::Int(id), until.map_or(SqlValue::Null, SqlValue::Int)],
    )?;
    notify(uid, "donations.thanks", "感谢你的捐赠，奖励已发放 / Thank you for your donation", &format!("donations:thanks:{id}"));
    ctx::publish(
        "donation.recorded",
        &format!("user:{uid}"),
        &json!({ "donation_id": id, "user_id": uid, "vip_days": vip_days, "bonus": bonus }),
    )?;
    audit::write(&audit::AuditEntry {
        action: "donations.record".into(),
        target: format!("user:{uid}"),
        details: json!({ "donation_id": id, "amount": amount, "vip_days": vip_days, "bonus": bonus, "medal_id": medal, "failed": failed }).to_string(),
    })?;
    Ok(json!({ "id": id, "user_id": uid, "vip_days": vip_days, "bonus": bonus, "vip_until": until, "failed": failed }))
}

fn revoke(id: i64, b: &Value) -> R<()> {
    let r = sql::execute("UPDATE donations SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL", &[SqlValue::Int(id)])?;
    if r == 0 {
        return Err(err("kernel.not_found", "donation not found or already revoked"));
    }
    audit::write(&audit::AuditEntry {
        action: "donations.revoke".into(),
        target: format!("donation:{id}"),
        details: json!({ "reason": b["reason"] }).to_string(),
    })
}

/// VIP 到期提醒：以 ctx.levels 的实际到期时间为准（可能已被商城等顺延）
fn remind() -> R<()> {
    let days = ctx::settings()["remind_days"].as_i64().unwrap_or(3);
    if days <= 0 {
        return Ok(());
    }
    let r = sql::query(
        "SELECT DISTINCT user_id FROM donations WHERE revoked_at IS NULL AND vip_days > 0 AND vip_until > now() - interval '1 year'",
        &[],
    )?;
    let now = time::now();
    for uid in r.rows.iter().filter_map(|row| row.int(0)) {
        let Some(until) = vip_until(uid) else { continue };
        if until > now + days * 86_400_000 {
            continue;
        }
        let fresh = sql::query(
            "INSERT INTO reminders (user_id, until) VALUES ($1, to_timestamp($2::float8 / 1000)) ON CONFLICT DO NOTHING RETURNING 1",
            &[SqlValue::Int(uid), SqlValue::Int(until)],
        )?;
        if !fresh.rows.is_empty() {
            notify(uid, "donations.vip_expiring", "你的 VIP 即将到期 / Your VIP is expiring soon", &format!("donations:remind:{uid}:{until}"));
        }
        sql::commit()?;
    }
    Ok(())
}

impl on_cron::Guest for Donations {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            "remind" => remind(),
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

impl http_handler::Guest for Donations {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let uid = ctx::current_user()?;
        let reply = match req.route() {
            "summary" => http::Reply::ok(&summary(uid)?),
            "admin_list" => http::Reply::ok(&admin_list()?),
            "admin_create" => http::Reply::created(&record(uid, &req.body_value())?),
            "admin_revoke" => {
                revoke(req.param_i64("id")?, &req.body_value())?;
                http::Reply::no_content()
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(Donations);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rewards() {
        assert_eq!(default_reward(5_000, 1.0), 50);
        assert_eq!(default_reward(9_999, 0.5), 49);
        assert_eq!(default_reward(10_000, 0.0), 0);
    }
}
