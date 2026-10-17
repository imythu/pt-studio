# 前端插槽目录

> 版本 0.1 · 2026-10-04 · 机制见 design/06 §3

## 1. 规则

- 插槽名：`<page_or_area>.<section>[.<sub>]`，小写 snake_case 段，用 `.` 分隔；正则 `^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*){1,3}$`。
- 插槽由**官方前端（web/app）定义**，属于平台契约；插件只能往已登记的插槽中注入组件（manifest `[frontend].slots`）。
- 插件 ESM 入口导出：

```ts
// ui/index.ts —— 构建为 ui/index.js
import type { PluginFrontend } from '@pt-studio/slots';

export default {
  slots: {
    'user.profile.stats': { component: BonusStat, order: 100 },
  },
  routes: [{ path: '/p/bonus/*', component: BonusPages }],
  // 可选：后台设置页自定义组件（settings_ui = "custom" 时）
  settings: BonusSettings,
} satisfies PluginFrontend;
```

- 组件 props：`{ ctx: <插槽上下文>, api: PluginApi }`，`PluginApi` 提供带鉴权的 `fetch`（只能请求 `/api/`）、`t()`（术语与 i18n）、`useTerm()`、`navigate()`、`toast()`。
- 同一插槽内按 `order` 升序排列（默认 100，相同时按插件 ID）。插件组件出错由错误边界隔离，不影响页面其他部分。
- 插槽类型：`list`（多个插件并列渲染）、`single`（只渲染 order 最小的一个，用于整块替换）、`tabs`（每个插件贡献一个标签页，需额外提供 `title_term`）、`columns`（表格列）。

## 2. 插槽清单

| 插槽 | 类型 | 上下文 `ctx` | 预期使用者 |
|---|---|---|---|
| `app.head` | list | `{}` | 主题、统计脚本（受 CSP 限制） |
| `nav.main` | list | `{ user? }` | forum、requests、shop、lottery、checkin |
| `nav.user_menu` | list | `{ user }` | bonus、messages、invite、seedbox、shop |
| `nav.admin` | list | `{ user }` | 各插件后台入口 |
| `home.announcements` | list | `{ user? }` | forum、promotion（全站活动横幅） |
| `home.widgets` | list | `{ user? }` | checkin、lottery、requests、ai-assistant |
| `home.sections` | list | `{ user? }` | recommend（首页主栏：为你推荐、热门、需要做种） |
| `user.profile.header` | list | `{ profile: UserSummary }` | medals、levels |
| `user.profile.stats` | list | `{ profile }` | accounting、bonus、levels、hnr、ratio-policy、invite |
| `user.profile.badges` | list | `{ profile }` | medals |
| `user.profile.tabs` | tabs | `{ profile }` | accounting（做种/下载记录）、invite（邀请树）、hnr、comments |
| `user.settings.tabs` | tabs | `{ user }` | identity（安全）、notify、rss、mcp（token）、seedbox |
| `torrent.list.row_badges` | list | `{ torrent: TorrentSummary }` | promotion、torrent（标签）、hnr |
| `torrent.list.columns` | columns | `{ torrent }` | accounting（做种数/完成数）、subtitles |
| `torrent.list.filters` | list | `{ index_schema }` | search-meilisearch（自动筛选面板之外的扩展） |
| `torrent.detail.header` | list | `{ torrent, group? }` | promotion、moderation |
| `torrent.detail.info` | list | `{ torrent, group? }` | type-*、mediainfo |
| `torrent.detail.sidebar` | list | `{ torrent, group? }` | metadata-*、requests、ai-upload-review（仅审核员） |
| `torrent.detail.actions` | list | `{ torrent }` | reports、bonus（赠送）、subtitles |
| `torrent.detail.tabs` | tabs | `{ torrent }` | comments、subtitles、tracker-bridge（peer 列表）、accounting（完成者列表） |
| `torrent.upload.type_form` | single | `{ type_id, schema }` | type-*（默认由 JSON Schema 自动生成，插件可替换） |
| `torrent.upload.extra` | list | `{ draft, update(patch), afterUpload(cb) }`（`update` 回填 title / subtitle / description；`afterUpload` 登记发布成功后的回调 `cb(torrentId)`，返回取消登记函数，回调失败不影响跳转） | mediainfo、metadata-*（拉取信息按钮） |
| `group.detail.header` | list | `{ group }` | type-*、metadata-* |
| `group.detail.tabs` | tabs | `{ group }` | comments、requests |
| `request.detail.sidebar` | list | `{ request }` | requests、comments |
| `forum.post.footer` | list | `{ post }` | medals（签名档勋章）、reports |
| `messages.compose.extra` | list | `{ draft }` | 预留 |
| `admin.dashboard.widgets` | list | `{ user }` | moderation（待审数）、reports、anticheat、accounting |
| `admin.user.tabs` | tabs | `{ profile }` | anticheat、accounting、ledger、hnr、invite、seedbox |
| `admin.torrent.tabs` | tabs | `{ torrent }` | ai-upload-review、moderation、accounting |
| `user.name.decorations` | list（行内，批量） | `{ user_id, size: "sm" \| "md" }` | medals、levels（评审 R1；列表中每个用户名一个实例，插件须用 `api.batch` 合并请求） |
| `auth.register.form_extra` | list | `{ form: FormBinding, attempt: number }`（插件把字段写入注册请求的 `form{}` 或顶层 `invite_code` / `captcha_token`） | invite、registration-policy（问卷）、captcha |
| `auth.login.extra` | list | `{ form: FormBinding, attempt: number }`（`attempt` 每次提交失败后加 1，验证码据此重置） | captcha（人机验证） |
| `auth.reset.extra` | list | `{ form: FormBinding, attempt: number }`（找回密码申请表单） | captcha（人机验证） |

## 3. 页面路由

插件页面路由一律 `/p/<plugin_id>/...` 或 `/admin/p/<plugin_id>/...`（design/06 §3），在 manifest `[frontend].routes` 声明。后台页面 `/admin/p/<plugin_id>/...`，默认由 `settings_schema` 自动生成设置页。
