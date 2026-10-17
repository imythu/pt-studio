//! MediaInfo（文本 / JSON）与 BDInfo 解析、脱敏、规格摘要、与声明字段比较（mediainfo LLD §1）。
//! 摘要的取值与资源类型字段一致（pt-type-kit specs）：分辨率 2160p…、视频 hevc / avc / av1 / vc1 / mpeg2、
//! 音频 truehd / atmos / dts_x / dts_hd_ma / dts / ddp / dd / aac / flac / lpcm / opus、HDR dv / hdr10plus / hdr10 / hlg。

use pt_plugin_sdk::serde_json::{self, Value, json};

pub const PARSER_VERSION: i64 = 1;
pub const MAX_LEN: usize = 256 * 1024;

/// (段类型 general / video / audio / text / menu, 归一化键 → 值)
pub type Section = (String, Vec<(String, String)>);

fn section_kind(header: &str) -> Option<&'static str> {
    let h = header.trim().trim_end_matches(|c: char| c.is_ascii_digit() || c == '#' || c.is_whitespace()).trim();
    Some(match h {
        "General" | "概要" | "概覽" | "常规" => "general",
        "Video" | "视频" | "視訊" | "影像" => "video",
        "Audio" | "音频" | "音訊" => "audio",
        "Text" | "文本" | "文字" | "字幕" => "text",
        "Menu" | "菜单" | "選單" | "章节" => "menu",
        _ => return None,
    })
}

/// 标签 → 归一化键（未列出的转为 snake_case 保留）
fn key_of(label: &str) -> String {
    let l = label.trim();
    let k = match l {
        "Complete name" | "完整名称" | "完整名稱" => "complete_name",
        "Format" | "格式" => "format",
        "Format profile" | "格式配置" | "格式設定" => "format_profile",
        "Commercial name" | "商业名称" => "commercial_name",
        "Format/Info" | "格式/信息" => "format_info",
        "Duration" | "时长" | "時長" => "duration",
        "Width" | "宽度" | "寬度" => "width",
        "Height" | "高度" => "height",
        "Frame rate" | "帧率" | "畫格率" => "frame_rate",
        "Bit depth" | "位深" | "位元深度" => "bit_depth",
        "HDR format" | "HDR 格式" => "hdr_format",
        "Transfer characteristics" | "传输特质" => "transfer_characteristics",
        "Channel(s)" | "声道" | "聲道" => "channels",
        "Language" | "语言" | "語言" => "language",
        "Title" | "标题" | "標題" => "title",
        "Codec ID" | "编解码器 ID" | "編解碼器 ID" => "codec_id",
        "Writing library" | "编码函数库" | "編碼函式庫" => "writing_library",
        "Format version" | "格式版本" => "format_version",
        "File size" | "文件大小" | "檔案大小" => "file_size",
        "Overall bit rate" | "总体码率" => "overall_bit_rate",
        "Bit rate" | "码率" | "位元率" => "bit_rate",
        _ => "",
    };
    if !k.is_empty() {
        return k.to_owned();
    }
    l.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

/// 文本格式：段标题独占一行，字段为 “标签 : 值”
pub fn parse_text(text: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for line in text.lines() {
        let t = line.trim_end();
        if t.trim().is_empty() {
            continue;
        }
        if let Some((label, value)) = t.split_once(" : ").or_else(|| t.split_once(" ：")).or_else(|| t.split_once(": ")) {
            if let Some(cur) = out.last_mut()
                && !label.trim().is_empty()
            {
                cur.1.push((key_of(label), value.trim().to_owned()));
            }
        } else if let Some(k) = section_kind(t) {
            out.push((k.to_owned(), Vec::new()));
        }
    }
    out
}

/// MediaInfo JSON（`--Output=JSON`）
pub fn parse_json(v: &Value) -> Vec<Section> {
    let tracks = v["media"]["track"].as_array().cloned().unwrap_or_default();
    tracks
        .iter()
        .filter_map(|t| {
            let kind = match t["@type"].as_str()? {
                "General" => "general",
                "Video" => "video",
                "Audio" => "audio",
                "Text" => "text",
                "Menu" => "menu",
                _ => return None,
            };
            let fields = t
                .as_object()?
                .iter()
                .filter(|(k, _)| !k.starts_with('@') && k.as_str() != "extra")
                .map(|(k, v)| {
                    let key = match k.as_str() {
                        "CompleteName" => "complete_name".to_owned(),
                        "Format_Profile" => "format_profile".to_owned(),
                        "Format_Commercial_IfAny" => "commercial_name".to_owned(),
                        "FrameRate" => "frame_rate".to_owned(),
                        "BitDepth" => "bit_depth".to_owned(),
                        "HDR_Format" => "hdr_format".to_owned(),
                        "transfer_characteristics" => "transfer_characteristics".to_owned(),
                        "Channels" => "channels".to_owned(),
                        "Format_AdditionalFeatures" => "format_additionalfeatures".to_owned(),
                        "CodecID" => "codec_id".to_owned(),
                        "Encoded_Library" => "writing_library".to_owned(),
                        "Format_Version" => "format_version".to_owned(),
                        other => key_of(&other.replace('_', " ")),
                    };
                    (key, v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()))
                })
                .collect();
            Some((kind.to_owned(), fields))
        })
        .collect()
}

/// BDInfo：完整报告或 Quick Summary，转换为与 MediaInfo 相同的段结构
pub fn parse_bdinfo(text: &str) -> Vec<Section> {
    let mut out: Vec<Section> = vec![("general".into(), vec![("format".into(), "BDAV".into())])];
    let mut mode = "";
    for line in text.lines() {
        let l = line.trim();
        let upper = l.to_ascii_uppercase();
        if upper.starts_with("DISC TITLE:") || upper.starts_with("DISC LABEL:") {
            out[0].1.push(("complete_name".into(), l.split_once(':').map_or("", |x| x.1).trim().to_owned()));
            continue;
        }
        match upper.as_str() {
            "VIDEO:" => mode = "video",
            "AUDIO:" => mode = "audio",
            "SUBTITLES:" => mode = "text",
            _ => {}
        }
        // Quick Summary：“Video: MPEG-H HEVC Video / 51520 kbps / 2160p …”
        let (kind, rest) = if let Some(r) = l.strip_prefix("Video:") {
            ("video", r.trim())
        } else if let Some(r) = l.strip_prefix("Audio:") {
            ("audio", r.trim())
        } else if let Some(r) = l.strip_prefix("Subtitle:") {
            ("text", r.trim())
        } else if !mode.is_empty() && !l.is_empty() && !l.starts_with("---") && !upper.starts_with("CODEC") && !l.ends_with(':') {
            (mode, l)
        } else {
            continue;
        };
        out.push((kind.to_owned(), bd_fields(kind, rest)));
    }
    out
}

fn bd_fields(kind: &str, rest: &str) -> Vec<(String, String)> {
    let parts: Vec<&str> = rest.split(" / ").map(str::trim).collect();
    let mut f = vec![("format".to_owned(), parts.first().copied().unwrap_or("").to_owned())];
    match kind {
        "video" => {
            for p in &parts {
                if (p.ends_with('p') || p.ends_with('i'))
                    && let Some(h) = p.trim_end_matches(['p', 'i']).split_whitespace().last().filter(|h| h.parse::<u32>().is_ok())
                {
                    f.push(("height".into(), h.to_owned()));
                }
                if p.ends_with("fps") {
                    f.push(("frame_rate".into(), p.trim_end_matches("fps").trim().to_owned()));
                }
                if p.ends_with("bits") {
                    f.push(("bit_depth".into(), p.trim_end_matches("bits").trim().to_owned()));
                }
                if p.contains("HDR10+") || p.contains("Dolby Vision") || p.contains("HDR10") || p.contains("HLG") {
                    f.push(("hdr_format".into(), (*p).to_owned()));
                }
            }
            // 完整报告中第一列可能把分辨率写在一起：“MPEG-H HEVC Video  51520 kbps  2160p / 23.976 fps …”
            if let Some(res) = parts.first().and_then(|p0| p0.split_whitespace().find(|w| w.ends_with('p') && w.trim_end_matches('p').parse::<u32>().is_ok())) {
                f.push(("height".into(), res.trim_end_matches('p').to_owned()));
            }
        }
        "audio" => {
            for p in &parts {
                if p.contains('.') && p.chars().next().is_some_and(|c| c.is_ascii_digit()) && p.len() <= 4 {
                    f.push(("channels".into(), (*p).to_owned()));
                }
            }
        }
        _ => {}
    }
    f
}

pub fn detect_and_parse(text: &str) -> Result<(String, Vec<Section>), &'static str> {
    let t = text.trim_start_matches('\u{feff}').trim();
    if t.starts_with('{') {
        let v: Value = serde_json::from_str(t).map_err(|_| "unrecognized")?;
        let s = parse_json(&v);
        return if s.is_empty() { Err("unrecognized") } else { Ok(("json".into(), s)) };
    }
    if t.contains("DISC INFO:") || t.contains("Disc Title:") || t.contains("PLAYLIST REPORT:") || t.lines().any(|l| l.trim().starts_with("Video:") && l.contains(" / ")) {
        return Ok(("bdinfo".into(), parse_bdinfo(t)));
    }
    let s = parse_text(t);
    if s.iter().any(|(k, f)| k == "general" && !f.is_empty()) || s.iter().any(|(k, _)| k == "video") {
        Ok(("text".into(), s))
    } else {
        Err("unrecognized")
    }
}

// ---------------- 脱敏 ----------------

fn basename(v: &str) -> &str {
    v.rsplit(['/', '\\']).next().unwrap_or(v)
}

/// 去掉目录：完整路径只保留文件名，用户目录片段替换为 “…/”
pub fn sanitize_value(v: &str) -> String {
    let t = v.trim();
    let looks_path = t.starts_with('/') || t.starts_with("\\\\") || (t.len() > 2 && t.as_bytes()[1] == b':' && (t.as_bytes()[2] == b'\\' || t.as_bytes()[2] == b'/'));
    if looks_path {
        return basename(t).to_owned();
    }
    let mut s = t.to_owned();
    for marker in ["/home/", "/Users/", "\\Users\\", "/root/"] {
        while let Some(i) = s.find(marker) {
            let after = &s[i + marker.len()..];
            let end = after.find(['/', '\\']).map_or(after.len(), |e| e + 1);
            s = format!("{}…/{}", &s[..i], &after[end..]);
        }
    }
    s
}

pub fn sanitize_sections(secs: &mut [Section]) {
    for (_, fields) in secs.iter_mut() {
        for (k, v) in fields.iter_mut() {
            if k == "complete_name" || v.contains('/') || v.contains('\\') {
                *v = sanitize_value(v);
            }
        }
    }
}

/// 原文逐行脱敏（保持格式）
pub fn sanitize_text(text: &str) -> String {
    text.lines()
        .map(|line| match line.split_once(" : ").or_else(|| line.split_once(" ：")) {
            Some((label, value)) if key_of(label) == "complete_name" || value.trim().starts_with('/') || value.contains(":\\") || value.trim().starts_with("\\\\") => {
                let sep = if line.contains(" : ") { " : " } else { " ：" };
                format!("{label}{sep}{}", sanitize_value(value))
            }
            _ if line.contains("/home/") || line.contains("/Users/") || line.contains("\\Users\\") => sanitize_value(line),
            _ => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------------- 摘要 ----------------

fn get<'a>(fields: &'a [(String, String)], k: &str) -> Option<&'a str> {
    fields.iter().find(|(x, _)| x == k).map(|(_, v)| v.as_str())
}

fn first_number(s: &str) -> Option<u32> {
    let digits: String = s.chars().filter(|c| !c.is_whitespace()).take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

pub fn resolution(w: Option<u32>, h: Option<u32>, scan_interlaced: bool) -> Option<String> {
    let (w, h) = (w.unwrap_or(0), h.unwrap_or(0));
    if w == 0 && h == 0 {
        return None;
    }
    let r = if w >= 7600 || h >= 4300 {
        "4320p"
    } else if w >= 3800 || h >= 2000 {
        "2160p"
    } else if w >= 1900 || h >= 1000 {
        if scan_interlaced { "1080i" } else { "1080p" }
    } else if w >= 1260 || h >= 700 {
        "720p"
    } else if h >= 560 {
        "576p"
    } else {
        "480p"
    };
    Some(r.to_owned())
}

fn video_codec(f: &[(String, String)]) -> Option<&'static str> {
    let fmt = get(f, "format")?.to_ascii_uppercase();
    Some(if fmt.contains("HEVC") {
        "hevc"
    } else if fmt.contains("AVC") {
        "avc"
    } else if fmt.contains("AV1") {
        "av1"
    } else if fmt.contains("VC-1") || fmt.contains("VC1") {
        "vc1"
    } else if fmt.contains("MPEG") && (get(f, "format_version").is_some_and(|v| v.contains('2')) || fmt.contains("MPEG-2")) {
        "mpeg2"
    } else {
        return None;
    })
}

fn audio_codecs(f: &[(String, String)]) -> Vec<&'static str> {
    let fmt = get(f, "format").unwrap_or("").to_ascii_uppercase();
    let all = format!(
        "{fmt} {} {} {}",
        get(f, "commercial_name").unwrap_or("").to_ascii_uppercase(),
        get(f, "format_additionalfeatures").unwrap_or("").to_ascii_uppercase(),
        get(f, "format_profile").unwrap_or("").to_ascii_uppercase()
    );
    let atmos = all.contains("ATMOS") || all.contains("JOC");
    let mut out = Vec::new();
    if fmt.contains("MLP FBA") || all.contains("TRUEHD") {
        out.push("truehd");
    } else if all.contains("DTS:X") || all.contains("XLL X") || all.contains("DTS-X") {
        out.push("dts_x");
    } else if (fmt.contains("DTS") && (all.contains("XLL") || all.contains("MA") || all.contains("MASTER AUDIO"))) || all.contains("DTS-HD MASTER") {
        out.push("dts_hd_ma");
    } else if fmt.contains("DTS") {
        out.push("dts");
    } else if fmt.contains("E-AC-3") || all.contains("DOLBY DIGITAL PLUS") {
        out.push("ddp");
    } else if fmt.contains("AC-3") || all.contains("DOLBY DIGITAL") {
        out.push("dd");
    } else if fmt.contains("AAC") {
        out.push("aac");
    } else if fmt.contains("FLAC") {
        out.push("flac");
    } else if fmt.contains("PCM") {
        out.push("lpcm");
    } else if fmt.contains("OPUS") {
        out.push("opus");
    }
    if atmos {
        out.push("atmos");
    }
    out
}

fn hdr(f: &[(String, String)]) -> Vec<&'static str> {
    let h = get(f, "hdr_format").unwrap_or("").to_ascii_uppercase();
    let mut out = Vec::new();
    if h.contains("DOLBY VISION") {
        out.push("dv");
    }
    if h.contains("2094") || h.contains("HDR10+") {
        out.push("hdr10plus");
    }
    if h.contains("2086") || (h.contains("HDR10") && !h.contains("HDR10+")) || (h.contains("HDR10") && h.contains("HDR10+") && h.contains("COMPATIBLE")) {
        out.push("hdr10");
    }
    if get(f, "transfer_characteristics").is_some_and(|t| t.contains("HLG")) || h.contains("HLG") {
        out.push("hlg");
    }
    out.dedup();
    out
}

fn duration_secs(s: &str) -> Option<u64> {
    let t = s.trim();
    if t.contains(':') {
        let parts: Vec<f64> = t.split(':').filter_map(|p| p.trim().parse().ok()).collect();
        if parts.len() == 3 {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            return Some((parts[0] * 3600.0 + parts[1] * 60.0 + parts[2]) as u64);
        }
    }
    if let Ok(ms) = t.parse::<f64>() {
        // JSON 输出为秒（带小数）
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        return Some(ms as u64);
    }
    let mut total = 0u64;
    let mut num = String::new();
    let mut found = false;
    let chars: Vec<char> = t.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_ascii_digit() {
            num.push(c);
        } else if !num.is_empty() && !c.is_whitespace() {
            let n: u64 = num.parse().unwrap_or(0);
            let rest: String = chars[i..].iter().collect();
            let unit = if rest.starts_with("min") || rest.starts_with("分") {
                60
            } else if rest.starts_with("ms") || rest.starts_with("毫秒") {
                0
            } else if rest.starts_with('h') || rest.starts_with("小时") || rest.starts_with("時") {
                3600
            } else if rest.starts_with('s') || rest.starts_with('秒') {
                1
            } else {
                0
            };
            total += n * unit;
            found = true;
            num.clear();
        }
        i += 1;
    }
    found.then_some(total)
}

fn lang(s: Option<&str>) -> Option<String> {
    let s = s?.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

pub fn summary(secs: &[Section]) -> Value {
    let general = secs.iter().find(|(k, _)| k == "general").map(|(_, f)| f.as_slice()).unwrap_or(&[]);
    let video = secs.iter().find(|(k, _)| k == "video").map(|(_, f)| f.as_slice()).unwrap_or(&[]);
    let w = get(video, "width").and_then(first_number);
    let h = get(video, "height").and_then(first_number);
    let interlaced = get(video, "scan_type").is_some_and(|s| s.to_ascii_lowercase().contains("interlaced"));
    let container = get(general, "format").map(|f| match f.to_ascii_uppercase().as_str() {
        x if x.contains("MATROSKA") => "mkv".to_owned(),
        x if x.contains("MPEG-4") => "mp4".to_owned(),
        x if x.contains("BDAV") => "m2ts".to_owned(),
        x if x.contains("AVI") => "avi".to_owned(),
        x => x.to_ascii_lowercase(),
    });
    let audio: Vec<Value> = secs
        .iter()
        .filter(|(k, _)| k == "audio")
        .map(|(_, f)| {
            json!({ "codecs": audio_codecs(f), "format": get(f, "format"), "channels": get(f, "channels"), "language": lang(get(f, "language")), "title": get(f, "title") })
        })
        .collect();
    let mut codecs: Vec<String> = Vec::new();
    for c in audio.iter().flat_map(|a| a["codecs"].as_array().cloned().unwrap_or_default()) {
        if let Some(c) = c.as_str()
            && !codecs.iter().any(|x| x == c)
        {
            codecs.push(c.to_owned());
        }
    }
    let subs: Vec<String> = secs.iter().filter(|(k, _)| k == "text").filter_map(|(_, f)| lang(get(f, "language")).or_else(|| lang(get(f, "format")))).collect();
    json!({
        "container": container,
        "resolution": resolution(w, h, interlaced),
        "width": w, "height": h,
        "video_codec": video_codec(video),
        "hdr": hdr(video),
        "bit_depth": get(video, "bit_depth").and_then(first_number),
        "frame_rate": get(video, "frame_rate").map(|f| f.split_whitespace().next().unwrap_or(f).to_owned()),
        "duration_secs": get(general, "duration").or_else(|| get(video, "duration")).and_then(duration_secs),
        "audio": audio,
        "audio_codecs": codecs,
        "subtitle_languages": subs,
    })
}

pub fn sections_json(secs: &[Section]) -> Value {
    Value::Array(
        secs.iter()
            .map(|(k, f)| json!({ "kind": k, "fields": f.iter().map(|(a, b)| json!([a, b])).collect::<Vec<_>>() }))
            .collect(),
    )
}

/// 声明值与检测值不一致的字段（检测不到的不计）
pub fn compare(summary: &Value, fields: &Value) -> Vec<Value> {
    let mut diffs = Vec::new();
    let s = |v: &Value| v.as_str().map(str::to_owned);
    for k in ["resolution", "video_codec"] {
        if let (Some(d), Some(x)) = (s(&fields[k]), s(&summary[k]))
            && d != x
        {
            diffs.push(json!({ "field": k, "declared": d, "detected": x }));
        }
    }
    let set = |v: &Value| -> Vec<String> { v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default() };
    for (k, sk) in [("hdr", "hdr"), ("audio_codecs", "audio_codecs")] {
        let declared = set(&fields[k]);
        let detected = set(&summary[sk]);
        if detected.is_empty() {
            continue;
        }
        let missing: Vec<&String> = declared.iter().filter(|d| !detected.contains(d)).collect();
        if !missing.is_empty() {
            diffs.push(json!({ "field": k, "declared": declared.join(","), "detected": detected.join(",") }));
        }
    }
    diffs
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "General
Unique ID                                : 1234
Complete name                            : D:\\Users\\alice\\Movies\\The.Movie.2019.2160p.UHD.BluRay.x265.mkv
Format                                   : Matroska
Duration                                 : 2 h 16 min
Overall bit rate                         : 58.3 Mb/s

Video
ID                                       : 1
Format                                   : HEVC
Format profile                           : Main 10@L5.1@High
HDR format                               : Dolby Vision, Version 1.0, dvhe.07.06, BL+EL+RPU, HDR10 compatible / SMPTE ST 2086, HDR10 compatible
Width                                    : 3 840 pixels
Height                                   : 2 160 pixels
Frame rate                               : 23.976 (24000/1001) FPS
Bit depth                                : 10 bits
Writing library                          : x265 3.5:[Linux][GCC 9.3.0][64 bit] 10bit, /home/bob/encode/settings

Audio #1
ID                                       : 2
Format                                   : MLP FBA 16-ch
Commercial name                          : Dolby TrueHD with Dolby Atmos
Channel(s)                               : 8 channels
Language                                 : English

Audio #2
Format                                   : E-AC-3
Channel(s)                               : 6 channels
Language                                 : Chinese

Text #1
Format                                   : PGS
Language                                 : Chinese
";

    #[test]
    fn parses_text_and_summarises() {
        let (fmt, mut secs) = detect_and_parse(SAMPLE).unwrap_or_default();
        assert_eq!(fmt, "text");
        sanitize_sections(&mut secs);
        let s = summary(&secs);
        assert_eq!(s["container"], "mkv");
        assert_eq!(s["resolution"], "2160p");
        assert_eq!((s["width"].clone(), s["height"].clone()), (json!(3840), json!(2160)));
        assert_eq!(s["video_codec"], "hevc");
        assert_eq!(s["hdr"], json!(["dv", "hdr10"]));
        assert_eq!(s["bit_depth"], 10);
        assert_eq!(s["duration_secs"], 2 * 3600 + 16 * 60);
        assert_eq!(s["audio_codecs"], json!(["truehd", "atmos", "ddp"]));
        assert_eq!(s["subtitle_languages"], json!(["Chinese"]));
        let general = &secs[0].1;
        assert_eq!(get(general, "complete_name"), Some("The.Movie.2019.2160p.UHD.BluRay.x265.mkv"));
        let clean = sanitize_text(SAMPLE);
        assert!(!clean.contains("alice") && !clean.contains("/home/bob"), "{clean}");
        assert!(clean.contains("The.Movie.2019.2160p.UHD.BluRay.x265.mkv"));
    }

    #[test]
    fn chinese_labels_and_json() {
        let zh = "概要\n完整名称 : /data/电影/示例.mkv\n格式 : Matroska\n\n视频\n格式 : AVC\n宽度 : 1 920 像素\n高度 : 1 080 像素\n\n音频\n格式 : AAC\n声道 : 2 声道\n";
        let (_, secs) = detect_and_parse(zh).unwrap_or_default();
        let s = summary(&secs);
        assert_eq!((s["resolution"].clone(), s["video_codec"].clone(), s["audio_codecs"].clone()), (json!("1080p"), json!("avc"), json!(["aac"])));
        let js = json!({ "media": { "track": [
            { "@type": "General", "Format": "MPEG-4", "Duration": "5400.123" },
            { "@type": "Video", "Format": "AV1", "Width": "1280", "Height": "720" },
            { "@type": "Audio", "Format": "E-AC-3", "Format_AdditionalFeatures": "JOC" } ] } });
        let (fmt, secs) = detect_and_parse(&js.to_string()).unwrap_or_default();
        let s = summary(&secs);
        assert_eq!(fmt, "json");
        assert_eq!((s["container"].clone(), s["resolution"].clone(), s["video_codec"].clone()), (json!("mp4"), json!("720p"), json!("av1")));
        assert_eq!((s["audio_codecs"].clone(), s["duration_secs"].clone()), (json!(["ddp", "atmos"]), json!(5400)));
        assert!(detect_and_parse("hello world").is_err());
    }

    #[test]
    fn bdinfo_quick_summary() {
        let bd = "Disc Title: SAMPLE_DISC\nDisc Size: 61,234,567,890 bytes\nVideo: MPEG-H HEVC Video / 51520 kbps / 2160p / 23.976 fps / 16:9 / Main 10 @ Level 5.1 @ High / 10 bits / HDR10 / BT.2020\nAudio: Dolby TrueHD/Atmos Audio / 4285 kbps / 7.1 / 48 kHz / 24-bit\nAudio: Dolby Digital Audio / 640 kbps / 5.1 / 48 kHz\nSubtitle: Chinese / 30.123 kbps\n";
        let (fmt, secs) = detect_and_parse(bd).unwrap_or_default();
        let s = summary(&secs);
        assert_eq!(fmt, "bdinfo");
        assert_eq!((s["container"].clone(), s["resolution"].clone(), s["video_codec"].clone()), (json!("m2ts"), json!("2160p"), json!("hevc")));
        assert_eq!(s["hdr"], json!(["hdr10"]));
        assert_eq!(s["audio_codecs"], json!(["truehd", "atmos", "dd"]));
    }

    #[test]
    fn compares_declared_fields() {
        let s = json!({ "resolution": "1080p", "video_codec": "avc", "hdr": [], "audio_codecs": ["dd"] });
        let d = compare(&s, &json!({ "resolution": "2160p", "video_codec": "avc", "hdr": ["hdr10"], "audio_codecs": ["truehd"] }));
        let fields: Vec<&str> = d.iter().map(|x| x["field"].as_str().unwrap_or("")).collect();
        assert_eq!(fields, vec!["resolution", "audio_codecs"], "empty detected HDR is not a diff: {d:?}");
    }

    #[test]
    fn durations() {
        assert_eq!(duration_secs("1 h 2 min"), Some(3720));
        assert_eq!(duration_secs("2小时16分钟"), Some(8160));
        assert_eq!(duration_secs("02:16:03.123"), Some(8163));
        assert_eq!(duration_secs("45 min 30 s"), Some(2730));
    }
}
