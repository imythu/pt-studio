# 更新日志

版本号遵循 [语义化版本](https://semver.org/lang/zh-CN/)。插件 API `pt:plugin@1` 自 0.1.0 起冻结，1.x 内只做向后兼容的新增。

发布包与 Docker 镜像见 [Releases](https://github.com/imythu/pt-studio/releases)。

## 未发布

- 后台批量发放 / 扣除魔力：按状态、等级或用户名单，预览后确认，后台分批记账，可逐人发站内信
- 魔力抽奖支持十连抽；抽奖、签到、购买后顶栏魔力值与签到提示即时刷新
- 等级预设对齐 NexusPHP 默认值（升级看下载量、注册周数 4 / 8 / 15 / 25 / 40 / 60 / 80 / 100 等）；等级显示常用简写（PU、EU……）
- VIP 等覆盖等级显示有效期（个人页、等级说明页、首页与侧栏用户卡片；新增 `GET /v1/users/{id}/class`）
- 新增主题「修道院 · 白玉京」；主题插件可覆盖术语
- 站点名与字标改为后台可改（外观 → 站点信息），插件与前端即时生效
- 反作弊：处置新增“重置 passkey”（复核时可选，旧种子文件立即失效并通知用户）；identity 新增服务方法 `ctx.users.reset_passkey`（作用域 `write:passkey`，幂等）
- 盒子接入反作弊与 H&R：记账信号记录是否来自已批准的盒子网段，反作弊超速等证据按默认规则减半；H&R 豁免规则可用 `is_seedbox`
- 敏感操作统一重新验证：任何页面（含插件页）遇到“需要重新验证”时弹出密码 / 两步验证码对话框，验证后自动重试原操作；反作弊的处理案件、批准封禁、复核申诉、激活规则集要求 15 分钟内验证过
- 客户端指纹信号（默认关闭，`[plugin.accounting.signals] client_fingerprint_check = true` 开启）：peer_id 与 User-Agent 不属同一客户端、同一 peer 中途换客户端时产生信号，反作弊按 `client_spoof` / `client_changed` 规则计分；建议先把这两条规则设为影子模式观察误判
- 反作弊新增“种子”标签页（种子视角）：列出交叉校验发现上传明显多于下载的种子与被归因用户，复核后可确认基线重新计数
- 反作弊多账号关联：同网段对端证据连成账号簇，后台新增“关联账号”标签页，用户详情与案件工作台显示关联账号；复核员可把账号对标记为“同一人”或“无关”（如校园网 / 公司 NAT），无关的账号对之后不再产生同网段证据
- 反作弊同 IP 对端关联：tracker-bridge 新增 `ctx.tracker.peers`（读取 tracker 的在线 peer）；站点在 `[grants.anticheat] sensitive = ["peer.ip"]` 授权后，案件补全时发现与其他用户同网段（IPv4 /24、IPv6 /48）会写入证据（默认影子规则，不计分）；IP 不落库
- Tracker 节点状态：pt-tracker 每 10 秒上报版本、同步位置与延迟、缓存规模、announce 速率；后台新增「Tracker」页（tracker-bridge），节点落后、announce 间隔不小于节点 peer 超时或没有在线节点时提示
- Tracker 客户端规则：后台「Tracker → 客户端规则」按顺序编辑放行 / 拒绝规则（peer_id 前缀或正则，可限最低 / 最高版本；User-Agent 前缀或正则），默认动作可选，内置 qBittorrent、Transmission、Deluge、rTorrent、迅雷等预设，可对未保存的草稿测试任意 peer_id / UA；保存后 1 秒内各 tracker 节点生效。原设置里的客户端白名单在升级后自动迁移为规则
- 被拒客户端统计：pt-tracker 记录因规则被拒的客户端（peer_id 前缀 + UA），后台按日排行（保留 8 天），可一键生成放行规则
- Tracker 节点告警：每分钟检查节点，同步落后超过 5 秒或停止上报 2 分钟以上时发 `tracker.inconsistency_detected` 事件，后台节点页提示失联节点
- 实测 Deluge 2.1（libtorrent 2.0）双向传输与记账正确，并验证拒绝规则即时生效（`web/tests/bt-deluge.mjs`）
- 元数据：TMDB 与豆瓣加入限速与熔断（上游故障时返回缓存、不再反复请求），每小时自动刷新近期用过的过期条目；TMDB 增加翻译标题与背景图，近期上映或连载中的条目缓存 7 天；只有豆瓣编号的电影 / 剧集发布后也会自动补全海报、简介与评分
- 豆瓣图书：按豆瓣图书编号、图书链接或 ISBN 查询（直连或自建代理），PT-Gen 版式的图书简介；通用类型的电子书新增“豆瓣”字段，发布后按豆瓣编号或 ISBN 自动补全封面、简介、评分、作者与出版社
- 新增 Bangumi 元数据源（metadata-bangumi）：条目查询、搜索、关联条目，封面转存；动漫发布填了 Bangumi 编号时优先用它补全中文名、封面、简介、话数与评分
- 新增 MusicBrainz 元数据源（metadata-musicbrainz）：发行组 / 发行 / 艺人查询、发行组搜索、Cover Art Archive 封面；音乐作品填了发行组编号时自动补全封面、流派、原始年份与发行类型
- 插件 SDK：新增 `upstream` 模块（外部 API 限速与熔断）
- 修复：反作弊案件补全（swarm 快照）一直失败（查询返回了宿主不支持的 NUMERIC 列）
- 修复：后台用户详情页签标题显示为术语键；修道院主题下描边等级标签看不见

## 0.5.5 · 2026-10-21

- 魔力值页：「获取与使用」规则说明（各插件经 `bonus.explain` 插槽补充）与分页明细
- 服务器负载插件（Native）：用户数 / 上限、活跃与连接数、CPU 各核心、内存 / 交换区、磁盘 IO、进程占用，可限定可见范围
- VIP 做种魔力加成生效；做种积分作为等级升级条件
- 背景壁纸；插件前端资源 URL 带内容哈希，修复升级后浏览器仍用旧缓存

## 0.5.0 – 0.5.2 · 2026-10-20

- 动漫风主题（theme-anime），前端主题可插拔
- 做种魔力插件（seeding-bonus）：NexusPHP 式公式、官种奖励、估算器
- TMDB / 豆瓣元数据接入完善；插件预置配置
- 魔力值按币种小数位记账；个人页数据可点击进入详情

## 0.4.0 – 0.4.1 · 2026-10-20

- 全新 PT 站前端；首页推荐（recommend）与列表海报、内置头像、站点图标
- 抽奖改为转盘；永久 VIP；管理员可查看同伴 IP

## 0.3.0 – 0.3.1 · 2026-10-18

- 补齐已有接口对应的插件界面
- notify 新增 Resend 邮件提供方

## 0.2.0 · 2026-10-18

开站计划 M1–M4 全部完成：

- **账号安全**：登录限流与 IP 记录、邮箱验证与找回密码、TOTP 两步验证、WebAuthn 安全密钥、人机验证、注册策略
- **稳定开站**：插件包签名（minisign）、qBittorrent 实测、分享率考核、盒子登记、新手考核、操作日志、Prometheus 指标、备份恢复
- **内容**：种子认领、论坛、豆瓣与 PT-Gen 简介、MediaInfo / BDInfo、置顶 / 收藏 / 合集 / 排行榜、图片上传、字幕区、去重与替代
- **运营**：魔力商城、勋章与头衔、可验证抽奖、工单与群发公告、捐赠与 VIP、魔力流水
- **管理**：插件后台页面、Markdown 编辑器、移动端适配
- **反作弊**：accounting 信号与交叉校验、anticheat 插件（证据、规则评分、案件、处置、申诉、回测）

## 0.1.0 · 2026-10-13

首个版本：微内核、WASM 宿主、插件 API `pt:plugin@1.0.0`、Rust SDK 与模板、自研 pt-tracker，以及身份、种子、记账、魔力、等级、邀请、H&R、促销、搜索、资源类型、元数据、评论、举报、审核、通知、站内信、签到、求种、RSS / Torznab、LLM、MCP、站内助手等插件和 React 前端。

---

## 开发里程碑

逐日的开发记录，按阶段（P1–P8、M1–M4）组织。阶段的含义见 [路线图](docs/design/09-roadmap.md)。

| 日期 | 内容 |
|---|---|
| 2026-10-06 | P1：内核骨架、identity、torrent |
| 2026-10-07 | P2：ledger、accounting、分区 worker、插件分区维护、网关限流与幂等键 |
| 2026-10-07 | cron、accounting 小时快照、bonus 小时结算与批量入账（WASM） |
| 2026-10-07 | P3：WASM 宿主、`pt:plugin@1` WIT、JSON 分派服务、bonus（WASM 试点）、沙箱越界测试插件 x-chaos |
| 2026-10-07 | P4：promotion（WASM）、levels（WASM）、identity 等级与等级角色 |
| 2026-10-07 | P4：search-meilisearch（Native）、内核搜索贡献者调用入口、torrent 搜索贡献 |
| 2026-10-07 | P4：invite（WASM）、identity 限制与 `user.status_changed` |
| 2026-10-07 | P4：hnr（WASM）；P4 计划中的插件全部完成 |
| 2026-10-12 | P5：Web 前端（sdk / terms / slots / ui-kit / schema-form、外壳与页面、5 个插件前端）、内核前端支持、浏览器端到端测试 |
| 2026-10-12 | P5：资源类型（内核 content-type 聚合、type-movie、type-generic、发布名与模板共享 crate）、torrent 分组与类型接口、类型驱动的发布表单与作品页 |
| 2026-10-12 | P5：个人设置与后台用户管理（identity 接口 + 页面）、种子编辑 / 审核 / 删除（torrent 接口 + 页面） |
| 2026-10-12 | 内核出站 HTTP（SSRF 防护）与 `ctx.metadata`、metadata-tmdb、type-movie 元数据联动、`ctx.torrents`、作品页元数据与 TMDB 预填 |
| 2026-10-12 | type-tv、type-anime（季集与范围、按季分组、上传时范围检查、字幕组风格发布名、剧与季元数据）、`pt-type-kit` 规格 / 集号 / 元数据模块 |
| 2026-10-12 | type-music（艺人与版本、格式规则、文件检查、抓轨日志评分与预检）、`ctx.torrents.update_fields` |
| 2026-10-12 | P6：llm-claude（`ctx.llm`）、ai-upload-review、torrent `v_torrent_review`、type-music log 存储上限修正 |
| 2026-10-12 | P6：MCP 网关（mcp、内核工具注册与根路由、identity 个人 / MCP token、首批工具）|
| 2026-10-12 | P6：MCP OAuth（动态注册、授权码 + PKCE、刷新轮换、吊销）与 mcp 前端（AI 客户端设置、同意页、待确认页）|
| 2026-10-12 | P6：ai-assistant（规则问答、工具调用、引用、会话、MCP `ask_site_help`）、MCP 工具 `get_my_stats` / `get_my_bonus` / `list_my_hnr`|
| 2026-10-13 | 社区：pt-markup（Markdown / BBCode 安全渲染）、comments、reports、评论区与举报前端 |
| 2026-10-13 | 审核：moderation（初始状态规则、队列、决定、候选区、敏感词）、ai-moderation（本地规则 + 模型分类 → 系统举报）、审核前端 |
| 2026-10-13 | 通知与站内信：notify（通知中心、偏好，评论 / 审核 / 举报接入）、messages（会话、拉黑、举报私信、MCP）、前端 |
| 2026-10-13 | 签到（checkin）与求种（requests：悬赏托管、应求、确认发放、退款）、前端 |
| 2026-10-13 | RSS 与 Torznab（rss：feed key、订阅、凭 key 下载）、通知的邮件渠道（偏好、投递、退订）、前端 |
| 2026-10-14 | P7：插件 API 冻结为 `pt:plugin@1.0.0`、Rust SDK（全部 WASM 插件改用）、第三方钩子点与错误码、`pt-server plugin check/package`、WASM / Native 模板、开发指南、README、`llms.txt` |
| 2026-10-14 | P8：一键运行站点、Docker 部署文件、首个管理员命令、浏览器冒烟检查；tracker-bridge 与 pt-tracker（announce → 记账 → 个人页流量全链路打通）；界面与搜索缓存修复 |
| 2026-10-14 | pt-tracker 的 swarm 改存 Redis（无状态、多节点，D20 取代 D9 / D14）、Redis 集成测试、压测工具、Transmission 真实传输测试；Linux x86_64 发布包（`deploy/release/build-release.sh`）与部署指南（`deploy/release/DEPLOY.md`），解压安装后冒烟检查与真实传输均通过 |
| 2026-10-14 | 默认端口改为 31251（站点）/ 31252（tracker）；pt-server 与 pt-tracker 处理 SIGTERM（systemd / docker 重启时释放集群租约，重启后同步延迟从约 10 秒降到 < 100 ms）；GitHub Actions 构建发布包（`release.yml`，ubuntu-22.04）与 Docker 镜像（`docker.yml`，ghcr.io）；roadmap §4 汇总未完成与待做 |
| 2026-10-16 | 反作弊：accounting 基础信号与交叉校验、anticheat（WASM，证据 / 规则评分 / 案件 / 处置 / 申诉 / 回测）、前端；pt-tracker 首报 started 不计、超速截断、BEP 7 IPv6 地址 |
| 2026-10-17 | 开站计划（roadmap §5）；M1.1 登录限流、设备、IP 记录与安全事件、新设备提醒、前端安全记录与后台 IP 查询 |
| 2026-10-17 | M1.2 邮箱验证与找回密码（identity 挑战 + notify 投递）、前端页面 |
| 2026-10-17 | M1.3 TOTP 两步验证、恢复码、登录第二步、近期重新验证、按角色强制 2FA；内核注入 `_site`；前端 |
| 2026-10-17 | M1.4 WebAuthn：安全密钥作为第二因素与免密码登录、前端 |
| 2026-10-17 | M1.5 人机验证（captcha 插件、`ctx.captcha`、插件声明 CSP 来源） |
| 2026-10-17 | M1.6 注册策略（registration-policy）、identity 注册审核限制、前端；M1 账号安全完成 |
| 2026-10-17 | M2.1 插件包签名（minisign：keygen / package --sign / check --key、加载时校验、发布构建接入） |
| 2026-10-17 | M2.2 qBittorrent 5.1 兼容性实测（双向真实传输与记账，`web/tests/bt-qbittorrent.mjs`）；结论：tracker 无需修改 |
| 2026-10-17 | M2.3 分享率考核（ratio-policy）、前端 |
| 2026-10-17 | M2.4 盒子登记（seedbox）与 accounting 盒子标记、前端 |
| 2026-10-17 | M2.5 新手考核（newbie-exam）、前端 |
| 2026-10-17 | M2.6 运维：后台操作日志（`kernel.audit.read`、筛选与游标）、Prometheus 指标（站点与 tracker，独立 `metrics_bind`）、备份 / 恢复脚本与演练 |
| 2026-10-17 | M3.1 种子认领（claims）、前端 |
| 2026-10-17 | M3.2 论坛（forum）、前端 |
| 2026-10-17 | M3.3 豆瓣元数据与 PT-Gen 版式简介（metadata-douban）、发布页回填 |
| 2026-10-17 | M3.4 MediaInfo / BDInfo 解析（mediainfo）、发布页预览与种子页规格 |
| 2026-10-17 | M3.5 置顶（torrent）、收藏与合集（bookmarks）、排行榜（top10）、前端 |
| 2026-10-17 | M3.6 图片上传（attachments）、种子简介 BBCode 渲染 |
| 2026-10-17 | M3.7 字幕区（subtitles）、前端 |
| 2026-10-17 | M3.8 去重与替代（dupe）、前端；M3 完成 |
| 2026-10-17 | M4.1 魔力值商城（shop）、前端 |
| 2026-10-17 | M4.2 勋章与头衔（medals）、shop 出售勋章、前端 |
| 2026-10-17 | M4.2 抽奖（lottery，可验证开奖）、前端 |
| 2026-10-17 | M4.3 联系管理组（tickets）、群发公告（messages）、前端 |
| 2026-10-17 | M4.4 捐赠（donations）、VIP 授予与到期（levels）、商城 VIP 商品、魔力值流水页 |
| 2026-10-17 | M4.5 插件后台页面、Markdown 编辑器与预览接口、移动端适配；开站计划 M1–M4 全部完成 |
