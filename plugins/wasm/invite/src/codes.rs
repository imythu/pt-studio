//! 邀请码派生与格式（invite 03 §1.1）、子密钥与邮箱 HMAC（§1.0）。纯函数，可在宿主上单元测试。

use sha2::{Digest, Sha256};

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn hmac(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&sha256(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(msg);
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// HKDF-SHA256（RFC 5869），输出 32 字节
pub fn hkdf(ikm: &[u8], salt: &[u8], info: &[u8]) -> [u8; 32] {
    let prk = hmac(salt, ikm);
    let mut t = info.to_vec();
    t.push(1);
    hmac(&prk, &t)
}

/// 标准 base64 解码（配置中的密钥）
pub fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        })
    };
    let s: Vec<u8> = s
        .bytes()
        .filter(|c| !c.is_ascii_whitespace() && *c != b'=')
        .collect();
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for chunk in s.chunks(4) {
        let mut acc = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            acc |= val(*c)? << (18 - 6 * i);
        }
        let bytes = acc.to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    Some(out)
}

fn crockford(data: &[u8]) -> String {
    let mut out = String::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for b in data {
        acc = (acc << 8) | u32::from(*b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(CROCKFORD[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(CROCKFORD[((acc << (5 - bits)) & 31) as usize] as char);
    }
    out
}

pub struct Keys {
    code: [u8; 32],
    email: [u8; 32],
}

impl Keys {
    pub fn new(secret: &[u8]) -> Self {
        Self {
            code: hkdf(secret, b"pt-studio/invite", b"code"),
            email: hkdf(secret, b"pt-studio/invite", b"email-binding"),
        }
    }

    /// `INV-` + 26 位 Crockford Base32（HMAC 前 128 位）
    pub fn derive_code(&self, scope: &str, key: &str) -> String {
        let mac = hmac(&self.code, format!("issue:{scope}:{key}").as_bytes());
        format!("INV-{}", crockford(&mac[..16]))
    }

    pub fn email_hmac(&self, email: &str) -> [u8; 32] {
        hmac(&self.email, canon_email(email).as_bytes())
    }
}

pub fn canon_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub fn email_domain(email: &str) -> Option<String> {
    let (local, domain) = canon_email(email)
        .rsplit_once('@')
        .map(|(l, d)| (l.to_owned(), d.to_owned()))?;
    (!local.is_empty() && domain.contains('.') && domain.len() <= 253).then_some(domain)
}

/// 用户粘贴的码 → 规范形式；不合格式返回 None
pub fn normalize(input: &str) -> Option<String> {
    let s: String = input
        .chars()
        .filter(|c| !matches!(c, ' ' | '-' | '\t'))
        .collect::<String>()
        .to_uppercase();
    let body: String = s
        .strip_prefix("INV")?
        .chars()
        .map(|c| match c {
            'O' => '0',
            'I' | 'L' => '1',
            _ => c,
        })
        .collect();
    (body.len() == 26 && body.bytes().all(|c| CROCKFORD.contains(&c))).then(|| format!("INV-{body}"))
}

pub fn code_hash(code: &str) -> [u8; 32] {
    sha256(code.as_bytes())
}

pub fn code_hint(code: &str) -> String {
    code[code.len().saturating_sub(4)..].to_owned()
}

/// 预留持有者：去空白、小写（identity 在钩子输入与 `user.registered` 中给出同一显示形用户名）
pub fn holder(username: &str) -> String {
    username.trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_rfc4231_case2() {
        let mac = hmac(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            mac.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn hkdf_rfc5869_case1_prefix() {
        let ikm = [0x0bu8; 22];
        let salt: Vec<u8> = (0u8..=0x0c).collect();
        let info: Vec<u8> = (0xf0u8..=0xf9).collect();
        let okm = hkdf(&ikm, &salt, &info);
        assert_eq!(
            okm.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf"
        );
    }

    #[test]
    fn codes() {
        let k = Keys::new(&[7u8; 32]);
        let c = k.derive_code("user:1", "abc");
        assert_eq!(c.len(), 30);
        assert_eq!(c, k.derive_code("user:1", "abc"));
        assert_ne!(c, k.derive_code("user:2", "abc"));
        assert_eq!(normalize(&c).as_deref(), Some(c.as_str()));
        let messy = format!(" inv {} ", c[4..].to_lowercase().replace('0', "o"));
        assert_eq!(normalize(&messy).as_deref(), Some(c.as_str()));
        assert_eq!(normalize("INV-SHORT"), None);
        assert_eq!(normalize("XYZ-0123456789ABCDEFGHJKMNPQ"), None);
        assert_eq!(code_hint(&c).len(), 4);
        assert_eq!(base64_decode("AAECAw==").unwrap_or_default(), vec![0, 1, 2, 3]);
        assert_eq!(email_domain("A@Example.COM").as_deref(), Some("example.com"));
        assert_eq!(k.email_hmac("a@example.com"), k.email_hmac(" A@EXAMPLE.com"));
    }
}
