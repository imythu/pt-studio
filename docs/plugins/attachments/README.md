# attachments（图片上传 / 站内图床）设计说明

> 2026-10-17 · 开站计划 M3.6。种子简介、论坛帖子、评论中的截图过去只能用外链（且需管理员把图床域名加入白名单），现在可直接上传到站内。

## 1. 规则

| 设置 | 默认 | 说明 |
|---|---|---|
| `max_size_kb` | 5120 | 单张最大体积（路由请求体上限 20 MiB） |
| `max_dimension` | 16384 | 最大边长（像素） |
| `daily_count` | 50 | 每人每天上传张数（24 小时滚动，含已删除的，防止反复删传） |
| `daily_mb` | 100 | 每人每天上传总量 |

- 格式：PNG / JPEG / GIF / WebP，按文件头识别，不信任 Content-Type；SVG 等一律拒绝（`attachments.unsupported` 415）。
- 元数据：只解析容器结构、不解码像素。PNG 去掉 `tEXt` / `zTXt` / `iTXt` / `eXIf` / `tIME`；JPEG 去掉 APP1（EXIF / XMP）、APP13（IPTC）、其他 APPn 与 COM，保留 APP0 / APP2（ICC）/ APP14；WebP 去掉 `EXIF` / `XMP ` 块并清除 VP8X 标志、重写 RIFF 长度；GIF 原样。结构损坏返回 `attachments.invalid`。
- 存储：按去除元数据后的 SHA-256 内容寻址，blob `public/i/<前两位>/<sha256>.<ext>`，内核以 `/media/attachments/i/...` 公开提供（长期缓存）。站内 Markdown / BBCode 渲染器总是允许 `/media/` 图片，因此论坛、评论、种子简介可直接引用。
- 同一用户重复上传同一文件返回已有记录；不同用户共享文件、各自一条记录。
- 删除：本人或 `attachments.manage`（默认 moderator，记审计）；软删除记录，最后一个引用删除时删除文件，引用处随即不再显示。

## 2. 接口

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/v1/images` | 请求体为图片原始字节（`attachments.upload`，`user:30/m`）；返回 `{id, url, width, height, size, mime, markdown, bbcode}` |
| GET | `/v1/images?page=` | 我的图片（每页 60）与今日用量 `quota` |
| DELETE | `/v1/images/{id}` | 删除 |
| GET | `/v1/admin/images?user=&page=` | 最近上传（`attachments.manage`） |

## 3. 前端

用户菜单“我的图片”（多选上传、复制 BBCode / Markdown、删除、今日用量）；后台 `/admin/p/attachments/`；发布页“插入图片到简介”（上传后把 `[img]...[/img]` 追加到简介）。

## 4. 相关改动

torrent 详情新增 `description_html`：简介按 BBCode 渲染（纯文本保留换行），站内 `/media/` 图片与设置 `description_image_hosts` 中的外链域名显示为图片，其余降级为链接；前端改用渲染结果。

## 5. 后续

论坛 / 评论编辑框内的上传按钮（随 M4.5 统一编辑器）、缩略图与 WebP 转码、按等级的配额、举报图片、对象存储（S3）与 CDN。
