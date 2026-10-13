//! greeter：PT Studio Native 插件模板。订阅 `user.registered` 记录欢迎时间（幂等），`GET /v1/me` 返回当前用户的记录。
//!
//! Native 插件实现 `pt-plugin-api` 中的 trait（`OnEvent`、`HttpRoutes` …），由 `pt-server` 的 cargo feature 编译进二进制；
//! 与 WASM 插件使用同一份清单格式与同样的权限模型（SQL 只能访问自己的 schema 与已授权视图）。

use std::sync::Arc;

use async_trait::async_trait;
use axum::{Json, Router, response::IntoResponse, routing::get};
use pt_plugin_api::{
    Ctx, DeliveryInfo, EventEnvelope, HttpRoutes, NativePlugin, OnEvent, PResult, PluginError, RequestCtx,
    json,
};
use sqlx::Row;

pub const MANIFEST: &str = include_str!("../plugin.toml");
pub const MIGRATIONS: &[(&str, &str)] = &[("0001_init.sql", include_str!("../migrations/0001_init.sql"))];

struct Events;

#[async_trait]
impl OnEvent for Events {
    async fn handle(&self, ctx: &Ctx, evt: EventEnvelope, d: DeliveryInfo) -> PResult<()> {
        if d.subscription != "on_registered" {
            return Ok(());
        }
        let Some(uid) = evt.payload["user_id"].as_i64() else {
            return Ok(());
        };
        // 事件至少投递一次：写入要幂等
        let mut c = ctx.sql.conn().await?;
        sqlx::query("INSERT INTO welcomed (user_id) VALUES ($1) ON CONFLICT DO NOTHING")
            .bind(uid)
            .execute(&mut *c)
            .await?;
        Ok(())
    }
}

async fn http_me(ctx: RequestCtx) -> PResult<axum::response::Response> {
    let uid = i64::try_from(ctx.require_user()?).map_err(|_| PluginError::bad_request("bad user"))?;
    let mut c = ctx.sql.conn().await?;
    let at: Option<i64> = sqlx::query(
        "SELECT (extract(epoch FROM welcomed_at) * 1000)::bigint FROM welcomed WHERE user_id = $1",
    )
    .bind(uid)
    .fetch_optional(&mut *c)
    .await?
    .map(|r| r.get(0));
    Ok(Json(json!({ "user_id": uid, "welcomed_at": at })).into_response())
}

struct Routes;

impl HttpRoutes for Routes {
    fn router(&self) -> Router {
        // 路径与清单 [[http.routes]] 一致（内核挂在 /api/p/greeter 下，鉴权、权限、限流已在网关完成）
        Router::new().route("/v1/me", get(http_me))
    }
}

/// 在 crates/server/src/lib.rs 的 `native_plugins()` 中 `v.push(pt_plugin_greeter::plugin())`
pub fn plugin() -> NativePlugin {
    NativePlugin {
        manifest: MANIFEST,
        migrations: MIGRATIONS,
        on_event: Some(Arc::new(Events)),
        http: Some(Arc::new(Routes)),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn manifest_parses() {
        let m = pt_plugin_api::manifest::Manifest::parse(super::MANIFEST);
        assert!(m.is_ok_and(|m| m.id == "greeter" && pt_plugin_api::api_supported(&m.api)));
    }
}
