# 10 · Torrust 集成技术方案（pt-tracker）

> 状态：spike 结论 v1.0 · 2026-10-04 · 对应决策 D9、D14～D17
> 调研对象：`torrust/torrust-tracker` develop 分支，提交 `135ec2ab`（2026-10-04）
> 原型：`.spike/pt-tracker-proto/`（独立 cargo 项目，可编译、可用 curl 跑通）

## 1. 目的与范围

### 1.1 目的

D9 规定 tracker 使用 Torrust，但集成方式要实测后再定。本文用读代码和跑原型的结果回答以下问题，作为 [03-tracker-and-accounting.md](03-tracker-and-accounting.md) 以及后续 tracker、记账相关设计的基础：

1. Torrust 现在能复用什么、怎么引用、许可证有什么影响；
2. PT 必需的能力该加在哪里：passkey 鉴权、用户状态、种子白名单、客户端白名单、按用户的流量增量；
3. 推荐哪种集成方案；
4. `pt-tracker` 的详细设计。

### 1.2 范围

| 包含 | 不包含 |
|---|---|
| HTTP(S) announce 主链路、增量计算、流量事件格式、与 tracker-bridge 的同步接口、分片与扩展 | 记账 worker 的窗口聚合与落库（见 03 号文档 §5）；反作弊判定规则；促销倍率 |
| Torrust 代码调研与许可证分析 | 正式压测（本次只做功能验证，见 §6.4） |

### 1.3 术语

| 术语 | 含义 |
|---|---|
| 累计值 | 客户端在 announce 中上报的 `uploaded` / `downloaded`，表示本次会话从开始到现在的总量 |
| 增量（Δ） | 两次 announce 之间累计值的差，加上做种 / 下载时长 |
| peer 会话 | 唯一键为 (user_id, info_hash, peer_id) 的一条记录 |
| swarm | 一个种子的 peer 集合 |

---

## 2. Torrust 现状调研

### 2.1 版本与发布状态

| 项 | 实际情况 |
|---|---|
| 最后一个正式版本 | **v3.0.0（2024-10-02）**，`main` 分支停在这个版本 |
| develop 分支 | 根包版本 `3.0.0-develop`，比 main 多 **4333 个提交**。拆分出来的包都只存在于 develop 上 |
| 改动频率（近 6 个月） | 全仓库 3340 个提交；`tracker-core` 107 个、`swarm-coordination-registry` 52 个、`primitives` 49 个、`http-protocol` 48 个、`http-core` 35 个、`events` 9 个 |
| 开发方式 | 仓库有大量 `AGENTS.md`、ADR、issue 规格文档，开发节奏很快；API 没有稳定承诺 |

**拆分出来的包都没有发布到 crates.io**。下表数据查自 crates.io API（2026-10-04）：

| 包（develop 中的版本） | 路径 | crates.io | 许可证 |
|---|---|---|---|
| `torrust-tracker-core` 0.1.0 | packages/tracker-core | **未发布** | AGPL-3.0-only |
| `torrust-tracker-swarm-coordination-registry` 0.1.0 | packages/swarm-coordination-registry | **未发布** | AGPL-3.0-only |
| `torrust-tracker-http-protocol` 0.1.0 | packages/http-protocol | **未发布** | AGPL-3.0-only |
| `torrust-tracker-http-core` 0.1.0 | packages/http-core | **未发布** | AGPL-3.0-only |
| `torrust-tracker-udp-protocol` / `udp-core` 0.1.0 | packages/udp-* | **未发布** | AGPL-3.0-only（udp-protocol 由 aquatic 的 Apache-2.0 代码派生） |
| `torrust-tracker-events` 0.1.0 | packages/events | **未发布** | AGPL-3.0-only |
| `torrust-tracker-axum-http-server` 0.1.0 | packages/axum-http-server | **未发布** | AGPL-3.0-only |
| `torrust-tracker-primitives` 3.0.0 | packages/primitives | 有 3.0.0，但那是 **2024 年的旧版本**（依赖 `aquatic_udp_protocol`、`tdyne-peer-id`），和 develop 中的同名同版本号包**内容不同，不能混用** | AGPL-3.0-only |
| `torrust-tracker-configuration` 3.0.0 | packages/configuration | 同上，是旧版本 | AGPL-3.0-only |
| `torrust-info-hash` 0.2.0 | 独立仓库 torrust-bittorrent | 已发布（2026-06） | **Apache-2.0** |
| `torrust-peer-id` 0.1.0 | 独立仓库 torrust-bittorrent | 已发布（2026-06） | **Apache-2.0** |
| `torrust-bencode` 3.0.0 | — | 已发布（2026-06） | **Apache-2.0** |
| `torrust-clock` 3.0.0、`torrust-located-error` 3.0.0、`torrust-metrics` 0.1.0、`torrust-net-primitives` 0.1.0、`torrust-server-lib` 0.3.0 | — | 已发布 | AGPL-3.0-only |

结论：**拆分出来的包只能通过 git 依赖引用，并且必须锁定 rev**，例如：

```toml
torrust-tracker-swarm-coordination-registry = { git = "https://github.com/torrust/torrust-tracker", rev = "135ec2ab410071d6417823acadfccbbb66b18ffe" }
```

已用 `cargo generate-lockfile` 实测：git 依赖能正确解析到 workspace 中的成员包。

### 2.2 包结构与依赖关系

```
                        primitives ◄──────────── configuration
                       (Peer, AnnounceEvent,       (Core、HttpTracker 等配置)
                        TrackerPolicy …)
                            ▲
 events ◄──── swarm-coordination-registry          http-protocol           udp-protocol
 (tokio        (Registry: SkipMap<InfoHash,          (Query/Announce 解析、   (BEP 15 编解码)
  broadcast)    Mutex<Coordinator>>，事件)            bencode 响应、IP 解析)        ▲
        ▲             ▲                                    ▲                       │
        └──── tracker-core ──────────────────────────────┐ │                       │
              (AnnounceHandler、ScrapeHandler、key 鉴权、  │ │                       │
               白名单、数据库：sqlx 多种数据库)             │ │                       │
                      ▲                                   │ │                       │
               http-core (AnnounceService：鉴权→白名单→IP→announce→事件)     udp-core (cookie、封禁)
                      ▲                                                            ▲
               axum-http-server (路由写死 /announce、/announce/{key})        udp-server
                      ▲                                                            ▲
                      └──────────────── torrust-tracker 根二进制 ──────────────────┘
```

各包职责与关键公开 API：

| 包 | 职责 | 关键公开 API | PT 是否复用 |
|---|---|---|---|
| primitives | 领域类型 | `peer::Peer { peer_id, peer_addr, updated, uploaded, downloaded, left, event }`、`AnnounceEvent`、`AnnouncePolicy`、`TrackerPolicy`、`SwarmMetadata` | ✅ |
| events | 进程内事件总线 | `EventBus<E>`、`Broadcaster<E>`（`tokio::sync::broadcast`，容量 65536）、`Sender` / `Receiver` trait | ✅（只用来观测） |
| swarm-coordination-registry | 内存 swarm | `Registry::handle_announcement(&InfoHash, &Peer, Option<NumberOfDownloads>) -> Result<(), Error>`、`get_peers_peers_excluding`、`get_swarm_metadata_or_default`、`remove_inactive_peers`、`remove_peerless_torrents`；`SwarmCoordinationRegistryContainer::initialize(SenderStatus)` | ✅ 核心复用 |
| http-protocol | HTTP tracker 协议 | `Query: FromStr`、`Announce: TryFrom<Query>`、`responses::Announce<Compact/Normal>`、`responses::error::Error`、`peer_ip_resolver::resolve_remote_client_addr` | ✅ |
| tracker-core | announce/scrape 编排、key、白名单、数据库 | `TrackerCoreContainer::initialize_from(core, swarm, Option<&Database>)`、`AnnounceHandler::handle_announcement(...)`、`AuthenticationService`、`WhitelistAuthorization` | ❌ 不用（原因见 §2.6、§3） |
| http-core | HTTP announce 服务 | `AnnounceService::handle_announce(&Announce, &ClientIpSources, &ServiceBinding, Option<Key>)` | ❌ 不用 |
| axum-http-server | axum 路由与提取器 | `v1::routes::router(...)`（路由写死）、`v1::extractors::*` | ❌ 不用 |
| udp-* | UDP tracker | — | ❌ 不用（见 §2.8） |

### 2.3 announce 处理流程（torrust 原生）

**HTTP**（`axum-http-server` → `http-core` → `tracker-core` → `swarm-coordination-registry`）：

```
GET /announce/{key}?info_hash=…
 │ axum-http-server::v1::handlers::announce::handle_with_key
 │   ├─ ExtractRequest：raw query → Query → Announce（http-protocol）
 │   ├─ ExtractClientIpSources（ConnectInfo / X-Forwarded-For）
 │   └─ ExtractKey：路径段 → tracker-core::authentication::Key
 ▼
 http-core::services::announce::AnnounceService::handle_announce
 │   ① authenticate(maybe_key)        仅当 core.private=true：在 InMemoryKeyRepository 中查 key，检查过期时间   ◄─ [插入点 1：鉴权]
 │   ② authorize(info_hash)           仅当 core.listed=true：查 InMemoryWhitelist                              ◄─ [插入点 2：种子白名单]
 │   ③ resolve_remote_client_addr / select_peer_ip
 │   ④ peer_from_request → Peer{peer_id, addr, uploaded, downloaded, left, event}                             ◄─ [插入点 3：客户端过滤]
 ▼
 tracker-core::announce_handler::AnnounceHandler::handle_announcement
 │   ⑤ whitelist 再检查一次 → change_ip → InMemoryTorrentRepository::handle_announcement
 ▼
 swarm-coordination-registry::Registry::handle_announcement
 │   ⑥ SkipMap 中找到或新建 Coordinator（Mutex），以 peer_addr 为键 upsert / remove
 │      → 发出 PeerAdded / PeerUpdated{old,new} / PeerRemoved / PeerDownloadCompleted 事件
 │   ⑦ 返回 ()（不会把旧 peer 返回给调用方）                                                                  ◄─ [插入点 4：增量计算——拿不到旧值]
 ▼
 ⑧ build_announce_data：peers_excluding（按 SocketAddr 顺序取前 N 个）+ SwarmMetadata
 ⑨ http-core 发出 TcpAnnounce{connection, info_hash, announcement} 事件                                       ◄─ [插入点 5：事件输出]
 ⑩ bencode 响应（错误也返回 200 + failure reason）
```

**UDP**（`udp-server` → `udp-core::services::announce` → 同上的 `AnnounceHandler`）：connection cookie 校验 → 白名单 → `peer_builder::from_request` → `handle_announcement` → `UdpAnnounce` 事件。**这条路径上没有任何 key 鉴权。**

**插入点评估**：插入点 1、2 只能在 torrust 的模型中工作（详见 §2.6）；插入点 3 在 torrust 中没有对应的钩子；插入点 4 拿不到旧值；插入点 5 的事件里没有用户身份。所以在 torrust 原有的 HTTP 链路里**没有一个能直接用的扩展点**。可行的做法是**自己写 HTTP 入口，只调用 swarm 层和协议层**，这正是 §4 推荐的方案。

### 2.4 events 机制

- 实现：`Broadcaster<E>` 包装 `tokio::sync::broadcast::channel(65536)`；`EventBus` 可以整体关闭（`SenderStatus::Disabled` 时 `sender()` 返回 `None`）。
- **只在进程内**，没有持久化，没有跨进程投递。订阅者处理慢了会收到 `RecvError::Lagged(n)`，直接丢事件；没有订阅者时 `send` 返回 `Err`，调用方忽略。
- 各包有自己的事件枚举（ADR `20260727000000_events_are_objective_facts`：事件是客观事实）：

| 来源 | 事件 | 字段 |
|---|---|---|
| swarm-coordination-registry | `TorrentAdded{info_hash, announcement}`、`TorrentRemoved`、`PeerAdded{info_hash, peer}`、`PeerRemoved{info_hash, peer}`、**`PeerUpdated{info_hash, old_peer, new_peer}`**、`PeerDownloadCompleted{info_hash, peer}` | `Peer` 带 `peer_id`、`peer_addr`、`uploaded`、`downloaded`、`left`、`event`、`updated` |
| http-core | `TcpAnnounce{connection, info_hash, announcement}`、`TcpScrape` | `connection` 中有客户端 IP、端口、服务绑定信息 |
| udp-core | `UdpAnnounce{…}` 等 | 同上 |

**能否订阅 announce 事件来做记账？不能。** 原因有三：

1. **事件里没有 key，也没有用户身份**（`Peer` 只有 peer_id 和地址）。passkey 只出现在 http-core 鉴权的局部变量里，不会进入事件。
2. 事件会丢（broadcast 满了就 Lagged），而记账必须可靠。
3. `PeerUpdated` 中虽然有旧累计值，但它是按 `SocketAddr` 关联的（见 §2.7），和我们需要的“按 peer 会话”的语义不一致。

原型实测确认了这一点（§6.3）：订阅 swarm 事件总线能收到 `PeerUpdated old_up=0 new_up=300`，但无法知道是哪个用户。**结论：events 只用于可观测性，不作为记账来源。**

### 2.5 配置

`configuration::v3_0_0::core::Core { announce_policy, database: Option<Database>, inactive_peer_cleanup_interval, listed, private, private_mode, tracker_policy, tracker_usage_statistics }`。默认 `AnnouncePolicy{interval:120, interval_min:120, max_peers_per_announce:74}`，`TrackerPolicy.max_peer_timeout = 900`。PT 场景的 announce 间隔通常是 1800 秒，**`max_peer_timeout` 必须调到大于 announce 间隔**，否则 peer 会在两次 announce 之间被清理掉（原型开发中实际遇到过这个问题）。

### 2.6 私有模式与 key 机制

| 项 | 实际情况（tracker-core/src/authentication） |
|---|---|
| 格式 | `Key(String)`，**正好 32 个字符，只允许 `[0-9A-Za-z]`**（`AUTH_KEY_LENGTH = 32`）。我们的 32 位小写十六进制 passkey 在**语法上兼容** |
| 携带方式 | HTTP 路径 `/announce/{key}`、`/scrape/{key}`（axum 路由写死） |
| 存储 | `InMemoryKeyRepository = RwLock<HashMap<Key, PeerKey>>`，`PeerKey{key, valid_until}`；持久化用 sqlx 数据库表 |
| 校验 | `core.private=true` 时：查内存表，并按 `private_mode.check_keys_expiration` 检查过期时间 |
| 能否换成自定义鉴权 | **不能直接替换**：① `PeerKey` 里**没有 user_id**，鉴权通过后上层也拿不到用户；② `InMemoryKeyRepository::insert/remove` 是 `pub(crate)`，外部只能通过 `KeysHandler`（必须有数据库）或 `reset_with`（全量替换）写入；③ 根二进制在 `private=true` 且没有数据库时拒绝启动（`PersistenceRequirementError::PrivateRequiresDatabase`）；④ 没有鉴权 trait 可以注入 |
| 白名单 | 情况相同：`InMemoryWhitelist::add` 是公开的，`remove` 是 `pub(crate)`，删除也只能走需要数据库的 `WhitelistManager` |

**结论：torrust 的 key 和白名单机制不使用**。设置 `private=false`、`listed=false`，鉴权和白名单由 pt-tracker 在调用 swarm 之前自己完成。

### 2.7 swarm 存储

- `Registry { swarms: SkipMap<InfoHash, Arc<Mutex<Coordinator>>> }`，每个种子一把 `tokio::Mutex`。
- `Coordinator { peers: BTreeMap<SocketAddr, Arc<Peer>>, metadata: SwarmMetadata }`。
- **peer 以 `SocketAddr`（IP:端口）为键，不是以 peer_id 为键**。同一个地址上换了 peer_id 会被当成同一个 peer 覆盖；地址相同的情况下，任何一个 peer_id 发来的 `stopped` 都会把这个地址上的 peer 删掉。原型实测复现了这一点（§6.3 第 11、12 步）。
- `handle_announcement` 返回 `()`，**不返回旧的 peer**；如果先 `get` 再 announce，两次调用之间存在竞态。
- `Peer` 中存了 `uploaded`、`downloaded`、`left`，但**没有用户身份**。
- **选 peer 的策略**：`peers_excluding` 按 `SocketAddr` 的顺序取前 N 个，**不随机，也不区分做种者和下载者**（做种者也会拿到其他做种者）。对大 swarm 来说，这会导致所有人都拿到同一批地址最小的 peer。

**结论：没法从 swarm 中拿到“上一次的累计值”来算增量。pt-tracker 必须自己维护 peer 会话状态**，以 (user_id, info_hash, peer_id) 为键。原设计里“peer 最近累计值写 Redis”的做法仍然成立，但这份数据是 pt-tracker 自己的，不是 torrust 的。

### 2.8 UDP

- torrust 在**私有模式下拒绝启动 UDP tracker**（`src/app.rs`：“UDP is not safe for private trackers!”）。
- UDP 链路完全没有 key 鉴权；BEP 15 中的 `key` 字段是客户端随机生成的 i32，不是 passkey。
- 携带 passkey 的标准做法是 BEP 41 的 URLData 扩展（把 `/announce/{passkey}` 放进选项里），但 **torrust 没有实现 BEP 41**（udp-server 文档中写明“not implemented yet”），客户端对它的支持程度也参差不齐（未验证）。
- UDP 本身没有 TLS，passkey 会以明文传输。

**结论：PT 场景只支持 HTTP(S)**（决策 D15）。UDP 相关的包都不引入。

### 2.9 许可证

- torrust-tracker 及其拆分出来的所有包都是 **AGPL-3.0-only**。只有 `torrust-info-hash`、`torrust-peer-id`、`torrust-bencode`（以及 `bittorrent-primitives`，MIT OR Apache-2.0）是宽松许可证。
- 影响（以下不构成法律意见，正式发布前需要法务确认）：
  1. **只要链接了任何一个 AGPL 包（方案 A、B、B′ 都会），`pt-tracker` 二进制就必须以 AGPL-3.0 分发**。AGPL 第 13 条要求向**通过网络与它交互的用户**（这里就是 BT 客户端和站点用户）提供对应源码，包括我们自己的修改。实际做法是在仓库中公开 `crates/pt-tracker` 的源码，并在站点页脚或 tracker 的错误响应中给出源码地址。
  2. 内核和插件与 pt-tracker 是**不同进程，只通过 Redis 数据结构通信**，通常不构成衍生作品，可以使用其他许可证。前提是**不能把 pt-tracker 的代码或 torrust 的包链接进内核或插件**。
  3. 这条约束需要并入 09 号文档中“开源协议”这个待决问题：仓库整体采用 MIT/Apache 也可以，`crates/pt-tracker` 单独标注 AGPL-3.0（多许可证仓库）。
  4. 下游站点如果修改了 pt-tracker，同样必须公开源码。这对“防止闭源魔改”的目标反而是有利的。
  5. 如果项目以后要求 tracker 也必须使用宽松许可证，只能换成方案 D（§3），并且只能复用 Apache 许可的基础包。

### 2.10 与原设计假设不一致的地方（汇总）

| # | 原假设（01 / 03 / 09 号文档） | 实际情况 | 影响 |
|---|---|---|---|
| 1 | 复用 tracker-core、http-core、udp-core 等各包 | tracker-core 的 key 和白名单机制无法换成我们的 passkey 和用户模型；http-core、axum-http-server 的路由和流程写死，没有钩子；UDP 不能用 | **只复用 swarm-coordination-registry、http-protocol、primitives、events**，HTTP 入口和流水线自己写 |
| 2 | 订阅 events 获取 announce 数据来做记账 | 事件只在进程内、会丢、不带用户身份 | events 只用于观测；增量在请求流水线内同步计算 |
| 3 | 从 swarm 拿到 peer 的上一次累计值 | swarm 以 SocketAddr 为键，不返回旧值，没有用户身份 | pt-tracker 自己维护 peer 会话状态（内存 + Redis） |
| 4 | 各包已经拆分，可以直接依赖 | 都没有发布，只能用 git 依赖并锁定 rev；develop 改动频繁，没有 API 稳定承诺 | 用适配层隔离 torrust；升级要主动做，并跑回归测试 |
| 5 | tracker 节点无状态 | swarm 在内存里，节点重启后 peer 列表为空，要等客户端重新 announce（最长 30 分钟） | 首次访问某个种子时，从 Redis 的 peer 会话状态懒加载重建 swarm（§5.6） |
| 6 | 负载均衡按 `info_hash` 参数做一致性哈希 | `info_hash` 是百分号编码的二进制，不同客户端的编码方式可能不同（非保留字符是否转义、十六进制大小写），直接按原始字符串哈希，同一个种子可能落到不同节点（根据协议推断，未实测各客户端） | 负载均衡必须先 URL 解码再哈希（§5.7） |
| 7 | — | torrust 选 peer 不随机，做种者也会拿到做种者 | 适配层先取全部 peer 再自己筛选；超大 swarm 有 O(n) 开销（§7） |
| 8 | — | torrust 的解析错误信息中带有源码文件路径（`Location`） | 返回给客户端前必须把错误信息替换成固定文案 |
| 9 | 许可证未作考虑 | 全部是 AGPL-3.0-only | pt-tracker 单独使用 AGPL（§2.9） |

---

## 3. 候选方案对比

| 方案 | 做法 |
|---|---|
| **A fork** | fork torrust-tracker，在 http-core 或 tracker-core 中加入 passkey→user、客户端过滤、增量计算和事件出口 |
| **B 整体组装** | 按原计划依赖 tracker-core + http-core（+ axum-http-server）组装，通过它们的扩展点加功能 |
| **B′ 收窄组装（推荐）** | 只依赖 `swarm-coordination-registry` + `http-protocol` + `primitives` + `events`（git rev 锁定）。HTTP 入口、准入、增量、事件自己写，torrust 包藏在 `SwarmBackend` / `protocol` 适配层后面 |
| **C 反向代理** | 原版 torrust 前面放一个代理：代理负责鉴权、解析和算增量，再把请求转发给 torrust |
| **D 自研 swarm** | 不依赖 AGPL 包，只复用 Apache 许可的 `torrust-bencode`、`torrust-info-hash`、`torrust-peer-id`，swarm 和协议解析自己写（参考 aquatic、UNIT3D-Announce 的设计） |

评分：5 分最好。“维护成本”一列分数越高表示维护越省事。

| 维度 | A fork | B 整体组装 | **B′ 收窄组装** | C 代理 | D 自研 |
|---|---|---|---|---|---|
| 可行性 | 5：什么都能改 | 2：鉴权和白名单不能替换（§2.6），axum 路由写死，**需要绕过大半** | **5：原型已跑通** | 3：可行，但 torrust 的私有模式需要把 key 同步进它的数据库，否则只能以公开模式运行 | 4：需要自己写约 1.5k～2k 行代码并测试 |
| 维护成本 | 2：要长期 rebase，上游 6 个月 3000+ 提交 | 3 | **4：只依赖 4 个包，改动面小** | 2：要部署和维护两个服务 | 3：所有代码都归自己维护 |
| 性能 | 4 | 4 | **4**（选 peer 需要绕一下，见 §7） | 2：多一跳，解析两遍 | 5：可以按 PT 场景专门优化 |
| 升级跟进 | 2：合并冲突多 | 2：依赖面大，上游 API 改动频繁 | **3：锁定 rev，改动集中在适配层** | 4：作为黑盒替换二进制 | 5：没有上游 |
| 依赖体积 | 大 | 大（sqlx 三种数据库驱动 + native-tls） | **小（不需要 sqlx 和 openssl）** | — | 最小 |
| 许可证 | AGPL | AGPL | AGPL | torrust 进程是 AGPL，代理可以用其他许可证 | **可以用宽松许可证** |
| 是否符合 D9“使用 Torrust” | 是 | 是 | 是 | 是 | 否（只用到它的基础包） |
| **合计（前 4 项）** | 13 | 11 | **16** | 11 | 17 |

其他发现：

- **原 B 方案不可行的关键点**：http-core 的 `AnnounceService` 把鉴权、白名单、IP 选择、swarm 更新、发事件串成一个方法，中间没有注入点；`AnnounceHandler`（tracker-core）倒是可以单独调用（原型第一版实测过，`private=false`、`listed=false`、`database=None` 能正常工作），但它只是对 Registry 的薄封装，代价是引入 sqlx 的三种数据库驱动。所以第二版原型直接改用 Registry。
- **D 虽然合计分最高，但不符合 D9**，而且要承担 swarm 的正确性测试。等到许可证决策（09 号文档）定为“tracker 也必须宽松许可”，或者 torrust 的 swarm API 发生破坏性变更、适配成本超过自研成本时，再切换到 D。B′ 的适配层就是为这次切换预留的。

## 4. 推荐方案及理由

**采用 B′：收窄组装。**（决策 D14）

1. **已验证可行**：原型用 4 个 torrust 包，完成了 passkey 路径鉴权、用户状态、客户端白名单、种子白名单、swarm 更新、增量计算、流量事件和 bencode 响应的全链路（§6）。
2. **复用了真正有价值的部分**：经过测试的 HTTP announce 解析（百分号解码、各种边界参数）、bencode 编码（compact 和普通格式、IPv4 和 IPv6）、并发 swarm 结构（SkipMap + 每个种子一把锁）以及不活跃 peer 的清理。
3. **避开了不适合 PT 的部分**：key 和白名单模型、数据库、写死的路由、UDP。
4. **控制风险**：所有 torrust 类型只出现在 `swarm/` 和 `protocol/` 两个适配模块中。业务模块（准入、增量、事件出口）只依赖自己定义的类型，需要时可以低成本切换到方案 D。
5. 符合 D9，也符合“tracker 不包含业务规则”的原则。

附带决策：

- **D15**：只支持 HTTP(S) announce，不提供 UDP。
- **D16**：`crates/pt-tracker` 单独使用 AGPL-3.0-only 许可证，与内核和插件只通过 Redis 通信，不链接任何内核或插件代码。
- **D17**：增量计算的依据是 pt-tracker 自己维护的 peer 会话状态（键为 user_id + info_hash + peer_id），不依赖 torrust 的 swarm 或 events。

---

## 5. pt-tracker 详细设计

### 5.1 模块划分

```
crates/pt-tracker/            (AGPL-3.0-only)
├─ src/main.rs                启动、配置加载、任务编排、优雅退出
├─ server/                    axum：/announce/{passkey}、/scrape/{passkey}、/health、/metrics
├─ protocol/   [适配层]       包装 torrust http-protocol：解析、编码、错误文案清洗
├─ admission/                 准入：passkey → UserCtx、用户状态、种子白名单、客户端白名单（全部本地缓存）
├─ sync/                      与 tracker-bridge 同步：启动时加载快照 + 消费 trk:sync 变更流
├─ swarm/      [适配层]       trait SwarmBackend；TorrustSwarm（包装 Registry）；懒加载重建；清理任务
├─ accounting/                增量纯函数 compute()；PeerStateStore（本地缓存 + Redis CAS）
├─ sink/                      trait TrafficSink；RedisSink（XADD + HINCRBY pipeline）；有界队列与背压
├─ shard/                     分片环配置、所有权判断、迁移期处理
└─ metrics/                   Prometheus 指标
```

依赖方向：`server → admission / protocol / swarm / accounting / sink`。`accounting` 和 `admission` 不依赖任何 torrust 类型。

### 5.2 请求处理流水线

```
GET /announce/{passkey}?info_hash=…&peer_id=…&port=…&uploaded=…&downloaded=…&left=…&event=…
 │
 ① 解析   protocol::parse(raw_query) → AnnounceReq           失败 → "bad request"（固定文案，不带内部错误）
 ② 鉴权   passkey 格式（32 位 [0-9a-f]）→ 本地缓存查 UserCtx  格式错误 → "invalid passkey"；查不到 → "unregistered passkey"
          用户状态 banned                                    → "account disabled"
 ③ 过滤   客户端白名单（peer_id 前缀 + User-Agent，默认拒绝）  → "client not allowed"
          （可选）单个 passkey 的 announce 限速
 ④ 种子   info_hash → TorrentCtx（torrent_id、状态、是否计账） → "torrent not registered"
          用户状态 leech_disabled 且 left>0 且 event≠stopped  → "download privilege disabled"
 ⑤ swarm  SwarmBackend::announce(info_hash, peer, numwant)    （torrust Registry：upsert/remove，取 peers 和统计）
 ⑥ 投递   AnnounceFact{user, torrent, peer_id, ip, port, report, ua} → accounting 队列（按 key 哈希分到 K 个 worker，保证同一个 peer 会话的顺序）
          队列满 → "tracker busy"（客户端会重试；不丢弃已接受的流量）
 ⑦ 响应   bencode（compact / 普通格式），interval、min interval 取自配置
 ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ 以下为异步，响应不等待 ─ ─ ─ ─ ─ ─ ─ ─ ─ ─
 ⑧ 增量   accounting worker：取旧状态（本地缓存，未命中则读 Redis）→ compute() → 写新状态（CAS）
 ⑨ 输出   sink：同一个 pipeline 中执行 XADD acct:<user_id % N>，再执行 HINCRBY u:<uid>:live
```

要点：

- **热路径不读 Redis**：②～④ 全部查本地缓存（§5.5），⑤ 只操作内存。只有 ⑧⑨ 访问 Redis，而且是在后台批量执行。
- **响应与记账解耦**：Redis 抖动不会拖慢 announce 响应；Redis 长时间不可用时队列会积压，触发背压（返回 busy），而不是丢数据。
- **为什么先更新 swarm 再记账**：只有通过准入并被 swarm 接受的 announce 才产生流量事件。swarm 更新不会因为业务原因失败。
- **错误响应**：遵循 BT 惯例，HTTP 200 + `d14:failure reason…e`，文案固定，不暴露内部信息，日志中 passkey 要打码。

### 5.3 增量计算算法

**状态**（`PeerState`，键为 `(user_id, info_hash, peer_id)`）：`uploaded`、`downloaded`、`left`、`last_announce`、`completed`、`ip`、`port`、`ver`。

**纯函数** `compute(prev: Option<&PeerState>, report, policy) -> (Delta, Option<PeerState>)`，实现和单元测试见原型 `src/accounting.rs`：

| 情况 | Δup / Δdown | Δseed_time / Δleech_time | 新状态 | 信号位 |
|---|---|---|---|---|
| 没有旧状态，且 event=started | 按上报值计（正常应为 0；部分客户端重启后会续报非 0 值，如实计入） | 0 | 建立 | `first_seen` |
| 没有旧状态，且 event≠started（状态丢失、Redis 也丢了、迁移出错或客户端异常） | **0（只建立基线）** | 0 | 建立 | `first_seen`、`baseline_reset` |
| 有旧状态，正常 | 新值 − 旧值 | `min(now − last, max_interval)`，记到**上一次的角色**上（上次 `left==0` 记为做种时长，否则记为下载时长） | 更新 | 间隔被截断时标 `seed_time_capped` |
| 累计值回退（新值 < 旧值） | 对应字段记 0，重新建立基线 | 同上 | 更新 | `upload_regressed` 或 `download_regressed` |
| event=completed，且该会话之前没有 completed 过 | 正常计算 | 正常计算 | `completed=true` | `first_completed`（worker 据此记 snatch，按 user + torrent 去重） |
| 重复的 completed | 正常计算 | 正常计算 | 不变 | 无 |
| event=stopped | 正常计算（最后一段流量要计入） | 正常计算 | **删除** | 无 |
| 同一个用户在同一个种子上有多个 peer_id（例如多台盒子） | 各会话分别计算 | | | 由下游做交叉校验 |
| 节点重启 / 分片迁移 | 从 Redis 读回旧状态，按“有旧状态”的情况处理 | | | 无 |

原则：

- **宁可少记，不能多记**：拿不准时只建立基线，不计增量（`baseline_reset`）。所有异常都通过信号位如实交给下游，tracker 本身不判定作弊。
- `max_interval` 取 `max_peer_timeout`（建议设为 announce 间隔的 1.5 倍，例如 2700 秒），防止长时间离线后一次性记上大量做种时长。
- 速度上限、`left` 大于种子体积等检查放在记账 worker 或风控插件中做（体积等信息在 TorrentCtx 中，可以一并下发）。
- hybrid（v1+v2）种子：客户端可能分别用 v1 hash 和截断后的 v2 hash 各 announce 一次，并且上报相同的累计值，这样会重复计数。**同一个 torrent_id 只对一个 hash（v1）计账**，另一个 hash 在 TorrentCtx 中标记 `account=false`，只参与 swarm。各客户端的实际行为未验证，见 §7。

**并发与一致性**：

- 同一个会话的 announce 由同一个 accounting worker 串行处理（按 key 哈希分队列）。
- 写回 Redis 时做 CAS：Lua 脚本比较 `ver`。不一致说明这个会话在别的节点上被处理过（分片迁移期间可能出现），这时用 Redis 中的值重新计算一次。
- 先写状态，再 XADD。如果两步之间崩溃，最多少记一个间隔的流量，**不会重复记账**。

### 5.4 流量事件数据结构

> **键名记法**：本文 Redis 键中的 `<x>` 表示占位符，真实键名如 `acct:17`、`u:10086:live`、`ps:<40 位 hex>`；`{}` 在 Redis Cluster 中是哈希标签语法，不用于表示占位。全局前缀分配以 [../platform/catalog/redis-keys.md](../platform/catalog/redis-keys.md) 为准。

Redis Stream：`XADD acct:<user_id % N> MAXLEN ~ <上限> * field value …`

| 字段 | 类型 | 说明 |
|---|---|---|
| `v` | u8 | 格式版本，目前为 1 |
| `eid` | string | `<node_id>:<boot_epoch>:<seq>`，排查用，也用于下游在 XADD 超时重试时去重 |
| `ts` | u64 | announce 时间（unix 秒） |
| `uid` | u64 | 用户 ID |
| `tid` | u64 | torrent_id（由 tracker-bridge 下发的映射得到） |
| `ih` | hex(40) | info_hash |
| `pid` | hex(40) | peer_id |
| `ip`、`port` | string、u16 | 供反作弊使用（敏感数据，插件默认不能读取，见 02 号文档 §5） |
| `ev` | `started` / `stopped` / `completed` / `none` | |
| `up`、`down`、`left` | u64 | 本次上报的原始累计值，便于复核 |
| `dup`、`ddown` | u64 | 增量 |
| `dseed`、`dleech` | u64 | 做种 / 下载时长增量（秒） |
| `flags` | u32 位掩码 | bit0 first_seen、bit1 baseline_reset、bit2 upload_regressed、bit3 download_regressed、bit4 first_completed、bit5 seed_time_capped |
| `ua` | string | User-Agent（截断到 128 字节） |

- 序列化：Stream 的平铺 field-value 结构，数值使用十进制字符串，不嵌套 JSON。原型为了方便核对，flags 暂时用 JSON 表示，正式实现改为位掩码。
- 分区：`N` 全站固定（建议 64），部署后不能修改。如需扩容，增加 worker 而不是增加分区。同一个用户的事件始终进入同一条流，保证用户维度有序。
- 实时计数：`HINCRBY u:<uid>:live up|down|seed_time Δ`。这是**原始值**，不乘倍率，只给前端做秒级展示；结算口径以数据库为准。
- 只有 `dup`、`ddown`、`dseed`、`dleech` 都为 0，并且没有 event 和 flags 的 announce，可以按配置不输出事件（`emit_zero_delta=false`），这样能大幅减少 Stream 的量。

### 5.5 与 tracker-bridge 插件的同步接口

tracker-bridge（Native 插件）是**唯一的写入方**，pt-tracker 只读。采用“**快照键 + 变更流**”两层结构：

| 数据 | Redis 键 | 内容 |
|---|---|---|
| passkey | `trk:pk:<passkey>` → HASH | `uid`、`st`（状态：0 正常、1 禁止下载、2 封禁）、`ver` |
| 种子 | `trk:t:<info_hash_hex>` → HASH | `tid`、`st`（0 正常、1 禁用）、`acct`（是否计账，hybrid 种子的 v2 hash 设为 0）、`size`、`ver` |
| 客户端白名单 | `trk:clients` → STRING(JSON) | `{ver, default:"deny", rules:[{name, peer_id_prefix, ua_prefix?, ua_regex?, action:"allow"/"deny"}]}` |
| 运行参数 | `trk:cfg` → HASH | `interval`、`min_interval`、`max_peers`、`emit_zero_delta` 等 |
| 变更流 | `trk:sync` → STREAM | `{kind: pk/t/clients/cfg, key, op: upsert/delete, ver}`，`MAXLEN ~ 1e6` |

节点行为：

1. 启动：记录 `trk:sync` 的最新 ID → SCAN 加载全部 `trk:pk:*`、`trk:t:*`、`trk:clients`、`trk:cfg` 到本地（30 万 passkey 约 24 MB，100 万种子约 40 MB）→ 从记录的 ID 开始 `XREAD BLOCK` 消费变更。
2. 变更：按 `kind` 和 `key` 重新读取对应的快照键（`ver` 更大才覆盖），`op=delete` 就从本地删除。
3. 发现缺口：如果自己的游标已经早于 `trk:sync` 中最早的条目（流被修剪过），就重新加载全量快照。
4. 本地查不到时回源：防止“刚注册或刚上传就 announce”的竞态。本地未命中时读一次 Redis，结果做负缓存（60 秒），并限制回源速率，防止被拿来做穷举。
5. 生效时间：正常情况下 1 秒内生效。封禁等安全相关的变更依赖这个时效。

tracker-bridge 的职责：在 passkey 生成或重置、用户状态变化、种子上传、删除或禁用、客户端白名单变化时，**在同一个逻辑操作中先写快照键，再 XADD `trk:sync`**；并提供“全量重建”命令用于修复数据。passkey 是敏感数据，Redis 要通过 ACL 限制 pt-tracker 只读 `trk:*`，只写 `ps:*`、`acct:*`、`u:*:live`。

### 5.6 peer 会话状态的存储与 swarm 重建

| 键 | 结构 | 说明 |
|---|---|---|
| `ps:<info_hash_hex>` | HASH，field 为 `<uid>:<peer_id_hex>`，value 为紧凑二进制（up、down、left、last_ts、completed、ip、port、ver，约 60 B） | 每次写入时刷新 key TTL（`max_peer_timeout × 2`）；加载时丢弃超时的 field。也可以用 Redis 7.4+ 的 `HEXPIRE` 给单个 field 设置过期时间 |

- 按 info_hash 组织，和 tracker 的分片维度一致；在 Redis Cluster 中同一个种子的数据落在同一个 slot，CAS 脚本只涉及一个 key。
- **懒加载重建**：节点收到一个本地 Registry 中还没有的 info_hash 时（重启后或分片迁入后），先 `HGETALL ps:<info_hash_hex>`，把仍然有效的会话①放入本地状态缓存，②以原来的 `updated` 时间作为 announce 写回 torrust Registry，这样 peer 列表马上恢复，不用等最长 30 分钟的重新 announce。每个种子在每个节点上只做一次。
- 容量估算（未验证）：3000 万 peer × 约 130 B ≈ 4 GB，建议使用**独立的 Redis（或 Cluster）存放 tracker 状态**，与记账流和实时计数分开。

### 5.7 水平扩展与分片

- **负载均衡**：按**URL 解码后的** `info_hash` 做一致性哈希。可选做法：HAProxy `balance hash url_param(info_hash),url_dec`（2.6+，未验证），Envoy `ring_hash` 配合 Lua 做规范化，或者自己写一个很薄的 L7 路由。**禁止直接按原始 query 字符串做哈希**（§2.10 #6）。
- **兜底**：节点维护和负载均衡一致的哈希环配置（来自 `trk:cfg` 或部署配置）。收到不属于自己的 info_hash 时，计数 `misrouted_total`，但仍然正常处理（CAS 能保证增量正确），不转发。
- **扩缩容**：大约 1/N 的种子迁到新节点，新节点通过懒加载接手；旧节点上残留的 swarm 由清理任务在超时后回收。迁移期间两个节点可能同时处理同一个会话，由 CAS 解决。
- **scrape**：单个 hash 的 scrape 按 info_hash 路由，直接由 swarm 回答；多个 hash 的 scrape 返回“不支持”（PT 客户端很少使用；需要的话可以由节点定期把 SwarmMetadata 汇总写入 Redis，再由一个独立接口回答）。
- **节点内**：torrust Registry 是 SkipMap 加上每个种子一把锁，天然支持多核；accounting worker 数量 K 等于 CPU 核数。

### 5.8 配置项

| 配置项 | 默认值 | 说明 |
|---|---|---|
| `node_id` | 主机名 | 写入事件 `eid` |
| `http.bind` / `http.tls` | `0.0.0.0:7070` / 无 | 通常由负载均衡终止 TLS |
| `http.reverse_proxy` | true | 从 X-Forwarded-For 取客户端 IP（torrust `peer_ip_resolver`） |
| `http.use_ip_from_query` | false | 是否信任 `ip=` 参数（PT 场景建议关闭） |
| `announce.interval` / `min_interval` | 1800 / 900 | |
| `announce.max_peers` | 50 | |
| `swarm.max_peer_timeout` | 2700 | **必须大于 interval** |
| `swarm.cleanup_interval` | 600 | |
| `swarm.rebuild_on_miss` | true | 懒加载重建 |
| `accounting.partitions`（N） | 64 | 全站固定 |
| `accounting.workers`（K） | CPU 核数 | |
| `accounting.queue_capacity` | 100000 | 超过后返回 busy |
| `accounting.emit_zero_delta` | false | |
| `redis.state_url` / `redis.acct_url` / `redis.sync_url` | — | 三种用途可以是同一个实例，也可以分开 |
| `admission.negative_cache_ttl` | 60s | |
| `admission.miss_lookup_rps` | 200 | 回源限速 |
| `admission.per_passkey_rps` | 关闭 | 可选限速 |
| `shard.ring` | — | 节点列表和虚拟节点数，与负载均衡保持一致 |

### 5.9 可观测性指标（Prometheus）

| 指标 | 类型 | 标签 |
|---|---|---|
| `pt_announce_total` | counter | `result`（ok / invalid_passkey / unregistered / banned / client_denied / torrent_unknown / leech_disabled / bad_request / busy） |
| `pt_announce_duration_seconds` | histogram | |
| `pt_swarm_torrents` / `pt_swarm_peers` / `pt_swarm_seeders` | gauge | |
| `pt_swarm_rebuild_total` / `pt_swarm_rebuild_peers` | counter | |
| `pt_accounting_queue_depth` | gauge | `worker` |
| `pt_accounting_events_total` | counter | `event` |
| `pt_accounting_flags_total` | counter | `flag`（first_seen、baseline_reset、regressed …），也作为反作弊的观测窗口 |
| `pt_accounting_bytes_total` | counter | `dir`（up / down） |
| `pt_peer_state_cas_conflict_total` | counter | |
| `pt_sink_errors_total` / `pt_sink_batch_size` | counter / histogram | |
| `pt_redis_rtt_seconds` | histogram | `role`（state / acct / sync） |
| `pt_sync_lag_seconds` / `pt_sync_full_reload_total` | gauge / counter | |
| `pt_admission_cache_entries` | gauge | `kind`（pk / t） |
| `pt_misrouted_total` | counter | |

日志：结构化 JSON；passkey 只输出前 4 位；使用 torrust events 订阅输出 swarm 级别的调试日志（默认关闭）。

---

## 6. 原型验证结果

### 6.1 原型做了什么

`.spike/pt-tracker-proto/`，Rust 2024，约 880 行（含 demo.sh）：

| 文件 | 内容 |
|---|---|
| `Cargo.toml` | path 依赖 torrust 的 `swarm-coordination-registry`、`primitives`、`http-protocol`、`events`；从 crates.io 引用 `torrust-info-hash =0.2.0`、`torrust-peer-id 0.1.0`；注释中给出等价的 git rev 写法 |
| `src/main.rs` | axum 入口 `/announce/{passkey}`、`/announce`（提示必须带 passkey）、`/debug/events`；流水线 ①～⑨（同步版本）；订阅 torrust swarm 事件总线；清理任务 |
| `src/policy.rs` | `PasskeyStore` / `TorrentStore` trait 及内存实现；客户端白名单（peer_id 前缀 + UA 前缀） |
| `src/swarm.rs` | `TorrustSwarm` 适配层：包装 `Registry`，自己筛选 peer（做种者不拿做种者，加简单轮转） |
| `src/accounting.rs` | `compute()` 纯函数 + 7 个单元测试；`PeerStateStore` trait 及分片内存实现 |
| `src/sink.rs` | `TrafficEvent`；`TrafficSink` trait；`LogSink`（输出 JSON，并渲染出等价的 `XADD` / `HINCRBY` 命令）；`MemorySink`；`Fanout` |
| `demo.sh` | 13 个 curl 场景 |

环境中没有 redis-server 和 docker，所以 Redis 部分通过 trait 抽象 + 日志 / 内存实现来验证。

演进过程：第一版通过 `tracker-core::TrackerCoreContainer`（`private=false`、`listed=false`、`database=None`）调用 `AnnounceHandler`，能编译能运行，demo 全部通过。第二版改为直接使用 `Registry`，**不再依赖 tracker-core，依赖树中不再有 sqlx 和 openssl**，demo 同样全部通过。

### 6.2 命令

```bash
cd .spike/pt-tracker-proto
cargo build -j 3          # 首次全量编译 2m00s（4 核，含 tracker-core 的第一版）；第二版增量编译 3s
cargo test -j 3           # test result: ok. 7 passed; 0 failed
./target/debug/pt-tracker-proto &      # 监听 127.0.0.1:7070
./demo.sh
```

### 6.3 输出（节选）

```
== 1. 非法 passkey                     d14:failure reason15:invalid passkeye
== 2. 未注册 passkey                   d14:failure reason20:unregistered passkeye
== 3. 被封禁用户                       d14:failure reason16:account disablede
== 4. 客户端不在白名单（UA 不符）       d14:failure reason18:client not allowede
== 5. 种子未注册                       d14:failure reason22:torrent not registerede
== 6. 禁止下载的用户以 leecher 身份    d14:failure reason27:download privilege disablede
== 7. started（首次出现）              d8:completei0e10:incompletei1e8:intervali1800e12:min intervali900e5:peers0:6:peers60:e
== 13. 无 passkey 路径                 d14:failure reason41:passkey required: use /announce/{passkey}e
```

`/debug/events` 中的流量事件（user 42，同一个 peer_id；第 11 步换了 peer_id，但 IP:端口相同）：

| 步骤 | 上报 up / down / left / event | Δup | Δdown | Δleech | Δseed | flags |
|---|---|---|---|---|---|---|
| 7 | 0 / 0 / 1000 / started | 0 | 0 | 0 | 0 | first_seen |
| 8 | 300 / 600 / 400 | 300 | 600 | 3 | 0 | |
| 9 | 500 / 1000 / 0 / completed | 200 | 400 | 1 | 0 | first_completed |
| 10 | 100 / 1000 / 0（回退） | 0 | 0 | 0 | 1 | upload_regressed |
| 11 | 另一个 peer_id 9999 / 0 / 0 | 0 | 0 | 0 | 0 | first_seen、baseline_reset |
| 12 | 700 / 1000 / 0 / stopped | 600 | 0 | 0 | 0 | 会话被删除 |

日志中渲染出的 Redis 命令（节选）：

```
redis> XADD acct:10 MAXLEN ~ 10000000 * v 1 node node-1 seq 1 ts 1791131176 uid 42 tid 1001 ih 3b2455…1bc0 pid 2d71…6b6c ev None up 300 down 600 left 400 dup 300 ddown 600 dseed 0 dleech 2 flags {…}
redis> HINCRBY u:42:live up 300
redis> HINCRBY u:42:live down 600
```

torrust swarm 事件总线（旁路订阅）：

```
swarm_event: PeerUpdated ih=3b2455…1bc0 addr=127.0.0.1:51413 old_up=0 new_up=300 (无用户身份字段)
swarm_event: PeerUpdated ih=3b2455…1bc0 addr=127.0.0.1:51413 old_up=100 new_up=9999   ← 第 11 步：另一个 peer_id 覆盖了同一个地址
swarm_event: PeerRemoved { … peer_id: "-qB4630-zzzzzzzzzzzz" … }                     ← 第 12 步：A 发出的 stopped 删掉了 B
```

peer 交换（3 个 peer）：做种者 B 只拿到下载者 C；下载者 C 拿到做种者 A 和 B；compact 响应中 `127.0.0.1:6881` 编码正确。

另外顺带发现：参数错误时，torrust 返回的错误文本中带有源码绝对路径，例如 `…/packages/http-protocol/src/v1/requests/announce.rs:559:38`。

### 6.4 结论与验证范围

| 项 | 结论 |
|---|---|
| 以 path 或 git 依赖引用 torrust 包，编译独立项目 | ✅ 已验证（path 编译通过；git rev 依赖已通过 `cargo generate-lockfile` 解析） |
| `/announce/{passkey}` 由自定义鉴权拦截 | ✅ 已验证（非法、未注册、封禁三种情况） |
| 用户状态、种子白名单、客户端白名单（peer_id + UA） | ✅ 已验证 |
| 拿到 announce 参数并计算增量（含 started / completed / stopped / 回退 / 状态丢失） | ✅ 已验证（curl + 7 个单元测试） |
| 输出流量事件（trait + 日志 / 内存实现，渲染出 XADD / HINCRBY） | ✅ 已验证（没有连接真实的 Redis） |
| torrust swarm 更新、peer 交换、compact 和普通格式响应 | ✅ 已验证 |
| torrust events 能订阅、字段内容、不带用户身份 | ✅ 已验证 |
| 真实 Redis：CAS 脚本、pipeline、Stream 吞吐 | ❌ **未验证**（环境中没有 redis-server） |
| 异步 accounting 队列与背压（原型中是同步计算） | ❌ **未验证** |
| tracker-bridge 同步（快照 + 变更流） | ❌ **未验证**（只有设计） |
| 节点重启、懒加载重建、分片迁移 | ❌ **未验证** |
| 负载均衡按解码后的 info_hash 哈希（HAProxy `url_dec` 等） | ❌ **未验证** |
| 性能（目标平均 1.7 万次/秒，高峰 2～3 倍） | ❌ **未验证**（环境中没有 wrk 等压测工具，原型是 debug 构建） |
| 真实 BT 客户端（qBittorrent、Transmission）接入，hybrid 种子的 announce 行为 | ❌ **未验证** |

---

## 7. 风险与待办

### 7.1 风险

| # | 风险 | 等级 | 应对 |
|---|---|---|---|
| R1 | torrust develop 的 API 改动频繁，包没有发布，没有稳定承诺 | 高 | 锁定 git rev；torrust 类型只出现在 `swarm/`、`protocol/` 两个适配模块；升级当作计划内工作，配合回归测试；预留切换到方案 D 的路径 |
| R2 | AGPL-3.0：pt-tracker 必须开源，并向网络用户提供源码 | 中 | D16：单独标注许可证，按进程隔离；正式发布前由法务确认；如果项目要求 tracker 也使用宽松许可证，就切换到方案 D |
| R3 | torrust 选 peer 不随机；适配层取全量后自己筛选，对超大 swarm（上万 peer）是 O(n) | 中 | 先观测 `pt_swarm_peers` 的分布；如果成为瓶颈，向上游提交随机选取的 PR，或者在方案 D 中自己实现 |
| R4 | swarm 以 SocketAddr 为键，同一地址上的不同 peer_id 会互相覆盖 | 低 | 只影响 peer 列表，不影响记账（记账以 peer_id 为键）；记录到 `pt_accounting_flags_total` 的观测中 |
| R5 | 负载均衡对 info_hash 编码做规范化后再哈希，方案未验证 | 中 | 优先验证 HAProxy；不行就自研一个很薄的路由；节点侧用 CAS 兜底，不会算错 |
| R6 | Redis 内存（约 4 GB 的 peer 会话状态）和写入量（每次 announce 1 次 CAS + 1 次 XADD + 1～3 次 HINCRBY） | 中 | 状态和记账使用不同的 Redis；`emit_zero_delta=false`；压测验证 |
| R7 | hybrid 种子重复计账 | 中 | tracker-bridge 下发 `acct=0`；用真实客户端验证 |
| R8 | 节点重启后 swarm 为空，需要懒加载重建 | 低 | §5.6；同时验证重启期间响应中的 peer 数量 |
| R9 | 状态丢失时只建立基线，用户少记一个会话的流量 | 低 | 设计上接受（宁可少记）；`baseline_reset` 指标告警 |

### 7.2 待办

- [ ] 在 `crates/pt-tracker` 中按 §5.1 建立正式工程（git rev 依赖），从原型迁移 `accounting`、`policy`、`swarm`、`sink`。
- [ ] 在有 Redis 的环境中实现并测试：CAS Lua 脚本、RedisSink pipeline、`trk:*` 同步、懒加载重建。
- [ ] 压测（release 构建 + wrk/oha）：单节点吞吐和 p99 延迟，3 节点分片，Redis 写入量。
- [ ] 验证 HAProxy `url_param(info_hash),url_dec` 的一致性哈希；验证多个客户端对 info_hash 的编码差异。
- [ ] 真实客户端联调：qBittorrent 4.x/5.x、Transmission 4.x、libtorrent 2.x 的 hybrid 种子 announce 行为。
- [ ] 与 tracker-bridge、accounting 的设计负责人对齐 §5.4、§5.5 中的键名和字段（变更需要同时更新本文档）。
- [ ] 把 §2.9 的许可证结论并入 09 号文档“开源协议”待决问题。
- [ ] 跟踪上游：随机选 peer、BEP 41、按 peer_id 作为键等改进，以及 swarm-coordination-registry 何时发布到 crates.io。
