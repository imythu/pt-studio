//! 影视类资源共用的规格字段：规范值、同义词映射、从发布名补全、标题中的显示写法（type-movie / type-tv / type-anime）。

use serde_json::{Map, Value, json};

use crate::Errors;

pub const RESOLUTIONS: &[&str] = &["4320p", "2160p", "1080p", "1080i", "720p", "576p", "480p"];
pub const SOURCES: &[&str] = &["uhd_bluray", "bluray", "web_dl", "webrip", "hdtv", "dvd"];
pub const RELEASE_TYPES: &[&str] = &["disc", "remux", "encode", "web", "hdtv"];
pub const VIDEO: &[&str] = &["hevc", "avc", "av1", "vc1", "mpeg2"];
pub const AUDIO: &[&str] = &[
    "truehd",
    "atmos",
    "dts_x",
    "dts_hd_ma",
    "dts",
    "ddp",
    "dd",
    "aac",
    "flac",
    "lpcm",
    "opus",
];
pub const HDR: &[&str] = &["dv", "hdr10plus", "hdr10", "hlg"];

/// 用户输入的同义写法 → 规范值
pub fn canon(field: &str, v: &str) -> Option<String> {
    let l = v.trim().to_lowercase().replace([' ', '.', '-'], "");
    let out = match field {
        "resolution" => match l.as_str() {
            "4k" | "uhd" => "2160p",
            "8k" => "4320p",
            x if RESOLUTIONS.contains(&x) => x,
            _ => return None,
        },
        "source" => match l.as_str() {
            "bluray" | "bd" | "bluray1080p" => "bluray",
            "uhdbluray" | "uhdbd" => "uhd_bluray",
            "webdl" | "web" | "web_dl" => "web_dl",
            "webrip" => "webrip",
            "hdtv" => "hdtv",
            "dvd" | "dvdrip" => "dvd",
            "uhd_bluray" => "uhd_bluray",
            _ => return None,
        },
        "release_type" => match l.as_str() {
            "disc" | "fulldisc" | "原盘" => "disc",
            "remux" => "remux",
            "encode" | "rip" => "encode",
            "web" => "web",
            "hdtv" => "hdtv",
            _ => return None,
        },
        "video_codec" => match l.as_str() {
            "hevc" | "h265" | "x265" => "hevc",
            "avc" | "h264" | "x264" => "avc",
            "av1" => "av1",
            "vc1" => "vc1",
            "mpeg2" => "mpeg2",
            _ => return None,
        },
        "audio_codecs" => match l.as_str() {
            "truehd" => "truehd",
            "atmos" => "atmos",
            "dtsx" | "dts:x" | "dts_x" => "dts_x",
            "dtshdma" | "dtshd" | "dts_hd_ma" => "dts_hd_ma",
            "dts" => "dts",
            "ddp" | "dd+" | "eac3" => "ddp",
            "dd" | "ac3" => "dd",
            "aac" => "aac",
            "flac" => "flac",
            "lpcm" | "pcm" => "lpcm",
            "opus" => "opus",
            _ => return None,
        },
        "hdr" => match l.as_str() {
            "dv" | "dovi" | "dolbyvision" => "dv",
            "hdr10+" | "hdr10plus" => "hdr10plus",
            "hdr" | "hdr10" => "hdr10",
            "hlg" => "hlg",
            _ => return None,
        },
        _ => return None,
    };
    Some(out.to_owned())
}

/// 规格字段（单值：resolution / source / release_type / video_codec；多值：audio_codecs / hdr）写入 `o`；
/// `plugin` 为错误码前缀（`<plugin>.<field>_invalid`），`fields` 限定要处理的字段
pub fn normalize_specs(
    input: &Value,
    o: &mut Map<String, Value>,
    errs: &mut Errors,
    plugin: &str,
    fields: &[&str],
) {
    let get = |k: &str| input.get(k).filter(|v| !v.is_null() && v.as_str() != Some(""));
    for k in ["resolution", "source", "release_type", "video_codec"]
        .iter()
        .filter(|k| fields.contains(k))
    {
        if let Some(v) = get(k).and_then(Value::as_str) {
            match canon(k, v) {
                Some(c) => {
                    o.insert((*k).into(), json!(c));
                }
                None => errs.add(k, &format!("{plugin}.{k}_invalid"), "unknown value"),
            }
        }
    }
    for k in ["audio_codecs", "hdr"].iter().filter(|k| fields.contains(k)) {
        if let Some(v) = get(k) {
            let xs: Vec<&str> = match v {
                Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
                Value::String(s) => s.split(',').collect(),
                _ => vec![],
            };
            let mut out: Vec<String> = Vec::new();
            for x in xs.iter().filter(|x| !x.trim().is_empty()) {
                match canon(k, x) {
                    Some(c) if !out.contains(&c) => out.push(c),
                    Some(_) => {}
                    None => errs.add(k, &format!("{plugin}.{k}_invalid"), "unknown value"),
                }
            }
            o.insert((*k).into(), json!(out));
        }
    }
}

fn fill(o: &mut Map<String, Value>, k: &str, v: Option<Value>) {
    if let Some(v) = v
        && !o.contains_key(k)
    {
        o.insert(k.into(), v);
    }
}

/// 从发布名补全规格（用户填写优先）；返回解析结果供调用方补全标题、季集等
pub fn fill_from_release(o: &mut Map<String, Value>, release_name: &str) -> pt_release_parser::ReleaseInfo {
    let p = pt_release_parser::parse_release(release_name);
    fill(o, "resolution", p.resolution.clone().map(|x| json!(x)));
    let (src, rtype) = match p.source.as_deref() {
        Some("remux") => (Some("bluray"), Some("remux")),
        Some("bluray") => (
            Some(if p.resolution.as_deref() == Some("2160p") {
                "uhd_bluray"
            } else {
                "bluray"
            }),
            Some("encode"),
        ),
        Some("web_dl") => (Some("web_dl"), Some("web")),
        Some("webrip") => (Some("webrip"), Some("web")),
        Some("hdtv") => (Some("hdtv"), Some("hdtv")),
        Some("dvd") => (Some("dvd"), Some("encode")),
        _ => (None, None),
    };
    fill(o, "source", src.map(|x| json!(x)));
    fill(o, "release_type", rtype.map(|x| json!(x)));
    fill(o, "video_codec", p.video_codec.clone().map(|x| json!(x)));
    if !p.audio_codec.is_empty() {
        fill(o, "audio_codecs", Some(json!(p.audio_codec)));
    }
    if !p.hdr.is_empty() {
        fill(o, "hdr", Some(json!(p.hdr)));
    }
    fill(o, "release_group", p.group.clone().map(|x| json!(x)));
    p
}

/// 规范值 → 标题中的显示写法
pub fn display(field: &str, v: &Value) -> Option<String> {
    fn m<'a>(field: &str, x: &'a str) -> &'a str {
        match (field, x) {
            ("source", "uhd_bluray") => "UHD BluRay",
            ("source", "bluray") => "BluRay",
            ("source", "web_dl") => "WEB-DL",
            ("source", "webrip") => "WEBRip",
            ("source", "hdtv") => "HDTV",
            ("source", "dvd") => "DVD",
            ("release_type", "remux") => "REMUX",
            ("release_type", _) => "",
            ("video_codec", "hevc") => "HEVC",
            ("video_codec", "avc") => "AVC",
            ("video_codec", "av1") => "AV1",
            ("video_codec", "vc1") => "VC-1",
            ("video_codec", "mpeg2") => "MPEG-2",
            ("audio_codecs", "truehd") => "TrueHD",
            ("audio_codecs", "atmos") => "Atmos",
            ("audio_codecs", "dts_x") => "DTS:X",
            ("audio_codecs", "dts_hd_ma") => "DTS-HD MA",
            ("audio_codecs", "dts") => "DTS",
            ("audio_codecs", "ddp") => "DDP",
            ("audio_codecs", "dd") => "DD",
            ("audio_codecs", "aac") => "AAC",
            ("audio_codecs", "flac") => "FLAC",
            ("audio_codecs", "lpcm") => "LPCM",
            ("audio_codecs", "opus") => "Opus",
            ("hdr", "dv") => "DV",
            ("hdr", "hdr10plus") => "HDR10+",
            ("hdr", "hdr10") => "HDR10",
            ("hdr", "hlg") => "HLG",
            (_, other) => other,
        }
    }
    match v.as_str() {
        Some(x) => Some(m(field, x).to_owned()),
        None => crate::plain(v),
    }
}

/// 规格字段的 schema 片段
pub fn spec_schema(label: &dyn Fn(&str) -> String) -> Map<String, Value> {
    let enum_field = |f: &str, values: &[&str]| json!({ "type": "string", "enum": values, "x-pt-label-term": label(f), "x-pt-widget": "select", "x-pt-filter": "facet" });
    let multi = |f: &str, values: &[&str]| {
        json!({ "type": "array", "items": { "type": "string", "enum": values }, "uniqueItems": true,
                "x-pt-label-term": label(f), "x-pt-widget": "multiselect", "x-pt-filter": "facet" })
    };
    let mut m = Map::new();
    m.insert("resolution".into(), enum_field("resolution", RESOLUTIONS));
    m.insert("source".into(), enum_field("source", SOURCES));
    m.insert("release_type".into(), enum_field("release_type", RELEASE_TYPES));
    m.insert("video_codec".into(), enum_field("video_codec", VIDEO));
    m.insert("audio_codecs".into(), multi("audio_codecs", AUDIO));
    m.insert("hdr".into(), multi("hdr", HDR));
    m.insert(
        "release_group".into(),
        json!({ "type": "string", "maxLength": 40, "x-pt-label-term": label("release_group"), "x-pt-filter": "facet" }),
    );
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs() {
        let mut o = Map::new();
        let mut e = Errors::default();
        normalize_specs(
            &json!({ "video_codec": "x265", "hdr": "DoVi, HDR10", "resolution": "999p" }),
            &mut o,
            &mut e,
            "type-x",
            &["video_codec", "hdr", "resolution"],
        );
        assert_eq!(o.get("video_codec"), Some(&json!("hevc")));
        assert_eq!(o.get("hdr"), Some(&json!(["dv", "hdr10"])));
        assert_eq!(e.first_field(), Some("resolution"));
        let p = fill_from_release(&mut o, "Show.S01.2160p.BluRay.REMUX.DTS-HD.MA-GRP");
        assert_eq!(p.season, Some(1));
        assert_eq!(o.get("source"), Some(&json!("bluray")));
        assert_eq!(o.get("release_type"), Some(&json!("remux")));
        assert_eq!(o.get("video_codec"), Some(&json!("hevc")), "user value wins");
        assert_eq!(display("source", &json!("web_dl")).as_deref(), Some("WEB-DL"));
    }
}
