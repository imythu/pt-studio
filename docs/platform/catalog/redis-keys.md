# Redis 键目录

> 版本 0.1 · 2026-10-04

## 1. 记法与规则

- 本目录中 `<x>` 表示占位符，**`{}` 只表示 Redis Cluster 哈希标签本身**（会出现在真实键名中）。design/10 中的 `acct:{user_id % N}`、`u:{id}:live`、`ps:{info_hash}` 是占位写法，真实键名为 `acct:17`、`u:10086:live`、`ps:<40位hex>`（见平台 README 问题 Q6）。
- 段分隔符为 `:`；段内用小写字母、数字、`_`、`-`、`.`。
- 一级前缀全站唯一，由本目录分配；**插件只能使用 `p:<plugin_id>:` 私有前缀**（由内核自动改写，插件代码里写的是去掉前缀后的键）。访问全局前缀需要 Native 插件 + `redis_global` 能力，且只能访问下表分配给它的前缀。
- 每个键应有 TTL，或在设计文档中说明为什么不需要以及如何清理。

## 2. Redis 实例角色

`site.toml [redis]` 定义实例角色，多个角色可以指向同一个实例；生产环境建议按下表拆分（design/10 §5.6）。

| 角色 | 用途 | 建议 |
|---|---|---|
| `main` | 内核与插件：事件流、锁、限流、缓存、插件私有键 | 必需 |
| `acct` | pt-tracker → accounting 的流量流与实时计数 | 高写入，建议独立 |
| `tracker_state` | pt-tracker 的 peer 会话状态（约 4 GB） | 建议独立 |
| `tracker_sync` | tracker-bridge → pt-tracker 的快照键与变更流 | 可与 main 共用 |

## 3. 已分配前缀

| 前缀 / 键 | 实例角色 | 类型 | 写入方 | 读取方 | TTL / 上限 | 说明 |
|---|---|---|---|---|---|---|
| `acct:<partition>` | acct | STREAM | pt-tracker | accounting（消费组 `accounting`） | `MAXLEN ~` 配置 | 流量增量，`partition = user_id % N`，N 全站固定（默认 64），字段见 design/10 §5.4 |
| `u:<user_id>:live` | acct | HASH | pt-tracker（HINCRBY）、accounting（结算后扣减/重置） | accounting | 7 天无更新过期 | 实时原始计数 `up`、`down`、`seed_time` |
| `ps:<info_hash_hex>` | tracker_state | HASH | pt-tracker | pt-tracker；tracker-bridge 只读（peer 列表） | `max_peer_timeout × 2` | peer 会话状态，design/10 §5.6 |
| `trk:pk:<passkey>` | tracker_sync | HASH | tracker-bridge | pt-tracker | 无（随用户生命周期删除） | `uid`、`st`、`ver` |
| `trk:t:<info_hash_hex>` | tracker_sync | HASH | tracker-bridge | pt-tracker | 无 | `tid`、`st`、`acct`、`size`、`ver` |
| `trk:clients` | tracker_sync | STRING(JSON) | tracker-bridge | pt-tracker | 无 | 客户端白名单 |
| `trk:cfg` | tracker_sync | HASH | tracker-bridge | pt-tracker | 无 | 运行参数 |
| `trk:sync` | tracker_sync | STREAM | tracker-bridge | pt-tracker | `MAXLEN ~ 1000000` | 变更流 |
| `trk:u:<user_id>` | tracker_sync | STRING | tracker-bridge | tracker-bridge | 无 | 用户当前 passkey（反向索引：换 passkey 时删除旧的 `trk:pk:*`） |
| `trk:ver` | tracker_sync | STRING(计数器) | tracker-bridge | tracker-bridge | 无 | 快照与变更流的 `ver` 来源（INCR，单调递增） |
| `trk:stats:*` | tracker_sync | — | 预留 | — | — | 预留给 pt-tracker 汇总数据（design/10 §5.7 scrape 汇总） |
| `ev:<event_type>:<partition>` | main | STREAM | 内核 outbox 中继 | 内核事件消费者 | `MAXLEN ~` 按事件配置（默认 1,000,000） | 领域事件流 |
| `evr:<replay_id>` | main | STREAM | 内核回放器 | 指定订阅 | 回放结束后 24 h | 回放流 |
| `evdlq:<plugin_id>:<subscription>` | main | STREAM | 内核 | 管理后台 | `MAXLEN ~ 100000` | 死信（同时写入 PG `kernel.event_dead_letter`） |
| `k:node:<node_id>` | main | HASH | 内核 | 内核 | 30 s，心跳续期 | 副本心跳：版本、插件集合哈希、启动时间 |
| `k:lock:<name>` | main | STRING | 内核 | 内核 | 租约（默认 30 s） | 分布式锁，值为 `<node_id>:<fencing_token>` |
| `k:fence:<name>` | main | STRING(INCR) | 内核 | 内核 | 无 | 锁的 fencing token 计数器 |
| `k:cron:<plugin_id>:<job>:<fire_ts>` | main | STRING | 内核 | 内核 | 24 h | cron 单次触发去重（SET NX） |
| `k:rl:<bucket>:<subject>` | main | STRING | 内核网关 | 内核网关 | 窗口长度 | 限流（GCRA，值为 TAT） |
| `k:idem:<principal>:<key>` | main | HASH | 内核网关 | 内核网关 | 24 h | 幂等键：请求哈希、状态、缓存的响应 |
| `k:cache:<area>:<key>` | main | 任意 | 内核 | 内核 | ≤ 1 h | 内核缓存（设置、术语快照版本号等） |
| `k:quota:<plugin_id>` | main | HASH | 内核 | 内核 | 无 | 插件 Redis 配额估算结果 |
| `k:bus` | main | PUB/SUB 频道 | 内核 | 内核各副本 | — | 进程间缓存失效广播（设置变更、术语变更） |
| `p:<plugin_id>:*` | main | 任意 | 该插件 | 该插件 | 插件自定 | 插件私有命名空间（自动改写） |

## 4. 插件私有前缀下的建议子键（非强制，便于评审）

| 插件 | 子键示例（插件代码中写法） | 说明 |
|---|---|---|
| identity | `sess:<session_id>`、`login_fail:<username_hash>` | 会话、登录失败计数 |
| accounting | `rate:<torrent_id>`、`rate:global`、`win:<partition>` | 生效倍率缓存、窗口游标 |
| search-meilisearch | `qc:<hash>` | 查询结果缓存（键含权限范围） |
| promotion | `active` | 活动促销快照 |
| checkin | `day:<yyyymmdd>` | 当日签到位图 |

## 5. pt-tracker 的 ACL

按 design/10 §5.5：pt-tracker 账号只读 `trk:*`，只写 `ps:*`、`acct:*`、`u:*:live`。内核账号不得写 `ps:*`；tracker-bridge 账号可写 `trk:*`、读 `ps:*`；accounting 账号可读写 `acct:*`（消费组）、`u:*:live`。ACL 模板放在 `deploy/redis/acl.conf`。
