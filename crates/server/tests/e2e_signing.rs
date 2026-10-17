//! 端到端：插件包签名（minisign）—— 生产模式下只加载受信公钥签名的包；篡改、多出文件、未签名都拒绝加载。
//!
//! 运行：`cargo test -p pt-server --test e2e_signing -- --test-threads=1`

#![allow(clippy::unwrap_used)]

mod common;

use axum::http::StatusCode;
use pt_kernel::{Kernel, config::SiteConfig};

use common::{call, reset};

fn prod_config(dir: &std::path::Path, pubkey: &std::path::Path) -> SiteConfig {
    let t = common::config_text().replace("mode = \"dev\"", "mode = \"prod\"");
    SiteConfig::parse(&format!(
        "{t}\n[plugins]\ndir = \"{}\"\ntrusted_keys = [\"{}\"]\n",
        dir.display(),
        pubkey.display()
    ))
    .unwrap()
}

async fn start_err(cfg: SiteConfig) -> String {
    match Kernel::start(cfg, pt_server::native_plugins()).await {
        Ok(_) => panic!("kernel should refuse the package"),
        Err(e) => e.to_string(),
    }
}

#[tokio::test]
async fn signed_packages_only_in_prod() {
    let _guard = common::LOCK.lock().await;
    common::package_wasm_plugins();
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let work = std::env::temp_dir().join(format!("pt-signing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let out = work.join("plugins");
    let pkg = pt_server::package(
        &root.join("plugins/wasm/checkin"),
        &root.join("plugins/wasm/target/wasm32-wasip2/release/pt_wasm_checkin.wasm"),
        &out,
    )
    .unwrap();
    let (key, pubkey) = pt_server::keygen(&work.join("site-signing"), true).unwrap();
    pt_server::sign(&pkg, &key).unwrap();
    pt_server::verify_signature(&pkg, std::fs::read_to_string(&pubkey).unwrap().lines().nth(1).unwrap()).unwrap();

    // 签名有效：生产模式加载成功，插件路由可用
    reset().await;
    let kernel = Kernel::start(prod_config(&out, &pubkey), pt_server::native_plugins())
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    let r = call(&kernel.router(), "GET", "/api/p/checkin/v1/leaderboard", None, &[]).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "route is mounted: {:?}", r.body);
    drop(kernel);

    // 多出一个迁移文件 → 拒绝
    std::fs::write(pkg.join("migrations/9999_evil.sql"), "SELECT 1;").unwrap();
    reset().await;
    let e = start_err(prod_config(&out, &pubkey)).await;
    assert!(e.contains("E-LOAD-SIGNATURE") && e.contains("not covered"), "{e}");
    std::fs::remove_file(pkg.join("migrations/9999_evil.sql")).unwrap();

    // 其他密钥签的 → 拒绝
    let (other_key, _) = pt_server::keygen(&work.join("other"), true).unwrap();
    pt_server::sign(&pkg, &other_key).unwrap();
    reset().await;
    let e = start_err(prod_config(&out, &pubkey)).await;
    assert!(e.contains("not made by a trusted key"), "{e}");

    // 未签名 → 生产模式拒绝
    std::fs::remove_file(pkg.join("plugin.sig")).unwrap();
    reset().await;
    let e = start_err(prod_config(&out, &pubkey)).await;
    assert!(e.contains("package is not signed"), "{e}");
    let _ = std::fs::remove_dir_all(&work);
}
