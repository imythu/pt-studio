//! 音乐类型的纯逻辑（type-music LLD §1.1）：schema、规范化（艺人、同义词、格式与编码组合、版本、发布名补全）、
//! 分组键（MusicBrainz 发行组 > 主艺人 + 专辑 + 原始年份）、版本键、作品标题、规范标题、文件检查。

use pt_type_kit::{Errors, Template};
use serde_json::{Map, Value, json};

use crate::logcheck;

pub const MEDIA: &[&str] = &[
    "cd",
    "web",
    "vinyl",
    "sacd",
    "dvd",
    "bluray",
    "cassette",
    "dat",
    "soundboard",
    "other",
];
pub const FORMATS: &[&str] = &[
    "flac", "mp3", "aac", "alac", "dsd", "opus", "ac3", "dts", "wavpack", "ape", "other",
];
pub const ENCODINGS: &[&str] = &[
    "lossless",
    "lossless_24",
    "320",
    "v0",
    "v1",
    "v2",
    "256",
    "192",
    "other",
];
pub const RELEASE_TYPES: &[&str] = &[
    "album",
    "ep",
    "single",
    "compilation",
    "soundtrack",
    "live",
    "remix",
    "anthology",
    "mixtape",
    "bootleg",
    "interview",
    "other",
];
/// 艺人字段（按角色）
pub const ARTIST_FIELDS: &[(&str, &str)] = &[
    ("artists", "main"),
    ("guest_artists", "guest"),
    ("composers", "composer"),
    ("conductors", "conductor"),
    ("producers", "producer"),
    ("remixers", "remixer"),
    ("djs", "dj"),
];
pub const MAX_LOGS: usize = 5;

pub const DEFAULT_TEMPLATE: &str =
    "{artist_credit} - {album} {original_year?[|]} {edition_title?(|)} [{media} {format} {encoding}{log? |}]";

pub const FIELDS: &[&str] = &[
    "artist_credit",
    "album",
    "original_year",
    "release_type",
    "edition_year",
    "edition_label",
    "edition_catalog",
    "edition_title",
    "media",
    "format",
    "encoding",
    "log",
];

fn label(f: &str) -> String {
    format!("type-music.field.{f}")
}

/// 格式允许的编码（LLD §1.1 合法组合表；ac3 / dts / other 不限）
pub fn encodings_for(format: &str) -> &'static [&'static str] {
    match format {
        "flac" | "alac" | "wavpack" | "ape" => &["lossless", "lossless_24"],
        "dsd" => &["lossless"],
        "mp3" => &["320", "v0", "v1", "v2", "256", "192", "other"],
        "aac" | "opus" => &["320", "256", "192", "other"],
        _ => ENCODINGS,
    }
}

/// 格式对应的音频扩展名（LLD §1.3）
pub fn extensions_for(format: &str) -> &'static [&'static str] {
    match format {
        "flac" => &["flac"],
        "mp3" => &["mp3"],
        "aac" => &["m4a", "aac"],
        "alac" => &["m4a"],
        "dsd" => &["dsf", "dff"],
        "opus" => &["opus"],
        "ac3" => &["ac3"],
        "dts" => &["dts"],
        "wavpack" => &["wv"],
        "ape" => &["ape"],
        _ => &[
            "flac", "mp3", "m4a", "aac", "dsf", "dff", "opus", "ac3", "dts", "wv", "ape", "wav", "ogg",
        ],
    }
}

pub const FORBIDDEN_EXT: &[&str] = &[
    "exe", "com", "bat", "cmd", "scr", "msi", "ps1", "vbs", "lnk", "pif", "dll",
];

pub fn field_schema() -> Value {
    let mut p = Map::new();
    let s = |max: i64, f: &str| json!({ "type": "string", "maxLength": max, "x-pt-label-term": label(f) });
    let e = |values: &[&str], f: &str| json!({ "type": "string", "enum": values, "x-pt-label-term": label(f), "x-pt-widget": "select", "x-pt-filter": "facet" });
    p.insert("release_name".into(), s(255, "release_name"));
    let mut album = s(250, "album");
    album["x-pt-group-level"] = json!(true);
    album["x-pt-search"] = json!("searchable");
    p.insert("album".into(), album);
    for (k, _) in ARTIST_FIELDS {
        p.insert(
            (*k).into(),
            json!({ "type": "array", "items": { "type": "string", "maxLength": 120 }, "maxItems": 50, "x-pt-label-term": label(k),
                    "x-pt-group-level": true, "x-pt-search": "searchable" }),
        );
    }
    let mut rt = e(RELEASE_TYPES, "release_type");
    rt["x-pt-group-level"] = json!(true);
    p.insert("release_type".into(), rt);
    p.insert("original_year".into(), json!({ "type": "integer", "minimum": 1877, "maximum": 2100, "x-pt-label-term": label("original_year"), "x-pt-group-level": true }));
    p.insert("mb_release_group_id".into(), json!({ "type": "string", "x-pt-label-term": label("mb_release_group_id"), "x-pt-widget": "external-id", "x-pt-group-level": true }));
    p.insert("media".into(), e(MEDIA, "media"));
    p.insert("edition_year".into(), json!({ "type": "integer", "minimum": 1877, "maximum": 2100, "x-pt-label-term": label("edition_year") }));
    p.insert("edition_label".into(), s(80, "edition_label"));
    p.insert("edition_catalog".into(), s(80, "edition_catalog"));
    p.insert("edition_title".into(), s(80, "edition_title"));
    p.insert("format".into(), e(FORMATS, "format"));
    p.insert("encoding".into(), e(ENCODINGS, "encoding"));
    for k in ["has_log", "has_cue", "scene"] {
        p.insert(
            k.into(),
            json!({ "type": "boolean", "x-pt-label-term": label(k), "x-pt-filter": "facet" }),
        );
    }
    p.insert("logs".into(), json!({ "type": "array", "items": { "type": "string" }, "maxItems": MAX_LOGS, "x-pt-label-term": label("logs"), "x-pt-widget": "log-files" }));
    p.insert("log_score".into(), json!({ "type": "integer", "minimum": 0, "maximum": 100, "readOnly": true, "x-pt-label-term": label("log_score") }));
    json!({ "type": "object", "additionalProperties": false, "properties": p,
            "x-pt-order": ["release_name", "artists", "album", "release_type", "original_year", "mb_release_group_id", "media", "format",
                           "encoding", "edition_year", "edition_label", "edition_catalog", "edition_title", "has_log", "has_cue", "logs",
                           "scene", "guest_artists", "composers", "conductors", "producers", "remixers", "djs"] })
}

pub fn group_schema() -> Value {
    let f = field_schema();
    let mut p = Map::new();
    for k in ["album", "release_type", "original_year", "mb_release_group_id"] {
        p.insert(k.into(), f["properties"][k].clone());
    }
    for (k, _) in ARTIST_FIELDS {
        p.insert((*k).into(), f["properties"][*k].clone());
    }
    for (k, ty) in [
        ("cover", json!({ "type": "string", "x-pt-widget": "image" })),
        (
            "genres",
            json!({ "type": "array", "items": { "type": "string" } }),
        ),
        ("description", json!({ "type": "string" })),
    ] {
        let mut v = ty;
        v["x-pt-label-term"] = json!(label(k));
        v["x-pt-from-metadata"] = json!(true);
        p.insert(k.into(), v);
    }
    json!({ "type": "object", "properties": p,
            "x-pt-order": ["cover", "artists", "album", "release_type", "original_year", "guest_artists", "composers", "conductors",
                           "producers", "remixers", "djs", "genres", "mb_release_group_id", "description"] })
}

fn text(v: &Value, max: usize) -> Option<String> {
    let s: String = v
        .as_str()?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    (!s.is_empty()).then(|| s.chars().take(max).collect())
}

/// 同义写法 → 规范值
pub fn canon(field: &str, v: &str) -> Option<&'static str> {
    let l = v.trim().to_lowercase().replace([' ', '-', '_', '.'], "");
    Some(match field {
        "format" => match l.as_str() {
            "flac" => "flac",
            "mp3" => "mp3",
            "aac" | "m4a" => "aac",
            "alac" => "alac",
            "dsd" | "dsf" | "dff" | "dsd64" | "dsd128" | "dsd256" => "dsd",
            "opus" => "opus",
            "ac3" | "dd" => "ac3",
            "dts" => "dts",
            "wavpack" | "wv" => "wavpack",
            "ape" | "monkeysaudio" | "monkey'saudio" => "ape",
            "other" => "other",
            _ => return None,
        },
        "encoding" => match l.as_str() {
            "lossless" | "16bit" | "16bitlossless" => "lossless",
            "lossless24" | "24bit" | "24bitlossless" | "hires" | "hr" => "lossless_24",
            "320" | "320k" | "320kbps" | "cbr320" => "320",
            "v0" | "v0vbr" | "vbrv0" => "v0",
            "v1" | "v1vbr" | "vbrv1" => "v1",
            "v2" | "v2vbr" | "vbrv2" => "v2",
            "256" | "256k" | "256kbps" => "256",
            "192" | "192k" | "192kbps" => "192",
            "other" => "other",
            _ => return None,
        },
        "media" => match l.as_str() {
            "cd" => "cd",
            "web" | "webdl" | "digital" => "web",
            "vinyl" | "lp" => "vinyl",
            "sacd" => "sacd",
            "dvd" | "dvda" => "dvd",
            "bluray" | "bd" => "bluray",
            "cassette" | "tape" => "cassette",
            "dat" => "dat",
            "soundboard" | "sbd" => "soundboard",
            "other" => "other",
            _ => return None,
        },
        "release_type" => RELEASE_TYPES.iter().copied().find(|x| *x == l)?,
        _ => return None,
    })
}

/// 艺人：字符串（`A & B`、`A, B`、`A; B`）、字符串数组或 `{name, role}` 对象数组（LLD 形态）
fn artist_lists(input: &Value) -> Map<String, Value> {
    let mut out: Map<String, Value> = Map::new();
    let mut push = |field: &str, name: &str| {
        let n: String = name
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(120)
            .collect();
        if n.is_empty() {
            return;
        }
        let a = out.entry(field).or_insert_with(|| json!([]));
        if let Some(arr) = a.as_array_mut()
            && !arr
                .iter()
                .any(|x| x.as_str().is_some_and(|s| s.eq_ignore_ascii_case(&n)))
            && arr.len() < 50
        {
            arr.push(json!(n));
        }
    };
    for (field, role) in ARTIST_FIELDS {
        match input.get(*field) {
            Some(Value::String(s)) => {
                for n in s.split([',', ';', '、']).flat_map(|x| x.split(" & ")) {
                    push(field, n);
                }
            }
            Some(Value::Array(a)) => {
                for x in a {
                    match x {
                        Value::String(s) => push(field, s),
                        Value::Object(o) => {
                            let r = o.get("role").and_then(Value::as_str).unwrap_or(role);
                            let target = ARTIST_FIELDS
                                .iter()
                                .find(|(_, rr)| *rr == r)
                                .map_or(*field, |(f, _)| *f);
                            if let Some(n) = o.get("name").and_then(Value::as_str) {
                                push(target, n);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// 发布名：`Artist - Album (2011) [FLAC 24bit]`、`Artist-Album-(CAT123)-WEB-2023-GRP`（场景风格只取艺人 / 专辑 / 年份 / 介质）
#[derive(Debug, Default, PartialEq)]
pub struct MusicName {
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<i64>,
    pub format: Option<&'static str>,
    pub encoding: Option<&'static str>,
    pub media: Option<&'static str>,
    pub catalog: Option<String>,
}

pub fn parse_name(name: &str) -> MusicName {
    let mut n = MusicName::default();
    let mut tags: Vec<String> = Vec::new();
    let mut plain = String::new();
    let mut depth = 0;
    let mut cur = String::new();
    for c in name.chars() {
        match c {
            '[' | '(' | '{' if depth == 0 => depth = 1,
            ']' | ')' | '}' if depth == 1 => {
                tags.push(std::mem::take(&mut cur));
                depth = 0;
            }
            _ if depth == 1 => cur.push(c),
            _ => plain.push(c),
        }
    }
    let year = |s: &str| s.trim().parse::<i64>().ok().filter(|y| (1877..=2100).contains(y));
    let scene = !plain.contains(" - ") && plain.contains('-') && !plain.contains(' ');
    let words: Vec<String> = if scene {
        plain.split('-').map(|w| w.replace('_', " ")).collect()
    } else {
        plain.split(" - ").map(str::to_owned).collect()
    };
    for t in tags
        .iter()
        .flat_map(|t| t.split([' ', ',', '/']).map(str::to_owned).collect::<Vec<_>>())
    {
        if let Some(y) = year(&t) {
            n.year.get_or_insert(y);
        } else if let Some(f) = canon("format", &t) {
            n.format.get_or_insert(f);
        } else if let Some(e) = canon("encoding", &t) {
            n.encoding.get_or_insert(e);
        } else if let Some(m) = canon("media", &t) {
            n.media.get_or_insert(m);
        } else if scene
            && t.chars().any(|c| c.is_ascii_digit())
            && t.chars().all(|c| c.is_ascii_alphanumeric())
        {
            n.catalog.get_or_insert(t);
        }
    }
    let words: Vec<&str> = words.iter().map(|w| w.trim()).filter(|w| !w.is_empty()).collect();
    if scene {
        // Artist-Album-…-WEB-2023-GRP：年份之前、介质之前的为专辑
        for w in &words[2.min(words.len())..] {
            if let Some(y) = year(w) {
                n.year.get_or_insert(y);
            } else if let Some(m) = canon("media", w) {
                n.media.get_or_insert(m);
            }
        }
    }
    if words.len() >= 2 {
        n.artist = Some(words[0].to_owned());
        n.album = Some(words[1].to_owned());
    } else if let Some(w) = words.first() {
        n.album = Some((*w).to_owned());
    }
    n
}

fn is_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12]) && s.chars().all(|c| c == '-' || c.is_ascii_hexdigit())
}

/// MusicBrainz 发行组 ID（UUID 或 `musicbrainz.org/release-group/<uuid>` 链接）
pub fn mbrg_id(s: &str) -> Option<String> {
    let s = s.trim().to_lowercase();
    let tail = s.split("/release-group/").nth(1).unwrap_or(&s);
    let id = tail.split(['/', '?', '#']).next()?;
    is_uuid(id).then(|| id.to_owned())
}

pub fn normalize(input: &Value) -> Result<Value, Errors> {
    let mut errs = Errors::default();
    let mut o = Map::new();
    let get = |k: &str| input.get(k).filter(|v| !v.is_null() && v.as_str() != Some(""));
    for (k, max) in [
        ("album", 250),
        ("release_name", 255),
        ("edition_label", 80),
        ("edition_catalog", 80),
        ("edition_title", 80),
    ] {
        if let Some(s) = get(k).and_then(|v| text(v, max)) {
            o.insert(k.into(), json!(s));
        }
    }
    o.extend(artist_lists(input));
    for k in ["original_year", "edition_year"] {
        if let Some(v) = get(k) {
            match v
                .as_i64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
            {
                Some(y) if (1877..=2100).contains(&y) => {
                    o.insert(k.into(), json!(y));
                }
                _ => errs.add(
                    k,
                    "type-music.field_invalid",
                    "year must be between 1877 and 2100",
                ),
            }
        }
    }
    if let Some(v) = get("mb_release_group_id") {
        match v.as_str().and_then(mbrg_id) {
            Some(id) => {
                o.insert("mb_release_group_id".into(), json!(id));
            }
            None => errs.add(
                "mb_release_group_id",
                "type-music.field_invalid",
                "not a MusicBrainz release group id or link",
            ),
        }
    }
    for k in ["format", "encoding", "media", "release_type"] {
        if let Some(v) = get(k).and_then(Value::as_str) {
            match canon(k, v) {
                Some(c) => {
                    o.insert(k.into(), json!(c));
                }
                None => errs.add(k, "type-music.field_invalid", "unknown value"),
            }
        }
    }
    for k in ["has_log", "has_cue", "scene"] {
        if let Some(b) = get(k).and_then(Value::as_bool) {
            o.insert(k.into(), json!(b));
        }
    }
    if let Some(v) = get("logs") {
        let logs: Vec<&str> = match v {
            Value::Array(a) => a
                .iter()
                .filter_map(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .collect(),
            Value::String(s) => vec![s.as_str()],
            _ => vec![],
        };
        if logs.len() > MAX_LOGS {
            errs.add("logs", "type-music.field_invalid", "at most 5 logs");
        } else if logs.iter().any(|l| l.len() > logcheck::MAX_LOG_BYTES) {
            errs.add("logs", "type-music.log_too_large", "a log is larger than 512 KiB");
        } else if !logs.is_empty() {
            o.insert("logs".into(), json!(logs));
            o.entry("has_log").or_insert(json!(true));
        }
    }
    // log_score 只由插件的检查任务写入，忽略上传者提供的值
    // 发布名补全（用户填写优先）
    if let Some(r) = o.get("release_name").and_then(Value::as_str).map(str::to_owned) {
        let n = parse_name(&r);
        if !o.contains_key("artists")
            && let Some(a) = n.artist
        {
            o.extend(artist_lists(&json!({ "artists": a })));
        }
        if let Some(a) = n.album {
            o.entry("album").or_insert(json!(a));
        }
        if let Some(y) = n.year {
            o.entry("original_year").or_insert(json!(y));
        }
        for (k, v) in [("format", n.format), ("encoding", n.encoding), ("media", n.media)] {
            if let Some(v) = v {
                o.entry(k).or_insert(json!(v));
            }
        }
        if let Some(c) = n.catalog {
            o.entry("edition_catalog").or_insert(json!(c));
        }
    }
    // 无损格式未给编码 → 16bit 无损
    if let Some(f) = o.get("format").and_then(Value::as_str)
        && !o.contains_key("encoding")
        && encodings_for(f).first() == Some(&"lossless")
    {
        o.insert("encoding".into(), json!("lossless"));
    }
    o.entry("release_type").or_insert(json!("album"));
    if let (Some(f), Some(e)) = (
        o.get("format").and_then(Value::as_str),
        o.get("encoding").and_then(Value::as_str),
    ) && !encodings_for(f).contains(&e)
    {
        errs.add(
            "encoding",
            "type-music.format_encoding_invalid",
            &format!("{f} cannot be {e}"),
        );
    }
    if !o.contains_key("mb_release_group_id") {
        if !o.contains_key("album") {
            errs.add("album", "type-music.field_invalid", "album is required");
        }
        if !o.contains_key("artists") {
            errs.add(
                "artists",
                "type-music.field_invalid",
                "at least one artist is required",
            );
        }
    }
    for k in ["media", "format", "encoding"] {
        if !o.contains_key(k) {
            errs.add(k, "type-music.field_invalid", "required");
        }
    }
    if errs.is_empty() {
        let v = Value::Object(o.clone());
        o.insert("group_title".into(), json!(group_title(&v)));
        o.insert("edition_key".into(), json!(edition_key(&v)));
        Ok(Value::Object(o))
    } else {
        Err(errs)
    }
}

/// 艺人署名：1 位 → 名字；2 位 → `A & B`；3 位及以上 → Various Artists
pub fn artist_credit(f: &Value) -> String {
    let a: Vec<&str> = f["artists"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    match a.as_slice() {
        [] => String::new(),
        [x] => (*x).to_owned(),
        [x, y] => format!("{x} & {y}"),
        _ => "Various Artists".into(),
    }
}

pub fn group_key(f: &Value) -> Option<String> {
    if let Some(id) = f["mb_release_group_id"].as_str() {
        return Some(format!("music:mbrg:{id}"));
    }
    let artist = f["artists"][0].as_str()?;
    let album = f["album"].as_str()?;
    Some(format!(
        "music:title:{}:{}:{}",
        pt_type_kit::norm_key(artist),
        pt_type_kit::norm_key(album),
        f["original_year"]
            .as_i64()
            .map(|y| y.to_string())
            .unwrap_or_default()
    ))
}

/// 版本键：介质 + 年份 + 厂牌 + 目录号 + 版本名（规范化），同一版本的不同格式相同
pub fn edition_key(f: &Value) -> String {
    let s = |k: &str| f[k].as_str().map(pt_type_kit::norm_key).unwrap_or_default();
    format!(
        "{}|{}|{}|{}|{}",
        s("media"),
        f["edition_year"]
            .as_i64()
            .map(|y| y.to_string())
            .unwrap_or_default(),
        s("edition_label"),
        s("edition_catalog"),
        s("edition_title")
    )
}

/// “艺人 - 专辑”
pub fn group_title(f: &Value) -> String {
    let credit = artist_credit(f);
    let album = f["album"].as_str().unwrap_or("");
    if credit.is_empty() {
        album.to_owned()
    } else {
        format!("{credit} - {album}")
    }
}

pub fn display(field: &str, v: &Value) -> Option<String> {
    let s = v.as_str();
    Some(match (field, s) {
        ("media", Some("cd")) => "CD".into(),
        ("media", Some("web")) => "WEB".into(),
        ("media", Some("sacd")) => "SACD".into(),
        ("media", Some("dvd")) => "DVD".into(),
        ("media", Some("dat")) => "DAT".into(),
        ("media", Some("bluray")) => "Blu-ray".into(),
        ("format", Some("wavpack")) => "WavPack".into(),
        ("format", Some("opus")) => "Opus".into(),
        ("format", Some(x)) => x.to_uppercase(),
        ("encoding", Some("lossless")) => "Lossless".into(),
        ("encoding", Some("lossless_24")) => "24bit Lossless".into(),
        ("encoding", Some(x)) if x.starts_with('v') => format!("{} (VBR)", x.to_uppercase()),
        ("media" | "release_type", Some(x)) => {
            let mut c = x.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        }
        _ => return pt_type_kit::plain(v),
    })
}

pub fn canonical_title(f: &Value, template: Option<&str>) -> String {
    let mut v = f.clone();
    v["artist_credit"] = json!(artist_credit(f));
    if let Some(s) = f["log_score"].as_i64() {
        v["log"] = json!(format!("Log ({s}%)"));
    } else if f["has_log"] == true {
        v["log"] = json!("Log");
    }
    let tpl = template
        .and_then(|t| Template::parse(t, Some(FIELDS)).ok())
        .or_else(|| Template::parse(DEFAULT_TEMPLATE, Some(FIELDS)).ok());
    tpl.map(|t| t.render(&v, &display)).unwrap_or_default()
}

/// 文件列表检查（LLD §1.3）：对应格式的音频文件、log / cue 存在、禁止可执行文件；不通过返回 (错误码, 说明)
pub fn check_files(f: &Value, files: &[(String, u64)]) -> Result<(), (&'static str, String)> {
    let ext = |p: &str| {
        p.rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default()
    };
    if let Some(p) = files
        .iter()
        .map(|(p, _)| p)
        .find(|p| FORBIDDEN_EXT.contains(&ext(p).as_str()))
    {
        return Err((
            "type-music.forbidden_file",
            format!("executable files are not allowed: {p}"),
        ));
    }
    let format = f["format"].as_str().unwrap_or("other");
    let allowed = extensions_for(format);
    if !files.iter().any(|(p, _)| allowed.contains(&ext(p).as_str())) {
        return Err(("type-music.no_audio_files", format!("no {format} audio files")));
    }
    if f["has_log"] == true && !files.iter().any(|(p, _)| ext(p) == "log") {
        return Err((
            "type-music.log_missing",
            "has_log is set but there is no .log file".into(),
        ));
    }
    if f["has_cue"] == true && !files.iter().any(|(p, _)| ext(p) == "cue") {
        return Err((
            "type-music.cue_missing",
            "has_cue is set but there is no .cue file".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_release_name() {
        let f = normalize(&json!({ "release_name": "Daft Punk - Random Access Memories (2013) [FLAC 24bit WEB]", "logs": [] }))
            .unwrap_or_else(|e| panic!("{:?}", e.details()));
        assert_eq!(f["artists"], json!(["Daft Punk"]));
        assert_eq!(
            (f["album"].as_str(), f["original_year"].as_i64()),
            (Some("Random Access Memories"), Some(2013))
        );
        assert_eq!(
            (f["format"].as_str(), f["encoding"].as_str(), f["media"].as_str()),
            (Some("flac"), Some("lossless_24"), Some("web"))
        );
        assert_eq!(f["group_title"], "Daft Punk - Random Access Memories");
        assert_eq!(
            group_key(&f).as_deref(),
            Some("music:title:daft-punk:random-access-memories:2013")
        );
        assert_eq!(
            canonical_title(&f, None),
            "Daft Punk - Random Access Memories [2013] [WEB FLAC 24bit Lossless]"
        );
        let s = parse_name("Artist_Name-Album_Title-(CAT123)-WEB-2023-GRP");
        assert_eq!(
            (
                s.artist.as_deref(),
                s.album.as_deref(),
                s.year,
                s.media,
                s.catalog.as_deref()
            ),
            (
                Some("Artist Name"),
                Some("Album Title"),
                Some(2023),
                Some("web"),
                Some("CAT123")
            )
        );
    }

    #[test]
    fn artists_and_combos() {
        let f = normalize(&json!({
            "artists": [{ "name": "Simon", "role": "main" }, { "name": "Garfunkel" }, { "name": "Guest Person", "role": "guest" }],
            "album": "Bridge", "media": "CD", "format": "FLAC", "has_log": true, "logs": ["x"], "log_score": 100,
            "mb_release_group_id": "https://musicbrainz.org/release-group/0D8A5B1A-1111-2222-3333-444455556666",
        }))
        .unwrap_or_else(|e| panic!("{:?}", e.details()));
        assert_eq!(
            (f["artists"].clone(), f["guest_artists"].clone()),
            (json!(["Simon", "Garfunkel"]), json!(["Guest Person"]))
        );
        assert_eq!(
            (f["encoding"].as_str(), f.get("log_score")),
            (Some("lossless"), None)
        );
        assert_eq!(
            group_key(&f).as_deref(),
            Some("music:mbrg:0d8a5b1a-1111-2222-3333-444455556666")
        );
        assert_eq!(
            canonical_title(&f, None),
            "Simon & Garfunkel - Bridge [CD FLAC Lossless Log]"
        );
        let e = normalize(
            &json!({ "artists": "A", "album": "B", "media": "web", "format": "mp3", "encoding": "24bit" }),
        )
        .err()
        .unwrap_or_default();
        assert_eq!(
            e.details()["errors"][0]["code"],
            "type-music.format_encoding_invalid"
        );
        let v = normalize(&json!({ "artists": "A, B & C", "album": "X", "media": "vinyl", "format": "mp3", "encoding": "V0" })).unwrap_or_default();
        assert_eq!(
            (artist_credit(&v), display("encoding", &v["encoding"])),
            ("Various Artists".into(), Some("V0 (VBR)".into()))
        );
    }

    #[test]
    fn files() {
        let f = json!({ "format": "flac", "has_log": true, "has_cue": false });
        let fl = |names: &[&str]| names.iter().map(|n| ((*n).to_owned(), 1u64)).collect::<Vec<_>>();
        assert!(check_files(&f, &fl(&["01.flac", "rip.log"])).is_ok());
        assert_eq!(
            check_files(&f, &fl(&["01.mp3", "rip.log"])).err().map(|e| e.0),
            Some("type-music.no_audio_files")
        );
        assert_eq!(
            check_files(&f, &fl(&["01.flac"])).err().map(|e| e.0),
            Some("type-music.log_missing")
        );
        assert_eq!(
            check_files(&f, &fl(&["01.flac", "rip.log", "setup.exe"]))
                .err()
                .map(|e| e.0),
            Some("type-music.forbidden_file")
        );
        assert_ne!(
            edition_key(&json!({ "media": "cd", "edition_year": 2011 })),
            edition_key(&json!({ "media": "cd" }))
        );
    }
}
