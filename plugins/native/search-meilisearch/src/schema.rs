//! 贡献者 schema 合并与 Meilisearch 设置生成（LLD §1.2、§1.3、§1.8）。

use std::collections::BTreeMap;

use pt_plugin_api::IndexSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    String,
    Int,
    Float,
    Bool,
    Timestamp,
    StringArray,
}

impl FieldType {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "string" => Self::String,
            "int" => Self::Int,
            "float" => Self::Float,
            "bool" => Self::Bool,
            "timestamp" => Self::Timestamp,
            "string[]" => Self::StringArray,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Int => "int",
            Self::Float => "float",
            Self::Bool => "bool",
            Self::Timestamp => "timestamp",
            Self::StringArray => "string[]",
        }
    }

    pub fn numeric(self) -> bool {
        matches!(self, Self::Int | Self::Float | Self::Timestamp)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MergedField {
    pub name: String,
    pub ty: FieldType,
    pub searchable: bool,
    pub filterable: bool,
    pub sortable: bool,
    pub rank: u8,
    pub exact: bool,
    pub label_term: String,
    pub source_plugin: String,
    /// rel_* 等跨类型通用字段
    #[serde(default)]
    pub generic: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Conflict {
    pub plugin: String,
    pub field: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MergedIndex {
    pub index: String,
    pub owner: String,
    pub fields: BTreeMap<String, MergedField>,
    pub title_fields: Vec<String>,
    pub synonyms: Vec<Vec<String>>,
    pub conflicts: Vec<Conflict>,
    /// 参与合并的贡献者（拥有者在前）
    pub contributors: Vec<String>,
}

impl MergedIndex {
    pub fn field(&self, name: &str) -> Option<&MergedField> {
        self.fields.get(name)
    }

    /// 贡献者在本索引声明的字段（片段校验用）
    pub fn fields_of(&self, plugin: &str) -> Vec<&str> {
        self.fields
            .values()
            .filter(|f| f.source_plugin == plugin)
            .map(|f| f.name.as_str())
            .collect()
    }
}

pub const RESERVED: &[&str] = &["id", "vis_level", "vis_min_class", "vis_owner_id", "created_at"];
const SENSITIVE: &[&str] = &[
    "passkey", "email", "ip", "password", "token", "secret", "totp", "webauthn",
];
pub const REL_FIELDS: &[(&str, FieldType)] = &[
    ("rel_resolution", FieldType::String),
    ("rel_source", FieldType::String),
    ("rel_video_codec", FieldType::String),
    ("rel_audio_codec", FieldType::StringArray),
    ("rel_hdr", FieldType::StringArray),
    ("rel_group", FieldType::String),
    ("rel_year", FieldType::Int),
];

/// 字段命名校验（LLD §1.2 表）
pub fn validate_name(plugin: &str, owner: &str, name: &str) -> Result<(), String> {
    let b = name.as_bytes();
    let shape = !b.is_empty()
        && b.len() <= 63
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_');
    if !shape {
        return Err("name must match ^[a-z][a-z0-9_]{0,62}$".into());
    }
    if RESERVED.contains(&name)
        || name.starts_with("vis_")
        || name.starts_with("rel_")
        || name.starts_with('_')
    {
        return Err("reserved field name".into());
    }
    if name.split('_').any(|seg| SENSITIVE.contains(&seg)) {
        return Err("sensitive field name".into());
    }
    if plugin != owner {
        let prefix = match plugin.strip_prefix("type-") {
            Some(t) => format!("{t}_"),
            None => format!("{}_", plugin.replace('-', "_")),
        };
        if !name.starts_with(&prefix) {
            return Err(format!("non-owner fields must start with `{prefix}`"));
        }
    }
    Ok(())
}

fn reserved_field(name: &str, ty: FieldType, sortable: bool) -> MergedField {
    MergedField {
        name: name.into(),
        ty,
        searchable: false,
        filterable: true,
        sortable,
        rank: 0,
        exact: true,
        label_term: String::new(),
        source_plugin: "search-meilisearch".into(),
        generic: false,
    }
}

/// 合并（拥有者优先，其余按插件 ID 字典序；同名字段先到先得）
pub fn merge(
    index: &str,
    owner: &str,
    title_fields: &[&str],
    decls: &[(String, IndexSchema)],
) -> MergedIndex {
    let mut decls: Vec<&(String, IndexSchema)> = decls.iter().filter(|(_, s)| s.index == index).collect();
    decls.sort_by_key(|(p, _)| (p != owner, p.clone()));
    let mut m = MergedIndex {
        index: index.into(),
        owner: owner.into(),
        ..Default::default()
    };
    for (name, ty, sortable) in [
        ("id", FieldType::Int, true),
        ("vis_level", FieldType::String, false),
        ("vis_min_class", FieldType::Int, false),
        ("vis_owner_id", FieldType::Int, false),
        ("created_at", FieldType::Timestamp, true),
    ] {
        m.fields.insert(name.into(), reserved_field(name, ty, sortable));
    }
    for (pid, s) in decls {
        m.contributors.push(pid.clone());
        for f in &s.fields {
            let conflict = |reason: String| Conflict {
                plugin: pid.clone(),
                field: f.name.clone(),
                reason,
            };
            if let Err(e) = validate_name(pid, owner, &f.name) {
                m.conflicts.push(conflict(e));
                continue;
            }
            if m.fields.contains_key(&f.name) {
                m.conflicts.push(conflict("duplicate field".into()));
                continue;
            }
            let Some(ty) = FieldType::parse(&f.field_type) else {
                m.conflicts
                    .push(conflict(format!("unknown type `{}`", f.field_type)));
                continue;
            };
            m.fields.insert(
                f.name.clone(),
                MergedField {
                    name: f.name.clone(),
                    ty,
                    searchable: f.searchable,
                    filterable: f.filterable,
                    sortable: f.sortable && ty.numeric(),
                    rank: f.rank,
                    exact: f.exact,
                    label_term: f.label_term.clone(),
                    source_plugin: pid.clone(),
                    generic: false,
                },
            );
        }
        for g in &s.synonyms {
            if (2..=20).contains(&g.len()) && g.iter().all(|w| !w.is_empty() && w.chars().count() <= 64) {
                m.synonyms.push(g.clone());
            }
        }
    }
    if !m.contributors.iter().any(|c| c == owner) {
        return m;
    }
    for t in title_fields {
        if let Some(f) = m.fields.get(*t).cloned() {
            m.title_fields.push((*t).to_owned());
            let norm = format!("{t}_norm");
            m.fields.insert(
                norm.clone(),
                MergedField {
                    name: norm,
                    ty: FieldType::String,
                    searchable: true,
                    filterable: false,
                    sortable: false,
                    rank: f.rank,
                    exact: false,
                    label_term: String::new(),
                    source_plugin: "search-meilisearch".into(),
                    generic: false,
                },
            );
        }
    }
    if !m.title_fields.is_empty() {
        for (name, ty) in REL_FIELDS {
            m.fields.entry((*name).to_owned()).or_insert_with(|| MergedField {
                label_term: format!("search-meilisearch.field.{name}"),
                generic: true,
                ..reserved_field(name, *ty, false)
            });
        }
    }
    m
}

/// 内置 PT 同义词包（LLD §1.8 来源 1）
pub const BUILTIN_SYNONYMS: &[&[&str]] = &[
    &["2160p", "4k", "uhd"],
    &["hevc", "h265", "x265", "h.265"],
    &["avc", "h264", "x264", "h.264"],
    &["bluray", "blu-ray", "bd"],
    &["web-dl", "webdl"],
    &["ddp", "dd+", "eac3", "e-ac-3"],
    &["dovi", "dv", "dolby vision"],
    &["国语", "普通话", "mandarin"],
    &["粤语", "cantonese"],
    &["中字", "中文字幕", "chs"],
];

/// 组 → Meili 单向映射（组内两两互为同义词）
pub fn synonyms_map(groups: &[Vec<String>]) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for g in groups {
        let g: Vec<String> = g.iter().map(|w| w.to_lowercase()).collect();
        for w in &g {
            let e = out.entry(w.clone()).or_default();
            for o in &g {
                if o != w && !e.contains(o) {
                    e.push(o.clone());
                }
            }
        }
    }
    out
}

/// 设置 JSON（LLD §1.3；filterableAttributes 用名称列表，兼容所有 1.x 版本）
pub fn build_settings(m: &MergedIndex, max_total_hits: u64) -> Value {
    let mut searchable: Vec<&MergedField> = m.fields.values().filter(|f| f.searchable).collect();
    searchable.sort_by(|a, b| (a.rank, &a.name).cmp(&(b.rank, &b.name)));
    let names = |p: &dyn Fn(&MergedField) -> bool| -> Vec<String> {
        m.fields
            .values()
            .filter(|f| p(f))
            .map(|f| f.name.clone())
            .collect()
    };
    let mut groups: Vec<Vec<String>> = BUILTIN_SYNONYMS
        .iter()
        .map(|g| g.iter().map(|w| (*w).to_owned()).collect())
        .collect();
    groups.extend(m.synonyms.iter().cloned());
    json!({
        "searchableAttributes": searchable.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
        "filterableAttributes": names(&|f| f.filterable),
        "sortableAttributes": names(&|f| f.sortable),
        "displayedAttributes": ["*"],
        "rankingRules": ["words", "typo", "proximity", "attribute", "sort", "exactness"],
        "typoTolerance": {
            "enabled": true,
            "minWordSizeForTypos": { "oneTypo": 5, "twoTypos": 9 },
            "disableOnAttributes": names(&|f| f.exact && f.searchable),
            "disableOnNumbers": true,
        },
        "synonyms": synonyms_map(&groups),
        "pagination": { "maxTotalHits": max_total_hits },
        "faceting": { "maxValuesPerFacet": 100 },
    })
}

pub fn sha256_hex(v: &Value) -> String {
    hex::encode(Sha256::digest(canonical(v).as_bytes()))
}

/// 键排序后的 JSON 文本（serde_json 的 Map 默认按键排序）
pub fn canonical(v: &Value) -> String {
    v.to_string()
}

/// 文档形状哈希：字段名与类型、标题字段、贡献者（变化需要重建）
pub fn shape_hash(m: &MergedIndex) -> String {
    let fields: Vec<(String, &str)> = m
        .fields
        .values()
        .map(|f| (f.name.clone(), f.ty.as_str()))
        .collect();
    sha256_hex(&json!({ "fields": fields, "title_fields": m.title_fields, "contributors": m.contributors }))
}

#[cfg(test)]
mod tests {
    use pt_plugin_api::FieldSpec;

    use super::*;

    fn fs(name: &str, ty: &str, searchable: bool, filterable: bool) -> FieldSpec {
        FieldSpec {
            name: name.into(),
            field_type: ty.into(),
            searchable,
            filterable,
            sortable: true,
            rank: 0,
            exact: false,
            label_term: String::new(),
        }
    }

    #[test]
    fn merge_rules() {
        let decls = vec![
            (
                "torrent".to_owned(),
                IndexSchema {
                    index: "torrents".into(),
                    fields: vec![
                        fs("title", "string", true, false),
                        fs("size", "int", false, true),
                        fs("user_email", "string", false, true),
                    ],
                    synonyms: vec![vec!["a".into(), "b".into()]],
                },
            ),
            (
                "accounting".to_owned(),
                IndexSchema {
                    index: "torrents".into(),
                    fields: vec![
                        fs("accounting_seeders", "int", false, true),
                        fs("seeders", "int", false, true),
                    ],
                    synonyms: vec![],
                },
            ),
            (
                "type-movie".to_owned(),
                IndexSchema {
                    index: "torrents".into(),
                    fields: vec![
                        fs("movie_imdb_id", "string", false, true),
                        fs("size", "int", false, true),
                    ],
                    synonyms: vec![],
                },
            ),
        ];
        let m = merge("torrents", "torrent", &["title"], &decls);
        assert_eq!(m.contributors, vec!["torrent", "accounting", "type-movie"]);
        for f in [
            "id",
            "vis_level",
            "title",
            "title_norm",
            "size",
            "accounting_seeders",
            "movie_imdb_id",
            "rel_resolution",
        ] {
            assert!(m.fields.contains_key(f), "{f}");
        }
        let bad: Vec<&str> = m.conflicts.iter().map(|c| c.field.as_str()).collect();
        assert_eq!(bad, vec!["user_email", "seeders", "size"]);
        assert!(!m.fields["title"].sortable);
        let s = build_settings(&m, 1000);
        assert_eq!(s["searchableAttributes"], json!(["title", "title_norm"]));
        assert!(
            s["filterableAttributes"]
                .as_array()
                .is_some_and(|a| a.contains(&json!("vis_min_class")))
        );
        assert_eq!(s["synonyms"]["a"], json!(["b"]));
        // 拥有者缺席：只有保留字段与非拥有者字段
        let m2 = merge("torrents", "torrent", &["title"], &decls[1..]);
        assert!(m2.title_fields.is_empty());
        assert_ne!(shape_hash(&m), shape_hash(&m2));
    }
}
