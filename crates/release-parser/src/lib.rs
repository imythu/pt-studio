//! 发布名解析与标题规范化（纯函数，无依赖）：search-meilisearch LLD §1.7 与资源类型共用设计 §3 共用。

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
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub episode_end: Option<i64>,
    /// REPACK、PROPER、INTERNAL、3D、HYBRID
    pub flags: Vec<String>,
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
        if let Some((s, e, e2)) = season_episode(tok) {
            r.season = Some(s);
            r.episode = e;
            r.episode_end = e2.or(r.episode_end);
        } else if r.season.is_some()
            && r.episode.is_some()
            && r.episode_end.is_none()
            && let Some(e) = tok.strip_prefix('e').and_then(|n| n.parse().ok())
            && i > 0
        {
            // S01E02-E03 中的 `e03`
            r.episode_end = Some(e);
        }
        match tok.as_str() {
            "repack" | "proper" | "internal" | "3d" | "hybrid" => {
                push_unique(&mut r.flags, &tok.to_uppercase())
            }
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
    if r.season.is_none()
        && let Some(s) = chinese_number_after(s, "第", "季")
    {
        r.season = Some(s);
    }
    if r.episode.is_none()
        && let Some(e) = chinese_number_after(s, "第", "集")
    {
        r.episode = Some(e);
    }
    r
}

/// `s01e02`、`s01`、`s01e02e03` → (季, 集, 结束集)
fn season_episode(tok: &str) -> Option<(i64, Option<i64>, Option<i64>)> {
    let rest = tok.strip_prefix('s')?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || digits.len() > 3 {
        return None;
    }
    let season = digits.parse().ok()?;
    let after = &rest[digits.len()..];
    if after.is_empty() {
        return Some((season, None, None));
    }
    let mut eps = after.split('e').filter(|x| !x.is_empty());
    if !after.starts_with('e') {
        return None;
    }
    let e1 = eps.next()?.parse().ok()?;
    let e2 = eps.next().and_then(|x| x.parse().ok());
    Some((season, Some(e1), e2))
}

fn cn_digit(c: char) -> Option<i64> {
    "零一二三四五六七八九"
        .chars()
        .position(|x| x == c)
        .map(|p| p as i64)
}

/// `第二季`、`第12集`、`第十一季`
fn chinese_number_after(s: &str, pre: &str, post: &str) -> Option<i64> {
    let start = s.find(pre)? + pre.len();
    let end = start + s[start..].find(post)?;
    let n = &s[start..end];
    if let Ok(v) = n.parse() {
        return Some(v);
    }
    let cs: Vec<char> = n.chars().collect();
    match cs.as_slice() {
        [c] if *c == '十' => Some(10),
        [c] => cn_digit(*c),
        ['十', u] => Some(10 + cn_digit(*u)?),
        [t, '十'] => Some(cn_digit(*t)? * 10),
        [t, '十', u] => Some(cn_digit(*t)? * 10 + cn_digit(*u)?),
        _ => None,
    }
}

/// 发布名中的作品标题：年份 / 季集 / 规格词之前的部分（`.`、`_` 视为空格）
pub fn parse_title(s: &str) -> Option<String> {
    let norm = normalize_title(s);
    let mut words = Vec::new();
    for w in norm.split(' ') {
        let lw = w.to_lowercase();
        let is_year = lw.len() == 4
            && (lw.starts_with("19") || lw.starts_with("20"))
            && lw.chars().all(|c| c.is_ascii_digit());
        let stop = is_year
            || season_episode(&lw).is_some()
            || matches!(
                lw.as_str(),
                "2160p"
                    | "1080p"
                    | "1080i"
                    | "720p"
                    | "576p"
                    | "480p"
                    | "4k"
                    | "uhd"
                    | "bluray"
                    | "web-dl"
                    | "webrip"
                    | "hdtv"
                    | "remux"
            );
        if stop && !words.is_empty() {
            break;
        }
        words.push(w);
    }
    let t = words.join(" ").trim().to_owned();
    (!t.is_empty()).then_some(t)
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
    fn episodes_and_titles() {
        let r = parse_release("The.Show.S01E02-E03.1080p.WEB-DL.REPACK.H.264-GRP");
        assert_eq!((r.season, r.episode, r.episode_end), (Some(1), Some(2), Some(3)));
        assert_eq!(r.flags, v(&["REPACK"]));
        assert_eq!(parse_release("某剧.第二季.第12集.1080p").season, Some(2));
        assert_eq!(parse_release("某剧.第十一季").season, Some(11));
        assert_eq!(parse_release("某剧 第12集").episode, Some(12));
        assert_eq!(
            parse_title("The.Wandering.Earth.II.2023.2160p.WEB-DL").as_deref(),
            Some("The Wandering Earth II")
        );
        assert_eq!(parse_title("Some.Show.S01.1080p").as_deref(), Some("Some Show"));
        assert_eq!(
            parse_title("2001.A.Space.Odyssey.1968.1080p").as_deref(),
            Some("2001 A Space Odyssey")
        );
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
