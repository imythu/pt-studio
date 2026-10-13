//! HTTP tracker 协议（BEP 3 / BEP 23 / BEP 7 / BEP 48）：原始查询串解析（info_hash、peer_id 是任意字节的 URL 编码）
//! 与 bencode 响应。

use std::net::IpAddr;

/// 百分号解码为字节（`+` 不当作空格：info_hash 中的 `+` 只会以 %2B 出现）
pub fn percent_decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = b.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Some(out)
}

/// 查询串 → (键, 原始字节值) 列表（同名键保留多个，scrape 会有多个 info_hash）
pub fn parse_query(q: &str) -> Vec<(String, Vec<u8>)> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .filter_map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            Some((String::from_utf8(percent_decode(k)?).ok()?, percent_decode(v)?))
        })
        .collect()
}

pub fn get<'a>(q: &'a [(String, Vec<u8>)], key: &str) -> Option<&'a [u8]> {
    q.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_slice())
}

pub fn get_str<'a>(q: &'a [(String, Vec<u8>)], key: &str) -> Option<&'a str> {
    get(q, key).and_then(|v| std::str::from_utf8(v).ok())
}

pub fn get_u64(q: &[(String, Vec<u8>)], key: &str) -> Option<u64> {
    get_str(q, key).and_then(|v| v.parse().ok())
}

pub fn hash20(v: &[u8]) -> Option<[u8; 20]> {
    v.try_into().ok()
}

// ---------------- bencode ----------------

pub fn bstr(out: &mut Vec<u8>, s: &[u8]) {
    out.extend(s.len().to_string().as_bytes());
    out.push(b':');
    out.extend(s);
}

pub fn bint(out: &mut Vec<u8>, n: i64) {
    out.push(b'i');
    out.extend(n.to_string().as_bytes());
    out.push(b'e');
}

/// `d14:failure reason<msg>e`（BT 惯例：HTTP 200 + 失败原因）
pub fn failure(msg: &str) -> Vec<u8> {
    let mut o = b"d".to_vec();
    bstr(&mut o, b"failure reason");
    bstr(&mut o, msg.as_bytes());
    o.push(b'e');
    o
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerAddr {
    pub ip: IpAddr,
    pub port: u16,
}

pub struct AnnounceReply<'a> {
    pub complete: u64,
    pub incomplete: u64,
    pub interval: u64,
    pub min_interval: u64,
    pub peers: &'a [PeerAddr],
}

/// 紧凑格式（BEP 23 / BEP 7）：IPv4 进 `peers`，IPv6 进 `peers6`；键按字典序
pub fn announce(r: &AnnounceReply) -> Vec<u8> {
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    for p in r.peers {
        match p.ip {
            IpAddr::V4(ip) => {
                v4.extend(ip.octets());
                v4.extend(p.port.to_be_bytes());
            }
            IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
                Some(ip4) => {
                    v4.extend(ip4.octets());
                    v4.extend(p.port.to_be_bytes());
                }
                None => {
                    v6.extend(ip.octets());
                    v6.extend(p.port.to_be_bytes());
                }
            },
        }
    }
    let n = |x: u64| i64::try_from(x).unwrap_or(i64::MAX);
    let mut o = b"d".to_vec();
    bstr(&mut o, b"complete");
    bint(&mut o, n(r.complete));
    bstr(&mut o, b"incomplete");
    bint(&mut o, n(r.incomplete));
    bstr(&mut o, b"interval");
    bint(&mut o, n(r.interval));
    bstr(&mut o, b"min interval");
    bint(&mut o, n(r.min_interval));
    bstr(&mut o, b"peers");
    bstr(&mut o, &v4);
    if !v6.is_empty() {
        bstr(&mut o, b"peers6");
        bstr(&mut o, &v6);
    }
    o.push(b'e');
    o
}

/// scrape：`d5:filesd<20 字节 hash>d8:completei…e10:downloadedi…e10:incompletei…eeee`
pub fn scrape(files: &[([u8; 20], u64, u64, u64)]) -> Vec<u8> {
    let n = |x: u64| i64::try_from(x).unwrap_or(i64::MAX);
    let mut o = b"d".to_vec();
    bstr(&mut o, b"files");
    o.push(b'd');
    let mut sorted = files.to_vec();
    sorted.sort_by_key(|f| f.0);
    for (h, complete, downloaded, incomplete) in sorted {
        bstr(&mut o, &h);
        o.push(b'd');
        bstr(&mut o, b"complete");
        bint(&mut o, n(complete));
        bstr(&mut o, b"downloaded");
        bint(&mut o, n(downloaded));
        bstr(&mut o, b"incomplete");
        bint(&mut o, n(incomplete));
        o.push(b'e');
    }
    o.extend(b"ee");
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_parsing() {
        let q = parse_query("info_hash=%12%34abc%FF&peer_id=-qB4650-%00%01&port=6881&left=0&event=started");
        assert_eq!(
            get(&q, "info_hash"),
            Some(&[0x12, 0x34, b'a', b'b', b'c', 0xff][..])
        );
        assert_eq!(get(&q, "peer_id"), Some(&b"-qB4650-\x00\x01"[..]));
        assert_eq!(
            (get_u64(&q, "port"), get_str(&q, "event")),
            (Some(6881), Some("started"))
        );
        assert_eq!(percent_decode("%G1"), None);
        assert_eq!(percent_decode("%1"), None);
    }

    #[test]
    fn bencode_replies() {
        assert_eq!(failure("bad passkey"), b"d14:failure reason11:bad passkeye");
        let peers = [
            PeerAddr {
                ip: "1.2.3.4".parse().unwrap(),
                port: 0x1ae1,
            },
            PeerAddr {
                ip: "::1".parse().unwrap(),
                port: 1,
            },
        ];
        let r = announce(&AnnounceReply {
            complete: 1,
            incomplete: 2,
            interval: 1800,
            min_interval: 900,
            peers: &peers,
        });
        let mut want = b"d8:completei1e10:incompletei2e8:intervali1800e12:min intervali900e5:peers6:\x01\x02\x03\x04\x1a\xe16:peers618:".to_vec();
        want.extend([0u8; 15]);
        want.extend([1, 0, 1, b'e']);
        assert_eq!(r, want);
        let s = scrape(&[([7u8; 20], 3, 5, 1)]);
        assert!(s.starts_with(b"d5:filesd20:"));
        assert!(s.ends_with(b"d8:completei3e10:downloadedi5e10:incompletei1eeee"));
    }
}
