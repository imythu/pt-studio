//! type-tv 插件（WASM，docs/plugins/type-tv/）：`content-type`、`search-contributor`、`torrent.before_upload` 钩子与元数据联动。
//!
//! 本版本实现：字段与 Group schema、规范化（发布名补全季集与规格、范围推断与组合校验）、按季分组（TMDB > IMDb > 豆瓣 >
//! 标题 + 首播年）、作品标题（“中文名 / 原名 第 N 季”）、规范标题、上传时按文件列表检查范围、`tv_*` 搜索字段、
//! 剧与季元数据补全。

#![allow(clippy::missing_errors_doc)]

pt_plugin_sdk::world!("type-tv");

pub mod tv;

use exports::pt::plugin::{content_type, hooks, on_cron, on_event, search_contributor};
use pt_plugin_sdk::{
    config, jobs, services,
    sql::{self, SqlValue},
    time,
    types::PluginError,
};
use serde_json::{Value, json};

struct TypeTv;

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

impl content_type::Guest for TypeTv {
    fn info() -> content_type::TypeInfo {
        content_type::TypeInfo {
            type_id: "tv".into(),
            name_term: "type-tv.name".into(),
            field_schema: tv::field_schema().to_string(),
            group_schema: Some(tv::group_schema().to_string()),
        }
    }

    fn normalize(fields: String) -> Result<String, PluginError> {
        match tv::normalize(&parse(&fields)?) {
            Ok(v) => Ok(v.to_string()),
            Err(e) => {
                // 首个错误的错误码（scope_invalid / external_id_required / <field>_invalid）
                let code = e.details()["errors"][0]["code"]
                    .as_str()
                    .unwrap_or("type-tv.field_invalid")
                    .to_owned();
                Err(err(&code, "tv fields are invalid", Some(e.details())))
            }
        }
    }

    fn group_key(fields: String) -> Result<Option<String>, PluginError> {
        Ok(tv::group_key(&parse(&fields)?))
    }

    fn canonical_title(fields: String) -> Result<String, PluginError> {
        Ok(tv::canonical_title(&parse(&fields)?, template().as_deref()))
    }
}

// ---------------- 搜索贡献（content-types-common §7） ----------------

const SEARCH_FIELDS: &[(&str, &str, bool, bool)] = &[
    // (字段, 类型, filterable, searchable)
    ("imdb_id", "string", true, false),
    ("first_air_year", "int", true, false),
    ("season", "int", true, false),
    ("episode", "int", true, false),
    ("scope", "string", true, false),
    ("network", "string", true, false),
    ("resolution", "string", true, false),
    ("source", "string", true, false),
    ("release_type", "string", true, false),
    ("video_codec", "string", true, false),
    ("audio_codecs", "string[]", true, false),
    ("hdr", "string[]", true, false),
    ("release_group", "string", true, false),
    ("title_zh", "string", false, true),
];

impl search_contributor::Guest for TypeTv {
    fn index_schemas() -> Vec<search_contributor::IndexSchema> {
        vec![search_contributor::IndexSchema {
            index: "torrents".into(),
            fields: SEARCH_FIELDS
                .iter()
                .map(|(f, ty, filterable, searchable)| search_contributor::FieldSpec {
                    name: format!("tv_{f}"),
                    field_type: (*ty).into(),
                    searchable: *searchable,
                    filterable: *filterable,
                    sortable: matches!(*f, "first_air_year" | "season" | "episode"),
                    rank: 2,
                    exact: *f == "imdb_id",
                    label_term: if *filterable {
                        format!("type-tv.field.{f}")
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
            "SELECT fields::text FROM torrent.v_torrent_fields WHERE id = $1 AND type_id = 'tv'",
            &[SqlValue::Int(i64::try_from(entity_id).unwrap_or(0))],
        )?;
        let Some(SqlValue::Text(t)) = r.rows.first().and_then(|row| row.first()) else {
            return Ok(None);
        };
        let f: Value = serde_json::from_str(t).unwrap_or(Value::Null);
        let mut doc = serde_json::Map::new();
        for (k, _, _, _) in SEARCH_FIELDS {
            if let Some(v) = f.get(*k).filter(|v| !v.is_null()) {
                doc.insert(format!("tv_{k}"), v.clone());
            }
        }
        Ok(Some(json!(doc).to_string()))
    }
}

// ---------------- 元数据联动（content-types-common §8） ----------------

fn svc(service: &str, method: &str, args: &Value) -> Result<Value, PluginError> {
    let out = services::call(service, method, &args.to_string())?;
    serde_json::from_str(&out)
        .map_err(|_| err("type-tv.bad_service_reply", "invalid JSON from service", None))
}

impl on_event::Guest for TypeTv {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> Result<(), PluginError> {
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        if evt.event_type == "group.created" && p["type_id"] == "tv" {
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

impl on_cron::Guest for TypeTv {
    fn run(job: on_cron::JobInvocation) -> Result<(), PluginError> {
        if job.name != "fetch_metadata" {
            return Err(err("kernel.not_found", "unknown job", None));
        }
        let p: Value = serde_json::from_str(&job.payload).unwrap_or(Value::Null);
        let gid = p["group_id"].as_i64().unwrap_or(0);
        let g = svc("ctx.torrents", "get_group", &json!({ "id": gid }))?;
        let f = &g["fields"];
        let Some(id) = f["tmdb_tv_id"].as_u64() else {
            // 只有 IMDb：先换算成 TMDB 剧 ID
            return match f["imdb_id"].as_str() {
                Some(i) => enrich(gid, &format!("imdb:{i}"), f["season"].as_i64()),
                None => Ok(()),
            };
        };
        enrich(gid, &format!("tv:{id}"), f["season"].as_i64())
    }
}

fn locale() -> String {
    let settings: Value = serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null);
    settings["metadata_locale"].as_str().unwrap_or("zh-CN").to_owned()
}

/// `Ok(None)`：元数据源未安装 / 未配置或无结果（不重试）
fn lookup(external: &str, locale: &str) -> Result<Option<Value>, PluginError> {
    match svc(
        "ctx.metadata",
        "lookup",
        &json!({ "source": "tmdb", "external_id": external, "locale": locale }),
    ) {
        Ok(r) if r.is_null() => Ok(None),
        Ok(r) => Ok(Some(r["data"].clone())),
        Err(e) if !e.retryable => Ok(None),
        Err(e) => Err(e),
    }
}

fn enrich(gid: i64, external: &str, season: Option<i64>) -> Result<(), PluginError> {
    let locale = locale();
    let Some(mut show) = lookup(external, &locale)? else {
        return Ok(());
    };
    // imdb:tt… 可能换算成电影；剧集只认 tv
    if show["kind"] != "tv" {
        return Ok(());
    }
    if external.starts_with("imdb:")
        && let Some(id) = show["ids"]["tmdb"].as_u64()
        && let Some(full) = lookup(&format!("tv:{id}"), &locale)?
    {
        show = full;
    }
    let season_data = match (show["ids"]["tmdb"].as_u64(), season.filter(|s| *s > 0)) {
        (Some(id), Some(n)) => lookup(&format!("tv:{id}:s{n}"), &locale)?,
        _ => None,
    };
    let patch = tv::metadata_patch(&show, season_data.as_ref(), &locale);
    svc(
        "ctx.torrents",
        "update_group",
        &json!({ "id": gid, "fields": patch }),
    )?;
    Ok(())
}

// ---------------- 上传钩子：文件列表与范围（type-tv LLD §1.2） ----------------

fn on_before_upload(input: &Value) -> Result<hooks::HookDecision, PluginError> {
    if input["type_id"] != "tv" {
        return Ok(hooks::HookDecision::Allow);
    }
    let settings: Value = serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null);
    if settings["scope_check"].as_bool() == Some(false) {
        return Ok(hooks::HookDecision::Allow);
    }
    let f = &input["fields"];
    let files: Vec<(String, u64)> = input["files_sample"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| Some((x["path"].as_str()?.to_owned(), x["size"].as_u64()?)))
                .collect()
        })
        .unwrap_or_default();
    let truncated = input["file_count"]
        .as_u64()
        .is_some_and(|n| n as usize > files.len());
    // 季包完整度：已有 Group 时用其元数据中的集数
    let episode_count = input["group_fields"]["episode_count"].as_i64();
    let check = pt_type_kit::episodes::ScopeCheck {
        scope: f["scope"].as_str().unwrap_or("season_pack"),
        episode: f["episode"].as_i64(),
        episode_end: f["episode_end"].as_i64(),
        ongoing: f["ongoing"].as_bool().unwrap_or(false),
        episode_count,
        min_ratio: settings["season_pack_min_ratio"].as_f64().unwrap_or(0.8),
    };
    Ok(
        match pt_type_kit::episodes::check_scope(&files, &check, truncated) {
            Ok(()) => hooks::HookDecision::Allow,
            Err(reason) => hooks::HookDecision::Deny(hooks::DenyInfo {
                code: "type-tv.scope_mismatch".into(),
                message: reason.clone(),
                details: Some(json!({ "scope": check.scope, "reason": reason }).to_string()),
            }),
        },
    )
}

impl hooks::Guest for TypeTv {
    fn handle(c: hooks::HookCall) -> Result<hooks::HookDecision, PluginError> {
        let input: Value = serde_json::from_str(&c.input).unwrap_or(Value::Null);
        match c.hook.as_str() {
            "torrent.before_upload" => on_before_upload(&input),
            _ => Ok(hooks::HookDecision::Allow),
        }
    }
}

export!(TypeTv);
