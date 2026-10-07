# 开发环境与当前实现状态

## 1. 本地依赖

| 组件 | 版本 | 说明 |
|---|---|---|
| Rust | stable（当前 1.98.1） | `rust-toolchain.toml`，需 clippy、rustfmt |
| PostgreSQL | ≥ 16（开发用 17） | 需要 `GRANT … WITH INHERIT FALSE, SET TRUE`（PG16+） |
| Redis | ≥ 7.4（开发用 8.0） | 需要 `HEXPIRE` 等命令；消费者使用 Streams |

### 1.1 初始化数据库（一次）

以超级用户执行 `deploy/postgres/bootstrap.sql`（创建 `pt_admin`、`pt_gateway`、数据库 `pt`，并在库内撤销 `set_config` 与 plpgsql 的 PUBLIC 权限）。测试库 `pt_test` 同样执行后半部分。

### 1.2 本机开发实例（无 Docker 时）

```bash
# PostgreSQL（端口 55432，数据目录 .dev/pg，以 postgres 用户运行）
su postgres -c "/usr/lib/postgresql/17/bin/pg_ctl -D .dev/pg -o '-p 55432 -k /tmp' -l .dev/pg.log start"
# Redis（端口 56379，不落盘）
redis-server --port 56379 --daemonize yes --save '' --appendonly no
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

端到端测试（`crates/server/tests/`）连接真实 PostgreSQL 与 Redis，默认地址对应 §1.2；可用环境变量 `PT_TEST_SUPERUSER_URL`、`PT_TEST_ADMIN_URL`、`PT_TEST_GATEWAY_URL`、`PT_TEST_REDIS_URL`（建议用独立 DB 号，测试会 `FLUSHDB`）、`PT_TEST_BLOB_DIR` 覆盖。测试会删除并重建测试库中的 `kernel`、`identity`、`torrent` 等 schema。

## 4. 实现状态（P1 内核骨架，2026-10-06）

| 模块 | 已实现 | 未实现 / 简化（后续迭代） |
|---|---|---|
| `pt-plugin-api` | 错误模型与状态推导、Actor / InvocationInfo、全部宿主 trait、导出 trait、`Ctx`（服务调用 `Svc::call`、`authenticate`、`with_principal`）、清单模型与基础校验 | WIT 包、`service!` 宏、清单 JSON Schema |
| 内核 · 加载器 | 依赖解析（single 服务选择、版本要求、缺失、歧义）、拓扑排序与环检测、钩子点归属检查、敏感能力授权检查 | WASM 插件发现与签名校验；catalog 只生成了钩子与错误码（`build.rs` 解析 docs） |
| 内核 · 存储 | 迁移（advisory lock、每插件角色与 schema、`_inbox`/`_kv`、校验和、视图授权同步）、工作单元（`SET LOCAL ROLE`、kv、outbox 事件、审计、延迟任务）、Redis 键改写与命令白名单、本地文件 blob | pg_query 语句检查（WASM 用）、配额、S3 后端、分区维护任务化（目前启动时预建事件日志日分区） |
| 内核 · 运行期 | 服务调用（深度、截止时间、重入检测 `kernel.reentrant_call`、独立工作单元）、钩子管线（顺序、超时、fail-closed / 锁定、可修改字段与只追加检查、敏感字段裁剪、`disabled_handlers`）、客户端 IP 只传给获授 `user.ip` 的插件 | WASM 宿主（wasmtime） |
| 内核 · 事件 | outbox 中继（租约、LISTEN/NOTIFY + 轮询）、消费者（每订阅每分区租约、专用连接、XAUTOCLAIM、inbox 去重、退避重试、死信）、延迟任务执行 | cron、回放、按类型保留期清理、分区 worker |
| 内核 · 网关 | 声明路由校验、会话 / Bearer 鉴权、CSRF、`check_principal`、`reauth_secs`、可信代理 IP、problem+json 与目录状态码、请求级工作单元 | 限流、幂等键、OpenAPI 聚合、前端清单、CORS |
| identity | 注册（规范化、钩子、加密邮箱与 passkey）、密码登录、会话、登出、`ctx.auth` / `ctx.rbac` / `ctx.users`、内置角色继承、权限登记、授予角色 | 2FA、WebAuthn、token、限制、改名、找回密码、Redis 缓存、限流、UTS #39 混淆骨架 |
| torrent | 严格 bencode、v1 / v2 / hybrid 解析、规范化（private、source、白名单）、上传（钩子、重复检测）、详情、文件列表、下载（passkey 注入、钩子、下载记录） | 资源类型插件接入、Group、状态机、编辑与替换、签名链接、批量下载、NFO |

## 5. 与设计文档的差异（实现时的决定）

- **Native 插件的 SQL**：除 `query`/`execute` 外增加 `ctx.sql.conn()`，直接拿到工作单元事务中的 `PgConnection` 使用 sqlx（仍在 `SET LOCAL ROLE` 之下）。
- **Native HTTP**：`HttpRoutes::router()` 返回 `axum::Router`，handler 用 `RequestCtx` 提取器取上下文（设计中为 `Router<Arc<NativeState>>`）。
- **服务调用**：`ctx.service::<dyn S>(name)?.call(|s, c| s.method(c, …))`，每次调用独立工作单元，超时取被调方截止时间。
- **InvocationInfo** 增加 `chain`（重入检测）、`authenticated`、`client_ip`、`user_agent`、`request_id`。
- **identity 邮箱**：按评审 U-1 推荐方案实现——`email_enc`（AES-256-GCM）+ `email_mac`（HMAC，唯一）+ `email_domain`，没有明文邮箱视图。
- **错误码后缀规则**：conventions/api.md §7 原先只引用未定义，已补充定义并实现。
