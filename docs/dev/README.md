# 开发环境与当前实现状态

## 1. 本地依赖

| 组件 | 版本 | 说明 |
|---|---|---|
| Rust | stable（当前 1.98.1） | `rust-toolchain.toml`，需 clippy、rustfmt |
| PostgreSQL | ≥ 16（开发用 17） | 需要 `GRANT … WITH INHERIT FALSE, SET TRUE`（PG16+） |
| Redis | ≥ 7.4（开发用 8.0） | 需要 `HEXPIRE` 等命令；消费者使用 Streams |
| Meilisearch | ≥ 1.13（CE） | search-meilisearch；只监听内网 |
| Node.js | ≥ 20（开发用 24） | 前端构建（`web/`，npm workspaces + esbuild）；浏览器端到端用 Playwright + Chromium |

### 1.1 初始化数据库（一次）

以超级用户执行 `deploy/postgres/bootstrap.sql`（创建 `pt_admin`、`pt_gateway`、数据库 `pt`，并在库内撤销 `set_config` 与 plpgsql 的 PUBLIC 权限）。测试库 `pt_test` 同样执行后半部分。

### 1.2 本机开发实例（无 Docker 时）

```bash
# PostgreSQL（端口 55432，数据目录 .dev/pg，以 postgres 用户运行）
su postgres -c "/usr/lib/postgresql/17/bin/pg_ctl -D .dev/pg -o '-p 55432 -k /tmp' -l .dev/pg.log start"
# Redis（端口 56379，不落盘）
redis-server --port 56379 --daemonize yes --save '' --appendonly no
# Meilisearch（端口 57700，二进制放在 .dev/bin，数据目录 .dev/meili）
.dev/bin/meilisearch --http-addr 127.0.0.1:57700 --master-key pt-test-master-key-0123456789 \
    --db-path .dev/meili/data.ms --dump-dir .dev/meili/dumps --no-analytics > .dev/meili.log 2>&1 &
```

## 2. 运行

```bash
cargo run -p pt-server -- --config site.toml check     # 只做清单与依赖检查
cargo run -p pt-server -- --config site.toml migrate   # 只执行迁移
cargo run -p pt-server -- --config site.toml serve
```

最小 `site.toml` 见 `crates/server/tests/common/mod.rs` 中的 `config()`。

前端：

```bash
cd web && npm ci && npm run build      # 输出 web/dist（index.html、static/、plugins/<id>/）
npm run typecheck && npm test          # tsc + vitest
```

`site.toml` 中设置 `[http] web_dir = "web/dist"` 后，内核在非 `/api` 路径上提供 SPA（每次响应生成 CSP nonce 写入 import map）、`/static/*` 与插件前端 `/static/plugins/<id>/<version>/*`。

## 3. 测试

```bash
cargo clippy --workspace --all-targets      # 0 警告
cargo test --workspace                      # 单元测试 + 端到端测试
```

端到端测试（`crates/server/tests/`）连接真实 PostgreSQL 与 Redis，默认地址对应 §1.2；可用环境变量 `PT_TEST_SUPERUSER_URL`、`PT_TEST_ADMIN_URL`、`PT_TEST_GATEWAY_URL`、`PT_TEST_REDIS_URL`（建议用独立 DB 号，测试会 `FLUSHDB`）、`PT_TEST_BLOB_DIR`、`PT_TEST_MEILI_URL` / `PT_TEST_MEILI_KEY` 覆盖。Meilisearch 不可达时 `e2e_search` 跳过（输出 `SKIPPED`）；缺少 node 或 `web/node_modules/playwright` 时 `e2e_web` 跳过（Chromium 用 Playwright 缓存，可用 `PT_CHROME` 指定）。测试会删除并重建测试库中的 `kernel`、`identity`、`torrent` 等 schema。

## 4. 实现状态（P1 内核骨架 + P2 记账 + P3 WASM 宿主 + P4 扩展插件 + P5 前端，2026-10-12）

| 模块 | 已实现 | 未实现 / 简化（后续迭代） |
|---|---|---|
| `pt-plugin-api` | 错误模型与状态推导、Actor / InvocationInfo、全部宿主 trait、导出 trait、`Ctx`（服务调用 `Svc::call`、`authenticate`、`with_principal`）、清单模型与基础校验 | WIT 包、`service!` 宏、清单 JSON Schema |
| 内核 · 加载器 | 依赖解析（single 服务选择、版本要求、缺失、歧义）、拓扑排序与环检测、钩子点归属检查、敏感能力授权检查 | WASM 插件发现与签名校验；catalog 只生成了钩子与错误码（`build.rs` 解析 docs） |
| 内核 · 存储 | 迁移（advisory lock、每插件角色与 schema、`_inbox`/`_kv`、校验和、视图授权同步）、插件范围分区预建（`[[database.partitions]]`）、工作单元（`SET LOCAL ROLE`、kv、outbox 事件、审计、延迟任务）、Redis 键改写与命令白名单、本地文件 blob | pg_query 语句检查（WASM 用）、配额、S3 后端、分区维护任务化（目前启动时预建事件日志日分区） |
| 内核 · 运行期 | 服务调用（深度、截止时间、重入检测 `kernel.reentrant_call`、独立工作单元）、JSON 分派服务调用、钩子管线（顺序、超时、fail-closed / 锁定、可修改字段与只追加检查、敏感字段裁剪、`disabled_handlers`）、客户端 IP 只传给获授 `user.ip` 的插件 | — |
| 内核 · WASM 宿主 | wasmtime 49 组件模型：`plugins.dir` 包加载（清单、编译、迁移、导出与清单一致性检查、未签名包仅限 dev）、每次调用新实例、epoch 截止时间（`kernel.timeout`）、内存上限（`kernel.resource_exhausted`）、陷入（`kernel.plugin_trapped`）、并发上限、WASI 无网络 / 无文件；全部宿主接口转发到 `Ctx`；导出包装为 Native 导出 trait（lifecycle、hooks、on-event、on-cron、http-handler、service-provider） | 包签名（minisign）校验、组件编译缓存、实例池、metadata、search-contributor / mcp-tool / content-type 导出接入 |
| 内核 · 事件与调度 | outbox 中继（租约、LISTEN/NOTIFY + 轮询）、消费者（每订阅每分区租约、专用连接、XAUTOCLAIM、inbox 去重、退避重试、死信）、延迟任务执行、分区 worker（`[[workers]]`，每分区租约 + fencing token，每次 poll 一个工作单元）、cron（5 段、站点时区、跨副本去重、禁止重叠、catch_up once、`kernel.cron_run`、`Kernel::run_job_now`） | 回放、按类型保留期清理、手动运行的 HTTP / CLI 入口 |
| 内核 · 网关 | 声明路由校验、会话 / Bearer 鉴权、CSRF、`check_principal`、`reauth_secs`、可信代理 IP、problem+json 与目录状态码、请求级工作单元、GCRA 限流（全站默认 + 路由规则，RateLimit 头、Retry-After）、幂等键（指纹、处理中 / 冲突 / 重放、5xx 释放） | OpenAPI 聚合、前端清单、CORS |
| identity | 注册（规范化、钩子、加密邮箱与 passkey）、密码登录、会话、登出、`ctx.auth` / `ctx.rbac` / `ctx.users`、内置角色继承、权限登记、授予角色、等级（`set_class`、`user.class_changed`）、等级绑定角色（`class_roles`，插件定义带前缀角色）、`roles_of`、限制与有效状态（施加 / 解除 / 到期清扫、`user.status_changed`、封禁使会话失效、插件不能限制员工、管理员 rank 检查、`ctx.users.set_status` / `apply_restriction` / `lift_restriction`、`ctx.rbac.check` JSON）、本人设置（资料 merge-patch、改密码并撤销其他会话、会话列表与撤销、passkey 重置；`user.profile_updated` / `password_changed` / `passkey_reset`）、后台用户（列表筛选与游标、详情、编辑、强制下线、移除角色） | 2FA、WebAuthn、token、改名、找回密码、Redis 缓存、限流、UTS #39 混淆骨架 |
| ledger | 币种注册、发行 / 回收 / 转账（手续费、每日限额、`ledger.before_transfer`）、批量发行、幂等、冻结（到期由延迟任务释放）/ 扣款、账户锁定、冲正、流水分页、供给统计、用户转账与管理员调整 / 冲正 / 设置 HTTP | 日对账、月结余快照、归档、用户删除处理、MCP 工具 |
| accounting | `acct:<p>` 微批消费、eid 去重、拒收记录、倍率规则（favorable / override、索引与版本号刷新）、peers、snatches、首次完成与 `traffic.snatch_completed`、用户总量与日统计、结算窗口与 `traffic.settled`、分段明细、游标 CAS（恰好一次，重启不重复计入）、peers 小时做种槽与小时快照（折叠做种时长、`traffic.seeding_snapshot_taken`）、流量调整、种子镜像、统计 HTTP | 基础信号、swarm 统计、实时计数扣减、交叉校验、重算、流修剪 |
| bonus（WASM） | 币种初始化（经 `ctx.ledger`）、安全表达式语言（词法、Pratt 解析、白名单函数、深度 / 节点上限、非有限值归零）、规则集（表达式与分档模式、NexusPHP 预设为 v1、按小时取生效版本）、小时结算（快照 → 用户小时 → 应计，多部分合并）、批量入账（postings、`transfer_batch`、幂等键、失败重试计数）、行为奖励、`ctx.bonus.award`、`GET /v1/users/me/summary` | 规则编辑 / 发布接口、回放重算、回收、估算明细、`has_role`（等 `ctx.rbac.roles_of`）、medal_factor（等 medals） |
| promotion（WASM） | 促销类型、单种促销（替换 / 结束）、自动规则（谓词与优先级，`torrent.created` 及手动促销结束后重新评估）、全站活动、个人促销卡（发放 / 使用、冗余检查）、同步到 accounting 倍率规则（对账重试）、到期 cron、展示状态、`ctx.promotions` | 促销申请审核、促销统计、自定义类型的前端展示 |
| levels（WASM） | NexusPHP 预设等级（农民为降级落点，用户 0.6 / 0.5 滞回）、角色同步（`levels:class_<id>` 绑定到等级）、指标（注册天数、上传 / 下载 / 分享率、做种时长、平均做种时长、做种体积、发布数）、评估（覆盖 > 升级 one_step / highest > 保护期 > 保级 / 警告 / 到期降级）、增量评估（`traffic.settled` 标记，只升级）与每日全量评估、`invite.before_issue`、手动锁定 / 解锁、保号、`ctx.levels`、`GET /v1/me` 进度 | 等级编辑接口、模拟评估、邀请奖励、不活跃处理、VIP / 荣誉覆盖来源、等级权限默认配置 |
| invite（WASM） | 码派生（HMAC、只存哈希、Crockford 容错）、个人 / 系统 / 活动码签发（资格检查、`invite.before_issue`、FIFO 消耗额度、`request_id` 重放）、注册钩子预留 + `user.registered` 确认（超额兑换标记、邀请树边与闭包表）、额度批次与流水（初始、晋升、周期、管理员、服务发放 / 扣减、到期落账）、撤销 / 过期（返还原批次）、预留释放、邀请树 / 上游 / 统计、发邀权暂停、连坐（封禁触发：扣额度、暂停、撤销未用、限制邀请人、复核；解封撤销）、`ctx.invites`、HTTP | 批量发放、整树操作、日统计与问题邀请人评分、对账、MCP、连坐通知（等 notify）、`anticheat.case_resolved` 触发、`user.deleted` 处理、链接再次显示 |
| 内核 · 出站 HTTP 与元数据 | `http-client`（清单 `http_outbound`：主机、`*.` 通配、`config:<键>`、`allow_private`；仅 https（`allow_private` / dev 可 http）；解析后拒绝私有 / 回环 / 链路本地 / CGNAT 等地址并固定连接地址；不跟随重定向；钩子内禁止；超时 ≤ 30 s、响应 ≤ 8 MiB；Native 与 WASM 共用）、`ctx.metadata` 多实现服务（按 `source` / `provider` 路由）、提供方声明的服务超时（`[wasm] default_timeout_ms`）、`/media/<plugin>/<key>` 公开媒体（blob `public/` 前缀） | 出站速率限制与熔断、站点级出站代理、HEAD / 流式响应 |
| 内核 · 前端支持 | 前端清单（插件入口、插槽、路由、settings_ui）、术语快照（清单 `[terms]` 合并、按语言回退）、插件设置读写（`kernel.settings.manage`，乐观版本、审计）、SPA 与静态文件（CSP nonce 注入 import map、哈希文件长期缓存、路径穿越拒绝）、插件前端静态路径、插槽登记检查（`catalog/slots.md`） | client-errors 上报、术语版本头、OpenAPI 聚合、CORS |
| web（P5） | `@pt-studio/sdk`（CSRF 双提交、幂等键、problem+json → ApiError）、`terms`（术语 / 文案 / 插值）、`slots`（清单驱动的懒加载、list / single / tabs / columns、错误边界、PluginApi、批量合并、插件页面与自定义设置）、`ui-kit`（基础组件、Toast、`UserContent` 唯一 HTML 出口）、`schema-form`（JSON Schema 表单与无 eval 校验）、外壳与页面（首页、登录 / 注册、种子搜索与筛选面板、详情、发布、个人页、后台插件列表与设置、插件路由）、主题令牌（亮 / 暗）、插件前端：levels、bonus、hnr、promotion、invite；按资源类型生成发布表单（`oneOf` 按 kind 切换、`x-pt-order` 字段顺序）、详情页字段展示、作品（Group）页；个人设置页（资料、安全与会话、passkey、插件标签页）、后台用户列表 / 详情（角色、限制、强制下线、`admin.user.tabs`）、种子页编辑与审核（`admin.torrent.tabs`）；作品页元数据面板（转存海报、简介、类型、评分、TMDB 署名）、发布表单 TMDB 搜索预填 | Markdown 编辑与预览、外部 ID 拉取、虚拟滚动、i18n 资源包按需加载、SDK 由 OpenAPI 生成 |
| 资源类型（type-movie、type-generic，WASM） | 内核 `content-type` 聚合调用（Native / WASM）；共享 crate `pt-release-parser`（发布名、季集、中文季集、标题）与 `pt-type-kit`（标题模板、字段错误、外部 ID 提取）；type-movie：字段 / Group schema、同义词规范化、IMDb / TMDB / 豆瓣链接提取、发布名补全、必填组合（原盘免视频编码）、分组键、规范标题（模板可设置）、`movie_*` 搜索字段；type-generic：kind 定义语言（校验、编译为 JSON Schema、`oneOf` 判别）、预设游戏 / 软件 / 电子书 / 课程、旧值映射、分组键、规范标题、后台新增 / 修改 / 停用、`generic_*` 搜索字段；torrent：上传时规范化与分组（Group 自动建立、`group.created`）、空标题用规范标题、`/v1/types`、`/v1/groups/{id}`、`v_group_public` / `v_group_fields`；元数据联动（`group.created` → `fetch_metadata` 延迟任务 → `ctx.metadata` → `ctx.torrents.update_group`：海报、简介、类型、片长、评分、国家、交叉 ID）；torrent 新增 `ctx.torrents`（`get_group`、`update_group`（`write:group`，只能更新本类型 Group，`group.updated`）） | 上传钩子规则（去重、trump、文件结构、标题策略）、元数据字段锁定（`locked_fields`）、词表与质量权重后台、`canonical-group-title`、新增 kind 后自动刷新搜索 schema |
| type-tv / type-anime（WASM） | `pt-type-kit` 新增 `specs`（影视规格的规范值、同义词、从发布名补全、显示写法，type-movie 改用）、`episodes`（季集字段与范围规范化、文件名集号解析 `SxxEyy` / `Eyy` / `第N集` / `[N]` / ` - N`、按范围检查文件列表）、`metadata`（剧 / 季统一模型 → Group 补丁）；type-tv：发布名推断季集与范围（单集 / 多集 / 季包 / 全集）与组合校验、按季分组（TMDB > IMDb > 豆瓣 > 标题 + 首播年）、作品标题“中文名 / 原名 第 N 季 / Season N”、规范标题 `S01E05`、`tv_*` 搜索字段、`torrent.before_upload` 按文件列表检查范围（忽略 sample 与 < 50 MiB 文件；完结季包按已有 Group 的集数 × 比例检查，连载中不检查）、剧 + 季元数据补全（季的海报 / 简介 / 集数优先，IMDb 先换算）；type-anime：字幕组风格发布名解析（`[组] 标题 - 05 […]`、`[组][标题][05][…]`、`【组】★…★`、区间与合集、标题中的季 `S2` / `Season 2` / `2nd Season` / `第二季`，无括号时交给通用解析器）、字幕语言 / 方式标签（简繁 / JPSC / 内封 / 内嵌 / 外挂）、字幕组别名（设置）、位深、形式（TV / 剧场版 / OVA / ONA / 特别篇）、按 Bangumi > AniDB（条目即季）> TMDB > 标题分组、`anime_*` 搜索字段、范围检查、TMDB 元数据；torrent：资源类型可给出作品标题（`group_title`）、首播年、钩子输入带已有 Group（`group_id`、`group_fields`）；作品页按 `<字段术语>.<值>` 显示枚举值 | Bangumi / AniDB 元数据源、季包按集号去重计数之外的完整性（缺集列表）、动漫番剧别名库、特别篇（S00）细分 |
| type-music（WASM） | 字段与 Group schema（艺人按角色分列：主艺人 / 客座 / 作曲 / 指挥 / 制作 / 混音 / DJ；发行类型；版本：介质、年份、厂牌、目录号、版本名）、规范化（格式 / 编码 / 介质同义词、格式与编码合法组合、无损格式默认 16bit、MusicBrainz 发行组链接、艺人字符串拆分与 `{name, role}` 对象、发布名补全 `Artist - Album (Year) [FLAC 24bit WEB]` 与场景风格 `Artist-Album-(CAT)-WEB-2023-GRP`）、分组键（MusicBrainz 发行组 > 主艺人 + 专辑 + 原始年份）、版本键、作品标题“艺人 - 专辑”（3 位及以上为 Various Artists）、规范标题（含 log 分数）、上传钩子文件检查（格式对应的音频文件、.log / .cue、可执行文件）、log 基础评分（EAC / XLD / whipper / CUERipper；安全模式、偏移、音频缓存、空隙、归一化、校验和存在性、逐轨 CRC 一致、测试拷贝、读取错误、AccurateRip；UTF-16 / UTF-8 解码）、`POST /v1/log-check` 预检、`torrent.created` / `torrent.updated` 后异步评分（`log_results` 表，最低分写回 `log_score`，上传者填的分数忽略）、`music_*` 搜索字段；torrent 新增 `ctx.torrents.update_fields`（`write:fields`）、Group 年份取 `original_year`；Schema 表单 `log-files` 控件（多文件、UTF-16 BOM 解码）与 `readOnly` 字段不显示 | 去重与替代（trump）、`no_log` / `log_100` 标签（torrent 标签接口）、MusicBrainz 元数据与封面、艺人页、驱动偏移表、日志校验和 / 签名验证、格式与编码启用开关 |
| llm-claude（Native） | `ctx.llm`（JSON 分派）：`complete`（profile 与作用域 `use` / `profile:<name>`、系统提示缓存断点与前缀消息断点、不可信内容 `<user_content>` 包裹与闭合标签转义及系统提示说明、工具与 `tool_mode`、结构化输出 schema 降级与本地子集校验、助手历史按 `provider_blob` 原样回放、`effort` 按模型省略（Haiku）、不发送 thinking 参数）、`models`、`embed`（501）；响应解析（文本、工具调用、stop、用量含缓存命中）；重试（429 / 5xx / 529 / 连接错误，`retry-after`，受截止时间约束）与错误映射（上游消息脱敏）；passkey / 邮箱 / IP 兜底脱敏；幂等（kv 24 小时）；每插件 / 每用户每日 token 配额（Redis）；`llm.usage_recorded` 与 `usage_daily`；后台用量查询（按插件 / 模型 / 日）与连通测试 | `llm-openai-compat`、调试日志、`count_tokens` 精确预估、启动时查询模型上下文长度、用量图表前端 |
| ai-upload-review（WASM） | `torrent.created` → `review` 任务；确定性规则（MediaInfo 缺失、只有压缩包（含分卷）、只有样片、要求 NFO 的类型）；模型预审（固定系统提示、可信事实与上传者内容分开、结构化输出、`cheap` profile、幂等键 `aiur:<种子>:<版本>`、每日预算、提供方未配置 / 拒答 / 结构化无效时只保留规则结果）；结论合并与 `model_disagreed`；报告表（每版本一份）；`ai.upload_reviewed`；HTTP 查看最新报告、同步重审、反馈、后台统计；torrent 新增 `v_torrent_review` 视图 | 规格与 MediaInfo 比对（ctx.mediainfo）、疑似重复（ctx.search）、标题规范度、审核页前端、`ctx.ai_upload_review` 服务、MCP 工具 |
| mcp（Native）与 MCP 注册 | 内核：清单 `[[mcp.tools]]` 聚合（名称规则、admin 前缀与 audience 一致、全站唯一、提供方须实现 `mcp-tool`）、`ctx.mcp_tools` / `ctx.mcp_call`（仅 `ctx.mcp` 提供方；主体须在本调用中认证；admin 工具要求 token 的 admin 作用域与 `mcp.admin_tools.use`；权限与 token 作用域经 `check_principal`；输入 schema 校验；写工具未确认返回 `mcp.confirmation_required`；以 `actor.kind = mcp` 调用 Native / WASM 导出，10 s；敏感字段名过滤；输出 schema 校验）、根路由（`[http] root_routes`，公开根路由由插件自行鉴权）、JSON Schema 子集校验移入 `pt-plugin-api::schema`；identity：个人 API / MCP token（`pts_` / `ptm_` + base62 + CRC32 校验和、只存哈希、作用域 read / write / admin、上限与有效期、Bearer 鉴权、`tok:<id>` 凭据 ID、封禁 / 待验证用户失效、只有浏览器会话能管理）；mcp：`POST /mcp`（2026-07-28 与 2025-11-25、`Mcp-Method` 校验、`initialize` 与无状态 HMAC 会话 ID、`ping`、`tools/list`（audience、作用域、权限过滤，写工具描述注明需确认）、`tools/call`）、`GET` 405、`DELETE` 204、受保护资源元数据、401 + `WWW-Authenticate`；写操作站内确认（先经内核检查权限与参数再保存、站内确认后立即执行并保存结果、重放返回结果、参数哈希一致性、拒绝、过期与清理 cron、站点可为非锁定工具关闭确认）；`mcp.before_tool_call`；结果字符串兜底过滤；事件与写操作审计；工具：`get_my_profile`、`admin_set_user_status`（identity）、`get_torrent`（torrent）、`get_my_level`（levels，WASM） | 按 token 限流、工具目录缓存、admin 作用域授权前的二次验证、其余目录中的工具 |
| mcp OAuth 与前端 | 授权服务器元数据；动态注册（RFC 7591：https / 回环 http / 原生应用自定义 scheme 回调、公开或机密客户端）；`/oauth/authorize` 校验（回调精确匹配、只允许 S256 PKCE、`resource` 必须是 MCP 端点、scope）后转到站内同意页，不可信参数错误不重定向；同意 / 拒绝（只允许浏览器会话，授权码 10 分钟一次性、只存哈希，回调带 `iss`）；令牌端点（授权码 + PKCE 校验、刷新令牌轮换、只能缩小作用域、同一用户与客户端只保留一份授权且旧访问令牌撤销）；访问令牌由 identity 签发（MCP token，1 小时，`ctx.auth.issue_token` 作用域 `tokens:issue`）；吊销（RFC 7009）；已授权客户端列表与吊销；web：mcp 插件前端（设置页“AI 客户端”标签：MCP 地址、个人 token 签发 / 撤销、已授权客户端、配置示例；用户菜单待确认提示；同意页；待确认操作页），浏览器端到端覆盖 token 签发与确认执行 | `/.well-known/openid-configuration`、客户端元数据文档（CIMD）、授权码与过期授权的清理任务 |
| ai-assistant（Native） | 规则文档管理（后台增改删，按二级标题切段、超长按空行再切）；插件内检索（拉丁词 + 中日韩二元组的类 BM25，标题加权，按等级过滤文档）；问答（固定系统提示、`<docs>` 资料与 `untrusted` 问题分开、工具循环 ≤ `max_tool_rounds` 且末轮禁用工具、以提问者身份经 `ctx.mcp_call` 调用清单 `capabilities.mcp_tools` 中的只读工具、拒答处理、引用标记只保留本轮检索结果）；多轮会话（本轮消息含 provider_blob 保存并原样回放）；每人每日次数与全站预算（Redis）；反馈、会话列表 / 删除、过期清理；MCP 工具 `ask_site_help`（单轮）；web：问答页、规则文档页、后台文档管理、用户菜单入口；内核：非 mcp 插件经 `capabilities.mcp_tools` 白名单调用只读工具，工具可声明 `timeout_ms`（≤ 120 s），工具调用上下文中主体视为已认证；新增 MCP 工具 `get_my_stats`（accounting）、`get_my_bonus`（bonus，WASM）、`list_my_hnr`（hnr，WASM） | 基于 `ctx.search` 的检索与混合检索、按等级（rank 而非 ID）过滤、答案流式输出、助手前端的 Markdown 渲染 |
| metadata-tmdb（WASM） | `ctx.metadata` 提供方：lookup（movie / tv / 季、imdb 经 /find 换算）、search、统一模型归一化（标题、年份、交叉 ID、类型映射、人员、评分、简介、署名）、PG 缓存（30 天、负缓存 1 天、上游失败时过期降级）、海报转存（类型与 ≤ 2 MiB 校验、内容寻址）、`metadata.fetched`、HTTP lookup / search | 速率限制、熔断、`refresh_stale`、翻译标题、背景图 |
| hnr（WASM） | `traffic.snatch_completed` 建档（豁免谓词规则：体积、本人发布、部分下载、促销、等级、注册天数；重下处理）、分层评估（临期每 10 分钟，其余每小时按用户哈希 1/6 轮转；做种时长或单种分享率达标，分母取 max(下载量, 体积)）、预警、计入与有效计数、越线经 `ctx.users.apply_restriction` 限制下载并在回落后解除（员工受保护时记录跳过）、每日过期重算、免除、种子删除 / 下架豁免、`torrent.before_download` / `invite.before_issue`、`ctx.hnr`、HTTP | 规则编辑接口、批量免除、盒子判定（等 seedbox）、预警通知（等 notify）、MCP、用户删除处理 |
| search-meilisearch | 贡献者 schema 合并（命名规则、保留 / 敏感字段、冲突记录）与设置生成、事件 → `pending_ops` → 延迟任务批量写入（文档哈希去重、删除优先、非拥有者失败降级）、发布名解析（`rel_*`）与标题规范化、swap 重建状态机（首次建索引、手动重建、双写与重放、文档数校验、取消）、查询（可见性基线、`search.before_query` 只追加且解析校验、结构化过滤、排序、facet、折叠、游标、高亮、Redis 结果缓存与代数失效）、联想、schema 视图、`ctx.search`、失败任务回收 | 语义 / 混合搜索、tenant token、多实例（replicas / network）、做种数部分更新、同义词管理与术语派生、一致性巡检、MCP、扩展索引注册、`users_admin` 等其他标准索引、熔断 |
| torrent | 严格 bencode、v1 / v2 / hybrid 解析、规范化（private、source、白名单）、上传（钩子、重复检测）、详情、文件列表、下载（passkey 注入、钩子、下载记录）、`torrents` 索引搜索贡献（可见性映射）、编辑（本人或 `torrent.update`，fields 重新规范化并按分组键迁移 Group，`torrent.updated`）、审核状态（`torrent.status_changed`）、软删除（`torrent.deleted`） | 资源类型插件接入、Group、状态机、编辑与替换、签名链接、批量下载、NFO |

## 5. 与设计文档的差异（实现时的决定）

- **Native 插件的 SQL**：除 `query`/`execute` 外增加 `ctx.sql.conn()`，直接拿到工作单元事务中的 `PgConnection` 使用 sqlx（仍在 `SET LOCAL ROLE` 之下）。
- **Native HTTP**：`HttpRoutes::router()` 返回 `axum::Router`，handler 用 `RequestCtx` 提取器取上下文（设计中为 `Router<Arc<NativeState>>`）。
- **服务调用**：`ctx.service::<dyn S>(name)?.call(|s, c| s.method(c, …))`，每次调用独立工作单元，超时取被调方截止时间。
- **InvocationInfo** 增加 `chain`（重入检测）、`authenticated`、`client_ip`、`user_agent`、`request_id`。
- **identity 邮箱**：按评审 U-1 推荐方案实现——`email_enc`（AES-256-GCM）+ `email_mac`（HMAC，唯一）+ `email_domain`，没有明文邮箱视图。
- **错误码后缀规则**：conventions/api.md §7 原先只引用未定义，已补充定义并实现。
- **ledger 币种设置**：内核设置接口完成前，可变设置存放在 `ledger.currency_settings` 表，管理员经 `PATCH /v1/admin/currencies/{code}` 修改。
- **ledger 冻结到期**：每个冻结创建时登记一个延迟任务 `expire_hold`（内核 cron 尚未实现）。
- **accounting 落库**：用 `unnest` 多行语句代替 COPY + 暂存表；用户日统计暂按 UTC 日期；`PartitionWorker` 采用 `poll(ctx, partition, fence)` 模型（accounting HLD §7 N-1），取代 plugin-api 初稿中的 `run(…, cancel)`。
- **网关幂等与插件幂等**：路由声明 `idempotent = true` 时，网关先按 `Idempotency-Key` 返回首次响应（`Idempotent-Replayed: true`）；ledger 等插件内部的服务级幂等仍然保留（服务调用没有网关）。
- **测试已知风险**：端到端测试中钩子处理者的超时为目录规定值（如注册钩子 500 ms），机器负载很高时偶发超时导致失败（2026-10-07 观察到一次，复跑 9 次未复现）。

- **WASM 业务服务**：v0.1 采用 JSON 分派（plugin-api.md §2.0），类型化服务 WIT 包推迟到 0.2。
- **WASM 插件构建**：`plugins/wasm` 为独立工作区，`cargo build --release --target wasm32-wasip2`；包目录为 `<plugins.dir>/<id>/{plugin.toml, plugin.wasm, migrations/}`，端到端测试自动构建并打包。
- **bonus 规则编译**：设计为编译结果 bincode 存 kv；当前每次调用从 `rule_versions.definition` 重新编译（单条规则集约数十微秒，后续需要时再加缓存）。
- **bonus 审核通过奖励**：torrent 状态机接入前，以 `torrent.created` 且初始状态为 approved 视为审核通过。
- **levels 等级表**：`classes` 增加 `is_default` 列明确默认等级（设计中由 rank 推断）；NexusPHP 预设中“农民 → 用户”滞回写成用户 promote = ratio ≥ 0.6、keep = ratio ≥ 0.5。
- **levels 角色同步**：identity 新增 `class_roles`（等级 → 角色），等级角色并入有效角色；插件只能绑定自身前缀角色或 rank ≤ 2 的内置角色。
- **搜索贡献者调用**：内核在 `Ctx` 上提供 `search_contributors` / `contributor_schemas` / `contributor_documents`（plugin-api.md §2.0.1），仅 `ctx.search` 提供方可用；WASM 贡献者逐条调用 `to-document`。
- **search-meilisearch 实现取舍**：运行时设置项（窗口、批量、缓存 TTL 等）暂放在静态配置；哈希以十六进制文本保存；索引拥有者与重建枚举视图由插件内的标准索引登记表决定（WIT 的 `owner`、`list-ids` 待 0.2）；flush 依赖 `FOR UPDATE SKIP LOCKED` 行锁而不另加 Redis 锁（同一实体只有一行待写，顺序由行锁保证）；`filterableAttributes` 用字段名列表（兼容所有 1.x）。
- **已知限制：延迟任务串行执行**：内核在持有 `k:lock:delayed-jobs` 的单个节点上逐条执行延迟任务，search 重建步骤等待 Meilisearch 任务时会推迟同期的 flush；后续改为按插件 / 任务名并发执行。
- **搜索结果缓存**：普通通道写入不使缓存失效（新文档最多滞后 `cache_ttl_secs`，默认 45 s），删除与优先通道写入立即失效（LLD §1.6）。
- **种子可见性映射（搜索）**：approved → member；candidate / pending → staff（上传者本人可见，匿名发布不写 `vis_owner_id`）；rejected / disabled / deleted 不进索引。
- **invite 实现取舍**：持有者规范化只做去空白与小写（WASM 中无 NFC 库；identity 在钩子与事件中给出同一显示形用户名，两处一致）；HMAC / HKDF / base64 在插件内用 `sha2` 自行实现（RFC 4231 / 5869 向量测试）；服务方法 `issue` 的来源记为 `api`；连坐的 `notify` 动作在 notify 插件接入前不执行。
- **identity 限制**：本版本以“同用户同类型同来源只有一条生效限制”的 upsert 保证幂等，不建 `restriction_requests` 表；不发布 `user.restriction_changed`；到期由每分钟的 `restriction_sweep` cron 处理（不调度逐条延迟任务）。
- **hnr 种子标签**：豁免谓词的 `tags` 字段在 torrent 提供标签前恒为空数组；`is_seedbox` 恒为 false。
- **钩子超时倍数**：`[kernel] hook_timeout_factor`（只在 dev 模式生效）放大钩子目录给出的超时，供调试构建与并行测试使用；生产保持目录预算。
- **已知限制：嵌套调用占用连接**：服务调用与钩子处理者各自开独立工作单元（各占一个连接），调用方的连接在等待期间不释放；连接池须不小于“并发请求 × 嵌套深度”（测试配置用 20）。后续考虑同插件链路复用或为嵌套调用预留连接。
- **前端选型调整**：构建用 esbuild（而非 Vite；内核直接提供构建产物，不需要开发服务器）；路由用 React Router 7（而非 TanStack Router）；包管理用 npm workspaces（而非 pnpm）；schema 表单校验用自写的无 eval 子集校验器（Ajv 依赖代码生成，严格 CSP 不允许 `unsafe-eval`）。
- **共享依赖打包**：React / ReactDOM / client / jsx-runtime 打进一个 `react-all` 模块，各说明符为 ESM 门面再导出（CJS 互操作下保持单实例）；`@tanstack/react-query`、`react-router` 与 `@pt-studio/*` 各自为独立模块；均写入 import map，插件与宿主把它们标记为 external。
- **插件前端位置**：源码在 `web/plugins/<id>/`，构建到 `web/dist/plugins/<id>/index.js`；清单 `[frontend] entry = "index.js"`，URL 为 `/static/plugins/<id>/<version>/index.js`（版本进路径，长期缓存）。
- **插件设置 schema**：清单内联 `[settings_schema]`（hnr 已提供），后台按 schema 生成表单；未提供时退化为 JSON 编辑。
- **资源类型的登记方式**：类型插件以 `capabilities.content_type = true` 声明 `content-type` 导出（与 `search_contributor` 一致），不在 `provides` 中声明 `ctx.content_types`（多个类型插件会被单实现服务解析视为歧义）；torrent 经 `ctx.content_types()` / `content_type_call` 聚合调用，类型信息缓存 30 s。
- **字段顺序**：JSON 对象经内核后键序不保证，类型 schema 用 `x-pt-order` 声明表单字段顺序。
- **外部 ID 字段**：客户端不按 `pattern` 校验 `x-pt-widget = external-id` 字段（允许粘贴链接），由类型插件规范化时提取。
- **type-generic 的 `pattern` 字段约束**：WASM 中未引入 regex，本版本不支持 kind 字段的 `pattern`。
- **改密码的再认证**：设计要求 reauth；本版本以“校验当前密码”代替，成功后撤销除当前外的全部会话。
- **元数据服务的形态**：`ctx.metadata` 作为多实现 JSON 服务（`MULTI_SERVICES`），元数据插件以 `service-provider` 导出 `info / lookup / search`，调用方传 `source`（路由到 `metadata-<source>`）；未使用 WIT 的 `metadata-provider` 导出（wasmtime 49 bindgen 对其 `lookup` 函数生成有误，见 plugin-api.md §2.0）。
- **出站的测试方式**：元数据插件的 API 与图片地址经静态配置（`config:api_base_url`，`allow_private`）指向本地模拟服务；生产默认 `api.themoviedb.org` / `image.tmdb.org`。
- **钩子输入带已有 Group**：钩子处理者不能回调 `ctx.torrents`（调用链重入），所以 torrent 在 `torrent.before_upload` 输入中直接带上已有 Group 的 `group_id` 与 `group_fields`（catalog/hooks.md 已补充）。
- **作品标题由类型给出**：规范化结果中的 `group_title` 作为新建 Group 的标题（剧集带季），不进入 Group 字段，作品页不显示。
- **测试种子的子目录**：`fixtures::sample_v1` 的文件路径用 `/` 分隔子目录（用于 sample 目录等场景）。
- **音乐艺人字段**：LLD 为 `artists[]` 对象数组（`{name, role, mbid?}`）；表单与搜索更适合字符串数组，本版本按角色分为 `artists`、`guest_artists`、`composers` 等字符串数组，规范化仍接受对象形态；MBID 留待 MusicBrainz 元数据接入。
- **log 存放位置**：按 LLD 放在种子字段 `logs[]`（作品页不显示）；种子字段整体受 torrent 表 64 KiB 约束，所以随种子保存的 log 总量上限为 56 KiB（LLD 为 5 个各 512 KiB；预检接口仍接受 512 KiB）。大 log 的独立存储（blob）留待后续。评分写回由 `ctx.torrents.update_fields` 完成，编辑字段后由 `torrent.updated` 重新评分（检查任务幂等，分数不变不写回，不会循环）。
- **`ctx.llm` 的形态**：v0.1 与其他业务服务一致，用 JSON 分派（请求 / 响应字段与 `pt:llm@0.1.0` WIT 一一对应，内容块以 `type` 区分：`text`、`untrusted`、`tool_call`、`tool_result`）；类型化 WIT 推迟到 0.2。
- **llm-claude 用量汇总**：每次调用在同一工作单元内直接累加 `usage_daily`（设计为自订阅 `llm.usage_recorded` 后累加），事件仍照常发布。
- **服务端回退默认关闭**：`fallbacks_enabled` 默认 false（beta 能力，开启前须按当期 API 文档核对请求头与参数）。
- **未配置的 profile**：调用方请求的 profile 未在 site.toml 配置时退回 `default`（作用域检查仍按请求的名字）。
- **MCP token 作用域**：identity 设计的 MCP token 作用域为 read / write / admin，mcp 设计写作 `mcp:user` / `mcp:admin`；本版本统一用前者（读工具需要任一作用域，写工具需要 write，admin 工具需要 admin）。
- **公开根路由不经网关鉴权**：`/mcp` 等根路由声明为 `auth = "public"` 时网关不处理凭据，由插件自行认证 Bearer（否则只读 MCP token 的 POST 会被“只读 token 不能写”规则拦下）。
- **token 凭据 ID**：Principal 的 `credential_id` 为 `tok:<id>`（会话为 UUID），插件据此区分浏览器会话与 token（如 passkey、token 管理、站内确认只允许会话）。
- **token 管理的再认证**：设计要求签发 token 前 reauth；本版本以“只有浏览器会话能签发”代替，reauth 接入后补上。
- **确认的执行身份**：站内确认后以当前浏览器会话身份执行工具（发起调用的 token 已在创建确认项时通过内核的权限与参数检查）。
- **助手的检索**：设计经 `ctx.search` 建自有索引；规则文档规模小（数百段），本版本在插件内打分（不依赖 Meilisearch），之后数据量变大再接入搜索。
- **非网关插件调用 MCP 工具**：设计中 ai-assistant 直接用 `mcp_registry.call`；内核以清单 `capabilities.mcp_tools` 白名单开放（只读工具），提问者主体为 `session` 类、作用域 read。
- **ai-moderation 暂缓**：依赖的评论、论坛、举报插件尚未实现，届时一并实现。
- **测试已知风险（2026-10-13）**：完整测试并行负载很高时，`e2e_wasm` 的沙箱越界用例偶发一次错误码不符（单独运行与随后的完整运行均通过）；待复现时记录具体用例与返回值。
- **手动运行 cron**：`Kernel::run_job_now` 在任务正在运行（如启动时 catch_up）时最多等待 30 s。

## 6. 进度记录

| 日期 | 内容 |
|---|---|
| 2026-10-06 | P1：内核骨架、identity、torrent |
| 2026-10-07 | P2：ledger、accounting、分区 worker、插件分区维护、网关限流与幂等键 |
| 2026-10-07 | cron、accounting 小时快照、bonus 小时结算与批量入账（WASM） |
| 2026-10-07 | P3：WASM 宿主、`pt:plugin@0.1` WIT、JSON 分派服务、bonus（WASM 试点）、沙箱越界测试插件 x-chaos |
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
