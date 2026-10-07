# 04 · 搜索插件 `search-meilisearch`

## 1. 定位

- 内核里没有搜索。搜索由 `ctx.search` 服务提供，默认实现是 `search-meilisearch`。以后可以换成 tantivy、PostgreSQL 全文检索或 Elasticsearch，只需要换一个实现这个服务的插件。
- 它是 **Native 插件**，原因是它在读路径的热点上，而且索引同步需要批量和并发控制。

```toml
[services]
"ctx.search" = "search-meilisearch"
```

## 2. 索引文档由资源类型插件提供

搜索插件本身不认识电影或音乐，每个资源类型插件实现索引贡献接口：

```wit
interface search-contributor {
  // 声明索引配置：可搜索字段、可筛选字段、可排序字段、权重
  index-schema: func() -> index-schema;
  // 把一个 group 或 torrent 转换成索引文档
  to-document: func(id: u64) -> option<document>;
}
```

- 公共字段由 torrent 插件提供：标题、大小、分类、促销状态、发布时间、可见性；做种数、下载数等频繁变化的字段来自 accounting（见 §4）。
- 类型字段由各资源类型插件提供：比如电影的 IMDb ID、年份、分辨率、片源，音乐的艺人、格式、码率。
- 搜索插件把这些 schema 合并后，自动生成 Meilisearch 的 `filterableAttributes`、`sortableAttributes`、`searchableAttributes`。

## 3. 索引划分

| 索引 | 内容 | 谁能搜 |
|---|---|---|
| `torrents` | 种子（平铺，带 group_id，可按作品折叠显示） | 用户 |
| `groups` | 作品 | 用户 |
| `requests` | 求种 / 悬赏（由求种插件贡献） | 用户 |
| `forum` | 帖子（由论坛插件贡献） | 用户 |
| `users_admin` | 用户名、邮箱哈希等 | 只有管理员 |

其他插件可以通过 `ctx.search.register_index()` 注册自己的索引。

## 4. 同步

```
torrent.created / updated / deleted / group.updated ...
        │  (事件日志 → Redis Streams)
        ▼
  索引 worker：按批合并（同一文档在窗口期内只保留最后一次）→ 批量更新文档
```

- **做种数、下载数等频繁变化的数据不要每次都推送**：每 3～5 分钟只对有变化的种子做一次部分更新（只更新 `seeders`、`leechers` 等字段），否则 Meilisearch 的写入队列会被打满。
  - **数据来源**：tracker 内存中的 swarm 不对内核开放。由 accounting 根据流量事件维护各种子的活跃 peer 统计，每 3～5 分钟发布 `traffic.swarm_stats_updated`（只含有变化的种子，载荷 `items[{torrent_id, seeders, leechers, completed}]`），search-meilisearch 订阅后做部分更新。统计精度待 accounting 与 tracker-bridge 设计确认（见 [03-tracker-and-accounting.md](03-tracker-and-accounting.md) §5）。
- **全量重建**：先建 `torrents_v2`，从数据库加事件回放重建，建好后用 swap 原子交换，不停服。索引配置升级也走这个流程。
- **删除优先**：删除、隐藏、下架、禁转走单独的高优先级流，避免已删除的资源还能被搜到。

## 5. 权限与安全

- **默认由内核代理搜索请求，不让前端直连 Meilisearch**：
  - 可见性过滤必须在服务端强制附加（候选区、等级限定资源、已删除资源）；
  - 可以做限流，统一处理术语和字段映射；
  - Meilisearch 的 master key 永远不出现在前端。
- 可选模式（默认关闭）：签发 Meilisearch 的 tenant token，把过滤条件嵌在 token 里，让前端直连。
- 索引文档里不放 passkey、上传者的隐私信息，也不放匿名发布者的真实身份。匿名发布要在 `to-document` 这一步就把身份去掉。

## 6. 中文和 PT 场景的细节

- **中文分词**：Meilisearch 的分词器支持中文切词。中英混排的标题（如 `xxx.2024.2160p.WEB-DL.H265`）需要预处理：把点号、下划线规范化成空格，并单独抽取分辨率、编码、制作组等字段作为可筛选字段。
- **同义词**：从术语表和各资源类型插件中汇总，比如 `4K = 2160p`、`HEVC = H265 = x265`、中英文片名、简繁体转换。管理员可以在后台补充。
- **拼写容错**：对 IMDb ID、制作组名这类精确字段关闭 typo 容错。
- **筛选面板**：前端筛选面板根据合并后的 schema 自动生成，资源类型插件不需要写前端代码。

## 7. 和 AI 的结合

- 使用 Meilisearch 的混合搜索（关键词 + 向量）。向量由 `ctx.llm` 提供方插件生成，或者使用 Meilisearch 内置的 embedder。用户可以用自然语言搜索，比如“上世纪 90 年代的港片动作片，要有国语音轨”。
- MCP 中的 `search_torrents` 工具直接调用 `ctx.search`，自动继承权限过滤，也使用站点自己的术语。

## 8. 水平扩展

- 在 Meilisearch 前面加一层结果缓存：用 Redis 缓存热门查询 30～60 秒，缓存键包含权限范围。
- 多实例复制和分片能力及相关授权条款**需要按当时的版本核实**（见 [09-roadmap.md](09-roadmap.md)）。退路方案：部署多个只读实例，由同一个索引 worker 同时写入，内核轮询分发查询。
