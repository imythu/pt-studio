//! 插件错误（契约见 docs/platform/plugin-api.md §3、conventions/api.md §7）。

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

/// 跨插件、跨宿主边界传递的错误。`code` 是稳定契约（`<plugin_id>.<snake_case>`）。
#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error, PartialEq)]
#[error("{code}: {message}")]
pub struct PluginError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<Json>,
    #[serde(default)]
    pub retryable: bool,
    /// 显式 HTTP 状态；缺省时按 error-codes 目录与后缀规则推导（conventions/api.md §7）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
}

pub type PResult<T> = Result<T, PluginError>;

impl PluginError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
            retryable: false,
            status: None,
        }
    }

    #[must_use]
    pub fn with_details(mut self, details: Json) -> Self {
        self.details = Some(details);
        self
    }

    #[must_use]
    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }

    #[must_use]
    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("kernel.internal", message)
    }
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new("kernel.not_found", message)
    }
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new("kernel.bad_request", message)
    }
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new("kernel.validation_failed", message)
    }
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new("kernel.forbidden", message)
    }
    pub fn unauthenticated() -> Self {
        Self::new("kernel.unauthenticated", "authentication required")
    }
    pub fn capability_denied(message: impl Into<String>) -> Self {
        Self::new("kernel.capability_denied", message)
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new("kernel.conflict", message).retryable()
    }
    pub fn unavailable(message: impl Into<String>) -> Self {
        Self::new("kernel.service_unavailable", message).retryable()
    }

    /// 推导 HTTP 状态：显式状态 → 内核通用码 → 后缀规则。
    /// 插件登记在 error-codes 目录中的状态由内核在网关层另行覆盖。
    pub fn http_status(&self) -> u16 {
        if let Some(s) = self.status {
            return s;
        }
        if let Some(s) = kernel_status(&self.code) {
            return s;
        }
        suffix_status(&self.code)
    }
}

fn kernel_status(code: &str) -> Option<u16> {
    Some(match code {
        "kernel.bad_request" => 400,
        "kernel.validation_failed" | "kernel.idempotency_key_reused" => 422,
        "kernel.unauthenticated" | "kernel.two_factor_required" => 401,
        "kernel.forbidden" | "kernel.csrf_failed" | "kernel.hook_denied" => 403,
        "kernel.not_found" => 404,
        "kernel.method_not_allowed" => 405,
        "kernel.conflict" | "kernel.idempotency_in_progress" => 409,
        "kernel.precondition_failed" => 412,
        "kernel.payload_too_large" => 413,
        "kernel.unsupported_media_type" => 415,
        "kernel.rate_limited" | "kernel.quota_exceeded" => 429,
        "kernel.hook_failed"
        | "kernel.service_unavailable"
        | "kernel.resource_exhausted"
        | "kernel.maintenance" => 503,
        "kernel.timeout" => 504,
        c if c.starts_with("kernel.") => 500,
        _ => return None,
    })
}

/// conventions/api.md §7 的后缀规则。
pub fn suffix_status(code: &str) -> u16 {
    let name = code.rsplit('.').next().unwrap_or(code);
    let ends = |sfx: &[&str]| sfx.iter().any(|s| name.ends_with(s));
    if ends(&["_not_found"]) {
        404
    } else if ends(&[
        "_taken",
        "_duplicate",
        "_conflict",
        "_exists",
        "_in_progress",
        "_stale",
    ]) {
        409
    } else if ends(&[
        "_denied",
        "_forbidden",
        "_restricted",
        "_disabled",
        "_banned",
        "_required",
    ]) {
        403
    } else if ends(&["_unauthenticated", "_expired_session"]) {
        401
    } else if ends(&["_rate_limited", "_too_many"]) {
        429
    } else if ends(&["_unavailable"]) {
        503
    } else if ends(&["_timeout"]) {
        504
    } else if ends(&["_upstream_error"]) {
        502
    } else {
        422
    }
}

impl From<sqlx::Error> for PluginError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(db) = &e {
            // 40001 serialization_failure / 40P01 deadlock：可重试
            if matches!(db.code().as_deref(), Some("40001" | "40P01")) {
                return PluginError::conflict("transaction conflict");
            }
        }
        tracing::warn!(error = %e, "sql error");
        PluginError::internal("database error")
    }
}

impl From<redis::RedisError> for PluginError {
    fn from(e: redis::RedisError) -> Self {
        tracing::warn!(error = %e, "redis error");
        PluginError::unavailable("redis error")
    }
}

impl From<serde_json::Error> for PluginError {
    fn from(e: serde_json::Error) -> Self {
        PluginError::bad_request(format!("invalid json: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_rules() {
        assert_eq!(PluginError::new("identity.username_taken", "").http_status(), 409);
        assert_eq!(
            PluginError::new("kernel.precondition_failed", "").http_status(),
            412
        );
        assert_eq!(PluginError::new("hnr.download_restricted", "").http_status(), 403);
        assert_eq!(PluginError::new("torrent.invalid_file", "").http_status(), 422);
        assert_eq!(PluginError::new("x.y_not_found", "").http_status(), 404);
        assert_eq!(PluginError::new("kernel.reentrant_call", "").http_status(), 500);
        assert_eq!(PluginError::new("a.b", "").with_status(202).http_status(), 202);
    }
}
