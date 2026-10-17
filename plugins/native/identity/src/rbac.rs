//! 权限（identity 03 §1.10）。P1：直接查询 PG；进程内快照与 Redis 缓存在后续迭代加入。

use std::collections::HashSet;

use pt_plugin_api::{Ctx, PResult, PluginError, Principal, services::PermissionDecl};

/// 内置角色按 rank 继承：高 rank 包含所有低 rank 角色的权限。
pub const BUILTIN: &[(&str, i16)] = &[
    ("guest", 0),
    ("user", 1),
    ("uploader", 2),
    ("moderator", 3),
    ("admin", 4),
    ("sysop", 5),
];

pub fn base_roles(status: &str) -> Vec<&'static str> {
    match status {
        "pending" => vec!["guest"],
        "active" | "leech_disabled" => vec!["user"],
        _ => vec![],
    }
}

/// 展开内置角色继承。
pub fn expand(roles: &HashSet<String>) -> HashSet<String> {
    let max_rank = roles
        .iter()
        .filter_map(|r| BUILTIN.iter().find(|(n, _)| n == r).map(|(_, k)| *k))
        .max();
    let mut out = roles.clone();
    if let Some(m) = max_rank {
        for (n, k) in BUILTIN {
            if *k <= m {
                out.insert((*n).to_owned());
            }
        }
    }
    out
}

pub async fn register_permissions(ctx: &Ctx, defs: Vec<PermissionDecl>) -> PResult<()> {
    if ctx.invocation.caller_plugin != "kernel" {
        return Err(PluginError::capability_denied(
            "register_permissions is reserved for the kernel",
        ));
    }
    let mut conn = ctx.sql.conn().await?;
    let mut names = Vec::with_capacity(defs.len());
    for d in &defs {
        let inserted: bool = sqlx::query_scalar(
            "INSERT INTO permissions (name, plugin_id, description_term, default_roles)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (name) DO UPDATE SET description_term = EXCLUDED.description_term, orphan = false, last_seen_at = now()
             RETURNING (xmax = 0)",
        )
        .bind(&d.name)
        .bind(&d.plugin_id)
        .bind(d.description_term.clone().unwrap_or_else(|| d.name.clone()))
        .bind(&d.default_roles)
        .fetch_one(&mut *conn)
        .await?;
        // 默认授权只在首次登记时发生（管理员之后撤销不会被覆盖）
        if inserted {
            for r in &d.default_roles {
                sqlx::query(
                    "INSERT INTO role_permissions (role, permission) SELECT $1, $2 WHERE EXISTS (SELECT 1 FROM roles WHERE name = $1)
                     ON CONFLICT DO NOTHING",
                )
                .bind(r)
                .bind(&d.name)
                .execute(&mut *conn)
                .await?;
            }
        }
        names.push(d.name.clone());
    }
    sqlx::query("UPDATE permissions SET orphan = true WHERE NOT (name = ANY($1)) AND NOT orphan")
        .bind(&names)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// 用户的有效角色（含继承）。
pub async fn effective_roles(ctx: &Ctx, user_id: i64) -> PResult<(String, HashSet<String>)> {
    let mut conn = ctx.sql.conn().await?;
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM users WHERE id = $1 AND deleted_at IS NULL")
            .bind(user_id)
            .fetch_optional(&mut *conn)
            .await?;
    let Some(status) = status else {
        return Ok(("deleted".into(), HashSet::new()));
    };
    let mut roles: HashSet<String> = base_roles(&status).into_iter().map(str::to_owned).collect();
    if !roles.is_empty() {
        // 直接授予的角色 ∪ 等级绑定的角色（class_roles）
        let granted: Vec<String> = sqlx::query_scalar(
            "SELECT role FROM user_roles WHERE user_id = $1 AND (expires_at IS NULL OR expires_at > now())
             UNION SELECT cr.role FROM class_roles cr JOIN users u ON u.class_id = cr.class_id WHERE u.id = $1",
        )
        .bind(user_id)
        .fetch_all(&mut *conn)
        .await?;
        roles.extend(granted);
    }
    Ok((status, expand(&roles)))
}

/// 角色集合中最高的内置角色 rank（没有内置角色为 0）
pub fn top_rank(roles: &HashSet<String>) -> i16 {
    roles
        .iter()
        .filter_map(|r| BUILTIN.iter().find(|(n, _)| n == r).map(|(_, k)| *k))
        .max()
        .unwrap_or(0)
}

pub async fn permissions_of(ctx: &Ctx, user_id: i64) -> PResult<Vec<String>> {
    permissions_capped(ctx, user_id, None).await
}

/// 有效权限；`cap = Some(rank)` 时剔除 rank ≥ cap 的内置角色（未通过 2FA 的会话不享有要求 2FA 的角色）
pub async fn permissions_capped(ctx: &Ctx, user_id: i64, cap: Option<i16>) -> PResult<Vec<String>> {
    let (_, roles) = effective_roles(ctx, user_id).await?;
    let roles: Vec<String> = roles
        .into_iter()
        .filter(|r| match (cap, BUILTIN.iter().find(|(n, _)| n == r)) {
            (Some(c), Some((_, k))) => *k < c,
            _ => true,
        })
        .collect();
    let mut conn = ctx.sql.conn().await?;
    let perms: Vec<String> = if roles.iter().any(|r| r == "sysop") {
        sqlx::query_scalar("SELECT name FROM permissions WHERE NOT orphan ORDER BY name")
            .fetch_all(&mut *conn)
            .await?
    } else {
        sqlx::query_scalar(
            "SELECT DISTINCT p.name FROM role_permissions rp JOIN permissions p ON p.name = rp.permission
              WHERE rp.role = ANY($1) AND NOT p.orphan ORDER BY p.name",
        )
        .bind(&roles)
        .fetch_all(&mut *conn)
        .await?
    };
    Ok(perms)
}

/// `ctx.rbac.check_principal`（评审 R1 B-06）。
pub async fn check_principal(
    ctx: &Ctx,
    p: &Principal,
    permission: Option<&str>,
    method: &str,
) -> PResult<bool> {
    let safe = matches!(method, "GET" | "HEAD" | "OPTIONS");
    // token 作用域：只读 token 不能执行写方法；feed key 只能读
    match p.credential_kind.as_str() {
        "token" | "mcp" if !safe && !p.scopes.iter().any(|s| s == "write" || s == "*") => return Ok(false),
        "feed" if !safe => return Ok(false),
        _ => {}
    }
    let Some(perm) = permission else { return Ok(true) };
    let uid = i64::try_from(p.user_id).map_err(|_| PluginError::bad_request("bad user id"))?;
    let perms = permissions_capped(ctx, uid, two_factor_cap(ctx, p).await).await?;
    let ok = perms.iter().any(|x| x == perm);
    if !ok {
        tracing::debug!(user = uid, permission = perm, "permission denied");
    }
    Ok(ok)
}

/// 会话未通过 2FA 且站点要求某角色及以上必须 2FA 时的权限上限
pub async fn two_factor_cap(ctx: &Ctx, p: &Principal) -> Option<i16> {
    if p.credential_kind != "session" || p.scopes.iter().any(|s| s == "2fa") {
        return None;
    }
    crate::policy::Policy::load(ctx).await.two_factor_required_rank
}

/// 授予角色（P1：仅内置角色，且不能授予 sysop）。
pub async fn grant_role(ctx: &Ctx, user_id: i64, role: &str, granted_by: &str) -> PResult<()> {
    if role == "sysop" {
        return Err(PluginError::forbidden(
            "sysop can only be granted from the command line",
        ));
    }
    let mut conn = ctx.sql.conn().await?;
    let r = sqlx::query(
        "INSERT INTO user_roles (user_id, role, granted_by) SELECT $1, $2, $3
          WHERE EXISTS (SELECT 1 FROM users WHERE id = $1 AND deleted_at IS NULL)
         ON CONFLICT (user_id, role) DO UPDATE SET expires_at = NULL, updated_at = now()",
    )
    .bind(user_id)
    .bind(role)
    .bind(granted_by)
    .execute(&mut *conn)
    .await;
    match r {
        Ok(res) if res.rows_affected() == 0 => Err(PluginError::not_found("user not found")),
        Ok(_) => Ok(()),
        Err(sqlx::Error::Database(db)) if db.is_foreign_key_violation() => Err(PluginError::new(
            "identity.role_not_found",
            format!("role `{role}` does not exist"),
        )),
        Err(e) => Err(e.into()),
    }
}

/// 设置用户等级（`ctx.users.set_class`）：相同等级不发事件；返回是否变化。
pub async fn set_class(ctx: &Ctx, user_id: i64, class_id: i64, reason: &str) -> PResult<bool> {
    let class = i32::try_from(class_id).map_err(|_| PluginError::validation("class_id out of range"))?;
    let mut conn = ctx.sql.conn().await?;
    let from: Option<i32> =
        sqlx::query_scalar("SELECT class_id FROM users WHERE id = $1 AND deleted_at IS NULL FOR UPDATE")
            .bind(user_id)
            .fetch_optional(&mut *conn)
            .await?;
    let Some(from) = from else {
        return Err(PluginError::not_found("user not found"));
    };
    if from == class {
        return Ok(false);
    }
    sqlx::query("UPDATE users SET class_id = $2, updated_at = now() WHERE id = $1")
        .bind(user_id)
        .bind(class)
        .execute(&mut *conn)
        .await?;
    drop(conn);
    ctx.events
        .publish(pt_plugin_api::NewEvent::v1(
            "user.class_changed",
            format!("user:{user_id}"),
            serde_json::json!({
            "user_id": user_id, "from_class": from, "to_class": class, "reason": reason,
            "by_plugin": ctx.invocation.caller_plugin }),
        ))
        .await?;
    Ok(true)
}

fn role_name_ok(caller: &str, role: &str) -> bool {
    role.strip_prefix(&format!("{caller}:")).is_some_and(|rest| {
        (1..=48).contains(&rest.len())
            && rest
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
    })
}

/// 定义插件角色（名必须为 `<调用方>:<名>`）；已存在则不变。
pub async fn define_role(ctx: &Ctx, caller: &str, role: &str) -> PResult<()> {
    if !role_name_ok(caller, role) {
        return Err(PluginError::new(
            "identity.role_owner_mismatch",
            "plugin roles must be named `<plugin_id>:<name>`",
        ));
    }
    let mut conn = ctx.sql.conn().await?;
    sqlx::query("INSERT INTO roles (name, kind, owner_plugin, name_term) VALUES ($1, 'plugin', $2, $3) ON CONFLICT (name) DO NOTHING")
        .bind(role)
        .bind(caller)
        .bind(format!("{caller}.role.{}", role.split_once(':').map_or(role, |r| r.1)))
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// 整体替换某等级绑定的角色：只能是调用方前缀的角色或内置 uploader 及以下（03 §1.10）。
pub async fn set_class_roles(ctx: &Ctx, caller: &str, class_id: i64, roles: &[String]) -> PResult<()> {
    let class = i32::try_from(class_id).map_err(|_| PluginError::validation("class_id out of range"))?;
    for r in roles {
        let builtin_ok = BUILTIN.iter().any(|(n, rank)| n == r && *rank <= 2);
        if !(builtin_ok || role_name_ok(caller, r)) {
            return Err(PluginError::new(
                "identity.role_protected",
                format!("role `{r}` cannot be bound to a class"),
            ));
        }
    }
    let mut conn = ctx.sql.conn().await?;
    sqlx::query("DELETE FROM class_roles WHERE class_id = $1 AND owner_plugin = $2")
        .bind(class)
        .bind(caller)
        .execute(&mut *conn)
        .await?;
    for r in roles {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM roles WHERE name = $1)")
            .bind(r)
            .fetch_one(&mut *conn)
            .await?;
        if !exists {
            return Err(PluginError::new(
                "identity.role_not_found",
                format!("role `{r}` does not exist"),
            ));
        }
        sqlx::query("INSERT INTO class_roles (class_id, role, owner_plugin) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING")
            .bind(class)
            .bind(r)
            .bind(caller)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inheritance() {
        let r = expand(&HashSet::from(["moderator".to_owned(), "x:y".to_owned()]));
        for want in ["guest", "user", "uploader", "moderator", "x:y"] {
            assert!(r.contains(want), "{want}");
        }
        assert!(!r.contains("admin"));
        assert_eq!(base_roles("banned"), Vec::<&str>::new());
        assert_eq!(base_roles("pending"), vec!["guest"]);
        assert!(role_name_ok("levels", "levels:class_30"));
        assert!(!role_name_ok("levels", "shop:vip"));
        assert!(!role_name_ok("levels", "levels:Bad"));
    }
}
