# 开发指南

> 本机搭建依赖、运行站点、跑测试。部署到服务器见 [deploy/release/DEPLOY.md](../../deploy/release/DEPLOY.md)，写插件见 [docs/sdk](../sdk/README.md)。
>
> 相关：[实现明细](implementation-status.md) · [与设计文档的差异](design-deviations.md) · [更新日志](../../CHANGELOG.md)

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

数据与配置在 `.dev/site/`（`site.toml` 由 `deploy/site.example.toml` 生成，随机密钥）。打开 http://localhost:31251 注册账号后：

```bash
target/release/pt-server --config .dev/site/site.toml user grant-role <用户名> admin
```

冒烟检查（浏览器走一遍注册、发布、下载、后台）：

```bash
PT_BASE=http://localhost:31251 PT_SERVER=target/release/pt-server PT_CONFIG=.dev/site/site.toml node web/tests/smoke.mjs [截图目录]
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

