# claims（种子认领）设计说明

> 2026-10-17 · 开站计划 M3.1。参考 NexusPHP 的“认领”：用户认领自己下载过的种子并承诺长期保种，按月考核做种时长，达标发魔力值。

## 1. 规则

| 设置 | 默认 | 说明 |
|---|---|---|
| `enabled` | true | 关闭后不能新认领，已有认领照常结算 |
| `max_claims_per_user` | 1000 | 每人同时有效的认领数 |
| `max_claimers_per_torrent` | 0 | 每个种子的认领人数上限（0 = 不限） |
| `require_completed` | true | 只能认领自己下载完成过的种子（`accounting.v_snatch.completed_at`） |
| `min_torrent_age_days` | 0 | 种子发布满多少天后才能认领 |
| `monthly_seed_hours` | 500 | 每月每个认领种子需要的做种时长 |
| `reward_bonus` | 300 | 每个达标的认领发放的魔力值（0 = 不发） |
| `remove_after_failures` | 2 | 连续几个月不达标取消认领（0 = 不取消） |
| `unclaim_cooldown_days` | 0 | 取消认领后多少天内不能再认领同一种子 |

## 2. 数据

- `claims(id, user_id, torrent_id, state active/removed, claimed_at, removed_at, remove_reason, period_start, seed_secs_base, fails)`，`(user_id, torrent_id)` 有效唯一。
- `settlements(id, period, user_id, claims, passed, failed, reward, created_at)`：每人每月一行（幂等键 = period + user）。

## 3. 流程

- 认领：校验开关、上限、是否下载完成、发布时长、冷却 → 记录当前累计做种秒数为基线。
- 月结（每月 1 日 00:30 UTC，可重入）：对上个周期开始前创建的每个有效认领，`delta = 当前 seed_secs − 基线`；`delta ≥ monthly_seed_hours × 3600` 为达标；按人汇总发奖（`ctx.ledger` 幂等发行）、连续不达标计数，达到 `remove_after_failures` 时取消；之后把基线设为当前值、周期前移。周期中途认领的种子按实际天数折算要求（不足一个月按比例）。
- 种子删除（`torrent.deleted`）：相关认领取消，不计失败。

## 4. 接口

- `POST /v1/torrents/{id}/claims`、`DELETE /v1/torrents/{id}/claims`、`GET /v1/torrents/{id}`（认领人数、本人是否认领）
- `GET /v1/me`（我的认领与本月进度、上次结算）、`GET /v1/torrents/{id}/claimers`（认领人列表，员工可见用户 ID）
- 事件：`claim.created`、`claim.removed`、`claim.settled`
- web：种子页“认领”按钮与人数（`torrent.detail.actions`），“我的认领”页（进度条）。
