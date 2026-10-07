# 插件清单

每个插件一个目录 `docs/plugins/<plugin-id>/`，包含 01～04 四份文档（模板见 [../process/templates/](../process/templates/)）。

> **以 [../platform/catalog/plugins.md](../platform/catalog/plugins.md) 为准**：插件 ID、文档缩写、类型、提供的服务以及由 ID 派生的名字均以该登记表为准；本表只是概览，二者不一致时以登记表为准。

| 组 | 插件 ID | 类型 | 说明 |
|---|---|---|---|
| G0 平台 | `terms` | Native | 术语表（`ctx.terms`） |
| G1 身份 | `identity` | Native | 用户、会话、2FA（TOTP/WebAuthn）、tracker passkey、RBAC（`ctx.auth`、`ctx.users`、`ctx.rbac`） |
| G2 注册 | `invite` | WASM | 邀请码、邀请树、邀请额度 |
| G2 注册 | `registration-policy` | WASM | 开放注册、答题注册、验证码、邮箱域名白名单、注册名额 |
| G3 种子 | `torrent` | Native | 种子解析/存储/下载、Group、审核状态（`ctx.torrents`） |
| G3 种子 | `tracker-bridge` | Native | 向 pt-tracker 同步 passkey、白名单、用户状态、客户端白名单（`ctx.tracker`） |
| G4 记账 | `accounting` | Native | 流量事件消费、窗口聚合、落库、倍率应用、反作弊基础信号（`ctx.accounting`） |
| G4 记账 | `ledger` | Native | 多币种账本（`ctx.ledger`） |
| G4 记账 | `anticheat` | WASM | 基于信号的作弊判定规则、处置 |
| G5 经济 | `bonus` | WASM | 魔力值 / 做种积分计算 |
| G5 经济 | `shop` | WASM | 商城：兑换上传量、邀请、改名卡、头衔等 |
| G5 经济 | `checkin` | WASM | 签到 |
| G5 经济 | `lottery` | WASM | 彩票 / 竞猜 |
| G5 经济 | `medals` | WASM | 勋章、头衔、徽章 |
| G6 规则 | `levels` | WASM | 用户等级（数据驱动规则）、升降级、保号 |
| G6 规则 | `ratio-policy` | WASM | 分享率要求、警告、限制下载、封禁 |
| G6 规则 | `hnr` | WASM | H&R 考核 |
| G6 规则 | `newbie-exam` | WASM | 新手考核 |
| G6 规则 | `seedbox` | WASM | 盒子识别与规则 |
| G7 促销 | `promotion` | WASM | 单种促销、自动促销、全站活动（通过 `ctx.accounting.set_rate_rule()` 下发流量倍率） |
| G8 搜索 | `search-meilisearch` | Native | `ctx.search` |
| G9 内容 | `type-movie` / `type-tv` / `type-music` / `type-anime` / `type-generic` | WASM | 资源类型插件（`ctx.content_types` 的多实现） |
| G9 内容 | `metadata-tmdb` / `metadata-douban` / `metadata-imdb` / `metadata-musicbrainz` / `metadata-bangumi` / `metadata-igdb` | WASM | 元数据源（`ctx.metadata` 的多实现） |
| G9 内容 | `mediainfo` | WASM | MediaInfo / NFO 解析 |
| G10 社区 | `comments` / `forum` / `messages` / `requests` / `subtitles` / `moderation` / `reports` | WASM | 评论、论坛、站内信、求种悬赏、字幕、候选与审核、举报 |
| G11 集成 | `rss` / `notify` / `crossseed-api` / `webhook` | WASM | RSS、通知（邮件/Telegram/Discord）、辅种接口、Webhook |
| G12 AI | `llm-claude` / `llm-openai-compat` | Native | `ctx.llm` 提供方：Anthropic Claude；OpenAI 兼容接口（含 vLLM、Ollama 等本地模型，不单列 `llm-local`） |
| G12 AI | `mcp` | Native | MCP 网关（`ctx.mcp`，含写操作确认流程） |
| G12 AI | `ai-upload-review` / `ai-moderation` / `ai-assistant` | WASM（ai-assistant 为 Native） | AI 业务插件：上传审核、内容审核、站内助手 |
