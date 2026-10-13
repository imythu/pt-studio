// moderation 前端：员工菜单的队列数、种子页侧栏的审核面板（状态、AI 结论、通过 / 拒绝 / 退回）、审核队列（/admin/p/moderation/）、
// 候选区投票（/p/moderation/candidates）。
import { useCallback, useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Empty, Input, Table } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'moderation.ui.menu': '审核队列（{n}）',
  'moderation.ui.queue': '审核队列',
  'moderation.ui.panel': '审核',
  'moderation.ui.state.pending': '待审核',
  'moderation.ui.state.candidate': '候选',
  'moderation.ui.state.awaiting_uploader': '等待上传者修改',
  'moderation.ui.ai': 'AI 预审：{v}',
  'moderation.ui.note': '说明（给上传者）',
  'moderation.ui.approve': '通过',
  'moderation.ui.reject': '拒绝',
  'moderation.ui.needs_edit': '退回修改',
  'moderation.ui.claim': '认领',
  'moderation.ui.done': '已处理：{d}',
  'moderation.ui.title': '标题',
  'moderation.ui.waiting': '等待（小时）',
  'moderation.ui.candidates': '候选区',
  'moderation.ui.up': '赞成（{n}）',
  'moderation.ui.down': '反对（{n}）',
});
registerMessages('en', {
  'moderation.ui.menu': 'Review queue ({n})',
  'moderation.ui.queue': 'Review queue',
  'moderation.ui.panel': 'Moderation',
  'moderation.ui.state.pending': 'Pending',
  'moderation.ui.state.candidate': 'Candidate',
  'moderation.ui.state.awaiting_uploader': 'Awaiting uploader',
  'moderation.ui.ai': 'AI review: {v}',
  'moderation.ui.note': 'Note to uploader',
  'moderation.ui.approve': 'Approve',
  'moderation.ui.reject': 'Reject',
  'moderation.ui.needs_edit': 'Needs edit',
  'moderation.ui.claim': 'Claim',
  'moderation.ui.done': 'Done: {d}',
  'moderation.ui.title': 'Title',
  'moderation.ui.waiting': 'Waiting (h)',
  'moderation.ui.candidates': 'Candidates',
  'moderation.ui.up': 'Up ({n})',
  'moderation.ui.down': 'Down ({n})',
});

function errText(api: PluginApi, e: unknown): string {
  if (e instanceof ApiError) {
    const s = api.t(e.code);
    return s === e.code ? e.message : s;
  }
  return String(e);
}

function link(api: PluginApi, to: string, text: string) {
  return (
    <a
      href={to}
      onClick={(e) => {
        e.preventDefault();
        api.navigate(to);
      }}
    >
      {text}
    </a>
  );
}

function StaffMenu({ api }: SlotProps) {
  const [n, setN] = useState<number | null>(null);
  useEffect(() => {
    void api
      .json<{ items: unknown[] }>('/api/p/moderation/v1/queue')
      .then((r) => setN(r.items.length))
      .catch(() => setN(null));
  }, [api]);
  if (n === null) return null;
  return link(api, '/admin/p/moderation/', api.t('moderation.ui.menu', { n }));
}

interface Item {
  state: string;
  ai_verdict: string | null;
  claimed_by: number | null;
}

function Panel({ api, ctx }: SlotProps<{ torrent: { id: number } }>) {
  const target = `torrent:${ctx.torrent.id}`;
  const [item, setItem] = useState<Item | null>(null);
  const [note, setNote] = useState('');
  const [done, setDone] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(async () => {
    try {
      setItem(await api.json<Item>(`/api/p/moderation/v1/queue/${target}`));
    } catch {
      setItem(null);
    }
  }, [api, target]);
  useEffect(() => {
    void load();
  }, [load]);
  async function decide(decision: string) {
    setErr(null);
    try {
      await api.json(`/api/p/moderation/v1/queue/${target}/decision`, { method: 'POST', json: { decision, note } });
      setDone(api.t(`moderation.ui.${decision}`));
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  async function claim() {
    try {
      await api.fetch(`/api/p/moderation/v1/queue/${target}/claim`, { method: 'POST', json: {} });
      await load();
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  if (!item) return null;
  return (
    <Card title={api.t('moderation.ui.panel')}>
      <p>
        <Badge tone="danger">{api.t(`moderation.ui.state.${item.state}`)}</Badge>{' '}
        {item.ai_verdict && <Badge tone={item.ai_verdict === 'fail' ? 'danger' : 'muted'}>{api.t('moderation.ui.ai', { v: item.ai_verdict })}</Badge>}
      </p>
      {done ? (
        <p>{api.t('moderation.ui.done', { d: done })}</p>
      ) : (
        <div className="pt-stack">
          {item.claimed_by === null && (
            <Button variant="ghost" onClick={() => void claim()}>
              {api.t('moderation.ui.claim')}
            </Button>
          )}
          <Input aria-label={api.t('moderation.ui.note')} placeholder={api.t('moderation.ui.note')} value={note} onChange={(e) => setNote(e.target.value)} />
          <span className="pt-row">
            <Button variant="primary" onClick={() => void decide('approve')}>
              {api.t('moderation.ui.approve')}
            </Button>
            <Button variant="danger" onClick={() => void decide('reject')}>
              {api.t('moderation.ui.reject')}
            </Button>
            <Button variant="ghost" onClick={() => void decide('needs_edit')}>
              {api.t('moderation.ui.needs_edit')}
            </Button>
          </span>
        </div>
      )}
      {err && <p className="pt-error">{err}</p>}
    </Card>
  );
}

interface QueueRow {
  target: string;
  state: string;
  title: string | null;
  ai_verdict: string | null;
  waiting_hours: number;
}

function Queue({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<QueueRow[]>([]);
  useEffect(() => {
    void api.json<{ items: QueueRow[] }>('/api/p/moderation/v1/queue').then((r) => setRows(r.items));
  }, [api]);
  return (
    <Card title={api.t('moderation.ui.queue')}>
      <Table
        rows={rows}
        rowKey={(r) => r.target}
        empty={<Empty>—</Empty>}
        columns={[
          { key: 't', header: api.t('moderation.ui.title'), render: (r) => link(api, `/torrents/${r.target.split(':')[1]}`, r.title ?? r.target) },
          { key: 's', header: '', render: (r) => api.t(`moderation.ui.state.${r.state}`) },
          { key: 'a', header: 'AI', render: (r) => r.ai_verdict ?? '—' },
          { key: 'w', header: api.t('moderation.ui.waiting'), render: (r) => r.waiting_hours },
        ]}
      />
    </Card>
  );
}

interface Candidate {
  torrent_id: number;
  title: string;
  up: number;
  down: number;
  my_vote: number | null;
}

function Candidates({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<Candidate[]>([]);
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(async () => setRows((await api.json<{ items: Candidate[] }>('/api/p/moderation/v1/candidates')).items), [api]);
  useEffect(() => {
    void load();
  }, [load]);
  async function vote(id: number, value: number) {
    try {
      await api.json(`/api/p/moderation/v1/candidates/${id}/vote`, { method: 'POST', json: { value } });
      await load();
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  return (
    <Card title={api.t('moderation.ui.candidates')}>
      {err && <p className="pt-error">{err}</p>}
      <Table
        rows={rows}
        rowKey={(r) => r.torrent_id}
        empty={<Empty>—</Empty>}
        columns={[
          { key: 't', header: api.t('moderation.ui.title'), render: (r) => link(api, `/torrents/${r.torrent_id}`, r.title) },
          {
            key: 'v',
            header: '',
            render: (r) => (
              <span className="pt-row">
                <Button variant={r.my_vote === 1 ? 'primary' : 'ghost'} onClick={() => void vote(r.torrent_id, 1)}>
                  {api.t('moderation.ui.up', { n: r.up })}
                </Button>
                <Button variant={r.my_vote === -1 ? 'danger' : 'ghost'} onClick={() => void vote(r.torrent_id, -1)}>
                  {api.t('moderation.ui.down', { n: r.down })}
                </Button>
              </span>
            ),
          },
        ]}
      />
    </Card>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  if (path.startsWith('/admin/p/moderation')) return <Queue api={api} />;
  return <Candidates api={api} />;
}

export default {
  slots: {
    'torrent.detail.sidebar': { component: Panel, order: 5 },
    'nav.user_menu': { component: StaffMenu, order: 81 },
  },
  routes: [
    { path: '/p/moderation/*', component: Pages },
    { path: '/admin/p/moderation/*', component: Pages },
  ],
} satisfies PluginFrontend;
