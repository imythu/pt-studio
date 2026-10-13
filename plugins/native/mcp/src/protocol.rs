//! MCP 协议的纯逻辑（mcp LLD §1.1、§1.3、§1.4）：版本协商、JSON-RPC 信封、工具目录映射、结果构造、会话 ID。

use hmac::{Hmac, Mac};
use pt_plugin_api::McpToolDef;
use serde_json::{Value, json};
use sha2::Sha256;

pub const LATEST: &str = "2026-07-28";
pub const LEGACY: &str = "2025-11-25";
pub const VERSIONS: &[&str] = &[LATEST, LEGACY];

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;

/// 会话 ID 有效期（2025-11-25）
pub const SESSION_TTL_MS: i64 = 24 * 3600 * 1000;

pub fn ok(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

pub fn error(id: &Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// 请求头版本 → 使用的版本（缺省按 2025-11-25；不支持 → None）
pub fn version(header: Option<&str>) -> Option<&'static str> {
    match header {
        None => Some(LEGACY),
        Some(v) => VERSIONS.iter().copied().find(|x| *x == v.trim()),
    }
}

/// initialize：客户端请求的版本受支持就用它，否则给最新版本
pub fn negotiate(requested: Option<&str>) -> &'static str {
    requested
        .and_then(|r| VERSIONS.iter().copied().find(|v| *v == r))
        .unwrap_or(LATEST)
}

/// 工具可见性：audience 与 token 作用域（读工具 read 或 write 均可；写工具在调用时再检查 write）
pub fn visible(def: &McpToolDef, scopes: &[String]) -> bool {
    let has = |s: &str| scopes.iter().any(|x| x == s || x == "*");
    match def.audience.as_str() {
        "admin" => has("admin"),
        _ => has("read") || has("write") || has("admin"),
    }
}

/// 写工具是否需要确认（站点可为非锁定的工具关闭确认）
pub fn needs_confirm(def: &McpToolDef, disabled: &[String]) -> bool {
    def.needs_confirm() && (def.confirm_locked || !disabled.iter().any(|d| d == &def.name))
}

pub fn tool_json(def: &McpToolDef, confirm: bool) -> Value {
    let mut description = def.description.clone();
    if confirm {
        description.push_str("\n（写操作：需要用户在站点确认后才会执行）");
    }
    let mut t = json!({
        "name": def.name, "description": description, "inputSchema": def.input_schema,
        "annotations": { "readOnlyHint": !def.write, "destructiveHint": def.destructive.unwrap_or(def.write),
                         "idempotentHint": !def.write, "openWorldHint": false },
    });
    if let Some(title) = &def.title {
        t["title"] = json!(title);
        t["annotations"]["title"] = json!(title);
    }
    if let Some(o) = &def.output_schema {
        t["outputSchema"] = o.clone();
    }
    t
}

pub fn tool_result(out: &Value) -> Value {
    let structured = if out.is_object() {
        out.clone()
    } else {
        json!({ "result": out })
    };
    json!({ "content": [{ "type": "text", "text": out.to_string() }], "structuredContent": structured, "isError": false })
}

pub fn tool_error(code: &str, message: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": format!("{code}: {message}") }], "isError": true })
}

pub fn confirmation_result(id: &str, url: &str, expires_at: i64) -> Value {
    let info = json!({ "status": "confirmation_required", "confirmation_id": id, "confirm_url": url, "expires_at": expires_at });
    json!({
        "content": [{ "type": "text", "text": format!(
            "该操作需要你在站点确认：{url}（10 分钟内有效）。确认后再次调用同一工具，并在 _meta 中带上 confirmation_id = \"{id}\"。") }],
        "structuredContent": info, "isError": false, "_meta": { "pt/confirmation": info },
    })
}

/// 参数的规范 JSON（键排序）→ SHA-256，用于确认项与重放调用的一致性检查
pub fn args_hash(v: &Value) -> Vec<u8> {
    fn canon(v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                Value::Object(keys.into_iter().map(|k| (k.clone(), canon(&m[k]))).collect())
            }
            Value::Array(a) => Value::Array(a.iter().map(canon).collect()),
            x => x.clone(),
        }
    }
    use sha2::Digest;
    Sha256::digest(canon(v).to_string().as_bytes()).to_vec()
}

fn mac(key: &[u8], token_id: &str, issued_at: i64) -> Vec<u8> {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key)
        .unwrap_or_else(|_| unreachable!("HMAC accepts any key length"));
    m.update(token_id.as_bytes());
    m.update(&issued_at.to_be_bytes());
    m.finalize().into_bytes()[..16].to_vec()
}

/// 2025-11-25 会话 ID：hex(HMAC(key, token_id ‖ issued_at)[..16]) + "." + issued_at（无状态校验）
pub fn session_id(key: &[u8], token_id: &str, issued_at: i64) -> String {
    format!("{}.{issued_at}", hex::encode(mac(key, token_id, issued_at)))
}

pub fn session_valid(key: &[u8], token_id: &str, sid: &str, now: i64) -> bool {
    let Some((sig, ts)) = sid.split_once('.') else {
        return false;
    };
    let Ok(ts) = ts.parse::<i64>() else { return false };
    let Ok(sig) = hex::decode(sig) else { return false };
    now - ts <= SESSION_TTL_MS && ts <= now + 60_000 && sig == mac(key, token_id, ts)
}

/// 工具结果中的字符串兜底过滤（32 位十六进制 passkey、邮箱）；返回命中次数
pub fn scrub_strings(v: &mut Value) -> usize {
    match v {
        Value::String(s) => {
            let mut hits = 0;
            let out: Vec<String> = s
                .split(' ')
                .map(|w| {
                    let core = w
                        .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '@' && c != '.')
                        .trim_end_matches('.');
                    let passkey = core.len() == 32 && core.chars().all(|c| c.is_ascii_hexdigit());
                    let email = core
                        .split_once('@')
                        .is_some_and(|(u, d)| !u.is_empty() && d.contains('.') && !d.starts_with('.'));
                    if passkey || email {
                        hits += 1;
                        w.replace(core, "[filtered]")
                    } else {
                        w.to_owned()
                    }
                })
                .collect();
            if hits > 0 {
                *s = out.join(" ");
            }
            hits
        }
        Value::Array(a) => a.iter_mut().map(scrub_strings).sum(),
        Value::Object(m) => m.values_mut().map(scrub_strings).sum(),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(name: &str, write: bool, audience: &str) -> McpToolDef {
        McpToolDef {
            name: name.into(),
            plugin: "p".into(),
            title: None,
            description: "d".into(),
            input_schema: json!({ "type": "object" }),
            output_schema: None,
            permission: None,
            write,
            confirm: None,
            confirm_locked: false,
            destructive: None,
            audience: audience.into(),
        }
    }

    #[test]
    fn versions_and_visibility() {
        assert_eq!(
            (version(None), version(Some(LATEST)), version(Some("2024-01-01"))),
            (Some(LEGACY), Some(LATEST), None)
        );
        assert_eq!(
            (negotiate(Some(LEGACY)), negotiate(Some("1999"))),
            (LEGACY, LATEST)
        );
        let read = vec!["read".to_owned()];
        assert!(visible(&def("get_x", false, "user"), &read));
        assert!(!visible(&def("admin_x", false, "admin"), &read));
        assert!(visible(&def("admin_x", false, "admin"), &["admin".to_owned()]));
        let w = def("do_x", true, "user");
        assert!(needs_confirm(&w, &[]) && !needs_confirm(&w, &["do_x".to_owned()]));
        let locked = McpToolDef {
            confirm_locked: true,
            ..w
        };
        assert!(needs_confirm(&locked, &["do_x".to_owned()]));
        let t = tool_json(&locked, true);
        assert_eq!(
            (
                t["annotations"]["readOnlyHint"].as_bool(),
                t["annotations"]["destructiveHint"].as_bool()
            ),
            (Some(false), Some(true))
        );
        assert!(t["description"].as_str().is_some_and(|d| d.contains("确认")));
    }

    #[test]
    fn sessions_and_hashes() {
        let k = b"key";
        let sid = session_id(k, "tok:1", 1000);
        assert!(session_valid(k, "tok:1", &sid, 2000));
        assert!(!session_valid(k, "tok:2", &sid, 2000));
        assert!(!session_valid(k, "tok:1", &sid, 1000 + SESSION_TTL_MS + 1));
        assert_eq!(
            args_hash(&json!({ "b": 1, "a": [1, { "y": 2, "x": 1 }] })),
            args_hash(&json!({ "a": [1, { "x": 1, "y": 2 }], "b": 1 }))
        );
        let mut v = json!({ "note": "key 0123456789abcdef0123456789abcdef, mail me@example.com." });
        assert_eq!(scrub_strings(&mut v), 2);
        assert_eq!(v["note"], "key [filtered], mail [filtered].");
    }
}
