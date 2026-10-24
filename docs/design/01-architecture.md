# 01 · 总体架构

## 1. 设计原则

1. **核心提供机制，插件决定策略。**
   判断一个功能该不该进核心，就问：换一个站点规则，它会不会变？会变的就做成插件。
   - 例：基础服务（accounting 插件）提供“流量倍率”字段和结算逻辑，“什么时候免费、什么时候 2x”由促销插件决定，并通过 `ctx.accounting.set_rate_rule()` / `remove_rate_rule()` 下发。
   - 例：身份插件提供 `user.before_register` 钩子，“必须有邀请码才能注册”由邀请插件实现（钩子统一命名为 `<domain>.before_<verb>`，见 [../platform/catalog/hooks.md](../platform/catalog/hooks.md)）。
2. **热路径不过 WASM。** announce 每秒上万次，绝不能每次都调插件。插件只在低频操作（上传、注册、下载种子）上做同步拦截，其余场景一律通过异步事件处理。
3. **先写插件，再冻结 API。** 先用插件 API 实现魔力值、促销、等级、邀请、H&R 等真实插件，API 被这些插件验证过之后再定为 v1。
4. **万物皆插件。** 内核只是平台，不包含任何 PT 业务，连身份系统、种子管理都是可替换的插件（参考 DeepSeek Harness / Cordis）。

## 2. 总体架构图

```
                         ┌──────────── pt-tracker 集群 (无状态，按 info_hash 分片) ────────────┐
 BT 客户端 ─► LB(hash info_hash) ─► [torrust 组件 + passkey鉴权 + 客户端过滤 + 增量计算] × N   │
                         └───────────────────────┬─────────────────────────────────────────┘
                                                 │ XADD (按 user_id 分区的 N 条流)
                                                 ▼
                                     Redis Streams / Redis 实时计数
                                                 │
                    ┌────────────────────────────▼───────────────────────────────┐
  React 前端 ◄─REST─┤  Kernel（微内核，可多副本）                                   │
  AI 客户端 ◄─MCP───┤  插件加载 │ 服务注册表 │ Hook 管线 │ 事件日志 │ 调度 │ 网关 │ 权限 │
                    │───────────────────────────────────────────────────────────│
                    │ Native 插件 (Rust, 编译进二进制)  │  WASM 插件 (wasmtime)      │
                    │ identity / torrent / accounting  │ bonus / levels / invite / │
                    │ ledger / search / mcp / ...      │ hnr / promotion / forum...│
                    └───────────┬───────────────────────────────────────────────┘
                                ▼
                 PostgreSQL（每个插件一个 schema） / Redis（键前缀隔离） / S3 / Meilisearch
```

## 3. 借鉴 DeepSeek Harness（Cordis）的部分

| Harness / Cordis 概念 | 在本项目中的对应 |
|---|---|
| 微内核，模型、工具、会话、UI 都是插件 | 内核只做平台，PT 业务全部是插件 |
| 通过服务名互相调用（`ctx.tools`、`ctx.llm`） | 服务注册表：`ctx.users`、`ctx.ledger`、`ctx.search`… |
| 插件只声明依赖，框架自动决定启动顺序 | 启动时根据清单中的 provides / requires 做拓扑排序 |
| 插件的副作用由框架记录，卸载时自动清理 | 插件注册的路由、钩子、订阅、定时任务全部由内核登记管理 |
| 只追加的事件日志，支持回放 | 事件日志（outbox → Redis Streams），可回放重算 |
| profile / preset | 站点预设（`presets/*.toml`） |

**不借鉴的部分**：Harness 被批评较多的是动态注册和插件生命周期的复杂性。本项目只在启动时加载插件（决策 D4），没有这类问题。

## 4. 内核职责

内核只做“平台”，不包含任何 PT 业务。

| 内核组件 | 作用 |
|---|---|
| 插件加载器 | 启动时扫描 `plugins/` 目录，读取清单，解析依赖，按拓扑顺序激活；启动后只读 |
| 服务注册表 | 插件提供服务、依赖服务；同一个服务可以有多个实现，由配置指定使用哪个 |
| Hook 管线 | 同步拦截器，带优先级，可以返回放行、拒绝或修改 |
| 事件日志 | 只追加，使用 outbox 模式，通过 Redis Streams 投递，支持回放 |
| 存储网关 | PostgreSQL、Redis、对象存储的连接池；按插件隔离；配额限制 |
| 网关 | HTTP 路由注册、MCP 工具注册、统一鉴权中间件（实际鉴权调用 `ctx.auth`） |
| 调度器 | cron 任务 + 分布式锁，多副本部署时只有一个副本执行 |
| 权限与审计 | 校验插件声明的能力；所有写操作留痕 |
| 术语与配置 | 术语表服务、配置的 JSON Schema 校验 |
| 可观测性 | 指标、链路追踪、结构化日志，按插件维度统计 |

## 5. 双层插件

| | Native 插件 | WASM 插件 |
|---|---|---|
| 形式 | Rust crate，**编译时选入**（Rust 没有稳定 ABI） | `.wasm` 组件，从目录加载 |
| 用途 | 热路径和基础服务 | 站点规则、业务功能 |
| 例子 | identity、torrent、accounting、ledger、terms、search-meilisearch、mcp、llm-* | bonus、levels、invite、promotion、hnr、forum、type-movie、type-music… |
| 契约 | **与 WASM 插件一致**，都通过服务注册表交互 | 同左 |

这样既做到了万物皆插件（连身份系统都可以替换），热路径也不受 WASM 的性能开销影响。

## 6. 站点预设

```toml
# site.toml
preset = "standard"            # minimal / standard / music / ...

[services]
"ctx.auth"       = "identity-basic"
"ctx.accounting" = "accounting-std"
"ctx.search"     = "search-meilisearch"
"ctx.levels"     = "levels-nexus-style"   # 可换成 levels-gazelle-style

[plugins.disabled]
list = ["lottery"]
```

## 7. 插件分类总览

**Native（基础服务）**

| 插件 | 提供的服务 |
|---|---|
| identity | `ctx.auth`、`ctx.users`、`ctx.rbac`（权限点可由其他插件注册）；2FA（TOTP / WebAuthn）、tracker passkey |
| torrent | `ctx.torrents`：解析 .torrent（v1 / v2 / hybrid）、强制 `private=1`、重写 announce、下载时注入 passkey |
| tracker-bridge | `ctx.tracker`：向 pt-tracker 同步 passkey、种子白名单、用户状态、客户端规则 |
| accounting | `ctx.accounting`：消费流量事件、窗口聚合、落库、倍率规则（`set_rate_rule`）、反作弊基础信号 |
| ledger | `ctx.ledger`：多币种账本 |
| terms | `ctx.terms`：术语表 |
| search-meilisearch | `ctx.search` |
| mcp | MCP 网关 |
| llm-claude、llm-openai-compat | `ctx.llm` 的实现（Claude；OpenAI 兼容接口，本地模型如 vLLM、Ollama 也走这一实现，不单列 `llm-local`） |

完整插件清单（ID、文档缩写、提供的服务）以 [../platform/catalog/plugins.md](../platform/catalog/plugins.md) 为准。

**WASM（站点规则与业务）**

- 经济类：魔力值 / 做种积分、商城、签到、彩票、勋章
- 规则类：分享率要求、H&R 考核、新手考核、封禁策略、盒子（seedbox）规则
- 促销类：免费 / 2x / 50%，按体积、新种或时间自动促销
- 社区类：评论、论坛、站内信、求种、悬赏、字幕、候选区
- 内容类：各资源类型插件（`type-*`，实现 `ctx.content_types`）、元数据源（`metadata-tmdb` / `-douban` / `-imdb` / `-musicbrainz` / `-bangumi` / `-igdb`，实现 `ctx.metadata`）、MediaInfo、NFO、截图
- 注册类：邀请制、开放注册、答题、验证码、邮箱域名白名单
- 集成类：RSS、Telegram / Discord 通知、IYUU 等辅种 API、Webhook
- 风控类：高级反作弊规则、多号检测

## 8. 技术栈

| 层 | 选型 |
|---|---|
| Web 框架 | axum + tower |
| 数据库 | PostgreSQL（最低 16，推荐 17，D18）+ sqlx |
| 缓存 / 队列 | Redis（实时计数、Redis Streams） |
| 插件运行时 | wasmtime（Component Model + WIT） |
| 搜索 | Meilisearch |
| 对象存储 | S3 兼容（MinIO），存放 .torrent、图片 |
| API 文档 | utoipa → OpenAPI → openapi-typescript |
| 前端 | React + Vite + TanStack Query + 动态 ESM 插件 |

## 9. 仓库结构

以下为概要，细化规则见 [../platform/conventions/coding.md](../platform/conventions/coding.md) §1。

```
pt-studio/
├─ Cargo.toml              # 主 workspace（MIT OR Apache-2.0，D19）；exclude = ["crates/pt-tracker"]
├─ rust-toolchain.toml
├─ deny.toml               # cargo-deny：许可证、漏洞、来源；禁止依赖 pt-tracker
├─ crates/
│  ├─ plugin-api/          # WIT（pt:plugin@1.0.0）+ Native 插件的 trait + 清单类型（两层共享同一份契约）
│  ├─ plugin-api-macros/   # proc-macro：service!、export_service!、native_plugins!
│  ├─ plugin-sdk-rust/     # 写 WASM 插件用的 SDK
│  ├─ kernel/              # 内核库：加载器、服务注册表、hooks、事件日志、调度、网关
│  ├─ server/              # pt-server 二进制，用 cargo features 选择 Native 插件
│  ├─ testkit/             # 插件测试工具（内存宿主、testcontainers 封装、事件断言）
│  └─ pt-tracker/          # 独立 workspace，AGPL-3.0-only（D16）；组装 torrust 各包，加入鉴权、
│                          #   客户端过滤和事件出口；只通过 Redis 通信，不得被任何其他 crate 依赖
├─ plugins/
│  ├─ native/<id>/         # identity, torrent, tracker-bridge, accounting, ledger, terms,
│  │                       #   search-meilisearch, mcp, llm-claude, llm-openai-compat …（crate 名 pt-plugin-<id>）
│  └─ wasm/<id>/           # bonus, levels, invite, promotion, hnr, type-movie, type-music, forum …
├─ web/                    # pnpm workspace：sdk(自动生成) / ui-kit / slots / theme / app
├─ presets/                # standard.toml, music.toml ...
├─ deploy/                 # compose、k8s、redis ACL 模板
├─ xtask/                  # cargo xtask：gen-catalog、package、sign、gen-openapi
└─ docs/
   ├─ design/              # 总体设计（本目录）
   ├─ platform/            # 平台契约：plugin-api、内核文档、名字目录、规范
   ├─ plugins/<id>/        # 各插件的四份文档
   └─ process/             # 文档流程与模板
```
