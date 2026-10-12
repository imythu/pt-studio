//! 动漫类型的纯逻辑（type-anime LLD §1）：schema、规范化（字幕组风格发布名补全、字幕组别名、字幕标签、范围）、
//! 分组（Bangumi > AniDB > TMDB > 标题 + 年份，按季；剧场版不分季）、作品标题、规范标题、元数据补丁。

use pt_type_kit::{Errors, Template, episodes, specs};
use serde_json::{Map, Value, json};

use crate::name;

pub const MEDIA: &[&str] = &["tv", "movie", "ova", "ona", "special"];
pub const SUB_LANGS: &[&str] = &["chs", "cht", "jpn", "eng"];
pub const SUB_KINDS: &[&str] = &["softsub", "hardsub", "external", "none"];

pub const DEFAULT_TEMPLATE: &str = "{fansub?[|] }{title} {se} {resolution?[|]}{source?[|]}{video_codec?[|]}{bit_depth?[|bit]}{subtitle_langs?[|]}";

pub const FIELDS: &[&str] = &[
    "fansub",
    "title",
    "title_zh",
    "year",
    "se",
    "season",
    "episode",
    "episode_end",
    "media",
    "resolution",
    "source",
    "video_codec",
    "bit_depth",
    "audio_codecs",
    "subtitle_langs",
    "subtitle_kind",
];

const SPEC_FIELDS: &[&str] = &["resolution", "source", "video_codec", "audio_codecs"];

fn label(f: &str) -> String {
    format!("type-anime.field.{f}")
}

pub fn field_schema() -> Value {
    let mut p = Map::new();
    let s = |max: i64, f: &str| json!({ "type": "string", "maxLength": max, "x-pt-label-term": label(f) });
    p.insert("release_name".into(), s(300, "release_name"));
    for k in ["title", "title_zh"] {
        let mut v = s(200, k);
        v["x-pt-group-level"] = json!(true);
        v["x-pt-search"] = json!("searchable");
        p.insert(k.into(), v);
    }
    p.insert("year".into(), json!({ "type": "integer", "minimum": 1917, "maximum": 2100, "x-pt-label-term": label("year"), "x-pt-group-level": true }));
    for k in ["bangumi_id", "anidb_id", "tmdb_tv_id"] {
        p.insert(k.into(), json!({ "type": "integer", "minimum": 1, "x-pt-label-term": label(k), "x-pt-widget": "external-id", "x-pt-group-level": true }));
    }
    p.insert("media".into(), json!({ "type": "string", "enum": MEDIA, "default": "tv", "x-pt-label-term": label("media"), "x-pt-widget": "select", "x-pt-filter": "facet" }));
    p.insert("scope".into(), json!({ "type": "string", "enum": episodes::SCOPES, "x-pt-label-term": label("scope"), "x-pt-widget": "select", "x-pt-filter": "facet" }));
    p.insert("season".into(), json!({ "type": "integer", "minimum": 0, "maximum": 999, "x-pt-label-term": label("season"), "x-pt-group-level": true }));
    p.insert(
        "episode".into(),
        json!({ "type": "integer", "minimum": 0, "maximum": 9999, "x-pt-label-term": label("episode") }),
    );
    p.insert(
        "episode_end".into(),
        json!({ "type": "integer", "minimum": 2, "maximum": 9999, "x-pt-label-term": label("episode_end") }),
    );
    p.insert(
        "ongoing".into(),
        json!({ "type": "boolean", "x-pt-label-term": label("ongoing") }),
    );
    let mut fansub = s(60, "fansub");
    fansub["x-pt-filter"] = json!("facet");
    p.insert("fansub".into(), fansub);
    p.insert(
        "subtitle_langs".into(),
        json!({ "type": "array", "items": { "type": "string", "enum": SUB_LANGS }, "uniqueItems": true,
        "x-pt-label-term": label("subtitle_langs"), "x-pt-widget": "multiselect", "x-pt-filter": "facet" }),
    );
    p.insert("subtitle_kind".into(), json!({ "type": "string", "enum": SUB_KINDS, "x-pt-label-term": label("subtitle_kind"), "x-pt-widget": "select", "x-pt-filter": "facet" }));
    p.insert("bit_depth".into(), json!({ "type": "integer", "enum": [8, 10], "x-pt-label-term": label("bit_depth"), "x-pt-widget": "select", "x-pt-filter": "facet" }));
    for (k, v) in specs::spec_schema(&label) {
        if SPEC_FIELDS.contains(&k.as_str()) {
            p.insert(k, v);
        }
    }
    json!({ "type": "object", "additionalProperties": false, "properties": p,
            "x-pt-order": ["release_name", "title", "title_zh", "year", "bangumi_id", "anidb_id", "tmdb_tv_id", "media", "scope", "season",
                           "episode", "episode_end", "ongoing", "fansub", "subtitle_langs", "subtitle_kind", "resolution", "source",
                           "video_codec", "bit_depth", "audio_codecs"] })
}

pub fn group_schema() -> Value {
    let f = field_schema();
    let mut p = Map::new();
    for k in [
        "title",
        "title_zh",
        "year",
        "season",
        "bangumi_id",
        "anidb_id",
        "tmdb_tv_id",
    ] {
        p.insert(k.into(), f["properties"][k].clone());
    }
    for (k, ty) in [
        ("poster", json!({ "type": "string", "x-pt-widget": "image" })),
        ("overview", json!({ "type": "string" })),
        (
            "genres",
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
        let mut v = ty;
        v["x-pt-label-term"] = json!(label(k));
        v["x-pt-from-metadata"] = json!(true);
        p.insert(k.into(), v);
    }
    json!({ "type": "object", "properties": p,
            "x-pt-order": ["poster", "title_zh", "title", "original_title", "season", "year", "status", "episode_count", "genres", "rating",
                           "bangumi_id", "anidb_id", "tmdb_tv_id", "overview"] })
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

/// 字幕组别名（设置 `fansub_aliases`：别名 → 规范名，大小写不敏感；规范名本身也按大小写不敏感归一）
pub fn canonical_fansub(raw: &str, aliases: &Map<String, Value>) -> String {
    let l = raw.trim().to_lowercase();
    for (alias, canon) in aliases {
        if let Some(c) = canon.as_str()
            && (alias.to_lowercase() == l || c.to_lowercase() == l)
        {
            return c.to_owned();
        }
    }
    raw.trim().to_owned()
}

fn bangumi_id(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| {
        let s = v.as_str()?.trim();
        let tail = s.split("/subject/").nth(1).unwrap_or(s);
        tail.trim_end_matches('/')
            .split(['/', '?', '#'])
            .next()?
            .parse()
            .ok()
    })
}

fn anidb_id(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| {
        let s = v.as_str()?.trim();
        if let Some(p) = s.find("aid=") {
            return s[p + 4..].split('&').next()?.parse().ok();
        }
        let tail = s.split("/anime/").nth(1).unwrap_or(s);
        tail.trim_end_matches('/')
            .split(['/', '?', '#'])
            .next()?
            .parse()
            .ok()
    })
}

fn fill(o: &mut Map<String, Value>, k: &str, v: Option<Value>) {
    if let Some(v) = v {
        o.entry(k).or_insert(v);
    }
}

pub fn normalize(input: &Value, aliases: &Map<String, Value>) -> Result<Value, Errors> {
    let mut errs = Errors::default();
    let mut o = Map::new();
    let get = |k: &str| input.get(k).filter(|v| !v.is_null() && v.as_str() != Some(""));
    for (k, max) in [("title", 200), ("title_zh", 200), ("release_name", 300)] {
        if let Some(s) = get(k).and_then(|v| text(v, max)) {
            o.insert(k.into(), json!(s));
        }
    }
    if let Some(f) = get("fansub").and_then(|v| text(v, 60)) {
        o.insert("fansub".into(), json!(canonical_fansub(&f, aliases)));
    }
    if let Some(v) = get("year") {
        match v
            .as_i64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        {
            Some(y) if (1917..=2100).contains(&y) => {
                o.insert("year".into(), json!(y));
            }
            _ => errs.add(
                "year",
                "type-anime.field_invalid",
                "year must be between 1917 and 2100",
            ),
        }
    }
    for (k, parse) in [
        ("bangumi_id", bangumi_id as fn(&Value) -> Option<u64>),
        ("anidb_id", anidb_id),
    ] {
        if let Some(v) = get(k) {
            match parse(v) {
                Some(id) if id > 0 => {
                    o.insert(k.into(), json!(id));
                }
                _ => errs.add(k, "type-anime.field_invalid", "not an id or link"),
            }
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
            None => errs.add("tmdb_tv_id", "type-anime.field_invalid", "not a TMDB id or link"),
        }
    }
    let media = match get("media").and_then(Value::as_str) {
        None => "tv",
        Some(m) if MEDIA.contains(&m) => m,
        Some(_) => {
            errs.add("media", "type-anime.field_invalid", "unknown media");
            "tv"
        }
    };
    o.insert("media".into(), json!(media));
    specs::normalize_specs(input, &mut o, &mut errs, "type-anime", SPEC_FIELDS);
    episodes::normalize_episode_fields(input, &mut o, &mut errs, "type-anime");
    if let Some(v) = get("subtitle_langs") {
        let xs: Vec<String> = match v {
            Value::Array(a) => a.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
            Value::String(s) => s
                .split(',')
                .map(|x| x.trim().to_lowercase())
                .filter(|x| !x.is_empty())
                .collect(),
            _ => vec![],
        };
        let mut out: Vec<String> = Vec::new();
        for x in xs {
            // 允许直接填标签（“简繁”“CHS”）
            let mapped: Vec<String> = if SUB_LANGS.contains(&x.as_str()) {
                vec![x.clone()]
            } else {
                name::subtitle_tag(&x)
                    .map(|(l, _)| l.into_iter().map(str::to_owned).collect())
                    .unwrap_or_default()
            };
            if mapped.is_empty() {
                errs.add(
                    "subtitle_langs",
                    "type-anime.subtitle_invalid",
                    "unknown subtitle language",
                );
            }
            for m in mapped {
                if !out.contains(&m) {
                    out.push(m);
                }
            }
        }
        o.insert("subtitle_langs".into(), json!(out));
    }
    if let Some(v) = get("subtitle_kind").and_then(Value::as_str) {
        if SUB_KINDS.contains(&v) {
            o.insert("subtitle_kind".into(), json!(v));
        } else {
            errs.add(
                "subtitle_kind",
                "type-anime.subtitle_invalid",
                "unknown subtitle kind",
            );
        }
    }
    if let Some(v) = get("bit_depth") {
        match v.as_i64().or_else(|| {
            v.as_str()
                .and_then(|s| s.trim().trim_end_matches("bit").parse().ok())
        }) {
            Some(b @ (8 | 10)) => {
                o.insert("bit_depth".into(), json!(b));
            }
            _ => errs.add(
                "bit_depth",
                "type-anime.field_invalid",
                "bit depth must be 8 or 10",
            ),
        }
    }
    // 发布名补全（用户填写优先）
    let mut batch = false;
    if let Some(r) = o.get("release_name").and_then(Value::as_str).map(str::to_owned) {
        let n = name::parse(&r);
        batch = n.batch;
        fill(&mut o, "title", n.title.map(|x| json!(x)));
        fill(&mut o, "title_zh", n.title_zh.map(|x| json!(x)));
        fill(
            &mut o,
            "fansub",
            n.fansub.map(|x| json!(canonical_fansub(&x, aliases))),
        );
        fill(&mut o, "season", n.season.map(|x| json!(x)));
        if !o.contains_key("episode") {
            fill(&mut o, "episode", n.episode.map(|x| json!(x)));
            fill(&mut o, "episode_end", n.episode_end.map(|x| json!(x)));
        }
        fill(&mut o, "resolution", n.resolution.map(|x| json!(x)));
        fill(&mut o, "source", n.source.map(|x| json!(x)));
        fill(&mut o, "video_codec", n.video_codec.map(|x| json!(x)));
        fill(&mut o, "bit_depth", n.bit_depth.map(|x| json!(x)));
        if !n.audio_codecs.is_empty() {
            fill(&mut o, "audio_codecs", Some(json!(n.audio_codecs)));
        }
        if !n.subtitle_langs.is_empty() {
            fill(&mut o, "subtitle_langs", Some(json!(n.subtitle_langs)));
        }
        fill(&mut o, "subtitle_kind", n.subtitle_kind.map(|x| json!(x)));
    }
    if media == "movie" {
        // 剧场版：不分季集
        for k in ["season", "episode", "episode_end", "scope", "ongoing"] {
            o.remove(k);
        }
    } else {
        // 合集标记且未给范围：有区间的视为季包
        let mut scope_input = input.clone();
        if batch && get("scope").is_none() && o.contains_key("episode_end") {
            o.remove("episode");
            o.remove("episode_end");
            scope_input["scope"] = json!("season_pack");
        }
        episodes::normalize_scope(&scope_input, &mut o, &mut errs, "type-anime");
    }
    if !o.contains_key("title")
        && !o.contains_key("title_zh")
        && !["bangumi_id", "anidb_id", "tmdb_tv_id"]
            .iter()
            .any(|k| o.contains_key(*k))
    {
        errs.add(
            "title",
            "type-anime.external_id_required",
            "title or an external id is required",
        );
    }
    if errs.is_empty() {
        let g = group_title(&Value::Object(o.clone()));
        o.insert("group_title".into(), json!(g));
        Ok(Value::Object(o))
    } else {
        Err(errs)
    }
}

/// `anime:<id 方案>[:s<季>]`
pub fn group_key(f: &Value) -> Option<String> {
    let base = if let Some(id) = f["bangumi_id"].as_u64() {
        format!("bangumi:{id}")
    } else if let Some(id) = f["anidb_id"].as_u64() {
        format!("anidb:{id}")
    } else if let Some(id) = f["tmdb_tv_id"].as_u64() {
        format!("tmdb:{id}")
    } else {
        let t = f["title"].as_str().or_else(|| f["title_zh"].as_str())?;
        format!(
            "title:{}:{}",
            pt_type_kit::norm_key(t),
            f["year"].as_i64().map(|y| y.to_string()).unwrap_or_default()
        )
    };
    // Bangumi / AniDB 条目本身按季（每季一个条目），不再拼季号
    let per_season = f["bangumi_id"].is_null() && f["anidb_id"].is_null();
    Some(match f["season"].as_i64() {
        Some(s) if per_season && f["media"] != "movie" => format!("anime:{base}:s{s:02}"),
        _ => format!("anime:{base}"),
    })
}

/// “中文名 / 原名 第 N 季”（第 1 季不写季）
pub fn group_title(f: &Value) -> String {
    let title = f["title"].as_str().unwrap_or("").trim();
    let zh = f["title_zh"].as_str().filter(|z| !z.is_empty() && *z != title);
    let names = match (zh, title.is_empty()) {
        (Some(z), false) => format!("{z} / {title}"),
        (Some(z), true) => z.to_owned(),
        (None, _) => title.to_owned(),
    };
    match (f["media"].as_str(), f["season"].as_i64()) {
        (Some("movie"), _) => format!("{names} 剧场版"),
        (_, Some(0)) => format!("{names} 全集"),
        (_, Some(s)) if s > 1 => format!("{names} 第{s}季"),
        _ => names,
    }
}

pub fn display(field: &str, v: &Value) -> Option<String> {
    match (field, v.as_str()) {
        ("subtitle_langs", Some(x)) => Some(x.to_uppercase()),
        ("se", _) | ("fansub", _) | ("title", _) | ("title_zh", _) => pt_type_kit::plain(v),
        _ => specs::display(field, v),
    }
}

pub fn canonical_title(f: &Value, template: Option<&str>) -> String {
    let mut v = f.clone();
    if f["media"] != "movie" {
        v["se"] = json!(episodes::se(f));
    }
    if v["title"].is_null() {
        v["title"] = f["title_zh"].clone();
    }
    let tpl = template
        .and_then(|t| Template::parse(t, Some(FIELDS)).ok())
        .or_else(|| Template::parse(DEFAULT_TEMPLATE, Some(FIELDS)).ok());
    tpl.map(|t| t.render(&v, &display)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aliases() -> Map<String, Value> {
        json!({ "Nekomoe kissaten": "喵萌奶茶屋", "ani": "ANi" })
            .as_object()
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn fansub_episode() {
        let f = normalize(&json!({ "release_name": "[ani] 葬送的芙莉莲 / Sousou no Frieren - 05 [1080P][Baha][WEB-DL][AAC AVC][CHT].mp4" }), &aliases())
            .unwrap_or_else(|e| panic!("{:?}", e.details()));
        assert_eq!(
            (
                f["fansub"].as_str(),
                f["scope"].as_str(),
                f["episode"].as_i64(),
                f["season"].as_i64()
            ),
            (Some("ANi"), Some("episode"), Some(5), Some(1))
        );
        assert_eq!(f["group_title"], "葬送的芙莉莲 / Sousou no Frieren");
        assert_eq!(
            group_key(&f).as_deref(),
            Some("anime:title:sousou-no-frieren::s01")
        );
        assert_eq!(
            canonical_title(&f, None),
            "[ANi] Sousou no Frieren S01E05 [1080p][WEB-DL][AVC][CHT]"
        );
    }

    #[test]
    fn batch_and_ids() {
        let f = normalize(
            &json!({ "release_name": "[Nekomoe kissaten][Kimetsu no Yaiba][01-26][BDRip][合集][1080p][JPSC]", "bangumi_id": "https://bgm.tv/subject/206382" }),
            &aliases(),
        )
        .unwrap_or_else(|e| panic!("{:?}", e.details()));
        assert_eq!(
            (
                f["fansub"].as_str(),
                f["scope"].as_str(),
                f.get("episode").is_none()
            ),
            (Some("喵萌奶茶屋"), Some("season_pack"), true)
        );
        assert_eq!(f["subtitle_langs"], json!(["chs", "jpn"]));
        assert_eq!(group_key(&f).as_deref(), Some("anime:bangumi:206382"));
        let m = normalize(&json!({ "title_zh": "铃芽之旅", "media": "movie", "anidb_id": "https://anidb.net/anime/17263", "episode": 3,
                                    "resolution": "1080p", "subtitle_langs": "简繁" }), &aliases()).unwrap_or_default();
        assert_eq!(
            (m.get("episode").is_none(), m["group_title"].as_str()),
            (true, Some("铃芽之旅 剧场版"))
        );
        assert_eq!(m["subtitle_langs"], json!(["chs", "cht"]));
        assert_eq!(group_key(&m).as_deref(), Some("anime:anidb:17263"));
    }

    #[test]
    fn invalid() {
        let e = normalize(
            &json!({ "media": "drama", "subtitle_langs": ["klingon"], "bit_depth": 12 }),
            &Map::new(),
        )
        .err()
        .unwrap_or_default();
        let codes: Vec<String> = e.details()["errors"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x["code"].as_str().map(str::to_owned))
            .collect();
        assert!(
            codes.contains(&"type-anime.subtitle_invalid".to_owned())
                && codes.contains(&"type-anime.external_id_required".to_owned()),
            "{codes:?}"
        );
    }
}
