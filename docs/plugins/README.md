# 官方插件

PT Studio 的站点功能全部由插件提供。下表列出全部官方插件、实现状态与设计文档。

> 插件 ID、文档缩写、提供的服务以 [平台插件登记表](../platform/catalog/plugins.md) 为准。功能层面的完成情况见 [../features.md](../features.md)。

图例：✅ 已实现　📐 已有设计，尚未实现

设计文档有两种形式：完整四件套（`01-需求规格说明书` / `02-概要设计说明书` / `03-详细设计说明书` / `04-测试计划`，模板见 [../process/templates/](../process/templates/)），或较小插件的单篇 `README.md`。共用部分见 [`_shared/`](_shared/)。

## 平台与身份

| 插件 | 类型 | 状态 | 说明 | 文档 |
|---|---|:-:|---|---|
| `identity` | Native | ✅ | 用户、会话、2FA / WebAuthn、passkey、RBAC、API 与 MCP token | [设计](./identity/) |
| `captcha` | WASM | ✅ | 人机验证：Turnstile / hCaptcha / reCAPTCHA | — |
| `registration-policy` | WASM | ✅ | 注册通道、开放时段与名额、邮箱域名名单、注册审核 | [设计](./registration-policy/) |
| `invite` | WASM | ✅ | 邀请码、额度、邀请树、连坐 | [设计](./invite/) |
| `terms` | Native | 📐 | 后台修改站点术语 | [设计](./terms/) |

## 种子与内容

| 插件 | 类型 | 状态 | 说明 | 文档 |
|---|---|:-:|---|---|
| `torrent` | Native | ✅ | 种子解析、上传下载、Group、审核状态、置顶 | [设计](./torrent/) |
| `tracker-bridge` | Native | ✅ | 向 pt-tracker 同步 passkey、种子与客户端白名单 | [设计](./tracker-bridge/) |
| `type-movie` | WASM | ✅ | 电影 | [设计](./type-movie/) |
| `type-tv` | WASM | ✅ | 剧集：季集与范围检查 | [设计](./type-tv/) |
| `type-anime` | WASM | ✅ | 动漫：字幕组风格发布名 | [设计](./type-anime/) |
| `type-music` | WASM | ✅ | 音乐：艺人与版本、抓轨日志评分 | [设计](./type-music/) |
| `type-generic` | WASM | ✅ | 通用类型（游戏、软件、电子书、课程…），后台可自定义 | [设计](./type-generic/) |
| `metadata-tmdb` | WASM | ✅ | TMDB 元数据与海报 | [设计](./metadata-tmdb/) |
| `metadata-douban` | WASM | ✅ | 豆瓣元数据、PT-Gen 版式简介 | [设计](./metadata-douban/) |
| `metadata-imdb` | WASM | 📐 | IMDb 元数据 | [设计](./metadata-imdb/) |
| `metadata-musicbrainz` | WASM | 📐 | MusicBrainz 元数据 | [设计](./metadata-musicbrainz/) |
| `metadata-bangumi` | WASM | 📐 | Bangumi 元数据 | [设计](./metadata-bangumi/) |
| `metadata-igdb` | WASM | 📐 | IGDB 游戏元数据 | [设计](./metadata-igdb/) |
| `mediainfo` | WASM | ✅ | MediaInfo / BDInfo 解析与规格比对 | [设计](./mediainfo/) |
| `dupe` | WASM | ✅ | 去重与替代（trump） | [设计](./dupe/README.md) |
| `subtitles` | WASM | ✅ | 字幕区 | [设计](./subtitles/) |
| `attachments` | WASM | ✅ | 图片上传 | [设计](./attachments/README.md) |

## 记账与经济

| 插件 | 类型 | 状态 | 说明 | 文档 |
|---|---|:-:|---|---|
| `accounting` | Native | ✅ | 流量记账、倍率、做种快照、反作弊信号 | [设计](./accounting/) |
| `ledger` | Native | ✅ | 多币种账本 | [设计](./ledger/) |
| `bonus` | WASM | ✅ | 魔力值规则与小时结算 | [设计](./bonus/) |
| `seeding-bonus` | WASM | ✅ | 做种魔力公式与官种奖励 | — |
| `promotion` | WASM | ✅ | 单种 / 自动 / 全站促销 | [设计](./promotion/) |
| `shop` | WASM | ✅ | 魔力商城 | [设计](./shop/) |
| `lottery` | WASM | ✅ | 幸运转盘（可验证开奖） | [设计](./lottery/) |
| `medals` | WASM | ✅ | 勋章与头衔 | [设计](./medals/) |
| `donations` | WASM | ✅ | 捐赠与 VIP | [设计](./donations/README.md) |
| `checkin` | WASM | ✅ | 每日签到 | [设计](./checkin/) |

## 规则与考核

| 插件 | 类型 | 状态 | 说明 | 文档 |
|---|---|:-:|---|---|
| `levels` | WASM | ✅ | 用户等级、升降级、VIP 覆盖 | [设计](./levels/) |
| `hnr` | WASM | ✅ | H&R 考核 | [设计](./hnr/) |
| `ratio-policy` | WASM | ✅ | 分享率考核 | [设计](./ratio-policy/) |
| `newbie-exam` | WASM | ✅ | 新手考核 | [设计](./newbie-exam/) |
| `seedbox` | WASM | ✅ | 盒子登记与识别 | [设计](./seedbox/) |
| `claims` | WASM | ✅ | 种子认领与保种奖励 | [设计](./claims/README.md) |
| `anticheat` | WASM | ✅ | 作弊判定、案件、处置与申诉 | [设计](./anticheat/) |

## 社区

| 插件 | 类型 | 状态 | 说明 | 文档 |
|---|---|:-:|---|---|
| `comments` | WASM | ✅ | 评论 | [设计](./comments/) |
| `forum` | WASM | ✅ | 论坛 | [设计](./forum/) |
| `messages` | WASM | ✅ | 站内信与群发公告 | [设计](./messages/) |
| `requests` | WASM | ✅ | 求种悬赏 | [设计](./requests/) |
| `reports` | WASM | ✅ | 举报工单 | [设计](./reports/) |
| `moderation` | WASM | ✅ | 上传审核、候选区、敏感词 | [设计](./moderation/) |
| `tickets` | WASM | ✅ | 联系管理组 | [设计](./tickets/README.md) |
| `bookmarks` | WASM | ✅ | 收藏与合集 | [设计](./bookmarks/README.md) |
| `top10` | WASM | ✅ | 排行榜 | [设计](./top10/README.md) |

## 搜索、订阅与集成

| 插件 | 类型 | 状态 | 说明 | 文档 |
|---|---|:-:|---|---|
| `search-meilisearch` | Native | ✅ | Meilisearch 搜索（`ctx.search`） | [设计](./search-meilisearch/) |
| `rss` | Native | ✅ | RSS 订阅与 Torznab | [设计](./rss/) |
| `recommend` | Native | ✅ | 首页推荐 | — |
| `notify` | WASM | ✅ | 站内通知与邮件 | [设计](./notify/) |
| `webhook` | WASM | 📐 | 站外 Webhook 推送 | [设计](./webhook/) |
| `crossseed-api` | WASM | 📐 | 辅种接口（兼容 IYUU） | [设计](./crossseed-api/) |

## AI

| 插件 | 类型 | 状态 | 说明 | 文档 |
|---|---|:-:|---|---|
| `llm-claude` | Native | ✅ | `ctx.llm`：Anthropic Claude | [设计](./llm-claude/) |
| `llm-openai-compat` | Native | 📐 | `ctx.llm`：OpenAI 兼容接口（vLLM、Ollama 等） | [设计](./llm-openai-compat/) |
| `mcp` | Native | ✅ | MCP 网关与 OAuth | [设计](./mcp/) |
| `ai-assistant` | Native | ✅ | 站内助手 | [设计](./ai-assistant/) |
| `ai-upload-review` | WASM | ✅ | AI 上传预审 | [设计](./ai-upload-review/) |
| `ai-moderation` | WASM | ✅ | AI 内容审核 | [设计](./ai-moderation/) |

## 外观与运维

| 插件 | 类型 | 状态 | 说明 | 文档 |
|---|---|:-:|---|---|
| `theme-anime` | WASM | ✅ | 主题：晴空樱花（默认） | — |
| `theme-xiudaoyuan` | WASM | ✅ | 主题：修道院 · 白玉京 | — |
| `server-load` | Native | ✅ | 首页服务器负载面板 | — |

---

测试用插件 `x-chaos`（`plugins/wasm/x-chaos`）只用于 WASM 沙箱越界测试，不随发布包分发。
