# Native 插件模板（greeter）

Native 插件编译进 `pt-server`：订阅 `user.registered` 记录欢迎时间，提供 `GET /api/p/greeter/v1/me`。

接入步骤：

1. 复制到 `plugins/native/<id>/`，`Cargo.toml` 中的依赖改为工作区依赖（`pt-plugin-api.workspace = true` 等），删除 `[workspace]`；
2. `crates/server/Cargo.toml`：加入 `pt-plugin-<id> = { path = "../../plugins/native/<id>", optional = true }`，
   一个同名 feature，并加入 `default`；
3. `crates/server/src/lib.rs` 的 `native_plugins()`：`#[cfg(feature = "<id>")] v.push(pt_plugin_<id>::plugin());`。

第三方插件请优先写 WASM 插件（见 `templates/wasm-plugin` 与 [docs/sdk/README.md](../../docs/sdk/README.md)）。
