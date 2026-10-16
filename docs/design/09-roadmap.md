# 09 · 路线图、待验证项与待决问题

## 1. 路线图

| 阶段 | 目标 |
|---|---|
| P0 验证 | 用 torrust 各包组装 pt-tracker，打通 passkey 鉴权 → 增量计算 → Redis Streams 链路并压测；验证 Meilisearch（见下） |
| P1 内核骨架 | 服务注册表、插件加载、Hook 管线、事件日志；跑通 identity、torrent 两个 Native 插件 |
| P2 记账 | accounting、ledger 插件；窗口聚合、批量落库、幂等 |
| P3 WASM 宿主 | 接入 wasmtime + WIT v0.1；用魔力值、促销、等级三个插件检验插件 API |
| P4 补全扩展点 | 邀请、H&R、资源类型插件、search-meilisearch；根据实现中的问题回头修改 API |
| P5 前端 | 插槽机制、Schema 驱动表单、插件前端加载、术语表 |
| P6 AI | ctx.llm、MCP 网关、AI 插件 |
| P7 冻结 API | WIT 定为 v1.0，发布插件 SDK、模板仓库、文档和 `llms.txt` |
| P8 可运行站点 | 一键运行脚本与部署示例、首个管理员、pt-tracker（HTTP announce、passkey、流量事件）接入，完整走通“注册 → 上传 → 下载 → 做种 → 记账” |
| 未来 | 插件包签名（minisign：`plugin.sig` 签 `checksums.txt`，站点配置信任公钥，加载时校验签名与文件哈希；`pt-server plugin keygen` / `package --sign`。当前只检查文件是否存在，生产部署前必须完成）；钩子输入按 schema 运行时校验；插件包内分发前端；模板仓库独立发布 |

> 进度（2026-10-12）：P1 内核骨架完成（identity、torrent）；P2 记账完成主体（ledger、accounting、分区 worker、网关限流与幂等键）；P3 WASM 宿主跑通（bonus 以 WASM 实现，经 JSON 分派与 Native 服务互调；截止时间、内存、陷入、隔离均有测试）；内核 cron、accounting 小时快照与 bonus 小时结算 / 批量入账打通（tracker 流 → 魔力值到账）；促销（promotion）与等级（levels）以 WASM 实现完成，P3 计划的三个检验插件全部落地（倍率经 accounting 生效、等级经 identity 生效并绑定角色）。P4：search-meilisearch 完成主体（贡献者合并、增量同步、swap 重建、可见性强制查询），对 Meilisearch 1.54 实测通过。invite 完成主体（签发、兑换、额度、邀请树、连坐），identity 增加限制与有效状态。hnr 完成主体（建档、分层评估、限制联动、免除）；P4 计划的插件（邀请、H&R、搜索）全部落地，资源类型插件留到前端阶段与插槽一起做。P5：前端外壳、插槽机制（import map 共享依赖、懒加载、错误隔离）、Schema 表单、术语层、插件前端（levels、bonus、hnr、promotion、invite）完成，真实浏览器端到端测试通过；资源类型（type-movie、type-generic）与作品页已完成，发布表单由类型 schema 生成；个人设置页与后台用户 / 种子管理已完成；元数据（内核出站 HTTP、metadata-tmdb、电影 Group 自动补全）已完成；剧集（type-tv）与动漫（type-anime）已完成（季集与范围、按季分组、上传时按文件列表检查范围、字幕组风格发布名）；音乐（type-music）已完成（艺人与版本、格式规则、文件检查、抓轨日志评分）；其他元数据源（MusicBrainz、Bangumi 等）与去重 / 替代规则待做。P6：`ctx.llm`（llm-claude）与 AI 上传预审（ai-upload-review）已完成；MCP 网关已完成（个人 MCP token 与 OAuth 授权、工具注册、两个协议版本、写操作站内确认、前端设置与确认页），站内助手（ai-assistant，含 MCP `ask_site_help`）已完成；社区的评论（comments）与举报（reports）已完成；审核（moderation：上传规则、队列、候选区、敏感词）与 AI 内容审核（ai-moderation）已完成；P6 计划的 AI 插件全部落地。站内通知（notify）与站内信（messages）已完成，外部通知渠道待接入。签到（checkin）与求种（requests）已完成。RSS 与 Torznab（rss）、通知的邮件渠道已完成。P7：插件 API 已冻结为 `pt:plugin@1.0.0`，Rust SDK（`pt-plugin-sdk`）、插件模板（`templates/`）、开发指南（[../sdk/README.md](../sdk/README.md)）与 `llms.txt` 已完成；包签名、模板仓库独立发布留到开源发布前。P8：一键运行脚本（`deploy/dev/run-site.sh`）、Docker 部署文件、tracker-bridge 与 pt-tracker 已完成，“注册 → 上传 → 下载 → announce → 记账 → 个人页流量”全链路在浏览器冒烟检查中走通。详见 [../dev/README.md](../dev/README.md) §4。

## 2. 待验证项

- [x] **Torrust 集成方案**：已确定采用方案 B′（见 [10-torrust-integration.md](10-torrust-integration.md)、决策 D14～D17）；原型已跑通 passkey 鉴权、过滤、增量计算、事件输出。
- [ ] **pt-tracker 未验证项**（P8 已验证：passkey 鉴权、客户端白名单、增量计算、Redis 中的 swarm（多节点共享）、真实 Transmission 4.1 传输与记账；以下仍未验证）：真实 Redis（会话状态 CAS、批量写入）、异步队列与背压、与 tracker-bridge 的同步、重启与分片迁移后的懒加载恢复、LB 按 URL 解码后的 info_hash 哈希、真实 BT 客户端接入、hybrid 种子只按 v1 hash 计账。
- [ ] **pt-tracker 压测**：目标平均 1.7 万次/秒，高峰 2～3 倍。（P8 开发机同机压测约 1.05 万次/秒、p99 13 ms；需在 tracker 与 Redis 分开部署的环境复测）
- [ ] **记账链路压测**：Redis Streams 吞吐、窗口聚合内存占用、`COPY` + `ON CONFLICT` 落库速度。
- [ ] **Meilisearch**：100 万～1000 万种子文档下的索引速度、查询 p99 延迟、中文和中英混排标题的检索效果、部分更新做种数的吞吐。
- [ ] **Meilisearch 扩展**：当前版本的复制 / 分片能力及授权条款。
- [ ] **wasmtime**：实例池下的调用开销（epoch 截止、内存上限、陷入映射已在 P3 测试中验证）。
- [ ] **SQL 执行器**：`SET LOCAL ROLE` + GRANT 隔离方案在连接池（含 pgbouncer 事务模式）下的正确性。
- [ ] **SET ROLE 加固的逃逸测试**（P1 已验证：插件角色读取他插件底表被拒绝、授权视图可读；`RESET ROLE` / `set_config` / plpgsql 逃逸用例待补）（平台 Q1）：在 PG16 成员关系 `INHERIT FALSE, SET TRUE`、撤销 `set_config` 与 plpgsql 的 PUBLIC 权限、`pg_query` 语句检查三层加固下，用 TC-KRN-051/053 验证 `RESET ROLE`、`SET ROLE`、`set_config('role', …)`、plpgsql 动态 SQL 等均无法切回高权限角色；P1 内核骨架阶段实测后定稿，不达标则改用备选方案“每插件独立登录角色 + 独立小连接池”（评估连接数代价），见 [02-plugin-system.md](02-plugin-system.md) §6.1。
- [ ] **动态 `Val` 服务桥接的开销**（平台 Q17）：Native ↔ WASM、WASM ↔ WASM 服务调用经内核以动态值转发的序列化与调用开销。
- [ ] **每次调用新实例的实例化开销**（平台 Q17）：JS（componentize-js）、Python 组件在“每次调用一个新实例”模型下的实例化耗时与内存。
- [ ] **Redis 配额抽样估算的误差**（平台 Q17）：插件私有前缀内存配额采用抽样估算时的误差与对超额判定的影响。
- [ ] **IYUU 对接规范核实**（评审 R1 F-11）：crossseed-api 的 IYUU 兼容接口需对照 IYUU 当前公开的站点接入文档与 IYUUPlus 客户端实际请求核实（路径、签名、返回字段）。
- [ ] **MCP 客户端兼容性矩阵**：Claude Desktop / Claude Code / 其他主流客户端对 2026-07-28 与 2025-11-25 两个协议版本、OAuth 动态注册的支持情况。
- [ ] **LLM 离线评测集**：ai-upload-review、ai-moderation 的标注样本与评分脚本，模型或提示词变更前必须跑一遍。
- [ ] **重入检测的误报率**（评审 R1 B-01）：type-*、search-contributor 等导出实现在真实链路下是否会触发 `kernel.reentrant_call`。

## 3. 待决问题

- [x] **开源协议**：内核与插件采用 MIT OR Apache-2.0（D19）；pt-tracker 为 AGPL-3.0-only（D16）。
- [x] **数据库**：PostgreSQL（D18）。
- [x] **集成类插件**：已展开设计（rss、crossseed-api、notify、webhook）。
- [ ] **评审 R1 用户决定项**：见 [../reviews/2026-10-05-design-review.md](../reviews/2026-10-05-design-review.md) §5（U-1～U-4，编码阶段先按推荐方案实现）。

## 4. 未完成与待做（2026-10-14 汇总）

已完成的功能见 [../dev/README.md](../dev/README.md) §4；以下是尚未做的部分，按领域列出。标“优先”的是正式开站前建议完成的。

### 4.1 有设计、尚未实现的插件

| 插件 | 作用 |
|---|---|
| ~~anticheat（优先）~~ | 已实现（2026-10-16，见 dev/README §4）；剩余：多账号 / 同 IP 关联、盒子识别联动 |
| registration-policy（优先） | 注册策略：开放 / 邀请 / 考试 / 名额、注册时段 |
| ratio-policy | 分享率考核：按等级的最低分享率、警告期、限制下载 |
| newbie-exam | 新手考核：限期完成上传量 / 做种量等任务 |
| seedbox | 盒子识别：按 IP 段 / ASN 识别盒子，用于 H&R 豁免与反作弊 |
| shop | 魔力值商城：购买上传量、邀请、改名卡、促销卡等 |
| lottery | 抽奖 / 彩票 |
| medals | 勋章与头衔 |
| forum | 论坛（含种子讨论帖、公告） |
| subtitles | 字幕上传与匹配 |
| mediainfo | MediaInfo / NFO 解析服务（供上传校验与 AI 预审） |
| terms | 术语服务：后台修改站点术语（目前术语只来自插件清单） |
| webhook | 站外 webhook 推送 |
| crossseed-api | 辅种接口（兼容 IYUU） |
| llm-openai-compat | OpenAI 兼容的大模型接入（本地模型等） |
| metadata-bangumi / douban / igdb / imdb / musicbrainz | 其他元数据源 |

### 4.2 平台与内核

- **插件包签名（优先，生产部署前必须完成）**：minisign 签 `checksums.txt`，站点配置信任公钥，加载时校验签名与文件哈希；`pt-server plugin keygen` / `package --sign`。完成后生产环境可用 `kernel.mode = "prod"`。
- 钩子输入按 schema 运行时校验；插件包内分发前端（目前前端需放进站点的 web 目录）。
- OpenAPI 聚合与 TS SDK 生成、CORS、client-errors 上报、术语版本头。
- SQL 语句检查（pg_query）、插件配额（KV / Redis / blob）、S3 对象存储后端、分区维护任务化。
- WASM：组件编译缓存、实例池、`metadata-provider` 等导出的静态绑定。
- 事件：回放、按类型的保留期清理、手动运行任务的 HTTP / CLI 入口。
- 出站 HTTP：速率限制、熔断、站点级代理、流式响应。
- SDK：其他语言（JS / Python）、从 schema 生成类型化事件 / 钩子结构；模板仓库独立发布。

### 4.3 tracker

- Prometheus 指标与告警；节点心跳、后台 peer 查询；客户端白名单的后台编辑界面。
- qBittorrent、Deluge、µTorrent 等客户端的兼容测试（目前只测了 Transmission 4.1）。
- tracker 与 Redis 分开部署的压测（目标平均 1.7 万次 / 秒）；Redis ACL 模板（`deploy/redis/acl.conf`）。
- 限制类变化（H&R 限制下载等）的实时同步（目前靠 tracker-bridge 每 10 分钟全量兜底）。

### 4.4 身份与安全

- （优先）找回密码、邮箱验证（依赖邮件渠道，已具备）、2FA（TOTP）、WebAuthn。
- 改名、UTS #39 混淆字符检查、身份数据的 Redis 缓存与登录限流细化。
- 后台修改用户资料的接口对未知字段静默忽略（如 `status`，封禁需用“限制”接口），应改为报错。

### 4.5 各插件的后续功能

- **种子 / 资源类型**：去重与替代（trump）规则、元数据字段锁定、词表与质量权重后台、缺集列表、艺人页、MusicBrainz 封面、抓轨日志签名验证。
- **搜索**：语义 / 混合搜索、多实例、做种数部分更新、同义词管理、一致性巡检。
- **记账 / 账本 / 魔力值**：日对账、月结余快照、归档、重算、交叉校验；魔力值规则的编辑与回放。
- **等级 / 邀请 / H&R / 促销**：规则编辑接口、模拟评估、批量操作、促销申请审核、预警与连坐通知。
- **社区**：图片上传、表情、@提醒通知、论坛举报目标；审核的规则编辑器、批量审核、通配符敏感词。
- **通知 / 站内信**：Telegram / Discord 渠道、合并通知、服务端模板渲染、mailgun / SES、群发公告。
- **签到 / 求种 / RSS**：补签卡、站点时区；求种认领、上传时选择应求；RSS 经 `ctx.search` 的订阅条件、H&R 属性、ETag。
- **AI**：上传预审的 MediaInfo 比对与疑似重复、站内助手的检索增强与流式输出、MCP 工具目录缓存与限流。

### 4.6 前端

- Markdown 编辑器与预览、外部 ID 自动拉取、长列表虚拟滚动、语言包按需加载。
- 各插件的后台管理页面（目前多数插件只有 JSON / schema 设置表单）。
- 移动端适配与整体视觉设计。

### 4.7 部署与运维

- Docker 镜像与 compose、systemd 服务文件在真实环境中实测。
- 监控（日志采集、指标、告警）、备份恢复演练、多节点部署指南。
- 同域名按路径分流 tracker 的 nginx 写法（目前文档写的是两个子域名）。

### 4.8 待验证项与待决问题

见 §2、§3（tracker / 记账 / Meilisearch 压测、SQL 隔离逃逸测试、MCP 客户端兼容性、LLM 评测集等）。

