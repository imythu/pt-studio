//! type-music 插件（WASM，docs/plugins/type-music/）：`content-type`、`search-contributor`、`torrent.before_upload` 钩子、
//! 抓轨日志检查（上传前预检接口与 `torrent.created` / `torrent.updated` 后的异步检查）。
//!
//! 本版本实现：字段与 Group schema（艺人按角色分列）、规范化（同义词、格式与编码组合、MusicBrainz 链接、发布名补全）、
//! 分组键（MusicBrainz 发行组 > 主艺人 + 专辑 + 原始年份）、版本键、作品标题、规范标题、文件检查（音频 / log / cue /
//! 可执行文件）、log 基础评分（`log_results` 表，分数写回种子字段 `log_score`）、`music_*` 搜索字段。去重与替代、
//! MusicBrainz 元数据、艺人页在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

wit_bindgen::generate!({
    path: "wit",
    world: "type-music",
    generate_all,
});

pub mod logcheck;
pub mod music;

use exports::pt::plugin::{content_type, hooks, http_handler, on_cron, on_event, search_contributor};
use pt::plugin::{
    config, jobs, services,
    sql::{self, SqlValue},
    time,
    types::PluginError,
};
use serde_json::{Value, json};

struct TypeMusic;

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

fn settings() -> Value {
    serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null)
}

impl content_type::Guest for TypeMusic {
    fn info() -> content_type::TypeInfo {
        content_type::TypeInfo {
            type_id: "music".into(),
            name_term: "type-music.name".into(),
            field_schema: music::field_schema().to_string(),
            group_schema: Some(music::group_schema().to_string()),
        }
    }

    fn normalize(fields: String) -> Result<String, PluginError> {
        match music::normalize(&parse(&fields)?) {
            Ok(v) => Ok(v.to_string()),
            Err(e) => {
                let code = e.details()["errors"][0]["code"]
                    .as_str()
                    .unwrap_or("type-music.field_invalid")
                    .to_owned();
                Err(err(&code, "music fields are invalid", Some(e.details())))
            }
        }
    }

    fn group_key(fields: String) -> Result<Option<String>, PluginError> {
        Ok(music::group_key(&parse(&fields)?))
    }

    fn canonical_title(fields: String) -> Result<String, PluginError> {
        let s = settings();
        Ok(music::canonical_title(
            &parse(&fields)?,
            s["title_template"].as_str(),
        ))
    }
}

// ---------------- 搜索贡献（LLD §1.5） ----------------

const SEARCH_FIELDS: &[(&str, &str, bool, bool)] = &[
    // (字段, 类型, filterable, searchable)
    ("format", "string", true, false),
    ("encoding", "string", true, false),
    ("media", "string", true, false),
    ("release_type", "string", true, false),
    ("has_log", "bool", true, false),
    ("has_cue", "bool", true, false),
    ("scene", "bool", true, false),
    ("log_score", "int", true, false),
    ("edition_year", "int", true, false),
    ("original_year", "int", true, false),
    ("edition_label", "string", false, true),
    ("edition_catalog", "string", true, true),
    ("artists", "string[]", true, true),
];

impl search_contributor::Guest for TypeMusic {
    fn index_schemas() -> Vec<search_contributor::IndexSchema> {
        vec![search_contributor::IndexSchema {
            index: "torrents".into(),
            fields: SEARCH_FIELDS
                .iter()
                .map(|(f, ty, filterable, searchable)| search_contributor::FieldSpec {
                    name: format!("music_{f}"),
                    field_type: (*ty).into(),
                    searchable: *searchable,
                    filterable: *filterable,
                    sortable: matches!(*f, "log_score" | "original_year" | "edition_year"),
                    rank: 2,
                    exact: *f == "edition_catalog",
                    label_term: if *filterable {
                        format!("type-music.field.{f}")
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
        let Some(f) = torrent_fields(i64::try_from(entity_id).unwrap_or(0))? else {
            return Ok(None);
        };
        let mut doc = serde_json::Map::new();
        for (k, _, _, _) in SEARCH_FIELDS {
            if let Some(v) = f.get(*k).filter(|v| !v.is_null()) {
                doc.insert(format!("music_{k}"), v.clone());
            }
        }
        Ok(Some(json!(doc).to_string()))
    }
}

fn torrent_fields(id: i64) -> Result<Option<Value>, PluginError> {
    let r = sql::query(
        "SELECT fields::text FROM torrent.v_torrent_fields WHERE id = $1 AND type_id = 'music'",
        &[SqlValue::Int(id)],
    )?;
    Ok(match r.rows.first().and_then(|row| row.first()) {
        Some(SqlValue::Text(t)) => serde_json::from_str(t).ok(),
        _ => None,
    })
}

// ---------------- 上传钩子：文件检查（LLD §1.3） ----------------

impl hooks::Guest for TypeMusic {
    fn handle(c: hooks::HookCall) -> Result<hooks::HookDecision, PluginError> {
        let input: Value = serde_json::from_str(&c.input).unwrap_or(Value::Null);
        if c.hook != "torrent.before_upload" || input["type_id"] != "music" {
            return Ok(hooks::HookDecision::Allow);
        }
        let files: Vec<(String, u64)> = input["files_sample"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| Some((x["path"].as_str()?.to_owned(), x["size"].as_u64()?)))
                    .collect()
            })
            .unwrap_or_default();
        Ok(match music::check_files(&input["fields"], &files) {
            Ok(()) => hooks::HookDecision::Allow,
            Err((code, message)) => hooks::HookDecision::Deny(hooks::DenyInfo {
                code: code.into(),
                message,
                details: None,
            }),
        })
    }
}

// ---------------- log 预检接口（LLD §2） ----------------

fn resp(status: u16, v: &Value) -> http_handler::Response {
    http_handler::Response {
        status,
        headers: vec![http_handler::Header {
            name: "content-type".into(),
            value: "application/json".into(),
        }],
        body: v.to_string().into_bytes(),
    }
}

impl http_handler::Guest for TypeMusic {
    fn handle(req: http_handler::Request) -> Result<http_handler::Response, PluginError> {
        if req.route_id != "log_check" {
            return Err(err("kernel.not_found", "no such route", None));
        }
        let b: Value = serde_json::from_slice(&req.body)
            .map_err(|_| err("kernel.bad_request", "body must be JSON", None))?;
        let Some(log) = b["log"].as_str() else {
            return Err(err("type-music.field_invalid", "`log` is required", None));
        };
        if log.len() > logcheck::MAX_LOG_BYTES {
            return Err(err(
                "type-music.log_too_large",
                "a log must not exceed 512 KiB",
                None,
            ));
        }
        Ok(resp(200, &logcheck::check(log).to_json()))
    }
}

// ---------------- 异步 log 检查（LLD §1.4） ----------------

fn svc(service: &str, method: &str, args: &Value) -> Result<Value, PluginError> {
    let out = services::call(service, method, &args.to_string())?;
    serde_json::from_str(&out)
        .map_err(|_| err("type-music.bad_service_reply", "invalid JSON from service", None))
}

impl on_event::Guest for TypeMusic {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> Result<(), PluginError> {
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        let id = p["torrent_id"].as_i64().unwrap_or(0);
        let relevant = match evt.event_type.as_str() {
            "torrent.created" => p["type_id"] == "music",
            // 编辑字段后重新检查（检查任务幂等：分数不变不写回，所以写回本身引起的更新不会循环）
            "torrent.updated" => {
                p["fields"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|f| f == "fields"))
                    && torrent_fields(id)?.is_some()
            }
            _ => false,
        };
        if relevant && settings()["log_check_enabled"].as_bool() != Some(false) {
            jobs::schedule_once(
                "check_logs",
                time::now(),
                &json!({ "torrent_id": id }).to_string(),
                Some(&format!("logs:{id}")),
            )?;
        }
        Ok(())
    }
}

fn check_logs(id: i64) -> Result<(), PluginError> {
    let Some(f) = torrent_fields(id)? else {
        return Ok(());
    };
    let logs: Vec<&str> = f["logs"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let reports: Vec<logcheck::Report> = logs.iter().map(|l| logcheck::check(l)).collect();
    sql::execute(
        "DELETE FROM log_results WHERE torrent_id = $1 AND log_index >= $2",
        &[
            SqlValue::Int(id),
            SqlValue::Int(i64::try_from(reports.len()).unwrap_or(0)),
        ],
    )?;
    for (i, r) in reports.iter().enumerate() {
        let opt = |v: Option<String>| v.map_or(SqlValue::Null, SqlValue::Text);
        sql::execute(
            "INSERT INTO log_results (torrent_id, log_index, software, software_version, score, issues, checker_version, checked_at)
             VALUES ($1, $2, $3, $4, $5, $6::jsonb, $7, now())
             ON CONFLICT (torrent_id, log_index) DO UPDATE SET software = EXCLUDED.software, software_version = EXCLUDED.software_version,
                 score = EXCLUDED.score, issues = EXCLUDED.issues, checker_version = EXCLUDED.checker_version, checked_at = now()",
            &[
                SqlValue::Int(id),
                SqlValue::Int(i64::try_from(i).unwrap_or(0)),
                opt(r.software.map(str::to_owned)),
                opt(r.version.clone()),
                r.score.map_or(SqlValue::Null, SqlValue::Int),
                SqlValue::Text(r.to_json()["issues"].to_string()),
                SqlValue::Int(logcheck::CHECKER_VERSION),
            ],
        )?;
    }
    // 多个 log（多碟）取最低分；全部无法识别 → 无分数
    let score = reports.iter().filter_map(|r| r.score).min();
    if f["log_score"].as_i64() != score {
        svc(
            "ctx.torrents",
            "update_fields",
            &json!({ "id": id, "fields": { "log_score": score } }),
        )?;
    }
    Ok(())
}

impl on_cron::Guest for TypeMusic {
    fn run(job: on_cron::JobInvocation) -> Result<(), PluginError> {
        if job.name != "check_logs" {
            return Err(err("kernel.not_found", "unknown job", None));
        }
        let p: Value = serde_json::from_str(&job.payload).unwrap_or(Value::Null);
        check_logs(p["torrent_id"].as_i64().unwrap_or(0))
    }
}

export!(TypeMusic);
