//! 发布名预处理（LLD §1.7）：标题规范化与发布名解析（纯函数）。

use serde_json::{Value, json};

/// 解析结果，写入文档的 `rel_*` 字段
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReleaseInfo {
    pub resolution: Option<String>,
    pub source: Option<String>,
    pub video_codec: Option<String>,
    pub audio_codec: Vec<String>,
    pub hdr: Vec<String>,
    pub group: Option<String>,
    pub year: Option<i64>,
}

impl ReleaseInfo {
    pub fn to_fields(&self) -> Vec<(&'static str, Value)> {
        vec![
            ("rel_resolution", json!(self.resolution)),
            ("rel_source", json!(self.source)),
            ("rel_video_codec", json!(self.video_codec)),
            ("rel_audio_codec", json!(self.audio_codec)),
            ("rel_hdr", json!(self.hdr)),
            ("rel_group", json!(self.group)),
            ("rel_year", json!(self.year)),
        ]
    }
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xAC00..=0xD7AF)
}

/// 全角 ASCII → 半角，去控制字符
fn fold(s: &str) -> Vec<char> {
    s.chars()
        .filter(|c| !c.is_control())
        .map(|c| match c as u32 {
            0xFF01..=0xFF5E => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            0x3000 => ' ',
            _ => c,
        })
        .collect()
}

/// 标题规范化：分隔符变空格（保护 `5.1`、`H.264` 等片段），中文与拉丁 / 数字之间插入空格。
pub fn normalize_title(s: &str) -> String {
    let cs = fold(s);
    let mut out = String::with_capacity(s.len() + 8);
    for (i, &c) in cs.iter().enumerate() {
        let prev = i.checked_sub(1).and_then(|j| cs.get(j)).copied();
        let next = cs.get(i + 1).copied();
        let sep = match c {
            '.' => {
                // 声道 / 版本号（5.1、7.1、2.0）：小数点后恰好一位数字；`流浪地球2.2023` 仍拆开
                let after = cs[i + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
                let digits = prev.is_some_and(|p| p.is_ascii_digit()) && after == 1;
                // H.264 / H.265 / x.264
                let codec = prev.is_some_and(|p| matches!(p, 'h' | 'H' | 'x' | 'X'))
                    && next == Some('2')
                    && cs.get(i + 2) == Some(&'6');
                !(digits || codec)
            }
            '_' | '[' | ']' | '【' | '】' | '(' | ')' | '（' | '）' => true,
            _ => false,
        };
        let c = if sep { ' ' } else { c };
        if let Some(p) = out.chars().last()
            && c != ' '
            && p != ' '
            && ((is_cjk(p) && c.is_ascii_alphanumeric()) || (p.is_ascii_alphanumeric() && is_cjk(c)))
        {
            out.push(' ');
        }
        if c == ' ' && out.ends_with(' ') {
            continue;
        }
        out.push(c);
    }
    out.trim().to_owned()
}

/// 词元：按非字母数字切分（保留 `+`），小写
fn tokens(s: &str) -> Vec<String> {
    fold(s)
        .iter()
        .collect::<String>()
        .split(|c: char| !(c.is_alphanumeric() || c == '+'))
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn push_unique(v: &mut Vec<String>, s: &str) {
    if !v.iter().any(|x| x == s) {
        v.push(s.to_owned());
    }
}

const NOT_GROUPS: &[&str] = &["DL", "HD", "MA", "RAY", "RIP", "X", "10BIT", "8BIT"];

fn release_group(s: &str) -> Option<String> {
    let s = s.trim();
    // `-XYZ@HDSky`：@ 后为发布站点，取 @ 前
    let s = match s.rfind('@') {
        Some(at) if s[at + 1..].chars().all(|c| c.is_ascii_alphanumeric()) => &s[..at],
        _ => s,
    };
    let dash = s.rfind('-')?;
    let g = &s[dash + 1..];
    let ok = (2..=20).contains(&g.len())
        && g.chars().all(|c| c.is_ascii_alphanumeric())
        && !g.chars().all(|c| c.is_ascii_digit())
        && !NOT_GROUPS.contains(&g.to_ascii_uppercase().as_str());
    ok.then(|| g.to_owned())
}

/// 词典驱动的发布名解析（LLD §1.7 表；多次出现取最后者，音频 / HDR 为出现顺序的去重数组）
pub fn parse_release(s: &str) -> ReleaseInfo {
    let t = tokens(s);
    let mut r = ReleaseInfo {
        group: release_group(s),
        ..Default::default()
    };
    let mut remux = false;
    for (i, tok) in t.iter().enumerate() {
        let next = t.get(i + 1).map(String::as_str);
        let next2 = t.get(i + 2).map(String::as_str);
        // 去掉声道数字后缀：ddp5 → ddp、aac2 → aac
        let stem = tok.trim_end_matches(|c: char| c.is_ascii_digit());
        match tok.as_str() {
            "4320p" | "2160p" | "1080p" | "1080i" | "720p" | "576p" | "480p" => {
                r.resolution = Some(tok.clone())
            }
            "4k" | "uhd" => r.resolution = Some("2160p".into()),
            _ => {}
        }
        match (tok.as_str(), next) {
            ("remux", _) => remux = true,
            ("bluray" | "bd" | "bdrip", _) | ("blu", Some("ray")) => r.source = Some("bluray".into()),
            ("webdl", _) | ("web", Some("dl")) => r.source = Some("web_dl".into()),
            ("webrip", _) | ("web", Some("rip")) => r.source = Some("webrip".into()),
            ("hdtv", _) => r.source = Some("hdtv".into()),
            ("dvd" | "dvdrip", _) => r.source = Some("dvd".into()),
            _ => {}
        }
        match (tok.as_str(), next) {
            ("x265" | "h265" | "hevc", _) | ("h" | "x", Some("265")) => r.video_codec = Some("hevc".into()),
            ("x264" | "h264" | "avc", _) | ("h" | "x", Some("264")) => r.video_codec = Some("avc".into()),
            ("av1", _) => r.video_codec = Some("av1".into()),
            ("vc1", _) | ("vc", Some("1")) => r.video_codec = Some("vc1".into()),
            ("mpeg2", _) | ("mpeg", Some("2")) => r.video_codec = Some("mpeg2".into()),
            _ => {}
        }
        match (stem, next, next2) {
            ("truehd", ..) => push_unique(&mut r.audio_codec, "truehd"),
            ("atmos", ..) => push_unique(&mut r.audio_codec, "atmos"),
            ("dts", Some("hd"), Some("ma")) => push_unique(&mut r.audio_codec, "dts_hd_ma"),
            ("dts", Some("x"), _) => push_unique(&mut r.audio_codec, "dts_x"),
            ("dts", Some(n), _) if n != "hd" && n != "x" => push_unique(&mut r.audio_codec, "dts"),
            ("dts", None, _) => push_unique(&mut r.audio_codec, "dts"),
            ("ddp" | "dd+" | "eac3", ..) => push_unique(&mut r.audio_codec, "ddp"),
            ("dd" | "ac3", ..) => push_unique(&mut r.audio_codec, "dd"),
            ("aac", ..) => push_unique(&mut r.audio_codec, "aac"),
            ("flac", ..) => push_unique(&mut r.audio_codec, "flac"),
            ("lpcm", ..) => push_unique(&mut r.audio_codec, "lpcm"),
            ("opus", ..) => push_unique(&mut r.audio_codec, "opus"),
            _ => {}
        }
        match (tok.as_str(), next) {
            ("dv" | "dovi", _) | ("dolby", Some("vision")) => push_unique(&mut r.hdr, "dv"),
            ("hdr10+", _) => push_unique(&mut r.hdr, "hdr10plus"),
            ("hdr10" | "hdr", _) => push_unique(&mut r.hdr, "hdr10"),
            ("hlg", _) => push_unique(&mut r.hdr, "hlg"),
            _ => {}
        }
        if tok.len() == 4
            && (tok.starts_with("19") || tok.starts_with("20"))
            && tok.chars().all(|c| c.is_ascii_digit())
            && let Ok(y) = tok.parse()
        {
            r.year = Some(y);
        }
    }
    if remux {
        r.source = Some("remux".into());
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn appendix_a() {
        let r = parse_release("The.Wandering.Earth.II.2023.2160p.WEB-DL.H265.DDP5.1-ABC");
        assert_eq!(r.resolution.as_deref(), Some("2160p"));
        assert_eq!(r.source.as_deref(), Some("web_dl"));
        assert_eq!(r.video_codec.as_deref(), Some("hevc"));
        assert_eq!(r.audio_codec, v(&["ddp"]));
        assert!(r.hdr.is_empty());
        assert_eq!(r.group.as_deref(), Some("ABC"));
        assert_eq!(r.year, Some(2023));

        let r = parse_release(
            "流浪地球2.The.Wandering.Earth.II.2023.UHD.BluRay.2160p.x265.10bit.DoVi.TrueHD.Atmos.7.1-XYZ@HDSky",
        );
        assert_eq!(r.resolution.as_deref(), Some("2160p"));
        assert_eq!(r.source.as_deref(), Some("bluray"));
        assert_eq!(r.video_codec.as_deref(), Some("hevc"));
        assert_eq!(r.audio_codec, v(&["truehd", "atmos"]));
        assert_eq!(r.hdr, v(&["dv"]));
        assert_eq!(r.group.as_deref(), Some("XYZ"));
        assert_eq!(r.year, Some(2023));

        let r = parse_release(
            "[喵萌奶茶屋&LoliHouse] 葬送的芙莉莲 - 01 [WebRip 1080p HEVC-10bit AAC][简繁内封字幕]",
        );
        assert_eq!(r.resolution.as_deref(), Some("1080p"));
        assert_eq!(r.source.as_deref(), Some("webrip"));
        assert_eq!(r.video_codec.as_deref(), Some("hevc"));
        assert_eq!(r.audio_codec, v(&["aac"]));
        assert_eq!(r.group, None);
        assert_eq!(r.year, None);

        let r = parse_release("Some.Show.S01.1080p.AMZN.WEB-DL.DDP5.1.H.264-GRP");
        assert_eq!(r.resolution.as_deref(), Some("1080p"));
        assert_eq!(r.source.as_deref(), Some("web_dl"));
        assert_eq!(r.video_codec.as_deref(), Some("avc"));
        assert_eq!(r.audio_codec, v(&["ddp"]));
        assert_eq!(r.group.as_deref(), Some("GRP"));
        assert_eq!(r.year, None);
    }

    #[test]
    fn remux_and_groups() {
        let r = parse_release("Movie.2019.1080p.BluRay.REMUX.AVC.DTS-HD.MA.5.1-FGT");
        assert_eq!(r.source.as_deref(), Some("remux"));
        assert_eq!(r.audio_codec, v(&["dts_hd_ma"]));
        assert_eq!(parse_release("Movie.2020.WEB-DL").group, None);
    }

    #[test]
    fn normalize() {
        assert_eq!(normalize_title("流浪地球2.2023"), "流浪地球 2 2023");
        assert_eq!(
            normalize_title("The.Movie.2023.2160p.WEB-DL.H.265.DDP5.1-ABC"),
            "The Movie 2023 2160p WEB-DL H.265 DDP5.1-ABC"
        );
        assert_eq!(normalize_title("［ABC］_x"), "ABC x");
        assert_eq!(normalize_title("[喵萌]葬送的芙莉莲"), "喵萌 葬送的芙莉莲");
    }
}
