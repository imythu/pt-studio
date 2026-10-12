//! 端到端：本人设置（资料、改密码、会话、passkey 重置）与后台管理（用户列表 / 详情 / 编辑 / 撤销会话 / 移除角色；
//! 种子编辑、审核状态、软删除及其事件）。

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::Kernel;
use serde_json::json;
use sqlx::{Connection, PgConnection};

use common::{User, call, env, hv, register_login, reset, upload};

async fn login(app: &axum::Router, name: &str, password: &str) -> common::Resp {
    call(
        app,
        "POST",
        "/api/p/identity/v1/sessions",
        Some(json!({ "username": name, "password": password })),
        &[],
    )
    .await
}

fn user_from(r: &common::Resp) -> User {
    User {
        id: r.body["user"]["id"].as_u64().unwrap(),
        cookie: r.cookies.join("; "),
        csrf: r.body["csrf_token"].as_str().unwrap().to_owned(),
    }
}

#[tokio::test]
async fn settings_and_admin_end_to_end() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let kernel = Kernel::start(common::config(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let mut db = PgConnection::connect(&env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap();
    let admin = register_login(&app, "mg_admin").await;
    common::grant_role(admin.id, "admin").await;
    let alice = register_login(&app, "mg_alice").await;
    let alice2 = user_from(&login(&app, "mg_alice", "correct horse").await);

    // 资料（merge-patch）与 profile_updated
    let r = call(
        &app,
        "PATCH",
        "/api/p/identity/v1/users/me",
        Some(json!({ "title": "  Seeder ", "bio": "hi", "locale": "en" })),
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(
        (r.body["title"].as_str(), r.body["locale"].as_str()),
        (Some("Seeder"), Some("en"))
    );
    let r = call(
        &app,
        "PATCH",
        "/api/p/identity/v1/users/me",
        Some(json!({ "title": null })),
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(
        (r.body["title"].clone(), r.body["bio"].as_str()),
        (serde_json::Value::Null, Some("hi"))
    );
    let r = call(
        &app,
        "PATCH",
        "/api/p/identity/v1/users/me",
        Some(json!({ "avatar_url": "http://x/a.png" })),
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    // 会话列表：两个，当前标记
    let r = call(
        &app,
        "GET",
        "/api/p/identity/v1/sessions",
        None,
        &hv(&alice.headers()),
    )
    .await;
    let items = r.body["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 2);
    assert_eq!(items.iter().filter(|s| s["current"] == true).count(), 1);
    let other = items.iter().find(|s| s["current"] == false).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = call(
        &app,
        "DELETE",
        &format!("/api/p/identity/v1/sessions/{other}"),
        None,
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/p/identity/v1/users/me",
            None,
            &hv(&alice2.headers())
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );

    // passkey 重置：新 passkey 与旧不同；下载用的 passkey 跟着变
    let old = call(
        &app,
        "GET",
        "/api/p/identity/v1/users/me/passkey",
        None,
        &hv(&alice.headers()),
    )
    .await
    .body["passkey"]
        .clone();
    let r = call(
        &app,
        "POST",
        "/api/p/identity/v1/users/me/passkey-resets",
        Some(json!({})),
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.headers["cache-control"], "no-store");
    let new = r.body["passkey"].clone();
    assert_ne!(old, new);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/p/identity/v1/users/me/passkey",
            None,
            &hv(&alice.headers())
        )
        .await
        .body["passkey"],
        new
    );

    // 改密码：错误的当前密码被拒；成功后其他会话失效、当前会话保留、新密码可登录
    let alice3 = user_from(&login(&app, "mg_alice", "correct horse").await);
    let r = call(
        &app,
        "PUT",
        "/api/p/identity/v1/users/me/password",
        Some(json!({ "current_password": "nope", "new_password": "battery staple 42" })),
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (StatusCode::FORBIDDEN, Some("identity.invalid_credentials"))
    );
    let r = call(
        &app,
        "PUT",
        "/api/p/identity/v1/users/me/password",
        Some(json!({ "current_password": "correct horse", "new_password": "battery staple 42" })),
        &hv(&alice.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["revoked_sessions"], 1);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/p/identity/v1/users/me",
            None,
            &hv(&alice3.headers())
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/p/identity/v1/users/me",
            None,
            &hv(&alice.headers())
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        login(&app, "mg_alice", "correct horse").await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&app, "mg_alice", "battery staple 42").await.status,
        StatusCode::OK
    );
    let events: Vec<String> = sqlx::query_scalar(
        "SELECT event_type FROM kernel.event_log WHERE event_type IN ('user.profile_updated', 'user.password_changed', 'user.passkey_reset') ORDER BY seq",
    )
    .fetch_all(&mut db)
    .await
    .unwrap();
    assert_eq!(events.iter().filter(|e| *e == "user.profile_updated").count(), 2);
    assert!(
        events.contains(&"user.password_changed".to_owned())
            && events.contains(&"user.passkey_reset".to_owned())
    );

    // 后台用户：普通用户无权；列表筛选、详情、编辑、撤销会话、移除角色
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/p/identity/v1/admin/users",
            None,
            &hv(&alice.headers())
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "GET",
        "/api/p/identity/v1/admin/users?username_prefix=MG_&limit=1",
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.body["items"].as_array().unwrap().len(), 1, "{:?}", r.body);
    let next = r.body["next_cursor"].as_i64().unwrap();
    let r2 = call(
        &app,
        "GET",
        &format!("/api/p/identity/v1/admin/users?username_prefix=mg_&cursor={next}"),
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r2.body["items"].as_array().unwrap().len(), 1);
    let r = call(
        &app,
        "GET",
        "/api/p/identity/v1/admin/users?role=admin",
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.body["items"][0]["id"].as_u64(), Some(admin.id));
    let r = call(
        &app,
        "GET",
        &format!("/api/p/identity/v1/admin/users/{}", alice.id),
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(
        (r.body["username"].as_str(), r.body["sessions"].as_i64()),
        (Some("mg_alice"), Some(2)),
        "{:?}",
        r.body
    );
    let r = call(
        &app,
        "PATCH",
        &format!("/api/p/identity/v1/admin/users/{}", alice.id),
        Some(json!({ "title": "VIP" })),
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.body["title"], "VIP");
    let r = call(
        &app,
        "POST",
        &format!("/api/p/identity/v1/admin/users/{}/session-revocations", alice.id),
        Some(json!({})),
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.body["revoked"], 2);
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/p/identity/v1/users/me",
            None,
            &hv(&alice.headers())
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    common::grant_role(alice.id, "uploader").await;
    let r = call(
        &app,
        "DELETE",
        &format!("/api/p/identity/v1/admin/users/{}/roles/uploader", alice.id),
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = call(
        &app,
        "DELETE",
        &format!("/api/p/identity/v1/admin/users/{}/roles/admin", admin.id),
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::FORBIDDEN,
        "cannot remove a role at or above own rank"
    );

    // 种子：本人编辑 → torrent.updated；他人不可编辑；审核状态与软删除
    let bob = register_login(&app, "mg_bob").await;
    let tid = upload(&app, &bob, "Manage.Test", &[("a.bin", 10_000)]).await;
    let path = format!("/api/p/torrent/v1/torrents/{tid}");
    let r = call(
        &app,
        "PATCH",
        &path,
        Some(json!({ "title": "Manage Test (edited)", "subtitle": "sub" })),
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "admin has torrent.update");
    let mallory = register_login(&app, "mg_mallory").await;
    assert_eq!(
        call(
            &app,
            "PATCH",
            &path,
            Some(json!({ "title": "x" })),
            &hv(&mallory.headers())
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "PATCH",
        &path,
        Some(json!({ "description": "new text" })),
        &hv(&bob.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let d = call(&app, "GET", &path, None, &hv(&bob.headers())).await.body;
    assert_eq!(
        (
            d["title"].as_str(),
            d["subtitle"].as_str(),
            d["description"].as_str()
        ),
        (Some("Manage Test (edited)"), Some("sub"), Some("new text"))
    );
    let st = format!("/api/p/torrent/v1/admin/torrents/{tid}/status");
    assert_eq!(
        call(
            &app,
            "POST",
            &st,
            Some(json!({ "to": "disabled" })),
            &hv(&bob.headers())
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    let r = call(
        &app,
        "POST",
        &st,
        Some(json!({ "to": "disabled", "reason": "dead" })),
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    assert_eq!(
        call(&app, "GET", &path, None, &hv(&mallory.headers())).await.body["can_download"],
        false
    );
    assert_eq!(
        call(
            &app,
            "POST",
            &st,
            Some(json!({ "to": "bogus" })),
            &hv(&admin.headers())
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(
        &app,
        "DELETE",
        &format!("/api/p/torrent/v1/admin/torrents/{tid}?reason=dupe"),
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        call(&app, "GET", &path, None, &hv(&bob.headers())).await.status,
        StatusCode::NOT_FOUND
    );
    let evs: Vec<(String, serde_json::Value)> = sqlx::query_as(
        "SELECT event_type, payload FROM kernel.event_log WHERE event_type LIKE 'torrent.%' AND event_type <> 'torrent.created' ORDER BY seq",
    )
    .fetch_all(&mut db)
    .await
    .unwrap();
    let kinds: Vec<&str> = evs.iter().map(|e| e.0.as_str()).collect();
    assert_eq!(
        kinds,
        vec![
            "torrent.updated",
            "torrent.updated",
            "torrent.status_changed",
            "torrent.status_changed",
            "torrent.deleted"
        ]
    );
    assert_eq!(evs[2].1["to"], "disabled");
    assert_eq!(evs[3].1["to"], "deleted");
    kernel.shutdown().await;
}
