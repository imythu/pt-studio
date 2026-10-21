//! server-load 插件（WASM）：首页侧栏的服务器负载。
//!
//! - `GET /v1/status`：注册用户数、用户上限（本插件设置 `user_limit`，为 0 时取 `ctx.registration` 的全站用户上限）、
//!   今日新注册、近一小时活跃用户（有活跃连接的用户）与连接数（做种 / 下载）；
//! - 可见范围：设置 `visible_classes`（等级 ID）与 `visible_roles`（角色）都为空时所有登录用户可见；
//!   否则用户的等级在列表中、或拥有列表中的任一角色即可查看（满足其一）；管理员始终可见。
//!   无权查看时返回 `{ "visible": false }`（不报错，前端据此不显示组件）。

pt_plugin_sdk::world!("server-load", http);

use exports::pt::plugin::http_handler;
use pt_plugin_sdk::{prelude::*, serde_json, sql};

struct ServerLoad;

/// 后台设置
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Settings {
    pub user_limit: i64,
    pub classes: Vec<i64>,
    pub roles: Vec<String>,
}

impl Settings {
    pub fn from_json(s: &Value) -> Self {
        Self {
            user_limit: s["user_limit"].as_i64().unwrap_or(0).max(0),
            classes: s["visible_classes"].as_array().into_iter().flatten().filter_map(Value::as_i64).collect(),
            roles: s["visible_roles"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
        }
    }
}

/// 管理员始终可见
const STAFF_ROLES: &[&str] = &["admin", "sysop"];

/// 可见性判定（纯函数）：未设限制时所有人可见；否则等级或角色满足其一
pub fn visible(s: &Settings, class_id: Option<i64>, roles: &[String]) -> bool {
    if roles.iter().any(|r| STAFF_ROLES.contains(&r.as_str())) {
        return true;
    }
    if s.classes.is_empty() && s.roles.is_empty() {
        return true;
    }
    class_id.is_some_and(|c| s.classes.contains(&c)) || roles.iter().any(|r| s.roles.contains(r))
}

fn count(q: &str, args: &[SqlValue]) -> R<i64> {
    Ok(sql::query(q, args)?.rows.first().and_then(|r| r.int(0)).unwrap_or(0))
}

fn status() -> R<Value> {
    let uid = ctx::current_user()?;
    let s = Settings::from_json(&ctx::settings());
    let class_id = sql::query("SELECT class_id FROM identity.v_user_public WHERE id = $1", &[SqlValue::Int(uid)])?
        .rows
        .first()
        .and_then(|r| r.int(0));
    let roles: Vec<String> = if s.classes.is_empty() && s.roles.is_empty() {
        Vec::new()
    } else {
        ctx::call("ctx.rbac", "roles_of", &json!({ "user": uid }))
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    };
    if !visible(&s, class_id, &roles) {
        return Ok(json!({ "visible": false }));
    }
    let users = count("SELECT count(*) FROM identity.v_user_public WHERE status <> 'deleted'", &[])?;
    let today = count(
        "SELECT count(*) FROM identity.v_user_public WHERE status <> 'deleted' AND created_at >= date_trunc('day', now())",
        &[],
    )?;
    let limit = if s.user_limit > 0 {
        s.user_limit
    } else {
        // 注册策略未安装或调用失败时视为不限
        ctx::call("ctx.registration", "status", &json!({}))
            .ok()
            .and_then(|v| v["max_users"].as_i64())
            .unwrap_or(0)
    };
    let peers = sql::query(
        "SELECT count(DISTINCT user_id), count(*) FILTER (WHERE is_seeder), count(*) FILTER (WHERE NOT is_seeder) FROM accounting.v_swarm_peers",
        &[],
    )?;
    let p = peers.rows.first();
    let get = |i: usize| p.and_then(|r| r.int(i)).unwrap_or(0);
    Ok(json!({
        "visible": true,
        "users": users,
        "user_limit": (limit > 0).then_some(limit),
        "new_today": today,
        "active_users": get(0),
        "seeders": get(1),
        "leechers": get(2),
    }))
}

impl http_handler::Guest for ServerLoad {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let reply = match req.route_id.as_str() {
            "status" => http::Reply::ok(&status()?),
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(ServerLoad);

#[cfg(test)]
mod tests {
    use super::*;

    fn roles(r: &[&str]) -> Vec<String> {
        r.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn visibility() {
        let open = Settings::default();
        assert!(visible(&open, Some(1), &roles(&["user"])));
        let s = Settings { user_limit: 0, classes: vec![3, 4], roles: roles(&["uploader"]) };
        // 等级满足
        assert!(visible(&s, Some(3), &roles(&["user"])));
        // 角色满足
        assert!(visible(&s, Some(1), &roles(&["user", "uploader"])));
        // 都不满足
        assert!(!visible(&s, Some(1), &roles(&["user"])));
        assert!(!visible(&s, None, &[]));
        // 管理员始终可见
        assert!(visible(&s, Some(1), &roles(&["admin"])));
    }

    #[test]
    fn settings_parse() {
        let s = Settings::from_json(&json!({ "user_limit": 5000, "visible_classes": [2, "x", 5], "visible_roles": ["moderator"] }));
        assert_eq!(s, Settings { user_limit: 5000, classes: vec![2, 5], roles: roles(&["moderator"]) });
        assert_eq!(Settings::from_json(&Value::Null), Settings::default());
    }
}
