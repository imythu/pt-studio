//! PT Studio 插件 API（契约见 docs/platform/plugin-api.md）。
//!
//! Native 插件与 WASM 插件对外行为一致；本 crate 是 Native 侧的 Rust 契约，
//! WASM 侧的 WIT 包 `pt:plugin@1.0.0` 与之一一对应。

mod ctx;
mod error;
mod exports;
pub mod host;
pub mod manifest;
pub mod schema;
pub mod services;
mod types;

pub use ctx::{Credential, Ctx, JsonService, KernelLink, Svc, service_object};
pub use error::{PResult, PluginError, suffix_status};
pub use exports::*;
pub use host::{AuditEntry, NewEvent, RedisCommand, RedisReply, SqlValue};
pub use types::*;

/// 本 crate 对应的 `pt:plugin` WIT 包版本（主, 次）。1.0 起冻结，只做增量（新增接口提升次版本）。
pub const API_VERSION: (u32, u32) = (1, 0);

/// 清单 `api` 字段是否被本内核支持：`pt:plugin@<主>` 或 `pt:plugin@<主>.<次>`，主版本相同且次版本不高于内核。
/// （清单可写到次版本，表示用到了该次版本新增的接口。）
pub fn api_supported(api: &str) -> bool {
    let Some(v) = api.strip_prefix("pt:plugin@") else {
        return false;
    };
    let mut parts = v.split('.');
    let major = parts.next().and_then(|m| m.parse::<u32>().ok());
    let minor = match parts.next() {
        None => Some(0),
        Some(m) => m.parse::<u32>().ok(),
    };
    parts.next().is_none() && major == Some(API_VERSION.0) && minor.is_some_and(|m| m <= API_VERSION.1)
}

pub use async_trait::async_trait;
pub use serde_json::{Value as Json, json};

#[cfg(test)]
mod api_version_tests {
    use super::api_supported;

    #[test]
    fn api_field() {
        assert!(api_supported("pt:plugin@1"));
        assert!(api_supported("pt:plugin@1.0"));
        assert!(!api_supported("pt:plugin@1.1"), "newer minor than the kernel");
        assert!(!api_supported("pt:plugin@0.1"), "pre-1.0 is no longer accepted");
        assert!(!api_supported("pt:plugin@2"));
        assert!(!api_supported("pt:plugin@1.0.0"));
        assert!(!api_supported("pt:other@1"));
    }
}
