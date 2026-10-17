//! attachments 插件（WASM，docs/plugins/attachments/README.md）：站内图片上传（图床）。
//!
//! - 只接受 PNG / JPEG / GIF / WebP（按文件头识别），去除 EXIF / XMP / 文本块后按 SHA-256 内容寻址存入 blob
//!   `public/i/<前两位>/<sha256>.<ext>`，经 `/media/attachments/...` 公开访问；站内 Markdown / BBCode 总是允许这些地址。
//! - 大小、尺寸、每日数量与总量限制；同一用户重复上传返回已有记录。
//! - 删除：本人或 `attachments.manage`；没有其他记录引用时删除文件。

pt_plugin_sdk::world!("attachments", http);

mod image;

use exports::pt::plugin::http_handler;
use pt_plugin_sdk::{audit, blob, prelude::*, sql};
use sha2::{Digest, Sha256};

struct Attachments;

const PAGE: i64 = 60;

struct Settings {
    max_bytes: usize,
    max_dimension: u32,
    daily_count: i64,
    daily_bytes: i64,
}

fn settings() -> Settings {
    let s = ctx::settings();
    let int = |k: &str, d: i64| s[k].as_i64().unwrap_or(d).max(1);
    Settings {
        max_bytes: usize::try_from(int("max_size_kb", 5120) * 1024).unwrap_or(5 << 20),
        max_dimension: u32::try_from(int("max_dimension", 16384)).unwrap_or(16384),
        daily_count: int("daily_count", 50),
        daily_bytes: int("daily_mb", 100) * 1024 * 1024,
    }
}

fn one(q: &str, p: &[SqlValue]) -> R<i64> {
    Ok(sql::query(q, p)?.rows.first().and_then(|r| r.int(0)).unwrap_or(0))
}

fn url_of(key: &str) -> String {
    format!("/media/attachments/{}", key.trim_start_matches("public/"))
}

fn item(id: Option<i64>, key: &str, mime: Option<String>, size: Option<i64>, w: Option<i64>, h: Option<i64>, at: Option<i64>) -> Value {
    let url = url_of(key);
    json!({ "id": id, "url": url, "mime": mime, "size": size, "width": w, "height": h, "created_at": at,
            "markdown": format!("![]({url})"), "bbcode": format!("[img]{url}[/img]") })
}

fn usage(uid: i64) -> R<(i64, i64)> {
    let r = sql::query(
        "SELECT count(*), coalesce(sum(size), 0)::bigint FROM images WHERE user_id = $1 AND created_at > now() - interval '1 day'",
        &[SqlValue::Int(uid)],
    )?;
    let row = r.rows.first();
    Ok((row.and_then(|r| r.int(0)).unwrap_or(0), row.and_then(|r| r.int(1)).unwrap_or(0)))
}

fn upload(uid: i64, body: &[u8]) -> R<Value> {
    let s = settings();
    if body.is_empty() {
        return Err(err("kernel.validation_failed", "the request body must be the image file"));
    }
    if body.len() > s.max_bytes {
        return Err(err("attachments.too_large", &format!("images must be at most {} KiB", s.max_bytes / 1024)));
    }
    let kind = image::detect(body).ok_or_else(|| err("attachments.unsupported", "only PNG, JPEG, GIF and WebP images are accepted"))?;
    let (data, w, h) = image::clean(kind, body).ok_or_else(|| err("attachments.invalid", "the image file is damaged"))?;
    if w > s.max_dimension || h > s.max_dimension {
        return Err(err("attachments.too_large", &format!("images must be at most {0}×{0} pixels", s.max_dimension)));
    }
    let digest = Sha256::digest(&data);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    let existing = sql::query(
        "SELECT id, blob_key, mime, size, width, height, (extract(epoch FROM created_at) * 1000)::bigint
           FROM images WHERE user_id = $1 AND sha256 = $2 AND deleted_at IS NULL",
        &[SqlValue::Int(uid), SqlValue::Bytes(digest.to_vec())],
    )?;
    if let Some(r) = existing.rows.first() {
        return Ok(item(r.int(0), &r.text(1).unwrap_or_default(), r.text(2), r.int(3), r.int(4), r.int(5), r.int(6)));
    }
    let (count, bytes) = usage(uid)?;
    let size = i64::try_from(data.len()).unwrap_or(i64::MAX);
    if count >= s.daily_count || bytes + size > s.daily_bytes {
        return Err(err("attachments.quota_exceeded", "you have reached today's upload limit"));
    }
    let key = format!("public/i/{}/{hex}.{}", &hex[..2], kind.ext());
    // 内容寻址：已有人上传过同一文件时不再写
    if one("SELECT count(*) FROM images WHERE blob_key = $1 AND deleted_at IS NULL", &[SqlValue::Text(key.clone())])? == 0 {
        blob::put(&key, kind.mime(), &data)?;
    }
    let r = sql::query(
        "INSERT INTO images (user_id, sha256, blob_key, mime, size, width, height) VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, (extract(epoch FROM created_at) * 1000)::bigint",
        &[
            SqlValue::Int(uid),
            SqlValue::Bytes(digest.to_vec()),
            SqlValue::Text(key.clone()),
            SqlValue::Text(kind.mime().into()),
            SqlValue::Int(size),
            SqlValue::Int(i64::from(w)),
            SqlValue::Int(i64::from(h)),
        ],
    )?;
    let row = r.rows.first();
    Ok(item(row.and_then(|r| r.int(0)), &key, Some(kind.mime().into()), Some(size), Some(i64::from(w)), Some(i64::from(h)), row.and_then(|r| r.int(1))))
}

fn is_staff(uid: i64) -> bool {
    ctx::call("ctx.rbac", "check", &json!({ "user": uid, "permission": "attachments.manage" }))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn delete(uid: i64, id: i64) -> R<()> {
    let r = sql::query("SELECT user_id, blob_key FROM images WHERE id = $1 AND deleted_at IS NULL FOR UPDATE", &[SqlValue::Int(id)])?;
    let Some(row) = r.rows.first() else { return Err(err("kernel.not_found", "image not found")) };
    let owner = row.int(0).unwrap_or(0);
    let key = row.text(1).unwrap_or_default();
    let staff = owner != uid && is_staff(uid);
    if owner != uid && !staff {
        return Err(err("kernel.not_found", "image not found"));
    }
    sql::execute("UPDATE images SET deleted_at = now(), deleted_by = $2 WHERE id = $1", &[SqlValue::Int(id), SqlValue::Int(uid)])?;
    if one("SELECT count(*) FROM images WHERE blob_key = $1 AND deleted_at IS NULL", &[SqlValue::Text(key.clone())])? == 0 {
        blob::delete(&key)?;
    }
    if staff {
        audit::write(&audit::AuditEntry {
            action: "attachments.delete".into(),
            target: format!("image:{id}"),
            details: json!({ "owner_id": owner, "key": key }).to_string(),
        })?;
    }
    Ok(())
}

fn list(uid: Option<i64>, page: i64) -> R<Value> {
    let r = sql::query(
        "SELECT i.id, i.blob_key, i.mime, i.size, i.width, i.height, (extract(epoch FROM i.created_at) * 1000)::bigint, i.user_id, u.username
           FROM images i LEFT JOIN identity.v_user_public u ON u.id = i.user_id
          WHERE i.deleted_at IS NULL AND ($1::bigint IS NULL OR i.user_id = $1)
          ORDER BY i.created_at DESC, i.id DESC LIMIT $2 OFFSET $3",
        &[uid.map_or(SqlValue::Null, SqlValue::Int), SqlValue::Int(PAGE), SqlValue::Int((page - 1) * PAGE)],
    )?;
    let items: Vec<Value> = r
        .rows
        .iter()
        .map(|row| {
            let mut v = item(row.int(0), &row.text(1).unwrap_or_default(), row.text(2), row.int(3), row.int(4), row.int(5), row.int(6));
            v["user_id"] = json!(row.int(7));
            v["username"] = json!(row.text(8));
            v
        })
        .collect();
    let total = one(
        "SELECT count(*) FROM images WHERE deleted_at IS NULL AND ($1::bigint IS NULL OR user_id = $1)",
        &[uid.map_or(SqlValue::Null, SqlValue::Int)],
    )?;
    Ok(json!({ "items": items, "total": total, "page": page, "page_size": PAGE }))
}

impl http_handler::Guest for Attachments {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let uid = ctx::current_user()?;
        let page = req.query("page").and_then(|p| p.parse::<i64>().ok()).unwrap_or(1).clamp(1, 10_000);
        let reply = match req.route() {
            "upload" => http::Reply::created(&upload(uid, req.body_bytes())?),
            "mine" => {
                let mut v = list(Some(uid), page)?;
                let s = settings();
                let (count, bytes) = usage(uid)?;
                v["quota"] = json!({ "count": count, "bytes": bytes, "daily_count": s.daily_count, "daily_bytes": s.daily_bytes,
                                     "max_bytes": s.max_bytes, "max_dimension": s.max_dimension });
                http::Reply::ok(&v)
            }
            "delete" => {
                delete(uid, req.param_i64("id")?)?;
                http::Reply::no_content()
            }
            "admin_list" => {
                let user = req.query("user").and_then(|u| u.parse::<i64>().ok());
                http::Reply::ok(&list(user, page)?)
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(Attachments);
