// ai-moderation 前端：后台页面（/admin/p/ai-moderation/）—— 各类别的举报数与准确率、最近的标记（按状态筛选，链接到举报工单）。
import { useEffect, useState } from 'react';
import type { PluginApi, PluginFrontend } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Card, Table, Tabs } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'ai-moderation.ui.title': 'AI 内容审核',
  'ai-moderation.ui.stats': '各类别准确率（按管理员处理结果）',
  'ai-moderation.ui.category': '类别',
  'ai-moderation.ui.reported': '举报数',
  'ai-moderation.ui.precision': '准确率',
  'ai-moderation.ui.flags': '最近的标记',
  'ai-moderation.ui.all': '全部',
  'ai-moderation.ui.case': '工单 #{id}',
});
registerMessages('en', {
  'ai-moderation.ui.title': 'AI moderation',
  'ai-moderation.ui.stats': 'Precision by category (from staff outcomes)',
  'ai-moderation.ui.category': 'Category',
  'ai-moderation.ui.reported': 'Reported',
  'ai-moderation.ui.precision': 'Precision',
  'ai-moderation.ui.flags': 'Recent flags',
  'ai-moderation.ui.all': 'All',
  'ai-moderation.ui.case': 'Case #{id}',
});

interface Stat {
  category: string;
  reported: number;
  true_positive: number;
  false_positive: number;
  precision: number | null;
}
interface Flag {
  id: number;
  target: string;
  max_category: string;
  max_score: number | null;
  source: string;
  state: string;
  report_case_id: number | null;
  outcome: string | null;
  created_at: number;
}

function Page({ api }: { api: PluginApi }) {
  const [stats, setStats] = useState<Stat[]>([]);
  const [state, setState] = useState('');
  const [flags, setFlags] = useState<Flag[]>([]);
  useEffect(() => {
    void api.json<{ items: Stat[] }>('/api/p/ai-moderation/v1/admin/stats').then((r) => setStats(r.items));
  }, [api]);
  useEffect(() => {
    void api.json<{ items: Flag[] }>(`/api/p/ai-moderation/v1/admin/flags${state ? `?state=${state}` : ''}`).then((r) => setFlags(r.items));
  }, [api, state]);
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t('ai-moderation.ui.title')}</h1>
      <Card title={api.t('ai-moderation.ui.stats')}>
        <Table
          rows={stats}
          rowKey={(r) => r.category}
          columns={[
            { key: 'c', header: api.t('ai-moderation.ui.category'), render: (r) => r.category },
            { key: 'n', header: api.t('ai-moderation.ui.reported'), render: (r) => r.reported },
            { key: 'o', header: '✓ / ✗', render: (r) => `${r.true_positive} / ${r.false_positive}` },
            { key: 'p', header: api.t('ai-moderation.ui.precision'), render: (r) => (r.precision === null ? '—' : `${(r.precision * 100).toFixed(1)}%`) },
          ]}
        />
      </Card>
      <Card title={api.t('ai-moderation.ui.flags')}>
        <Tabs
          value={state}
          onChange={setState}
          tabs={['', 'reported', 'review', 'dismissed'].map((s) => ({ id: s, label: s || api.t('ai-moderation.ui.all') }))}
        />
        <Table
          rows={flags}
          rowKey={(r) => r.id}
          columns={[
            { key: 't', header: '', render: (r) => <span className="pt-muted small">{new Date(r.created_at).toLocaleString()}</span> },
            { key: 'g', header: '', render: (r) => <code>{r.target}</code> },
            { key: 'c', header: '', render: (r) => <>{r.max_category} {r.max_score !== null && <span className="pt-muted small">{r.max_score.toFixed(2)}</span>}</> },
            { key: 's', header: '', render: (r) => <Badge tone={r.outcome === 'true_positive' ? 'danger' : 'muted'}>{r.outcome ?? r.state}</Badge> },
            {
              key: 'r',
              header: '',
              render: (r) =>
                r.report_case_id ? (
                  <a href="/admin/p/moderation/" onClick={(e) => (e.preventDefault(), api.navigate('/admin/p/moderation/'))}>
                    {api.t('ai-moderation.ui.case', { id: r.report_case_id })}
                  </a>
                ) : null,
            },
          ]}
        />
      </Card>
    </div>
  );
}

export default { slots: {}, routes: [{ path: '/admin/p/ai-moderation/*', component: Page }] } satisfies PluginFrontend;
