//! bonus 插件（WASM，docs/plugins/bonus/）——P3 试点子集：
//! - 初始化：经 `ctx.ledger` 注册 `bonus`、`seed_points` 两个币种，写入默认行为奖励规则；
//! - 行为奖励：`torrent.created` 且初始状态为 approved 时视为“审核通过”（torrent 状态机接入前的简化），按规则发放；
//! - `ctx.bonus.award`：其他插件发放魔力值（作用域 `award`，幂等）；
//! - HTTP：`GET /v1/users/me/summary` 返回本插件币种余额。
//!
//! 小时结算（规则表达式、做种快照）、回放重算、回收在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

wit_bindgen::generate!({
    path: "wit",
    world: "bonus",
    generate_all,
});

use exports::pt::plugin::{http_handler, lifecycle, on_event, service_provider};
use pt::plugin::{
    events, invocation, log, services,
    sql::{self, SqlValue},
    types::PluginError,
};
use serde_json::{Value, json};

struct Bonus;

type R<T> = Result<T, PluginError>;

fn err(code: &str, message: &str) -> PluginError {
    PluginError {
        code: code.into(),
        message: message.into(),
        details: None,
        retryable: false,
    }
}

fn call(service: &str, method: &str, args: &Value) -> R<Value> {
    let out = services::call(service, method, &args.to_string())?;
    serde_json::from_str(&out).map_err(|_| err("bonus.bad_service_reply", "invalid JSON from service"))
}

fn ledger(method: &str, args: &Value) -> R<Value> {
    call("ctx.ledger", method, args)
}

fn int(v: &SqlValue) -> i64 {
    match v {
        SqlValue::Int(i) => *i,
        _ => 0,
    }
}

fn text(v: &SqlValue) -> String {
    match v {
        SqlValue::Text(s) => s.clone(),
        _ => String::new(),
    }
}

const CURRENCIES: &[(&str, u8, &str, bool, bool)] = &[
    // (code, scale, name_term, transferable, external_award)
    ("bonus", 1, "bonus.currency", true, true),
    ("seed_points", 1, "bonus.seed_points", false, false),
];

impl lifecycle::Guest for Bonus {
    fn init() -> R<()> {
        for (code, scale, term, transferable, external) in CURRENCIES {
            ledger(
                "register_currency",
                &json!({ "code": code, "scale": scale, "name_term": term,
                                                 "options": { "transferable": transferable } }),
            )?;
            sql::execute(
                "INSERT INTO currencies (code, scale, name_term, source, external_award) VALUES ($1, $2, $3, 'builtin', $4)
                 ON CONFLICT (code) DO NOTHING",
                &[SqlValue::Text((*code).into()), SqlValue::Int(i64::from(*scale)), SqlValue::Text((*term).into()),
                  SqlValue::Boolean(*external)],
            )?;
        }
        // 默认行为奖励：种子审核通过奖励发布者 10.0 魔力值（scale 1 → 100）
        sql::execute(
            "INSERT INTO action_rules (code, trigger, currency, amount, daily_count_limit) VALUES ('torrent_approved', 'torrent_approved', 'bonus', 100, 20)
             ON CONFLICT (code) DO NOTHING",
            &[],
        )?;
        log::emit(log::Level::Info, "bonus initialised", &[]);
        Ok(())
    }
}

/// 行为奖励（03 §1.5）
fn on_action(event_id: &str, trigger: &str, user: i64, reference: &str) -> R<()> {
    let rules = sql::query(
        "SELECT id, code, currency, amount, daily_count_limit, daily_amount_limit FROM action_rules WHERE trigger = $1 AND is_enabled ORDER BY id",
        &[SqlValue::Text(trigger.into())],
    )?;
    for r in rules.rows {
        let (rule_id, code, currency, amount, count_limit, amount_limit) = (
            int(&r[0]),
            text(&r[1]),
            text(&r[2]),
            int(&r[3]),
            int(&r[4]),
            int(&r[5]),
        );
        let capped = sql::query(
            "INSERT INTO action_daily (user_id, rule_id, day, count, amount) VALUES ($1, $2, current_date, 1, $3)
             ON CONFLICT (user_id, rule_id, day) DO UPDATE SET count = action_daily.count + 1, amount = action_daily.amount + EXCLUDED.amount
               WHERE ($4 = 0 OR action_daily.count + 1 <= $4) AND ($5 = 0 OR action_daily.amount + EXCLUDED.amount <= $5)
             RETURNING 1",
            &[SqlValue::Int(user), SqlValue::Int(rule_id), SqlValue::Int(amount), SqlValue::Int(count_limit), SqlValue::Int(amount_limit)],
        )?;
        if capped.rows.is_empty() {
            continue;
        }
        let inserted = sql::query(
            "INSERT INTO action_awards (rule_id, user_id, ref, event_id, currency, amount, state) VALUES ($1, $2, $3, $4, $5, $6, 'pending')
             ON CONFLICT (rule_id, ref) DO NOTHING RETURNING id",
            &[SqlValue::Int(rule_id), SqlValue::Int(user), SqlValue::Text(reference.into()), SqlValue::Uuid(event_id.into()),
              SqlValue::Text(currency.clone()), SqlValue::Int(amount)],
        )?;
        let Some(row) = inserted.rows.first() else {
            // 同一对象已奖励过：撤回本次 daily 计数
            sql::execute(
                "UPDATE action_daily SET count = count - 1, amount = amount - $3 WHERE user_id = $1 AND rule_id = $2 AND day = current_date",
                &[SqlValue::Int(user), SqlValue::Int(rule_id), SqlValue::Int(amount)],
            )?;
            continue;
        };
        let award_id = int(&row[0]);
        let res = ledger(
            "transfer",
            &json!({ "currency": currency, "from_user": null, "to_user": user, "amount": amount,
                                              "reason": format!("bonus.action_{code}"), "memo": "",
                                              "idempotency_key": format!("act:{event_id}:{rule_id}") }),
        );
        match res {
            Ok(v) => {
                sql::execute(
                    "UPDATE action_awards SET state = 'awarded', entry_id = $2::uuid, updated_at = now() WHERE id = $1",
                    &[
                        SqlValue::Int(award_id),
                        SqlValue::Text(v["entry_id"].as_str().unwrap_or_default().into()),
                    ],
                )?;
                events::publish(&events::NewEvent {
                    event_type: "bonus.awarded".into(),
                    version: 1,
                    key: format!("user:{user}"),
                    payload: json!({ "award_id": award_id, "user_id": user, "currency": currency, "amount": amount,
                                     "reason": format!("bonus.action_{code}"), "source": "action", "ref": reference }).to_string(),
                })?;
            }
            // 可重试错误：整个事件工作单元回滚，内核重投时以相同幂等键重试
            Err(e) if e.retryable => return Err(e),
            Err(e) => {
                sql::execute(
                    "UPDATE action_awards SET state = 'failed', error_code = $2, updated_at = now() WHERE id = $1",
                    &[SqlValue::Int(award_id), SqlValue::Text(e.code.clone())],
                )?;
            }
        }
    }
    Ok(())
}

impl on_event::Guest for Bonus {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        let p: Value =
            serde_json::from_str(&evt.payload).map_err(|_| err("bonus.bad_event", "invalid payload"))?;
        if evt.event_type == "torrent.created"
            && p["status"] == "approved"
            && let (Some(uid), Some(tid)) = (p["uploader_id"].as_i64(), p["torrent_id"].as_i64())
        {
            on_action(&evt.id, "torrent_approved", uid, &format!("torrent:{tid}"))?;
        }
        Ok(())
    }
}

fn json_response(status: u16, v: &Value) -> http_handler::Response {
    http_handler::Response {
        status,
        headers: vec![http_handler::Header {
            name: "content-type".into(),
            value: "application/json".into(),
        }],
        body: v.to_string().into_bytes(),
    }
}

impl http_handler::Guest for Bonus {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        match req.route_id.as_str() {
            "my_summary" => {
                let me = invocation::current()
                    .actor
                    .user_id
                    .ok_or_else(|| err("kernel.unauthenticated", "login required"))?;
                let ours = sql::query(
                    "SELECT code, scale, name_term FROM currencies WHERE is_active ORDER BY code",
                    &[],
                )?;
                let balances = ledger("balances", &json!({ "user": me }))?;
                let items: Vec<Value> = ours
                    .rows
                    .iter()
                    .map(|r| {
                        let code = text(&r[0]);
                        let b = balances
                            .as_array()
                            .and_then(|a| a.iter().find(|x| x["currency"] == code.as_str()));
                        json!({ "currency": code, "scale": int(&r[1]), "name_term": text(&r[2]),
                                "balance": b.map_or(json!(0), |x| x["balance"].clone()),
                                "available": b.map_or(json!(0), |x| x["available"].clone()) })
                    })
                    .collect();
                Ok(json_response(200, &json!({ "balances": items })))
            }
            _ => Err(err("kernel.not_found", "no such route")),
        }
    }
}

/// `ctx.bonus.award`（03 §1.10）
fn award(args: &Value) -> R<Value> {
    let inv = invocation::current();
    let caller = inv.caller_plugin.clone();
    if !inv.caller_scopes.iter().any(|s| s == "award") {
        return Err(err("bonus.scope_required", "scope `award` required"));
    }
    let user = args["user"]
        .as_i64()
        .ok_or_else(|| err("kernel.validation_failed", "user is required"))?;
    let currency = args["currency"].as_str().unwrap_or("bonus").to_owned();
    let amount = args["amount"].as_i64().unwrap_or(0);
    let reason = args["reason"].as_str().unwrap_or_default().to_owned();
    let memo = args["memo"].as_str().unwrap_or_default();
    let key = args["idempotency_key"].as_str().unwrap_or_default().to_owned();
    if !(1..=10_000_000).contains(&amount) {
        return Err(err("bonus.award_amount_invalid", "amount out of range"));
    }
    if !reason.starts_with(&format!("{caller}.")) || key.is_empty() || key.len() > 128 {
        return Err(err(
            "kernel.validation_failed",
            "reason must be prefixed with the caller id; idempotency_key is required",
        ));
    }
    let awardable = sql::query(
        "SELECT external_award FROM currencies WHERE code = $1",
        &[SqlValue::Text(currency.clone())],
    )?;
    if !matches!(
        awardable.rows.first().map(|r| &r[0]),
        Some(SqlValue::Boolean(true))
    ) {
        return Err(err(
            "bonus.currency_not_awardable",
            "this currency cannot be awarded by other plugins",
        ));
    }
    let hash = format!("{user}:{currency}:{amount}:{reason}");
    let ins = sql::query(
        "INSERT INTO service_awards (caller_plugin, idempotency_key, request_hash, user_id, currency, amount, reason)
         VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT DO NOTHING RETURNING 1",
        &[SqlValue::Text(caller.clone()), SqlValue::Text(key.clone()), SqlValue::Text(hash.clone()), SqlValue::Int(user),
          SqlValue::Text(currency.clone()), SqlValue::Int(amount), SqlValue::Text(reason.clone())],
    )?;
    if ins.rows.is_empty() {
        let hit = sql::query(
            "SELECT request_hash, entry_id::text FROM service_awards WHERE caller_plugin = $1 AND idempotency_key = $2",
            &[SqlValue::Text(caller.clone()), SqlValue::Text(key.clone())],
        )?;
        if let Some(r) = hit.rows.first() {
            if text(&r[0]) != hash {
                return Err(err(
                    "kernel.idempotency_key_reused",
                    "idempotency key was used for a different request",
                ));
            }
            if let SqlValue::Text(e) = &r[1] {
                return Ok(json!({ "entry_id": e, "duplicate": true }));
            }
        }
    }
    let memo: String = format!("{reason} {memo}").chars().take(200).collect();
    let res = ledger(
        "transfer",
        &json!({ "currency": currency, "from_user": null, "to_user": user, "amount": amount,
                                          "reason": "bonus.award", "memo": memo.trim(),
                                          "idempotency_key": format!("svc:{caller}:{key}") }),
    )?;
    let entry = res["entry_id"].as_str().unwrap_or_default().to_owned();
    sql::execute(
        "UPDATE service_awards SET entry_id = $3::uuid WHERE caller_plugin = $1 AND idempotency_key = $2",
        &[
            SqlValue::Text(caller),
            SqlValue::Text(key),
            SqlValue::Text(entry.clone()),
        ],
    )?;
    events::publish(&events::NewEvent {
        event_type: "bonus.awarded".into(),
        version: 1,
        key: format!("user:{user}"),
        payload: json!({ "user_id": user, "currency": currency, "amount": amount, "reason": reason, "source": "service" }).to_string(),
    })?;
    Ok(json!({ "entry_id": entry, "duplicate": res["duplicate"].as_bool().unwrap_or(false) }))
}

impl service_provider::Guest for Bonus {
    fn call(service: String, method: String, args: String) -> R<String> {
        let a: Value =
            serde_json::from_str(&args).map_err(|_| err("kernel.bad_request", "invalid JSON arguments"))?;
        let out = match (service.as_str(), method.as_str()) {
            ("ctx.bonus", "award") => award(&a)?,
            _ => return Err(err("kernel.not_found", "unknown service method")),
        };
        Ok(out.to_string())
    }
}

export!(Bonus);
