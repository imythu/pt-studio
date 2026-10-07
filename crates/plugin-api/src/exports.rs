//! 导出接口（插件实现，内核调用）与 Native 插件入口。plugin-api.md §3。

use std::{any::Any, sync::Arc};

use async_trait::async_trait;
use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use serde_json::{Value as Json, json};

use crate::{Actor, Ctx, PResult, PluginError, TimestampMs};

#[async_trait]
pub trait Lifecycle: Send + Sync {
    async fn init(&self, ctx: &Ctx) -> PResult<()>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct HookCall {
    pub hook: String,
    pub input: Json,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HookDecision {
    Allow,
    Deny {
        code: String,
        message: String,
        details: Option<Json>,
    },
    /// JSON Merge Patch，只能触及钩子点登记的可修改字段
    Modify(Json),
}

#[async_trait]
pub trait Hooks: Send + Sync {
    async fn handle(&self, ctx: &Ctx, call: HookCall) -> PResult<HookDecision>;
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EventEnvelope {
    pub id: uuid::Uuid,
    #[serde(rename = "type")]
    pub event_type: String,
    pub version: u32,
    pub occurred_at: TimestampMs,
    pub actor: Actor,
    pub source: String,
    pub key: String,
    pub payload: Json,
    #[serde(default)]
    pub traceparent: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryInfo {
    pub subscription: String,
    pub attempt: u32,
    pub replay: bool,
}

#[async_trait]
pub trait OnEvent: Send + Sync {
    async fn handle(&self, ctx: &Ctx, evt: EventEnvelope, d: DeliveryInfo) -> PResult<()>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Cron,
    Delayed,
    Manual,
}

#[derive(Debug, Clone, PartialEq)]
pub struct JobInvocation {
    pub name: String,
    pub kind: JobKind,
    pub scheduled_at: TimestampMs,
    pub payload: Json,
    pub run_id: String,
}

#[async_trait]
pub trait OnCron: Send + Sync {
    async fn run(&self, ctx: &Ctx, job: JobInvocation) -> PResult<()>;
}

/// Native 插件的 HTTP：返回挂在 `/api/p/<plugin_id>` 下的 axum Router。
/// 内核按清单路由声明执行鉴权、权限、限流；未声明的路由返回 404。
/// handler 用 [`RequestCtx`] 取得本次请求的上下文。
pub trait HttpRoutes: Send + Sync {
    fn router(&self) -> axum::Router;
}

/// 请求上下文提取器：内核在调用 handler 前放入请求扩展。
#[derive(Debug, Clone)]
pub struct RequestCtx(pub Arc<Ctx>);

impl std::ops::Deref for RequestCtx {
    type Target = Ctx;
    fn deref(&self) -> &Ctx {
        &self.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for RequestCtx {
    type Rejection = PluginError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<RequestCtx>()
            .cloned()
            .ok_or_else(|| PluginError::internal("request context missing (route not declared?)"))
    }
}

/// 响应扩展：标记本次响应来自插件错误（内核据此回滚工作单元并补全 problem+json）。
#[derive(Debug, Clone)]
pub struct ErrorMarker(pub PluginError);

impl IntoResponse for PluginError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = if status.is_server_error() {
            json!({ "type": format!("https://pt-studio.dev/errors/{}", self.code), "title": "internal error",
                    "status": status.as_u16(), "code": self.code })
        } else {
            let mut b = json!({ "type": format!("https://pt-studio.dev/errors/{}", self.code),
                                "title": self.message, "status": status.as_u16(), "code": self.code });
            if let Some(d) = &self.details {
                b["details"] = d.clone();
                if let Some(errs) = d.get("errors") {
                    b["errors"] = errs.clone();
                }
            }
            b
        };
        let mut resp = (
            status,
            [(http::header::CONTENT_TYPE, "application/problem+json")],
            body.to_string(),
        )
            .into_response();
        resp.extensions_mut().insert(ErrorMarker(self));
        resp
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldSpec {
    pub name: String,
    pub field_type: String,
    pub searchable: bool,
    pub filterable: bool,
    pub sortable: bool,
    pub rank: u8,
    pub exact: bool,
    pub label_term: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndexSchema {
    pub index: String,
    pub fields: Vec<FieldSpec>,
    pub synonyms: Vec<Vec<String>>,
}

#[async_trait]
pub trait SearchContributor: Send + Sync {
    async fn index_schemas(&self, ctx: &Ctx) -> Vec<IndexSchema>;
    async fn to_documents(
        &self,
        ctx: &Ctx,
        index: &str,
        entity_ids: &[u64],
    ) -> PResult<Vec<(u64, Option<Json>)>>;
}

#[async_trait]
pub trait McpTool: Send + Sync {
    async fn call(&self, ctx: &Ctx, tool: &str, arguments: Json) -> PResult<Json>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeInfo {
    pub type_id: String,
    pub name_term: String,
    pub field_schema: Json,
    pub group_schema: Option<Json>,
}

#[async_trait]
pub trait ContentType: Send + Sync {
    async fn info(&self, ctx: &Ctx) -> TypeInfo;
    async fn normalize(&self, ctx: &Ctx, fields: Json) -> PResult<Json>;
    async fn group_key(&self, ctx: &Ctx, fields: &Json) -> PResult<Option<String>>;
    async fn canonical_title(&self, ctx: &Ctx, fields: &Json) -> PResult<String>;
}

/// 分区 worker 一次轮询的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollOutcome {
    /// 处理了数据，立即再次轮询
    Busy,
    /// 没有新数据，`sleep_ms` 后再轮询
    Idle { sleep_ms: u64 },
}

/// Native 分区 worker（清单 `[[workers]]`，kernel/03 §9.4、accounting HLD §7 N-1）。
/// 内核持有分区租约期间循环调用 `poll`；每次调用一个工作单元（返回 Ok 时提交）。
/// `fence` 为租约的 fencing token（单调递增），插件应与自己的游标一起检查，防止旧持有者写入。
#[async_trait]
pub trait PartitionWorker: Send + Sync {
    async fn poll(&self, ctx: &Ctx, partition: u32, fence: u64) -> PResult<PollOutcome>;
}

/// Native 插件入口：每个 Native 插件 crate 导出 `pub fn plugin() -> NativePlugin`。
#[derive(Default)]
pub struct NativePlugin {
    /// plugin.toml 原文（`include_str!`）
    pub manifest: &'static str,
    /// 迁移文件 (文件名, SQL)
    pub migrations: &'static [(&'static str, &'static str)],
    pub lifecycle: Option<Arc<dyn Lifecycle>>,
    pub hooks: Option<Arc<dyn Hooks>>,
    pub on_event: Option<Arc<dyn OnEvent>>,
    pub on_cron: Option<Arc<dyn OnCron>>,
    pub http: Option<Arc<dyn HttpRoutes>>,
    pub search_contributor: Option<Arc<dyn SearchContributor>>,
    pub mcp_tool: Option<Arc<dyn McpTool>>,
    pub content_type: Option<Arc<dyn ContentType>>,
    pub workers: Vec<(&'static str, Arc<dyn PartitionWorker>)>,
    /// 服务名 → 类型擦除的实现（[`crate::service_object`]）
    pub services: Vec<(&'static str, Arc<dyn Any + Send + Sync>)>,
}

impl std::fmt::Debug for NativePlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativePlugin")
            .field("services", &self.services.iter().map(|s| s.0).collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}
