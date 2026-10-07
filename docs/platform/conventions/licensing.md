# 许可证规范

> 版本 0.1 · 2026-10-04 · 本文不构成法律意见；正式发布前由法务确认（design/10 R2）

## 1. 已确定的许可证

| 范围 | 许可证 | 决策 |
|---|---|---|
| 内核（`crates/kernel`、`crates/server`、`crates/plugin-api*`、`crates/plugin-sdk-rust`、`crates/testkit`、`xtask`） | **`MIT OR Apache-2.0`**（双许可证，使用者任选其一） | D19 |
| 全部官方插件（`plugins/native/*`、`plugins/wasm/*`） | **`MIT OR Apache-2.0`** | D19 |
| 前端（`web/*`）、预设、部署模板、文档 | **`MIT OR Apache-2.0`** | D19 |
| `crates/pt-tracker` | **`AGPL-3.0-only`** | D16（torrust 各包均为 AGPL-3.0-only） |

- 每个 crate 的 `Cargo.toml` 写 `license = "MIT OR Apache-2.0"`（pt-tracker 写 `license = "AGPL-3.0-only"`）；每个插件的 `plugin.toml` 写 `license = "MIT OR Apache-2.0"`；`web/` 各包的 `package.json` 写 `"license": "(MIT OR Apache-2.0)"`。
- 仓库根目录放 `LICENSE-MIT` 与 `LICENSE-APACHE`；`crates/pt-tracker/` 放自己的 `LICENSE`（AGPL-3.0 全文）与 `NOTICE`。
- 源文件头部使用 SPDX 标识：`// SPDX-License-Identifier: MIT OR Apache-2.0`（pt-tracker 中为 `AGPL-3.0-only`）。
- 贡献：采用 Apache-2.0 第 5 条的“入站 = 出站”约定，在 `CONTRIBUTING.md` 中说明贡献按 `MIT OR Apache-2.0` 授权；不要求 CLA。

## 2. pt-tracker 隔离边界（D16）

pt-tracker 是 AGPL-3.0-only 程序。为了让内核与插件保持宽松许可证，**必须**遵守以下边界：

| 规则 | 说明 |
|---|---|
| 不链接 | 内核、插件、SDK、前端**不得**以任何形式依赖或链接 pt-tracker 的代码或任何 torrust 包（Cargo 依赖、git 子模块、代码复制、`include!`、共享 crate 都不行） |
| pt-tracker 也不链接我们 | pt-tracker 不依赖 `crates/plugin-api`、`crates/kernel` 等任何主 workspace crate；它需要的数据结构（流量事件字段、同步键格式）**各自独立实现**，契约以文档为准（design/10 §5.4、§5.5、[../catalog/redis-keys.md](../catalog/redis-keys.md)） |
| 只通过 Redis 通信 | 进程间交互只有 Redis 数据结构：`trk:*`（tracker-bridge → pt-tracker）、`acct:*`、`u:*:live`、`ps:*`（pt-tracker 写）。不使用共享内存、FFI、RPC 库共享代码 |
| 独立 workspace | `crates/pt-tracker` 是独立 Cargo workspace（主 `Cargo.toml` 中 `exclude`），独立构建、独立发布镜像 |
| 源码提供义务 | pt-tracker 对网络用户提供服务，须按 AGPL 第 13 条提供对应源码：在 tracker 的 HTTP 响应头或 `/source` 端点给出源码地址，站点修改 pt-tracker 后必须公开修改 |
| 共享的格式定义 | 字段表、键名等**文档化的数据格式**不是代码，两边各自实现，不构成衍生作品；不要为了“复用”而抽出共享 crate |

若未来要求 tracker 也使用宽松许可证，切换到方案 D（自研 swarm，只复用 Apache/MIT 许可的基础包，design/03 §2）。

## 3. 不得复制其他 PT 项目的 copyleft 代码

可以借鉴**思路、数据模型、产品设计和公开协议**（BEP 等），**不得**把以下项目的代码（包括片段、SQL、正则、模板、前端组件、翻译文本）复制、改写后放入内核或插件：

| 项目 | 许可证 | 说明 |
|---|---|---|
| Arcadia | AGPL-3.0 | design/08 已提示 |
| UNIT3D、UNIT3D-Announce | AGPL-3.0 | |
| NexusPHP | GPL-2.0 / GPL-3.0（以各版本声明为准） | |
| Gazelle / Luminance 等 | 以各项目声明为准，多数为 copyleft 或无明确许可 | 无许可证 = 保留所有权利，同样不得复制 |
| torrust 各包 | AGPL-3.0-only | 只允许在 pt-tracker 中依赖 |

- 调研文档（各插件 01 文档 §2.3）可以描述其他项目的做法，但引用代码不得超过说明所需的最小片段，且注明来源；实现时必须自行编写。
- 插件文档和代码评审中如发现疑似复制，按阻断级问题处理。
- AI 生成代码同样适用：提示词中不得粘贴上述项目的代码要求“改写”。

## 4. 依赖许可证检查（cargo-deny）

主 workspace 的 `deny.toml`：

```toml
[licenses]
version = 2
allow = [
  "MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception",
  "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "0BSD",
  "Unicode-3.0", "Unicode-DFS-2016", "CC0-1.0", "BSL-1.0",
  "MPL-2.0",          # 弱 copyleft（文件级）：允许作为未修改的依赖使用，修改其文件须公开该文件
  "CDLA-Permissive-2.0",
]
confidence-threshold = 0.9
# 未列出的一律拒绝，特别是：AGPL-*、GPL-*、LGPL-*（静态链接下义务复杂，需个案评审）、SSPL、BUSL、Commons Clause

[bans]
deny = [
  # 禁止主 workspace 依赖 pt-tracker 或任何 torrust 包
  { name = "pt-tracker" },
  { name = "torrust-tracker-swarm-coordination-registry" },
  { name = "torrust-tracker-http-protocol" },
  { name = "torrust-tracker-primitives" },
  { name = "torrust-tracker-events" },
]
wildcards = "deny"

[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

- pt-tracker 自己的 workspace 使用单独的 `deny.toml`，允许 AGPL-3.0-only，并按 torrust 的 git rev 白名单放行。
- 前端依赖：CI 用 `license-checker`（或 `pnpm licenses list`）按同一白名单检查。
- WASM 插件的依赖同样受 cargo-deny 检查；第三方语言（JS/Python）插件需在包内提供依赖许可证清单（`THIRD_PARTY_LICENSES`）。
- LGPL、MPL 以外的弱 copyleft、以及任何白名单外的许可证，需要在评审中个案批准并记录在 `docs/reviews/`。

## 5. 第三方插件

- 第三方插件可以使用自己的许可证。注意：
  - **WASM 插件**作为独立组件在运行时加载，通过 WIT 接口交互，许可证由作者自行决定；
  - **Native 插件**会被静态编译进 `pt-server` 二进制，分发该二进制时须满足所有 Native 插件许可证的要求，因此 Native 插件不得使用 GPL/AGPL 等与 `MIT OR Apache-2.0` 组合后会改变整体分发义务的许可证；
- `plugin.toml` 的 `license` 字段必须是合法 SPDX 表达式；后台插件页面展示每个插件的许可证。

## 6. 第三方数据与服务

- 元数据源（TMDB、豆瓣、IMDb、MusicBrainz、Bangumi、IGDB）的数据使用受各自服务条款约束（署名要求、缓存期限、商业限制）；各 `metadata-*` 插件的需求文档必须列出其条款要点与合规措施（如 TMDB 署名、MusicBrainz 的 CC0 / CC BY-NC-SA 数据分区）。
- 预置的 MediaInfo 等解析逻辑须自行实现或使用许可证兼容的库（MediaInfoLib 为 BSD-2-Clause，可用）。
