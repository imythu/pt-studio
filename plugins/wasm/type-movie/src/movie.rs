//! 电影类型的纯逻辑（可在宿主上单元测试）：schema、规范化、分组键、规范标题。

use pt_type_kit::{Errors, Template};
use serde_json::{Map, Value, json};

pub use pt_type_kit::specs::{AUDIO, HDR, RELEASE_TYPES, RESOLUTIONS, SOURCES, VIDEO};

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
    let meta = |ty: Value, f: &str| {
        let mut v = ty;
        v["x-pt-label-term"] = json!(label(f));
        v["x-pt-from-metadata"] = json!(true);
        v
    };
    props.insert(
        "poster".into(),
        meta(json!({ "type": "string", "x-pt-widget": "image" }), "poster"),
    );
    props.insert(
        "overview".into(),
        meta(json!({ "type": "string", "x-pt-widget": "textarea" }), "overview"),
    );
    props.insert(
        "genres".into(),
        meta(
            json!({ "type": "array", "items": { "type": "string" } }),
            "genres",
        ),
    );
    props.insert(
        "runtime_min".into(),
        meta(json!({ "type": "integer" }), "runtime_min"),
    );
    props.insert("rating".into(), meta(json!({ "type": "number" }), "rating"));
    props.insert(
        "countries".into(),
        meta(
            json!({ "type": "array", "items": { "type": "string" } }),
            "countries",
        ),
    );
    json!({ "type": "object", "properties": props,
            "x-pt-order": ["poster", "title", "original_title", "year", "genres", "runtime_min", "rating", "countries", "imdb_id", "tmdb_id", "douban_id", "overview"] })
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
    pt_type_kit::specs::normalize_specs(
        input,
        &mut o,
        &mut errs,
        "type-movie",
        &[
            "resolution",
            "source",
            "release_type",
            "video_codec",
            "audio_codecs",
            "hdr",
        ],
    );
    if let Some(b) = get("is_3d").and_then(Value::as_bool) {
        o.insert("is_3d".into(), json!(b));
    }
    // 发布名补全（用户填写优先）
    if let Some(r) = &release_name {
        let fill = |o: &mut Map<String, Value>, k: &str, v: Option<Value>| {
            if let Some(v) = v
                && !o.contains_key(k)
            {
                o.insert(k.into(), v);
            }
        };
        let p = pt_type_kit::specs::fill_from_release(&mut o, r);
        fill(
            &mut o,
            "title",
            pt_release_parser::parse_title(r).map(|t| json!(t)),
        );
        fill(&mut o, "year", p.year.map(|y| json!(y)));
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

use pt_type_kit::specs::display;

pub fn canonical_title(f: &Value, template: Option<&str>) -> String {
    let tpl = template
        .and_then(|t| Template::parse(t, Some(FIELDS)).ok())
        .or_else(|| Template::parse(DEFAULT_TEMPLATE, Some(FIELDS)).ok());
    tpl.map(|t| t.render(f, &display)).unwrap_or_default()
}

/// 元数据（统一模型）→ Group 字段补丁（只含 group_schema 中 `x-pt-from-metadata` 与外部 ID 字段）
pub fn metadata_patch(data: &Value, locale: &str) -> Value {
    let mut p = Map::new();
    if let Some(v) = data["titles"]["original"].as_str() {
        p.insert("original_title".into(), json!(v));
    }
    if let Some(v) = data["images"]["poster"].as_str() {
        p.insert("poster".into(), json!(v));
    }
    let ov = &data["overview"];
    if let Some(v) = ov[locale].as_str().or_else(|| {
        ov.as_object()
            .and_then(|o| o.values().next())
            .and_then(Value::as_str)
    }) {
        p.insert("overview".into(), json!(v));
    }
    for k in ["genres", "countries"] {
        if let Some(a) = data[k].as_array().filter(|a| !a.is_empty()) {
            p.insert(k.into(), json!(a));
        }
    }
    if let Some(v) = data["runtime_min"].as_i64() {
        p.insert("runtime_min".into(), json!(v));
    }
    // TMDB 评分优先，豆瓣条目补全时用豆瓣评分
    if let Some(v) = data["ratings"]["tmdb"]["value"]
        .as_f64()
        .or_else(|| data["ratings"]["douban"]["value"].as_f64())
    {
        p.insert("rating".into(), json!(v));
    }
    if let Some(v) = data["ids"]["tmdb"].as_u64() {
        p.insert("tmdb_id".into(), json!(v));
    }
    if let Some(v) = data["ids"]["imdb"].as_str() {
        p.insert("imdb_id".into(), json!(v));
    }
    Value::Object(p)
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

    #[test]
    fn metadata_to_group_fields() {
        let data = json!({ "titles": { "original": "The Matrix" }, "images": { "poster": "/media/metadata-tmdb/images/a.jpg" },
                           "overview": { "en-US": "Neo" }, "genres": ["scifi"], "countries": [], "runtime_min": 136,
                           "ratings": { "tmdb": { "value": 8.2 } }, "ids": { "tmdb": 603, "imdb": "tt0133093" } });
        let p = metadata_patch(&data, "zh-CN");
        assert_eq!(
            p,
            json!({ "original_title": "The Matrix", "poster": "/media/metadata-tmdb/images/a.jpg", "overview": "Neo",
                              "genres": ["scifi"], "runtime_min": 136, "rating": 8.2, "tmdb_id": 603, "imdb_id": "tt0133093" })
        );
    }
}
