//! HTTP 处理的便捷封装。`world!("…", http)` 会为插件的 `Request` 实现 [`RequestExt`]，并提供 `Reply → Response` 的转换。

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{R, err};

/// 应答（`.into()` 转为插件导出接口的 `Response`）
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    /// JSON 应答
    pub fn json(status: u16, v: &Value) -> Self {
        Self {
            status,
            headers: vec![("content-type".into(), "application/json".into())],
            body: v.to_string().into_bytes(),
        }
    }

    pub fn ok(v: &Value) -> Self {
        Self::json(200, v)
    }

    pub fn created(v: &Value) -> Self {
        Self::json(201, v)
    }

    pub fn no_content() -> Self {
        Self {
            status: 204,
            headers: vec![],
            body: vec![],
        }
    }

    /// 任意内容类型
    pub fn bytes(status: u16, content_type: &str, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: vec![("content-type".into(), content_type.into())],
            body,
        }
    }

    #[must_use]
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// 请求的读取方法（由 `world!(…, http)` 为插件的 `Request` 实现）
pub trait RequestExt {
    /// 清单中的路由 ID
    fn route(&self) -> &str;
    fn query_pairs(&self) -> &[(String, String)];
    fn param_pairs(&self) -> &[(String, String)];
    fn header(&self, name: &str) -> Option<&str>;
    fn body_bytes(&self) -> &[u8];

    /// 查询参数
    fn query(&self, key: &str) -> Option<&str> {
        self.query_pairs()
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// 路径参数（按路由模板解析，如 `/v1/items/{id}` 的 `id`）
    fn param(&self, key: &str) -> Option<&str> {
        self.param_pairs()
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// 整数路径参数；缺失或格式不对返回 `kernel.bad_request`
    fn param_i64(&self, key: &str) -> R<i64> {
        self.param(key).and_then(|v| v.parse().ok()).ok_or_else(|| {
            err(
                "kernel.bad_request",
                &format!("path parameter `{key}` must be an integer"),
            )
        })
    }

    /// 请求体解析为类型；格式不对返回 `kernel.bad_request`
    fn body_json<T: DeserializeOwned>(&self) -> R<T> {
        serde_json::from_slice(self.body_bytes())
            .map_err(|e| err("kernel.bad_request", &format!("invalid JSON body: {e}")))
    }

    /// 请求体解析为 `Value`（空或非法时为 `Null`）
    fn body_value(&self) -> Value {
        serde_json::from_slice(self.body_bytes()).unwrap_or(Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Req {
        q: Vec<(String, String)>,
        p: Vec<(String, String)>,
        body: Vec<u8>,
    }

    impl RequestExt for Req {
        fn route(&self) -> &str {
            "r"
        }
        fn query_pairs(&self) -> &[(String, String)] {
            &self.q
        }
        fn param_pairs(&self) -> &[(String, String)] {
            &self.p
        }
        fn header(&self, _: &str) -> Option<&str> {
            None
        }
        fn body_bytes(&self) -> &[u8] {
            &self.body
        }
    }

    #[test]
    fn request_helpers() {
        let r = Req {
            q: vec![("a".into(), "1".into())],
            p: vec![("id".into(), "42".into()), ("x".into(), "y".into())],
            body: br#"{"n": 3}"#.to_vec(),
        };
        assert_eq!(r.query("a"), Some("1"));
        assert_eq!(r.query("b"), None);
        assert_eq!(r.param_i64("id").ok(), Some(42));
        assert_eq!(
            r.param_i64("x").err().map(|e| e.code),
            Some("kernel.bad_request".to_owned())
        );
        assert_eq!(r.body_value(), json!({ "n": 3 }));
        #[derive(serde::Deserialize)]
        struct B {
            n: u8,
        }
        assert_eq!(r.body_json::<B>().ok().map(|b| b.n), Some(3));
    }

    #[test]
    fn replies() {
        let r = Reply::created(&json!({ "id": 1 })).header("x-a", "b");
        assert_eq!(r.status, 201);
        assert_eq!(r.headers.len(), 2);
        assert_eq!(r.body, br#"{"id":1}"#);
        assert_eq!(Reply::no_content().status, 204);
    }
}
