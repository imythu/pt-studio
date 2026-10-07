# 设计评审记录 R1（2026-10-05）

| 项 | 内容 |
|---|---|
| 评审范围 | docs/platform/（平台契约）、docs/plugins/（50 个插件 × 4 份）、docs/plugins/_shared/（3 份共用设计）、docs/components/pt-tracker、docs/components/web |
| 评审依据 | docs/process/README.md 的通过标准；各插件 HLD §7“对平台契约的新增”；跨文档一致性检查 |
| 评审人 | 主设计（Claude） |
| 结论 | **有条件通过**（2026-10-05 修正完成：§2 已合入 kernel/03 与 plugin-api，§4 F-01～F-12 已修改，§3.2 名称登记随编码逐个合入）。原结论：§2 的阻断项与 §4 的文档修正完成后进入编码；§5 的用户决定项不阻断编码（采用推荐方案为默认，用户确认后可调整） |

## 1. 总体评价

- 覆盖完整：插件登记表中的 50 个插件与 2 个组件均有需求、概要、详细设计与测试计划；需求均有编号，测试计划含需求覆盖矩阵与“重复执行”用例。
- 一致性主要风险集中在三处：① 各插件对平台契约提出的新增（约 300 项）尚未合入目录；② 服务依赖方向与“导出回调”的规则未在内核中明确；③ 少量插件之间的接口签名在不同文档中不一致。
- 安全与隐私设计整体到位：敏感字段不进事件、钩子不出站、写操作幂等、MCP 写操作站内确认、用户内容统一渲染。

## 2. 阻断项（编码前必须完成）

| 编号 | 问题 | 涉及 | 决定 |
|---|---|---|---|
| B-01 | **服务依赖环规则不完整**：torrent 同步调用 type-* 的 `content-type` 导出，而 type-* `requires ctx.torrents`；identity 的 `ctx.users.create` 触发 `user.before_register`，回到 registration-policy 的新实例 | kernel、torrent、type-*、identity、registration-policy | 内核规则补充：**依赖无环只约束服务（`requires`）**；导出接口（hooks、content-type、search-contributor、metadata-provider、mcp-tool、on-event、on-cron）的调用不计入依赖图。为防止重入死锁，内核在同一调用链中禁止“导出实现回调触发它的插件的服务”（检测到返回 `kernel.reentrant_call`）。type-* 的 `normalize` 与钩子保持只读视图访问，不调用 `ctx.torrents` |
| B-02 | **已识别并消除的依赖环**：reports → notify → messages → reports | notify、messages、reports | 已在设计中消除：notify 自建通知中心、不依赖 messages；reports 不依赖 messages。依赖图写入内核启动检查的测试用例（TC-KRN） |
| B-03 | **以用户身份调用**：rss、crossseed-api 的 public 路由用 feed key / API token 自行鉴权后，需要以该用户身份调用 search 与 torrent，否则可见性过滤失效 | kernel、rss、crossseed-api、mcp、ai-assistant | 内核新增 `invocation.with_principal(p)`：只接受本次调用中由 `ctx.auth.authenticate` 返回的 principal（内核记录），之后的服务调用 actor 为该用户 |
| B-04 | **出站主机声明**：多个插件需要由配置决定的出站主机（metadata 代理、llm base_url、webhook 端点） | kernel、metadata-*、llm-*、notify、webhook | 清单 `http_outbound` 支持 `config:<静态配置键>`（启动时解析）与 `runtime:<设置键>`（管理员修改设置后生效，仅 webhook）；**SSRF 防护放在内核 http-client 层**（解析后检查地址、禁止私有地址与重定向到私有地址；`config:` 声明的主机可显式允许内网，供本地 LLM 使用） |
| B-05 | **钩子输入按处理者裁剪敏感字段**（`email`、`ip`） | kernel、identity、invite、registration-policy | 接受：内核在 `hook-runner.run` 中按每个处理者的有效敏感能力删除字段；identity 另提供非敏感的 `ip_hmac`、`ip_prefix_hmac`、`device_hmac` |
| B-06 | **网关改用 `ctx.rbac.check_principal`**（含无 permission 的路由），否则只读 token 的限制无法执行 | kernel、identity、mcp、rss | 接受，上线前必须实现 |
| B-07 | **缓存失效广播**：内核以单个消费组订阅 `terms.updated` 时，只有一个副本收到 | kernel、terms、search | 内核规则：内核自身的缓存失效订阅在收到后 `PUBLISH k:bus`，所有副本刷新；同时各副本每 2 秒比较 `ctx.terms.version()` 兜底 |
| B-08 | **事件日志体量**：`traffic.settled`、`traffic.seeding_snapshot_taken`、`ledger.entry_posted` 每日数千万条，默认 24 个月保留不可承受 | kernel、accounting、ledger、bonus | 接受 accounting N-2：按事件类型配置日志保留期与流长度；`kernel.event_log` 改为按日分区；默认 `traffic.*` 30 天、`ledger.entry_posted` 90 天（流水由 ledger 永久保存） |
| B-09 | **Native 分区 worker**（accounting 消费 `acct:*`） | kernel、accounting | 接受 accounting N-1：清单 `[[workers]]` + 内核租约与 `PartitionWorker` trait |
| B-10 | **WASM 插件不能用 EVAL**：metadata-* 的令牌桶原设想用 Lua | metadata-* | 已改为 `INCR` 固定窗口；命令白名单保持禁止 EVAL |
| B-11 | **思考块与历史回放**：Claude 当前模型的思考块与对话绑定，删改历史会被拒绝 | llm-claude、ai-* | 已在 `pt:llm` 接口加入 `provider-blob` 与“只追加”约定；ai-assistant 已按此保存 `llm_messages` |

## 3. 平台契约合入决定

所有插件 HLD §7 的新增项**默认接受**，按下表合入 `docs/platform/`（合入后各目录版本升为 0.2）。拒绝或调整的条目单列。

### 3.1 调整或拒绝的条目

| 条目 | 来源 | 决定 |
|---|---|---|
| `If-Match` 不匹配的状态码（412 与 409 并存） | terms Q、api.md | `If-Match` / `If-None-Match` 前置条件失败 → **412 `kernel.precondition_failed`**（新增内核错误码）；请求体中的 `expected_version` 不匹配 → 409 `kernel.conflict`。terms 改用 412 |
| 前端插件路由前缀（`/p/<id>/` 与 `/admin/p/<id>/`） | terms、search | plugin-api §4.2 改为“必须以 `/p/<id>/` 或 `/admin/p/<id>/` 开头” |
| 钩子拒绝的 HTTP 状态（后缀规则与目录不一致） | registration-policy | hooks.md §3 改为：处理者返回的错误码**已在 error-codes 目录登记时使用登记的状态**，否则按后缀规则 |
| `search` 作用域 `query:all`（跳过可见性） | search-meilisearch | 接受，但视为敏感授权：只能在 `site.toml [grants.<id>]` 中显式授予（与敏感视图同一机制） |
| `ai-assistant` 类型 | ai-assistant DD-AIAST-1 | 改为 **Native**（需要 `ctx.mcp_registry`）；catalog/plugins.md 更新 |
| `ctx.medals.bonus_factor` | bonus B-6 | 接受：medals 物品增加可选 `bonus_factor_permille`，服务新增 `bonus_factor(user_id)`（佩戴或持有的有效物品之和，上限可配置） |
| identity 邮箱只读视图 `v_user_email` | identity DD-IDN-8 | 见 §5 U-1（推荐取消该视图） |
| `user.registered.invite_code` 明文 | invite Q-INV-1 | 事件不再携带邀请码：invite 在 `user.before_register` 钩子中 modify `attributes.invite_id`，确认时按 ID 查找 |
| `[[http.routes]] reauth_secs` | tracker-bridge | 接受（网关调用 identity 检查最近一次重新验证时间） |
| `[hooks."<name>"].disabled_handlers` 运维开关 | anticheat A-13 | 接受，仅 sysop 在 site.toml 中配置，修改需重启 |

### 3.2 合入清单（按目录）

**合入方式**：
- 跨插件的规则性条目（§2、§3.1、§4）已在本轮直接改入平台文档。
- 逐条的名称与归属登记（服务方法、事件、权限、MCP 工具、Redis 键、术语键），**随各插件编码逐个合入**：每个插件的实现提交同时更新 catalog；内核启动时校验清单与 `catalog.json`（kernel/03 §2.1 第 5 步），CI 的 `xtask gen-catalog --check` 保证两者一致。
- 字段级细节以各插件 03 详细设计为准，catalog 只登记名称、归属与关键属性。

已在本轮合入：hooks.md（处理规则、`user.before_title_change`、`user.before_register` / `user.before_login` 输入字段、`attributes.invite_id`）、slots.md（3 个插槽、路由前缀）、error-codes.md（`kernel.precondition_failed`、`kernel.reentrant_call`）、events.md（`user.registered`）、plugins.md（ai-assistant）、plugin-api.md（清单字段）、kernel/03（§2 全部阻断项）。

下表是各目录待合入条目的来源：

| 目录 | 合入来源（插件 HLD §7 / 设计中的 ⊕ 条目） |
|---|---|
| catalog/services.md | identity §7.3；invite §7.3；registration-policy §7.3；torrent §7；tracker-bridge §7；accounting N-5、N-8；ledger L-1、L-2；anticheat A-3、A-11、A-12；bonus B-1、B-2；terms §7；search §7；checkin §7；medals §7（含 bonus_factor）；promotion §7；shop §7；lottery §7；levels §7；ratio-policy §7；hnr §7；newbie-exam §7；seedbox §7；comments §7（`count_many`、`get_raw`）；forum §7（`topic_for`、`delete_post`、`get_raw`）；messages §7；requests §7；subtitles §7（`count_many`、`delete`）；moderation §7；reports §7；notify §7；mcp §7；llm（`pt:llm@0.1.0` 以共用设计为准）；ai-* §7 |
| catalog/events.md | identity §7.1；invite §7.1；registration-policy §7.1；torrent §7；tracker-bridge §7；accounting N-3、N-10；ledger L-3；anticheat A-1、A-2；bonus B-3、B-4；terms §7；checkin、medals、promotion、shop、lottery、levels、ratio-policy、hnr、newbie-exam、seedbox、forum、messages、requests、moderation 的新增事件；llm.usage_recorded 字段 |
| catalog/hooks.md | identity §7.2；invite §7.2；ledger L-4；checkin（`kind`、`date`）；shop（`gift_to`、`params_keys`）；comments（`markup`、`reply_to`、`mentions`）；forum（`markup`、`is_topic`）；新钩子 `user.before_title_change`（identity 拥有）；预期处理者补充（moderation、requests、forum、levels、ai-moderation） |
| catalog/permissions.md | identity、invite、registration-policy、tracker-bridge、anticheat、bonus、accounting、ledger、search、checkin、shop、moderation 的新增权限点 |
| catalog/slots.md | `auth.register.form_extra`、`auth.login.extra`、`user.name.decorations` |
| catalog/mcp-tools.md | invite、registration-policy、ledger、checkin、medals、lottery、tracker-bridge 的新增工具；新增列 `confirm_locked`（`admin_set_user_status`、含封禁动作的 anticheat / reports 工具、`admin_decide_moderation`） |
| catalog/redis-keys.md | tracker-bridge（`trk:u:*`、`trk:stats:*` 细化、ACL）；pt-tracker N-1～N-10；accounting N-9；search、terms、bonus、torrent、checkin、medals、promotion 的子键 |
| catalog/error-codes.md | `kernel.precondition_failed`（412）、`kernel.reentrant_call`（500）；跨插件常用：anticheat A-5、ledger L-8、torrent（`not_downloadable`、`invalid_status_transition`） |
| catalog/terms.md | 各插件新增术语键 |
| catalog/plugins.md | ai-assistant 改 Native；共享 crate 清单（不是插件，另起 §4） |
| plugin-api.md | `search-contributor` 的 `to-documents`、`list-ids`、`index-schema` 字段；`content-type` 的 `canonical-group-title`、可选 `quality-rank`；宿主 `terms` 记录增加 `plural`、`icon-url`；清单字段 `[[workers]]`、`[notify.templates]`、`http_outbound` 的 `config:` / `runtime:` 前缀、`[[http.routes]].reauth_secs`、`[[database.partitions]]` 的 `retention_days` 与 `archive_hook`；`[frontend].routes` 前缀规则 |
| kernel/03 | B-01 重入检测；B-03 `with_principal`；B-04 出站声明与 SSRF；B-05 钩子输入裁剪；B-06 `check_principal`；B-07 缓存失效广播；B-08 事件日志按类型保留与日分区；B-09 分区 worker；`[notify.templates]` 登记；mcp 插件的根路径路由（`/mcp`、`/.well-known/oauth-*`、`/.well-known/openid-configuration`、`/oauth/*`）；`index.html` 的 CSP nonce 与 import map 注入 |

### 3.3 共享 crate（新增，统一由平台维护）

| crate | 用途 | 使用方 |
|---|---|---|
| `pt-predicate` | 谓词 JSON 求值与 schema | anticheat、medals、promotion、levels、newbie-exam、hnr、requests、webhook、moderation |
| `pt-markup` | 用户内容渲染与净化 | comments、forum、messages、requests、promotion（横幅）、notify（邮件 HTML） |
| `pt-antispam` | 频率、重复、新用户限制 | comments、forum、messages |
| `pt-release-parser` | 发布名解析 | type-*、search-meilisearch |
| `pt-episodes` | 季集解析 | type-tv、type-anime |
| `pt-metrics` | 用户指标计算（视图批量查询） | levels、newbie-exam |
| `pt-llm-common` | 配额、脱敏、幂等、计量 | llm-claude、llm-openai-compat |
| `pt-term-render` | 术语与 MessageFormat 子集渲染 | kernel、terms、notify、web（WASM 构建） |

## 4. 文档修正清单

| 编号 | 文档 | 修正 |
|---|---|---|
| F-01 | tracker-bridge 03 | ACL 收窄为 pt-tracker 03 §6.2 的写法（不能读 `trk:u:*`）；`trk:stats:denied` 日期用 UTC 并写明 member 转义规则；`st = 1` 时写 `rsn`；注明 hybrid 种子 v2 截断 hash 无会话（peer 列表为空属正常） |
| F-02 | tracker-bridge 01 | FR-TRB-014 的方法名与 02 / 03 一致（`status`、`peers_of_user`） |
| F-03 | accounting 03 | 解析流量事件时“忽略未知 flags 位”；分区数以 `acct:_meta` 为准，不使用 `SCAN acct:*`；容量规划按每分区 `MAXLEN ~ 2,000,000`；增量 > 2^50 时只丢弃流量字段、保留时长与完成标记（不整条拒收）；`v_user_totals` 增加 `completed_count`、`seeding_completed_count`、`seeding_count`、`seeding_size`；`v_user_daily` 增加 `uploaded_raw`、`downloaded_raw` |
| F-04 | identity 03 | 首次生成 passkey 不另发事件（tracker-bridge 已订阅 `user.registered` 同步 passkey）；`user.registered` 删除 `invite_code`、`attributes` 增加 `invite_id`；新增钩子点 `user.before_title_change`；钩子输入增加 `ip_hmac`、`ip_prefix_hmac`、`device_hmac` |
| F-05 | torrent 03 | 新增视图 `v_torrent_fields`、`v_group_fields`、`v_torrent_fingerprint`（`content_fp` 索引）、`v_uploader_counts`；`torrent.created` 载荷增加 `file_count`、`created_at`；`get_many` 结果增加 `can_download`（供 crossseed-api）；`build_download` 接受 `via` |
| F-06 | medals 02 / 03 | 增加 `bonus_factor_permille` 与服务 `bonus_factor(user_id)` |
| F-07 | shop 03 | `hnr_removal` 适配器调用 `hnr.forgive(record_id, reason, idempotency_key)` |
| F-08 | checkin 03 | `remind_streak` 改用 `notify.send` 的幂等键，删除 kv 去重的过渡方案 |
| F-09 | terms 03 | `If-Match` 不匹配返回 412 `kernel.precondition_failed` |
| F-10 | catalog/plugins.md | ai-assistant 改为 Native |
| F-12 | components/web 02 / 03 | 静态资源路径与 kernel/03 §8.6 一致：插件入口 `/static/plugins/<id>/<hash12>/index.js`，共享依赖 `/static/vendor/*` |
| F-11 | design/09-roadmap.md | 待验证项增加：IYUU 对接规范核实（crossseed-api）、Meilisearch 多实例能力、MCP 客户端兼容性矩阵、llm 离线评测集 |

## 5. 需要用户决定的问题（不阻断编码，先按推荐方案实现）

| 编号 | 问题 | 推荐 |
|---|---|---|
| U-1 | identity 邮箱：保留只读视图 `v_user_email`（邮箱只能明文存储）还是取消视图、邮箱只经服务调用获取（可应用层加密存储） | **取消视图**：notify 通过 `challenge_delivery` 与 `get_sensitive` 获取邮箱；邮箱用应用层密钥加密存储，另存 HMAC 供唯一性检查 |
| U-2 | 是否允许 MPL-2.0 依赖（`webauthn-rs`）与 Unicode-3.0（ICU） | **允许**：MPL-2.0 为文件级弱著佐权，作为未修改的依赖使用不影响本项目的 MIT / Apache 许可 |
| U-3 | anticheat 自动处置边界 | **按现设计**：只对结论性规则自动冻结流量与锁钱包；自动封禁默认关闭 |
| U-4 | anticheat 封禁四眼规则对单 sysop 小站 | **保留默认开启**，单 sysop 站点在设置中关闭（有审计记录） |
| U-5 | lottery / 竞猜 | 默认关闭，开启需合规确认（已设计），无需决定 |

## 6. 后续

1. 按 §3、§4 修订平台契约与插件文档（本轮完成后记为 R1 关闭）。
2. 编码阶段按 docs/design/09-roadmap.md 的顺序：内核骨架 → identity / torrent → accounting / ledger → WASM 宿主 → 其余插件 → 前端。
