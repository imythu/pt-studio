//! shop 插件（WASM，docs/plugins/shop/）：魔力值商城。
//!
//! - 商品类型：上传量（`ctx.accounting.adjust`）、邀请名额（`ctx.invites.grant_quota`）、促销卡（`ctx.promotions.grant_cards`）、
//!   消除 H&R（`ctx.hnr.forgive`）。依赖都是可选的，缺失时该类型购买失败并退回魔力值。
//! - 购买：锁商品行（库存与限购一致）→ 写订单取得 ID → `ledger.hold` 冻结 → 履约 → `capture_hold` 扣款；
//!   履约失败则 `release_hold` 解冻并报错。各步骤的幂等键由订单 ID 派生。服务调用各自提交，
//!   若进程在履约后、扣款前中断，冻结到期自动解冻（用户得到商品而未扣款，偏向用户）。

pt_plugin_sdk::world!("shop", http);

use exports::pt::plugin::http_handler;
use pt_plugin_sdk::{audit, prelude::*, sql, time};

struct Shop;

pub const KINDS: &[&str] = &["upload_credit", "invite", "promo_card", "hnr_removal"];

fn one(q: &str, p: &[SqlValue]) -> R<i64> {
    Ok(sql::query(q, p)?.rows.first().and_then(|r| r.int(0)).unwrap_or(0))
}

/// 校验商品参数（管理员保存时）
pub fn check_params(kind: &str, p: &Value) -> Result<(), &'static str> {
    match kind {
        "upload_credit" => match p["bytes"].as_i64() {
            Some(b) if (1..=(100_i64 << 40)).contains(&b) => Ok(()),
            _ => Err("params.bytes must be between 1 byte and 100 TiB"),
        },
        "invite" => match (p["count"].as_i64(), p["expires_days"].as_i64()) {
            (Some(c), d) if (1..=100).contains(&c) && d.is_none_or(|d| (1..=3650).contains(&d)) => Ok(()),
            _ => Err("params.count must be 1..100 and params.expires_days 1..3650 when set"),
        },
        "promo_card" => match (p["card_kind"].as_str(), p["count"].as_i64()) {
            (Some("free" | "double_up"), Some(c)) if (1..=100).contains(&c) => Ok(()),
            _ => Err("params.card_kind must be free or double_up and params.count 1..100"),
        },
        "hnr_removal" => Ok(()),
        _ => Err("unknown item kind"),
    }
}

fn period_sql(p: &str) -> &'static str {
    match p {
        "day" => "now() - interval '1 day'",
        "week" => "now() - interval '7 days'",
        "month" => "now() - interval '30 days'",
        _ => "'-infinity'::timestamptz",
    }
}

fn item_json(row: &impl pt_plugin_sdk::RowExt) -> Value {
    json!({ "id": row.int(0), "kind": row.text(1), "title": row.text(2), "description": row.text(3), "price": row.int(4),
            "params": row.json(5).unwrap_or(json!({})), "stock": row.int(6), "per_user_limit": row.int(7),
            "limit_period": row.text(8), "active": row.bool(9), "sort": row.int(10) })
}

const ITEM_COLS: &str = "id, kind, title, description, price, params, stock, per_user_limit, limit_period, active, sort";

fn bought(uid: i64, item: i64, period: &str) -> R<i64> {
    one(
        &format!("SELECT coalesce(sum(quantity), 0)::bigint FROM orders WHERE user_id = $1 AND item_id = $2 AND state = 'completed' AND created_at > {}", period_sql(period)),
        &[SqlValue::Int(uid), SqlValue::Int(item)],
    )
}

fn catalog(uid: i64) -> R<Value> {
    let r = sql::query(&format!("SELECT {ITEM_COLS} FROM items WHERE active ORDER BY sort, id"), &[])?;
    let mut items = Vec::new();
    for row in &r.rows {
        let mut v = item_json(row);
        v["bought"] = json!(bought(uid, row.int(0).unwrap_or(0), &row.text(8).unwrap_or_default())?);
        items.push(v);
    }
    Ok(json!({ "items": items }))
}

fn fulfill(kind: &str, p: &Value, uid: i64, qty: i64, target: &Value, oid: i64) -> R<()> {
    let key = format!("shop:fulfill:{oid}");
    match kind {
        "upload_credit" => {
            ctx::call(
                "ctx.accounting",
                "adjust",
                &json!({ "user": uid, "d_up": p["bytes"].as_i64().unwrap_or(0) * qty, "d_down": 0, "reason": "shop.upload_credit", "idempotency_key": key }),
            )?;
        }
        "invite" => {
            let expires = p["expires_days"].as_i64().map(|d| time::now() + d * 86_400_000);
            ctx::call(
                "ctx.invites",
                "grant_quota",
                &json!({ "user": uid, "amount": p["count"].as_i64().unwrap_or(1) * qty, "expires_at": expires, "reason": "shop", "idempotency_key": key }),
            )?;
        }
        "promo_card" => {
            ctx::call(
                "ctx.promotions",
                "grant_cards",
                &json!({ "user": uid, "card_kind": p["card_kind"], "n": p["count"].as_i64().unwrap_or(1) * qty, "reason": format!("shop order {oid}") }),
            )?;
        }
        "hnr_removal" => {
            ctx::call(
                "ctx.hnr",
                "forgive",
                &json!({ "record_id": target["record_id"], "reason": format!("shop order {oid}"), "idempotency_key": key }),
            )?;
        }
        _ => return Err(err("shop.unavailable", "unknown item kind")),
    }
    Ok(())
}

/// 购买前校验目标（消除 H&R：记录属于本人且进行中 / 已失败）
fn check_target(kind: &str, uid: i64, b: &Value) -> R<Value> {
    if kind != "hnr_removal" {
        return Ok(json!({}));
    }
    let rid = b["record_id"].as_i64().ok_or_else(|| err("shop.invalid_target", "choose the H&R record to clear"))?;
    for state in ["active", "failed"] {
        let r = ctx::call("ctx.hnr", "records", &json!({ "user": uid, "filter": { "state": state } }))?;
        if r["items"].as_array().is_some_and(|a| a.iter().any(|x| x["id"].as_i64() == Some(rid))) {
            return Ok(json!({ "record_id": rid }));
        }
    }
    Err(err("shop.invalid_target", "this H&R record cannot be cleared"))
}

fn purchase(uid: i64, item_id: i64, b: &Value) -> R<Value> {
    let r = sql::query(&format!("SELECT {ITEM_COLS} FROM items WHERE id = $1 FOR UPDATE"), &[SqlValue::Int(item_id)])?;
    let Some(row) = r.rows.first() else { return Err(err("kernel.not_found", "item not found")) };
    if !row.bool(9).unwrap_or(false) {
        return Err(err("shop.unavailable", "this item is not on sale"));
    }
    let kind = row.text(1).unwrap_or_default();
    let title = row.text(2).unwrap_or_default();
    let price = row.int(4).unwrap_or(0);
    let params = row.json(5).unwrap_or(json!({}));
    let stock = row.int(6);
    let (limit, period) = (row.int(7).unwrap_or(0), row.text(8).unwrap_or_default());
    let qty = if kind == "hnr_removal" { 1 } else { b["quantity"].as_i64().unwrap_or(1) };
    if !(1..=10).contains(&qty) {
        return Err(err("kernel.validation_failed", "quantity must be between 1 and 10"));
    }
    if stock.is_some_and(|s| s < qty) {
        return Err(err("shop.sold_out", "not enough stock"));
    }
    if limit > 0 && bought(uid, item_id, &period)? + qty > limit {
        return Err(err("shop.limit_reached", "you have reached the purchase limit for this item"));
    }
    let target = check_target(&kind, uid, b)?;
    let total = price.checked_mul(qty).ok_or_else(|| err("kernel.validation_failed", "price overflow"))?;
    // 订单 ID 来自序列：即使本事务回滚也不会复用，用作幂等键
    let oid = one(
        "INSERT INTO orders (user_id, item_id, kind, item_title, quantity, total, target) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
        &[
            SqlValue::Int(uid),
            SqlValue::Int(item_id),
            SqlValue::Text(kind.clone()),
            SqlValue::Text(title.clone()),
            SqlValue::Int(qty),
            SqlValue::Int(total),
            SqlValue::Json(target.to_string()),
        ],
    )?;
    let hold = ctx::call(
        "ctx.ledger",
        "hold",
        &json!({ "currency": "bonus", "user": uid, "amount": total, "reason": "shop.purchase",
                 "expires_at": time::now() + 15 * 60_000, "idempotency_key": format!("shop:hold:{oid}") }),
    )
    .map_err(|e| if e.code.contains("insufficient") { err("shop.insufficient_funds", "not enough bonus") } else { e })?;
    let hold_id = hold.as_str().or_else(|| hold["hold_id"].as_str()).unwrap_or_default().to_owned();
    if let Err(e) = fulfill(&kind, &params, uid, qty, &target, oid) {
        let _ = ctx::call("ctx.ledger", "release_hold", &json!({ "hold_id": hold_id, "idempotency_key": format!("shop:release:{oid}") }));
        logging::warn("shop fulfillment failed", &[("order", &oid.to_string()), ("code", &e.code), ("message", &e.message)]);
        return Err(err("shop.fulfillment_failed", "the item could not be delivered; your bonus was returned"));
    }
    ctx::call(
        "ctx.ledger",
        "capture_hold",
        &json!({ "hold_id": hold_id, "to_user": null, "amount": total, "reason": "shop.purchase", "memo": format!("order {oid}: {title} x{qty}"),
                 "idempotency_key": format!("shop:capture:{oid}") }),
    )?;
    if stock.is_some() {
        sql::execute("UPDATE items SET stock = stock - $2 WHERE id = $1", &[SqlValue::Int(item_id), SqlValue::Int(qty)])?;
    }
    ctx::publish("shop.item_purchased", &format!("user:{uid}"), &json!({ "order_id": oid, "user_id": uid, "item_id": item_id, "kind": kind, "quantity": qty, "total": total }))?;
    Ok(json!({ "order_id": oid, "total": total }))
}

fn orders(uid: Option<i64>) -> R<Value> {
    let r = sql::query(
        "SELECT o.id, o.item_title, o.quantity, o.total, o.state, (extract(epoch FROM o.created_at) * 1000)::bigint, u.username
           FROM orders o LEFT JOIN identity.v_user_public u ON u.id = o.user_id
          WHERE $1::bigint IS NULL OR o.user_id = $1 ORDER BY o.id DESC LIMIT 100",
        &[uid.map_or(SqlValue::Null, SqlValue::Int)],
    )?;
    Ok(json!({ "items": r.rows.iter().map(|row| json!({ "id": row.int(0), "item_title": row.text(1), "quantity": row.int(2), "total": row.int(3),
        "state": row.text(4), "created_at": row.int(5), "username": row.text(6) })).collect::<Vec<_>>() }))
}

fn save_item(id: Option<i64>, b: &Value) -> R<Value> {
    let old = match id {
        Some(id) => Some(
            sql::query(&format!("SELECT {ITEM_COLS} FROM items WHERE id = $1"), &[SqlValue::Int(id)])?
                .rows
                .first()
                .map(item_json)
                .ok_or_else(|| err("kernel.not_found", "item not found"))?,
        ),
        None => None,
    };
    let pick = |k: &str| if b.get(k).is_some() { b[k].clone() } else { old.as_ref().map_or(Value::Null, |o| o[k].clone()) };
    let kind = old.as_ref().map_or_else(|| b["kind"].as_str().unwrap_or("").to_owned(), |o| o["kind"].as_str().unwrap_or("").to_owned());
    if !KINDS.contains(&kind.as_str()) {
        return Err(err("kernel.validation_failed", "unknown item kind"));
    }
    let title = pick("title").as_str().unwrap_or("").trim().to_owned();
    if title.is_empty() || title.chars().count() > 100 {
        return Err(err("kernel.validation_failed", "title is required (≤ 100 characters)"));
    }
    let price = pick("price").as_i64().filter(|p| *p > 0).ok_or_else(|| err("kernel.validation_failed", "price must be positive"))?;
    let params = match pick("params") {
        Value::Null => json!({}),
        v => v,
    };
    check_params(&kind, &params).map_err(|m| err("kernel.validation_failed", m))?;
    let period = pick("limit_period").as_str().unwrap_or("none").to_owned();
    if !matches!(period.as_str(), "none" | "day" | "week" | "month") {
        return Err(err("kernel.validation_failed", "limit_period must be none, day, week or month"));
    }
    let vals = [
        SqlValue::Text(title),
        SqlValue::Text(pick("description").as_str().unwrap_or("").chars().take(500).collect()),
        SqlValue::Int(price),
        SqlValue::Json(params.to_string()),
        pick("stock").as_i64().map_or(SqlValue::Null, |s| SqlValue::Int(s.max(0))),
        SqlValue::Int(pick("per_user_limit").as_i64().unwrap_or(0).max(0)),
        SqlValue::Text(period),
        SqlValue::Boolean(pick("active").as_bool().unwrap_or(false)),
        SqlValue::Int(pick("sort").as_i64().unwrap_or(0)),
    ];
    let new_id = match id {
        Some(id) => {
            let mut p = vec![SqlValue::Int(id)];
            p.extend(vals);
            sql::execute(
                "UPDATE items SET title = $2, description = $3, price = $4, params = $5, stock = $6, per_user_limit = $7, limit_period = $8,
                        active = $9, sort = $10, updated_at = now() WHERE id = $1",
                &p,
            )?;
            id
        }
        None => {
            let mut p = vec![SqlValue::Text(kind.clone())];
            p.extend(vals);
            one(
                "INSERT INTO items (kind, title, description, price, params, stock, per_user_limit, limit_period, active, sort)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING id",
                &p,
            )?
        }
    };
    audit::write(&audit::AuditEntry {
        action: if id.is_some() { "shop.item_update" } else { "shop.item_create" }.into(),
        target: format!("shop_item:{new_id}"),
        details: b.to_string(),
    })?;
    Ok(json!({ "id": new_id }))
}

impl http_handler::Guest for Shop {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let uid = ctx::current_user()?;
        let reply = match req.route() {
            "items" => http::Reply::ok(&catalog(uid)?),
            "purchase" => http::Reply::created(&purchase(uid, req.param_i64("id")?, &req.body_value())?),
            "orders" => http::Reply::ok(&orders(Some(uid))?),
            "admin_items" => {
                let r = sql::query(&format!("SELECT {ITEM_COLS} FROM items ORDER BY sort, id"), &[])?;
                http::Reply::ok(&json!({ "items": r.rows.iter().map(item_json).collect::<Vec<_>>() }))
            }
            "admin_create" => http::Reply::created(&save_item(None, &req.body_value())?),
            "admin_update" => http::Reply::ok(&save_item(Some(req.param_i64("id")?), &req.body_value())?),
            "admin_orders" => http::Reply::ok(&orders(None)?),
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(Shop);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params() {
        assert!(check_params("upload_credit", &json!({ "bytes": 1_073_741_824_i64 })).is_ok());
        assert!(check_params("upload_credit", &json!({ "bytes": 0 })).is_err());
        assert!(check_params("invite", &json!({ "count": 1, "expires_days": 30 })).is_ok());
        assert!(check_params("invite", &json!({ "count": 0 })).is_err());
        assert!(check_params("promo_card", &json!({ "card_kind": "free", "count": 2 })).is_ok());
        assert!(check_params("promo_card", &json!({ "card_kind": "gold", "count": 2 })).is_err());
        assert!(check_params("rename", &json!({})).is_err());
    }
}
