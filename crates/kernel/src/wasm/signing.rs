//! 插件包签名（minisign，roadmap §4.2）：`plugin.sig` 签 `checksums.txt`，`checksums.txt` 列出包内每个文件的 SHA-256。
//!
//! 校验：签名由某个受信公钥签发 → 清单中的每个文件哈希一致 → 包内没有清单之外的文件（`plugin.sig`、`checksums.txt` 除外）。

use std::path::Path;

use sha2::{Digest, Sha256};

/// 受信公钥（site.toml `plugins.trusted_keys`）
#[derive(Default)]
pub struct TrustedKeys(Vec<minisign::PublicKey>);

impl std::fmt::Debug for TrustedKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TrustedKeys({})", self.0.len())
    }
}

impl TrustedKeys {
    /// 每项为 base64 公钥（minisign 公钥文件的第二行）或 `.pub` 文件路径
    pub fn parse(items: &[String]) -> Result<Self, String> {
        let mut keys = Vec::new();
        for item in items {
            let item = item.trim();
            let pk = if item.starts_with("RW") && !item.contains('/') {
                minisign::PublicKey::from_base64(item)
            } else {
                let text = std::fs::read_to_string(item).map_err(|e| format!("trusted key `{item}`: {e}"))?;
                minisign::PublicKeyBox::from_string(&text).and_then(minisign::PublicKeyBox::into_public_key)
            }
            .map_err(|e| format!("trusted key `{item}`: {e}"))?;
            keys.push(pk);
        }
        Ok(Self(keys))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn single(pk: minisign::PublicKey) -> Self {
        Self(vec![pk])
    }
}

/// 包内相对路径（递归，`/` 分隔）
fn files_of(dir: &Path, prefix: &str, out: &mut Vec<String>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        if e.file_type()?.is_dir() {
            files_of(&e.path(), &rel, out)?;
        } else {
            out.push(rel);
        }
    }
    Ok(())
}

/// 校验包目录的签名与文件完整性
pub fn verify_package(dir: &Path, keys: &TrustedKeys) -> Result<(), String> {
    if keys.is_empty() {
        return Err("no trusted signing keys are configured (plugins.trusted_keys)".into());
    }
    let sig_text = std::fs::read_to_string(dir.join("plugin.sig")).map_err(|e| format!("plugin.sig: {e}"))?;
    let sums = std::fs::read(dir.join("checksums.txt")).map_err(|e| format!("checksums.txt: {e}"))?;
    let sig = minisign::SignatureBox::from_string(&sig_text).map_err(|e| format!("plugin.sig: {e}"))?;
    let trusted = keys
        .0
        .iter()
        .any(|pk| minisign::verify(pk, &sig, std::io::Cursor::new(&sums), true, false, false).is_ok());
    if !trusted {
        return Err("signature is invalid or not made by a trusted key".into());
    }
    let sums = String::from_utf8(sums).map_err(|_| "checksums.txt is not UTF-8".to_owned())?;
    let mut listed = std::collections::BTreeSet::new();
    for line in sums.lines().filter(|l| !l.trim().is_empty()) {
        let (hash, name) = line
            .split_once("  ")
            .ok_or_else(|| format!("checksums.txt: malformed line `{line}`"))?;
        let safe = !name.is_empty()
            && !name.starts_with('/')
            && name.split('/').all(|seg| !seg.is_empty() && seg != ".." && seg != ".");
        if !safe {
            return Err(format!("checksums.txt: unsafe path `{name}`"));
        }
        let bytes = std::fs::read(dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
        if hex::encode(Sha256::digest(&bytes)) != hash.trim().to_ascii_lowercase() {
            return Err(format!("{name}: checksum mismatch"));
        }
        listed.insert(name.to_owned());
    }
    for required in ["plugin.toml", "plugin.wasm"] {
        if !listed.contains(required) {
            return Err(format!("checksums.txt does not cover {required}"));
        }
    }
    let mut present = Vec::new();
    files_of(dir, "", &mut present).map_err(|e| e.to_string())?;
    if let Some(extra) = present
        .iter()
        .find(|f| *f != "plugin.sig" && *f != "checksums.txt" && !listed.contains(*f))
    {
        return Err(format!("{extra} is not covered by checksums.txt"));
    }
    Ok(())
}

/// 用 minisign 密钥签 `checksums.txt`，写出 `plugin.sig`（`pt-server plugin package --sign`）
pub fn sign_package(dir: &Path, sk: &minisign::SecretKey, plugin: &str, version: &str) -> Result<(), String> {
    let sums = std::fs::read(dir.join("checksums.txt")).map_err(|e| format!("checksums.txt: {e}"))?;
    let comment = format!("pt-studio plugin {plugin} {version}");
    let sig = minisign::sign(None, sk, std::io::Cursor::new(sums), Some(&comment), Some("signature of checksums.txt"))
        .map_err(|e| e.to_string())?;
    std::fs::write(dir.join("plugin.sig"), sig.to_string()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_and_verify() {
        let dir = std::env::temp_dir().join(format!("pt-sign-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("migrations")).unwrap_or_default();
        let files = [("plugin.toml", "id = \"x\""), ("plugin.wasm", "\0asm"), ("migrations/0001_init.sql", "SELECT 1")];
        let mut sums = String::new();
        for (n, c) in files {
            std::fs::write(dir.join(n), c).unwrap_or_default();
            sums.push_str(&format!("{}  {n}\n", hex::encode(Sha256::digest(c.as_bytes()))));
        }
        std::fs::write(dir.join("checksums.txt"), sums).unwrap_or_default();
        let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap_or_else(|_| unreachable!());
        sign_package(&dir, &kp.sk, "x", "0.1.0").unwrap_or_else(|e| panic!("{e}"));
        let keys = TrustedKeys::single(kp.pk.clone());
        assert_eq!(verify_package(&dir, &keys), Ok(()));
        // 其他密钥、篡改文件、多出文件都被拒绝
        let other = minisign::KeyPair::generate_unencrypted_keypair().unwrap_or_else(|_| unreachable!());
        assert!(verify_package(&dir, &TrustedKeys::single(other.pk)).is_err());
        assert!(verify_package(&dir, &TrustedKeys::default()).is_err());
        std::fs::write(dir.join("migrations/0002_evil.sql"), "DROP TABLE x").unwrap_or_default();
        assert!(verify_package(&dir, &keys).is_err_and(|e| e.contains("not covered")));
        std::fs::remove_file(dir.join("migrations/0002_evil.sql")).unwrap_or_default();
        std::fs::write(dir.join("plugin.wasm"), "\0asm2").unwrap_or_default();
        assert!(verify_package(&dir, &keys).is_err_and(|e| e.contains("checksum mismatch")));
        // 公钥文本解析
        let b64 = kp.pk.to_base64();
        assert!(TrustedKeys::parse(&[b64]).is_ok_and(|k| !k.is_empty()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
