// requests 前端：用户菜单入口、求种列表（/p/requests/）、发布（/p/requests/new）、详情（/p/requests/<id>：悬赏、投票、应求、
// 确认 / 驳回、撤回，request.detail.sidebar 插槽供评论区使用）。
import { useCallback, useEffect, useState } from 'react';
import { ApiError, renderPreview } from '@pt-studio/sdk';
import { Slot, type PluginApi, type PluginFrontend, type SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Empty, Field, Input, MarkdownEditor, Table, UserContent } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'requests.ui.menu': '求种',
  'requests.ui.title': '求种',
  'requests.ui.new': '发布求种',
  'requests.ui.type': '类型',
  'requests.ui.name': '标题',
  'requests.ui.description': '描述',
  'requests.ui.bounty': '悬赏',
  'requests.ui.requirements': '规格要求（JSON，可选）',
  'requests.ui.submit': '发布',
  'requests.ui.votes': '票数',
  'requests.ui.status': '状态',
  'requests.ui.vote': '我也想要',
  'requests.ui.unvote': '取消',
  'requests.ui.add_bounty': '追加悬赏',
  'requests.ui.fill': '应求',
  'requests.ui.torrent_id': '种子 ID',
  'requests.ui.fills': '应求记录',
  'requests.ui.confirm': '确认',
  'requests.ui.reject': '驳回',
  'requests.ui.withdraw': '撤回求种',
  'requests.ui.contributors': '悬赏贡献',
  'requests.ui.status.open': '求种中',
  'requests.ui.status.fill_pending': '待确认',
  'requests.ui.status.filled': '已应求',
  'requests.ui.status.closed': '已关闭',
  'requests.ui.sort.new': '最新',
  'requests.ui.sort.bounty': '悬赏最高',
  'requests.ui.sort.votes': '票数最多',
});
registerMessages('en', {
  'requests.ui.menu': 'Requests',
  'requests.ui.title': 'Requests',
  'requests.ui.new': 'New request',
  'requests.ui.type': 'Type',
  'requests.ui.name': 'Title',
  'requests.ui.description': 'Description',
  'requests.ui.bounty': 'Bounty',
  'requests.ui.requirements': 'Requirements (JSON, optional)',
  'requests.ui.submit': 'Submit',
  'requests.ui.votes': 'Votes',
  'requests.ui.status': 'Status',
  'requests.ui.vote': 'Me too',
  'requests.ui.unvote': 'Unvote',
  'requests.ui.add_bounty': 'Add bounty',
  'requests.ui.fill': 'Fill',
  'requests.ui.torrent_id': 'Torrent ID',
  'requests.ui.fills': 'Fills',
  'requests.ui.confirm': 'Confirm',
  'requests.ui.reject': 'Reject',
  'requests.ui.withdraw': 'Withdraw',
  'requests.ui.contributors': 'Contributors',
  'requests.ui.status.open': 'Open',
  'requests.ui.status.fill_pending': 'Fill pending',
  'requests.ui.status.filled': 'Filled',
  'requests.ui.status.closed': 'Closed',
  'requests.ui.sort.new': 'Newest',
  'requests.ui.sort.bounty': 'Top bounty',
  'requests.ui.sort.votes': 'Most votes',
});

function errText(api: PluginApi, e: unknown): string {
  if (e instanceof ApiError) {
    const s = api.t(e.code);
    return s === e.code ? e.message : s;
  }
  return String(e);
}

function go(api: PluginApi, to: string) {
  return (e: { preventDefault(): void }) => {
    e.preventDefault();
    api.navigate(to);
  };
}

function MenuItem({ api }: SlotProps) {
  return (
    <a href="/p/requests/" onClick={go(api, '/p/requests/')}>
      {api.t('requests.ui.menu')}
    </a>
  );
}

interface Row {
  id: number;
  type_id: string;
  title: string;
  bounty_total: number;
  votes: number;
  status: string;
}

function List({ api }: { api: PluginApi }) {
  const [sort, setSort] = useState('new');
  const [rows, setRows] = useState<Row[]>([]);
  useEffect(() => {
    void api.json<{ items: Row[] }>(`/api/p/requests/v1/requests?sort=${sort}`).then((r) => setRows(r.items));
  }, [api, sort]);
  return (
    <Card
      title={api.t('requests.ui.title')}
      actions={
        <span className="pt-row">
          <select className="pt-input" value={sort} onChange={(e) => setSort(e.target.value)}>
            {['new', 'bounty', 'votes'].map((s) => (
              <option key={s} value={s}>
                {api.t(`requests.ui.sort.${s}`)}
              </option>
            ))}
          </select>
          <Button variant="primary" onClick={() => api.navigate('/p/requests/new')}>
            {api.t('requests.ui.new')}
          </Button>
        </span>
      }
    >
      <Table
        rows={rows}
        rowKey={(r) => r.id}
        empty={<Empty>—</Empty>}
        columns={[
          { key: 't', header: api.t('requests.ui.name'), render: (r) => <a href={`/p/requests/${r.id}`} onClick={go(api, `/p/requests/${r.id}`)}>{r.title}</a> },
          { key: 'y', header: api.t('requests.ui.type'), render: (r) => api.t(`term:type-${r.type_id}.name`) === `type-${r.type_id}.name` ? r.type_id : api.t(`term:type-${r.type_id}.name`) },
          { key: 'b', header: api.t('requests.ui.bounty'), render: (r) => r.bounty_total },
          { key: 'v', header: api.t('requests.ui.votes'), render: (r) => r.votes },
          { key: 's', header: api.t('requests.ui.status'), render: (r) => api.t(`requests.ui.status.${r.status}`) },
        ]}
      />
    </Card>
  );
}

function New({ api }: { api: PluginApi }) {
  const [types, setTypes] = useState<{ type_id: string; name_term: string }[]>([]);
  const [typeId, setTypeId] = useState('');
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [bounty, setBounty] = useState(100);
  const [reqs, setReqs] = useState('');
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    void api.json<{ items: { type_id: string; name_term: string }[] }>('/api/p/torrent/v1/types').then((r) => {
      setTypes(r.items);
      setTypeId(r.items[0]?.type_id ?? '');
    });
  }, [api]);
  async function submit() {
    setErr(null);
    let requirements: unknown = [];
    if (reqs.trim()) {
      try {
        requirements = JSON.parse(reqs);
      } catch {
        setErr('requirements: invalid JSON');
        return;
      }
    }
    try {
      const r = await api.json<{ id: number }>('/api/p/requests/v1/requests', { method: 'POST', json: { type_id: typeId, title, description, bounty, requirements }, idempotent: true });
      api.navigate(`/p/requests/${r.id}`);
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  return (
    <Card title={api.t('requests.ui.new')}>
      <div className="pt-stack">
        <Field label={api.t('requests.ui.type')}>
          <select className="pt-input" value={typeId} onChange={(e) => setTypeId(e.target.value)}>
            {types.map((t) => (
              <option key={t.type_id} value={t.type_id}>
                {api.t(`term:${t.name_term}`)}
              </option>
            ))}
          </select>
        </Field>
        <Field label={api.t('requests.ui.name')}>
          <Input value={title} maxLength={200} onChange={(e) => setTitle(e.target.value)} />
        </Field>
        <Field label={api.t('requests.ui.description')}>
          <MarkdownEditor name="request_description" rows={6} value={description} onChange={setDescription} preview={renderPreview} />
        </Field>
        <Field label={api.t('requests.ui.bounty')}>
          <Input type="number" min={1} value={bounty} onChange={(e) => setBounty(Number(e.target.value) || 0)} />
        </Field>
        <Field label={api.t('requests.ui.requirements')}>
          <textarea className="pt-input pt-mono" rows={3} placeholder='[{"field": "resolution", "op": "in", "value": ["2160p"]}]' value={reqs} onChange={(e) => setReqs(e.target.value)} />
        </Field>
        <Button variant="primary" disabled={!title.trim() || bounty < 1} onClick={() => void submit()}>
          {api.t('requests.ui.submit')}
        </Button>
        {err && <p className="pt-error">{err}</p>}
      </div>
    </Card>
  );
}

interface Detail {
  id: number;
  title: string;
  type_id: string;
  description_html: string;
  bounty_total: number;
  currency: string;
  votes: number;
  voted: boolean;
  status: string;
  can_manage: boolean;
  user: { id: number; username: string | null };
  contributors: { user_id: number; username: string | null; amount: number }[];
  fills: { id: number; torrent_id: number; filler: { id: number; username: string | null }; state: string; reason: string | null; payout: number | null }[];
}

function DetailPage({ api, id }: { api: PluginApi; id: number }) {
  const [d, setD] = useState<Detail | null>(null);
  const [amount, setAmount] = useState(10);
  const [tid, setTid] = useState('');
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(async () => setD(await api.json<Detail>(`/api/p/requests/v1/requests/${id}`)), [api, id]);
  useEffect(() => {
    void load().catch((e) => setErr(errText(api, e)));
  }, [api, load]);
  async function act(path: `/api/${string}`, body: unknown = {}) {
    setErr(null);
    try {
      await api.json(path, { method: 'POST', json: body });
      await load();
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  async function toggleVote() {
    await api.fetch(`/api/p/requests/v1/requests/${id}/vote`, { method: d?.voted ? 'DELETE' : 'POST', json: {} });
    await load();
  }
  if (!d) return err ? <p className="pt-error">{err}</p> : null;
  const open = d.status === 'open' || d.status === 'fill_pending';
  return (
    <div className="pt-stack">
      <Card
        title={d.title}
        actions={
          <span className="pt-row">
            <Badge tone="primary">{api.t(`requests.ui.status.${d.status}`)}</Badge>
            <Badge>
              {api.t('requests.ui.bounty')} {d.bounty_total}
            </Badge>
            <Button variant="ghost" onClick={() => void toggleVote()}>
              {api.t(d.voted ? 'requests.ui.unvote' : 'requests.ui.vote')} ({d.votes})
            </Button>
          </span>
        }
      >
        <UserContent html={d.description_html} />
        {open && (
          <div className="pt-row">
            <Input type="number" min={1} aria-label={api.t('requests.ui.add_bounty')} value={amount} onChange={(e) => setAmount(Number(e.target.value) || 0)} />
            <Button onClick={() => void act(`/api/p/requests/v1/requests/${id}/bounty`, { amount })}>{api.t('requests.ui.add_bounty')}</Button>
          </div>
        )}
        {d.status === 'open' && (
          <div className="pt-row">
            <Input aria-label={api.t('requests.ui.torrent_id')} placeholder={api.t('requests.ui.torrent_id')} value={tid} onChange={(e) => setTid(e.target.value)} />
            <Button variant="primary" disabled={!Number(tid)} onClick={() => void act(`/api/p/requests/v1/requests/${id}/fills`, { torrent_id: Number(tid) })}>
              {api.t('requests.ui.fill')}
            </Button>
            {d.can_manage && (
              <Button variant="danger" onClick={() => void act(`/api/p/requests/v1/requests/${id}/withdraw`)}>
                {api.t('requests.ui.withdraw')}
              </Button>
            )}
          </div>
        )}
        {err && <p className="pt-error">{err}</p>}
      </Card>
      <Card title={api.t('requests.ui.fills')}>
        <Table
          rows={d.fills}
          rowKey={(f) => f.id}
          empty={<Empty>—</Empty>}
          columns={[
            { key: 't', header: api.t('requests.ui.torrent_id'), render: (f) => <a href={`/torrents/${f.torrent_id}`} onClick={go(api, `/torrents/${f.torrent_id}`)}>#{f.torrent_id}</a> },
            { key: 'u', header: '', render: (f) => f.filler.username ?? `#${f.filler.id}` },
            { key: 's', header: api.t('requests.ui.status'), render: (f) => f.state },
            {
              key: 'x',
              header: '',
              render: (f) =>
                f.state === 'pending' && d.can_manage ? (
                  <span className="pt-row">
                    <Button variant="primary" onClick={() => void act(`/api/p/requests/v1/fills/${f.id}/confirm`)}>
                      {api.t('requests.ui.confirm')}
                    </Button>
                    <Button variant="ghost" onClick={() => void act(`/api/p/requests/v1/fills/${f.id}/reject`, { reason: '' })}>
                      {api.t('requests.ui.reject')}
                    </Button>
                  </span>
                ) : null,
            },
          ]}
        />
      </Card>
      <Card title={api.t('requests.ui.contributors')}>
        <Table rows={d.contributors} rowKey={(c) => c.user_id} columns={[{ key: 'u', header: '', render: (c) => c.username ?? `#${c.user_id}` }, { key: 'a', header: api.t('requests.ui.bounty'), render: (c) => c.amount }]} />
      </Card>
      <Slot name="request.detail.sidebar" ctx={{ request: d }} />
    </div>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  const rest = path.replace(/^\/p\/requests\/?/, '');
  if (rest.startsWith('new')) return <New api={api} />;
  const id = Number(rest.split('/')[0]);
  if (id > 0) return <DetailPage api={api} id={id} />;
  return <List api={api} />;
}

export default {
  slots: {
    'nav.user_menu': { component: MenuItem, order: 30 },
  },
  routes: [{ path: '/p/requests/*', component: Pages }],
} satisfies PluginFrontend;
