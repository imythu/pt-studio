//! type-generic 插件（WASM，docs/plugins/type-generic/）。
//!
//! 本版本实现：kind 定义（预设：游戏、软件、电子书、课程；后台新增 / 修改 / 停用）、编译为 JSON Schema（`info()` 为按 kind 判别的
//! `oneOf`）、规范化（含旧值映射）、分组键、规范标题、`torrents` 索引的 `generic_kind` 与 `generic_<kind>_<field>` 字段。
//! 电子书元数据：`group.created` / `torrent.created` → `fetch_metadata` 延迟任务 → 豆瓣图书（`douban_id` 或 ISBN）→
//! `ctx.torrents.update_group`（封面、简介、评分、作者、出版社……）。
//! 文件规则钩子、去重、词表管理接口在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

pt_plugin_sdk::world!("type-generic");

pub mod kinds;

use std::collections::BTreeMap;

use exports::pt::plugin::{content_type, http_handler, lifecycle, on_cron, on_event, search_contributor};
use kinds::KindDef;
use pt_plugin_sdk::{
    config, jobs, services,
    sql::{self, SqlValue},
    time,
    types::PluginError,
};
use serde_json::{Value, json};

struct TypeGeneric;

type R<T> = Result<T, PluginError>;

fn err(code: &str, message: &str, details: Option<Value>) -> PluginError {
    PluginError {
        code: code.into(),
        message: message.into(),
        details: details.map(|d| d.to_string()),
        retryable: false,
    }
}

fn text(v: &SqlValue) -> String {
    match v {
        SqlValue::Text(s) | SqlValue::Json(s) => s.clone(),
        _ => String::new(),
    }
}

fn max_fields() -> usize {
    let s: Value = serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null);
    usize::try_from(s["max_fields_per_kind"].as_u64().unwrap_or(30))
        .unwrap_or(30)
        .clamp(1, 30)
}

/// 已启用的 kind（`all = true` 含停用）
fn load(all: bool) -> R<Vec<(KindDef, bool)>> {
    let r = sql::query(
        &format!(
            "SELECT definition::text, active FROM kinds{} ORDER BY kind",
            if all { "" } else { " WHERE active" }
        ),
        &[],
    )?;
    Ok(r.rows
        .iter()
        .filter_map(|row| {
            let d: KindDef = serde_json::from_str(&text(&row[0])).ok()?;
            Some((d, matches!(row[1], SqlValue::Boolean(true))))
        })
        .collect())
}

fn kind_of(fields: &Value) -> R<KindDef> {
    let k = fields["kind"].as_str().unwrap_or("");
    load(false)?
        .into_iter()
        .map(|(d, _)| d)
        .find(|d| d.kind == k)
        .ok_or_else(|| {
            err(
                "type-generic.kind_invalid",
                "unknown or disabled kind",
                Some(json!({ "errors": [{ "path": "/kind" }] })),
            )
        })
}

fn vocab(kind: &str) -> R<BTreeMap<String, BTreeMap<String, String>>> {
    let r = sql::query(
        "SELECT field, alias, value FROM vocab WHERE kind = $1",
        &[SqlValue::Text(kind.into())],
    )?;
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for row in &r.rows {
        out.entry(text(&row[0]))
            .or_default()
            .insert(text(&row[1]), text(&row[2]));
    }
    Ok(out)
}

fn parse(fields: &str) -> R<Value> {
    serde_json::from_str(fields).map_err(|_| err("kernel.bad_request", "fields must be JSON", None))
}

fn upsert(d: &KindDef, preset: bool) -> R<()> {
    sql::execute(
        "INSERT INTO kinds (kind, definition, compiled, preset) VALUES ($1, $2, $3, $4)
         ON CONFLICT (kind) DO UPDATE SET definition = EXCLUDED.definition, compiled = EXCLUDED.compiled,
                version = kinds.version + 1, active = true, updated_at = now()",
        &[
            SqlValue::Text(d.kind.clone()),
            SqlValue::Json(serde_json::to_string(d).unwrap_or_default()),
            SqlValue::Json(kinds::compile(d).to_string()),
            SqlValue::Boolean(preset),
        ],
    )?;
    Ok(())
}

impl lifecycle::Guest for TypeGeneric {
    fn init() -> R<()> {
        if sql::query("SELECT 1 FROM kinds LIMIT 1", &[])?.rows.is_empty() {
            for p in kinds::presets() {
                upsert(&p, true)?;
            }
        }
        let presets = sql::query("SELECT definition::text FROM kinds WHERE preset", &[])?;
        for row in &presets.rows {
            if let Some(d) = serde_json::from_str::<KindDef>(&text(&row[0]))
                .ok()
                .as_ref()
                .and_then(kinds::upgrade_preset)
            {
                upsert(&d, true)?;
            }
        }
        Ok(())
    }
}

impl content_type::Guest for TypeGeneric {
    fn info() -> content_type::TypeInfo {
        let ks: Vec<KindDef> = load(false)
            .unwrap_or_default()
            .into_iter()
            .map(|(d, _)| d)
            .collect();
        let field_schema = json!({
            "type": "object",
            "required": ["kind"],
            "properties": { "kind": { "type": "string", "enum": ks.iter().map(|k| k.kind.clone()).collect::<Vec<_>>(),
                                      "x-pt-label-term": "type-generic.field.kind",
                                      "x-pt-enum-titles": ks.iter().map(|k| k.name_zh.clone().or_else(|| k.name_en.clone()).unwrap_or_else(|| k.kind.clone())).collect::<Vec<_>>() } },
            "oneOf": ks.iter().map(kinds::compile).collect::<Vec<_>>(),
        });
        content_type::TypeInfo {
            type_id: "generic".into(),
            name_term: "type-generic.name".into(),
            field_schema: field_schema.to_string(),
            group_schema: None,
        }
    }

    fn normalize(fields: String) -> R<String> {
        let f = parse(&fields)?;
        let k = kind_of(&f)?;
        kinds::normalize(&k, &f, &vocab(&k.kind)?)
            .map(|v| v.to_string())
            .map_err(|e| {
                err(
                    "type-generic.field_invalid",
                    "fields are invalid for this kind",
                    Some(e.details()),
                )
            })
    }

    fn group_key(fields: String) -> R<Option<String>> {
        let f = parse(&fields)?;
        Ok(kinds::group_key(&kind_of(&f)?, &f))
    }

    fn canonical_title(fields: String) -> R<String> {
        let f = parse(&fields)?;
        Ok(kinds::canonical_title(&kind_of(&f)?, &f))
    }
}

// ---------------- 搜索贡献（LLD §1.6） ----------------

fn search_type(ty: &str) -> &'static str {
    match ty {
        "integer" => "int",
        "number" => "float",
        "bool" => "bool",
        "multi_enum" => "string[]",
        _ => "string",
    }
}

impl search_contributor::Guest for TypeGeneric {
    fn index_schemas() -> Vec<search_contributor::IndexSchema> {
        let mut fields = vec![search_contributor::FieldSpec {
            name: "generic_kind".into(),
            field_type: "string".into(),
            searchable: false,
            filterable: true,
            sortable: false,
            rank: 0,
            exact: true,
            label_term: "type-generic.field.kind".into(),
        }];
        for (k, _) in load(false).unwrap_or_default() {
            for f in k
                .fields
                .iter()
                .filter(|f| !f.deprecated && (f.filter != "none" || f.search != "none"))
            {
                let ty = search_type(&f.ty);
                fields.push(search_contributor::FieldSpec {
                    name: format!("generic_{}_{}", k.kind, f.key),
                    field_type: ty.into(),
                    searchable: f.search == "searchable",
                    filterable: f.filter != "none" || f.search == "exact",
                    sortable: f.filter == "range" && ty != "string",
                    rank: 3,
                    exact: f.search == "exact",
                    label_term: if f.filter == "none" {
                        String::new()
                    } else {
                        format!("type-generic.field.{}.{}", k.kind, f.key)
                    },
                });
            }
        }
        vec![search_contributor::IndexSchema {
            index: "torrents".into(),
            fields,
            synonyms: vec![],
        }]
    }

    fn to_document(index: String, entity_id: u64) -> R<Option<String>> {
        if index != "torrents" {
            return Ok(None);
        }
        let r = sql::query(
            "SELECT fields::text FROM torrent.v_torrent_fields WHERE id = $1 AND type_id = 'generic'",
            &[SqlValue::Int(i64::try_from(entity_id).unwrap_or(0))],
        )?;
        let Some(row) = r.rows.first() else {
            return Ok(None);
        };
        let f: Value = serde_json::from_str(&text(&row[0])).unwrap_or(Value::Null);
        let Some(kind) = f["kind"].as_str() else {
            return Ok(Some("{}".into()));
        };
        let mut doc = serde_json::Map::new();
        doc.insert("generic_kind".into(), json!(kind));
        if let Some((k, _)) = load(false)?.into_iter().find(|(k, _)| k.kind == kind) {
            for fd in k
                .fields
                .iter()
                .filter(|x| !x.deprecated && (x.filter != "none" || x.search != "none"))
            {
                if let Some(v) = f.get(&fd.key).filter(|v| !v.is_null()) {
                    doc.insert(format!("generic_{kind}_{}", fd.key), v.clone());
                }
            }
        }
        Ok(Some(Value::Object(doc).to_string()))
    }
}

// ---------------- HTTP（LLD §2） ----------------

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

fn kind_json(d: &KindDef, active: bool) -> Value {
    json!({ "kind": d.kind, "name_zh": d.name_zh, "name_en": d.name_en, "icon": d.icon, "active": active,
            "schema": kinds::compile(d), "definition": d })
}

fn body(req: &http_handler::Request) -> R<Value> {
    serde_json::from_slice(&req.body).map_err(|_| err("kernel.bad_request", "invalid JSON body", None))
}

fn define(v: Value) -> R<KindDef> {
    let d: KindDef = serde_json::from_value(v).map_err(|e| {
        err(
            "type-generic.kind_definition_invalid",
            &format!("definition: {e}"),
            None,
        )
    })?;
    kinds::validate(&d, max_fields()).map_err(|e| {
        err(
            "type-generic.kind_definition_invalid",
            "kind definition is invalid",
            Some(e.details()),
        )
    })?;
    Ok(d)
}

impl http_handler::Guest for TypeGeneric {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let param = || {
            req.params
                .iter()
                .find(|(k, _)| k == "kind")
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        match req.route_id.as_str() {
            "list_kinds" => {
                let items: Vec<Value> = load(false)?.iter().map(|(d, a)| kind_json(d, *a)).collect();
                Ok(resp(200, &json!({ "items": items })))
            }
            "create_kind" => {
                let d = define(body(&req)?)?;
                if load(true)?.iter().any(|(k, _)| k.kind == d.kind) {
                    return Err(err("type-generic.kind_exists", "kind already exists", None));
                }
                if load(true)?.len() >= 50 {
                    return Err(err("type-generic.limit_exceeded", "too many kinds", None));
                }
                upsert(&d, false)?;
                Ok(resp(201, &kind_json(&d, true)))
            }
            "update_kind" => {
                let kind = param();
                let mut v = body(&req)?;
                v["kind"] = json!(kind);
                if !load(true)?.iter().any(|(k, _)| k.kind == kind) {
                    return Err(err("kernel.not_found", "kind not found", None));
                }
                let d = define(v)?;
                upsert(&d, false)?;
                Ok(resp(200, &kind_json(&d, true)))
            }
            "disable_kind" => {
                let n = sql::execute(
                    "UPDATE kinds SET active = false, updated_at = now() WHERE kind = $1 AND active",
                    &[SqlValue::Text(param())],
                )?;
                if n == 0 {
                    return Err(err("kernel.not_found", "kind not found", None));
                }
                Ok(http_handler::Response {
                    status: 204,
                    headers: vec![],
                    body: vec![],
                })
            }
            _ => Err(err("kernel.not_found", "no such route", None)),
        }
    }
}

// ---------------- 元数据联动（content-types-common §8）：电子书 ← 豆瓣图书 ----------------

fn svc(service: &str, method: &str, args: &Value) -> R<Value> {
    let out = services::call(service, method, &args.to_string())?;
    serde_json::from_str(&out).map_err(|_| {
        err(
            "type-generic.bad_service_reply",
            "invalid JSON from service",
            None,
        )
    })
}

impl on_event::Guest for TypeGeneric {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        let fresh_torrent = evt.event_type == "torrent.created" && p["group_id"].as_i64().is_some();
        // kind 只在种子字段中，任务里再判断是否为电子书
        if (evt.event_type == "group.created" || fresh_torrent) && p["type_id"] == "generic" {
            let gid = p["group_id"].as_i64().unwrap_or(0);
            jobs::schedule_once(
                "fetch_metadata",
                time::now(),
                &json!({ "group_id": gid }).to_string(),
                Some(&match p["torrent_id"].as_i64() {
                    Some(tid) if fresh_torrent => format!("meta:{gid}:t{tid}"),
                    _ => format!("meta:{gid}"),
                }),
            )?;
        }
        Ok(())
    }
}

impl on_cron::Guest for TypeGeneric {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        if job.name != "fetch_metadata" {
            return Err(err("kernel.not_found", "unknown job", None));
        }
        let p: Value = serde_json::from_str(&job.payload).unwrap_or(Value::Null);
        let gid = p["group_id"].as_i64().unwrap_or(0);
        // Group 不保存通用类型的字段，从组内最新的种子取豆瓣编号 / ISBN
        let rows = sql::query(
            "SELECT fields::text FROM torrent.v_torrent_fields WHERE group_id = $1 AND type_id = 'generic' ORDER BY id DESC LIMIT 20",
            &[SqlValue::Int(gid)],
        )?;
        let Some(key) = rows
            .rows
            .iter()
            .find_map(|r| kinds::book_lookup_key(&serde_json::from_str(&text(&r[0])).unwrap_or(Value::Null)))
        else {
            return Ok(());
        };
        // 豆瓣未安装 / 未启用 / 查无结果：不重试
        let data = match svc(
            "ctx.metadata",
            "lookup",
            &json!({ "source": "douban", "external_id": key, "locale": "zh-CN" }),
        ) {
            Ok(r) if r.is_null() => return Ok(()),
            Ok(r) => r["data"].clone(),
            Err(e) if !e.retryable => return Ok(()),
            Err(e) => return Err(e),
        };
        let Some(patch) = kinds::book_patch(&data) else {
            return Ok(());
        };
        let g = svc("ctx.torrents", "get_group", &json!({ "id": gid }))?;
        let mut args = json!({ "id": gid, "fields": patch });
        if g["year"].is_null() {
            args["year"] = data["year"].clone();
        }
        svc("ctx.torrents", "update_group", &args)?;
        Ok(())
    }
}

export!(TypeGeneric);
