//! 端到端：图片上传（attachments，WASM）—— 识别与元数据去除、公开访问、去重、限额、删除（引用计数）、
//! 种子简介渲染站内图片（torrent `description_html`）。
//!
//! 运行：`cargo test -p pt-server --test e2e_attachments -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use axum::{body::Body, http::StatusCode};
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::json;
use sqlx::{Connection, PgConnection};

use common::{call, env, grant_role, hv, register_login, reset, send, upload};

fn chunk(ty: &[u8], data: &[u8]) -> Vec<u8> {
    let mut c = u32::try_from(data.len()).unwrap().to_be_bytes().to_vec();
    c.extend_from_slice(ty);
    c.extend_from_slice(data);
    c.extend_from_slice(&[0, 0, 0, 0]);
    c
}

fn png(w: u32, seed: u8) -> Vec<u8> {
    let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&64u32.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    b.extend(chunk(b"IHDR", &ihdr));
    b.extend(chunk(b"tEXt", b"Location\0home address 42"));
    b.extend(chunk(b"IDAT", &[seed, 2, 3]));
    b.extend(chunk(b"IEND", &[]));
    b
}

async fn put_image(app: &axum::Router, u: &common::User, body: Vec<u8>, ct: &str) -> common::Resp {
    let h = u.headers();
    let mut hs: Vec<(&str, &str)> = h.iter().map(|(k, v)| (*k, v.as_str())).collect();
    hs.push(("content-type", ct));
    send(app, "POST", "/api/p/attachments/v1/images", &hs, |b| b.body(Body::from(body)).unwrap()).await
}

async fn get_media(app: &axum::Router, url: &str) -> common::Resp {
    send(app, "GET", url, &[], |b| b.body(Body::empty()).unwrap()).await
}

#[tokio::test]
async fn attachments_end_to_end() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!("\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n", dir.display()));
    let kernel = Kernel::start(SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let alice = register_login(&app, "img_alice").await;
    let bob = register_login(&app, "img_bob").await;
    let staff = register_login(&app, "img_staff").await;
    grant_role(staff.id, "moderator").await;

    // 上传：去除文本块，按内容寻址公开访问
    let r = put_image(&app, &alice, png(320, 1), "image/png").await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let url = r.body["url"].as_str().unwrap().to_owned();
    assert!(url.starts_with("/media/attachments/i/") && url.ends_with(".png"), "{url}");
    assert_eq!((r.body["width"].clone(), r.body["height"].clone()), (json!(320), json!(64)));
    assert_eq!(r.body["bbcode"], format!("[img]{url}[/img]"));
    let id = r.body["id"].as_i64().unwrap();
    let m = get_media(&app, &url).await;
    assert_eq!(m.status, StatusCode::OK);
    assert!(m.bytes.starts_with(b"\x89PNG") && !m.bytes.windows(12).any(|x| x == b"home address"), "metadata stripped");
    // 同一用户重复上传返回同一记录；其他用户上传同一文件得到同一地址
    let again = put_image(&app, &alice, png(320, 1), "application/octet-stream").await;
    assert_eq!((again.status, again.body["id"].as_i64()), (StatusCode::CREATED, Some(id)));
    let rb = put_image(&app, &bob, png(320, 1), "image/png").await;
    assert_eq!(rb.body["url"], json!(url));
    let bob_id = rb.body["id"].as_i64().unwrap();
    assert_ne!(bob_id, id);

    // 拒绝：非图片、损坏
    let bad = put_image(&app, &alice, b"<svg onload=alert(1)>".to_vec(), "image/svg+xml").await;
    assert_eq!((bad.status, bad.body["code"].clone()), (StatusCode::UNSUPPORTED_MEDIA_TYPE, json!("attachments.unsupported")));
    let mut broken = png(100, 9);
    broken.truncate(broken.len() - 6);
    assert_eq!(put_image(&app, &alice, broken, "image/png").await.body["code"], "attachments.invalid");

    // 每日数量限制
    sqlx::query("INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('attachments', $1)")
        .bind(json!({ "daily_count": 2 }))
        .execute(&mut PgConnection::connect(&env("PT_TEST_SUPERUSER_URL", "postgres://postgres@127.0.0.1:55432/pt_test")).await.unwrap())
        .await
        .unwrap();
    assert_eq!(put_image(&app, &alice, png(200, 2), "image/png").await.status, StatusCode::CREATED);
    let over = put_image(&app, &alice, png(200, 3), "image/png").await;
    assert_eq!((over.status, over.body["code"].clone()), (StatusCode::TOO_MANY_REQUESTS, json!("attachments.quota_exceeded")));
    let mine = call(&app, "GET", "/api/p/attachments/v1/images", None, &hv(&alice.headers())).await.body;
    assert_eq!((mine["total"].clone(), mine["quota"]["count"].clone()), (json!(2), json!(2)), "{mine:?}");

    // 种子简介：站内图片渲染为 <img>，外链不允许时降级
    let tid = upload(&app, &alice, "Img.Torrent", &[("a.bin", 1 << 20)]).await;
    let desc = format!("截图：\n[img]{url}[/img]\n[img]https://evil.example/x.png[/img]");
    let r = call(&app, "PATCH", &format!("/api/p/torrent/v1/torrents/{tid}"), Some(json!({ "description": desc })), &hv(&alice.headers())).await;
    assert!(r.status.is_success(), "{:?}", r.body);
    let d = call(&app, "GET", &format!("/api/p/torrent/v1/torrents/{tid}"), None, &hv(&bob.headers())).await.body;
    let html = d["description_html"].as_str().unwrap();
    assert!(html.contains(&format!("<img src=\"{url}\"")), "{html}");
    assert!(!html.contains("<img src=\"https://evil.example"), "{html}");

    // 删除：他人不能删；文件在最后一个引用删除后才移除；管理员可删他人的
    assert_eq!(call(&app, "DELETE", &format!("/api/p/attachments/v1/images/{id}"), None, &hv(&bob.headers())).await.status, StatusCode::NOT_FOUND);
    assert_eq!(call(&app, "DELETE", &format!("/api/p/attachments/v1/images/{id}"), None, &hv(&alice.headers())).await.status, StatusCode::NO_CONTENT);
    assert_eq!(get_media(&app, &url).await.status, StatusCode::OK, "bob still uses the file");
    let admin_list = call(&app, "GET", "/api/p/attachments/v1/admin/images", None, &hv(&staff.headers())).await.body;
    assert!(admin_list["items"].as_array().unwrap().iter().any(|x| x["username"] == "img_bob"), "{admin_list:?}");
    assert_eq!(call(&app, "DELETE", &format!("/api/p/attachments/v1/images/{bob_id}"), None, &hv(&staff.headers())).await.status, StatusCode::NO_CONTENT);
    assert_eq!(get_media(&app, &url).await.status, StatusCode::NOT_FOUND);
}
