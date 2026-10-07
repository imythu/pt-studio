# PT Studio 平台契约

> 版本 0.1 · 状态：草案 · 2026-10-04

本目录是内核与全部插件之间的**唯一契约**。插件文档（`docs/plugins/<id>/`）引用的服务名、事件名、钩子名、权限点、术语键、插槽、MCP 工具、Redis 前缀、错误码，都必须与这里的目录一致；需要新增时，在插件概要设计第 7 节“对平台契约的新增”中提出，评审后合入本目录。

已确定的相关决策：D1～D17（见 [../design/README.md](../design/README.md)），以及本轮新增的：

| # | 决策 |
|---|---|
| D18 | 数据库确定为 PostgreSQL（最低 16，推荐 17），不支持 MySQL |
| D19 | 内核、全部官方插件、前端使用 `MIT OR Apache-2.0` 双许可证；pt-tracker 仍为 AGPL-3.0-only（D16），只通过 Redis 通信，不得被链接 |

## 1. 目录

| 文档 | 内容 |
|---|---|
| [plugin-api.md](plugin-api.md) | **核心契约**：WIT 包 `pt:plugin@0.1.0` 全文（已用 wasm-tools 解析验证）、Native 插件 Rust trait、`plugin.toml` 字段规范、能力清单、插件目录结构、版本兼容策略 |
| **kernel/** | 内核的四份工程文档 |
| [kernel/01-需求规格说明书.md](kernel/01-需求规格说明书.md) | FR-KRN / NFR-KRN |
| [kernel/02-概要设计说明书.md](kernel/02-概要设计说明书.md) | 模块、内核 HTTP 路由、关键流程、设计决策 DD-KRN |
| [kernel/03-详细设计说明书.md](kernel/03-详细设计说明书.md) | 加载器、服务注册表、wasmtime 运行时、Hook 管线、存储网关（SET ROLE、语句检查、Redis 改写）、事件日志（outbox / Streams / inbox / 回放）、HTTP 网关、调度器、MCP 注册、site.toml、能力与审计、可观测性、多副本、kernel DDL |
| [kernel/04-测试计划.md](kernel/04-测试计划.md) | TC-KRN 用例与覆盖矩阵 |
| **catalog/** | 名字目录（并行编写插件文档时互相引用的依据） |
| [catalog/plugins.md](catalog/plugins.md) | 插件 ID、文档缩写、类型、提供的服务；由 ID 派生的 schema、角色、Redis 前缀、路由前缀等 |
| [catalog/services.md](catalog/services.md) | 全部 `ctx.*` 服务：提供方、基数、主要方法概要、作用域；只读视图 |
| [catalog/events.md](catalog/events.md) | 事件命名、信封格式、分区、全部领域事件 |
| [catalog/hooks.md](catalog/hooks.md) | 全部同步钩子：触发点、输入、可修改字段、超时、失败策略 |
| [catalog/permissions.md](catalog/permissions.md) | RBAC 权限点命名与清单、内置角色 |
| [catalog/terms.md](catalog/terms.md) | 术语键命名与清单（按 design/05 分类） |
| [catalog/slots.md](catalog/slots.md) | 前端插槽清单与插件前端入口格式 |
| [catalog/mcp-tools.md](catalog/mcp-tools.md) | MCP 工具命名与预留清单 |
| [catalog/redis-keys.md](catalog/redis-keys.md) | Redis 实例角色、全局前缀分配（含 pt-tracker 的 `acct:`、`u:<id>:live`、`ps:`、`trk:`） |
| [catalog/error-codes.md](catalog/error-codes.md) | 错误码规则、内核通用错误码、跨插件常用错误码 |
| **conventions/** | 规范 |
| [conventions/api.md](conventions/api.md) | REST：路径、版本、分页、过滤排序、错误格式、鉴权、限流头、幂等、OpenAPI |
| [conventions/database.md](conventions/database.md) | schema/角色、命名、主键结论、类型、审计字段、软删除、分区、迁移、只读视图 |
| [conventions/coding.md](conventions/coding.md) | 仓库布局、Rust、WASM 插件、TypeScript/React、测试 |
| [conventions/security.md](conventions/security.md) | 输入校验、XSS/CSRF/CSP、密码与 2FA、passkey 与敏感字段、能力最小化、审计、MCP 安全 |
| [conventions/licensing.md](conventions/licensing.md) | 许可证、pt-tracker 隔离边界、禁止复制 copyleft 代码、cargo-deny |

## 2. 阅读顺序

**插件文档作者（必读，按顺序）**：

1. [catalog/plugins.md](catalog/plugins.md) —— 找到自己的插件 ID、缩写和派生名字
2. [plugin-api.md](plugin-api.md) §1、§2.4（调用语义）、§4（清单）—— 插件长什么样
3. 与本插件相关的目录行：[services](catalog/services.md)、[events](catalog/events.md)、[hooks](catalog/hooks.md)、[permissions](catalog/permissions.md)、[terms](catalog/terms.md)、[slots](catalog/slots.md)、[mcp-tools](catalog/mcp-tools.md)、[redis-keys](catalog/redis-keys.md)、[error-codes](catalog/error-codes.md)
4. [conventions/database.md](conventions/database.md)、[conventions/api.md](conventions/api.md)、[conventions/security.md](conventions/security.md)
5. 需要时查 [kernel/03](kernel/03-详细设计说明书.md)：§5 钩子、§6 存储、§7 事件、§9 调度

**内核实现者**：kernel/01 → 02 → 03 → 04 → plugin-api.md → 全部 catalog 与 conventions。

## 3. 给插件文档作者的要点

1. **名字只用目录里已分配的**。缺少的名字写进本插件概要设计 §7，不要自行发明后直接使用；跨插件引用对方尚未写出的详细签名时，以 services.md 的概要签名为准。
2. **服务方法签名**：services.md 只给概要，详细设计 §2.1 用 WIT 写出正式签名（遵守 plugin-api §1.2：不用 resource、可能失败的返回 `result<_, plugin-error>`、写操作带 `idempotency-key`）。
3. **事件订阅必须幂等**：内核的 inbox 只保证本插件**数据库**副作用恰好一次；调用其他服务、写 Redis、出站 HTTP 要自带幂等键（推荐 `<event_id>:<子键>`）。每个订阅、每个 cron 写一个“重复执行”的测试用例。
4. **钩子处理者独立提交**：钩子处理者的写入不会因为管线后续被拒绝而回滚。需要“预留”的（如邀请码）写带 TTL 的预留，再在拥有者发布的事件中确认。钩子中不能出站 HTTP、不能调 LLM，慢检查改为“事件 + 审核状态”。
5. **不读别人的表**：跨插件只用服务、事件、钩子、已登记的只读视图。需要新视图向提供方提出。
6. **每次调用是一个新实例**：WASM 插件不能在内存中保存跨调用状态；cron 任务默认 `tx = "manual"`，分批 `sql.commit()`，不要长事务。
7. **敏感数据**：passkey（指 tracker passkey）、邮箱、IP 默认拿不到；确需时在清单 `sensitive` 中申请并说明理由，管理员在 site.toml 授权。敏感字段永不进事件、搜索、MCP、日志。
8. **站点叫法用术语键**，界面文案用 i18n；二者不要混用。
9. **ID 用 BIGINT**，时间 `timestamptz`，金额用最小单位整数（database.md §3、§4）。
10. **许可证**：官方插件 `MIT OR Apache-2.0`；调研其他 PT 项目时只借鉴思路，不得复制 AGPL/GPL 代码（licensing.md §3）。
11. **文档编号**用 catalog/plugins.md 的缩写：`FR-<缩写>-001`、`TC-<缩写>-001`、`DD-<缩写>-1`；错误码 `<plugin_id>.<snake_case>`。

## 4. 需要回写总体设计的问题

以下是编写平台契约时发现的、与 `docs/design/` 不一致或需要补充之处。本轮**未修改** docs/design，待确认后回写。

| # | 位置 | 问题 | 平台契约中的处理 / 建议 |
|---|---|---|---|
| Q1 | design/02 §6.1 | “每个事务 `SET ROLE plugin_<id>`，隔离由 GRANT 保证，不依赖解析 SQL”：运行期连接的登录角色是所有插件角色的成员，插件 SQL 中的 `RESET ROLE`、`SET ROLE`、`set_config('role', …)`、plpgsql 中的动态 SQL 都能切回高权限角色，**仅靠 GRANT 不够** | 保留 SET ROLE 方案，加三层加固：PG16 成员关系 `INHERIT FALSE, SET TRUE`；撤销 `set_config` 与 plpgsql 的 PUBLIC 权限；`pg_query` 语句检查（kernel/03 §6.1～6.3）。**备选**：每插件独立登录角色 + 独立小连接池（根本上不可逃逸，代价是连接数），建议 P1 内核骨架阶段用 TC-KRN-051/053 实测后定稿，并更新 design/09“SQL 执行器”待验证项 |
| Q2 | design/03 §5 | “查询当前生效的流量倍率（由促销插件写入 Redis）”：WASM 插件只能写 `p:promotion:` 私有前缀，accounting 读取它违反隔离 | 改为 promotion 调用 `ctx.accounting.set_rate_rule()` / `remove_rate_rule()`，倍率由 accounting 存储与缓存（符合 design/01 原则 1“核心提供倍率字段和结算逻辑”） |
| Q3 | design/01 §7、plugins/README | `terms`（Native，`ctx.terms`）在 design/01 中列出，但 plugins/README 的插件清单中没有；`llm-*`、`metadata-*` 未给出具体 ID | catalog/plugins.md 新增 G0 组 `terms`，并确定 `llm-claude`、`llm-openai-compat`（本地模型走 OpenAI 兼容接口，不单列 `llm-local`）、`metadata-tmdb/douban/imdb/musicbrainz/bangumi/igdb`。建议 plugins/README 同步，并给 terms 分配文档作者 |
| Q4 | design/06 §3、design/02 §3 | 前端清单路径 `/api/plugins/frontend-manifest`；插件路由 `/api/p/{plugin_id}/...` 未带版本 | 内核路由统一为 `/api/kernel/v1/…`，前端清单为 `/api/kernel/v1/frontend/manifest`；插件路由为 `/api/p/<id>/v<N>/…`（conventions/api.md §1、§2） |
| Q5 | design/02 §5 | 清单示例中 `events = ["traffic.settled", "torrent.seeding_snapshot"]`：① `torrent.seeding_snapshot` 不符合 `<domain>.<past_tense>`，且发布者不明；② 订阅放在 `[capabilities]` 中，无法表达订阅名、版本、顺序；③ `ledger = ["bonus"]` 是服务专用写法，不通用 | ① 改名 `traffic.seeding_snapshot_taken`，由 accounting 发布；② 订阅改为 `[[subscriptions]]`，发布改为 `publishes`；③ 改为通用的 `[capabilities.scopes] "ctx.ledger" = ["currency:bonus"]`（plugin-api §4） |
| Q6 | design/10 §5.4～5.6、design/03 | Redis 键写作 `acct:{user_id % N}`、`u:{id}:live`、`ps:{info_hash}`、design/02 §6.2 的 `p:{plugin_id}:`。在 Redis Cluster 中 `{…}` 是哈希标签语法，按字面实现会改变分槽行为 | 平台目录统一用 `<x>` 表示占位，真实键名为 `acct:17`、`u:10086:live`、`ps:<hex>`、`p:bonus:…`（catalog/redis-keys.md §1）。请确认 design/10 的原意并改用同一记法 |
| Q7 | design/02 §4 | WIT 示例的 `user-summary` 含 `uploaded`、`downloaded`；`host-storage` 把 kv/sql/redis 合在一个接口；`variant value` 等类型未定义 | 以 plugin-api.md 为准：流量属于 `ctx.accounting.user_totals()`，不在 `ctx.users` 中；宿主接口拆分为 `kv`、`sql`、`redis` 等独立接口（按能力链接）。建议 design/02 §4 改为引用 plugin-api.md |
| Q8 | design/01 原则 2、design/02 §3 | 钩子名写作 `before_register`、`before_torrent_upload`、`before_torrent_download` | 统一为 `<domain>.before_<verb>`：`user.before_register`、`torrent.before_upload`、`torrent.before_download`（catalog/hooks.md） |
| Q9 | design/01 §8、design/09 §3 | 数据库标注“待确认”，开源协议列为待决问题 | 已由 D18、D19 确定；请在 design/README 决策表中补 D18、D19，并关闭 design/09 的两个待决项 |
| Q10 | design/01 §9 | 仓库结构中 `crates/pt-tracker` 与其他 crate 并列，未说明其独立 workspace；缺少 `crates/server`、`plugin-api-macros`、`testkit`、`xtask`、`docs/platform`、`docs/process` | 见 conventions/coding.md §1：pt-tracker 为独立 workspace 并由 cargo-deny 禁止被依赖 |
| Q11 | design/02 §6.1 | “卸载时可以选择保留或删除数据”与 D4（运行时不能卸载）的关系不清 | 插件从目录移除后数据默认保留；离线命令 `pt-server plugin purge <id>` 删除 schema、角色、Redis 前缀与对象存储前缀（kernel/03 §17） |
| Q12 | plugins/README（identity 行）、design/02 | “passkey”一词同时可能指 tracker passkey 和 WebAuthn passkey | 平台契约中 passkey 专指 tracker passkey；WebAuthn 一律称“WebAuthn 凭据”（kernel/01 §1.3、security.md §4）。建议 plugins/README identity 行改为“2FA（TOTP/WebAuthn）、tracker passkey” |
| Q13 | design/04 §2、§4 | 做种数、下载数等频繁变化字段的数据来源未定（tracker 内存中的 swarm 不对内核开放） | 由 accounting 根据流量事件维护各种子的活跃 peer 统计，每 3～5 分钟发布 `traffic.swarm_stats_updated`（只含有变化的种子），search 订阅后部分更新。需 accounting 与 tracker-bridge 设计确认精度（`emit_zero_delta=false` 时，无任何增量的 announce 不产生事件，需评估对 peer 存活判断的影响；必要时 pt-tracker 另写 `trk:stats:*` 汇总，前缀已预留） |
| Q14 | design/02 §3 | 扩展点表未列出“资源类型”与“元数据源”这类多实现扩展点，也未说明服务的单实现 / 多实现之分 | plugin-api 新增导出接口 `content-type`、`metadata-provider`；services.md 引入基数（single / multi / 聚合）概念 |
| Q15 | design/02 §1 | 启动流程中“校验签名”未定义签名格式与信任根 | kernel/03 §2.2：`checksums.txt` + minisign（Ed25519）签名，可信公钥在 site.toml 配置 |
| Q16 | design/07 §2 | 写操作“人工确认”的流程归属未定 | 确认流程由 mcp 插件实现（`ctx.mcp.confirm`），内核注册表只检查 `confirmed` 标志（kernel/03 §10） |
| Q17 | design/09 §2 | 新增待验证项 | 建议加入：SET ROLE 加固的逃逸测试（Q1）；动态 `Val` 服务桥接的开销；每次调用新实例在 JS/Python 组件上的实例化开销；Redis 配额抽样估算的误差 |

以上 Q1～Q17 已于 2026-10-04 回写至 docs/design/。
