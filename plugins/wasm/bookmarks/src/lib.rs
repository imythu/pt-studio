//! bookmarks 插件（WASM，docs/plugins/bookmarks/README.md）：收藏与合集。
//!
//! - 收藏：私有，按用户上限；种子页显示是否已收藏与收藏人数；我的收藏列表（分页）。
//! - 合集：用户整理的种子列表（标题、说明、公开 / 私有、条目备注与顺序），公开合集可浏览；
//!   作者或 `bookmarks.collections.manage` 可编辑；种子页列出收录它的公开合集。
//! - 种子删除时从收藏与合集中移除。服务 `ctx.bookmarks`：`list`（某用户收藏的种子 ID，供 RSS 等使用）、`is_bookmarked`。

pt_plugin_sdk::world!("bookmarks", http);

use exports::pt::plugin::{http_handler, on_event, service_provider};
use pt_plugin_sdk::{prelude::*, serde_json, sql};

struct Bookmarks;

const PAGE: i64 = 50;

struct Settings {
    max_bookmarks: i64,
    max_collections: i64,
    max_items: i64,
}

fn settings() -> Settings {
    let s = ctx::settings();
    let int = |k: &str, d: i64| s[k].as_i64().unwrap_or(d).max(1);
    Settings {
        max_bookmarks: int("max_bookmarks", 5000),
        max_collections: int("max_collections_per_user", 50),
        max_items: int("max_items_per_collection", 500),
    }
}

fn one(q: &str, p: &[SqlValue]) -> R<i64> {
    Ok(sql::query(q, p)?.rows.first().and_then(|r| r.int(0)).unwrap_or(0))
}

fn page_of(req: &http_handler::Request) -> i64 {
    req.query("page").and_then(|p| p.parse::<i64>().ok()).unwrap_or(1).clamp(1, 10_000)
}

fn approved(tid: i64) -> R<bool> {
    Ok(one("SELECT count(*) FROM torrent.v_torrent_public WHERE id = $1 AND status = 'approved'", &[SqlValue::Int(tid)])? > 0)
}

fn is_staff(uid: i64) -> bool {
    ctx::call("ctx.rbac", "check", &json!({ "user": uid, "permission": "bookmarks.collections.manage" }))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

// ---------------- 收藏 ----------------

fn add_bookmark(uid: i64, tid: i64) -> R<()> {
    if !approved(tid)? {
        return Err(err("kernel.not_found", "torrent not found"));
    }
    if one("SELECT count(*) FROM bookmarks WHERE user_id = $1", &[SqlValue::Int(uid)])? >= settings().max_bookmarks {
        return Err(err("bookmarks.limit_reached", "you have reached the bookmark limit"));
    }
    sql::execute(
        "INSERT INTO bookmarks (user_id, torrent_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        &[SqlValue::Int(uid), SqlValue::Int(tid)],
    )?;
    Ok(())
}

fn my_bookmarks(uid: i64, page: i64) -> R<Value> {
    let r = sql::query(
        "SELECT b.torrent_id, t.title, t.size, (extract(epoch FROM b.created_at) * 1000)::bigint, t.type_id
           FROM bookmarks b JOIN torrent.v_torrent_public t ON t.id = b.torrent_id
          WHERE b.user_id = $1 ORDER BY b.created_at DESC, b.torrent_id DESC LIMIT $2 OFFSET $3",
        &[SqlValue::Int(uid), SqlValue::Int(PAGE), SqlValue::Int((page - 1) * PAGE)],
    )?;
    let items: Vec<Value> = r
        .rows
        .iter()
        .map(|row| json!({ "torrent_id": row.int(0), "title": row.text(1), "size": row.int(2), "bookmarked_at": row.int(3), "type_id": row.text(4) }))
        .collect();
    let total = one("SELECT count(*) FROM bookmarks WHERE user_id = $1", &[SqlValue::Int(uid)])?;
    Ok(json!({ "items": items, "total": total, "page": page, "page_size": PAGE }))
}

fn torrent_info(tid: i64, uid: i64) -> R<Value> {
    let count = one("SELECT count(*) FROM bookmarks WHERE torrent_id = $1", &[SqlValue::Int(tid)])?;
    let mine = one("SELECT count(*) FROM bookmarks WHERE torrent_id = $1 AND user_id = $2", &[SqlValue::Int(tid), SqlValue::Int(uid)])? > 0;
    let cols = sql::query(
        "SELECT c.id, c.title, c.item_count, u.username
           FROM collection_items i JOIN collections c ON c.id = i.collection_id
           LEFT JOIN identity.v_user_public u ON u.id = c.owner_id
          WHERE i.torrent_id = $1 AND (c.is_public OR c.owner_id = $2) ORDER BY c.item_count DESC, c.id LIMIT 20",
        &[SqlValue::Int(tid), SqlValue::Int(uid)],
    )?;
    let collections: Vec<Value> = cols
        .rows
        .iter()
        .map(|row| json!({ "id": row.int(0), "title": row.text(1), "item_count": row.int(2), "owner": row.text(3) }))
        .collect();
    // 我的合集（用于“加入合集”），标出是否已包含
    let own = sql::query(
        "SELECT c.id, c.title, EXISTS (SELECT 1 FROM collection_items i WHERE i.collection_id = c.id AND i.torrent_id = $2)
           FROM collections c WHERE c.owner_id = $1 ORDER BY c.updated_at DESC LIMIT 100",
        &[SqlValue::Int(uid), SqlValue::Int(tid)],
    )?;
    let own: Vec<Value> = own.rows.iter().map(|row| json!({ "id": row.int(0), "title": row.text(1), "contains": row.bool(2) })).collect();
    Ok(json!({ "bookmarks": count, "bookmarked": mine, "collections": collections, "my_collections": own }))
}

// ---------------- 合集 ----------------

struct CollectionRow {
    owner: i64,
    public: bool,
}

fn load_collection(id: i64) -> R<Option<CollectionRow>> {
    let r = sql::query("SELECT owner_id, is_public FROM collections WHERE id = $1", &[SqlValue::Int(id)])?;
    Ok(r.rows.first().map(|row| CollectionRow { owner: row.int(0).unwrap_or(0), public: row.bool(1).unwrap_or(false) }))
}

/// 可编辑的合集（作者或管理员），否则 not_found / forbidden
fn editable(id: i64, uid: i64) -> R<CollectionRow> {
    let c = load_collection(id)?.ok_or_else(|| err("kernel.not_found", "collection not found"))?;
    if c.owner != uid && !is_staff(uid) {
        return Err(if c.public { err("kernel.forbidden", "only the owner can edit this collection") } else { err("kernel.not_found", "collection not found") });
    }
    Ok(c)
}

fn text_field(b: &Value, k: &str, max: usize) -> R<Option<String>> {
    match &b[k] {
        Value::Null => Ok(None),
        Value::String(s) => {
            let s = s.trim().to_owned();
            if s.chars().count() > max {
                return Err(err("kernel.validation_failed", &format!("`{k}` is too long")));
            }
            Ok(Some(s))
        }
        _ => Err(err("kernel.validation_failed", &format!("`{k}` must be a string"))),
    }
}

fn create_collection(uid: i64, b: &Value) -> R<Value> {
    let title = text_field(b, "title", 100)?.filter(|t| !t.is_empty()).ok_or_else(|| err("kernel.validation_failed", "title is required"))?;
    let desc = text_field(b, "description", 5000)?.unwrap_or_default();
    if one("SELECT count(*) FROM collections WHERE owner_id = $1", &[SqlValue::Int(uid)])? >= settings().max_collections {
        return Err(err("bookmarks.limit_reached", "you have reached the collection limit"));
    }
    let id = one(
        "INSERT INTO collections (owner_id, title, description, is_public) VALUES ($1, $2, $3, $4) RETURNING id",
        &[SqlValue::Int(uid), SqlValue::Text(title), SqlValue::Text(desc), SqlValue::Boolean(b["is_public"].as_bool().unwrap_or(true))],
    )?;
    ctx::publish("collection.created", &format!("user:{uid}"), &json!({ "collection_id": id, "owner_id": uid }))?;
    Ok(json!({ "id": id }))
}

fn update_collection(id: i64, uid: i64, b: &Value) -> R<()> {
    editable(id, uid)?;
    let title = text_field(b, "title", 100)?;
    if title.as_deref() == Some("") {
        return Err(err("kernel.validation_failed", "title is required"));
    }
    sql::execute(
        "UPDATE collections SET title = coalesce($2, title), description = coalesce($3, description),
                is_public = coalesce($4, is_public), updated_at = now() WHERE id = $1",
        &[
            SqlValue::Int(id),
            title.map_or(SqlValue::Null, SqlValue::Text),
            text_field(b, "description", 5000)?.map_or(SqlValue::Null, SqlValue::Text),
            b["is_public"].as_bool().map_or(SqlValue::Null, SqlValue::Boolean),
        ],
    )?;
    Ok(())
}

fn add_item(id: i64, uid: i64, b: &Value) -> R<()> {
    editable(id, uid)?;
    let tid = b["torrent_id"].as_i64().ok_or_else(|| err("kernel.validation_failed", "torrent_id is required"))?;
    if !approved(tid)? {
        return Err(err("kernel.not_found", "torrent not found"));
    }
    let note = text_field(b, "note", 500)?.unwrap_or_default();
    // 锁住合集行，计数与位置一致
    let n = one("SELECT item_count FROM collections WHERE id = $1 FOR UPDATE", &[SqlValue::Int(id)])?;
    let exists = one("SELECT count(*) FROM collection_items WHERE collection_id = $1 AND torrent_id = $2", &[SqlValue::Int(id), SqlValue::Int(tid)])? > 0;
    if exists {
        sql::execute(
            "UPDATE collection_items SET note = $3 WHERE collection_id = $1 AND torrent_id = $2",
            &[SqlValue::Int(id), SqlValue::Int(tid), SqlValue::Text(note)],
        )?;
        return Ok(());
    }
    if n >= settings().max_items {
        return Err(err("bookmarks.collection_full", "this collection is full"));
    }
    sql::execute(
        "INSERT INTO collection_items (collection_id, torrent_id, position, note)
         VALUES ($1, $2, coalesce((SELECT max(position) + 1 FROM collection_items WHERE collection_id = $1), 0), $3)",
        &[SqlValue::Int(id), SqlValue::Int(tid), SqlValue::Text(note)],
    )?;
    sql::execute("UPDATE collections SET item_count = item_count + 1, updated_at = now() WHERE id = $1", &[SqlValue::Int(id)])?;
    Ok(())
}

fn remove_item(id: i64, uid: i64, tid: i64) -> R<bool> {
    editable(id, uid)?;
    let n = sql::execute("DELETE FROM collection_items WHERE collection_id = $1 AND torrent_id = $2", &[SqlValue::Int(id), SqlValue::Int(tid)])?;
    if n > 0 {
        sql::execute("UPDATE collections SET item_count = item_count - 1, updated_at = now() WHERE id = $1", &[SqlValue::Int(id)])?;
    }
    Ok(n > 0)
}

/// 调整顺序：按给出的种子 ID 顺序重排（未列出的排在后面，保持原相对顺序）
fn reorder(id: i64, uid: i64, b: &Value) -> R<()> {
    editable(id, uid)?;
    let ids: Vec<i64> = b["torrent_ids"].as_array().map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default();
    if ids.len() > 1000 {
        return Err(err("kernel.validation_failed", "too many items"));
    }
    sql::execute(
        "UPDATE collection_items i SET position = o.pos
           FROM (SELECT torrent_id, row_number() OVER (ORDER BY coalesce(array_position($2::bigint[], torrent_id), 2147483647), position) AS pos
                   FROM collection_items WHERE collection_id = $1) o
          WHERE i.collection_id = $1 AND i.torrent_id = o.torrent_id",
        &[SqlValue::Int(id), SqlValue::IntArray(ids)],
    )?;
    sql::execute("UPDATE collections SET updated_at = now() WHERE id = $1", &[SqlValue::Int(id)])?;
    Ok(())
}

fn collection_json(row: &impl pt_plugin_sdk::RowExt) -> Value {
    json!({ "id": row.int(0), "title": row.text(1), "description": row.text(2), "is_public": row.bool(3), "item_count": row.int(4),
            "owner_id": row.int(5), "owner": row.text(6), "updated_at": row.int(7) })
}

const COLLECTION_COLS: &str = "c.id, c.title, c.description, c.is_public, c.item_count, c.owner_id, u.username,
                               (extract(epoch FROM c.updated_at) * 1000)::bigint";

fn get_collection(id: i64, uid: i64) -> R<Value> {
    let r = sql::query(
        &format!("SELECT {COLLECTION_COLS} FROM collections c LEFT JOIN identity.v_user_public u ON u.id = c.owner_id WHERE c.id = $1"),
        &[SqlValue::Int(id)],
    )?;
    let Some(row) = r.rows.first() else { return Err(err("kernel.not_found", "collection not found")) };
    let mut c = collection_json(row);
    let owner = row.int(5).unwrap_or(0);
    let staff = owner != uid && is_staff(uid);
    if !row.bool(3).unwrap_or(false) && owner != uid && !staff {
        return Err(err("kernel.not_found", "collection not found"));
    }
    let items = sql::query(
        "SELECT i.torrent_id, t.title, t.size, t.type_id, i.note, (extract(epoch FROM i.added_at) * 1000)::bigint
           FROM collection_items i JOIN torrent.v_torrent_public t ON t.id = i.torrent_id AND t.status = 'approved'
          WHERE i.collection_id = $1 ORDER BY i.position, i.added_at LIMIT 1000",
        &[SqlValue::Int(id)],
    )?;
    c["items"] = Value::Array(
        items
            .rows
            .iter()
            .map(|r| json!({ "torrent_id": r.int(0), "title": r.text(1), "size": r.int(2), "type_id": r.text(3), "note": r.text(4), "added_at": r.int(5) }))
            .collect(),
    );
    c["editable"] = json!(owner == uid || staff);
    Ok(c)
}

fn list_collections(uid: i64, mine: bool, q: Option<&str>, page: i64) -> R<Value> {
    let pattern = q.map(|s| format!("%{}%", s.trim().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")));
    let (where_, params): (&str, Vec<SqlValue>) = if mine {
        ("c.owner_id = $1", vec![SqlValue::Int(uid)])
    } else {
        ("c.is_public AND c.item_count > 0 AND ($1::text IS NULL OR c.title ILIKE $1)", vec![pattern.map_or(SqlValue::Null, SqlValue::Text)])
    };
    let mut p = params.clone();
    p.push(SqlValue::Int(PAGE));
    p.push(SqlValue::Int((page - 1) * PAGE));
    let r = sql::query(
        &format!(
            "SELECT {COLLECTION_COLS} FROM collections c LEFT JOIN identity.v_user_public u ON u.id = c.owner_id
              WHERE {where_} ORDER BY c.updated_at DESC, c.id DESC LIMIT $2 OFFSET $3"
        ),
        &p,
    )?;
    let total = one(&format!("SELECT count(*) FROM collections c WHERE {where_}"), &params)?;
    Ok(json!({ "items": r.rows.iter().map(collection_json).collect::<Vec<_>>(), "total": total, "page": page, "page_size": PAGE }))
}

// ---------------- 导出 ----------------

impl on_event::Guest for Bookmarks {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        if evt.event_type != "torrent.deleted" {
            return Ok(());
        }
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        let Some(tid) = p["torrent_id"].as_i64() else { return Ok(()) };
        sql::execute("DELETE FROM bookmarks WHERE torrent_id = $1", &[SqlValue::Int(tid)])?;
        sql::execute(
            "WITH d AS (DELETE FROM collection_items WHERE torrent_id = $1 RETURNING collection_id)
             UPDATE collections SET item_count = item_count - 1 WHERE id IN (SELECT collection_id FROM d)",
            &[SqlValue::Int(tid)],
        )?;
        Ok(())
    }
}

impl service_provider::Guest for Bookmarks {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        let uid = a["user"].as_i64().ok_or_else(|| err("kernel.validation_failed", "user"))?;
        match method.as_str() {
            "list" => {
                let limit = a["limit"].as_i64().unwrap_or(100).clamp(1, 1000);
                let r = sql::query(
                    "SELECT torrent_id FROM bookmarks WHERE user_id = $1 ORDER BY created_at DESC LIMIT $2",
                    &[SqlValue::Int(uid), SqlValue::Int(limit)],
                )?;
                Ok(json!(r.rows.iter().filter_map(|r| r.int(0)).collect::<Vec<_>>()).to_string())
            }
            "is_bookmarked" => {
                let tid = a["torrent_id"].as_i64().ok_or_else(|| err("kernel.validation_failed", "torrent_id"))?;
                let n = one("SELECT count(*) FROM bookmarks WHERE user_id = $1 AND torrent_id = $2", &[SqlValue::Int(uid), SqlValue::Int(tid)])?;
                Ok(json!(n > 0).to_string())
            }
            other => Err(err("kernel.not_found", &format!("ctx.bookmarks has no method `{other}`"))),
        }
    }
}

impl http_handler::Guest for Bookmarks {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let uid = ctx::current_user()?;
        let reply = match req.route() {
            "bookmark_add" => {
                add_bookmark(uid, req.param_i64("id")?)?;
                http::Reply::no_content()
            }
            "bookmark_remove" => {
                sql::execute(
                    "DELETE FROM bookmarks WHERE user_id = $1 AND torrent_id = $2",
                    &[SqlValue::Int(uid), SqlValue::Int(req.param_i64("id")?)],
                )?;
                http::Reply::no_content()
            }
            "bookmarks" => http::Reply::ok(&my_bookmarks(uid, page_of(&req))?),
            "torrent" => http::Reply::ok(&torrent_info(req.param_i64("id")?, uid)?),
            "collections" => http::Reply::ok(&list_collections(uid, req.query("mine") == Some("1"), req.query("q").filter(|q| !q.trim().is_empty()), page_of(&req))?),
            "collection_create" => http::Reply::created(&create_collection(uid, &req.body_value())?),
            "collection" => http::Reply::ok(&get_collection(req.param_i64("id")?, uid)?),
            "collection_update" => {
                update_collection(req.param_i64("id")?, uid, &req.body_value())?;
                http::Reply::no_content()
            }
            "collection_delete" => {
                let id = req.param_i64("id")?;
                editable(id, uid)?;
                sql::execute("DELETE FROM collections WHERE id = $1", &[SqlValue::Int(id)])?;
                http::Reply::no_content()
            }
            "item_add" => {
                add_item(req.param_i64("id")?, uid, &req.body_value())?;
                http::Reply::no_content()
            }
            "item_remove" => {
                if !remove_item(req.param_i64("id")?, uid, req.param_i64("tid")?)? {
                    return Err(err("kernel.not_found", "the torrent is not in this collection"));
                }
                http::Reply::no_content()
            }
            "reorder" => {
                reorder(req.param_i64("id")?, uid, &req.body_value())?;
                http::Reply::no_content()
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(Bookmarks);
