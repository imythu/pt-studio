//! 限制与有效状态（identity 03 §1.6）。
//!
//! 有效状态 = 生效限制中严重度最高者（`active < leech_disabled < pending < banned`）；变化时发布 `user.status_changed`，
//! 封禁时递增 `auth_version` 使全部会话失效。

use pt_plugin_api::{Ctx, NewEvent, PResult, PluginError, json};
use serde_json::Value;
use sqlx::Row;

use crate::rbac;

/// 限制类型 → 用户状态
fn status_of(kind: &str) -> &'static str {
    match kind {
        "ban" => "banned",
        "pending" => "pending",
        _ => "leech_disabled",
    }
}

/// 用户状态 → 限制类型（`set_status` 用）
pub fn kind_of(status: &str) -> Option<&'static str> {
    match status {
        "banned" => Some("ban"),
        "leech_disabled" => Some("leech_disabled"),
        "pending" => Some("pending"),
        _ => None,
    }
}

fn severity(status: &str) -> u8 {
    match status {
        "leech_disabled" => 1,
        "pending" => 2,
        "banned" => 3,
        "deleted" => 4,
        _ => 0,
    }
}

pub struct Apply<'a> {
    pub user_id: i64,
    pub kind: &'a str,
    pub source: &'a str,
    pub reason_code: &'a str,
    pub note: Option<&'a str>,
    /// Unix 毫秒
    pub until_ms: Option<i64>,
    pub by_user: Option<i64>,
}

/// 插件来源不能限制 moderator 及以上（03 §1.6 `identity.target_protected`）
async fn check_protected(ctx: &Ctx, uid: i64, source: &str) -> PResult<()> {
    if source == "admin" {
        return Ok(());
    }
    let (_, roles) = rbac::effective_roles(ctx, uid).await?;
    let top = roles
        .iter()
        .filter_map(|r| rbac::BUILTIN.iter().find(|(n, _)| n == r).map(|(_, k)| *k))
        .max()
        .unwrap_or(0);
    if top >= 3 {
        return Err(PluginError::new(
            "identity.target_protected",
            "plugins cannot restrict staff accounts",
        ));
    }
    Ok(())
}

/// 施加（或更新同来源同类型的）限制，返回限制 ID
pub async fn apply(ctx: &Ctx, a: &Apply<'_>) -> PResult<i64> {
    if !matches!(a.kind, "ban" | "leech_disabled" | "pending") {
        return Err(PluginError::new(
            "identity.invalid_status",
            "unknown restriction kind",
        ));
    }
    if a.reason_code.is_empty()
        || a.reason_code.len() > 64
        || a.note.is_some_and(|n| n.chars().count() > 1000)
    {
        return Err(PluginError::validation(
            "reason_code (1..=64) and note (≤ 1000) are required",
        ));
    }
    if a.until_ms.is_some_and(|u| u <= ctx.time.now()) {
        return Err(PluginError::validation("until must be in the future"));
    }
    check_protected(ctx, a.user_id, a.source).await?;
    let mut c = ctx.sql.conn().await?;
    let exists: Option<String> = sqlx::query_scalar("SELECT status FROM users WHERE id = $1 FOR UPDATE")
        .bind(a.user_id)
        .fetch_optional(&mut *c)
        .await?;
    match exists.as_deref() {
        None => return Err(PluginError::new("identity.user_not_found", "user not found")),
        Some("deleted") => return Err(PluginError::new("identity.invalid_status", "user is deleted")),
        _ => {}
    }
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO restrictions (user_id, kind, source, reason_code, note, until, created_by)
         VALUES ($1, $2, $3, $4, $5, CASE WHEN $6::bigint IS NULL THEN NULL ELSE to_timestamp($6::float8 / 1000) END, $7)
         ON CONFLICT (user_id, kind, source) WHERE lifted_at IS NULL DO UPDATE
            SET reason_code = EXCLUDED.reason_code, note = EXCLUDED.note, until = EXCLUDED.until,
                updated_at = now(), updated_by = EXCLUDED.created_by
         RETURNING id",
    )
    .bind(a.user_id)
    .bind(a.kind)
    .bind(a.source)
    .bind(a.reason_code)
    .bind(a.note)
    .bind(a.until_ms)
    .bind(a.by_user)
    .fetch_one(&mut *c)
    .await?;
    drop(c);
    recompute(ctx, a.user_id, a.source, a.reason_code).await?;
    Ok(id)
}

/// 解除限制：`kind = None` 表示该来源的全部类型；`all_sources` 时忽略来源（管理员“全部解除”）。返回解除条数
pub async fn lift(
    ctx: &Ctx,
    uid: i64,
    kind: Option<&str>,
    source: &str,
    reason: &str,
    by_user: Option<i64>,
    all_sources: bool,
) -> PResult<u64> {
    let mut c = ctx.sql.conn().await?;
    let n = sqlx::query(
        "UPDATE restrictions SET lifted_at = now(), lifted_by = $5, lift_reason = $6
          WHERE user_id = $1 AND lifted_at IS NULL AND ($4 OR source = $3) AND ($2::text IS NULL OR kind = $2)",
    )
    .bind(uid)
    .bind(kind)
    .bind(source)
    .bind(all_sources)
    .bind(by_user)
    .bind(reason)
    .execute(&mut *c)
    .await?
    .rows_affected();
    drop(c);
    if n > 0 {
        recompute(ctx, uid, source, reason).await?;
    }
    Ok(n)
}

/// 按单条限制 ID 解除（管理员接口）
pub async fn lift_one(ctx: &Ctx, uid: i64, rid: i64, reason: &str, by_user: Option<i64>) -> PResult<bool> {
    let mut c = ctx.sql.conn().await?;
    let n = sqlx::query(
        "UPDATE restrictions SET lifted_at = now(), lifted_by = $3, lift_reason = $4
          WHERE id = $2 AND user_id = $1 AND lifted_at IS NULL",
    )
    .bind(uid)
    .bind(rid)
    .bind(by_user)
    .bind(reason)
    .execute(&mut *c)
    .await?
    .rows_affected();
    drop(c);
    if n > 0 {
        recompute(ctx, uid, "admin", reason).await?;
    }
    Ok(n > 0)
}

/// 重新计算有效状态；变化时发布 `user.status_changed`
pub async fn recompute(ctx: &Ctx, uid: i64, trigger_source: &str, reason: &str) -> PResult<()> {
    let mut c = ctx.sql.conn().await?;
    let Some(cur) = sqlx::query_scalar::<_, String>("SELECT status FROM users WHERE id = $1 FOR UPDATE")
        .bind(uid)
        .fetch_optional(&mut *c)
        .await?
    else {
        return Ok(());
    };
    if cur == "deleted" {
        return Ok(());
    }
    let rows = sqlx::query(
        "SELECT kind, (extract(epoch FROM until) * 1000)::bigint FROM restrictions
          WHERE user_id = $1 AND lifted_at IS NULL AND (until IS NULL OR until > now())",
    )
    .bind(uid)
    .fetch_all(&mut *c)
    .await?;
    let mut eff = "active";
    for r in &rows {
        let s = status_of(r.get::<String, _>(0).as_str());
        if severity(s) > severity(eff) {
            eff = s;
        }
    }
    if eff == cur {
        return Ok(());
    }
    // 该状态下的最晚到期（任一同状态限制无到期 → 无到期）
    let same: Vec<Option<i64>> = rows
        .iter()
        .filter(|r| status_of(r.get::<String, _>(0).as_str()) == eff)
        .map(|r| r.get(1))
        .collect();
    let until = if same.iter().any(Option::is_none) {
        None
    } else {
        same.iter().flatten().max().copied()
    };
    sqlx::query(
        "UPDATE users SET status = $2, status_version = status_version + 1,
                auth_version = auth_version + CASE WHEN $2 = 'banned' THEN 1 ELSE 0 END, updated_at = now()
          WHERE id = $1",
    )
    .bind(uid)
    .bind(eff)
    .execute(&mut *c)
    .await?;
    if eff == "banned" {
        sqlx::query("UPDATE sessions SET revoked_at = now(), revoked_reason = 'banned' WHERE user_id = $1 AND revoked_at IS NULL")
            .bind(uid)
            .execute(&mut *c)
            .await?;
    }
    drop(c);
    let mut payload = json!({ "user_id": uid, "from": cur, "to": eff, "reason": reason, "restriction_source": trigger_source });
    if let (Some(u), Some(o)) = (until, payload.as_object_mut()) {
        o.insert("until".into(), Value::from(u));
    }
    ctx.events
        .publish(NewEvent::v1(
            "user.status_changed",
            format!("user:{uid}"),
            payload,
        ))
        .await?;
    ctx.audit
        .write(pt_plugin_api::AuditEntry {
            action: "identity.status_change".into(),
            target: format!("user:{uid}"),
            details: json!({ "from": cur, "to": eff, "source": trigger_source, "reason": reason }),
        })
        .await?;
    Ok(())
}

/// 到期限制：标记解除并重算（cron `restriction_sweep`，每批 500）
pub async fn sweep(ctx: &Ctx) -> PResult<usize> {
    let mut total = 0;
    loop {
        let mut c = ctx.sql.conn().await?;
        let rows = sqlx::query(
            "UPDATE restrictions SET lifted_at = until, lift_reason = 'expired'
              WHERE id IN (SELECT id FROM restrictions WHERE lifted_at IS NULL AND until <= now()
                           ORDER BY until LIMIT 500 FOR UPDATE SKIP LOCKED)
             RETURNING user_id, source",
        )
        .fetch_all(&mut *c)
        .await?;
        drop(c);
        if rows.is_empty() {
            return Ok(total);
        }
        total += rows.len();
        let mut users: Vec<(i64, String)> = rows.iter().map(|r| (r.get(0), r.get(1))).collect();
        users.sort();
        users.dedup_by_key(|u| u.0);
        for (uid, source) in users {
            recompute(ctx, uid, &source, "expired").await?;
        }
        ctx.sql.commit().await?;
    }
}

pub async fn list(ctx: &Ctx, uid: i64) -> PResult<Value> {
    let mut c = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT id, kind, source, reason_code, note, (extract(epoch FROM until) * 1000)::bigint,
                (extract(epoch FROM created_at) * 1000)::bigint, created_by,
                (extract(epoch FROM lifted_at) * 1000)::bigint, lift_reason
           FROM restrictions WHERE user_id = $1 ORDER BY created_at DESC, id DESC LIMIT 200",
    )
    .bind(uid)
    .fetch_all(&mut *c)
    .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({ "id": r.get::<i64, _>(0), "kind": r.get::<String, _>(1), "source": r.get::<String, _>(2),
                    "reason_code": r.get::<String, _>(3), "note": r.get::<Option<String>, _>(4),
                    "until": r.get::<Option<i64>, _>(5), "created_at": r.get::<i64, _>(6),
                    "created_by": r.get::<Option<i64>, _>(7), "lifted_at": r.get::<Option<i64>, _>(8),
                    "lift_reason": r.get::<Option<String>, _>(9) })
        })
        .collect();
    Ok(json!({ "items": items }))
}
