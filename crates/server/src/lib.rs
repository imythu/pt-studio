//! `pt-server`：选择编译进二进制的 Native 插件（plugin-api.md §3.1）。

use pt_plugin_api::NativePlugin;

/// 本二进制编译进的全部 Native 插件（是否启用由 site.toml `[plugins] disabled` 决定）。
#[allow(unused_mut, clippy::vec_init_then_push)]
pub fn native_plugins() -> Vec<NativePlugin> {
    let mut v = Vec::new();
    #[cfg(feature = "identity")]
    v.push(pt_plugin_identity::plugin());
    #[cfg(feature = "torrent")]
    v.push(pt_plugin_torrent::plugin());
    #[cfg(feature = "ledger")]
    v.push(pt_plugin_ledger::plugin());
    #[cfg(feature = "accounting")]
    v.push(pt_plugin_accounting::plugin());
    #[cfg(feature = "search")]
    v.push(pt_plugin_search_meilisearch::plugin());
    #[cfg(feature = "llm-claude")]
    v.push(pt_plugin_llm_claude::plugin());
    #[cfg(feature = "mcp")]
    v.push(pt_plugin_mcp::plugin());
    #[cfg(feature = "ai-assistant")]
    v.push(pt_plugin_ai_assistant::plugin());
    v
}
