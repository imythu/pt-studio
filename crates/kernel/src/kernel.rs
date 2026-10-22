//! 启动编排（kernel/03 §2.5、§2.6）。

use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Duration,
};

use pt_plugin_api::{
    Actor, InvocationKind, NativePlugin, PluginError,
    services::{PermissionDecl, RBAC, RbacService},
};
use redis::aio::ConnectionManager;
use sqlx::{Executor, postgres::PgPoolOptions};
use tokio::sync::watch;

use crate::{
    catalog,
    config::SiteConfig,
    events,
    loader::{self, LoadError, PluginSpec},
    runtime::{
        CallSpec, HookHandlerEntry, PluginRuntime, ServiceObj, Shared, effective_scopes, effective_sensitive,
        role_names,
    },
    storage::{migrate, redis::RedisGateway},
};

#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))]
    Load(Vec<LoadError>),
    #[error(transparent)]
    Migrate(#[from] migrate::MigrateError),
    #[error("infrastructure unavailable: {0}")]
    Infra(String),
    #[error("plugin `{plugin}` init failed: {error}")]
    Init { plugin: String, error: PluginError },
}

impl StartError {
    /// 退出码（§2.6）
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Load(_) => 2,
            Self::Migrate(_) => 3,
            Self::Infra(_) => 4,
            Self::Init { .. } => 5,
        }
    }
}

pub struct Kernel {
    pub shared: Arc<Shared>,
    stop: watch::Sender<bool>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl std::fmt::Debug for Kernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Kernel")
            .field("shared", &self.shared)
            .finish_non_exhaustive()
    }
}

/// 只做清单与依赖检查（`pt-server check --offline`）。
pub fn check(cfg: &SiteConfig, natives: Vec<NativePlugin>) -> Result<loader::Resolution, Vec<LoadError>> {
    let specs = enabled_specs(cfg, natives)?;
    let res = loader::resolve(&specs, cfg)?;
    let errs = static_checks(cfg, &specs);
    if errs.is_empty() { Ok(res) } else { Err(errs) }
}

fn enabled_specs(cfg: &SiteConfig, natives: Vec<NativePlugin>) -> Result<Vec<PluginSpec>, Vec<LoadError>> {
    let mut specs = Vec::new();
    let mut errors = Vec::new();
    // WASM 插件包（plugins.dir）
    let mut wasm = Vec::new();
    if let Some(dir) = &cfg.plugins.dir {
        if cfg.plugins.allow_unsigned && cfg.kernel.mode != crate::config::Mode::Dev {
            errors.push(LoadError::new(
                "E-LOAD-SIGNATURE",
                "*",
                "plugins.allow_unsigned is only accepted in kernel.mode = \"dev\"",
            ));
        }
        let keys = match crate::wasm::signing::TrustedKeys::parse(&cfg.plugins.trusted_keys) {
            Ok(k) => k,
            Err(e) => {
                errors.push(LoadError::new("E-LOAD-SIGNATURE", "*", e));
                crate::wasm::signing::TrustedKeys::default()
            }
        };
        match crate::wasm::WasmRuntime::new() {
            Ok(rt) => {
                let (ok, failed) =
                    crate::wasm::load_dir(&rt, std::path::Path::new(dir), cfg.plugins.allow_unsigned, &keys);
                wasm = ok;
                errors.extend(
                    failed
                        .into_iter()
                        .map(|f| LoadError::new(f.code, &f.plugin, f.message)),
                );
            }
            Err(e) => errors.push(LoadError::new("E-LOAD-WORLD", "*", format!("wasm engine: {e}"))),
        }
    }
    for np in wasm {
        match PluginSpec::wasm(np) {
            Ok(s) if cfg.plugins.disabled.iter().any(|d| d == s.id()) => {}
            Ok(s) => specs.push(s),
            Err(e) => errors.push(e),
        }
    }
    for np in natives {
        match PluginSpec::native(np) {
            Ok(s) if cfg.plugins.disabled.iter().any(|d| d == s.id()) => {}
            Ok(s) => specs.push(s),
            Err(e) => errors.push(e),
        }
    }
    if errors.is_empty() { Ok(specs) } else { Err(errors) }
}

/// 与 catalog 相关的检查：钩子点归属、处理的钩子存在、敏感能力授权。
fn static_checks(cfg: &SiteConfig, specs: &[PluginSpec]) -> Vec<LoadError> {
    let mut errs = Vec::new();
    let registry = catalog::HookRegistry::build(specs.iter().map(|s| &s.manifest));
    for s in specs {
        let m = &s.manifest;
        for hp in &m.hook_points {
            match catalog::hook_point(&hp.name) {
                Some(def) if def.owner == m.id => {}
                Some(def) => errs.push(LoadError::new(
                    "E-CATALOG",
                    &m.id,
                    format!("hook point `{}` belongs to `{}`", hp.name, def.owner),
                )),
                // 目录之外的钩子点：名字在本插件的命名空间内即可（第三方插件，定义见清单）
                None if hp.name.starts_with(&format!("{}.", m.id)) => {}
                None if cfg.kernel.strict_catalog && !m.id.starts_with("x-") => {
                    errs.push(LoadError::new(
                        "E-CATALOG",
                        &m.id,
                        format!("hook point `{}` is not in catalog/hooks.md", hp.name),
                    ));
                }
                None => {}
            }
        }
        for slot in m.frontend.iter().flat_map(|f| &f.slots) {
            if !catalog::SLOTS.contains(&slot.as_str())
                && cfg.kernel.strict_catalog
                && !m.id.starts_with("x-")
            {
                errs.push(LoadError::new(
                    "E-CATALOG",
                    &m.id,
                    format!("frontend slot `{slot}` is not in catalog/slots.md"),
                ));
            }
        }
        for h in &m.hooks {
            if registry.get(&h.hook).is_none() {
                errs.push(LoadError::new(
                    "E-CATALOG",
                    &m.id,
                    format!("handles unknown hook `{}`", h.hook),
                ));
            }
        }
        if s.native.as_ref().is_some_and(|n| n.hooks.is_none()) && !m.hooks.is_empty() {
            errs.push(LoadError::new(
                "E-LOAD-WORLD",
                &m.id,
                "declares [[hooks]] but does not implement Hooks",
            ));
        }
        if s.native.as_ref().is_some_and(|n| n.on_event.is_none()) && !m.subscriptions.is_empty() {
            errs.push(LoadError::new(
                "E-LOAD-WORLD",
                &m.id,
                "declares [[subscriptions]] but does not implement OnEvent",
            ));
        }
        for w in &m.workers {
            if s.native
                .as_ref()
                .is_some_and(|n| !n.workers.iter().any(|(name, _)| *name == w.name))
            {
                errs.push(LoadError::new(
                    "E-LOAD-WORLD",
                    &m.id,
                    format!("declares worker `{}` but does not implement it", w.name),
                ));
            }
        }
        let granted: HashSet<&String> = cfg
            .grants
            .get(&m.id)
            .map(|g| g.sensitive.iter().collect())
            .unwrap_or_default();
        for sens in &m.capabilities.sensitive {
            if !granted.contains(sens) {
                errs.push(LoadError::new(
                    "E-CAP-SENSITIVE",
                    &m.id,
                    format!("sensitive capability `{sens}` requires site.toml [grants.{}] sensitive = [\"{sens}\"]", m.id),
                ));
            }
        }
    }
    errs
}

/// 只执行迁移（`pt-server migrate`）。
pub async fn migrate_only(cfg: &SiteConfig, natives: Vec<NativePlugin>) -> Result<(), StartError> {
    let specs = enabled_specs(cfg, natives).map_err(StartError::Load)?;
    let res = loader::resolve(&specs, cfg).map_err(StartError::Load)?;
    let ordered: Vec<&PluginSpec> = res
        .order
        .iter()
        .filter_map(|id| specs.iter().find(|s| s.id() == id))
        .collect();
    migrate::migrate_all(cfg, &ordered).await?;
    Ok(())
}

async fn with_retry<T, F, Fut>(what: &str, f: F) -> Result<T, StartError>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    let mut delay = Duration::from_millis(200);
    loop {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) if tokio::time::Instant::now() + delay < deadline => {
                tracing::warn!(what, error = %e, "retrying");
                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(5));
            }
            Err(e) => return Err(StartError::Infra(format!("{what}: {e}"))),
        }
    }
}

impl Kernel {
    pub async fn start(cfg: SiteConfig, natives: Vec<NativePlugin>) -> Result<Kernel, StartError> {
        let cfg = Arc::new(cfg);
        let specs = enabled_specs(&cfg, natives).map_err(StartError::Load)?;
        let res = loader::resolve(&specs, &cfg).map_err(StartError::Load)?;
        let errs = static_checks(&cfg, &specs);
        if !errs.is_empty() {
            return Err(StartError::Load(errs));
        }
        let ordered: Vec<&PluginSpec> = res
            .order
            .iter()
            .filter_map(|id| specs.iter().find(|s| s.id() == id))
            .collect();

        if cfg.database.migrate_on_start {
            migrate::migrate_all(&cfg, &ordered).await?;
        }
        let url = cfg.database.url.clone();
        let opts = (cfg.database.pool_size, cfg.database.acquire_timeout_ms);
        let pool = with_retry("postgres", || {
            let url = url.clone();
            async move {
                PgPoolOptions::new()
                    .max_connections(opts.0)
                    .acquire_timeout(Duration::from_millis(opts.1))
                    .connect(&url)
                    .await
                    .map_err(|e| e.to_string())
            }
        })
        .await?;
        let mut redis = BTreeMap::new();
        let mut redis_clients = BTreeMap::new();
        for (role, inst) in &cfg.redis {
            if let Ok(c) = redis::Client::open(inst.url.as_str()) {
                redis_clients.insert(role.clone(), c);
            }
            let url = inst.url.clone();
            let conn = with_retry("redis", || {
                let url = url.clone();
                async move {
                    let client = redis::Client::open(url.as_str()).map_err(|e| e.to_string())?;
                    ConnectionManager::new(client).await.map_err(|e| e.to_string())
                }
            })
            .await?;
            redis.insert(role.clone(), conn);
        }

        // 运行时表
        let hook_registry = catalog::HookRegistry::build(ordered.iter().map(|s| &s.manifest));
        let error_codes: std::collections::HashMap<String, u16> = ordered
            .iter()
            .flat_map(|s| s.manifest.error_codes.iter().map(|(c, st)| (c.clone(), *st)))
            .collect();
        let mut plugins = BTreeMap::new();
        let mut services: BTreeMap<String, Vec<(String, ServiceObj)>> = BTreeMap::new();
        let mut json_services = crate::runtime::JsonServices::new();
        let mut hook_handlers: BTreeMap<String, Vec<HookHandlerEntry>> = BTreeMap::new();
        let mut errors = Vec::new();
        for spec in &ordered {
            let m = spec.manifest.clone();
            let (role, schema) = role_names(&cfg, &m);
            let mut globals = BTreeMap::new();
            for g in &m.capabilities.redis_global {
                match redis.get(&g.role).or_else(|| redis.get("main")) {
                    Some(c) => {
                        globals.insert(g.role.clone(), c.clone());
                    }
                    None => errors.push(LoadError::new(
                        "E-CAP-REDIS",
                        &m.id,
                        format!("redis role `{}` is not configured", g.role),
                    )),
                }
            }
            let rg = RedisGateway::new(&m.id, m.capabilities.redis, redis.get("main").cloned(), globals);
            let native = spec.native.as_ref();
            if m.capabilities.content_type != native.is_some_and(|n| n.content_type.is_some()) {
                errors.push(LoadError::new(
                    "E-LOAD-WORLD",
                    &m.id,
                    "capabilities.content_type must match the content-type export",
                ));
            }
            if m.capabilities.search_contributor != native.is_some_and(|n| n.search_contributor.is_some()) {
                errors.push(LoadError::new(
                    "E-LOAD-WORLD",
                    &m.id,
                    "capabilities.search_contributor must match the search-contributor export",
                ));
            }
            for (svc, obj) in native.map(|n| n.services.as_slice()).unwrap_or(&[]) {
                if !m.provides.iter().any(|p| p.split('@').next() == Some(*svc)) {
                    errors.push(LoadError::new(
                        "E-LOAD-WORLD",
                        &m.id,
                        format!("implements `{svc}` but does not declare it in provides"),
                    ));
                    continue;
                }
                if res.providers.get(*svc).is_some_and(|p| p.contains(&m.id)) {
                    services
                        .entry((*svc).to_owned())
                        .or_default()
                        .push((m.id.clone(), obj.clone()));
                }
            }
            for (svc, obj) in native.map(|n| n.json_services.as_slice()).unwrap_or(&[]) {
                if !m.provides.iter().any(|p| p.split('@').next() == Some(*svc)) {
                    errors.push(LoadError::new(
                        "E-LOAD-WORLD",
                        &m.id,
                        format!("implements `{svc}` but does not declare it in provides"),
                    ));
                    continue;
                }
                if res.providers.get(*svc).is_some_and(|p| p.contains(&m.id)) {
                    json_services
                        .entry((*svc).to_owned())
                        .or_default()
                        .push((m.id.clone(), obj.clone()));
                }
            }
            for h in &m.hooks {
                let def = hook_registry.get(&h.hook);
                let point_timeout = def.map_or(200, |d| d.timeout_ms);
                let priority = cfg
                    .hooks
                    .get(&h.hook)
                    .and_then(|o| o.priority.get(&m.id).copied())
                    .unwrap_or(h.priority);
                hook_handlers
                    .entry(h.hook.clone())
                    .or_default()
                    .push(HookHandlerEntry {
                        plugin: m.id.clone(),
                        priority,
                        timeout_ms: h.timeout_ms.unwrap_or(point_timeout).min(point_timeout),
                    });
            }
            let mut static_config = cfg
                .plugin
                .get(&m.id)
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));
            // 站点信息（只读，保留键 `_site`）：名称、对外地址、时区、默认语言
            if let Some(obj) = static_config.as_object_mut() {
                obj.insert(
                    "_site".into(),
                    serde_json::json!({ "name": cfg.site.name, "base_url": cfg.site.base_url.trim_end_matches('/'),
                                        "timezone": cfg.site.timezone, "default_locale": cfg.site.default_locale }),
                );
            }
            let outbound = match crate::http_client::OutboundPolicy::parse(
                &m.capabilities.http_outbound,
                &static_config,
                cfg.kernel.mode == crate::config::Mode::Dev,
            ) {
                Ok(p) => Arc::new(p),
                Err(e) => {
                    errors.push(LoadError::new(
                        "E-LOAD-MANIFEST",
                        &m.id,
                        format!("http_outbound: {e}"),
                    ));
                    Arc::default()
                }
            };
            let rt = PluginRuntime {
                id: m.id.clone(),
                outbound,
                schema,
                role,
                static_config,
                sensitive: effective_sensitive(&cfg, &m),
                scopes: effective_scopes(&cfg, &m),
                lifecycle: native.and_then(|n| n.lifecycle.clone()),
                hooks: native.and_then(|n| n.hooks.clone()),
                on_event: native.and_then(|n| n.on_event.clone()),
                on_cron: native.and_then(|n| n.on_cron.clone()),
                workers: native
                    .map(|n| {
                        n.workers
                            .iter()
                            .map(|(k, w)| ((*k).to_owned(), w.clone()))
                            .collect()
                    })
                    .unwrap_or_default(),
                http: native.and_then(|n| n.http.clone()),
                search_contributor: native.and_then(|n| n.search_contributor.clone()),
                content_type: native.and_then(|n| n.content_type.clone()),
                mcp_tool: native.and_then(|n| n.mcp_tool.clone()),
                redis: rg,
                manifest: m,
            };
            plugins.insert(rt.id.clone(), Arc::new(rt));
        }
        for (svc, provs) in &res.providers {
            if !services.contains_key(svc) && !json_services.contains_key(svc) {
                errors.push(LoadError::new(
                    "E-LOAD-WORLD",
                    &provs.join(","),
                    format!("declares `{svc}` in provides but registers no implementation"),
                ));
            }
        }
        let mcp_tools = collect_mcp_tools(&plugins, &mut errors);
        if !errors.is_empty() {
            return Err(StartError::Load(errors));
        }
        for list in hook_handlers.values_mut() {
            list.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.plugin.cmp(&b.plugin)));
        }

        let events_cfg = Arc::new(cfg.events.clone());
        let node_id = cfg.node_id();
        let csp_extra = crate::frontend::CspExtra::from_plugins(
            plugins
                .values()
                .filter_map(|rt| rt.manifest.frontend.as_ref().and_then(|f| f.csp.as_ref())),
        );
        let shared = Arc::new_cyclic(|me| Shared {
            cfg: cfg.clone(),
            pool,
            redis,
            redis_clients,
            plugins,
            order: res.order.clone(),
            services,
            json_services,
            allowed: res.allowed.clone(),
            hook_handlers,
            hook_registry,
            error_codes,
            mcp_tools,
            csp_extra,
            site: crate::frontend::SiteCache::new(&cfg.site.name),
            node_id,
            events_cfg,
            me: me.clone(),
        });

        crate::frontend::SiteCache::refresh(&shared).await;
        record_registry(&shared, &ordered).await?;
        maintain_partitions(&shared).await;
        maintain_plugin_partitions(&shared).await;
        register_permissions(&shared).await?;
        for id in &shared.order {
            let rt = shared.plugin(id).map_err(|e| StartError::Init {
                plugin: id.clone(),
                error: e,
            })?;
            let Some(lc) = rt.lifecycle.clone() else { continue };
            let (ctx, uow) = shared
                .make_ctx(id, CallSpec::new(InvocationKind::Init, Actor::system(), 30_000))
                .map_err(|e| StartError::Init {
                    plugin: id.clone(),
                    error: e,
                })?;
            let r = match tokio::time::timeout(Duration::from_secs(30), lc.init(&ctx)).await {
                Ok(r) => r,
                Err(_) => Err(PluginError::new("kernel.timeout", "init timed out")),
            };
            uow.finish(r.is_ok()).await.map_err(|e| StartError::Init {
                plugin: id.clone(),
                error: e,
            })?;
            r.map_err(|e| StartError::Init {
                plugin: id.clone(),
                error: e,
            })?;
        }

        let (stop, stop_rx) = watch::channel(false);
        let mut tasks = events::spawn_background(shared.clone(), stop_rx.clone());
        tasks.push(crate::frontend::SiteCache::spawn_refresh(shared.clone(), stop_rx.clone()));
        tasks.extend(crate::scheduler::spawn(shared.clone(), stop_rx));
        tracing::info!(plugins = ?shared.order, node = %shared.node_id, "kernel started");
        Ok(Kernel { shared, stop, tasks })
    }

    /// 手动运行一个 cron 任务（kind = manual，不做按触发时间去重，仍禁止重叠）。
    /// 供 CLI（`pt-server jobs run`）与测试使用。
    pub async fn run_job_now(
        &self,
        plugin: &str,
        name: &str,
        payload: serde_json::Value,
    ) -> Result<(), PluginError> {
        let rt = self.shared.plugin(plugin)?;
        let decl = rt
            .manifest
            .cron
            .iter()
            .find(|c| c.name == name)
            .cloned()
            .ok_or_else(|| PluginError::not_found(format!("plugin `{plugin}` has no cron job `{name}`")))?;
        // 正在运行（如启动时的 catch_up）时等待其结束，最多 30 s
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            let r = crate::scheduler::fire(
                &self.shared,
                plugin,
                &decl,
                chrono::Utc::now(),
                pt_plugin_api::JobKind::Manual,
                payload.clone(),
            )
            .await;
            match r {
                Some(r) => return r,
                None if tokio::time::Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(200)).await
                }
                None => return Err(PluginError::new("kernel.conflict", "job is already running").retryable()),
            }
        }
    }

    pub fn router(&self) -> axum::Router {
        crate::gateway::router(self.shared.clone())
    }

    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        for t in self.tasks {
            let _ = tokio::time::timeout(Duration::from_secs(10), t).await;
        }
        self.shared.pool.close().await;
    }
}

async fn record_registry(sh: &Shared, specs: &[&PluginSpec]) -> Result<(), StartError> {
    for s in specs {
        let kind = match s.manifest.kind {
            pt_plugin_api::manifest::PluginKind::Native => "native",
            pt_plugin_api::manifest::PluginKind::Wasm => "wasm",
        };
        sqlx::query(
            "INSERT INTO kernel.plugin_registry (plugin_id, version, kind, manifest_sha256) VALUES ($1,$2,$3,$4)
             ON CONFLICT (plugin_id) DO UPDATE SET version = EXCLUDED.version, kind = EXCLUDED.kind,
               manifest_sha256 = EXCLUDED.manifest_sha256, last_seen_at = now(), removed_at = NULL",
        )
        .bind(s.id())
        .bind(s.manifest.version.to_string())
        .bind(kind)
        .bind(&s.manifest_sha256)
        .execute(&sh.pool)
        .await
        .map_err(|e| StartError::Infra(e.to_string()))?;
    }
    Ok(())
}

/// 事件日志按日分区（评审 R1 B-08）：预建昨天到 3 天后的分区。
pub async fn maintain_partitions(sh: &Shared) {
    let today = chrono::Utc::now().date_naive();
    for offset in -1..=3i64 {
        let d = today + chrono::Duration::days(offset);
        let next = d + chrono::Duration::days(1);
        let name = format!("event_log_d{}", d.format("%Y%m%d"));
        let sql = format!(
            "CREATE TABLE IF NOT EXISTS kernel.{name} PARTITION OF kernel.event_log FOR VALUES FROM ('{d}') TO ('{next}')"
        );
        if let Err(e) = sh.pool.execute(sql.as_str()).await {
            // 默认分区中已有该日数据时无法建分区：保留在默认分区，记录告警
            tracing::warn!(partition = %name, error = %e, "event_log partition not created");
        }
    }
}

/// 插件声明的范围分区（`[[database.partitions]]`，conventions/database.md §7）：以插件角色预建
/// 上一周期到 `premake` 个周期之后的分区。`by = "hash"` 的分区由插件迁移自行创建。
pub async fn maintain_plugin_partitions(sh: &Shared) {
    use chrono::Datelike;
    for rt in sh.plugins.values() {
        let Some(db) = &rt.manifest.database else { continue };
        for p in &db.partitions {
            let (Some(table), Some(by)) = (
                p.get("table").and_then(|v| v.as_str()),
                p.get("by").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            let premake = p
                .get("premake")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(3)
                .clamp(0, 36);
            let valid_ident = |s: &str| {
                !s.is_empty()
                    && s.bytes()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
            };
            if !valid_ident(table) {
                tracing::warn!(plugin = %rt.id, table, "invalid partition table name");
                continue;
            }
            let today = chrono::Utc::now().date_naive();
            let mut ranges = Vec::new();
            for off in -1..=premake {
                match by {
                    "range_month" => {
                        let m0 = today.with_day(1).unwrap_or(today);
                        let start = m0
                            .checked_add_months(chrono::Months::new(u32::try_from(off.max(0)).unwrap_or(0)))
                            .and_then(|d| {
                                if off < 0 {
                                    d.checked_sub_months(chrono::Months::new(1))
                                } else {
                                    Some(d)
                                }
                            });
                        let Some(start) = start else { continue };
                        let Some(end) = start.checked_add_months(chrono::Months::new(1)) else {
                            continue;
                        };
                        ranges.push((
                            format!("{table}_y{}m{:02}", start.year(), start.month()),
                            start,
                            end,
                        ));
                    }
                    "range_day" => {
                        let start = today + chrono::Duration::days(off);
                        ranges.push((
                            format!("{table}_d{}", start.format("%Y%m%d")),
                            start,
                            start + chrono::Duration::days(1),
                        ));
                    }
                    _ => {}
                }
            }
            for (name, start, end) in ranges {
                let sql = format!(
                    "CREATE TABLE IF NOT EXISTS \"{schema}\".\"{name}\" PARTITION OF \"{schema}\".\"{table}\" FOR VALUES FROM ('{start}') TO ('{end}')",
                    schema = rt.schema
                );
                let r = async {
                    let mut tx = sh.pool.begin().await?;
                    tx.execute(format!("SET LOCAL ROLE \"{}\"", rt.role).as_str())
                        .await?;
                    tx.execute(sql.as_str()).await?;
                    tx.commit().await
                }
                .await;
                if let Err(e) = r {
                    tracing::warn!(plugin = %rt.id, partition = %name, error = %e, "partition not created");
                }
            }
        }
    }
}

/// 收集全部插件的权限点，交给 `ctx.rbac`（§2.5 第 3 步）。
async fn register_permissions(sh: &Shared) -> Result<(), StartError> {
    let Some((provider, obj)) = sh.services.get(RBAC).and_then(|v| v.first()).cloned() else {
        return Ok(());
    };
    let rbac = obj
        .downcast_ref::<Arc<dyn RbacService>>()
        .cloned()
        .ok_or_else(|| StartError::Init {
            plugin: provider.clone(),
            error: PluginError::internal("ctx.rbac has wrong type"),
        })?;
    let mut perms: Vec<PermissionDecl> = sh
        .plugins
        .values()
        .flat_map(|rt| {
            rt.manifest.permissions.iter().map(|p| PermissionDecl {
                plugin_id: rt.id.clone(),
                name: p.name.clone(),
                description_term: p.description_term.clone(),
                default_roles: p.default_roles.clone(),
            })
        })
        .collect();
    perms.extend(
        crate::frontend::KERNEL_PERMISSIONS
            .iter()
            .map(|(name, role)| PermissionDecl {
                plugin_id: "kernel".into(),
                name: (*name).into(),
                description_term: Some(format!(
                    "kernel.perm.{}",
                    name.trim_start_matches("kernel.").replace('.', "_")
                )),
                default_roles: vec![(*role).into()],
            }),
    );
    let err = |e| StartError::Init {
        plugin: provider.clone(),
        error: e,
    };
    let (ctx, uow) = sh
        .make_ctx(
            &provider,
            CallSpec::new(InvocationKind::Init, Actor::system(), 30_000),
        )
        .map_err(err)?;
    let r = rbac.register_permissions(&ctx, perms).await;
    uow.finish(r.is_ok()).await.map_err(err)?;
    r.map_err(err)
}

/// 聚合清单 `[[mcp.tools]]`（catalog/mcp-tools.md §1）：名称规则、全站唯一、提供方须实现 `mcp-tool` 导出
fn collect_mcp_tools(
    plugins: &BTreeMap<String, Arc<PluginRuntime>>,
    errors: &mut Vec<LoadError>,
) -> Vec<pt_plugin_api::McpToolDef> {
    let mut out: Vec<pt_plugin_api::McpToolDef> = Vec::new();
    for (id, rt) in plugins {
        let Some(decl) = &rt.manifest.mcp else { continue };
        if decl.tools.is_empty() {
            continue;
        }
        if rt.mcp_tool.is_none() {
            errors.push(LoadError::new(
                "E-LOAD-WORLD",
                id,
                "declares [[mcp.tools]] but does not implement mcp-tool",
            ));
            continue;
        }
        for t in &decl.tools {
            let mut def: pt_plugin_api::McpToolDef = match serde_json::from_value(t.clone()) {
                Ok(d) => d,
                Err(e) => {
                    errors.push(LoadError::new("E-LOAD-MANIFEST", id, format!("mcp.tools: {e}")));
                    continue;
                }
            };
            def.plugin = id.clone();
            let name_ok = def.name.len() >= 3
                && def.name.len() <= 64
                && def.name.starts_with(|c: char| c.is_ascii_lowercase())
                && def
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
            let admin_ok = (def.audience == "admin") == def.name.starts_with("admin_")
                && matches!(def.audience.as_str(), "user" | "admin");
            if !name_ok || !admin_ok || !def.input_schema.is_object() {
                errors.push(LoadError::new(
                    "E-LOAD-MANIFEST",
                    id,
                    format!("mcp tool `{}`: invalid name, audience (admin tools use the admin_ prefix) or input_schema", def.name),
                ));
                continue;
            }
            if let Some(other) = out.iter().find(|o| o.name == def.name) {
                errors.push(LoadError::new(
                    "E-LOAD-MANIFEST",
                    id,
                    format!(
                        "mcp tool `{}` is already declared by `{}`",
                        def.name, other.plugin
                    ),
                ));
                continue;
            }
            out.push(def);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}
