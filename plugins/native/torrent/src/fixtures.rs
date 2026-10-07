//! 测试用种子构造（单元测试与端到端测试共用）。

use crate::bencode;

/// 构造一个 v1 多文件种子（测试与 e2e 共用）。
pub fn sample_v1(name: &str, files: &[(&str, u64)], private: bool) -> Vec<u8> {
    let pl: u64 = 16_384;
    let total: u64 = files.iter().map(|f| f.1).sum();
    let pieces = vec![7u8; usize::try_from(total.div_ceil(pl) * 20).unwrap_or(0)];
    let mut fl = vec![b'l'];
    for (p, s) in files {
        let mut e = Vec::new();
        let mut len = Vec::new();
        bencode::encode_int(i64::try_from(*s).unwrap_or(0), &mut len);
        let mut path = vec![b'l'];
        bencode::encode_bytes(p.as_bytes(), &mut path);
        path.push(b'e');
        e.push((b"length".to_vec(), len));
        e.push((b"path".to_vec(), path));
        fl.extend(bencode::encode_raw_dict(e));
    }
    fl.push(b'e');
    let mut info = Vec::new();
    let mut n = Vec::new();
    bencode::encode_bytes(name.as_bytes(), &mut n);
    info.push((b"name".to_vec(), n));
    info.push((b"files".to_vec(), fl));
    let mut plv = Vec::new();
    bencode::encode_int(16_384, &mut plv);
    info.push((b"piece length".to_vec(), plv));
    let mut pc = Vec::new();
    bencode::encode_bytes(&pieces, &mut pc);
    info.push((b"pieces".to_vec(), pc));
    if private {
        info.push((b"private".to_vec(), b"i1e".to_vec()));
    }
    let mut ann = Vec::new();
    bencode::encode_bytes(b"http://public.example/announce", &mut ann);
    bencode::encode_raw_dict(vec![
        (b"announce".to_vec(), ann),
        (b"info".to_vec(), bencode::encode_raw_dict(info)),
    ])
}
