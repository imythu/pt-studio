//! PT Studio 插件 API（契约见 docs/platform/plugin-api.md）。
//!
//! Native 插件与 WASM 插件对外行为一致；本 crate 是 Native 侧的 Rust 契约，
//! WASM 侧的 WIT 包 `pt:plugin@0.1.0` 与之一一对应。

mod ctx;
mod error;
mod exports;
pub mod host;
pub mod manifest;
pub mod services;
mod types;

pub use ctx::{Credential, Ctx, JsonService, KernelLink, Svc, service_object};
pub use error::{PResult, PluginError, suffix_status};
pub use exports::*;
pub use host::{AuditEntry, NewEvent, RedisCommand, RedisReply, SqlValue};
pub use types::*;

/// 内核支持的插件 API 版本（主.次）。
pub const API_VERSIONS: &[&str] = &["pt:plugin@0.1"];

pub use async_trait::async_trait;
pub use serde_json::{Value as Json, json};
