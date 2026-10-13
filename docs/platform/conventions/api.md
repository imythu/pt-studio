# REST API 规范

> 版本 0.1 · 2026-10-04 · 适用于内核与所有插件的 HTTP 接口

## 1. 路径

| 范围 | 前缀 | 例子 |
|---|---|---|
| 内核 | `/api/kernel/v<N>/` | `/api/kernel/v1/openapi.json` |
| 插件 | `/api/p/<plugin_id>/v<N>/` | `/api/p/torrent/v1/torrents/5501` |
| 健康 / 指标 | 根路径 | `/healthz`、`/readyz`、`/metrics` |
| 前端页面 | `/p/<plugin_id>/…`、`/admin/p/<plugin_id>/…` | 不属于 API |

- 资源名用**复数名词、kebab-case**：`/torrents`、`/rate-rules`、`/dead-letters`。
- 层级不超过两层：`/torrents/{id}/comments` 可以，再深则改为过滤：`/comments?target=torrent:5501`。
- 路径参数 `{id}`；ID 为数字（BIGINT）或 UUID 字符串。
- 当前用户的资源用 `me`：`/api/p/bonus/v1/users/me/balance`。
- 非 CRUD 的动作用子资源 + POST：`POST /torrents/{id}/status-changes`、`POST /users/{id}/passkey-resets`。避免 `/doSomething` 式路径；必要时允许 `POST /torrents/{id}:approve` 形式（冒号动作，参照 Google AIP-136），二选一时优先子资源。
- 后台接口与用户接口共用资源路径，用权限区分；只有后台才有的集合放在 `/admin/` 段下：`/api/p/identity/v1/admin/users`。

## 2. 版本

- 主版本在路径中（`v1`），**每个插件独立演进**。
- 同一主版本内只做兼容变更：新增端点、新增可选参数、新增响应字段、新增枚举值（客户端必须容忍未知枚举值与未知字段）。
- 不兼容变更：新开 `v2`，`v1` 至少保留一个插件次版本周期，响应加 `Deprecation: true` 与 `Sunset` 头。

## 3. 方法与状态码

| 方法 | 语义 | 成功状态 |
|---|---|---|
| GET | 读取，安全且幂等 | 200 |
| POST | 创建或动作 | 201（创建，带 `Location`）/ 200（动作有结果）/ 202（异步受理）/ 204 |
| PUT | 整体替换（少用） | 200 |
| PATCH | 部分更新，`Content-Type: application/merge-patch+json`（RFC 7396） | 200 |
| DELETE | 删除（可以是软删除） | 204 |

错误状态码见 [../catalog/error-codes.md](../catalog/error-codes.md) §2。

## 4. 数据格式

- 请求与响应 `application/json; charset=utf-8`；字段名 **snake_case**。
- 时间：RFC 3339 UTC，毫秒精度，`2026-10-04T12:34:56.789Z`；日期 `YYYY-MM-DD`。
- ID：BIGINT 以 JSON 数字表示（平台保证 < 2^53）；UUID 以字符串表示。
- 字节数、计数：JSON 数字。**可能超过 2^53 的 64 位整数**（如账本中极端金额，一般不会出现）以字符串表示，并在 schema 中 `format: int64-string`。
- 金额 / 货币：整数最小单位 + 币种的 `scale`（由 `ctx.ledger` 定义），不用浮点。
- 枚举：小写 snake_case 字符串。
- 空值：可选字段缺省时**省略**，不返回 `null`；需要显式表示“清空”的 PATCH 字段使用 `null`（merge-patch 语义）。
- 布尔不用 0/1。
- 站点叫法不在 API 中翻译：API 返回术语键或枚举值，前端用 `t('term:…')` 渲染。例外：MCP 输出与通知已渲染。

## 5. 分页

**默认使用游标分页**：

```
GET /api/p/torrent/v1/torrents?limit=50&cursor=eyJ0IjoiMjAyNi0…
```

```json
{
  "items": [ … ],
  "next_cursor": "eyJ0IjoiMjAyNi0…",
  "has_more": true
}
```

- `limit` 默认 20，最大 100（后台接口最大 500）。
- `cursor` 不透明（base64url 编码的 JSON，含排序键与方向），客户端不得解析；无效时 400 `kernel.bad_request`。
- 不返回总数；需要总数的界面单独调用 `…/count`（可返回近似值，字段 `approximate: true`）。
- **偏移分页**只允许用于后台小表：`?page=3&per_page=50`，响应 `{items, page, per_page, total}`，`page × per_page ≤ 10000`。

## 6. 过滤与排序

- 过滤：普通查询参数，字段名即参数名：`?status=approved&type_id=movie`。
  - 多值：逗号分隔 `?status=approved,pending`。
  - 范围：后缀 `_gte`、`_lte`、`_gt`、`_lt`：`?size_gte=1073741824&created_at_lt=2026-10-01T00:00:00Z`。
  - 目标引用：`?target=torrent:5501`。
  - 全文搜索不在列表接口中实现，统一走 `ctx.search` 的搜索接口（`/api/p/search-meilisearch/v1/search`）。
- 排序：`?sort=-created_at,size`（`-` 表示降序）；每个接口在 OpenAPI 中列出允许排序的字段，其他字段 400。
- 字段裁剪（可选）：`?fields=id,title,size`。
- 未知查询参数：忽略（不报错），但 OpenAPI 中未声明的参数不得影响结果。

## 7. 错误响应

RFC 9457 `application/problem+json`：

```json
{
  "type": "https://pt-studio.dev/errors/ledger.insufficient_balance",
  "title": "余额不足",
  "status": 422,
  "code": "ledger.insufficient_balance",
  "detail": "当前魔力值 120，需要 500。",
  "instance": "/api/p/shop/v1/orders",
  "request_id": "01928c3e-…",
  "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
  "details": { "balance": 120, "required": 500, "currency": "bonus" },
  "errors": [ { "path": "/qty", "code": "kernel.validation_failed", "message": "必须大于 0" } ]
}
```

- `code` 必填，是客户端分支的唯一依据；`title`、`detail` 按请求语言，可含已渲染的术语。
- `errors[]` 只在校验错误时出现，`path` 为 JSON Pointer。
- 5xx 不返回内部信息，只有 `request_id` / `trace_id`。
- **HTTP 状态的确定**（评审 R1 补充）：① 插件返回错误时可显式给出状态（`PluginError.status`）；② 否则用 [catalog/error-codes.md](../catalog/error-codes.md) 登记的状态；③ 都没有时按错误码后缀：

| 后缀 | 状态 |
|---|---|
| `_not_found` | 404 |
| `_taken`、`_duplicate`、`_conflict`、`_exists`、`_in_progress`、`_stale` | 409 |
| `_denied`、`_forbidden`、`_restricted`、`_disabled`、`_banned`、`_required`（权限、等级等前提） | 403 |
| `_unauthenticated`、`_expired_session` | 401 |
| `_rate_limited`、`_too_many` | 429 |
| `_unavailable` | 503 |
| `_timeout` | 504 |
| `_upstream_error` | 502 |
| 其他 | 422 |

错误名（最后一段）恰好等于后缀词本身时同样适用，如 `notes.not_found` → 404。目录未登记且不符合后缀规则的码，插件可在清单 `[error_codes]` 中给出状态。

## 8. 鉴权

| 方式 | 用途 | 传递 |
|---|---|---|
| 会话 cookie | 浏览器 | `__Host-pt_session`（HttpOnly、Secure、SameSite=Lax、Path=/）；非安全方法需 CSRF 头 `X-CSRF-Token`（值等于 cookie `__Host-pt_csrf`） |
| 个人 API token | 脚本、第三方客户端 | `Authorization: Bearer pts_<base62>`；有 scope 与过期时间 |
| MCP token | AI 客户端 | `Authorization: Bearer ptm_<base62>`；由 mcp 插件经 OAuth 签发 |
| RSS / 辅种 key | RSS 阅读器、辅种工具 | 查询参数 `?key=`，仅限 rss、crossseed-api 插件的只读路由，独立于 tracker passkey，可单独重置 |

- **tracker passkey 不得用于 API 鉴权。**
- 路由的 `auth` 级别：`public` / `user` / `admin`（见 [../plugin-api.md](../plugin-api.md) §4）。
- 默认不开放 CORS；需要时在 `site.toml [http.cors]` 显式配置。

## 9. 限流

- 路由在清单中声明：`rate_limit = "ip:30/m,user:120/m"`（语法见 kernel/03 §8.3）。没有声明时只受全站默认限制。
- 写接口、登录、注册、搜索、下载等必须声明限流；评审检查项。
- 响应头（IETF draft-ietf-httpapi-ratelimit-headers）：

```
RateLimit-Policy: "user";q=120;w=60
RateLimit: "user";r=87;t=34
```

- 超限：`429` + `Retry-After: <秒>` + `code = kernel.rate_limited`。

## 10. 幂等

- 有副作用且客户端可能重试的 POST（下单、转账、发帖、上传）在清单声明 `idempotent = true`，客户端发送 `Idempotency-Key: <uuid>`。行为见 kernel/03 §8.4。
- 服务调用之间的幂等用服务方法的 `idempotency_key` 参数，不用 HTTP 头。

## 11. 其他约定

- 请求体上限默认 1 MiB；文件上传（.torrent、字幕、图片）用 `multipart/form-data`，在路由声明 `max_body_kb`。大文件（> 10 MiB）走 `blob` 预签名上传（插件提供签发接口）。
- 条件请求：可缓存的 GET 返回 `ETag`，支持 `If-None-Match`；PATCH / PUT 支持 `If-Match` 做乐观锁（前置条件头不匹配 → 412 `kernel.precondition_failed`；请求体中的 `expected_version` 不匹配 → 409 `kernel.conflict`）。
- 长任务：返回 `202` + `Location: …/jobs/{id}`，客户端轮询。
- 时间相关参数一律 UTC；按站点时区显示由前端负责。
- 不在 URL 中放敏感数据（除 §8 的 RSS key 例外）。

## 12. OpenAPI

- OpenAPI 3.1。Native 插件用 utoipa 从代码生成；WASM 插件随包提供 `openapi.json`（可由 Rust SDK 宏或手写）。
- 片段中路径写插件内相对路径（`/v1/...`），由内核加前缀、改名并合并（kernel/03 §8.5）。
- 每个操作必须：`operationId`（snake_case 动词开头，如 `list_torrents`）、`summary`（中文）、`tags`、全部错误响应（引用内核 `Problem`）、`security`。
- schema 名 PascalCase；MCP 工具的输入输出 schema 与对应 API 共用文件（`$ref` 到 `schemas/`）。
- CI 检查：OpenAPI 可通过 `openapi-typescript` 生成且 `tsc` 通过；与上一个发布版本做 breaking change 检测（`oasdiff`），同一主版本内出现不兼容变更则失败。
