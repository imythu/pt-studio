# pt-tracker

PT Studio 的 HTTP tracker（许可证 AGPL-3.0-only，见决策 D16；自研实现，见决策 D20）。独立程序，只经 Redis 与站点通信，
不链接站点代码。**无状态**：swarm 与会话状态都在 Redis 中，可以同时运行多个节点、随意重启，负载均衡不需要按 info_hash 分片。

- `GET /announce/<passkey>`：准入（passkey、账号状态、客户端白名单、种子状态）→ swarm → 增量计算 → 写记账流
- `GET /scrape/<passkey>?info_hash=…`：单个（最多 10 个）hash 的做种 / 完成 / 下载中人数
- `GET /health`：状态与已加载的 passkey / 种子数

## 运行

```bash
cargo build --release                          # 本目录是独立的 cargo 工作区
cp tracker.example.toml tracker.toml           # 修改 redis_url 等
target/release/pt-tracker tracker.toml
```

站点 `site.toml` 的 `[plugin.torrent] announce_base_urls` 写 `http://<tracker 地址>/announce`（种子中为 `<地址>/<passkey>`）。
`deploy/dev/run-site.sh` 会自动构建并启动。

## 与站点的契约（design/10 §5.4～§5.6、catalog/redis-keys.md）

| 方向 | 键 | 说明 |
|---|---|---|
| 读 | `trk:pk:<passkey>`、`trk:t:<info_hash>`、`trk:clients`、`trk:cfg` | tracker-bridge 插件写入的快照；启动时全量加载到内存。`trk:clients` 为有序客户端规则（peer_id 前缀 / 正则 + 最低 / 最高版本、UA 前缀 / 正则，`src/clients.rs`，语义与 tracker-bridge 的规则测试接口一致，共用测试向量 `docs/plugins/tracker-bridge/client-rules-vectors.json`）；规则集无效时保留上一版 |
| 读 | `trk:sync` | 变更流，`XREAD BLOCK` 跟随；本地未命中时回源一次并负缓存 10 秒 |
| 写 | `acct:<user_id % N>` | 流量事件（`v`、`eid`、`ts`、`uid`、`tid`、`ih`、`pid`、`ip`、`port`、`ev`、`up`、`down`、`left`、`dup`、`ddown`、`dseed`、`dleech`、`flags`、`ua`），由 accounting 消费；`N` 取 `acct:_meta` 的 `partitions` |
| 写 | `u:<uid>:live` | 实时原始计数 `up`、`down`、`seed_time` |
| 读写 | `ps:{<info_hash>}` | 会话状态（field `<uid>:<peer_id_hex>`，JSON：累计值、left、时间、完成、IP、端口） |
| 读写 | `sw:{<info_hash>}:s` / `:l` / `:n` | 做种者 / 下载者 ZSET（score = 最近 announce 时间）、完成次数 |
| 写 | `trk:stats:denied:<UTC yyyymmdd>` | 被拒客户端计数（ZSET，member `<peer_id 前 8 字节转义>|<UA 前 32 字节>`，score 次数，8 天过期）；本地累加，随心跳每 10 秒写入，不同 member 超过 1 万时计入 `~overflow|` |
| 写 | `trk:stats:node:<node_id>` | 节点状态，每 10 秒写、30 秒过期：`ver`、`boot`、`sid`（已应用的 `trk:sync` 条目）、`lag_ms`（本周期最大应用延迟）、`pk`、`t`（准入缓存条目数）、`mpt`、`rps`（最近 60 秒 announce/秒）、`ts`；tracker-bridge 后台「Tracker 节点」页读取 |

每次 announce 执行一个 Lua 脚本：比较会话状态（CAS：两个节点同时处理同一会话时，后到的重新读取再算，不会重复记账）→
清理超时 peer → 更新状态与集合 → 返回人数与随机抽取的 peer（下载者优先拿做种者，做种者只拿下载者）。本节点最近写入的
会话原值缓存在内存中，省掉一次读取；CAS 保证缓存过期时仍然正确。

增量计算见 `src/accounting.rs`：宁可少记、不多记；状态丢失时只建立基线，回退、时长截断、首次完成等以 `flags` 位交给下游。

## 验证

- 单元测试 + Redis 集成测试：`cargo test`（`PT_TRACKER_TEST_REDIS`，默认 `redis://127.0.0.1:56379/13`，会清空该库）。
  覆盖两个节点共用 Redis、swarm 内容、紧凑 / 非紧凑响应、scrape、准入拒绝、流量事件、CAS 冲突、变更流实时生效、节点状态、
  客户端规则更新（带最低版本的规则经变更流生效、无效规则集不生效）、被拒客户端计数。
- 真实客户端：`node web/tests/bt-real.mjs`（站点与 tracker 运行中）——两个 transmission-cli 4.1 实例经本 tracker 完成
  32 MiB 传输，sha256 一致，accounting 记到做种者上传与下载者下载。注意 Transmission 4 忽略回环地址上的 peer，
  本机测试时 announce 地址要用局域网 IP（脚本会自动替换）。
- qBittorrent：`PT_BT_IFACE=eth0 node web/tests/bt-qbittorrent.mjs`——qBittorrent 5.1.0（libtorrent 2.0）与 Transmission 4.1
  双向各传 16 MiB（Transmission 做种 → qBittorrent 下载、qBittorrent 做种 → Transmission 下载），sha256 一致、两端记账正确、
  qBittorrent 显示 tracker 工作中。qBittorrent 的 announce 带 `key`、`corrupt`、`redundant`、`no_peer_id=1`、`supportcrypto=1`、
  `numwant=200` 与 BEP 7 的 `ipv4=`（只接受与连接不同族的自报地址，IPv4 连接上的 `ipv4=` 被忽略）。
  - 多网卡机器（VPN、透明代理）上 libtorrent 会从每个网卡分别 announce，到不了 tracker 的那一路在 qBittorrent 里显示
    “timed out”，属于客户端网络环境问题；测试中用 `PT_BT_IFACE` 让 qBittorrent 只绑定一个网卡。
  - 一端强制加密（如 Transmission `-er`）、另一端只是“允许加密”时，libtorrent 先尝试明文握手、失败后退避重试，
    首次连接慢约一分钟（实测 70 s → 两端都强制加密后 8 s），与 tracker 无关；站点可建议用户统一开启“强制加密”。
- Deluge：`PT_BT_IFACE=eth0 PT_ADMIN_USER=… PT_ADMIN_PASS=… node web/tests/bt-deluge.mjs`（2026-10-24 实测 Deluge 2.1.2 /
  libtorrent 2.0.10，`apt install deluged`）——Transmission 做种 → Deluge 下载、Deluge 做种 → Transmission 下载，各 16 MiB，
  sha256 一致、两端记账正确、Deluge 显示 “Announce OK”；给出管理员账号时再加一条拒绝 Deluge 的规则，Deluge 下一次 announce
  显示 “Error: client not allowed”，被拒客户端排行出现 `-DE212s-` / `Deluge/2.1.2.dev0 libtorrent/2.0`，恢复规则后重新放行。
  Deluge 的 peer_id 版本位末尾是小写字母（`-DE212s-`），规则正则要允许小写。libtorrent 的强制重新 announce 仍受
  `min interval` 限制，脚本用暂停 / 继续触发 stopped / started。µTorrent 只有 Windows / macOS 版本，未实测。
- 压测：`cargo run --release --bin pt-tracker-bench <redis_url> [并发] [请求数] [用户] [种子]`（会清空该库）。
  4 核开发机上 tracker、Redis 与压测程序同机：约 1.05 万次 announce / 秒，p50 5.7 ms、p99 13 ms，无失败；
  Redis CPU 约 60%。生产环境 tracker 与 Redis 分开部署时会更高；多个 tracker 节点共享 Redis 时，瓶颈是 Redis。

## 本版本的限制

- Redis 是单点：多节点 tracker 共享一个 Redis（或 Cluster，键已带哈希标签）；Redis 的容量与吞吐决定上限。
- 没有 Prometheus 指标、UDP、BEP 41。
- 不使用 torrust（D20）：协议与 swarm 自研；数据契约与 design/10 一致。
