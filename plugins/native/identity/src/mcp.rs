//! MCP 工具（catalog/mcp-tools.md）：`get_my_profile`、`admin_set_user_status`。

use async_trait::async_trait;
use pt_plugin_api::{Ctx, McpTool, PResult, PluginError, json};
use serde_json::Value;

use crate::{Identity, rbac, status};

pub struct Tools(pub std::sync::Arc<Identity>);

#[async_trait]
impl McpTool for Tools {
    async fn call(&self, ctx: &Ctx, tool: &str, a: Value) -> PResult<Value> {
        let uid = i64::try_from(ctx.require_user()?).map_err(|_| PluginError::bad_request("bad user id"))?;
        match tool {
            "get_my_profile" => {
                let u = self
                    .0
                    .summary(ctx, uid)
                    .await?
                    .ok_or_else(|| PluginError::not_found("user not found"))?;
                let (_, roles) = rbac::effective_roles(ctx, uid).await?;
                let mut roles: Vec<String> = roles.into_iter().collect();
                roles.sort();
                Ok(
                    json!({ "id": u.id, "username": u.username, "class_id": u.class_id, "status": u.status, "title": u.title,
                           "roles": roles, "created_at": u.created_at }),
                )
            }
            "admin_set_user_status" => {
                let target = a["user_id"]
                    .as_i64()
                    .ok_or_else(|| PluginError::validation("user_id"))?;
                let kind = status::kind_of(a["status"].as_str().unwrap_or(""))
                    .filter(|k| *k != "pending")
                    .ok_or_else(|| PluginError::validation("status must be banned or leech_disabled"))?;
                let by = crate::http::admin_rank_ok(ctx, target).await?;
                let reason = a["reason_code"].as_str().unwrap_or("other");
                let rid = status::apply(
                    ctx,
                    &status::Apply {
                        user_id: target,
                        kind,
                        source: "admin",
                        reason_code: reason,
                        note: a["note"].as_str(),
                        until_ms: a["until"].as_i64(),
                        by_user: Some(by),
                    },
                )
                .await?;
                ctx.audit
                    .write(pt_plugin_api::AuditEntry {
                        action: "identity.restriction_apply".into(),
                        target: format!("user:{target}"),
                        details: json!({ "restriction_id": rid, "kind": kind, "reason_code": reason, "via": "mcp" }),
                    })
                    .await?;
                let s = self.0.summary(ctx, target).await?;
                Ok(json!({ "restriction_id": rid, "user_id": target, "status": s.map(|s| s.status) }))
            }
            other => Err(PluginError::not_found(format!("unknown tool `{other}`"))),
        }
    }
}
