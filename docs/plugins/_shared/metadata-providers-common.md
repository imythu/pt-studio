# 元数据源插件共用设计（metadata-tmdb / metadata-douban / metadata-imdb / metadata-musicbrainz / metadata-bangumi / metadata-igdb）

| 项 | 内容 |
|---|---|
| 文档编号 | CMN-META |
| 版本 | 0.1 |
| 状态 | 草案 |
| 日期 | 2026-10-05 |

各元数据插件的 01～04 文档只描述本数据源特有的接口、字段映射、条款与限制，其余引用本文。

## 1. 定位

- 每个 `metadata-<source>` 是 WASM 插件，导出 plugin-api 的 `metadata-provider`（`info`、`lookup`、`search`），内核把 `ctx.metadata.lookup(source, …)` 路由到对应插件。
- 只做三件事：**取数**（出站 HTTP）、**归一化**（映射到统一模型）、**缓存**。写入 Group 由资源类型插件负责（content-types-common §8）。
- 出站只在 `lookup` / `search`（由类型插件的延迟任务或用户的 HTTP 请求触发）与本插件的 cron 中进行；**绝不在钩子中调用**（平台约束：钩子禁止出站，`lookup` 若在钩子调用链中被调用，宿主会拒绝出站）。

## 2. 统一元数据模型（`lookup-result.data`）

| 字段 | 类型 | 说明 |
|---|---|---|
| `kind` | string | `movie` / `tv` / `tv_season` / `anime` / `music_release_group` / `game` / `book` |
| `titles` | `{original, localized{<locale>: string}, aliases[]}` | |
| `year`、`release_date` | integer / YYYY-MM-DD | |
| `ids` | `{tmdb?, imdb?, douban?, mbid?, bangumi?, igdb?, tvdb?, anidb?}` | 交叉 ID（来源提供时） |
| `genres` | string[] | 统一键（映射表在各插件） |
| `countries`、`languages` | string[] | ISO 代码 |
| `people` | `[{name, role(director/cast/writer/composer/artist/developer/publisher), mbid?}]` | ≤ 50 |
| `runtime_min`、`episode_count`、`season` | integer? | |
| `ratings` | `{<source>: {value, votes}}` | 0～10 |
| `images` | `{poster?, backdrop?, cover?}` | **已转存**的 blob 相对路径（§4） |
| `overview` | `{<locale>: string}` | 纯文本，≤ 5000 |
| `extra` | object | 数据源特有字段（各插件定义） |
| `attribution` | `{source, url, license_note}` | 前端展示署名用 |

## 3. 缓存与刷新

| 层 | 存储 | TTL |
|---|---|---|
| 结果缓存 | 本插件 PG 表 `cache(external_id, locale, data, fetched_at, etag)` | 默认 30 天（可配置）；未完结剧集 / 新片（上映 < 90 天）7 天 |
| 负缓存 | 同表 `not_found = true` | 1 天 |
| 搜索缓存 | Redis `p:<plugin>:s:<hash(query,type,locale)>` | 1 小时 |

`lookup` 先查缓存，未过期直接返回；过期则请求上游，失败时返回过期数据并标记 `stale = true`（降级）。cron `refresh_stale`（每日）按热度（最近被 lookup 的时间）刷新过期条目，受速率限制约束。

## 4. 图片转存

- 只转存海报 / 封面 / 背景各一张；按目标尺寸下载（TMDB `w500`、Cover Art Archive `500`）；校验 Content-Type（image/jpeg、image/png、image/webp）与大小（≤ 2 MiB）；
- 存储路径 `blob: images/<sha256>.<ext>`（内容寻址，跨插件去重由内核 blob 前缀隔离决定——各插件独立）；
- 前端只使用转存后的路径（防止盗链失效与跟踪），数据源条款要求的署名在详情页展示。

## 5. 速率限制与 API key

- 每个插件在 Redis 私有前缀实现令牌桶（`p:<plugin>:rl`，全站所有副本共享），速率取该数据源的公开限制并留 20% 余量；
- API key / token 存放在 `site.toml [plugin.<id>]` 静态配置（`x-pt-secret`），不出现在日志与前端；
- 统一 User-Agent：`PT-Studio/<version> (<site.name>; +<site.url>)`，可在配置中覆盖（MusicBrainz、Bangumi 要求有意义的 User-Agent）；
- 429 / 503 → 按 `Retry-After` 退避；连续失败触发熔断（5 分钟），期间只返回缓存。

## 6. 出站能力声明

清单 `[capabilities] http_outbound` 只列该数据源的主机（如 `api.themoviedb.org`、`image.tmdb.org`）；站点可在 `site.toml` 覆盖为自建代理主机。

## 7. 合规

| 数据源 | 条款要点（以官方最新条款为准，部署前站点自行确认） | 默认 |
|---|---|---|
| TMDB | API 对非商业用途免费，须展示 TMDB 标志与声明 “This product uses the TMDB API but is not endorsed or certified by TMDB”；商业用途需付费授权 | 启用（需 API key） |
| IMDb | 官方无公开免费 API；Non-Commercial Datasets 仅限个人非商业用途，且不得用于建立电影信息数据库；不得爬取网站 | **只做 ID 校验与外链**，不取数（metadata-imdb 03） |
| 豆瓣 | 官方 API 已不对普通开发者开放；第三方方案多为爬取移动端或网页，存在服务条款与法律风险 | **默认关闭**；只支持站点自行配置的代理端点，并在启用时要求管理员确认风险 |
| MusicBrainz | 核心数据 CC0；API 每个应用每秒不超过 1 次请求，必须设置 User-Agent | 启用 |
| Cover Art Archive | 图片版权归权利人，按其说明使用 | 随 MusicBrainz 启用 |
| Bangumi | 公开 API（`api.bgm.tv`），要求符合规范的 User-Agent | 启用 |
| IGDB | 通过 Twitch 开发者应用（client credentials）访问；速率 4 次/秒 | 启用（需 Twitch client id / secret） |

## 8. 统一接口约定

- `info().id-pattern` 用于前端从链接中识别 ID；
- `lookup(external_id, locale)`：`locale` 为 BCP 47；不支持的语言回退到数据源默认语言；
- `search(query, type_id, limit ≤ 20)`：返回简要结果（`data` 只含 titles、year、ids、images.poster）；
- 每个插件另提供 HTTP：`GET /api/p/<id>/v1/lookup?id=&locale=`、`GET /v1/search?q=&type=`（user，限流 `user:30/m`），供上传表单预填；
- 发布事件 `metadata.fetched{source, external_id, type_id, group_id?}`（仅在上游实际拉取成功且数据变化时发布）。

## 9. 共同测试策略

- 上游使用录制的响应（`tests/fixtures/<source>/*.json`）做回放测试；
- 速率限制：并发 100 次 lookup，验证上游请求速率不超过限制；
- 降级：上游 5xx / 超时 → 返回过期缓存并 `stale = true`；
- 合规：前端署名组件存在；未配置 key 时插件以“不可用”状态运行（`info()` 正常，`lookup` 返回 `kernel.service_unavailable`）。
