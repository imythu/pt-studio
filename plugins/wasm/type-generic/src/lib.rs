//! type-generic 插件（WASM，docs/plugins/type-generic/）。
//!
//! 本版本实现：kind 定义（预设：游戏、软件、电子书、课程；后台新增 / 修改 / 停用）、编译为 JSON Schema（`info()` 为按 kind 判别的
//! `oneOf`）、规范化（含旧值映射）、分组键、规范标题、`torrents` 索引的 `generic_kind` 与 `generic_<kind>_<field>` 字段。
//! 文件规则钩子、去重、元数据绑定、词表管理接口在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

pt_plugin_sdk::world!("type-generic");

pub mod kinds;

use std::collections::BTreeMap;

use exports::pt::plugin::{content_type, http_handler, lifecycle, search_contributor};
use kinds::KindDef;
use pt_plugin_sdk::{
    config,
    sql::{self, SqlValue},
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

export!(TypeGeneric);
