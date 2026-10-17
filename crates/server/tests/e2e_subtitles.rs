//! 端到端：字幕区（subtitles，WASM）—— 上传校验（格式、ZIP 条目、重复）、列表与匿名、下载与计数、上传奖励、
//! 删除权限（管理员需原因）、种子删除时清理。
//!
//! 运行：`cargo test -p pt-server --test e2e_subtitles -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use axum::{body::Body, http::StatusCode};
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{call, env, grant_role, hv, register_login, reset, send, upload};

const SRT: &str = "1\n00:00:01,000 --> 00:00:02,500\n你好，世界\n";

fn zip(names: &[(&str, u32)]) -> Vec<u8> {
    let mut b = b"PK\x03\x04".to_vec();
    b.extend_from_slice(&[0; 26]);
    let cd = u32::try_from(b.len()).unwrap();
    for (name, size) in names {
        b.extend_from_slice(b"PK\x01\x02");
        b.extend_from_slice(&[0; 20]);
        b.extend_from_slice(&size.to_le_bytes());
        b.extend_from_slice(&u16::try_from(name.len()).unwrap().to_le_bytes());
        b.extend_from_slice(&[0; 16]);
        b.extend_from_slice(name.as_bytes());
    }
    let cd_len = u32::try_from(b.len()).unwrap() - cd;
    b.extend_from_slice(b"PK\x05\x06\0\0\0\0");
    let n = u16::try_from(names.len()).unwrap();
    b.extend_from_slice(&n.to_le_bytes());
    b.extend_from_slice(&n.to_le_bytes());
    b.extend_from_slice(&cd_len.to_le_bytes());
    b.extend_from_slice(&cd.to_le_bytes());
    b.extend_from_slice(&[0; 2]);
    b
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"._-".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

async fn put_sub(app: &axum::Router, u: &common::User, tid: i64, name: &str, lang: &str, extra: &str, body: Vec<u8>) -> common::Resp {
    let h = u.headers();
    let mut hs: Vec<(&str, &str)> = h.iter().map(|(k, v)| (*k, v.as_str())).collect();
    hs.push(("content-type", "application/octet-stream"));
    let uri = format!("/api/p/subtitles/v1/torrents/{tid}/subtitles?filename={}&lang={lang}{extra}", enc(name));
    send(app, "POST", &uri, &hs, |b| b.body(Body::from(body)).unwrap()).await
}

async fn db() -> PgConnection {
    PgConnection::connect(&env("PT_TEST_SUPERUSER_URL", "postgres://postgres@127.0.0.1:55432/pt_test")).await.unwrap()
}

async fn bonus(app: &axum::Router, u: &common::User) -> Value {
    let b = call(app, "GET", "/api/p/ledger/v1/users/me/balances", None, &hv(&u.headers())).await.body;
    b.as_array().or(b["items"].as_array()).unwrap().iter().find(|x| x["currency"] == "bonus").map_or(json!(0), |x| x["balance"].clone())
}

#[tokio::test]
async fn subtitles_end_to_end() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!("\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n", dir.display()));
    let kernel = Kernel::start(SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let alice = register_login(&app, "sub_alice").await;
    let bob = register_login(&app, "sub_bob").await;
    let staff = register_login(&app, "sub_staff").await;
    grant_role(staff.id, "moderator").await;
    sqlx::query("INSERT INTO kernel.plugin_settings (plugin_id, settings) VALUES ('subtitles', $1)")
        .bind(json!({ "reward_bonus": 7 }))
        .execute(&mut db().await)
        .await
        .unwrap();
    let tid = upload(&app, &alice, "Sub.Movie.2024.1080p", &[("a.mkv", 1 << 20)]).await;
    let before = bonus(&app, &bob).await.as_i64().unwrap_or(0);

    // 上传：中文文件名、匿名、编码识别；发放奖励
    let r = put_sub(&app, &bob, tid, "C:\\subs\\简体中文.srt", "zh-hans", "&title=%E5%AE%98%E6%96%B9&anonymous=1", SRT.as_bytes().to_vec()).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    assert_eq!((r.body["format"].clone(), r.body["encoding"].clone(), r.body["filename"].clone()), (json!("srt"), json!("utf-8"), json!("简体中文.srt")));
    let sid = r.body["id"].as_i64().unwrap();
    assert_eq!(bonus(&app, &bob).await.as_i64().unwrap_or(0), before + 7, "upload reward");
    // 重复、内容不符、压缩包含可执行文件、不支持的格式
    assert_eq!(put_sub(&app, &bob, tid, "again.srt", "zh-hans", "", SRT.as_bytes().to_vec()).await.body["code"], "subtitles.duplicate");
    let bad = put_sub(&app, &bob, tid, "x.srt", "en", "", b"<html>not a subtitle</html>".to_vec()).await;
    assert_eq!((bad.status, bad.body["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("subtitles.invalid_file")), "{:?}", bad.body);
    let evil = put_sub(&app, &bob, tid, "pack.zip", "en", "", zip(&[("pack/en.srt", 100), ("pack/setup.exe", 100)])).await;
    assert!(evil.body["title"].as_str().unwrap_or("").contains("not subtitles"), "{:?}", evil.body);
    assert_eq!(put_sub(&app, &bob, tid, "pack.rar", "en", "", b"Rar!\x1a\x07".to_vec()).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(put_sub(&app, &bob, tid, "x.srt", "xx", "", SRT.as_bytes().to_vec()).await.status, StatusCode::UNPROCESSABLE_ENTITY, "unknown language");
    let pack = put_sub(&app, &alice, tid, "pack.zip", "zh-hans-en", "", zip(&[("pack/", 0), ("pack/chs.ass", 4000), ("pack/cht.ass", 4000)])).await;
    assert_eq!(pack.status, StatusCode::CREATED, "{:?}", pack.body);
    let pack_id = pack.body["id"].as_i64().unwrap();

    // 列表：匿名对他人隐藏上传者，对本人与管理员可见
    let path = format!("/api/p/subtitles/v1/torrents/{tid}/subtitles");
    let l = call(&app, "GET", &path, None, &hv(&alice.headers())).await.body;
    let items = l["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "{l:?}");
    let mine = items.iter().find(|x| x["id"] == sid).unwrap();
    assert_eq!((mine["uploader"].clone(), mine["anonymous"].clone(), mine["can_delete"].clone(), mine["title"].clone()), (Value::Null, json!(true), json!(false), json!("官方")));
    let packed = items.iter().find(|x| x["id"] == pack_id).unwrap();
    assert_eq!(packed["entries"].as_array().unwrap().len(), 2);
    let l = call(&app, "GET", &path, None, &hv(&staff.headers())).await.body;
    assert_eq!(l["items"].as_array().unwrap().iter().find(|x| x["id"] == sid).unwrap()["uploader"], "sub_bob");

    // 下载：原样返回、附件文件名、计数
    let d = send(&app, "GET", &format!("/api/p/subtitles/v1/subtitles/{sid}/download"), &hv(&alice.headers()), |b| b.body(Body::empty()).unwrap()).await;
    assert_eq!(d.status, StatusCode::OK);
    assert_eq!(d.bytes, SRT.as_bytes());
    let cd = d.headers.get("content-disposition").unwrap().to_str().unwrap().to_owned();
    assert!(cd.starts_with("attachment;") && cd.contains("filename*=UTF-8''%E7%AE%80"), "{cd}");
    let l = call(&app, "GET", &path, None, &hv(&alice.headers())).await.body;
    assert_eq!(l["items"].as_array().unwrap().iter().find(|x| x["id"] == sid).unwrap()["downloads"], 1);

    // 删除：他人不能；管理员需要原因；上传者可直接删
    let del = |u: &common::User, id: i64, reason: &str| {
        let app = app.clone();
        let h = u.headers();
        let uri = format!("/api/p/subtitles/v1/subtitles/{id}?reason={}", enc(reason));
        async move { call(&app, "DELETE", &uri, None, &hv(&h)).await }
    };
    assert_eq!(del(&alice, sid, "").await.status, StatusCode::FORBIDDEN);
    assert_eq!(del(&staff, sid, "").await.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(del(&staff, sid, "时间轴错误").await.status, StatusCode::NO_CONTENT);
    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM kernel.audit_log WHERE action = 'subtitles.delete'")
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert_eq!(audit, 1);
    assert_eq!(send(&app, "GET", &format!("/api/p/subtitles/v1/subtitles/{sid}/download"), &hv(&alice.headers()), |b| b.body(Body::empty()).unwrap()).await.status, StatusCode::NOT_FOUND);
    let admin = call(&app, "GET", "/api/p/subtitles/v1/admin/subtitles", None, &hv(&staff.headers())).await.body;
    assert_eq!(admin["items"][0]["id"], json!(pack_id), "{admin:?}");

    // 种子删除时一并删除
    let r = call(&app, "DELETE", &format!("/api/p/torrent/v1/admin/torrents/{tid}?reason=dupe"), None, &hv(&staff.headers())).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let mut gone = false;
    for _ in 0..50 {
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM subtitles.subtitles WHERE deleted_at IS NULL")
            .fetch_one(&mut db().await)
            .await
            .unwrap();
        if n == 0 {
            gone = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(gone);
}
