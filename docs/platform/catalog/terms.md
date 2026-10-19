# 术语键目录

> 版本 0.1 · 2026-10-04 · 背景见 design/05 §2

## 1. 规则

- 键：`<plugin_id>.<path>`，`path` 由 `.` 分隔的 snake_case 段组成；动态段写作 `<name>`，实际使用时替换为值（如 `levels.class.<class_id>.name` → `levels.class.5.name`）。正则 `^[a-z][a-z0-9-]*(\.[a-z0-9_]+)+$`。
- 值结构（每种语言一份）：

| 字段 | 必填 | 说明 |
|---|---|---|
| `name` | 是 | 显示名，如“魔力值” |
| `short` | 否 | 短名 / 缩写，如“魔力” |
| `unit` | 否 | 单位，如“点” |
| `icon` | 否 | 图标：内置图标名（`icon:coin`）或 `blob:` 相对路径 |
| `plural` | 否 | ICU MessageFormat 复数形式，如 `{count, plural, one {# Bonus Point} other {# Bonus Points}}` |

- 插件在 manifest `[terms]` 中声明默认值，至少提供 `zh-CN` 与 `en`；管理员在后台覆盖。站点语言回退顺序：请求语言 → `site.default_locale` → `en` → 键名。
- 用法：前端 `t('term:bonus.currency')`；后端 `ctx.terms.get("bonus.currency")`；模板中 `{term:bonus.currency}`（由 `ctx.terms.render` 展开）。
- **非术语**（界面文案、错误信息）走普通 i18n，不进术语表。判断标准：换一个站点会不会改叫法？会改的才是术语。
- 权限点说明文字用 `<plugin_id>.perm.<name>` 形式的术语键（manifest `description_term`）；这类键不在下表逐一列出。

## 2. 分类与清单

分类对应 design/05 §2.2。

### 2.1 虚拟货币

| 键 | 所属插件 | zh-CN | en |
|---|---|---|---|
| `bonus.currency` | bonus | 魔力值 | Bonus Points |
| `bonus.currency.unit` | bonus | 点 | pts |
| `bonus.seed_points` | bonus | 做种积分 | Seeding Points |
| `ledger.currency.<code>` | ledger（登记时由币种所属插件提供默认值） | — | — |
| `accounting.uploaded` | accounting | 上传量 | Uploaded |
| `accounting.downloaded` | accounting | 下载量 | Downloaded |
| `accounting.ratio` | accounting | 分享率 | Ratio |
| `accounting.seed_time` | accounting | 做种时间 | Seed Time |
| `shop.name` | shop | 魔力值商城 | Bonus Shop |
| `shop.item.<item_id>` | shop | （管理员定义） | |

### 2.2 用户等级

| 键 | 所属插件 | zh-CN | en |
|---|---|---|---|
| `levels.class.<class_id>.name` | levels | （按预设，如“User / Power User / Elite User …”的中文） | |
| `levels.class.<class_id>.short` | levels | | |
| `levels.protected` | levels | 保号 | Account Protected |
| `levels.vip` | levels | 贵宾 | VIP |
| `levels.honor.<honor_id>` | levels | 荣誉等级（管理员定义） | |
| `identity.role.<role>` | identity | 用户 / 发布员 / 管理员 / 站务 … | User / Uploader / Moderator / Admin / Sysop |

### 2.3 身份标识

| 键 | 所属插件 | zh-CN | en |
|---|---|---|---|
| `medals.medal` | medals | 勋章 | Medal |
| `medals.badge` | medals | 徽章 | Badge |
| `medals.title` | medals | 头衔 | Title |
| `medals.item.<medal_id>` | medals | （管理员定义） | |
| `identity.custom_title` | identity | 自定义称号 | Custom Title |

### 2.4 促销

| 键 | 所属插件 | zh-CN | en |
|---|---|---|---|
| `promotion.free` | promotion | 免费 | Freeleech |
| `promotion.double_up` | promotion | 2x 上传 | 2x Upload |
| `promotion.double_up_free` | promotion | 2x 免费 | 2x Free |
| `promotion.half_down` | promotion | 50% | 50% Off |
| `promotion.thirty_down` | promotion | 30% | 70% Off |
| `promotion.double_up_half_down` | promotion | 2x 50% | 2x 50% |
| `promotion.global_event` | promotion | 全站活动 | Site-wide Event |
| `promotion.event.<event_id>` | promotion | （管理员定义） | |

### 2.5 考核

| 键 | 所属插件 | zh-CN | en |
|---|---|---|---|
| `newbie-exam.name` | newbie-exam | 新手考核 | Newbie Exam |
| `hnr.name` | hnr | H&R | Hit and Run |
| `hnr.rule_summary` | hnr | H&R 规则说明 | H&R Rules |
| `ratio-policy.warning` | ratio-policy | 分享率警告 | Ratio Watch |
| `seedbox.name` | seedbox | 盒子 | Seedbox |
| `requests.keep_seeding_task` | requests | 保种任务 | Seeding Task |

### 2.6 种子状态与标签

| 键 | 所属插件 | zh-CN | en |
|---|---|---|---|
| `torrent.status.candidate` | torrent | 候选 | Candidate |
| `torrent.status.pending` | torrent | 待审核 | Pending |
| `torrent.status.approved` | torrent | 已审核 | Approved |
| `torrent.status.rejected` | torrent | 已拒绝 | Rejected |
| `torrent.status.disabled` | torrent | 已下架 | Disabled |
| `torrent.tag.no_repost` | torrent | 禁转 | No Repost |
| `torrent.tag.official` | torrent | 官方 | Official |
| `torrent.tag.original` | torrent | 原创 | Original |
| `torrent.tag.sticky` | torrent | 置顶 | Sticky |
| `torrent.tag.<tag>` | torrent | （管理员定义） | |
| `moderation.candidate_area` | moderation | 候选区 | Candidates |

### 2.7 社区

| 键 | 所属插件 | zh-CN | en |
|---|---|---|---|
| `requests.name` | requests | 求种 | Requests |
| `requests.bounty` | requests | 悬赏 | Bounty |
| `messages.name` | messages | 站内信 | Messages |
| `forum.name` | forum | 论坛 | Forums |
| `forum.board.<board_id>` | forum | （管理员定义） | |
| `checkin.name` | checkin | 签到 | Check-in |
| `lottery.name` | lottery | 抽奖 | Lottery |
| `comments.name` | comments | 评论 | Comments |
| `subtitles.name` | subtitles | 字幕 | Subtitles |
| `reports.name` | reports | 举报 | Reports |
| `invite.name` | invite | 邀请 | Invite |

### 2.8 内容

| 键 | 所属插件 | zh-CN | en |
|---|---|---|---|
| `type-movie.name` | type-movie | 电影 | Movies |
| `type-tv.name` | type-tv | 剧集 | TV |
| `type-music.name` | type-music | 音乐 | Music |
| `type-anime.name` | type-anime | 动漫 | Anime |
| `type-generic.name` | type-generic | 其他 | Other |
| `type-generic.kind.<kind>` | type-generic | 游戏 / 软件 / 电子书 / 课程 … | |
| `type-<x>.field.<field>` | 对应 type-* | 字段显示名（筛选面板用） | |
| `torrent.category.<category>` | torrent | 分类名（管理员定义） | |
| `torrent.release_group.<group>` | torrent | 制作组标签（管理员定义） | |

### 2.9 其他

| 键 | 所属插件 | zh-CN | en |
|---|---|---|---|
| `kernel.site_name` | kernel（默认值来自 site.toml `site.name`，可覆盖） | | |
| `identity.passkey` | identity | 密钥 | Passkey |
| `invite.code` | invite | 邀请码 | Invite Code |
| `bonus.hourly_income` | bonus | 每小时收入 | Hourly Income |
