# 08 · 前人的教训（决策 D11）

## 1. 调研结论

| 项目 | 发现 |
|---|---|
| NexusPHP | 仓库关联了 19 个 CVE，主要是 SQL 字符串拼接导致的注入，以及输出未转义导致的 XSS |
| UNIT3D | announce 原本用 PHP 写在主站里，每核约 250 次/秒；后来另外用 Rust 写了外挂 announce 服务（UNIT3D-Announce），每核约 5 万次/秒 |
| Gazelle / Luminance | 已停止开发；Gazelle 的数据模型围绕音乐设计 |
| UNIT3D | 绑定 TMDB，偏向影视 |
| Arcadia | 新项目（Rust 后端 + Vue 前端），目标是“内容无关”，仍处于早期；AGPL 协议，借鉴代码时要注意 |
| Torrust | 没有按用户统计上传/下载量（2024），维护者计划先做事件系统；现在已有 `events` 等独立包 |
| 作弊 | BT 协议只能相信客户端上报；RatioMaster、GreedyTorrent 等工具会虚报上传量 |

## 2. 对策

| 前人的问题 | 我们的对策 |
|---|---|
| NexusPHP 的 SQL 注入和 XSS | 全部使用 sqlx 参数化查询；插件 SQL 走数据库角色沙箱；React 默认转义，加 CSP；插件前端在受限范围内渲染 |
| UNIT3D 用 PHP 处理 announce，扛不住后再外挂 | announce 一开始就独立部署、使用 Rust、可以水平扩展 |
| 每次 announce 都直接写数据库 | 走 Redis Streams，按窗口聚合后批量落库，保证幂等 |
| Gazelle 和 UNIT3D 的数据模型绑定内容类型 | 资源类型做成插件 |
| 站点规则硬编码，各站只能 fork 魔改，之后无法升级 | 规则全部放进插件，加上术语表、预设和主题，核心可以平滑升级 |
| Gazelle 和 Luminance 停更，生态断档 | 插件契约版本化（WIT semver），提供文档和 SDK，降低贡献门槛 |
| 客户端虚报流量 | 记账插件输出基础信号（同一种子内交叉校验、速度上限、peer 不在 swarm 中），配合客户端白名单；判定规则交给插件 |
| 大量清理和结算任务挤在一个 cron 脚本里，长时间锁表 | 调度器加分布式锁，结算按分区增量执行 |

## 3. 参考来源

- [DeepSeek Harness (GitHub)](https://github.com/deepseek-ai/deepseek-harness)
- [InfoQ: DeepSeek Harness](https://www.infoq.com/news/2026/08/deep-seek-harness/)
- [DataCamp: What Is DeepSeek Harness](https://www.datacamp.com/blog/what-is-deepseek-harness)
- [Torrust 讨论 #819：上传/下载统计](https://github.com/torrust/torrust-tracker/discussions/819)
- [Torrust packages 目录](https://github.com/torrust/torrust-tracker/tree/develop/packages)
- [UNIT3D-Announce](https://github.com/HDInnovations/UNIT3D-Announce)
- [UNIT3D Announce 文档](https://rawsmoke.codeberg.page/UNIT3D/unit3d_announce.html)
- [NexusPHP CVE 列表 (vulners)](https://vulners.com/search/vendors/nexusphp%20project/products/nexusphp?year=2017)
- [CVE-2022-46889](https://ocve.arawa.fr/cve/CVE-2022-46889)
- [Torrent Tracker Platforms 对比](https://github.com/meehow/Torrent-Tracker-Platforms)
- [Arcadia (AlternativeTo)](https://alternativeto.net/software/arcadia/about)
- [TU Wien：私有 tracker 作弊检测论文](https://repositum.tuwien.at/handle/20.500.12708/184318)
- [TorrentFreak: GreedyTorrent](https://torrentfreak.com/greedytorrent-the-deadly-bittorrent-sin/)
- [PT 入门与站点别称](https://iecho.cc/posts/beginners-guide-to-private-tracker-1)
