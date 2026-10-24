# 服务目录（`ctx.*`）

> 版本 0.1 · 2026-10-04

## 0. 规则

- 服务名 `ctx.<snake_case>`，WIT 包 `pt:<kebab-case>@<semver>`，Rust trait `<Pascal>Service`（映射见 [../plugin-api.md](../plugin-api.md) §1.1）。
- **基数**：`single` = 同一时刻只启用一个实现，由 `site.toml [services]` 选定（只有一个候选时自动选中）；`multi` = 所有启用的提供方同时生效，消费方按提供方 ID 枚举或经内核聚合。
- 下表“主要方法”是**概要签名**（Rust 风格，省略 `&Ctx` 与 `PResult`），用于并行设计时互相引用；正式签名、错误码、权限要求由提供方插件的详细设计（03 文档 §2.1）给出，可以增加方法，但**不得改变已列方法的语义**。如确需修改，在本插件文档“对平台契约的新增”中提出。
- “作用域”列是消费方在 `[capabilities.scopes]` 中可申请的值；未列出则调用不需要作用域（但仍需在 `requires` 中声明）。
- 所有服务初始版本均为 `0.1.0`。

公共类型（各服务共用，定义在 `pt:plugin/types` 或提供方 WIT 中）：

| 类型 | 字段 |
|---|---|
| `UserSummary` | `id: u64, username, class_id: u32, status: UserStatus, avatar_url: Option, title: Option, created_at` |
| `UserStatus` | enum：`active`、`pending`（待审核/待验证）、`leech_disabled`（禁止下载）、`banned`、`deleted` |
| `Principal` | `user_id: Option<u64>, roles: Vec<String>, scopes: Vec<String>, credential: CredentialKind(session/api_token/mcp_token), credential_id, two_factor_passed: bool` |
| `TorrentSummary` | `id: u64, group_id: Option<u64>, type_id, title, size: u64, status: TorrentStatus, uploader_id: Option<u64>（匿名时对无权限者为 None）, info_hash_v1: Option<hex>, info_hash_v2: Option<hex>, created_at` |
| `TorrentStatus` | enum：`candidate`（候选）、`pending`（待审核）、`approved`、`rejected`、`disabled`（禁转/下架）、`deleted` |
| `TargetRef` | 字符串 `"<kind>:<id>"`，kind ∈ `user`、`torrent`、`group`、`comment`、`forum_post`、`forum_topic`、`message`、`request`、`subtitle`、`report` |
| `Page<T>` | `items: Vec<T>, next_cursor: Option<String>` |

## 1. 内核提供的服务（provider = `kernel`）

WASM 插件通过 `pt:plugin` 的宿主接口使用；Native 插件通过 `Ctx` 字段使用。

| 服务 | 基数 | WIT | 主要方法 | 版本 |
|---|---|---|---|---|
| `ctx.invocation` | single | `pt:plugin/invocation` | `current() -> InvocationInfo` | 0.1.0 |
| `ctx.log` | single | `pt:plugin/log` | `emit(level, msg, fields)` | 0.1.0 |
| `ctx.config` | single | `pt:plugin/config` | `get_static() -> Json`、`get_settings() -> Json` | 0.1.0 |
| `ctx.kv` | single | `pt:plugin/kv` | `get`、`set`、`delete`、`compare_and_set`、`list_keys` | 0.1.0 |
| `ctx.sql` | single | `pt:plugin/sql` | `query`、`execute`、`execute_batch`、`commit`、`copy_in`（仅 Native） | 0.1.0 |
| `ctx.redis` | single | `pt:plugin/redis` | `call`、`pipeline`、`global`（仅 Native） | 0.1.0 |
| `ctx.events` | single | `pt:plugin/events` | `publish(NewEvent) -> event_id` | 0.1.0 |
| `ctx.http` | single | `pt:plugin/http-client` | `send(req) -> resp` | 0.1.0 |
| `ctx.blob` | single | `pt:plugin/blob` | `put`、`get`、`delete`、`presign_get` | 0.1.0 |
| `ctx.time` | single | `pt:plugin/time` | `now()`、`site_timezone()` | 0.1.0 |
| `ctx.random` | single | `pt:plugin/random` | `bytes(n)`、`uuid_v7()` | 0.1.0 |
| `ctx.audit` | single | `pt:plugin/audit` | `write(AuditEntry)` | 0.1.0 |
| `ctx.jobs` | single | `pt:plugin/jobs` | `schedule_once(name, run_at, payload, dedupe_key)`、`cancel(job_id)` | 0.1.0 |
| `ctx.metrics` | single | `pt:plugin/metrics` | `counter_add`、`gauge_set`、`histogram_observe` | 0.1.0 |
| `ctx.hooks` | single | `pt:plugin/hook-runner` | `run(hook, input) -> HookOutcome` | 0.1.0 |
| `ctx.metadata` | 聚合 | `pt:plugin/metadata` | `sources()`、`lookup(source, external_id, locale)`、`search(source, query, type_id, limit)`；内核路由到 `metadata-*` 插件的 `metadata-provider` 导出 | 0.1.0 |
| `ctx.content_types` | 聚合 | `pt:content-types`（仅 Native 可直接使用；WASM 经 torrent 的 HTTP API 读取） | `list() -> Vec<TypeInfo>`、`get(type_id)`、`normalize(type_id, fields)`、`group_key(type_id, fields)`、`canonical_title(type_id, fields)`；内核路由到 `type-*` 插件的 `content-type` 导出 | 0.1.0 |
| `ctx.plugins` | single | `pt:plugins` | `list() -> Vec<PluginInfo>`、`frontend_manifest() -> Json`、`openapi() -> Json`、`search_contributors() -> Vec<plugin_id>` | 0.1.0 |
| `ctx.mcp_registry` | single | `pt:mcp-registry`（仅 Native） | `tools() -> Vec<McpToolDef>`、`call(tool, args, principal) -> Json` | 0.1.0 |

## 2. 插件提供的服务

| 服务 | 提供方 | 基数 | 主要方法（概要） | 作用域 | 版本 |
|---|---|---|---|---|---|
| `ctx.terms` | terms | single | `get(key, locale)`、`get_many(keys, locale)`、`render(template, args, locale)`、`set_override(key, locale, value)`、`register(defs)`（内核启动时调用） | `manage`（改覆盖值） | 0.1.0 |
| `ctx.auth` | identity | single | `authenticate(Credential) -> Option<Principal>`、`create_session(user_id, meta) -> SessionToken`、`revoke_session(id)`、`issue_token(user_id, kind, scopes, ttl) -> (token_id, secret)`（kind = feed 时 ttl 忽略，key 只对签发插件有效）、`revoke_token(id)`、`verify_second_factor(user_id, code) -> bool` | `tokens:issue`（mcp 插件签发 MCP token 用） | 0.1.0 |
| `ctx.users` | identity | single | `get(id) -> Option<UserSummary>`、`get_many(ids)`、`find_by_username(name)`、`set_status(id, UserStatus, reason)`、`apply_restriction(user, kind, reason_code, note?, until?)`、`lift_restriction(user, kind?, reason_code)`、`restrictions(user)`（作用域 `write:status` 或 `read:restrictions`）、`set_class(id, class_id, reason)`、`rename(id, new_name)`、`set_title(id, Option<title>)`、`reset_passkey(user, idempotency_key) -> {reset, version, replayed}`（作用域 `write:passkey`；不返回新 passkey；不能用于 moderator 及以上；同一调用方同一幂等键只重置一次）、`get_sensitive(id, field) -> Option<String>`（field ∈ email/ip/passkey，须对应 sensitive 授权；本版本实现 email，返回 `{value, verified}`）、`create(RegisterInput) -> user_id`（仅 identity 自己与 registration 流程） | `write:status`、`write:class`、`write:profile`、`write:passkey` | 0.1.0 |
| `ctx.rbac` | identity | single | `check(user_id, permission) -> bool`、`permissions_of(user_id) -> Vec<String>`、`roles_of(user_id)`、`grant_role(user_id, role, expires_at)`、`revoke_role(user_id, role)`、`register_permissions(defs)`（内核启动时调用）、`define_role(role, permissions)` | `write:roles`（levels 等插件用于动态角色 `<plugin_id>:<name>`） | 0.1.0 |
| `ctx.invites` | invite | single | `validate(code) -> InviteInfo`、`reserve(code, ttl)`、`issue(inviter_id, email_hint) -> code`、`quota(user_id)`、`grant_quota(user_id, n, reason)`、`tree(user_id, depth)` | `write:quota` | 0.1.0 |
| `ctx.registration` | registration-policy | single | `status() -> {open, invite, review, next_open_at, window, message, max_users, users, remaining_slots, requires_invite}`（`max_users` 为 0 表示不限，此时 `remaining_slots` 为 null） | — | 0.1.0 |
| `ctx.torrents` | torrent | single | `get(id)`、`get_many(ids)`、`by_info_hash(hex)`、`list_by_uploader(user_id, cursor)`、`set_status(id, TorrentStatus, reason)`、`set_tags(id, tags)`、`delete(id, reason)`、`get_group(id)`、`update_group(id, patch)`、`update_fields(id, patch)`（资源类型插件写回种子字段，只能更新本类型的种子）、`merge_groups(from, into)`、`build_download(id, user_id) -> bytes`（注入 passkey；需 `download`） | `write:status`、`write:tags`、`write:group`、`write:fields`、`download` | 0.1.0 |
| `ctx.tracker` | tracker-bridge | single | `sync_user(user_id)`、`sync_torrent(torrent_id)`、`set_client_rules(rules)`、`client_rules()`、`peers(torrent_id, include_ip?) -> {peers: [{user_id, peer_id_prefix, seeder, uploaded, downloaded, left, last_announce_at, ip?, alt?}], truncated}`（本版本已实现的唯一方法：读 pt-tracker 的 `ps:{H}`，最近 1 小时内 announce 过的会话，最多 2000 条；`include_ip = true` 须调用方获授敏感能力 `peer.ip`，否则 `kernel.capability_denied`，读取写审计 `tracker-bridge.peer_ip_read`）、`rebuild_all()` | `write:clients`、`admin` | 0.1.0 |
| `ctx.accounting` | accounting | single | `user_totals(user_id) -> {uploaded, downloaded, seed_secs, leech_secs, live_uploaded, live_downloaded}`、`snatches_of_user(user_id, cursor)`、`snatches_of_torrent(torrent_id, cursor)`、`seeding_now(user_id) -> Vec<torrent_id>`、`set_rate_rule(RateRule) -> rule_id`、`remove_rate_rule(rule_id)`、`adjust(user_id, d_up, d_down, reason, idempotency_key)`、`signals(user_id, from, to)` | `read`、`write:rates`、`write:adjust`、`read:signals` | 0.1.0 |
| `ctx.ledger` | ledger | single | `register_currency(code, scale, name_term)`、`transfer(TransferRequest) -> entry_id`、`balance(currency, user_id) -> i64`、`balances(user_id)`、`history(currency, user_id, cursor)`、`reverse(entry_id, reason)` | `currency:<code>`（只能操作该币种）、`read`、`reverse` | 0.1.0 |
| `ctx.anticheat` | anticheat | single | `risk(user_id) -> RiskSummary`、`cases(filter, cursor)`、`case(id)`、`resolve(case_id, decision, actions[], false_positive_evidence_ids[], note, idempotency_key)`、`report(req) -> evidence_id`（种类须以调用方插件 ID 为前缀） | `read`、`write`、`report` | 0.1.0 |
| `ctx.bonus` | bonus | single | `estimate_hourly(user_id) -> i64`、`award(user_id, amount, reason, idempotency_key)`、`rules_summary(locale)` | `award` | 0.1.0 |
| `ctx.seeding_bonus` | seeding-bonus | single | `rules() -> RuleDef`（bonus 每小时结算用的做种魔力规则，由插件设置生成）、`classify({title, uploader_id}) -> {tags}`（官种认定，bonus 在种子发布时打 `official` 标签） | — | 0.1.0 |
| `ctx.shop` | shop | single | `items(locale)`、`purchase(user_id, item_id, qty, idempotency_key) -> order_id` | — | 0.1.0 |
| `ctx.checkin` | checkin | single | `status(user_id) -> {checked_today, streak}`、`check_in(user_id) -> reward` | — | 0.1.0 |
| `ctx.lottery` | lottery | single | `wheel(user_id?)`（转盘配置、奖品与概率、最近中奖；带 user_id 时含今日次数与资格） | — | 0.2.0 |
| `ctx.medals` | medals | single | `of_user(user_id, include_hidden?)`、`worn_many(user_ids)`、`definitions()`、`grant(user_id, medal_id, reason, expires_at?, idempotency_key)`、`revoke(user_id, medal_id, reason, idempotency_key)`（`set_title` 尚未实现） | `grant` | 0.1.0 |
| `ctx.levels` | levels | single | `level_of(user_id) -> {class_id, name_term, next_class_id, progress}`、`classes() -> Vec<ClassDef>`、`evaluate(user_id)`、`is_protected(user_id) -> bool`（保号）、`grant_vip(user, days | permanent, source_ref) -> {until_ms, permanent}`（顺延、幂等；`permanent: true` 授予永久 VIP，已是永久 VIP 时按天授予不再生效）、`vip_until(user) -> {until_ms, permanent}` | `evaluate`、`grant` | 0.1.0 |
| `ctx.ratio` | ratio-policy | single | `status(user_id) -> {ratio, required, warned_until, can_download}` | — | 0.1.0 |
| `ctx.hnr` | hnr | single | `status(user_id) -> {active, failed, can_download}`、`records(user_id, cursor)`、`forgive(record_id, reason)` | `forgive` | 0.1.0 |
| `ctx.exams` | newbie-exam | single | `status(user_id) -> Option<ExamProgress>`、`assign(user_id, exam_id)`、`exempt(user_id, reason)` | `write` | 0.1.0 |
| `ctx.seedbox` | seedbox | single | `is_seedbox(user_id, torrent_id) -> bool`、`registered(user_id) -> Vec<SeedboxRecord>` | — | 0.1.0 |
| `ctx.promotions` | promotion | single | `current(torrent_id) -> Option<PromotionState>`、`current_many(ids)`、`create(PromotionDef) -> id`、`end(id)`、`active_global()` | `write` | 0.1.0 |
| `ctx.search` | search-meilisearch | single | `query(SearchRequest) -> SearchResult`（服务端强制可见性过滤）、`suggest(index, prefix)`、`register_index(IndexDef)`、`reindex(index)`、`remove(index, id)`（高优先级删除） | `admin` | 0.1.0 |
| `ctx.mediainfo` | mediainfo | single | `parse(text) -> MediaInfo`、`parse_nfo(text) -> Nfo` | — | 0.1.0 |
| `ctx.comments` | comments | single | `count(target) -> u64`、`count_many(targets)`、`exists(id)`（目标、作者、状态、摘要）、`get(id)`（原文与纯文本，需 `read:raw`）、`create(author_id, target, body)`、`delete(id, reason)` | `read:raw`、`write` | 0.1.0 |
| `ctx.forum` | forum | single | `create_topic(forum_id, author_id, title, body) -> topic_id`、`get_topic(id)`、`link_topic(target, topic_id)` | `write` | 0.1.0 |
| `ctx.messages` | messages | single | `send_system(user_id, subject, body)`（Markdown）、`unread_count(user_id)`、`broadcast(subject, body, min_class?, expires_at?, idempotency_key)` | `send`、`broadcast` | 0.1.0 |
| `ctx.requests` | requests | single | `get(id)`、`fill(request_id, torrent_id, filler_id)` | `write` | 0.1.0 |
| `ctx.subtitles` | subtitles | single | `list_for(torrent_id)` | — | 0.1.0 |
| `ctx.moderation` | moderation | single | `submit(target, reason)`、`decide(target, Decision, note)`、`queue_stats()` | `write` | 0.1.0 |
| `ctx.reports` | reports | single | `create(reporter_id, target, reason_code, note) -> id`、`resolve(id, resolution)` | `write` | 0.1.0 |
| `ctx.notify` | notify | single | `send(user_id, template_key, category, args, title, body, link?, channels?, idempotency_key)`（模板键前缀为调用方；标题正文为回退文本，前端按 `<key>.title` / `<key>.body` 术语渲染，邮件直接使用；`channels` 缺省为 center + email，再按用户偏好过滤）、`send_many(user_ids, ...)`、`channels_of(user_id)` | `send` | 0.1.0 |
| `ctx.llm` | llm-claude / llm-openai-compat | single（另支持 `profiles`，见下） | `complete(LlmRequest) -> LlmResponse`、`stream` 不提供（v0.1）、`embed(texts, model) -> Vec<Vec<f32>>`、`models()` | `use`、`profile:<name>` | 0.1.0 |
| `ctx.mcp` | mcp | single | `pending_confirmations(user_id)`、`confirm(confirmation_id, user_id)`、`reject(confirmation_id, user_id)` | — | 0.1.0 |
| `ctx.ai_upload_review` | ai-upload-review | single | `review(torrent_id) -> ReviewReport` | — | 0.1.0 |
| `ctx.ai_moderation` | ai-moderation | single | `classify(text, context) -> ModerationVerdict` | — | 0.1.0 |

### 2.1 `ctx.llm` 的 profiles

`ctx.llm` 为 single 服务，但 `site.toml` 可额外定义命名配置，供不同插件选用不同模型：

```toml
[services]
"ctx.llm" = "llm-claude"

[plugin.llm-claude.profiles]
default = { model = "claude-sonnet-x", max_tokens = 4096 }
cheap   = { model = "claude-haiku-x",  max_tokens = 1024 }
```

`LlmRequest.profile` 为空时用 `default`；使用其他 profile 需要作用域 `profile:<name>`。

## 3. 只读视图（随服务一同发布）

| 视图 | 提供方 | 列（概要） | 敏感 |
|---|---|---|---|
| `identity.v_user_public` | identity | `id, username, class_id, status, created_at` | 否 |
| `identity.v_user_email` | identity | `id, email` | `user.email` |
| `torrent.v_torrent_public` | torrent | `id, group_id, type_id, title, size, status, created_at, uploader_id`（匿名时为 NULL） | 否 |
| `torrent.v_group_public` | torrent | `id, type_id, title, year, created_at` | 否 |
| `accounting.v_user_totals` | accounting | `user_id, uploaded, downloaded, seed_secs, leech_secs, updated_at` | 否 |
| `accounting.v_snatch` | accounting | `user_id, torrent_id, uploaded, downloaded, seed_secs, leech_secs, completed_at, last_action_at` | 否 |
| `ledger.v_balance` | ledger | `user_id, currency, balance, updated_at` | 否 |

视图规范见 [../conventions/database.md](../conventions/database.md) §9。新增视图在提供方文档中提出。
