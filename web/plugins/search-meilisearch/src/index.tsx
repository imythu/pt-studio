// search-meilisearch 前端：后台页面（/admin/p/search-meilisearch/）—— 各索引状态、文档数、待同步队列、冲突；手动重建与进度、取消。
import { useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Progress, Table } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'search-meilisearch.ui.title': '搜索索引',
  'search-meilisearch.ui.index': '索引',
  'search-meilisearch.ui.status': '状态',
  'search-meilisearch.ui.documents': '文档数',
  'search-meilisearch.ui.pending': '待同步（普通 / 优先）',
  'search-meilisearch.ui.lag': '最旧 {s} 秒',
  'search-meilisearch.ui.conflicts': '字段冲突',
  'search-meilisearch.ui.rebuild': '重建',
  'search-meilisearch.ui.rebuild_confirm': '重建会在后台建立新索引并在完成后切换，期间搜索照常可用。继续？',
  'search-meilisearch.ui.cancel': '取消重建',
  'search-meilisearch.ui.progress': '重建 {index}：{state}',
});
registerMessages('en', {
  'search-meilisearch.ui.title': 'Search indexes',
  'search-meilisearch.ui.index': 'Index',
  'search-meilisearch.ui.status': 'Status',
  'search-meilisearch.ui.documents': 'Documents',
  'search-meilisearch.ui.pending': 'Pending (normal / priority)',
  'search-meilisearch.ui.lag': 'oldest {s}s',
  'search-meilisearch.ui.conflicts': 'Field conflicts',
  'search-meilisearch.ui.rebuild': 'Rebuild',
  'search-meilisearch.ui.rebuild_confirm': 'A rebuild builds a new index in the background and swaps it in when done; search keeps working. Continue?',
  'search-meilisearch.ui.cancel': 'Cancel rebuild',
  'search-meilisearch.ui.progress': 'Rebuilding {index}: {state}',
});

interface Index {
  index: string;
  owner: string;
  status: string;
  documents: number | null;
  pending_normal: number;
  pending_priority: number;
  oldest_pending_age_ms: number;
  conflicts: unknown[] | null;
}
interface Rebuild {
  id: string;
  index: string;
  state: string;
  total: number | null;
  done: number;
  error: string | null;
}

const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e);
const RUNNING = ['preparing', 'building', 'replaying', 'verifying', 'swapping', 'cleanup', 'cancelling'];

function Page({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<Index[]>([]);
  const [rb, setRb] = useState<Rebuild | null>(null);
  const load = () => void api.json<{ items: Index[] }>('/api/p/search-meilisearch/v1/admin/indexes').then((r) => setRows(r.items));
  useEffect(load, []); // eslint-disable-line react-hooks/exhaustive-deps
  // 重建进度：运行中每 2 秒刷新
  useEffect(() => {
    if (!rb || !RUNNING.includes(rb.state)) return;
    const t = setTimeout(() => {
      void api.json<Rebuild>(`/api/p/search-meilisearch/v1/admin/rebuilds/${rb.id}`).then((r) => {
        setRb(r);
        if (!RUNNING.includes(r.state)) load();
      });
    }, 2000);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rb]);
  const rebuild = async (index: string) => {
    if (!window.confirm(api.t('search-meilisearch.ui.rebuild_confirm'))) return;
    try {
      const r = await api.json<{ id: string; state: string }>(`/api/p/search-meilisearch/v1/admin/indexes/${index}/rebuilds`, { json: { reason: 'admin' } });
      setRb({ id: r.id, index, state: r.state, total: null, done: 0, error: null });
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t('search-meilisearch.ui.title')}</h1>
      {rb && (
        <Card>
          <p>
            {api.t('search-meilisearch.ui.progress', { index: rb.index, state: rb.state })} {rb.error && <span className="pt-field-error">{rb.error}</span>}
          </p>
          {rb.total ? <Progress value={rb.done} max={rb.total} /> : null}
          {RUNNING.includes(rb.state) && (
            <Button variant="ghost" onClick={() => void api.json(`/api/p/search-meilisearch/v1/admin/rebuilds/${rb.id}/cancellations`, { json: {} }).then(() => setRb({ ...rb, state: 'cancelling' }))}>
              {api.t('search-meilisearch.ui.cancel')}
            </Button>
          )}
        </Card>
      )}
      <Card>
        <Table
          rows={rows}
          rowKey={(r) => r.index}
          columns={[
            { key: 'i', header: api.t('search-meilisearch.ui.index'), render: (r) => <><strong>{r.index}</strong> <span className="pt-muted small">{r.owner}</span></> },
            { key: 's', header: api.t('search-meilisearch.ui.status'), render: (r) => <Badge tone={r.status === 'ready' ? 'success' : 'danger'}>{r.status}</Badge> },
            { key: 'd', header: api.t('search-meilisearch.ui.documents'), render: (r) => r.documents ?? '—' },
            {
              key: 'p',
              header: api.t('search-meilisearch.ui.pending'),
              render: (r) => (
                <>
                  {r.pending_normal} / {r.pending_priority}{' '}
                  {r.oldest_pending_age_ms > 0 && <span className="pt-muted small">{api.t('search-meilisearch.ui.lag', { s: Math.round(r.oldest_pending_age_ms / 1000) })}</span>}
                </>
              ),
            },
            { key: 'c', header: api.t('search-meilisearch.ui.conflicts'), render: (r) => (r.conflicts?.length ? <Badge tone="danger">{r.conflicts.length}</Badge> : '—') },
            {
              key: 'x',
              header: '',
              render: (r) => (
                <Button variant="ghost" disabled={!!rb && RUNNING.includes(rb.state)} onClick={() => void rebuild(r.index)}>
                  {api.t('search-meilisearch.ui.rebuild')}
                </Button>
              ),
            },
          ]}
        />
      </Card>
    </div>
  );
}

export default { slots: {}, routes: [{ path: '/admin/p/search-meilisearch/*', component: Page }] } satisfies PluginFrontend;
