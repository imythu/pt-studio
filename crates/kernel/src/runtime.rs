//! 运行期核心：插件运行时表、上下文构造、服务调用（kernel/03 §3）。

use std::{
    any::Any,
    collections::{BTreeMap, HashMap, HashSet},
    sync::{Arc, Mutex, Weak},
};

use async_trait::async_trait;
use pt_plugin_api::{
    Actor, Credential, Ctx, HookCall, HookDecision, Hooks, HttpRoutes, IndexSchema, InvocationInfo,
    InvocationKind, Json, KernelLink, Lifecycle, OnCron, OnEvent, PResult, PluginError, Principal,
    SearchContributor,
    host::{Config, HookOutcome, HookRunner, Metrics, Random, Term, Terms, Time},
    manifest::Manifest,
    services::{AUTH, AuthService},
};
use redis::aio::ConnectionManager;
use sqlx::PgPool;

use crate::{
    catalog,
    config::SiteConfig,
    storage::{
        migrate::{gateway_role, role_of},
        redis::RedisGateway,
        uow::{Uow, UowCaps},
    },
};

pub const MAX_DEPTH: u8 = 8;

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// 启动后只读的插件运行时。
pub struct PluginRuntime {
    pub id: String,
    pub manifest: Manifest,
    pub schema: String,
    pub role: String,
    pub static_config: Json,
    /// 有效敏感能力（清单 sensitive ∪ sensitive_optional 与 site.toml grants 的交集）
    pub sensitive: HashSet<String>,
    /// 服务 → 有效作用域
    pub scopes: BTreeMap<String, Vec<String>>,
    pub lifecycle: Option<Arc<dyn Lifecycle>>,
    pub hooks: Option<Arc<dyn Hooks>>,
    pub on_event: Option<Arc<dyn OnEvent>>,
    pub on_cron: Option<Arc<dyn OnCron>>,
    pub workers: Vec<(String, Arc<dyn pt_plugin_api::PartitionWorker>)>,
    pub http: Option<Arc<dyn HttpRoutes>>,
    pub search_contributor: Option<Arc<dyn SearchContributor>>,
    pub content_type: Option<Arc<dyn pt_plugin_api::ContentType>>,
    /// 出站 HTTP 策略（清单 `http_outbound` + 静态配置）
    pub outbound: Arc<crate::http_client::OutboundPolicy>,
    pub redis: RedisGateway,
}

impl std::fmt::Debug for PluginRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginRuntime")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone)]
pub struct HookHandlerEntry {
    pub plugin: String,
    pub priority: i32,
    pub timeout_ms: u32,
}

pub type ServiceObj = Arc<dyn Any + Send + Sync>;
/// 服务名 → [(提供方, JSON 分派实现)]
pub type JsonServices = BTreeMap<String, Vec<(String, Arc<dyn pt_plugin_api::JsonService>)>>;

pub struct Shared {
    pub cfg: Arc<SiteConfig>,
    pub pool: PgPool,
    pub redis: BTreeMap<String, ConnectionManager>,
    /// 用于建立专用连接（阻塞命令不能占用共享的多路复用连接）
    pub redis_clients: BTreeMap<String, redis::Client>,
    pub plugins: BTreeMap<String, Arc<PluginRuntime>>,
    /// 激活顺序
    pub order: Vec<String>,
    pub services: BTreeMap<String, Vec<(String, ServiceObj)>>,
    /// JSON 分派的服务实现（WASM 提供方，以及提供了 JSON 适配的 Native 服务）
    pub json_services: JsonServices,
    pub allowed: HashMap<String, HashSet<String>>,
    pub hook_handlers: BTreeMap<String, Vec<HookHandlerEntry>>,
    pub node_id: String,
    pub events_cfg: Arc<crate::config::EventsSection>,
    pub me: Weak<Shared>,
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared")
            .field("plugins", &self.plugins.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

/// 新调用的参数。
#[derive(Debug, Clone)]
pub struct CallSpec {
    pub kind: InvocationKind,
    pub actor: Actor,
    pub caller_plugin: String,
    pub caller_scopes: Vec<String>,
    pub locale: Option<String>,
    pub timeout_ms: i64,
    pub depth: u8,
    pub chain: Vec<String>,
    pub request_id: String,
    pub traceparent: String,
    pub client_ip: Option<String>,
    pub user_agent: Option<String>,
}

impl CallSpec {
    pub fn new(kind: InvocationKind, actor: Actor, timeout_ms: i64) -> Self {
        Self {
            kind,
            actor,
            caller_plugin: "kernel".into(),
            caller_scopes: vec![],
            locale: None,
            timeout_ms,
            depth: 0,
            chain: vec![],
            request_id: uuid::Uuid::now_v7().to_string(),
            traceparent: String::new(),
            client_ip: None,
            user_agent: None,
        }
    }
}

impl Shared {
    fn arc(&self) -> PResult<Arc<Shared>> {
        self.me
            .upgrade()
            .ok_or_else(|| PluginError::unavailable("kernel is shutting down"))
    }

    pub fn plugin(&self, id: &str) -> PResult<&Arc<PluginRuntime>> {
        self.plugins
            .get(id)
            .ok_or_else(|| PluginError::internal(format!("unknown plugin `{id}`")))
    }

    /// 为插件构造一次调用的上下文与工作单元。`plugin = "kernel"` 时无数据库身份。
    pub fn make_ctx(&self, plugin: &str, spec: CallSpec) -> PResult<(Ctx, Arc<Uow>)> {
        let me = self.arc()?;
        let deadline = now_ms() + spec.timeout_ms.max(1);
        let inv = InvocationInfo {
            kind: spec.kind,
            actor: spec.actor,
            caller_plugin: spec.caller_plugin,
            caller_scopes: spec.caller_scopes,
            traceparent: spec.traceparent,
            tracestate: None,
            locale: spec
                .locale
                .unwrap_or_else(|| self.cfg.site.default_locale.clone()),
            deadline,
            depth: spec.depth,
            chain: spec.chain,
            authenticated: vec![],
            principal_switched: false,
            request_id: spec.request_id,
            client_ip: spec.client_ip,
            user_agent: spec.user_agent,
        };
        let outbound = self
            .plugins
            .get(plugin)
            .map(|rt| rt.outbound.clone())
            .unwrap_or_default();
        let kind = spec.kind;
        let (role, schema, caps, redis, static_config, terms) = match self.plugins.get(plugin) {
            Some(rt) => {
                let m = &rt.manifest;
                let caps = UowCaps {
                    sql: m.capabilities.sql,
                    kv: m.capabilities.kv,
                    publishes: m.publishes.clone(),
                    jobs: m.jobs.iter().map(|j| j.name.clone()).collect(),
                    max_rows: m
                        .database
                        .as_ref()
                        .and_then(|d| d.max_rows)
                        .unwrap_or(self.cfg.database.max_rows),
                };
                (
                    Some(rt.role.clone()),
                    rt.schema.clone(),
                    caps,
                    rt.redis.clone(),
                    rt.static_config.clone(),
                    m.terms.clone(),
                )
            }
            None if plugin == "kernel" => (
                None,
                "kernel".into(),
                UowCaps::default(),
                RedisGateway::new("kernel", false, None, BTreeMap::new()),
                Json::Null,
                BTreeMap::new(),
            ),
            None => return Err(PluginError::internal(format!("unknown plugin `{plugin}`"))),
        };
        let uow = Arc::new(Uow::new(
            self.pool.clone(),
            plugin,
            role,
            schema,
            self.cfg.database.statement_timeout_ms,
            caps,
            self.events_cfg.clone(),
            inv.clone(),
        ));
        let hooks = Arc::new(PipelineRunner {
            shared: me.clone(),
            caller: plugin.to_owned(),
            inv: inv.clone(),
        });
        let ctx = Ctx {
            plugin_id: Arc::from(plugin),
            invocation: inv,
            config: Arc::new(StaticConfig {
                plugin: plugin.to_owned(),
                value: static_config,
                pool: self.pool.clone(),
            }),
            kv: uow.clone(),
            sql: uow.clone(),
            redis: Arc::new(redis),
            events: uow.clone(),
            http: Arc::new(crate::http_client::KernelHttp {
                plugin: plugin.to_owned(),
                policy: outbound,
                kind,
            }),
            blob: Arc::new(crate::storage::blob::FsBlob::new(
                self.plugins
                    .get(plugin)
                    .is_some_and(|rt| rt.manifest.capabilities.blob),
                self.cfg.blob.dir.as_deref(),
                plugin,
            )),
            terms: Arc::new(ManifestTerms {
                terms,
                default_locale: self.cfg.site.default_locale.clone(),
            }),
            time: Arc::new(SystemTime),
            random: Arc::new(OsRandom),
            audit: uow.clone(),
            jobs: uow.clone(),
            metrics: Arc::new(NoopMetrics),
            hooks,
            link: me,
            authenticated: Arc::new(Mutex::new(vec![])),
            unit: uow.clone(),
        };
        Ok((ctx, uow))
    }

    /// 由调用方上下文派生出被调插件的调用参数（服务调用与导出调用共用）。
    pub fn child_spec(
        &self,
        caller: &Ctx,
        kind: InvocationKind,
        callee: &str,
        timeout_ms: i64,
    ) -> PResult<CallSpec> {
        let inv = &caller.invocation;
        if inv.depth + 1 > MAX_DEPTH {
            return Err(PluginError::new(
                "kernel.call_depth_exceeded",
                "service call nesting too deep",
            ));
        }
        let mut chain = inv.chain.clone();
        if !chain.iter().any(|c| c.as_str() == &*caller.plugin_id) && &*caller.plugin_id != "kernel" {
            chain.push(caller.plugin_id.to_string());
        }
        if kind == InvocationKind::Service && chain.iter().any(|c| c == callee) {
            return Err(PluginError::new(
                "kernel.reentrant_call",
                format!(
                    "`{callee}` is already executing in this call chain ({})",
                    chain.join(" → ")
                ),
            )
            .with_details(serde_json::json!({ "chain": chain, "callee": callee })));
        }
        let remaining = inv.deadline - now_ms();
        if remaining <= 0 {
            return Err(PluginError::new("kernel.timeout", "caller deadline exceeded").retryable());
        }
        Ok(CallSpec {
            kind,
            actor: inv.actor.clone(),
            caller_plugin: caller.plugin_id.to_string(),
            caller_scopes: vec![],
            locale: Some(inv.locale.clone()),
            timeout_ms: remaining.min(timeout_ms),
            depth: inv.depth + 1,
            chain,
            request_id: inv.request_id.clone(),
            traceparent: inv.traceparent.clone(),
            // 客户端 IP 是敏感数据：只传给获授 user.ip 的被调方
            client_ip: inv
                .client_ip
                .clone()
                .filter(|_| self.has_sensitive(callee, "user.ip")),
            user_agent: inv.user_agent.clone(),
        })
    }

    pub fn has_sensitive(&self, plugin: &str, cap: &str) -> bool {
        self.plugins
            .get(plugin)
            .is_some_and(|rt| rt.sensitive.contains(cap))
    }

    pub fn auth_provider(&self) -> PResult<(String, Arc<dyn AuthService>)> {
        let (id, obj) = self
            .services
            .get(AUTH)
            .and_then(|v| v.first())
            .ok_or_else(|| PluginError::unavailable("no ctx.auth provider"))?;
        let imp = obj
            .downcast_ref::<Arc<dyn AuthService>>()
            .cloned()
            .ok_or_else(|| PluginError::internal("ctx.auth provider has wrong type"))?;
        Ok((id.clone(), imp))
    }

    /// 内核发起的服务调用（网关鉴权、权限检查）：不检查依赖边，其余与普通服务调用相同。
    pub async fn kernel_service_ctx(&self, caller: &Ctx, provider: &str) -> PResult<(Ctx, Arc<Uow>)> {
        let spec = self.child_spec(
            caller,
            InvocationKind::Service,
            provider,
            self.cfg.kernel.service_timeout_ms,
        )?;
        self.make_ctx(provider, spec)
    }
}

/// 资源类型导出（content-types-common §2：normalize 为纯函数，p99 < 5 ms）
const CONTENT_TYPE_TIMEOUT_MS: i64 = 1_000;

/// 贡献者导出调用的超时（search-meilisearch LLD §1.6.1）
const CONTRIBUTOR_SCHEMA_TIMEOUT_MS: i64 = 2_000;
const CONTRIBUTOR_DOCS_TIMEOUT_MS: i64 = 5_000;

impl Shared {
    /// 贡献者接口只对 `ctx.search` 提供方开放
    fn contributor(&self, caller: &Ctx, plugin: Option<&str>) -> PResult<Option<Arc<dyn SearchContributor>>> {
        let is_search = self.plugins.get(&*caller.plugin_id).is_some_and(|rt| {
            rt.manifest
                .provides
                .iter()
                .any(|p| p.split('@').next() == Some("ctx.search"))
        });
        if !is_search {
            return Err(PluginError::capability_denied(format!(
                "plugin `{}` does not provide ctx.search",
                caller.plugin_id
            )));
        }
        let Some(plugin) = plugin else { return Ok(None) };
        self.plugins
            .get(plugin)
            .and_then(|rt| rt.search_contributor.clone())
            .map(Some)
            .ok_or_else(|| PluginError::not_found(format!("plugin `{plugin}` is not a search contributor")))
    }

    async fn run_contributor<T, F>(&self, caller: &Ctx, plugin: &str, timeout_ms: i64, f: F) -> PResult<T>
    where
        T: Send,
        F: for<'a> FnOnce(&'a Ctx) -> futures::future::BoxFuture<'a, PResult<T>> + Send,
    {
        let spec = self.child_spec(caller, InvocationKind::Search, plugin, timeout_ms)?;
        let (ctx, uow) = self.make_ctx(plugin, spec)?;
        let remaining = ctx.invocation.deadline - now_ms();
        let budget = std::time::Duration::from_millis(u64::try_from(remaining.max(0)).unwrap_or(0));
        let r = match tokio::time::timeout(budget, f(&ctx)).await {
            Ok(r) => r,
            Err(_) => Err(PluginError::new(
                "kernel.timeout",
                format!("search contributor `{plugin}` timed out"),
            )
            .retryable()),
        };
        uow.finish(r.is_ok()).await?;
        r
    }
}

fn uow_of(ctx: &Ctx) -> PResult<Arc<Uow>> {
    ctx.unit
        .clone()
        .downcast::<Uow>()
        .map_err(|_| PluginError::internal("context without unit of work"))
}

#[async_trait]
impl KernelLink for Shared {
    fn resolve(&self, caller: &Ctx, service: &str) -> PResult<Vec<(String, Arc<dyn Any + Send + Sync>)>> {
        let allowed = self
            .allowed
            .get(&*caller.plugin_id)
            .is_some_and(|s| s.contains(service));
        if !allowed {
            return Err(PluginError::capability_denied(format!(
                "plugin `{}` did not declare `{service}` in requires",
                caller.plugin_id
            )));
        }
        Ok(self.services.get(service).cloned().unwrap_or_default())
    }

    async fn enter_service(&self, caller: &Ctx, provider: &str, service: &str) -> PResult<Ctx> {
        // 提供方可声明更长的默认超时（如出站取数的元数据插件，`[wasm] default_timeout_ms`），仍受调用方剩余时间限制
        let own = self
            .plugins
            .get(provider)
            .and_then(|rt| rt.manifest.wasm.as_ref())
            .and_then(|w| w.default_timeout_ms)
            .map_or(0, i64::from);
        let mut spec = self.child_spec(
            caller,
            InvocationKind::Service,
            provider,
            self.cfg.kernel.service_timeout_ms.max(own),
        )?;
        spec.caller_scopes = self
            .plugins
            .get(&*caller.plugin_id)
            .and_then(|rt| rt.scopes.get(service).cloned())
            .unwrap_or_default();
        let (ctx, _) = self.make_ctx(provider, spec)?;
        Ok(ctx)
    }

    async fn exit_service(&self, callee: Ctx, ok: bool) -> PResult<()> {
        uow_of(&callee)?.finish(ok).await
    }

    async fn call_json(&self, caller: &Ctx, service: &str, method: &str, args: Json) -> PResult<Json> {
        let allowed = self
            .allowed
            .get(&*caller.plugin_id)
            .is_some_and(|s| s.contains(service));
        if !allowed {
            return Err(PluginError::capability_denied(format!(
                "plugin `{}` did not declare `{service}` in requires",
                caller.plugin_id
            )));
        }
        let providers = self.json_services.get(service).cloned().unwrap_or_default();
        // 多实现服务（如 ctx.metadata）：按 `provider`（插件 ID）或 `source`（`metadata-<source>`）路由
        let multi = crate::loader::MULTI_SERVICES.contains(&service);
        let (provider, imp) = if multi {
            if method == "providers" {
                return Ok(serde_json::json!(
                    providers.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>()
                ));
            }
            let want = args
                .get("provider")
                .and_then(Json::as_str)
                .map(str::to_owned)
                .or_else(|| {
                    args.get("source")
                        .and_then(Json::as_str)
                        .map(|s| format!("metadata-{s}"))
                })
                .ok_or_else(|| {
                    PluginError::validation(format!(
                        "{service} is a multi-provider service: pass `source` or `provider`"
                    ))
                })?;
            providers
                .into_iter()
                .find(|(p, _)| *p == want)
                .ok_or_else(|| PluginError::unavailable(format!("{service} has no provider `{want}`")))?
        } else {
            providers.into_iter().next().ok_or_else(|| {
                PluginError::unavailable(format!("service {service} has no JSON-callable provider"))
            })?
        };
        let callee = self.enter_service(caller, &provider, service).await?;
        let remaining = callee.invocation.deadline - now_ms();
        let budget = std::time::Duration::from_millis(u64::try_from(remaining.max(0)).unwrap_or(0));
        let r = match tokio::time::timeout(budget, imp.call(&callee, method, args)).await {
            Ok(r) => r,
            Err(_) => {
                Err(PluginError::new("kernel.timeout", format!("service {service} timed out")).retryable())
            }
        };
        uow_of(&callee)?.finish(r.is_ok()).await?;
        r
    }

    fn search_contributors(&self, caller: &Ctx) -> PResult<Vec<String>> {
        self.contributor(caller, None)?;
        Ok(self
            .order
            .iter()
            .filter(|id| {
                self.plugins
                    .get(*id)
                    .is_some_and(|rt| rt.search_contributor.is_some())
            })
            .cloned()
            .collect())
    }

    async fn contributor_schemas(&self, caller: &Ctx, plugin: &str) -> PResult<Vec<IndexSchema>> {
        let imp = self
            .contributor(caller, Some(plugin))?
            .ok_or_else(|| PluginError::internal("no contributor"))?;
        self.run_contributor(caller, plugin, CONTRIBUTOR_SCHEMA_TIMEOUT_MS, |c| {
            Box::pin(async move { Ok(imp.index_schemas(c).await) })
        })
        .await
    }

    async fn contributor_documents(
        &self,
        caller: &Ctx,
        plugin: &str,
        index: &str,
        entity_ids: &[u64],
    ) -> PResult<Vec<(u64, Option<Json>)>> {
        let imp = self
            .contributor(caller, Some(plugin))?
            .ok_or_else(|| PluginError::internal("no contributor"))?;
        let (index, ids) = (index.to_owned(), entity_ids.to_vec());
        self.run_contributor(caller, plugin, CONTRIBUTOR_DOCS_TIMEOUT_MS, |c| {
            Box::pin(async move { imp.to_documents(c, &index, &ids).await })
        })
        .await
    }

    fn content_type_plugins(&self, _caller: &Ctx) -> PResult<Vec<String>> {
        Ok(self
            .order
            .iter()
            .filter(|id| self.plugins.get(*id).is_some_and(|rt| rt.content_type.is_some()))
            .cloned()
            .collect())
    }

    async fn content_type_call(&self, caller: &Ctx, plugin: &str, op: &str, fields: Json) -> PResult<Json> {
        let imp = self
            .plugins
            .get(plugin)
            .and_then(|rt| rt.content_type.clone())
            .ok_or_else(|| PluginError::not_found(format!("plugin `{plugin}` is not a content type")))?;
        let op = op.to_owned();
        let spec = self.child_spec(
            caller,
            InvocationKind::ContentType,
            plugin,
            CONTENT_TYPE_TIMEOUT_MS,
        )?;
        let (ctx, uow) = self.make_ctx(plugin, spec)?;
        let budget = std::time::Duration::from_millis(
            u64::try_from((ctx.invocation.deadline - now_ms()).max(0)).unwrap_or(0),
        );
        let fut = async {
            match op.as_str() {
                "info" => {
                    let i = imp.info(&ctx).await;
                    Ok(
                        serde_json::json!({ "type_id": i.type_id, "name_term": i.name_term,
                                           "field_schema": i.field_schema, "group_schema": i.group_schema }),
                    )
                }
                "normalize" => imp.normalize(&ctx, fields).await,
                "group_key" => imp.group_key(&ctx, &fields).await.map(|k| serde_json::json!(k)),
                "canonical_title" => imp.canonical_title(&ctx, &fields).await.map(Json::String),
                other => Err(PluginError::bad_request(format!(
                    "unknown content-type op `{other}`"
                ))),
            }
        };
        let r = match tokio::time::timeout(budget, fut).await {
            Ok(r) => r,
            Err(_) => Err(
                PluginError::new("kernel.timeout", format!("content type `{plugin}` timed out")).retryable(),
            ),
        };
        uow.finish(r.is_ok()).await?;
        r
    }

    async fn authenticate(&self, caller: &Ctx, credential: Credential) -> PResult<Option<Principal>> {
        let (provider, imp) = self.auth_provider()?;
        let (ctx, uow) = self.kernel_service_ctx(caller, &provider).await?;
        let r = imp.authenticate(&ctx, credential).await;
        uow.finish(r.is_ok()).await?;
        r
    }
}

// ---------------- 钩子管线（kernel/03 §5.2） ----------------

struct PipelineRunner {
    shared: Arc<Shared>,
    caller: String,
    /// 触发方本次调用（钩子沿用其 actor、截止时间与调用链）
    inv: InvocationInfo,
}

/// 只读视图 → 可修改字段检查：补丁的每个叶子路径都必须落在可修改字段（或其子路径）上。
fn patch_paths(v: &Json, prefix: &str, out: &mut Vec<String>) {
    match v {
        Json::Object(m) if !m.is_empty() => {
            for (k, sub) in m {
                let p = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                patch_paths(sub, &p, out);
            }
        }
        _ => out.push(prefix.to_owned()),
    }
}

fn check_patch(
    def: &catalog::HookPointDef,
    patch: &Json,
    cur: &Json,
    redacted: &[&str],
) -> Result<(), String> {
    if !def.allow_modify {
        return Err("hook point does not allow modify".into());
    }
    if !patch.is_object() {
        return Err("patch must be an object".into());
    }
    let mut paths = Vec::new();
    patch_paths(patch, "", &mut paths);
    for p in &paths {
        let ok = def
            .modifiable
            .iter()
            .any(|m| p == m || p.starts_with(&format!("{m}.")));
        if !ok {
            return Err(format!("field `{p}` is not modifiable"));
        }
        if redacted.iter().any(|r| p == r || p.starts_with(&format!("{r}."))) {
            return Err(format!("field `{p}` was redacted for this handler"));
        }
    }
    if def.append_only {
        for m in def.modifiable {
            let (Some(Json::Array(new)), Some(Json::Array(old))) = (pointer(patch, m), pointer(cur, m))
            else {
                continue;
            };
            if new.len() < old.len() || new[..old.len()] != old[..] {
                return Err(format!("`{m}` is append-only"));
            }
        }
    }
    Ok(())
}

fn pointer<'a>(v: &'a Json, dotted: &str) -> Option<&'a Json> {
    v.pointer(&format!("/{}", dotted.replace('.', "/")))
}

/// 按处理者的敏感能力裁剪输入（评审 R1 B-05）。
fn redact(input: &Json, sensitive: &HashSet<String>) -> (Json, Vec<&'static str>) {
    let mut v = input.clone();
    let mut removed = Vec::new();
    if let Some(m) = v.as_object_mut() {
        for (field, cap) in [("email", "user.email"), ("ip", "user.ip")] {
            if !sensitive.contains(cap) && m.remove(field).is_some() {
                removed.push(field);
            }
        }
    }
    (v, removed)
}

#[async_trait]
impl HookRunner for PipelineRunner {
    async fn run(&self, hook: &str, input: Json) -> PResult<HookOutcome> {
        let sh = &self.shared;
        let def = catalog::hook_point(hook)
            .ok_or_else(|| PluginError::internal(format!("unknown hook `{hook}`")))?;
        if def.owner != self.caller {
            return Err(PluginError::capability_denied(format!(
                "only `{}` may run `{hook}`",
                def.owner
            )));
        }
        let site = sh.cfg.hooks.get(hook);
        let on_error_deny = match site.and_then(|o| o.on_error.as_deref()) {
            Some("allow") if !def.locked => false,
            Some("deny") => true,
            _ => def.deny_on_error,
        };
        let disabled: &[String] = site.map(|o| o.disabled_handlers.as_slice()).unwrap_or(&[]);
        if self.inv.depth + 1 > MAX_DEPTH {
            return Err(PluginError::new(
                "kernel.call_depth_exceeded",
                "hook nesting too deep",
            ));
        }
        let mut chain = self.inv.chain.clone();
        if !chain.contains(&self.caller) {
            chain.push(self.caller.clone());
        }
        let mut cur = input;
        let started = now_ms();
        let factor = sh.cfg.kernel.hook_factor();
        let budget = i64::from(def.timeout_ms) * 3 * factor;
        let handlers = sh.hook_handlers.get(hook).cloned().unwrap_or_default();
        for h in handlers.iter().filter(|h| !disabled.contains(&h.plugin)) {
            let rt = sh.plugin(&h.plugin)?;
            let Some(imp) = rt.hooks.clone() else { continue };
            let remaining = (budget - (now_ms() - started))
                .min(i64::from(h.timeout_ms) * factor)
                .min(self.inv.deadline - now_ms());
            let outcome: Result<HookDecision, PluginError> = if remaining <= 0 {
                Err(PluginError::new("kernel.timeout", "hook budget exhausted"))
            } else {
                let (view, removed) = redact(&cur, &rt.sensitive);
                let mut spec = CallSpec::new(InvocationKind::Hook, self.inv.actor.clone(), remaining);
                spec.caller_plugin = self.caller.clone();
                spec.chain = chain.clone();
                spec.depth = self.inv.depth + 1;
                spec.locale = Some(self.inv.locale.clone());
                spec.request_id = self.inv.request_id.clone();
                spec.client_ip = self
                    .inv
                    .client_ip
                    .clone()
                    .filter(|_| rt.sensitive.contains("user.ip"));
                spec.user_agent = self.inv.user_agent.clone();
                let (ctx, uow) = sh.make_ctx(&h.plugin, spec)?;
                let budget = std::time::Duration::from_millis(u64::try_from(remaining).unwrap_or(0));
                let call = HookCall {
                    hook: hook.to_owned(),
                    input: view.clone(),
                };
                let r = match tokio::time::timeout(budget, imp.handle(&ctx, call)).await {
                    Ok(r) => r,
                    Err(_) => Err(PluginError::new("kernel.timeout", "hook handler timed out")),
                };
                // 处理者的写入独立提交（hooks.md）
                uow.finish(r.is_ok()).await?;
                match r {
                    Ok(HookDecision::Modify(patch)) => match check_patch(def, &patch, &view, &removed) {
                        Ok(()) => Ok(HookDecision::Modify(patch)),
                        Err(why) => Err(PluginError::internal(format!("invalid hook patch: {why}"))),
                    },
                    other => other,
                }
            };
            match outcome {
                Ok(HookDecision::Allow) => {}
                Ok(HookDecision::Modify(patch)) => json_patch::merge(&mut cur, &patch),
                Ok(HookDecision::Deny {
                    code,
                    message,
                    details,
                }) => {
                    let code = if code.is_empty() {
                        "kernel.hook_denied".to_owned()
                    } else {
                        code
                    };
                    return Ok(HookOutcome::Denied {
                        code,
                        message,
                        details,
                        by_plugin: h.plugin.clone(),
                    });
                }
                Err(e) => {
                    tracing::warn!(hook, plugin = %h.plugin, error = %e, "hook handler failed");
                    if on_error_deny {
                        return Err(PluginError::new(
                            "kernel.hook_failed",
                            format!("hook `{hook}` handler failed"),
                        )
                        .with_details(serde_json::json!({ "hook": hook, "plugin": h.plugin }))
                        .retryable());
                    }
                }
            }
        }
        Ok(HookOutcome::Allowed(cur))
    }
}

// ---------------- 尚未接入的宿主能力与简单实现 ----------------

struct StaticConfig {
    plugin: String,
    value: Json,
    pool: PgPool,
}

#[async_trait]
impl Config for StaticConfig {
    fn get_static(&self) -> Json {
        self.value.clone()
    }
    async fn get_settings(&self) -> Json {
        sqlx::query_scalar::<_, Json>("SELECT settings FROM kernel.plugin_settings WHERE plugin_id = $1")
            .bind(&self.plugin)
            .fetch_optional(&self.pool)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| serde_json::json!({}))
    }
}

/// 术语：terms 插件接入前，直接使用清单中的默认术语。
struct ManifestTerms {
    terms: BTreeMap<String, Json>,
    default_locale: String,
}

#[async_trait]
impl Terms for ManifestTerms {
    async fn get(&self, key: &str, locale: Option<&str>) -> Option<Term> {
        let v = self.terms.get(key)?;
        let loc = locale.unwrap_or(&self.default_locale);
        let entry = v
            .get(loc)
            .or_else(|| v.get(&self.default_locale))
            .or_else(|| v.as_object()?.values().next())?;
        Some(match entry {
            Json::String(s) => Term {
                key: key.into(),
                name: s.clone(),
                short_name: None,
                unit: None,
                icon: None,
            },
            o => Term {
                key: key.into(),
                name: o.get("name").and_then(Json::as_str).unwrap_or(key).to_owned(),
                short_name: o.get("short").and_then(Json::as_str).map(str::to_owned),
                unit: o.get("unit").and_then(Json::as_str).map(str::to_owned),
                icon: o.get("icon").and_then(Json::as_str).map(str::to_owned),
            },
        })
    }
    async fn get_many(&self, keys: &[&str], locale: Option<&str>) -> Vec<Term> {
        let mut out = Vec::new();
        for k in keys {
            if let Some(t) = self.get(k, locale).await {
                out.push(t);
            }
        }
        out
    }
    async fn render(&self, template: &str, _args: &Json, _locale: Option<&str>) -> PResult<String> {
        Ok(template.to_owned())
    }
}

struct SystemTime;
impl Time for SystemTime {
    fn now(&self) -> i64 {
        now_ms()
    }
}

struct OsRandom;
impl Random for OsRandom {
    fn bytes(&self, len: usize) -> Vec<u8> {
        use rand::RngCore;
        let mut v = vec![0u8; len];
        rand::rng().fill_bytes(&mut v);
        v
    }
    fn uuid_v7(&self) -> uuid::Uuid {
        uuid::Uuid::now_v7()
    }
}

struct NoopMetrics;
impl Metrics for NoopMetrics {
    fn counter_add(&self, _name: &str, _v: u64, _labels: &[(&str, &str)]) {}
    fn gauge_set(&self, _name: &str, _v: f64, _labels: &[(&str, &str)]) {}
    fn histogram_observe(&self, _name: &str, _v: f64, _labels: &[(&str, &str)]) {}
}

/// 有效敏感能力 = (sensitive ∪ sensitive_optional) ∩ grants.<id>.sensitive。
pub fn effective_sensitive(cfg: &SiteConfig, m: &Manifest) -> HashSet<String> {
    let granted: HashSet<&String> = cfg
        .grants
        .get(&m.id)
        .map(|g| g.sensitive.iter().collect())
        .unwrap_or_default();
    m.capabilities
        .sensitive
        .iter()
        .chain(&m.capabilities.sensitive_optional)
        .filter(|s| granted.contains(s))
        .cloned()
        .collect()
}

/// 有效作用域：site.toml 对该服务有授权表时取交集，否则采用清单声明。
pub fn effective_scopes(cfg: &SiteConfig, m: &Manifest) -> BTreeMap<String, Vec<String>> {
    let grants = cfg.grants.get(&m.id).map(|g| &g.scopes);
    m.capabilities
        .scopes
        .iter()
        .map(|(svc, want)| {
            let eff = match grants.and_then(|g| g.get(svc)) {
                Some(allowed) => want.iter().filter(|w| allowed.contains(w)).cloned().collect(),
                None => want.clone(),
            };
            (svc.clone(), eff)
        })
        .collect()
}

pub fn role_names(cfg: &SiteConfig, m: &Manifest) -> (String, String) {
    let schema = m.schema();
    (role_of(cfg, &schema), schema)
}

pub fn gateway(cfg: &SiteConfig) -> String {
    gateway_role(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEF: catalog::HookPointDef = catalog::HookPointDef {
        name: "x.before_y",
        owner: "x",
        allow_modify: true,
        modifiable: &["attributes.inviter_id", "title"],
        append_only: false,
        timeout_ms: 100,
        deny_on_error: true,
        locked: false,
    };

    #[test]
    fn patch_rules() {
        let cur = serde_json::json!({"title": "a", "attributes": {}});
        assert!(
            check_patch(
                &DEF,
                &serde_json::json!({"attributes": {"inviter_id": 3}}),
                &cur,
                &[]
            )
            .is_ok()
        );
        assert!(check_patch(&DEF, &serde_json::json!({"attributes": {"tags": []}}), &cur, &[]).is_err());
        assert!(check_patch(&DEF, &serde_json::json!({"email": "x"}), &cur, &["email"]).is_err());
        let ao = catalog::HookPointDef {
            append_only: true,
            modifiable: &["filters"],
            ..DEF
        };
        let cur = serde_json::json!({"filters": ["a"]});
        assert!(check_patch(&ao, &serde_json::json!({"filters": ["a", "b"]}), &cur, &[]).is_ok());
        assert!(check_patch(&ao, &serde_json::json!({"filters": ["b"]}), &cur, &[]).is_err());
    }

    #[test]
    fn redaction() {
        let input = serde_json::json!({"email": "a@b", "ip": "1.2.3.4", "username": "u"});
        let (v, removed) = redact(&input, &HashSet::from(["user.ip".to_owned()]));
        assert_eq!(removed, vec!["email"]);
        assert!(v.get("ip").is_some() && v.get("email").is_none());
    }
}
