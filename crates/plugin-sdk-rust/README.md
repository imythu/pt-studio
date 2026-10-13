# pt-plugin-sdk

PT Studio WASM 插件的 Rust SDK（目标 `wasm32-wasip2`，插件 API `pt:plugin@1.0.0`）：

- 宿主接口绑定（`pt_plugin_sdk::{sql, kv, redis, events, …}`），插件用 `pt_plugin_sdk::world!("<世界名>"[, http])`
  生成自己的导出绑定并复用这些导入；
- 便捷封装：`err`、`SqlValue` 的 `From` 转换与 `RowExt`、`ctx`（当前用户、作用域、设置、服务调用、事件、钩子、延迟任务）、
  `http`（`Reply`、`RequestExt`）、`logging`。

用法见 [docs/sdk/README.md](../../docs/sdk/README.md) §5，完整示例见 [templates/wasm-plugin](../../templates/wasm-plugin)，
参考插件见 `plugins/wasm/checkin`。
