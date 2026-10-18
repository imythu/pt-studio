# donations（捐赠）

| 项 | 内容 |
|---|---|
| 版本 | 0.1.0（开站计划 M4.4） |
| 类型 | WASM |
| 依赖 | 可选 `ctx.levels`（VIP）、`ctx.ledger`（魔力值）、`ctx.medals`（勋章）、`ctx.notify`（通知） |

## 功能

- 站点不接入支付渠道：用户按说明页付款并通过“联系管理组”告知，管理员核实后在后台登记（`donations.manage`，默认 admin）。
- 登记即发放奖励，幂等键由捐赠 ID 派生：
  - VIP 天数：`ctx.levels.grant_vip`，在当前有效的 VIP 之后顺延（同时与商城 VIP 商品叠加）；
  - 魔力值：`ctx.ledger.transfer`（凭空发放，原因 `donations.reward`，流水页显示“捐赠奖励”）；
  - 勋章：`ctx.medals.grant`（可选，填勋章 ID）。
  - 未填写 VIP 天数 / 魔力值时按设置的“每 1 元奖励”计算（向下取整）。
  - 某项奖励失败时捐赠仍然记录，返回 `failed` 列表、写入审计，由管理员手动补发（服务调用各自提交，无法整体回滚）。
- 撤销：只把捐赠排除出统计与名单，已发放的奖励不收回（需要时用等级 / 账本后台手动处理）。
- 说明页 `/p/donations/`：Markdown 说明（设置 `instructions`）、本月进度条（设置 `monthly_goal`，0 不显示）、
  公开捐赠名单（只列出登记时勾选“同意公开”的用户名，近一年，不显示金额；设置 `show_donors`）、本人捐赠记录与 VIP 到期时间。
  设置 `enabled` 关闭时页面显示“暂未开放”，用户菜单不显示入口。
- 每日任务 `remind`：对捐赠过的用户，以 `ctx.levels.vip_until` 的实际到期时间为准，到期前 `remind_days`（默认 3）天提醒一次
  （模板 `donations.vip_expiring`，类别 `account`）。登记成功通知用户（模板 `donations.thanks`）。
- 事件 `donation.recorded`；登记与撤销写审计。金额以分存储。

## 接口（`/api/p/donations/v1`）

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/summary` | 说明页数据：`enabled, instructions_html, currency_symbol, monthly_goal（分）, month_total, donors, mine, vip_until` |
| GET | `/admin/donations` | 最近 200 笔与本月 / 累计金额 |
| POST | `/admin/donations` | `{user_id \| username, amount（分）, method?, note?, public?, vip_days?, bonus?, medal_id?}` → `{id, vip_days, bonus, vip_until, failed}` |
| POST | `/admin/donations/{id}/revoke` | `{reason?}` |

## levels 配套（同一里程碑）

- `ctx.levels.grant_vip {user, days, source_ref, reason?}`（scope `grant`）：结束当前 VIP 覆盖并新建一条，到期时间 = max(当前到期, 现在) + days；
  `source_ref` 唯一，重复调用返回同一结果（`duplicate: true`）。VIP 等级取 key 为 `vip` 的 VIP 类型等级，没有则取第一个 VIP 类型等级。
- `ctx.levels.vip_until {user}` → `{until_ms}`。
- `incr_evaluate`（每 15 分钟）结束已到期的覆盖（VIP 与带期限的手动锁定）并立即重新评估这些用户，回到基线等级。
- 商城新增 `vip` 商品类型（`params.days`，数量 × 天数）。

## 后续

在线支付渠道接入（插件化）、捐赠档位与自定义奖励、年度名单、到期前自动续费提醒的多次提醒。
