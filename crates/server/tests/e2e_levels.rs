//! 端到端：levels（WASM）—— 默认等级、角色同步、升级与保护期、保级警告与到期降级、手动锁定与解锁。

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use common::{call, env, hv, register_login, reset};

const GIB: i64 = 1 << 30;

fn config(dir: &std::path::Path) -> SiteConfig {
    let mut t = common::config_text();
    t.push_str(&format!(
        "\n[plugins]\ndir = \"{}\"\nallow_unsigned = true\n",
        dir.display()
    ));
    SiteConfig::parse(&t).unwrap()
}

async fn class_of(db: &mut PgConnection, uid: u64) -> (i32, i32) {
    let identity: i32 = sqlx::query_scalar("SELECT class_id FROM identity.users WHERE id = $1")
        .bind(uid as i64)
        .fetch_one(&mut *db)
        .await
        .unwrap();
    let levels: i32 = sqlx::query_scalar("SELECT class_id FROM levels.user_levels WHERE user_id = $1")
        .bind(uid as i64)
        .fetch_one(&mut *db)
        .await
        .unwrap();
    (identity, levels)
}

async fn set_traffic(db: &mut PgConnection, uid: u64, up: i64, down: i64) {
    sqlx::query(
        "INSERT INTO accounting.user_totals (user_id, uploaded_adjust, downloaded_adjust) VALUES ($1, $2, $3)
         ON CONFLICT (user_id) DO UPDATE SET uploaded_adjust = $2, downloaded_adjust = $3",
    )
    .bind(uid as i64)
    .bind(up)
    .bind(down)
    .execute(&mut *db)
    .await
    .unwrap();
}

#[tokio::test]
async fn levels_end_to_end() {
    let _guard = common::LOCK.lock().await;
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_env_filter("info,sqlx=warn,cranelift=warn,wasmtime=warn")
        .try_init();
    let dir = common::package_wasm_plugins();
    reset().await;
    let kernel = Kernel::start(config(&dir), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let app = kernel.router();
    let mut db = PgConnection::connect(&env(
        "PT_TEST_SUPERUSER_URL",
        "postgres://postgres@127.0.0.1:55432/pt_test",
    ))
    .await
    .unwrap();

    // 初始化：预设等级与角色同步
    let n: i64 =
        sqlx::query_scalar("SELECT count(*) FROM identity.class_roles WHERE owner_plugin = 'levels'")
            .fetch_one(&mut db)
            .await
            .unwrap();
    assert_eq!(n, 17);

    let admin = register_login(&app, "lv_admin").await;
    common::grant_role(admin.id, "admin").await;
    let user = register_login(&app, "lv_user").await;

    // 注册事件：写入默认等级（用户 = 20）并同步到 identity
    let mut ok = false;
    for _ in 0..100 {
        let r: Option<i32> = sqlx::query_scalar(
            "SELECT u.class_id FROM identity.users u JOIN levels.user_levels l ON l.user_id = u.id WHERE u.id = $1",
        )
        .bind(user.id as i64)
        .fetch_optional(&mut db)
        .await
        .unwrap();
        if r == Some(20) {
            ok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(ok, "default class was not applied");
    // 等级角色并入有效角色
    let roles: Vec<String> = sqlx::query_scalar("SELECT role FROM identity.class_roles WHERE class_id = 20")
        .fetch_all(&mut db)
        .await
        .unwrap();
    assert_eq!(roles, vec!["levels:class_20".to_owned()]);

    let r = call(&app, "GET", "/api/p/levels/v1/me", None, &hv(&user.headers())).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["class"]["id"], 20);
    assert_eq!(r.body["class"]["key"], "user");
    assert_eq!(r.body["next"]["id"], 30);
    assert_eq!(r.body["next"]["key"], "power_user");
    let progress = r.body["next"]["progress"].as_array().unwrap();
    assert_eq!(progress.len(), 3);
    assert!(
        progress
            .iter()
            .any(|p| p["metric"] == "ratio" && p["met"] == true && p["current"] == "inf")
    );
    let r = call(
        &app,
        "GET",
        "/api/p/levels/v1/classes",
        None,
        &hv(&user.headers()),
    )
    .await;
    let items = r.body["items"].as_array().unwrap();
    assert_eq!(items.len(), 17);
    assert_eq!(items[2]["key"], "power_user");
    assert_eq!(items[2]["id"], 30);
    assert_eq!(items[2]["name_key"], "levels.class.power_user.name");
    assert_eq!(items[16]["key"], "staff_leader");
    assert_eq!(r.body["items"][1]["is_default"], true);

    // 满足 power_user：注册 30 天、上传 60 GiB、分享率 ∞ → 每日评估升级，带保护期
    sqlx::query("UPDATE identity.users SET created_at = now() - interval '30 days' WHERE id = $1")
        .bind(user.id as i64)
        .execute(&mut db)
        .await
        .unwrap();
    set_traffic(&mut db, user.id, 60 * GIB, 0).await;
    kernel
        .run_job_now("levels", "daily_evaluate", json!({}))
        .await
        .unwrap();
    assert_eq!(class_of(&mut db, user.id).await, (30, 30));
    let (cause, prot): (String, bool) = sqlx::query_as(
        "SELECT h.cause, l.protect_until > now() FROM levels.history h JOIN levels.user_levels l USING (user_id)
          WHERE h.user_id = $1 ORDER BY h.at DESC LIMIT 1",
    )
    .bind(user.id as i64)
    .fetch_one(&mut db)
    .await
    .unwrap();
    assert_eq!((cause.as_str(), prot), ("promoted", true));

    // 分享率跌到 0.6（< 0.95）：保护期内不处理
    set_traffic(&mut db, user.id, 60 * GIB, 100 * GIB).await;
    kernel
        .run_job_now("levels", "daily_evaluate", json!({}))
        .await
        .unwrap();
    let warned: bool =
        sqlx::query_scalar("SELECT warning_deadline IS NOT NULL FROM levels.user_levels WHERE user_id = $1")
            .bind(user.id as i64)
            .fetch_one(&mut db)
            .await
            .unwrap();
    assert!(!warned);
    // 保护期结束 → 警告；截止后 → 降到保级条件仍满足的最高等级（用户 keep ≥ 0.5）
    sqlx::query("UPDATE levels.user_levels SET protect_until = NULL WHERE user_id = $1")
        .bind(user.id as i64)
        .execute(&mut db)
        .await
        .unwrap();
    kernel
        .run_job_now("levels", "daily_evaluate", json!({}))
        .await
        .unwrap();
    let r = call(&app, "GET", "/api/p/levels/v1/me", None, &hv(&user.headers())).await;
    assert!(r.body["warning"]["deadline"].as_i64().is_some(), "{:?}", r.body);
    assert_eq!(class_of(&mut db, user.id).await, (30, 30));
    sqlx::query(
        "UPDATE levels.user_levels SET warning_deadline = now() - interval '1 second' WHERE user_id = $1",
    )
    .bind(user.id as i64)
    .execute(&mut db)
    .await
    .unwrap();
    kernel
        .run_job_now("levels", "daily_evaluate", json!({}))
        .await
        .unwrap();
    assert_eq!(class_of(&mut db, user.id).await, (20, 20));
    let r = call(&app, "GET", "/api/p/levels/v1/me", None, &hv(&user.headers())).await;
    assert_eq!(r.body["warning"], Value::Null);

    // 分享率 0.6 时用户等级不会再次升级（power_user 需 1.05）
    kernel
        .run_job_now("levels", "daily_evaluate", json!({}))
        .await
        .unwrap();
    assert_eq!(class_of(&mut db, user.id).await, (20, 20));

    // 手动锁定：普通用户无权；管理员锁到 农民（10），解锁后回到基线
    let mut h = user.headers();
    h.push(("idempotency-key", "l1".into()));
    let path = format!("/api/p/levels/v1/admin/users/{}/class", user.id);
    let r = call(
        &app,
        "POST",
        &path,
        Some(json!({"class_id": 10, "reason": "x"})),
        &hv(&h),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let mut h = admin.headers();
    h.push(("idempotency-key", "l2".into()));
    let r = call(
        &app,
        "POST",
        &path,
        Some(json!({"class_id": 10, "reason": "test"})),
        &hv(&h),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    assert_eq!(class_of(&mut db, user.id).await, (10, 10));
    kernel
        .run_job_now("levels", "daily_evaluate", json!({}))
        .await
        .unwrap();
    assert_eq!(class_of(&mut db, user.id).await, (10, 10));
    let mut h = admin.headers();
    h.push(("idempotency-key", "l3".into()));
    let r = call(
        &app,
        "POST",
        &path,
        Some(json!({"class_id": 999, "reason": "x"})),
        &hv(&h),
    )
    .await;
    assert_eq!(r.body["code"], "levels.class_not_found", "{:?}", r.body);
    let mut h = admin.headers();
    h.push(("idempotency-key", "l4".into()));
    let r = call(&app, "DELETE", &path, None, &hv(&h)).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    assert_eq!(class_of(&mut db, user.id).await, (20, 20));

    // 增量评估：流量结算后只升级
    set_traffic(&mut db, user.id, 60 * GIB, 0).await;
    sqlx::query("INSERT INTO levels.dirty_users (user_id, since) VALUES ($1, now()) ON CONFLICT DO NOTHING")
        .bind(user.id as i64)
        .execute(&mut db)
        .await
        .unwrap();
    kernel
        .run_job_now("levels", "incr_evaluate", json!({}))
        .await
        .unwrap();
    assert_eq!(class_of(&mut db, user.id).await, (30, 30));
    let causes: Vec<String> =
        sqlx::query_scalar("SELECT cause FROM levels.history WHERE user_id = $1 ORDER BY at, id")
            .bind(user.id as i64)
            .fetch_all(&mut db)
            .await
            .unwrap();
    assert_eq!(
        causes,
        vec!["promoted", "demoted", "override", "override_expired", "promoted"]
    );

    // 等级定义改由插件设置维护：改名、降低 elite_user 的条件、删除其他等级、新增荣誉等级 legend
    let settings_path = "/api/kernel/v1/admin/plugins/levels/settings";
    let r = call(&app, "GET", settings_path, None, &hv(&admin.headers())).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(
        r.body["schema"]["properties"]["classes"]["default"]
            .as_array()
            .unwrap()
            .len()
            == 17
    );
    let version = r.body["version"].as_i64().unwrap();
    let classes = json!([
        { "key": "peasant", "type": "normal", "keep": [] },
        { "key": "user", "type": "normal", "is_default": true,
          "promote": [{ "metric": "ratio", "op": "gte", "value": 0.6 }], "keep": [{ "metric": "ratio", "op": "gte", "value": 0.5 }] },
        { "key": "power_user", "name": "大佬", "description": "自定义说明", "type": "normal",
          "promote": [{ "metric": "uploaded_gib", "op": "gte", "value": 50 }], "keep": [] },
        { "key": "elite_user", "type": "normal", "color": "#00ff00",
          "promote": [{ "metric": "uploaded_gib", "op": "gte", "value": 1 }], "keep": [] },
        { "key": "legend", "type": "honor", "protected": true }
    ]);
    let mut h = admin.headers();
    h.push(("idempotency-key", "s1".into()));
    let r = call(
        &app,
        "PUT",
        settings_path,
        Some(json!({ "settings": { "classes": classes, "protect_days": 3 }, "version": version })),
        &hv(&h),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "GET",
        "/api/p/levels/v1/classes",
        None,
        &hv(&user.headers()),
    )
    .await;
    let items = r.body["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 5, "{items:?}");
    assert_eq!(
        (items[2]["id"].clone(), items[2]["name"].clone()),
        (json!(30), json!("大佬"))
    );
    assert_eq!(items[3]["color"], "#00ff00");
    let legend = items[4]["id"].as_i64().unwrap();
    assert!(legend > 999, "{legend}");
    let inactive: i64 = sqlx::query_scalar("SELECT count(*) FROM levels.classes WHERE NOT active")
        .fetch_one(&mut db)
        .await
        .unwrap();
    assert_eq!(inactive, 13);
    // 定义变化 → 下一次增量评估先做一次全量评估：60 GiB 满足新的 elite_user
    kernel
        .run_job_now("levels", "incr_evaluate", json!({}))
        .await
        .unwrap();
    assert_eq!(class_of(&mut db, user.id).await, (40, 40));

    // 无效设置（重复 key）：保留上一份定义，后台可看到原因
    let r = call(&app, "GET", settings_path, None, &hv(&admin.headers())).await;
    let version = r.body["version"].as_i64().unwrap();
    let mut h = admin.headers();
    h.push(("idempotency-key", "s2".into()));
    let bad = json!([{ "key": "user", "is_default": true }, { "key": "user" }]);
    let r = call(
        &app,
        "PUT",
        settings_path,
        Some(json!({ "settings": { "classes": bad }, "version": version })),
        &hv(&h),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        &app,
        "GET",
        "/api/p/levels/v1/admin/classes",
        None,
        &hv(&admin.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert!(
        r.body["settings_error"].as_str().unwrap().contains("duplicate"),
        "{:?}",
        r.body
    );
    assert_eq!(r.body["items"].as_array().unwrap().len(), 5);
    let r = call(
        &app,
        "GET",
        "/api/p/levels/v1/admin/classes",
        None,
        &hv(&user.headers()),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // 手动等级按 key 指定
    let mut h = admin.headers();
    h.push(("idempotency-key", "l5".into()));
    let r = call(
        &app,
        "POST",
        &path,
        Some(json!({"class": "legend", "reason": "x"})),
        &hv(&h),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{:?}", r.body);
    assert_eq!(class_of(&mut db, user.id).await, (legend as i32, legend as i32));
    let r = call(&app, "GET", "/api/p/levels/v1/me", None, &hv(&user.headers())).await;
    assert_eq!(r.body["class"]["key"], "legend", "{:?}", r.body);

    kernel.shutdown().await;
}
