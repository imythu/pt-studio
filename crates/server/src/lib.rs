//! `pt-server`：选择编译进二进制的 Native 插件（plugin-api.md §3.1）。

use pt_plugin_api::NativePlugin;

/// 本二进制编译进的全部 Native 插件（是否启用由 site.toml `[plugins] disabled` 决定）。
#[allow(unused_mut, clippy::vec_init_then_push)]
pub fn native_plugins() -> Vec<NativePlugin> {
    let mut v = Vec::new();
    #[cfg(feature = "identity")]
    v.push(pt_plugin_identity::plugin());
    #[cfg(feature = "torrent")]
    v.push(pt_plugin_torrent::plugin());
    #[cfg(feature = "ledger")]
    v.push(pt_plugin_ledger::plugin());
    #[cfg(feature = "accounting")]
    v.push(pt_plugin_accounting::plugin());
    #[cfg(feature = "search")]
    v.push(pt_plugin_search_meilisearch::plugin());
    #[cfg(feature = "llm-claude")]
    v.push(pt_plugin_llm_claude::plugin());
    #[cfg(feature = "mcp")]
    v.push(pt_plugin_mcp::plugin());
    #[cfg(feature = "ai-assistant")]
    v.push(pt_plugin_ai_assistant::plugin());
    #[cfg(feature = "rss")]
    v.push(pt_plugin_rss::plugin());
    #[cfg(feature = "tracker-bridge")]
    v.push(pt_plugin_tracker_bridge::plugin());
    #[cfg(feature = "recommend")]
    v.push(pt_plugin_recommend::plugin());
    #[cfg(feature = "server-load")]
    v.push(pt_plugin_server_load::plugin());
    v
}

/// 组装 WASM 插件包：`<out>/<id>/{plugin.toml, plugin.wasm, migrations/*.sql, schemas/hooks/*.json, checksums.txt}`，并做 [`check`]。
/// `src` 是插件源码目录（含 `plugin.toml` 与可选的 `migrations/`），`wasm` 是构建出的组件。返回包目录。
pub fn package(
    src: &std::path::Path,
    wasm: &std::path::Path,
    out: &std::path::Path,
) -> anyhow::Result<std::path::PathBuf> {
    use sha2::{Digest, Sha256};
    let text = std::fs::read_to_string(src.join("plugin.toml"))?;
    let m = pt_plugin_api::manifest::Manifest::parse(&text)
        .map_err(|e| anyhow::anyhow!("plugin.toml: {}", e.message))?;
    let dst = out.join(&m.id);
    if dst.exists() {
        std::fs::remove_dir_all(&dst)?;
    }
    std::fs::create_dir_all(&dst)?;
    let mut files: Vec<(String, Vec<u8>)> = vec![
        ("plugin.toml".into(), text.into_bytes()),
        ("plugin.wasm".into(), std::fs::read(wasm)?),
    ];
    // 钩子输入等 JSON Schema（随包分发，供文档与校验工具使用）
    if let Ok(rd) = std::fs::read_dir(src.join("schemas/hooks")) {
        std::fs::create_dir_all(dst.join("schemas/hooks"))?;
        let mut fs: Vec<std::path::PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
        fs.sort();
        for f in fs
            .into_iter()
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
        {
            let name = f
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            files.push((format!("schemas/hooks/{name}"), std::fs::read(&f)?));
        }
    }
    if let Ok(rd) = std::fs::read_dir(src.join("migrations")) {
        let mut sqls: Vec<std::path::PathBuf> = rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "sql"))
            .collect();
        sqls.sort();
        std::fs::create_dir_all(dst.join("migrations"))?;
        for f in sqls {
            let name = f
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            files.push((format!("migrations/{name}"), std::fs::read(&f)?));
        }
    }
    let mut sums = String::new();
    for (name, bytes) in &files {
        std::fs::write(dst.join(name), bytes)?;
        sums.push_str(&format!("{}  {name}\n", hex::encode(Sha256::digest(bytes))));
    }
    std::fs::write(dst.join("checksums.txt"), sums)?;
    check(&dst)?;
    Ok(dst)
}

/// 检查插件包目录（不连接数据库），见 [`pt_kernel::wasm::check_package`]
pub fn check(dir: &std::path::Path) -> anyhow::Result<pt_plugin_api::manifest::Manifest> {
    pt_kernel::wasm::check_package(dir)
        .map_err(|e| anyhow::anyhow!("error[{}] {}: {}", e.code, e.plugin, e.message))
}

fn key_password() -> Option<String> {
    std::env::var("PT_PLUGIN_KEY_PASSWORD").ok().filter(|p| !p.is_empty())
}

/// 生成 minisign 密钥对：`<out>.key`（私钥）与 `<out>.pub`（公钥）
pub fn keygen(out: &std::path::Path, unencrypted: bool) -> anyhow::Result<(std::path::PathBuf, std::path::PathBuf)> {
    let kp = if unencrypted {
        minisign::KeyPair::generate_unencrypted_keypair()
    } else {
        let pw = key_password()
            .ok_or_else(|| anyhow::anyhow!("set PT_PLUGIN_KEY_PASSWORD (or pass --unencrypted)"))?;
        minisign::KeyPair::generate_encrypted_keypair(Some(pw))
    }
    .map_err(|e| anyhow::anyhow!("keygen: {e}"))?;
    let key = out.with_extension("key");
    let pubf = out.with_extension("pub");
    if key.exists() {
        anyhow::bail!("{} already exists", key.display());
    }
    let sk_box = kp
        .sk
        .to_box(Some("pt-studio plugin signing key"))
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let pk_box = kp.pk.to_box().map_err(|e| anyhow::anyhow!("{e}"))?;
    std::fs::write(&key, sk_box.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::write(&pubf, pk_box.to_string())?;
    Ok((key, pubf))
}

/// 签名包目录（写 `plugin.sig`）
pub fn sign(dir: &std::path::Path, key: &std::path::Path) -> anyhow::Result<()> {
    let text = std::fs::read_to_string(key)?;
    let sk_box = minisign::SecretKeyBox::from_string(&text).map_err(|e| anyhow::anyhow!("{}: {e}", key.display()))?;
    // 不加密的私钥直接使用；加密的从环境变量取口令（不交互提示）
    let sk = match sk_box.clone().into_unencrypted_secret_key() {
        Ok(sk) => sk,
        Err(_) => {
            let pw = key_password()
                .ok_or_else(|| anyhow::anyhow!("{} is encrypted: set PT_PLUGIN_KEY_PASSWORD", key.display()))?;
            sk_box
                .into_secret_key(Some(pw))
                .map_err(|e| anyhow::anyhow!("{}: {e}", key.display()))?
        }
    };
    let m = pt_plugin_api::manifest::Manifest::parse(&std::fs::read_to_string(dir.join("plugin.toml"))?)
        .map_err(|e| anyhow::anyhow!("plugin.toml: {}", e.message))?;
    pt_kernel::wasm::signing::sign_package(dir, &sk, &m.id, &m.version.to_string()).map_err(|e| anyhow::anyhow!(e))
}

/// 用公钥校验包签名
pub fn verify_signature(dir: &std::path::Path, key: &str) -> anyhow::Result<()> {
    let keys = pt_kernel::wasm::signing::TrustedKeys::parse(&[key.to_owned()]).map_err(|e| anyhow::anyhow!(e))?;
    pt_kernel::wasm::signing::verify_package(dir, &keys).map_err(|e| anyhow::anyhow!("error[E-LOAD-SIGNATURE] {e}"))
}
