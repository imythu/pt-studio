# 领域事件目录

> 版本 0.1 · 2026-10-04 · 投递机制见 [../kernel/03-详细设计说明书.md](../kernel/03-详细设计说明书.md) §7

## 1. 命名规则

- 事件类型：`<domain>.<past_tense>`，全小写，单词间用 `_`；正则 `^[a-z][a-z0-9_]*\.[a-z][a-z0-9_]*$`。
- `domain` 是业务领域名，通常等于插件 ID 或其单数名词（`user`、`torrent`、`traffic`……）；**每个事件类型只有一个发布插件**（`metadata.fetched` 除外，见下表）。
- 动词用过去式或完成态短语：`created`、`updated`、`deleted`、`status_changed`、`settled`、`snapshot_taken`。
- 事件是**已经发生的事实**，不是命令；不要用 `*.requested` 表达“请别人去做”，需要别人做事请调用服务。
- 载荷版本为整数 `version`，从 1 开始，规则见 [../plugin-api.md](../plugin-api.md) §6。

## 2. 事件信封

所有事件（在 PostgreSQL 事件日志、Redis Streams、`on-event` 导出中）使用同一信封：

| 字段 | 类型 | 说明 |
|---|---|---|
| `id` | UUIDv7（字符串） | 全局唯一，幂等去重键 |
| `type` | string | 事件类型 |
| `version` | u32 | 载荷版本 |
| `occurred_at` | RFC 3339（UTC，毫秒）；WIT 中为 Unix 毫秒 | 事实发生时间 |
| `actor` | object | `{kind: "anonymous"|"user"|"system"|"plugin"|"mcp", user_id?: u64, plugin_id?: string, credential_id?: string}` |
| `source` | string | 发布插件 ID（内核填写，插件不可伪造） |
| `key` | string | 分区与顺序键：`user:<id>`、`torrent:<id>`、`group:<id>`、`global` 等。同一 key 的事件在同一分区内有序 |
| `payload` | object | 载荷，结构见各事件的 JSON Schema（`schemas/events/<type>.v<version>.json`，放在发布方插件目录） |
| `trace` | object | `{traceparent: string, tracestate?: string}`（W3C Trace Context） |

JSON 示例：

```json
{
  "id": "01928c3e-5f7a-7b2e-9c1d-3e4f5a6b7c8d",
  "type": "torrent.created",
  "version": 1,
  "occurred_at": "2026-10-04T12:34:56.789Z",
  "actor": { "kind": "user", "user_id": 10086 },
  "source": "torrent",
  "key": "torrent:5501",
  "payload": { "torrent_id": 5501, "group_id": 301, "type_id": "movie", "uploader_id": 10086, "anonymous": false, "size": 53687091200, "status": "pending" },
  "trace": { "traceparent": "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01" }
}
```

**载荷通用约定**：

- ID 一律数字（`u64`，保证 < 2^53）；时间一律 RFC 3339 UTC 字符串；字节数为数字。
- **不放敏感数据**：passkey、邮箱、IP、密码哈希、TOTP 密钥、WebAuthn 公钥、站内信正文永远不进事件载荷。需要时消费方凭 ID 调服务取（并接受授权检查）。
- 匿名发布：`uploader_id` 照常填写（事件只在服务端流转），但面向外部的投递（webhook、通知、AI）必须先判断 `anonymous`。
- 载荷 ≤ 64 KiB；批量事件（如 `traffic.settled`）超过上限时拆成多条，共享 `batch_id` 与 `part`/`parts` 字段。

## 3. 分区

| 事件 | 分区数（默认 1） |
|---|---|
| `traffic.settled`、`traffic.seeding_snapshot_taken`、`torrent.downloaded` | 16 |
| `ledger.entry_posted`、`traffic.swarm_stats_updated` | 8 |
| 其余 | 1 |

分区号 = `xxh3_64(key) % partitions`；分区数在 `site.toml [events.partitions]` 可调，**调整后需要停机排空**（见 kernel/03 §7.6）。

## 4. 事件清单

“主要字段”只列关键字段；完整 schema 由发布方详细设计给出。所有事件初始版本均为 1。

### 4.1 内核

| 事件 | 发布方 | key | 主要字段 |
|---|---|---|---|
| `kernel.settings_changed` | kernel | `plugin:<id>` | `plugin_id, changed_keys[], changed_by` |
| `kernel.node_started` | kernel | `global` | `node_id, version, plugin_set_hash` |

### 4.2 身份与注册

| 事件 | 发布方 | key | 主要字段 |
|---|---|---|---|
| `user.registered` | identity | `user:<id>` | `user_id, username, status, inviter_id?, registration_mode, class_id, attributes{invite_id?, initial_class_id?, tags?}` |
| `user.verified` | identity | `user:<id>` | `user_id, method(email/admin)` |
| `user.challenge_issued` | identity | `user:<id>` | `user_id, challenge_id, purpose(email_verify/password_reset), expires_at, locale`（不含令牌；投递方调用 `ctx.auth.challenge_delivery`） |
| `user.logged_in` | identity | `user:<id>` | `user_id, method(password/webauthn/token), two_factor: bool, session_id` |
| `user.login_failed` | identity | `user:<id>` 或 `global` | `user_id?, username_attempted, reason` |
| `user.profile_updated` | identity | `user:<id>` | `user_id, fields[]` |
| `user.renamed` | identity | `user:<id>` | `user_id, old_username, new_username` |
| `user.password_changed` | identity | `user:<id>` | `user_id` |
| `user.email_changed` | identity | `user:<id>` | `user_id`（不含邮箱） |
| `user.two_factor_changed` | identity | `user:<id>` | `user_id, kind(totp/webauthn), enabled: bool` |
| `user.passkey_reset` | identity | `user:<id>` | `user_id`（不含 passkey） |
| `user.status_changed` | identity | `user:<id>` | `user_id, from, to, reason, until?` |
| `user.restriction_changed` | identity | `user:<id>` | `user_id, restriction_id, kind(ban/leech_disabled/pending), source, op(applied/lifted/expired), reason_code, until?, lifted_by?`（解除方：插件 ID、`admin` 或 `identity`；anticheat 据此识别外部解除） |
| `user.class_changed` | identity | `user:<id>` | `user_id, from_class, to_class, reason, by_plugin` |
| `user.title_changed` | identity | `user:<id>` | `user_id, title?` |
| `user.roles_changed` | identity | `user:<id>` | `user_id, added[], removed[]` |
| `user.deleted` | identity | `user:<id>` | `user_id, mode(soft/purge)` |
| `invite.issued` | invite | `user:<inviter>` | `invite_id, inviter_id, expires_at` |
| `invite.redeemed` | invite | `user:<inviter>` | `invite_id, inviter_id, invitee_id` |
| `invite.revoked` | invite | `user:<inviter>` | `invite_id, inviter_id, reason` |
| `invite.quota_changed` | invite | `user:<id>` | `user_id, delta, balance, reason` |
| `registration.application_submitted` | registration-policy | `global` | `application_id, username` |
| `registration.application_reviewed` | registration-policy | `global` | `application_id, approved: bool, reviewer_id` |

### 4.3 种子与 tracker

| 事件 | 发布方 | key | 主要字段 |
|---|---|---|---|
| `torrent.created` | torrent | `torrent:<id>` | `torrent_id, group_id?, type_id, uploader_id, anonymous, size, status, info_hash_v1?, info_hash_v2?` |
| `torrent.updated` | torrent | `torrent:<id>` | `torrent_id, fields[]` |
| `torrent.status_changed` | torrent | `torrent:<id>` | `torrent_id, from, to, reason, by_user_id?` |
| `torrent.tags_changed` | torrent | `torrent:<id>` | `torrent_id, tags[]`（官方/原创/置顶/禁转等标签键） |
| `torrent.deleted` | torrent | `torrent:<id>` | `torrent_id, reason, by_user_id?` |
| `torrent.downloaded` | torrent | `user:<id>` | `torrent_id, user_id`（下载 .torrent 文件，非 BT 下载） |
| `group.created` | torrent | `group:<id>` | `group_id, type_id` |
| `group.updated` | torrent | `group:<id>` | `group_id, fields[]` |
| `group.merged` | torrent | `group:<into>` | `from_group_id, into_group_id` |
| `group.deleted` | torrent | `group:<id>` | `group_id` |
| `tracker.client_rules_changed` | tracker-bridge | `global` | `version, rule_count` |
| `tracker.resynced` | tracker-bridge | `global` | `scope(full/user/torrent), count` |

### 4.4 记账与账本

| 事件 | 发布方 | key | 主要字段 |
|---|---|---|---|
| `traffic.settled` | accounting | `user:<id>` | `user_id, window_start, window_end, items[{torrent_id, d_up_raw, d_down_raw, d_up, d_down, d_seed_secs, d_leech_secs, rate_rule_ids[]}], batch_id` —— 每用户每窗口一条 |
| `traffic.seeding_snapshot_taken` | accounting | `user:<id>` | `user_id, taken_at, period_secs, torrents[{torrent_id, size, seed_secs, seeders}]` —— 每小时一次，供魔力值等计算 |
| `traffic.snatch_completed` | accounting | `user:<id>` | `user_id, torrent_id, completed_at` |
| `traffic.adjusted` | accounting | `user:<id>` | `user_id, d_up, d_down, reason, by` |
| `traffic.signal_raised` | accounting | `user:<id>` | `user_id, torrent_id?, signal(cross_check_mismatch/speed_exceeded/peer_not_in_swarm/negative_delta/counter_jump/client_not_allowed), severity, details{}` |
| `traffic.swarm_stats_updated` | accounting | `torrent:<id>` | `items[{torrent_id, seeders, leechers, completed}]` —— 每 3～5 分钟，只含有变化的种子 |
| `ledger.currency_registered` | ledger | `global` | `currency, scale, name_term, owner_plugin` |
| `ledger.entry_posted` | ledger | `user:<id>` | `entry_id, currency, user_id, amount, balance_after, reason, counterparty_user_id?` |
| `ledger.entry_reversed` | ledger | `user:<id>` | `entry_id, reversal_entry_id, reason` |
| `anticheat.case_opened` | anticheat | `user:<id>` | `case_id, user_id, rule, score, band, opened_reason(score/manual/report), ruleset_version, evidence_count` |
| `anticheat.case_resolved` | anticheat | `user:<id>` | `case_id, user_id, action(none/warn/freeze_traffic/lock_wallet/adjust_traffic/disable_download/ban/reset_passkey，最严重的一项), actions[], resolution(sanctioned/no_action/dismissed/overturned), false_positive, by` |
| `anticheat.sanction_changed` | anticheat | `user:<id>` | `sanction_id, user_id, case_id?, kind, op(applied/expired/lifted/overturned/failed), until?` |
| `anticheat.appeal_submitted` | anticheat | `user:<id>` | `appeal_id, case_id, user_id, via(session/token)`（不含正文） |
| `anticheat.appeal_decided` | anticheat | `user:<id>` | `appeal_id, case_id, user_id, outcome(upheld/reduced/overturned), by` |

### 4.5 经济

| 事件 | 发布方 | key | 主要字段 |
|---|---|---|---|
| `bonus.hourly_settled` | bonus | `user:<id>` | `user_id, period_start, amount, currency, breakdown{}` |
| `shop.item_purchased` | shop | `user:<id>` | `order_id, user_id, item_id, kind, qty, price, currency` |
| `checkin.checked_in` | checkin | `user:<id>` | `user_id, date, streak, reward` |
| `lottery.ticket_bought` | lottery | `user:<id>` | `round_id, user_id, tickets, cost, first_no, currency` |
| `lottery.round_drawn` | lottery | `global` | `round_id, winners[{user_id, tier, ticket_no, prize}]`（最多 100 条）、`winners_truncated` |
| `lottery.round_cancelled` | lottery | `global` | `round_id, reason, refunded_batches` |
| `medal.granted` | medals | `user:<id>` | `user_id, medal_id, reason` |
| `medal.revoked` | medals | `user:<id>` | `user_id, medal_id, reason` |

### 4.6 规则

| 事件 | 发布方 | key | 主要字段 |
|---|---|---|---|
| `level.promoted` | levels | `user:<id>` | `user_id, from_class, to_class` |
| `level.demoted` | levels | `user:<id>` | `user_id, from_class, to_class, reason` |
| `ratio.warned` | ratio-policy | `user:<id>` | `user_id, ratio, required, deadline` |
| `ratio.warning_lifted` | ratio-policy | `user:<id>` | `user_id, ratio` |
| `hnr.recorded` | hnr | `user:<id>` | `record_id, user_id, torrent_id, deadline` |
| `hnr.satisfied` | hnr | `user:<id>` | `record_id, user_id, torrent_id` |
| `hnr.failed` | hnr | `user:<id>` | `record_id, user_id, torrent_id` |
| `hnr.forgiven` | hnr | `user:<id>` | `record_id, user_id, by, reason` |
| `exam.assigned` | newbie-exam | `user:<id>` | `user_id, exam_id, deadline` |
| `exam.passed` | newbie-exam | `user:<id>` | `user_id, exam_id` |
| `exam.failed` | newbie-exam | `user:<id>` | `user_id, exam_id, action` |
| `seedbox.detected` | seedbox | `user:<id>` | `user_id, torrent_id?, rule` |
| `seedbox.registered` | seedbox | `user:<id>` | `user_id, record_id` |

### 4.7 促销与搜索

| 事件 | 发布方 | key | 主要字段 |
|---|---|---|---|
| `promotion.started` | promotion | `torrent:<id>` 或 `global` | `promotion_id, scope(torrent/global/rule), torrent_ids[], up_rate, down_rate, starts_at, ends_at` |
| `promotion.ended` | promotion | 同上 | `promotion_id, torrent_ids[]` |
| `search.index_rebuilt` | search-meilisearch | `global` | `index, documents, duration_ms` |

### 4.8 内容

| 事件 | 发布方 | key | 主要字段 |
|---|---|---|---|
| `metadata.fetched` | 任一 `metadata-*`（`source` 区分） | `group:<id>` 或 `global` | `source, external_id, type_id, group_id?` |
| `mediainfo.parsed` | mediainfo | `torrent:<id>` | `torrent_id, summary{resolution, video_codec, audio_codecs[], subtitles[]}` |

### 4.9 社区

| 事件 | 发布方 | key | 主要字段 |
|---|---|---|---|
| `comment.created` | comments | `<target>` | `comment_id, target, author_id` |
| `comment.updated` | comments | `<target>` | `comment_id, target, editor_id` |
| `comment.deleted` | comments | `<target>` | `comment_id, target, by, reason` |
| `forum.topic_created` | forum | `forum_topic:<id>` | `topic_id, forum_id, author_id` |
| `forum.post_created` | forum | `forum_topic:<id>` | `post_id, topic_id, author_id` |
| `forum.post_updated` | forum | `forum_topic:<id>` | `post_id, topic_id, editor_id` |
| `forum.post_deleted` | forum | `forum_topic:<id>` | `post_id, topic_id, by` |
| `message.sent` | messages | `user:<recipient>` | `message_id, sender_id?（系统消息为空）, recipient_id`（不含正文） |
| `request.created` | requests | `request:<id>` | `request_id, user_id, type_id, bounty` |
| `request.bounty_added` | requests | `request:<id>` | `request_id, user_id, amount, currency` |
| `request.filled` | requests | `request:<id>` | `request_id, torrent_id, filler_id` |
| `request.closed` | requests | `request:<id>` | `request_id, reason(confirmed/expired/withdrawn)` |
| `subtitle.uploaded` | subtitles | `torrent:<id>` | `subtitle_id, torrent_id, uploader_id, language` |
| `subtitle.deleted` | subtitles | `torrent:<id>` | `subtitle_id, torrent_id` |
| `moderation.decided` | moderation | `<target>` | `target, decision(approve/reject/needs_edit), moderator_id?, by_ai: bool, note` |
| `report.created` | reports | `<target>` | `report_id, target, reporter_id, reason_code` |
| `report.resolved` | reports | `<target>` | `report_id, resolution, by` |

### 4.10 集成与 AI

| 事件 | 发布方 | key | 主要字段 |
|---|---|---|---|
| `notify.delivered` | notify | `user:<id>` | `notification_id, user_id, channel, template_key` |
| `notify.failed` | notify | `user:<id>` | `notification_id, user_id, channel, error_code` |
| `webhook.delivery_failed` | webhook | `global` | `endpoint_id, event_id, status, attempts` |
| `llm.usage_recorded` | llm-claude / llm-openai-compat（`source` 区分） | `plugin:<caller>` | `caller_plugin, model, input_tokens, output_tokens, latency_ms` |
| `mcp.tool_called` | mcp | `user:<id>` | `user_id, token_id, tool, write: bool, outcome(ok/denied/error/pending_confirmation)` |
| `mcp.confirmation_resolved` | mcp | `user:<id>` | `confirmation_id, tool, approved: bool` |
| `ai.upload_reviewed` | ai-upload-review | `torrent:<id>` | `torrent_id, verdict(pass/warn/fail), findings[]` |
| `ai.content_flagged` | ai-moderation | `<target>` | `target, categories[], score` |
| `terms.updated` | terms | `global` | `keys[], locale` |

## 5. 非内核事件流（不经过事件日志）

| 流 | 生产者 | 消费者 | 说明 |
|---|---|---|---|
| `acct:<partition>` | pt-tracker | accounting | 原始流量增量，格式见 design/10 §5.4。**不是领域事件**，不走 outbox，不能被其他插件订阅 |
| `trk:sync` | tracker-bridge | pt-tracker | 同步变更流，见 design/10 §5.5 |
