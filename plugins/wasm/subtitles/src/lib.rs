//! subtitles 插件（WASM，docs/plugins/subtitles/）：字幕区（种子级）。
//!
//! - 上传：请求体为文件原始字节，文件名 / 语言 / 说明在查询参数中；SRT / ASS / SSA / VTT 按内容校验并记录编码，
//!   SUP 校验文件头，ZIP 只读中央目录检查条目（只允许字幕相关文件、无加密、无路径穿越、解压总量有界），7z / RAR 不接受。
//! - 文件按 SHA-256 存 blob `files/<sha256>`（非公开），经下载接口（需 `subtitles.download`）返回并计数。
//! - 删除：上传者或 `subtitles.moderate`（填写原因、记审计）；种子删除时一并删除；没有引用的文件随之删除。
//! - 可选上传奖励（`reward_bonus`，经 `ctx.ledger` 幂等发放）。服务 `ctx.subtitles`：`count_many`、`list_for`。

pt_plugin_sdk::world!("subtitles", http);

mod sniff;

use exports::pt::plugin::{http_handler, on_event, service_provider};
use pt_plugin_sdk::{audit, blob, prelude::*, serde_json, services, sql};
use sha2::{Digest, Sha256};

struct Subtitles;

pub const LANGS: &[&str] = &["zh-hans", "zh-hant", "zh-hans-en", "zh-hant-en", "en", "ja", "ko", "other"];

struct Settings {
    max_bytes: usize,
    max_entries: usize,
    max_extracted: u64,
    per_torrent: i64,
    reward: i64,
}

fn settings() -> Settings {
    let s = ctx::settings();
    let int = |k: &str, d: i64| s[k].as_i64().unwrap_or(d).max(0);
    Settings {
        max_bytes: usize::try_from(int("max_size_kb", 5120).max(1) * 1024).unwrap_or(5 << 20),
        max_entries: usize::try_from(int("max_archive_entries", 100).max(1)).unwrap_or(100),
        max_extracted: u64::try_from(int("max_extracted_mb", 50).max(1)).unwrap_or(50) * 1024 * 1024,
        per_torrent: int("max_per_torrent", 50).max(1),
        reward: int("reward_bonus", 0),
    }
}

fn one(q: &str, p: &[SqlValue]) -> R<i64> {
    Ok(sql::query(q, p)?.rows.first().and_then(|r| r.int(0)).unwrap_or(0))
}

fn is_staff(uid: i64) -> bool {
    ctx::call("ctx.rbac", "check", &json!({ "user": uid, "permission": "subtitles.moderate" }))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// 文件名只保留最后一段，去掉控制字符与引号，最长 200 字符
pub fn clean_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    base.chars().filter(|c| !c.is_control() && !matches!(c, '"' | '\'' | ';')).take(200).collect::<String>().trim().to_owned()
}

/// Content-Disposition：ASCII 回退名 + RFC 5987 UTF-8 名
pub fn disposition(name: &str) -> String {
    let ascii: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || "._- ()[]".contains(c) { c } else { '_' }).collect();
    let enc: String = name
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"._-".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect();
    format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{enc}")
}

struct UploadArgs<'a> {
    filename: &'a str,
    lang: &'a str,
    title: &'a str,
    anonymous: bool,
}

fn upload(uid: i64, tid: i64, a: &UploadArgs<'_>, body: &[u8]) -> R<Value> {
    let s = settings();
    if one("SELECT count(*) FROM torrent.v_torrent_public WHERE id = $1 AND status = 'approved'", &[SqlValue::Int(tid)])? == 0 {
        return Err(err("kernel.not_found", "torrent not found"));
    }
    if !LANGS.contains(&a.lang) {
        return Err(err("kernel.validation_failed", "unknown language"));
    }
    let filename = clean_filename(a.filename);
    if filename.is_empty() {
        return Err(err("kernel.validation_failed", "filename is required"));
    }
    let title: String = a.title.trim().chars().take(200).collect();
    if body.is_empty() {
        return Err(err("kernel.validation_failed", "the request body must be the subtitle file"));
    }
    if body.len() > s.max_bytes {
        return Err(err("subtitles.too_large", &format!("subtitle files must be at most {} KiB", s.max_bytes / 1024)));
    }
    let c = sniff::check(&filename, body, s.max_entries, s.max_extracted).map_err(|m| err("subtitles.invalid_file", m))?;
    if one("SELECT count(*) FROM subtitles WHERE torrent_id = $1 AND deleted_at IS NULL", &[SqlValue::Int(tid)])? >= s.per_torrent {
        return Err(err("subtitles.limit_reached", "this torrent already has too many subtitles"));
    }
    let digest = Sha256::digest(body).to_vec();
    if one("SELECT count(*) FROM subtitles WHERE torrent_id = $1 AND sha256 = $2 AND deleted_at IS NULL", &[SqlValue::Int(tid), SqlValue::Bytes(digest.clone())])? > 0 {
        return Err(err("subtitles.duplicate", "this subtitle file was already uploaded for this torrent"));
    }
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let key = format!("files/{hex}");
    if one("SELECT count(*) FROM subtitles WHERE blob_key = $1 AND deleted_at IS NULL", &[SqlValue::Text(key.clone())])? == 0 {
        blob::put(&key, "application/octet-stream", body)?;
    }
    let entries: Vec<Value> = c.entries.iter().map(|(n, sz)| json!({ "name": n, "size": sz })).collect();
    let id = one(
        "INSERT INTO subtitles (torrent_id, uploader_id, is_anonymous, lang, title, filename, format, encoding, entries, size, sha256, blob_key)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) RETURNING id",
        &[
            SqlValue::Int(tid),
            SqlValue::Int(uid),
            SqlValue::Boolean(a.anonymous),
            SqlValue::Text(a.lang.into()),
            SqlValue::Text(title),
            SqlValue::Text(filename.clone()),
            SqlValue::Text(c.format.into()),
            c.encoding.map_or(SqlValue::Null, |e| SqlValue::Text(e.into())),
            SqlValue::Json(Value::Array(entries).to_string()),
            SqlValue::Int(i64::try_from(body.len()).unwrap_or(0)),
            SqlValue::Bytes(digest),
            SqlValue::Text(key),
        ],
    )?;
    ctx::publish("subtitle.uploaded", &format!("torrent:{tid}"), &json!({ "subtitle_id": id, "torrent_id": tid, "uploader_id": uid, "lang": a.lang }))?;
    if s.reward > 0 {
        // 奖励失败不影响上传（ledger 未安装等）
        let _ = services::call(
            "ctx.ledger",
            "transfer",
            &json!({ "currency": "bonus", "from_user": null, "to_user": uid, "amount": s.reward.saturating_mul(pt_plugin_sdk::ctx::currency_unit("bonus")), "reason": "subtitles.upload_reward",
                     "memo": format!("subtitle:{id}"), "idempotency_key": format!("subtitles:reward:{id}") })
            .to_string(),
        );
    }
    Ok(json!({ "id": id, "format": c.format, "encoding": c.encoding, "filename": filename }))
}

fn list(tid: i64, uid: i64) -> R<Value> {
    let staff = is_staff(uid);
    let r = sql::query(
        "SELECT s.id, s.lang, s.title, s.filename, s.format, s.encoding, s.size, s.downloads, s.uploader_id, s.is_anonymous, u.username,
                (extract(epoch FROM s.created_at) * 1000)::bigint, s.entries
           FROM subtitles s LEFT JOIN identity.v_user_public u ON u.id = s.uploader_id
          WHERE s.torrent_id = $1 AND s.deleted_at IS NULL ORDER BY s.lang, s.created_at DESC LIMIT 200",
        &[SqlValue::Int(tid)],
    )?;
    let items: Vec<Value> = r
        .rows
        .iter()
        .map(|row| {
            let owner = row.int(8).unwrap_or(0);
            let show = !row.bool(9).unwrap_or(false) || staff || owner == uid;
            json!({ "id": row.int(0), "lang": row.text(1), "title": row.text(2), "filename": row.text(3), "format": row.text(4),
                    "encoding": row.text(5), "size": row.int(6), "downloads": row.int(7),
                    "uploader": if show { row.text(10) } else { None }, "anonymous": row.bool(9),
                    "created_at": row.int(11), "entries": row.json(12), "can_delete": owner == uid || staff })
        })
        .collect();
    Ok(json!({ "items": items, "langs": LANGS }))
}

fn download(id: i64) -> R<http::Reply> {
    let r = sql::query(
        "UPDATE subtitles SET downloads = downloads + 1 WHERE id = $1 AND deleted_at IS NULL RETURNING blob_key, filename",
        &[SqlValue::Int(id)],
    )?;
    let Some(row) = r.rows.first() else { return Err(err("kernel.not_found", "subtitle not found")) };
    let key = row.text(0).unwrap_or_default();
    let name = row.text(1).unwrap_or_default();
    let data = blob::get(&key)?.ok_or_else(|| err("kernel.not_found", "subtitle file is missing"))?;
    Ok(http::Reply::bytes(200, "application/octet-stream", data)
        .header("content-disposition", &disposition(&name))
        .header("x-content-type-options", "nosniff")
        .header("cache-control", "private, no-store"))
}

/// 软删除并在没有其他引用时删除文件；返回 (torrent_id, uploader_id)
fn remove(id: i64, by: Option<i64>, reason: &str) -> R<Option<(i64, i64)>> {
    let r = sql::query(
        "UPDATE subtitles SET deleted_at = now(), deleted_by = $2, delete_reason = $3 WHERE id = $1 AND deleted_at IS NULL
         RETURNING torrent_id, uploader_id, blob_key",
        &[SqlValue::Int(id), by.map_or(SqlValue::Null, SqlValue::Int), SqlValue::Text(reason.chars().take(200).collect())],
    )?;
    let Some(row) = r.rows.first() else { return Ok(None) };
    let key = row.text(2).unwrap_or_default();
    if one("SELECT count(*) FROM subtitles WHERE blob_key = $1 AND deleted_at IS NULL", &[SqlValue::Text(key.clone())])? == 0 {
        blob::delete(&key)?;
    }
    let (tid, owner) = (row.int(0).unwrap_or(0), row.int(1).unwrap_or(0));
    ctx::publish("subtitle.deleted", &format!("torrent:{tid}"), &json!({ "subtitle_id": id, "torrent_id": tid, "uploader_id": owner, "reason": reason }))?;
    Ok(Some((tid, owner)))
}

fn delete(uid: i64, id: i64, reason: &str) -> R<()> {
    let owner = sql::query("SELECT uploader_id FROM subtitles WHERE id = $1 AND deleted_at IS NULL", &[SqlValue::Int(id)])?
        .rows
        .first()
        .and_then(|r| r.int(0))
        .ok_or_else(|| err("kernel.not_found", "subtitle not found"))?;
    if owner != uid {
        if !is_staff(uid) {
            return Err(err("kernel.forbidden", "only the uploader or staff can delete this subtitle"));
        }
        if reason.trim().is_empty() {
            return Err(err("kernel.validation_failed", "a reason is required"));
        }
    }
    let reason = if reason.trim().is_empty() { "uploader" } else { reason.trim() };
    remove(id, Some(uid), reason)?;
    if owner != uid {
        audit::write(&audit::AuditEntry {
            action: "subtitles.delete".into(),
            target: format!("subtitle:{id}"),
            details: json!({ "owner_id": owner, "reason": reason }).to_string(),
        })?;
        let _ = services::call(
            "ctx.notify",
            "send",
            &json!({ "user_id": owner, "template_key": "subtitles.deleted", "category": "system",
                     "title": "字幕被删除 / Subtitle removed", "body": format!("你上传的字幕已被管理员删除：{reason}\nYour subtitle was removed by staff: {reason}"),
                     "idempotency_key": format!("subtitles:deleted:{id}") })
            .to_string(),
        );
    }
    Ok(())
}

fn admin_list(page: i64) -> R<Value> {
    let r = sql::query(
        "SELECT s.id, s.torrent_id, t.title, s.lang, s.filename, s.size, s.downloads, u.username, (extract(epoch FROM s.created_at) * 1000)::bigint
           FROM subtitles s LEFT JOIN torrent.v_torrent_public t ON t.id = s.torrent_id
           LEFT JOIN identity.v_user_public u ON u.id = s.uploader_id
          WHERE s.deleted_at IS NULL ORDER BY s.created_at DESC, s.id DESC LIMIT 50 OFFSET $1",
        &[SqlValue::Int((page - 1) * 50)],
    )?;
    let items: Vec<Value> = r
        .rows
        .iter()
        .map(|row| json!({ "id": row.int(0), "torrent_id": row.int(1), "torrent_title": row.text(2), "lang": row.text(3), "filename": row.text(4),
                           "size": row.int(5), "downloads": row.int(6), "uploader": row.text(7), "created_at": row.int(8) }))
        .collect();
    Ok(json!({ "items": items, "page": page }))
}

// ---------------- 导出 ----------------

impl on_event::Guest for Subtitles {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        if evt.event_type != "torrent.deleted" {
            return Ok(());
        }
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        let Some(tid) = p["torrent_id"].as_i64() else { return Ok(()) };
        let ids = sql::query("SELECT id FROM subtitles WHERE torrent_id = $1 AND deleted_at IS NULL", &[SqlValue::Int(tid)])?;
        for id in ids.rows.iter().filter_map(|r| r.int(0)) {
            remove(id, None, "torrent_deleted")?;
        }
        Ok(())
    }
}

impl service_provider::Guest for Subtitles {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        match method.as_str() {
            // 列表页标记：{ "<torrent_id>": 数量 }
            "count_many" => {
                let ids: Vec<i64> = a["torrent_ids"].as_array().map(|x| x.iter().filter_map(Value::as_i64).take(500).collect()).unwrap_or_default();
                let r = sql::query(
                    "SELECT torrent_id, count(*) FROM subtitles WHERE torrent_id = ANY($1) AND deleted_at IS NULL GROUP BY torrent_id",
                    &[SqlValue::IntArray(ids)],
                )?;
                let m: serde_json::Map<String, Value> = r.rows.iter().map(|r| (r.int(0).unwrap_or(0).to_string(), json!(r.int(1)))).collect();
                Ok(Value::Object(m).to_string())
            }
            "list_for" => {
                let tid = a["torrent_id"].as_i64().ok_or_else(|| err("kernel.validation_failed", "torrent_id"))?;
                let r = sql::query(
                    "SELECT id, lang, filename, format FROM subtitles WHERE torrent_id = $1 AND deleted_at IS NULL ORDER BY id",
                    &[SqlValue::Int(tid)],
                )?;
                Ok(json!(r.rows.iter().map(|r| json!({ "id": r.int(0), "lang": r.text(1), "filename": r.text(2), "format": r.text(3) })).collect::<Vec<_>>()).to_string())
            }
            other => Err(err("kernel.not_found", &format!("ctx.subtitles has no method `{other}`"))),
        }
    }
}

impl http_handler::Guest for Subtitles {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let uid = ctx::current_user()?;
        let reply = match req.route() {
            "upload" => {
                let a = UploadArgs {
                    filename: req.query("filename").unwrap_or(""),
                    lang: req.query("lang").unwrap_or(""),
                    title: req.query("title").unwrap_or(""),
                    anonymous: req.query("anonymous") == Some("1"),
                };
                http::Reply::created(&upload(uid, req.param_i64("id")?, &a, req.body_bytes())?)
            }
            "list" => http::Reply::ok(&list(req.param_i64("id")?, uid)?),
            "download" => download(req.param_i64("id")?)?,
            "delete" => {
                delete(uid, req.param_i64("id")?, req.query("reason").unwrap_or(""))?;
                http::Reply::no_content()
            }
            "admin_list" => http::Reply::ok(&admin_list(req.query("page").and_then(|p| p.parse::<i64>().ok()).unwrap_or(1).clamp(1, 10_000))?),
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(Subtitles);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames() {
        assert_eq!(clean_filename("C:\\x\\简体中文.srt"), "简体中文.srt");
        assert_eq!(clean_filename("a\"b;\n.ass"), "ab.ass");
        assert_eq!(disposition("中.srt"), "attachment; filename=\"_.srt\"; filename*=UTF-8''%E4%B8%AD.srt");
    }
}
