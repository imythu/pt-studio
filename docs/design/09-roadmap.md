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
| 未来 | 钩子输入按 schema 运行时校验；插件包内分发前端；模板仓库独立发布（插件包签名已在 M2.1 完成） |

> P0–P8 与开站计划 M1–M4 均已完成（2026-10-18）。当前功能状态见 [../features.md](../features.md)，逐日记录见 [CHANGELOG](../../CHANGELOG.md)。

## 2. 待验证项

- [x] **Torrust 集成方案**：已确定采用方案 B′（见 [10-torrust-integration.md](10-torrust-integration.md)、决策 D14～D17）；原型已跑通 passkey 鉴权、过滤、增量计算、事件输出。
- [ ] **pt-tracker 未验证项**（已验证：passkey 鉴权、客户端规则（含即时生效与被拒统计）、增量计算、Redis 中的 swarm（多节点共享）、与 tracker-bridge 的同步、节点状态上报与告警、真实 Transmission 4.1 / qBittorrent 5.1 / Deluge 2.1 传输与记账；以下仍未验证）：真实 Redis（会话状态 CAS、批量写入）、异步队列与背压、重启与分片迁移后的懒加载恢复、LB 按 URL 解码后的 info_hash 哈希、µTorrent 接入、hybrid 种子只按 v1 hash 计账。
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

## 4. 未完成与待做

已移到 [../features.md](../features.md#未完成)，与已完成的功能对照维护。

## 5. 开站计划（2026-10-17）

依据：对照 NexusPHP（1.9 / 1.10）、UNIT3D、Gazelle 系与国内 PT 站的常见功能做的差距调查。**周边工具对接**（IYUU 辅种接口、给 MoviePilot / PT-depiler 等工具用的长期 token 接口、IRC 播报、上游适配）**暂不做**。按下列顺序逐项实现，每项完成后在此打勾并更新 [功能清单](../features.md)。

### M1 账号安全（identity 为主）

- [x] M1.1 登录限流（Redis 失败计数、用户软锁、IP 封锁、指数延迟）+ IP 记录（`ip_log`，登录 / 会话 IP 变化）+ 用户 IP 历史 + 管理员按 IP / CIDR 查同 IP 账户 + 安全事件表
- [x] M1.2 邮箱验证（注册后 pending，验证后解除）、重发验证邮件、找回密码（挑战表，邮件经 `ctx.notify` 的 security 类别）
- [x] M1.3 TOTP 两步验证 + 恢复码 + 登录第二步票据 + 近期重新验证（reauth），按角色强制 2FA
- [x] M1.4 WebAuthn：作为第二因素与无密码登录（通行密钥）
- [x] M1.5 人机验证（captcha 插件：Cloudflare Turnstile / hCaptcha / reCAPTCHA v2，注册、登录、找回密码；签到待接入）
- [x] M1.6 registration-policy：开放 / 邀请 / 申请 / 关闭、注册时段与名额、邮箱域名黑白名单

### M2 能稳定开站

- [x] M2.1 插件包签名（minisign；`plugin keygen` / `package --sign`；`kernel.mode = "prod"` 校验）
- [x] M2.2 qBittorrent / Deluge 客户端兼容测试（qBittorrent 5.1、Deluge 2.1 双向实测通过）
- [x] M2.3 ratio-policy（按等级的最低分享率、警告期、限制下载）
- [x] M2.4 seedbox（用户登记 IP 段 + 审核，accounting 标记盒子 peer；ASN 自动识别与 H&R / 魔力值规则接入待做）
- [x] M2.5 newbie-exam（新手考核）
- [x] M2.6 管理操作日志页（审计查询）、Prometheus 指标端点、备份恢复脚本与演练文档

### M3 内容功能（国内站刚需）

- [x] M3.1 种子认领（claims：认领、保种考核、认领奖励）
- [x] M3.2 forum（版块、主题、回复、种子讨论帖、公告）
- [x] M3.3 metadata-douban / PT-Gen 兼容简介生成（含豆瓣图书）；另有 metadata-bangumi、metadata-musicbrainz
- [x] M3.4 mediainfo（MediaInfo / BDInfo 解析，上传校验）
- [x] M3.5 收藏、置顶、排行榜（top10）、合集
- [x] M3.6 图片上传（附件 / 图床，blob 存储）
- [x] M3.7 subtitles（字幕区）
- [x] M3.8 去重与替代（trump）规则

### M4 运营玩法与管理

- [x] M4.1 shop（魔力值商城：上传量、邀请、促销卡、消除 H&R；改名卡与个人促销留待后续）
- [x] M4.2 medals（勋章与头衔）、lottery（抽奖；竞猜留待后续）
- [x] M4.3 工单 / 联系管理组、群发公告
- [x] M4.4 捐赠 / VIP 到期、魔力值流水页
- [x] M4.5 各插件的管理页面、Markdown 编辑器、移动端适配
