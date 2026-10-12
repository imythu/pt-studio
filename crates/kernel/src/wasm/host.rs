//! WASM 宿主接口实现（plugin-api.md §2）：每个导入都转发到本次调用的 [`Ctx`]，
//! 因此能力检查、工作单元、事件 outbox 与 Native 插件完全一致。

use pt_plugin_api::{
    ActorKind, AuditEntry, Ctx, InvocationKind, NewEvent, PluginError, RedisCommand, RedisReply, SqlValue,
    host::{HookOutcome, RowSet},
};
use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

use super::bindings::pt::plugin as w;

/// 每次调用一个 Store，Store 的数据即本结构。
pub struct HostState {
    pub ctx: Ctx,
    pub wasi: WasiCtx,
    pub table: ResourceTable,
    pub limits: wasmtime::StoreLimits,
}

impl std::fmt::Debug for HostState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostState")
            .field("plugin", &self.ctx.plugin_id)
            .finish_non_exhaustive()
    }
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

// ---------------- 类型转换 ----------------

pub fn to_wit_err(e: PluginError) -> w::types::PluginError {
    w::types::PluginError {
        code: e.code,
        message: e.message,
        details: e.details.map(|d| d.to_string()),
        retryable: e.retryable,
    }
}

pub fn from_wit_err(e: w::types::PluginError) -> PluginError {
    PluginError {
        code: e.code,
        message: e.message,
        details: e.details.and_then(|d| serde_json::from_str(&d).ok()),
        retryable: e.retryable,
        status: None,
    }
}

type WResult<T> = Result<T, w::types::PluginError>;

fn wr<T>(r: pt_plugin_api::PResult<T>) -> WResult<T> {
    r.map_err(to_wit_err)
}

fn parse_json(s: &str) -> WResult<serde_json::Value> {
    serde_json::from_str(s)
        .map_err(|e| to_wit_err(PluginError::bad_request(format!("invalid JSON from plugin: {e}"))))
}

pub fn wit_actor(a: &pt_plugin_api::Actor) -> w::types::Actor {
    w::types::Actor {
        kind: match a.kind {
            ActorKind::Anonymous => w::types::ActorKind::Anonymous,
            ActorKind::User => w::types::ActorKind::User,
            ActorKind::System => w::types::ActorKind::System,
            ActorKind::Plugin => w::types::ActorKind::Plugin,
            ActorKind::Mcp => w::types::ActorKind::Mcp,
        },
        user_id: a.user_id,
        plugin_id: a.plugin_id.clone(),
        credential_id: a.credential_id.clone(),
    }
}

fn to_sql(v: w::sql::SqlValue) -> WResult<SqlValue> {
    use w::sql::SqlValue as W;
    Ok(match v {
        W::Null => SqlValue::Null,
        W::Boolean(b) => SqlValue::Boolean(b),
        W::Int(i) => SqlValue::Int(i),
        W::Float(f) => SqlValue::Float(f),
        W::Numeric(s) => SqlValue::Numeric(s),
        W::Text(s) => SqlValue::Text(s),
        W::Bytes(b) => SqlValue::Bytes(b),
        W::Json(s) => SqlValue::Json(parse_json(&s)?),
        W::Timestamp(t) => SqlValue::Timestamp(t),
        W::Uuid(s) => SqlValue::Uuid(
            uuid::Uuid::parse_str(&s)
                .map_err(|_| to_wit_err(PluginError::bad_request("invalid uuid parameter")))?,
        ),
        W::IntArray(a) => SqlValue::IntArray(a),
        W::TextArray(a) => SqlValue::TextArray(a),
    })
}

fn from_sql(v: SqlValue) -> w::sql::SqlValue {
    use w::sql::SqlValue as W;
    match v {
        SqlValue::Null => W::Null,
        SqlValue::Boolean(b) => W::Boolean(b),
        SqlValue::Int(i) => W::Int(i),
        SqlValue::Float(f) => W::Float(f),
        SqlValue::Numeric(s) => W::Numeric(s),
        SqlValue::Text(s) => W::Text(s),
        SqlValue::Bytes(b) => W::Bytes(b),
        SqlValue::Json(j) => W::Json(j.to_string()),
        SqlValue::Timestamp(t) => W::Timestamp(t),
        SqlValue::Uuid(u) => W::Uuid(u.to_string()),
        SqlValue::IntArray(a) => W::IntArray(a),
        SqlValue::TextArray(a) => W::TextArray(a),
    }
}

fn params(p: Vec<w::sql::SqlValue>) -> WResult<Vec<SqlValue>> {
    p.into_iter().map(to_sql).collect()
}

fn from_rowset(r: RowSet) -> w::sql::RowSet {
    w::sql::RowSet {
        columns: r
            .columns
            .into_iter()
            .map(|c| w::sql::Column {
                name: c.name,
                type_name: c.type_name,
            })
            .collect(),
        rows: r
            .rows
            .into_iter()
            .map(|row| row.into_iter().map(from_sql).collect())
            .collect(),
        truncated: r.truncated,
    }
}

fn from_reply(r: RedisReply) -> w::redis::RedisReply {
    use w::redis::{RedisReply as W, RedisScalar as S};
    match r {
        RedisReply::Nil => W::Nil,
        RedisReply::Ok => W::Ok,
        RedisReply::Int(i) => W::Int(i),
        RedisReply::Double(d) => W::Double(d),
        RedisReply::Bulk(b) => W::Bulk(b),
        RedisReply::Simple(s) => W::Simple(s),
        RedisReply::Array(items) => {
            // 一层数组；含嵌套时以 JSON 编码放入 nested（SDK 解析）
            let flat: Option<Vec<S>> = items
                .iter()
                .map(|x| match x {
                    RedisReply::Nil => Some(S::Nil),
                    RedisReply::Int(i) => Some(S::Int(*i)),
                    RedisReply::Bulk(b) => Some(S::Bulk(b.clone())),
                    RedisReply::Simple(s) => Some(S::Bulk(s.clone().into_bytes())),
                    _ => None,
                })
                .collect();
            match flat {
                Some(f) => W::Array(f),
                None => {
                    W::Nested(serde_json::to_vec(&reply_json(&RedisReply::Array(items))).unwrap_or_default())
                }
            }
        }
    }
}

fn reply_json(r: &RedisReply) -> serde_json::Value {
    match r {
        RedisReply::Nil => serde_json::Value::Null,
        RedisReply::Ok => serde_json::json!("OK"),
        RedisReply::Int(i) => serde_json::json!(i),
        RedisReply::Double(d) => serde_json::json!(d),
        RedisReply::Bulk(b) => serde_json::json!(String::from_utf8_lossy(b)),
        RedisReply::Simple(s) => serde_json::json!(s),
        RedisReply::Array(a) => serde_json::Value::Array(a.iter().map(reply_json).collect()),
    }
}

fn cmd(c: w::redis::Command) -> RedisCommand {
    RedisCommand {
        name: c.name,
        args: c.args,
    }
}

fn unavailable(what: &str) -> w::types::PluginError {
    to_wit_err(PluginError::unavailable(format!(
        "{what} is not available in this build"
    )))
}

// ---------------- 接口实现 ----------------

impl w::types::Host for HostState {}

impl w::invocation::Host for HostState {
    async fn current(&mut self) -> w::types::InvocationInfo {
        let i = &self.ctx.invocation;
        w::types::InvocationInfo {
            kind: match i.kind {
                InvocationKind::Init => w::types::InvocationKind::Init,
                InvocationKind::Hook => w::types::InvocationKind::Hook,
                InvocationKind::Event => w::types::InvocationKind::Event,
                InvocationKind::Cron | InvocationKind::Worker => w::types::InvocationKind::Cron,
                InvocationKind::Http => w::types::InvocationKind::Http,
                InvocationKind::Mcp => w::types::InvocationKind::Mcp,
                InvocationKind::Service => w::types::InvocationKind::Service,
                InvocationKind::Search => w::types::InvocationKind::Search,
                InvocationKind::ContentType => w::types::InvocationKind::ContentType,
                InvocationKind::Metadata => w::types::InvocationKind::Metadata,
            },
            actor: wit_actor(&i.actor),
            caller_plugin: i.caller_plugin.clone(),
            caller_scopes: i.caller_scopes.clone(),
            trace: w::types::TraceContext {
                traceparent: i.traceparent.clone(),
                tracestate: i.tracestate.clone(),
            },
            locale: i.locale.clone(),
            deadline: i.deadline,
            depth: i.depth,
        }
    }
}

impl w::log::Host for HostState {
    async fn emit(&mut self, level: w::log::Level, message: String, fields: Vec<w::log::Field>) {
        let plugin = &*self.ctx.plugin_id;
        let fields: Vec<String> = fields.iter().map(|f| format!("{}={}", f.key, f.value)).collect();
        let fields = fields.join(" ");
        match level {
            w::log::Level::Trace => tracing::trace!(plugin, %fields, "{message}"),
            w::log::Level::Debug => tracing::debug!(plugin, %fields, "{message}"),
            w::log::Level::Info => tracing::info!(plugin, %fields, "{message}"),
            w::log::Level::Warn => tracing::warn!(plugin, %fields, "{message}"),
            w::log::Level::Error => tracing::error!(plugin, %fields, "{message}"),
        }
    }
}

impl w::config::Host for HostState {
    async fn get_static(&mut self) -> String {
        self.ctx.config.get_static().to_string()
    }
    async fn get_settings(&mut self) -> String {
        self.ctx.config.get_settings().await.to_string()
    }
}

impl w::kv::Host for HostState {
    async fn get(&mut self, key: String) -> WResult<Option<Vec<u8>>> {
        wr(self.ctx.kv.get(&key).await)
    }
    async fn set(&mut self, key: String, value: Vec<u8>, expires_at: Option<i64>) -> WResult<()> {
        wr(self.ctx.kv.set(&key, &value, expires_at).await)
    }
    async fn delete(&mut self, key: String) -> WResult<bool> {
        wr(self.ctx.kv.delete(&key).await)
    }
    async fn compare_and_set(
        &mut self,
        key: String,
        expected: Option<Vec<u8>>,
        value: Vec<u8>,
    ) -> WResult<bool> {
        wr(self
            .ctx
            .kv
            .compare_and_set(&key, expected.as_deref(), &value)
            .await)
    }
    async fn list_keys(
        &mut self,
        prefix: String,
        cursor: Option<String>,
        limit: u32,
    ) -> WResult<Vec<String>> {
        wr(self.ctx.kv.list_keys(&prefix, cursor.as_deref(), limit).await)
    }
}

impl w::sql::Host for HostState {
    async fn query(&mut self, statement: String, p: Vec<w::sql::SqlValue>) -> WResult<w::sql::RowSet> {
        let p = params(p)?;
        wr(self.ctx.sql.query(&statement, &p).await).map(from_rowset)
    }
    async fn execute(&mut self, statement: String, p: Vec<w::sql::SqlValue>) -> WResult<u64> {
        let p = params(p)?;
        wr(self.ctx.sql.execute(&statement, &p).await)
    }
    async fn execute_batch(&mut self, statement: String, sets: Vec<Vec<w::sql::SqlValue>>) -> WResult<u64> {
        let sets = sets.into_iter().map(params).collect::<WResult<Vec<_>>>()?;
        wr(self.ctx.sql.execute_batch(&statement, &sets).await)
    }
    async fn commit(&mut self) -> WResult<()> {
        wr(self.ctx.sql.commit().await)
    }
}

impl w::redis::Host for HostState {
    async fn call(&mut self, c: w::redis::Command) -> WResult<w::redis::RedisReply> {
        wr(self.ctx.redis.call(cmd(c)).await).map(from_reply)
    }
    async fn pipeline(
        &mut self,
        cmds: Vec<w::redis::Command>,
        atomic: bool,
    ) -> WResult<Vec<w::redis::RedisReply>> {
        wr(self
            .ctx
            .redis
            .pipeline(cmds.into_iter().map(cmd).collect(), atomic)
            .await)
        .map(|v| v.into_iter().map(from_reply).collect())
    }
}

impl w::events::Host for HostState {
    async fn publish(&mut self, evt: w::events::NewEvent) -> WResult<String> {
        let payload = parse_json(&evt.payload)?;
        let e = NewEvent {
            event_type: evt.event_type,
            version: evt.version,
            key: evt.key,
            payload,
        };
        wr(self.ctx.events.publish(e).await).map(|id| id.to_string())
    }
}

impl w::http_client::Host for HostState {
    async fn send(
        &mut self,
        req: w::http_client::OutboundRequest,
    ) -> WResult<w::http_client::OutboundResponse> {
        let mut b = http::Request::builder()
            .method(req.method.as_str())
            .uri(req.url.as_str());
        for h in &req.headers {
            b = b.header(h.name.as_str(), h.value.as_str());
        }
        let r = match b.body(bytes::Bytes::from(req.body.unwrap_or_default())) {
            Ok(r) => r,
            Err(e) => {
                return wr(Err(pt_plugin_api::PluginError::bad_request(format!(
                    "invalid request: {e}"
                ))));
            }
        };
        let timeout = std::time::Duration::from_millis(u64::from(req.timeout_ms.clamp(1, 30_000)));
        wr(self.ctx.http.send(r, timeout).await.map(|resp| {
            let (parts, body) = resp.into_parts();
            w::http_client::OutboundResponse {
                status: parts.status.as_u16(),
                headers: parts
                    .headers
                    .iter()
                    .filter_map(|(k, v)| {
                        v.to_str().ok().map(|v| w::http_client::Header {
                            name: k.as_str().into(),
                            value: v.into(),
                        })
                    })
                    .collect(),
                body: body.to_vec(),
            }
        }))
    }
}

impl w::blob::Host for HostState {
    async fn put(&mut self, key: String, content_type: String, data: Vec<u8>) -> WResult<()> {
        wr(self.ctx.blob.put(&key, &content_type, data.into()).await)
    }
    async fn get(&mut self, key: String) -> WResult<Option<Vec<u8>>> {
        wr(self.ctx.blob.get(&key).await).map(|o| o.map(|b| b.to_vec()))
    }
    async fn delete(&mut self, key: String) -> WResult<()> {
        wr(self.ctx.blob.delete(&key).await)
    }
    async fn presign_get(&mut self, key: String, ttl_secs: u32) -> WResult<String> {
        wr(self.ctx.blob.presign_get(&key, ttl_secs).await)
    }
}

impl w::terms::Host for HostState {
    async fn get(&mut self, key: String, locale: Option<String>) -> Option<w::terms::Term> {
        self.ctx
            .terms
            .get(&key, locale.as_deref())
            .await
            .map(|t| w::terms::Term {
                key: t.key,
                name: t.name,
                short_name: t.short_name,
                unit: t.unit,
                icon: t.icon,
            })
    }
    async fn get_many(&mut self, keys: Vec<String>, locale: Option<String>) -> Vec<w::terms::Term> {
        let refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        self.ctx
            .terms
            .get_many(&refs, locale.as_deref())
            .await
            .into_iter()
            .map(|t| w::terms::Term {
                key: t.key,
                name: t.name,
                short_name: t.short_name,
                unit: t.unit,
                icon: t.icon,
            })
            .collect()
    }
    async fn render(&mut self, template: String, args: String, locale: Option<String>) -> WResult<String> {
        let args = parse_json(&args)?;
        wr(self.ctx.terms.render(&template, &args, locale.as_deref()).await)
    }
}

impl w::time::Host for HostState {
    async fn now(&mut self) -> i64 {
        self.ctx.time.now()
    }
    async fn site_timezone(&mut self) -> String {
        "UTC".into()
    }
}

impl w::random::Host for HostState {
    async fn bytes(&mut self, len: u32) -> Vec<u8> {
        self.ctx
            .random
            .bytes(usize::try_from(len.min(1 << 20)).unwrap_or(0))
    }
    async fn uuid_v7(&mut self) -> String {
        self.ctx.random.uuid_v7().to_string()
    }
}

impl w::audit::Host for HostState {
    async fn write(&mut self, entry: w::audit::AuditEntry) -> WResult<()> {
        let details = parse_json(&entry.details)?;
        wr(self
            .ctx
            .audit
            .write(AuditEntry {
                action: entry.action,
                target: entry.target,
                details,
            })
            .await)
    }
}

impl w::jobs::Host for HostState {
    async fn schedule_once(
        &mut self,
        name: String,
        run_at: i64,
        payload: String,
        dedupe_key: Option<String>,
    ) -> WResult<String> {
        let payload = parse_json(&payload)?;
        wr(self
            .ctx
            .jobs
            .schedule_once(&name, run_at, payload, dedupe_key.as_deref())
            .await)
    }
    async fn cancel(&mut self, job_id: String) -> WResult<bool> {
        wr(self.ctx.jobs.cancel(&job_id).await)
    }
}

impl w::metrics::Host for HostState {
    async fn counter_add(&mut self, name: String, value: u64, labels: Vec<w::metrics::Label>) {
        let l: Vec<(&str, &str)> = labels
            .iter()
            .map(|x| (x.key.as_str(), x.value.as_str()))
            .collect();
        self.ctx.metrics.counter_add(&name, value, &l);
    }
    async fn gauge_set(&mut self, name: String, value: f64, labels: Vec<w::metrics::Label>) {
        let l: Vec<(&str, &str)> = labels
            .iter()
            .map(|x| (x.key.as_str(), x.value.as_str()))
            .collect();
        self.ctx.metrics.gauge_set(&name, value, &l);
    }
    async fn histogram_observe(&mut self, name: String, value: f64, labels: Vec<w::metrics::Label>) {
        let l: Vec<(&str, &str)> = labels
            .iter()
            .map(|x| (x.key.as_str(), x.value.as_str()))
            .collect();
        self.ctx.metrics.histogram_observe(&name, value, &l);
    }
}

impl w::hook_runner::Host for HostState {
    async fn run(&mut self, hook: String, input: String) -> WResult<w::hook_runner::HookOutcome> {
        let input = parse_json(&input)?;
        match wr(self.ctx.hooks.run(&hook, input).await)? {
            HookOutcome::Allowed(v) => Ok(w::hook_runner::HookOutcome::Allowed(v.to_string())),
            HookOutcome::Denied {
                code,
                message,
                by_plugin,
                ..
            } => Ok(w::hook_runner::HookOutcome::Denied(w::hook_runner::Denial {
                code,
                message,
                by_plugin,
            })),
        }
    }
}

impl w::metadata::Host for HostState {
    async fn sources(&mut self) -> Vec<String> {
        vec![]
    }
    async fn lookup(
        &mut self,
        _s: String,
        _id: String,
        _l: Option<String>,
    ) -> WResult<Option<w::metadata::MetadataRecord>> {
        Err(unavailable("ctx.metadata"))
    }
    async fn search(
        &mut self,
        _s: String,
        _q: String,
        _t: String,
        _n: u32,
    ) -> WResult<Vec<w::metadata::MetadataRecord>> {
        Err(unavailable("ctx.metadata"))
    }
}

impl w::services::Host for HostState {
    async fn call(&mut self, service: String, method: String, args: String) -> WResult<String> {
        let args = parse_json(&args)?;
        wr(self.ctx.call_json(&service, &method, args).await).map(|v| v.to_string())
    }
}
