//! mediainfo 插件（WASM，docs/plugins/mediainfo/README.md）：MediaInfo / BDInfo 解析。
//!
//! - 发种者或管理员粘贴 MediaInfo（文本 / JSON）或 BDInfo，插件脱敏（去掉目录与用户名）后按种子保存，
//!   解析出规格摘要（分辨率、编码、HDR、音轨、字幕语言、时长），与发布时声明的字段比较，列出不一致项。
//! - `POST /v1/parse` 只预览不保存（发种页边写边看）；种子删除时清理。
//! - 服务 `ctx.mediainfo`：`parse`（预览）、`compare`（摘要与字段比较）、`get`（读取某种子的摘要，供审核 / 规则插件使用）。

pt_plugin_sdk::world!("mediainfo", http);

mod parse;

use exports::pt::plugin::{http_handler, on_event, service_provider};
use pt_plugin_sdk::{prelude::*, serde_json, sql};
use sha2::{Digest, Sha256};

struct MediaInfo;

struct Parsed {
    text: String,
    format: String,
    summary: Value,
    sections: Value,
}

fn analyse(text: &str) -> R<Parsed> {
    if text.len() > parse::MAX_LEN {
        return Err(err("mediainfo.too_large", "the report is larger than 256 KiB"));
    }
    let (format, mut secs) = parse::detect_and_parse(text).map_err(|_| err("mediainfo.unrecognized", "not a MediaInfo or BDInfo report"))?;
    parse::sanitize_sections(&mut secs);
    let clean = if format == "json" {
        // JSON 原文按解析后的段重新输出，不保留路径字段
        serde_json::to_string_pretty(&parse::sections_json(&secs)).unwrap_or_default()
    } else {
        parse::sanitize_text(text.trim_start_matches('\u{feff}'))
    };
    Ok(Parsed { text: clean, summary: parse::summary(&secs), sections: parse::sections_json(&secs), format })
}

fn declared_fields(tid: i64) -> R<Option<Value>> {
    Ok(sql::query("SELECT fields FROM torrent.v_torrent_fields WHERE id = $1", &[SqlValue::Int(tid)])?
        .rows
        .first()
        .map(|r| r.json(0).unwrap_or(Value::Null)))
}

fn may_edit(uid: i64, tid: i64) -> R<bool> {
    let r = sql::query("SELECT uploader_id FROM torrent.v_torrent_tracker WHERE id = $1 AND deleted_at IS NULL", &[SqlValue::Int(tid)])?;
    let Some(row) = r.rows.first() else {
        return Err(err("kernel.not_found", "torrent not found"));
    };
    Ok(row.int(0) == Some(uid) || is_staff(uid))
}

fn is_staff(uid: i64) -> bool {
    ctx::call("ctx.rbac", "check", &json!({ "user": uid, "permission": "mediainfo.manage" }))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn save(uid: i64, tid: i64, text: &str) -> R<Value> {
    if !may_edit(uid, tid)? {
        return Err(err("kernel.forbidden", "only the uploader or staff can edit the MediaInfo"));
    }
    let p = analyse(text)?;
    let fields = declared_fields(tid)?.unwrap_or(Value::Null);
    let diffs = Value::Array(parse::compare(&p.summary, &fields));
    let hash = Sha256::digest(p.text.as_bytes()).to_vec();
    sql::execute(
        "INSERT INTO results (torrent_id, text, text_hash, format, summary, sections, diffs, parser_version, updated_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         ON CONFLICT (torrent_id) DO UPDATE SET text = EXCLUDED.text, text_hash = EXCLUDED.text_hash, format = EXCLUDED.format,
                summary = EXCLUDED.summary, sections = EXCLUDED.sections, diffs = EXCLUDED.diffs,
                parser_version = EXCLUDED.parser_version, updated_by = EXCLUDED.updated_by, parsed_at = now()",
        &[
            SqlValue::Int(tid),
            SqlValue::Text(p.text.clone()),
            SqlValue::Bytes(hash),
            SqlValue::Text(p.format.clone()),
            SqlValue::Json(p.summary.to_string()),
            SqlValue::Json(p.sections.to_string()),
            SqlValue::Json(diffs.to_string()),
            SqlValue::Int(parse::PARSER_VERSION),
            SqlValue::Int(uid),
        ],
    )?;
    ctx::publish("mediainfo.updated", &format!("torrent:{tid}"), &json!({ "torrent_id": tid, "diffs": diffs.as_array().map_or(0, Vec::len) }))?;
    Ok(json!({ "torrent_id": tid, "format": p.format, "summary": p.summary, "sections": p.sections, "diffs": diffs, "text": p.text }))
}

fn get(tid: i64, with_text: bool) -> R<Option<Value>> {
    let r = sql::query(
        "SELECT format, summary, sections, diffs, CASE WHEN $2 THEN text END, (extract(epoch FROM parsed_at) * 1000)::bigint
           FROM results WHERE torrent_id = $1",
        &[SqlValue::Int(tid), SqlValue::Boolean(with_text)],
    )?;
    Ok(r.rows.first().map(|row| {
        json!({ "torrent_id": tid, "format": row.text(0), "summary": row.json(1), "sections": row.json(2), "diffs": row.json(3),
                "text": row.text(4), "parsed_at": row.int(5) })
    }))
}

// ---------------- 导出 ----------------

impl on_event::Guest for MediaInfo {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        if evt.event_type != "torrent.deleted" {
            return Ok(());
        }
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        if let Some(tid) = p["torrent_id"].as_i64() {
            sql::execute("DELETE FROM results WHERE torrent_id = $1", &[SqlValue::Int(tid)])?;
        }
        Ok(())
    }
}

impl service_provider::Guest for MediaInfo {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        match method.as_str() {
            "parse" => {
                let p = analyse(a["text"].as_str().unwrap_or(""))?;
                Ok(json!({ "format": p.format, "summary": p.summary }).to_string())
            }
            "compare" => Ok(Value::Array(parse::compare(&a["summary"], &a["fields"])).to_string()),
            "get" => {
                let tid = a["torrent_id"].as_i64().ok_or_else(|| err("kernel.validation_failed", "torrent_id"))?;
                Ok(get(tid, false)?.unwrap_or(Value::Null).to_string())
            }
            other => Err(err("kernel.not_found", &format!("ctx.mediainfo has no method `{other}`"))),
        }
    }
}

impl http_handler::Guest for MediaInfo {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let reply = match req.route() {
            "parse" => {
                let b = req.body_value();
                let p = analyse(b["text"].as_str().unwrap_or(""))?;
                let diffs = b.get("fields").map_or(Value::Array(vec![]), |f| Value::Array(parse::compare(&p.summary, f)));
                http::Reply::ok(&json!({ "format": p.format, "summary": p.summary, "sections": p.sections, "diffs": diffs, "text": p.text }))
            }
            "put" => {
                let b = req.body_value();
                http::Reply::ok(&save(ctx::current_user()?, req.param_i64("id")?, b["text"].as_str().unwrap_or(""))?)
            }
            "delete" => {
                let (uid, tid) = (ctx::current_user()?, req.param_i64("id")?);
                if !may_edit(uid, tid)? {
                    return Err(err("kernel.forbidden", "only the uploader or staff can edit the MediaInfo"));
                }
                sql::execute("DELETE FROM results WHERE torrent_id = $1", &[SqlValue::Int(tid)])?;
                http::Reply::no_content()
            }
            "get" => {
                let tid = req.param_i64("id")?;
                let editable = ctx::current_user_opt().is_some_and(|uid| may_edit(uid, tid).unwrap_or(false));
                let r = get(tid, true)?;
                http::Reply::ok(&json!({ "result": r, "editable": editable }))
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(MediaInfo);
