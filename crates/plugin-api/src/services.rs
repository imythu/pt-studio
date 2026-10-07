//! 内核直接调用的服务契约（`ctx.auth`、`ctx.rbac`、`ctx.terms`）。
//! 其他业务服务由各插件 crate 自己定义 trait（plugin-api.md §3.2）。

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{Credential, Ctx, PResult, Principal};

pub const AUTH: &str = "ctx.auth";
pub const RBAC: &str = "ctx.rbac";

/// `ctx.auth`：网关鉴权（identity 提供）。
#[async_trait]
pub trait AuthService: Send + Sync {
    /// 无效、过期、被撤销、账号被封禁 → `Ok(None)`
    async fn authenticate(&self, ctx: &Ctx, credential: Credential) -> PResult<Option<Principal>>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionDecl {
    pub plugin_id: String,
    pub name: String,
    #[serde(default)]
    pub description_term: Option<String>,
    #[serde(default)]
    pub default_roles: Vec<String>,
}

/// `ctx.rbac`：权限检查（identity 提供）。
#[async_trait]
pub trait RbacService: Send + Sync {
    /// 启动时由内核调用一次，登记全部插件的权限点（幂等）
    async fn register_permissions(&self, ctx: &Ctx, perms: Vec<PermissionDecl>) -> PResult<()>;
    /// 网关对每个请求调用（评审 R1 B-06）：`permission = None` 时只检查 token 作用域与方法限制。
    /// `method` 为 HTTP 方法（只读 token 只能 GET/HEAD）。
    async fn check_principal(
        &self,
        ctx: &Ctx,
        principal: &Principal,
        permission: Option<&str>,
        method: &str,
    ) -> PResult<bool>;
    /// 以会话身份检查某用户是否拥有权限点（插件做可见性判断时使用）
    async fn check(&self, ctx: &Ctx, user_id: u64, permission: &str) -> PResult<bool>;
}
