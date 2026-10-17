//! 端到端：运维接口 —— 管理操作日志查询（权限、筛选、游标翻页）、Prometheus 指标（HTTP 请求、插件自定义指标前缀）。
//!
//! 运行：`cargo test -p pt-server --test e2e_ops -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::Kernel;
use serde_json::json;

use common::{call, config, hv, register_login, reset, send};

#[tokio::test]
async fn audit_log_and_metrics() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let kernel = Kernel::start(config(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let admin = register_login(&app, "ops_admin").await;
    let user = register_login(&app, "ops_user").await;
    let other = register_login(&app, "ops_other").await;
    common::grant_role(admin.id, "admin").await;

    // 产生审计：施加 / 解除限制
    for target in [user.id, other.id] {
        let r = call(&app, "POST", &format!("/api/p/identity/v1/admin/users/{target}/restrictions"),
            Some(json!({ "kind": "leech_disabled", "reason_code": "ops" })), &hv(&admin.headers())).await;
        assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    }

    // 普通用户无权查看
    let r = call(&app, "GET", "/api/kernel/v1/admin/audit", None, &hv(&user.headers())).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    // 动作前缀 + 操作者筛选
    let r = call(&app, "GET", &format!("/api/kernel/v1/admin/audit?action=identity.restriction&actor={}", admin.id), None, &hv(&admin.headers())).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let items = r.body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "{:?}", r.body);
    assert!(items.iter().all(|e| e["action"] == "identity.restriction_apply" && e["actor"]["user_id"] == admin.id));
    assert!(items[0]["id"].as_i64() > items[1]["id"].as_i64(), "newest first");
    // 目标筛选
    let r = call(&app, "GET", &format!("/api/kernel/v1/admin/audit?target=user:{}", other.id), None, &hv(&admin.headers())).await;
    let items = r.body["items"].as_array().unwrap();
    assert!(!items.is_empty() && items.iter().all(|e| e["target"] == format!("user:{}", other.id)), "{:?}", r.body);
    // 游标翻页
    let r = call(&app, "GET", "/api/kernel/v1/admin/audit?action=identity.&limit=1", None, &hv(&admin.headers())).await;
    let next = r.body["next_before"].as_i64().unwrap();
    let r2 = call(&app, "GET", &format!("/api/kernel/v1/admin/audit?action=identity.&limit=1&before={next}"), None, &hv(&admin.headers())).await;
    assert!(r2.body["items"][0]["id"].as_i64().unwrap() < r.body["items"][0]["id"].as_i64().unwrap());
    // LIKE 通配符被转义：`_` 不当作任意字符
    let r = call(&app, "GET", "/api/kernel/v1/admin/audit?action=identity_restriction", None, &hv(&admin.headers())).await;
    assert_eq!(r.body["items"].as_array().unwrap().len(), 0, "{:?}", r.body);

    // 指标：HTTP 请求计数与耗时
    let m = send(&pt_kernel::metrics::router(), "GET", "/metrics", &[], |b| b.body(axum::body::Body::empty()).unwrap()).await;
    let text = String::from_utf8(m.bytes).unwrap();
    assert!(text.contains("# TYPE pt_http_requests_total counter"), "{text}");
    assert!(text.contains("pt_http_requests_total{plugin=\"identity\",route=\"/api/p/identity/v1/users\",status=\"201\"}"), "{text}");
    assert!(text.contains("pt_http_request_duration_seconds_bucket{plugin=\"identity\",route=\"/api/p/identity/v1/sessions\",le=\"+Inf\"}"), "{text}");
}
