//! 动漫发布名解析（type-anime LLD §1.1）：字幕组风格 `[组] 标题 - 05 [1080p][CHS]`、`[组][标题][05][1080P][简繁内封]`、
//! `【组】★10月新番★[标题][01-12][合集]`，以及场景风格（交给 pt-release-parser）。纯函数。

#[derive(Debug, Default, Clone, PartialEq)]
pub struct AnimeName {
    pub fansub: Option<String>,
    /// 拉丁字母标题 / 中文标题（`中文名 / Romaji` 时拆开）
    pub title: Option<String>,
    pub title_zh: Option<String>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    pub episode_end: Option<i64>,
    /// 合集 / 全集 / BDBOX / Batch / Fin
    pub batch: bool,
    pub resolution: Option<String>,
    pub source: Option<String>,
    pub video_codec: Option<String>,
    pub bit_depth: Option<i64>,
    pub audio_codecs: Vec<String>,
    pub subtitle_langs: Vec<String>,
    pub subtitle_kind: Option<String>,
}

/// 字幕标签 → (语言, 方式)。方式：`softsub`（内封）、`hardsub`（内嵌）、`external`（外挂）
pub fn subtitle_tag(tag: &str) -> Option<(Vec<&'static str>, Option<&'static str>)> {
    let t = tag.to_lowercase();
    let mut langs: Vec<&'static str> = Vec::new();
    let push = |l: &'static str, langs: &mut Vec<&'static str>| {
        if !langs.contains(&l) {
            langs.push(l);
        }
    };
    let both = ["简繁", "簡繁", "繁简", "chs&cht", "chs_cht", "gb&big5", "sc&tc"];
    if both.iter().any(|b| t.contains(b)) {
        push("chs", &mut langs);
        push("cht", &mut langs);
    }
    for (keys, l) in [
        (
            &["简体", "簡體", "简中", "简日", "chs", "gb", "sc", "jpsc", "chsjp"][..],
            "chs",
        ),
        (
            &[
                "繁体", "繁體", "繁中", "繁日", "cht", "big5", "tc", "jptc", "chtjp",
            ][..],
            "cht",
        ),
        (
            &[
                "日文", "日语", "简日", "繁日", "jpn", "jp", "jap", "jpsc", "jptc", "chsjp", "chtjp",
            ][..],
            "jpn",
        ),
        (&["英文", "eng", "en"][..], "eng"),
    ] {
        // 拉丁缩写要整词匹配（避免 "sc" 命中 "scene"）
        let hit = keys.iter().any(|k| {
            if k.is_ascii() {
                t.split(|c: char| !c.is_ascii_alphanumeric()).any(|w| w == *k)
            } else {
                t.contains(k)
            }
        });
        if hit {
            push(l, &mut langs);
        }
    }
    let kind = if t.contains("内封") || t.contains("內封") || t.contains("softsub") {
        Some("softsub")
    } else if t.contains("内嵌") || t.contains("內嵌") || t.contains("hardsub") {
        Some("hardsub")
    } else if t.contains("外挂") || t.contains("外掛") {
        Some("external")
    } else {
        None
    };
    // 单独的“字幕”“中字”只给出中文
    if langs.is_empty() && (t.contains("中字") || t.contains("中文字幕")) {
        langs.push("chs");
    }
    (!langs.is_empty() || kind.is_some()).then_some((langs, kind))
}

fn cn_num(s: &str) -> Option<i64> {
    let d = |c: char| {
        "零一二三四五六七八九"
            .chars()
            .position(|x| x == c)
            .map(|p| p as i64)
    };
    let cs: Vec<char> = s.chars().collect();
    match cs.as_slice() {
        [c] if *c == '十' => Some(10),
        [c] => d(*c),
        ['十', b] => d(*b).map(|b| 10 + b),
        [a, '十'] => d(*a).map(|a| a * 10),
        [a, '十', b] => Some(d(*a)? * 10 + d(*b)?),
        _ => s.parse().ok(),
    }
}

/// 标题中的季：`S2`、`Season 2`、`2nd Season`、`第二季`、`第2期`；返回 (去掉季的标题, 季)
fn strip_season(t: &str) -> (String, Option<i64>) {
    let words: Vec<&str> = t.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        let l = w.to_lowercase();
        let rest = |n: usize| {
            let mut v = words.clone();
            v.drain(i..(i + n).min(words.len()));
            v.join(" ")
        };
        if let Some(n) = l
            .strip_prefix('s')
            .and_then(|d| d.parse::<i64>().ok())
            .filter(|n| (1..100).contains(n))
        {
            return (rest(1), Some(n));
        }
        if l == "season"
            && let Some(n) = words.get(i + 1).and_then(|d| d.parse::<i64>().ok())
        {
            return (rest(2), Some(n));
        }
        if words.get(i + 1).is_some_and(|x| x.eq_ignore_ascii_case("season"))
            && let Some(n) = l
                .trim_end_matches(|c: char| c.is_ascii_alphabetic())
                .parse::<i64>()
                .ok()
                .filter(|_| l.ends_with(['d', 'h', 't']))
        {
            return (rest(2), Some(n));
        }
    }
    if let Some(p) = t.find('第') {
        let after = &t[p + '第'.len_utf8()..];
        let num: String = after
            .chars()
            .take_while(|c| c.is_ascii_digit() || "零一二三四五六七八九十".contains(*c))
            .collect();
        let tail = &after[num.len()..];
        if !num.is_empty()
            && let Some(unit) = tail.chars().next().filter(|c| matches!(c, '季' | '期'))
            && let Some(n) = cn_num(&num)
        {
            let end = p + '第'.len_utf8() + num.len() + unit.len_utf8();
            let s = format!("{}{}", &t[..p], &t[end..]);
            return (s.split_whitespace().collect::<Vec<_>>().join(" "), Some(n));
        }
    }
    (t.to_owned(), None)
}

fn is_cjk(s: &str) -> bool {
    s.chars()
        .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3040}'..='\u{30ff}').contains(&c))
}

/// 集号括号：`05`、`05v2`、`01-12`、`01~12`、`01-12 Fin`、`EP05`
fn episode_token(t: &str) -> Option<(i64, Option<i64>)> {
    let t = t.trim();
    let l = t.to_lowercase();
    let l = l
        .trim_start_matches("ep")
        .trim_start_matches('e')
        .trim_end_matches(" fin")
        .trim_end_matches("end");
    let l = l.trim();
    let num = |s: &str| -> Option<i64> {
        let s = s.trim();
        let s = s.split_once('v').map_or(s, |(n, v)| {
            if v.chars().all(|c| c.is_ascii_digit()) {
                n
            } else {
                s
            }
        });
        (!s.is_empty() && s.len() <= 4 && s.chars().all(|c| c.is_ascii_digit()))
            .then(|| s.parse().ok())
            .flatten()
    };
    if let Some((a, b)) = l.split_once(['-', '~']) {
        let (a, b) = (num(a)?, num(b)?);
        return (b > a).then_some((a, Some(b)));
    }
    num(l).filter(|n| *n < 1900).map(|n| (n, None))
}

/// 技术标签（分辨率、来源、编码、位深、音频）；识别到任何一项返回 true
fn tech_tag(tok: &str, n: &mut AnimeName) -> bool {
    let l = tok.to_lowercase();
    let mut hit = false;
    let res = match l.as_str() {
        "2160p" | "4k" | "3840x2160" => Some("2160p"),
        "1080p" | "1920x1080" | "1080" => Some("1080p"),
        "720p" | "1280x720" => Some("720p"),
        "480p" | "848x480" | "640x480" => Some("480p"),
        _ => None,
    };
    if let Some(r) = res {
        n.resolution.get_or_insert(r.into());
        hit = true;
    }
    let src = match l.as_str() {
        "web-dl" | "webdl" | "web" | "baha" | "b-global" | "bilibili" | "cr" | "crunchyroll" | "abema"
        | "netflix" | "nf" | "amzn" => Some("web_dl"),
        "webrip" => Some("webrip"),
        "bdrip" | "bd" | "bluray" | "blu-ray" | "bdremux" => Some("bluray"),
        "dvdrip" | "dvd" => Some("dvd"),
        "tvrip" | "hdtv" | "tv" => Some("hdtv"),
        _ => None,
    };
    if let Some(s) = src {
        n.source.get_or_insert(s.into());
        hit = true;
    }
    let codec = match l.as_str() {
        x if x.starts_with("hevc") || x.starts_with("x265") || x.starts_with("h265") || x == "h.265" => {
            Some("hevc")
        }
        x if x.starts_with("avc") || x.starts_with("x264") || x.starts_with("h264") || x == "h.264" => {
            Some("avc")
        }
        "av1" => Some("av1"),
        _ => None,
    };
    if let Some(c) = codec {
        n.video_codec.get_or_insert(c.into());
        hit = true;
    }
    if l.contains("10bit") || l.contains("10-bit") || l == "ma10p" || l == "hi10p" || l.contains("yuv420p10")
    {
        n.bit_depth = Some(10);
        hit = true;
    } else if l.contains("8bit") || l.contains("8-bit") {
        n.bit_depth.get_or_insert(8);
        hit = true;
    }
    for (keys, a) in [
        (&["aac"][..], "aac"),
        (&["flac"][..], "flac"),
        (&["opus"][..], "opus"),
        (&["ac3", "dd"][..], "dd"),
        (&["eac3", "ddp"][..], "ddp"),
    ] {
        if keys.iter().any(|k| {
            l == *k
                || l.starts_with(&format!("{k}x"))
                || l.starts_with(&format!("{k}2"))
                || l.starts_with(&format!("{k}5"))
        }) {
            if !n.audio_codecs.iter().any(|x| x == a) {
                n.audio_codecs.push(a.into());
            }
            hit = true;
        }
    }
    if matches!(l.as_str(), "mkv" | "mp4" | "chs" | "cht") {
        hit = true;
    }
    hit
}

const BATCH: &[&str] = &[
    "合集", "全集", "bdbox", "batch", "complete", "fin", "全话", "全話",
];

/// 一个括号 / 自由文本段是否全是标签（识别到的写入 `n`）
fn tags(seg: &str, n: &mut AnimeName) -> bool {
    let l = seg.to_lowercase();
    let mut any = false;
    if BATCH.iter().any(|b| l.contains(b)) {
        n.batch = true;
        any = true;
    }
    if let Some((langs, kind)) = subtitle_tag(seg) {
        for x in langs {
            if !n.subtitle_langs.iter().any(|y| y == x) {
                n.subtitle_langs.push(x.into());
            }
        }
        if let Some(k) = kind {
            n.subtitle_kind.get_or_insert(k.into());
        }
        any = true;
    }
    let toks: Vec<&str> = seg
        .split([' ', '_', '+', '&', ',', '/'])
        .filter(|t| !t.is_empty())
        .collect();
    let mut all = !toks.is_empty();
    for t in &toks {
        // AVC-8bit、HEVC-10bit
        let parts: Vec<&str> = t.split('-').collect();
        let hit =
            tech_tag(t, n) | (parts.len() > 1 && parts.iter().fold(false, |acc, p| tech_tag(p, n) | acc));
        any |= hit;
        all &= hit;
    }
    any && (all
        || seg.chars().count() <= 24
        || !seg.chars().any(char::is_alphabetic)
        || is_cjk(seg) && !seg.contains(' '))
}

fn set_titles(raw: &str, n: &mut AnimeName) {
    let (t, season) = strip_season(raw.trim());
    if season.is_some() {
        n.season = season;
    }
    for part in t.split(" / ") {
        let p = part.trim().trim_matches(['-', '_']).trim();
        if p.is_empty() {
            continue;
        }
        let (p, s2) = strip_season(p);
        if s2.is_some() {
            n.season = s2;
        }
        let slot = if is_cjk(&p) { &mut n.title_zh } else { &mut n.title };
        if slot.is_none() {
            *slot = Some(p);
        }
    }
}

pub fn parse(name: &str) -> AnimeName {
    let mut n = AnimeName::default();
    let stem = match name.rsplit_once('.') {
        Some((s, e))
            if e.len() <= 4
                && e.chars().all(|c| c.is_ascii_alphanumeric())
                && !e.chars().all(|c| c.is_ascii_digit()) =>
        {
            s
        }
        _ => name,
    };
    // 切成括号段与自由文本段
    let mut segs: Vec<(bool, String)> = Vec::new();
    let mut cur = String::new();
    let mut depth = 0;
    for c in stem.chars() {
        match c {
            '[' | '【' if depth == 0 => {
                if !cur.trim().is_empty() {
                    segs.push((false, cur.trim().to_owned()));
                }
                cur.clear();
                depth = 1;
            }
            ']' | '】' if depth == 1 => {
                segs.push((true, cur.trim().to_owned()));
                cur.clear();
                depth = 0;
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        segs.push((false, cur.trim().to_owned()));
    }
    // 场景风格（无括号）：交给通用解析器
    if !segs.iter().any(|s| s.0) {
        let p = pt_release_parser::parse_release(name);
        n.fansub = p.group.clone();
        n.title = pt_release_parser::parse_title(name);
        n.season = p.season;
        n.episode = p.episode;
        n.episode_end = p.episode_end;
        n.resolution = p.resolution;
        n.source = p.source.map(|s| if s == "remux" { "bluray".into() } else { s });
        n.video_codec = p.video_codec;
        n.audio_codecs = p.audio_codec;
        return n;
    }
    let mut title_candidates: Vec<String> = Vec::new();
    for (i, (bracket, s)) in segs.iter().enumerate() {
        if s.is_empty() || s.contains('★') {
            continue;
        }
        if *bracket {
            if i == 0 {
                n.fansub = Some(s.clone());
                continue;
            }
            // 裸 `[1080]` 是分辨率不是集号
            if n.episode.is_none()
                && !matches!(s.as_str(), "720" | "1080" | "2160")
                && let Some((e, e2)) = episode_token(s)
            {
                n.episode = Some(e);
                n.episode_end = e2;
                continue;
            }
            if !tags(s, &mut n) {
                title_candidates.push(s.clone());
            }
            continue;
        }
        // 自由文本：`标题 - 05 (…)`、`标题 S2 - 05`
        let mut text = s.as_str();
        if let Some(p) = text.rfind(" - ") {
            let rest = text[p + 3..].trim();
            let first = rest.split_whitespace().next().unwrap_or("");
            if let Some((e, e2)) = episode_token(first) {
                n.episode = Some(e);
                n.episode_end = e2;
                text = &text[..p];
                tags(rest[first.len()..].trim().trim_matches(['(', ')']), &mut n);
            }
        }
        // 自由文本末尾的括号标签：`(1080p)` / `(Baha 1920x1080 AVC AAC MP4)`
        let text = match text.find('(') {
            Some(p) => {
                tags(text[p..].trim_matches(['(', ')', ' ']), &mut n);
                &text[..p]
            }
            None => text,
        };
        if !text.trim().is_empty() && !tags(text, &mut n.clone()) {
            title_candidates.push(text.trim().to_owned());
        } else {
            tags(text, &mut n);
        }
    }
    for t in &title_candidates {
        set_titles(t, &mut n);
        if n.title.is_some() && n.title_zh.is_some() {
            break;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dash_style() {
        let n = parse("[ANi] 葬送的芙莉莲 / Sousou no Frieren - 05 [1080P][Baha][WEB-DL][AAC AVC][CHT].mp4");
        assert_eq!(n.fansub.as_deref(), Some("ANi"));
        assert_eq!(
            (n.title.as_deref(), n.title_zh.as_deref()),
            (Some("Sousou no Frieren"), Some("葬送的芙莉莲"))
        );
        assert_eq!(
            (n.episode, n.resolution.as_deref(), n.source.as_deref()),
            (Some(5), Some("1080p"), Some("web_dl"))
        );
        assert_eq!(
            (n.video_codec.as_deref(), n.subtitle_langs.clone()),
            (Some("avc"), vec!["cht".to_owned()])
        );
        assert_eq!(n.audio_codecs, vec!["aac"]);
        let n =
            parse("[Lilith-Raws] Kusuriya no Hitorigoto - 13 [Baha][WEB-DL][1080p][AVC AAC][CHT][MP4].mp4");
        assert_eq!(
            (n.fansub.as_deref(), n.title.as_deref(), n.episode),
            (Some("Lilith-Raws"), Some("Kusuriya no Hitorigoto"), Some(13))
        );
        let n = parse("[SweetSub] Oshi no Ko S2 - 03 (1080p)");
        assert_eq!(
            (n.title.as_deref(), n.season, n.episode, n.resolution.as_deref()),
            (Some("Oshi no Ko"), Some(2), Some(3), Some("1080p"))
        );
    }

    #[test]
    fn bracket_style() {
        let n = parse("[Sakurato] Spy x Family Season 2 [07][AVC-8bit 1080p AAC][CHS].mp4");
        assert_eq!(
            (n.title.as_deref(), n.season, n.episode),
            (Some("Spy x Family"), Some(2), Some(7))
        );
        assert_eq!(
            (n.bit_depth, n.subtitle_langs.clone()),
            (Some(8), vec!["chs".to_owned()])
        );
        let n = parse("【喵萌奶茶屋】★10月新番★[药屋少女的呢喃 第二季][13v2][1080p][简繁内封][招募翻译]");
        assert_eq!(
            (n.fansub.as_deref(), n.title_zh.as_deref(), n.season, n.episode),
            (Some("喵萌奶茶屋"), Some("药屋少女的呢喃"), Some(2), Some(13))
        );
        assert_eq!(
            (n.subtitle_langs.clone(), n.subtitle_kind.as_deref()),
            (vec!["chs".to_owned(), "cht".to_owned()], Some("softsub"))
        );
    }

    #[test]
    fn batch_and_scene() {
        let n = parse("[VCB-Studio] Bocchi the Rock! [Ma10p_1080p][x265_flac] [01-12 Fin]");
        assert_eq!(
            (n.title.as_deref(), n.episode, n.episode_end),
            (Some("Bocchi the Rock!"), Some(1), Some(12))
        );
        assert_eq!(
            (n.bit_depth, n.video_codec.as_deref(), n.audio_codecs.clone()),
            (Some(10), Some("hevc"), vec!["flac".to_owned()])
        );
        let n = parse("[Nekomoe kissaten][Kimetsu no Yaiba][BDRip][合集][1080p][JPSC]");
        assert!(n.batch);
        assert_eq!(
            (n.title.as_deref(), n.source.as_deref(), n.subtitle_langs.clone()),
            (
                Some("Kimetsu no Yaiba"),
                Some("bluray"),
                vec!["chs".to_owned(), "jpn".to_owned()]
            )
        );
        let n = parse("Frieren.S01E05.1080p.WEB-DL.AAC2.0.H.264-VARYG");
        assert_eq!(
            (n.title.as_deref(), n.season, n.episode, n.fansub.as_deref()),
            (Some("Frieren"), Some(1), Some(5), Some("VARYG"))
        );
    }

    #[test]
    fn subtitle_tags() {
        assert_eq!(subtitle_tag("简日双语"), Some((vec!["chs", "jpn"], None)));
        assert_eq!(subtitle_tag("CHS_CHT"), Some((vec!["chs", "cht"], None)));
        assert_eq!(subtitle_tag("繁體內嵌"), Some((vec!["cht"], Some("hardsub"))));
        assert_eq!(subtitle_tag("1080p"), None);
        assert_eq!(cn_num("十二"), Some(12));
        assert_eq!(
            strip_season("Oshi no Ko 2nd Season"),
            ("Oshi no Ko".into(), Some(2))
        );
    }
}
