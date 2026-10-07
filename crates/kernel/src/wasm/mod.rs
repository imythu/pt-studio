//! WASM 插件宿主（kernel/03 §4）。
//!
//! 设计要点：WASM 插件在加载时被包装成与 Native 插件相同的导出 trait（`Lifecycle`、`Hooks`、`OnEvent`、
//! `OnCron`、`HttpRoutes`、`JsonService`）。内核运行期（工作单元、钩子管线、事件消费、网关）因此
//! 对两类插件一视同仁。每次调用新建一个组件实例（plugin-api.md §2.4），截止时间由 epoch 中断强制执行。

pub mod bindings;
pub mod host;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    Router,
    body::Bytes,
    extract::{Path as AxPath, RawQuery},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::on,
};
use pt_plugin_api::{
    Ctx, DeliveryInfo, EventEnvelope, FieldSpec, HookCall, HookDecision, Hooks, HttpRoutes, IndexSchema,
    JobInvocation, JobKind, JsonService, Lifecycle, NativePlugin, OnCron, OnEvent, PResult, PluginError,
    RequestCtx, SearchContributor,
    manifest::{Manifest, PluginKind},
};
use wasmtime::{
    Config, Engine, Store, StoreLimitsBuilder,
    component::{Component, HasSelf, Instance, InstancePre, Linker, ResourceTable},
};
use wasmtime_wasi::WasiCtxBuilder;

use self::{
    bindings::exports::pt::plugin as ex,
    host::{HostState, from_wit_err, wit_actor},
};

const EPOCH_TICK: Duration = Duration::from_millis(10);

/// 全局 WASM 引擎与链接器（启动时创建一次）。
pub struct WasmRuntime {
    engine: Engine,
    linker: Linker<HostState>,
}

impl std::fmt::Debug for WasmRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WasmRuntime")
    }
}

impl WasmRuntime {
    pub fn new() -> anyhow::Result<Arc<Self>> {
        let mut cfg = Config::new();
        cfg.wasm_component_model(true);
        cfg.epoch_interruption(true);
        let engine = Engine::new(&cfg).map_err(|e| anyhow::anyhow!("{e}"))?;
        let mut linker: Linker<HostState> = Linker::new(&engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(|e| anyhow::anyhow!("{e}"))?;
        bindings::Kernel::add_to_linker::<_, HasSelf<HostState>>(&mut linker, |s| s)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        // epoch 计时：每 10 ms 推进一次，Store 的截止时间以 tick 计
        let weak = engine.weak();
        std::thread::Builder::new()
            .name("pt-wasm-epoch".into())
            .spawn(move || {
                while let Some(e) = weak.upgrade() {
                    e.increment_epoch();
                    drop(e);
                    std::thread::sleep(EPOCH_TICK);
                }
            })
            .map_err(|e| anyhow::anyhow!("epoch thread: {e}"))?;
        Ok(Arc::new(Self { engine, linker }))
    }
}

/// 已编译的 WASM 插件。
pub struct WasmPlugin {
    pub id: String,
    pre: InstancePre<HostState>,
    memory_bytes: usize,
    permits: tokio::sync::Semaphore,
    lifecycle: Option<ex::lifecycle::GuestIndices>,
    hooks: Option<ex::hooks::GuestIndices>,
    on_event: Option<ex::on_event::GuestIndices>,
    on_cron: Option<ex::on_cron::GuestIndices>,
    http: Option<ex::http_handler::GuestIndices>,
    service: Option<ex::service_provider::GuestIndices>,
    search: Option<ex::search_contributor::GuestIndices>,
}

impl std::fmt::Debug for WasmPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmPlugin")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

/// wasmtime 错误 → 插件错误（陷入、超时、资源耗尽）
fn trap_error(id: &str, e: &wasmtime::Error) -> PluginError {
    let text = format!("{e:?}");
    if let Some(t) = e.downcast_ref::<wasmtime::Trap>() {
        if *t == wasmtime::Trap::Interrupt {
            return PluginError::new("kernel.timeout", format!("plugin `{id}` exceeded its deadline"))
                .retryable();
        }
        if *t == wasmtime::Trap::StackOverflow {
            return PluginError::new(
                "kernel.resource_exhausted",
                format!("plugin `{id}` overflowed its stack"),
            );
        }
    }
    if text.contains("forcing trap when growing memory") || text.contains("forcing a memory growth failure") {
        return PluginError::new(
            "kernel.resource_exhausted",
            format!("plugin `{id}` exceeded its memory limit"),
        );
    }
    tracing::warn!(plugin = id, error = %text, "wasm plugin trapped");
    PluginError::new("kernel.plugin_trapped", format!("plugin `{id}` trapped"))
}

impl WasmPlugin {
    /// 为一次调用实例化组件。
    async fn instantiate(
        &self,
        ctx: &Ctx,
    ) -> PResult<(Store<HostState>, Instance, tokio::sync::SemaphorePermit<'_>)> {
        let permit = self.permits.try_acquire().map_err(|_| {
            PluginError::new(
                "kernel.resource_exhausted",
                format!("plugin `{}` is at max concurrency", self.id),
            )
            .retryable()
        })?;
        let mut wasi = WasiCtxBuilder::new();
        wasi.allow_tcp(false).allow_udp(false).allow_ip_name_lookup(false);
        let state = HostState {
            ctx: ctx.clone(),
            wasi: wasi.build(),
            table: ResourceTable::new(),
            limits: StoreLimitsBuilder::new()
                .memory_size(self.memory_bytes)
                .instances(16)
                .trap_on_grow_failure(true)
                .build(),
        };
        let mut store = Store::new(self.pre.engine(), state);
        store.limiter(|s| &mut s.limits);
        let remaining = (ctx.invocation.deadline - ctx.time.now()).max(1);
        let ticks =
            u64::try_from(remaining).unwrap_or(1) / u64::try_from(EPOCH_TICK.as_millis()).unwrap_or(10) + 1;
        store.set_epoch_deadline(ticks);
        store.epoch_deadline_trap();
        let inst = self
            .pre
            .instantiate_async(&mut store)
            .await
            .map_err(|e| trap_error(&self.id, &e))?;
        Ok((store, inst, permit))
    }
}

macro_rules! call_export {
    ($plugin:expr, $ctx:expr, $field:ident, |$guest:ident, $store:ident| $body:expr) => {{
        let p = &$plugin;
        let idx = p.$field.as_ref().ok_or_else(|| {
            PluginError::internal(format!(
                "plugin `{}` does not export {}",
                p.id,
                stringify!($field)
            ))
        })?;
        let (mut $store, inst, _permit) = p.instantiate($ctx).await?;
        let $guest = idx.load(&mut $store, &inst).map_err(|e| trap_error(&p.id, &e))?;
        let r: wasmtime::Result<_> = $body;
        match r {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => Err(from_wit_err(e)),
            Err(e) => Err(trap_error(&p.id, &e)),
        }
    }};
}

// ---------------- 导出适配器 ----------------

struct Adapter(Arc<WasmPlugin>);

#[async_trait]
impl Lifecycle for Adapter {
    async fn init(&self, ctx: &Ctx) -> PResult<()> {
        call_export!(self.0, ctx, lifecycle, |g, store| g.call_init(&mut store).await)
    }
}

#[async_trait]
impl Hooks for Adapter {
    async fn handle(&self, ctx: &Ctx, call: HookCall) -> PResult<HookDecision> {
        let c = ex::hooks::HookCall {
            hook: call.hook,
            input: call.input.to_string(),
        };
        let d = call_export!(self.0, ctx, hooks, |g, store| g.call_handle(&mut store, &c).await)?;
        Ok(match d {
            ex::hooks::HookDecision::Allow => HookDecision::Allow,
            ex::hooks::HookDecision::Deny(i) => HookDecision::Deny {
                code: i.code,
                message: i.message,
                details: i.details.and_then(|d| serde_json::from_str(&d).ok()),
            },
            ex::hooks::HookDecision::Modify(p) => HookDecision::Modify(
                serde_json::from_str(&p)
                    .map_err(|_| PluginError::internal("hook returned invalid JSON patch"))?,
            ),
        })
    }
}

#[async_trait]
impl OnEvent for Adapter {
    async fn handle(&self, ctx: &Ctx, evt: EventEnvelope, d: DeliveryInfo) -> PResult<()> {
        let e = ex::on_event::EventEnvelope {
            id: evt.id.to_string(),
            event_type: evt.event_type,
            version: evt.version,
            occurred_at: evt.occurred_at,
            actor: wit_actor(&evt.actor),
            source: evt.source,
            key: evt.key,
            payload: evt.payload.to_string(),
            traceparent: evt.traceparent,
        };
        let di = ex::on_event::DeliveryInfo {
            subscription: d.subscription,
            attempt: d.attempt,
            replay: d.replay,
        };
        call_export!(self.0, ctx, on_event, |g, store| g
            .call_handle(&mut store, &e, &di)
            .await)
    }
}

#[async_trait]
impl OnCron for Adapter {
    async fn run(&self, ctx: &Ctx, job: JobInvocation) -> PResult<()> {
        let j = ex::on_cron::JobInvocation {
            name: job.name,
            kind: match job.kind {
                JobKind::Cron => ex::on_cron::JobKind::Cron,
                JobKind::Delayed => ex::on_cron::JobKind::Delayed,
                JobKind::Manual => ex::on_cron::JobKind::Manual,
            },
            scheduled_at: job.scheduled_at,
            payload: job.payload.to_string(),
            run_id: job.run_id,
        };
        call_export!(self.0, ctx, on_cron, |g, store| g.call_run(&mut store, &j).await)
    }
}

impl Adapter {
    async fn wit_index_schemas(&self, ctx: &Ctx) -> PResult<Vec<ex::search_contributor::IndexSchema>> {
        let p = &self.0;
        let idx = p
            .search
            .as_ref()
            .ok_or_else(|| PluginError::internal("search-contributor not exported"))?;
        let (mut store, inst, _permit) = p.instantiate(ctx).await?;
        let g = idx.load(&mut store, &inst).map_err(|e| trap_error(&p.id, &e))?;
        g.call_index_schemas(&mut store)
            .await
            .map_err(|e| trap_error(&p.id, &e))
    }
}

#[async_trait]
impl SearchContributor for Adapter {
    async fn index_schemas(&self, ctx: &Ctx) -> Vec<IndexSchema> {
        let r = self.wit_index_schemas(ctx).await;
        match r {
            Ok(list) => list
                .into_iter()
                .map(|s| IndexSchema {
                    index: s.index,
                    fields: s
                        .fields
                        .into_iter()
                        .map(|f| FieldSpec {
                            name: f.name,
                            field_type: f.field_type,
                            searchable: f.searchable,
                            filterable: f.filterable,
                            sortable: f.sortable,
                            rank: f.rank,
                            exact: f.exact,
                            label_term: f.label_term,
                        })
                        .collect(),
                    synonyms: s.synonyms,
                })
                .collect(),
            Err(e) => {
                tracing::warn!(plugin = %self.0.id, error = %e, "index-schemas failed");
                vec![]
            }
        }
    }

    /// WIT 0.1 只有单条 `to-document`：逐条调用（每次一个新实例）
    async fn to_documents(
        &self,
        ctx: &Ctx,
        index: &str,
        entity_ids: &[u64],
    ) -> PResult<Vec<(u64, Option<serde_json::Value>)>> {
        let mut out = Vec::with_capacity(entity_ids.len());
        for id in entity_ids {
            let doc = call_export!(self.0, ctx, search, |g, store| g
                .call_to_document(&mut store, index, *id)
                .await)?;
            let doc = doc
                .map(|d| serde_json::from_str(&d))
                .transpose()
                .map_err(|_| PluginError::internal("to-document returned invalid JSON"))?;
            out.push((*id, doc));
        }
        Ok(out)
    }
}

/// WASM 提供的业务服务（`service-provider` 导出），每个服务名一个实例。
struct WasmService {
    plugin: Arc<WasmPlugin>,
    service: String,
}

#[async_trait]
impl JsonService for WasmService {
    async fn call(&self, ctx: &Ctx, method: &str, args: serde_json::Value) -> PResult<serde_json::Value> {
        let a = args.to_string();
        let svc = self.service.clone();
        let out = call_export!(self.plugin, ctx, service, |g, store| g
            .call_call(&mut store, &svc, method, &a)
            .await)?;
        serde_json::from_str(&out).map_err(|_| PluginError::internal("service returned invalid JSON"))
    }
}

/// HTTP：按清单路由生成 axum 路由，请求转换为 `http-handler.request` 调用导出。
struct WasmHttp {
    plugin: Arc<WasmPlugin>,
    routes: Vec<(String, String, String)>, // (id, method, path)
}

#[allow(clippy::too_many_arguments)]
async fn http_call(
    plugin: Arc<WasmPlugin>,
    route_id: String,
    ctx: RequestCtx,
    method: Method,
    uri: axum::http::Uri,
    params: HashMap<String, String>,
    query: Option<String>,
    headers: HeaderMap,
    body: Bytes,
) -> PResult<Response> {
    let prefix = format!("/api/p/{}", plugin.id);
    let path = uri.path().strip_prefix(&prefix).unwrap_or(uri.path()).to_owned();
    let query: Vec<(String, String)> = query
        .as_deref()
        .unwrap_or("")
        .split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (pct_decode(k), pct_decode(v))
        })
        .collect();
    // 只传必要的请求头；Cookie / Authorization 不进入插件（鉴权已由网关完成）
    let hdrs: Vec<ex::http_handler::Header> = headers
        .iter()
        .filter(|(n, _)| !matches!(n.as_str(), "cookie" | "authorization" | "x-csrf-token"))
        .filter_map(|(n, v)| {
            Some(ex::http_handler::Header {
                name: n.as_str().to_owned(),
                value: v.to_str().ok()?.to_owned(),
            })
        })
        .collect();
    let req = ex::http_handler::Request {
        route_id,
        method: method.as_str().to_owned(),
        path,
        params: params.into_iter().collect(),
        query,
        headers: hdrs,
        body: body.to_vec(),
    };
    let resp = call_export!(plugin, &ctx, http, |g, store| g
        .call_handle(&mut store, &req)
        .await)?;
    let mut out = (
        StatusCode::from_u16(resp.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        resp.body,
    )
        .into_response();
    for h in resp.headers {
        if let (Ok(n), Ok(v)) = (
            HeaderName::try_from(h.name.as_str()),
            HeaderValue::from_str(&h.value),
        ) {
            if n.as_str() == "set-cookie" {
                continue; // 插件不能设置 cookie
            }
            out.headers_mut().insert(n, v);
        }
    }
    if resp.status >= 400 {
        // 插件以状态码返回错误时，尽量转为 problem+json 的错误标记（便于网关统一处理事务回滚）
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(
            &axum::body::to_bytes(std::mem::take(out.body_mut()), 1 << 20)
                .await
                .unwrap_or_default(),
        ) && let Some(code) = v["code"].as_str()
        {
            let mut e = PluginError::new(
                code,
                v["title"].as_str().or(v["message"].as_str()).unwrap_or(code),
            );
            e.details = v.get("details").cloned();
            return Ok(e.with_status(resp.status).into_response());
        }
        return Err(
            PluginError::new("kernel.internal", "plugin returned an error without a code")
                .with_status(resp.status),
        );
    }
    Ok(out)
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                    Ok(v) => {
                        out.push(v);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl HttpRoutes for WasmHttp {
    fn router(&self) -> Router {
        let mut r = Router::new();
        for (id, method, path) in &self.routes {
            let Ok(m) = Method::from_bytes(method.as_bytes()) else {
                continue;
            };
            let Ok(filter) = axum::routing::MethodFilter::try_from(m) else {
                continue;
            };
            let (plugin, id) = (self.plugin.clone(), id.clone());
            r = r.route(
                path,
                on(
                    filter,
                    move |ctx: RequestCtx,
                          method: Method,
                          uri: axum::http::Uri,
                          params: Option<AxPath<HashMap<String, String>>>,
                          RawQuery(q): RawQuery,
                          headers: HeaderMap,
                          body: Bytes| {
                        let (plugin, id) = (plugin.clone(), id.clone());
                        async move {
                            http_call(
                                plugin,
                                id,
                                ctx,
                                method,
                                uri,
                                params.map(|p| p.0).unwrap_or_default(),
                                q,
                                headers,
                                body,
                            )
                            .await
                            .unwrap_or_else(IntoResponse::into_response)
                        }
                    },
                ),
            );
        }
        r
    }
}

// ---------------- 加载（kernel/03 §2.1、§2.2、§4.3） ----------------

#[derive(Debug)]
pub struct LoadFailure {
    pub plugin: String,
    pub code: &'static str,
    pub message: String,
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// 读取 `plugins.dir` 下的全部 WASM 插件包，编译并包装为 [`NativePlugin`] 形态。
/// 目录结构：`<dir>/<id>/plugin.toml`、`plugin.wasm`、`migrations/NNNN_*.sql`。
pub fn load_dir(
    rt: &Arc<WasmRuntime>,
    dir: &Path,
    allow_unsigned: bool,
) -> (Vec<NativePlugin>, Vec<LoadFailure>) {
    let mut out = Vec::new();
    let mut errs = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else {
        return (out, errs);
    };
    let mut dirs: Vec<PathBuf> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for d in dirs {
        let name = d
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        match load_one(rt, &d, &name, allow_unsigned) {
            Ok(p) => out.push(p),
            Err(e) => errs.push(e),
        }
    }
    (out, errs)
}

fn load_one(
    rt: &Arc<WasmRuntime>,
    d: &Path,
    name: &str,
    allow_unsigned: bool,
) -> Result<NativePlugin, LoadFailure> {
    let fail = |code: &'static str, message: String| LoadFailure {
        plugin: name.to_owned(),
        code,
        message,
    };
    let text = std::fs::read_to_string(d.join("plugin.toml"))
        .map_err(|e| fail("E-LOAD-MANIFEST", format!("plugin.toml: {e}")))?;
    let m = Manifest::parse(&text).map_err(|e| fail("E-LOAD-MANIFEST", e.message))?;
    if m.id != name {
        return Err(fail(
            "E-LOAD-MANIFEST",
            format!("directory name `{name}` must equal plugin id `{}`", m.id),
        ));
    }
    if m.kind != PluginKind::Wasm {
        return Err(fail(
            "E-LOAD-KIND",
            "packages in plugins.dir must declare kind = \"wasm\"".into(),
        ));
    }
    if !d.join("plugin.sig").exists() && !allow_unsigned {
        // 包签名（minisign）校验尚未实现：生产模式下拒绝未签名包
        return Err(fail(
            "E-LOAD-SIGNATURE",
            "package is not signed (set plugins.allow_unsigned = true in dev mode)".into(),
        ));
    }
    let bytes = std::fs::read(d.join("plugin.wasm"))
        .map_err(|e| fail("E-LOAD-WORLD", format!("plugin.wasm: {e}")))?;
    let component =
        Component::new(&rt.engine, &bytes).map_err(|e| fail("E-LOAD-WORLD", format!("compile: {e}")))?;
    let pre = rt
        .linker
        .instantiate_pre(&component)
        .map_err(|e| fail("E-LOAD-WORLD", format!("link: {e}")))?;

    // 迁移文件
    let mut migrations: Vec<(&'static str, &'static str)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(d.join("migrations")) {
        let mut files: Vec<PathBuf> = rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "sql"))
            .collect();
        files.sort();
        for f in files {
            let sql = std::fs::read_to_string(&f)
                .map_err(|e| fail("E-MIG-NAME", format!("{}: {e}", f.display())))?;
            let fname = f
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            migrations.push((leak(fname), leak(sql)));
        }
    }

    let wp = Arc::new(WasmPlugin {
        id: m.id.clone(),
        memory_bytes: usize::try_from(m.wasm.as_ref().and_then(|w| w.memory_mb).unwrap_or(64)).unwrap_or(64)
            << 20,
        permits: tokio::sync::Semaphore::new(
            usize::try_from(m.wasm.as_ref().and_then(|w| w.max_concurrency).unwrap_or(32)).unwrap_or(32),
        ),
        lifecycle: ex::lifecycle::GuestIndices::new(&pre).ok(),
        hooks: ex::hooks::GuestIndices::new(&pre).ok(),
        on_event: ex::on_event::GuestIndices::new(&pre).ok(),
        on_cron: ex::on_cron::GuestIndices::new(&pre).ok(),
        http: ex::http_handler::GuestIndices::new(&pre).ok(),
        service: ex::service_provider::GuestIndices::new(&pre).ok(),
        search: ex::search_contributor::GuestIndices::new(&pre).ok(),
        pre,
    });
    // 导出与清单一致（plugin-api.md §2.1）
    let routes: Vec<(String, String, String)> = m
        .http
        .as_ref()
        .map(|h| {
            h.routes
                .iter()
                .map(|r| (r.id.clone(), r.method.to_ascii_uppercase(), r.path.clone()))
                .collect()
        })
        .unwrap_or_default();
    let checks = [
        ("hooks", wp.hooks.is_some(), !m.hooks.is_empty()),
        ("on-event", wp.on_event.is_some(), !m.subscriptions.is_empty()),
        (
            "on-cron",
            wp.on_cron.is_some(),
            !m.cron.is_empty() || !m.jobs.is_empty(),
        ),
        ("http-handler", wp.http.is_some(), !routes.is_empty()),
        ("service-provider", wp.service.is_some(), !m.provides.is_empty()),
        (
            "search-contributor",
            wp.search.is_some(),
            m.capabilities.search_contributor,
        ),
    ];
    for (iface, exported, declared) in checks {
        if exported != declared {
            let why = if exported {
                "exports it but the manifest does not declare it"
            } else {
                "the manifest declares it but it is not exported"
            };
            return Err(fail("E-LOAD-WORLD", format!("interface `{iface}`: {why}")));
        }
    }
    let adapter = Arc::new(Adapter(wp.clone()));
    let json_services: Vec<(&'static str, Arc<dyn JsonService>)> = m
        .provides
        .iter()
        .map(|p| {
            let svc = p.split('@').next().unwrap_or(p).to_owned();
            let obj: Arc<dyn JsonService> = Arc::new(WasmService {
                plugin: wp.clone(),
                service: svc.clone(),
            });
            (leak(svc), obj)
        })
        .collect();
    Ok(NativePlugin {
        manifest: leak(text),
        migrations: Box::leak(migrations.into_boxed_slice()),
        lifecycle: wp
            .lifecycle
            .is_some()
            .then(|| adapter.clone() as Arc<dyn Lifecycle>),
        hooks: wp.hooks.is_some().then(|| adapter.clone() as Arc<dyn Hooks>),
        on_event: wp.on_event.is_some().then(|| adapter.clone() as Arc<dyn OnEvent>),
        on_cron: wp.on_cron.is_some().then(|| adapter.clone() as Arc<dyn OnCron>),
        http: wp.http.is_some().then(|| {
            Arc::new(WasmHttp {
                plugin: wp.clone(),
                routes,
            }) as Arc<dyn HttpRoutes>
        }),
        search_contributor: wp
            .search
            .is_some()
            .then(|| adapter.clone() as Arc<dyn SearchContributor>),
        json_services,
        ..Default::default()
    })
}
