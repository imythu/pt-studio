//! 端到端测试共用：清理测试库、站点配置、HTTP 调用。

#![allow(dead_code, clippy::unwrap_used)]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use pt_kernel::config::SiteConfig;
use serde_json::Value;
use sqlx::{Connection, Executor, PgConnection};
use tower::ServiceExt;

/// 各测试共用同一个数据库，串行执行
pub static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn env(k: &str, d: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| d.to_owned())
}

pub fn blob_dir() -> String {
    env("PT_TEST_BLOB_DIR", "/tmp/pt-studio-test-blob")
}

pub struct Resp {
    pub status: StatusCode,
    pub cookies: Vec<String>,
    pub headers: axum::http::HeaderMap,
    pub bytes: Vec<u8>,
    pub body: Value,
}

pub async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> Resp {
    let req = |b: axum::http::request::Builder| match &body {
        Some(v) => b
            .header("content-type", "application/json")
            .body(Body::from(v.to_string()))
            .unwrap(),
        None => b.body(Body::empty()).unwrap(),
    };
    send(app, method, uri, headers, req).await
}

pub async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    build: impl FnOnce(axum::http::request::Builder) -> Request<Body>,
) -> Resp {
    let mut b = Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    let resp = app.clone().oneshot(build(b)).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let cookies = headers
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_owned())
        .collect();
    let bytes = to_bytes(resp.into_body(), 16 << 20).await.unwrap().to_vec();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Resp {
        status,
        cookies,
        headers,
        bytes,
        body,
    }
}

pub async fn reset() {
    let su = env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    );
    let mut c = PgConnection::connect(&su).await.unwrap();
    for s in ["kernel", "identity", "torrent", "x_probe", "ext"] {
        c.execute(format!("DROP SCHEMA IF EXISTS {s} CASCADE").as_str())
            .await
            .unwrap();
    }
    for r in ["pt_plugin_identity", "pt_plugin_torrent", "pt_plugin_x_probe"] {
        c.execute(format!("DROP ROLE IF EXISTS {r}").as_str())
            .await
            .unwrap();
    }
    let client = redis::Client::open(env("PT_TEST_REDIS_URL", "redis://127.0.0.1:56379/15")).unwrap();
    let mut rc = client.get_multiplexed_async_connection().await.unwrap();
    let _: () = redis::cmd("FLUSHDB").query_async(&mut rc).await.unwrap();
    let _ = std::fs::remove_dir_all(blob_dir());
}

pub fn config() -> SiteConfig {
    let text = format!(
        r#"
[site]
name = "test"
base_url = "https://pt.test"

[kernel]
mode = "dev"
node_id = "test-node"

[database]
url = "{gw}"
admin_url = "{admin}"
pool_size = 8

[redis.main]
url = "{redis}"

[events]
default_partitions = 2

[grants.identity]
sensitive = ["user.ip", "user.email"]

[blob]
dir = "{blob}"

[plugin.identity]
secret_key = "{key}"

[plugin.torrent]
announce_base_urls = ["https://tracker.test/announce"]
types = ["generic", "movie"]
"#,
        blob = blob_dir(),
        gw = env(
            "PT_TEST_GATEWAY_URL",
            "postgres://pt_gateway:pt_gateway@127.0.0.1:55432/pt_test"
        ),
        admin = env(
            "PT_TEST_ADMIN_URL",
            "postgres://pt_admin:pt_admin@127.0.0.1:55432/pt_test"
        ),
        redis = env("PT_TEST_REDIS_URL", "redis://127.0.0.1:56379/15"),
        key = "11".repeat(32),
    );
    SiteConfig::parse(&text).unwrap()
}
