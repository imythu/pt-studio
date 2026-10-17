# PT Studio 部署指南（Linux x86_64 发布包）

本文说明如何用发布包 `pt-studio-<版本>-linux-x86_64.tar.gz` 在一台 Linux 服务器上部署 PT Studio：站点（pt-server，含全部插件与前端）+ tracker（pt-tracker）。

获取方式：

- **发布包**：GitHub Releases（https://github.com/imythu/pt-studio/releases ），由 GitHub Actions（`.github/workflows/release.yml`）在推送 `v*` 标签时构建；也可以在 x86_64 的 Linux 上运行 `deploy/release/build-release.sh` 自行构建。
- **Docker 镜像**：`ghcr.io/imythu/pt-studio:<版本>` / `latest`（`.github/workflows/docker.yml` 构建），快速开始见 [deploy/docker/README.md](../docker/README.md)（数据映射到 compose 所在目录的 `./data/`）。

## 1. 包内容

```
pt-studio-<版本>-linux-x86_64/
├─ bin/
│  ├─ pt-server            内核 + Native 插件（身份、种子、记账、账本、搜索、MCP、RSS、tracker 同步……）+ 管理命令
│  └─ pt-tracker           HTTP tracker（AGPL-3.0-only，独立进程，只经 Redis 与站点通信）
├─ plugins/<id>/           WASM 插件包（plugin.toml、plugin.wasm、migrations/、checksums.txt）
├─ web/                    前端构建产物（由 pt-server 提供）
├─ config/
│  ├─ site.example.toml    站点配置模板（各节有注释）
│  └─ tracker.example.toml tracker 配置模板
├─ sql/bootstrap.sql       数据库初始化 SQL（scripts/init-db.sh 的等价手工版本）
├─ scripts/
│  ├─ init-db.sh           创建数据库与角色
│  └─ init-config.sh       生成 /etc/pt-studio/site.toml 与 tracker.toml（随机密钥）
├─ systemd/                pt-server.service、pt-tracker.service
├─ licenses/               MIT、Apache-2.0、pt-tracker 的 AGPL-3.0
├─ source/                 pt-tracker 源码（AGPL 要求）
├─ VERSION                 版本、提交、构建环境、最低 glibc
└─ SHA256SUMS              包内所有文件的校验和
```

校验：`sha256sum -c pt-studio-<版本>-linux-x86_64.tar.gz.sha256`；解压后 `cd pt-studio-* && sha256sum -c SHA256SUMS`。

## 2. 系统要求

| 组件 | 要求 | 说明 |
|---|---|---|
| 操作系统 | Linux x86_64，glibc ≥ 2.34 | Debian 12+、Ubuntu 22.04+、RHEL / Rocky / Alma 9+ |
| PostgreSQL | ≥ 16（推荐 17） | 需要 PG16 的 `GRANT … WITH INHERIT FALSE, SET TRUE`；Debian 12 / Ubuntu 22.04 自带的版本太旧，请用 [PGDG 软件源](https://www.postgresql.org/download/linux/) |
| Redis | ≥ 7.4（推荐 8） | 需要 `HEXPIRE`、Streams；Debian 12 自带 7.0 太旧，请用 [Redis 官方软件源](https://redis.io/docs/latest/operate/oss_and_stack/install/install-redis/install-redis-on-linux/) |
| Meilisearch | ≥ 1.13 | 种子列表与搜索依赖它；[下载二进制](https://www.meilisearch.com/docs/learn/self_hosted/install_meilisearch_locally) 即可，只监听本机 |
| 端口 | 31251（站点）、31252（tracker） | 对外通常经反向代理（§5） |
| 资源（起步） | 2 核、4 GB 内存、20 GB 磁盘 | 种子文件存在本地目录（`/var/lib/pt-studio/blob`） |

还需要 `openssl`（生成密钥）与 `psql`（初始化数据库）。

## 3. 快速部署（单机）

以下命令以 root 执行，示例使用 Debian / Ubuntu。

### 3.1 安装依赖

```bash
# PostgreSQL 17（PGDG 源，按官网说明添加源后）
apt install postgresql-17
# Redis 8（官方源，按官网说明添加源后）
apt install redis-server
systemctl enable --now postgresql redis-server

# Meilisearch：下载二进制并作为服务运行（只监听本机）
curl -L https://install.meilisearch.com | sh && mv meilisearch /usr/local/bin/
MEILI_KEY="$(openssl rand -hex 24)"; echo "Meilisearch master key: $MEILI_KEY"   # 记下
useradd --system --home /var/lib/meilisearch --create-home meilisearch
cat > /etc/systemd/system/meilisearch.service <<EOF
[Unit]
Description=Meilisearch
After=network.target
[Service]
User=meilisearch
WorkingDirectory=/var/lib/meilisearch
ExecStart=/usr/local/bin/meilisearch --env production --http-addr 127.0.0.1:7700 --master-key $MEILI_KEY --no-analytics
Restart=on-failure
[Install]
WantedBy=multi-user.target
EOF
systemctl daemon-reload && systemctl enable --now meilisearch
```

### 3.2 安装 PT Studio

```bash
tar -xzf pt-studio-<版本>-linux-x86_64.tar.gz
mv pt-studio-<版本>-linux-x86_64 /opt/pt-studio
useradd --system --home /var/lib/pt-studio --create-home --shell /usr/sbin/nologin pt-studio
```

### 3.3 初始化数据库

```bash
export PT_DB_ADMIN_PASSWORD="$(openssl rand -hex 16)" PT_DB_GATEWAY_PASSWORD="$(openssl rand -hex 16)"
sudo -u postgres env PT_DB_ADMIN_PASSWORD="$PT_DB_ADMIN_PASSWORD" PT_DB_GATEWAY_PASSWORD="$PT_DB_GATEWAY_PASSWORD" \
  /opt/pt-studio/scripts/init-db.sh
```

创建 `pt_admin`（执行迁移）、`pt_gateway`（运行期）两个角色与数据库 `pt`，并撤销 `set_config` 与 plpgsql 的 PUBLIC 权限（插件 SQL 隔离依赖这一步）。每个插件的 schema 与角色在首次迁移时自动创建。

### 3.4 生成配置

```bash
PT_BASE_URL=https://pt.example.com PT_TRACKER_URL=https://tracker.example.com \
PT_DB_ADMIN_PASSWORD="$PT_DB_ADMIN_PASSWORD" PT_DB_GATEWAY_PASSWORD="$PT_DB_GATEWAY_PASSWORD" \
PT_MEILI_KEY="$MEILI_KEY" /opt/pt-studio/scripts/init-config.sh
```

- `PT_BASE_URL`：浏览器访问站点的地址（链接、Cookie、CSRF 校验都以它为准）。先在本机试用可以用 `http://<服务器 IP>:31251`。
- `PT_TRACKER_URL`：BT 客户端访问 tracker 的地址，写进下载的种子（`<地址>/announce/<passkey>`）。
- 其他可选变量见脚本开头的注释（数据库地址、Redis 地址、监听地址、目录）。

生成 `/etc/pt-studio/site.toml`（权限 640，含随机生成的身份密钥与邀请密钥）与 `/etc/pt-studio/tracker.toml`，并运行 `pt-server check` 检查配置与全部插件。**备份 `site.toml`**：`[plugin.identity] secret_key` 用于加密邮箱与 passkey，丢失后无法恢复这些数据。

### 3.5 启动

```bash
cp /opt/pt-studio/systemd/*.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now pt-server pt-tracker
systemctl status pt-server pt-tracker
curl -s http://127.0.0.1:31252/health      # {"status":"ok",…,"passkeys":…}
```

`pt-server.service` 启动前自动执行迁移（`ExecStartPre … migrate`）。首次启动会创建全部插件的 schema，需要几秒。

### 3.6 首个管理员

打开站点注册一个账号（`admin`、`root` 等是保留用户名），然后：

```bash
sudo -u pt-studio /opt/pt-studio/bin/pt-server --config /etc/pt-studio/site.toml user grant-role <用户名> admin
```

重新登录后导航栏出现“管理”。后台可以修改各插件的设置（注册方式、魔力值规则、促销、H&R、tracker 参数、客户端白名单等）。

### 3.7 验证

1. 发布一个种子（“发布”页，上传 `.torrent`）；
2. 下载种子文件（种子页“下载种子”）：其中的 announce 地址应为 `PT_TRACKER_URL/announce/<32 位 passkey>`；
3. 用 qBittorrent / Transmission 打开，做种与下载；稍后个人页的上传量、下载量会更新。

## 4. 配置要点

完整字段见 `config/site.example.toml` 中的注释。常改的项：

| 配置 | 位置 | 说明 |
|---|---|---|
| 站点名称、语言、时区 | `[site]` | |
| 监听地址 | `[http] bind` | 默认 `0.0.0.0:31251`；反向代理在本机时可改为 `127.0.0.1:31251` |
| 反向代理 | `[http] trusted_proxies = ["127.0.0.1"]` | 从 `X-Forwarded-For` 取客户端 IP（限流、审计、安全日志都依赖它） |
| 资源类型 | `[plugin.torrent] types` | 默认电影、剧集、动漫、音乐、通用 |
| tracker 地址 | `[plugin.torrent] announce_base_urls` | 改动后新下载的种子生效；已下载的种子不变 |
| TMDB 元数据 | `[plugin.metadata-tmdb] api_key` | 可选：电影 / 剧集自动补全 |
| Claude | `[plugin.llm-claude] api_key` | 可选：AI 上传预审、内容审核、站内助手 |
| 邮件通知 | `[plugin.notify]` | 可选：`email_provider`（http_relay / postmark / sendgrid）、`email_token`、`email_from`、`site_url`、`unsubscribe_secret` |
| 运行时设置 | 后台“插件设置” | 注册是否开放、邀请、魔力值、等级、H&R、tracker 间隔与客户端白名单等，修改后数秒内生效 |

tracker（`/etc/pt-studio/tracker.toml`）：

| 配置 | 说明 |
|---|---|
| `bind` | 默认 `0.0.0.0:31252` |
| `redis_url` | 与站点相同的 Redis；`sync_url` / `acct_url` / `state_url` 可把三种用途分到不同实例 |
| `trust_forwarded` | 在反向代理之后时设为 `true` |
| `max_peer_timeout` | peer 超时（默认 2700 秒，应大于 announce 间隔） |
| `max_rate` | 单个会话的平均速度上限（字节 / 秒，默认 1250000000 即 10 Gbit/s；0 不限），超过时该段流量不计 |

tracker 无状态（swarm 存在 Redis），可以在多台机器上运行多个实例共享同一个 Redis，前面用任意负载均衡。

关于 `kernel.mode`：WASM 插件包以 minisign 签名（`plugin.sig` 签 `checksums.txt`）。发布包带 `config/plugin-signing.pub` 时插件已签名，`init-config.sh` 生成生产模式配置（`kernel.mode = "prod"`、`plugins.trusted_keys` 指向该公钥），加载时校验签名与每个文件的哈希，包内多出、缺少或被改动的文件都会拒绝加载。没有公钥的发布包（未配置签名密钥的构建）只能在 dev 模式下加载（`plugins.allow_unsigned = true`）。

自己构建或修改插件后重新签名：`pt-server plugin keygen --out mysite`（私钥口令取自环境变量 `PT_PLUGIN_KEY_PASSWORD`；`--unencrypted` 生成不加密私钥）生成 `mysite.key` / `mysite.pub`，`pt-server plugin package … --sign mysite.key` 打包并签名，把 `mysite.pub` 加入 `plugins.trusted_keys`（可配置多个公钥）。`pt-server plugin check <包目录> --key mysite.pub` 校验签名。签名文件与官方 `minisign -V -p mysite.pub -m checksums.txt -x plugin.sig` 兼容。

dev 模式另外只放宽两点：后台可设置钩子超时倍数、插件出站请求允许 `http://`（其他仍要求 https）。鉴权、权限、CSRF、限流、SQL 隔离在两种模式下相同。

### 小内存部署（≤ 1 GB）

站点、PostgreSQL、Redis、Meilisearch 同机且内存只有 1 GB 左右时：

- `[database] pool_size` 降到 8～10，PostgreSQL 的 `max_connections` 相应调小（如 30），`shared_buffers` 64 MB 左右；
- 记账（accounting）后台 worker 的并发轮询数为 `pool_size / 4`，其余分区排队，不会占满连接池；
  空闲分区只查 Redis，不开数据库事务；依赖超时或不可用时按 1 s → 60 s 指数退避重试；
- 新站点可以把记账分区数调小：`site.toml` 的 `[plugin.accounting] default_partitions = 8`，
  同时 `tracker.toml` 的 `partitions = 8`。分区数在首次启动时写入 Redis（`acct:_meta`），之后改配置不生效，已运行的站点不要改；
- Meilisearch 设 `MEILI_MAX_INDEXING_MEMORY=128Mb`、`MEILI_MAX_INDEXING_THREADS=1`；
- 建议开启 1～2 GB swap。没有 swap 时内存一紧张就会触发 OOM，SSH 也可能失去响应。

## 5. 反向代理与 HTTPS（nginx 示例）

```nginx
server {
    listen 443 ssl http2;
    server_name pt.example.com;
    ssl_certificate     /etc/letsencrypt/live/pt.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/pt.example.com/privkey.pem;
    client_max_body_size 20m;                  # 种子与图片上传
    location / {
        proxy_pass http://127.0.0.1:31251;
        proxy_set_header Host $host;
        proxy_set_header X-Forwarded-For $remote_addr;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
server {
    listen 443 ssl http2;
    server_name tracker.example.com;
    ssl_certificate     /etc/letsencrypt/live/tracker.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/tracker.example.com/privkey.pem;
    access_log off;                            # announce 量大，且 URL 中含 passkey
    location / {
        proxy_pass http://127.0.0.1:31252;
        proxy_set_header X-Forwarded-For $remote_addr;
    }
}
```

对应设置：`site.toml` 中 `[site] base_url = "https://pt.example.com"`、`[http] trusted_proxies = ["127.0.0.1"]`、`announce_base_urls = ["https://tracker.example.com/announce"]`；`tracker.toml` 中 `trust_forwarded = true`。站点与 tracker 监听地址可改为 `127.0.0.1:…`，只让 nginx 对外。

防火墙只需要开放 80 / 443（不用反向代理、直接访问时开放 31251 与 31252）；PostgreSQL、Redis、Meilisearch 只监听本机或内网，**不要暴露到公网**。

## 6. 日常运维

| 操作 | 命令 |
|---|---|
| 日志 | `journalctl -u pt-server -f`、`journalctl -u pt-tracker -f`（`RUST_LOG=debug` 可在 service 中调整） |
| 检查配置与插件 | `sudo -u pt-studio /opt/pt-studio/bin/pt-server --config /etc/pt-studio/site.toml check` |
| 只执行迁移 | `… pt-server --config /etc/pt-studio/site.toml migrate` |
| 授予 / 管理角色 | `… pt-server --config … user grant-role <用户名> <admin\|moderator\|…>`；其他用户管理在后台进行 |
| 检查单个插件包 | `/opt/pt-studio/bin/pt-server plugin check /opt/pt-studio/plugins/<id>` |
| tracker 健康 | `curl http://127.0.0.1:31252/health` |
| 重启 | `systemctl restart pt-server pt-tracker`（tracker 重启不丢 swarm：peer 与会话状态在 Redis 中） |

### 备份

- PostgreSQL：`pg_dump -Fc pt > pt-$(date +%F).dump`（每日）；
- 种子文件：`/var/lib/pt-studio/blob`；
- 配置：`/etc/pt-studio/`（含密钥，单独加密保存）；
- Redis：开启 AOF（`appendonly yes`）。Redis 中有记账流（尚未落库的流量）与 tracker 的 swarm；丢失时 swarm 会在客户端下次 announce 时重建，未落库的少量流量会丢失。
- Meilisearch：可随时由站点重建索引，不需要备份。

### 升级

```bash
systemctl stop pt-server pt-tracker
mv /opt/pt-studio /opt/pt-studio.prev
tar -xzf pt-studio-<新版本>-linux-x86_64.tar.gz && mv pt-studio-<新版本>-linux-x86_64 /opt/pt-studio
systemctl start pt-server pt-tracker    # 启动前自动迁移
```

先备份数据库。回滚：停服务、换回旧目录、用备份恢复数据库（迁移不提供自动回退）。配置在 `/etc/pt-studio`，升级不受影响；新版本新增的配置项见新包中的 `config/site.example.toml`。

## 7. 故障排查

| 现象 | 处理 |
|---|---|
| `pt-server` 启动失败，`E-CONFIG` | 配置文件语法或字段错误，按提示修改 `site.toml` |
| `E-LOAD-…` / `E-CATALOG` | 插件加载失败：`pt-server … check` 查看详细原因；确认 `plugins.dir` 指向包内的 `plugins/` |
| `E-LOAD-SIGNATURE` | 包未签名（生产模式拒绝；开发模式需 `plugins.allow_unsigned = true`）、签名不是受信公钥签的、或文件与 `checksums.txt` 不一致（§4） |
| 数据库连接 / 权限错误 | 确认执行过 `init-db.sh`，`site.toml` 中两个连接串的密码正确；PostgreSQL 版本 ≥ 16 |
| Redis 报 `unknown command 'HEXPIRE'` | Redis 版本低于 7.4，升级 Redis |
| 种子列表为空、搜索报错 | Meilisearch 未运行或 `api_key` 不对；`curl http://127.0.0.1:7700/health` |
| 客户端提示 `unregistered passkey` | 刚注册 / 刚重置 passkey 时 tracker 还没同步（通常 1 秒内）；仍然失败时看 `journalctl -u pt-server` 中 tracker-bridge 的日志 |
| 客户端提示 `client not allowed` | 客户端不在白名单中：后台 tracker-bridge 设置中添加 peer_id 前缀，或允许未知客户端 |
| 客户端提示 `torrent not registered` / `torrent not available` | 种子未发布或被禁用（待审核的种子不能做种） |
| 上传量 / 下载量不更新 | 确认 tracker 与站点使用同一个 Redis；`journalctl -u pt-server` 中查看 accounting 的日志 |
| 页面能打开但登录后立即失效 | `base_url` 与浏览器访问的地址（协议、域名、端口）不一致，或反向代理没有传 `Host` |

## 8. 许可证

- pt-server、全部插件、前端：MIT OR Apache-2.0（`licenses/LICENSE-MIT`、`licenses/LICENSE-APACHE`）。
- pt-tracker：AGPL-3.0-only（`licenses/LICENSE-AGPL-3.0-pt-tracker`）。如果你修改 pt-tracker 并向用户提供服务，需要按 AGPL 提供修改后的源码；原始源码见 `source/`。pt-tracker 与站点只经 Redis 通信，不影响站点其他部分的许可证。
