# 文档中心

PT Studio 的全部文档。以中文为主；代码标识符、接口名与错误信息为英文。

## 按角色阅读

| 我想… | 从这里开始 |
|---|---|
| 了解项目能做什么 | [功能清单](features.md) → [更新日志](../CHANGELOG.md) |
| 搭一个站 | [Docker 部署](../deploy/docker/README.md) 或 [服务器部署](../deploy/release/DEPLOY.md) → [配置示例](../deploy/site.example.toml) |
| 写一个插件 | [插件开发指南](sdk/README.md) → [WASM 模板](../templates/wasm-plugin/README.md) → [插件 API](platform/plugin-api.md) |
| 参与内核或官方插件开发 | [开发指南](dev/README.md) → [贡献指南](../CONTRIBUTING.md) → [设计总览](design/README.md) |

## 目录

```
docs/
├─ features.md        功能清单：已完成 / 未完成
├─ dev/               开发：本机环境、运行、测试；实现明细；与设计的差异
├─ sdk/               插件开发指南（第三方插件作者）
├─ design/            总体设计：架构、插件系统、tracker 与记账、搜索、前端、AI、路线图
├─ platform/          平台契约：插件 API、名字目录（服务 / 事件 / 钩子 / 权限 / 插槽…）、规范、内核设计
├─ plugins/           每个官方插件的设计文档与实现状态
├─ components/        独立组件的设计：pt-tracker、web 前端
├─ process/           文档流程与模板
└─ reviews/           设计评审记录
```

## 开发

- [开发指南](dev/README.md) — 本地依赖、一键运行、手动运行、测试
- [实现明细](dev/implementation-status.md) — 每个模块已实现与未实现的细项
- [与设计文档的差异](dev/design-deviations.md) — 实现时的取舍；与设计文档冲突时以此为准

## 插件开发

- [插件开发指南](sdk/README.md) — 快速开始、清单、SDK、扩展点、事务与幂等、测试、前端插件、兼容承诺
- [插件 API 契约](platform/plugin-api.md) — WIT `pt:plugin@1.0.0` 原文、Native trait、`plugin.toml` 字段表
- [WASM 模板](../templates/wasm-plugin/README.md) · [Native 模板](../templates/native-plugin/README.md) · [Rust SDK](../crates/plugin-sdk-rust/README.md)

## 设计与契约

- [设计总览](design/README.md) — 设计原则与全部已确定的决策（D1～D21）
- [路线图](design/09-roadmap.md) — 阶段划分、待验证项、待决问题
- [平台契约](platform/README.md) — 名字目录与规范；插件文档引用的名字必须与这里一致
- [官方插件](plugins/README.md) — 插件索引、状态与设计文档
- 组件：[pt-tracker](components/pt-tracker/) · [web 前端](components/web/)

## 流程

- [文档流程与模板](process/README.md) — 需求 → 概要设计 → 详细设计 → 测试计划 → 评审
- [评审记录](reviews/)

> 设计文档写于实现之前，部分细节已在实现中调整。阅读时请对照 [与设计文档的差异](dev/design-deviations.md) 与 [实现明细](dev/implementation-status.md)。
