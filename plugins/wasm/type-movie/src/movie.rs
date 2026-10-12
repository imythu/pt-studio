//! 电影类型的纯逻辑（可在宿主上单元测试）：schema、规范化、分组键、规范标题。

use pt_type_kit::{Errors, Template};
use serde_json::{Map, Value, json};

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

pub const DEFAULT_TEMPLATE: &str = "{title} {year} {edition?| }{resolution} {release_type?| }{source} {hdr?| }{video_codec} {audio_codecs?| }-{release_group}";

/// 已知字段（未知字段在规范化时丢弃）
pub const FIELDS: &[&str] = &[
    "title",
    "original_title",
    "year",
    "imdb_id",
    "tmdb_id",
    "douban_id",
    "resolution",
    "source",
    "release_type",
    "video_codec",
    "audio_codecs",
    "hdr",
    "release_group",
    "edition",
    "is_3d",
    "release_name",
];

fn label(f: &str) -> String {
    format!("type-movie.field.{f}")
}

fn enum_field(f: &str, values: &[&str], facet: bool) -> Value {
    json!({ "type": "string", "enum": values, "x-pt-label-term": label(f), "x-pt-widget": "select",
            "x-pt-filter": if facet { "facet" } else { "none" } })
}

fn multi(f: &str, values: &[&str]) -> Value {
    json!({ "type": "array", "items": { "type": "string", "enum": values }, "uniqueItems": true,
            "x-pt-label-term": label(f), "x-pt-widget": "multiselect", "x-pt-filter": "facet" })
}

pub fn field_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "x-pt-order": ["release_name", "title", "original_title", "year", "imdb_id", "tmdb_id", "douban_id", "resolution", "source",
                       "release_type", "video_codec", "audio_codecs", "hdr", "release_group", "edition", "is_3d"],
        "properties": {
            "release_name": { "type": "string", "maxLength": 300, "x-pt-label-term": label("release_name"),
                               "description": "Scene release name; missing specs are filled from it" },
            "title": { "type": "string", "maxLength": 200, "x-pt-label-term": label("title"), "x-pt-group-level": true, "x-pt-search": "searchable" },
            "original_title": { "type": "string", "maxLength": 200, "x-pt-label-term": label("original_title"), "x-pt-group-level": true, "x-pt-search": "searchable" },
            "year": { "type": "integer", "minimum": 1880, "maximum": 2100, "x-pt-label-term": label("year"), "x-pt-group-level": true, "x-pt-filter": "range" },
            "imdb_id": { "type": "string", "pattern": "^tt[0-9]{7,9}$", "x-pt-label-term": label("imdb_id"), "x-pt-widget": "external-id", "x-pt-group-level": true, "x-pt-search": "exact" },
            "tmdb_id": { "type": "integer", "minimum": 1, "x-pt-label-term": label("tmdb_id"), "x-pt-widget": "external-id", "x-pt-group-level": true },
            "douban_id": { "type": "string", "pattern": "^[0-9]{1,12}$", "x-pt-label-term": label("douban_id"), "x-pt-widget": "external-id", "x-pt-group-level": true },
            "resolution": enum_field("resolution", RESOLUTIONS, true),
            "source": enum_field("source", SOURCES, true),
            "release_type": enum_field("release_type", RELEASE_TYPES, true),
            "video_codec": enum_field("video_codec", VIDEO, true),
            "audio_codecs": multi("audio_codecs", AUDIO),
            "hdr": multi("hdr", HDR),
            "release_group": { "type": "string", "maxLength": 40, "x-pt-label-term": label("release_group"), "x-pt-filter": "facet" },
            "edition": { "type": "string", "maxLength": 60, "x-pt-label-term": label("edition") },
            "is_3d": { "type": "boolean", "x-pt-label-term": "type-movie.field.is_3d" },
        },
    })
}

pub fn group_schema() -> Value {
    let f = field_schema();
    let mut props = Map::new();
    for k in [
        "title",
        "original_title",
        "year",
        "imdb_id",
        "tmdb_id",
        "douban_id",
    ] {
        props.insert(k.into(), f["properties"][k].clone());
    }
    json!({ "type": "object", "x-pt-order": ["title", "original_title", "year", "imdb_id", "tmdb_id", "douban_id"], "properties": props })
}

/// 用户输入的同义写法 → 规范值
fn canon(field: &str, v: &str) -> Option<String> {
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
            "webdl" | "web" => "web_dl",
            "webrip" => "webrip",
            "hdtv" => "hdtv",
            "dvd" | "dvdrip" => "dvd",
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
            "dtsx" | "dts:x" => "dts_x",
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

fn text(v: &Value, max: usize) -> Option<String> {
    let s: String = v.as_str()?.trim().chars().filter(|c| !c.is_control()).collect();
    (!s.is_empty()).then(|| s.chars().take(max).collect())
}

/// 规范化（纯函数）：裁剪未知字段、同义词映射、外部 ID 提取、从发布名补全、校验
pub fn normalize(input: &Value) -> Result<Value, Errors> {
    let mut errs = Errors::default();
    let mut o = Map::new();
    let get = |k: &str| input.get(k).filter(|v| !v.is_null());
    for k in ["title", "original_title", "release_group", "edition"] {
        if let Some(s) = get(k).and_then(|v| text(v, if k == "release_group" { 40 } else { 200 })) {
            o.insert(k.into(), json!(s));
        }
    }
    let release_name = get("release_name").and_then(|v| text(v, 300));
    if let Some(r) = &release_name {
        o.insert("release_name".into(), json!(r));
    }
    if let Some(v) = get("year") {
        match v
            .as_i64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        {
            Some(y) if (1880..=2100).contains(&y) => {
                o.insert("year".into(), json!(y));
            }
            _ => errs.add(
                "year",
                "type-movie.year_invalid",
                "year must be between 1880 and 2100",
            ),
        }
    }
    if let Some(v) = get("imdb_id") {
        match v.as_str().and_then(pt_type_kit::imdb_id) {
            Some(id) => {
                o.insert("imdb_id".into(), json!(id));
            }
            None => errs.add("imdb_id", "type-movie.imdb_id_invalid", "not an IMDb id or link"),
        }
    }
    if let Some(v) = get("tmdb_id") {
        match v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| pt_type_kit::tmdb_id(s, "movie")))
        {
            Some(id) => {
                o.insert("tmdb_id".into(), json!(id));
            }
            None => errs.add("tmdb_id", "type-movie.tmdb_id_invalid", "not a TMDB id or link"),
        }
    }
    if let Some(v) = get("douban_id") {
        let s = v
            .as_str()
            .map(str::to_owned)
            .or_else(|| v.as_u64().map(|n| n.to_string()));
        match s.as_deref().and_then(pt_type_kit::douban_id) {
            Some(id) => {
                o.insert("douban_id".into(), json!(id));
            }
            None => errs.add(
                "douban_id",
                "type-movie.douban_id_invalid",
                "not a Douban id or link",
            ),
        }
    }
    for k in ["resolution", "source", "release_type", "video_codec"] {
        if let Some(v) = get(k).and_then(Value::as_str) {
            match canon(k, v) {
                Some(c) => {
                    o.insert(k.into(), json!(c));
                }
                None => errs.add(k, &format!("type-movie.{k}_invalid"), "unknown value"),
            }
        }
    }
    for k in ["audio_codecs", "hdr"] {
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
                    None => errs.add(k, &format!("type-movie.{k}_invalid"), "unknown value"),
                }
            }
            o.insert(k.into(), json!(out));
        }
    }
    if let Some(b) = get("is_3d").and_then(Value::as_bool) {
        o.insert("is_3d".into(), json!(b));
    }
    // 发布名补全（用户填写优先）
    if let Some(r) = &release_name {
        let p = pt_release_parser::parse_release(r);
        let fill = |o: &mut Map<String, Value>, k: &str, v: Option<Value>| {
            if let Some(v) = v
                && !o.contains_key(k)
            {
                o.insert(k.into(), v);
            }
        };
        fill(
            &mut o,
            "title",
            pt_release_parser::parse_title(r).map(|t| json!(t)),
        );
        fill(&mut o, "year", p.year.map(|y| json!(y)));
        fill(&mut o, "resolution", p.resolution.clone().map(|x| json!(x)));
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
        fill(&mut o, "source", src.map(|x| json!(x)));
        fill(&mut o, "release_type", rtype.map(|x| json!(x)));
        fill(&mut o, "video_codec", p.video_codec.map(|x| json!(x)));
        if !p.audio_codec.is_empty() {
            fill(&mut o, "audio_codecs", Some(json!(p.audio_codec)));
        }
        if !p.hdr.is_empty() {
            fill(&mut o, "hdr", Some(json!(p.hdr)));
        }
        fill(&mut o, "release_group", p.group.map(|x| json!(x)));
        if p.flags.iter().any(|f| f == "3D") {
            fill(&mut o, "is_3d", Some(json!(true)));
        }
    }
    // 必填组合（content-types-common §2；原盘不要求视频编码）
    if !o.contains_key("title") && !o.contains_key("imdb_id") {
        errs.add(
            "title",
            "type-movie.title_invalid",
            "title or IMDb id is required",
        );
    }
    for k in ["year", "resolution", "source"] {
        if !o.contains_key(k) && !errs.first_field().is_some_and(|f| f == k) {
            errs.add(k, &format!("type-movie.{k}_invalid"), "required");
        }
    }
    if !o.contains_key("video_codec") && o.get("release_type").and_then(Value::as_str) != Some("disc") {
        errs.add("video_codec", "type-movie.video_codec_invalid", "required");
    }
    if errs.is_empty() {
        Ok(Value::Object(o))
    } else {
        Err(errs)
    }
}

pub fn group_key(f: &Value) -> Option<String> {
    if let Some(id) = f["imdb_id"].as_str() {
        return Some(format!("movie:imdb:{id}"));
    }
    if let Some(id) = f["tmdb_id"].as_u64() {
        return Some(format!("movie:tmdb:{id}"));
    }
    if let Some(id) = f["douban_id"].as_str() {
        return Some(format!("movie:douban:{id}"));
    }
    match (f["title"].as_str(), f["year"].as_i64()) {
        (Some(t), Some(y)) => Some(format!("movie:title:{}:{y}", pt_type_kit::norm_key(t))),
        _ => None,
    }
}

/// 规范值 → 标题中的显示写法
fn display(field: &str, v: &Value) -> Option<String> {
    let s = v.as_str();
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
    match s {
        Some(x) => Some(m(field, x).to_owned()),
        None => pt_type_kit::plain(v),
    }
}

pub fn canonical_title(f: &Value, template: Option<&str>) -> String {
    let tpl = template
        .and_then(|t| Template::parse(t, Some(FIELDS)).ok())
        .or_else(|| Template::parse(DEFAULT_TEMPLATE, Some(FIELDS)).ok());
    tpl.map(|t| t.render(f, &display)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_release_name() {
        let f = normalize(
            &json!({ "release_name": "The.Wandering.Earth.II.2023.2160p.WEB-DL.H265.DDP5.1-ABC", "junk": 1 }),
        )
        .unwrap_or_else(|e| panic!("{:?}", e.details()));
        assert_eq!(f["title"], "The Wandering Earth II");
        assert_eq!(f["year"], 2023);
        assert_eq!(f["resolution"], "2160p");
        assert_eq!(
            (f["source"].as_str(), f["release_type"].as_str()),
            (Some("web_dl"), Some("web"))
        );
        assert_eq!(f["video_codec"], "hevc");
        assert_eq!(f["audio_codecs"], json!(["ddp"]));
        assert_eq!(f["release_group"], "ABC");
        assert!(f.get("junk").is_none());
        assert_eq!(
            group_key(&f).as_deref(),
            Some("movie:title:the-wandering-earth-ii:2023")
        );
        assert_eq!(
            canonical_title(&f, None),
            "The Wandering Earth II 2023 2160p WEB-DL HEVC DDP -ABC"
        );
    }

    #[test]
    fn user_values_win_and_synonyms() {
        let f = normalize(&json!({
            "release_name": "Heat.1995.1080p.BluRay.REMUX.AVC.DTS-HD.MA.5.1-FGT", "title": "Heat",
            "imdb_id": "https://www.imdb.com/title/tt0113277/", "video_codec": "x265", "hdr": "DoVi, HDR10",
        }))
        .unwrap_or_else(|e| panic!("{:?}", e.details()));
        assert_eq!(f["video_codec"], "hevc");
        assert_eq!(f["hdr"], json!(["dv", "hdr10"]));
        assert_eq!(
            (f["source"].as_str(), f["release_type"].as_str()),
            (Some("bluray"), Some("remux"))
        );
        assert_eq!(group_key(&f).as_deref(), Some("movie:imdb:tt0113277"));
        assert_eq!(
            canonical_title(&f, None),
            "Heat 1995 1080p REMUX BluRay DV HDR10 HEVC DTS-HD MA -FGT"
        );
    }

    #[test]
    fn validation() {
        let e = normalize(&json!({ "title": "X", "year": 1700, "resolution": "999p" }))
            .err()
            .unwrap_or_default();
        let d = e.details();
        let empty = vec![];
        let paths: Vec<&str> = d["errors"]
            .as_array()
            .unwrap_or(&empty)
            .iter()
            .filter_map(|x| x["path"].as_str())
            .collect();
        assert!(
            paths.contains(&"/year")
                && paths.contains(&"/resolution")
                && paths.contains(&"/source")
                && paths.contains(&"/video_codec"),
            "{paths:?}"
        );
        // 原盘不要求视频编码
        assert!(normalize(&json!({ "title": "X", "year": 2000, "resolution": "2160p", "source": "UHD BluRay", "release_type": "原盘" })).is_ok());
    }
}
