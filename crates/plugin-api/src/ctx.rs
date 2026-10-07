//! 插件上下文与服务调用（plugin-api.md §3、kernel/03 §3）。

use std::{
    any::Any,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use futures::future::BoxFuture;

use crate::{
    Actor, ActorKind, InvocationInfo, PResult, PluginError, Principal, UserId,
    host::{
        Audit, Blob, Config, Events, HookRunner, HttpClient, Jobs, Kv, Metrics, Random, Redis, Sql, Terms,
        Time,
    },
};

/// 网关与插件提交给 `ctx.auth` 的凭据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    /// 会话 cookie 值
    Session(String),
    /// `Authorization: Bearer` 值（`pts_` 个人 token、`ptm_` MCP token）
    Bearer(String),
    /// RSS / 辅种 key（只读路由）
    Feed(String),
}

/// 内核与插件上下文之间的连接（内核实现）。
#[async_trait]
pub trait KernelLink: Send + Sync {
    /// 检查调用方可调用该服务，返回全部提供方（plugin_id, 类型擦除的实现）
    fn resolve(&self, caller: &Ctx, service: &str) -> PResult<Vec<(String, Arc<dyn Any + Send + Sync>)>>;
    /// 为一次服务调用构造提供方的上下文（深度、截止时间、重入检测、独立工作单元）
    async fn enter_service(&self, caller: &Ctx, provider: &str, service: &str) -> PResult<Ctx>;
    /// 结束服务调用：成功则提交提供方工作单元（含事件），失败则回滚
    async fn exit_service(&self, callee: Ctx, ok: bool) -> PResult<()>;
    /// 经 `ctx.auth` 提供方鉴权
    async fn authenticate(&self, caller: &Ctx, credential: Credential) -> PResult<Option<Principal>>;
}

/// 插件上下文：每次调用由内核构造，绑定插件身份、能力与 [`InvocationInfo`]。
#[derive(Clone)]
pub struct Ctx {
    pub plugin_id: Arc<str>,
    pub invocation: InvocationInfo,
    pub config: Arc<dyn Config>,
    pub kv: Arc<dyn Kv>,
    pub sql: Arc<dyn Sql>,
    pub redis: Arc<dyn Redis>,
    pub events: Arc<dyn Events>,
    pub http: Arc<dyn HttpClient>,
    pub blob: Arc<dyn Blob>,
    pub terms: Arc<dyn Terms>,
    pub time: Arc<dyn Time>,
    pub random: Arc<dyn Random>,
    pub audit: Arc<dyn Audit>,
    pub jobs: Arc<dyn Jobs>,
    pub metrics: Arc<dyn Metrics>,
    pub hooks: Arc<dyn HookRunner>,
    #[doc(hidden)]
    pub link: Arc<dyn KernelLink>,
    /// 本调用链中经 [`Ctx::authenticate`] 认证过的用户（`with_principal` 只接受这些）
    #[doc(hidden)]
    pub authenticated: Arc<Mutex<Vec<UserId>>>,
    /// 内核私有：本次调用的工作单元
    #[doc(hidden)]
    pub unit: Arc<dyn Any + Send + Sync>,
}

impl std::fmt::Debug for Ctx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ctx")
            .field("plugin_id", &self.plugin_id)
            .field("invocation", &self.invocation)
            .finish()
    }
}

/// 把服务实现登记为类型擦除对象：`service_object::<dyn LedgerService>(Arc::new(imp))`。
pub fn service_object<S: ?Sized + Send + Sync + 'static>(imp: Arc<S>) -> Arc<dyn Any + Send + Sync> {
    Arc::new(imp)
}

impl Ctx {
    pub fn actor(&self) -> &Actor {
        &self.invocation.actor
    }

    pub fn user_id(&self) -> Option<UserId> {
        self.invocation.actor.user_id
    }

    /// 要求调用者为已登录用户。
    pub fn require_user(&self) -> PResult<UserId> {
        match (self.invocation.actor.kind, self.invocation.actor.user_id) {
            (ActorKind::User | ActorKind::Mcp, Some(u)) => Ok(u),
            _ => Err(PluginError::unauthenticated()),
        }
    }

    /// 取已选定的单实现服务：`ctx.service::<dyn LedgerService>("ctx.ledger")`。
    pub fn service<S: ?Sized + Send + Sync + 'static>(&self, name: &str) -> PResult<Svc<S>> {
        let mut all = self.providers::<S>(name)?;
        match all.len() {
            1 => Ok(all.remove(0)),
            0 => Err(PluginError::unavailable(format!(
                "service {name} has no provider"
            ))),
            _ => Err(PluginError::internal(format!(
                "service {name} is multi; use providers()"
            ))),
        }
    }

    /// 取多实现服务的全部提供方。
    pub fn providers<S: ?Sized + Send + Sync + 'static>(&self, name: &str) -> PResult<Vec<Svc<S>>> {
        let list = self.link.resolve(self, name)?;
        list.into_iter()
            .map(|(provider, obj)| {
                let imp = obj
                    .downcast_ref::<Arc<S>>()
                    .cloned()
                    .ok_or_else(|| PluginError::internal(format!("service {name} type mismatch")))?;
                Ok(Svc {
                    imp,
                    provider,
                    service: name.to_owned(),
                    caller: self.clone(),
                })
            })
            .collect()
    }

    /// 经 `ctx.auth` 鉴权；成功的主体记入本调用链，供 [`Ctx::with_principal`] 使用（kernel/03 §3.4）。
    pub async fn authenticate(&self, credential: Credential) -> PResult<Option<Principal>> {
        let p = self.link.authenticate(self, credential).await?;
        if let Some(p) = &p
            && let Ok(mut list) = self.authenticated.lock()
        {
            list.push(p.user_id);
        }
        Ok(p)
    }

    /// 以已认证用户的身份继续调用（只能切换一次）。
    pub fn with_principal(&self, principal: &Principal) -> PResult<Ctx> {
        if self.invocation.principal_switched {
            return Err(PluginError::capability_denied("principal already switched"));
        }
        let ok = self
            .authenticated
            .lock()
            .map(|l| l.contains(&principal.user_id))
            .unwrap_or(false);
        if !ok {
            return Err(PluginError::capability_denied(
                "principal was not authenticated in this invocation",
            ));
        }
        let mut c = self.clone();
        c.invocation.actor = Actor::user(principal.user_id, Some(principal.credential_id.clone()));
        c.invocation.principal_switched = true;
        c.invocation.authenticated = vec![principal.user_id];
        Ok(c)
    }
}

/// 服务句柄：每次 [`Svc::call`] 都是一次独立的服务调用（提供方独立工作单元）。
pub struct Svc<S: ?Sized> {
    imp: Arc<S>,
    provider: String,
    service: String,
    caller: Ctx,
}

impl<S: ?Sized> std::fmt::Debug for Svc<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Svc")
            .field("service", &self.service)
            .field("provider", &self.provider)
            .finish()
    }
}

impl<S: ?Sized + Send + Sync + 'static> Svc<S> {
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// `svc.call(|s, c| s.get(c, id)).await`
    pub async fn call<R, F>(&self, f: F) -> PResult<R>
    where
        R: Send,
        F: for<'a> FnOnce(&'a S, &'a Ctx) -> BoxFuture<'a, PResult<R>> + Send,
    {
        let callee = self
            .caller
            .link
            .enter_service(&self.caller, &self.provider, &self.service)
            .await?;
        let remaining = callee
            .invocation
            .deadline
            .saturating_sub(callee.time.now())
            .max(0);
        let budget = std::time::Duration::from_millis(u64::try_from(remaining).unwrap_or(0));
        let r = match tokio::time::timeout(budget, f(&self.imp, &callee)).await {
            Ok(r) => r,
            Err(_) => Err(
                PluginError::new("kernel.timeout", format!("service {} timed out", self.service)).retryable(),
            ),
        };
        let link = self.caller.link.clone();
        match r {
            Ok(v) => {
                link.exit_service(callee, true).await?;
                Ok(v)
            }
            Err(e) => {
                link.exit_service(callee, false).await?;
                Err(e)
            }
        }
    }
}
