//! 字幕文件检查：按内容识别格式（不信任扩展名之外的声明），ZIP 只读中央目录检查条目（不解压），编码粗分。

pub const TEXT_EXTS: &[&str] = &["srt", "ass", "ssa", "vtt"];
/// 压缩包内允许的条目扩展名
pub const ARCHIVE_ENTRY_EXTS: &[&str] = &["srt", "ass", "ssa", "vtt", "sup", "idx", "sub", "txt", "nfo", "ttf", "otf"];

#[derive(Debug, PartialEq, Eq)]
pub struct Checked {
    pub format: &'static str,
    pub encoding: Option<&'static str>,
    /// 压缩包条目 (名称, 解压后大小)
    pub entries: Vec<(String, u64)>,
}

fn ext_of(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

/// 文本字幕的编码：UTF-8（含 BOM）/ UTF-16（BOM）/ legacy（GBK、Big5 等，原样保存）
pub fn encoding(b: &[u8]) -> &'static str {
    if b.starts_with(&[0xFF, 0xFE]) || b.starts_with(&[0xFE, 0xFF]) {
        "utf-16"
    } else if std::str::from_utf8(b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b)).is_ok() {
        "utf-8"
    } else {
        "legacy"
    }
}

/// 文本字幕的特征（取前 64 KiB，按 UTF-8 / UTF-16LE / 字节宽松匹配）
fn text_head(b: &[u8]) -> String {
    let head = &b[..b.len().min(64 * 1024)];
    if head.starts_with(&[0xFF, 0xFE]) {
        let u: Vec<u16> = head[2..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        return String::from_utf16_lossy(&u);
    }
    if head.starts_with(&[0xFE, 0xFF]) {
        let u: Vec<u16> = head[2..].as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect();
        return String::from_utf16_lossy(&u);
    }
    String::from_utf8_lossy(head).into_owned()
}

fn text_format(ext: &str, b: &[u8]) -> Option<&'static str> {
    let h = text_head(b);
    let t = h.trim_start_matches('\u{feff}').trim_start();
    match ext {
        "srt" if h.contains("-->") => Some("srt"),
        "vtt" if t.starts_with("WEBVTT") => Some("vtt"),
        "ass" | "ssa" if h.contains("[Script Info]") || h.contains("[Events]") => Some(if ext == "ass" { "ass" } else { "ssa" }),
        _ => None,
    }
}

fn le16(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?])))
}
fn le32(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from(u32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?])))
}

/// ZIP：定位中央目录结束记录，逐条读取文件名、标志、解压后大小
pub fn zip_entries(b: &[u8], max_entries: usize, max_total: u64) -> Result<Vec<(String, u64)>, &'static str> {
    // EOCD 在最后 22 + 65535 字节内
    let start = b.len().saturating_sub(22 + 65_535);
    let eocd = (start..b.len().saturating_sub(21)).rev().find(|&i| b[i..].starts_with(b"PK\x05\x06")).ok_or("not a zip archive")?;
    let count = usize::try_from(le16(b, eocd + 10).ok_or("bad zip")?).map_err(|_| "bad zip")?;
    let mut off = usize::try_from(le32(b, eocd + 16).ok_or("bad zip")?).map_err(|_| "bad zip")?;
    if count > max_entries {
        return Err("too many files in the archive");
    }
    let mut out = Vec::new();
    let mut total = 0u64;
    for _ in 0..count {
        if !b.get(off..).is_some_and(|x| x.starts_with(b"PK\x01\x02")) {
            return Err("bad zip");
        }
        let flags = le16(b, off + 8).ok_or("bad zip")?;
        let size = le32(b, off + 24).ok_or("bad zip")?;
        let n = usize::try_from(le16(b, off + 28).ok_or("bad zip")?).map_err(|_| "bad zip")?;
        let extra = usize::try_from(le16(b, off + 30).ok_or("bad zip")?).map_err(|_| "bad zip")?;
        let comment = usize::try_from(le16(b, off + 32).ok_or("bad zip")?).map_err(|_| "bad zip")?;
        let name = String::from_utf8_lossy(b.get(off + 46..off + 46 + n).ok_or("bad zip")?).into_owned();
        if flags & 1 != 0 {
            return Err("encrypted archives are not accepted");
        }
        if size == 0xFFFF_FFFF {
            return Err("zip64 archives are not accepted");
        }
        if name.starts_with('/') || name.starts_with('\\') || name.split(['/', '\\']).any(|p| p == "..") || name.contains(':') {
            return Err("unsafe file name in the archive");
        }
        if !name.ends_with('/') {
            let base = name.rsplit(['/', '\\']).next().unwrap_or(&name);
            if !ARCHIVE_ENTRY_EXTS.contains(&ext_of(base).as_str()) {
                return Err("the archive contains files that are not subtitles");
            }
            total += size;
            if total > max_total {
                return Err("the archive is too large when extracted");
            }
            out.push((name, size));
        }
        off += 46 + n + extra + comment;
    }
    if out.is_empty() {
        return Err("the archive is empty");
    }
    Ok(out)
}

/// 检查上传的文件：返回格式与（压缩包的）条目；错误为给用户看的英文说明
pub fn check(filename: &str, b: &[u8], max_entries: usize, max_total: u64) -> Result<Checked, &'static str> {
    let ext = ext_of(filename);
    if TEXT_EXTS.contains(&ext.as_str()) {
        let format = text_format(&ext, b).ok_or("the file content does not match its subtitle format")?;
        return Ok(Checked { format, encoding: Some(encoding(b)), entries: vec![] });
    }
    match ext.as_str() {
        "sup" if b.starts_with(b"PG") => Ok(Checked { format: "sup", encoding: None, entries: vec![] }),
        "zip" if b.starts_with(b"PK\x03\x04") => Ok(Checked { format: "zip", encoding: None, entries: zip_entries(b, max_entries, max_total)? }),
        "sup" | "zip" => Err("the file content does not match its subtitle format"),
        "7z" | "rar" => Err("only ZIP archives are accepted"),
        _ => Err("unsupported subtitle format"),
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// 构造只有中央目录有效的 ZIP（条目内容不检查）
    pub fn zip(names: &[(&str, u32)], flags: u16) -> Vec<u8> {
        let mut b = b"PK\x03\x04".to_vec();
        b.extend_from_slice(&[0; 26]);
        let cd = u32::try_from(b.len()).unwrap_or(0);
        for (name, size) in names {
            b.extend_from_slice(b"PK\x01\x02");
            b.extend_from_slice(&[0; 4]);
            b.extend_from_slice(&flags.to_le_bytes());
            b.extend_from_slice(&[0; 14]);
            b.extend_from_slice(&size.to_le_bytes());
            b.extend_from_slice(&u16::try_from(name.len()).unwrap_or(0).to_le_bytes());
            b.extend_from_slice(&[0; 16]);
            b.extend_from_slice(name.as_bytes());
        }
        let cd_len = u32::try_from(b.len()).unwrap_or(0) - cd;
        b.extend_from_slice(b"PK\x05\x06");
        b.extend_from_slice(&[0; 4]);
        let n = u16::try_from(names.len()).unwrap_or(0);
        b.extend_from_slice(&n.to_le_bytes());
        b.extend_from_slice(&n.to_le_bytes());
        b.extend_from_slice(&cd_len.to_le_bytes());
        b.extend_from_slice(&cd.to_le_bytes());
        b.extend_from_slice(&[0; 2]);
        b
    }

    #[test]
    fn text_formats() {
        let srt = "1\n00:00:01,000 --> 00:00:02,000\n你好\n".as_bytes();
        assert_eq!(check("a.srt", srt, 10, 1 << 20).map(|c| (c.format, c.encoding)), Ok(("srt", Some("utf-8"))));
        assert_eq!(check("a.vtt", b"\xEF\xBB\xBFWEBVTT\n\n", 10, 1 << 20).map(|c| c.format), Ok("vtt"));
        let ass = b"[Script Info]\nTitle: x\n\xC4\xE3\xBA\xC3"; // GBK 的“你好”
        assert_eq!(check("a.ass", ass, 10, 1 << 20).map(|c| c.encoding), Ok(Some("legacy")));
        let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain("1\n00:00:01,000 --> 00:00:02,000\n".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(check("b.srt", &utf16, 10, 1 << 20).map(|c| c.encoding), Ok(Some("utf-16")));
        assert!(check("a.srt", b"<html>", 10, 1 << 20).is_err());
        assert!(check("a.exe", b"MZ", 10, 1 << 20).is_err());
        assert_eq!(check("a.rar", b"Rar!", 10, 1 << 20), Err("only ZIP archives are accepted"));
    }

    #[test]
    fn zip_checks() {
        let ok = zip(&[("Movie/", 0), ("Movie/chs.srt", 1000), ("Movie/cht.ass", 2000)], 0);
        let c = check("pack.zip", &ok, 10, 1 << 20);
        assert_eq!(c.map(|c| c.entries.len()), Ok(2));
        assert_eq!(check("pack.zip", &zip(&[("run.exe", 10)], 0), 10, 1 << 20).err(), Some("the archive contains files that are not subtitles"));
        assert_eq!(check("pack.zip", &zip(&[("../x.srt", 10)], 0), 10, 1 << 20).err(), Some("unsafe file name in the archive"));
        assert_eq!(check("pack.zip", &zip(&[("a.srt", 10)], 1), 10, 1 << 20).err(), Some("encrypted archives are not accepted"));
        assert_eq!(check("pack.zip", &zip(&[("a.srt", 900_000), ("b.srt", 900_000)], 0), 10, 1 << 20).err(), Some("the archive is too large when extracted"));
        assert_eq!(check("pack.zip", &zip(&[("a.srt", 1), ("b.srt", 1)], 0), 1, 1 << 20).err(), Some("too many files in the archive"));
    }
}
