//! 端到端：网关限流（GCRA）与幂等键。

#![allow(clippy::unwrap_used)]

mod common;

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use axum::{Json, Router, extract::State, http::StatusCode, response::IntoResponse, routing::post};
use pt_kernel::Kernel;
use pt_plugin_api::{HttpRoutes, NativePlugin, PResult, PluginError, RequestCtx, json};
use serde_json::Value;

use common::{call, config, reset};

const MANIFEST: &str = r#"
id = "x-gate"
name = "gate"
version = "0.1.0"
kind = "native"
api = "pt:plugin@0.1"
license = "MIT OR Apache-2.0"

[[http.routes]]
id = "counter"
method = "POST"
path = "/v1/counter"
auth = "user"
rate_limit = "user:4/m"
idempotent = true
"#;

struct Gate(Arc<AtomicU32>);

async fn counter(
    State(n): State<Arc<AtomicU32>>,
    _ctx: RequestCtx,
    Json(b): Json<Value>,
) -> PResult<axum::response::Response> {
    if b["fail"] == true {
        return Err(PluginError::internal("boom"));
    }
    let v = n.fetch_add(1, Ordering::SeqCst) + 1;
    Ok((StatusCode::CREATED, Json(json!({ "n": v, "echo": b }))).into_response())
}

impl HttpRoutes for Gate {
    fn router(&self) -> Router {
        Router::new()
            .route("/v1/counter", post(counter))
            .with_state(self.0.clone())
    }
}

#[tokio::test]
async fn gateway_limits_and_idempotency() {
    let _guard = common::LOCK.lock().await;
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter("info,sqlx=warn")
        .try_init();
    reset().await;
    let mut plugins = pt_server::native_plugins();
    plugins.push(NativePlugin {
        manifest: MANIFEST,
        http: Some(Arc::new(Gate(Arc::new(AtomicU32::new(0))))),
        ..Default::default()
    });
    let kernel = Kernel::start(config(), plugins)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();

    call(
        &app,
        "POST",
        "/api/p/identity/v1/users",
        Some(json!({"username": "gater", "email": "g@example.com", "password": "correct horse"})),
        &[],
    )
    .await;
    let login = call(
        &app,
        "POST",
        "/api/p/identity/v1/sessions",
        Some(json!({"username": "gater", "password": "correct horse"})),
        &[],
    )
    .await;
    let cookie = login.cookies.join("; ");
    let csrf = login.body["csrf_token"].as_str().unwrap().to_owned();
    let hdr = |key: &'static str| {
        vec![
            ("cookie", cookie.clone()),
            ("x-csrf-token", csrf.clone()),
            ("sec-fetch-site", "same-origin".into()),
            ("idempotency-key", key.into()),
        ]
    };
    let post = |h: Vec<(&'static str, String)>, body: Value| {
        let app = app.clone();
        async move {
            let hv: Vec<(&str, &str)> = h.iter().map(|(k, v)| (*k, v.as_str())).collect();
            call(&app, "POST", "/api/p/x-gate/v1/counter", Some(body), &hv).await
        }
    };

    // 首次执行；同键同体重放：不再执行，原样返回并带 Idempotent-Replayed
    let r1 = post(hdr("k1"), json!({"a": 1})).await;
    assert_eq!(
        (r1.status, r1.body["n"].as_u64()),
        (StatusCode::CREATED, Some(1)),
        "{:?}",
        r1.body
    );
    assert_eq!(r1.headers["ratelimit-policy"], "\"user:4/m\"");
    assert!(
        r1.headers["ratelimit"]
            .to_str()
            .unwrap()
            .starts_with("remaining=3")
    );
    let r2 = post(hdr("k1"), json!({"a": 1})).await;
    assert_eq!((r2.status, r2.body["n"].as_u64()), (StatusCode::CREATED, Some(1)));
    assert_eq!(r2.headers["idempotent-replayed"], "true");
    // 同键不同体
    let r3 = post(hdr("k1"), json!({"a": 2})).await;
    assert_eq!(
        (r3.status, r3.body["code"].as_str()),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Some("kernel.idempotency_key_reused")
        )
    );
    // 服务端错误不缓存：同键可重试
    let r4 = post(hdr("k2"), json!({"fail": true})).await;
    assert_eq!(r4.status, StatusCode::INTERNAL_SERVER_ERROR);

    // 限流：每分钟 4 次。限流先于幂等检查（kernel/03 §8.2），因此重放与冲突请求同样计数：前 4 个请求已用完额度
    let r5 = post(hdr("k3"), json!({"b": 1})).await;
    assert_eq!(r5.status, StatusCode::TOO_MANY_REQUESTS, "{:?}", r5.body);
    assert_eq!(r5.body["code"], "kernel.rate_limited");
    assert!(
        r5.headers["retry-after"]
            .to_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            >= 1
    );

    kernel.shutdown().await;
}
