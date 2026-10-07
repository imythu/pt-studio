//! 密钥环（identity 03 §1.1）：HKDF-SHA256 派生子密钥；AES-256-GCM 加密；HMAC-SHA256 查找。

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use pt_plugin_api::{PResult, PluginError};
use sha2::{Digest, Sha256};

const KEY_VER: u8 = 1;

#[derive(Clone)]
pub struct Keyring {
    passkey_enc: [u8; 32],
    passkey_mac: [u8; 32],
    email_enc: [u8; 32],
    email_mac: [u8; 32],
    ip_mac: [u8; 32],
}

impl std::fmt::Debug for Keyring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keyring(..)")
    }
}

fn derive(master: &[u8], info: &str) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(b"pt-studio/identity"), master);
    let mut out = [0u8; 32];
    // 32 字节输出不会超过 HKDF 上限
    let _ = hk.expand(info.as_bytes(), &mut out);
    out
}

impl Keyring {
    /// `secret_key`：hex 或 base64url，解码后至少 32 字节。
    pub fn from_secret(secret: &str) -> PResult<Self> {
        let raw = hex::decode(secret)
            .ok()
            .or_else(|| URL_SAFE_NO_PAD.decode(secret.trim_end_matches('=')).ok())
            .ok_or_else(|| PluginError::validation("identity.secret_key must be hex or base64url"))?;
        if raw.len() < 32 {
            return Err(PluginError::validation(
                "identity.secret_key must be at least 32 bytes",
            ));
        }
        Ok(Self {
            passkey_enc: derive(&raw, "passkey-enc"),
            passkey_mac: derive(&raw, "passkey-mac"),
            email_enc: derive(&raw, "email-enc"),
            email_mac: derive(&raw, "email-mac"),
            ip_mac: derive(&raw, "ip-mac"),
        })
    }

    fn mac(key: &[u8; 32], data: &[u8]) -> Vec<u8> {
        // HMAC 接受任意长度密钥，new_from_slice 不会失败
        match <Hmac<Sha256> as Mac>::new_from_slice(key) {
            Ok(mut m) => {
                m.update(data);
                m.finalize().into_bytes().to_vec()
            }
            Err(_) => Vec::new(),
        }
    }

    fn seal(key: &[u8; 32], plaintext: &[u8], aad: &str, nonce: [u8; 12]) -> PResult<Vec<u8>> {
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| PluginError::internal("bad key"))?;
        let ct = cipher
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| PluginError::internal("encryption failed"))?;
        let mut out = Vec::with_capacity(1 + 12 + ct.len());
        out.push(KEY_VER);
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        Ok(out)
    }

    fn open(key: &[u8; 32], blob: &[u8], aad: &str) -> PResult<Vec<u8>> {
        if blob.len() < 1 + 12 + 16 || blob[0] != KEY_VER {
            return Err(PluginError::internal("malformed ciphertext"));
        }
        let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| PluginError::internal("bad key"))?;
        cipher
            .decrypt(
                Nonce::from_slice(&blob[1..13]),
                Payload {
                    msg: &blob[13..],
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| PluginError::internal("decryption failed"))
    }

    pub fn passkey_mac(&self, passkey: &str) -> Vec<u8> {
        Self::mac(&self.passkey_mac, passkey.as_bytes())
    }
    pub fn passkey_seal(&self, uid: i64, passkey: &str, nonce: [u8; 12]) -> PResult<Vec<u8>> {
        Self::seal(
            &self.passkey_enc,
            passkey.as_bytes(),
            &format!("passkeys:passkey_enc:{uid}"),
            nonce,
        )
    }
    pub fn passkey_open(&self, uid: i64, blob: &[u8]) -> PResult<String> {
        String::from_utf8(Self::open(
            &self.passkey_enc,
            blob,
            &format!("passkeys:passkey_enc:{uid}"),
        )?)
        .map_err(|_| PluginError::internal("bad passkey"))
    }
    pub fn email_mac(&self, canonical: &str) -> Vec<u8> {
        Self::mac(&self.email_mac, canonical.as_bytes())
    }
    pub fn email_seal(&self, uid: i64, email: &str, nonce: [u8; 12]) -> PResult<Vec<u8>> {
        Self::seal(
            &self.email_enc,
            email.as_bytes(),
            &format!("user_emails:email_enc:{uid}"),
            nonce,
        )
    }
    pub fn email_open(&self, uid: i64, blob: &[u8]) -> PResult<String> {
        String::from_utf8(Self::open(
            &self.email_enc,
            blob,
            &format!("user_emails:email_enc:{uid}"),
        )?)
        .map_err(|_| PluginError::internal("bad email"))
    }
    pub fn ip_mac(&self, ip: &str) -> Vec<u8> {
        Self::mac(&self.ip_mac, ip.as_bytes())
    }
    /// IPv4 /24、IPv6 /48 前缀的 HMAC（钩子输入 ip_prefix_hmac）
    pub fn ip_prefix_mac(&self, ip: &str) -> Vec<u8> {
        let prefix = match ip.parse::<std::net::IpAddr>() {
            Ok(std::net::IpAddr::V4(v4)) => {
                let o = v4.octets();
                format!("{}.{}.{}.0/24", o[0], o[1], o[2])
            }
            Ok(std::net::IpAddr::V6(v6)) => {
                let s = v6.segments();
                format!("{:x}:{:x}:{:x}::/48", s[0], s[1], s[2])
            }
            Err(_) => ip.to_owned(),
        };
        Self::mac(&self.ip_mac, prefix.as_bytes())
    }
}

pub fn sha256(data: &[u8]) -> Vec<u8> {
    Sha256::digest(data).to_vec()
}

pub fn b64(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

fn argon2() -> Argon2<'static> {
    // OWASP 2024：m = 19 MiB, t = 2, p = 1
    let params = argon2::Params::new(19 * 1024, 2, 1, None).unwrap_or_default();
    Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params)
}

/// argon2id 哈希（CPU 密集，放到阻塞线程）。
pub async fn hash_password(password: String) -> PResult<String> {
    tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(&mut OsRng);
        argon2()
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|_| PluginError::internal("password hashing failed"))
    })
    .await
    .map_err(|_| PluginError::internal("hash task failed"))?
}

/// 用户不存在时对固定哈希做一次验证，对齐耗时。
pub const DUMMY_PHC: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$Qp1kWhhC2tW7gqK/i7kQCKSnT9oQdPJ2ocNsmGIZmQM";

pub async fn verify_password(phc: String, password: String) -> bool {
    tokio::task::spawn_blocking(move || {
        PasswordHash::new(&phc).is_ok_and(|h| argon2().verify_password(password.as_bytes(), &h).is_ok())
    })
    .await
    .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_roundtrip_and_aad_binding() -> PResult<()> {
        let k = Keyring::from_secret(&"ab".repeat(32))?;
        let blob = k.passkey_seal(7, "0123456789abcdef0123456789abcdef", [1; 12])?;
        assert_eq!(k.passkey_open(7, &blob)?, "0123456789abcdef0123456789abcdef");
        assert!(
            k.passkey_open(8, &blob).is_err(),
            "ciphertext must be bound to the row"
        );
        assert_eq!(k.passkey_mac("x"), k.passkey_mac("x"));
        assert_ne!(k.passkey_mac("x"), k.email_mac("x"));
        assert_eq!(k.ip_prefix_mac("1.2.3.4"), k.ip_prefix_mac("1.2.3.200"));
        assert!(Keyring::from_secret("short").is_err());
        Ok(())
    }

    #[tokio::test]
    async fn password_roundtrip() -> PResult<()> {
        let h = hash_password("correct horse".into()).await?;
        assert!(verify_password(h.clone(), "correct horse".into()).await);
        assert!(!verify_password(h, "wrong".into()).await);
        assert!(!verify_password(DUMMY_PHC.into(), "x".into()).await);
        Ok(())
    }
}
