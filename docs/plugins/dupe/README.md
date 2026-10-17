# dupe（去重与替代）设计说明

> 2026-10-17 · 开站计划 M3.8。参考 Gazelle 的 dupe / trump 规则与 NexusPHP 的“替代”操作。

## 1. 去重

| 检查 | 时机 | 行为 |
|---|---|---|
| info hash 相同 | torrent 上传 | torrent 插件自身拒绝（`torrent.duplicate`） |
| 内容相同（piece 指纹 `content_fp` 一致，如只改了 source / 名称重新制种） | 钩子 `torrent.before_upload`（优先级 30） | `content_check`（默认开）时拒绝 `dupe.same_content`，details 带已有种子 ID；已下架（rejected / disabled）与已删除的不算 |
| 同作品同规格（槽位） | 钩子 / 发布后事件 | `slot_policy`：`off` 不检查；`review`（默认）发布后若同作品已有同槽位的已发布种子，记入审核队列；`block` 直接拒绝 `dupe.slot_taken`（details 带已有种子 ID） |

槽位 = 设置 `slot_fields`（默认 resolution、source、video_codec、hdr、edition）各字段的值（小写，数组排序后以 `+` 连接，缺失记 `-`），全部缺失时不判定。没有作品（group）的种子不做槽位检查。

## 2. 替代（trump）

- 申请：`POST /v1/trumps {old_id, new_id, reason}`（`dupe.request`）。只有新种子的发布者或 `dupe.manage` 可以申请；两个种子必须属于同一作品，旧种子必须已发布；同一旧种子同时只能有一个待审核 / 已批准的替代。管理员可带 `approve: true` 直接批准。
- 审核：`POST /v1/admin/trumps/{id}/decision {decision: approve|reject, note?}`。批准后经 `ctx.torrents.set_status`（作用域 `write:status`）把旧种子设为 `disabled`（原因 “trumped by #新ID: 理由”），关闭相关的疑似重复记录，发布 `torrent.trumped`，并（`notify_snatchers`，默认开）通知下载完成过旧种子的用户（最多 5000 人，幂等键按替代 + 用户）；批准与驳回都写审计。
- 种子删除时：相关的待审核替代自动驳回、疑似重复关闭。

## 3. 接口

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/v1/torrents/{id}` | 被替代（`trumped_by`）/ 替代了（`replaces`）/ 同槽位的已发布种子 / 能否申请及可选的旧种子 |
| POST | `/v1/trumps` | 申请替代 |
| GET | `/v1/admin/queue` | 待审核替代与未处理的疑似重复（`dupe.manage`） |
| POST | `/v1/admin/trumps/{id}/decision` | 批准 / 驳回 |
| POST | `/v1/admin/candidates/{id}/dismiss` | 疑似重复判定为保留 |

## 4. 前端

种子页“版本与替代”卡片（被替代提示、替代了、同规格版本、申请表单，管理员可直接批准）；后台 `/admin/p/dupe/` 审核队列；仪表盘待办数。

## 5. 后续

按类型配置槽位与“可共存”规则（如不同压制组、不同音轨可共存）、发布页提前提示同槽位种子、替代后自动把旧种子做种者引导到新种子（RSS / 客户端）、文件列表相似度检查。
