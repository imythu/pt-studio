# top10（排行榜）设计说明

> 2026-10-17 · 开站计划 M3.5。参考 NexusPHP 的 Top 10。

## 1. 榜单

| 榜单 | 周期 | 数据来源 |
|---|---|---|
| `torrents.snatched` 完成数最多 | 24 小时 / 7 天 / 30 天 / 全部 | `accounting.v_snatch.completed_at`；全部用 `v_torrent_peers.completed` |
| `torrents.active` 最活跃 | 当前 | `v_torrent_peers` 做种 + 下载（1 小时内有汇报的 peer） |
| `torrents.seeded` 做种最多 | 当前 | `v_torrent_peers.seeders` |
| `users.uploaded` 上传量 | 7 天 / 30 天 / 全部 | `v_user_daily.uploaded` 求和；全部用 `v_user_totals.uploaded` |
| `users.seed_time` 做种时长 | 7 天 / 30 天 / 全部 | `v_user_daily.seed_secs` / `v_user_totals.seed_secs` |
| `users.uploads` 发布数 | 7 天 / 30 天 / 全部 | `torrent.v_torrent_public`（已发布、非匿名） |

- 种子榜只计已发布的种子；用户榜只计状态正常（active）且未退出的用户。
- 设置：`size` 每榜名次数（默认 10，最多 100）、`user_boards` 是否显示用户榜（默认开）。
- 用户可在排行榜页勾选“不在用户榜中显示我”（`PUT /v1/me {opted_out}`），读取时即过滤，下次重算时也排除。

## 2. 计算

`refresh` 任务每小时第 7 分钟运行（`catch_up = once`，启动时补跑一次），清空 `entries` 后按榜单 × 周期写入前 N 名，记录 `refreshes.refreshed_at`。读取 `GET /v1/boards`（`top10.view`，默认 user）只查快照，并按当前状态再过滤（种子已下架、用户已退出）。

## 3. 后续

按资源类型分榜、魔力值 / 分享率榜（需要 ledger 视图）、榜单历史、首页小组件。
