# 贡献指南

感谢你愿意参与 PT Studio。无论是报告问题、改进文档，还是实现一个插件，都非常欢迎。

## 提问与反馈

- **Bug 与功能建议**：在 [GitHub Issues](https://github.com/imythu/pt-studio/issues) 提交。报告 Bug 时请附上版本号、部署方式（Docker / 发布包 / 源码）、复现步骤和相关日志。
- **使用交流**：QQ 交流群 **1067590352**。
- **安全问题**：请不要公开提交 Issue，通过仓库的 [Security → Report a vulnerability](https://github.com/imythu/pt-studio/security/advisories/new) 私下报告。

## 从哪里开始

- 想找事做：[功能清单 · 未完成](docs/features.md#未完成) 列出了尚未实现的插件和各模块的后续项，其中“尚未实现的插件”都已有完整设计文档，适合作为第一个贡献。
- 想写第三方插件：不需要改本仓库，按 [插件开发指南](docs/sdk/README.md) 从模板开始即可。

## 开发流程

1. 按 [开发指南](docs/dev/README.md) 准备本机环境，`deploy/dev/run-site.sh` 能跑起站点。
2. 从 `main` 新建分支，一个 PR 只做一件事。
3. 新增或修改功能时：
   - 新插件或较大改动，先在 `docs/plugins/<id>/` 写设计（流程与模板见 [docs/process](docs/process/README.md)）。
   - 服务名、事件、钩子、权限点、术语键等名字必须来自 [平台目录](docs/platform/README.md)；需要新增时一并修改目录。
   - 实现与设计有出入时，记入 [与设计文档的差异](docs/dev/design-deviations.md)。
   - 完成后更新 [功能清单](docs/features.md)、[实现明细](docs/dev/implementation-status.md) 与 [更新日志](CHANGELOG.md) 的“未发布”一节。
4. 提交前确认检查通过：

   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-targets      # 0 警告
   cargo test --workspace                      # 端到端测试需要本机 PostgreSQL 与 Redis
   (cd plugins/wasm && cargo build --release --target wasm32-wasip2)
   (cd web && npm run typecheck && npm test)
   ```

## 约定

- 文档使用中文；代码标识符、接口名、错误信息使用英文。
- 编码、API、数据库与安全规范见 [docs/platform/conventions](docs/platform/conventions/)。
- 不复制 copyleft 许可证的代码进入内核与插件，见 [许可证规范](docs/platform/conventions/licensing.md)。

## 许可

提交到本仓库的贡献，除 `crates/pt-tracker` 外均按 MIT OR Apache-2.0 双许可证授权；`crates/pt-tracker` 下的贡献按 AGPL-3.0-only 授权。
