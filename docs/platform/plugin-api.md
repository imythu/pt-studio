# 插件 API 契约（`pt:plugin@1.0.0`）

| 项 | 内容 |
|---|---|
| 文档编号 | PLAT-API |
| 版本 | 1.0 |
| 状态 | 已冻结（P7，2026-10-14） |
| 适用 | 全部 Native 插件与 WASM 插件 |

本文是插件与内核之间的唯一契约：WIT 包、Native Rust trait、`plugin.toml` 清单、插件目录结构和版本兼容策略。名字（服务、事件、钩子、权限点、术语键……）由 [catalog/](catalog/) 下的目录统一分配。

---

## 1. 总体模型

```
                       ┌──────────────────────── 内核 ────────────────────────┐
 plugin.toml ──解析──► │ 加载器 → 服务注册表 → Hook 管线 → 事件日志 → 调度器 → 网关 │
                       └───────▲───────────────────────────────▲──────────────┘
                     host 接口 │ (pt:plugin/*)          导出接口 │ (hooks / on-event / ...)
                       ┌───────┴───────────┐           ┌────────┴───────────┐
                       │ WASM 插件 (组件)   │  服务调用  │ Native 插件 (crate)│
                       │ import pt:ledger/… │◄────────►│ impl LedgerService │
                       └───────────────────┘   经内核   └────────────────────┘
```

三类契约：

| 类别 | 定义位置 | 说明 |
|---|---|---|
| 宿主通用接口（host） | `pt:plugin@1.0.0` 中的 `log`、`config`、`kv`、`sql`、`redis`、`events`、`http-client`、`blob`、`terms`、`time`、`random`、`audit`、`jobs`、`metrics`、`invocation`、`hook-runner`、`metadata` | 由内核实现，所有插件可按能力导入 |
| 插件导出接口（export） | `pt:plugin@1.0.0` 中的 `lifecycle`、`hooks`、`on-event`、`on-cron`、`http-handler`、`search-contributor`、`mcp-tool`、`content-type`、`metadata-provider` | 由插件按需实现，内核调用 |
| 业务服务接口（service） | 每个服务一个 WIT 包：`pt:<service>@x.y.z`，如 `pt:ledger@0.1.0` | 由提供方插件导出、消费方插件导入，内核负责连线 |

### 1.1 服务名与 WIT 名的映射

| 层面 | 规则 | 例子 |
|---|---|---|
| 服务名 | `ctx.<snake_case>` | `ctx.ledger`、`ctx.content_types` |
| WIT 包 | `pt:<kebab-case>@<semver>` | `pt:ledger@0.1.0`、`pt:content-types@0.1.0` |
| WIT 接口 | 与包同名 | `pt:ledger/ledger@0.1.0` |
| Rust trait | `<PascalCase>Service` | `LedgerService`、`ContentTypesService` |
| WIT 函数 / 字段 | kebab-case；Rust 侧为 snake_case | `transfer-request` ↔ `TransferRequest`，`from-user` ↔ `from_user` |

服务 WIT 包的源文件放在**提供方插件**的 `wit/` 目录；内核内置服务（provider = `kernel`）的 WIT 在 `crates/plugin-api/wit/`。

### 1.2 服务接口的类型限制

为了让内核能在 Native ↔ WASM、WASM ↔ WASM 之间通用转发调用，**业务服务接口必须遵守**：

1. 不使用 `resource`、`borrow`、`own`、`stream`、`future`（跨组件资源句柄无法通用桥接）。
2. 只用 `bool`、整数、`f64`、`string`、`list`、`option`、`result`、`tuple`、`record`、`variant`、`enum`、`flags`，以及 `pt:plugin/types` 中的类型别名。
3. 每个可能失败的函数返回 `result<T, plugin-error>`。
4. 不允许回调（服务调用只能沿 `requires` 依赖方向进行，依赖图无环，见 kernel/03 §3.3）。
5. 单次调用参数 + 返回值编码后不超过 4 MiB；更大的数据走 `blob`。

---

## 2. WIT 包 `pt:plugin@1.0.0`（完整定义）

文件：`crates/plugin-api/wit/plugin.wit`（下面是其原文）。1.0.0 的快照保存在 `crates/plugin-api/wit-frozen/pt-plugin-1.0.0.wit`，`crates/plugin-api/tests/api_freeze.rs` 检查已冻结的接口没有被修改。

```wit
/// PT Studio 插件 API。1.0.0 起冻结：此后只做增量变更（新增接口放在新的次版本中），
/// 已有接口、记录、函数签名不再改变。变更记录见 docs/platform/plugin-api.md §6。
package pt:plugin@1.0.0;

/// 公共类型。所有 JSON 一律为 UTF-8 文本，内容须符合对应目录（事件/钩子/MCP）中登记的 JSON Schema。
interface types {
  /// JSON 文本（UTF-8）
  type json = string;
  /// 用户 ID（PostgreSQL BIGINT，保证 < 2^53）
  type user-id = u64;
  /// Unix 毫秒时间戳（UTC）
  type timestamp-ms = s64;

  /// 统一错误。code 必须是 catalog/error-codes.md 规则下的错误码，如 "bonus.insufficient_balance"
  record plugin-error {
    code: string,
    message: string,
    /// 可选的结构化细节（JSON 对象）
    details: option<json>,
    /// 调用方是否可以安全重试
    retryable: bool,
  }

  enum actor-kind {
    anonymous,
    user,
    system,
    %plugin,
    mcp,
  }

  /// 操作发起者
  record actor {
    kind: actor-kind,
    /// kind = user / mcp 时为用户 ID
    user-id: option<user-id>,
    /// kind = plugin / system 时为插件 ID 或 "kernel"
    plugin-id: option<string>,
    /// mcp 时为 token ID；user 时为会话 ID（已脱敏）
    credential-id: option<string>,
  }

  record trace-context {
    /// W3C traceparent
    traceparent: string,
    tracestate: option<string>,
  }

  enum invocation-kind {
    init,
    hook,
    event,
    cron,
    http,
    mcp,
    service,
    search,
    content-type,
    metadata,
  }

  /// 当前调用的上下文（由内核注入，插件只读）
  record invocation-info {
    kind: invocation-kind,
    actor: actor,
    /// 调用方插件（服务调用时为调用者；其他情况为 "kernel"）
    caller-plugin: string,
    /// 调用方在本服务上被授予的作用域，如 ["currency:bonus"]
    caller-scopes: list<string>,
    trace: trace-context,
    /// 请求语言，如 "zh-CN"
    locale: string,
    /// 本次调用的截止时间
    deadline: timestamp-ms,
    /// 服务调用深度（从 0 开始，内核上限 8）
    depth: u8,
    /// 请求 ID（与响应头 x-request-id、日志一致；非 HTTP 入口为内核生成的 ID）
    request-id: string,
    /// 客户端 IP：只传给获授敏感能力 user.ip 的插件，其余为 none
    client-ip: option<string>,
    /// 客户端 User-Agent（截断到 256 字符）
    user-agent: option<string>,
  }
}

/// 读取调用上下文
interface invocation {
  use types.{invocation-info};
  current: func() -> invocation-info;
}

/// 结构化日志（自动附带 plugin_id、trace_id）
interface log {
  enum level { trace, debug, info, warn, error }
  record field { key: string, value: string }
  emit: func(level: level, message: string, fields: list<field>);
}

/// 插件配置：site.toml 静态配置 + 后台可改设置，均已通过插件 JSON Schema 校验
interface config {
  use types.{json};
  /// 整份静态配置（JSON 对象）
  get-static: func() -> json;
  /// 整份运行时设置（JSON 对象），每次调用都返回最新值
  get-settings: func() -> json;
}

/// 插件私有 KV（PostgreSQL 中 kernel 管理的表，按插件隔离）
interface kv {
  use types.{plugin-error, timestamp-ms};
  get: func(key: string) -> result<option<list<u8>>, plugin-error>;
  set: func(key: string, value: list<u8>, expires-at: option<timestamp-ms>) -> result<_, plugin-error>;
  delete: func(key: string) -> result<bool, plugin-error>;
  /// 比较并设置：当前值等于 expected（none 表示不存在）时才写入
  compare-and-set: func(key: string, expected: option<list<u8>>, value: list<u8>) -> result<bool, plugin-error>;
  /// 按前缀列出键，最多 limit 个；cursor 为上次返回的最后一个键
  list-keys: func(prefix: string, cursor: option<string>, limit: u32) -> result<list<string>, plugin-error>;
}

/// 高级 SQL 执行器：只能访问本插件 schema 与已授权的只读视图；强制参数化
interface sql {
  use types.{plugin-error};

  variant sql-value {
    null,
    boolean(bool),
    int(s64),
    float(f64),
    numeric(string),
    text(string),
    bytes(list<u8>),
    /// JSON 文本，对应 jsonb
    json(string),
    /// Unix 微秒，对应 timestamptz
    timestamp(s64),
    /// 规范文本形式，对应 uuid
    uuid(string),
    int-array(list<s64>),
    text-array(list<string>),
  }

  record column { name: string, type-name: string }

  record row-set {
    columns: list<column>,
    rows: list<list<sql-value>>,
    /// 结果被行数上限截断时为 true
    truncated: bool,
  }

  /// 执行查询并返回行（SELECT 或带 RETURNING 的 DML）
  query: func(statement: string, params: list<sql-value>) -> result<row-set, plugin-error>;
  /// 执行 DML，返回受影响行数
  execute: func(statement: string, params: list<sql-value>) -> result<u64, plugin-error>;
  /// 同一语句多组参数批量执行（单次往返），返回受影响总行数
  execute-batch: func(statement: string, param-sets: list<list<sql-value>>) -> result<u64, plugin-error>;
  /// 仅在 manifest 声明 tx = "manual" 的入口中可用：提交当前事务（含已发布事件）并开启新事务
  commit: func() -> result<_, plugin-error>;
}

/// Redis 直接操作：键自动改写为 p:<plugin_id>:<key>，命令受白名单限制
interface redis {
  use types.{plugin-error};

  variant redis-scalar {
    nil,
    int(s64),
    bulk(list<u8>),
  }

  variant redis-reply {
    nil,
    ok,
    int(s64),
    double(f64),
    bulk(list<u8>),
    simple(string),
    /// 一层数组（HGETALL、ZRANGE、SMEMBERS 等）
    array(list<redis-scalar>),
    /// 嵌套结构，按 RESP3 原样编码，由 SDK 解析
    nested(list<u8>),
  }

  record command {
    name: string,
    args: list<list<u8>>,
  }

  /// 执行单条命令
  call: func(cmd: command) -> result<redis-reply, plugin-error>;
  /// 管道执行；atomic = true 时以 MULTI/EXEC 包裹
  pipeline: func(cmds: list<command>, atomic: bool) -> result<list<redis-reply>, plugin-error>;
}

/// 发布领域事件（写入事务内 outbox，提交后投递）
interface events {
  use types.{json, plugin-error};

  record new-event {
    /// 事件类型，如 "bonus.hourly_settled"；必须在 manifest publishes 中声明
    event-type: string,
    /// 载荷版本（正整数）
    version: u32,
    /// 分区与顺序键，如 "user:123"
    key: string,
    payload: json,
  }

  /// 返回事件 ID（UUIDv7）
  publish: func(evt: new-event) -> result<string, plugin-error>;
}

/// 出站 HTTP，仅允许 capabilities.http_outbound 中列出的主机
interface http-client {
  use types.{plugin-error};

  record header { name: string, value: string }

  record outbound-request {
    method: string,
    url: string,
    headers: list<header>,
    body: option<list<u8>>,
    timeout-ms: u32,
  }

  record outbound-response {
    status: u16,
    headers: list<header>,
    body: list<u8>,
  }

  send: func(req: outbound-request) -> result<outbound-response, plugin-error>;
}

/// 对象存储（S3 兼容），键自动加前缀 plugins/<plugin_id>/
interface blob {
  use types.{plugin-error};
  put: func(key: string, content-type: string, data: list<u8>) -> result<_, plugin-error>;
  get: func(key: string) -> result<option<list<u8>>, plugin-error>;
  delete: func(key: string) -> result<_, plugin-error>;
  /// 生成限时下载地址
  presign-get: func(key: string, ttl-secs: u32) -> result<string, plugin-error>;
}

/// 术语服务的便捷接入（实际由 ctx.terms 提供方实现）
interface terms {
  use types.{plugin-error};
  record term {
    key: string,
    name: string,
    short-name: option<string>,
    unit: option<string>,
    icon: option<string>,
  }
  /// locale 为空时使用当前调用上下文的 locale
  get: func(key: string, locale: option<string>) -> option<term>;
  get-many: func(keys: list<string>, locale: option<string>) -> list<term>;
  /// 按 ICU MessageFormat 渲染文本，参数为 JSON 对象
  render: func(template: string, args: string, locale: option<string>) -> result<string, plugin-error>;
}

/// 时间（可在测试中被宿主冻结）
interface time {
  use types.{timestamp-ms};
  now: func() -> timestamp-ms;
  /// 站点时区，如 "Asia/Shanghai"
  site-timezone: func() -> string;
}

/// 随机数与 ID
interface random {
  bytes: func(len: u32) -> list<u8>;
  /// 新 UUIDv7（文本形式）
  uuid-v7: func() -> string;
}

/// 审计（内核已自动审计写操作入口；此接口用于补充业务细节）
interface audit {
  use types.{json, plugin-error};
  record audit-entry {
    /// 动作，如 "bonus.adjust"
    action: string,
    /// 目标，如 "user:123"
    target: string,
    details: json,
  }
  write: func(entry: audit-entry) -> result<_, plugin-error>;
}

/// 一次性延迟任务；到期后通过 on-cron.run 回调（kind = delayed）
interface jobs {
  use types.{json, plugin-error, timestamp-ms};
  /// 返回任务 ID；dedupe-key 相同的未执行任务只保留一个
  schedule-once: func(name: string, run-at: timestamp-ms, payload: json, dedupe-key: option<string>) -> result<string, plugin-error>;
  cancel: func(job-id: string) -> result<bool, plugin-error>;
}

/// 指标（自动附带 plugin 标签）
interface metrics {
  record label { key: string, value: string }
  counter-add: func(name: string, value: u64, labels: list<label>);
  gauge-set: func(name: string, value: f64, labels: list<label>);
  histogram-observe: func(name: string, value: f64, labels: list<label>);
}

/// 触发由本插件定义的钩子点（manifest [[hook_points]] 中声明）
interface hook-runner {
  use types.{json, plugin-error};

  record denial {
    code: string,
    message: string,
    /// 作出拒绝的插件
    by-plugin: string,
  }

  variant hook-outcome {
    /// 全部放行；携带合并了所有 modify 补丁后的最终输入
    allowed(json),
    denied(denial),
  }

  run: func(hook: string, input: json) -> result<hook-outcome, plugin-error>;
}

/// 元数据聚合（内核把调用路由到对应 metadata-* 插件的 metadata-provider 导出）
interface metadata {
  use types.{json, plugin-error};

  record metadata-record {
    source: string,
    external-id: string,
    data: json,
    fetched-at: s64,
  }

  sources: func() -> list<string>;
  lookup: func(source: string, external-id: string, locale: option<string>) -> result<option<metadata-record>, plugin-error>;
  search: func(source: string, query: string, type-id: string, limit: u32) -> result<list<metadata-record>, plugin-error>;
}


/// 调用依赖的业务服务（JSON 分派；清单 `requires` / `optional_requires` 中声明的服务才可调用）。
/// method 为服务方法名（snake_case），args / 返回值为 JSON，方法与参数见 catalog/services.md。
/// 类型化的业务服务 WIT 包（`pt:<svc>`）若引入，将以新增包的方式提供，本接口保持不变。
interface services {
  use types.{json, plugin-error};
  call: func(service: string, method: string, args: json) -> result<json, plugin-error>;
}

/// 提供业务服务（清单 `provides` 中的服务），与 services.call 对应
interface service-provider {
  use types.{json, plugin-error};
  call: func(service: string, method: string, args: json) -> result<json, plugin-error>;
}

// ---------------------------------------------------------------------------
// 插件导出接口
// ---------------------------------------------------------------------------

/// 生命周期：启动时按拓扑顺序调用一次
interface lifecycle {
  use types.{plugin-error};
  init: func() -> result<_, plugin-error>;
}

/// 同步钩子
interface hooks {
  use types.{json, plugin-error};

  record hook-call {
    /// 钩子名，如 "torrent.before_upload"
    hook: string,
    /// 输入（JSON，结构见 catalog/hooks.md 与对应 schema）
    input: json,
  }

  record deny-info {
    /// 错误码，如 "invite.code_invalid"
    code: string,
    /// 面向用户的说明（可含术语占位，由内核渲染）
    message: string,
    details: option<json>,
  }

  variant hook-decision {
    allow,
    deny(deny-info),
    /// JSON Merge Patch（RFC 7396），只能修改该钩子登记的可修改字段
    modify(json),
  }

  handle: func(call: hook-call) -> result<hook-decision, plugin-error>;
}

/// 事件订阅
interface on-event {
  use types.{actor, json, plugin-error, timestamp-ms};

  record event-envelope {
    id: string,
    event-type: string,
    version: u32,
    occurred-at: timestamp-ms,
    actor: actor,
    source: string,
    key: string,
    payload: json,
    traceparent: string,
  }

  record delivery-info {
    /// 订阅名（manifest [[subscriptions]].name）
    subscription: string,
    /// 第几次投递，从 1 开始
    attempt: u32,
    /// 是否来自回放
    replay: bool,
  }

  handle: func(evt: event-envelope, delivery: delivery-info) -> result<_, plugin-error>;
}

/// 定时任务与延迟任务
interface on-cron {
  use types.{json, plugin-error, timestamp-ms};

  enum job-kind { cron, delayed, manual }

  record job-invocation {
    /// cron 名或延迟任务名
    name: string,
    kind: job-kind,
    /// 计划触发时间
    scheduled-at: timestamp-ms,
    /// 延迟任务的载荷；cron 为 "{}"
    payload: json,
    run-id: string,
  }

  run: func(job: job-invocation) -> result<_, plugin-error>;
}

/// HTTP 处理（路由在 manifest [[http.routes]] 中声明，内核已完成鉴权、权限、限流）
interface http-handler {
  use types.{plugin-error};

  record header { name: string, value: string }

  record request {
    /// manifest 中的路由 ID
    route-id: string,
    method: string,
    /// 去掉 /api/p/<plugin_id> 前缀后的路径，如 "/v1/items/42"
    path: string,
    /// 路径参数（按路由模板解析）
    params: list<tuple<string, string>>,
    query: list<tuple<string, string>>,
    headers: list<header>,
    body: list<u8>,
  }

  record response {
    status: u16,
    headers: list<header>,
    body: list<u8>,
  }

  handle: func(req: request) -> result<response, plugin-error>;
}

/// 搜索贡献（由 ctx.search 的索引 worker 调用）
interface search-contributor {
  use types.{json, plugin-error};

  record field-spec {
    /// 字段名，类型插件的字段须带前缀，如 "movie_imdb_id"
    name: string,
    /// "string" | "int" | "float" | "bool" | "timestamp" | "string[]"
    field-type: string,
    searchable: bool,
    filterable: bool,
    sortable: bool,
    /// 可搜索字段的权重顺序，越小越靠前
    rank: u8,
    /// 关闭拼写容错
    exact: bool,
    /// 术语键（筛选面板显示名）
    label-term: string,
  }

  record index-schema {
    /// 目标索引，如 "torrents"、"groups"
    index: string,
    fields: list<field-spec>,
    /// 同义词组
    synonyms: list<list<string>>,
  }

  index-schemas: func() -> list<index-schema>;
  /// 把实体转成文档片段（JSON 对象）；返回 none 表示该实体不属于本插件
  to-document: func(index: string, entity-id: u64) -> result<option<json>, plugin-error>;
}

/// MCP 工具实现（工具在 manifest [[mcp.tools]] 声明）
interface mcp-tool {
  use types.{json, plugin-error};
  /// arguments 已通过输入 schema 校验；返回值须符合输出 schema
  call: func(tool: string, arguments: json) -> result<json, plugin-error>;
}

/// 资源类型（ctx.content_types 的多实现提供方导出）
interface content-type {
  use types.{json, plugin-error};

  record type-info {
    /// 类型 ID，如 "movie"
    type-id: string,
    /// 术语键，如 "type-movie.name"
    name-term: string,
    /// 字段 JSON Schema（上传表单据此生成）
    field-schema: json,
    /// Group 字段 JSON Schema；不使用 Group 时为 none
    group-schema: option<json>,
  }

  info: func() -> type-info;
  /// 校验并规范化上传数据，返回规范化后的字段 JSON
  normalize: func(fields: json) -> result<json, plugin-error>;
  /// 计算分组键（同键归入同一 Group）；不分组返回 none
  group-key: func(fields: json) -> result<option<string>, plugin-error>;
  /// 生成规范标题
  canonical-title: func(fields: json) -> result<string, plugin-error>;
}

/// 元数据源（ctx.metadata 的多实现提供方导出）
interface metadata-provider {
  use types.{json, plugin-error};

  record source-info {
    /// 来源 ID，如 "tmdb"
    source: string,
    /// 支持的资源类型 ID
    type-ids: list<string>,
    /// 外部 ID 的正则（用于识别链接/ID）
    id-pattern: string,
  }

  record lookup-result {
    source: string,
    external-id: string,
    /// 归一化字段（JSON 对象，键名见 ctx.metadata 约定）
    data: json,
    fetched-at: s64,
  }

  info: func() -> source-info;
  lookup: func(external-id: string, locale: string) -> result<option<lookup-result>, plugin-error>;
  search: func(query: string, type-id: string, limit: u32) -> result<list<lookup-result>, plugin-error>;
}

// ---------------------------------------------------------------------------
// 世界
// ---------------------------------------------------------------------------

/// 宿主可提供的全部通用导入。插件只会被链接到它实际导入且已获授权的接口。
world host {
  import types;
  import invocation;
  import log;
  import config;
  import kv;
  import sql;
  import redis;
  import events;
  import http-client;
  import blob;
  import terms;
  import time;
  import random;
  import audit;
  import jobs;
  import metrics;
  import hook-runner;
  import metadata;
  import services;
}

/// 参考世界：插件按需 include host 并只 export 自己实现的接口
world example-full {
  include host;
  export lifecycle;
  export hooks;
  export on-event;
  export on-cron;
  export http-handler;
  export search-contributor;
  export mcp-tool;
  export content-type;
  export metadata-provider;
  export service-provider;
}

/// 内核绑定使用的世界。暂不含 metadata-provider：wasmtime 49 的 bindgen 在导出接口含名为 `lookup`
/// 的函数时生成的代码有变量遮蔽错误；元数据插件接入前改为动态查找导出。
world kernel {
  include host;
  export lifecycle;
  export hooks;
  export on-event;
  export on-cron;
  export http-handler;
  export search-contributor;
  export mcp-tool;
  export content-type;
  export service-provider;
}
```

### 2.0 v0.1 实现说明：业务服务的 JSON 分派（2026-10-07）

§2.2 / §2.3 描述的“每个业务服务一个 WIT 包、内核动态桥接”需要 `service!` 宏为每个服务生成 Val ↔ Rust 适配层，该宏尚未实现。为不阻塞 WASM 插件，`pt:plugin@1.0.0` 增加两个接口（已写入 `crates/plugin-api/wit/plugin.wit`）：

| 接口 | 方向 | 说明 |
|---|---|---|
| `services.call(service, method, args: json) -> result<json, plugin-error>` | import | 调用 `requires` 中声明的服务；依赖检查、调用深度、重入检测、作用域传递、独立工作单元与类型化调用完全相同 |
| `service-provider.call(service, method, args: json)` | export | WASM 插件提供 `provides` 中的服务 |

约定：方法名为服务方法的 snake_case 名；只有一个请求结构体参数的方法，`args` 即该结构体（如 `ledger.transfer` 的 `TransferRequest`）；多参数方法用具名字段（如 `{"currency": "bonus", "user": 1}`）。Native 服务通过 `JsonService` trait 提供同样的分派（`NativePlugin.json_services`），Native 插件也可用 `ctx.call_json(...)` 调用 WASM 提供的服务。

1.0 的业务服务一律经 JSON 分派（`services` / `service-provider`）。类型化业务服务 WIT 包（`pt:<svc>`）如果引入，将作为新增的包提供，JSON 分派继续保留。

另：内核以 `world kernel`（不含 `metadata-provider` 导出）生成宿主绑定，原因是 wasmtime 49 bindgen 在导出接口含名为 `lookup` 的函数时生成代码有变量遮蔽错误；元数据插件接入前改为动态查找该导出或升级 wasmtime。

### 2.0.1 v0.1 实现说明：搜索贡献者的调用入口（2026-10-07）

services.md 中 `ctx.plugins.search_contributors()` 只返回插件 ID，没有调用贡献者的入口。v0.1 在 `Ctx` 上提供三个方法，仅对 `provides` 含 `ctx.search` 的插件开放（否则 `kernel.capability_denied`）：

| 方法 | 说明 |
|---|---|
| `ctx.search_contributors() -> Vec<plugin_id>` | 实现了 `search-contributor` 的插件，按激活顺序 |
| `ctx.contributor_schemas(plugin) -> Vec<IndexSchema>` | 调用其 `index-schemas`，超时 2 s |
| `ctx.contributor_documents(plugin, index, ids) -> Vec<(id, Option<json>)>` | 调用其 `to-documents`，超时 5 s；WASM 贡献者逐条调用 WIT 的 `to-document` |

每次调用为贡献者构造独立工作单元（`InvocationKind::Search`，沿用调用方的 actor、截止时间与调用链）。加载时检查清单 `capabilities.search_contributor` 与导出一致（`E-LOAD-WORLD`）。WIT 的批量 `to-documents`、`list-ids` 与 `index-schema.owner` 等字段（search-meilisearch HLD §7）留待 0.2；在此之前拥有者由搜索插件的标准索引登记表决定，重建用拥有者的只读视图枚举。

### 2.1 接口说明与能力映射

| 接口 | 方向 | 需要的能力（`[capabilities]`） | 说明 |
|---|---|---|---|
| `types`、`invocation`、`log`、`config`、`time`、`random`、`metrics` | import | 无（总是可用） | |
| `kv` | import | `kv = true`（默认 true） | 存在 `kernel.plugin_kv` 表，按插件隔离，单值 ≤ 64 KiB |
| `sql` | import | `sql = true` | 见 kernel/03 §6 |
| `redis` | import | `redis = true` | 见 [catalog/redis-keys.md](catalog/redis-keys.md) |
| `events` | import | 事件须列在 `publishes` | 事件类型须在 [catalog/events.md](catalog/events.md) 中归属本插件 |
| `http-client` | import | `http_outbound = ["api.themoviedb.org", ...]` | 空列表等于禁止 |
| `blob` | import | `blob = true` | 键前缀 `plugins/<plugin_id>/` |
| `terms` | import | 无 | 转发给 `ctx.terms` |
| `audit` | import | 无 | |
| `jobs` | import | 无；任务名须在 `[[jobs]]` 中声明 | |
| `hook-runner` | import | 钩子须在 `[[hook_points]]` 中声明 | 只能触发本插件拥有的钩子点 |
| `metadata` | import | `requires` 含 `ctx.metadata` | |
| `lifecycle` | export | — | 可选 |
| `hooks` | export | `[[hooks]]` 中声明 | |
| `on-event` | export | `[[subscriptions]]` 中声明 | |
| `on-cron` | export | `[[cron]]` 或 `[[jobs]]` 中声明 | |
| `http-handler` | export | `[[http.routes]]` 中声明 | |
| `search-contributor` | export | `search_contributor = true` | |
| `mcp-tool` | export | `[[mcp.tools]]` 中声明 | |
| `content-type` | export | `provides` 含 `ctx.content_types` | |
| `metadata-provider` | export | `provides` 含 `ctx.metadata` | |

**导入与授权的关系**：组件导入了未获授权的接口时，内核**不拒绝启动**（某些工具链会导入全部接口），而是链接一个“拒绝桩”：每次调用都返回 `kernel.capability_denied`，并在启动日志中列出。导出了清单未声明的接口，或清单声明了却没有导出，则**拒绝启动**。

### 2.2 插件自己的 world

插件不直接使用 `example-full`，而是在自己的 `wit/world.wit` 中 `include` host 并只导出实现了的接口：

```wit
package pt:bonus-plugin@1.2.0;

world bonus {
  include pt:plugin/host@1.0.0;
  import pt:ledger/ledger@0.1.0;      // 依赖的业务服务
  import pt:users/users@0.1.0;
  export pt:bonus/bonus@0.1.0;        // 提供的业务服务
  export pt:plugin/lifecycle@1.0.0;
  export pt:plugin/on-event@1.0.0;
  export pt:plugin/on-cron@1.0.0;
  export pt:plugin/mcp-tool@1.0.0;
}
```

### 2.3 业务服务 WIT 示例（格式范本）

```wit
package pt:ledger@0.1.0;

interface ledger {
  use pt:plugin/types@1.0.0.{plugin-error, user-id};

  record transfer-request {
    currency: string,
    from-user: option<user-id>,   // none = 系统发行
    to-user: option<user-id>,     // none = 系统回收
    amount: s64,                  // 最小单位，见 currency.scale
    reason: string,               // 原因码，如 "bonus.hourly"
    memo: string,
    idempotency-key: string,
  }

  transfer: func(req: transfer-request) -> result<string, plugin-error>;
  balance: func(currency: string, user: user-id) -> result<s64, plugin-error>;
}
```

以上仅为格式示例，`ctx.ledger` 的正式签名由 ledger 插件的详细设计给出。

### 2.4 调用语义

| 项 | 约定 |
|---|---|
| 实例模型 | 每次调用（一次钩子、一条事件、一次 HTTP 请求、一次服务调用）使用一个新的组件实例，调用结束即销毁。**插件不能在内存中跨调用保存状态**，状态放 kv / sql / redis |
| 事务 | 每次调用是一个工作单元：首次 `sql` 调用时开启事务；`events.publish` 写入同一事务的 outbox；调用返回 `ok` 时提交，返回 `err` 或陷入（trap）时回滚。`tx = "manual"` 的入口可用 `sql.commit()` 分段提交 |
| 嵌套服务调用 | 服务调用在被调方自己的事务中执行（不共享调用方事务）。需要原子性的跨插件操作必须用幂等键 + 事件补偿 |
| 超时 | 每次调用有截止时间（`invocation.current().deadline`），由 epoch 中断强制执行；嵌套调用继承剩余时间 |
| 调用深度 | 服务调用最多嵌套 8 层，超过返回 `kernel.call_depth_exceeded` |
| actor 传递 | 服务调用自动传递原始 actor；`caller-plugin` 改为直接调用方 |
| 错误 | 业务错误用 `plugin-error`；trap、超时、内存超限由内核转为 `kernel.plugin_trapped` / `kernel.timeout` / `kernel.resource_exhausted` |

---

## 3. Native 插件的 Rust trait（与 WIT 一一对应）

crate：`crates/plugin-api`（模块 `pt_plugin_api`）。Native 插件与 WASM 插件对外行为完全一致；Native 插件额外可以使用少量高性能接口（标注“仅 Native”）。

```rust
//! crates/plugin-api/src/lib.rs（节选，签名为契约）
use async_trait::async_trait;
use serde_json::Value as Json;

pub type UserId = u64;
pub type TimestampMs = i64;

// ---------- types ----------
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct PluginError {
    pub code: String,            // "bonus.insufficient_balance"
    pub message: String,
    pub details: Option<Json>,
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind { Anonymous, User, System, Plugin, Mcp }

#[derive(Debug, Clone)]
pub struct Actor {
    pub kind: ActorKind,
    pub user_id: Option<UserId>,
    pub plugin_id: Option<String>,
    pub credential_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvocationKind { Init, Hook, Event, Cron, Http, Mcp, Service, Search, ContentType, Metadata }

#[derive(Debug, Clone)]
pub struct InvocationInfo {
    pub kind: InvocationKind,
    pub actor: Actor,
    pub caller_plugin: String,
    pub caller_scopes: Vec<String>,
    pub traceparent: String,
    pub tracestate: Option<String>,
    pub locale: String,
    pub deadline: TimestampMs,
    pub depth: u8,
}

pub type PResult<T> = Result<T, PluginError>;

// ---------- host 接口（内核实现，插件通过 Ctx 使用）----------
#[async_trait] pub trait Log: Send + Sync {
    fn emit(&self, level: tracing::Level, message: &str, fields: &[(&str, &str)]);
}
#[async_trait] pub trait Config: Send + Sync {
    fn get_static(&self) -> Json;
    async fn get_settings(&self) -> Json;
}
#[async_trait] pub trait Kv: Send + Sync {
    async fn get(&self, key: &str) -> PResult<Option<Vec<u8>>>;
    async fn set(&self, key: &str, value: &[u8], expires_at: Option<TimestampMs>) -> PResult<()>;
    async fn delete(&self, key: &str) -> PResult<bool>;
    async fn compare_and_set(&self, key: &str, expected: Option<&[u8]>, value: &[u8]) -> PResult<bool>;
    async fn list_keys(&self, prefix: &str, cursor: Option<&str>, limit: u32) -> PResult<Vec<String>>;
}

pub enum SqlValue { Null, Boolean(bool), Int(i64), Float(f64), Numeric(String), Text(String),
    Bytes(Vec<u8>), Json(Json), Timestamp(i64 /* unix µs */), Uuid(uuid::Uuid),
    IntArray(Vec<i64>), TextArray(Vec<String>) }
pub struct Column { pub name: String, pub type_name: String }
pub struct RowSet { pub columns: Vec<Column>, pub rows: Vec<Vec<SqlValue>>, pub truncated: bool }

#[async_trait] pub trait Sql: Send + Sync {
    async fn query(&self, stmt: &str, params: &[SqlValue]) -> PResult<RowSet>;
    async fn execute(&self, stmt: &str, params: &[SqlValue]) -> PResult<u64>;
    async fn execute_batch(&self, stmt: &str, param_sets: &[Vec<SqlValue>]) -> PResult<u64>;
    async fn commit(&self) -> PResult<()>;
    /// 仅 Native：COPY FROM STDIN 到本插件 schema 中的表（同样在 SET ROLE 之下执行）
    async fn copy_in(&self, table: &str, columns: &[&str], csv: bytes::Bytes) -> PResult<u64>;
}

pub enum RedisScalar { Nil, Int(i64), Bulk(Vec<u8>) }
pub enum RedisReply { Nil, Ok, Int(i64), Double(f64), Bulk(Vec<u8>), Simple(String),
    Array(Vec<RedisScalar>), Nested(Vec<u8> /* RESP3 */) }
pub struct RedisCommand { pub name: String, pub args: Vec<Vec<u8>> }

#[async_trait] pub trait Redis: Send + Sync {
    async fn call(&self, cmd: RedisCommand) -> PResult<RedisReply>;
    async fn pipeline(&self, cmds: Vec<RedisCommand>, atomic: bool) -> PResult<Vec<RedisReply>>;
    /// 仅 Native 且获授 redis_global 的插件：访问全局前缀（不改写键名），
    /// role 为 site.toml [redis] 中的实例角色名（main / acct / tracker_state / tracker_sync）
    fn global(&self, role: &str) -> PResult<redis::aio::ConnectionManager>;
}

pub struct NewEvent { pub event_type: String, pub version: u32, pub key: String, pub payload: Json }
#[async_trait] pub trait Events: Send + Sync {
    async fn publish(&self, evt: NewEvent) -> PResult<uuid::Uuid>;
}

#[async_trait] pub trait HttpClient: Send + Sync {
    async fn send(&self, req: http::Request<bytes::Bytes>, timeout: std::time::Duration)
        -> PResult<http::Response<bytes::Bytes>>;
}
#[async_trait] pub trait Blob: Send + Sync {
    async fn put(&self, key: &str, content_type: &str, data: bytes::Bytes) -> PResult<()>;
    async fn get(&self, key: &str) -> PResult<Option<bytes::Bytes>>;
    async fn delete(&self, key: &str) -> PResult<()>;
    async fn presign_get(&self, key: &str, ttl_secs: u32) -> PResult<String>;
}
pub struct Term { pub key: String, pub name: String, pub short_name: Option<String>,
    pub unit: Option<String>, pub icon: Option<String> }
#[async_trait] pub trait Terms: Send + Sync {
    async fn get(&self, key: &str, locale: Option<&str>) -> Option<Term>;
    async fn get_many(&self, keys: &[&str], locale: Option<&str>) -> Vec<Term>;
    async fn render(&self, template: &str, args: &Json, locale: Option<&str>) -> PResult<String>;
}
pub trait Time: Send + Sync { fn now(&self) -> TimestampMs; fn site_timezone(&self) -> chrono_tz::Tz; }
pub trait Random: Send + Sync { fn bytes(&self, len: usize) -> Vec<u8>; fn uuid_v7(&self) -> uuid::Uuid; }
pub struct AuditEntry { pub action: String, pub target: String, pub details: Json }
#[async_trait] pub trait Audit: Send + Sync { async fn write(&self, e: AuditEntry) -> PResult<()>; }
#[async_trait] pub trait Jobs: Send + Sync {
    async fn schedule_once(&self, name: &str, run_at: TimestampMs, payload: Json, dedupe_key: Option<&str>) -> PResult<String>;
    async fn cancel(&self, job_id: &str) -> PResult<bool>;
}
pub trait Metrics: Send + Sync {
    fn counter_add(&self, name: &str, v: u64, labels: &[(&str, &str)]);
    fn gauge_set(&self, name: &str, v: f64, labels: &[(&str, &str)]);
    fn histogram_observe(&self, name: &str, v: f64, labels: &[(&str, &str)]);
}
pub enum HookOutcome { Allowed(Json), Denied { code: String, message: String, by_plugin: String } }
#[async_trait] pub trait HookRunner: Send + Sync {
    async fn run(&self, hook: &str, input: Json) -> PResult<HookOutcome>;
}

/// 插件上下文：每次调用由内核构造，绑定插件身份、能力与 InvocationInfo
pub struct Ctx {
    pub plugin_id: &'static str,
    pub invocation: InvocationInfo,
    pub log: std::sync::Arc<dyn Log>,
    pub config: std::sync::Arc<dyn Config>,
    pub kv: std::sync::Arc<dyn Kv>,
    pub sql: std::sync::Arc<dyn Sql>,
    pub redis: std::sync::Arc<dyn Redis>,
    pub events: std::sync::Arc<dyn Events>,
    pub http: std::sync::Arc<dyn HttpClient>,
    pub blob: std::sync::Arc<dyn Blob>,
    pub terms: std::sync::Arc<dyn Terms>,
    pub time: std::sync::Arc<dyn Time>,
    pub random: std::sync::Arc<dyn Random>,
    pub audit: std::sync::Arc<dyn Audit>,
    pub jobs: std::sync::Arc<dyn Jobs>,
    pub metrics: std::sync::Arc<dyn Metrics>,
    pub hooks: std::sync::Arc<dyn HookRunner>,
    pub services: ServiceHandle,
}

impl Ctx {
    /// 取已选定的单实现服务：ctx.service::<dyn LedgerService>("ctx.ledger")
    pub fn service<S: ?Sized + 'static>(&self, name: &str) -> PResult<std::sync::Arc<S>> { unimplemented!() }
    /// 取多实现服务的全部提供方：(plugin_id, 实现)
    pub fn providers<S: ?Sized + 'static>(&self, name: &str) -> Vec<(String, std::sync::Arc<S>)> { unimplemented!() }
}
pub struct ServiceHandle { /* 内核私有 */ }

// ---------- 导出接口（插件实现，内核调用）----------
#[async_trait] pub trait Lifecycle: Send + Sync {
    async fn init(&self, ctx: &Ctx) -> PResult<()>;
}

pub struct HookCall { pub hook: String, pub input: Json }
pub enum HookDecision { Allow, Deny { code: String, message: String, details: Option<Json> }, Modify(Json /* merge patch */) }
#[async_trait] pub trait Hooks: Send + Sync {
    async fn handle(&self, ctx: &Ctx, call: HookCall) -> PResult<HookDecision>;
}

pub struct EventEnvelope { pub id: uuid::Uuid, pub event_type: String, pub version: u32,
    pub occurred_at: TimestampMs, pub actor: Actor, pub source: String, pub key: String,
    pub payload: Json, pub traceparent: String }
pub struct DeliveryInfo { pub subscription: String, pub attempt: u32, pub replay: bool }
#[async_trait] pub trait OnEvent: Send + Sync {
    async fn handle(&self, ctx: &Ctx, evt: EventEnvelope, d: DeliveryInfo) -> PResult<()>;
}

pub enum JobKind { Cron, Delayed, Manual }
pub struct JobInvocation { pub name: String, pub kind: JobKind, pub scheduled_at: TimestampMs, pub payload: Json, pub run_id: String }
#[async_trait] pub trait OnCron: Send + Sync {
    async fn run(&self, ctx: &Ctx, job: JobInvocation) -> PResult<()>;
}

/// Native 插件的 HTTP 直接注册 axum Router（挂在 /api/p/<plugin_id> 下）；
/// 内核仍按清单中的路由声明执行鉴权、权限、限流（未声明的路由不会被挂载）。
pub trait HttpRoutes: Send + Sync {
    fn router(&self) -> axum::Router<std::sync::Arc<NativeState>>;
}
pub struct NativeState { /* 内核注入：用于在 handler 中构造 Ctx */ }

pub struct FieldSpec { pub name: String, pub field_type: String, pub searchable: bool, pub filterable: bool,
    pub sortable: bool, pub rank: u8, pub exact: bool, pub label_term: String }
pub struct IndexSchema { pub index: String, pub fields: Vec<FieldSpec>, pub synonyms: Vec<Vec<String>> }
#[async_trait] pub trait SearchContributor: Send + Sync {
    async fn index_schemas(&self, ctx: &Ctx) -> Vec<IndexSchema>;
    async fn to_document(&self, ctx: &Ctx, index: &str, entity_id: u64) -> PResult<Option<Json>>;
}

#[async_trait] pub trait McpTool: Send + Sync {
    async fn call(&self, ctx: &Ctx, tool: &str, arguments: Json) -> PResult<Json>;
}

pub struct TypeInfo { pub type_id: String, pub name_term: String, pub field_schema: Json, pub group_schema: Option<Json> }
#[async_trait] pub trait ContentType: Send + Sync {
    async fn info(&self, ctx: &Ctx) -> TypeInfo;
    async fn normalize(&self, ctx: &Ctx, fields: Json) -> PResult<Json>;
    async fn group_key(&self, ctx: &Ctx, fields: &Json) -> PResult<Option<String>>;
    async fn canonical_title(&self, ctx: &Ctx, fields: &Json) -> PResult<String>;
}

pub struct SourceInfo { pub source: String, pub type_ids: Vec<String>, pub id_pattern: String }
pub struct LookupResult { pub source: String, pub external_id: String, pub data: Json, pub fetched_at: i64 }
#[async_trait] pub trait MetadataProvider: Send + Sync {
    async fn info(&self, ctx: &Ctx) -> SourceInfo;
    async fn lookup(&self, ctx: &Ctx, external_id: &str, locale: &str) -> PResult<Option<LookupResult>>;
    async fn search(&self, ctx: &Ctx, query: &str, type_id: &str, limit: u32) -> PResult<Vec<LookupResult>>;
}

// ---------- Native 插件入口 ----------
pub struct NativePlugin {
    /// plugin.toml 原文（include_str!），与 WASM 插件使用同一份清单格式
    pub manifest: &'static str,
    /// 迁移文件（include_dir!），文件名与 WASM 插件 migrations/ 规则相同
    pub migrations: &'static [(&'static str, &'static str)],
    pub lifecycle: Option<std::sync::Arc<dyn Lifecycle>>,
    pub hooks: Option<std::sync::Arc<dyn Hooks>>,
    pub on_event: Option<std::sync::Arc<dyn OnEvent>>,
    pub on_cron: Option<std::sync::Arc<dyn OnCron>>,
    pub http: Option<std::sync::Arc<dyn HttpRoutes>>,
    pub search_contributor: Option<std::sync::Arc<dyn SearchContributor>>,
    pub mcp_tool: Option<std::sync::Arc<dyn McpTool>>,
    pub content_type: Option<std::sync::Arc<dyn ContentType>>,
    pub metadata_provider: Option<std::sync::Arc<dyn MetadataProvider>>,
    /// 提供的业务服务：服务名 → 实现（类型擦除，由 ctx.service::<dyn XxxService>() 取回）
    pub services: Vec<(&'static str, std::sync::Arc<dyn std::any::Any + Send + Sync>)>,
    /// 把业务服务暴露给 WASM 消费方的适配器（由 pt_plugin_api::export_service! 宏生成）
    pub service_adapters: Vec<ServiceAdapter>,
}
pub struct ServiceAdapter { /* 由宏生成：WIT 包名 + 动态调用表（component::Val ↔ Rust 类型） */ }
```

### 3.1 Native 插件注册

- 每个 Native 插件 crate 导出 `pub fn plugin() -> pt_plugin_api::NativePlugin`。
- 内核二进制 `pt-server`（`crates/server`）用 cargo feature 选择编译哪些 Native 插件，`native_plugins![identity, torrent, ...]` 宏生成静态列表。**不使用** `inventory`/`linkme` 等链接期注册（避免链接器差异）。
- 编译进二进制 ≠ 启用：是否启用由 `site.toml [plugins]` 决定（见 kernel/03 §11）。

### 3.2 业务服务的 Rust 侧

- 业务服务先写 WIT，再用 `pt_plugin_api::service!("wit/ledger.wit")` 宏生成：`trait LedgerService`（`#[async_trait]`，每个方法第一个参数为 `&Ctx`）、请求/响应结构体、`ServiceAdapter`（把 Rust 实现暴露给 WASM）、`LedgerClient`（把 WASM 实现包装成 `dyn LedgerService`，供 Native 消费方使用）。
- 因此四种组合（Native→Native、Native→WASM、WASM→Native、WASM→WASM）对插件作者都是透明的，详见 kernel/03 §4。

---

## 4. 插件清单 `plugin.toml`

### 4.1 完整示例

```toml
# ---------- 基本信息 ----------
id = "bonus"                       # ^[a-z][a-z0-9-]{1,31}$，全局唯一，见 catalog/plugins.md
name = "魔力值"                     # 后台显示名（可用术语键：name_term）
version = "1.2.0"                  # semver
kind = "wasm"                      # "wasm" | "native"
api = "pt:plugin@1"              # 依赖的插件 API 版本（主.次）
kernel = ">=0.1.0, <0.3.0"         # 可选：兼容的内核版本范围
license = "MIT OR Apache-2.0"      # SPDX；官方插件必须为此值（D19），见 conventions/licensing.md
authors = ["PT Studio"]
description = "按做种情况发放魔力值"

# ---------- 服务 ----------
provides = ["ctx.bonus@0.1.0"]
requires = ["ctx.ledger@^0.1", "ctx.users@^0.1", "ctx.accounting@^0.1"]
optional_requires = ["ctx.levels@^0.1"]  # 缺失时插件仍可启动

publishes = ["bonus.hourly_settled"]  # 顶层字段：本插件发布的事件

# ---------- 能力 ----------
[capabilities]
kv = true
sql = true
redis = true
blob = false
http_outbound = []                 # 允许访问的主机（精确或 *.example.com）
views = ["identity.v_user_public"] # 只读视图（提供方 schema.视图名）
sensitive = []                     # 敏感数据：须在 site.toml [grants.<id>] 中授权
sensitive_optional = []
search_contributor = false

[capabilities.scopes]              # 在其他服务上申请的作用域（由提供方解释）
"ctx.ledger" = ["currency:bonus"]
"ctx.accounting" = ["read"]

# ---------- 事件 ----------
# 注意：publishes 是顶层字段，必须写在任何 [table] 之前（见本示例开头）

[[subscriptions]]
name = "on_seeding_snapshot"       # 订阅名，同插件内唯一
event = "traffic.seeding_snapshot_taken"
versions = [1]                     # 接受的载荷版本
ordered = true                     # 按 key 顺序串行处理（同分区）
max_attempts = 10
tx = "auto"                        # "auto" | "manual"

# ---------- 钩子 ----------
[[hooks]]
hook = "shop.before_purchase"
priority = 100                     # 越小越先执行，默认 0
timeout_ms = 200                   # 不得超过钩子点定义的上限

# 本插件定义、由其他插件处理的钩子点（须在 catalog/hooks.md 归属本插件；bonus 目前没有，以 shop 为例）：
# [[hook_points]]
# name = "shop.before_purchase"
# input_schema = "schemas/hooks/shop.before_purchase.json"

# ---------- 定时任务 / 延迟任务 ----------
[[cron]]
name = "hourly_seed_bonus"
schedule = "5 * * * *"             # 5 段 cron，按站点时区
timeout_secs = 600
catch_up = "once"                  # "none" | "once"
tx = "manual"

[[jobs]]
name = "expire_bonus_boost"        # 允许通过 jobs.schedule-once 调度的延迟任务名

# ---------- HTTP ----------
[http]
openapi = "openapi.json"           # 本插件路由的 OpenAPI 3.1 片段

[[http.routes]]
id = "get_summary"
method = "GET"
path = "/v1/summary"               # 实际路径 /api/p/bonus/v1/summary
auth = "user"                      # "public" | "user" | "admin"（admin = 需要 permission）
permission = "bonus.balance.read_own"  # 可选
rate_limit = "user:60/m"           # 见 conventions/api.md §9
idempotent = false                 # true = POST 支持 Idempotency-Key
max_body_kb = 64

# ---------- 权限点 ----------
[[permissions]]
name = "bonus.balance.adjust"
description_term = "bonus.perm.balance_adjust"
default_roles = ["admin"]

# ---------- 配置 ----------
[config]
static_schema = "schemas/config.schema.json"      # site.toml [plugin.bonus]
settings_schema = "schemas/settings.schema.json"  # 后台可改

# ---------- 数据库 ----------
[database]
migrations = "migrations"
views = []                         # 本插件对外提供的只读视图（须在 catalog/services.md §3 登记）

[[database.partitions]]            # 由内核维护分区（见 conventions/database.md §7）
table = "award_log"
by = "range_month"                 # "range_day" | "range_month" | "hash"
premake = 3
retention_months = 24

# ---------- Redis ----------
[redis]
quota_mb = 64
max_keys = 1000000

# ---------- WASM 资源 ----------
[wasm]
memory_mb = 64                     # ≤ site.toml [wasm].max_memory_mb
max_concurrency = 32
default_timeout_ms = 1000

# ---------- 术语 ----------
[terms]
"bonus.currency" = { zh-CN = "魔力值", en = "Bonus Points" }
"bonus.currency.unit" = { zh-CN = "点", en = "pts" }

# ---------- 前端 ----------
[frontend]
entry = "ui/index.js"
slots = ["user.profile.stats", "nav.user_menu"]
routes = ["/p/bonus/*"]
settings_ui = "schema"             # "schema" = 由 settings_schema 自动生成；"custom" = 插件自带

# ---------- MCP ----------
[[mcp.tools]]
name = "get_my_bonus"
description = "查询当前用户的魔力值余额和最近收支"
input_schema = "schemas/mcp/get_my_bonus.input.json"
output_schema = "schemas/mcp/get_my_bonus.output.json"
permission = "bonus.balance.read_own"
write = false
confirm = false                    # 写操作默认 true
audience = "user"                  # "user" | "admin"
```

### 4.2 字段表

| 字段 | 类型 | 必填 | 规则 |
|---|---|---|---|
| `id` | string | 是 | `^[a-z][a-z0-9-]{1,31}$`；不得以 `-` 结尾；保留字：`kernel`、`public`、`core`、`shared`、`system`、`pg-*`、`information-schema`。官方插件在 [catalog/plugins.md](catalog/plugins.md) 中登记；第三方插件选一个不冲突的 ID（`x-` 前缀留给测试插件） |
| `name` / `name_term` | string | 是（二选一） | |
| `version` | semver | 是 | |
| `kind` | enum | 是 | `wasm` / `native`；与实际加载方式不符则拒绝启动 |
| `api` | string | 是 | `pt:plugin@<major>` 或 `pt:plugin@<major>.<minor>`（如 `pt:plugin@1`）；兼容规则见 §6 |
| `kernel` | semver req | 否 | |
| `license` | SPDX | 是 | 官方插件必须为 `MIT OR Apache-2.0`（D19）；第三方插件可用其他许可证，但不得为 AGPL/GPL 等与内核链接后产生传染的许可证（Native 插件会被静态链接进内核） |
| `provides` | [string] | 否 | `ctx.<name>@<semver>`；服务必须在 [catalog/services.md](catalog/services.md) 中归属本插件 |
| `requires` / `optional_requires` | [string] | 否 | `ctx.<name>[@<semver-req>]`，缺省版本表示 `*` |
| `capabilities.*` | | 否 | 见 §4.3 |
| `publishes` | [string] | 否 | 必须是 events 目录中归属本插件的事件 |
| `[[subscriptions]]` | table[] | 否 | `name`、`event`、`versions`（默认 `[1]`）、`ordered`（默认 false）、`max_attempts`（默认 10）、`tx`（默认 auto）、`batch`（仅 Native，默认 1） |
| `[[hooks]]` | table[] | 否 | `hook`、`priority`（i32，默认 0）、`timeout_ms` |
| `[[hook_points]]` | table[] | 否 | `name`、`input_schema`；目录中登记的钩子点必须归属本插件（其参数以目录为准）。目录之外的钩子点名字须以 `<id>.` 开头，参数由清单给出：`modifiable`（可修改字段的点分路径，为空则只能放行 / 拒绝）、`timeout_ms`（默认 200，≤ 1000）、`deny_on_error`（默认 false） |
| `[[cron]]` | table[] | 否 | `name`（`^[a-z][a-z0-9_]{1,63}$`）、`schedule`（5 段 cron）、`timeout_secs`（默认 300）、`catch_up`（默认 none）、`tx`（默认 manual） |
| `[[jobs]]` | table[] | 否 | `name`、`timeout_secs` |
| `[http]`、`[[http.routes]]` | | 否 | 见 [conventions/api.md](conventions/api.md)；路由可选 `reauth_secs`（要求最近 N 秒内重新验证过身份，否则 403 `identity.reauth_required`） |
| `[[workers]]` | table[] | 否 | 仅 Native：`name`、`stream_prefix`（如 `acct:`）、`partitions_from`（分区数来源键，如 `acct:_meta`）；内核按租约把分区分配给各副本，调用插件实现的 `PartitionWorker` trait |
| `[notify.templates]` | map | 否 | 本插件发送的通知模板：`<template_key> = { channels = [...], term_args = [...] }`；键须以 `<plugin_id>.` 开头，由内核登记后交给 notify |
| `[[permissions]]` | table[] | 否 | 必须在 [catalog/permissions.md](catalog/permissions.md) 中登记 |
| `[config]` | | 否 | JSON Schema（draft 2020-12）路径 |
| `[database]` | | 否 | `migrations` 目录、对外 `views`、`[[database.partitions]]`（`table`、`by`、`interval`、`retention_days`、可选 `archive_hook`） |
| `[redis]` | | 否 | `quota_mb`（默认 32）、`max_keys`（默认 100000） |
| `[wasm]` | | 否 | 仅 `kind = "wasm"` |
| `[terms]` | map | 否 | 键必须以 `<plugin_id>.` 开头；值为 `{ <locale> = "..." }` 或 `{ <locale> = { name, short, unit, icon } }` |
| `[frontend]` | | 否 | `entry`、`slots`（须在 [catalog/slots.md](catalog/slots.md)）、`routes`（须以 `/p/<plugin_id>/` 或 `/admin/p/<plugin_id>/` 开头）、`settings_ui` |
| `[[mcp.tools]]` | table[] | 否 | 官方插件的工具名在 [catalog/mcp-tools.md](catalog/mcp-tools.md) 中登记；管理员工具以 `admin_` 开头 |
| `[error_codes]` | map | 否 | 本插件错误码 → HTTP 状态（4xx / 5xx），如 `"notes.too_many" = 422`；键须以 `<id>.` 开头。只对 [catalog/error-codes.md](catalog/error-codes.md) 未登记的码生效，都未声明时按后缀规则（conventions/api.md §7） |

清单的 JSON Schema 放在 `crates/plugin-api/schemas/plugin-manifest.schema.json`，内核用它校验 TOML 解析后的结果；上表与该 schema 不一致时以本文为准并修正 schema。

### 4.3 能力（capabilities）清单

| 能力 | 取值 | 默认 | Native 才可用 | 说明 |
|---|---|---|---|---|
| `kv` | bool | true | | |
| `sql` | bool | false | | 本插件 schema 全权 + 授权视图 SELECT |
| `redis` | bool | false | | 前缀 `p:<plugin_id>:` |
| `redis_global` | [{ role, prefix, mode }] | [] | 是 | 访问全局前缀，`mode` = `r` / `rw`；只能申请 [catalog/redis-keys.md](catalog/redis-keys.md) 中分配给本插件的前缀 |
| `blob` | bool | false | | |
| `http_outbound` | [host] | [] | | 主机名；或 `config:<静态配置键>`（启动时从插件配置解析，可加 `allow_private = true` 供本地 LLM 等内网服务）；或 `runtime:<设置键>`（管理员修改设置后生效，仅 webhook 类插件）。所有出站请求经内核 http-client 的 SSRF 防护：解析后检查地址，禁止私有 / 回环 / 链路本地地址及重定向到这些地址（`allow_private` 声明的除外） |
| `views` | [string] | [] | | `<provider_schema>.<view>` |
| `sensitive` | [string] | [] | | 取值：`user.email`、`user.ip`、`user.passkey`、`peer.ip`、`user.webauthn`、`user.totp`；全部须在 site.toml 授权，否则拒绝启动 |
| `sensitive_optional` | [string] | [] | | 未授权时插件照常启动，相关调用返回 `kernel.capability_denied` |
| `scopes` | map<service, [string]> | {} | | 由服务提供方定义含义，在 services 目录中给出可选值 |
| `search_contributor` | bool | false | | |

---

## 5. 插件目录结构

### 5.1 WASM 插件（运行时目录 `plugins/<id>/`）

```
plugins/bonus/
├─ plugin.toml                 # 清单（必需）
├─ plugin.wasm                 # 组件（必需，Component Model 二进制）
├─ plugin.sig                  # 签名（生产环境必需，见 kernel/03 §2.2）
├─ checksums.txt               # 包内所有文件的 sha256（签名对象）
├─ wit/                        # 本插件提供的服务 WIT 包 + 自己的 world
│  ├─ bonus.wit
│  └─ world.wit
├─ migrations/
│  ├─ 0001_init.sql
│  └─ 0002_add_award_log.sql
├─ schemas/
│  ├─ config.schema.json
│  ├─ settings.schema.json
│  ├─ events/bonus.hourly_settled.v1.json
│  └─ mcp/get_my_bonus.input.json …
├─ openapi.json                # HTTP 路由的 OpenAPI 片段
├─ ui/                         # 前端 ESM bundle（可选）
│  └─ index.js
└─ README.md
```

### 5.2 Native 插件（源码目录 `plugins/native/<id>/`）

```
plugins/native/identity/
├─ Cargo.toml                  # crate 名 pt-plugin-identity
├─ plugin.toml                 # 与 WASM 相同格式，kind = "native"
├─ wit/                        # 提供的服务 WIT
├─ migrations/
├─ schemas/
├─ ui/                         # 前端源码，构建产物进入 web/ 的插件目录
└─ src/lib.rs                  # pub fn plugin() -> NativePlugin
```

### 5.3 源码中的 WASM 插件（`plugins/wasm/<id>/`）

与 5.1 相同，另加 `Cargo.toml`（或其他语言工程文件）与 `src/`。`pt-server plugin package --src <源码目录> --wasm <组件> --out <目录>` 组装运行时目录（plugin.toml、plugin.wasm、migrations/、schemas/hooks/、checksums.txt）并检查；`pt-server plugin check <包目录>` 只做检查（清单、API 版本、组件链接、导出与声明一致、迁移文件名），都不需要数据库。包签名（plugin.sig）尚未实现，开发环境用 `plugins.allow_unsigned = true`。

---

## 6. 版本兼容策略

| 对象 | 版本规则 | 兼容判断 |
|---|---|---|
| `pt:plugin` WIT 包 | semver。**v1.0 之前**：次版本号可以不兼容（0.1 → 0.2 视为不兼容），修订号只做增量；**v1.0 之后**：只允许增量（新增接口、新增函数、新增 variant 分支须在新接口中） | 清单 `api = "pt:plugin@1"`（或 `@1.<次>`，表示用到了该次版本新增的接口）；内核接受主版本相同、次版本不高于自身的插件；1.0 之前的版本（`@0.x`）不再接受 |
| 业务服务 WIT（`pt:<svc>`） | semver，同上 | 消费方 `requires` 的版本要求必须被提供方 `provides` 的版本满足 |
| 事件载荷 | 整数 `version`；字段只增不删为同一版本；删除/改语义必须升版本，且发布方在过渡期（至少一个次版本）**双发**新旧版本 | 订阅方声明 `versions`，内核只投递匹配版本 |
| 钩子输入 | 与事件相同，钩子输入 schema 带 `version`，只增不删 | |
| 只读视图 | 视为提供方服务 API 的一部分：只增列；删列/改语义须新建 `v_xxx_v2` 并保留旧视图一个次版本 | |
| REST API | 路径带主版本 `/v1/`；见 conventions/api.md | |
| plugin.toml 格式 | 跟随 `api` 字段 | |

**冻结**：`pt:plugin` 已于 P7 定为 1.0.0（2026-10-14）。此后：

- 已有接口、记录、变体、函数签名不再改变；新增能力放进**新接口**，并把包的次版本加一（1.1.0），插件清单写 `api = "pt:plugin@1.1"` 表示依赖它。
- 世界（`host`、`kernel` 等）只增不减。
- 业务服务（JSON 分派）的方法、事件载荷、钩子输入按各自的版本规则演进（见上表），不受 WIT 冻结影响。
- Native 插件与内核一起编译，Rust API（`pt-plugin-api` 的 trait 与类型）随内核版本演进，不承诺二进制兼容；但同一主版本内不删除公开项。

修订记录：

| 版本 | 日期 | 说明 |
|---|---|---|
| 0.1 | 2026-10-04 | 初稿 |
| 1.0 | 2026-10-14 | 冻结为 `pt:plugin@1.0.0`。相对 0.1 的变更：`invocation-info` 新增 `request-id`、`client-ip`（仅获授 `user.ip` 的插件）、`user-agent`；`services` 的说明改为“JSON 分派为 1.0 的正式机制”。迁移：WIT 中 `pt:plugin/*@0.1.0` 改为 `@1.0.0`，清单 `api = "pt:plugin@0.1"` 改为 `"pt:plugin@1"`，重新构建 |
