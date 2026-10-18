// moderation 前端：员工菜单的队列数、种子页侧栏的审核面板（状态、AI 结论、认领 / 释放、通过 / 拒绝 / 退回）、
// 后台（/admin/p/moderation/：审核队列、审核记录、敏感词、敏感词命中）、候选区投票（/p/moderation/candidates）。
import { useCallback, useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Empty, Field, Input, Table, Tabs } from '@pt-studio/ui-kit';

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
  'moderation.ui.release': '释放认领',
  'moderation.ui.claimed_by': '已被 #{id} 认领',
  'moderation.ui.history': '审核记录',
  'moderation.ui.decision': '结论',
  'moderation.ui.moderator': '审核员',
  'moderation.ui.time': '时间',
  'moderation.ui.filter_target': '对象（如 torrent:123）',
  'moderation.ui.filter_moderator': '审核员 ID',
  'moderation.ui.filter': '筛选',
  'moderation.ui.words': '敏感词',
  'moderation.ui.words_hint': '命中时：拦截（不允许发布）、打码（替换为 *）或送审（照常发布并记录命中）。目前对评论生效。',
  'moderation.ui.pattern': '词语',
  'moderation.ui.action': '处理方式',
  'moderation.ui.action.block': '拦截',
  'moderation.ui.action.mask': '打码',
  'moderation.ui.action.review': '送审',
  'moderation.ui.add': '添加',
  'moderation.ui.delete': '删除',
  'moderation.ui.inactive': '已停用',
  'moderation.ui.hits': '命中记录',
  'moderation.ui.scope': '位置',
  'moderation.ui.author': '作者',
  'moderation.ui.excerpt': '片段',
  'moderation.ui.saved': '已保存',
  'moderation.ui.scope.comment': '评论',
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
  'moderation.ui.release': 'Release claim',
  'moderation.ui.claimed_by': 'Claimed by #{id}',
  'moderation.ui.history': 'Decisions',
  'moderation.ui.decision': 'Decision',
  'moderation.ui.moderator': 'Moderator',
  'moderation.ui.time': 'Time',
  'moderation.ui.filter_target': 'Target (e.g. torrent:123)',
  'moderation.ui.filter_moderator': 'Moderator ID',
  'moderation.ui.filter': 'Filter',
  'moderation.ui.words': 'Word filter',
  'moderation.ui.words_hint': 'On a match: block (reject the text), mask (replace with *) or review (publish and record the hit). Applies to comments for now.',
  'moderation.ui.pattern': 'Word',
  'moderation.ui.action': 'Action',
  'moderation.ui.action.block': 'Block',
  'moderation.ui.action.mask': 'Mask',
  'moderation.ui.action.review': 'Review',
  'moderation.ui.add': 'Add',
  'moderation.ui.delete': 'Delete',
  'moderation.ui.inactive': 'inactive',
  'moderation.ui.hits': 'Word hits',
  'moderation.ui.scope': 'Where',
  'moderation.ui.author': 'Author',
  'moderation.ui.excerpt': 'Excerpt',
  'moderation.ui.saved': 'Saved',
  'moderation.ui.scope.comment': 'Comment',
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
  async function claim(release = false) {
    try {
      await api.json(`/api/p/moderation/v1/queue/${target}/claim`, release ? { method: 'DELETE' } : { method: 'POST', json: {} });
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
          {item.claimed_by === null ? (
            <Button variant="ghost" onClick={() => void claim()}>
              {api.t('moderation.ui.claim')}
            </Button>
          ) : item.claimed_by === api.user()?.id ? (
            <Button variant="ghost" onClick={() => void claim(true)}>
              {api.t('moderation.ui.release')}
            </Button>
          ) : (
            <span className="pt-muted small">{api.t('moderation.ui.claimed_by', { id: item.claimed_by })}</span>
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

interface Decision {
  target: string;
  decision: string;
  note: string | null;
  moderator_id: number | null;
  actor: string | null;
  created_at: number;
}

function History({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<Decision[]>([]);
  const [f, setF] = useState({ target: '', moderator: '' });
  const load = useCallback(
    async (target: string, moderator: string) => {
      const q = new URLSearchParams();
      if (target.trim()) q.set('target', target.trim());
      if (moderator.trim()) q.set('moderator_id', moderator.trim());
      try {
        setRows((await api.json<{ items: Decision[] }>(`/api/p/moderation/v1/history?${q.toString()}`)).items);
      } catch (e) {
        api.toast(errText(api, e), 'error');
      }
    },
    [api],
  );
  useEffect(() => {
    void load('', '');
  }, [load]);
  const targetLink = (t: string) => (t.startsWith('torrent:') ? link(api, `/torrents/${t.slice(8)}`, t) : t);
  return (
    <Card title={api.t('moderation.ui.history')}>
      <form
        className="searchbar"
        onSubmit={(e) => {
          e.preventDefault();
          void load(f.target, f.moderator);
        }}
      >
        <Input name="history_target" aria-label={api.t('moderation.ui.filter_target')} placeholder={api.t('moderation.ui.filter_target')} value={f.target} onChange={(e) => setF({ ...f, target: e.target.value })} />
        <Input name="history_moderator" type="number" aria-label={api.t('moderation.ui.filter_moderator')} placeholder={api.t('moderation.ui.filter_moderator')} value={f.moderator} onChange={(e) => setF({ ...f, moderator: e.target.value })} />
        <Button type="submit">{api.t('moderation.ui.filter')}</Button>
      </form>
      <Table
        rows={rows}
        rowKey={(r) => `${r.target}:${r.created_at}:${r.decision}`}
        empty={<Empty>—</Empty>}
        columns={[
          { key: 't', header: api.t('moderation.ui.time'), render: (r) => <span className="pt-muted small">{new Date(r.created_at).toLocaleString()}</span> },
          { key: 'o', header: api.t('moderation.ui.title'), render: (r) => targetLink(r.target) },
          {
            key: 'd',
            header: api.t('moderation.ui.decision'),
            render: (r) => (
              <>
                <Badge tone={r.decision === 'approve' ? 'success' : r.decision === 'reject' ? 'danger' : 'muted'}>{api.t(`moderation.ui.${r.decision}`)}</Badge>
                {r.note && <div className="pt-muted small">{r.note}</div>}
              </>
            ),
          },
          { key: 'm', header: api.t('moderation.ui.moderator'), render: (r) => (r.moderator_id ? link(api, `/users/${r.moderator_id}`, `#${r.moderator_id}`) : (r.actor ?? '—')) },
        ]}
      />
    </Card>
  );
}

interface Word {
  id: number;
  pattern: string;
  action: string;
  scopes: string;
  active: boolean;
}

function Words({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<Word[]>([]);
  const [f, setF] = useState({ pattern: '', action: 'review' });
  const load = useCallback(async () => setRows((await api.json<{ items: Word[] }>('/api/p/moderation/v1/admin/words')).items), [api]);
  useEffect(() => {
    void load().catch(() => undefined);
  }, [load]);
  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      api.toast(api.t('moderation.ui.saved'), 'success');
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <Card title={api.t('moderation.ui.words')}>
      <p className="pt-muted small">{api.t('moderation.ui.words_hint')}</p>
      <form
        className="upload-grid"
        onSubmit={(e) => {
          e.preventDefault();
          void act(async () => {
            await api.json('/api/p/moderation/v1/admin/words', { json: { pattern: f.pattern.trim(), action: f.action, scopes: ['comment'] } });
            setF({ ...f, pattern: '' });
          });
        }}
      >
        <Field label={api.t('moderation.ui.pattern')}>
          <Input name="word_pattern" required maxLength={100} value={f.pattern} onChange={(e) => setF({ ...f, pattern: e.target.value })} />
        </Field>
        <Field label={api.t('moderation.ui.action')}>
          <select className="pt-input" name="word_action" value={f.action} onChange={(e) => setF({ ...f, action: e.target.value })}>
            {['block', 'mask', 'review'].map((a) => (
              <option key={a} value={a}>
                {api.t(`moderation.ui.action.${a}`)}
              </option>
            ))}
          </select>
        </Field>
        <div className="span2">
          <Button type="submit" variant="primary" disabled={!f.pattern.trim()}>
            {api.t('moderation.ui.add')}
          </Button>
        </div>
      </form>
      <Table
        rows={rows}
        rowKey={(r) => r.id}
        empty={<Empty>—</Empty>}
        columns={[
          { key: 'p', header: api.t('moderation.ui.pattern'), render: (r) => <code>{r.pattern}</code> },
          { key: 'a', header: api.t('moderation.ui.action'), render: (r) => <Badge tone={r.action === 'block' ? 'danger' : 'muted'}>{api.t(`moderation.ui.action.${r.action}`)}</Badge> },
          { key: 's', header: api.t('moderation.ui.scope'), render: (r) => <span className="pt-muted small">{r.scopes.replace(/[{}]/g, '')}</span> },
          {
            key: 'x',
            header: '',
            render: (r) => (
              <>
                {!r.active && <Badge>{api.t('moderation.ui.inactive')}</Badge>}{' '}
                <Button variant="ghost" onClick={() => void act(() => api.json(`/api/p/moderation/v1/admin/words/${r.id}`, { method: 'DELETE' }))}>
                  {api.t('moderation.ui.delete')}
                </Button>
              </>
            ),
          },
        ]}
      />
    </Card>
  );
}

function Hits({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<{ id: number; scope: string; author_id: number; excerpt: string; created_at: number }[]>([]);
  useEffect(() => {
    void api
      .json<{ items: typeof rows }>('/api/p/moderation/v1/admin/word-hits')
      .then((r) => setRows(r.items))
      .catch(() => undefined);
  }, [api]);
  return (
    <Card title={api.t('moderation.ui.hits')}>
      <Table
        rows={rows}
        rowKey={(r) => r.id}
        empty={<Empty>—</Empty>}
        columns={[
          { key: 't', header: api.t('moderation.ui.time'), render: (r) => <span className="pt-muted small">{new Date(r.created_at).toLocaleString()}</span> },
          { key: 's', header: api.t('moderation.ui.scope'), render: (r) => (api.t(`moderation.ui.scope.${r.scope}`) === `moderation.ui.scope.${r.scope}` ? r.scope : api.t(`moderation.ui.scope.${r.scope}`)) },
          { key: 'a', header: api.t('moderation.ui.author'), render: (r) => link(api, `/users/${r.author_id}`, `#${r.author_id}`) },
          { key: 'e', header: api.t('moderation.ui.excerpt'), render: (r) => <span className="pt-prewrap">{r.excerpt}</span> },
        ]}
      />
    </Card>
  );
}

function Admin({ api }: { api: PluginApi }) {
  const [tab, setTab] = useState('queue');
  return (
    <div className="pt-stack">
      <Tabs
        value={tab}
        onChange={setTab}
        tabs={['queue', 'history', 'words', 'hits'].map((id) => ({ id, label: api.t(`moderation.ui.${id}`) }))}
      />
      {tab === 'queue' ? <Queue api={api} /> : tab === 'history' ? <History api={api} /> : tab === 'words' ? <Words api={api} /> : <Hits api={api} />}
    </div>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  if (path.startsWith('/admin/p/moderation')) return <Admin api={api} />;
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
