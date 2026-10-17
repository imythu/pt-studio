//! 端到端：MediaInfo（mediainfo，WASM）—— 预览解析、脱敏、发种者保存与权限、与声明字段比较、服务调用、种子删除清理。
//!
//! 运行：`cargo test -p pt-server --test e2e_mediainfo -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::json;
use sqlx::{Connection, PgConnection};

use common::{call, env, grant_role, hv, register_login, reset, upload};

const REPORT: &str = "General
Complete name                            : C:\\Users\\alice\\Downloads\\Sample.2019.1080p.BluRay.x264-GRP.mkv
Format                                   : Matroska
Duration                                 : 1 h 52 min

Video
Format                                   : AVC
Width                                    : 1 920 pixels
Height                                   : 1 080 pixels
Frame rate                               : 23.976 FPS
Bit depth                                : 8 bits

Audio
Format                                   : DTS
Format/Info                              : Digital Theater Systems
Commercial name                          : DTS-HD Master Audio
Format profile                           : MA / Core
Channel(s)                               : 6 channels
Language                                 : English

Text
Format                                   : PGS
Language                                 : Chinese
";

async fn db() -> PgConnection {
    PgConnection::connect(&env("PT_TEST_SUPERUSER_URL", "postgres://postgres@127.0.0.1:55432/pt_test"))
        .await
        .unwrap()
}

#[tokio::test]
async fn mediainfo_end_to_end() {
    let _guard = common::LOCK.lock().await;
    reset().await;
    let dir = common::package_wasm_plugins();
    let mut t = common::config_text();
    t.push_str(&format!("\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n", dir.display()));
    let kernel = Kernel::start(SiteConfig::parse(&t).unwrap(), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let uploader = register_login(&app, "mi_up").await;
    let other = register_login(&app, "mi_other").await;
    let staff = register_login(&app, "mi_staff").await;
    grant_role(staff.id, "moderator").await;
    let tid = upload(&app, &uploader, "Sample.2019.1080p", &[("a.mkv", 1 << 20)]).await;
    // 声明的规格：分辨率对，音频错
    sqlx::query("UPDATE torrent.torrents SET fields = fields || $2 WHERE id = $1")
        .bind(tid)
        .bind(json!({ "resolution": "1080p", "video_codec": "avc", "audio_codecs": ["truehd"] }))
        .execute(&mut db().await)
        .await
        .unwrap();

    // 预览：识别、脱敏、摘要
    let r = call(&app, "POST", "/api/p/mediainfo/v1/parse", Some(json!({ "text": REPORT })), &hv(&other.headers())).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["format"], "text");
    let s = &r.body["summary"];
    assert_eq!((s["resolution"].clone(), s["video_codec"].clone(), s["container"].clone()), (json!("1080p"), json!("avc"), json!("mkv")));
    assert_eq!(s["audio_codecs"], json!(["dts_hd_ma"]));
    assert_eq!(s["subtitle_languages"], json!(["Chinese"]));
    assert_eq!(s["duration_secs"], json!(112 * 60));
    let text = r.body["text"].as_str().unwrap();
    assert!(!text.contains("alice") && text.contains("Sample.2019.1080p.BluRay.x264-GRP.mkv"), "{text}");
    let bad = call(&app, "POST", "/api/p/mediainfo/v1/parse", Some(json!({ "text": "just some words" })), &hv(&other.headers())).await;
    assert_eq!((bad.status, bad.body["code"].clone()), (StatusCode::UNPROCESSABLE_ENTITY, json!("mediainfo.unrecognized")));
    let big = "General\nFormat : Matroska\n".repeat(20_000);
    let big = call(&app, "POST", "/api/p/mediainfo/v1/parse", Some(json!({ "text": big })), &hv(&other.headers())).await;
    assert_eq!(big.body["code"], "mediainfo.too_large", "{:?}", big.body);

    // 只有发种者 / 管理员能保存
    let path = format!("/api/p/mediainfo/v1/torrents/{tid}");
    let r = call(&app, "PUT", &path, Some(json!({ "text": REPORT })), &hv(&other.headers())).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN, "{:?}", r.body);
    let r = call(&app, "PUT", &path, Some(json!({ "text": REPORT })), &hv(&uploader.headers())).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["diffs"], json!([{ "field": "audio_codecs", "declared": "truehd", "detected": "dts_hd_ma" }]));
    let g = call(&app, "GET", &path, None, &hv(&other.headers())).await.body;
    assert_eq!((g["editable"].clone(), g["result"]["summary"]["resolution"].clone()), (json!(false), json!("1080p")), "{g:?}");
    assert!(!g["result"]["text"].as_str().unwrap().contains("alice"));
    let g = call(&app, "GET", &path, None, &hv(&staff.headers())).await.body;
    assert_eq!(g["editable"], json!(true));
    let stored: String = sqlx::query_scalar("SELECT text FROM mediainfo.results WHERE torrent_id = $1")
        .bind(tid)
        .fetch_one(&mut db().await)
        .await
        .unwrap();
    assert!(!stored.contains("Users\\alice"), "raw paths never reach storage");

    // 管理员可改；JSON 格式
    let js = json!({ "media": { "track": [
        { "@type": "General", "Format": "Matroska", "CompleteName": "/home/bob/x.mkv" },
        { "@type": "Video", "Format": "AVC", "Width": "1920", "Height": "1080" },
        { "@type": "Audio", "Format": "MLP FBA", "Format_Commercial_IfAny": "Dolby TrueHD" } ] } });
    let r = call(&app, "PUT", &path, Some(json!({ "text": js.to_string() })), &hv(&staff.headers())).await;
    assert_eq!((r.status, r.body["format"].clone(), r.body["diffs"].clone()), (StatusCode::OK, json!("json"), json!([])), "{:?}", r.body);
    assert!(!r.body["text"].as_str().unwrap().contains("/home/bob"));

    // 种子删除后清理
    let del = call(&app, "DELETE", &format!("/api/p/torrent/v1/admin/torrents/{tid}?reason=dupe"), None, &hv(&staff.headers())).await;
    assert!(del.status.is_success(), "{:?}", del.body);
    let mut gone = false;
    for _ in 0..50 {
        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM mediainfo.results WHERE torrent_id = $1")
            .bind(tid)
            .fetch_one(&mut db().await)
            .await
            .unwrap();
        if n == 0 {
            gone = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(gone, "results are removed with the torrent");
}
