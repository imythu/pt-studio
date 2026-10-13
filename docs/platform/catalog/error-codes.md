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
| `type-tv.scope_invalid` | 422 | type-tv | 范围（单集 / 多集 / 季包 / 全集）与季集字段组合不合法 |
| `type-tv.external_id_required` | 422 | type-tv | 标题与外部 ID 至少填一项 |
| `type-tv.scope_mismatch` | 422 | type-tv | 种子文件列表与声明的范围不符（集号越界、季包缺集等） |
| `type-anime.scope_invalid` | 422 | type-anime | 范围与季集字段组合不合法 |
| `type-anime.external_id_required` | 422 | type-anime | 标题与外部 ID 至少填一项 |
| `type-anime.subtitle_invalid` | 422 | type-anime | 字幕语言 / 方式无法识别 |
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
| `mcp.confirmation_required` | 202 | mcp | 写操作待用户确认（MCP 层返回，不是失败） |
| `mcp.confirmation_invalid` | 409 | mcp | 确认项不存在、已处理、过期或参数不一致 |
| `mcp.confirmation_pending` | 409 | mcp | 用户尚未确认（工具结果中返回） |
| `mcp.insufficient_scope` | 403 | mcp | token 作用域不足 |
| `mcp.oauth_invalid_request` | 400 | mcp | OAuth 授权请求参数错误（同意页接口；令牌端点按 RFC 6749 返回 `error` 字段） |
| `identity.token_limit_exceeded` | 409 | identity | 有效 token 数量达到上限 |
| `llm-claude.upstream_error`、`llm-openai-compat.upstream_error` | 502 | llm-* | 模型服务出错 |
