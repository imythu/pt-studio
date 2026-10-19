//! 种子解析与规范化（torrent 03 §1.2、§1.3）。

use pt_plugin_api::{PResult, PluginError, json};
use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::bencode::{self, BError, BValue, Limits};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetaVersion {
    V1 = 1,
    V2 = 2,
    Hybrid = 3,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FileEntry {
    pub path: Vec<String>,
    pub size: u64,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub padding: bool,
}

#[derive(Debug, Clone)]
pub struct Parsed {
    pub meta: MetaVersion,
    pub name: String,
    pub piece_length: u64,
    pub files: Vec<FileEntry>,
    pub total_size: u64,
    pub pieces_sha1: Option<[u8; 20]>,
    pub was_private: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HybridPolicy {
    StripV2,
    KeepHybrid,
    Reject,
}

#[derive(Debug, Clone)]
pub struct Policy {
    pub hybrid: HybridPolicy,
    pub accept_v2_only: bool,
    pub reject_non_private: bool,
    pub source_tag: String,
    pub max_files: usize,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            hybrid: HybridPolicy::KeepHybrid,
            accept_v2_only: false,
            reject_non_private: false,
            source_tag: "PT Studio".into(),
            max_files: 20_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Normalized {
    pub stored: Vec<u8>,
    pub ih_v1: Option<[u8; 20]>,
    pub ih_v2: Option<[u8; 32]>,
    pub meta: MetaVersion,
}

pub fn invalid(reason: &str) -> PluginError {
    PluginError::new("torrent.invalid_file", "invalid .torrent file")
        .with_details(json!({ "reason": reason }))
}

fn invalid_path(reason: &str) -> PluginError {
    PluginError::new("torrent.invalid_path", "invalid file path in torrent")
        .with_details(json!({ "reason": reason }))
}

impl From<BError> for PluginError {
    fn from(e: BError) -> Self {
        invalid(e.0)
    }
}

fn utf8(v: Option<&BValue<'_>>) -> Option<String> {
    v.and_then(BValue::as_bytes)
        .and_then(|b| std::str::from_utf8(b).ok())
        .map(str::to_owned)
}

const WINDOWS_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9",
    "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

fn check_component(s: &str) -> PResult<()> {
    if s.is_empty() || s == "." || s == ".." {
        return Err(invalid_path("empty_or_dot"));
    }
    if s.len() > 255 {
        return Err(invalid_path("component_too_long"));
    }
    if s.chars().any(|c| c == '/' || c == '\\' || c.is_control()) {
        return Err(invalid_path("forbidden_char"));
    }
    if s.ends_with(' ') {
        return Err(invalid_path("trailing_space"));
    }
    let stem = s.split('.').next().unwrap_or(s).to_ascii_lowercase();
    if WINDOWS_RESERVED.contains(&stem.as_str()) {
        return Err(invalid_path("reserved_name"));
    }
    Ok(())
}

fn path_of(f: &BValue<'_>) -> PResult<Vec<String>> {
    let list = f
        .get("path.utf-8")
        .or_else(|| f.get("path"))
        .and_then(BValue::as_list)
        .ok_or_else(|| invalid("path"))?;
    if list.is_empty() {
        return Err(invalid_path("empty_path"));
    }
    list.iter()
        .map(|p| {
            let s = p
                .as_bytes()
                .and_then(|b| std::str::from_utf8(b).ok())
                .ok_or_else(|| invalid("path_encoding"))?;
            check_component(s)?;
            Ok(s.to_owned())
        })
        .collect()
}

/// BEP 47 符号链接目标：相对种子根目录的路径分量，与文件路径同样校验
fn symlink_of(v: &BValue<'_>) -> PResult<Vec<String>> {
    let list = v
        .as_list()
        .filter(|l| !l.is_empty())
        .ok_or_else(|| invalid_path("symlink"))?;
    list.iter()
        .map(|p| {
            let s = p
                .as_bytes()
                .and_then(|b| std::str::from_utf8(b).ok())
                .ok_or_else(|| invalid("path_encoding"))?;
            check_component(s)?;
            Ok(s.to_owned())
        })
        .collect()
}

fn encode_str_list(items: &[String], out: &mut Vec<u8>) {
    out.push(b'l');
    for s in items {
        bencode::encode_bytes(s.as_bytes(), out);
    }
    out.push(b'e');
}

/// v2 file tree：叶子为键 "" 的字典 `{length, pieces root?}`。返回 (路径, 大小, pieces root)。
/// (路径, 大小, pieces root)
type V2File = (Vec<String>, u64, Option<Vec<u8>>);

fn walk_tree(tree: &BValue<'_>, prefix: &mut Vec<String>, out: &mut Vec<V2File>) -> PResult<()> {
    for (k, v) in tree.entries().ok_or_else(|| invalid("file_tree"))? {
        if k.is_empty() {
            let len = v
                .get("length")
                .and_then(BValue::as_int)
                .filter(|l| *l >= 0)
                .ok_or_else(|| invalid("file_tree"))?;
            let root = v
                .get("pieces root")
                .and_then(BValue::as_bytes)
                .map(<[u8]>::to_vec);
            out.push((prefix.clone(), u64::try_from(len).unwrap_or(0), root));
            continue;
        }
        let name = std::str::from_utf8(k).map_err(|_| invalid("path_encoding"))?;
        check_component(name)?;
        prefix.push(name.to_owned());
        walk_tree(v, prefix, out)?;
        prefix.pop();
    }
    Ok(())
}

pub fn parse(root: &BValue<'_>, max_files: usize) -> PResult<Parsed> {
    let info = root
        .get("info")
        .filter(|v| v.entries().is_some())
        .ok_or_else(|| invalid("no_info"))?;
    let has_v1 = info.get("pieces").is_some();
    let has_v2 = info.get("meta version").and_then(BValue::as_int) == Some(2);
    let meta = match (has_v1, has_v2) {
        (true, false) => MetaVersion::V1,
        (false, true) => MetaVersion::V2,
        (true, true) => MetaVersion::Hybrid,
        _ => return Err(invalid("no_pieces")),
    };
    let name = utf8(info.get("name.utf-8"))
        .or_else(|| utf8(info.get("name")))
        .ok_or_else(|| invalid("name_encoding"))?;
    check_component(&name)?;
    let pl = info
        .get("piece length")
        .and_then(BValue::as_int)
        .ok_or_else(|| invalid("piece_length"))?;
    if !(16_384..=67_108_864).contains(&pl) || pl.count_ones() != 1 {
        return Err(invalid("piece_length"));
    }
    let pl = u64::try_from(pl).unwrap_or(0);

    let mut files_v1 = Vec::new();
    let mut pieces_sha1 = None;
    if has_v1 {
        let single = info.get("length");
        let multi = info.get("files");
        if single.is_some() && multi.is_some() {
            return Err(invalid("length_and_files"));
        }
        if let Some(l) = single {
            let size = l.as_int().filter(|x| *x >= 0).ok_or_else(|| invalid("length"))?;
            files_v1.push(FileEntry {
                path: vec![name.clone()],
                size: u64::try_from(size).unwrap_or(0),
                padding: false,
            });
        } else if let Some(list) = multi.and_then(BValue::as_list) {
            for f in list {
                let size = f
                    .get("length")
                    .and_then(BValue::as_int)
                    .filter(|x| *x >= 0)
                    .ok_or_else(|| invalid("length"))?;
                let padding = f
                    .get("attr")
                    .and_then(BValue::as_bytes)
                    .is_some_and(|a| a.contains(&b'p'));
                let mut path = vec![name.clone()];
                path.extend(path_of(f)?);
                files_v1.push(FileEntry {
                    path,
                    size: u64::try_from(size).unwrap_or(0),
                    padding,
                });
            }
        } else {
            return Err(invalid("no_files"));
        }
        let total: u64 = files_v1.iter().map(|f| f.size).sum();
        let pieces = info
            .get("pieces")
            .and_then(BValue::as_bytes)
            .ok_or_else(|| invalid("pieces"))?;
        let expected = total
            .div_ceil(pl)
            .checked_mul(20)
            .ok_or_else(|| invalid("pieces_len"))?;
        if u64::try_from(pieces.len()).ok() != Some(expected) {
            return Err(invalid("pieces_len"));
        }
        pieces_sha1 = Some(Sha1::digest(pieces).into());
    }
    let mut files_v2 = Vec::new();
    if has_v2 {
        let tree = info.get("file tree").ok_or_else(|| invalid("no_file_tree"))?;
        let mut raw = Vec::new();
        walk_tree(tree, &mut vec![], &mut raw)?;
        let layers = root.get("piece layers");
        for (_, len, proot) in &raw {
            if *len > pl {
                let proot = proot.as_deref().ok_or_else(|| invalid("piece_layers"))?;
                let layer = layers
                    .and_then(|l| l.entries())
                    .and_then(|e| e.iter().find(|(k, _)| *k == proot))
                    .and_then(|(_, v)| v.as_bytes())
                    .ok_or_else(|| invalid("piece_layers"))?;
                if u64::try_from(layer.len()).ok() != Some(len.div_ceil(pl) * 32) {
                    return Err(invalid("piece_layers"));
                }
            }
        }
        files_v2 = raw
            .into_iter()
            .map(|(mut p, size, _)| {
                p.insert(0, name.clone());
                FileEntry {
                    path: p,
                    size,
                    padding: false,
                }
            })
            .collect();
    }
    if meta == MetaVersion::Hybrid {
        let a: Vec<(&[String], u64)> = files_v1
            .iter()
            .filter(|f| !f.padding)
            .map(|f| (f.path.as_slice(), f.size))
            .collect();
        let b: Vec<(&[String], u64)> = files_v2.iter().map(|f| (f.path.as_slice(), f.size)).collect();
        // 单文件 v1 的路径就是 name；v2 单文件为 name/<file>
        let same = a.len() == b.len()
            && a.iter()
                .zip(&b)
                .all(|(x, y)| x.1 == y.1 && (x.0 == y.0 || a.len() == 1));
        if !same {
            return Err(invalid("hybrid_mismatch"));
        }
    }
    let files = if has_v1 { files_v1 } else { files_v2 };
    if files.len() > max_files {
        return Err(PluginError::new(
            "torrent.too_many_files",
            "too many files in torrent",
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for f in &files {
        let joined = f.path.join("/");
        if joined.len() > 4096 {
            return Err(invalid_path("path_too_long"));
        }
        if !seen.insert(joined) {
            return Err(invalid_path("duplicate_path"));
        }
    }
    let total_size = files.iter().filter(|f| !f.padding).map(|f| f.size).sum();
    Ok(Parsed {
        meta,
        name,
        piece_length: pl,
        files,
        total_size,
        pieces_sha1,
        was_private: info.get("private").and_then(BValue::as_int) == Some(1),
    })
}

/// 规范化：白名单拷贝 info 字段，强制 private=1 与 source；announce 等在下载时拼接，不存储。
pub fn normalize(root: &BValue<'_>, parsed: &Parsed, policy: &Policy) -> PResult<Normalized> {
    let target = match (parsed.meta, policy.hybrid, policy.accept_v2_only) {
        (MetaVersion::Hybrid, HybridPolicy::Reject, _) => {
            return Err(PluginError::new(
                "torrent.hybrid_not_supported",
                "hybrid torrents are not accepted",
            ));
        }
        (MetaVersion::V2, _, false) => {
            return Err(PluginError::new(
                "torrent.v2_not_supported",
                "v2-only torrents are not accepted",
            ));
        }
        (MetaVersion::Hybrid, HybridPolicy::StripV2, _) => MetaVersion::V1,
        (m, _, _) => m,
    };
    if policy.reject_non_private && !parsed.was_private {
        return Err(PluginError::new(
            "torrent.not_private",
            "torrent must have private=1",
        ));
    }
    let info = root.get("info").ok_or_else(|| invalid("no_info"))?;
    let with_v1 = matches!(target, MetaVersion::V1 | MetaVersion::Hybrid);
    let with_v2 = matches!(target, MetaVersion::V2 | MetaVersion::Hybrid);
    // name / path 只保留校验过的 UTF-8 值（写回 name / path，丢弃 *.utf-8）：
    // 否则可用干净的 name.utf-8 / path.utf-8 通过校验，同时在原始 name / path 里夹带 `..` 等，
    // 由只认原始字段的客户端落盘到种子目录之外
    let mut keep: Vec<&[u8]> = vec![b"piece length"];
    if with_v1 {
        keep.extend([&b"pieces"[..], b"length", b"files", b"md5sum"]);
    }
    if with_v2 {
        keep.extend([&b"meta version"[..], b"file tree"]);
    }
    let mut name = Vec::new();
    bencode::encode_bytes(parsed.name.as_bytes(), &mut name);
    let mut entries: Vec<(Vec<u8>, Vec<u8>)> = vec![(b"name".to_vec(), name)];
    for (k, v) in info.entries().unwrap_or_default() {
        if !keep.contains(k) {
            continue;
        }
        let mut raw = Vec::new();
        if *k == b"files" {
            // 逐条只保留已知字段
            raw.push(b'l');
            for f in v.as_list().unwrap_or_default() {
                let mut path = Vec::new();
                encode_str_list(&path_of(f)?, &mut path);
                let mut fe: Vec<(Vec<u8>, Vec<u8>)> = vec![(b"path".to_vec(), path)];
                for (fk, fv) in f.entries().unwrap_or_default() {
                    let mut b = Vec::new();
                    match *fk {
                        b"symlink path" => encode_str_list(&symlink_of(fv)?, &mut b),
                        b"length" | b"attr" | b"sha1" => bencode::encode(fv, &mut b),
                        _ => continue,
                    }
                    fe.push((fk.to_vec(), b));
                }
                raw.extend(bencode::encode_raw_dict(fe));
            }
            raw.push(b'e');
        } else {
            bencode::encode(v, &mut raw);
        }
        entries.push((k.to_vec(), raw));
    }
    let mut private = Vec::new();
    bencode::encode_int(1, &mut private);
    entries.push((b"private".to_vec(), private));
    let mut source = Vec::new();
    bencode::encode_bytes(policy.source_tag.as_bytes(), &mut source);
    entries.push((b"source".to_vec(), source));
    let info_raw = bencode::encode_raw_dict(entries);
    let ih_v1: Option<[u8; 20]> = with_v1.then(|| Sha1::digest(&info_raw).into());
    let ih_v2: Option<[u8; 32]> = with_v2.then(|| Sha256::digest(&info_raw).into());
    let mut top = vec![(b"info".to_vec(), info_raw)];
    if with_v2 && let Some(pl) = root.get("piece layers") {
        let mut raw = Vec::new();
        bencode::encode(pl, &mut raw);
        top.push((b"piece layers".to_vec(), raw));
    }
    Ok(Normalized {
        stored: bencode::encode_raw_dict(top),
        ih_v1,
        ih_v2,
        meta: target,
    })
}

pub fn process(file: &[u8], policy: &Policy) -> PResult<(Parsed, Normalized)> {
    let root = bencode::decode(file, &Limits::default())?;
    let parsed = parse(&root, policy.max_files)?;
    let norm = normalize(&root, &parsed, policy)?;
    Ok((parsed, norm))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::sample_v1;

    #[test]
    fn v1_roundtrip_forces_private_and_source() -> PResult<()> {
        let t = sample_v1("Movie.2024", &[("a.mkv", 40_000), ("b.nfo", 100)], false);
        let (p, n) = process(&t, &Policy::default())?;
        assert_eq!(p.meta, MetaVersion::V1);
        assert_eq!(p.total_size, 40_100);
        assert_eq!(p.files[0].path, vec!["Movie.2024", "a.mkv"]);
        let root = bencode::decode(&n.stored, &Limits::default())?;
        let info = root.get("info").ok_or_else(|| invalid("x"))?;
        assert_eq!(info.get("private").and_then(BValue::as_int), Some(1));
        assert_eq!(
            info.get("source").and_then(BValue::as_bytes),
            Some(&b"PT Studio"[..])
        );
        assert!(root.get("announce").is_none(), "announce must not be stored");
        assert_eq!(
            n.ih_v1,
            Some(<[u8; 20]>::from(Sha1::digest(info.raw().unwrap_or_default())))
        );
        // 再次规范化结果稳定（幂等）
        let (_, n2) = process(&n.stored, &Policy::default())?;
        assert_eq!(n.ih_v1, n2.ih_v1);
        Ok(())
    }

    #[test]
    fn rejects_bad_content() {
        let code = |t: &[u8], pol: &Policy| process(t, pol).err().map(|e| (e.code, e.details));
        let bad_path = sample_v1("x", &[("..", 10)], true);
        assert_eq!(
            code(&bad_path, &Policy::default()).map(|c| c.0),
            Some("torrent.invalid_path".into())
        );
        let dup = sample_v1("x", &[("a", 10), ("a", 10)], true);
        assert_eq!(
            code(&dup, &Policy::default()).map(|c| c.0),
            Some("torrent.invalid_path".into())
        );
        let public = sample_v1("x", &[("a", 10)], false);
        let strict = Policy {
            reject_non_private: true,
            ..Policy::default()
        };
        assert_eq!(
            code(&public, &strict).map(|c| c.0),
            Some("torrent.not_private".into())
        );
        let few = Policy {
            max_files: 1,
            ..Policy::default()
        };
        assert_eq!(
            code(&sample_v1("x", &[("a", 1), ("b", 1)], true), &few).map(|c| c.0),
            Some("torrent.too_many_files".into())
        );
        assert_eq!(
            code(b"d4:infod4:name1:xee", &Policy::default()).map(|c| c.0),
            Some("torrent.invalid_file".into())
        );
        assert!(check_component("CON.txt").is_err());
    }

    /// 手工拼一个单文件条目的多文件 v1 种子（name 与 files[0] 的键值原样写入）
    fn raw_v1(name_keys: &[(&[u8], &[u8])], file_keys: &[(&[u8], Vec<u8>)]) -> Vec<u8> {
        let mut fe = Vec::new();
        let mut len = Vec::new();
        bencode::encode_int(10, &mut len);
        fe.push((b"length".to_vec(), len));
        for (k, v) in file_keys {
            fe.push((k.to_vec(), v.clone()));
        }
        let mut files = vec![b'l'];
        files.extend(bencode::encode_raw_dict(fe));
        files.push(b'e');
        let mut info = vec![(b"files".to_vec(), files)];
        for (k, v) in name_keys {
            let mut b = Vec::new();
            bencode::encode_bytes(v, &mut b);
            info.push((k.to_vec(), b));
        }
        let mut pl = Vec::new();
        bencode::encode_int(16_384, &mut pl);
        info.push((b"piece length".to_vec(), pl));
        let mut pc = Vec::new();
        bencode::encode_bytes(&[7u8; 20], &mut pc);
        info.push((b"pieces".to_vec(), pc));
        bencode::encode_raw_dict(vec![(b"info".to_vec(), bencode::encode_raw_dict(info))])
    }

    fn list(items: &[&[u8]]) -> Vec<u8> {
        let mut b = vec![b'l'];
        for i in items {
            bencode::encode_bytes(i, &mut b);
        }
        b.push(b'e');
        b
    }

    #[test]
    fn stored_paths_are_the_validated_utf8_ones() -> PResult<()> {
        // 原始 name / path 夹带穿越，*.utf-8 是干净值：存储结果只能出现校验过的值
        let t = raw_v1(
            &[(b"name", b".."), (b"name.utf-8", b"Movie")],
            &[
                (b"path", list(&[b"..", b"..", b"evil.sh"])),
                (b"path.utf-8", list(&[b"a.mkv"])),
            ],
        );
        let (p, n) = process(&t, &Policy::default())?;
        assert_eq!(p.files[0].path, vec!["Movie", "a.mkv"]);
        let root = bencode::decode(&n.stored, &Limits::default())?;
        let info = root.get("info").ok_or_else(|| invalid("x"))?;
        assert_eq!(info.get("name").and_then(BValue::as_bytes), Some(&b"Movie"[..]));
        assert!(info.get("name.utf-8").is_none());
        let f = &info.get("files").and_then(BValue::as_list).unwrap_or_default()[0];
        assert!(f.get("path.utf-8").is_none());
        let path: Vec<&[u8]> = f
            .get("path")
            .and_then(BValue::as_list)
            .unwrap_or_default()
            .iter()
            .filter_map(BValue::as_bytes)
            .collect();
        assert_eq!(path, vec![&b"a.mkv"[..]]);
        // 规范化幂等：再处理一次 info hash 不变
        let (_, n2) = process(&n.stored, &Policy::default())?;
        assert_eq!(n.ih_v1, n2.ih_v1);
        Ok(())
    }

    #[test]
    fn rejects_escaping_symlink() {
        let t = raw_v1(
            &[(b"name", b"Movie")],
            &[
                (b"path", list(&[b"link"])),
                (b"attr", b"1:l".to_vec()),
                (b"symlink path", list(&[b"..", b"etc", b"passwd"])),
            ],
        );
        assert_eq!(
            process(&t, &Policy::default()).err().map(|e| e.code),
            Some("torrent.invalid_path".into())
        );
    }
}
