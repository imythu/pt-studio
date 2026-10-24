# 错误码目录

> 版本 0.1 · 2026-10-04

## 1. 规则

- 格式：`<plugin_id>.<snake_case>`（见 process/README §3），内核用 `kernel.`。正则 `^[a-z][a-z0-9-]*\.[a-z][a-z0-9_]*$`。
- 错误码是**稳定契约**：前端、MCP 客户端、其他插件依赖它分支处理。发布后不得改名或改含义，只能新增、废弃。
- 每个插件在详细设计第 9 节给出完整错误码表（错误码、HTTP 状态、说明）。
- 错误码与 HTTP 状态的对应关系必须遵守下面的通用映射；插件特有错误优先复用通用码，只有调用方需要区分处理时才新增。
- 错误响应格式见 [../conventions/api.md](../conventions/api.md) §7（RFC 9457 problem+json，`code` 字段即错误码）。
- `message` 给人看（按请求语言），不得包含敏感数据或内部实现细节（SQL、堆栈、插件内部路径）。

## 2. 通用错误码（内核）

| 错误码 | HTTP | 说明 | retryable |
|---|---|---|---|
| `kernel.bad_request` | 400 | 请求格式错误（JSON 解析失败、参数类型错误） | 否 |
| `kernel.validation_failed` | 422 | 请求通过解析但未通过 schema / 业务校验；`details.errors[]` 给出 `{path, code, message}` | 否 |
| `kernel.unauthenticated` | 401 | 未登录或凭据无效 | 否 |
| `kernel.two_factor_required` | 401 | 需要二次验证 | 否 |
| `kernel.forbidden` | 403 | 已登录但无权限；`details.permission` 给出缺少的权限点 | 否 |
| `kernel.csrf_failed` | 403 | CSRF 校验失败 | 否 |
| `kernel.not_found` | 404 | 资源不存在（或无权得知其存在） | 否 |
| `kernel.method_not_allowed` | 405 | | 否 |
| `kernel.conflict` | 409 | 并发冲突（请求体 `expected_version` 不匹配等） | 是 |
| `kernel.precondition_failed` | 412 | `If-Match` / `If-None-Match` 前置条件不满足；`details.current_rev` 给出当前版本 | 是 |
| `kernel.reentrant_call` | 500 | 导出实现在同一调用链中回调了触发它的插件的服务（见 kernel/03 重入检测） | 否 |
| `kernel.idempotency_key_reused` | 422 | 同一幂等键用于不同请求体 | 否 |
| `kernel.idempotency_in_progress` | 409 | 同一幂等键的请求仍在处理中 | 是 |
| `kernel.payload_too_large` | 413 | | 否 |
| `kernel.unsupported_media_type` | 415 | | 否 |
| `kernel.rate_limited` | 429 | 触发限流；带 `Retry-After` | 是 |
| `kernel.quota_exceeded` | 429 | 插件资源配额（Redis、kv、blob）超限 | 否 |
| `kernel.hook_denied` | 403 | 钩子拒绝但处理者未给出错误码时的兜底 | 否 |
| `kernel.hook_failed` | 503 | fail-closed 钩子的处理者失败 | 是 |
| `kernel.service_unavailable` | 503 | 依赖服务不可用（未启用 optional 服务、下游超时熔断） | 是 |
| `kernel.timeout` | 504 | 插件调用超时 | 是 |
| `kernel.plugin_trapped` | 500 | WASM 插件陷入（panic、非法内存访问） | 否 |
| `kernel.resource_exhausted` | 503 | 插件内存 / 并发 / 实例池耗尽 | 是 |
| `kernel.capability_denied` | 500 | 插件调用了未获授权的宿主能力（属于插件缺陷，对外显示为内部错误） | 否 |
| `kernel.call_depth_exceeded` | 500 | 服务调用嵌套过深 | 否 |
| `kernel.sql_rejected` | 500 | 插件 SQL 未通过语句检查（见 kernel/03 §6.3） | 否 |
| `kernel.redis_command_denied` | 500 | 插件 Redis 命令不在白名单 | 否 |
| `kernel.internal` | 500 | 其他内部错误；`details.trace_id` 供排查 | 否 |
| `kernel.maintenance` | 503 | 维护模式 | 是 |

## 3. 跨插件常用错误码（预分配）

| 错误码 | HTTP | 所属插件 | 说明 |
|---|---|---|---|
| `identity.invalid_credentials` | 401 | identity | 用户名或密码错误（不区分哪一个） |
| `identity.account_banned` | 403 | identity | 账号被封禁 |
| `identity.account_pending` | 403 | identity | 账号待审核 / 待验证 |
| `identity.download_disabled` | 403 | identity | 账号被禁止下载 |
| `identity.username_taken` | 409 | identity | 用户名已存在 |
| `invite.code_invalid` | 422 | invite | 邀请码无效或已过期 |
| `invite.quota_exhausted` | 403 | invite | 邀请额度不足 |
| `invite.code_reserved` | 403 | invite | 邀请码正被其他注册占用 |
| `invite.binding_invalid` | 422 | invite | 邮箱或用户名与邀请绑定不符 |
| `invite.disabled` | 403 | invite | 邀请系统已关闭 |
| `invite.account_inactive` | 403 | invite | 账号状态不允许发邀 |
| `invite.account_too_new` | 403 | invite | 注册时间不足 |
| `invite.privilege_suspended` | 403 | invite | 发邀权暂停中 |
| `invite.outstanding_limit_reached` | 403 | invite | 未使用邀请数达到上限 |
| `invite.daily_limit_reached` | 403 | invite | 今日发邀数达到上限 |
| `invite.already_redeemed` | 409 | invite | 邀请已被使用，不能撤销 |
| `invite.invite_reserved` | 409 | invite | 邀请正在注册中，稍后再撤销 |
| `invite.not_owner` | 403 | invite | 不是该邀请的签发者 |
| `invite.penalty_state_conflict` | 409 | invite | 连坐记录状态不允许该操作 |
| `invite.scope_denied` | 403 | invite | 服务调用方缺少作用域 |
| `identity.target_protected` | 403 | identity | 插件不能限制员工账号 |
| `identity.insufficient_rank` | 403 | identity | 不能操作同级或更高级别的用户 |
| `identity.invalid_status` | 422 | identity | 状态或限制类型非法 |
| `registration-policy.closed` | 403 | registration-policy | 注册已关闭 / 名额已满 |
| `registration-policy.email_domain_not_allowed` | 422 | registration-policy | 邮箱域名不在白名单 |
| `torrent.not_private` | 422 | torrent | 种子不是 private（上传时强制 private=1 前的校验失败） |
| `torrent.duplicate` | 409 | torrent | 重复种子（info_hash 已存在） |
| `torrent.invalid_file` | 422 | torrent | .torrent 解析失败 |
| `ledger.insufficient_balance` | 422 | ledger | 余额不足 |
| `ledger.currency_not_allowed` | 403 | ledger | 调用方无该币种作用域 |
| `ledger.duplicate_transfer` | 200 | ledger | 幂等键重复：返回首次结果（不作为错误抛出，仅用于日志） |
| `ratio-policy.download_restricted` | 403 | ratio-policy | 分享率不足禁止下载 |
| `hnr.download_restricted` | 403 | hnr | H&R 过多禁止下载 |
| `hnr.restricted` | 403 | hnr | 有未处理的 H&R，不能执行该操作 |
| `hnr.record_not_found` | 404 | hnr | H&R 记录不存在 |
| `hnr.not_forgivable` | 409 | hnr | 记录状态不可免除 |
| `newbie-exam.restricted` | 403 | newbie-exam | 考核期限制 |
| `levels.class_required` | 403 | levels | 等级不足 |
| `search-meilisearch.unavailable` | 503 | search-meilisearch | 搜索后端不可用 |
| `metadata-tmdb.not_configured` | 503 | metadata-tmdb | 未配置 TMDB API key |
| `metadata-tmdb.id_invalid` | 422 | metadata-tmdb | ID 格式错误 |
| `metadata-tmdb.not_found` | 404 | metadata-tmdb | TMDB 条目不存在 |
| `metadata-tmdb.upstream_error` | 502 | metadata-tmdb | TMDB 上游错误 |
| `metadata-douban.not_enabled` | 503 | metadata-douban | 豆瓣数据源未启用 |
| `metadata-douban.not_configured` | 503 | metadata-douban | 代理 / PT-Gen 模式未配置地址 |
| `metadata-douban.id_invalid` | 422 | metadata-douban | 不是豆瓣条目编号、条目链接、`book:<编号>` 或有效 ISBN |
| `metadata-douban.not_found` | 404 | metadata-douban | 条目不存在 |
| `metadata-douban.upstream_error` | 502 | metadata-douban | 豆瓣 / 代理 / PT-Gen 上游错误 |
| `metadata-douban.search_unsupported` | 501 | metadata-douban | 所配置的上游不支持搜索 |
| `metadata-douban.unsupported` | 501 | metadata-douban | 所配置的上游不支持该查询（PT-Gen 模式不提供图书） |
| `metadata-douban.blocked` | 503 | metadata-douban | 被豆瓣拦截（限流），可在设置中填 Cookie |
| `metadata-bangumi.id_invalid` | 422 | metadata-bangumi | 不是 Bangumi 条目编号或链接 |
| `metadata-bangumi.not_found` | 404 | metadata-bangumi | 条目不存在，或为未允许的 NSFW 条目 |
| `metadata-bangumi.upstream_error` | 502 | metadata-bangumi | Bangumi 上游错误（限速 / 熔断时可重试） |
| `metadata-musicbrainz.id_invalid` | 422 | metadata-musicbrainz | 不是 `release-group:` / `release:` / `artist:` 加 MBID 或对应链接 |
| `metadata-musicbrainz.not_found` | 404 | metadata-musicbrainz | 条目不存在 |
| `metadata-musicbrainz.upstream_error` | 502 | metadata-musicbrainz | MusicBrainz 上游错误（限速 / 熔断时可重试） |
| `type-tv.scope_invalid` | 422 | type-tv | 范围（单集 / 多集 / 季包 / 全集）与季集字段组合不合法 |
| `type-tv.external_id_required` | 422 | type-tv | 标题与外部 ID 至少填一项 |
| `type-tv.scope_mismatch` | 422 | type-tv | 种子文件列表与声明的范围不符（集号越界、季包缺集等） |
| `type-anime.scope_invalid` | 422 | type-anime | 范围与季集字段组合不合法 |
| `type-anime.external_id_required` | 422 | type-anime | 标题与外部 ID 至少填一项 |
| `type-anime.subtitle_invalid` | 422 | type-anime | 字幕语言 / 方式无法识别 |
| `tracker-bridge.rules_invalid` | 422 | tracker-bridge | 客户端规则校验失败；`errors[]` 逐条定位（`/rules/<下标>/<字段>`） |
| `tracker-bridge.invalid_peer_id` | 422 | tracker-bridge | 规则测试的 peer_id 无法解码为 ≤ 20 字节 |
| `tracker-bridge.redis_unavailable` | 503 | tracker-bridge | tracker 用的 Redis 不可用 |
| `type-anime.scope_mismatch` | 422 | type-anime | 种子文件列表与声明的范围不符 |
| `type-music.format_encoding_invalid` | 422 | type-music | 格式与编码组合不合法 |
| `type-music.no_audio_files` | 422 | type-music | 种子中没有与格式对应的音频文件 |
| `type-music.log_missing` | 422 | type-music | 声明有 log 但种子中没有 .log 文件 |
| `type-music.cue_missing` | 422 | type-music | 声明有 cue 但种子中没有 .cue 文件 |
| `type-music.forbidden_file` | 422 | type-music | 种子中含可执行文件 |
| `type-music.log_too_large` | 413 | type-music | 预检的 log 超过 512 KiB，或随种子保存的 log 总量超过 56 KiB |
| `llm-claude.not_configured` | 503 | llm-claude | 未配置 API key、key 无效或模型不可用 |
| `llm-claude.rate_limited` | 429 | llm-claude | 上游限流（重试后仍失败） |
| `llm-claude.quota_exceeded` | 429 | llm-claude | 站内每日 token 配额用尽 |
| `llm-claude.structured_invalid` | 502 | llm-claude | 结构化输出未通过本地校验 |
| `llm-claude.unsupported` | 501 | llm-claude | 不支持的能力（嵌入、图片） |
| `llm-claude.input_too_large` | 413 | llm-claude | 输入超过模型上限 |
| `ai-upload-review.not_found` | 404 | ai-upload-review | 没有该种子的预审报告 |
| `ai-upload-review.budget_exhausted` | 429 | ai-upload-review | 手动重审时当日模型预算用尽 |
| `ai-assistant.rate_limited` | 429 | ai-assistant | 今日提问次数用尽 |
| `ai-assistant.unavailable` | 503 | ai-assistant | 全站预算用尽或模型未配置 |
| `ai-assistant.session_not_found` | 404 | ai-assistant | 会话不存在 |
| `ai-assistant.question_too_long` | 422 | ai-assistant | 问题为空或超过 2000 字 |
| `comments.target_not_found` | 404 | comments | 目标不存在或不可见 |
| `comments.target_disabled` | 403 | comments | 该目标类型未启用评论 |
| `comments.target_locked` | 403 | comments | 目标不允许评论 |
| `comments.rate_limited` | 429 | comments | 发言过快 |
| `comments.duplicate` | 422 | comments | 与近期的评论内容重复 |
| `comments.too_many_links` | 422 | comments | 链接过多 |
| `comments.too_long` | 422 | comments | 评论为空或超长 |
| `comments.reply_invalid` | 422 | comments | 回复 / 引用对象不合法 |
| `comments.edit_window_expired` | 403 | comments | 超过编辑时限 |
| `comments.not_found` | 404 | comments | 评论不存在 |
| `reports.target_not_found` | 404 | reports | 举报对象不存在 |
| `reports.reason_invalid` | 422 | reports | 理由不属于该目标类型 |
| `reports.note_required` | 422 | reports | 该理由要求填写说明 |
| `reports.already_reported` | 409 | reports | 已举报过该对象 |
| `reports.self_report` | 422 | reports | 不能举报自己 |
| `reports.rate_limited` | 429 | reports | 举报过于频繁 |
| `reports.temporarily_banned` | 403 | reports | 因多次无效举报暂时禁止举报 |
| `reports.case_not_found` | 404 | reports | 工单不存在或已结案 |
| `reports.claimed_by_other` | 409 | reports | 工单已被其他人认领 |
| `reports.action_unavailable` | 422 | reports | 动作不适用于该对象或依赖插件未启用 |
| `reports.escalation_required` | 403 | reports | 升级工单需管理员处理 |
| `moderation.not_in_queue` | 404 | moderation | 目标不在审核队列 |
| `moderation.claimed_by_other` | 409 | moderation | 已被其他审核员认领 |
| `moderation.stale` | 409 | moderation | 目标状态已变化 |
| `moderation.blocked_word` | 422 | moderation | 含禁止词（不回显具体词） |
| `moderation.vote_not_allowed` | 403 | moderation | 不能对该种子投票 |
| `moderation.rule_invalid` | 422 | moderation | 规则校验失败 |
| `ai-moderation.spam_detected` | 422 | ai-moderation | 发表前钩子拒绝（本地规则高置信垃圾） |
| `notify.template_not_owned` | 403 | notify | 模板键前缀不是调用方插件 ID |
| `notify.args_invalid` | 422 | notify | 通知参数、类别、标题或正文不合法 |
| `notify.unsubscribe_invalid` | 400 | notify | 退订链接无效或已过期 |
| `notify.delivery_failed` | 502 | notify | 邮件提供方暂时不可用（投递任务重试） |
| `rss.key_invalid` | 401 | rss | feed key 无效 / 已吊销 / 用户不可用（统一错误） |
| `rss.rate_limited` | 429 | rss | 该 key 请求过于频繁 |
| `rss.feed_not_found` | 404 | rss | 订阅不存在或不属于该 key 的用户 |
| `rss.too_many_feeds` | 422 | rss | 订阅数量达到上限 |
| `rss.filter_invalid` | 422 | rss | 订阅条件不合法 |
| `rss.torznab_function_unsupported` | 400 | rss | Torznab 不支持的 `t`（以 Newznab 错误 202 返回） |
| `messages.not_found` | 404 | messages | 会话或消息不存在（或不是参与者） |
| `messages.recipient_not_found` | 404 | messages | 收件人不存在 |
| `messages.blocked` | 403 | messages | 收件人已拉黑你 |
| `messages.recipient_not_accepting` | 403 | messages | 收件人只接收员工的站内信 |
| `messages.rate_limited` | 429 | messages | 发送过于频繁 |
| `messages.too_long` | 422 | messages | 正文为空或超长 |
| `messages.reply_not_allowed` | 403 | messages | 系统消息不能回复 |
| `checkin.disabled` | 403 | checkin | 签到已关闭 |
| `checkin.not_allowed` | 403 | checkin | 账号状态不允许签到 |
| `checkin.already_checked_in` | 409 | checkin | 今天已经签到过 |
| `requests.not_found` | 404 | requests | 求种不存在 |
| `requests.not_open` | 409 | requests | 求种不在可操作的状态 |
| `requests.bounty_too_small` | 422 | requests | 悬赏低于最低值 |
| `requests.self_fill` | 422 | requests | 不能应求自己的求种 |
| `requests.torrent_invalid` | 422 | requests | 种子未通过审核或类型不符 |
| `requests.requirements_not_met` | 422 | requests | 种子不满足规格要求 |
| `requests.fill_not_found` | 404 | requests | 应求不存在 |
| `requests.fill_not_pending` | 409 | requests | 应求不是待确认状态 |
| `mcp.confirmation_required` | 202 | mcp | 写操作待用户确认（MCP 层返回，不是失败） |
| `mcp.confirmation_invalid` | 409 | mcp | 确认项不存在、已处理、过期或参数不一致 |
| `mcp.confirmation_pending` | 409 | mcp | 用户尚未确认（工具结果中返回） |
| `mcp.insufficient_scope` | 403 | mcp | token 作用域不足 |
| `mcp.oauth_invalid_request` | 400 | mcp | OAuth 授权请求参数错误（同意页接口；令牌端点按 RFC 6749 返回 `error` 字段） |
| `identity.token_limit_exceeded` | 409 | identity | 有效 token 数量达到上限 |
| `llm-claude.upstream_error`、`llm-openai-compat.upstream_error` | 502 | llm-* | 模型服务出错 |
| `anticheat.scope_required` | 403 | anticheat | 服务调用方缺少 `read` / `write` / `report` 作用域 |
| `anticheat.case_not_found` | 404 | anticheat | 案件不存在 |
| `anticheat.case_state_invalid` | 409 | anticheat | 案件状态不允许该操作 |
| `anticheat.case_claimed` | 409 | anticheat | 案件已被他人认领 |
| `anticheat.claim_required` | 409 | anticheat | 需要先认领（或认领已被他人取得） |
| `anticheat.action_not_allowed` | 403 | anticheat | 缺少该处置所需权限（`details.permission`） |
| `anticheat.target_protected` | 403 | anticheat | 受保护用户不能施加该处置 |
| `anticheat.action_unavailable` | 422 | anticheat | 依赖服务未启用（如 ledger 缺失时锁定钱包、passkey 重置） |
| `anticheat.adjust_invalid` | 422 | anticheat | 回收字节数非正或超过当前计入上传 |
| `anticheat.four_eyes_required` | 403 | anticheat | 违反四眼原则 |
| `anticheat.sanction_not_found` | 404 | anticheat | 处置不存在 |
| `anticheat.sanction_state_invalid` | 409 | anticheat | 处置状态不允许批准 / 解除 |
| `anticheat.report_invalid` | 422 | anticheat | 外部证据种类前缀、时间或 details 不合法 |
| `anticheat.user_not_found` | 404 | anticheat | 用户不存在 |
| `anticheat.ruleset_invalid` | 422 | anticheat | 规则集未通过校验（`details.errors[]`） |
| `anticheat.ruleset_not_draft` | 409 | anticheat | 只能修改草案 |
| `anticheat.ruleset_not_found` | 404 | anticheat | 规则集版本不存在 |
| `anticheat.simulation_required` | 409 | anticheat | 激活前需要该草案的完成模拟 |
| `anticheat.simulation_in_progress` | 409 | anticheat | 该版本已有模拟在运行 |
| `anticheat.appeal_not_allowed` | 422 | anticheat | 案件结论不可申诉 |
| `anticheat.appeal_window_closed` | 422 | anticheat | 已过申诉期限 |
| `anticheat.appeal_limit_reached` | 422 | anticheat | 超过申诉次数 |
| `anticheat.appeal_exists` | 409 | anticheat | 已有处理中的申诉 |
| `anticheat.appeal_token_invalid` | 422 | anticheat | 申诉令牌无效、已用或过期（不区分） |
| `anticheat.appeal_state_invalid` | 409 | anticheat | 申诉状态不允许该操作 |
| `anticheat.appeal_change_invalid` | 422 | anticheat | 减轻处置时试图加重或修改非本案处置 |
| `anticheat.transfer_blocked` | 403 | anticheat | 账户审核中禁止转出（`ledger.before_transfer`） |
| `anticheat.checkin_blocked` | 403 | anticheat | 流量冻结期禁止签到（`checkin.before_check_in`） |
| `anticheat.llm_unavailable` | 503 | anticheat | `ctx.llm` 未启用或调用失败 |
