# 03 · Tracker 与流量记账

## 1. Torrust 现状

- Torrust Tracker 支持私有模式（key 鉴权）和白名单模式。
- **它没有按用户统计上传/下载量。** 维护者 2024 年在 [discussion #819](https://github.com/torrust/torrust-tracker/discussions/819) 中明确说过 “The tracker does not track that info for the time being”，计划先做事件系统和运行时插件。
- 仓库现在已经拆成多个独立包，包括 `tracker-core`、`http-protocol`、`http-core`、`udp-core`、`udp-protocol`、`swarm-coordination-registry`、`events` 等。

## 2. 集成方向（已实测确认，决策 D9、D14～D17）

详细调研、方案对比、详细设计和原型结果见 [10-torrust-integration.md](10-torrust-integration.md)。结论如下：

**采用方案 B′“收窄组装”**：`pt-tracker` 只依赖 torrust 的 `swarm-coordination-registry`（内存 swarm）、`http-protocol`（announce 解析和 bencode 响应）、`primitives`、`events` 四个包，以 git 依赖锁定 rev。HTTP 入口和以下 PT 能力由 pt-tracker 自己实现：

- passkey 鉴权：`/announce/{passkey}`，32 位十六进制；本地缓存，数据由 tracker-bridge 通过 Redis“快照键 + 变更流”同步；
- 种子白名单、用户状态检查（封禁、禁止下载）；
- 客户端白名单（检查 peer_id 前缀和 User-Agent，默认拒绝）；
- 增量计算：依据 **pt-tracker 自己维护的 peer 会话状态**（键为 user_id + info_hash + peer_id），状态写穿到 Redis；
- 流量事件出口（`XADD acct:<user_id % N>` + `HINCRBY u:<user_id>:live`；`<x>` 表示占位，真实键名如 `acct:17`、`u:10086:live`，见 [../platform/catalog/redis-keys.md](../platform/catalog/redis-keys.md)）。

与原设想不同的关键实测结论：

- torrust 的 key 和白名单机制**无法换成我们的 passkey 和用户模型**（key 不关联用户，写入接口是 crate 私有的，私有模式必须有数据库），所以 tracker-core、http-core、axum-http-server 都不使用；
- torrust 的 events 是进程内 broadcast，会丢事件，并且**不带用户身份**，只能用于观测，**不能作为记账来源**；
- torrust 的 swarm 以 IP:端口为键，不返回旧值，**拿不到“上一次累计值”**；
- 拆分出来的包**都没有发布到 crates.io**，develop 分支改动频繁；所有包都是 **AGPL-3.0-only**，pt-tracker 需要单独使用 AGPL 许可证（D16）；
- 只支持 HTTP(S)，不提供 UDP（D15）：torrust 在私有模式下禁用 UDP，也没有实现 BEP 41。

| 方案 | 做法 | 结论 |
|---|---|---|
| A | fork torrust | 上游改动太频繁，rebase 成本高，不采用 |
| B | 整体依赖 tracker-core + http-core 组装 | 鉴权和白名单无法替换，需要绕过大半，不采用 |
| **B′（采用）** | 只依赖 swarm、协议、基础类型、事件 4 个包，torrust 藏在适配层后面 | 原型已跑通 |
| C | 在 tracker 前加反向代理截取 announce | 多一跳、重复解析，不采用 |
| D（备选） | 自研 swarm，只复用 Apache 许可的基础包 | 当许可证要求宽松，或 torrust API 变化导致适配成本过高时切换 |

## 3. 负载估算

| 项 | 估算 |
|---|---|
| 用户数 | 30 万 |
| 人均做种数 | 100 |
| peer 总数 | 约 3000 万 |
| announce 间隔 | 30 分钟 |
| 平均 announce 请求 | 约 1.7 万次/秒 |
| 高峰 | 平均值的 2～3 倍 |

## 4. 水平扩展

| 组件 | 扩展方式 |
|---|---|
| pt-tracker | 无状态（swarm 是可以重建的软状态）。负载均衡按 **URL 解码后的** `info_hash` 做一致性哈希，保证同一个种子的 swarm 落在同一个节点。peer 会话的最近一次累计值写一份到 Redis（`ps:<info_hash_hex>`），节点重启或分片迁移后懒加载恢复，并用 CAS 防止增量算错，详见 [10-torrust-integration.md](10-torrust-integration.md) §5.6、§5.7 |
| Kernel API | 无状态，可以随意增加副本 |
| 记账 worker | 按 user_id 把流分成 N 个分区，每个 worker 认领一部分分区 |
| PostgreSQL | 主从部署；大表（snatch、流量账本）按 user_id 哈希分区或按时间分区 |

## 5. 记账链路（决策 D2）

```
客户端 announce
 → pt-tracker：校验 passkey / 种子 / 客户端，返回 peers（不访问数据库）
 → 用 peer 的上一次累计值计算增量 Δup / Δdown / Δseed_time
 → XADD acct:<user_id % N>            （Redis Streams）
 → HINCRBY u:<user_id>:live ...       （Redis 实时计数，前端秒级可见）
 → 记账 worker 按分区消费，内存中按时间窗口（如 60 秒）聚合
     · 查询当前生效的流量倍率（倍率规则由促销插件通过 ctx.accounting.set_rate_rule() 下发，
       accounting 自己存储与缓存）
     · 计算反作弊基础信号
 → COPY 写入暂存表 → INSERT … ON CONFLICT 合并到 snatch 表和流量账本
     · 同一个事务内记录已处理到的 stream ID（崩溃重放不会重复计数）
 → 发出 traffic.settled 事件（批量汇总）
 → 魔力值 / H&R / 等级等插件订阅处理
另：accounting 每小时发布 traffic.seeding_snapshot_taken（做种快照，供魔力值等计算），
    每 3～5 分钟发布 traffic.swarm_stats_updated（只含有变化的种子，供搜索部分更新做种数）
```

要点：

- **tracker 只做计算增量这一件事**，不包含任何业务规则（免费、2x 等倍率在 worker 中应用）。
- **倍率归属**：倍率规则由 accounting 存储与缓存，促销插件只调用 `ctx.accounting.set_rate_rule()` / `remove_rate_rule()`（需要 `write:rates` 作用域）。WASM 插件只能写自己的 `p:<plugin_id>:` 私有前缀，accounting 不读取其他插件的 Redis 键。
- **做种数统计**：tracker 内存中的 swarm 不对内核开放；各种子的活跃 peer 统计由 accounting 根据流量事件维护。`emit_zero_delta = false` 时，没有任何增量的 announce 不产生事件，对 peer 存活判断的影响需在 accounting 与 tracker-bridge 设计中评估；必要时由 pt-tracker 另写 `trk:stats:*` 汇总（前缀已预留）。
- **幂等性**：stream ID 和落库在同一个事务内提交，实现“恰好一次”的效果。
- **实时性**：用户界面读 Redis 实时计数，数据库中的数据是按窗口最终一致的。

## 6. 反作弊

BT 协议只能相信客户端自己上报的数据，RatioMaster、GreedyTorrent 等工具会虚报上传量。

accounting 插件只输出**基础信号**，判定规则交给风控插件：

- 同一种子内上传总量与下载总量对不上（交叉校验）；
- 超过合理速度上限；
- 上报的 peer 不在 swarm 中；
- 增量为负、累计值异常跳变；
- 客户端不在白名单中。
