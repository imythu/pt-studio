//! type-anime 插件（WASM，docs/plugins/type-anime/）：`content-type`、`search-contributor`、`torrent.before_upload` 钩子与元数据联动。
//!
//! 本版本实现：字段与 Group schema、规范化（字幕组风格发布名解析、字幕组别名、字幕语言 / 方式标签、范围推断）、分组
//! （Bangumi > AniDB > TMDB > 标题 + 年份；后两者按季）、作品标题、规范标题、上传时按文件列表检查范围、`anime_*` 搜索字段、
//! 有 TMDB ID 时补全剧与季元数据。Bangumi / AniDB 元数据源在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

wit_bindgen::generate!({
    path: "wit",
    world: "type-anime",
    generate_all,
});

pub mod anime;
pub mod name;

use exports::pt::plugin::{content_type, hooks, on_cron, on_event, search_contributor};
use pt::plugin::{
    config, jobs, services,
    sql::{self, SqlValue},
    time,
    types::PluginError,
};
use serde_json::{Value, json};

struct TypeAnime;

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

impl content_type::Guest for TypeAnime {
    fn info() -> content_type::TypeInfo {
        content_type::TypeInfo {
            type_id: "anime".into(),
            name_term: "type-anime.name".into(),
            field_schema: anime::field_schema().to_string(),
            group_schema: Some(anime::group_schema().to_string()),
        }
    }

    fn normalize(fields: String) -> Result<String, PluginError> {
        let settings: Value = serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null);
        let aliases = settings["fansub_aliases"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        match anime::normalize(&parse(&fields)?, &aliases) {
            Ok(v) => Ok(v.to_string()),
            Err(e) => {
                // 首个错误的错误码（scope_invalid / external_id_required / <field>_invalid）
                let code = e.details()["errors"][0]["code"]
                    .as_str()
                    .unwrap_or("type-anime.field_invalid")
                    .to_owned();
                Err(err(&code, "anime fields are invalid", Some(e.details())))
            }
        }
    }

    fn group_key(fields: String) -> Result<Option<String>, PluginError> {
        Ok(anime::group_key(&parse(&fields)?))
    }

    fn canonical_title(fields: String) -> Result<String, PluginError> {
        Ok(anime::canonical_title(&parse(&fields)?, template().as_deref()))
    }
}

// ---------------- 搜索贡献（content-types-common §7） ----------------

const SEARCH_FIELDS: &[(&str, &str, bool, bool)] = &[
    // (字段, 类型, filterable, searchable)
    ("media", "string", true, false),
    ("year", "int", true, false),
    ("season", "int", true, false),
    ("episode", "int", true, false),
    ("scope", "string", true, false),
    ("fansub", "string", true, true),
    ("subtitle_langs", "string[]", true, false),
    ("subtitle_kind", "string", true, false),
    ("resolution", "string", true, false),
    ("source", "string", true, false),
    ("video_codec", "string", true, false),
    ("bit_depth", "int", true, false),
    ("title_zh", "string", false, true),
];

impl search_contributor::Guest for TypeAnime {
    fn index_schemas() -> Vec<search_contributor::IndexSchema> {
        vec![search_contributor::IndexSchema {
            index: "torrents".into(),
            fields: SEARCH_FIELDS
                .iter()
                .map(|(f, ty, filterable, searchable)| search_contributor::FieldSpec {
                    name: format!("anime_{f}"),
                    field_type: (*ty).into(),
                    searchable: *searchable,
                    filterable: *filterable,
                    sortable: matches!(*f, "year" | "season" | "episode"),
                    rank: 2,
                    exact: false,
                    label_term: if *filterable {
                        format!("type-anime.field.{f}")
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
            "SELECT fields::text FROM torrent.v_torrent_fields WHERE id = $1 AND type_id = 'anime'",
            &[SqlValue::Int(i64::try_from(entity_id).unwrap_or(0))],
        )?;
        let Some(SqlValue::Text(t)) = r.rows.first().and_then(|row| row.first()) else {
            return Ok(None);
        };
        let f: Value = serde_json::from_str(t).unwrap_or(Value::Null);
        let mut doc = serde_json::Map::new();
        for (k, _, _, _) in SEARCH_FIELDS {
            if let Some(v) = f.get(*k).filter(|v| !v.is_null()) {
                doc.insert(format!("anime_{k}"), v.clone());
            }
        }
        Ok(Some(json!(doc).to_string()))
    }
}

// ---------------- 元数据联动（content-types-common §8） ----------------

fn svc(service: &str, method: &str, args: &Value) -> Result<Value, PluginError> {
    let out = services::call(service, method, &args.to_string())?;
    serde_json::from_str(&out)
        .map_err(|_| err("type-anime.bad_service_reply", "invalid JSON from service", None))
}

impl on_event::Guest for TypeAnime {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> Result<(), PluginError> {
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        if evt.event_type == "group.created" && p["type_id"] == "anime" {
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

impl on_cron::Guest for TypeAnime {
    fn run(job: on_cron::JobInvocation) -> Result<(), PluginError> {
        if job.name != "fetch_metadata" {
            return Err(err("kernel.not_found", "unknown job", None));
        }
        let p: Value = serde_json::from_str(&job.payload).unwrap_or(Value::Null);
        let gid = p["group_id"].as_i64().unwrap_or(0);
        let g = svc("ctx.torrents", "get_group", &json!({ "id": gid }))?;
        let f = &g["fields"];
        // 目前只有 TMDB 元数据源；剧场版暂不补全
        match (f["tmdb_tv_id"].as_u64(), f["media"].as_str()) {
            (Some(id), Some(m)) if m != "movie" => enrich(gid, &format!("tv:{id}"), f["season"].as_i64()),
            _ => Ok(()),
        }
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
    let Some(show) = lookup(external, &locale)? else {
        return Ok(());
    };
    if show["kind"] != "tv" {
        return Ok(());
    }
    let season_data = match (show["ids"]["tmdb"].as_u64(), season.filter(|s| *s > 0)) {
        (Some(id), Some(n)) => lookup(&format!("tv:{id}:s{n}"), &locale)?,
        _ => None,
    };
    let mut patch = pt_type_kit::metadata::show_patch(&show, season_data.as_ref(), &locale);
    if let Some(o) = patch.as_object_mut() {
        o.remove("imdb_id");
        o.remove("countries");
    }
    svc(
        "ctx.torrents",
        "update_group",
        &json!({ "id": gid, "fields": patch }),
    )?;
    Ok(())
}

// ---------------- 上传钩子：文件列表与范围（type-tv LLD §1.2，动漫同） ----------------

fn on_before_upload(input: &Value) -> Result<hooks::HookDecision, PluginError> {
    // 剧场版没有范围，不检查
    if input["type_id"] != "anime" || input["fields"]["scope"].is_null() {
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
                code: "type-anime.scope_mismatch".into(),
                message: reason.clone(),
                details: Some(json!({ "scope": check.scope, "reason": reason }).to_string()),
            }),
        },
    )
}

impl hooks::Guest for TypeAnime {
    fn handle(c: hooks::HookCall) -> Result<hooks::HookDecision, PluginError> {
        let input: Value = serde_json::from_str(&c.input).unwrap_or(Value::Null);
        match c.hook.as_str() {
            "torrent.before_upload" => on_before_upload(&input),
            _ => Ok(hooks::HookDecision::Allow),
        }
    }
}

export!(TypeAnime);
