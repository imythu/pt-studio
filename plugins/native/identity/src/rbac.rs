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
        let granted: Vec<String> = sqlx::query_scalar(
            "SELECT role FROM user_roles WHERE user_id = $1 AND (expires_at IS NULL OR expires_at > now())",
        )
        .bind(user_id)
        .fetch_all(&mut *conn)
        .await?;
        roles.extend(granted);
    }
    Ok((status, expand(&roles)))
}

pub async fn permissions_of(ctx: &Ctx, user_id: i64) -> PResult<Vec<String>> {
    let (_, roles) = effective_roles(ctx, user_id).await?;
    let roles: Vec<String> = roles.into_iter().collect();
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
    let perms = permissions_of(ctx, uid).await?;
    let ok = perms.iter().any(|x| x == perm);
    if !ok {
        tracing::debug!(user = uid, permission = perm, "permission denied");
    }
    Ok(ok)
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
    }
}
