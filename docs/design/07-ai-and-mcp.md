# 07 · AI 与 MCP（决策 D10）

## 1. `ctx.llm` 服务

参考 DeepSeek Harness 的模型适配器：`ctx.llm` 由提供方插件实现——`llm-claude`（Anthropic Claude）、`llm-openai-compat`（OpenAI 兼容接口；vLLM、Ollama 等本地模型也走这一实现，不单列 `llm-local`），其他插件只调用 `ctx.llm`，不关心具体是哪个模型。

## 2. MCP 网关（`mcp` 插件）

- 插件在清单中声明 `[[mcp.tools]]`，MCP 网关自动暴露；工具 schema 与 OpenAPI 共用同一份定义。
- **用户侧**：用户用个人 token（OAuth 授权并限定 scope）连接自己的 AI 客户端。例如：
  - “帮我找某部片的 4K 版本”
  - “我的分享率和 H&R 情况”
  - “推荐需要保种的资源”
- **管理员侧**：审核队列、举报处理、异常用户排查。
- **安全**：
  - 写操作默认需要人工确认，全部记审计日志。确认流程由 `mcp` 插件实现（保存待确认项，提供 `ctx.mcp.pending_confirmations` / `confirm` / `reject`）；内核 MCP 注册表只检查 `confirmed` 标志，`write && confirm` 的工具未确认时返回 `mcp.confirmation_required`（见 [../platform/kernel/03-详细设计说明书.md](../platform/kernel/03-详细设计说明书.md) §10）；
  - tracker passkey、IP、邮箱永远不会出现在 MCP 的返回结果里（内核对输出做敏感字段过滤兜底）；
  - 按 token 限流。

## 3. AI 业务插件

| 插件 | 作用 |
|---|---|
| 上传审核 | 检查标题规范、分析 MediaInfo、判断是否重复资源 |
| 语义搜索 | Meilisearch 混合搜索（见 [04-search.md](04-search.md)），或 pgvector |
| 内容审核 | 论坛、评论审核 |
| 反作弊辅助 | 对异常信号做解释和打分 |
| 客服机器人 | 回答新手问题 |

## 4. 对 AI 写插件友好

- 插件开发采用 WIT 契约 + SDK + 示例 + 本地测试工具；
- 提供 `llms.txt` 和完整的开发文档，让 AI 能直接写出合格的插件。

长期来看，这一点对生态最有价值。

## 5. 事件日志作为数据源

只追加的事件日志可以用于站点运营报表、异常检测等 AI 分析。
