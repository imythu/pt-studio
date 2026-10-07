# 开发环境与当前实现状态

## 1. 本地依赖

| 组件 | 版本 | 说明 |
|---|---|---|
| Rust | stable（当前 1.98.1） | `rust-toolchain.toml`，需 clippy、rustfmt |
| PostgreSQL | ≥ 16（开发用 17） | 需要 `GRANT … WITH INHERIT FALSE, SET TRUE`（PG16+） |
| Redis | ≥ 7.4（开发用 8.0） | 需要 `HEXPIRE` 等命令；消费者使用 Streams |
| Meilisearch | ≥ 1.13（CE） | search-meilisearch；只监听内网 |

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

## 3. 测试

```bash
cargo clippy --workspace --all-targets      # 0 警告
cargo test --workspace                      # 单元测试 + 端到端测试
```

端到端测试（`crates/server/tests/`）连接真实 PostgreSQL 与 Redis，默认地址对应 §1.2；可用环境变量 `PT_TEST_SUPERUSER_URL`、`PT_TEST_ADMIN_URL`、`PT_TEST_GATEWAY_URL`、`PT_TEST_REDIS_URL`（建议用独立 DB 号，测试会 `FLUSHDB`）、`PT_TEST_BLOB_DIR`、`PT_TEST_MEILI_URL` / `PT_TEST_MEILI_KEY` 覆盖。Meilisearch 不可达时 `e2e_search` 跳过（输出 `SKIPPED`）。测试会删除并重建测试库中的 `kernel`、`identity`、`torrent` 等 schema。

## 4. 实现状态（P1 内核骨架 + P2 记账 + P3 WASM 宿主 + P4 部分插件，2026-10-07）

| 模块 | 已实现 | 未实现 / 简化（后续迭代） |
|---|---|---|
| `pt-plugin-api` | 错误模型与状态推导、Actor / InvocationInfo、全部宿主 trait、导出 trait、`Ctx`（服务调用 `Svc::call`、`authenticate`、`with_principal`）、清单模型与基础校验 | WIT 包、`service!` 宏、清单 JSON Schema |
| 内核 · 加载器 | 依赖解析（single 服务选择、版本要求、缺失、歧义）、拓扑排序与环检测、钩子点归属检查、敏感能力授权检查 | WASM 插件发现与签名校验；catalog 只生成了钩子与错误码（`build.rs` 解析 docs） |
| 内核 · 存储 | 迁移（advisory lock、每插件角色与 schema、`_inbox`/`_kv`、校验和、视图授权同步）、插件范围分区预建（`[[database.partitions]]`）、工作单元（`SET LOCAL ROLE`、kv、outbox 事件、审计、延迟任务）、Redis 键改写与命令白名单、本地文件 blob | pg_query 语句检查（WASM 用）、配额、S3 后端、分区维护任务化（目前启动时预建事件日志日分区） |
| 内核 · 运行期 | 服务调用（深度、截止时间、重入检测 `kernel.reentrant_call`、独立工作单元）、JSON 分派服务调用、钩子管线（顺序、超时、fail-closed / 锁定、可修改字段与只追加检查、敏感字段裁剪、`disabled_handlers`）、客户端 IP 只传给获授 `user.ip` 的插件 | — |
| 内核 · WASM 宿主 | wasmtime 49 组件模型：`plugins.dir` 包加载（清单、编译、迁移、导出与清单一致性检查、未签名包仅限 dev）、每次调用新实例、epoch 截止时间（`kernel.timeout`）、内存上限（`kernel.resource_exhausted`）、陷入（`kernel.plugin_trapped`）、并发上限、WASI 无网络 / 无文件；全部宿主接口转发到 `Ctx`；导出包装为 Native 导出 trait（lifecycle、hooks、on-event、on-cron、http-handler、service-provider） | 包签名（minisign）校验、组件编译缓存、实例池、`http-client`（出站 HTTP 与 SSRF 防护）、metadata、search-contributor / mcp-tool / content-type 导出接入 |
| 内核 · 事件与调度 | outbox 中继（租约、LISTEN/NOTIFY + 轮询）、消费者（每订阅每分区租约、专用连接、XAUTOCLAIM、inbox 去重、退避重试、死信）、延迟任务执行、分区 worker（`[[workers]]`，每分区租约 + fencing token，每次 poll 一个工作单元）、cron（5 段、站点时区、跨副本去重、禁止重叠、catch_up once、`kernel.cron_run`、`Kernel::run_job_now`） | 回放、按类型保留期清理、手动运行的 HTTP / CLI 入口 |
| 内核 · 网关 | 声明路由校验、会话 / Bearer 鉴权、CSRF、`check_principal`、`reauth_secs`、可信代理 IP、problem+json 与目录状态码、请求级工作单元、GCRA 限流（全站默认 + 路由规则，RateLimit 头、Retry-After）、幂等键（指纹、处理中 / 冲突 / 重放、5xx 释放） | OpenAPI 聚合、前端清单、CORS |
| identity | 注册（规范化、钩子、加密邮箱与 passkey）、密码登录、会话、登出、`ctx.auth` / `ctx.rbac` / `ctx.users`、内置角色继承、权限登记、授予角色、等级（`set_class`、`user.class_changed`）、等级绑定角色（`class_roles`，插件定义带前缀角色）、`roles_of`、限制与有效状态（施加 / 解除 / 到期清扫、`user.status_changed`、封禁使会话失效、插件不能限制员工、管理员 rank 检查、`ctx.users.set_status` / `apply_restriction` / `lift_restriction`、`ctx.rbac.check` JSON） | 2FA、WebAuthn、token、改名、找回密码、Redis 缓存、限流、UTS #39 混淆骨架 |
| ledger | 币种注册、发行 / 回收 / 转账（手续费、每日限额、`ledger.before_transfer`）、批量发行、幂等、冻结（到期由延迟任务释放）/ 扣款、账户锁定、冲正、流水分页、供给统计、用户转账与管理员调整 / 冲正 / 设置 HTTP | 日对账、月结余快照、归档、用户删除处理、MCP 工具 |
| accounting | `acct:<p>` 微批消费、eid 去重、拒收记录、倍率规则（favorable / override、索引与版本号刷新）、peers、snatches、首次完成与 `traffic.snatch_completed`、用户总量与日统计、结算窗口与 `traffic.settled`、分段明细、游标 CAS（恰好一次，重启不重复计入）、peers 小时做种槽与小时快照（折叠做种时长、`traffic.seeding_snapshot_taken`）、流量调整、种子镜像、统计 HTTP | 基础信号、swarm 统计、实时计数扣减、交叉校验、重算、流修剪 |
| bonus（WASM） | 币种初始化（经 `ctx.ledger`）、安全表达式语言（词法、Pratt 解析、白名单函数、深度 / 节点上限、非有限值归零）、规则集（表达式与分档模式、NexusPHP 预设为 v1、按小时取生效版本）、小时结算（快照 → 用户小时 → 应计，多部分合并）、批量入账（postings、`transfer_batch`、幂等键、失败重试计数）、行为奖励、`ctx.bonus.award`、`GET /v1/users/me/summary` | 规则编辑 / 发布接口、回放重算、回收、估算明细、`has_role`（等 `ctx.rbac.roles_of`）、medal_factor（等 medals） |
| promotion（WASM） | 促销类型、单种促销（替换 / 结束）、自动规则（谓词与优先级，`torrent.created` 及手动促销结束后重新评估）、全站活动、个人促销卡（发放 / 使用、冗余检查）、同步到 accounting 倍率规则（对账重试）、到期 cron、展示状态、`ctx.promotions` | 促销申请审核、促销统计、自定义类型的前端展示 |
| levels（WASM） | NexusPHP 预设等级（农民为降级落点，用户 0.6 / 0.5 滞回）、角色同步（`levels:class_<id>` 绑定到等级）、指标（注册天数、上传 / 下载 / 分享率、做种时长、平均做种时长、做种体积、发布数）、评估（覆盖 > 升级 one_step / highest > 保护期 > 保级 / 警告 / 到期降级）、增量评估（`traffic.settled` 标记，只升级）与每日全量评估、`invite.before_issue`、手动锁定 / 解锁、保号、`ctx.levels`、`GET /v1/me` 进度 | 等级编辑接口、模拟评估、邀请奖励、不活跃处理、VIP / 荣誉覆盖来源、等级权限默认配置 |
| invite（WASM） | 码派生（HMAC、只存哈希、Crockford 容错）、个人 / 系统 / 活动码签发（资格检查、`invite.before_issue`、FIFO 消耗额度、`request_id` 重放）、注册钩子预留 + `user.registered` 确认（超额兑换标记、邀请树边与闭包表）、额度批次与流水（初始、晋升、周期、管理员、服务发放 / 扣减、到期落账）、撤销 / 过期（返还原批次）、预留释放、邀请树 / 上游 / 统计、发邀权暂停、连坐（封禁触发：扣额度、暂停、撤销未用、限制邀请人、复核；解封撤销）、`ctx.invites`、HTTP | 批量发放、整树操作、日统计与问题邀请人评分、对账、MCP、连坐通知（等 notify）、`anticheat.case_resolved` 触发、`user.deleted` 处理、链接再次显示 |
| search-meilisearch | 贡献者 schema 合并（命名规则、保留 / 敏感字段、冲突记录）与设置生成、事件 → `pending_ops` → 延迟任务批量写入（文档哈希去重、删除优先、非拥有者失败降级）、发布名解析（`rel_*`）与标题规范化、swap 重建状态机（首次建索引、手动重建、双写与重放、文档数校验、取消）、查询（可见性基线、`search.before_query` 只追加且解析校验、结构化过滤、排序、facet、折叠、游标、高亮、Redis 结果缓存与代数失效）、联想、schema 视图、`ctx.search`、失败任务回收 | 语义 / 混合搜索、tenant token、多实例（replicas / network）、做种数部分更新、同义词管理与术语派生、一致性巡检、MCP、扩展索引注册、`users_admin` 等其他标准索引、熔断 |
| torrent | 严格 bencode、v1 / v2 / hybrid 解析、规范化（private、source、白名单）、上传（钩子、重复检测）、详情、文件列表、下载（passkey 注入、钩子、下载记录）、`torrents` 索引搜索贡献（可见性映射） | 资源类型插件接入、Group、状态机、编辑与替换、签名链接、批量下载、NFO |

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
