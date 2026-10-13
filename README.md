# PT Studio

私有 BT 站点（PT 站）的开源实现：Rust 微内核 + “一切皆插件”（WASM 与 Native 插件），PostgreSQL + Redis，React 前端。

- **内核**（`crates/kernel`）：插件加载与依赖解析、权限（RBAC、作用域、敏感能力授权）、调用上下文与工作单元、钩子管线、
  事件（outbox）、cron 与延迟任务、HTTP 网关（鉴权、CSRF、限流、幂等键）、MCP 工具注册。
- **插件**（`plugins/native/*`、`plugins/wasm/*`）：身份、种子、记账、魔力值、促销、等级、邀请、H&R、搜索、资源类型与元数据、
  评论、举报、审核、通知（站内 + 邮件）、站内信、签到、求种、RSS / Torznab、LLM、MCP 网关、站内助手……
- **前端**（`web/`）：应用外壳 + 插槽机制，插件前端按需加载。
- **插件 API**：`pt:plugin@1`（WIT 1.0.0）已冻结。

## 目录

| 路径 | 内容 |
|---|---|
| `crates/kernel`、`crates/server` | 内核库与 `pt-server` 二进制（Native 插件由 cargo feature 选择） |
| `crates/plugin-api` | 插件契约：WIT、Rust trait、清单类型 |
| `crates/plugin-sdk-rust` | WASM 插件的 Rust SDK（`pt-plugin-sdk`） |
| `crates/markup`、`crates/release-parser`、`crates/type-kit` | 共享库：安全的 Markdown / BBCode 渲染、发布名解析、资源类型工具 |
| `plugins/native`、`plugins/wasm` | 官方插件 |
| `templates/` | 插件模板（WASM、Native） |
| `web/` | 官方 React 前端 |
| `docs/` | 设计、平台契约与目录、各插件的设计文档、开发说明 |

## 开始

- 运行与测试：[docs/dev/README.md](docs/dev/README.md)（本地 PostgreSQL / Redis / Meilisearch、`pt-server serve`、测试方法）
- 写插件：[docs/sdk/README.md](docs/sdk/README.md)
- 设计总览：[docs/design/README.md](docs/design/README.md)
- 给 AI 工具的索引：[llms.txt](llms.txt)

```sh
pt-server --config site.toml check     # 清单与依赖检查（不连接数据库）
pt-server --config site.toml migrate   # 只执行迁移
pt-server --config site.toml serve     # 启动
pt-server plugin check <包目录>          # 检查一个 WASM 插件包
```

## 许可证

内核、插件、SDK 与前端：MIT OR Apache-2.0。pt-tracker（独立组件）：AGPL-3.0-only，不被其他代码链接。
