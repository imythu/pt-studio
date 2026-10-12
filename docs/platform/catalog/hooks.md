# 同步钩子目录

> 版本 0.1 · 2026-10-04 · 管线机制见 [../kernel/03-详细设计说明书.md](../kernel/03-详细设计说明书.md) §5

## 1. 规则

- 钩子名：`<domain>.before_<verb>`，例如 `torrent.before_upload`；正则 `^[a-z][a-z0-9_]*\.before_[a-z0-9_]+$`。只有 `before_*` 一种钩子，**“之后”发生的事一律用事件**。
- 钩子点由**拥有者插件**定义（manifest `[[hook_points]]` + 输入 schema），由拥有者在业务流程中通过 `ctx.hooks.run()` 触发；处理者在 `[[hooks]]` 中注册。
- 决策：`allow`（继续）、`deny(code, message)`（**短路**，后续处理者不再执行）、`modify(merge_patch)`（只能修改下表“可修改字段”，内核校验，越界视为插件错误）。
- 执行顺序：`priority` 升序（小的先），相同时按插件 ID 字典序。site.toml 可覆盖：`[hooks."torrent.before_upload".priority] bonus = 50`。
- 超时：每个处理者有单独超时（默认见表，处理者可在清单中设更小的值），整条管线另有总预算（默认 = 3 × 单个超时上限）。
- 失败策略（处理者超时、trap、返回 `err`）：
  - `deny`（失败即拒绝，fail-closed）：管线以 `kernel.hook_failed` 拒绝；
  - `allow`（失败即跳过，fail-open）：忽略该处理者，记录告警指标；
  - 站点可在 `[hooks."<name>"] on_error = "deny"|"allow"` 覆盖，**但标注“锁定”的钩子不可改为 allow**。
- 拒绝的 HTTP 状态：处理者返回的错误码**已在 [error-codes.md](error-codes.md) 登记时使用登记的状态**，否则按错误码后缀规则（conventions/api.md）。
- 输入裁剪：内核按**每个处理者**的有效敏感能力删除字段（无 `user.email` 删除 `email`，无 `user.ip` 删除 `ip`）；identity 另提供非敏感的 `ip_hmac`、`ip_prefix_hmac`、`device_hmac` 供关联检查。
- 运维开关：`site.toml` 中 `[hooks."<name>"] disabled_handlers = ["<plugin_id>"]` 可停用某个处理者（仅 sysop，修改需重启，启动日志记录）。
- 钩子处理器**不得**进行出站 HTTP 或调用 `ctx.llm`（宿主在 hook 调用中拒绝 `http-client`，返回 `kernel.capability_denied`）；需要慢操作的检查改为事件 + 审核状态。
- 输入通用字段：每个输入都含 `version`（u32）与 `actor`（同事件信封）；下表省略。

## 2. 钩子清单

| 钩子 | 拥有者（触发点） | 输入（主要字段） | 可返回决策 / 可修改字段 | 默认超时 | 失败策略 | 预期处理者 |
|---|---|---|---|---|---|---|
| `user.before_register` | identity（提交注册） | `username, email_domain, email?（需 user.email）, ip?（需 user.ip）, ip_hmac, ip_prefix_hmac, device_hmac?, invite_code?, locale, form{}（注册表单扩展字段）` | allow / deny / modify：`attributes.invite_id`、`attributes.inviter_id`、`attributes.initial_status`（active/pending）、`attributes.initial_class_id`、`attributes.tags[]` | 500 ms | deny（锁定） | invite、registration-policy、anticheat |
| `user.before_login` | identity（凭据校验通过后、签发会话前） | `user_id, username, method, ip?, ip_hmac, device_hmac?, new_device, risk_flags[]` | allow / deny | 200 ms | allow | anticheat、seedbox |
| `user.before_rename` | identity | `user_id, old_username, new_username` | allow / deny | 200 ms | deny | shop（改名卡）、moderation |
| `user.before_title_change` | identity | `user_id, old_title?, new_title` | allow / deny / modify：`new_title` | 200 ms | deny | shop（头衔卡）、moderation、levels |
| `invite.before_issue` | invite | `inviter_id, email_domain?` | allow / deny | 200 ms | deny | levels、ratio-policy、hnr |
| `torrent.before_upload` | torrent（解析 .torrent 后、落库前） | `uploader_id, type_id, title, size, file_count, files_sample[]（前 200 个路径与大小）, info_hash_v1?, info_hash_v2?, fields{}, anonymous, group_id?, group_fields?（已有 Group 的字段）, nfo_present` | allow / deny / modify：`title`、`fields`、`initial_status`（candidate/pending/approved）、`tags[]` | 1000 ms | deny（锁定） | type-*、moderation、levels、newbie-exam |
| `torrent.before_update` | torrent | `torrent_id, editor_id, changes{}` | allow / deny / modify：`changes` | 500 ms | deny | type-*、moderation |
| `torrent.before_status_change` | torrent | `torrent_id, from, to, reason` | allow / deny | 200 ms | deny | moderation |
| `torrent.before_delete` | torrent | `torrent_id, reason` | allow / deny | 200 ms | deny | moderation、requests |
| `torrent.before_download` | torrent（下载 .torrent 文件前） | `user_id, torrent_id, uploader_id, size, status, promotion?{up_rate, down_rate}` | allow / deny | 200 ms | allow（可改为 deny） | ratio-policy、hnr、newbie-exam、levels、seedbox |
| `search.before_query` | search-meilisearch | `index, query, filters[]（Meilisearch 过滤表达式）, sort[], user_id?, class_id?` | allow / deny / modify：`filters`（**只能追加**，内核比对补丁确认未删除已有条件） | 50 ms | deny（锁定） | moderation（候选区可见性）、levels（等级限定资源） |
| `comment.before_create` | comments | `author_id, target, body` | allow / deny / modify：`body` | 300 ms | allow | moderation、anticheat、ai-moderation（仅本地规则，不调 LLM） |
| `forum.before_post` | forum | `author_id, forum_id, topic_id?, title?, body` | allow / deny / modify：`title`、`body` | 300 ms | allow | moderation、levels |
| `message.before_send` | messages | `sender_id, recipient_ids[], subject, body_length`（不含正文） | allow / deny | 200 ms | allow | moderation、levels |
| `request.before_create` | requests | `user_id, type_id, title, bounty, currency` | allow / deny / modify：`title` | 300 ms | deny | levels、type-* |
| `subtitle.before_upload` | subtitles | `uploader_id, torrent_id, language, filename, size` | allow / deny | 300 ms | deny | moderation |
| `shop.before_purchase` | shop | `user_id, item_id, kind, qty, price, currency` | allow / deny | 200 ms | deny | levels、ratio-policy |
| `checkin.before_check_in` | checkin | `user_id` | allow / deny | 200 ms | allow | anticheat |
| `ledger.before_transfer` | ledger（仅 `from_user` 为用户的转出） | `currency, from_user, to_user?, amount, reason` | allow / deny | 100 ms | deny | anticheat |
| `mcp.before_tool_call` | mcp | `user_id, token_id, tool, arguments{}, write: bool` | allow / deny | 200 ms | deny（锁定） | anticheat、ai-assistant（策略） |

## 3. 结果在调用方的表现

- deny：HTTP 层返回 `403`（`code` 为处理者给出的错误码，如 `invite.code_invalid`）或 `422`（处理者错误码以 `.invalid` / `_invalid` 结尾时）；MCP 层返回工具错误。
- 管线失败（fail-closed）：`503` + `kernel.hook_failed`，`details.hook` 给出钩子名，`details.plugin` 只在管理员可见。
