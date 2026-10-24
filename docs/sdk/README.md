# PT Studio 插件开发指南

> 适用于插件 API `pt:plugin@1`（1.0.0，2026-10-14 冻结）。契约原文见 [../platform/plugin-api.md](../platform/plugin-api.md)。

PT Studio 的站点功能都是插件：内核只负责加载、权限、调用、事务、事件与网关。本文说明如何写一个第三方插件：从模板开始、
清单怎么写、SDK 提供什么、如何打包与测试，以及 1.0 之后的兼容承诺。

## 1. 两种插件

| | WASM 插件 | Native 插件 |
|---|---|---|
| 形式 | Component Model 组件（`wasm32-wasip2`），放在站点的 `plugins.dir` 中 | Rust crate，编译进 `pt-server` 二进制 |
| 隔离 | 沙箱：只能调用清单授权的宿主接口；内存、截止时间、并发受限 | 与内核同进程，靠 trait 与 SQL 角色约束 |
| 稳定性 | 依赖冻结的 WIT `pt:plugin@1`，不随内核重新编译 | 依赖 `pt-plugin-api` 的 Rust API，随内核版本演进 |
| 适合 | 绝大多数业务插件（签到、求种、通知、审核……） | 高吞吐或需要 Rust 生态的部分（身份、记账、搜索、MCP 网关） |
| 模板 | [templates/wasm-plugin](../../templates/wasm-plugin) | [templates/native-plugin](../../templates/native-plugin) |

**第三方插件请优先写 WASM 插件。**

## 2. 快速开始（WASM）

```sh
rustup target add wasm32-wasip2
cp -r templates/wasm-plugin ../my-plugin && cd ../my-plugin
# 1. 改 plugin.toml 的 id（如 my-notes），把权限、术语、错误码、事件、钩子点的前缀 `notes.` 换成 `my-notes.`
# 2. Cargo.toml：包名改为 pt-wasm-my-notes；仓库外开发时把 pt-plugin-sdk 改成 git 依赖
# 3. wit/world.wit：世界名与 src/lib.rs 中 world!("…") 一致；只 export 实际实现的接口
cargo build --release --target wasm32-wasip2
pt-server plugin package --src . --wasm target/wasm32-wasip2/release/pt_wasm_my_notes.wasm --out dist
```

站点配置（开发环境）：

```toml
[plugins]
dir = "/path/to/my-plugin/dist"   # 可放多个插件：dist/<id>/
allow_unsigned = true             # 包签名尚未实现，生产模式会拒绝未签名的包
```

`pt-server serve` 启动时执行插件迁移、登记权限与术语。路由挂在 `/api/p/<id>/` 下，如模板的 `GET /api/p/notes/v1/notes`。

只检查不启动：`pt-server plugin check dist/<id>`。它检查清单格式、API 版本，组件能否链接到宿主，导出接口与清单声明是否一致，
以及迁移文件名；不需要数据库，适合放进插件仓库的 CI。

## 3. 目录结构

```
my-plugin/
├─ Cargo.toml
├─ plugin.toml                 # 清单
├─ wit/
│  ├─ world.wit                # 插件世界：include pt:plugin/host@1.0.0 + export …
│  └─ deps/pt-plugin/plugin.wit  # pt:plugin@1.0.0 的副本（来自 crates/plugin-api/wit/plugin.wit）
├─ migrations/0001_init.sql    # 在插件自己的 schema 中执行
├─ schemas/hooks/*.json        # 本插件钩子点的输入 schema
└─ src/lib.rs
```

## 4. 清单要点

完整字段表见 [plugin-api.md §4](../platform/plugin-api.md)。常用的几项：

- **命名空间**：权限点、术语键、错误码、事件、目录之外的钩子点都以 `<id>.` 开头。
- **`api = "pt:plugin@1"`**：用到 1.x 后续次版本新增的接口时写 `@1.<次>`，旧内核会拒绝加载，而不是运行时报错。
- **导出与声明一致**：世界 export 了 `http-handler` 就必须有 `[[http.routes]]`，反之亦然。hooks、on-event、on-cron
  （`[[cron]]` / `[[jobs]]`）、service-provider（`provides`）、mcp-tool、search-contributor、content-type 也一样。
  不一致时加载报 `E-LOAD-WORLD`。
- **能力最小化**：`[capabilities]` 中 `sql`、`redis`、`blob`、`http_outbound`（主机白名单）都要显式开启。
  读其他插件的数据只能经它开放的只读视图（`views = ["identity.v_user_public"]`）；
  敏感数据（邮箱、IP）用 `sensitive` / `sensitive_optional` 申请，并需站点在 site.toml `[grants.<id>]` 授权。
- **调用其他插件的服务**：`requires` / `optional_requires` 声明服务，`[capabilities.scopes]` 申请作用域。
  方法与参数见 [catalog/services.md](../platform/catalog/services.md)。
- **路由**：`auth = "public" | "user" | "admin"`，可加 `permission`、`rate_limit`（如 `user:30/m`、`ip:5/h`）、`max_body_kb`、
  `timeout_ms`。网关在调用插件前已完成鉴权、CSRF、权限、限流与幂等键。
- **设置**：`[settings_schema]`（JSON Schema）生成后台设置表单；`ctx::settings()` 读取。schema 中的 `default` 不会自动填入，
  读取时自己给默认值。site.toml 中的静态配置用 `ctx::static_config()` 读取，凭据放这里。

## 5. SDK（`pt-plugin-sdk`）

源码在 [crates/plugin-sdk-rust](../../crates/plugin-sdk-rust)。SDK 生成一次 `pt:plugin@1.0.0` 的宿主绑定；插件用 `world!`
宏生成自己的导出绑定，并复用这些导入。

```rust
pt_plugin_sdk::world!("notes", http);           // 世界导出 http-handler 时加 http

use exports::pt::plugin::{http_handler, mcp_tool};
use pt_plugin_sdk::{prelude::*, sql};

struct Notes;

impl http_handler::Guest for Notes {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let uid = ctx::current_user()?;                       // 匿名 → kernel.unauthenticated
        let reply = match req.route() {                       // 清单中的路由 id
            "list" => http::Reply::ok(&list(uid)?),
            "create" => http::Reply::created(&create(uid, req.body_json()?)?),
            "delete" => { delete(uid, req.param_i64("id")?)?; http::Reply::no_content() }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(Notes);
```

| 需求 | 写法 |
|---|---|
| 错误 | `err("notes.too_many", "…")`，可加 `.with_details(&json!({…}))`、`.retryable()` |
| SQL | `sql::query("… WHERE id = $1 AND title = $2", &[id.into(), title.into()])?`。参数可由 `i64`、`bool`、`&str`、`String`、`Option<T>`（NULL）、`&Value`（jsonb）、`Vec<i64>` / `Vec<String>`（数组）转换；结果行用 `row.int(0)`、`row.text(1)`、`row.json(2)`。`sql::execute` 返回受影响行数 |
| 当前用户 / 调用方 | `ctx::current_user()`、`ctx::current_user_opt()`、`ctx::caller()`、`ctx::has_scope("send")`、`ctx::require_scope(…)`；完整上下文 `ctx::current()`（含 `request_id`、`locale`、`deadline`） |
| 设置 | `ctx::settings()`、`ctx::static_config()` |
| 业务服务 | `ctx::call("ctx.ledger", "transfer", &json!({…}))?` |
| 事件 | `ctx::publish("notes.created", "user:1", &json!({…}))?`（事件类型须在 `publishes` 中） |
| 钩子点 | `let input = ctx::run_hook("notes.before_create", &json!({…}))?;` 放行时返回合并补丁后的输入；被拒绝时返回处理者的错误 |
| 延迟任务 | `ctx::schedule("deliver", run_at_ms, &json!({…}), Some("dedupe"))?`，到期经 `on-cron.run` 回调（`job.kind = delayed`）；失败由内核重试 3 次 |
| 日志 | `logging::info("msg", &[("k", "v")])` |
| HTTP 请求 | `req.query("limit")`、`req.param("id")` / `param_i64`、`req.header("user-agent")`、`req.body_json::<T>()` / `body_value()` |
| HTTP 应答 | `http::Reply::ok / created / json(status, &v) / no_content / bytes(status, ct, body)`，可链式 `.header(k, v)` |
| 上游限速与熔断 | `const API: Upstream = Upstream::new("api", "my-plugin.upstream_error").per_sec(10).breaker(5, 600);` 请求前 `API.acquire()?`（熔断打开或限速等待 2 次仍超限时返回可重试错误），请求后 `API.record(ok)`；`upstream::retry_after(header, cap)` 解析 `Retry-After`。状态在插件 Redis（需 `redis = true`），不随事务回滚 |
| 其他宿主接口 | 原样导出：`pt_plugin_sdk::{kv, redis, blob, http_client, time, random, audit, metrics, terms, metadata}` |

不在 SDK 中的事：线程、文件系统、环境变量、直接网络访问（运行时不提供）。出站 HTTP 只能经 `http_client::send`，
且主机须在 `http_outbound` 白名单中。

## 6. 扩展点

| 想做的事 | 世界中 export | 清单中声明 |
|---|---|---|
| 提供 REST 接口 | `http-handler` | `[[http.routes]]` |
| 在其他插件的流程中放行 / 拒绝 / 修改 | `hooks` | `[[hooks]] hook = "torrent.before_upload"`（可用钩子见 [catalog/hooks.md](../platform/catalog/hooks.md)） |
| 定义自己的钩子点 | —（用 `ctx::run_hook`） | `[[hook_points]] name = "<id>.before_x"`，可选 `modifiable`、`timeout_ms`、`deny_on_error` |
| 订阅事件 | `on-event` | `[[subscriptions]] name, event`（事件见 [catalog/events.md](../platform/catalog/events.md)）；至少投递一次，处理要幂等 |
| 定时 / 延迟任务 | `on-cron` | `[[cron]] name, schedule`（5 段 cron）/ `[[jobs]] name` |
| 提供服务给其他插件 | `service-provider` | `provides = ["ctx.<name>@0.1.0"]`；方法按 JSON 分派 |
| AI 工具 | `mcp-tool` | `[[mcp.tools]]`（输入 schema；写工具默认需用户在站内确认） |
| 搜索字段 | `search-contributor` | `capabilities.search_contributor = true` |
| 资源类型 | `content-type` | `capabilities.content_type = true` |
| 前端 | — | `[frontend] entry, slots, routes`（见 §9） |

## 7. 事务、幂等与错误

- 每次入口调用（HTTP、钩子、事件、任务、服务调用）是一个工作单元：SQL、发布的事件、安排的任务一起提交；返回错误则全部回滚。
  被调服务有独立的工作单元。
- 钩子处理者不能回调钩子点所有者的服务（重入保护），需要的数据应放进钩子输入。
- 事件与延迟任务至少投递一次：用唯一约束、`ON CONFLICT`、幂等键保证重复处理无副作用。
- **错误码与 HTTP 状态**：[catalog/error-codes.md](../platform/catalog/error-codes.md) 登记的码优先；其次是清单 `[error_codes]`；
  再次按后缀规则：`_not_found` → 404，`_taken` / `_exists` / `_conflict` → 409，`_denied` / `_forbidden` / `_required` → 403，
  `_too_many` / `_rate_limited` → 429，其余 422。错误名恰好等于后缀词时同样适用，如 `notes.not_found` → 404。
  错误以 `application/problem+json` 返回（`code`、`message`、`details`、`request_id`）。

## 8. 测试

- **单元测试**：把规则、公式、解析写成纯函数，`cargo test` 在主机目标上运行（宿主接口在主机目标上不可调用）。
- **加载检查**：`pt-server plugin check`，CI 必跑。
- **端到端**：本仓库的 `crates/server/tests/e2e_template.rs` 演示了完整流程：构建模板、`pt_server::package`、内核加载，
  用 Native 测试插件处理第三方钩子，经 HTTP 与 MCP 调用。测试环境需要 PostgreSQL 与 Redis（见 [../dev/README.md](../dev/README.md)）。

## 9. 前端插件

插件前端是一个 ESM 模块，默认导出 `PluginFrontend`：

```tsx
import type { PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Card } from '@pt-studio/ui-kit';

registerMessages('zh-CN', { 'my-notes.ui.tab': '便签' });

function Tab({ api }: SlotProps) {
  // api.json 会附带会话、CSRF 与幂等键；路径限定为 /api/…
  return <Card title={api.t('my-notes.ui.tab')}>…</Card>;
}

export default {
  slots: { 'user.settings.tabs': { component: Tab, order: 60, title_term: 'my-notes.ui.tab' } },
  routes: [{ path: '/p/my-notes/*', component: Tab }],
} satisfies PluginFrontend;
```

- 插槽见 [catalog/slots.md](../platform/catalog/slots.md)。前端路由须以 `/p/<id>/` 或 `/admin/p/<id>/` 开头。
- 只从 `@pt-studio/*` 与 import map 中的共享依赖（React、TanStack Query 等）导入，不写全局 CSS，
  用户内容只经 ui-kit 的 `<RichText>` 渲染。
- 构建：本仓库中把源码放在 `web/plugins/<id>/src/index.tsx`，`npm run build`（web/）输出 `web/dist/plugins/<id>/index.js`。
  仓库外构建的 bundle 放到站点 `http.web_dir` 的 `plugins/<id>/index.js` 即可（内核按 `/static/plugins/<id>/<版本>/` 提供）。
  随插件包分发前端尚未实现。

## 10. 兼容承诺（1.0 起）

- `pt:plugin@1` 的接口、记录、函数签名不再改变；新增能力放进新接口，并提升次版本。冻结快照在
  `crates/plugin-api/wit-frozen/`，测试保证不被修改。
- 清单格式只增字段。
- 业务服务方法、事件载荷、钩子输入、只读视图按各自的版本规则演进：字段只增不删，改语义要升版本并过渡。
  详见 [plugin-api.md §6](../platform/plugin-api.md)。
- Native 插件的 Rust API 随内核演进，不承诺跨版本二进制兼容。

## 11. Native 插件

见 [templates/native-plugin](../../templates/native-plugin)：

1. 实现 `pt-plugin-api` 的 trait（`HttpRoutes`、`OnEvent`、`Hooks`、`OnCron`、`McpTool`、`JsonService` …）。
2. 写 `pub fn plugin() -> NativePlugin`。
3. 放进 `plugins/native/<id>/`，在 `crates/server/Cargo.toml` 加 feature 与依赖，在 `native_plugins()` 中 `push`。

SQL 经 `ctx.sql.conn()` 取得连接（sqlx），自动切换到插件角色。其余能力与 WASM 插件一一对应，见 plugin-api.md §3。

## 12. 目前的限制

- 包签名（`plugin.sig`）未实现：生产模式拒绝未签名包，开发环境用 `allow_unsigned`。
- 钩子输入 schema 随包分发，但内核尚未在运行时按 schema 校验钩子输入。
- 插件前端需要另外放进站点的 web 目录（见 §9）。
- 只提供 Rust SDK；其他语言（componentize-js / -py、TinyGo）可按 WIT 自行生成绑定。
