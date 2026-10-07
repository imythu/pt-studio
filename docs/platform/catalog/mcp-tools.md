# MCP 工具目录

> 版本 0.1 · 2026-10-04 · 网关由 `mcp` 插件实现（design/07 §2），注册入口见 [../kernel/03-详细设计说明书.md](../kernel/03-详细设计说明书.md) §10

## 1. 规则

- 工具名：snake_case，动词开头，**全站唯一**；正则 `^[a-z][a-z0-9_]{2,63}$`（符合 MCP 工具名字符集）。
- 用户侧读自己数据的工具用 `get_my_*` / `list_my_*`；管理员侧工具一律 `admin_` 前缀，并设 `audience = "admin"`。
- 每个工具在 manifest `[[mcp.tools]]` 声明：`name`、`description`（中文，面向模型，说明何时使用）、`input_schema`、`output_schema`、`permission`、`write`、`confirm`、`audience`。输入输出 schema 与对应 HTTP 接口的 OpenAPI schema **共用同一份文件**（design/07 §2）。
- `write = true` 的工具 `confirm` 默认为 true：网关先返回“待确认”（`mcp.confirmation_required`），用户在站内或通过 `ctx.mcp.confirm` 确认后才执行。只有管理员可在 site.toml 为个别工具关闭确认。
- 返回结果中**永远不含** passkey、IP、邮箱（网关在出口再做一次字段名黑名单过滤，见 conventions/security.md §8）。
- 返回中的站点叫法使用术语表渲染后的名称。

## 2. 预留工具清单

| 工具 | 所属插件 | 简述 | audience | write | 权限点 | 版本 |
|---|---|---|---|---|---|---|
| `search_torrents` | search-meilisearch | 关键词/自然语言搜索种子，支持筛选 | user | 否 | `search-meilisearch.use` | 0.1.0 |
| `get_torrent` | torrent | 种子与 Group 详情 | user | 否 | `torrent.download` | 0.1.0 |
| `get_download_link` | torrent | 生成限时下载链接（不返回 passkey 本身） | user | 否 | `torrent.download` | 0.1.0 |
| `get_my_profile` | identity | 自己的资料、等级、状态 | user | 否 | — | 0.1.0 |
| `get_my_stats` | accounting | 上传/下载/分享率/做种时间 | user | 否 | — | 0.1.0 |
| `list_my_seeding` | accounting | 当前做种列表 | user | 否 | — | 0.1.0 |
| `list_torrents_needing_seeders` | accounting | 推荐需要保种的资源 | user | 否 | — | 0.1.0 |
| `get_my_bonus` | bonus | 魔力值余额和最近收支 | user | 否 | `bonus.balance.read_own` | 0.1.0 |
| `get_my_level` | levels | 当前等级与升级进度 | user | 否 | — | 0.1.0 |
| `get_my_ratio_status` | ratio-policy | 分享率要求与警告状态 | user | 否 | — | 0.1.0 |
| `list_my_hnr` | hnr | 自己的 H&R 记录 | user | 否 | — | 0.1.0 |
| `get_my_exam` | newbie-exam | 新手考核进度 | user | 否 | — | 0.1.0 |
| `list_active_promotions` | promotion | 当前促销 | user | 否 | — | 0.1.0 |
| `list_shop_items` | shop | 商城商品 | user | 否 | `shop.use` | 0.1.0 |
| `buy_shop_item` | shop | 购买商品 | user | 是（确认） | `shop.use` | 0.1.0 |
| `check_in` | checkin | 签到 | user | 是（可免确认） | `checkin.use` | 0.1.0 |
| `list_requests` | requests | 浏览求种 | user | 否 | — | 0.1.0 |
| `create_request` | requests | 发布求种 | user | 是（确认） | `requests.create` | 0.1.0 |
| `list_my_messages` | messages | 站内信摘要（标题、发件人，不含正文） | user | 否 | — | 0.1.0 |
| `send_message` | messages | 发送站内信 | user | 是（确认） | `messages.send` | 0.1.0 |
| `list_subtitles` | subtitles | 种子的字幕 | user | 否 | — | 0.1.0 |
| `get_term` | terms | 查询站点术语（帮助模型使用正确叫法） | user | 否 | — | 0.1.0 |
| `ask_site_help` | ai-assistant | 站点规则问答（基于规则文档） | user | 否 | `ai-assistant.use` | 0.1.0 |
| `admin_list_moderation_queue` | moderation | 审核队列 | admin | 否 | `moderation.queue.read` | 0.1.0 |
| `admin_decide_moderation` | moderation | 审核通过/拒绝 | admin | 是（确认） | `moderation.decide` | 0.1.0 |
| `admin_get_ai_review` | ai-upload-review | 查看 AI 审核报告 | admin | 否 | `ai-upload-review.read` | 0.1.0 |
| `admin_list_reports` | reports | 举报列表 | admin | 否 | `reports.moderate` | 0.1.0 |
| `admin_resolve_report` | reports | 处理举报 | admin | 是（确认） | `reports.moderate` | 0.1.0 |
| `admin_inspect_user` | identity | 用户概况（不含邮箱/IP） | admin | 否 | `identity.users.read` | 0.1.0 |
| `admin_get_user_traffic` | accounting | 流量明细与基础信号 | admin | 否 | `accounting.signals.read` | 0.1.0 |
| `admin_list_anticheat_cases` | anticheat | 作弊案件列表 | admin | 否 | `anticheat.cases.read` | 0.1.0 |
| `admin_explain_anticheat_case` | anticheat | 用 `ctx.llm` 解释异常信号 | admin | 否 | `anticheat.cases.read` | 0.1.0 |
| `admin_resolve_anticheat_case` | anticheat | 处置作弊案件 | admin | 是（确认） | `anticheat.cases.moderate` | 0.1.0 |
| `admin_set_user_status` | identity | 封禁/禁止下载/恢复 | admin | 是（确认，不可关闭） | `identity.users.ban` | 0.1.0 |
| `admin_forgive_hnr` | hnr | 免除 H&R | admin | 是（确认） | `hnr.forgive` | 0.1.0 |
| `admin_site_stats` | accounting | 站点运营概况 | admin | 否 | `accounting.stats.read` | 0.1.0 |
