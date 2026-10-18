# tickets（联系管理组）

| 项 | 内容 |
|---|---|
| 版本 | 0.1.0（开站计划 M4.3） |
| 类型 | WASM |
| 依赖 | `ctx.rbac`；可选 `ctx.notify` |

## 功能

- 用户在 `/p/tickets/` 按类别（账号、种子、申诉、建议、其他）提交工单；同时未关闭的工单数有上限（设置 `max_open`，默认 3）。
- 状态：`open`（等待员工）→ 员工回复后 `pending_user`（等待用户）→ 用户回复回到 `open`；任一方可关闭；用户在关闭后 `reopen_days`（默认 14）天内可重新打开，员工不受限。
- 员工（`tickets.handle`，默认 moderator）：队列按状态筛选、“只看我认领的”、认领 / 指派 / 取消认领、回复（首次回复自动认领）、内部备注（`internal`，用户看不到，已关闭的工单也可写）。用户端员工回复显示为“管理组”，不显示员工名。
- 正文经 pt-markup 渲染（Markdown，≤ 20000 字）。员工回复与员工关闭通知用户，用户回复通知被指派的员工（`ctx.notify`，类别 `account`，模板 `tickets.update`）。
- 事件：`ticket.created`、`ticket.closed`；用户删除时其工单全部关闭。员工关闭、指派写审计。

## 接口（`/api/p/tickets/v1`）

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/tickets` | 我的工单 |
| POST | `/tickets` | 提交 `{category, subject, body}` |
| GET | `/tickets/{id}` | 详情（本人或员工；员工可见内部备注与处理人） |
| POST | `/tickets/{id}/messages` | 回复 `{body, internal?}` |
| POST | `/tickets/{id}/close`、`/reopen` | 关闭 / 重新打开 |
| GET | `/admin/tickets?state=open\|pending_user\|closed&mine=1` | 队列与计数 |
| POST | `/admin/tickets/{id}/assign` | `{user_id}` 指派；省略为认领给自己；`0` 取消认领 |

## 前端

用户菜单“联系管理组”、`/p/tickets/`、`/p/tickets/<id>`、后台 `/admin/p/tickets/`、仪表盘待处理数。

## 后续

工单模板与必填字段（如申诉需填种子链接）、附件、满意度评价、超时提醒、与举报工单（reports）合并视图、关联用户资料侧栏。
