//! 宿主接口（内核实现，插件通过 [`crate::Ctx`] 使用）。plugin-api.md §3。

use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value as Json;

use crate::{PResult, TimestampMs};

#[async_trait]
pub trait Config: Send + Sync {
    /// site.toml `[plugin.<id>]`，另含内核注入的只读 `_site`（name、base_url、timezone、default_locale）
    fn get_static(&self) -> Json;
    /// 后台可改的设置（缓存，最多滞后数秒）
    async fn get_settings(&self) -> Json;
}

#[async_trait]
pub trait Kv: Send + Sync {
    async fn get(&self, key: &str) -> PResult<Option<Vec<u8>>>;
    async fn set(&self, key: &str, value: &[u8], expires_at: Option<TimestampMs>) -> PResult<()>;
    async fn delete(&self, key: &str) -> PResult<bool>;
    async fn compare_and_set(&self, key: &str, expected: Option<&[u8]>, value: &[u8]) -> PResult<bool>;
    async fn list_keys(&self, prefix: &str, cursor: Option<&str>, limit: u32) -> PResult<Vec<String>>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    Null,
    Boolean(bool),
    Int(i64),
    Float(f64),
    Numeric(String),
    Text(String),
    Bytes(Vec<u8>),
    Json(Json),
    /// Unix 微秒
    Timestamp(i64),
    Uuid(uuid::Uuid),
    IntArray(Vec<i64>),
    TextArray(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub type_name: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RowSet {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<SqlValue>>,
    pub truncated: bool,
}

pub type PgTx = sqlx::Transaction<'static, sqlx::Postgres>;

/// 仅 Native：工作单元事务的直接访问（已 `SET LOCAL ROLE` 到本插件角色、`search_path` 为本插件 schema）。
/// 通过 `&mut *conn` 得到 `&mut PgConnection`，可直接使用 sqlx。
#[derive(Debug)]
pub struct SqlConn<'a> {
    pub(crate) guard: tokio::sync::MutexGuard<'a, Option<PgTx>>,
}

impl<'a> SqlConn<'a> {
    #[doc(hidden)]
    pub fn new(guard: tokio::sync::MutexGuard<'a, Option<PgTx>>) -> Option<Self> {
        guard.is_some().then_some(Self { guard })
    }
}

impl std::ops::Deref for SqlConn<'_> {
    type Target = sqlx::PgConnection;
    fn deref(&self) -> &Self::Target {
        // SqlConn::new 保证 Some
        match self.guard.as_ref() {
            Some(tx) => tx,
            None => unreachable!("SqlConn constructed without transaction"),
        }
    }
}

impl std::ops::DerefMut for SqlConn<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self.guard.as_mut() {
            Some(tx) => tx,
            None => unreachable!("SqlConn constructed without transaction"),
        }
    }
}

#[async_trait]
pub trait Sql: Send + Sync {
    async fn query(&self, stmt: &str, params: &[SqlValue]) -> PResult<RowSet>;
    async fn execute(&self, stmt: &str, params: &[SqlValue]) -> PResult<u64>;
    async fn execute_batch(&self, stmt: &str, param_sets: &[Vec<SqlValue>]) -> PResult<u64>;
    /// 仅 `tx = "manual"` 的调用：提交当前事务（连同已发布事件）并开始新事务
    async fn commit(&self) -> PResult<()>;
    /// 仅 Native：工作单元事务（首次调用时开始）。
    async fn conn(&self) -> PResult<SqlConn<'_>>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum RedisReply {
    Nil,
    Ok,
    Int(i64),
    Double(f64),
    Bulk(Vec<u8>),
    Simple(String),
    Array(Vec<RedisReply>),
}

impl RedisReply {
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            Self::Bulk(b) => std::str::from_utf8(b).ok()?.parse().ok(),
            _ => None,
        }
    }
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bulk(b) => Some(b),
            Self::Simple(s) => Some(s.as_bytes()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedisCommand {
    pub name: String,
    pub args: Vec<Vec<u8>>,
}

impl RedisCommand {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            args: Vec::new(),
        }
    }
    #[must_use]
    pub fn arg(mut self, a: impl AsRef<[u8]>) -> Self {
        self.args.push(a.as_ref().to_vec());
        self
    }
}

#[async_trait]
pub trait Redis: Send + Sync {
    /// 私有前缀 `p:<plugin_id>:`（内核改写键名、检查白名单）
    async fn call(&self, cmd: RedisCommand) -> PResult<RedisReply>;
    async fn pipeline(&self, cmds: Vec<RedisCommand>, atomic: bool) -> PResult<Vec<RedisReply>>;
    /// 仅 Native 且获授 `redis_global`：实例角色的原生连接（不改写键名）
    fn global(&self, role: &str) -> PResult<redis::aio::ConnectionManager>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewEvent {
    pub event_type: String,
    pub version: u32,
    pub key: String,
    pub payload: Json,
}

impl NewEvent {
    pub fn v1(event_type: &str, key: impl Into<String>, payload: Json) -> Self {
        Self {
            event_type: event_type.to_owned(),
            version: 1,
            key: key.into(),
            payload,
        }
    }
}

#[async_trait]
pub trait Events: Send + Sync {
    /// 写入本工作单元，随事务提交（outbox）
    async fn publish(&self, evt: NewEvent) -> PResult<uuid::Uuid>;
}

#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn send(
        &self,
        req: http::Request<bytes::Bytes>,
        timeout: Duration,
    ) -> PResult<http::Response<bytes::Bytes>>;
}

#[async_trait]
pub trait Blob: Send + Sync {
    async fn put(&self, key: &str, content_type: &str, data: bytes::Bytes) -> PResult<()>;
    async fn get(&self, key: &str) -> PResult<Option<bytes::Bytes>>;
    async fn delete(&self, key: &str) -> PResult<()>;
    async fn presign_get(&self, key: &str, ttl_secs: u32) -> PResult<String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    pub key: String,
    pub name: String,
    pub short_name: Option<String>,
    pub unit: Option<String>,
    pub icon: Option<String>,
}

#[async_trait]
pub trait Terms: Send + Sync {
    async fn get(&self, key: &str, locale: Option<&str>) -> Option<Term>;
    async fn get_many(&self, keys: &[&str], locale: Option<&str>) -> Vec<Term>;
    async fn render(&self, template: &str, args: &Json, locale: Option<&str>) -> PResult<String>;
}

pub trait Time: Send + Sync {
    fn now(&self) -> TimestampMs;
}

pub trait Random: Send + Sync {
    fn bytes(&self, len: usize) -> Vec<u8>;
    fn uuid_v7(&self) -> uuid::Uuid;
}

#[derive(Debug, Clone, PartialEq)]
pub struct AuditEntry {
    pub action: String,
    pub target: String,
    pub details: Json,
}

#[async_trait]
pub trait Audit: Send + Sync {
    /// 与工作单元同事务写入 `kernel.audit_log`
    async fn write(&self, e: AuditEntry) -> PResult<()>;
}

#[async_trait]
pub trait Jobs: Send + Sync {
    async fn schedule_once(
        &self,
        name: &str,
        run_at: TimestampMs,
        payload: Json,
        dedupe_key: Option<&str>,
    ) -> PResult<String>;
    async fn cancel(&self, job_id: &str) -> PResult<bool>;
}

pub trait Metrics: Send + Sync {
    fn counter_add(&self, name: &str, v: u64, labels: &[(&str, &str)]);
    fn gauge_set(&self, name: &str, v: f64, labels: &[(&str, &str)]);
    fn histogram_observe(&self, name: &str, v: f64, labels: &[(&str, &str)]);
}

#[derive(Debug, Clone, PartialEq)]
pub enum HookOutcome {
    Allowed(Json),
    Denied {
        code: String,
        message: String,
        details: Option<Json>,
        by_plugin: String,
    },
}

#[async_trait]
pub trait HookRunner: Send + Sync {
    /// 只有钩子点拥有者能触发
    async fn run(&self, hook: &str, input: Json) -> PResult<HookOutcome>;
}
