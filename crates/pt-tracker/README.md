# pt-tracker

PT Studio 的 HTTP tracker（许可证 AGPL-3.0-only，见决策 D16）。独立程序，只经 Redis 与站点通信，不链接站点代码。

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
| 读 | `trk:pk:<passkey>`、`trk:t:<info_hash>`、`trk:clients`、`trk:cfg` | tracker-bridge 插件写入的快照；启动时全量加载到内存 |
| 读 | `trk:sync` | 变更流，`XREAD BLOCK` 跟随；本地未命中时回源一次并负缓存 60 秒 |
| 写 | `acct:<user_id % N>` | 流量事件（`v`、`eid`、`ts`、`uid`、`tid`、`ih`、`pid`、`ip`、`port`、`ev`、`up`、`down`、`left`、`dup`、`ddown`、`dseed`、`dleech`、`flags`、`ua`），由 accounting 消费；`N` 取 `acct:_meta` 的 `partitions` |
| 写 | `u:<uid>:live` | 实时原始计数 `up`、`down`、`seed_time` |
| 读写 | `ps:<info_hash>` | 会话状态（field `<uid>:<peer_id_hex>`，JSON），重启后据此恢复基线 |

增量计算见 `src/accounting.rs`：宁可少记、不多记；状态丢失时只建立基线，回退、时长截断、首次完成等以 `flags` 位交给下游。

## 本版本的限制

- 单节点：swarm 在内存中，重启后 peer 列表要等客户端重新 announce 才恢复（会话基线从 Redis 恢复，不影响记账）；
  分片与一致性哈希未实现。
- 没有 Prometheus 指标、UDP、BEP 41。
- 未使用 torrust 包：设计（design/10）计划复用 torrust 的 swarm 与协议包，但这些包没有发布到 crates.io，本版本用自己的
  实现；数据契约与设计一致，将来可以替换内部实现而不影响站点。
