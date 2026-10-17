//! lottery 插件（WASM，docs/plugins/lottery/）：抽奖。
//!
//! - 期次：管理员设置票价、每人限购、最少票数、抽水比例、奖级（每级中奖数与奖池占比）与停售时间。创建时生成随机
//!   seed 并公开 `seed_commit = sha256(seed)`；开奖后公开 seed。
//! - 购票：经 `ctx.ledger.transfer` 从用户回收魔力值（幂等键 `lot:t:<批次>`），票号按购买顺序连续分配；每个批次的
//!   链哈希 = sha256(上一批链哈希 ‖ 批次 ID ‖ 用户 ‖ 首票号 ‖ 张数)，最后一批即 `tickets_root`。
//! - 开奖（每分钟检查停售的期次）：`rand = sha256(seed ‖ tickets_root)`，按奖级依次以 `sha256(rand ‖ 奖级 ‖ 序号 ‖ 尝试)`
//!   抽取不重复的票号；奖池 = 销售额 × (1 − 抽水)，每级奖金 = ⌊奖池 × 占比 / 中奖数⌋，尾差回收。派奖由系统发行
//!   （幂等键 `lot:p:<期>:<票号>`），可重入。票数不足最少票数时取消并全额退款（`lot:r:<批次>`）。
//! - 站方事先知道 seed，但结果取决于最终的 tickets_root，后来的任何购买都会改变结果；任何人都可以用公开的数据复算。

pt_plugin_sdk::world!("lottery", http);

use exports::pt::plugin::{http_handler, on_cron, service_provider};
use pt_plugin_sdk::{audit, prelude::*, random, serde_json, services, sql, time};
use sha2::{Digest, Sha256};

struct Lottery;

struct Settings {
    enabled: bool,
    min_account_days: i64,
    max_per_buy: i64,
}

fn settings() -> Settings {
    let s = ctx::settings();
    Settings {
        enabled: s["enabled"].as_bool().unwrap_or(false),
        min_account_days: s["min_account_days"].as_i64().unwrap_or(7).clamp(0, 365),
        max_per_buy: s["max_tickets_per_buy"].as_i64().unwrap_or(100).clamp(1, 1000),
    }
}

fn one(q: &str, p: &[SqlValue]) -> R<i64> {
    Ok(sql::query(q, p)?.rows.first().and_then(|r| r.int(0)).unwrap_or(0))
}

fn invalid(m: &str) -> PluginError {
    err("kernel.validation_failed", m)
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).filter_map(|i| u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()).collect()
}

fn sha(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

/// 票据链：sha256(上一批 ‖ 批次 ID ‖ 用户 ‖ 首票号 ‖ 张数)，整数按 8 字节大端
pub fn chain_hash(prev: &[u8], batch: i64, user: i64, first_no: i64, count: i64) -> [u8; 32] {
    sha(&[prev, &batch.to_be_bytes(), &user.to_be_bytes(), &first_no.to_be_bytes(), &count.to_be_bytes()])
}

/// 奖级：每级中奖数与奖池占比（百分比）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tier {
    pub count: i64,
    pub pct: i64,
}

pub fn parse_tiers(v: &Value) -> Result<Vec<Tier>, &'static str> {
    let a = v.as_array().filter(|a| (1..=10).contains(&a.len())).ok_or("tiers must be a list of 1 to 10 tiers")?;
    let tiers: Vec<Tier> = a
        .iter()
        .map(|t| match (t["count"].as_i64(), t["pct"].as_i64()) {
            (Some(c), Some(p)) if (1..=1000).contains(&c) && (1..=100).contains(&p) => Ok(Tier { count: c, pct: p }),
            _ => Err("each tier needs count 1..1000 and pct 1..100"),
        })
        .collect::<Result<_, _>>()?;
    if tiers.iter().map(|t| t.pct).sum::<i64>() > 100 {
        return Err("tier percentages must add up to at most 100");
    }
    Ok(tiers)
}

/// 开奖：返回 (奖级序号, 票号)；票号 1..=total 不重复，中奖数超过票数时截断
pub fn draw_numbers(seed: &[u8], root: &[u8], total: i64, tiers: &[Tier]) -> Vec<(usize, i64)> {
    let rand = sha(&[seed, root]);
    let mut out: Vec<(usize, i64)> = Vec::new();
    let Ok(total_u) = u64::try_from(total) else { return out };
    if total_u == 0 {
        return out;
    }
    for (ti, t) in tiers.iter().enumerate() {
        for k in 0..t.count {
            if i64::try_from(out.len()).unwrap_or(i64::MAX) >= total {
                return out;
            }
            let mut attempt: u32 = 0;
            loop {
                let h = sha(&[&rand, &u32::try_from(ti).unwrap_or(0).to_be_bytes(), &k.to_be_bytes(), &attempt.to_be_bytes()]);
                let mut n8 = [0u8; 8];
                n8.copy_from_slice(&h[..8]);
                let n = i64::try_from(u64::from_be_bytes(n8) % total_u).unwrap_or(0) + 1;
                if !out.iter().any(|(_, x)| *x == n) {
                    out.push((ti, n));
                    break;
                }
                attempt += 1;
            }
        }
    }
    out
}

/// 每级单注奖金
pub fn prize_amounts(pool: i64, house_cut_pct: i64, tiers: &[Tier]) -> Vec<i64> {
    let net = i128::from(pool) * i128::from(100 - house_cut_pct) / 100;
    tiers.iter().map(|t| i64::try_from(net * i128::from(t.pct) / 100 / i128::from(t.count)).unwrap_or(0)).collect()
}

fn transfer(from: Option<i64>, to: Option<i64>, amount: i64, reason: &str, memo: &str, key: &str) -> R<()> {
    services::call(
        "ctx.ledger",
        "transfer",
        &json!({ "currency": "bonus", "from_user": from, "to_user": to, "amount": amount, "reason": reason, "memo": memo, "idempotency_key": key }).to_string(),
    )?;
    Ok(())
}

// ---------------- 查询 ----------------

const ROUND_COLS: &str = "id, title, description, ticket_price, max_per_user, min_tickets, house_cut_pct, tiers, (extract(epoch FROM opens_at) * 1000)::bigint,
    (extract(epoch FROM closes_at) * 1000)::bigint, state, encode(seed_commit, 'hex'), total_tickets, pool, cancel_reason, (extract(epoch FROM finished_at) * 1000)::bigint";

fn round_json(r: &[SqlValue]) -> Value {
    let tiers = parse_tiers(&r.json(7).unwrap_or(Value::Null)).unwrap_or_default();
    let pool = r.int(13).unwrap_or(0);
    let amounts = prize_amounts(pool, r.int(6).unwrap_or(0), &tiers);
    json!({ "id": r.int(0), "title": r.text(1), "description": r.text(2), "ticket_price": r.int(3), "max_per_user": r.int(4), "min_tickets": r.int(5),
            "house_cut_pct": r.int(6), "tiers": tiers.iter().zip(&amounts).map(|(t, a)| json!({ "count": t.count, "pct": t.pct, "amount": a })).collect::<Vec<_>>(),
            "opens_at": r.int(8), "closes_at": r.int(9), "state": r.text(10), "commit": r.text(11), "total_tickets": r.int(12), "pool": pool,
            "cancel_reason": r.text(14), "finished_at": r.int(15) })
}

fn rounds(admin: bool) -> R<Value> {
    let r = sql::query(
        &format!(
            "SELECT {ROUND_COLS} FROM rounds WHERE $1 OR state = 'open' OR finished_at > now() - interval '30 days'
              ORDER BY (state = 'open') DESC, closes_at DESC LIMIT 50"
        ),
        &[SqlValue::Boolean(admin)],
    )?;
    Ok(json!({ "items": r.rows.iter().map(|r| round_json(r)).collect::<Vec<_>>(), "enabled": settings().enabled }))
}

fn winners(rid: i64) -> R<Vec<Value>> {
    let r = sql::query(
        "SELECT p.tier, p.ticket_no, p.amount, u.username FROM prizes p LEFT JOIN identity.v_user_public u ON u.id = p.user_id WHERE p.round_id = $1 ORDER BY p.tier, p.id",
        &[SqlValue::Int(rid)],
    )?;
    Ok(r.rows.iter().map(|r| json!({ "tier": r.int(0), "ticket_no": r.int(1), "amount": r.int(2), "username": r.text(3) })).collect())
}

fn round_detail(rid: i64, uid: i64) -> R<Value> {
    let r = sql::query(&format!("SELECT {ROUND_COLS} FROM rounds WHERE id = $1"), &[SqlValue::Int(rid)])?;
    let Some(row) = r.rows.first() else { return Err(err("kernel.not_found", "round not found")) };
    let mut v = round_json(row);
    let mine = sql::query("SELECT first_no, count, cost, refunded FROM tickets WHERE round_id = $1 AND user_id = $2 ORDER BY id", &[SqlValue::Int(rid), SqlValue::Int(uid)])?;
    v["my_tickets"] = json!(mine.rows.iter().map(|r| json!({ "first_no": r.int(0), "count": r.int(1), "cost": r.int(2), "refunded": r.bool(3) })).collect::<Vec<_>>());
    v["winners"] = json!(winners(rid)?);
    v["enabled"] = json!(settings().enabled);
    Ok(v)
}

fn verify(rid: i64) -> R<Value> {
    let r = sql::query(
        "SELECT state, encode(seed_commit, 'hex'), encode(seed, 'hex'), encode(tickets_root, 'hex'), total_tickets, tiers, pool, house_cut_pct FROM rounds WHERE id = $1",
        &[SqlValue::Int(rid)],
    )?;
    let Some(row) = r.rows.first() else { return Err(err("kernel.not_found", "round not found")) };
    let drawn = row.text(0).as_deref() == Some("drawn");
    let batches = sql::query("SELECT id, user_id, first_no, count, encode(chain, 'hex') FROM tickets WHERE round_id = $1 ORDER BY id", &[SqlValue::Int(rid)])?;
    Ok(json!({
        "state": row.text(0), "commit": row.text(1), "seed": if drawn { json!(row.text(2)) } else { Value::Null },
        "tickets_root": row.text(3), "total_tickets": row.int(4), "tiers": row.json(5), "pool": row.int(6), "house_cut_pct": row.int(7),
        "batches": batches.rows.iter().map(|b| json!({ "batch_id": b.int(0), "user_id": b.int(1), "first_no": b.int(2), "count": b.int(3), "chain": b.text(4) })).collect::<Vec<_>>(),
        "winners": winners(rid)?,
        "algorithm": "commit = sha256(seed); chain_i = sha256(chain_{i-1} || be64(batch_id) || be64(user_id) || be64(first_no) || be64(count)), chain_0 = empty; \
                      rand = sha256(seed || tickets_root); for tier t (be32), winner k (be64), attempt a (be32): n = be64(sha256(rand || t || k || a)[0..8]) mod total + 1, skipping numbers already drawn",
    }))
}

fn me(uid: i64) -> R<Value> {
    let t = sql::query(
        "SELECT t.round_id, r.title, sum(t.count)::bigint, sum(t.cost)::bigint, r.state FROM tickets t JOIN rounds r ON r.id = t.round_id
          WHERE t.user_id = $1 GROUP BY t.round_id, r.title, r.state ORDER BY t.round_id DESC LIMIT 50",
        &[SqlValue::Int(uid)],
    )?;
    let p = sql::query(
        "SELECT p.round_id, r.title, p.tier, p.ticket_no, p.amount, p.paid FROM prizes p JOIN rounds r ON r.id = p.round_id WHERE p.user_id = $1 ORDER BY p.id DESC LIMIT 50",
        &[SqlValue::Int(uid)],
    )?;
    Ok(json!({
        "tickets": t.rows.iter().map(|r| json!({ "round_id": r.int(0), "title": r.text(1), "count": r.int(2), "cost": r.int(3), "state": r.text(4) })).collect::<Vec<_>>(),
        "prizes": p.rows.iter().map(|r| json!({ "round_id": r.int(0), "title": r.text(1), "tier": r.int(2), "ticket_no": r.int(3), "amount": r.int(4), "paid": r.bool(5) })).collect::<Vec<_>>(),
    }))
}

// ---------------- 购票 ----------------

fn buy(uid: i64, rid: i64, count: i64) -> R<Value> {
    let s = settings();
    if !s.enabled {
        return Err(err("lottery.disabled", "the lottery is not open on this site"));
    }
    if !(1..=s.max_per_buy).contains(&count) {
        return Err(invalid(&format!("you can buy 1 to {} tickets at a time", s.max_per_buy)));
    }
    let age = one("SELECT (extract(epoch FROM now() - created_at) / 86400)::bigint FROM identity.v_user_public WHERE id = $1", &[SqlValue::Int(uid)])?;
    if age < s.min_account_days {
        return Err(err("lottery.not_eligible", &format!("accounts younger than {} days cannot buy tickets", s.min_account_days)));
    }
    let r = sql::query(
        "SELECT ticket_price, max_per_user, total_tickets, encode(tickets_root, 'hex'), state = 'open' AND now() >= opens_at AND now() < closes_at, title
           FROM rounds WHERE id = $1 FOR UPDATE",
        &[SqlValue::Int(rid)],
    )?;
    let Some(row) = r.rows.first() else { return Err(err("kernel.not_found", "round not found")) };
    if !row.bool(4).unwrap_or(false) {
        return Err(err("lottery.closed", "this round is not on sale"));
    }
    let (price, max, total) = (row.int(0).unwrap_or(0), row.int(1).unwrap_or(0), row.int(2).unwrap_or(0));
    if max > 0 {
        let have = one("SELECT coalesce(sum(count), 0)::bigint FROM tickets WHERE round_id = $1 AND user_id = $2", &[SqlValue::Int(rid), SqlValue::Int(uid)])?;
        if have + count > max {
            return Err(err("lottery.limit_reached", &format!("at most {max} tickets per user in this round")));
        }
    }
    let cost = price.checked_mul(count).ok_or_else(|| invalid("cost overflow"))?;
    let first_no = total + 1;
    let batch = one("SELECT nextval(pg_get_serial_sequence('tickets', 'id'))", &[])?;
    let chain = chain_hash(&unhex(&row.text(3).unwrap_or_default()), batch, uid, first_no, count);
    transfer(Some(uid), None, cost, "lottery.ticket", &format!("round {rid} #{first_no}+{count}"), &format!("lot:t:{batch}"))
        .map_err(|e| if e.code.contains("insufficient") { err("lottery.insufficient_funds", "not enough bonus") } else { e })?;
    sql::execute(
        "INSERT INTO tickets (id, round_id, user_id, first_no, count, cost, chain) VALUES ($1, $2, $3, $4, $5, $6, decode($7, 'hex'))",
        &[SqlValue::Int(batch), SqlValue::Int(rid), SqlValue::Int(uid), SqlValue::Int(first_no), SqlValue::Int(count), SqlValue::Int(cost), SqlValue::Text(hex(&chain))],
    )?;
    sql::execute(
        "UPDATE rounds SET total_tickets = total_tickets + $2, pool = pool + $3, tickets_root = decode($4, 'hex') WHERE id = $1",
        &[SqlValue::Int(rid), SqlValue::Int(count), SqlValue::Int(cost), SqlValue::Text(hex(&chain))],
    )?;
    ctx::publish(
        "lottery.ticket_bought",
        &format!("user:{uid}"),
        &json!({ "round_id": rid, "user_id": uid, "tickets": count, "cost": cost, "first_no": first_no, "currency": "bonus" }),
    )?;
    Ok(json!({ "batch_id": batch, "first_no": first_no, "count": count, "cost": cost }))
}

// ---------------- 开奖与取消 ----------------

fn cancel(rid: i64, reason: &str) -> R<i64> {
    let batches = sql::query("SELECT id, user_id, cost FROM tickets WHERE round_id = $1 AND NOT refunded ORDER BY id", &[SqlValue::Int(rid)])?;
    for b in &batches.rows {
        let (bid, uid, cost) = (b.int(0).unwrap_or(0), b.int(1).unwrap_or(0), b.int(2).unwrap_or(0));
        transfer(None, Some(uid), cost, "lottery.refund", &format!("round {rid} cancelled"), &format!("lot:r:{bid}"))?;
        sql::execute("UPDATE tickets SET refunded = true WHERE id = $1", &[SqlValue::Int(bid)])?;
    }
    sql::execute(
        "UPDATE rounds SET state = 'cancelled', cancel_reason = $2, finished_at = now() WHERE id = $1",
        &[SqlValue::Int(rid), SqlValue::Text(reason.chars().take(300).collect())],
    )?;
    let n = i64::try_from(batches.rows.len()).unwrap_or(0);
    ctx::publish("lottery.round_cancelled", "global", &json!({ "round_id": rid, "reason": reason, "refunded_batches": n }))?;
    Ok(n)
}

fn draw(rid: i64) -> R<Value> {
    let r = sql::query(
        "SELECT encode(seed, 'hex'), encode(tickets_root, 'hex'), total_tickets, tiers, pool, house_cut_pct, min_tickets, title, state FROM rounds WHERE id = $1 FOR UPDATE",
        &[SqlValue::Int(rid)],
    )?;
    let Some(row) = r.rows.first() else { return Err(err("kernel.not_found", "round not found")) };
    if row.text(8).as_deref() != Some("open") {
        return Err(err("lottery.closed", "this round is already finished"));
    }
    let total = row.int(2).unwrap_or(0);
    if total < row.int(6).unwrap_or(1) {
        let n = cancel(rid, "not_enough_tickets")?;
        return Ok(json!({ "state": "cancelled", "refunded_batches": n }));
    }
    let tiers = parse_tiers(&row.json(3).unwrap_or(Value::Null)).map_err(invalid)?;
    let amounts = prize_amounts(row.int(4).unwrap_or(0), row.int(5).unwrap_or(0), &tiers);
    let title = row.text(7).unwrap_or_default();
    let nums = draw_numbers(&unhex(&row.text(0).unwrap_or_default()), &unhex(&row.text(1).unwrap_or_default()), total, &tiers);
    let mut out = Vec::new();
    for (tier, n) in nums {
        let uid = one(
            "SELECT user_id FROM tickets WHERE round_id = $1 AND first_no <= $2 AND first_no + count > $2",
            &[SqlValue::Int(rid), SqlValue::Int(n)],
        )?;
        let amount = amounts.get(tier).copied().unwrap_or(0);
        let tier_no = i64::try_from(tier).unwrap_or(0) + 1;
        sql::execute(
            "INSERT INTO prizes (round_id, tier, ticket_no, user_id, amount) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (round_id, ticket_no) DO NOTHING",
            &[SqlValue::Int(rid), SqlValue::Int(tier_no), SqlValue::Int(n), SqlValue::Int(uid), SqlValue::Int(amount)],
        )?;
        if amount > 0 {
            transfer(None, Some(uid), amount, "lottery.prize", &format!("round {rid} tier {tier_no} #{n}"), &format!("lot:p:{rid}:{n}"))?;
        }
        sql::execute("UPDATE prizes SET paid = true WHERE round_id = $1 AND ticket_no = $2", &[SqlValue::Int(rid), SqlValue::Int(n)])?;
        let _ = services::call(
            "ctx.notify",
            "send",
            &json!({ "user_id": uid, "template_key": "lottery.won", "category": "economy",
                     "title": format!("你在「{title}」中奖了 / You won in \"{title}\""),
                     "body": format!("第 {tier_no} 等奖，票号 {n}，奖金 {amount} 魔力值\nTier {tier_no}, ticket {n}, prize {amount} bonus"),
                     "link": format!("/p/lottery/{rid}"), "args": { "round_id": rid, "tier": tier_no, "amount": amount },
                     "idempotency_key": format!("lottery:won:{rid}:{n}") })
            .to_string(),
        );
        out.push(json!({ "user_id": uid, "tier": tier_no, "ticket_no": n, "prize": amount }));
    }
    sql::execute("UPDATE rounds SET state = 'drawn', finished_at = now() WHERE id = $1", &[SqlValue::Int(rid)])?;
    let truncated = out.len() > 100;
    ctx::publish("lottery.round_drawn", "global", &json!({ "round_id": rid, "winners": out.iter().take(100).collect::<Vec<_>>(), "winners_truncated": truncated }))?;
    Ok(json!({ "state": "drawn", "winners": out }))
}

fn tick() -> R<()> {
    let due = sql::query("SELECT id FROM rounds WHERE state = 'open' AND closes_at <= now() ORDER BY closes_at LIMIT 20", &[])?;
    // 每期开奖后提交；失败时返回错误，本期未提交的写入回滚，下一分钟重试（派奖幂等）
    for rid in due.rows.iter().filter_map(|r| r.int(0)) {
        if let Err(e) = draw(rid) {
            logging::warn("lottery draw failed", &[("round", &rid.to_string()), ("code", &e.code), ("message", &e.message)]);
            return Err(e);
        }
        sql::commit()?;
    }
    Ok(())
}

// ---------------- 管理 ----------------

fn create_round(actor: i64, b: &Value) -> R<Value> {
    let title = b["title"].as_str().unwrap_or("").trim().to_owned();
    if title.is_empty() || title.chars().count() > 100 {
        return Err(invalid("title is required (≤ 100 characters)"));
    }
    let price = b["ticket_price"].as_i64().filter(|p| *p > 0).ok_or_else(|| invalid("ticket_price must be positive"))?;
    let tiers = parse_tiers(&b["tiers"]).map_err(invalid)?;
    let house = b["house_cut_pct"].as_i64().unwrap_or(10);
    if !(0..=50).contains(&house) {
        return Err(invalid("house_cut_pct must be 0..50"));
    }
    let now = time::now();
    let closes = b["closes_at"].as_i64().ok_or_else(|| invalid("closes_at is required"))?;
    if closes <= now + 60_000 || closes > now + 90 * 86_400_000 {
        return Err(invalid("closes_at must be between 1 minute and 90 days from now"));
    }
    let seed = random::bytes(32);
    let commit = sha(&[&seed]);
    let tiers_json = json!(tiers.iter().map(|t| json!({ "count": t.count, "pct": t.pct })).collect::<Vec<_>>());
    let id = one(
        "INSERT INTO rounds (title, description, ticket_price, max_per_user, min_tickets, house_cut_pct, tiers, closes_at, seed, seed_commit, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, to_timestamp($8::bigint / 1000.0), decode($9, 'hex'), decode($10, 'hex'), $11) RETURNING id",
        &[
            SqlValue::Text(title),
            SqlValue::Text(b["description"].as_str().unwrap_or("").chars().take(1000).collect()),
            SqlValue::Int(price),
            SqlValue::Int(b["max_per_user"].as_i64().unwrap_or(0).max(0)),
            SqlValue::Int(b["min_tickets"].as_i64().unwrap_or(1).max(1)),
            SqlValue::Int(house),
            SqlValue::Json(tiers_json.to_string()),
            SqlValue::Int(closes),
            SqlValue::Text(hex(&seed)),
            SqlValue::Text(hex(&commit)),
            SqlValue::Int(actor),
        ],
    )?;
    audit::write(&audit::AuditEntry { action: "lottery.round_create".into(), target: format!("lottery_round:{id}"), details: b.to_string() })?;
    Ok(json!({ "id": id, "commit": hex(&commit) }))
}

// ---------------- 导出 ----------------

impl on_cron::Guest for Lottery {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            "tick" => tick(),
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

impl service_provider::Guest for Lottery {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        let out = match method.as_str() {
            "rounds" => rounds(false)?,
            "buy" => {
                ctx::require_scope("buy")?;
                let uid = a["user_id"].as_i64().or_else(|| a["user"].as_i64()).ok_or_else(|| invalid("user_id is required"))?;
                buy(uid, a["round_id"].as_i64().ok_or_else(|| invalid("round_id is required"))?, a["tickets"].as_i64().unwrap_or(1))?
            }
            other => return Err(err("kernel.not_found", &format!("ctx.lottery has no method `{other}`"))),
        };
        Ok(out.to_string())
    }
}

impl http_handler::Guest for Lottery {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let uid = ctx::current_user()?;
        let reply = match req.route() {
            "rounds" => http::Reply::ok(&rounds(false)?),
            "round" => http::Reply::ok(&round_detail(req.param_i64("id")?, uid)?),
            "buy" => http::Reply::created(&buy(uid, req.param_i64("id")?, req.body_value()["count"].as_i64().unwrap_or(1))?),
            "verify" => http::Reply::ok(&verify(req.param_i64("id")?)?),
            "me" => http::Reply::ok(&me(uid)?),
            "admin_rounds" => http::Reply::ok(&rounds(true)?),
            "admin_create" => http::Reply::created(&create_round(uid, &req.body_value())?),
            "admin_cancel" => {
                let rid = req.param_i64("id")?;
                let reason = req.body_value()["reason"].as_str().unwrap_or("").trim().to_owned();
                if reason.is_empty() {
                    return Err(invalid("reason is required"));
                }
                let st = sql::query("SELECT state FROM rounds WHERE id = $1 FOR UPDATE", &[SqlValue::Int(rid)])?;
                match st.rows.first().and_then(|r| r.text(0)).as_deref() {
                    None => return Err(err("kernel.not_found", "round not found")),
                    Some("open") => {}
                    Some(_) => return Err(err("lottery.closed", "this round is already finished")),
                }
                let n = cancel(rid, &reason)?;
                audit::write(&audit::AuditEntry { action: "lottery.round_cancel".into(), target: format!("lottery_round:{rid}"), details: json!({ "reason": reason }).to_string() })?;
                http::Reply::ok(&json!({ "refunded_batches": n }))
            }
            "admin_draw" => {
                // 提前停售并立即开奖
                let rid = req.param_i64("id")?;
                sql::execute("UPDATE rounds SET closes_at = least(closes_at, now()) WHERE id = $1 AND state = 'open'", &[SqlValue::Int(rid)])?;
                let v = draw(rid)?;
                audit::write(&audit::AuditEntry { action: "lottery.round_draw".into(), target: format!("lottery_round:{rid}"), details: "{}".into() })?;
                http::Reply::ok(&v)
            }
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
    fn tiers_and_amounts() {
        let t = parse_tiers(&json!([{ "count": 1, "pct": 50 }, { "count": 3, "pct": 30 }])).unwrap();
        assert_eq!(prize_amounts(1000, 10, &t), vec![450, 90]);
        assert!(parse_tiers(&json!([{ "count": 1, "pct": 80 }, { "count": 1, "pct": 30 }])).is_err());
        assert!(parse_tiers(&json!([])).is_err());
    }

    #[test]
    fn draw_is_deterministic_and_distinct() {
        let t = vec![Tier { count: 1, pct: 50 }, Tier { count: 3, pct: 30 }];
        let a = draw_numbers(b"seed", b"root", 10, &t);
        assert_eq!(a, draw_numbers(b"seed", b"root", 10, &t));
        assert_eq!(a.len(), 4);
        let mut ns: Vec<i64> = a.iter().map(|x| x.1).collect();
        ns.sort_unstable();
        ns.dedup();
        assert_eq!(ns.len(), 4);
        assert!(ns.iter().all(|n| (1..=10).contains(n)));
        assert_ne!(a, draw_numbers(b"seed", b"root2", 10, &t));
        // 中奖数超过票数时截断
        assert_eq!(draw_numbers(b"s", b"r", 2, &t).len(), 2);
    }

    #[test]
    fn hex_roundtrip() {
        let h = chain_hash(&[], 1, 2, 1, 5);
        assert_eq!(unhex(&hex(&h)), h.to_vec());
    }
}
