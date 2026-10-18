// llm-claude 前端：后台页面（/admin/p/llm-claude/）—— 用量统计（按插件 / 模型 / 日期，默认近 30 天）与连通性测试。
import { useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Card, Table, Tabs } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'llm-claude.ui.title': 'Claude 模型',
  'llm-claude.ui.usage': '近 30 天用量',
  'llm-claude.ui.by_plugin': '按插件',
  'llm-claude.ui.by_model': '按模型',
  'llm-claude.ui.by_day': '按日期',
  'llm-claude.ui.calls': '调用',
  'llm-claude.ui.input': '输入 token',
  'llm-claude.ui.cached': '其中缓存',
  'llm-claude.ui.output': '输出 token',
  'llm-claude.ui.refusals': '拒绝',
  'llm-claude.ui.test': '连通性测试',
  'llm-claude.ui.test_ok': '{model} 回复「{text}」，耗时 {ms} ms',
});
registerMessages('en', {
  'llm-claude.ui.title': 'Claude models',
  'llm-claude.ui.usage': 'Usage, last 30 days',
  'llm-claude.ui.by_plugin': 'By plugin',
  'llm-claude.ui.by_model': 'By model',
  'llm-claude.ui.by_day': 'By day',
  'llm-claude.ui.calls': 'Calls',
  'llm-claude.ui.input': 'Input tokens',
  'llm-claude.ui.cached': 'Cached',
  'llm-claude.ui.output': 'Output tokens',
  'llm-claude.ui.refusals': 'Refusals',
  'llm-claude.ui.test': 'Connectivity test',
  'llm-claude.ui.test_ok': '{model} replied "{text}" in {ms} ms',
});

interface Row {
  key: string;
  calls: number;
  input_tokens: number;
  output_tokens: number;
  cached_input_tokens: number;
  refusals: number;
}

const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e);
const n = (v: number) => v.toLocaleString();

function Page({ api }: { api: PluginApi }) {
  const [by, setBy] = useState('plugin');
  const [rows, setRows] = useState<Row[]>([]);
  const [result, setResult] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    void api.json<{ items: Row[] }>(`/api/p/llm-claude/v1/admin/usage?group_by=${by}`).then((r) => setRows(r.items));
  }, [api, by]);
  const test = async () => {
    setBusy(true);
    try {
      const r = await api.json<{ model: string; text: string; latency_ms: number }>('/api/p/llm-claude/v1/admin/test', { json: {} });
      setResult(api.t('llm-claude.ui.test_ok', { model: r.model, text: r.text, ms: r.latency_ms }));
    } catch (e) {
      setResult(errText(api, e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t('llm-claude.ui.title')}</h1>
      <Card title={api.t('llm-claude.ui.usage')}>
        <Tabs
          value={by}
          onChange={setBy}
          tabs={[
            { id: 'plugin', label: api.t('llm-claude.ui.by_plugin') },
            { id: 'model', label: api.t('llm-claude.ui.by_model') },
            { id: 'day', label: api.t('llm-claude.ui.by_day') },
          ]}
        />
        <Table
          rows={rows}
          rowKey={(r) => r.key}
          columns={[
            { key: 'k', header: '', render: (r) => <code>{r.key}</code> },
            { key: 'c', header: api.t('llm-claude.ui.calls'), render: (r) => n(r.calls) },
            { key: 'i', header: api.t('llm-claude.ui.input'), render: (r) => <>{n(r.input_tokens)} <span className="pt-muted small">({api.t('llm-claude.ui.cached')} {n(r.cached_input_tokens)})</span></> },
            { key: 'o', header: api.t('llm-claude.ui.output'), render: (r) => n(r.output_tokens) },
            { key: 'r', header: api.t('llm-claude.ui.refusals'), render: (r) => n(r.refusals) },
          ]}
        />
      </Card>
      <Card title={api.t('llm-claude.ui.test')}>
        <Button onClick={() => void test()} disabled={busy}>
          {api.t('llm-claude.ui.test')}
        </Button>
        {result && <p>{result}</p>}
      </Card>
    </div>
  );
}

export default { slots: {}, routes: [{ path: '/admin/p/llm-claude/*', component: Page }] } satisfies PluginFrontend;
