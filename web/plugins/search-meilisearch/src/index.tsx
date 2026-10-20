// search-meilisearch 前端：后台页面（/admin/p/search-meilisearch/）—— 各索引状态、文档数、待同步队列、冲突；手动重建（抽屉确认）与进度、取消。
import { useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Note, PageHead, Panel, Progress, Sheet, SkeletonRows, Status, Table } from '@pt-studio/ui-kit';

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
  'search-meilisearch.ui.close': '取消',
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
  'search-meilisearch.ui.close': 'Cancel',
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
  const [rows, setRows] = useState<Index[] | null>(null);
  const [rb, setRb] = useState<Rebuild | null>(null);
  const [ask, setAsk] = useState<string | null>(null);
  const load = () =>
    void api
      .json<{ items: Index[] }>('/api/p/search-meilisearch/v1/admin/indexes')
      .then((r) => setRows(r.items))
      .catch(() => setRows([]));
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
    setAsk(null);
    try {
      const r = await api.json<{ id: string; state: string }>(`/api/p/search-meilisearch/v1/admin/indexes/${index}/rebuilds`, { json: { reason: 'admin' } });
      setRb({ id: r.id, index, state: r.state, total: null, done: 0, error: null });
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const running = !!rb && RUNNING.includes(rb.state);
  return (
    <div className="stack">
      <PageHead title={api.t('search-meilisearch.ui.title')} />
      {rb && (
        <Panel
          title={api.t('search-meilisearch.ui.progress', { index: rb.index, state: rb.state })}
          actions={
            running && (
              <Button size="sm" variant="ghost" onClick={() => void api.json(`/api/p/search-meilisearch/v1/admin/rebuilds/${rb.id}/cancellations`, { json: {} }).then(() => setRb({ ...rb, state: 'cancelling' }))}>
                {api.t('search-meilisearch.ui.cancel')}
              </Button>
            )
          }
        >
          <div className="stack-s">
            {rb.total ? (
              <>
                <Progress value={rb.done} max={rb.total} label={rb.index} />
                <span className="tiny muted num">
                  {rb.done.toLocaleString()} / {rb.total.toLocaleString()}
                </span>
              </>
            ) : null}
            {rb.error && <Note tone="bad">{rb.error}</Note>}
          </div>
        </Panel>
      )}
      <Panel flush primary>
        {rows === null ? (
          <SkeletonRows n={3} />
        ) : (
          <Table
            rows={rows}
            rowKey={(r) => r.index}
            minWidth={820}
            columns={[
              { key: 'i', header: api.t('search-meilisearch.ui.index'), render: (r) => <><b>{r.index}</b> <span className="tiny muted">{r.owner}</span></> },
              { key: 's', header: api.t('search-meilisearch.ui.status'), render: (r) => <Status tone={r.status === 'ready' ? 'ok' : 'bad'}>{r.status}</Status> },
              { key: 'd', header: api.t('search-meilisearch.ui.documents'), right: true, render: (r) => <span className="num">{r.documents?.toLocaleString() ?? '—'}</span> },
              {
                key: 'p',
                header: api.t('search-meilisearch.ui.pending'),
                right: true,
                render: (r) => (
                  <span className="num">
                    {r.pending_normal} / {r.pending_priority}
                    {r.oldest_pending_age_ms > 0 && <div className="tiny muted">{api.t('search-meilisearch.ui.lag', { s: Math.round(r.oldest_pending_age_ms / 1000) })}</div>}
                  </span>
                ),
              },
              { key: 'c', header: api.t('search-meilisearch.ui.conflicts'), right: true, render: (r) => (r.conflicts?.length ? <Status tone="bad">{r.conflicts.length}</Status> : '—') },
              {
                key: 'x',
                header: '',
                right: true,
                render: (r) => (
                  <Button size="sm" variant="ghost" disabled={running} onClick={() => setAsk(r.index)}>
                    {api.t('search-meilisearch.ui.rebuild')}
                  </Button>
                ),
              },
            ]}
          />
        )}
      </Panel>
      <Sheet
        open={ask !== null}
        onClose={() => setAsk(null)}
        title={`${api.t('search-meilisearch.ui.rebuild')} · ${ask ?? ''}`}
        footer={
          <>
            <Button variant="ghost" onClick={() => setAsk(null)}>
              {api.t('search-meilisearch.ui.close')}
            </Button>
            <Button variant="danger" onClick={() => ask && void rebuild(ask)}>
              {api.t('search-meilisearch.ui.rebuild')}
            </Button>
          </>
        }
      >
        <p>{api.t('search-meilisearch.ui.rebuild_confirm')}</p>
      </Sheet>
    </div>
  );
}

export default { slots: {}, routes: [{ path: '/admin/p/search-meilisearch/*', component: Page }] } satisfies PluginFrontend;
