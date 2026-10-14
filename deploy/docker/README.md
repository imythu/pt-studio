# Docker Compose 快速开始

用 `docker compose` 在一台机器上运行完整的 PT Studio：站点（pt-server，含全部插件与前端）、tracker（pt-tracker）、
PostgreSQL 17、Redis 8、Meilisearch。镜像 `ghcr.io/imythu/pt-studio` 由 GitHub Actions 构建，不需要在本地编译。

需要 Docker 24+（含 compose 插件）。

## 1. 获取文件

```bash
mkdir pt-studio && cd pt-studio
curl -fsSLO https://raw.githubusercontent.com/imythu/pt-studio/main/deploy/docker/compose.yaml
curl -fsSLO https://raw.githubusercontent.com/imythu/pt-studio/main/deploy/docker/initdb.sh && chmod +x initdb.sh
curl -fsSL  https://raw.githubusercontent.com/imythu/pt-studio/main/deploy/docker/.env.example -o .env
```

## 2. 修改 `.env`

至少改掉四个密码 / 密钥（`openssl rand -hex 24` 生成），并把地址改成别人访问你的方式：

```bash
PT_BASE_URL=http://<服务器 IP 或域名>:31251
PT_TRACKER_URL=http://<服务器 IP 或域名>:31252
```

只在本机试用时可以不改。注意：这些值在**首次启动**时写入数据库与 `./data/pt/site.toml`，之后再改 `.env` 不会生效
（地址可以直接编辑 `site.toml` 后重启；密码要同时改数据库）。

## 3. 启动

```bash
docker compose up -d
docker compose ps            # 等 server 变为 running（首次启动要执行全部插件的迁移，约十几秒）
docker compose logs -f server
```

打开 `http://<服务器>:31251`，注册一个账号（`admin`、`root` 等是保留用户名），然后授予管理员：

```bash
docker compose exec server pt-server --config /data/site.toml user grant-role <用户名> admin
```

重新登录后导航栏出现“管理”。发布种子后，用 qBittorrent / Transmission 打开下载的 `.torrent` 即可做种与下载。

## 4. 数据目录

所有数据都在 compose 文件所在目录的 `./data/` 下，备份与迁移只需要这个目录（和 `.env`）：

| 目录 | 内容 |
|---|---|
| `./data/pt/` | `site.toml`（站点配置，**含加密密钥，务必备份**）、`blob/`（种子文件） |
| `./data/postgres/` | PostgreSQL 数据 |
| `./data/redis/` | Redis（AOF 持久化：未落库的流量、tracker 的 swarm） |
| `./data/meilisearch/` | 搜索索引（可由站点重建） |

备份：`docker compose stop && tar -czf pt-backup-$(date +%F).tar.gz data .env && docker compose start`
（不停机时用 `docker compose exec postgres pg_dump -U postgres -Fc pt > pt.dump` 备份数据库）。

## 5. 常用操作

| 操作 | 命令 |
|---|---|
| 升级到最新版本 | `docker compose pull && docker compose up -d`（启动时自动迁移；先备份） |
| 固定版本 | `.env` 中 `PT_IMAGE=ghcr.io/imythu/pt-studio:0.1.0` |
| 查看日志 | `docker compose logs -f server tracker` |
| 检查配置与插件 | `docker compose exec server pt-server --config /data/site.toml check` |
| tracker 健康 | `curl http://localhost:31252/health` |
| 停止 / 删除容器（保留数据） | `docker compose down` |
| 彻底重置 | `docker compose down && rm -rf data`（删除全部数据） |

## 6. 正式使用前

- HTTPS：在前面放一个反向代理（nginx / Caddy），写法见 [../release/DEPLOY.md](../release/DEPLOY.md) §5；
  把 `site.toml` 中 `base_url`、`announce_base_urls` 改为 https 地址，tracker 设 `PT_TRUST_FORWARDED=true`，
  站点设 `[http] trusted_proxies`。
- 只暴露 31251 / 31252（或反向代理的 443）；PostgreSQL、Redis、Meilisearch 在 compose 内部网络中，没有映射到宿主机。
- 可选功能（TMDB 元数据、Claude、邮件通知）在 `./data/pt/site.toml` 中配置，见其中的注释；改完 `docker compose restart server`。

更完整的说明（配置项、故障排查）见 [../release/DEPLOY.md](../release/DEPLOY.md)。
