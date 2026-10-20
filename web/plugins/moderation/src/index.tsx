// moderation 前端：员工导航的队列数、种子页侧栏的审核面板（状态、AI 结论、认领 / 释放、通过 / 拒绝 / 退回）、
// 后台（/admin/p/moderation/：审核队列、审核记录、敏感词、敏感词命中）、候选区投票（/p/moderation/candidates）。
import { useCallback, useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Empty, Field, FilterBar, Input, NavItem, Note, PageHead, Panel, PLink, Select, Sheet, SkeletonRows, Status, Table, Tabs, Tag, Textarea, type Tone } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'moderation.ui.menu': '审核队列',
  'moderation.ui.lede': '在种子页右侧的审核面板里处理；这里列出所有待处理项。',
  'moderation.ui.candidates_lede': '候选区的种子由会员投票决定是否发布。',
  'moderation.ui.new_word': '添加敏感词',
  'moderation.ui.open': '去审核',
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
  'moderation.ui.menu': 'Review queue',
  'moderation.ui.lede': 'Review from the moderation panel on each torrent page; this lists everything waiting.',
  'moderation.ui.candidates_lede': 'Members vote on candidate torrents to decide whether they are published.',
  'moderation.ui.new_word': 'Add a word',
  'moderation.ui.open': 'Review',
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

const VERDICT_TONE: Record<string, Tone> = { pass: 'ok', warn: 'warn', fail: 'bad' };
const DECISION_TONE: Record<string, Tone> = { approve: 'ok', reject: 'bad', needs_edit: 'warn' };

function StaffMenu({ api }: SlotProps) {
  const [n, setN] = useState<number | null>(null);
  useEffect(() => {
    void api
      .json<{ items: unknown[] }>('/api/p/moderation/v1/queue')
      .then((r) => setN(r.items.length))
      .catch(() => setN(null));
  }, [api]);
  if (n === null) return null;
  return (
    <NavItem to="/admin/p/moderation/" navigate={api.navigate} count={n} alert>
      {api.t('moderation.ui.menu')}
    </NavItem>
  );
}

interface Item {
  state: string;
  ai_verdict: string | null;
  claimed_by: number | null;
}

/** 种子页侧栏：审核面板（只有待审核的种子、且有审核权限时显示） */
function ModPanel({ api, ctx }: SlotProps<{ torrent: { id: number } }>) {
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
    <Panel
      title={api.t('moderation.ui.panel')}
      className="mod-panel"
      actions={
        !done &&
        (item.claimed_by === null ? (
          <Button size="xs" onClick={() => void claim()}>
            {api.t('moderation.ui.claim')}
          </Button>
        ) : item.claimed_by === api.user()?.id ? (
          <Button size="xs" variant="ghost" onClick={() => void claim(true)}>
            {api.t('moderation.ui.release')}
          </Button>
        ) : undefined)
      }
    >
      <div className="stack-s">
        <div className="row" style={{ gap: 6 }}>
          <Status tone="warn">{api.t(`moderation.ui.state.${item.state}`)}</Status>
          {item.ai_verdict && <Status tone={VERDICT_TONE[item.ai_verdict] ?? 'mute'}>{api.t('moderation.ui.ai', { v: item.ai_verdict })}</Status>}
        </div>
        {item.claimed_by !== null && item.claimed_by !== api.user()?.id && <span className="small muted">{api.t('moderation.ui.claimed_by', { id: item.claimed_by })}</span>}
        {done ? (
          <Note tone="ok">{api.t('moderation.ui.done', { d: done })}</Note>
        ) : (
          <>
            <Textarea aria-label={api.t('moderation.ui.note')} placeholder={api.t('moderation.ui.note')} rows={3} value={note} onChange={(e) => setNote(e.target.value)} />
            <div className="row">
              <Button size="sm" variant="primary" onClick={() => void decide('approve')}>
                {api.t('moderation.ui.approve')}
              </Button>
              <Button size="sm" onClick={() => void decide('needs_edit')}>
                {api.t('moderation.ui.needs_edit')}
              </Button>
              <Button size="sm" variant="danger" onClick={() => void decide('reject')}>
                {api.t('moderation.ui.reject')}
              </Button>
            </div>
          </>
        )}
        {err && <Note tone="bad">{err}</Note>}
      </div>
    </Panel>
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
  const [rows, setRows] = useState<QueueRow[] | null>(null);
  useEffect(() => {
    void api.json<{ items: QueueRow[] }>('/api/p/moderation/v1/queue').then((r) => setRows(r.items));
  }, [api]);
  if (rows === null) return <SkeletonRows n={5} />;
  return (
    <Table
      rows={rows}
      rowKey={(r) => r.target}
      empty={<Empty>{api.t('moderation.ui.queue')} · 0</Empty>}
      minWidth={640}
      onRowClick={(r) => api.navigate(`/torrents/${r.target.split(':')[1]}`)}
      columns={[
        { key: 't', header: api.t('moderation.ui.title'), render: (r) => <PLink to={`/torrents/${r.target.split(':')[1]}`} navigate={api.navigate} className="tt-name">{r.title ?? r.target}</PLink> },
        { key: 's', header: '', render: (r) => <Status tone="warn">{api.t(`moderation.ui.state.${r.state}`)}</Status> },
        { key: 'a', header: 'AI', render: (r) => (r.ai_verdict ? <Status tone={VERDICT_TONE[r.ai_verdict] ?? 'mute'}>{r.ai_verdict}</Status> : <span className="muted">—</span>) },
        { key: 'w', header: api.t('moderation.ui.waiting'), right: true, render: (r) => <span className={r.waiting_hours >= 24 ? 'num down' : 'num'}>{r.waiting_hours}</span> },
      ]}
    />
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
  const [rows, setRows] = useState<Candidate[] | null>(null);
  const load = useCallback(async () => setRows((await api.json<{ items: Candidate[] }>('/api/p/moderation/v1/candidates')).items), [api]);
  useEffect(() => {
    void load();
  }, [load]);
  async function vote(id: number, value: number) {
    try {
      await api.json(`/api/p/moderation/v1/candidates/${id}/vote`, { method: 'POST', json: { value } });
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  }
  return (
    <div className="stack">
      <PageHead title={api.t('moderation.ui.candidates')} lede={api.t('moderation.ui.candidates_lede')} />
      <Panel flush primary>
        {rows === null ? (
          <SkeletonRows n={4} />
        ) : (
          <Table
            rows={rows}
            rowKey={(r) => r.torrent_id}
            empty={<Empty>—</Empty>}
            columns={[
              { key: 't', header: api.t('moderation.ui.title'), render: (r) => <PLink to={`/torrents/${r.torrent_id}`} navigate={api.navigate} className="tt-name">{r.title}</PLink> },
              {
                key: 'v',
                header: '',
                render: (r) => (
                  <div className="acts">
                    <Button size="xs" variant={r.my_vote === 1 ? 'primary' : 'default'} aria-pressed={r.my_vote === 1} onClick={() => void vote(r.torrent_id, 1)}>
                      {api.t('moderation.ui.up', { n: r.up })}
                    </Button>
                    <Button size="xs" variant={r.my_vote === -1 ? 'danger' : 'default'} aria-pressed={r.my_vote === -1} onClick={() => void vote(r.torrent_id, -1)}>
                      {api.t('moderation.ui.down', { n: r.down })}
                    </Button>
                  </div>
                ),
              },
            ]}
          />
        )}
      </Panel>
    </div>
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
  const [rows, setRows] = useState<Decision[] | null>(null);
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
  const targetLink = (t: string) =>
    t.startsWith('torrent:') ? (
      <PLink to={`/torrents/${t.slice(8)}`} navigate={api.navigate} className="link mono">
        {t}
      </PLink>
    ) : (
      <span className="mono">{t}</span>
    );
  return (
    <>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void load(f.target, f.moderator);
        }}
      >
        <FilterBar
          quick={
            <>
              <Input name="history_target" aria-label={api.t('moderation.ui.filter_target')} placeholder={api.t('moderation.ui.filter_target')} value={f.target} onChange={(e) => setF({ ...f, target: e.target.value })} style={{ width: 220 }} />
              <Input name="history_moderator" type="number" aria-label={api.t('moderation.ui.filter_moderator')} placeholder={api.t('moderation.ui.filter_moderator')} value={f.moderator} onChange={(e) => setF({ ...f, moderator: e.target.value })} style={{ width: 140 }} />
            </>
          }
          right={
            <Button type="submit" size="sm" variant="primary">
              {api.t('moderation.ui.filter')}
            </Button>
          }
        />
      </form>
      {rows === null ? (
        <SkeletonRows n={5} />
      ) : (
        <Table
          rows={rows}
          rowKey={(r) => `${r.target}:${r.created_at}:${r.decision}`}
          empty={<Empty>—</Empty>}
          minWidth={640}
          columns={[
            { key: 't', header: api.t('moderation.ui.time'), render: (r) => <span className="small muted nowrap">{new Date(r.created_at).toLocaleString()}</span> },
            { key: 'o', header: api.t('moderation.ui.title'), render: (r) => targetLink(r.target) },
            {
              key: 'd',
              header: api.t('moderation.ui.decision'),
              render: (r) => (
                <div className="tt">
                  <Status tone={DECISION_TONE[r.decision] ?? 'mute'}>{api.t(`moderation.ui.${r.decision}`)}</Status>
                  {r.note && <span className="small muted">{r.note}</span>}
                </div>
              ),
            },
            { key: 'm', header: api.t('moderation.ui.moderator'), render: (r) => (r.moderator_id ? <PLink to={`/users/${r.moderator_id}`} navigate={api.navigate} className="link">#{r.moderator_id}</PLink> : (r.actor ?? '—')) },
          ]}
        />
      )}
    </>
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
  const [rows, setRows] = useState<Word[] | null>(null);
  const [open, setOpen] = useState(false);
  const [f, setF] = useState({ pattern: '', action: 'review' });
  const load = useCallback(async () => setRows((await api.json<{ items: Word[] }>('/api/p/moderation/v1/admin/words')).items), [api]);
  useEffect(() => {
    void load().catch(() => setRows([]));
  }, [load]);
  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      api.toast(api.t('moderation.ui.saved'), 'success');
      await load();
      return true;
    } catch (e) {
      api.toast(errText(api, e), 'error');
      return false;
    }
  };
  return (
    <>
      <div className="fbar">
        <span className="small muted grow">{api.t('moderation.ui.words_hint')}</span>
        <Button size="sm" variant="primary" onClick={() => setOpen(true)}>
          {api.t('moderation.ui.new_word')}
        </Button>
      </div>
      {rows === null ? (
        <SkeletonRows n={4} />
      ) : (
        <Table
          rows={rows}
          rowKey={(r) => r.id}
          empty={<Empty>—</Empty>}
          columns={[
            { key: 'p', header: api.t('moderation.ui.pattern'), render: (r) => <code className="mono">{r.pattern}</code> },
            { key: 'a', header: api.t('moderation.ui.action'), render: (r) => <Status tone={r.action === 'block' ? 'bad' : r.action === 'review' ? 'warn' : 'mute'}>{api.t(`moderation.ui.action.${r.action}`)}</Status> },
            { key: 's', header: api.t('moderation.ui.scope'), render: (r) => <span className="small muted">{r.scopes.replace(/[{}]/g, '')}</span> },
            {
              key: 'x',
              header: '',
              render: (r) => (
                <div className="acts">
                  {!r.active && <Tag>{api.t('moderation.ui.inactive')}</Tag>}
                  <Button size="xs" variant="ghost" onClick={() => void act(() => api.json(`/api/p/moderation/v1/admin/words/${r.id}`, { method: 'DELETE' }))}>
                    {api.t('moderation.ui.delete')}
                  </Button>
                </div>
              ),
            },
          ]}
        />
      )}
      <Sheet
        open={open}
        onClose={() => setOpen(false)}
        title={api.t('moderation.ui.new_word')}
        footer={
          <Button
            variant="primary"
            disabled={!f.pattern.trim()}
            onClick={() =>
              void act(() => api.json('/api/p/moderation/v1/admin/words', { json: { pattern: f.pattern.trim(), action: f.action, scopes: ['comment'] } })).then((ok) => {
                if (ok) {
                  setF({ ...f, pattern: '' });
                  setOpen(false);
                }
              })
            }
          >
            {api.t('moderation.ui.add')}
          </Button>
        }
      >
        <Field label={api.t('moderation.ui.pattern')}>
          <Input name="word_pattern" required maxLength={100} value={f.pattern} onChange={(e) => setF({ ...f, pattern: e.target.value })} />
        </Field>
        <Field label={api.t('moderation.ui.action')}>
          <Select name="word_action" value={f.action} onChange={(e) => setF({ ...f, action: e.target.value })} options={['block', 'mask', 'review'].map((a) => ({ value: a, label: api.t(`moderation.ui.action.${a}`) }))} />
        </Field>
        <Note>{api.t('moderation.ui.words_hint')}</Note>
      </Sheet>
    </>
  );
}

function Hits({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<{ id: number; scope: string; author_id: number; excerpt: string; created_at: number }[] | null>(null);
  useEffect(() => {
    void api
      .json<{ items: NonNullable<typeof rows> }>('/api/p/moderation/v1/admin/word-hits')
      .then((r) => setRows(r.items))
      .catch(() => setRows([]));
  }, [api]);
  if (rows === null) return <SkeletonRows n={4} />;
  const scope = (s: string) => (api.t(`moderation.ui.scope.${s}`) === `moderation.ui.scope.${s}` ? s : api.t(`moderation.ui.scope.${s}`));
  return (
    <Table
      rows={rows}
      rowKey={(r) => r.id}
      empty={<Empty>—</Empty>}
      minWidth={640}
      columns={[
        { key: 't', header: api.t('moderation.ui.time'), render: (r) => <span className="small muted nowrap">{new Date(r.created_at).toLocaleString()}</span> },
        { key: 's', header: api.t('moderation.ui.scope'), render: (r) => scope(r.scope) },
        { key: 'a', header: api.t('moderation.ui.author'), render: (r) => <PLink to={`/users/${r.author_id}`} navigate={api.navigate} className="link">#{r.author_id}</PLink> },
        { key: 'e', header: api.t('moderation.ui.excerpt'), render: (r) => <span className="pt-prewrap small">{r.excerpt}</span> },
      ]}
    />
  );
}

function Admin({ api }: { api: PluginApi }) {
  const [tab, setTab] = useState('queue');
  return (
    <div className="stack">
      <PageHead title={api.t('moderation.ui.queue')} lede={api.t('moderation.ui.lede')} />
      <Panel flush primary>
        <Tabs variant="flush" value={tab} onChange={setTab} tabs={['queue', 'history', 'words', 'hits'].map((id) => ({ id, label: api.t(`moderation.ui.${id}`) }))} />
        {tab === 'queue' ? <Queue api={api} /> : tab === 'history' ? <History api={api} /> : tab === 'words' ? <Words api={api} /> : <Hits api={api} />}
      </Panel>
    </div>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  if (path.startsWith('/admin/p/moderation')) return <Admin api={api} />;
  return <Candidates api={api} />;
}

export default {
  slots: {
    'torrent.detail.sidebar': { component: ModPanel, order: 5 },
    'nav.user_menu': { component: StaffMenu, order: 81 },
  },
  routes: [
    { path: '/p/moderation/*', component: Pages },
    { path: '/admin/p/moderation/*', component: Pages },
  ],
} satisfies PluginFrontend;
