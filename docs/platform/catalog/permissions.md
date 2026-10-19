# RBAC 权限点目录

> 版本 0.1 · 2026-10-04

## 1. 规则

- 格式：`<plugin_id>.<resource>.<action>`；资源唯一的插件可省略资源段：`<plugin_id>.<action>`。正则 `^[a-z][a-z0-9-]*(\.[a-z][a-z0-9_]*){1,2}$`。
- `action` 只用以下动词：`read`（查看他人/全部）、`read_own`、`create`、`update`、`update_own`、`delete`、`delete_own`、`manage`（后台配置与全部操作）、`moderate`（审核）、`use`（使用功能）、`export`、`adjust`（数值调整）、以及表中个别专用动词。
- 权限点由插件在 manifest `[[permissions]]` 中声明，内核启动时通过 `ctx.rbac.register_permissions()` 交给 identity；`default_roles` 只在首次登记时生效，之后以管理员配置为准。
- 内置角色（identity 创建，不可删除）：`guest`、`user`、`uploader`、`moderator`、`admin`、`sysop`。`sysop` 隐含全部权限。插件可登记动态角色 `<plugin_id>:<name>`（如 `levels:class_5`、`medals:vip`），需要 `ctx.rbac` 作用域 `write:roles`。
- 路由与 MCP 工具上的 `permission` 由内核网关统一检查；服务内部的细粒度检查由提供方调用 `ctx.rbac.check()`。

## 2. 权限点清单

“默认角色”指首次登记时授予的最低内置角色（更高角色自动继承：guest ⊂ user ⊂ uploader ⊂ moderator ⊂ admin ⊂ sysop）。

| 权限点 | 所属插件 | 说明 | 默认角色 |
|---|---|---|---|
| `kernel.plugins.read` | kernel | 查看插件列表与状态 | admin |
| `kernel.settings.manage` | kernel | 修改插件运行时设置 | admin |
| `kernel.audit.read` | kernel | 查看管理操作日志 | admin |
| `kernel.audit.read` | kernel | 查看审计日志 | admin |
| `kernel.events.replay` | kernel | 触发事件回放 | sysop |
| `kernel.jobs.manage` | kernel | 手动运行 / 暂停定时任务 | sysop |
| `kernel.events.read` | kernel | 查看事件日志、死信 | admin |
| `terms.manage` | terms | 修改术语覆盖 | admin |
| `identity.users.read` | identity | 查看用户详情（非敏感） | moderator |
| `identity.users.read_sensitive` | identity | 查看邮箱、IP 记录 | admin |
| `identity.users.update` | identity | 编辑用户资料 | moderator |
| `identity.users.ban` | identity | 封禁 / 解封、禁止下载 | moderator |
| `identity.users.delete` | identity | 删除用户 | admin |
| `identity.roles.manage` | identity | 角色与权限配置 | sysop |
| `identity.profile.update_own` | identity | 修改自己的资料 | user |
| `identity.tokens.manage_own` | identity | 管理自己的 API / MCP token | user |
| `identity.passkey.reset_own` | identity | 重置自己的 tracker passkey | user |
| `invite.send` | invite | 发送邀请 | user |
| `invite.manage` | invite | 管理邀请、调整额度 | admin |
| `invite.tree.read` | invite | 查看邀请树 | moderator |
| `registration-policy.manage` | registration-policy | 注册策略配置 | admin |
| `registration-policy.applications.moderate` | registration-policy | 审核注册申请 | moderator |
| `torrent.upload` | torrent | 上传种子 | user |
| `torrent.download` | torrent | 下载种子文件 | user |
| `torrent.update_own` | torrent | 编辑自己的种子 | user |
| `torrent.update` | torrent | 编辑任意种子 | moderator |
| `torrent.delete` | torrent | 删除种子 | moderator |
| `torrent.tags.manage` | torrent | 设置官方 / 置顶 / 禁转等标签 | moderator |
| `torrent.groups.manage` | torrent | 编辑、合并 Group | moderator |
| `torrent.anonymous.reveal` | torrent | 查看匿名发布者 | admin |
| `tracker-bridge.clients.manage` | tracker-bridge | 客户端白名单 | admin |
| `tracker-bridge.peers.read` | tracker-bridge | 查看 peer 列表 | moderator |
| `tracker-bridge.resync` | tracker-bridge | 全量重建同步 | sysop |
| `accounting.stats.read` | accounting | 查看他人流量明细 | moderator |
| `accounting.traffic.adjust` | accounting | 手工调整上传 / 下载量 | admin |
| `accounting.signals.read` | accounting | 查看反作弊基础信号 | moderator |
| `ledger.balance.read` | ledger | 查看他人余额与流水 | moderator |
| `ledger.balance.adjust` | ledger | 手工调整余额 / 冲正 | admin |
| `ledger.transfer.use` | ledger | 用户间转账（若站点开放） | user |
| `anticheat.cases.read` | anticheat | 查看作弊案件 | moderator |
| `anticheat.cases.moderate` | anticheat | 处置作弊案件 | moderator |
| `anticheat.rules.manage` | anticheat | 规则配置 | admin |
| `anticheat.sanctions.manage` | anticheat | 禁止下载、封禁、批准封禁（四眼） | admin |
| `anticheat.appeals.moderate` | anticheat | 复核申诉 | moderator |
| `anticheat.appeals.create` | anticheat | 提交申诉 | user |
| `bonus.balance.read_own` | bonus | 查看自己的魔力值 | user |
| `bonus.rules.manage` | bonus | 魔力值公式配置 | admin |
| `shop.use` | shop | 购买 | user |
| `shop.items.manage` | shop | 商品管理 | admin |
| `checkin.use` | checkin | 签到 | user |
| `lottery.use` | lottery | 转动幸运转盘 | user |
| `lottery.manage` | lottery | 管理奖品、概率与统计 | admin |
| `medals.grant` | medals | 颁发 / 收回勋章 | admin |
| `medals.manage` | medals | 勋章定义 | admin |
| `levels.manage` | levels | 等级规则配置 | admin |
| `levels.protect` | levels | 设置保号 | admin |
| `ratio-policy.manage` | ratio-policy | 分享率规则配置 | admin |
| `ratio-policy.warnings.moderate` | ratio-policy | 手动警告 / 解除 | moderator |
| `hnr.read` | hnr | 查看他人 H&R | moderator |
| `hnr.forgive` | hnr | 免除 H&R | moderator |
| `hnr.manage` | hnr | H&R 规则配置 | admin |
| `newbie-exam.manage` | newbie-exam | 考核配置、豁免 | admin |
| `seedbox.register_own` | seedbox | 登记自己的盒子 | user |
| `seedbox.manage` | seedbox | 盒子规则与审核 | admin |
| `promotion.create` | promotion | 设置单种促销 | moderator |
| `promotion.manage` | promotion | 自动 / 全站促销规则 | admin |
| `search-meilisearch.use` | search-meilisearch | 搜索 | user |
| `search-meilisearch.admin_index.read` | search-meilisearch | 搜索 users_admin 索引 | admin |
| `search-meilisearch.manage` | search-meilisearch | 重建索引、同义词 | admin |
| `type-movie.manage`、`type-tv.manage`、`type-music.manage`、`type-anime.manage`、`type-generic.manage` | 对应 type-* | 类型字段与规范配置 | admin |
| `metadata-<source>.manage` | 对应 metadata-* | 元数据源配置（API Key 等） | admin |
| `metadata-<source>.refresh` | 对应 metadata-* | 手动刷新元数据 | uploader |
| `mediainfo.use` | mediainfo | 解析 MediaInfo | user |
| `comments.create` | comments | 发表评论 | user |
| `comments.update_own` | comments | 编辑自己的评论 | user |
| `comments.moderate` | comments | 编辑 / 删除任意评论 | moderator |
| `forum.read` | forum | 浏览论坛 | user |
| `forum.post` | forum | 发帖回帖 | user |
| `forum.moderate` | forum | 版务 | moderator |
| `forum.manage` | forum | 版块配置 | admin |
| `messages.send` | messages | 发送站内信 | user |
| `messages.broadcast` | messages | 群发系统消息 | admin |
| `requests.create` | requests | 发布求种 | user |
| `requests.fill` | requests | 应求 | user |
| `requests.moderate` | requests | 管理求种 | moderator |
| `subtitles.upload` | subtitles | 上传字幕 | user |
| `subtitles.moderate` | subtitles | 管理字幕 | moderator |
| `moderation.queue.read` | moderation | 查看审核队列 | moderator |
| `moderation.decide` | moderation | 审核通过 / 拒绝 | moderator |
| `moderation.manage` | moderation | 审核规则配置 | admin |
| `moderation.candidates.vote` | moderation | 候选区投票 | user |
| `reports.create` | reports | 举报 | user |
| `reports.moderate` | reports | 处理举报 | moderator |
| `rss.use` | rss | 使用 RSS（含个人 RSS 密钥） | user |
| `notify.preferences.update_own` | notify | 设置通知偏好 | user |
| `notify.manage` | notify | 通知渠道配置 | admin |
| `crossseed-api.use` | crossseed-api | 使用辅种接口 | user |
| `crossseed-api.manage` | crossseed-api | 辅种接口配置 | admin |
| `webhook.manage` | webhook | Webhook 配置 | admin |
| `llm-claude.manage`、`llm-openai-compat.manage` | 对应 llm-* | 模型提供方配置 | sysop |
| `mcp.connect` | mcp | 使用个人 MCP token 连接 | user |
| `mcp.admin_tools.use` | mcp | 使用管理员侧 MCP 工具 | moderator |
| `mcp.manage` | mcp | MCP 网关配置 | sysop |
| `ai-upload-review.read` | ai-upload-review | 查看 AI 审核报告 | moderator |
| `ai-upload-review.manage` | ai-upload-review | 配置 | admin |
| `ai-moderation.manage` | ai-moderation | 配置 | admin |
| `ai-assistant.use` | ai-assistant | 使用站内助手 | user |
| `ai-assistant.manage` | ai-assistant | 配置 | admin |
