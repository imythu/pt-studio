//! 端到端：RSS 与 Torznab（rss，Native）—— feed key（生成、只对 rss 有效、重置后旧 key 失效）、订阅（字段条件、排除词、
//! 促销条件、校验）、订阅 XML、凭 key 下载（注入 passkey）、Torznab（caps、搜索、分类、Newznab 错误）、他人订阅不可读。

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::json;

use common::{call, hv, register_login, reset, upload};

fn config(dir: &std::path::Path) -> SiteConfig {
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n",
        dir.display()
    ));
    SiteConfig::parse(&t).unwrap()
}

fn text(r: &common::Resp) -> String {
    String::from_utf8_lossy(&r.bytes).into_owned()
}

async fn get(app: &axum::Router, uri: &str) -> common::Resp {
    call(app, "GET", uri, None, &[]).await
}

#[tokio::test]
async fn rss_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let dir = common::package_wasm_plugins();
    reset().await;
    let kernel = Kernel::start(config(&dir), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let alice = register_login(&app, "rss_alice").await;
    let bob = register_login(&app, "rss_bob").await;
    let admin = register_login(&app, "rss_admin").await;
    common::grant_role(admin.id, "admin").await;
    let ah = alice.headers();

    // feed key：初始没有；生成后明文只返回一次
    let r = call(&app, "GET", "/api/p/rss/v1/me/key", None, &hv(&ah)).await;
    assert_eq!(r.body["exists"], false, "{:?}", r.body);
    let r = call(&app, "POST", "/api/p/rss/v1/me/key", Some(json!({})), &hv(&ah)).await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let key = r.body["key"].as_str().unwrap().to_owned();
    assert!(key.starts_with("ptf_"), "{key}");
    // feed key 不能当作 API token 使用；个人 token 页面也不列出
    let r = call(
        &app,
        "GET",
        "/api/p/identity/v1/users/me",
        None,
        &[("authorization", &format!("Bearer {key}"))],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = call(&app, "GET", "/api/p/identity/v1/users/me/tokens", None, &hv(&ah)).await;
    assert_eq!(r.body["items"].as_array().map(Vec::len), Some(0), "{:?}", r.body);
    // 用户不能直接创建 feed 类 token
    let r = call(
        &app,
        "POST",
        "/api/p/identity/v1/users/me/tokens",
        Some(json!({ "kind": "feed", "name": "x", "scopes": ["rss"] })),
        &hv(&ah),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);

    // 种子：两个普通、一个带 CAM 的（排除词）
    let t_final = upload(&app, &bob, "Bob.Tool.Final", &[("a.bin", 4 << 20)]).await;
    let t_cam = upload(&app, &bob, "Bob.Tool.CAM", &[("a.bin", 5 << 20)]).await;
    let t_free = upload(&app, &bob, "Bob.Free.Thing", &[("a.bin", 6 << 20)]).await;

    // 订阅：字段条件 + 排除词；非法条件被拒
    let r = call(
        &app,
        "POST",
        "/api/p/rss/v1/me/feeds",
        Some(json!({ "name": "工具", "definition": { "type_ids": ["generic"], "q": "Tool", "exclude": ["CAM"],
                     "filters": [{ "field": "kind", "op": "eq", "value": "software" }, { "field": "platform", "op": "in", "value": ["windows", "mac"] }] } })),
        &hv(&ah),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{:?}", r.body);
    let fid = r.body["id"].as_i64().unwrap();
    let r = call(
        &app,
        "POST",
        "/api/p/rss/v1/me/feeds",
        Some(json!({ "name": "x", "definition": { "filters": [{ "field": "size; DROP", "op": "eq", "value": 1 }] } })),
        &hv(&ah),
    )
    .await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (StatusCode::UNPROCESSABLE_ENTITY, Some("rss.filter_invalid")),
        "{:?}",
        r.body
    );

    // 订阅 XML
    let r = get(&app, &format!("/api/p/rss/v1/feeds/{fid}.xml?k={key}")).await;
    assert_eq!(r.status, StatusCode::OK, "{}", text(&r));
    let x = text(&r);
    assert!(
        r.headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/rss+xml")
    );
    assert!(x.contains("<title>Bob.Tool.Final</title>"), "{x}");
    assert!(
        !x.contains("Bob.Tool.CAM") && !x.contains("Bob.Free.Thing"),
        "{x}"
    );
    assert!(x.contains(&format!("/api/p/rss/v1/dl/{t_final}?k={key}")), "{x}");
    assert!(x.contains("name=\"category\" value=\"4000\""), "{x}");
    // 错误 key / 缺 key / 别人的订阅
    let r = get(&app, &format!("/api/p/rss/v1/feeds/{fid}.xml?k=ptf_nope")).await;
    assert_eq!(
        (r.status, r.body["code"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("rss.key_invalid"))
    );
    assert_eq!(
        get(&app, &format!("/api/p/rss/v1/feeds/{fid}.xml")).await.status,
        StatusCode::UNAUTHORIZED
    );
    let bh = bob.headers();
    let bkey = call(&app, "POST", "/api/p/rss/v1/me/key", Some(json!({})), &hv(&bh))
        .await
        .body["key"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = get(&app, &format!("/api/p/rss/v1/feeds/{fid}.xml?k={bkey}")).await;
    assert_eq!(r.body["code"], "rss.feed_not_found");

    // 凭 key 下载：注入 alice 的 passkey，记入下载记录
    let r = get(&app, &format!("/api/p/rss/v1/dl/{t_final}?k={key}")).await;
    assert_eq!(r.status, StatusCode::OK, "{}", text(&r));
    assert_eq!(r.headers["content-type"], "application/x-bittorrent");
    assert!(r.bytes.starts_with(b"d"), "bencoded dict");
    let w = call(
        &app,
        "GET",
        &format!("/api/p/torrent/v1/torrents/{t_final}/download"),
        None,
        &hv(&ah),
    )
    .await;
    assert_eq!(w.status, StatusCode::OK);
    assert_eq!(r.bytes, w.bytes, "same file as the web download");
    let r = get(&app, &format!("/api/p/rss/v1/dl/999999?k={key}")).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", text(&r));
    let r = get(&app, &format!("/api/p/rss/v1/dl/{t_final}?k=ptf_bad")).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // Torznab
    let r = get(&app, "/api/p/rss/v1/torznab/api?t=caps").await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(text(&r).contains("<tv-search available=\"yes\""), "{}", text(&r));
    let r = get(
        &app,
        &format!("/api/p/rss/v1/torznab/api?t=search&q=Bob&apikey={key}"),
    )
    .await;
    let x = text(&r);
    assert_eq!(r.status, StatusCode::OK, "{x}");
    assert_eq!(x.matches("<item>").count(), 3, "{x}");
    assert!(x.contains(&format!("dl/{t_cam}?k={key}")), "{x}");
    let r = get(
        &app,
        &format!("/api/p/rss/v1/torznab/api?t=search&cat=2000&apikey={key}"),
    )
    .await;
    assert_eq!(text(&r).matches("<item>").count(), 0, "{}", text(&r));
    let r = get(
        &app,
        &format!("/api/p/rss/v1/torznab/api?t=search&cat=4000&q=Final&apikey={key}"),
    )
    .await;
    assert_eq!(text(&r).matches("<item>").count(), 1, "{}", text(&r));
    let r = get(&app, &format!("/api/p/rss/v1/torznab/api?t=nope&apikey={key}")).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(text(&r).contains("code=\"202\""));
    let r = get(&app, "/api/p/rss/v1/torznab/api?t=search&apikey=ptf_bad").await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert!(text(&r).contains("code=\"100\""));

    // 促销条件：只要免费的种子
    let mut done = false;
    for i in 0..50 {
        let mut h = admin.headers();
        h.push(("idempotency-key", format!("rss-p{i}")));
        let r = call(
            &app,
            "POST",
            "/api/p/promotion/v1/torrents/promotions",
            Some(json!({ "torrent_ids": [t_free], "kind": "free", "duration_secs": 86400, "reason": "rss" })),
            &hv(&h),
        )
        .await;
        if r.body["items"][0]["promotion_id"].as_i64().is_some() {
            done = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(done, "promotion was not created");
    let r = call(
        &app,
        "POST",
        "/api/p/rss/v1/me/feeds",
        Some(json!({ "name": "免费", "definition": { "promotion": ["free"] } })),
        &hv(&ah),
    )
    .await;
    let free_id = r.body["id"].as_i64().unwrap();
    let x = text(&get(&app, &format!("/api/p/rss/v1/feeds/{free_id}.xml?k={key}")).await);
    assert_eq!(x.matches("<item>").count(), 1, "{x}");
    assert!(
        x.contains("Bob.Free.Thing") && x.contains("name=\"downloadvolumefactor\" value=\"0\""),
        "{x}"
    );

    // 列表、修改、删除
    let l = call(&app, "GET", "/api/p/rss/v1/me/feeds", None, &hv(&ah)).await;
    assert_eq!(l.body["items"].as_array().map(Vec::len), Some(2));
    let r = call(
        &app,
        "PUT",
        &format!("/api/p/rss/v1/me/feeds/{fid}"),
        Some(json!({ "name": "工具（含 CAM）", "definition": { "q": "Tool" } })),
        &hv(&ah),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let x = text(&get(&app, &format!("/api/p/rss/v1/feeds/{fid}.xml?k={key}")).await);
    assert_eq!(x.matches("<item>").count(), 2, "cache invalidated on update: {x}");
    let r = call(
        &app,
        "DELETE",
        &format!("/api/p/rss/v1/me/feeds/{free_id}"),
        None,
        &hv(&ah),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);

    // 使用记录；重置后旧 key 失效
    let r = call(&app, "GET", "/api/p/rss/v1/me/key", None, &hv(&ah)).await;
    assert!(r.body["last_used_at"].as_i64().is_some(), "{:?}", r.body);
    assert_eq!(r.body["prefix"].as_str(), Some(&key[..8]));
    let r = call(&app, "POST", "/api/p/rss/v1/me/key", Some(json!({})), &hv(&ah)).await;
    let key2 = r.body["key"].as_str().unwrap().to_owned();
    assert_ne!(key, key2);
    assert_eq!(
        get(&app, &format!("/api/p/rss/v1/feeds/{fid}.xml?k={key}"))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(&app, &format!("/api/p/rss/v1/feeds/{fid}.xml?k={key2}"))
            .await
            .status,
        StatusCode::OK
    );
    let r = call(&app, "DELETE", "/api/p/rss/v1/me/key", None, &hv(&ah)).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        get(&app, &format!("/api/p/rss/v1/dl/{t_final}?k={key2}"))
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    kernel.shutdown().await;
}
