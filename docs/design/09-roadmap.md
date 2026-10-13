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

> 进度（2026-10-12）：P1 内核骨架完成（identity、torrent）；P2 记账完成主体（ledger、accounting、分区 worker、网关限流与幂等键）；P3 WASM 宿主跑通（bonus 以 WASM 实现，经 JSON 分派与 Native 服务互调；截止时间、内存、陷入、隔离均有测试）；内核 cron、accounting 小时快照与 bonus 小时结算 / 批量入账打通（tracker 流 → 魔力值到账）；促销（promotion）与等级（levels）以 WASM 实现完成，P3 计划的三个检验插件全部落地（倍率经 accounting 生效、等级经 identity 生效并绑定角色）。P4：search-meilisearch 完成主体（贡献者合并、增量同步、swap 重建、可见性强制查询），对 Meilisearch 1.54 实测通过。invite 完成主体（签发、兑换、额度、邀请树、连坐），identity 增加限制与有效状态。hnr 完成主体（建档、分层评估、限制联动、免除）；P4 计划的插件（邀请、H&R、搜索）全部落地，资源类型插件留到前端阶段与插槽一起做。P5：前端外壳、插槽机制（import map 共享依赖、懒加载、错误隔离）、Schema 表单、术语层、插件前端（levels、bonus、hnr、promotion、invite）完成，真实浏览器端到端测试通过；资源类型（type-movie、type-generic）与作品页已完成，发布表单由类型 schema 生成；个人设置页与后台用户 / 种子管理已完成；元数据（内核出站 HTTP、metadata-tmdb、电影 Group 自动补全）已完成；剧集（type-tv）与动漫（type-anime）已完成（季集与范围、按季分组、上传时按文件列表检查范围、字幕组风格发布名）；音乐（type-music）已完成（艺人与版本、格式规则、文件检查、抓轨日志评分）；其他元数据源（MusicBrainz、Bangumi 等）与去重 / 替代规则待做。P6：`ctx.llm`（llm-claude）与 AI 上传预审（ai-upload-review）已完成；MCP 网关已完成主体（个人 MCP token、工具注册、两个协议版本、写操作站内确认），OAuth 与前端、ai-assistant、ai-moderation 待做。详见 [../dev/README.md](../dev/README.md) §4。

## 2. 待验证项

- [x] **Torrust 集成方案**：已确定采用方案 B′（见 [10-torrust-integration.md](10-torrust-integration.md)、决策 D14～D17）；原型已跑通 passkey 鉴权、过滤、增量计算、事件输出。
- [ ] **pt-tracker 未验证项**：真实 Redis（会话状态 CAS、批量写入）、异步队列与背压、与 tracker-bridge 的同步、重启与分片迁移后的懒加载恢复、LB 按 URL 解码后的 info_hash 哈希、真实 BT 客户端接入、hybrid 种子只按 v1 hash 计账。
- [ ] **pt-tracker 压测**：目标平均 1.7 万次/秒，高峰 2～3 倍。
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
