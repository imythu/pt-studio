//! type-movie 插件（WASM，docs/plugins/type-movie/）：`content-type` 与 `search-contributor` 导出。
//!
//! 本版本实现：字段与 Group schema、规范化（同义词、外部 ID 提取、发布名补全、必填组合）、分组键（IMDb > TMDB > 豆瓣 >
//! 标题 + 年份）、规范标题（模板可在设置中修改）、`torrents` 索引的 `movie_*` 字段。上传钩子规则（去重、trump、文件结构）、
//! 元数据联动、词表与质量权重后台在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

wit_bindgen::generate!({
    path: "wit",
    world: "type-movie",
    generate_all,
});

pub mod movie;

use exports::pt::plugin::{content_type, on_cron, on_event, search_contributor};
use pt::plugin::{
    config, jobs, services,
    sql::{self, SqlValue},
    time,
    types::PluginError,
};
use serde_json::{Value, json};

struct TypeMovie;

fn err(code: &str, message: &str, details: Option<Value>) -> PluginError {
    PluginError {
        code: code.into(),
        message: message.into(),
        details: details.map(|d| d.to_string()),
        retryable: false,
    }
}

fn parse(fields: &str) -> Result<Value, PluginError> {
    serde_json::from_str(fields).map_err(|_| err("kernel.bad_request", "fields must be JSON", None))
}

fn template() -> Option<String> {
    let s: Value = serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null);
    s["title_template"].as_str().map(str::to_owned)
}

impl content_type::Guest for TypeMovie {
    fn info() -> content_type::TypeInfo {
        content_type::TypeInfo {
            type_id: "movie".into(),
            name_term: "type-movie.name".into(),
            field_schema: movie::field_schema().to_string(),
            group_schema: Some(movie::group_schema().to_string()),
        }
    }

    fn normalize(fields: String) -> Result<String, PluginError> {
        match movie::normalize(&parse(&fields)?) {
            Ok(v) => Ok(v.to_string()),
            Err(e) => {
                let field = e.first_field().unwrap_or("fields").to_owned();
                Err(err(
                    &format!("type-movie.{field}_invalid"),
                    "movie fields are invalid",
                    Some(e.details()),
                ))
            }
        }
    }

    fn group_key(fields: String) -> Result<Option<String>, PluginError> {
        Ok(movie::group_key(&parse(&fields)?))
    }

    fn canonical_title(fields: String) -> Result<String, PluginError> {
        Ok(movie::canonical_title(&parse(&fields)?, template().as_deref()))
    }
}

// ---------------- 搜索贡献（content-types-common §7） ----------------

const SEARCH_FIELDS: &[(&str, &str, bool, bool)] = &[
    // (字段, 类型, filterable, searchable)
    ("imdb_id", "string", true, false),
    ("year", "int", true, false),
    ("resolution", "string", true, false),
    ("source", "string", true, false),
    ("release_type", "string", true, false),
    ("video_codec", "string", true, false),
    ("audio_codecs", "string[]", true, false),
    ("hdr", "string[]", true, false),
    ("release_group", "string", true, false),
    ("original_title", "string", false, true),
];

impl search_contributor::Guest for TypeMovie {
    fn index_schemas() -> Vec<search_contributor::IndexSchema> {
        vec![search_contributor::IndexSchema {
            index: "torrents".into(),
            fields: SEARCH_FIELDS
                .iter()
                .map(|(f, ty, filterable, searchable)| search_contributor::FieldSpec {
                    name: format!("movie_{f}"),
                    field_type: (*ty).into(),
                    searchable: *searchable,
                    filterable: *filterable,
                    sortable: *f == "year",
                    rank: 2,
                    exact: *f == "imdb_id",
                    label_term: if *filterable {
                        format!("type-movie.field.{f}")
                    } else {
                        String::new()
                    },
                })
                .collect(),
            synonyms: vec![],
        }]
    }

    fn to_document(index: String, entity_id: u64) -> Result<Option<String>, PluginError> {
        if index != "torrents" {
            return Ok(None);
        }
        let r = sql::query(
            "SELECT fields::text FROM torrent.v_torrent_fields WHERE id = $1 AND type_id = 'movie'",
            &[SqlValue::Int(i64::try_from(entity_id).unwrap_or(0))],
        )?;
        let Some(SqlValue::Text(t)) = r.rows.first().and_then(|row| row.first()) else {
            return Ok(None);
        };
        let f: Value = serde_json::from_str(t).unwrap_or(Value::Null);
        let mut doc = serde_json::Map::new();
        for (k, _, _, _) in SEARCH_FIELDS {
            if let Some(v) = f.get(*k).filter(|v| !v.is_null()) {
                doc.insert(format!("movie_{k}"), v.clone());
            }
        }
        Ok(Some(json!(doc).to_string()))
    }
}

// ---------------- 元数据联动（content-types-common §8） ----------------

fn svc(service: &str, method: &str, args: &Value) -> Result<Value, PluginError> {
    let out = services::call(service, method, &args.to_string())?;
    serde_json::from_str(&out)
        .map_err(|_| err("type-movie.bad_service_reply", "invalid JSON from service", None))
}

impl on_event::Guest for TypeMovie {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> Result<(), PluginError> {
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        if evt.event_type == "group.created" && p["type_id"] == "movie" {
            let gid = p["group_id"].as_i64().unwrap_or(0);
            jobs::schedule_once(
                "fetch_metadata",
                time::now(),
                &json!({ "group_id": gid }).to_string(),
                Some(&format!("meta:{gid}")),
            )?;
        }
        Ok(())
    }
}

impl on_cron::Guest for TypeMovie {
    fn run(job: on_cron::JobInvocation) -> Result<(), PluginError> {
        if job.name != "fetch_metadata" {
            return Err(err("kernel.not_found", "unknown job", None));
        }
        let p: Value = serde_json::from_str(&job.payload).unwrap_or(Value::Null);
        let gid = p["group_id"].as_i64().unwrap_or(0);
        let g = svc("ctx.torrents", "get_group", &json!({ "id": gid }))?;
        let f = &g["fields"];
        let external = match (f["tmdb_id"].as_u64(), f["imdb_id"].as_str()) {
            (Some(t), _) => format!("movie:{t}"),
            (None, Some(i)) => format!("imdb:{i}"),
            _ => return Ok(()),
        };
        let settings: Value = serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null);
        let locale = settings["metadata_locale"].as_str().unwrap_or("zh-CN").to_owned();
        let r = match svc(
            "ctx.metadata",
            "lookup",
            &json!({ "source": "tmdb", "external_id": external, "locale": locale }),
        ) {
            Ok(r) => r,
            // 未安装 / 未配置元数据源：不重试
            Err(e) if !e.retryable => return Ok(()),
            Err(e) => return Err(e),
        };
        if r.is_null() {
            return Ok(());
        }
        let patch = movie::metadata_patch(&r["data"], &locale);
        svc(
            "ctx.torrents",
            "update_group",
            &json!({ "id": gid, "fields": patch }),
        )?;
        Ok(())
    }
}

export!(TypeMovie);
