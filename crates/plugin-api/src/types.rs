//! 调用上下文中的基础类型（plugin-api.md §3、kernel/03 §3.3～3.4）。

use serde::{Deserialize, Serialize};

pub type UserId = u64;
/// Unix 毫秒。
pub type TimestampMs = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorKind {
    Anonymous,
    User,
    System,
    Plugin,
    Mcp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actor {
    pub kind: ActorKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<UserId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_id: Option<String>,
}

impl Actor {
    pub fn anonymous() -> Self {
        Self {
            kind: ActorKind::Anonymous,
            user_id: None,
            plugin_id: None,
            credential_id: None,
        }
    }
    pub fn system() -> Self {
        Self {
            kind: ActorKind::System,
            user_id: None,
            plugin_id: None,
            credential_id: None,
        }
    }
    pub fn plugin(id: &str) -> Self {
        Self {
            kind: ActorKind::Plugin,
            user_id: None,
            plugin_id: Some(id.to_owned()),
            credential_id: None,
        }
    }
    pub fn user(id: UserId, credential_id: Option<String>) -> Self {
        Self {
            kind: ActorKind::User,
            user_id: Some(id),
            plugin_id: None,
            credential_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationKind {
    Init,
    Hook,
    Event,
    Cron,
    Http,
    Mcp,
    Service,
    Search,
    ContentType,
    Metadata,
    Worker,
}

impl InvocationKind {
    /// 导出调用（由内核发起、不形成依赖边，参与重入检测，见 kernel/03 §3.3）。
    pub fn is_export(self) -> bool {
        matches!(
            self,
            Self::Hook | Self::Search | Self::ContentType | Self::Metadata | Self::Mcp
        )
    }
}

/// 网关鉴权得到的主体（由 `ctx.auth` 提供方返回）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    pub user_id: UserId,
    /// 会话 ID 或 token ID
    pub credential_id: String,
    /// `session` | `token` | `mcp` | `feed`
    pub credential_kind: String,
    /// token 作用域；会话为 `["*"]`
    #[serde(default)]
    pub scopes: Vec<String>,
    /// 最近一次重新验证身份的时间（`reauth_secs` 路由使用）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reauth_at: Option<TimestampMs>,
    /// 当前用户状态（active / leech_disabled / ...），供网关与插件快速判断
    #[serde(default)]
    pub status: String,
    /// 用户等级（class），网关按 `[http.rate_limit] class_multiplier` 放宽限流；未知时为 None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_id: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvocationInfo {
    pub kind: InvocationKind,
    pub actor: Actor,
    /// 发起本次调用的插件（网关 / 内核为 "kernel"）
    pub caller_plugin: String,
    pub caller_scopes: Vec<String>,
    pub traceparent: String,
    pub tracestate: Option<String>,
    pub locale: String,
    pub deadline: TimestampMs,
    pub depth: u8,
    /// 重入检测：本调用链上正在执行服务方法或发起导出调用的插件
    pub chain: Vec<String>,
    /// 本调用链中经 `ctx.auth.authenticate` 认证过的主体（`with_principal` 只接受这些）
    #[serde(default)]
    pub authenticated: Vec<UserId>,
    /// 是否已切换过 actor（不能切回、不能二次切换）
    #[serde(default)]
    pub principal_switched: bool,
    pub request_id: String,
    /// 仅 HTTP 调用：客户端 IP（网关按 `http.trusted_proxies` 解析 X-Forwarded-For）。敏感数据，不得写入事件。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<String>,
    /// 仅 HTTP 调用：User-Agent（截断到 256 字节）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
}

impl InvocationInfo {
    pub fn user_id(&self) -> Option<UserId> {
        self.actor.user_id
    }

    pub fn has_scope(&self, scope: &str) -> bool {
        self.caller_scopes.iter().any(|s| s == scope || s == "*")
    }
}
