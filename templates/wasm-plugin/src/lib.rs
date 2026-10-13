//! notes：PT Studio WASM 插件模板（个人便签）。
//!
//! 演示：HTTP 路由（`world!(…, http)` + `RequestExt` + `http::Reply`）、SQL（参数用 `.into()`，结果用 `RowExt`）、
//! 后台设置、钩子点（`ctx::run_hook`，其他插件可修改 title）、事件（`ctx::publish`）、错误码、MCP 工具。

#![allow(clippy::missing_errors_doc)]

pt_plugin_sdk::world!("notes", http);

use exports::pt::plugin::{http_handler, mcp_tool};
use pt_plugin_sdk::{prelude::*, sql};
use serde::Deserialize;

struct Notes;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewNote {
    title: String,
    #[serde(default)]
    body: String,
}

/// 标题规整：去空白、1..=100 字符（纯函数，便于单元测试）
pub fn clean_title(t: &str) -> R<String> {
    let t = t.trim();
    if t.is_empty() || t.chars().count() > 100 {
        return Err(err("notes.title_invalid", "title must be 1..=100 characters"));
    }
    Ok(t.to_owned())
}

fn list(uid: i64, limit: i64) -> R<Value> {
    let r = sql::query(
        "SELECT id, title, body, (extract(epoch FROM created_at) * 1000)::bigint FROM notes
          WHERE user_id = $1 ORDER BY id DESC LIMIT $2",
        &[uid.into(), limit.clamp(1, 100).into()],
    )?;
    let items: Vec<Value> = r
        .rows
        .iter()
        .map(|row| json!({ "id": row.int(0), "title": row.text(1), "body": row.text(2), "created_at": row.int(3) }))
        .collect();
    Ok(json!({ "items": items }))
}

fn create(uid: i64, n: NewNote) -> R<Value> {
    let max = ctx::settings()["max_notes"].as_i64().unwrap_or(100);
    let count = sql::query("SELECT count(*) FROM notes WHERE user_id = $1", &[uid.into()])?
        .rows
        .first()
        .and_then(|r| r.int(0))
        .unwrap_or(0);
    if count >= max {
        return Err(err("notes.too_many", "note limit reached").with_details(&json!({ "max": max })));
    }
    // 钩子：其他插件可以拒绝，或按清单 modifiable 改写 title
    let input = ctx::run_hook(
        "notes.before_create",
        &json!({ "version": 1, "user_id": uid, "title": clean_title(&n.title)?, "body": n.body }),
    )?;
    let title = clean_title(input["title"].as_str().unwrap_or(""))?;
    let r = sql::query(
        "INSERT INTO notes (user_id, title, body) VALUES ($1, $2, $3) RETURNING id",
        &[
            uid.into(),
            (&title).into(),
            n.body.chars().take(10_000).collect::<String>().into(),
        ],
    )?;
    let id = r.rows.first().and_then(|r| r.int(0)).unwrap_or(0);
    ctx::publish(
        "notes.created",
        &format!("user:{uid}"),
        &json!({ "note_id": id, "user_id": uid }),
    )?;
    logging::info("note created", &[("note_id", &id.to_string())]);
    Ok(json!({ "id": id, "title": title }))
}

fn delete(uid: i64, id: i64) -> R<()> {
    let n = sql::execute(
        "DELETE FROM notes WHERE id = $1 AND user_id = $2",
        &[id.into(), uid.into()],
    )?;
    if n == 0 {
        return Err(err("notes.not_found", "note not found"));
    }
    Ok(())
}

impl http_handler::Guest for Notes {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let uid = ctx::current_user()?;
        let reply = match req.route() {
            "list" => http::Reply::ok(&list(
                uid,
                req.query("limit").and_then(|l| l.parse().ok()).unwrap_or(50),
            )?),
            "create" => http::Reply::created(&create(uid, req.body_json()?)?),
            "delete" => {
                delete(uid, req.param_i64("id")?)?;
                http::Reply::no_content()
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

impl mcp_tool::Guest for Notes {
    fn call(tool: String, arguments: String) -> R<String> {
        let a: Value = pt_plugin_sdk::serde_json::from_str(&arguments).unwrap_or(Value::Null);
        match tool.as_str() {
            "list_my_notes" => {
                let mut out = list(ctx::current_user()?, a["limit"].as_i64().unwrap_or(10))?;
                // MCP 结果只给标题（正文是用户内容，按需再取）
                for it in out["items"].as_array_mut().into_iter().flatten() {
                    if let Some(o) = it.as_object_mut() {
                        o.remove("body");
                    }
                }
                Ok(out.to_string())
            }
            _ => Err(err("kernel.not_found", "unknown tool")),
        }
    }
}

export!(Notes);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles() {
        assert_eq!(clean_title("  hi ").ok().as_deref(), Some("hi"));
        assert!(clean_title("   ").is_err());
        assert!(clean_title(&"x".repeat(101)).is_err());
    }
}
