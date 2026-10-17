//! seedbox 插件（WASM，docs/plugins/seedbox/）：盒子登记。
//!
//! - 用户登记盒子的 IP 段（IPv4 前缀 ≥ /24、IPv6 ≥ /48，不得为私有 / 保留地址段，主机位归零），每人上限可配置；
//!   需要审核时为待审核，管理员批准 / 拒绝（可设过期）；与其他用户的网段重叠时记录给管理员参考。
//! - 已批准的网段经视图 `v_ranges` 交给 accounting：写入 peer 时按记账流中的 IP 标记 `is_seedbox`。
//! - `ctx.seedbox.is_seedbox {user, torrent}`：该用户在该种子上是否有来自盒子的活跃 peer（`accounting.v_swarm_peers`）。
//!
//! 偏差：ASN 自动识别（数据中心检测）未实现。

pt_plugin_sdk::world!("seedbox", http);

use std::net::IpAddr;

use exports::pt::plugin::{http_handler, on_cron, service_provider};
use pt_plugin_sdk::{audit, prelude::*, serde_json, sql};

struct Seedbox;

// ---------------- 网段 ----------------

/// 解析并规范化 CIDR（主机位归零）；返回规范文本
pub fn normalize_cidr(s: &str) -> Result<String, &'static str> {
    let s = s.trim();
    let (addr, bits) = match s.split_once('/') {
        Some((a, b)) => (a, b.parse::<u8>().map_err(|_| "bad prefix length")?),
        None => (s, if s.contains(':') { 128 } else { 32 }),
    };
    let ip: IpAddr = addr.parse().map_err(|_| "not an IP address")?;
    match ip {
        IpAddr::V4(v4) => {
            if !(24..=32).contains(&bits) {
                return Err("IPv4 ranges must be /24 or narrower");
            }
            let o = v4.octets();
            let reserved = o[0] == 0
                || o[0] == 10
                || o[0] == 127
                || (o[0] == 100 && (64..=127).contains(&o[1]))
                || (o[0] == 169 && o[1] == 254)
                || (o[0] == 172 && (16..=31).contains(&o[1]))
                || (o[0] == 192 && o[1] == 168)
                || o[0] >= 224;
            if reserved {
                return Err("private or reserved address range");
            }
            let mask = if bits == 0 { 0 } else { u32::MAX << (32 - u32::from(bits)) };
            Ok(format!("{}/{bits}", std::net::Ipv4Addr::from(u32::from(v4) & mask)))
        }
        IpAddr::V6(v6) => {
            if !(48..=128).contains(&bits) {
                return Err("IPv6 ranges must be /48 or narrower");
            }
            let seg = v6.segments();
            let reserved = v6.is_loopback()
                || v6.is_unspecified()
                || (seg[0] & 0xfe00) == 0xfc00
                || (seg[0] & 0xffc0) == 0xfe80
                || (seg[0] & 0xff00) == 0xff00
                || v6.to_ipv4_mapped().is_some();
            if reserved {
                return Err("private or reserved address range");
            }
            let mask = if bits == 0 { 0 } else { u128::MAX << (128 - u32::from(bits)) };
            Ok(format!("{}/{bits}", std::net::Ipv6Addr::from(u128::from(v6) & mask)))
        }
    }
}

// ---------------- 设置 ----------------

struct Settings {
    require_review: bool,
    max_ranges: i64,
    approval_days: i64,
}

fn settings() -> Settings {
    let s = ctx::settings();
    Settings {
        require_review: s["require_review"].as_bool().unwrap_or(true),
        max_ranges: s["max_ranges_per_user"].as_i64().unwrap_or(5).clamp(1, 20),
        approval_days: s["approval_days"].as_i64().unwrap_or(0).max(0),
    }
}

fn publish(event: &str, uid: i64, payload: &Value) -> R<()> {
    ctx::publish(event, &format!("user:{uid}"), payload)?;
    Ok(())
}

fn audit_write(action: &str, target: String, details: &Value) -> R<()> {
    audit::write(&audit::AuditEntry {
        action: action.into(),
        target,
        details: details.to_string(),
    })
}

// ---------------- 用户 ----------------

fn boxes_of(uid: Option<i64>, state: Option<&str>) -> R<Vec<Value>> {
    let r = sql::query(
        "SELECT b.id, b.user_id, u.username, b.provider, b.note, b.state, b.review_reason,
                (extract(epoch FROM b.expires_at) * 1000)::bigint, (extract(epoch FROM b.created_at) * 1000)::bigint, b.overlap_note,
                (SELECT coalesce(array_to_json(array_agg(host(cidr) || '/' || masklen(cidr) ORDER BY cidr)), '[]')::text FROM box_ranges r WHERE r.box_id = b.id)
           FROM boxes b LEFT JOIN identity.v_user_public u ON u.id = b.user_id
          WHERE ($1::bigint IS NULL OR b.user_id = $1) AND ($2::text IS NULL OR b.state = $2)
          ORDER BY b.created_at DESC LIMIT 200",
        &[uid.map_or(SqlValue::Null, SqlValue::Int), state.map_or(SqlValue::Null, |s| SqlValue::Text(s.into()))],
    )?;
    Ok(r.rows
        .iter()
        .map(|row| {
            json!({ "id": row.int(0), "user_id": row.int(1), "username": row.text(2), "provider": row.text(3), "note": row.text(4),
                    "state": row.text(5), "review_reason": row.text(6), "expires_at": row.int(7), "created_at": row.int(8),
                    "overlap_note": row.text(9),
                    "ranges": row.text(10).and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(json!([])) })
        })
        .collect())
}

fn create_box(uid: i64, b: &Value) -> R<Value> {
    let s = settings();
    let raw: Vec<String> = b["ranges"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    if raw.is_empty() || raw.len() > 5 {
        return Err(err("seedbox.range_invalid", "give 1 to 5 IP ranges"));
    }
    let mut ranges = Vec::new();
    for (i, r) in raw.iter().enumerate() {
        let n = normalize_cidr(r).map_err(|why| {
            err("seedbox.range_invalid", &format!("{r}: {why}"))
                .with_details(&json!({ "errors": [{ "path": format!("/ranges/{i}"), "code": "seedbox.range_invalid", "message": why }] }))
        })?;
        if !ranges.contains(&n) {
            ranges.push(n);
        }
    }
    let have = sql::query(
        "SELECT count(*) FROM box_ranges r JOIN boxes b ON b.id = r.box_id WHERE r.user_id = $1 AND b.state IN ('pending', 'approved')",
        &[SqlValue::Int(uid)],
    )?
    .rows
    .first()
    .and_then(|row| row.int(0))
    .unwrap_or(0);
    if have + i64::try_from(ranges.len()).unwrap_or(5) > s.max_ranges {
        return Err(err("seedbox.too_many_ranges", &format!("at most {} IP ranges per user", s.max_ranges)));
    }
    // 与他人的网段重叠：只记录给管理员（共享盒子、同一机房都可能）
    let overlap = sql::query(
        "SELECT DISTINCT u.username FROM box_ranges r JOIN boxes b ON b.id = r.box_id LEFT JOIN identity.v_user_public u ON u.id = r.user_id
          WHERE r.user_id <> $1 AND b.state IN ('pending', 'approved') AND r.cidr && ANY($2::cidr[]) LIMIT 10",
        &[SqlValue::Int(uid), SqlValue::TextArray(ranges.clone())],
    )?;
    let overlap_note = (!overlap.rows.is_empty()).then(|| {
        overlap.rows.iter().filter_map(|r| r.text(0)).collect::<Vec<_>>().join(", ")
    });
    let state = if s.require_review { "pending" } else { "approved" };
    let text = |k: &str, max: usize| b[k].as_str().map(|v| v.trim().chars().take(max).collect::<String>()).filter(|v| !v.is_empty());
    let id = sql::query(
        "INSERT INTO boxes (user_id, provider, note, state, overlap_note, expires_at)
         VALUES ($1, $2, $3, $4, $5, CASE WHEN $4 = 'approved' AND $6 > 0 THEN now() + make_interval(days => $6::int) END) RETURNING id",
        &[
            SqlValue::Int(uid),
            text("provider", 64).map_or(SqlValue::Null, SqlValue::Text),
            text("note", 200).map_or(SqlValue::Null, SqlValue::Text),
            SqlValue::Text(state.into()),
            overlap_note.map_or(SqlValue::Null, SqlValue::Text),
            SqlValue::Int(s.approval_days),
        ],
    )?
    .rows
    .first()
    .and_then(|row| row.int(0))
    .ok_or_else(|| err("seedbox.internal", "insert failed"))?;
    for r in &ranges {
        sql::execute(
            "INSERT INTO box_ranges (box_id, user_id, cidr) VALUES ($1, $2, $3::cidr)",
            &[SqlValue::Int(id), SqlValue::Int(uid), SqlValue::Text(r.clone())],
        )?;
    }
    publish("seedbox.registered", uid, &json!({ "user_id": uid, "record_id": id, "state": state }))?;
    Ok(json!({ "id": id, "state": state, "ranges": ranges }))
}

fn delete_box(uid: i64, id: i64) -> R<()> {
    let n = sql::execute("DELETE FROM boxes WHERE id = $1 AND user_id = $2", &[SqlValue::Int(id), SqlValue::Int(uid)])?;
    if n == 0 {
        return Err(err("kernel.not_found", "box not found"));
    }
    publish("seedbox.removed", uid, &json!({ "user_id": uid, "record_id": id }))
}

// ---------------- 管理员 ----------------

fn review(id: i64, b: &Value) -> R<()> {
    let approve = b["approve"].as_bool().ok_or_else(|| err("kernel.validation_failed", "approve (bool) is required"))?;
    let reason = b["reason"].as_str().unwrap_or("").chars().take(500).collect::<String>();
    let expires = b["expires_at"].as_i64();
    let days = settings().approval_days;
    let r = sql::query(
        "UPDATE boxes SET state = $2, review_reason = $3, reviewed_by = $4, reviewed_at = now(), updated_at = now(),
                expires_at = CASE WHEN $2 = 'approved' THEN coalesce(to_timestamp($5::float8 / 1000),
                                  CASE WHEN $6 > 0 THEN now() + make_interval(days => $6::int) END) END
          WHERE id = $1 AND state = 'pending' RETURNING user_id",
        &[
            SqlValue::Int(id),
            SqlValue::Text(if approve { "approved" } else { "rejected" }.into()),
            SqlValue::Text(reason.clone()),
            SqlValue::Int(ctx::current_user()?),
            expires.map_or(SqlValue::Null, SqlValue::Int),
            SqlValue::Int(days),
        ],
    )?;
    let Some(uid) = r.rows.first().and_then(|row| row.int(0)) else {
        return Err(err("seedbox.not_pending", "the registration is not waiting for review"));
    };
    audit_write("seedbox.review", format!("seedbox:{id}"), &json!({ "user_id": uid, "approve": approve, "reason": reason }))?;
    publish("seedbox.reviewed", uid, &json!({ "user_id": uid, "record_id": id, "approved": approve }))
}

fn is_seedbox(uid: i64, tid: i64) -> R<bool> {
    let r = sql::query(
        "SELECT EXISTS (SELECT 1 FROM accounting.v_swarm_peers WHERE user_id = $1 AND torrent_id = $2 AND is_seedbox)",
        &[SqlValue::Int(uid), SqlValue::Int(tid)],
    )?;
    Ok(r.rows.first().and_then(|row| row.bool(0)).unwrap_or(false))
}

impl service_provider::Guest for Seedbox {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        match method.as_str() {
            "is_seedbox" => {
                let (Some(u), Some(t)) = (a["user"].as_i64(), a["torrent"].as_i64()) else {
                    return Err(err("kernel.validation_failed", "user and torrent are required"));
                };
                Ok(json!(is_seedbox(u, t)?).to_string())
            }
            "has_box" => {
                let u = a["user"].as_i64().ok_or_else(|| err("kernel.validation_failed", "user"))?;
                let r = sql::query("SELECT EXISTS (SELECT 1 FROM v_ranges WHERE user_id = $1)", &[SqlValue::Int(u)])?;
                Ok(json!(r.rows.first().and_then(|row| row.bool(0)).unwrap_or(false)).to_string())
            }
            other => Err(err("kernel.not_found", &format!("ctx.seedbox has no method `{other}`"))),
        }
    }
}

impl on_cron::Guest for Seedbox {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        if job.name != "expire" {
            return Err(err("kernel.not_found", "unknown job"));
        }
        sql::execute(
            "UPDATE boxes SET state = 'expired', updated_at = now() WHERE state = 'approved' AND expires_at IS NOT NULL AND expires_at <= now()",
            &[],
        )?;
        Ok(())
    }
}

impl http_handler::Guest for Seedbox {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let reply = match req.route() {
            "my_boxes" => http::Reply::ok(&json!({ "items": boxes_of(Some(ctx::current_user()?), None)?, "max_ranges": settings().max_ranges })),
            "create_box" => http::Reply::created(&create_box(ctx::current_user()?, &req.body_json::<Value>()?)?),
            "delete_box" => {
                delete_box(ctx::current_user()?, req.param_i64("id")?)?;
                http::Reply::no_content()
            }
            "admin_list" => {
                let st = req.query("state").filter(|s| matches!(*s, "pending" | "approved" | "rejected" | "expired"));
                http::Reply::ok(&json!({ "items": boxes_of(None, st)? }))
            }
            "admin_review" => {
                review(req.param_i64("id")?, &req.body_json::<Value>()?)?;
                http::Reply::no_content()
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(Seedbox);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidrs() {
        assert_eq!(normalize_cidr("203.0.113.77/24"), Ok("203.0.113.0/24".into()));
        assert_eq!(normalize_cidr(" 198.51.100.9 "), Ok("198.51.100.9/32".into()));
        assert_eq!(normalize_cidr("2001:db8:1:2::5/48"), Ok("2001:db8:1::/48".into()));
        assert!(normalize_cidr("203.0.0.0/16").is_err(), "too wide");
        assert!(normalize_cidr("10.1.2.0/24").is_err());
        assert!(normalize_cidr("192.168.1.0/24").is_err());
        assert!(normalize_cidr("172.20.0.0/24").is_err());
        assert!(normalize_cidr("fd00::/64").is_err());
        assert!(normalize_cidr("::ffff:1.2.3.4").is_err());
        assert!(normalize_cidr("nope").is_err());
    }
}
