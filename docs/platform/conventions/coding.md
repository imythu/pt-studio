# 编码规范

> 版本 0.1 · 2026-10-04

## 1. 仓库布局

在 design/01 §9 的基础上细化：

```
pt-studio/
├─ Cargo.toml                 # 主 workspace（MIT OR Apache-2.0）；exclude = ["crates/pt-tracker"]
├─ rust-toolchain.toml        # 固定 Rust 版本
├─ deny.toml                  # cargo-deny：许可证、漏洞、来源
├─ crates/
│  ├─ plugin-api/             # pt-plugin-api：WIT（wit/）、Rust trait、清单类型、schemas/、service! 宏
│  ├─ plugin-api-macros/      # proc-macro：service!、export_service!、native_plugins!
│  ├─ plugin-sdk-rust/        # pt-plugin-sdk：写 WASM 插件的 Rust SDK（基于 wit-bindgen）
│  ├─ kernel/                 # pt-kernel：内核库
│  ├─ server/                 # pt-server：二进制，cargo features 选择 Native 插件
│  ├─ testkit/                # pt-testkit：插件测试工具（内存宿主、testcontainers 封装、事件断言）
│  └─ pt-tracker/             # 独立 workspace，AGPL-3.0-only（D16），不得被任何其他 crate 依赖
├─ plugins/
│  ├─ native/<id>/            # crate 名 pt-plugin-<id>
│  └─ wasm/<id>/              # 源码；构建产物 dist/plugins/<id>/
├─ web/                       # pnpm workspace
│  ├─ sdk/                    # @pt-studio/sdk（openapi-typescript 生成，勿手改）
│  ├─ ui-kit/                 # @pt-studio/ui-kit
│  ├─ slots/                  # @pt-studio/slots（插槽机制、插件加载器、PluginApi）
│  ├─ theme/
│  └─ app/                    # 官方前端
├─ presets/                   # standard.toml、music.toml …
├─ deploy/                    # compose、k8s、redis ACL 模板
├─ xtask/                     # cargo xtask：gen-catalog、package、sign、gen-openapi
└─ docs/
```

- 插件前端源码放在插件目录的 `ui/` 下，构建时依赖 `@pt-studio/slots`、`@pt-studio/ui-kit`、`@pt-studio/sdk`（作为 external，由 import map 提供）。
- `crates/pt-tracker` 不加入主 workspace，CI 中 `cargo deny` 规则禁止任何主 workspace crate 依赖它（见 licensing.md）。

## 2. Rust

### 2.1 工具链与 lint

- **edition 2024**；`rust-toolchain.toml` 固定稳定版（最低 1.85，跟随稳定版每季度升级一次）。
- `cargo fmt`（`rustfmt.toml`：`max_width = 110`、`imports_granularity = "Crate"`、`group_imports = "StdExternalCrate"`）。
- workspace 级 lint（`Cargo.toml [workspace.lints]`）：

```toml
[workspace.lints.rust]
unsafe_code = "deny"                 # 需要时在模块上 #[allow] 并写 SAFETY 注释（如 Component::deserialize）
missing_debug_implementations = "warn"
unused_must_use = "deny"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "warn", priority = -1 }
module_name_repetitions = "allow"
missing_errors_doc = "allow"
unwrap_used = "deny"                 # 测试中允许
expect_used = "warn"
panic = "deny"
todo = "deny"
dbg_macro = "deny"
print_stdout = "deny"
large_futures = "warn"
```

- CI：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo nextest run`、`cargo deny check`、`cargo udeps`（每周）。

### 2.2 错误处理

- 库与插件：`thiserror` 定义错误枚举；对外统一转换为 `PluginError { code, message, details, retryable }`，`code` 必须是已登记的错误码。
- 二进制、xtask、测试：可用 `anyhow`。
- **禁止 panic 作为错误处理**：不用 `unwrap()`/`expect()`（测试除外；不变量确实成立时用 `expect("说明为何不会失败")` 并经评审）。WASM 插件 panic 会变成 trap，对用户表现为 500。
- 不吞错误：`let _ = …` 丢弃 `Result` 时必须注释原因。
- 错误信息不含敏感数据。

### 2.3 异步

- 运行时 tokio（多线程）；插件代码不得自行创建运行时或线程。
- 不在异步上下文中阻塞：CPU 密集（解析大 .torrent、哈希）用 `tokio::task::spawn_blocking` 或 rayon；Native 插件中不调用阻塞 I/O。
- 每个外部调用都有超时（内核已为插件调用设截止时间，Native 插件自己发起的 Redis/HTTP 也必须设）。
- 取消安全：`select!` 中的分支需是取消安全的；持有锁跨 `.await` 时用 `tokio::sync::Mutex` 并尽量缩短。
- dyn trait 中的异步方法用 `#[async_trait]`（plugin-api 统一采用，保证 dyn 兼容）。

### 2.4 日志与追踪

- 使用 `tracing`；内核与 Native 插件用 `tracing::info!` 等宏，WASM 插件用 SDK 的 `log::info!`（转发到 `pt:plugin/log`）。
- 结构化字段优先：`info!(user_id, torrent_id, "torrent approved")`，不拼接字符串。
- 级别：`error` = 需要人处理；`warn` = 异常但已自动处理；`info` = 业务关键节点（低频）；`debug`/`trace` = 排查。热路径不打 info。
- **禁止**记录 passkey、密码、token、邮箱、IP、会话 ID 原文（内核有脱敏层，但不要依赖它）。
- 关键函数加 `#[tracing::instrument(skip_all, fields(...))]`。

### 2.5 其他

- 依赖：新增依赖须说明理由并通过 `cargo deny`；优先使用 workspace 统一版本（`[workspace.dependencies]`）。
- 数值：金额、流量用 `i64`/`u64`，溢出用 `checked_*` / `saturating_*`，禁止隐式 `as` 截断（clippy `cast_possible_truncation` 等保持 warn 并处理）。
- 时间：全仓库统一用 `chrono` + `chrono-tz`（与 sqlx、croner 生态一致），不混用 `time` / `jiff`。
- 序列化：`serde`；JSON 字段 snake_case（`#[serde(rename_all = "snake_case")]`）；对外结构体加 `#[serde(deny_unknown_fields)]` 只用于输入，输出不加（保持向前兼容）。
- 文档注释：公开项必须有 `///`，中文；`plugin-api` 中的每个类型与方法必须有文档。

## 3. WASM 插件（Rust）

- 目标 `wasm32-wasip2`；用 `cargo component` 或 `wit-bindgen` + `wasm-tools component new` 构建，`xtask package` 打包签名。
- SDK `pt-plugin-sdk` 提供：宿主接口的便捷封装（`kv::get`、`sql::query!` 宏做参数绑定、`events::publish(&T)` 序列化）、钩子/事件/MCP 的类型化分发（从 `schemas/` 生成的 Rust 类型）、`#[pt_plugin::main]` 宏。
- 产物大小：release + `opt-level = "s"` + `lto = true` + `strip = true`；单个组件建议 < 5 MiB。
- 不依赖 `std::thread`、文件系统、网络、环境变量（运行时不提供）。
- 其他语言（TinyGo、componentize-js、componentize-py）可用，但官方插件一律用 Rust。

## 4. TypeScript / React

- TypeScript 5.x，`"strict": true`、`"noUncheckedIndexedAccess": true`、`"exactOptionalPropertyTypes": true`；禁止 `any`（ESLint `@typescript-eslint/no-explicit-any: error`），确需时用 `unknown` + 类型收窄。
- React 19，函数组件 + Hooks；数据请求统一 TanStack Query，API 调用只经 `@pt-studio/sdk`（生成代码）或插件 `PluginApi.fetch`。
- 构建 Vite；包管理 pnpm workspace；格式化 Prettier；lint ESLint（flat config）+ `eslint-plugin-react-hooks` + `eslint-plugin-jsx-a11y`。
- 状态：服务器状态用 TanStack Query；本地 UI 状态用 `useState`/`useReducer`；跨页面少量全局状态用 Zustand。不引入 Redux。
- 样式：主题变量（CSS 变量，来自 `web/theme`）；组件库 `@pt-studio/ui-kit`；插件不得写全局 CSS（用 CSS Modules 或带前缀的类名）。
- 文案：界面文案走 i18n（`t('bonus:page.title')`），站点术语走 `t('term:bonus.currency')`，**不硬编码**任何会因站点变化的叫法。
- 安全：禁止 `dangerouslySetInnerHTML`，渲染用户 HTML/BBCode/Markdown 时只能用 `ui-kit` 的 `<RichText>`（内部 DOMPurify + 白名单）；禁止 `eval`、`new Function`；外链加 `rel="noopener noreferrer"`。
- 可访问性：表单控件有 label；可交互元素可键盘操作。
- 插件前端：只从 `@pt-studio/*` 和 import map 中的共享依赖导入；bundle（gzip 后）建议 < 200 KiB。

## 5. 测试规范

| 层次 | 要求 |
|---|---|
| 单元 | 纯逻辑必须有单元测试（规则计算、公式、解析）；`proptest` 覆盖数值与边界 |
| 插件集成 | 用 `pt-testkit`：内存宿主（不需要 PG/Redis 的快速测试）或 testcontainers（真实 PG/Redis）；断言发布的事件、钩子决策、SQL 结果 |
| 契约 | 插件的事件载荷、钩子输入/输出、MCP 输入输出必须通过对应 JSON Schema 校验的测试；服务 WIT 改动跑兼容性检查 |
| 幂等 | 每个事件订阅至少一个“同一事件投递两次”的测试；每个 cron 至少一个“同一 scheduled_at 运行两次”的测试 |
| 前端 | Vitest + Testing Library；关键流程 Playwright |
| 性能 | 热点服务方法有 criterion 基准；回归超过 20% CI 告警 |

- 测试命名：`<被测行为>_<条件>_<预期>`，如 `transfer_with_insufficient_balance_returns_error`。
- 测试用例编号写在测试函数文档注释中：`/// TC-BONUS-003`，便于追溯到测试计划。
- 覆盖率目标：插件业务逻辑行覆盖率 ≥ 80%。

## 6. 提交与评审

- 提交信息：Conventional Commits（`feat(bonus): …`、`fix(kernel): …`），scope 为插件 ID 或 `kernel`、`web`、`docs`。
- 修改 `docs/platform/` 契约的 PR 须由平台组评审；修改插件对外接口（服务、事件、钩子、视图、REST）的 PR 须同步更新文档。
