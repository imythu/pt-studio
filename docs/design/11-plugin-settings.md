# 11 · 插件配置（统一配置模型）

> 状态：草案 v0.1 · 2026-10-14 · 决策 D21
>
> 破坏式更新：不保留旧的 `config.get-static` / `config.get-settings`、`settings_schema`、`settings_ui`、`site.toml [plugin.x]` 自由格式，所有插件一次性迁移。

## 0. 背景

现状有三套互不相干的配置机制：

| 层次 | 现在怎么做 | 问题 |
|---|---|---|
| 静态配置 | `site.toml [plugin.<id>]` 自由 JSON，插件 `get_static()` 自己解析 | 没有声明，后台看不到；拼错键名静默失效；`config.static_schema` 字段预留但没人用 |
| 运行时设置 | `plugin.toml [settings_schema]` 手写 JSON Schema + 插件里另写一份 `Settings` 结构体 | 两份定义会漂移（invite 读 19 个字段，schema 一个没声明）；内核保存时不校验；插件 `serde_json::from_str(..).unwrap_or_default()` 解析失败时**整份设置静默退回默认**；键序丢失要靠 `x-pt-order` 补；标题只有中文 |
| 用户偏好 | 各插件自建表 + 自建 HTTP 接口 + 自写设置页（notify `prefs`、messages `settings`） | 每个插件重复做存储、接口、界面、校验 |

目标：**一份声明、分层取值、内核全权负责**。插件作者只写声明，拿到的永远是合法、带默认值、强类型的值；宿主统一渲染后台与个人设置界面。

非目标：

- 插件的**业务数据**不归这里管（RSS 订阅、MCP token、等级定义、促销规则等有增删改查语义的数据仍由插件自己建表、出接口、做页面）。判断标准：值是否“每个作用域最多一份、整体覆盖”——是则为配置，否则为数据。
- 资源类型的字段 schema（`field_schema`，上传表单用）不在本次范围，仍走 `@pt-studio/schema-form`。

## 1. 决策 D21

1. 插件配置只有一种声明：`plugin.toml` 中的 `[[settings]]` 字段列表（加可选的命名类型 `[settings_types.*]`）。删除 `settings_schema`、`settings_ui`、`[config]`。
2. 每个字段的值按层覆盖：**清单默认值 → 部署层（site.toml / 环境变量）→ 站点层（后台）→ 用户层（个人设置）**。字段用 `editable` 声明最高可写层。
3. 内核负责解析、校验、合并、存储、缓存、审计、变更通知；插件只读取生效值。
4. 插件代码中的类型由 SDK 在编译期从 `plugin.toml` 生成，声明是唯一来源。
5. 前端由宿主统一渲染；取消整页自定义设置（`settings_ui = "custom"`），只保留**字段级自定义控件**。
6. 显示文案一律走术语表约定键，声明里不写显示文字。

## 2. 概念

- **字段（field）**：一个配置项，有键名、类型、默认值、可写层。
- **层（layer）**：`default`（清单）、`deploy`（site.toml / 环境变量）、`site`（后台）、`user`（个人设置）。
- **可写层（editable）**：字段能被哪些层设置。`editable = "admin"` 表示 deploy 与 site 层都能设，site 层优先；`editable = "user"` 表示 deploy、site、user 都能设，user 层优先；`editable = "deploy"` 只能在部署层设。
- **锁（lock）**：上层禁止下层覆盖。部署层可以锁定站点 / 用户字段；站点层（管理员）可以锁定用户字段。锁定后下层存量值被忽略（保留不删，解锁后恢复生效）。
- **生效值（effective value）**：按层合并后的值。插件只看到生效值。
  - 站点生效值：`default ← deploy ← site`
  - 用户生效值：`站点生效值 ← user`（只对 `editable = "user"` 且未锁定的字段）

## 3. 声明语法

```toml
[[settings]]
key = "enabled"
type = "bool"
default = true
editable = "admin"

[[settings]]
key = "default_expiry_days"
type = "int"
range = [1, "@max_expiry_days"]      # 可引用其他字段
default = 7
editable = "admin"
group = "codes"

[[settings]]
key = "secret"
type = "secret"
editable = "deploy"
required = true                     # 部署层必须给出，否则拒绝启动

[[settings]]
key = "class_rules"
type = "map"
key_type = "ref:class"
wildcard = true                     # 允许 "*" 作为缺省键
value = "class_rule"                # 命名类型
default = { "*" = { max_outstanding = 1 } }
editable = "admin"
group = "quota"

[[settings]]
key = "accept"
type = "enum"
options = ["all", "staff_only"]
default = "all"
editable = "user"

[[settings_groups]]                 # 可选；未声明的组按出现顺序排在最后
id = "codes"
[[settings_groups]]
id = "quota"

[settings_types.class_rule]         # 命名类型 = 一组字段（同样用数组保序）
[[settings_types.class_rule.fields]]
key = "max_outstanding"
type = "int"
range = [0, 100]
default = 1
[[settings_types.class_rule.fields]]
key = "require_email_binding"
type = "bool"
default = false
```

字段属性：

| 属性 | 必填 | 说明 |
|---|---|---|
| `key` | 是 | `^[a-z][a-z0-9_]{0,47}$`，插件内唯一 |
| `type` | 是 | 见 §4 |
| `default` | 否 | 缺省时：`optional = true` 则为空，否则按类型零值（bool=false、int=0、list=[]、map={}、string=""）；`secret`、`ref:*` 无零值，必须 `optional` 或 `required` |
| `editable` | 否 | `deploy` / `admin` / `user`，默认 `admin` |
| `optional` | 否 | 允许“未设置”（插件侧为 `Option<T>`）；界面显示启用开关或留空 |
| `required` | 否 | 生效值不能为空；对 `deploy` 字段即“必须在 site.toml 中配置”，否则拒绝启动 |
| `group` | 否 | 分组 id |
| `visible_if` | 否 | 简单条件：`"enabled"`、`"!enabled"`、`"mode == 'x'"`；只影响显示，不影响校验与取值 |
| `perm` | 否 | 修改该字段额外需要的权限（本插件声明的权限 id），如 `restrict_inviter` 相关字段 |
| `widget` | 否 | 字段级自定义控件 id（§9.3） |
| `renamed_from` | 否 | 旧键名列表；读到旧键时自动迁移并写审计（插件版本间演进用） |
| 类型参数 | — | `range`、`len`、`pattern`、`options`、`item`、`key_type`、`value`、`wildcard`、`unit` 等，见 §4 |

字段顺序 = 声明顺序；命名类型的字段顺序 = 其 `fields` 顺序。不再需要 `x-pt-order`。

## 4. 类型清单

类型清单就是界面能渲染、内核能校验的全部。不允许任意 JSON Schema。

### 4.1 基础类型

| 类型 | 参数 | 存储 JSON | 插件类型（Rust） | 控件 |
|---|---|---|---|---|
| `bool` | — | `true` | `bool` | 开关 |
| `int` | `range = [min, max]`（端点可为 `"@field"` 或省略） | `5` | `i64` | 数字框，显示范围 |
| `number` | `range`、`step` | `0.5` | `f64` | 数字框 |
| `ratio` | — | `0.3`（0..1） | `f64` | 百分比输入 |
| `string` | `len = [min, max]`、`pattern` | `"x"` | `String` | 单行文本 |
| `text` | `len`、`format = "plain" / "markdown"` | `"..."` | `String` | 多行文本 |
| `enum` | `options = [...]` | `"monthly"` | 生成的 Rust enum | 下拉 / 单选（≤ 4 项用单选） |
| `flags` | `options = [...]` | `["a","b"]` | `Vec<生成的 enum>` | 多选 |
| `duration` | `range`（秒）、`unit = "s" / "min" / "h" / "d"`（显示单位） | `86400`（秒） | `std::time::Duration` | 数字 + 单位 |
| `bytes` | `range` | `1073741824` | `u64` | 数字 + 单位（KiB…TiB） |
| `secret` | `len` | 不回显 | `Secret<String>`（不实现 Debug 明文） | 后台只显示“已设置 / 未设置” |
| `url` | `schemes = ["https"]` | `"https://…"` | `String` | 单行文本 |
| `cron` | — | `"0 3 * * *"` | `String` | 单行文本 + 下次执行时间预览 |

`secret` 只能 `editable = "deploy"`（内核暂无密钥加密存储，见 §13）。

### 4.2 结构类型

| 类型 | 参数 | 存储 JSON | 插件类型 | 控件 |
|---|---|---|---|---|
| `list` | `item = "<类型>"`、`len = [min, max]`、`unique_by = "id"` | `[...]` | `Vec<T>` | 字符串 / ref 项为标签输入；对象项为卡片列表（增删、排序） |
| `map` | `key_type = "string" / "enum" / "ref:<kind>"`、`value = "<类型>"`、`wildcard` | `{...}` | `BTreeMap<K, T>`；`wildcard` 时生成 `get_or_wildcard(k)` | 键值列表；`ref` 键为下拉 |
| 命名类型 | `type = "<名字>"` | `{...}` | 生成的 struct | 分组卡片 |
| 判别联合 | 命名类型声明 `tag = "type"` 与 `[[settings_types.X.variants]]` | `{"type": "deduct_quota", "amount": 1}` | 生成的 Rust enum（`#[serde(tag)]`） | 先选种类，再显示该种类字段 |

`item` / `value` 可以是基础类型名、`ref:<kind>` 或命名类型名。嵌套深度上限 4。

### 4.3 引用类型 `ref:<kind>`

值是另一个插件管理的对象的 id（等级、货币、用户、资源类型、通知类别…），界面显示为可搜索的下拉框而不是让管理员手填 id。

- 存储：id 原样（`3`、`"BONUS"`、`"movie"`）。插件类型：`i64` 或 `String`，由提供方声明。
- 提供方：在清单 `provides` 中声明 `settings.ref.<kind>`，并实现选项接口（§7.3）。如 levels 提供 `settings.ref.class`，ledger 提供 `settings.ref.currency`，torrent 提供 `settings.ref.content_type`，identity 提供 `settings.ref.user`，notify 提供 `settings.ref.notify_category`。
- 使用方必须在 `requires` / `optional_requires` 中声明对应能力；提供方缺失时字段仍可保存原始 id，界面退化为输入框并提示。
- 校验：保存时调用提供方 `validate(kind, ids)`，不存在的 id 报 `settings.ref_not_found`。之后对象被删除不回溯修改配置；插件读取时自行容错（文档约定）。

## 5. 取值与合并

### 5.1 部署层

- `site.toml` 中 `[plugin.<id>]` 只允许出现已声明字段，未知键**拒绝启动**并提示最接近的键名。
- 每个字段也可由环境变量给出：`PT_<PLUGIN>__<KEY>`（插件 id 与键名转大写，`-` 换成 `_`），优先级高于 site.toml。值按 TOML 语法解析（`PT_INVITE__DAILY_LIMIT=8`、`PT_SEARCH_MEILISEARCH__INSTANCES='[{...}]'`）。
- 锁：`[plugin.<id>] lock = ["key", ...]`，锁定站点层与用户层；部署层设置了 `editable = "deploy"` 的字段天然不可下层修改，无需写 lock。
- 部署层在启动时完整校验，失败拒绝启动（与插件加载其他校验一致，见 02 号文档 §1）。

### 5.2 站点层与用户层

- 站点层：后台修改，存 `kernel.settings_site`，只存**显式设置过的字段**（不把默认值写进库），“恢复默认” = 删除该键。
- 用户层：个人设置修改，存 `kernel.settings_user`，只存覆盖值。
- 管理员锁定用户字段：站点层 `locks = ["accept"]`。锁定后用户界面显示为只读并说明“由站点统一设置”。

### 5.3 合并规则

```
site_effective[k]  = first_set(site[k] if !locked_by_deploy(k), deploy[k], default[k])
user_effective[k]  = user[k]  if editable(k) == user && !locked(k) && user has k
                     else site_effective[k]
```

对象类型整体覆盖，不做深合并（避免“部分字段来自上层”难以理解）。`map` 同样整体覆盖。

## 6. 校验

- **保存时**：内核按声明校验 PATCH 后的完整值。错误格式沿用 API 约定：`422 kernel.validation`，`details.errors[] = {path, code, message}`，`path` 为 JSON Pointer（`/penalty_rules/0/levels/1/depth`）。错误码：`settings.type`、`settings.range`、`settings.len`、`settings.pattern`、`settings.option`、`settings.required`、`settings.unknown_field`、`settings.ref_not_found`、`settings.duplicate`（`unique_by`）、`settings.locked`、`settings.forbidden`（缺 `perm`）。
- **跨字段引用**：`range = [1, "@max_expiry_days"]` 按同一次保存后的生效值计算；被引用字段改小导致其他字段越界时，同样报错在越界字段上。
- **启动时**：逐字段检查站点层、用户层存量值（插件升级可能改变声明）：
  - 不合法的**单个字段**丢弃（回到上层值），写审计 `kernel.settings_dropped`，后台设置页顶部提示；不影响其他字段。
  - `renamed_from` 命中的键自动改名。
  - 用户层量大，启动时不扫描，改为读取时惰性校验 + 后台任务批量清理。
- **插件侧**：内核保证交给插件的值合法，SDK 反序列化失败视为内核缺陷，返回 `kernel.internal` 并记录错误日志——不再有“解析失败退回默认”的路径。

## 7. 插件接口

### 7.1 WIT

删除 `interface config`，新增：

```wit
interface settings {
  use types.{json, user-id, plugin-error};

  /// 站点生效值（已校验、已补默认值，JSON 对象）。内核缓存，变更后至多滞后 1 秒。
  get: func() -> json;

  /// 叠加该用户覆盖后的生效值。
  get-for-user: func(user: user-id) -> result<json, plugin-error>;

  /// 批量读取多个用户的生效值（通知扇出等场景），顺序与输入一致。
  get-for-users: func(users: list<user-id>) -> result<list<json>, plugin-error>;
}

/// 引用类型选项提供方（provides settings.ref.<kind> 的插件导出）
interface settings-ref {
  use types.{plugin-error};

  record ref-option {
    id: string,
    /// 术语键或直接文字（二选一，术语键优先）
    label-term: option<string>,
    label: option<string>,
    hint: option<string>,
    disabled: bool,
  }

  options: func(kind: string, query: option<string>, limit: u32) -> result<list<ref-option>, plugin-error>;
  validate: func(kind: string, ids: list<string>) -> result<list<string>, plugin-error>; // 返回不存在的 id
}
```

`plugin-api` 的 Native trait 对应改为 `trait Settings { fn get(&self) -> Json; async fn get_for_user(..); async fn get_for_users(..); }` 与 `trait SettingsRef`。

### 7.2 Rust SDK：编译期生成类型

新增 crate `crates/settings`（解析、校验、合并、编译为前端描述符，内核与宏共用）和 `crates/settings-macros`（过程宏）。插件中：

```rust
pt_plugin_sdk::settings!();   // 读取 $CARGO_MANIFEST_DIR/plugin.toml

// 生成：
// pub struct Settings { pub enabled: bool, pub default_expiry_days: i64,
//                       pub class_rules: Wildcard<BTreeMap<i64, ClassRule>>, pub secret: Secret<String>, ... }
// pub struct UserSettings { pub accept: Accept }        // 只含 editable = "user" 的字段
// pub enum Accept { All, StaffOnly }
// impl Settings { pub fn get() -> Self }
// impl UserSettings { pub fn get(user: i64) -> PResult<Self>; pub fn get_many(users: &[i64]) -> PResult<Vec<Self>> }

let s = Settings::get();
if s.enabled && s.class_rules.get_or_wildcard(&class).max_outstanding > n { ... }
```

Native 插件用同一个宏（`pt_plugin_api::settings!()`），生成 `Settings::get(&ctx)` 形式。声明有误（未知类型、默认值越界、`@` 引用不存在）在**编译期**报错。其他语言的插件读 JSON，按 §4 的存储格式自行解析；`pt-server plugin check` 可导出描述符 JSON 供代码生成。

### 7.3 变更通知

内核在站点层变更、部署层锁变化（重启后）时向**所属插件**投递事件 `kernel.settings_changed { scope: "site", keys: [...], version }`；用户层变更投递 `{ scope: "user", user_id, keys }`（仅当插件订阅了用户层变更，避免噪声）。插件在清单中订阅：

```toml
[[subscriptions]]
event = "kernel.settings_changed"
scopes = ["site"]           # 可选 "user"
```

替代 search-meilisearch 的 `settings_apply` 任务等自建机制。

## 8. 内核实现

### 8.1 存储

```sql
CREATE TABLE kernel.settings_site (
    plugin_id  text PRIMARY KEY,
    values     jsonb  NOT NULL DEFAULT '{}',   -- 只含显式设置的字段
    locks      text[] NOT NULL DEFAULT '{}',   -- 管理员锁定的用户字段
    version    bigint NOT NULL DEFAULT 0,
    updated_by bigint,
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE kernel.settings_user (
    plugin_id  text   NOT NULL,
    user_id    bigint NOT NULL,
    values     jsonb  NOT NULL,                -- 只含覆盖值；为空时删除行
    version    bigint NOT NULL DEFAULT 0,
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (plugin_id, user_id)
);
```

删除 `kernel.plugin_settings`。用户被删除时由 identity 的 `user.deleted` 事件清理 `settings_user`。

### 8.2 缓存

- 站点生效值：每节点内存缓存，按插件整份；变更后经 Redis pub/sub（`kernel:settings`）通知所有节点失效重算。
- 用户生效值：每节点 LRU（默认 50k 条、TTL 60 秒）+ 同一 pub/sub 失效；`get-for-users` 一次 SQL 批量补齐缺失项。

### 8.3 HTTP API

| 方法 | 路径 | 权限 | 说明 |
|---|---|---|---|
| GET | `/api/kernel/v1/admin/settings` | `kernel.settings.manage` | 有可显示字段的插件列表（id、名称、字段数、被丢弃字段数、是否有部署层锁） |
| GET | `/api/kernel/v1/admin/settings/{plugin}` | 同上 | `{ descriptor, layers: { default, deploy, site }, effective, locks: { deploy, site }, dropped, version }`；`secret` 只给 `{ set: true }` |
| PATCH | `/api/kernel/v1/admin/settings/{plugin}` | 同上 + 字段 `perm` | `{ version, set: { key: value }, unset: [key], lock: [key], unlock: [key] }`；版本不符 `409 kernel.conflict` |
| GET | `/api/kernel/v1/me/settings` | 登录 | 有用户字段的插件列表（用于生成个人设置标签页） |
| GET | `/api/kernel/v1/me/settings/{plugin}` | 登录 | `{ descriptor（仅用户字段）, site_effective, user, locked, version }` |
| PATCH | `/api/kernel/v1/me/settings/{plugin}` | 登录 | `{ version, set, unset }` |
| GET | `/api/kernel/v1/settings/ref/{kind}?q=&limit=` | 登录（后台用时需 manage） | 转发给提供方 `options` |

`descriptor` 是 `crates/settings` 编译出的前端描述符：字段列表（含类型参数、默认值、分组、`visible_if`、`widget`、术语键），前端不需要理解 TOML 或 JSON Schema。

### 8.4 审计

每次站点层 / 用户层修改写 `kernel.audit_log`：`action = kernel.settings_update`，`details = { scope, user_id?, changes: [{ key, before, after }] }`，`secret` 字段值替换为 `"***"`。用户层修改默认不审计（量大），声明 `audit = true` 的用户字段除外。

## 9. 前端

### 9.1 新包 `@pt-studio/settings-form`

输入描述符与各层值，输出表单。复用现有 `schema-form` / `ui-kit` 的控件实现（开关、标签输入、列表 / 映射编辑器、数字框、未保存标记、恢复默认），但不再经过 JSON Schema。新增：

- 每个字段显示**来源**：“默认值” / “site.toml” / “后台设置” / “已锁定（site.toml）”；部署层字段只读。
- “恢复默认” = `unset`，恢复到上层值（不一定是清单默认值，提示中写明上层值）。
- `visible_if` 实时生效；跨字段 `@` 引用的范围实时更新。
- `ref:*` 控件：可搜索下拉，调用 `/settings/ref/{kind}`。
- 保存只发送改动的键（`set` / `unset`），服务端错误按 `path` 落到字段。
- JSON 视图保留，作为只读导出 + 高级编辑。

### 9.2 页面

- 后台 `/admin/settings`：插件卡片列表（沿用本次改版的样式）；`/admin/settings/{plugin}`：分组表单 + 底部保存栏。用户字段在后台显示为“站点默认值”，旁边有“锁定”开关。
- 个人设置页：宿主按 `/me/settings` 自动为每个插件生成一个标签页（标题 = 插件名术语），排在插件通过 `user.settings.tabs` 插槽注册的标签页之后。
- 插件自有后台页（`/admin/p/<id>/*`）不受影响，用于业务数据管理。

### 9.3 字段级自定义控件

插件前端在 `PluginFrontend` 中注册：

```ts
settingsWidgets: {
  'invite.penalty-rules': PenaltyRulesEditor,   // props: { value, onChange, field, errors, api }
}
```

字段声明 `widget = "invite.penalty-rules"` 时使用该控件；保存、校验、审计、来源显示仍由宿主负责。插件前端未加载时退回默认控件。取消 `usePluginSettings` 与 `settings_ui = "custom"`。

## 10. 术语约定

声明中不写显示文字，宿主按约定键取术语，缺失时显示键名（开发期一眼可见漏写）：

| 用途 | 术语键 |
|---|---|
| 字段名 | `<plugin>.settings.<key>` |
| 字段说明 | `<plugin>.settings.<key>.hint` |
| 枚举项 | `<plugin>.settings.<key>.<option>` |
| 命名类型内字段 | `<plugin>.settings.<type>.<field>`（及 `.hint`、`.<option>`） |
| 分组标题 | `<plugin>.settings.group.<id>` |
| 判别联合种类 | `<plugin>.settings.<type>.<variant>` |

`pt-server plugin check` 检查每个字段的字段名术语在 `zh-CN` 与 `en` 下都存在，缺失报错（CI 中执行）。

## 11. 删除清单

- WIT `interface config`、`plugin-api` 的 `Config` trait、SDK `config::get_static` / `get_settings` / `ctx::settings()`。
- 清单 `settings_schema`、`[config]`（`ConfigDecl`）、`[frontend] settings_ui`。
- `kernel.plugin_settings` 表与 `/api/kernel/v1/admin/plugins/{id}/settings` 接口。
- 前端 `PluginSettings`、`CustomSettings`、`usePluginSettings`、`x-pt-order` 等仅用于设置的 schema 扩展。
- `site.toml [plugin.x]` 中未声明的键（启动报错）。
- notify 的 `prefs` 表与 `/v1/preferences`、messages 的 `settings` 表与 `/v1/settings`，以及两者前端的偏好页。

## 12. 迁移清单

| 插件 | 部署层字段（原 site.toml） | 站点层字段（原 settings_schema / 代码默认值） | 用户层字段 | 备注 |
|---|---|---|---|---|
| identity | `secret_key`、`insecure_cookies` 等 | — | — | 提供 `settings.ref.user` |
| invite | `secret`、`register_path` | 19 个（`class_rules` → `map<ref:class>`，`class_grants` → `list`，`penalty_rules` → 判别联合动作） | — | `penalty_rules` 可选自定义控件（含“模拟”） |
| torrent | `announce_base_urls`、`types` | — | — | 提供 `settings.ref.content_type` |
| search-meilisearch | `instances`（含 `api_key` → 拆出 secret） | 待盘点 | — | `settings_apply` 任务改为订阅 `kernel.settings_changed` |
| llm-claude | `api_key` 等 | 待盘点 | — | |
| metadata-tmdb | `api_key` | — | — | |
| notify | `email_provider`、`email_token`、`email_from`、`site_url`、`unsubscribe_secret`、`email_per_hour` | — | `channels: map<ref:notify_category, {center, email}>` | 提供 `settings.ref.notify_category`；强制类别由提供方 `disabled` + 站点层锁表达 |
| messages | — | 3 个 | `accept: enum` | 删除自建 `settings` 表与接口 |
| mcp | `issuer`、`session_key` | `disabled_confirm_tools` | — | |
| levels、ledger、promotion、accounting | 按代码盘点 | — | — | levels 提供 `settings.ref.class`，ledger 提供 `settings.ref.currency` |
| 其余带 `settings_schema` 的插件（ai-assistant、recommend、rss、tracker-bridge、ai-moderation、ai-upload-review、checkin、comments、hnr、moderation、reports、requests、type-*） | — | 按现有 schema 逐项转换 | — | moderation `initial_rules` 定义为命名类型或自定义控件 |

迁移工具：`tools/migrate-settings.py`（一次性，迁移完删除）把现有 `settings_schema` 转成 `[[settings]]` 草稿并生成术语键占位；`kernel.plugin_settings` 存量数据由内核迁移 SQL 搬到 `settings_site`（去掉与默认值相同的键）。部署层字段与类型需人工核对代码中的 `get_static()` 读取点。

## 13. 待决问题

1. **可在后台修改的密钥**：如 LLM API key 是否允许管理员在后台改？需要内核级密钥加密（KEK 来自部署层），本期不做，`secret` 仅限 deploy。
2. **用户层字段的管理员批量操作**：如“把所有用户的某项重置为站点默认值”，是否需要？倾向提供 `DELETE /admin/settings/{plugin}/users?key=` 并写审计。
3. **按用户组 / 等级的站点默认值**（例如 VIP 默认开启某项）：会引入第五层，本期不做；需要时由插件把它声明为 `map<ref:class, T>` 字段。
4. **字段级“需要重启”**：目前所有配置热生效；若出现必须重启的部署层字段，增加 `restart = true` 仅作提示。

## 14. 实施顺序

1. `crates/settings`：声明解析、类型检查、默认值、校验、合并、描述符编译；单元测试覆盖 §4 全部类型、§5 合并、§6 错误路径。
2. `crates/settings-macros` + SDK / plugin-api 接口替换（WIT `settings`、`settings-ref`）。
3. 内核：表与迁移、部署层加载与启动校验、缓存与 pub/sub、HTTP API、审计、`kernel.settings_changed`。
4. 前端：`@pt-studio/settings-form`、后台设置页、个人设置自动标签页、字段级控件注册。
5. 引用类型提供方：levels、ledger、torrent、identity、notify。
6. 逐个迁移插件（先 invite、notify、messages 验证三层，再其余），删除旧机制。
7. 文档：更新 `platform/plugin-api.md`（清单、WIT）、`catalog`、02 / 06 号设计文档、`site.example.toml`；e2e 覆盖后台修改、用户覆盖、锁定、启动时丢弃非法字段。
