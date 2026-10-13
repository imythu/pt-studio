//! PT Studio 插件 SDK（Rust，`wasm32-wasip2`）。
//!
//! - 宿主接口 `pt:plugin@1.0.0` 的绑定在本 crate 中生成一次（`pt_plugin_sdk::sql`、`pt_plugin_sdk::kv` …），插件用
//!   [`world!`] 生成自己的导出绑定并复用这些导入，不必各自生成宿主接口。
//! - 便捷封装：错误构造（[`err`]）、SQL 参数与结果读取（[`SqlValue`] 的 `From` 实现、[`RowExt`]）、调用上下文
//!   （[`ctx`]）、业务服务（[`ctx::call`]）、事件、钩子、延迟任务、日志（[`logging`]）、HTTP 应答（[`http`]）。
//!
//! 最小插件：
//!
//! ```ignore
//! pt_plugin_sdk::world!("hello", http);
//!
//! use pt_plugin_sdk::prelude::*;
//! use exports::pt::plugin::http_handler;
//!
//! struct Hello;
//!
//! impl http_handler::Guest for Hello {
//!     fn handle(req: http_handler::Request) -> R<http_handler::Response> {
//!         let uid = ctx::current_user()?;
//!         Ok(http::Reply::json(200, &json!({ "hello": uid, "q": req.query("q") })).into())
//!     }
//! }
//!
//! export!(Hello);
//! ```

#![allow(clippy::missing_errors_doc)]

#[allow(clippy::all, clippy::pedantic)]
mod bindings {
    wit_bindgen::generate!({
        path: "wit",
        world: "pt:plugin/host@1.0.0",
        generate_all,
    });
}

pub use bindings::pt::plugin::{
    audit, blob, config, events, hook_runner, http_client, invocation, jobs, kv, log, metadata, metrics,
    random, redis, services, sql, terms, time, types,
};
pub use serde_json;
#[doc(hidden)]
pub use wit_bindgen;

pub use sql::SqlValue;
pub use types::PluginError;

pub mod ctx;
pub mod http;
pub mod logging;
mod values;

pub use values::RowExt;

/// 插件函数的返回类型
pub type R<T> = Result<T, PluginError>;

/// 常用导入：`use pt_plugin_sdk::prelude::*;`
pub mod prelude {
    pub use crate::http::{self, RequestExt as _};
    pub use crate::{PluginError, R, RowExt as _, SqlValue, ctx, err, logging};
    pub use serde_json::{Value, json};
}

/// 构造错误（code 须符合 catalog/error-codes.md 的规则，如 `"checkin.already_checked_in"`）
pub fn err(code: &str, message: &str) -> PluginError {
    PluginError {
        code: code.into(),
        message: message.into(),
        details: None,
        retryable: false,
    }
}

impl PluginError {
    /// 附带结构化细节（JSON 对象）
    #[must_use]
    pub fn with_details(mut self, details: &serde_json::Value) -> Self {
        self.details = Some(details.to_string());
        self
    }

    /// 标记为可重试（延迟任务与事件订阅会由内核重试）
    #[must_use]
    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }
}

/// 生成插件世界的绑定（导出接口在插件 crate 中生成，导入接口复用本 SDK）。
///
/// - `world!("checkin")`：插件目录下 `wit/world.wit` 中的世界名；
/// - `world!("checkin", http)`：世界导出 `http-handler` 时加上 `http`，额外生成 [`http::Reply`] → `Response` 的转换与
///   [`http::RequestExt`] 的实现。
#[macro_export]
macro_rules! world {
    ($world:literal) => {
        $crate::__generate!($world);
    };
    ($world:literal, http) => {
        $crate::__generate!($world);
        $crate::__http_glue!();
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __generate {
    ($world:literal) => {
        $crate::wit_bindgen::generate!({
            path: "wit",
            world: $world,
            runtime_path: "::pt_plugin_sdk::wit_bindgen::rt",
            with: {
                "pt:plugin/types@1.0.0": ::pt_plugin_sdk::types,
                "pt:plugin/invocation@1.0.0": ::pt_plugin_sdk::invocation,
                "pt:plugin/log@1.0.0": ::pt_plugin_sdk::log,
                "pt:plugin/config@1.0.0": ::pt_plugin_sdk::config,
                "pt:plugin/kv@1.0.0": ::pt_plugin_sdk::kv,
                "pt:plugin/sql@1.0.0": ::pt_plugin_sdk::sql,
                "pt:plugin/redis@1.0.0": ::pt_plugin_sdk::redis,
                "pt:plugin/events@1.0.0": ::pt_plugin_sdk::events,
                "pt:plugin/http-client@1.0.0": ::pt_plugin_sdk::http_client,
                "pt:plugin/blob@1.0.0": ::pt_plugin_sdk::blob,
                "pt:plugin/terms@1.0.0": ::pt_plugin_sdk::terms,
                "pt:plugin/time@1.0.0": ::pt_plugin_sdk::time,
                "pt:plugin/random@1.0.0": ::pt_plugin_sdk::random,
                "pt:plugin/audit@1.0.0": ::pt_plugin_sdk::audit,
                "pt:plugin/jobs@1.0.0": ::pt_plugin_sdk::jobs,
                "pt:plugin/metrics@1.0.0": ::pt_plugin_sdk::metrics,
                "pt:plugin/hook-runner@1.0.0": ::pt_plugin_sdk::hook_runner,
                "pt:plugin/metadata@1.0.0": ::pt_plugin_sdk::metadata,
                "pt:plugin/services@1.0.0": ::pt_plugin_sdk::services,
            },
            generate_all,
        });
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __http_glue {
    () => {
        impl ::core::convert::From<$crate::http::Reply> for exports::pt::plugin::http_handler::Response {
            fn from(r: $crate::http::Reply) -> Self {
                Self {
                    status: r.status,
                    headers: r
                        .headers
                        .into_iter()
                        .map(|(name, value)| exports::pt::plugin::http_handler::Header { name, value })
                        .collect(),
                    body: r.body,
                }
            }
        }

        impl $crate::http::RequestExt for exports::pt::plugin::http_handler::Request {
            fn route(&self) -> &str {
                &self.route_id
            }
            fn query_pairs(&self) -> &[(String, String)] {
                &self.query
            }
            fn param_pairs(&self) -> &[(String, String)] {
                &self.params
            }
            fn header(&self, name: &str) -> Option<&str> {
                self.headers
                    .iter()
                    .find(|h| h.name.eq_ignore_ascii_case(name))
                    .map(|h| h.value.as_str())
            }
            fn body_bytes(&self) -> &[u8] {
                &self.body
            }
        }
    };
}
