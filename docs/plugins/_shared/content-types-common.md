# 资源类型插件共用设计（type-movie / type-tv / type-music / type-anime / type-generic）

| 项 | 内容 |
|---|---|
| 文档编号 | CMN-TYPE |
| 版本 | 0.1 |
| 状态 | 草案 |
| 日期 | 2026-10-05 |

本文定义所有资源类型插件共同遵守的模型、接口实现约定、校验规则、搜索贡献、元数据联动与测试策略。各类型插件的 01～04 文档只描述本类型特有的字段与规则，其余引用本文。

## 1. 模型

```
Group（作品）               由类型插件定义 group_schema 与 group_key
  ├─ Edition（版本，可选）   只用于需要“版本”层的类型（音乐：发行版 / 再版；影视：剪辑版，可选）——以 torrent.fields 中的 edition_key 表示，不建独立表
  └─ Torrent（种子）         torrent 插件存储，fields 由类型插件定义（field_schema）
```

- torrent 插件存储 `groups(type_id, group_key, title, year, fields jsonb)` 与 `torrents.fields jsonb`；类型插件**不建种子与 Group 的主表**，只建自己的辅助表（例如规则配置、trump 记录、元数据关联缓存）。
- `group_key` 规范：`<type_id>:<scheme>:<value>`，如 `movie:imdb:tt0111161`、`tv:tmdb:1399:s01`、`music:mbrg:<release-group-mbid>`；无外部 ID 时 `<type_id>:title:<规范化标题>:<year>`（降级，审核员可合并）。
- Edition（版本）键：`edition_key = sha256(规范化的版本属性)` 前 16 位，同一 Group 内相同 edition 的种子在列表中归为一组（Gazelle/OPS 的 remaster 思路）。

## 2. `content-type` 导出的实现约定

| 函数 | 约定 |
|---|---|
| `info()` | `type_id` 与插件 ID 后缀一致（`type-movie` → `movie`）；`field-schema`、`group-schema` 为 JSON Schema 2020-12，带 `x-pt-*` 扩展（§4） |
| `normalize(fields)` | **纯函数**：不做出站 HTTP、不调用 LLM（上传流程同步调用）；可读本插件配置与只读视图；完成：字段裁剪（未知字段丢弃）、类型转换、枚举规范化（同义词映射，如 `x265 → HEVC`）、从 `release_name` 解析补全缺失字段（§3）、校验；错误码 `<plugin_id>.<field>_invalid` 并在 `details.errors[]` 给出 JSON Pointer |
| `group-key(fields)` | 按 §1 规则；返回 none 表示不分组 |
| `canonical-title(fields)` | 按本类型的标题模板生成规范标题（§5） |
| `canonical-group-title`（torrent 02 §7 提议） | 实现；未合入前 torrent 使用 `fields.title + year` |

钩子 `torrent.before_upload`（priority 10，所有类型插件注册，但只处理 `type_id` 属于自己的输入）：

- 规则检查（标题规范、必填项组合、禁止的编码 / 片源组合、体积合理性、文件列表结构如音乐必须含音频文件）；
- 可 `modify`：`title`（规范标题）、`fields`（补全）、`initial_status`（命中“需人工审核”规则时为 `pending`）、`tags`；
- 只读本插件表与只读视图；**不出站**；超时预算 ≤ 300 ms（钩子总预算 1000 ms 中的份额）。

## 3. 发布名解析（共享 crate `pt-release-parser`）

PT 资源普遍使用场景发布名（`Title.Year.Resolution.Source.Codec-Group`）。建议所有类型插件与 search-meilisearch（其 `rel_*` 字段）共用一个解析库：

| 输出字段 | 示例 |
|---|---|
| `title`、`year`、`season`、`episode`、`episode_end` | `The.Show.S01E02-E03` |
| `resolution` | `2160p`、`1080p`、`720p`、`480p`、`4320p` |
| `source` | `BluRay`、`UHD BluRay`、`WEB-DL`、`WEBRip`、`HDTV`、`DVD`、`Remux`（Remux 作为标志） |
| `video_codec` | `AVC/H.264`、`HEVC/H.265`、`AV1`、`VC-1`、`MPEG-2` |
| `audio_codecs[]` | `TrueHD Atmos`、`DTS-HD MA`、`DDP 5.1`、`AAC`、`FLAC`、`LPCM` |
| `hdr[]` | `HDR10`、`HDR10+`、`DV`、`HLG` |
| `release_group` | `-` 之后的组名 |
| `flags[]` | `REPACK`、`PROPER`、`INTERNAL`、`3D`、`HYBRID` |

规范化词表（同义词 → 规范值）随 crate 维护，类型插件可在设置中追加站点词表。解析结果只用于补全，用户填写的字段优先。中文资源名（如“某剧.第一季”）提供简单规则：`第(\d+|[一二三…])季`、`第(\d+)集`、`(\d{4})年`。

## 4. 字段 Schema 扩展（供前端 Schema 表单与筛选面板）

| 扩展 | 含义 |
|---|---|
| `x-pt-label-term` | 字段显示名术语键（`type-<x>.field.<field>`） |
| `x-pt-widget` | `select`、`multiselect`、`tags`、`external-id`（带“从元数据拉取”按钮）、`mediainfo`（多行文本，交给 mediainfo 解析）、`markdown` |
| `x-pt-filter` | `facet` / `range` / `none`：是否出现在搜索筛选面板 |
| `x-pt-search` | `searchable` / `exact` / `none` |
| `x-pt-from-metadata` | 可由元数据填充的字段路径（如 `metadata.year`） |
| `x-pt-group-level` | true 表示属于 Group 字段（出现在 group_schema） |

## 5. 标题规范

每个类型插件有默认标题模板（设置可改），模板语法为 `{field}`、`{field?前缀|后缀}`（字段为空时整段省略），例如电影：

```
{title} {year} {edition?| }{resolution} {source} {hdr?| }{video_codec} {audio_codecs?| }-{release_group?}
```

上传时若用户提交的标题与规范标题差异过大（编辑距离 / 规范标题长度 > 0.5），钩子 `modify.title` 为规范标题并把原标题放入 `fields.original_title`；站点可设置为“拒绝并提示”（`title_policy = reject`）。

## 6. 去重与替代（trump）

| 规则 | 说明 |
|---|---|
| 精确重复 | info_hash 重复由 torrent 处理（`torrent.duplicate`） |
| 同 Edition 同规格 | 同一 Group、同一 edition_key、相同（分辨率、片源、编码）且非“不同制作组的允许并存”时：钩子返回 `deny(<plugin_id>.dupe)`，`details.existing_torrent_id`；站点可设为 `pending`（交给审核）而非拒绝 |
| 替代 | 新种在本类型的质量排序中高于旧种（如 Remux > Encode、无损 > 有损、带 log 的 100% CD 抓轨 > 无 log）时：允许上传，`tags` 加 `trump_candidate`，并写入 `trumps(old_torrent_id, new_torrent_id, reason)`，交给 moderation 决定是否下架旧种 |

质量排序由各类型插件定义（数据驱动的权重表）。

## 7. 搜索贡献

每个类型插件实现 `search-contributor`：

- `index-schemas()`：为 `torrents` 与 `groups` 两个索引贡献 `<x>_*` 前缀字段（search 03 §1.2 命名规则），声明 `title_fields`（供 search 的发布名解析与 `_norm` 字段）、同义词组（中英文片名不在此处，来自元数据别名）。
- `to-document(index, entity_id)`：读 `torrent.v_torrent_public` / `torrent.v_group_public` 与 torrent 插件提供的字段视图（需要 torrent 增加 `v_torrent_fields(torrent_id, type_id, fields)` 与 `v_group_fields(group_id, type_id, fields)` 只读视图——见各插件 02 §7）；返回本插件字段；`type_id` 不属于自己时返回 none。

## 8. 元数据联动

```
torrent.created / group.created（异步）
  └► 类型插件订阅：若 Group 带外部 ID 且元数据缓存过期 → jobs.schedule_once("fetch_metadata")
       └► ctx.metadata.lookup(source, external_id, locale)（内核路由到 metadata-* 插件；出站在其中进行）
            └► 结果写入本插件 group_meta 缓存表；ctx.torrents.update_group(group_id, patch)（作用域 write:group）
                 只更新 Group 中标注 x-pt-from-metadata 的字段，且不覆盖人工编辑过的字段（group_meta.locked_fields）
订阅 metadata.fetched（其他插件或手动刷新触发）→ 同样处理
```

上传表单中的“从元数据拉取”按钮由前端直接调用 `GET /api/p/<metadata-plugin>/v1/lookup?id=` 预填，不经过类型插件。

## 9. 共同的平台契约新增

| 类别 | 内容 |
|---|---|
| torrent 视图 | `v_torrent_fields(torrent_id, type_id, group_id, fields)`、`v_group_fields(group_id, type_id, fields, group_key)` |
| torrent 服务 | `update_group(id, patch)` 已登记；需支持“仅更新指定字段 + 乐观版本号” |
| content-type 导出 | `canonical-group-title`（torrent 02 §7）；`quality-rank(fields) -> s32`（供 trump 判定，可选） |
| 共享 crate | `pt-release-parser`（MIT OR Apache-2.0）；与 search-meilisearch §1.7 共用 |
| 依赖方向 | 类型插件 `requires ctx.torrents`（update_group），torrent 通过内核聚合服务调用类型插件的 `content-type` 导出：这是“导出回调”而非服务依赖，需内核确认不违反依赖无环规则（plugin-api §1.2 第 4 条） |

## 10. 共同测试策略

- 每个类型插件提供 `tests/vectors/<type>/*.json`：输入 fields / 发布名 → 期望的规范化结果、group_key、规范标题、钩子决策；
- 契约测试：`info()` 的 schema 通过 JSON Schema 元校验，且只使用 §4 的扩展；
- 性能：`normalize` p99 < 5 ms，钩子 p99 < 50 ms；
- 安全：字段长度上限、描述 Markdown 由统一安全渲染器处理（comments 插件提供的渲染 crate），类型插件不输出 HTML。
