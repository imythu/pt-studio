# 06 · 前端

## 1. 可替换

- 核心 API 先行：用 utoipa 生成 OpenAPI 文档，再用 openapi-typescript 自动生成 TS SDK。
- 前端只依赖这个 SDK，因此整个前端都可以换掉。
- 官方前端只是参考实现。

## 2. 分层

```
web/
├─ sdk/       # 自动生成的 TS SDK
├─ ui-kit/    # 基础组件
├─ app/       # 页面与功能（features）
├─ theme/     # 主题变量
└─ slots/     # 插槽机制与插件加载器
```

## 3. 插槽（Slot）

```tsx
<Slot name="torrent.detail.sidebar" ctx={torrent} />
```

- 前端启动时拉取 `/api/kernel/v1/frontend/manifest`（内核路由统一为 `/api/kernel/v<N>/…`，插件路由为 `/api/p/<plugin_id>/v<N>/…`，见 [../platform/conventions/api.md](../platform/conventions/api.md)），再动态 `import()` 各插件的 ESM bundle（也可以用 Module Federation）。
- React 等共享依赖通过 import map 提供。
- 插件可以注册完整的页面路由，必须以 `/p/<plugin_id>/` 开头，例如 `/p/forum/*`。插槽清单见 [../platform/catalog/slots.md](../platform/catalog/slots.md)。

## 4. Schema 驱动 UI

简单插件不需要写 JS：插件返回 JSON Schema，前端自动生成设置页和表单。大多数插件的管理页面这样就够用。

以下内容也由 schema 驱动：

- 上传表单（资源类型插件的字段 schema）；
- 搜索筛选面板（合并后的索引 schema）。

## 5. 术语

所有站点相关的名称都通过 `t('term:…')` 获取，不在前端硬编码（见 [05-content-types-and-terms.md](05-content-types-and-terms.md)）。
