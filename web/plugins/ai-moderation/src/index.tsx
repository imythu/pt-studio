// ai-moderation 前端：后台页面（/admin/p/ai-moderation/）—— 各类别的举报数与准确率、最近的标记（按状态筛选，链接到举报工单）。
import { useEffect, useState } from 'react';
import type { PluginApi, PluginFrontend } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { PageHead, Panel, PLink, SkeletonRows, Status, Table, Tabs } from '@pt-studio/ui-kit';

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
  const [stats, setStats] = useState<Stat[] | null>(null);
  const [state, setState] = useState('');
  const [flags, setFlags] = useState<Flag[] | null>(null);
  useEffect(() => {
    void api
      .json<{ items: Stat[] }>('/api/p/ai-moderation/v1/admin/stats')
      .then((r) => setStats(r.items))
      .catch(() => setStats([]));
  }, [api]);
  useEffect(() => {
    setFlags(null);
    void api
      .json<{ items: Flag[] }>(`/api/p/ai-moderation/v1/admin/flags${state ? `?state=${state}` : ''}`)
      .then((r) => setFlags(r.items))
      .catch(() => setFlags([]));
  }, [api, state]);
  return (
    <div className="stack">
      <PageHead title={api.t('ai-moderation.ui.title')} />
      <Panel title={api.t('ai-moderation.ui.stats')} flush>
        {stats === null ? (
          <SkeletonRows n={3} />
        ) : (
          <Table
            rows={stats}
            rowKey={(r) => r.category}
            empty="—"
            columns={[
              { key: 'c', header: api.t('ai-moderation.ui.category'), render: (r) => <b>{r.category}</b> },
              { key: 'n', header: api.t('ai-moderation.ui.reported'), right: true, render: (r) => <span className="num">{r.reported}</span> },
              { key: 'o', header: '✓ / ✗', right: true, render: (r) => <span className="num"><span className="up">{r.true_positive}</span> / <span className="down">{r.false_positive}</span></span> },
              { key: 'p', header: api.t('ai-moderation.ui.precision'), right: true, render: (r) => <span className="num">{r.precision === null ? '—' : `${(r.precision * 100).toFixed(1)}%`}</span> },
            ]}
          />
        )}
      </Panel>
      <Panel title={api.t('ai-moderation.ui.flags')} flush primary>
        <Tabs variant="flush" value={state} onChange={setState} tabs={['', 'reported', 'review', 'dismissed'].map((s) => ({ id: s, label: s || api.t('ai-moderation.ui.all') }))} />
        {flags === null ? (
          <SkeletonRows n={4} />
        ) : (
          <Table
            rows={flags}
            rowKey={(r) => r.id}
            empty="—"
            minWidth={640}
            columns={[
              { key: 't', header: '', render: (r) => <span className="small muted num nowrap">{new Date(r.created_at).toLocaleString()}</span> },
              { key: 'g', header: '', render: (r) => <span className="pt-mono small">{r.target}</span> },
              { key: 'c', header: '', render: (r) => <>{r.max_category} {r.max_score !== null && <span className="tiny muted num">{r.max_score.toFixed(2)}</span>}</> },
              { key: 's', header: '', render: (r) => <Status tone={r.outcome === 'true_positive' ? 'bad' : r.outcome === 'false_positive' ? 'ok' : 'mute'}>{r.outcome ?? r.state}</Status> },
              {
                key: 'r',
                header: '',
                right: true,
                render: (r) =>
                  r.report_case_id ? (
                    <PLink to="/admin/p/moderation/" navigate={api.navigate}>
                      {api.t('ai-moderation.ui.case', { id: r.report_case_id })}
                    </PLink>
                  ) : null,
              },
            ]}
          />
        )}
      </Panel>
    </div>
  );
}

export default { slots: {}, routes: [{ path: '/admin/p/ai-moderation/*', component: Page }] } satisfies PluginFrontend;
