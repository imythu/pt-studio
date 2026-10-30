# 功能清单

> 更新于 2026-10-24 · 对应版本 0.5.5 之后的 `main`
>
> 本页按领域说明**已经能用的功能**和**还没做的部分**。每个模块的实现细项见 [dev/implementation-status.md](dev/implementation-status.md)，实现与设计不一致之处见 [dev/design-deviations.md](dev/design-deviations.md)。

图例：✅ 已完成　🟡 主体完成，有后续项　📐 已有设计，尚未实现　💤 暂不做

## 总览

| 领域 | 状态 | 插件 / 组件 |
|---|:-:|---|
| 微内核与插件运行时 | ✅ | `crates/kernel`、`crates/server` |
| 插件 API 与 SDK | ✅ | `pt:plugin@1.0.0`（已冻结）、`pt-plugin-sdk`、模板 |
| Tracker | 🟡 | `pt-tracker`、`tracker-bridge` |
| 账号与安全 | ✅ | `identity`、`captcha`、`registration-policy`、`invite` |
| 种子与资源类型 | ✅ | `torrent`、`type-movie` / `tv` / `anime` / `music` / `generic`、`mediainfo`、`dupe`、`subtitles` |
| 元数据 | 🟡 | `metadata-tmdb`、`metadata-douban`、`metadata-bangumi`、`metadata-musicbrainz` ✅；IMDb、IGDB 📐 |
| 记账与经济 | ✅ | `accounting`、`ledger`、`bonus`、`seeding-bonus`、`promotion`、`shop`、`lottery`、`medals`、`donations`、`checkin` |
| 规则与考核 | ✅ | `levels`、`hnr`、`ratio-policy`、`newbie-exam`、`seedbox`、`claims` |
| 反作弊 | ✅ | `anticheat` |
| 社区 | ✅ | `comments`、`forum`、`messages`、`requests`、`reports`、`moderation`、`tickets`、`bookmarks`、`top10`、`attachments` |
| 搜索与订阅 | ✅ | `search-meilisearch`、`rss`（含 Torznab）、`recommend` |
| 通知 | 🟡 | `notify`：站内与邮件 ✅；Telegram / Discord、Webhook 📐 |
| AI 与 MCP | 🟡 | `llm-claude`、`mcp`、`ai-assistant`、`ai-upload-review`、`ai-moderation` ✅；`llm-openai-compat` 📐 |
| 前端 | ✅ | React 外壳、插槽、Schema 表单、主题（`theme-anime`、`theme-xiudaoyuan`） |
| 部署与运维 | ✅ | 发布包、Docker、systemd、备份恢复、Prometheus 指标、`server-load` |

---

## 已完成

### 平台

- **微内核**：插件发现与依赖解析（版本要求、单实现 / 多实现服务、环检测）、RBAC 与作用域、敏感能力授权、调用上下文与工作单元（每插件独立 PostgreSQL schema 与角色、`SET LOCAL ROLE` 隔离）、钩子管线（超时、fail-closed、可修改字段检查）、outbox 事件（至少一次 + inbox 去重 + 死信）、cron 与延迟任务、分区 worker、多副本租约。
- **HTTP 网关**：会话 / Bearer 鉴权、CSRF、problem+json 错误、GCRA 限流（可按等级放宽）、幂等键、近期重新验证、可信代理。
- **WASM 宿主**：wasmtime 组件模型，每次调用新实例，截止时间、内存上限、并发上限，WASI 无网络 / 无文件；SQL 语句检查防止角色逃逸。
- **出站 HTTP**：主机白名单、SSRF 防护（拒绝私有地址并固定连接地址）、不跟随重定向、大小与超时上限。
- **插件包签名**：minisign 签名与校验，生产模式只加载已签名的包。
- **插件设置**：后台可视化编辑、乐观版本、审计；插件预置配置。
- **插件 API 冻结**：`pt:plugin@1.0.0`，带冻结快照测试；第三方插件可登记自己的钩子点与错误码。
- **开发工具**：`pt-server plugin check / package / keygen`，WASM 与 Native 两套模板，[插件开发指南](sdk/README.md)。

### Tracker

- 自研 HTTP tracker（独立程序，AGPL-3.0-only）：announce / scrape、紧凑与非紧凑响应、IPv4 / IPv6（BEP 7）。
- swarm 与会话状态全部存于 Redis（Lua 原子脚本），**无状态、可多节点**。
- 增量计算（基线、回退、截断、首次完成）、首报 `started` 不计流量、超速截断。
- `tracker-bridge` 同步 passkey、种子、客户端规则与运行参数（全量 + 实时）。
- 客户端规则：有序的放行 / 拒绝规则，按 peer_id 前缀或正则（可限最低 / 最高版本）与 User-Agent 匹配，默认动作可选；后台编辑、预设模板、规则测试，保存后 1 秒内各节点生效；被拒客户端按日排行（保留 8 天），可一键生成放行规则。
- 节点状态与告警：各节点每 10 秒上报，后台「Tracker」页显示版本、同步落后、缓存规模与 announce 速率；节点落后超过 5 秒或停止上报时发 `tracker.inconsistency_detected` 事件并在后台提示。
- 实测：Transmission 4.1、qBittorrent 5.1、Deluge 2.1 双向真实传输与记账正确（Deluge 另验证了拒绝规则与被拒统计）；同机压测约 1.05 万次 / 秒（p99 13 ms）。

### 账号与安全

- 注册 / 登录 / 会话、个人资料、改密码、passkey 重置、会话管理。
- 登录限流（用户软锁、IP 封锁、指数延迟）、已知设备、IP 历史与同 IP 账户查询、安全事件、新设备提醒。
- 邮箱验证、找回密码。
- **两步验证**：TOTP + 恢复码；**WebAuthn** 安全密钥（第二因素与免密码登录）；可按角色强制 2FA。
- 人机验证：Cloudflare Turnstile / hCaptcha / reCAPTCHA v2。
- 注册策略：开放 / 邀请、开放时段与限时窗口、名额、同 IP 上限、邮箱域名黑白名单、注册审核。
- 邀请：邀请码（只存哈希）、额度批次、邀请树、连坐。
- 个人 API token 与 MCP token。

### 种子与内容

- 种子解析（v1 / v2 / hybrid）、规范化、上传、下载（passkey 注入）、编辑、审核状态、软删除、置顶。
- **资源类型插件**驱动的发布表单与作品（Group）页：电影、剧集（季集范围检查）、动漫（字幕组风格发布名）、音乐（艺人与版本、抓轨日志评分）、通用类型（游戏 / 软件 / 电子书 / 课程，后台可自定义）。
- 元数据：TMDB（电影 / 剧集 / 季、海报与背景图转存、翻译标题、过期自动刷新）、豆瓣（直连、代理或 PT-Gen 兼容端点、PT-Gen 版式简介；只有豆瓣编号的电影 / 剧集也能自动补全；图书按豆瓣编号或 ISBN 查询，电子书自动补全封面、简介、作者与出版社）、Bangumi（动漫条目、关联条目；type-anime 优先使用）、MusicBrainz（发行组 / 发行 / 艺人、Cover Art Archive 封面；音乐作品按发行组补全封面、流派与年份）；Bangumi 与 MusicBrainz 为公开 API，无需申请凭证；上游限速与熔断，上游故障时返回缓存。
- MediaInfo / BDInfo 解析与脱敏、与种子声明规格比对。
- 去重与替代（trump）、字幕区、图片上传（去除 EXIF）、种子简介 BBCode 渲染。

### 记账与经济

- 流量记账：微批消费、去重、倍率规则、做种时长快照、结算窗口，重启不重复计入。
- 多币种账本：发行 / 回收 / 转账、冻结与扣款、冲正、流水、后台批量发放 / 扣除。
- 魔力值：安全表达式规则、NexusPHP 预设、小时结算；做种魔力公式与官种奖励、VIP 加成。
- 促销：单种 / 自动规则 / 全站活动 / 个人促销卡。
- 商城（上传量、邀请、促销卡、消除 H&R、VIP、勋章）、勋章与头衔、可验证抽奖（commit-reveal，支持十连抽）、捐赠与 VIP、每日签到。

### 规则与考核

- 等级：NexusPHP 预设、升降级与保级、VIP 等覆盖等级与到期、等级绑定角色。
- H&R：建档、分层评估、预警、限制下载与自动解除、免除。
- 分享率考核、新手考核、盒子登记与盒子 peer 标记、种子认领（月结奖励）。

### 反作弊

- 记账信号（速度、负增量、计数异常、无对端、客户端指纹，盒子来源标记）与每小时交叉校验。
- 规则集 DSL、评分与衰减、案件工作台、复核与四眼封禁、自动处置与熔断、申诉、回测、种子视角、同 IP 对端关联（需授权 `peer.ip`）、多账号关联（同网段与共用登录 IP 的账号簇、复核同一人 / 无关，同一人计分）。
- 处置：警告、冻结流量、锁钱包、回收流量、重置 passkey、禁止下载、封禁。

### 社区

- 评论（楼层、引用、编辑历史、防刷）、论坛（版块权限、投票、精华、订阅、已读）、站内信与群发公告、求种悬赏、举报工单、上传审核（规则、队列、候选区投票、敏感词）、联系管理组、收藏与合集、排行榜。
- 安全的 Markdown / BBCode 渲染（`pt-markup`）与前端二次净化。

### 搜索、订阅与推荐

- Meilisearch 搜索：插件贡献字段、增量同步、无停机重建、可见性强制过滤、facet、联想、结果缓存。
- RSS 订阅与 Torznab 接口、凭 feed key 下载。
- 首页推荐（按做种 / 下载历史、热度、缺种情况）。

### 通知

- 站内通知中心（类别 × 渠道偏好）。
- 邮件渠道：http_relay、Postmark、SendGrid、Resend，一键退订。

### AI 与 MCP

- `ctx.llm`（Anthropic Claude）：提示缓存、不可信内容隔离、结构化输出、重试、脱敏、按插件 / 用户的每日配额与用量统计。
- MCP 网关：两个协议版本、OAuth（动态注册、PKCE、刷新轮换、吊销）、个人 MCP token、写操作站内确认。
- 站内助手（规则问答、工具调用、引用）、AI 上传预审、AI 内容审核（本地规则 + 模型分类）。

### 前端

- 插槽机制（import map 共享依赖、懒加载、错误隔离）、JSON Schema 表单（无 eval 校验，严格 CSP）、术语层（站点可改叫法）。
- 个人设置、后台用户 / 种子 / 插件管理、各插件后台页、Markdown 编辑器、移动端适配。
- 可插拔主题：晴空樱花（theme-anime）、修道院 · 白玉京（theme-xiudaoyuan），主题可覆盖术语；站点名与字标后台可改。

### 部署与运维

- 一键本机运行（`deploy/dev/run-site.sh`）、Linux x86_64 发布包与 [部署指南](../deploy/release/DEPLOY.md)、Docker 镜像与 compose。
- 后台操作日志、Prometheus 指标（站点与 tracker）、备份 / 恢复脚本、服务器负载面板。

---

## 未完成

### 尚未实现的插件

已有完整设计文档（`docs/plugins/<id>/`），等待实现：

| 插件 | 作用 |
|---|---|
| [`terms`](plugins/terms/) | 术语服务：后台修改站点术语（目前术语来自插件清单与主题覆盖） |
| [`webhook`](plugins/webhook/) | 站外 Webhook 推送 |
| [`crossseed-api`](plugins/crossseed-api/) | 辅种接口（兼容 IYUU） |
| [`llm-openai-compat`](plugins/llm-openai-compat/) | OpenAI 兼容的大模型接入（vLLM、Ollama 等本地模型） |
| [`metadata-imdb`](plugins/metadata-imdb/) / [`metadata-igdb`](plugins/metadata-igdb/) | 其他元数据源（IGDB 需要 Twitch 开发者凭证） |

### 平台与内核

- 钩子输入按 schema 运行时校验；插件包内分发前端（目前前端需放进站点的 web 目录）。
- OpenAPI 聚合与 TS SDK 生成、CORS、前端错误上报。
- 插件配额（KV / Redis / blob）、S3 对象存储后端、分区维护任务化。
- WASM 组件编译缓存与实例池。
- 事件回放、按类型的保留期清理、手动运行任务的 HTTP / CLI 入口；延迟任务并发执行。
- 平台级的出站 HTTP 速率限制与熔断（目前由插件用 SDK 的 `upstream` 模块自行处理）、站点级代理。
- 其他语言的 SDK（JS / Python）、类型化事件 / 钩子结构；模板仓库独立发布。

### Tracker

- UDP（💤 设计上只做 HTTP(S)，UDP 无法带 passkey 鉴权）；请求头检查（要求 UA、拒绝浏览器 UA、异常请求头、端口黑名单）。
- 节点告警只发事件与指标，不直接通知管理员（可用 webhook 插件转发事件）。
- 限制类变化（如 H&R 限制下载）的实时同步，目前靠每 10 分钟全量同步兜底。
- tracker 与 Redis 分开部署的压测（目标平均 1.7 万次 / 秒）；µTorrent 实测（只有 Windows / macOS 版本）。

### 各功能的后续项

- **账号**：改名、改邮箱、UTS #39 混淆字符检查、GeoIP。
- **种子与资源类型**：元数据字段锁定、缺集列表、艺人页、抓轨日志签名验证、NFO 解析、批量下载。
- **搜索**：语义 / 混合搜索、多实例、同义词管理、一致性巡检。
- **记账与经济**：日对账、月结余快照、归档、重算；魔力规则的编辑与回放；改名卡、个人促销、竞猜。
- **规则**：等级 / H&R / 邀请规则的编辑接口与模拟评估、批量操作、盒子 ASN 自动识别、盒子接入魔力规则。
- **反作弊**：邀请链、转账往来并入多账号关联的账号图，详见 [anticheat 后续工作](plugins/anticheat/05-后续工作.md)。
- **社区**：表情、@提醒通知、合并 / 拆分主题、批量审核、通配符敏感词。
- **通知**：Telegram / Discord 渠道、合并通知、服务端模板渲染、mailgun / SES。
- **AI**：上传预审接入 MediaInfo 比对与疑似重复、助手的检索增强与流式输出、MCP 工具目录缓存与按 token 限流。
- **前端**：外部 ID 自动拉取、长列表虚拟滚动、语言包按需加载。
- **运维**：多节点部署指南、同域名按路径分流 tracker 的 nginx 写法、日志采集与告警。

### 暂不做

- 周边工具对接：IYUU 辅种（见 `crossseed-api`）、MoviePilot / PT-depiler 等工具的长期 token 接口、IRC 播报。
- 多租户（一套部署对应一个站点，决策 D6）。

### 待验证

大规模压测（记账链路、Meilisearch 百万级文档）、连接池（含 pgbouncer）下的 SQL 隔离、MCP 客户端兼容性矩阵、LLM 离线评测集等，见 [路线图 §2](design/09-roadmap.md#2-待验证项)。
