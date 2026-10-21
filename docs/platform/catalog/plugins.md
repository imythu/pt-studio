# 插件登记表

> 版本 0.1 · 2026-10-04 · 所有其他目录（服务、事件、钩子、权限点、术语、插槽、MCP 工具、Redis 前缀、错误码）都以这里的插件 ID 为准。

## 1. 派生名称规则

给定插件 ID `<id>`（如 `tracker-bridge`），以下名称**自动派生，不需要另行申请**：

| 名称 | 规则 | 例子（`tracker-bridge`） |
|---|---|---|
| 文档目录 | `docs/plugins/<id>/` | `docs/plugins/tracker-bridge/` |
| 文档编号缩写 | 见下表“缩写”列 | `TRB` → `FR-TRB-001` |
| PostgreSQL schema | `<id>` 把 `-` 换成 `_` | `tracker_bridge` |
| PostgreSQL 角色 | `<role_prefix>plugin_<schema>`，`role_prefix` 默认 `pt_` | `pt_plugin_tracker_bridge` |
| Redis 私有前缀 | `p:<id>:` | `p:tracker-bridge:` |
| 对象存储前缀 | `plugins/<id>/` | `plugins/tracker-bridge/` |
| HTTP 路由前缀 | `/api/p/<id>/v<N>/` | `/api/p/tracker-bridge/v1/` |
| 前端页面路由 | `/p/<id>/` | `/p/tracker-bridge/` |
| 错误码前缀 | `<id>.` | `tracker-bridge.rebuild_in_progress` |
| 权限点 / 术语键前缀 | `<id>.` | `tracker-bridge.clients.manage` |
| Rust crate | `pt-plugin-<id>` | `pt-plugin-tracker-bridge` |
| 指标前缀 | `pt_plugin_<schema>_` | `pt_plugin_tracker_bridge_` |

## 2. 插件清单

| 组 | 插件 ID | 缩写 | 类型 | 提供的服务 | 说明 |
|---|---|---|---|---|---|
| G0 平台 | `kernel`（保留，不是插件） | KRN | 内核 | 见 services.md §1 | 内核本身 |
| G0 平台 | `terms` | TERMS | Native | `ctx.terms` | 术语表（design/01 §7 已列出，plugins/README 未列，见平台 README 问题 Q3） |
| G1 身份 | `identity` | IDN | Native | `ctx.auth`、`ctx.users`、`ctx.rbac` | 用户、会话、2FA、WebAuthn、tracker passkey、RBAC |
| G2 注册 | `invite` | INV | WASM | `ctx.invites` | 邀请码、邀请树、邀请额度 |
| G2 注册 | `registration-policy` | REGP | WASM | `ctx.registration` | 开放注册、答题、验证码、邮箱域名白名单、名额 |
| G3 种子 | `torrent` | TOR | Native | `ctx.torrents` | 种子、Group、审核状态、下载 |
| G3 种子 | `tracker-bridge` | TRB | Native | `ctx.tracker` | 向 pt-tracker 同步 passkey / 白名单 / 用户状态 / 客户端规则 |
| G4 记账 | `accounting` | ACC | Native | `ctx.accounting` | 流量消费、窗口聚合、落库、倍率、基础信号 |
| G4 记账 | `ledger` | LED | Native | `ctx.ledger` | 多币种账本 |
| G4 记账 | `anticheat` | ACH | WASM | `ctx.anticheat` | 作弊判定与处置 |
| G5 经济 | `bonus` | BONUS | WASM | `ctx.bonus` | 魔力值 / 做种积分 |
| G5 经济 | `seeding-bonus` | SBONUS | WASM | `ctx.seeding_bonus` | 做种魔力公式参数、官种奖励、计算说明页 |
| G5 经济 | `shop` | SHOP | WASM | `ctx.shop` | 商城 |
| G5 经济 | `checkin` | CHKIN | WASM | `ctx.checkin` | 签到 |
| G5 经济 | `lottery` | LOT | WASM | `ctx.lottery` | 幸运转盘（魔力值、上传量、VIP 天数、永久 VIP） |
| G5 经济 | `medals` | MEDAL | WASM | `ctx.medals` | 勋章、头衔、徽章 |
| G6 规则 | `levels` | LVL | WASM | `ctx.levels` | 用户等级、升降级、保号 |
| G6 规则 | `ratio-policy` | RATIO | WASM | `ctx.ratio` | 分享率要求、警告、限制 |
| G6 规则 | `hnr` | HNR | WASM | `ctx.hnr` | H&R 考核 |
| G6 规则 | `newbie-exam` | EXAM | WASM | `ctx.exams` | 新手考核 |
| G6 规则 | `seedbox` | SBOX | WASM | `ctx.seedbox` | 盒子识别与规则 |
| G7 促销 | `promotion` | PROMO | WASM | `ctx.promotions` | 单种 / 自动 / 全站促销 |
| G8 搜索 | `search-meilisearch` | SRCH | Native | `ctx.search` | 搜索 |
| G9 内容 | `type-movie` | TMOV | WASM | `ctx.content_types`（多实现之一） | 电影 |
| G9 内容 | `type-tv` | TTV | WASM | `ctx.content_types` | 剧集 |
| G9 内容 | `type-music` | TMUS | WASM | `ctx.content_types` | 音乐 |
| G9 内容 | `type-anime` | TANI | WASM | `ctx.content_types` | 动漫 |
| G9 内容 | `type-generic` | TGEN | WASM | `ctx.content_types` | 通用（游戏、软件、电子书、课程等未单独建插件的类型） |
| G9 内容 | `metadata-tmdb` | MDTMDB | WASM | `ctx.metadata`（多实现之一） | TMDB |
| G9 内容 | `metadata-douban` | MDDOUBAN | WASM | `ctx.metadata` | 豆瓣 |
| G9 内容 | `metadata-imdb` | MDIMDB | WASM | `ctx.metadata` | IMDb |
| G9 内容 | `metadata-musicbrainz` | MDMB | WASM | `ctx.metadata` | MusicBrainz |
| G9 内容 | `metadata-bangumi` | MDBGM | WASM | `ctx.metadata` | Bangumi |
| G9 内容 | `metadata-igdb` | MDIGDB | WASM | `ctx.metadata` | IGDB |
| G9 内容 | `mediainfo` | MINFO | WASM | `ctx.mediainfo` | MediaInfo / NFO 解析 |
| G10 社区 | `comments` | CMT | WASM | `ctx.comments` | 评论 |
| G10 社区 | `forum` | FORUM | WASM | `ctx.forum` | 论坛 |
| G10 社区 | `messages` | MSG | WASM | `ctx.messages` | 站内信 |
| G10 社区 | `requests` | REQ | WASM | `ctx.requests` | 求种 / 悬赏 |
| G10 社区 | `subtitles` | SUB | WASM | `ctx.subtitles` | 字幕 |
| G10 社区 | `moderation` | MOD | WASM | `ctx.moderation` | 候选区与审核 |
| G10 社区 | `reports` | RPT | WASM | `ctx.reports` | 举报 |
| G10 社区 | `server-load` | SLOAD | Native | — | 首页「服务器负载」：注册用户数 / 用户上限（默认取 `ctx.registration` 的全站上限）、今日新注册、近一小时活跃；主机负载（读 `/proc`）：CPU 各核心使用率与 iowait、内存 / 交换区、各磁盘读写吞吐与繁忙度、本进程（pt-server）CPU / 内存 / 线程、负载均值；可限定可查看的等级或角色（满足其一） |
| G11 集成 | `rss` | RSS | WASM | — | RSS 订阅 |
| G11 集成 | `notify` | NOTIFY | WASM | `ctx.notify` | 通知（邮件 / Telegram / Discord） |
| G11 集成 | `crossseed-api` | XSEED | WASM | — | 辅种接口（IYUU 兼容等） |
| G11 集成 | `webhook` | WHOOK | WASM | — | Webhook 出站 |
| G12 AI | `llm-claude` | LLMC | Native | `ctx.llm`（单实现候选） | Anthropic Claude |
| G12 AI | `llm-openai-compat` | LLMO | Native | `ctx.llm`（单实现候选） | OpenAI 兼容接口（含 vLLM、Ollama 等本地模型） |
| G12 AI | `mcp` | MCP | Native | `ctx.mcp` | MCP 网关 |
| G12 AI | `ai-upload-review` | AIUR | WASM | `ctx.ai_upload_review` | 上传 AI 审核 |
| G12 AI | `ai-moderation` | AIMOD | WASM | `ctx.ai_moderation` | 内容 AI 审核 |
| G12 AI | `ai-assistant` | AIAST | Native | — | 站内助手 / 客服机器人 |
| G13 外观 | `theme-anime` | THANI | WASM | — | 动漫风站点主题（晴空樱花 / 星空夜），默认主题 |

## 3. 新增插件

新插件须先在本表登记 ID 与缩写，再按 [../../process/README.md](../../process/README.md) 编写四份文档。第三方插件建议使用 `x-<vendor>-<name>` 形式的 ID，避免与官方插件冲突。
