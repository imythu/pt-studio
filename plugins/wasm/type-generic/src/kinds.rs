//! kind 定义：校验、编译为 JSON Schema、规范化、分组键、规范标题（LLD §1.1～§1.4）。纯函数。

use std::collections::BTreeMap;

use pt_type_kit::{Errors, Template};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Opt {
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_zh: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_en: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldDef {
    pub key: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_len: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<Opt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default)]
    pub group_level: bool,
    #[serde(default = "none")]
    pub filter: String,
    #[serde(default = "none")]
    pub search: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_zh: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label_en: Option<String>,
    #[serde(default)]
    pub deprecated: bool,
}

fn none() -> String {
    "none".into()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FileRules {
    #[serde(default)]
    pub allowed_extensions: Vec<String>,
    #[serde(default)]
    pub forbidden_extensions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KindDef {
    #[serde(default = "one")]
    pub schema: u32,
    pub kind: String,
    #[serde(default)]
    pub name_zh: Option<String>,
    #[serde(default)]
    pub name_en: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    pub fields: Vec<FieldDef>,
    #[serde(default)]
    pub group_fields: Vec<String>,
    #[serde(default)]
    pub group_fallback_fields: Vec<String>,
    #[serde(default)]
    pub title_template: Option<String>,
    #[serde(default)]
    pub files: FileRules,
}

fn one() -> u32 {
    1
}

const TYPES: &[&str] = &[
    "string",
    "integer",
    "number",
    "enum",
    "multi_enum",
    "bool",
    "date",
    "url",
    "external_id",
];
const RESERVED: &[&str] = &["kind", "release_name", "description"];

fn ident_ok(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

/// 校验 kind 定义（`type-generic.kind_definition_invalid`，details.errors[]）
pub fn validate(d: &KindDef, max_fields: usize) -> Result<(), Errors> {
    let mut e = Errors::default();
    if !ident_ok(&d.kind) {
        e.add(
            "kind",
            "kernel.validation_failed",
            "kind must match ^[a-z][a-z0-9_]{0,31}$",
        );
    }
    if d.fields.is_empty() || d.fields.len() > max_fields {
        e.add(
            "fields",
            "type-generic.limit_exceeded",
            &format!("1..={max_fields} fields are allowed"),
        );
    }
    let mut seen = Vec::new();
    for (i, f) in d.fields.iter().enumerate() {
        let at = format!("fields/{i}");
        if !ident_ok(&f.key) || RESERVED.contains(&f.key.as_str()) {
            e.add(
                &format!("{at}/key"),
                "kernel.validation_failed",
                "invalid or reserved field key",
            );
        }
        if seen.contains(&f.key) {
            e.add(
                &format!("{at}/key"),
                "kernel.validation_failed",
                "duplicate field key",
            );
        }
        seen.push(f.key.clone());
        if !TYPES.contains(&f.ty.as_str()) {
            e.add(
                &format!("{at}/type"),
                "kernel.validation_failed",
                "unknown field type",
            );
        }
        if matches!(f.ty.as_str(), "enum" | "multi_enum") && (f.options.is_empty() || f.options.len() > 100) {
            e.add(
                &format!("{at}/options"),
                "kernel.validation_failed",
                "1..=100 options are required",
            );
        }
        if f.max_len.is_some_and(|m| m == 0 || m > 2000) {
            e.add(
                &format!("{at}/max_len"),
                "kernel.validation_failed",
                "max_len must be 1..=2000",
            );
        }
        if !matches!(f.filter.as_str(), "facet" | "range" | "none")
            || !matches!(f.search.as_str(), "searchable" | "exact" | "none")
        {
            e.add(
                &format!("{at}/filter"),
                "kernel.validation_failed",
                "filter must be facet|range|none, search searchable|exact|none",
            );
        }
        if f.filter == "range" && !matches!(f.ty.as_str(), "integer" | "number" | "date") {
            e.add(
                &format!("{at}/filter"),
                "kernel.validation_failed",
                "range filters need a numeric or date field",
            );
        }
    }
    for g in d.group_fields.iter().chain(&d.group_fallback_fields) {
        if !d.fields.iter().any(|f| &f.key == g && f.group_level) {
            e.add(
                "group_fields",
                "kernel.validation_failed",
                &format!("`{g}` is not a group-level field"),
            );
        }
    }
    if let Some(t) = &d.title_template {
        let keys: Vec<&str> = d.fields.iter().map(|f| f.key.as_str()).collect();
        if let Err(msg) = Template::parse(t, Some(&keys)) {
            e.add("title_template", "kernel.validation_failed", &msg);
        }
    }
    if e.is_empty() { Ok(()) } else { Err(e) }
}

fn title_of(zh: Option<&String>, en: Option<&String>, fallback: &str) -> String {
    zh.or(en).cloned().unwrap_or_else(|| fallback.to_owned())
}

/// 编译为 JSON Schema（带 x-pt-* 扩展，content-types-common §4）
pub fn compile(d: &KindDef) -> Value {
    let mut props = Map::new();
    props.insert(
        "kind".into(),
        json!({ "const": d.kind, "x-pt-label-term": "type-generic.field.kind" }),
    );
    let mut required = vec![json!("kind")];
    for f in d.fields.iter().filter(|f| !f.deprecated) {
        let mut s = match f.ty.as_str() {
            "integer" => json!({ "type": "integer" }),
            "number" => json!({ "type": "number" }),
            "bool" => json!({ "type": "boolean" }),
            "date" => json!({ "type": "string", "format": "date" }),
            "url" => json!({ "type": "string", "format": "uri", "maxLength": 500, "pattern": "^https://" }),
            "enum" => {
                json!({ "type": "string", "enum": f.options.iter().map(|o| o.value.clone()).collect::<Vec<_>>() })
            }
            "multi_enum" => {
                json!({ "type": "array", "uniqueItems": true, "items": { "type": "string", "enum": f.options.iter().map(|o| o.value.clone()).collect::<Vec<_>>() } })
            }
            "external_id" => {
                json!({ "type": "string", "maxLength": 64, "x-pt-widget": "external-id", "x-pt-source": f.source })
            }
            _ => json!({ "type": "string", "maxLength": f.max_len.unwrap_or(200) }),
        };
        let o = s.as_object_mut().unwrap_or_else(|| unreachable!());
        if let Some(m) = f.min {
            o.insert("minimum".into(), json!(m));
        }
        if let Some(m) = f.max {
            o.insert("maximum".into(), json!(m));
        }
        o.insert(
            "title".into(),
            json!(title_of(f.label_zh.as_ref(), f.label_en.as_ref(), &f.key)),
        );
        o.insert(
            "x-pt-label-term".into(),
            json!(format!("type-generic.field.{}.{}", d.kind, f.key)),
        );
        o.insert("x-pt-filter".into(), json!(f.filter));
        o.insert("x-pt-search".into(), json!(f.search));
        if f.group_level {
            o.insert("x-pt-group-level".into(), json!(true));
        }
        if f.required {
            required.push(json!(f.key));
        }
        props.insert(f.key.clone(), s);
    }
    json!({
        "type": "object", "title": title_of(d.name_zh.as_ref(), d.name_en.as_ref(), &d.kind),
        "required": required, "properties": props, "additionalProperties": false,
        "x-pt-order": d.fields.iter().filter(|f| !f.deprecated).map(|f| f.key.clone()).collect::<Vec<_>>(),
    })
}

/// 字段级规范化与校验；`vocab` 为旧值 → 新值映射（field → alias → value）
pub fn normalize(
    d: &KindDef,
    input: &Value,
    vocab: &BTreeMap<String, BTreeMap<String, String>>,
) -> Result<Value, Errors> {
    let mut e = Errors::default();
    let mut o = Map::new();
    o.insert("kind".into(), json!(d.kind));
    if let Some(r) = input.get("release_name").and_then(Value::as_str) {
        o.insert(
            "release_name".into(),
            json!(r.trim().chars().take(300).collect::<String>()),
        );
    }
    for f in d.fields.iter().filter(|f| !f.deprecated) {
        let raw = input
            .get(&f.key)
            .filter(|v| !v.is_null() && v.as_str() != Some(""));
        let Some(v) = raw else {
            if f.required {
                e.add(&f.key, "type-generic.field_invalid", "required");
            }
            continue;
        };
        let map = |s: &str| -> String {
            vocab
                .get(&f.key)
                .and_then(|m| m.get(s))
                .cloned()
                .unwrap_or_else(|| s.to_owned())
        };
        let opt_ok = |s: &str| f.options.iter().any(|o| o.value == s);
        let bad = |e: &mut Errors, msg: &str| e.add(&f.key, "type-generic.field_invalid", msg);
        let num = || {
            v.as_f64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
        };
        let range_ok = |n: f64| f.min.is_none_or(|m| n >= m) && f.max.is_none_or(|m| n <= m);
        match f.ty.as_str() {
            "string" | "external_id" => match v.as_str().map(str::trim) {
                Some(s)
                    if s.chars().count()
                        <= f.max_len.unwrap_or(if f.ty == "external_id" { 64 } else { 200 }) =>
                {
                    o.insert(
                        f.key.clone(),
                        json!(s.chars().filter(|c| !c.is_control()).collect::<String>()),
                    );
                }
                Some(_) => bad(&mut e, "too long"),
                None => bad(&mut e, "expected a string"),
            },
            "integer" => match num() {
                Some(n) if n.fract() == 0.0 && range_ok(n) => {
                    o.insert(f.key.clone(), json!(n as i64));
                }
                _ => bad(&mut e, "expected an integer in range"),
            },
            "number" => match num() {
                Some(n) if n.is_finite() && range_ok(n) => {
                    o.insert(f.key.clone(), json!(n));
                }
                _ => bad(&mut e, "expected a number in range"),
            },
            "bool" => match v.as_bool() {
                Some(b) => {
                    o.insert(f.key.clone(), json!(b));
                }
                None => bad(&mut e, "expected a boolean"),
            },
            "date" => match v.as_str() {
                Some(s)
                    if s.len() == 10
                        && s.as_bytes()[4] == b'-'
                        && s.as_bytes()[7] == b'-'
                        && s.replace('-', "").bytes().all(|c| c.is_ascii_digit()) =>
                {
                    o.insert(f.key.clone(), json!(s));
                }
                _ => bad(&mut e, "expected YYYY-MM-DD"),
            },
            "url" => match v.as_str() {
                Some(s)
                    if s.starts_with("https://") && s.len() <= 500 && !s.contains(char::is_whitespace) =>
                {
                    o.insert(f.key.clone(), json!(s));
                }
                _ => bad(&mut e, "expected an https URL"),
            },
            "enum" => match v.as_str().map(|s| map(s.trim())) {
                Some(s) if opt_ok(&s) => {
                    o.insert(f.key.clone(), json!(s));
                }
                _ => bad(&mut e, "not an allowed value"),
            },
            "multi_enum" => {
                let xs: Vec<String> = match v {
                    Value::Array(a) => a
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|s| map(s.trim()))
                        .collect(),
                    Value::String(s) => s
                        .split(',')
                        .map(|x| map(x.trim()))
                        .filter(|x| !x.is_empty())
                        .collect(),
                    _ => vec![],
                };
                if xs.iter().all(|x| opt_ok(x)) {
                    let mut out: Vec<String> = Vec::new();
                    for x in xs {
                        if !out.contains(&x) {
                            out.push(x);
                        }
                    }
                    o.insert(f.key.clone(), json!(out));
                } else {
                    bad(&mut e, "not an allowed value");
                }
            }
            _ => {}
        }
    }
    if e.is_empty() {
        Ok(Value::Object(o))
    } else {
        Err(e)
    }
}

fn key_part(v: &Value) -> Option<String> {
    match v {
        Value::String(s) if !s.is_empty() => Some(pt_type_kit::norm_key(s)),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

pub fn group_key(d: &KindDef, f: &Value) -> Option<String> {
    let join = |keys: &[String]| -> Option<String> {
        let parts: Option<Vec<String>> = keys.iter().map(|k| key_part(&f[k.as_str()])).collect();
        parts.filter(|p| !p.is_empty()).map(|p| p.join(":"))
    };
    if !d.group_fields.is_empty()
        && let Some(k) = join(&d.group_fields)
    {
        return Some(format!("generic:{}:{k}", d.kind));
    }
    if !d.group_fallback_fields.is_empty()
        && let Some(k) = join(&d.group_fallback_fields)
    {
        return Some(format!("generic:{}:t:{k}", d.kind));
    }
    None
}

pub fn canonical_title(d: &KindDef, f: &Value) -> String {
    let keys: Vec<&str> = d.fields.iter().map(|x| x.key.as_str()).collect();
    let src = d.title_template.clone().unwrap_or_else(|| "{title}".into());
    let label = |field: &str, v: &Value| -> Option<String> {
        // 枚举值显示为选项标签（若有）
        let s = v.as_str()?;
        let def = d.fields.iter().find(|x| x.key == field)?;
        Some(
            def.options
                .iter()
                .find(|o| o.value == s)
                .and_then(|o| o.label_en.clone().or_else(|| o.label_zh.clone()))
                .unwrap_or_else(|| s.to_owned()),
        )
        .or_else(|| pt_type_kit::plain(v))
    };
    Template::parse(&src, Some(&keys))
        .map(|t| t.render(f, &|k, v| label(k, v).or_else(|| pt_type_kit::plain(v))))
        .unwrap_or_default()
}

/// 预设 kind（LLD §1.1）
pub fn presets() -> Vec<KindDef> {
    let v = json!([
      { "kind": "game", "name_zh": "游戏", "name_en": "Games",
        "fields": [
          { "key": "title", "type": "string", "required": true, "max_len": 200, "group_level": true, "search": "searchable", "label_zh": "名称", "label_en": "Title" },
          { "key": "igdb_id", "type": "external_id", "source": "igdb", "group_level": true, "search": "exact", "label_en": "IGDB" },
          { "key": "platform", "type": "enum", "required": true, "filter": "facet", "label_zh": "平台", "label_en": "Platform",
            "options": [ { "value": "windows", "label_en": "Windows" }, { "value": "macos", "label_en": "macOS" }, { "value": "linux", "label_en": "Linux" },
                         { "value": "switch", "label_en": "Switch" }, { "value": "ps5", "label_en": "PS5" }, { "value": "ps4", "label_en": "PS4" },
                         { "value": "xbox", "label_en": "Xbox" }, { "value": "other", "label_en": "Other" } ] },
          { "key": "release_kind", "type": "enum", "filter": "facet", "label_zh": "发布形式", "label_en": "Release",
            "options": [ { "value": "iso", "label_en": "ISO" }, { "value": "repack", "label_en": "Repack" }, { "value": "portable", "label_en": "Portable" },
                         { "value": "dlc", "label_en": "DLC" }, { "value": "update", "label_en": "Update" } ] },
          { "key": "version", "type": "string", "max_len": 40, "label_zh": "版本", "label_en": "Version" },
          { "key": "languages", "type": "multi_enum", "filter": "facet", "label_zh": "语言", "label_en": "Languages",
            "options": [ { "value": "zh-Hans" }, { "value": "zh-Hant" }, { "value": "en" }, { "value": "ja" }, { "value": "other" } ] },
          { "key": "year", "type": "integer", "min": 1970, "max": 2100, "group_level": true, "filter": "range", "label_zh": "年份", "label_en": "Year" }
        ],
        "group_fields": ["igdb_id"], "group_fallback_fields": ["title", "year"],
        "title_template": "{title} {version?v| }[{platform}] {release_kind?[|]}",
        "files": { "forbidden_extensions": ["lnk", "scr"] } },
      { "kind": "software", "name_zh": "软件", "name_en": "Software",
        "fields": [
          { "key": "title", "type": "string", "required": true, "max_len": 200, "group_level": true, "search": "searchable", "label_zh": "名称", "label_en": "Title" },
          { "key": "version", "type": "string", "max_len": 40, "label_zh": "版本", "label_en": "Version" },
          { "key": "platform", "type": "enum", "required": true, "filter": "facet", "label_zh": "平台", "label_en": "Platform",
            "options": [ { "value": "windows", "label_en": "Windows" }, { "value": "macos", "label_en": "macOS" }, { "value": "linux", "label_en": "Linux" },
                         { "value": "android", "label_en": "Android" }, { "value": "ios", "label_en": "iOS" }, { "value": "cross", "label_en": "Cross-platform" } ] },
          { "key": "license", "type": "enum", "filter": "facet", "label_zh": "授权", "label_en": "License",
            "options": [ { "value": "free" }, { "value": "freeware" }, { "value": "shareware" }, { "value": "commercial" } ] },
          { "key": "architecture", "type": "multi_enum", "filter": "facet", "label_zh": "架构", "label_en": "Architecture",
            "options": [ { "value": "x86" }, { "value": "x64" }, { "value": "arm64" } ] }
        ],
        "group_fallback_fields": ["title"],
        "title_template": "{title} {version?v| }[{platform}]" },
      { "kind": "ebook", "name_zh": "电子书", "name_en": "E-books",
        "fields": [
          { "key": "title", "type": "string", "required": true, "max_len": 300, "group_level": true, "search": "searchable", "label_zh": "书名", "label_en": "Title" },
          { "key": "authors", "type": "string", "max_len": 200, "group_level": true, "search": "searchable", "label_zh": "作者", "label_en": "Authors" },
          { "key": "isbn", "type": "string", "max_len": 17, "group_level": true, "search": "exact", "label_en": "ISBN" },
          { "key": "format", "type": "multi_enum", "filter": "facet", "label_zh": "格式", "label_en": "Format",
            "options": [ { "value": "epub" }, { "value": "pdf" }, { "value": "mobi" }, { "value": "azw3" }, { "value": "djvu" }, { "value": "cbz" } ] },
          { "key": "language", "type": "enum", "filter": "facet", "label_zh": "语言", "label_en": "Language",
            "options": [ { "value": "zh" }, { "value": "en" }, { "value": "ja" }, { "value": "other" } ] },
          { "key": "publisher", "type": "string", "max_len": 120, "label_zh": "出版社", "label_en": "Publisher" },
          { "key": "year", "type": "integer", "min": 1450, "max": 2100, "group_level": true, "filter": "range", "label_zh": "年份", "label_en": "Year" }
        ],
        "group_fields": ["isbn"], "group_fallback_fields": ["title", "authors"],
        "title_template": "{title} {authors?- | }{year?(|)}" },
      { "kind": "course", "name_zh": "课程", "name_en": "Courses",
        "fields": [
          { "key": "title", "type": "string", "required": true, "max_len": 200, "group_level": true, "search": "searchable", "label_zh": "课程名", "label_en": "Title" },
          { "key": "provider", "type": "string", "max_len": 80, "filter": "facet", "label_zh": "平台", "label_en": "Provider" },
          { "key": "instructor", "type": "string", "max_len": 120, "search": "searchable", "label_zh": "讲师", "label_en": "Instructor" },
          { "key": "language", "type": "enum", "filter": "facet", "label_zh": "语言", "label_en": "Language",
            "options": [ { "value": "zh" }, { "value": "en" }, { "value": "other" } ] },
          { "key": "level", "type": "enum", "filter": "facet", "label_zh": "难度", "label_en": "Level",
            "options": [ { "value": "beginner" }, { "value": "intermediate" }, { "value": "advanced" } ] },
          { "key": "duration_hours", "type": "number", "min": 0, "max": 2000, "filter": "range", "label_zh": "时长（小时）", "label_en": "Hours" }
        ],
        "group_fallback_fields": ["title"],
        "title_template": "{title} {provider?[|]}" }
    ]);
    serde_json::from_value(v).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_validate_and_compile() {
        for p in presets() {
            assert!(validate(&p, 30).is_ok(), "{}", p.kind);
            let s = compile(&p);
            assert_eq!(s["properties"]["kind"]["const"], p.kind);
        }
        assert_eq!(presets().len(), 4);
    }

    #[test]
    fn game_flow() {
        let game = presets()
            .into_iter()
            .find(|k| k.kind == "game")
            .unwrap_or_else(|| unreachable!());
        let mut vocab = BTreeMap::new();
        vocab.insert(
            "platform".to_owned(),
            BTreeMap::from([("win".to_owned(), "windows".to_owned())]),
        );
        let f = normalize(&game, &json!({ "title": " Hades ", "platform": "win", "release_kind": "repack", "version": "1.38", "year": "2020", "languages": "zh-Hans, en", "junk": 1 }), &vocab)
            .unwrap_or_else(|e| panic!("{:?}", e.details()));
        assert_eq!(f["platform"], "windows");
        assert_eq!(f["year"], 2020);
        assert_eq!(f["languages"], json!(["zh-Hans", "en"]));
        assert!(f.get("junk").is_none());
        assert_eq!(group_key(&game, &f).as_deref(), Some("generic:game:t:hades:2020"));
        assert_eq!(canonical_title(&game, &f), "Hades v1.38 [Windows] [Repack]");
        let f2 = normalize(
            &game,
            &json!({ "title": "Hades", "platform": "windows", "igdb_id": "113112" }),
            &BTreeMap::new(),
        )
        .unwrap_or_default();
        assert_eq!(group_key(&game, &f2).as_deref(), Some("generic:game:113112"));
        let e = normalize(
            &game,
            &json!({ "platform": "dreamcast", "year": 1800 }),
            &BTreeMap::new(),
        )
        .err()
        .unwrap_or_default();
        let d = e.details();
        let n = d["errors"].as_array().map_or(0, Vec::len);
        assert_eq!(n, 3, "{d}");
    }

    #[test]
    fn definitions_are_checked() {
        let mut bad = presets().remove(0);
        bad.fields[0].key = "kind".into();
        bad.title_template = Some("{nope}".into());
        bad.group_fields = vec!["version".into()];
        let e = validate(&bad, 30).err().unwrap_or_default();
        assert!(e.details()["errors"].as_array().map_or(0, Vec::len) >= 3);
    }
}
