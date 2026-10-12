//! 剧集类型的纯逻辑（type-tv LLD §1）：schema、规范化（季集与范围推断）、按季分组、作品标题、规范标题、元数据补丁。

use pt_type_kit::{Errors, Template, specs};
use serde_json::{Map, Value, json};

pub use pt_type_kit::episodes::SCOPES;

pub const DEFAULT_TEMPLATE: &str = "{title} {se} {resolution} {release_type?| }{source} {hdr?| }{video_codec} {audio_codecs?| }-{release_group}";

const SPEC_FIELDS: &[&str] = &[
    "resolution",
    "source",
    "release_type",
    "video_codec",
    "audio_codecs",
    "hdr",
];

fn label(f: &str) -> String {
    format!("type-tv.field.{f}")
}

pub fn field_schema() -> Value {
    let mut p = Map::new();
    p.insert(
        "release_name".into(),
        json!({ "type": "string", "maxLength": 300, "x-pt-label-term": label("release_name") }),
    );
    p.insert("title".into(), json!({ "type": "string", "maxLength": 200, "x-pt-label-term": label("title"), "x-pt-group-level": true, "x-pt-search": "searchable" }));
    p.insert("title_zh".into(), json!({ "type": "string", "maxLength": 200, "x-pt-label-term": label("title_zh"), "x-pt-group-level": true, "x-pt-search": "searchable" }));
    p.insert("first_air_year".into(), json!({ "type": "integer", "minimum": 1920, "maximum": 2100, "x-pt-label-term": label("first_air_year"), "x-pt-group-level": true }));
    p.insert("tmdb_tv_id".into(), json!({ "type": "integer", "minimum": 1, "x-pt-label-term": label("tmdb_tv_id"), "x-pt-widget": "external-id", "x-pt-group-level": true }));
    p.insert("imdb_id".into(), json!({ "type": "string", "pattern": "^tt[0-9]{7,9}$", "x-pt-label-term": label("imdb_id"), "x-pt-widget": "external-id", "x-pt-group-level": true }));
    p.insert("douban_id".into(), json!({ "type": "string", "pattern": "^[0-9]{1,12}$", "x-pt-label-term": label("douban_id"), "x-pt-widget": "external-id", "x-pt-group-level": true }));
    p.insert("scope".into(), json!({ "type": "string", "enum": SCOPES, "x-pt-label-term": label("scope"), "x-pt-widget": "select", "x-pt-filter": "facet" }));
    p.insert("season".into(), json!({ "type": "integer", "minimum": 0, "maximum": 999, "x-pt-label-term": label("season"), "x-pt-group-level": true }));
    p.insert(
        "episode".into(),
        json!({ "type": "integer", "minimum": 1, "maximum": 9999, "x-pt-label-term": label("episode") }),
    );
    p.insert(
        "episode_end".into(),
        json!({ "type": "integer", "minimum": 2, "maximum": 9999, "x-pt-label-term": label("episode_end") }),
    );
    p.insert(
        "ongoing".into(),
        json!({ "type": "boolean", "x-pt-label-term": label("ongoing") }),
    );
    p.insert("network".into(), json!({ "type": "string", "maxLength": 40, "x-pt-label-term": label("network"), "x-pt-filter": "facet" }));
    for (k, v) in specs::spec_schema(&label) {
        p.insert(k, v);
    }
    json!({ "type": "object", "additionalProperties": false, "properties": p,
            "x-pt-order": ["release_name", "title", "title_zh", "first_air_year", "tmdb_tv_id", "imdb_id", "douban_id", "scope", "season",
                           "episode", "episode_end", "ongoing", "network", "resolution", "source", "release_type", "video_codec",
                           "audio_codecs", "hdr", "release_group"] })
}

pub fn group_schema() -> Value {
    let f = field_schema();
    let mut p = Map::new();
    for k in [
        "title",
        "title_zh",
        "first_air_year",
        "season",
        "tmdb_tv_id",
        "imdb_id",
        "douban_id",
    ] {
        p.insert(k.into(), f["properties"][k].clone());
    }
    let meta = |ty: Value, k: &str| {
        let mut v = ty;
        v["x-pt-label-term"] = json!(label(k));
        v["x-pt-from-metadata"] = json!(true);
        v
    };
    for (k, ty) in [
        ("poster", json!({ "type": "string", "x-pt-widget": "image" })),
        ("overview", json!({ "type": "string" })),
        (
            "genres",
            json!({ "type": "array", "items": { "type": "string" } }),
        ),
        (
            "countries",
            json!({ "type": "array", "items": { "type": "string" } }),
        ),
        ("episode_count", json!({ "type": "integer" })),
        (
            "status",
            json!({ "type": "string", "enum": ["returning", "ended", "canceled"] }),
        ),
        ("rating", json!({ "type": "number" })),
        ("original_title", json!({ "type": "string" })),
    ] {
        p.insert(k.into(), meta(ty, k));
    }
    json!({ "type": "object", "properties": p,
            "x-pt-order": ["poster", "title", "title_zh", "original_title", "season", "first_air_year", "status", "episode_count", "genres",
                           "countries", "rating", "tmdb_tv_id", "imdb_id", "douban_id", "overview"] })
}

fn text(v: &Value, max: usize) -> Option<String> {
    let s: String = v.as_str()?.trim().chars().filter(|c| !c.is_control()).collect();
    (!s.is_empty()).then(|| s.chars().take(max).collect())
}

fn int(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

pub fn normalize(input: &Value) -> Result<Value, Errors> {
    let mut errs = Errors::default();
    let mut o = Map::new();
    let get = |k: &str| input.get(k).filter(|v| !v.is_null() && v.as_str() != Some(""));
    for (k, max) in [
        ("title", 200),
        ("title_zh", 200),
        ("release_group", 40),
        ("network", 40),
    ] {
        if let Some(s) = get(k).and_then(|v| text(v, max)) {
            o.insert(k.into(), json!(s));
        }
    }
    let release_name = get("release_name").and_then(|v| text(v, 300));
    if let Some(r) = &release_name {
        o.insert("release_name".into(), json!(r));
    }
    if let Some(v) = get("first_air_year").or_else(|| get("year")) {
        match int(v) {
            Some(y) if (1920..=2100).contains(&y) => {
                o.insert("first_air_year".into(), json!(y));
            }
            _ => errs.add(
                "first_air_year",
                "type-tv.field_invalid",
                "year must be between 1920 and 2100",
            ),
        }
    }
    if let Some(v) = get("tmdb_tv_id") {
        match v
            .as_u64()
            .or_else(|| v.as_str().and_then(|s| pt_type_kit::tmdb_id(s, "tv")))
        {
            Some(id) => {
                o.insert("tmdb_tv_id".into(), json!(id));
            }
            None => errs.add("tmdb_tv_id", "type-tv.field_invalid", "not a TMDB id or link"),
        }
    }
    if let Some(v) = get("imdb_id") {
        match v.as_str().and_then(pt_type_kit::imdb_id) {
            Some(id) => {
                o.insert("imdb_id".into(), json!(id));
            }
            None => errs.add("imdb_id", "type-tv.field_invalid", "not an IMDb id or link"),
        }
    }
    if let Some(v) = get("douban_id") {
        match v
            .as_str()
            .map(str::to_owned)
            .or_else(|| v.as_u64().map(|n| n.to_string()))
            .as_deref()
            .and_then(pt_type_kit::douban_id)
        {
            Some(id) => {
                o.insert("douban_id".into(), json!(id));
            }
            None => errs.add("douban_id", "type-tv.field_invalid", "not a Douban id or link"),
        }
    }
    specs::normalize_specs(input, &mut o, &mut errs, "type-tv", SPEC_FIELDS);
    pt_type_kit::episodes::normalize_episode_fields(input, &mut o, &mut errs, "type-tv");
    // 发布名补全：标题、季集、规格
    if let Some(r) = &release_name {
        let p = specs::fill_from_release(&mut o, r);
        if !o.contains_key("title")
            && let Some(t) = pt_release_parser::parse_title(r)
        {
            o.insert("title".into(), json!(t));
        }
        if !o.contains_key("season")
            && let Some(s) = p.season
        {
            o.insert("season".into(), json!(s));
        }
        if !o.contains_key("episode")
            && let Some(e) = p.episode
        {
            o.insert("episode".into(), json!(e));
            if let Some(e2) = p.episode_end.filter(|e2| *e2 > e) {
                o.entry("episode_end").or_insert(json!(e2));
            }
        }
        if !o.contains_key("first_air_year")
            && let Some(y) = p.year
        {
            o.insert("first_air_year".into(), json!(y));
        }
    }
    pt_type_kit::episodes::normalize_scope(input, &mut o, &mut errs, "type-tv");
    if !o.contains_key("title") && !o.contains_key("tmdb_tv_id") && !o.contains_key("imdb_id") {
        errs.add(
            "title",
            "type-tv.external_id_required",
            "title or an external id is required",
        );
    }
    for k in ["resolution", "source"] {
        if !o.contains_key(k) {
            errs.add(k, &format!("type-tv.{k}_invalid"), "required");
        }
    }
    if errs.is_empty() {
        let g = group_title(&Value::Object(o.clone()));
        o.insert("group_title".into(), json!(g));
        Ok(Value::Object(o))
    } else {
        Err(errs)
    }
}

/// `tv:<id 方案>:s<季>`（全集为 s00）
pub fn group_key(f: &Value) -> Option<String> {
    let season = f["season"].as_i64().unwrap_or(1);
    let base = if let Some(id) = f["tmdb_tv_id"].as_u64() {
        format!("tmdb:{id}")
    } else if let Some(id) = f["imdb_id"].as_str() {
        format!("imdb:{id}")
    } else if let Some(id) = f["douban_id"].as_str() {
        format!("douban:{id}")
    } else {
        let t = f["title"].as_str()?;
        format!(
            "title:{}:{}",
            pt_type_kit::norm_key(t),
            f["first_air_year"]
                .as_i64()
                .map(|y| y.to_string())
                .unwrap_or_default()
        )
    };
    Some(format!("tv:{base}:s{season:02}"))
}

/// “中文名 / 原名 第 N 季 / Season N”
pub fn group_title(f: &Value) -> String {
    let title = f["title"].as_str().unwrap_or("").trim();
    let zh = f["title_zh"].as_str().filter(|z| !z.is_empty() && *z != title);
    let season = f["season"].as_i64().unwrap_or(1);
    let suffix = if season > 0 {
        format!(" 第{season}季 / Season {season}")
    } else {
        " 全集 / Complete".into()
    };
    format!(
        "{}{title}{suffix}",
        zh.map(|z| format!("{z} / ")).unwrap_or_default()
    )
}

pub use pt_type_kit::episodes::se;

/// 标题模板可用的字段（含派生的 `se`）
pub const FIELDS: &[&str] = &[
    "title",
    "title_zh",
    "first_air_year",
    "se",
    "season",
    "episode",
    "episode_end",
    "network",
    "resolution",
    "source",
    "release_type",
    "video_codec",
    "audio_codecs",
    "hdr",
    "release_group",
];

pub fn canonical_title(f: &Value, template: Option<&str>) -> String {
    let mut v = f.clone();
    v["se"] = json!(se(f));
    let tpl = template
        .and_then(|t| Template::parse(t, Some(FIELDS)).ok())
        .or_else(|| Template::parse(DEFAULT_TEMPLATE, Some(FIELDS)).ok());
    tpl.map(|t| t.render(&v, &specs::display)).unwrap_or_default()
}

pub use pt_type_kit::metadata::show_patch as metadata_patch;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn season_pack_from_release_name() {
        let f = normalize(&json!({ "release_name": "The.Last.Show.S02.2160p.WEB-DL.DDP5.1.H.265-NTb", "title_zh": "最后的剧" }))
            .unwrap_or_else(|e| panic!("{:?}", e.details()));
        assert_eq!(
            (f["title"].as_str(), f["season"].as_i64(), f["scope"].as_str()),
            (Some("The Last Show"), Some(2), Some("season_pack"))
        );
        assert_eq!(f["video_codec"], "hevc");
        assert_eq!(f["group_title"], "最后的剧 / The Last Show 第2季 / Season 2");
        assert_eq!(group_key(&f).as_deref(), Some("tv:title:the-last-show::s02"));
        assert_eq!(
            canonical_title(&f, None),
            "The Last Show S02 2160p WEB-DL HEVC DDP -NTb"
        );
    }

    #[test]
    fn episodes_and_ids() {
        let f = normalize(&json!({ "release_name": "Show.S01E05-E06.1080p.HDTV.x264-GRP", "tmdb_tv_id": "https://www.themoviedb.org/tv/1399-x" }))
            .unwrap_or_else(|e| panic!("{:?}", e.details()));
        assert_eq!(
            (
                f["scope"].as_str(),
                f["episode"].as_i64(),
                f["episode_end"].as_i64()
            ),
            (Some("episodes"), Some(5), Some(6))
        );
        assert_eq!(group_key(&f).as_deref(), Some("tv:tmdb:1399:s01"));
        assert_eq!(se(&f), "S01E05-E06");
        let c = normalize(&json!({ "title": "X", "scope": "complete", "season": 3, "episode": 2, "resolution": "720p", "source": "dvd" })).unwrap_or_default();
        assert_eq!(
            (c["season"].as_i64(), c.get("episode").is_none()),
            (Some(0), true)
        );
        assert_eq!(group_key(&c).as_deref(), Some("tv:title:x::s00"));
    }

    #[test]
    fn invalid() {
        let e = normalize(&json!({ "title": "X", "scope": "episodes", "episode": 5, "episode_end": 3, "resolution": "1080p", "source": "web" })).err().unwrap_or_default();
        assert_eq!(e.first_field(), Some("episode_end"));
        let e = normalize(&json!({ "scope": "weekly" })).err().unwrap_or_default();
        assert!(e.details()["errors"].as_array().is_some_and(|a| a.len() >= 3));
    }

    #[test]
    fn metadata() {
        let show = json!({ "titles": { "original": "Game of Thrones", "localized": { "zh-CN": "权力的游戏" } }, "images": { "poster": "/media/a.jpg" },
                           "overview": { "zh-CN": "show" }, "genres": ["drama"], "extra": { "status": "ended" }, "ids": { "tmdb": 1399, "imdb": "tt0944947" } });
        let season = json!({ "images": { "poster": "/media/s1.jpg" }, "overview": { "zh-CN": "season 1" }, "episode_count": 10 });
        let p = metadata_patch(&show, Some(&season), "zh-CN");
        assert_eq!(p["poster"], "/media/s1.jpg");
        assert_eq!(p["overview"], "season 1");
        assert_eq!(p["episode_count"], 10);
        assert_eq!(p["status"], "ended");
        assert_eq!(p["title_zh"], "权力的游戏");
    }
}
