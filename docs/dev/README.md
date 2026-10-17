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

### 2.1 一键运行站点

```bash
deploy/dev/run-site.sh                 # 检查 / 启动依赖、建库、构建服务端 + WASM 插件 + 前端、打包插件、生成配置、迁移、启动
PT_PROFILE=debug PT_SKIP_BUILD=1 deploy/dev/run-site.sh   # 已构建过时直接启动（debug 构建）
```

数据与配置在 `.dev/site/`（`site.toml` 由 `deploy/site.example.toml` 生成，随机密钥）。打开 http://localhost:8080 注册账号后：

```bash
target/release/pt-server --config .dev/site/site.toml user grant-role <用户名> admin
```

冒烟检查（浏览器走一遍注册、发布、下载、后台）：

```bash
PT_BASE=http://localhost:8080 PT_SERVER=target/release/pt-server PT_CONFIG=.dev/site/site.toml node web/tests/smoke.mjs [截图目录]
```

### 2.2 手动运行

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
| identity | 注册（规范化、钩子、加密邮箱与 passkey）、密码登录、会话、登出、`ctx.auth` / `ctx.rbac` / `ctx.users`、内置角色继承、权限登记、授予角色、等级（`set_class`、`user.class_changed`）、等级绑定角色（`class_roles`，插件定义带前缀角色）、`roles_of`、限制与有效状态（施加 / 解除 / 到期清扫、`user.status_changed`、封禁使会话失效、插件不能限制员工、管理员 rank 检查、`ctx.users.set_status` / `apply_restriction` / `lift_restriction`、`ctx.rbac.check` JSON）、本人设置（资料 merge-patch、改密码并撤销其他会话、会话列表与撤销、passkey 重置；`user.profile_updated` / `password_changed` / `passkey_reset`）、后台用户（列表筛选与游标、详情、编辑、强制下线、移除角色）；账号安全 M1.1（登录限流：用户名维度指数延迟与软锁、IP 封锁，Redis 不可用时放行；设备 cookie `__Host-pt_dev` 与已知设备豁免软锁；IP 记录 `ip_log`（登录、会话 IP 变化）、本人 IP 历史（默认只显示网段）、管理员 IP 历史 / 共用 IP 账户 / 按 IP 或 CIDR 查账户（审计）、安全事件、保留期 cron `ip_retention`（截断后仍可按 HMAC 查同 IP）、`user.logged_in` 的 `new_device` / `alert`，notify 订阅后发新设备提醒；运行时设置 schema）；M1.2 邮件挑战（`challenges`，HMAC 令牌只存哈希、同用途旧挑战作废、一次性、统一 `identity.challenge_invalid`）：邮箱验证（设置 `require_email_verification`，HTTP 注册后为待验证、只有访客权限，验证后解除）、重发验证邮件、找回密码（永远 202、新密码策略校验失败不消耗链接、撤销全部会话、同时视为邮箱已验证）、`user.challenge_issued` / `user.verified`、`ctx.auth.challenge_delivery`（作用域 `challenges:deliver` + `user.email`，每挑战最多 3 次投递）；notify 订阅挑战事件、outbox 只存挑战 ID，投递时取收件人与链接；web：忘记密码、重置密码页、验证邮箱页（读取后立即从地址栏去掉令牌）、未验证提示条与重发；M1.3 两步验证（TOTP：RFC 6238 SHA-1 / 6 位 / 30 秒，±1 步、`last_step` 行锁防重放，密钥 AES-GCM 加密；绑定时服务端生成二维码 SVG；10 个恢复码加盐哈希、一次性；登录第二步票据（Redis，5 分钟、5 次、GETDEL 原子取出）；`POST /sessions/current/reauth`（密码或第二因素）配合路由 `reauth_secs`；设置 `two_factor_required_role`：该角色及以上的会话未通过 2FA 时剔除这些角色（会话 Principal 的 `2fa` 作用域，网关与 `/users/me/permissions` 同一规则）、不能关闭；`user.two_factor_changed`）；内核向插件静态配置注入只读 `_site`（名称、地址、时区、默认语言）；web：登录第二步、两步验证卡片（二维码、恢复码、重新验证弹层）、员工未开 2FA 提示条；M1.4 WebAuthn（自行实现：最小 CBOR 解码、认证器数据、COSE 公钥 ES256 / EdDSA / RS256、clientData 类型 / 挑战 / 来源校验、rp_id 为站点主机名（可用 `webauthn_rp_id` / `webauthn_origin` 覆盖）；登记只取公钥（等同 attestation none）、`credProps.rk` 标记可发现凭据、第一个第二因素时给出恢复码；作为第二因素（票据 + 断言选项）与免密码登录（要求 UV，视为通过 2FA）；签名计数器不递增时拒绝并记安全事件；仪式状态存 Redis 一次性；删除最后一个第二因素时恢复码作废）；web：安全密钥管理、登录页“用安全密钥登录”、第二步“使用安全密钥”，浏览器测试用 Chromium 虚拟认证器；M1.5 人机验证：identity 在登录 / 注册 / 找回密码前调用可选服务 `ctx.captcha.verify`（未安装插件时放行，失败 `identity.captcha_failed`），请求顶层字段 `captcha_token` | 改名、改邮箱、Redis 会话缓存、GeoIP、token 的 IP 记录、UTS #39 混淆骨架 |
| ledger | 币种注册、发行 / 回收 / 转账（手续费、每日限额、`ledger.before_transfer`）、批量发行、幂等、冻结（到期由延迟任务释放）/ 扣款、账户锁定、冲正、流水分页、供给统计、用户转账与管理员调整 / 冲正 / 设置 HTTP | 日对账、月结余快照、归档、用户删除处理、MCP 工具 |
| accounting | `acct:<p>` 微批消费、eid 去重、拒收记录、倍率规则（favorable / override、索引与版本号刷新）、peers、snatches、首次完成与 `traffic.snatch_completed`、用户总量与日统计、结算窗口与 `traffic.settled`、分段明细、游标 CAS（恰好一次，重启不重复计入）、peers 小时做种槽与小时快照（折叠做种时长、`traffic.seeding_snapshot_taken`）、流量调整、种子镜像、统计 HTTP、基础信号（速度含 tracker 截断段 / 负增量 / 计数异常含 `started_nonzero` / 无对端，去重与 `traffic.signal_raised`）、每小时交叉校验、`ctx.accounting.signals`、后台信号列表、`v_swarm_peers` | swarm 统计、实时计数扣减、重算、流修剪、客户端指纹信号 |
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
| 社区：pt-markup、comments、reports（WASM） | `pt-markup`：Markdown（pulldown-cmark，表格 / 删除线 / 任务列表，原始 HTML 当作文本，标题降级为 h3～h6）与 BBCode（有界解析，嵌套 ≤ 32、未知 / 非法标签原文输出、未闭合自动闭合、颜色与字号白名单）→ 自行输出的安全 HTML，链接只允许 https / http / 站内路径（`rel="nofollow noopener ugc"`），图片只允许站内前缀或白名单主机，@提醒解析，输入 ≤ 64 KiB；comments：种子 / 作品目标（视图判断可见性，待审种子只对发布者与版主可见）、发表（楼层、单层回复、引用、`comment.before_create`、幂等键）、防刷（30 秒 / 每小时 / 新用户每日、simhash 近似重复、链接数）、列表（楼层分页、回复聚合、感谢）、编辑（时限、历史）、删除（占位）、感谢、预览、用户历史、`ctx.comments`、事件；reports：理由目录、同一目标合并工单、重复 / 自举报 / 频率 / 无效举报暂停、认领、处理动作（删除评论、禁用种子、限制用户，部分失败 207 不结案）、我的举报、`ctx.reports`、MCP `admin_list_reports` / `admin_resolve_report`；torrent 新增 `ctx.torrents.set_status`（`write:status`）；ui-kit 净化放行渲染器生成的 class / 样式（只留颜色与字号）与只读复选框；web：评论区（种子与作品 tabs）、举报按钮与工单处理页 | 图片上传、表情、@提醒通知（ctx.notify）、重新渲染任务、举报的通知与快照清理、论坛等其他举报目标 |
| moderation、ai-moderation（WASM） | moderation：上传初始状态规则（`torrent.before_upload`，按角色 / 等级 / 已通过与被拒数 / 被拒比例 / 账号天数 / 类型 / 体积匹配第一条，只能变严，未命中时可设默认状态）、审核队列（`torrent.created` / `status_changed` / `updated` / `ai.upload_reviewed` 维护，优先级 = 等待小时 + AI 结论权重）、认领与冲突、决定（通过 / 拒绝经 `ctx.torrents.set_status`，退回修改后上传者编辑即回到待审）、决定日志与快照、上传者统计、候选区投票（不能给自己投、账号天数、净票数达标即决定）、敏感词（评论：拦截不回显 / 遮蔽 / 复核记录）、`ctx.moderation`、MCP `admin_list_moderation_queue` / `admin_decide_moderation`；ai-moderation：`comment.created` 抽样（新用户必审）→ 延迟任务、本地规则（新账号大量链接、垃圾域名、刷屏、passkey / 邮箱 / 手机号）、模型分类（固定提示、正文为不可信内容、结构化输出、预算、拒答记为待抽查）、达到阈值以系统身份建举报工单（`ai.content_flagged`），抽查列表、工单结论反馈与按类别准确率、发表前钩子（只用本地规则，默认关闭）、`ctx.ai_moderation.classify`；web：审核侧栏面板、审核队列页、候选区页、员工菜单队列数 | `torrent.before_status_change` 防绕过钩子与搜索可见性钩子、理由模板与规则编辑器、通配符敏感词与 Aho-Corasick、批量审核、论坛与私信的 AI 审核、ai-moderation 抽查页前端 |
| notify、messages（WASM） | notify：`ctx.notify.send` / `send_many`（作用域 `send`、模板键前缀必须是调用方、类别、按类别偏好（security 不可关）、幂等键）、通知中心（列表、只看未读、未读数、标记已读）、偏好、清理；模板由前端按术语 `<key>.title` / `<key>.body` 插值渲染，存储的标题正文为回退文本；接入方：comments（@提醒、回复）、moderation（审核决定通知上传者）、reports（结论通知举报者）；messages：会话（按用户名 / ID 收件、去重、拉黑、只接收员工、员工不受限、频率、`message.before_send` 钩子）、回复（回到对方收件箱、未读数）、打开即已读、移出、收件箱 / 已发送 / 系统文件夹、pt-markup 渲染、`ctx.messages.send_system`、拉黑与接收设置、举报私信（同意后快照经 `ctx.reports` 提交；reports 新增 message 目标与理由）、MCP `list_my_messages`（不含正文）/ `send_message`（需确认）；web：通知菜单与通知中心、通知偏好标签、站内信菜单 / 文件夹 / 写信 / 会话（回复、举报、拉黑、删除） | Telegram / Discord 渠道与绑定、合并通知、服务端模板渲染（邮件目前用调用方给出的回退文本）、mailgun / SES 提供方、群发公告、星标、私信清理任务（邮件渠道见下一行） |
| checkin、requests（WASM） | checkin：每日签到（`checkin.before_check_in`、连续天数、按日唯一）、奖励（基础 + 每连续一天递增、上限、里程碑，经 `ctx.ledger` 发行，幂等键按用户与日期，发放失败签到仍成立并记录状态）、状态（今日、连续 / 最长 / 累计、本月日历、下次奖励）、当日名次、连续排行、`checkin.checked_in`、MCP `check_in`（免确认）；requests：发布（类型、标题、Markdown 描述、规格要求谓词、`request.before_create`）、悬赏托管（经 `ctx.ledger` 从贡献者回收，幂等键按贡献）与追加、投票、应求（已通过、同类型、规格要求逐条检查（版主可放宽）、不能应求自己、同时只有一个待确认）、确认（求种者 / 版主，扣手续费后发行给应求者）、驳回、撤回与到期退款、待确认超时自动确认、通知（应求待确认、确认、驳回）、`ctx.requests`、`v_request_public` 视图、MCP `list_requests` / `create_request`；comments 新增 `request:<id>` 目标（已关闭的求种不能评论）；web：签到入口与签到页（日历、排行）、求种列表 / 发布 / 详情（悬赏、投票、应求、确认、撤回、评论区） | 补签卡、随机奖励、站点时区；求种的认领、撤销已发放的悬赏、对账、上传时选择应求 |
| rss（Native）与邮件通知渠道 | rss：feed key（identity 新增 kind = feed 的 token，前缀 `ptf_`，只对签发插件有效、不过期、不能当 API token 使用；重置即吊销旧 key；最近使用时间与 UA）、订阅增删改查（类型、关键词、字段条件 eq / in / has / gte / lte、促销、体积、排除词；每人上限）、订阅 XML（RSS 2.0 + torznab 属性：分类、体积、做种 / 下载人数、完成数、infohash、上传 / 下载系数、imdb / tmdb / tvdb / 季 / 集；60 秒缓存）、Torznab（caps / search / tvsearch / movie / music / book，Newznab 分类映射与 `cat` 过滤、Newznab 错误 XML）、凭 key 下载（以 key 的用户身份调用 `ctx.torrents.build_download`，与网页下载同一份文件与同样的检查）、每 key 每分钟上限；accounting 新增 `v_torrent_peers` 视图；identity 新增 `ctx.users.get_sensitive`（email，内核以 `sensitive:<能力>` 作用域传入 site.toml 授权的敏感能力，清单自行声明无效）；notify 邮件渠道：类别 × 渠道偏好（security / account 默认发邮件）、每人每小时上限、待投递记录 + 延迟任务（投递时读取已验证邮箱，4xx 永久失败，429 / 5xx 由内核重试）、提供方 http_relay / Postmark / SendGrid、一键退订（HMAC 令牌、List-Unsubscribe / List-Unsubscribe-Post）；web：设置页“RSS”标签（key、订阅、Torznab 说明）、通知偏好改为类别 × 渠道表格、退订确认页 | 订阅条件经 `ctx.search`（相关度、可筛选字段校验）、hnr 的 minimumratio / minimumseedtime 属性、ETag；mailgun / SES、邮件 HTML 模板与服务端渲染 |
| P7：API 冻结、SDK、模板、文档 | WIT 定为 `pt:plugin@1.0.0`（`invocation-info` 补 `request-id` / `client-ip` / `user-agent`；清单 `api = "pt:plugin@1"`，内核接受同主版本、次版本不高于自身；冻结快照 + 测试保证已冻结接口不变、模板中的 WIT 副本同步）；`pt-plugin-sdk`（宿主绑定生成一次，插件经 `world!` 宏以 wit-bindgen `with` 复用；`err`、`SqlValue` 转换与 `RowExt`、`ctx`、`http::Reply` / `RequestExt`、`logging`），全部 WASM 插件改用 SDK 绑定，checkin 为参考插件；第三方扩展：清单 `[[hook_points]]` 的 `modifiable` / `timeout_ms` / `deny_on_error`（目录之外、`<id>.` 前缀的钩子点由内核按清单登记）、清单 `[error_codes]`（HTTP 状态），后缀规则支持恰好等于后缀词的错误名；`pt-server plugin check` / `plugin package`（不需要数据库）；模板 `templates/wasm-plugin`（notes，e2e 构建并加载，Native 测试插件处理其钩子点）与 `templates/native-plugin`（greeter）；文档：插件开发指南 `docs/sdk/README.md`、根 `README.md`、`llms.txt`，plugin-api.md 字段表与版本策略更新 | 钩子输入按 schema 运行时校验、插件包内前端分发、其他语言 SDK、从 schema 生成类型化事件 / 钩子结构、模板仓库独立发布 |
| P8：可运行站点与 tracker | 一键运行 `deploy/dev/run-site.sh`（依赖检查 / 启动、独立角色前缀建库、构建、打包插件、生成配置、迁移、启动 pt-server 与 pt-tracker）、`deploy/site.example.toml`、Docker（`deploy/docker/`：多阶段镜像、compose，**未在本环境验证**）、`pt-server user grant-role`、浏览器冒烟检查 `web/tests/smoke.mjs`（注册、发布、下载、模拟 BT 客户端 announce、流量到账、后台）；tracker-bridge（Native）：启动 / 每 10 分钟全量同步与事件实时同步 `trk:pk` / `trk:t` / `trk:clients` / `trk:cfg` / `trk:sync`（Lua 原子写快照 + 变更流，`ver` 由 `trk:ver` 生成），换 passkey 删旧键，默认客户端白名单；pt-tracker（独立程序，AGPL，自研）：announce / scrape / health、本地准入缓存 + 变更流 + 回源负缓存、**Redis 中的 swarm 与会话状态**（Lua 脚本 CAS、超时清理、随机抽取 peer，无状态多节点）、紧凑与非紧凑响应（IPv4 / IPv6、`no_peer_id`）、增量计算（基线、回退、截断、首次完成）、记账流 `acct:*` 与 `u:*:live`；Redis 集成测试、压测工具 `pt-tracker-bench`（约 1.05 万次 / 秒）、真实客户端测试 `web/tests/bt-real.mjs`（Transmission 4.1，32 MiB 传输与记账正确）；修复：搜索缓存在新文档入库后失效、个人页分享率解析、首页 / 种子页的若干界面问题 | UDP、节点心跳与后台 peer 查询、客户端规则后台编辑、Deluge / µTorrent 实测（qBittorrent 5.1 已于 2026-10-17 实测，见 crates/pt-tracker/README.md） |
| metadata-tmdb（WASM） | `ctx.metadata` 提供方：lookup（movie / tv / 季、imdb 经 /find 换算）、search、统一模型归一化（标题、年份、交叉 ID、类型映射、人员、评分、简介、署名）、PG 缓存（30 天、负缓存 1 天、上游失败时过期降级）、海报转存（类型与 ≤ 2 MiB 校验、内容寻址）、`metadata.fetched`、HTTP lookup / search | 速率限制、熔断、`refresh_stale`、翻译标题、背景图 |
| hnr（WASM） | `traffic.snatch_completed` 建档（豁免谓词规则：体积、本人发布、部分下载、促销、等级、注册天数；重下处理）、分层评估（临期每 10 分钟，其余每小时按用户哈希 1/6 轮转；做种时长或单种分享率达标，分母取 max(下载量, 体积)）、预警、计入与有效计数、越线经 `ctx.users.apply_restriction` 限制下载并在回落后解除（员工受保护时记录跳过）、每日过期重算、免除、种子删除 / 下架豁免、`torrent.before_download` / `invite.before_issue`、`ctx.hnr`、HTTP | 规则编辑接口、批量免除、盒子判定（等 seedbox）、预警通知（等 notify）、MCP、用户删除处理 |
| metadata-douban（WASM） | 不内置爬取：`proxy` 模式（站点自建代理，LLD §1 接口）或 `ptgen` 模式（PT-Gen 兼容端点，如 pt-gen-cfworker 的字段）；默认关闭，管理员确认风险声明后启用（KV + 审计）；`ctx.metadata` 的 `douban` 提供方（`info` / `lookup` / `search`，ID 为数字或条目链接）、归一化到统一模型（中文类型 → 统一键、地区 → ISO、评分、人员、简介去标签）、PG 缓存 30 天与过期降级、Redis 负缓存 1 天、海报转存（`image_hosts`、带 Referer、类型与大小校验）、`metadata.fetched`；`GET /v1/ptgen?markup=bbcode|markdown|text` 生成 PT-Gen 版式简介（◎译名 / 片名 / 年代 / 产地 / 类别 / 语言 / 上映日期 / IMDb / 豆瓣评分 / 片长 / 导演 / 主演 / 简介）；插槽 `torrent.upload.extra` 增加 `update(patch)` 回填；web：发布页“从豆瓣生成简介”、后台启用页 | 速率限制与熔断、`refresh_stale`、图书条目、资源类型自动补全接入 |
| mediainfo（WASM） | 解析 MediaInfo 文本（英文 / 简体 / 繁體标签，段 General / Video / Audio / Text / Menu）、MediaInfo JSON（`media.track[]`）与 BDInfo（完整报告与 Quick Summary）；脱敏（完整路径只留文件名，`/home/x/`、`C:\Users\x\` 等替换为 `…/`，JSON 按解析结果重新输出）；规格摘要（容器、分辨率按宽高、视频编码、HDR、位深、帧率、时长、音轨与音频编码、字幕语言，取值与 type-kit 规格一致）；与种子声明字段比较（分辨率、视频编码、HDR、音频编码；检测不到的不计）；插件自存结果（每种子一行，限 256 KiB），发种者或 `mediainfo.manage` 可改 / 删；`POST /v1/parse` 预览、`ctx.mediainfo.parse/compare/get`、`mediainfo.updated`；种子删除时清理；插槽 `torrent.upload.extra` 增加 `afterUpload(cb)`；web：发布页粘贴即预览（差异提示）、种子页规格卡片与编辑 | ai-upload-review 改读 `ctx.mediainfo`（torrent 的 `mediainfo` 列只在 API 直传时有值）、NFO 解码（CP437）、发布时强制要求 / 差异阻止发布、按解析结果自动补全字段、审核页展示差异 |
| bookmarks（WASM） | 设计见 docs/plugins/bookmarks/README.md：收藏（私有、上限、只收已发布种子、种子页人数）、合集（标题 / 说明 / 公开私有、条目备注与排序、公开合集列表与标题搜索、作者或 `bookmarks.collections.manage` 编辑）；种子删除时移除；`ctx.bookmarks.list/is_bookmarked`；web：种子页收藏按钮与“加入合集”、侧栏“收录于合集”、我的收藏、合集列表 / 详情 / 编辑 | RSS 收藏订阅、收藏种子的新版本 / 促销通知、合集关注与封面 |
| top10（WASM）与 torrent 置顶 | top10（设计见 docs/plugins/top10/README.md）：完成数 / 最活跃 / 做种最多的种子，上传量 / 做种时长 / 发布数的用户（可关闭、用户可退出）；每小时快照；web：排行榜页、主导航。torrent：`sticky_until`（`POST /v1/admin/torrents/{id}/sticky`，`torrent.update`，天数或永久、审计）、搜索字段 `sticky`（可排序），列表“最新”排序置顶在前并带标记，`unsticky` 任务每 5 分钟清除到期置顶并重建索引；种子页管理区置顶 / 取消 | 按类型分榜、魔力值 / 分享率榜、首页小组件；置顶分级（NexusPHP 的一级 / 二级置顶） |
| attachments（WASM）与 torrent 简介渲染 | 设计见 docs/plugins/attachments/README.md：PNG / JPEG / GIF / WebP 按文件头识别、只解析容器去除 EXIF / XMP / 文本块（不解码像素）、SHA-256 内容寻址存 blob 并经 `/media/attachments/` 公开、体积 / 边长 / 每日张数与总量限制、同用户去重、删除按引用计数移除文件（管理员删除记审计）；torrent 详情 `description_html`（BBCode 渲染，站内图片与 `description_image_hosts` 外链）；web：我的图片、后台最近上传、发布页“插入图片到简介” | 编辑器内上传按钮（M4.5）、缩略图 / 转码、按等级配额、对象存储与 CDN |
| subtitles（WASM） | 种子级字幕（实现说明见 docs/plugins/subtitles/02 末节）：SRT / VTT / ASS / SSA 内容校验与编码识别、SUP 文件头、ZIP 中央目录检查（白名单、加密 / zip64 / 路径穿越、条目数与解压总量），7z / RAR 拒绝；内容寻址私有 blob、下载接口与计数、重复检测、每种子上限、匿名、可选上传奖励（ledger 幂等）、上传者或管理员（需原因、审计、通知）删除、种子删除清理；`ctx.subtitles.count_many/list_for`；web：种子页“字幕”标签页（列表 / 下载 / 上传 / 删除）、后台最近上传 | Group 级字幕、7z 与流式解压检查、`subtitle.before_upload` 钩子、列表页字幕标记、字幕求档 |
| dupe（WASM） | 设计见 docs/plugins/dupe/README.md：`torrent.before_upload` 钩子拒绝内容相同（`content_fp`）的重复发布；同作品同规格槽位（可配置字段）`off / review / block`，review 发布后进入审核队列；替代申请（新种子发布者或管理员、同作品、旧种子已发布、同时只能一个）与审核（批准后旧种子 disabled、`torrent.trumped`、通知下载过的用户、审计）；种子删除时清理；web：种子页“版本与替代”卡片、后台审核队列、仪表盘待办。测试夹具 `sample_v1` 的 piece 改为随名称与文件列表变化 | 按类型的共存规则、发布页提前提示、文件列表相似度 |
| forum（WASM） | 分区与版块（读 / 回复 / 发主题按最低等级 ID 或角色、版块版主、隐藏版块对无权者 404）、主题与楼层（Markdown / BBCode 经 pt-markup 渲染、引用、作者编辑窗口与编辑历史、版主编辑审计、作者删回复 / 版主删主楼即删主题、感谢）、版主操作（置顶、锁定、精华、全站公告（论坛管理员）、移动，审计）、投票（多选上限、改票、截止）、已读（`topic_reads` 按楼层、“全部标为已读”）与版块 / 主题未读标记、订阅（作者自动订阅）与汇总通知（排除已读者与最后发帖者）、首页公告、`forum.before_post` 钩子、防刷、`ctx.forum.create_topic` / `topic_for`（讨论帖）、后台分区 / 版块 / 版主管理；web：版块首页、主题列表与发主题（可带投票）、主题页（分页、引用、感谢、编辑 / 删除、投票、订阅、版主工具栏）、主导航入口、首页公告、后台页 | 合并 / 拆分主题、站内搜索贡献、@提醒、版块排序与编辑界面、阅读记录清理任务 |
| claims（WASM） | 种子认领（设计见 docs/plugins/claims/README.md）：认领条件（开关、每人 / 每种上限、需下载完成、发布天数、取消冷却），基线 = 认领时的累计做种秒数；月结（每月 1 日，结算本月之前开始的认领，按覆盖时长折算要求、达标奖励魔力值、连续不达标取消、按人 `settlements` 幂等、`ctx.ledger` 幂等发行、通知）；种子删除时取消；`ctx.claims.count_of`；web：种子页认领按钮与人数、用户菜单、我的认领页（本月进度） | 认领列表的种子页标签、等级条件接入认领数、认领奖励按体积 / 做种人数加权 |
| newbie-exam（WASM） | 考核定义在设置中（1～5 阶段、每阶段 1～8 目标，指标 `uploaded_gib` / `downloaded_gib` / `seed_hours` / `uploads_count` / `bonus` 为考核期增量，`ratio` 为当前值），分配时快照定义与基线；按注册方式自动分配（默认关闭）、管理员批量分配（重复报错）、免考等级；评估（`traffic.settled` 标脏 + 每 20 分钟脏用户与一小时内到期、每小时全量）、阶段推进、预警、未通过 none / restrict（identity 禁止下载）/ ban（需 `allow_ban`）、补救期内完成最后阶段自动解除、通过奖励魔力值（`ctx.ledger` 幂等发行，失败由全量任务补发）；钩子 `invite.before_issue`；`ctx.exams.status`、`GET /v1/me`（无考核 204）、后台分配 / 延期 / 豁免（审计）；web：首页进度小部件、考核页、后台用户标签页 | 多套考核、勋章奖励（等 medals）、MCP、批量操作界面 |
| seedbox（WASM）与 accounting 盒子标记 | seedbox：用户登记盒子 IP 段（IPv4 ≥ /24、IPv6 ≥ /48，拒绝私有 / 保留段，主机位归零，每人上限）、审核（批准 / 拒绝、可设过期、过期 cron）、与他人网段重叠记录给管理员、删除即删网段、视图 `v_ranges`（已批准且未过期）、`ctx.seedbox.is_seedbox` / `has_box`、事件 `seedbox.registered` / `reviewed` / `removed`；accounting：解析记账流中的 peer IP，按 `seedbox.v_ranges`（每分钟重载；未安装 / 未授权时按 oid 检查后视为空，不让事务出错）标记 `peers.is_seedbox`，`v_swarm_peers` 与种子页 peer 列表带 `is_seedbox` / `seedbox`；web：设置页“盒子”标签、后台审核页、peer 列表“盒子”标签 | ASN 数据中心自动识别；H&R 豁免、魔力值系数、反作弊放宽等规则接入 is_seedbox |
| ratio-policy（WASM） | 要求分享率按下载量分档（默认 Gazelle 表，设置中按 GiB 编辑）、做种减免（仍在做种的已完成种子比例）、豁免（下载量阈值、注册宽限期、等级、管理员豁免）；状态机 ok → warned（警告期）→ restricted（`ctx.users.apply_restriction` leech_disabled，员工受保护时记 restricted_skipped）→ 可选 banned，恢复后自动解除本插件的限制，手动警告不因恢复提前解除；`traffic.settled` 标脏 + 每 15 分钟增量、每日全量（处理到期、清理两年前历史）；钩子 `torrent.before_download`（警告期禁止新种子，已下载过的不受影响，可选）、`invite.before_issue`；`ctx.ratio.status`、`GET /v1/me`、后台警告 / 解除 / 豁免 / 列表（审计）；通知（account 类别）；web：个人页要求分享率、考核页（要求表）、后台用户标签页与警告列表 | MCP 工具、历史分区、要求表编辑器（目前为 schema 表单）、首页警告横幅 |
| registration-policy（WASM） | `user.before_register`（优先级 20，在 invite 之后）：通道（开放 / 邀请；管理员、服务建号不受限）、开放注册开关与每周时段（跨午夜、站点时区取常见时区的固定偏移或设置 `utc_offset_minutes`）、限时开放窗口（可限名额，窗口之间不重叠）、邀请注册开关、全站用户上限、同 IP / 同网段上限（只用 identity 的 `ip_hmac` / `ip_prefix_hmac`）、邮箱域名黑白名单（精确 / `.后缀`）、开放注册审核（`initial_status = pending`，identity 写 `identity:review` 待审核限制，管理员解除即通过）；名额占位（钩子中预留 10 分钟、`user.registered` 确认、`claim_locks` 串行化、IP 类占位确认后不关联用户）、清理 cron；`GET /v1/status`、窗口管理接口（审计）；web：注册页开放状态提示、后台窗口管理页 | 问卷、申请制、设备上限、紧急开关单独接口、统计、预演 |
| captcha（WASM） | `ctx.captcha.verify {action, token, ip?}`（Turnstile / hCaptcha / reCAPTCHA v2 siteverify，表单编码、5 秒超时；服务商、site key、密钥在 site.toml，未配置或操作未开启时 `required = false`；服务不可用按设置 `fail_open` 放行或 `captcha.unavailable`）、`GET /v1/config`（不含密钥）、设置（需要验证的操作，默认注册与找回密码）；内核：清单 `[frontend.csp]` 声明的外部来源（只接受 https 主机）并入首页 CSP；插槽 `auth.reset.extra`，登录 / 注册 / 找回插槽上下文带 `attempt`（失败后重置组件）；web：captcha 前端按服务商加载脚本并渲染组件 | 签到验证、按风险（失败次数）触发、证书 / hostname 校验 |
| anticheat（WASM） | 证据（accounting 信号订阅、`ctx.anticheat.report` 外部证据、每日上传跳变、手工证据）、规则集 DSL（证据 / 聚合 / 乘数规则、谓词与 `ref`、语义校验带 JSON Pointer、默认 v1）、评分（权重、严重度、命中函数、半衰期衰减、单种子与规则上限、shadow 影子分、四舍六入五成双）、风险分段与观察名单、案件（自动 / 手动开案、证据快照、优先级、认领、时间线、备注、过期关闭）、复核决定（权限矩阵、员工保护、四眼封禁批准、手动解除）、自动处置（结论性规则 + 可逆措施、熔断）、期望状态协调（ban / leech → identity 限制，freeze → accounting override 倍率，wallet → ledger 锁定；修订号幂等键、退避重试、失败与外部覆盖检测、巡检）、一次性操作（通知、流量回收与撤销、补偿）、到期、申诉（会话 / 封禁令牌、四眼复核、维持 / 减轻 / 撤销回滚）、案件补全（命中数刷新、swarm 快照）、回测模拟、规则统计与准确率、规则集版本与激活后重算、`ledger.before_transfer` / `checkin.before_check_in`、`ctx.anticheat`、MCP 三个工具、AI 解释、用户删除与数据保留；identity 新增 `user.restriction_changed` 与 `ctx.users.restrictions`；pt-tracker 首报 started 只建基线、`max_rate` 超速截断（流字段 `xup` / `xdown`）、BEP 7 `ipv6=` / `ipv4=` 地址下发；web：复核队列、案件工作台、申诉、观察名单、规则集编辑 / 校验 / 回测 / 激活、规则统计、仪表板小部件、用户风险标签页、我的案件与令牌申诉页 | 盒子识别、同 IP 对端关联、种子视角、passkey 重置、客户端指纹信号、近期重新验证 |
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
- **评论渲染不引入 ammonia**：设计为 ammonia 白名单净化；本版本由 `pt-markup` 自行输出全部标签（用户输入只作为转义后的文本与经校验的属性），WASM 包更小；前端 UserContent 再做一次白名单净化。
- **系统举报者**：`ctx.reports.create` 未给出举报者时记为用户 0（系统，如 ai-moderation）。
- **初始状态规则的默认值**：设计中没有规则命中时默认 `pending`；本版本由设置 `default_status` 决定（默认留空 = 不改变 torrent 的初始状态），站点启用审核时再配置，避免安装插件即改变全部上传的行为。
- **候选区投票即时决定**：设计为调度 `candidate_resolve` 延迟任务以保证状态变更钩子的 actor 为本插件；torrent 尚无状态变更钩子，本版本在投票请求中直接决定。
- **敏感词只支持精确匹配**：规范化（小写、去空白与零宽字符）后逐词查找；通配符与 Aho-Corasick 自动机在词表规模变大时再引入。
- **通知模板的渲染位置**：设计为 notify 服务端按模板术语渲染；内核的术语对象只含插件自己的清单术语，本版本由前端（术语快照包含全部插件的术语）按 `<key>.title` / `<key>.body` 插值，服务端保存调用方给出的回退文本（外部渠道接入时再做服务端渲染）。
- **rss 的查询方式**：设计为把订阅编译成 `ctx.search` 查询；本版本直接查询 torrent / accounting 的只读视图（只含已发布的种子），按发布时间倒序——订阅只需要“最新若干条”，不需要相关度，也不受搜索索引同步延迟影响；字段条件在种子字段与分组字段合并后的 JSON 上比较，字段名只做格式校验（不校验是否为可筛选字段）。Torznab 的 `offset` 直接用 SQL 偏移（≤ 1000）。
- **feed key 的签发**：identity 的 `tokens` 表早已允许 kind = feed（`audience_plugin` 必填）；本版本补上签发与校验：`ctx.auth.issue_token` 接受 kind = feed（忽略 ttl、不过期），`Credential::Feed` 只接受 `audience_plugin` 等于调用方插件的 key，Bearer 不接受 feed key；用户的 token 管理接口不能创建、也不列出 feed key。
- **邮件渠道的配置位置**：提供方、凭据、发件人、站点地址与退订密钥放在 site.toml `[plugin.notify]`（静态配置），不放后台可改的设置；邮件正文使用调用方给出的回退标题与正文（纯文本，HTML 版本转义），服务端模板渲染留待术语服务支持渲染后实现。
- **SDK 的形态**：设计为 `pt-plugin-sdk` 提供 `#[pt_plugin::main]` 过程宏、`sql::query!` 绑定宏与从 schema 生成的类型化事件 / 钩子；本版本用声明宏 `world!`（内部调用 wit-bindgen，以 `with` 把宿主接口映射到 SDK 中生成的绑定）、`SqlValue` 的 `From` 转换代替 `query!`，事件与钩子仍以 `serde_json::Value` 传递。SDK 放在 `crates/plugin-sdk-rust`，不在主工作区（只为 `wasm32-wasip2` 构建，由 WASM 工作区与模板以路径依赖使用）。
- **打包工具**：设计为 `xtask package` / `sign`；本版本由 `pt-server plugin package` 组装包（含 `checksums.txt`）并调用 `plugin check`；`--sign` 用 minisign 私钥签名（`plugin keygen` 生成，加密私钥的口令取自 `PT_PLUGIN_KEY_PASSWORD`）。内核加载时有 `plugin.sig` 就按 `plugins.trusted_keys` 校验签名、文件哈希与清单之外的文件（开发模式且未配置公钥时跳过）；没有签名只在 dev 模式 + `allow_unsigned` 时接受。发布构建在配置了签名密钥时签名全部插件包，并让 `init-config.sh` 生成生产模式配置；Docker 镜像仍为未签名包（dev 模式）。
- **第三方钩子点与错误码**：钩子点原本只能来自 catalog/hooks.md（内核编译时生成）；为了让第三方插件定义钩子点，目录之外、以 `<id>.` 开头的钩子点按清单中的参数登记（不受 `strict_catalog` 限制）。错误码同理，清单 `[error_codes]` 补充目录之外的 HTTP 状态。
- **模板仓库**：设计为独立的模板仓库；本版本放在本仓库 `templates/` 下（带独立 `[workspace]` 与 WIT 副本，可直接复制到仓库外），独立发布留到开源发布时。
- **pt-tracker 的实现方式**（决策 D20，取代 D9 / D14）：不使用 torrust，协议、准入、增量与事件出口自研；swarm 与会话状态全部存在 Redis（每个种子的做种 / 下载 ZSET + 会话 HASH，键带 Cluster 哈希标签 `{info_hash}`），每次 announce 一个 Lua 脚本完成 CAS、清理超时 peer、更新与抽取 peer，tracker 无状态、可多节点。会话值用 JSON（设计为紧凑二进制）；announce 在请求内同步写 Redis（设计为异步 worker 队列 + 背压）。已用 Transmission 4.1 真实传输与压测验证（见 crates/pt-tracker/README.md）。
- **tracker-bridge 的 `ver`**：设计为“先取 ver 再读源”并在脚本中比较；本版本由 `trk:ver` 计数器在写入脚本内生成，快照总是被最新一次写覆盖。限制类变化（H&R 限制下载等）没有事件，由每 10 分钟的全量同步兜底。
- **开发站点的数据库**：`run-site.sh` 使用库 `ptsite` 与角色前缀 `ptsite_`（端到端测试会删除 `pt_plugin_*` 角色，同一 PostgreSQL 集群中不能共用）。
- **通知模板不单独登记**：类别、链接随每次发送给出；模板键前缀必须是调用方插件 ID。
- **求种悬赏同步结算**：设计中托管 / 发放 / 退款经 `bounty_step` 延迟任务异步执行；本版本在请求内直接调用 `ctx.ledger`（ledger 工作单元独立提交，幂等键按贡献 / 应求，重试不会重复扣款）。
- **签到按 UTC 计日**：WASM 宿主的站点时区尚未接入，签到日期用 UTC。
- **测试已知风险（2026-10-13）**：完整测试并行负载很高时，`e2e_wasm` 的沙箱越界用例偶发一次错误码不符（单独运行与随后的完整运行均通过）；待复现时记录具体用例与返回值。
- **identity 的 IP 记录不分区**：设计中 `ip_log` / `security_events` 按月分区，由分区保留期删除；内核的分区维护只建分区、不删旧分区，本版本改为普通表 + 每日 `ip_retention` 按批截断与删除。新设备提醒由 notify 订阅 `user.logged_in`（`alert = true`）发送：identity 不能依赖 notify（notify 依赖 `ctx.users`，会形成依赖环）。
- **邮箱验证默认关闭**：设计默认 `require_email_verification = true`；本版本默认 false（未配置邮件渠道的站点开启后新用户无法激活），配置好 notify 邮件渠道后在后台开启。挑战投递采用设计中的拉取模式，但链接以站内相对路径返回，由 notify 用自己的 `site_url` 补全。
- **强制 2FA 的配置**：设计为 `two_factor.required_roles`（角色列表，默认 `["moderator"]`）；本版本为单个“最低角色” `two_factor_required_role`，默认不强制（开启后已有员工须先绑定才能继续使用管理权限）。个人 / MCP token 不受此限制。
- **WebAuthn 不用 webauthn-rs**：设计使用 webauthn-rs；本环境没有该 crate 的缓存，改为自行实现最小子集（不校验证明链，只接受签名算法 ES256 / EdDSA / RS256），依赖 p256、ed25519-dalek、rsa。
- **registration-policy 的取舍**：人机验证拆到 captcha 插件（钩子中不能出站 HTTP）；域名名单直接放在设置里（设计为独立表与导入）；时区只支持固定偏移（WASM 中没有时区库）；同 IP 上限默认每 24 小时 3 个。
- **默认启用程度**：发布包会带上全部 WASM 插件。会改变已有行为的默认值尽量保守：newbie-exam 默认不自动分配；captcha 未配置服务商时不验证；registration-policy 默认只限制同 IP 每 24 小时 3 个账号；ratio-policy 默认按 Gazelle 表考核（下载 ≥ 5 GiB、注册满 7 天才考核），不需要的站点可在 `plugins.disabled` 中停用。
- **WASM 插件的 HTTP 错误会回滚工作单元**：插件以 ≥ 400 状态返回时内核按错误处理并回滚本次写入。需要在“失败”响应里保留的写入（如负缓存）要放到 Redis。已知问题：metadata-tmdb 的 `/v1/lookup` 对不存在的条目写入的 PG 负缓存会被回滚（经 `ctx.metadata` 服务调用的路径不受影响）。
- **手动运行 cron**：`Kernel::run_job_now` 在任务正在运行（如启动时 catch_up）时最多等待 30 s。

## 6. 进度记录

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
