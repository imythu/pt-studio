//! 端到端：Web 前端（真实浏览器）—— 内核提供 SPA 与插件前端、CSP nonce、前端清单与术语；
//! 浏览器脚本见 web/tests/e2e.mjs。缺少 node / Playwright / Chromium 时跳过。

#![allow(clippy::unwrap_used)]

mod common;

use std::{net::SocketAddr, path::PathBuf, process::Command, time::Duration};

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::json;
use sqlx::{Connection, PgConnection};

use common::{call, env, hv, register_login, reset, upload};

fn web_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web")
}

fn config(plugins: &std::path::Path, tmdb: &str, port: u16) -> SiteConfig {
    // 浏览器经 http://localhost:<port> 访问：WebAuthn 的来源与 rp_id 按实际地址
    let mut t = common::config_text().replace(
        "[plugin.identity]\n",
        &format!("[plugin.identity]\nwebauthn_origin = \"http://localhost:{port}\"\n"),
    );
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n",
        plugins.display()
    ));
    t.push_str(&format!(
        "\n[plugin.metadata-tmdb]\napi_key = \"testkey\"\napi_base_url = \"{tmdb}\"\nimage_base_url = \"{tmdb}\"\n"
    ));
    // [http] 段放在 config_text 之后追加；两个浏览器账号都来自 127.0.0.1，共用按 IP 的全站限流，测试里放宽
    t.push_str(&format!(
        "\n[http]\nweb_dir = \"{}\"\n\n[http.rate_limit]\ndefault = \"ip:6000/m\"\n",
        web_root().join("dist").display()
    ));
    SiteConfig::parse(&t).unwrap()
}

fn tooling_ready() -> bool {
    let node = Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    node && web_root().join("node_modules/playwright").exists()
}

#[tokio::test]
async fn web_end_to_end() {
    if !tooling_ready() {
        eprintln!("SKIPPED: node or web/node_modules/playwright is missing");
        return;
    }
    let _guard = common::LOCK.lock().await;
    let st = Command::new("node")
        .arg("tools/build.mjs")
        .current_dir(web_root())
        .status()
        .unwrap();
    assert!(st.success(), "web build failed");
    let dir = common::package_wasm_plugins();
    reset().await;
    let tmdb = common::mock_tmdb(std::sync::Arc::default()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let kernel = Kernel::start(config(&dir, &tmdb, port), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let mut db = PgConnection::connect(&env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap();
    sqlx::query("INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('invite', $1)")
        .bind(json!({ "min_account_days": 0, "initial_quota": { "amount": 2 } }))
        .execute(&mut db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('lottery', $1)")
        .bind(json!({ "enabled": true, "min_account_days": 0 }))
        .execute(&mut db)
        .await
        .unwrap();
    sqlx::query("INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('donations', $1)")
        .bind(json!({ "enabled": true, "monthly_goal": 100, "bonus_per_unit": 2 }))
        .execute(&mut db)
        .await
        .unwrap();

    // 服务端检查：首页带 CSP nonce，清单含插件前端，术语可取，路径穿越被拒
    let r = call(&app, "GET", "/", None, &[]).await;
    assert_eq!(r.status, StatusCode::OK);
    let html = String::from_utf8(r.bytes.clone()).unwrap();
    let csp = r.headers["content-security-policy"].to_str().unwrap().to_owned();
    let nonce = csp
        .split("'nonce-")
        .nth(1)
        .unwrap()
        .split('\'')
        .next()
        .unwrap()
        .to_owned();
    assert!(
        html.contains(&format!("nonce=\"{nonce}\"")),
        "import map must carry the CSP nonce"
    );
    assert!(!html.contains("__CSP_NONCE__"));
    let r2 = call(&app, "GET", "/torrents/5", None, &[]).await;
    assert_ne!(
        String::from_utf8(r2.bytes)
            .unwrap()
            .split("nonce=\"")
            .nth(1)
            .map(|s| &s[..32]),
        Some(nonce.as_str()),
        "nonce must be per response"
    );
    let m = call(&app, "GET", "/api/kernel/v1/frontend/manifest", None, &[]).await;
    let invite = m.body["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == "invite")
        .unwrap()
        .clone();
    let entry = invite["entry"].as_str().unwrap().to_owned();
    assert_eq!(invite["routes"], json!(["/p/invite/*", "/admin/p/invite/*"]));
    // 入口路径带「版本-内容哈希」：前端改了而插件版本没变时浏览器也会拿到新包
    let tag = entry.split('/').nth(4).unwrap();
    assert!(tag.starts_with("0.1.0-") && tag.len() == "0.1.0-".len() + 12, "{entry}");
    let js = call(&app, "GET", &entry, None, &[]).await;
    assert_eq!(js.status, StatusCode::OK);
    assert!(
        js.headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/javascript")
    );
    assert!(
        js.headers["cache-control"]
            .to_str()
            .unwrap()
            .contains("immutable")
    );
    let terms = call(&app, "GET", "/api/kernel/v1/terms?locale=en", None, &[]).await;
    assert_eq!(terms.body["terms"]["invite.code"]["name"], "Invite code");
    assert_eq!(
        call(
            &app,
            "GET",
            "/static/plugins/invite/0.1.0/../../../Cargo.toml",
            None,
            &[]
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(&app, "GET", "/api/nope", None, &[]).await.status,
        StatusCode::NOT_FOUND
    );
    // 后台设置接口：未登录 401
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/kernel/v1/admin/plugins/invite/settings",
            None,
            &[]
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );

    // 浏览器场景的数据
    let admin = register_login(&app, "web_admin").await;
    common::grant_role(admin.id, "admin").await;
    let alice = register_login(&app, "web_alice").await;

    // 外观：默认用第一个主题插件（theme-anime）；可切回经典；非管理员不能改；未知主题被拒
    let theme = m.body["theme"].clone();
    assert_eq!(theme["id"], "theme-anime", "{theme:?}");
    let css = call(&app, "GET", theme["css"].as_str().unwrap(), None, &[]).await;
    assert_eq!(css.status, StatusCode::OK);
    assert!(css.headers["content-type"].to_str().unwrap().starts_with("text/css"));
    let appearance = "/api/kernel/v1/admin/appearance";
    assert_eq!(
        call(&app, "PUT", appearance, Some(json!({ "theme": "classic" })), &hv(&alice.headers())).await.status,
        StatusCode::FORBIDDEN
    );
    let r = call(&app, "GET", appearance, None, &hv(&admin.headers())).await;
    assert_eq!(r.body["theme"], "theme-anime", "{:?}", r.body);
    assert!(r.body["themes"].as_array().unwrap().iter().any(|t| t["id"] == "theme-anime"));
    let put = |theme: &'static str| {
        let app = app.clone();
        let h = admin.headers();
        async move { call(&app, "PUT", appearance, Some(json!({ "theme": theme })), &hv(&h)).await }
    };
    assert_eq!(put("classic").await.status, StatusCode::NO_CONTENT);
    let m2 = call(&app, "GET", "/api/kernel/v1/frontend/manifest", None, &[]).await;
    assert_eq!(m2.body["theme"], serde_json::Value::Null);
    assert_eq!(put("invite").await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(put("theme-anime").await.status, StatusCode::NO_CONTENT);
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM kernel.audit_log WHERE action = 'kernel.appearance_update'")
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(n, 2);

    // 主题覆盖术语：修道院主题启用期间等级改称修为境界，切回后恢复原名；术语快照不做共享缓存
    assert_eq!(put("theme-xiudaoyuan").await.status, StatusCode::NO_CONTENT);
    let m3 = call(&app, "GET", "/api/kernel/v1/frontend/manifest", None, &[]).await;
    assert_eq!(m3.body["theme"]["terms"], true, "{:?}", m3.body["theme"]);
    let zh = call(&app, "GET", "/api/kernel/v1/terms?locale=zh-CN", None, &[]).await;
    assert_eq!(zh.headers["cache-control"], "no-cache");
    assert_eq!(zh.body["terms"]["levels.class.administrator.name"]["name"], "仙人");
    assert_eq!(zh.body["terms"]["levels.class.elite_user.name"]["name"], "渡劫二重");
    let en = call(&app, "GET", "/api/kernel/v1/terms?locale=en", None, &[]).await;
    assert_eq!(en.body["terms"]["levels.class.administrator.name"]["name"], "Immortal");
    assert_eq!(put("theme-anime").await.status, StatusCode::NO_CONTENT);
    let zh = call(&app, "GET", "/api/kernel/v1/terms?locale=zh-CN", None, &[]).await;
    assert_eq!(zh.body["terms"]["levels.class.administrator.name"]["name"], "管理员");
    let m4 = call(&app, "GET", "/api/kernel/v1/frontend/manifest", None, &[]).await;
    assert_eq!(m4.body["theme"]["terms"], false);

    // 站点信息：默认来自 site.toml；管理员可改站名与字标，清单立即生效；非管理员不能改；空名、超长被拒
    assert_eq!(m4.body["site"]["name"], "test");
    assert_eq!(m4.body["site"]["mark"], json!(["test", ""]));
    let site = "/api/kernel/v1/admin/site";
    assert_eq!(
        call(&app, "PUT", site, Some(json!({ "name": "x" })), &hv(&alice.headers())).await.status,
        StatusCode::FORBIDDEN
    );
    let r = call(&app, "GET", site, None, &hv(&admin.headers())).await;
    assert_eq!(r.body["default"]["name"], "test", "{:?}", r.body);
    for bad in [json!({ "name": "  " }), json!({ "name": "长".repeat(41) }), json!({ "name": "仙门", "mark": ["", "x"] })] {
        let r = call(&app, "PUT", site, Some(bad.clone()), &hv(&admin.headers())).await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    }
    let r = call(&app, "PUT", site, Some(json!({ "name": " 仙门 PT ", "mark": ["仙门", "PT"] })), &hv(&admin.headers())).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let m5 = call(&app, "GET", "/api/kernel/v1/frontend/manifest", None, &[]).await;
    assert_eq!(m5.body["site"]["name"], "仙门 PT");
    assert_eq!(m5.body["site"]["mark"], json!(["仙门", "PT"]));
    let r = call(&app, "PUT", site, Some(json!({ "name": "test" })), &hv(&admin.headers())).await;
    assert_eq!(r.body["site"]["mark"], json!(["test", ""]));
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM kernel.audit_log WHERE action = 'kernel.site_update'")
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(n, 2);

    let tid = upload(
        &app,
        &alice,
        "Web.Test.Torrent.2026.1080p",
        &[("a.mkv", 700 << 20)],
    )
    .await;
    // levels 预设（同 NexusPHP）中“用户”不能发邀，放到高级用户（等级行由 user.registered 异步写入，下面的重试一并等待）
    let mut code = None;
    for _ in 0..100 {
        sqlx::query("UPDATE levels.user_levels SET class_id = 30 WHERE user_id = $1")
            .bind(alice.id as i64)
            .execute(&mut db)
            .await
            .unwrap();
        let r = call(
            &app,
            "POST",
            "/api/p/invite/v1/users/me/invites",
            Some(json!({ "request_id": "web1" })),
            &hv(&alice.headers()),
        )
        .await;
        if r.status == StatusCode::CREATED || r.status == StatusCode::OK {
            code = r.body["code"].as_str().map(str::to_owned);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let code = code.expect("alice could not issue an invite");

    let torrent_file2 = std::env::temp_dir().join(format!("pt-web-e2e-{}-2.torrent", std::process::id()));
    std::fs::write(
        &torrent_file2,
        pt_plugin_torrent::fixtures::sample_v1("Matrix", &[("x.mkv", 600 << 20)], true),
    )
    .unwrap();
    let torrent_file = std::env::temp_dir().join(format!("pt-web-e2e-{}.torrent", std::process::id()));
    std::fs::write(
        &torrent_file,
        pt_plugin_torrent::fixtures::sample_v1("Web.Movie", &[("m.mkv", 600 << 20)], true),
    )
    .unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .unwrap();
    });
    let out = tokio::process::Command::new("node")
        .arg("tests/e2e.mjs")
        .current_dir(web_root())
        .env("PT_BASE", format!("http://localhost:{port}"))
        .env("PT_INVITE_CODE", &code)
        .env("PT_TORRENT_ID", tid.to_string())
        .env("PT_ADMIN_USER", "web_admin")
        .env("PT_TORRENT_FILE", &torrent_file)
        .env("PT_TORRENT_FILE2", &torrent_file2)
        .output()
        .await
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "browser e2e failed\n{stdout}\n{stderr}");
    assert!(stdout.contains("e2e ok"), "{stdout}");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM invite.redemptions")
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(n, 1, "registration through the browser must redeem the invite");
    server.abort();
    kernel.shutdown().await;
}
