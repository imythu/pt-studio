# 02 · 插件系统

## 1. 加载方式（决策 D4）

插件只在启动前放进目录，启动时加载，后台只能查看已安装的插件。

好处：

- 不需要处理运行时的依赖变化；
- 多副本部署时，所有节点的插件集合天然一致；
- 启动时就能做完整校验：WIT 版本兼容、依赖齐全、能力已授权、数据库迁移成功。任何一项失败都拒绝启动，并给出明确的错误信息。

启动流程：

```
扫描 plugins/ → 解析 plugin.toml → 校验签名与 API 版本 → 根据 provides/requires 拓扑排序
→ 执行各插件 migrations → 实例化 → 注册服务 / 钩子 / 路由 / 订阅 / 定时任务 / MCP 工具 → 就绪
```

**签名**（仅 WASM 插件；Native 插件随二进制构建，视为可信）：插件包内的 `checksums.txt` 列出全部文件的 SHA-256，`plugin.sig` 是对它的 minisign（Ed25519）签名；可信公钥在 `site.toml [plugins.trust] keys` 中配置。未签名插件只在开发模式（`allow_unsigned`）下接受。详见 [../platform/kernel/03-详细设计说明书.md](../platform/kernel/03-详细设计说明书.md) §2.2。

## 2. 运行时

- **wasmtime + Component Model（WIT 接口）**，插件可以用 Rust、Go（TinyGo）、JS（componentize-js）、Python 编写。
- 用 fuel 或 epoch interruption 限制 CPU，限制内存，每次调用设超时。
- 用 pooling allocator 做实例池，降低实例化开销。

## 3. 扩展点

| 扩展点 | 同步/异步 | 例子 |
|---|---|---|
| Hook（拦截器） | 同步，可拒绝或修改 | `user.before_register`（校验邀请码）、`torrent.before_upload`（校验标题格式）、`torrent.before_download`（分享率不足时禁止下载）。命名统一为 `<domain>.before_<verb>`，清单见 [../platform/catalog/hooks.md](../platform/catalog/hooks.md) |
| 事件订阅 | 异步，至少投递一次（插件需幂等） | `traffic.settled` → 加魔力值；`torrent.created` → 抓取豆瓣信息 |
| 定时任务 | 异步 | 每小时结算做种积分、每天做 H&R 考核 |
| HTTP 路由 | 同步 | 挂在 `/api/p/<plugin_id>/v<N>/…` 下（主版本在路径中，每个插件独立演进）；内核自身路由为 `/api/kernel/v<N>/…`，见 [../platform/conventions/api.md](../platform/conventions/api.md) §1、§2 |
| 服务导出 | 同步 | 魔力值插件提供服务，商城插件依赖它。服务分基数：`single`（同一时刻只启用一个实现，由 `site.toml [services]` 选定）与 `multi`（所有启用的提供方同时生效，由消费方枚举或经内核聚合），见 [../platform/catalog/services.md](../platform/catalog/services.md) |
| 资源类型 | 同步 | 导出接口 `content-type`，`ctx.content_types` 的多实现之一（`type-movie`、`type-music` 等），见 [05-content-types-and-terms.md](05-content-types-and-terms.md) |
| 元数据源 | 同步 | 导出接口 `metadata-provider`，`ctx.metadata` 的多实现之一（`metadata-tmdb`、`metadata-douban` 等） |
| 搜索贡献 | 同步（由索引 worker 调用） | 资源类型插件提供索引 schema 和文档，见 [04-search.md](04-search.md) |
| MCP 工具 | 同步 | 在清单中声明，自动暴露，见 [07-ai-and-mcp.md](07-ai-and-mcp.md) |
| 前端插槽 | — | 见 [06-frontend.md](06-frontend.md) |

## 4. WIT 契约

WIT 包 `pt:plugin@0.1.0` 的完整定义、Native 插件的 Rust trait 以及调用语义，**以 [../platform/plugin-api.md](../platform/plugin-api.md) 为准**，本文不再给出示例。要点：

- 宿主接口按能力拆分为独立接口（`log`、`config`、`kv`、`sql`、`redis`、`events`、`http-client`、`blob`、`terms`、`audit`、`jobs` 等），插件按清单中的能力链接；未授权的导入链接到“拒绝桩”。
- 插件导出接口：`lifecycle`、`hooks`、`on-event`、`on-cron`、`http-handler`、`search-contributor`、`mcp-tool`、`content-type`、`metadata-provider`。
- 业务服务每个一个 WIT 包（如 `pt:ledger@0.1.0`），由提供方插件导出、消费方导入，内核负责连线。
- 用户流量（上传 / 下载量）属于 `ctx.accounting.user_totals()`，不在 `ctx.users` 的用户摘要中。
- 每次调用（钩子、事件、HTTP 请求、服务调用）使用一个新的组件实例，插件不能在内存中跨调用保存状态。

WIT 包按 semver 管理版本。在 v1.0 之前允许不兼容变更，v1.0 之后只允许增量变更。

## 5. 清单与权限

以下为节选，完整示例与字段表见 [../platform/plugin-api.md](../platform/plugin-api.md) §4。

```toml
# plugins/bonus/plugin.toml
id = "bonus"
name = "魔力值"
version = "1.2.0"
kind = "wasm"
api = "pt:plugin@0.1"
license = "MIT OR Apache-2.0"
provides = ["ctx.bonus@0.1.0"]
requires = ["ctx.ledger@^0.1", "ctx.users@^0.1", "ctx.accounting@^0.1"]
publishes = ["bonus.hourly_settled"]       # 顶层键须写在第一个 [表] 之前

[capabilities]
http_outbound = []          # 不允许访问外网
sql = true                  # 允许使用高级 SQL 执行器（只能访问自己的 schema 和授权视图）
redis = true                # 只能使用私有前缀 p:bonus:
sensitive = []              # 读不到 tracker passkey、邮箱、IP

[capabilities.scopes]       # 在其他服务上申请的作用域（由服务提供方定义含义）
"ctx.ledger" = ["currency:bonus"]   # 只能操作 bonus 这一种货币
"ctx.accounting" = ["read"]

[[subscriptions]]
name = "on_seeding_snapshot"
event = "traffic.seeding_snapshot_taken"   # 由 accounting 发布
versions = [1]
ordered = true

[[cron]]
name = "hourly_seed_bonus"
schedule = "5 * * * *"
tx = "manual"

[terms]
"bonus.currency" = { zh-CN = "魔力值", en = "Bonus Points" }

[frontend]
entry = "ui/index.js"
slots = ["user.profile.stats", "nav.user_menu"]

[[mcp.tools]]
name = "get_my_bonus"
description = "查询当前用户的魔力值余额和最近收支"
input_schema = "schemas/mcp/get_my_bonus.input.json"
output_schema = "schemas/mcp/get_my_bonus.output.json"
```

- 事件订阅用 `[[subscriptions]]`（可表达订阅名、载荷版本、是否按 key 顺序处理），发布的事件列在 `publishes`；事件名为 `<domain>.<past_tense>`，见 [../platform/catalog/events.md](../platform/catalog/events.md)。
- 对其他服务的细粒度授权统一写在 `[capabilities.scopes]` 中，不为某个服务设计专用字段。

**敏感数据（tracker passkey、邮箱、IP、WebAuthn 凭据、TOTP 密钥）默认不对插件开放**，插件在清单 `sensitive` 中申请并说明理由，由管理员在 `site.toml` 中显式授权。本项目文档中 “passkey” 专指 tracker passkey；WebAuthn 一律称“WebAuthn 凭据”。

## 6. 数据访问（决策 D3）

| 层级 | 做法 | 安全措施 |
|---|---|---|
| 基础方法 | `ctx.users.get()`、`ctx.ledger.transfer()` 等类型化服务 | 按服务授权 |
| 插件 KV | `kv_get/set` | 自动加前缀 |
| 高级 SQL 执行器 | 插件可以对**自己的 schema** 执行任意 SQL；核心数据通过**只读视图**访问 | 见下 |
| Redis | 直接执行命令 | 见下 |

### 6.1 SQL 执行器的隔离

- **每个插件对应一个独立的 PostgreSQL schema 和一个独立的数据库角色。**
- 每个事务开始时执行 `SET LOCAL ROLE pt_plugin_<schema>`，主隔离由数据库的角色与 GRANT 保证。
- **仅靠 GRANT 不够**：运行期连接的登录角色是所有插件角色的成员，插件 SQL 中的 `RESET ROLE`、`SET ROLE`、`set_config('role', …)`、plpgsql 动态 SQL 都可能切回高权限角色。因此加三层加固：
  1. PG16 成员关系 `INHERIT FALSE, SET TRUE`（登录角色 `pt_gateway` 为 `NOINHERIT`）；
  2. 撤销 `set_config` 与 `LANGUAGE plpgsql` 的 PUBLIC 权限（WASM 插件只能写 `LANGUAGE sql` 函数）；
  3. 用 `pg_query` 做语句检查（纵深防御）：拒绝多语句、`SET`/`RESET`、事务控制、`COPY`、`DO`、DDL 等及危险函数。
- **备选方案**：每个插件独立登录角色 + 独立小连接池，从根本上不可逃逸，代价是连接数。P1 内核骨架阶段用逃逸测试（TC-KRN-051/053）实测后定稿，见 [09-roadmap.md](09-roadmap.md)。详见 [../platform/kernel/03-详细设计说明书.md](../platform/kernel/03-详细设计说明书.md) §6.1～6.3。
- 核心数据以只读视图的形式暴露，按插件声明的能力授予 SELECT 权限。
- 强制参数化查询（不提供字符串拼接接口）。
- 设置 `statement_timeout` 和返回行数上限。
- 插件在清单中声明 migrations，由宿主执行。
- 运行时不能卸载插件（D4）。插件从目录移除后，其数据**默认保留**；需要删除时停机执行离线命令 `pt-server plugin purge <id>`，删除该插件的 schema、角色、Redis 前缀与对象存储前缀（插件仍在目录中时拒绝执行，见 kernel/03 §17）。

### 6.2 Redis 的隔离

- 键名强制改写为 `p:<plugin_id>:…`（`<x>` 表示占位，真实键名如 `p:bonus:…`；`{}` 在 Redis Cluster 中是哈希标签语法，不用于表示占位）。全局前缀只能由 Native 插件经 `redis_global` 能力访问已分配给它的前缀，见 [../platform/catalog/redis-keys.md](../platform/catalog/redis-keys.md)。
- 命令白名单：禁止 `KEYS`、`FLUSH*`、`CONFIG`、`EVAL` 等。
- 每个插件有内存配额。

### 6.3 跨插件访问

跨插件读写数据**一律通过服务调用**，不允许直接访问其他插件的表。这样插件升级不会连带破坏别人。

## 7. 账本（决策 D13）

账本本身也是插件：`ledger` 是 Native 插件，提供 `ctx.ledger` 服务。

- 支持多币种：魔力值插件、做种积分插件各自注册自己的币种。
- 只追加的流水表，余额可以由流水重算，便于对账。
- 商城等插件只依赖 `ctx.ledger`，不关心币种是谁注册的。
