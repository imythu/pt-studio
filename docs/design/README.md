# PT Studio 设计文档

> 架构级设计文档。写于实现之前（2026-10-04 起），实现中的调整记录在下方决策表与 [../dev/design-deviations.md](../dev/design-deviations.md)。

PT Studio 是一个面向几十万用户、可水平扩展的 PT 站点系统：

- **Tracker**：自研的 PT 专用 HTTP tracker，状态全部存于 Redis、无状态多节点（D20；早期的 Torrust 方案见 10 号文档）
- **后端**：Rust，微内核 + 万物皆插件（Native 插件 + WASM 插件）
- **前端**：React，可整体替换，也可二次开发
- **AI**：原生支持，提供 MCP 服务

## 文档目录

| 文档 | 内容 |
|---|---|
| [01-architecture.md](01-architecture.md) | 设计原则、总体架构、微内核、双层插件、预设、技术栈、仓库结构 |
| [02-plugin-system.md](02-plugin-system.md) | 插件加载、扩展点、WIT 契约、清单与权限、数据访问（基础方法 / SQL 执行器 / Redis） |
| [03-tracker-and-accounting.md](03-tracker-and-accounting.md) | Tracker、水平扩展、记账链路、反作弊信号 |
| [04-search.md](04-search.md) | `search-meilisearch` 插件 |
| [05-content-types-and-terms.md](05-content-types-and-terms.md) | 混合资源类型、术语表系统 |
| [06-frontend.md](06-frontend.md) | 可替换前端、插槽、Schema 驱动 UI |
| [07-ai-and-mcp.md](07-ai-and-mcp.md) | `ctx.llm`、MCP 网关、AI 插件 |
| [08-lessons-learned.md](08-lessons-learned.md) | 现有开源 PT 程序的教训与对策 |
| [09-roadmap.md](09-roadmap.md) | 路线图、待验证项、待决问题 |
| [10-torrust-integration.md](10-torrust-integration.md) | Torrust 集成调研（已由 D20 取代，保留作为决策依据）：源码调研、方案对比、原型验证结果 |
| [11-plugin-settings.md](11-plugin-settings.md) | 统一插件配置模型：一份声明、分层取值（默认 / 部署 / 站点 / 用户）、类型清单、内核校验与合并、SDK 生成类型、宿主统一渲染 |

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
| D9 | ~~Tracker 使用 Torrust，具体集成方案通过实测确定~~ → 由 D20 取代：tracker 自研 | 需求（2026-10-14 用户改为自研） |
| D10 | 原生支持 AI，提供 MCP 服务 | 需求 |
| D11 | 借鉴现有开源 PT 程序的教训，避免重复犯错 | 需求 |
| D12 | 搜索由 `search-meilisearch` 插件提供 `ctx.search` 服务 | 需求 |
| D13 | 账本也是插件（`ledger` 提供 `ctx.ledger`，支持多币种） | 由 D5 推出 |
| D14 | ~~pt-tracker 采用“收窄组装”（方案 B′）：只依赖 torrust 的 swarm-coordination-registry、http-protocol、primitives、events（git rev 锁定），HTTP 入口、鉴权、过滤、增量和事件出口自己实现，torrust 藏在适配层后面~~ → 由 D20 取代 | spike 实测，见 10 号文档；细化 D9 |
| D15 | tracker 只支持 HTTP(S) announce，不提供 UDP | spike 实测（torrust 私有模式禁用 UDP，也没有实现 BEP 41） |
| D16 | `crates/pt-tracker` 单独使用 AGPL-3.0-only 许可证，与内核和插件只通过 Redis 通信，不互相链接代码 | torrust 各包都是 AGPL-3.0-only；用户确认保留 AGPL，不切换方案 D |
| D17 | 增量计算依据 pt-tracker 自己维护的 peer 会话状态（user_id + info_hash + peer_id，写穿 Redis），不依赖 torrust 的 swarm 或 events | spike 实测（torrust 的事件和 swarm 不带用户身份，也拿不到旧值） |
| D18 | 数据库使用 PostgreSQL（最低 16，推荐 17），不支持 MySQL | 用户确认 |
| D19 | 内核与插件采用 MIT OR Apache-2.0 双许可证；pt-tracker 仍为 AGPL-3.0-only（D16），二者只通过 Redis 通信、互不链接代码 | 用户确认 |
| D20 | pt-tracker 自研，不依赖 torrust：HTTP 协议（BEP 3 / 23 / 7 / 48）、准入、增量计算与事件出口自己实现；**swarm 与会话状态全部存在 Redis**（每个种子的做种 / 下载 ZSET + 会话 HASH，Lua 脚本 CAS），tracker 无状态、可多节点水平扩展、不需要按 info_hash 分片。D15（只做 HTTP）、D16（AGPL，只经 Redis 通信）、D17（自己维护会话状态）保持不变 | 2026-10-14 用户决定（torrust 相关包未发布到 crates.io，develop 分支 API 不稳定）；真实客户端（Transmission 4.1）与压测验证见 components/pt-tracker、dev/README |
| D21 | 插件配置统一为 `plugin.toml [[settings]]` 一份声明：值按“清单默认 → 部署层（site.toml / 环境变量）→ 站点层（后台）→ 用户层（个人设置）”覆盖，字段以 `editable` 声明最高可写层；内核负责校验、合并、存储、审计与变更通知，SDK 编译期生成强类型，宿主统一渲染后台与个人设置界面。破坏式替换 `settings_schema` / `settings_ui` / `config.get-static` / `get-settings`，见 11 号文档 | 2026-10-14 用户决定（不考虑历史兼容） |
