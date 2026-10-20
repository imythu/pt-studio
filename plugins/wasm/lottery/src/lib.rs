//! lottery 插件（WASM，docs/plugins/lottery/）：幸运转盘。
//!
//! - 奖品（管理员维护）：魔力值（`ctx.ledger` 系统发行）、上传量（`ctx.accounting.adjust`）、VIP 天数与永久 VIP
//!   （`ctx.levels.grant_vip`）。每个奖品有中奖概率（百万分之一）与可选库存；启用奖品的概率之和 ≤ 100%，剩余为“谢谢参与”。
//! - 抽奖：开关、每次消耗的魔力值、每日次数（UTC 日）、注册天数门槛在插件设置里。扣费（幂等键 `lot:s:<抽奖 ID>`）→
//!   用宿主随机数抽取 → 发奖（幂等键 `lot:w:<抽奖 ID>`）。发奖失败时退回本次消耗（`lot:rf:<抽奖 ID>`）并报错，
//!   本次抽奖不留记录、不扣库存。
//! - 服务调用各自提交：若进程在扣费后、写入记录前中断，用户损失一次消耗（概率极低，可由管理员按账本记录补偿）。

pt_plugin_sdk::world!("lottery", http);

use exports::pt::plugin::{http_handler, service_provider};
use pt_plugin_sdk::{audit, prelude::*, random, serde_json, sql};

struct Lottery;

pub const KINDS: &[&str] = &["bonus", "upload", "vip_days", "vip_permanent"];
pub const PPM: i64 = 1_000_000;
const MAX_PRIZES: usize = 16;

struct Settings {
    enabled: bool,
    spin_cost: i64,
    daily_limit: i64,
    min_account_days: i64,
}

fn settings() -> Settings {
    let s = ctx::settings();
    Settings {
        enabled: s["enabled"].as_bool().unwrap_or(true),
        spin_cost: s["spin_cost"].as_i64().unwrap_or(50).clamp(0, 1_000_000),
        daily_limit: s["daily_limit"].as_i64().unwrap_or(10).clamp(0, 1000),
        min_account_days: s["min_account_days"].as_i64().unwrap_or(7).clamp(0, 365),
    }
}

fn one(q: &str, p: &[SqlValue]) -> R<i64> {
    Ok(sql::query(q, p)?.rows.first().and_then(|r| r.int(0)).unwrap_or(0))
}

fn invalid(m: &str) -> PluginError {
    err("kernel.validation_failed", m)
}

/// 奖品数量的取值范围（按类型）
pub fn check_amount(kind: &str, amount: i64) -> Result<(), &'static str> {
    let ok = match kind {
        "bonus" => (1..=1_000_000_000).contains(&amount),
        "upload" => (1..=(1_i64 << 50)).contains(&amount),
        "vip_days" => (1..=3650).contains(&amount),
        "vip_permanent" => true,
        _ => return Err("unknown prize kind"),
    };
    if ok {
        Ok(())
    } else {
        Err(match kind {
            "bonus" => "bonus amount must be 1..1000000000",
            "upload" => "upload amount must be 1 byte .. 1 PiB",
            _ => "VIP days must be 1..3650",
        })
    }
}

/// 抽取：roll ∈ [0, 1 000 000)，按顺序累加概率，落在哪个区间就是哪个奖品；超出总和为未中奖
pub fn pick(chances: &[(i64, i64)], roll: i64) -> Option<i64> {
    let mut acc = 0;
    for (id, ppm) in chances {
        acc += ppm;
        if roll < acc {
            return Some(*id);
        }
    }
    None
}

fn roll() -> i64 {
    let b = random::bytes(8);
    let mut n8 = [0u8; 8];
    n8.copy_from_slice(&b[..8]);
    // 2^64 对 10^6 取模的偏差约 5e-14，可忽略
    i64::try_from(u64::from_be_bytes(n8) % 1_000_000).unwrap_or(0)
}

fn transfer(from: Option<i64>, to: Option<i64>, amount: i64, reason: &str, memo: &str, key: &str) -> R<()> {
    ctx::call(
        "ctx.ledger",
        "transfer",
        &json!({ "currency": "bonus", "from_user": from, "to_user": to, "amount": amount, "reason": reason, "memo": memo, "idempotency_key": key }),
    )?;
    Ok(())
}

// ---------------- 查询 ----------------

const PRIZE_COLS: &str = "id, kind, amount, label, chance_ppm, stock, sort, active";

fn prize_json(r: &[SqlValue]) -> Value {
    json!({ "id": r.int(0), "kind": r.text(1), "amount": r.int(2), "label": r.text(3), "chance_ppm": r.int(4),
            "stock": r.int(5), "sort": r.int(6), "active": r.bool(7) })
}

/// 转盘上的奖品：启用、未删除且有库存
fn live_prizes(lock: bool) -> R<Vec<Vec<SqlValue>>> {
    Ok(sql::query(
        &format!(
            "SELECT {PRIZE_COLS} FROM wheel_prizes WHERE active AND deleted_at IS NULL AND (stock IS NULL OR stock > 0)
              ORDER BY sort, id{}",
            if lock { " FOR UPDATE" } else { "" }
        ),
        &[],
    )?
    .rows)
}

fn spins_today(uid: i64) -> R<i64> {
    one(
        "SELECT count(*) FROM spins WHERE user_id = $1 AND created_at >= (now() AT TIME ZONE 'UTC')::date AT TIME ZONE 'UTC'",
        &[SqlValue::Int(uid)],
    )
}

fn account_age_days(uid: i64) -> R<i64> {
    one("SELECT (extract(epoch FROM now() - created_at) / 86400)::bigint FROM identity.v_user_public WHERE id = $1", &[SqlValue::Int(uid)])
}

fn recent_wins() -> R<Vec<Value>> {
    let r = sql::query(
        "SELECT s.kind, s.amount, u.username, (extract(epoch FROM s.created_at) * 1000)::bigint
           FROM spins s LEFT JOIN identity.v_user_public u ON u.id = s.user_id
          WHERE s.prize_id IS NOT NULL ORDER BY s.id DESC LIMIT 20",
        &[],
    )?;
    Ok(r.rows.iter().map(|r| json!({ "kind": r.text(0), "amount": r.int(1), "username": r.text(2), "at": r.int(3) })).collect())
}

fn my_spins(uid: i64, limit: i64) -> R<Vec<Value>> {
    let r = sql::query(
        "SELECT id, cost, prize_id, kind, amount, (extract(epoch FROM created_at) * 1000)::bigint FROM spins WHERE user_id = $1 ORDER BY id DESC LIMIT $2",
        &[SqlValue::Int(uid), SqlValue::Int(limit)],
    )?;
    Ok(r.rows
        .iter()
        .map(|r| json!({ "id": r.int(0), "cost": r.int(1), "prize_id": r.int(2), "kind": r.text(3), "amount": r.int(4), "at": r.int(5) }))
        .collect())
}

fn wheel(uid: Option<i64>) -> R<Value> {
    let s = settings();
    let prizes = live_prizes(false)?;
    let total: i64 = prizes.iter().map(|p| p.int(4).unwrap_or(0)).sum();
    let mut v = json!({
        "enabled": s.enabled, "spin_cost": s.spin_cost, "daily_limit": s.daily_limit, "min_account_days": s.min_account_days,
        "prizes": prizes.iter().map(|p| { let mut j = prize_json(p); j.as_object_mut().map(|o| { o.remove("stock"); o.remove("active"); o.remove("sort") }); j }).collect::<Vec<_>>(),
        "none_chance_ppm": (PPM - total).max(0),
        "recent_wins": recent_wins()?,
    });
    if let Some(uid) = uid {
        v["spins_today"] = json!(spins_today(uid)?);
        v["eligible"] = json!(account_age_days(uid)? >= s.min_account_days);
        v["mine"] = json!(my_spins(uid, 10)?);
    }
    Ok(v)
}

// ---------------- 抽奖 ----------------

fn grant(uid: i64, kind: &str, amount: i64, sid: i64) -> R<()> {
    let key = format!("lot:w:{sid}");
    let memo = format!("wheel spin {sid}");
    match kind {
        "bonus" => transfer(None, Some(uid), amount, "lottery.prize", &memo, &key),
        "upload" => ctx::call(
            "ctx.accounting",
            "adjust",
            &json!({ "user": uid, "d_up": amount, "d_down": 0, "reason": "lottery.prize", "idempotency_key": key }),
        )
        .map(drop),
        "vip_days" => ctx::call("ctx.levels", "grant_vip", &json!({ "user": uid, "days": amount, "reason": memo, "source_ref": key })).map(drop),
        "vip_permanent" => ctx::call("ctx.levels", "grant_vip", &json!({ "user": uid, "permanent": true, "reason": memo, "source_ref": key })).map(drop),
        _ => Err(err("lottery.unavailable", "unknown prize kind")),
    }
}

fn spin(uid: i64) -> R<Value> {
    let s = settings();
    if !s.enabled {
        return Err(err("lottery.disabled", "the prize wheel is not open on this site"));
    }
    if account_age_days(uid)? < s.min_account_days {
        return Err(err("lottery.not_eligible", &format!("accounts younger than {} days cannot spin", s.min_account_days)));
    }
    // 同一用户的抽奖串行化（每日次数检查与写入一致）
    sql::execute(
        "INSERT INTO spin_locks (user_id) VALUES ($1) ON CONFLICT (user_id) DO UPDATE SET user_id = EXCLUDED.user_id",
        &[SqlValue::Int(uid)],
    )?;
    if s.daily_limit > 0 && spins_today(uid)? >= s.daily_limit {
        return Err(err("lottery.limit_reached", &format!("at most {} spins per day", s.daily_limit)));
    }
    let prizes = live_prizes(true)?;
    let sid = one("SELECT nextval(pg_get_serial_sequence('spins', 'id'))", &[])?;
    if s.spin_cost > 0 {
        transfer(Some(uid), None, s.spin_cost, "lottery.spin", &format!("wheel spin {sid}"), &format!("lot:s:{sid}"))
            .map_err(|e| if e.code.contains("insufficient") { err("lottery.insufficient_funds", "not enough bonus") } else { e })?;
    }
    let chances: Vec<(i64, i64)> = prizes.iter().map(|p| (p.int(0).unwrap_or(0), p.int(4).unwrap_or(0))).collect();
    let won = pick(&chances, roll()).and_then(|id| prizes.iter().find(|p| p.int(0) == Some(id)));
    let (kind, amount) = won.map_or((None, 0), |p| (p.text(1), p.int(2).unwrap_or(0)));
    if let Some(kind) = &kind
        && let Err(e) = grant(uid, kind, amount, sid)
    {
        logging::warn("lottery grant failed", &[("spin", &sid.to_string()), ("code", &e.code), ("message", &e.message)]);
        if s.spin_cost > 0 {
            transfer(None, Some(uid), s.spin_cost, "lottery.refund", &format!("wheel spin {sid} failed"), &format!("lot:rf:{sid}"))?;
        }
        return Err(err("lottery.grant_failed", "the prize could not be granted; your bonus was refunded"));
    }
    let prize_id = won.and_then(|p| p.int(0));
    if let Some(p) = won
        && p.int(5).is_some()
    {
        sql::execute("UPDATE wheel_prizes SET stock = stock - 1, updated_at = now() WHERE id = $1", &[SqlValue::Int(prize_id.unwrap_or(0))])?;
    }
    sql::execute(
        "INSERT INTO spins (id, user_id, cost, prize_id, kind, amount) VALUES ($1, $2, $3, $4, $5, $6)",
        &[
            SqlValue::Int(sid),
            SqlValue::Int(uid),
            SqlValue::Int(s.spin_cost),
            prize_id.map_or(SqlValue::Null, SqlValue::Int),
            kind.clone().map_or(SqlValue::Null, SqlValue::Text),
            SqlValue::Int(amount),
        ],
    )?;
    ctx::publish(
        "lottery.spun",
        &format!("user:{uid}"),
        &json!({ "spin_id": sid, "user_id": uid, "cost": s.spin_cost, "currency": "bonus", "prize_id": prize_id, "kind": kind, "amount": amount }),
    )?;
    Ok(json!({ "spin_id": sid, "cost": s.spin_cost, "prize_id": prize_id, "kind": kind, "amount": amount, "spins_today": spins_today(uid)? }))
}

// ---------------- 管理 ----------------

fn admin_wheel() -> R<Value> {
    let r = sql::query(&format!("SELECT {PRIZE_COLS} FROM wheel_prizes WHERE deleted_at IS NULL ORDER BY sort, id"), &[])?;
    let st = sql::query(
        "SELECT count(*), coalesce(sum(cost), 0)::bigint, count(*) FILTER (WHERE prize_id IS NOT NULL), count(DISTINCT user_id)
           FROM spins WHERE created_at >= (now() AT TIME ZONE 'UTC')::date AT TIME ZONE 'UTC'",
        &[],
    )?;
    let by_kind = sql::query(
        "SELECT kind, count(*), coalesce(sum(amount), 0)::bigint FROM spins WHERE prize_id IS NOT NULL AND created_at > now() - interval '30 days' GROUP BY kind",
        &[],
    )?;
    let t = st.rows.first();
    Ok(json!({
        "prizes": r.rows.iter().map(|r| prize_json(r)).collect::<Vec<_>>(),
        "today": { "spins": t.and_then(|t| t.int(0)), "cost": t.and_then(|t| t.int(1)), "wins": t.and_then(|t| t.int(2)), "users": t.and_then(|t| t.int(3)) },
        "last_30_days": by_kind.rows.iter().map(|r| json!({ "kind": r.text(0), "count": r.int(1), "amount": r.int(2) })).collect::<Vec<_>>(),
    }))
}

/// 整表保存：带 id 的更新，不带 id 的新建，未出现的标记删除（历史记录仍引用它们）
fn save_prizes(b: &Value) -> R<Value> {
    let list = b["prizes"].as_array().ok_or_else(|| invalid("prizes must be a list"))?;
    let mut total = 0;
    let mut active = 0;
    for p in list {
        let kind = p["kind"].as_str().unwrap_or("");
        if !KINDS.contains(&kind) {
            return Err(invalid("unknown prize kind"));
        }
        check_amount(kind, p["amount"].as_i64().unwrap_or(0)).map_err(invalid)?;
        let ppm = p["chance_ppm"].as_i64().filter(|c| (0..=PPM).contains(c)).ok_or_else(|| invalid("chance_ppm must be 0..1000000"))?;
        if p["stock"].as_i64().is_some_and(|s| !(0..=i64::from(i32::MAX)).contains(&s)) {
            return Err(invalid("stock must be ≥ 0"));
        }
        if p["label"].as_str().is_some_and(|l| l.chars().count() > 40) {
            return Err(invalid("label must be at most 40 characters"));
        }
        if p["active"].as_bool().unwrap_or(true) {
            total += ppm;
            active += 1;
        }
    }
    if total > PPM {
        return Err(invalid("the chances of active prizes add up to more than 100%"));
    }
    if active > MAX_PRIZES {
        return Err(invalid(&format!("at most {MAX_PRIZES} active prizes")));
    }
    let mut keep: Vec<i64> = Vec::new();
    for (i, p) in list.iter().enumerate() {
        let kind = p["kind"].as_str().unwrap_or("");
        let amount = if kind == "vip_permanent" { 0 } else { p["amount"].as_i64().unwrap_or(0) };
        let params = [
            SqlValue::Text(kind.into()),
            SqlValue::Int(amount),
            SqlValue::Text(p["label"].as_str().unwrap_or("").trim().into()),
            SqlValue::Int(p["chance_ppm"].as_i64().unwrap_or(0)),
            p["stock"].as_i64().map_or(SqlValue::Null, SqlValue::Int),
            SqlValue::Int(i64::try_from(i).unwrap_or(0) * 10 + 10),
            SqlValue::Boolean(p["active"].as_bool().unwrap_or(true)),
        ];
        let id = match p["id"].as_i64() {
            Some(id) => {
                let mut ps = vec![SqlValue::Int(id)];
                ps.extend(params);
                let n = sql::execute(
                    "UPDATE wheel_prizes SET kind = $2, amount = $3, label = $4, chance_ppm = $5, stock = $6::int, sort = $7, active = $8, updated_at = now()
                      WHERE id = $1 AND deleted_at IS NULL",
                    &ps,
                )?;
                if n == 0 {
                    return Err(err("kernel.not_found", &format!("prize {id} not found")));
                }
                id
            }
            None => one(
                "INSERT INTO wheel_prizes (kind, amount, label, chance_ppm, stock, sort, active) VALUES ($1, $2, $3, $4, $5::int, $6, $7) RETURNING id",
                &params,
            )?,
        };
        keep.push(id);
    }
    let ids = keep.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    sql::execute(
        "UPDATE wheel_prizes SET deleted_at = now(), active = false WHERE deleted_at IS NULL AND NOT (id = ANY (string_to_array($1, ',')::bigint[]))",
        &[SqlValue::Text(ids)],
    )?;
    audit::write(&audit::AuditEntry { action: "lottery.prizes_update".into(), target: "lottery_wheel".into(), details: b.to_string() })?;
    admin_wheel()
}

// ---------------- 导出 ----------------

impl service_provider::Guest for Lottery {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        let out = match method.as_str() {
            "wheel" => wheel(a["user_id"].as_i64().or_else(|| a["user"].as_i64()))?,
            other => return Err(err("kernel.not_found", &format!("ctx.lottery has no method `{other}`"))),
        };
        Ok(out.to_string())
    }
}

impl http_handler::Guest for Lottery {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let uid = ctx::current_user()?;
        let reply = match req.route() {
            "wheel" => http::Reply::ok(&wheel(Some(uid))?),
            "spin" => http::Reply::created(&spin(uid)?),
            "me" => http::Reply::ok(&json!({ "items": my_spins(uid, 50)? })),
            "admin_wheel" => http::Reply::ok(&admin_wheel()?),
            "admin_prizes" => http::Reply::ok(&save_prizes(&req.body_value())?),
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(Lottery);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_follows_cumulative_chances() {
        let c = [(1, 100_000), (2, 50_000), (3, 0), (4, 1)];
        assert_eq!(pick(&c, 0), Some(1));
        assert_eq!(pick(&c, 99_999), Some(1));
        assert_eq!(pick(&c, 100_000), Some(2));
        assert_eq!(pick(&c, 149_999), Some(2));
        assert_eq!(pick(&c, 150_000), Some(4));
        assert_eq!(pick(&c, 150_001), None);
        assert_eq!(pick(&[], 0), None);
    }

    #[test]
    fn amounts_by_kind() {
        assert!(check_amount("bonus", 100).is_ok());
        assert!(check_amount("bonus", 0).is_err());
        assert!(check_amount("upload", 1 << 30).is_ok());
        assert!(check_amount("upload", (1 << 50) + 1).is_err());
        assert!(check_amount("vip_days", 3650).is_ok());
        assert!(check_amount("vip_days", 3651).is_err());
        assert!(check_amount("vip_permanent", 0).is_ok());
        assert!(check_amount("medal", 1).is_err());
    }
}
