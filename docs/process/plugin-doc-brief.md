# 插件设计文档编写任务说明（所有插件文档 agent 共用）

## 1. 必读材料（按顺序）

1. `docs/process/README.md` 与 `docs/process/templates/`：工程流程、编号规则、四份模板。
2. `docs/platform/README.md` 及其指向的全部平台契约（`plugin-api.md`、`kernel/`、`catalog/`、`conventions/`）。
   **平台契约是唯一权威来源**：当 `docs/design/` 与 `docs/platform/` 不一致时，以 `docs/platform/` 为准（平台 README §4 的 Q1～Q17 处理方案均已被采纳）。
3. `docs/design/` 全部文档（总体设计、决策 D1～D19），特别是 `10-torrust-integration.md`。
4. `docs/platform/catalog/plugins.md`：插件 ID、缩写、提供的服务。

## 2. 产出

对分配给你的每个插件，在 `docs/plugins/<plugin-id>/` 下按模板写四份文档：

- `01-需求规格说明书.md`
- `02-概要设计说明书.md`
- `03-详细设计说明书.md`
- `04-测试计划.md`

## 3. 质量要求

- **参考现有开源 PT 程序的优秀之处**：在需求规格 §2.3 中，对 NexusPHP、UNIT3D、Gazelle（含 OPS/Orpheus、RED 的公开做法）、Luminance、Arcadia、TorrentPier 等在本插件领域的做法做调研（可上网查阅文档、源码、wiki；也可以把仓库只读克隆到 `/workspace/pt-studio/.spike/refs/` 阅读），写清楚：它们的做法、优点、缺点、我们采纳什么、改进什么。**只借鉴思路，不得复制任何代码**（这些项目多为 GPL/AGPL，我们的内核与插件是 MIT OR Apache-2.0）。
- 需求可验证、有编号；设计可追溯到需求；测试用例覆盖每条需求（含异常、并发/幂等、安全、性能用例；每个事件订阅和每个 cron 至少一个“重复执行”用例）。
- 详细设计要能直接照着编码：WIT/trait 签名、事件载荷字段、完整 DDL、Redis 键、HTTP API（含请求/响应示例）、MCP 工具、配置项、错误码、前端页面与插槽。
- 站点可变的叫法一律使用术语键；敏感数据（tracker passkey、邮箱、IP）遵守 `conventions/security.md`。
- 记住平台关键约束：WASM 插件每次调用新实例、无跨调用内存状态；每次调用一个数据库事务；钩子中不能出站 HTTP / 调用 LLM；钩子写入独立提交；跨插件只能通过服务、事件、钩子、已登记只读视图；跨服务调用、写 Redis、出站 HTTP 要自带幂等键（推荐 `<event_id>:<子键>`）；cron 分批提交，不写长事务。

## 4. 边界

- **只能写入自己负责的 `docs/plugins/<plugin-id>/` 目录**（以及任务中额外指定的目录）。不要修改 `docs/platform/`、`docs/design/`、`docs/process/` 和其他插件的目录。
- 需要平台目录中没有的服务、事件、钩子、权限点、术语、插槽、MCP 工具、Redis 前缀、错误码时，**不要自造进目录**，而是写在本插件概要设计 §7“对平台契约的新增”中，评审时统一合入。
- 引用其他插件的服务时，以 `catalog/services.md` 的概要签名为准；如果需要的方法不存在，同样写进 §7。
- 不要 git commit。

## 5. 完成汇报（中文，简洁）

- 产出文件清单；
- 从开源 PT 程序借鉴的主要做法；
- 对平台契约的新增需求（汇总 §7）；
- 与其他插件的依赖和需要对方配合的地方；
- 你认为有风险或需要用户决定的问题。
