//! identity 插件（docs/plugins/identity/）：账号、会话、`ctx.auth`、`ctx.rbac`、`ctx.users`。
//!
//! 本版本实现 P1 子集：注册、密码登录、会话、权限检查、用户摘要、passkey。
//! 2FA、token、限制、改名等按 03 详细设计在后续迭代实现。

pub mod accounts;
pub mod api;
pub mod auth;
pub mod crypto;
mod http;
pub mod rbac;

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use pt_plugin_api::{
    Credential, Ctx, HttpRoutes, Lifecycle, NativePlugin, PResult, PluginError, Principal, service_object,
    services::{AUTH, AuthService, PermissionDecl, RBAC, RbacService},
};
use sqlx::Row;

use crate::api::{USERS, UserSummary, UsersService};

pub const MANIFEST: &str = include_str!("../plugin.toml");
pub const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_init.sql", include_str!("../migrations/0001_init.sql")),
    (
        "0002_class_roles.sql",
        include_str!("../migrations/0002_class_roles.sql"),
    ),
];

#[derive(Debug, Default)]
pub struct Identity {
    keys: OnceLock<crypto::Keyring>,
}

impl Identity {
    pub fn keys(&self) -> PResult<&crypto::Keyring> {
        self.keys
            .get()
            .ok_or_else(|| PluginError::unavailable("identity is not initialised"))
    }

    pub async fn summary(&self, ctx: &Ctx, uid: i64) -> PResult<Option<UserSummary>> {
        Ok(self.summaries(ctx, &[uid]).await?.pop())
    }

    pub async fn summaries(&self, ctx: &Ctx, ids: &[i64]) -> PResult<Vec<UserSummary>> {
        if ids.len() > 1000 {
            return Err(PluginError::validation("at most 1000 ids"));
        }
        let mut conn = ctx.sql.conn().await?;
        let rows = sqlx::query(
            "SELECT id, username, class_id, status, title, avatar_url, created_at FROM users
              WHERE id = ANY($1) AND deleted_at IS NULL ORDER BY id",
        )
        .bind(ids)
        .fetch_all(&mut *conn)
        .await?;
        Ok(rows
            .iter()
            .map(|r| UserSummary {
                id: u64::try_from(r.get::<i64, _>(0)).unwrap_or_default(),
                username: r.get(1),
                class_id: u32::try_from(r.get::<i32, _>(2)).unwrap_or_default(),
                status: r.get(3),
                title: r.get(4),
                avatar_url: r.get(5),
                created_at: r.get::<chrono::DateTime<chrono::Utc>, _>(6).timestamp_millis(),
            })
            .collect())
    }

    pub async fn passkey(&self, ctx: &Ctx, uid: u64) -> PResult<Option<String>> {
        let uid = i64::try_from(uid).map_err(|_| PluginError::bad_request("bad user id"))?;
        let mut conn = ctx.sql.conn().await?;
        let enc: Option<Option<Vec<u8>>> =
            sqlx::query_scalar("SELECT passkey_enc FROM passkeys WHERE user_id = $1")
                .bind(uid)
                .fetch_optional(&mut *conn)
                .await?;
        match enc.flatten() {
            Some(blob) => Ok(Some(self.keys()?.passkey_open(uid, &blob)?)),
            None => Ok(None),
        }
    }
}

#[async_trait]
impl Lifecycle for Identity {
    async fn init(&self, ctx: &Ctx) -> PResult<()> {
        let cfg = ctx.config.get_static();
        let secret = cfg
            .get("secret_key")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| PluginError::validation("site.toml [plugin.identity] secret_key is required"))?;
        let _ = self.keys.set(crypto::Keyring::from_secret(secret)?);
        Ok(())
    }
}

struct Svc(Arc<Identity>);

#[async_trait]
impl AuthService for Svc {
    async fn authenticate(&self, ctx: &Ctx, credential: Credential) -> PResult<Option<Principal>> {
        auth::authenticate(ctx, credential).await
    }
}

#[async_trait]
impl RbacService for Svc {
    async fn register_permissions(&self, ctx: &Ctx, perms: Vec<PermissionDecl>) -> PResult<()> {
        rbac::register_permissions(ctx, perms).await
    }
    async fn check_principal(
        &self,
        ctx: &Ctx,
        p: &Principal,
        permission: Option<&str>,
        method: &str,
    ) -> PResult<bool> {
        rbac::check_principal(ctx, p, permission, method).await
    }
    async fn check(&self, ctx: &Ctx, user_id: u64, permission: &str) -> PResult<bool> {
        let uid = i64::try_from(user_id).map_err(|_| PluginError::bad_request("bad user id"))?;
        Ok(rbac::permissions_of(ctx, uid)
            .await?
            .iter()
            .any(|p| p == permission))
    }
}

#[async_trait]
impl UsersService for Svc {
    async fn get(&self, ctx: &Ctx, id: u64) -> PResult<Option<UserSummary>> {
        self.0
            .summary(
                ctx,
                i64::try_from(id).map_err(|_| PluginError::bad_request("bad user id"))?,
            )
            .await
    }
    async fn get_many(&self, ctx: &Ctx, ids: &[u64]) -> PResult<Vec<UserSummary>> {
        let ids: Vec<i64> = ids.iter().filter_map(|i| i64::try_from(*i).ok()).collect();
        self.0.summaries(ctx, &ids).await
    }
    async fn passkey_of(&self, ctx: &Ctx, id: u64) -> PResult<Option<String>> {
        if !ctx.invocation.has_scope("read:passkey") {
            return Err(PluginError::forbidden("scope read:passkey required"));
        }
        self.0.passkey(ctx, id).await
    }
}

#[async_trait]
impl pt_plugin_api::JsonService for Svc {
    async fn call(&self, ctx: &Ctx, method: &str, a: serde_json::Value) -> PResult<serde_json::Value> {
        let id = |k: &str| {
            a.get(k)
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| PluginError::validation(format!("argument `{k}` must be a user id")))
        };
        match method {
            "get" => Ok(serde_json::to_value(
                UsersService::get(self, ctx, id("id")?).await?,
            )?),
            "get_many" => {
                let ids: Vec<u64> = serde_json::from_value(a.get("ids").cloned().unwrap_or_default())
                    .map_err(|e| PluginError::validation(format!("argument `ids`: {e}")))?;
                Ok(serde_json::to_value(
                    UsersService::get_many(self, ctx, &ids).await?,
                )?)
            }
            // ctx.users.set_class：作用域 write:class（levels）
            "set_class" => {
                if !ctx.invocation.has_scope("write:class") {
                    return Err(PluginError::forbidden("scope write:class required"));
                }
                let uid = i64::try_from(id("id")?).map_err(|_| PluginError::bad_request("bad user id"))?;
                let class = a
                    .get("class_id")
                    .and_then(serde_json::Value::as_i64)
                    .ok_or_else(|| PluginError::validation("class_id"))?;
                let reason = a
                    .get("reason")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_owned();
                Ok(serde_json::json!({ "changed": rbac::set_class(ctx, uid, class, &reason).await? }))
            }
            // ctx.rbac.define_role / set_class_roles：作用域 write:roles（+ write:class）
            "define_role" => {
                if !ctx.invocation.has_scope("write:roles") {
                    return Err(PluginError::forbidden("scope write:roles required"));
                }
                let role = a
                    .get("role")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| PluginError::validation("role"))?;
                rbac::define_role(ctx, &ctx.invocation.caller_plugin, role).await?;
                Ok(serde_json::json!({}))
            }
            "set_class_roles" => {
                if !(ctx.invocation.has_scope("write:roles") && ctx.invocation.has_scope("write:class")) {
                    return Err(PluginError::forbidden(
                        "scopes write:roles and write:class required",
                    ));
                }
                let class = a
                    .get("class_id")
                    .and_then(serde_json::Value::as_i64)
                    .ok_or_else(|| PluginError::validation("class_id"))?;
                let roles: Vec<String> = serde_json::from_value(a.get("roles").cloned().unwrap_or_default())
                    .map_err(|e| PluginError::validation(format!("roles: {e}")))?;
                rbac::set_class_roles(ctx, &ctx.invocation.caller_plugin, class, &roles).await?;
                Ok(serde_json::json!({}))
            }
            "roles_of" => {
                let uid = i64::try_from(id("user")?).map_err(|_| PluginError::bad_request("bad user id"))?;
                let (_, roles) = rbac::effective_roles(ctx, uid).await?;
                let mut roles: Vec<String> = roles.into_iter().collect();
                roles.sort();
                Ok(serde_json::json!(roles))
            }
            other => Err(PluginError::new(
                "kernel.not_found",
                format!("ctx.users / ctx.rbac has no method `{other}`"),
            )
            .with_status(404)),
        }
    }
}

struct Routes(Arc<Identity>);

impl HttpRoutes for Routes {
    fn router(&self) -> axum::Router {
        http::router(self.0.clone())
    }
}

pub fn plugin() -> NativePlugin {
    let id = Arc::new(Identity::default());
    let svc = Arc::new(Svc(id.clone()));
    NativePlugin {
        manifest: MANIFEST,
        migrations: MIGRATIONS,
        lifecycle: Some(id.clone()),
        http: Some(Arc::new(Routes(id))),
        services: vec![
            (AUTH, service_object::<dyn AuthService>(svc.clone())),
            (RBAC, service_object::<dyn RbacService>(svc.clone())),
            (USERS, service_object::<dyn UsersService>(svc.clone())),
        ],
        json_services: vec![(USERS, svc.clone()), (RBAC, svc)],
        ..Default::default()
    }
}
