# bookmarks（收藏与合集）设计说明

> 2026-10-17 · 开站计划 M3.5。参考 NexusPHP 的“收藏”与 UNIT3D 的 Playlists。

## 1. 规则

| 设置 | 默认 | 说明 |
|---|---|---|
| `max_bookmarks` | 5000 | 每人最多收藏数 |
| `max_collections_per_user` | 50 | 每人最多合集数 |
| `max_items_per_collection` | 500 | 每个合集最多种子数（上限 1000） |

- 收藏只有本人可见；种子页显示收藏人数。只能收藏已发布（approved）的种子。
- 合集：标题（≤100）、说明（≤5000）、公开 / 私有；条目带备注（≤500）与顺序。公开且非空的合集出现在合集列表（可按标题搜索）；私有合集只有作者与 `bookmarks.collections.manage` 可见。
- 编辑（改信息、增删条目、排序、删除合集）：作者或 `bookmarks.collections.manage`（默认 moderator）。
- 种子删除（`torrent.deleted`）：从所有收藏与合集中移除，合集计数同步。

## 2. 数据

- `bookmarks(user_id, torrent_id, created_at)`，主键 (user, torrent)。
- `collections(id, owner_id, title, description, is_public, item_count, created_at, updated_at)`。
- `collection_items(collection_id, torrent_id, position, note, added_at)`，主键 (collection, torrent)，随合集级联删除。

## 3. 接口

| 方法 | 路径 | 说明 |
|---|---|---|
| PUT / DELETE | `/v1/bookmarks/{torrent_id}` | 收藏 / 取消（幂等；PUT 需 `bookmarks.use`） |
| GET | `/v1/bookmarks?page=` | 我的收藏（每页 50，新的在前） |
| GET | `/v1/torrents/{id}` | 收藏人数、我是否收藏、收录它的合集（公开或我的）、我的合集（是否已包含） |
| GET | `/v1/collections?mine=1&q=&page=` | 公开合集 / 我的合集 |
| POST | `/v1/collections` | 新建（`bookmarks.collections.create`） |
| GET / PATCH / DELETE | `/v1/collections/{id}` | 详情（含条目与 `editable`）/ 修改 / 删除 |
| POST | `/v1/collections/{id}/items` | 加入种子 `{torrent_id, note?}`；已在合集中时只更新备注 |
| DELETE | `/v1/collections/{id}/items/{torrent_id}` | 移出 |
| PUT | `/v1/collections/{id}/order` | `{torrent_ids: [...]}` 按给出的顺序重排，未列出的排在后面 |

服务 `ctx.bookmarks`：`list {user, limit?}` → 收藏的种子 ID（新的在前）、`is_bookmarked {user, torrent_id}`。事件 `collection.created`。

## 4. 前端

种子页操作区“☆ 收藏 / ★ 取消收藏”、收藏人数、“加入合集…”下拉；侧栏“收录于合集”；用户菜单“我的收藏”；主导航“合集”（列表、搜索、新建、详情页编辑 / 排序 / 备注 / 移出）。

## 5. 后续

RSS 收藏订阅（rss 插件调用 `ctx.bookmarks.list`）、收藏的种子有新版本 / 促销时通知、合集关注与封面。
