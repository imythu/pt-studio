//! JSON Schema 子集校验（插件与内核共用；无代码生成，可用于 WASM 宿主之外的任何场景）：
//! type、enum、const、required、properties、additionalProperties: false、items、minimum / maximum、
//! minLength / maxLength、minItems / maxItems。不支持 `$ref`、`pattern`、组合关键字（视为通过）。

use serde_json::{Value, json};

/// 校验；不通过时返回首个错误（`<JSON Pointer>: <说明>`）
pub fn validate(schema: &Value, v: &Value) -> Result<(), String> {
    fn ty_ok(t: &str, v: &Value) -> bool {
        match t {
            "object" => v.is_object(),
            "array" => v.is_array(),
            "string" => v.is_string(),
            "integer" => v.as_i64().is_some() || v.as_u64().is_some(),
            "number" => v.is_number(),
            "boolean" => v.is_boolean(),
            "null" => v.is_null(),
            _ => true,
        }
    }
    fn go(s: &Value, v: &Value, path: &str) -> Result<(), String> {
        let fail = |m: &str| Err(format!("{}: {m}", if path.is_empty() { "/" } else { path }));
        let types: Vec<&str> = match &s["type"] {
            Value::String(t) => vec![t.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        if !types.is_empty() && !types.iter().any(|t| ty_ok(t, v)) {
            return fail(&format!("expected {}", types.join(" | ")));
        }
        if let Some(e) = s["enum"].as_array()
            && !e.contains(v)
        {
            return fail("not one of the allowed values");
        }
        if let Some(c) = s.get("const")
            && c != v
        {
            return fail("does not match const");
        }
        if let Some(n) = v.as_f64()
            && (s["minimum"].as_f64().is_some_and(|m| n < m) || s["maximum"].as_f64().is_some_and(|m| n > m))
        {
            return fail("out of range");
        }
        if let Some(t) = v.as_str() {
            let len = t.chars().count() as u64;
            if s["minLength"].as_u64().is_some_and(|m| len < m)
                || s["maxLength"].as_u64().is_some_and(|m| len > m)
            {
                return fail("length out of range");
            }
        }
        if let Some(a) = v.as_array() {
            let len = a.len() as u64;
            if s["minItems"].as_u64().is_some_and(|m| len < m)
                || s["maxItems"].as_u64().is_some_and(|m| len > m)
            {
                return fail("item count out of range");
            }
            if s.get("items").is_some() {
                for (i, x) in a.iter().enumerate() {
                    go(&s["items"], x, &format!("{path}/{i}"))?;
                }
            }
        }
        if let Some(o) = v.as_object() {
            for r in s["required"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !o.contains_key(r) {
                    return Err(format!("{path}/{r}: required"));
                }
            }
            let props = s["properties"].as_object();
            for (k, x) in o {
                match props.and_then(|p| p.get(k)) {
                    Some(ps) => go(ps, x, &format!("{path}/{k}"))?,
                    None if s["additionalProperties"] == json!(false) => {
                        return Err(format!("{path}/{k}: not allowed"));
                    }
                    None => {}
                }
            }
        }
        Ok(())
    }
    go(schema, v, "")
}
