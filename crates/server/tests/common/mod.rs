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
    // 删除测试库中全部非系统 schema 与插件角色
    let schemas: Vec<String> = sqlx::query_scalar(
        "SELECT nspname FROM pg_namespace WHERE nspname NOT LIKE 'pg\\_%' AND nspname NOT IN ('public', 'information_schema')",
    )
    .fetch_all(&mut c)
    .await
    .unwrap();
    for s in schemas {
        c.execute(format!("DROP SCHEMA IF EXISTS \"{s}\" CASCADE").as_str())
            .await
            .unwrap();
    }
    let roles: Vec<String> =
        sqlx::query_scalar("SELECT rolname FROM pg_roles WHERE rolname LIKE 'pt\\_plugin\\_%'")
            .fetch_all(&mut c)
            .await
            .unwrap();
    for r in roles {
        c.execute(format!("DROP ROLE IF EXISTS \"{r}\"").as_str())
            .await
            .unwrap();
    }
    let client = redis::Client::open(env("PT_TEST_REDIS_URL", "redis://127.0.0.1:56379/15")).unwrap();
    let mut rc = client.get_multiplexed_async_connection().await.unwrap();
    let _: () = redis::cmd("FLUSHDB").query_async(&mut rc).await.unwrap();
    let _ = std::fs::remove_dir_all(blob_dir());
}

pub fn config() -> SiteConfig {
    SiteConfig::parse(&config_text()).unwrap()
}

pub fn config_text() -> String {
    format!(
        r#"
[site]
name = "test"
base_url = "https://pt.test"

[kernel]
mode = "dev"
node_id = "test-node"
# 调试构建且整套测试并行时，WASM 首次实例化较慢
service_timeout_ms = 5000

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

[plugin.invite]
secret = "MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY="

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
    )
}

/// 构建 plugins/wasm 工作区并组装插件目录 `<dir>/<id>/{plugin.toml, plugin.wasm, migrations/}`。
pub fn package_wasm_plugins() -> std::path::PathBuf {
    let ws = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/wasm");
    let st = std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .current_dir(&ws)
        .status()
        .unwrap();
    assert!(st.success(), "building plugins/wasm failed");
    let dir = std::path::PathBuf::from(env("PT_TEST_PLUGINS_DIR", "/tmp/pt-studio-test-plugins"));
    let _ = std::fs::remove_dir_all(&dir);
    for entry in std::fs::read_dir(&ws).unwrap() {
        let src = entry.unwrap().path();
        if !src.join("plugin.toml").exists() {
            continue;
        }
        let id = src.file_name().unwrap().to_string_lossy().into_owned();
        let artifact = format!("pt_wasm_{}.wasm", id.replace('-', "_"));
        let dst = dir.join(&id);
        std::fs::create_dir_all(dst.join("migrations")).unwrap();
        std::fs::copy(src.join("plugin.toml"), dst.join("plugin.toml")).unwrap();
        std::fs::copy(
            ws.join("target/wasm32-wasip2/release").join(artifact),
            dst.join("plugin.wasm"),
        )
        .unwrap();
        if let Ok(rd) = std::fs::read_dir(src.join("migrations")) {
            for f in rd {
                let f = f.unwrap().path();
                std::fs::copy(&f, dst.join("migrations").join(f.file_name().unwrap())).unwrap();
            }
        }
    }
    dir
}

/// 已登录用户：(用户 ID, cookie, csrf)
pub struct User {
    pub id: u64,
    pub cookie: String,
    pub csrf: String,
}

impl User {
    /// 写请求所需的请求头
    pub fn headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("cookie", self.cookie.clone()),
            ("x-csrf-token", self.csrf.clone()),
            ("sec-fetch-site", "same-origin".into()),
        ]
    }
}

pub fn hv<'a>(h: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
    h.iter().map(|(k, v)| (*k, v.as_str())).collect()
}

pub async fn register_login(app: &Router, name: &str) -> User {
    let r = call(app, "POST", "/api/p/identity/v1/users",
        Some(serde_json::json!({"username": name, "email": format!("{name}@example.com"), "password": "correct horse"})), &[]).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let r = call(
        app,
        "POST",
        "/api/p/identity/v1/sessions",
        Some(serde_json::json!({"username": name, "password": "correct horse"})),
        &[],
    )
    .await;
    User {
        id: r.body["user"]["id"].as_u64().unwrap(),
        cookie: r.cookies.join("; "),
        csrf: r.body["csrf_token"].as_str().unwrap().to_owned(),
    }
}

/// 以超级用户直接授予内置角色（测试用）
pub async fn grant_role(user: u64, role: &str) {
    let mut c = PgConnection::connect(&env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap();
    sqlx::query("INSERT INTO identity.user_roles (user_id, role, granted_by) VALUES ($1, $2, 'test')")
        .bind(i64::try_from(user).unwrap())
        .bind(role)
        .execute(&mut c)
        .await
        .unwrap();
}

/// 上传一个 v1 种子，返回种子 ID
pub async fn upload(app: &Router, u: &User, title: &str, files: &[(&str, u64)]) -> i64 {
    let file = pt_plugin_torrent::fixtures::sample_v1(title, files, true);
    let boundary = "b0undary-x";
    let mut body = Vec::new();
    body.extend(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"meta\"\r\n\r\n{}\r\n",
            serde_json::json!({"type_id": "generic", "title": title})
        )
        .as_bytes(),
    );
    body.extend(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"torrent\"; filename=\"t.torrent\"\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend(&file);
    body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
    let h = u.headers();
    let r = send(app, "POST", "/api/p/torrent/v1/torrents", &hv(&h), |b| {
        b.header(
            "content-type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(Body::from(body.clone()))
        .unwrap()
    })
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    r.body["id"].as_i64().unwrap()
}
