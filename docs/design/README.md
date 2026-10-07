# PT Studio 设计文档

> 状态：草案 v0.1 · 最后更新：2026-10-04

PT Studio 是一个面向几十万用户、可水平扩展的 PT 站点系统：

- **Tracker**：基于 [Torrust](https://github.com/torrust/torrust-tracker) 各组件组装的 PT 专用 tracker
- **后端**：Rust，微内核 + 万物皆插件（Native 插件 + WASM 插件）
- **前端**：React，可整体替换，也可二次开发
- **AI**：原生支持，提供 MCP 服务

## 文档目录

| 文档 | 内容 |
|---|---|
| [01-architecture.md](01-architecture.md) | 设计原则、总体架构、微内核、双层插件、预设、技术栈、仓库结构 |
| [02-plugin-system.md](02-plugin-system.md) | 插件加载、扩展点、WIT 契约、清单与权限、数据访问（基础方法 / SQL 执行器 / Redis） |
| [03-tracker-and-accounting.md](03-tracker-and-accounting.md) | Torrust 集成、水平扩展、记账链路、反作弊信号 |
| [04-search.md](04-search.md) | `search-meilisearch` 插件 |
| [05-content-types-and-terms.md](05-content-types-and-terms.md) | 混合资源类型、术语表系统 |
| [06-frontend.md](06-frontend.md) | 可替换前端、插槽、Schema 驱动 UI |
| [07-ai-and-mcp.md](07-ai-and-mcp.md) | `ctx.llm`、MCP 网关、AI 插件 |
| [08-lessons-learned.md](08-lessons-learned.md) | 现有开源 PT 程序的教训与对策 |
| [09-roadmap.md](09-roadmap.md) | 路线图、待验证项、待决问题 |
| [10-torrust-integration.md](10-torrust-integration.md) | Torrust 集成技术方案：源码调研、方案对比、pt-tracker 详细设计、原型验证结果 |

## 已确定的决策

| # | 决策 | 来源 |
|---|---|---|
| D1 | 目标规模几十万用户，所有组件必须可水平扩展 | 需求 |
| D2 | 流量统计采用 Redis 实时计数 + Redis Streams 队列 + 按时间窗口聚合 + 定时批量落库 | 需求 |
| D3 | 插件数据访问 = 类型化基础方法 + 高级 SQL 执行器 + Redis 直接操作 | 需求 |
| D4 | 插件只在启动前放入指定目录，启动时加载；启动后后台只能查看，不能安装或卸载 | 需求 |
| D5 | 核心引擎 + 万物皆插件，参考 DeepSeek Harness（Cordis）的微内核设计 | 需求 |
| D6 | 不做多租户：一套部署对应一个 PT 站 | 需求 |
| D7 | 站点可自定义术语（魔力值、等级等的名称），由术语表系统统一支持 | 需求 |
| D8 | 支持混合资源类型，资源类型本身是插件 | 需求 |
| D9 | Tracker 使用 Torrust，具体集成方案通过实测确定 | 需求 |
| D10 | 原生支持 AI，提供 MCP 服务 | 需求 |
| D11 | 借鉴现有开源 PT 程序的教训，避免重复犯错 | 需求 |
| D12 | 搜索由 `search-meilisearch` 插件提供 `ctx.search` 服务 | 需求 |
| D13 | 账本也是插件（`ledger` 提供 `ctx.ledger`，支持多币种） | 由 D5 推出 |
| D14 | pt-tracker 采用“收窄组装”（方案 B′）：只依赖 torrust 的 swarm-coordination-registry、http-protocol、primitives、events（git rev 锁定），HTTP 入口、鉴权、过滤、增量和事件出口自己实现，torrust 藏在适配层后面 | spike 实测，见 10 号文档；细化 D9 |
| D15 | tracker 只支持 HTTP(S) announce，不提供 UDP | spike 实测（torrust 私有模式禁用 UDP，也没有实现 BEP 41） |
| D16 | `crates/pt-tracker` 单独使用 AGPL-3.0-only 许可证，与内核和插件只通过 Redis 通信，不互相链接代码 | torrust 各包都是 AGPL-3.0-only；用户确认保留 AGPL，不切换方案 D |
| D17 | 增量计算依据 pt-tracker 自己维护的 peer 会话状态（user_id + info_hash + peer_id，写穿 Redis），不依赖 torrust 的 swarm 或 events | spike 实测（torrust 的事件和 swarm 不带用户身份，也拿不到旧值） |
| D18 | 数据库使用 PostgreSQL（最低 16，推荐 17），不支持 MySQL | 用户确认 |
| D19 | 内核与插件采用 MIT OR Apache-2.0 双许可证；pt-tracker 仍为 AGPL-3.0-only（D16），二者只通过 Redis 通信、互不链接代码 | 用户确认 |
