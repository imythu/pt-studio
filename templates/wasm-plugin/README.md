# WASM 插件模板（notes）

一个可运行的完整示例：每个用户的个人便签。用到 HTTP 路由、SQL 与迁移、后台设置、事件、自定义钩子点、清单错误码、
MCP 工具、权限点与术语。开发指南见 [docs/sdk/README.md](../../docs/sdk/README.md)。

```sh
rustup target add wasm32-wasip2
cargo build --release --target wasm32-wasip2
cargo test                       # 纯函数的单元测试（主机目标）
pt-server plugin package --src . --wasm target/wasm32-wasip2/release/pt_wasm_notes.wasm --out dist
```

然后在站点 `site.toml` 中设置 `[plugins] dir = "<本目录>/dist"`（开发环境加 `allow_unsigned = true`）。

改成自己的插件：

1. `plugin.toml` 的 `id`，以及所有 `notes.` 前缀（权限、术语、错误码、事件、钩子点）；
2. `Cargo.toml` 的包名（产物文件名随之变化）；仓库外开发时把 `pt-plugin-sdk` 改为 git 依赖；
3. `wit/world.wit` 的世界名与 `src/lib.rs` 中 `world!("…")` 一致，只 export 实现了的接口；
4. `wit/deps/pt-plugin/plugin.wit` 是 `pt:plugin@1.0.0` 的副本，不要修改。

本仓库的 `crates/server/tests/e2e_template.rs` 会构建并加载这个模板。
