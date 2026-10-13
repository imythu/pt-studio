//! 端到端：内核 + identity + 探针插件，使用真实 PostgreSQL 与 Redis。
//!
//! 环境变量（缺省值对应 .dev 本地实例）：
//! - `PT_TEST_SUPERUSER_URL`：用于清理测试库（默认 postgres://postgres@127.0.0.1:55432/pt_test）
//! - `PT_TEST_ADMIN_URL` / `PT_TEST_GATEWAY_URL` / `PT_TEST_REDIS_URL`
//!
//! 运行：`cargo test -p pt-server --test e2e_identity -- --test-threads=1`

#![allow(clippy::unwrap_used)]

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use axum::{Json, Router, http::StatusCode, response::IntoResponse, routing::get};
use pt_kernel::Kernel;
use pt_plugin_api::{
    Ctx, DeliveryInfo, EventEnvelope, HookCall, HookDecision, Hooks, HttpRoutes, NativePlugin, OnEvent,
    PResult, PluginError, RequestCtx, json,
};
use pt_plugin_identity::api::{USERS, UsersService};
use sqlx::{Connection, PgConnection};

mod common;

use common::{call, config, env, reset};

const PROBE_MANIFEST: &str = r#"
id = "x-probe"
name = "probe"
version = "0.1.0"
kind = "native"
api = "pt:plugin@1"
license = "MIT OR Apache-2.0"
requires = ["ctx.users@^0.1"]

[capabilities]
sql = true
views = ["identity.v_user_public"]

[[hooks]]
hook = "user.before_register"
priority = 10

[[subscriptions]]
name = "on_registered"
event = "user.registered"
start = "earliest"

[[http.routes]]
id = "seen"
method = "GET"
path = "/v1/seen"
auth = "public"

[[http.routes]]
id = "peek"
method = "GET"
path = "/v1/peek"
auth = "public"

[[http.routes]]
id = "view"
method = "GET"
path = "/v1/view"
auth = "public"

[database]
migrations = "migrations"
"#;

const PROBE_MIGRATIONS: &[(&str, &str)] = &[(
    "0001_init.sql",
    "CREATE TABLE seen (user_id bigint PRIMARY KEY, username text NOT NULL, hook_email_visible boolean NOT NULL DEFAULT false);
     CREATE TABLE hook_calls (username text NOT NULL, email_visible boolean NOT NULL);",
)];

struct Probe;

#[async_trait]
impl Hooks for Probe {
    async fn handle(&self, ctx: &Ctx, call: HookCall) -> PResult<HookDecision> {
        let username = call.input["username"].as_str().unwrap_or_default().to_owned();
        // 未获授 user.email：内核应已删除 email 字段（评审 R1 B-05）
        let email_visible = call.input.get("email").is_some();
        let mut c = ctx.sql.conn().await?;
        sqlx::query("INSERT INTO hook_calls (username, email_visible) VALUES ($1, $2)")
            .bind(&username)
            .bind(email_visible)
            .execute(&mut *c)
            .await?;
        if username.starts_with("deny") {
            return Ok(HookDecision::Deny {
                code: "x-probe.registration_denied".into(),
                message: "nope".into(),
                details: None,
            });
        }
        Ok(HookDecision::Modify(
            json!({ "attributes": { "tags": ["probed"] } }),
        ))
    }
}

#[async_trait]
impl OnEvent for Probe {
    async fn handle(&self, ctx: &Ctx, evt: EventEnvelope, _d: DeliveryInfo) -> PResult<()> {
        let uid = evt.payload["user_id"]
            .as_u64()
            .ok_or_else(|| PluginError::validation("user_id"))?;
        // 服务调用：经内核进入 identity 的独立工作单元
        let users = ctx.service::<dyn UsersService>(USERS)?;
        let u = users
            .call(|s, c| s.get(c, uid))
            .await?
            .ok_or_else(|| PluginError::not_found("user"))?;
        let mut c = ctx.sql.conn().await?;
        sqlx::query("INSERT INTO seen (user_id, username) VALUES ($1, $2)")
            .bind(i64::try_from(uid).unwrap_or_default())
            .bind(&u.username)
            .execute(&mut *c)
            .await?;
        Ok(())
    }
}

async fn seen(ctx: RequestCtx) -> PResult<axum::response::Response> {
    let mut c = ctx.sql.conn().await?;
    let names: Vec<String> = sqlx::query_scalar("SELECT username FROM seen ORDER BY user_id")
        .fetch_all(&mut *c)
        .await?;
    let hooks: Vec<(String, bool)> =
        sqlx::query_as("SELECT username, email_visible FROM hook_calls ORDER BY username")
            .fetch_all(&mut *c)
            .await?;
    Ok(Json(json!({ "seen": names, "hooks": hooks })).into_response())
}

/// 越权读取 identity 底表：必须失败（schema 与角色隔离）
async fn peek(ctx: RequestCtx) -> PResult<axum::response::Response> {
    let mut c = ctx.sql.conn().await?;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM identity.users")
        .fetch_one(&mut *c)
        .await?;
    Ok(Json(json!({ "count": n })).into_response())
}

/// 授权视图：可以读取
async fn view(ctx: RequestCtx) -> PResult<axum::response::Response> {
    let mut c = ctx.sql.conn().await?;
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM identity.v_user_public")
        .fetch_one(&mut *c)
        .await?;
    Ok(Json(json!({ "count": n })).into_response())
}

struct ProbeRoutes;
impl HttpRoutes for ProbeRoutes {
    fn router(&self) -> Router {
        Router::new()
            .route("/v1/seen", get(seen))
            .route("/v1/peek", get(peek))
            .route("/v1/view", get(view))
    }
}

fn probe() -> NativePlugin {
    let p = Arc::new(Probe);
    NativePlugin {
        manifest: PROBE_MANIFEST,
        migrations: PROBE_MIGRATIONS,
        hooks: Some(p.clone()),
        on_event: Some(p),
        http: Some(Arc::new(ProbeRoutes)),
        ..Default::default()
    }
}

#[tokio::test]
async fn identity_end_to_end() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter("info,sqlx=warn")
        .try_init();
    let _guard = common::LOCK.lock().await;
    reset().await;
    let mut plugins = pt_server::native_plugins();
    plugins.push(probe());
    let kernel = Kernel::start(config(), plugins)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();

    // 注册：钩子 modify 生效
    let r = call(
        &app,
        "POST",
        "/api/p/identity/v1/users",
        Some(json!({"username": "Alice_01", "email": "Alice@Example.com", "password": "correct horse"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let alice_id = r.body["user"]["id"].as_u64().unwrap();

    // 钩子拒绝：使用处理者给出的错误码
    let r = call(
        &app,
        "POST",
        "/api/p/identity/v1/users",
        Some(json!({"username": "deny_me", "email": "d@example.com", "password": "correct horse"})),
        &[],
    )
    .await;
    assert_eq!(r.body["code"], "x-probe.registration_denied", "{:?}", r.body);

    // 重复
    let r = call(
        &app,
        "POST",
        "/api/p/identity/v1/users",
        Some(json!({"username": "alice_01", "email": "x@example.com", "password": "correct horse"})),
        &[],
    )
    .await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (StatusCode::CONFLICT, Some("identity.username_taken"))
    );
    let r = call(
        &app,
        "POST",
        "/api/p/identity/v1/users",
        Some(json!({"username": "bob_02", "email": "alice@example.COM", "password": "correct horse"})),
        &[],
    )
    .await;
    assert_eq!(r.body["code"], "identity.email_taken");

    // 登录
    let r = call(
        &app,
        "POST",
        "/api/p/identity/v1/sessions",
        Some(json!({"username": "alice_01", "password": "wrong pass"})),
        &[],
    )
    .await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("identity.invalid_credentials"))
    );
    let r = call(
        &app,
        "POST",
        "/api/p/identity/v1/sessions",
        Some(json!({"username": "alice@example.com", "password": "correct horse", "remember": true})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let csrf = r.body["csrf_token"].as_str().unwrap().to_owned();
    let cookie = r.cookies.join("; ");

    let r = call(
        &app,
        "GET",
        "/api/p/identity/v1/users/me",
        None,
        &[("cookie", &cookie)],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["user"]["id"].as_u64(), Some(alice_id));
    assert!(r.body["roles"].as_array().unwrap().iter().any(|v| v == "user"));

    let r = call(
        &app,
        "GET",
        "/api/p/identity/v1/users/me/permissions",
        None,
        &[("cookie", &cookie)],
    )
    .await;
    let perms = r.body["permissions"].as_array().unwrap();
    assert!(perms.iter().any(|p| p == "identity.profile.update_own"));
    assert!(!perms.iter().any(|p| p == "identity.roles.assign"));

    let r = call(
        &app,
        "GET",
        "/api/p/identity/v1/users/me/passkey",
        None,
        &[("cookie", &cookie)],
    )
    .await;
    assert_eq!(r.body["passkey"].as_str().map(str::len), Some(32), "{:?}", r.body);

    // 管理路由：普通用户无权限
    let r = call(
        &app,
        "POST",
        &format!("/api/p/identity/v1/admin/users/{alice_id}/roles"),
        Some(json!({"role": "uploader"})),
        &[
            ("cookie", &cookie),
            ("x-csrf-token", &csrf),
            ("sec-fetch-site", "same-origin"),
        ],
    )
    .await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("kernel.forbidden"))
    );

    // 未声明的路由、未登录
    let r = call(&app, "GET", "/api/p/identity/v1/nope", None, &[]).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = call(&app, "GET", "/api/p/identity/v1/users/me", None, &[]).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // 隔离：探针不能读 identity 底表，能读授权视图
    let r = call(&app, "GET", "/api/p/x-probe/v1/peek", None, &[]).await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (StatusCode::INTERNAL_SERVER_ERROR, Some("kernel.internal")),
        "{:?}",
        r.body
    );
    let r = call(&app, "GET", "/api/p/x-probe/v1/view", None, &[]).await;
    assert_eq!(r.body["count"], 1, "{:?}", r.body);

    // 事件 → 探针订阅（内含对 ctx.users 的服务调用）
    let mut ok = false;
    for _ in 0..100 {
        let r = call(&app, "GET", "/api/p/x-probe/v1/seen", None, &[]).await;
        if r.body["seen"] == json!(["Alice_01"]) {
            // 钩子收到的输入中没有 email（探针未获授 user.email）；重复邮箱在钩子之前被拒绝
            assert_eq!(
                r.body["hooks"],
                json!([["Alice_01", false], ["deny_me", false]]),
                "{:?}",
                r.body
            );
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ok, "user.registered was not delivered to x-probe");

    // 登出：缺 CSRF 被拒，带上后成功，之后会话失效
    let r = call(
        &app,
        "DELETE",
        "/api/p/identity/v1/sessions/current",
        None,
        &[("cookie", &cookie)],
    )
    .await;
    assert_eq!(r.body["code"], "kernel.csrf_failed");
    let r = call(
        &app,
        "DELETE",
        "/api/p/identity/v1/sessions/current",
        None,
        &[
            ("cookie", &cookie),
            ("x-csrf-token", &csrf),
            ("sec-fetch-site", "same-origin"),
        ],
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    let r = call(
        &app,
        "GET",
        "/api/p/identity/v1/users/me",
        None,
        &[("cookie", &cookie)],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // 事件日志与 outbox：注册与登录事件已写入并投递
    let admin = env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    );
    let mut c = PgConnection::connect(&admin).await.unwrap();
    let types: Vec<String> = sqlx::query_scalar("SELECT event_type FROM kernel.event_log ORDER BY seq")
        .fetch_all(&mut c)
        .await
        .unwrap();
    assert!(
        types.contains(&"user.registered".to_owned()) && types.contains(&"user.logged_in".to_owned()),
        "{types:?}"
    );
    assert!(
        types.contains(&"user.login_failed".to_owned()),
        "failed logins must be recorded: {types:?}"
    );
    let backlog: i64 = sqlx::query_scalar("SELECT count(*) FROM kernel.event_outbox")
        .fetch_one(&mut c)
        .await
        .unwrap();
    assert_eq!(backlog, 0);
    let tags: Vec<String> =
        sqlx::query_scalar("SELECT unnest(tags) FROM identity.users WHERE username = 'Alice_01'")
            .fetch_all(&mut c)
            .await
            .unwrap();
    assert_eq!(tags, vec!["probed"]);

    kernel.shutdown().await;
}
