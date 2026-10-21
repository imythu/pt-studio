// requests 前端：用户菜单入口、求种列表（/p/requests/：状态页签、类型筛选、排序）、发布（/p/requests/new）、
// 详情（/p/requests/<id>：悬赏、投票、应求、确认 / 驳回、撤回，request.detail.sidebar 插槽供评论区使用）。
import { useCallback, useEffect, useState } from 'react';
import { ApiError, renderPreview } from '@pt-studio/sdk';
import { Slot, type PluginApi, type PluginFrontend, type SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Chips, Field, Input, KeyFacts, MarkdownEditor, NavItem, Note, PageHead, Panel, PLink, RuleItem, Select, Sheet, SideLayout, SkeletonRows, Status, Table, Tabs, Textarea, UserContent, type Tone } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'requests.ui.rule_earn': '应求',
  'requests.ui.rule_earn_body': '上传别人求的资源，求种人确认后拿到全部悬赏。',
  'requests.ui.rule_spend': '求种悬赏',
  'requests.ui.rule_spend_body': '发布求种时拿出魔力值作悬赏，其他人也可以追加；没人应求时可以撤回。',
  'requests.ui.rule_val': '悬赏',
  'requests.ui.rule_link': '去求种区',
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
  'requests.ui.lede': '没找到想要的资源？发一条求种，其他人应求并被确认后，悬赏的魔力值归他。',
  'requests.ui.status.active': '进行中',
  'requests.ui.all_types': '全部类型',
  'requests.ui.sort': '排序',
  'requests.ui.want': '想要',
  'requests.ui.by': '{user} 发起',
  'requests.ui.filler': '应求者',
  'requests.ui.user': '用户',
  'requests.ui.empty': '暂无求种',
  'requests.ui.no_fills': '还没有人应求',
  'requests.ui.bounty_hint': '应求被确认后发放给应求者',
  'requests.ui.bounty_freeze': '发布时从你的魔力值中冻结',
  'requests.ui.owner': '发起人操作',
  'requests.ui.withdraw_hint': '撤回后悬赏退还给各贡献者',
  'requests.ui.withdraw_confirm': '确定撤回这条求种？',
  'requests.ui.reject_reason': '驳回理由（可选）',
  'requests.ui.cancel': '取消',
  'requests.ui.amount': '追加数量',
  'requests.ui.fill_hint': '填写满足要求的种子 ID，等待发起人确认',
  'requests.ui.placeholder_title': '片名 + 年份 + 想要的版本',
  'requests.ui.fill_state.pending': '待确认',
  'requests.ui.fill_state.confirmed': '已确认',
  'requests.ui.fill_state.rejected': '已驳回',
});
registerMessages('en', {
  'requests.ui.rule_earn': 'Filling requests',
  'requests.ui.rule_earn_body': 'Upload what someone requested and collect the bounty once they confirm.',
  'requests.ui.rule_spend': 'Request bounties',
  'requests.ui.rule_spend_body': 'Put up bonus as a bounty when you request something; others can add to it. Withdraw it if nobody fills the request.',
  'requests.ui.rule_val': 'bounty',
  'requests.ui.rule_link': 'Browse requests',
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
  'requests.ui.lede': 'Can not find it? Post a request — whoever fills it gets the bounty once you confirm.',
  'requests.ui.status.active': 'Active',
  'requests.ui.all_types': 'All types',
  'requests.ui.sort': 'Sort',
  'requests.ui.want': 'want',
  'requests.ui.by': 'by {user}',
  'requests.ui.filler': 'Filler',
  'requests.ui.user': 'User',
  'requests.ui.empty': 'No requests',
  'requests.ui.no_fills': 'No fills yet',
  'requests.ui.bounty_hint': 'Paid to the filler once confirmed',
  'requests.ui.bounty_freeze': 'Held from your bonus when posted',
  'requests.ui.owner': 'Owner actions',
  'requests.ui.withdraw_hint': 'Withdrawing refunds every contributor',
  'requests.ui.withdraw_confirm': 'Withdraw this request?',
  'requests.ui.reject_reason': 'Reason (optional)',
  'requests.ui.cancel': 'Cancel',
  'requests.ui.amount': 'Amount',
  'requests.ui.fill_hint': 'Enter the ID of a torrent that meets the request; the owner confirms it',
  'requests.ui.placeholder_title': 'Title + year + wanted edition',
  'requests.ui.fill_state.pending': 'Pending',
  'requests.ui.fill_state.confirmed': 'Confirmed',
  'requests.ui.fill_state.rejected': 'Rejected',
});

function errText(api: PluginApi, e: unknown): string {
  if (e instanceof ApiError) {
    const s = api.t(e.code);
    return s === e.code ? e.message : s;
  }
  return String(e);
}

const TONE: Record<string, Tone> = { open: 'info', fill_pending: 'warn', filled: 'ok', closed: 'mute', pending: 'warn', confirmed: 'ok', rejected: 'bad' };
const num = (n: number) => n.toLocaleString();

function typeLabel(api: PluginApi, id: string): string {
  const k = `term:type-${id}.name`;
  const s = api.t(k);
  return s === k || s === `type-${id}.name` ? id : s;
}

function useTypes(api: PluginApi) {
  const [types, setTypes] = useState<{ type_id: string; name_term: string }[]>([]);
  useEffect(() => {
    void api
      .json<{ items: { type_id: string; name_term: string }[] }>('/api/p/torrent/v1/types')
      .then((r) => setTypes(r.items))
      .catch(() => undefined);
  }, [api]);
  return types;
}

function MenuItem({ api }: SlotProps) {
  return (
    <NavItem to="/p/requests/" navigate={api.navigate}>
      {api.t('requests.ui.menu')}
    </NavItem>
  );
}

interface Row {
  id: number;
  type_id: string;
  title: string;
  bounty_total: number;
  votes: number;
  status: string;
  created_at: number;
}

const STATUSES = ['', 'open', 'fill_pending', 'filled', 'closed'];

function List({ api }: { api: PluginApi }) {
  const types = useTypes(api);
  const [status, setStatus] = useState('');
  const [type, setType] = useState('');
  const [sort, setSort] = useState('new');
  const [rows, setRows] = useState<Row[] | null>(null);
  useEffect(() => {
    setRows(null);
    const q = new URLSearchParams({ sort });
    if (status) q.set('status', status);
    if (type) q.set('type_id', type);
    void api
      .json<{ items: Row[] }>(`/api/p/requests/v1/requests?${q.toString()}`)
      .then((r) => setRows(r.items))
      .catch(() => setRows([]));
  }, [api, sort, status, type]);
  return (
    <div className="stack">
      <PageHead
        title={api.t('requests.ui.title')}
        lede={api.t('requests.ui.lede')}
        actions={
          <PLink to="/p/requests/new" navigate={api.navigate} className="btn btn-primary">
            {api.t('requests.ui.new')}
          </PLink>
        }
      />
      <Tabs variant="sub" value={status} onChange={setStatus} tabs={STATUSES.map((s) => ({ id: s, label: api.t(`requests.ui.status.${s || 'active'}`) }))} />
      <section className="panel" data-primary>
        <div className="fbar">
          <div className="quick">
            <Chips label={api.t('requests.ui.type')} value={type} onChange={(v) => setType(v === type ? '' : v)} options={[{ value: '', label: api.t('requests.ui.all_types') }, ...types.map((t) => ({ value: t.type_id, label: api.t(`term:${t.name_term}`) }))]} />
          </div>
          <span className="grow" />
          <Select aria-label={api.t('requests.ui.sort')} value={sort} onChange={(e) => setSort(e.target.value)} options={['new', 'bounty', 'votes'].map((s) => ({ value: s, label: api.t(`requests.ui.sort.${s}`) }))} />
        </div>
        {rows === null ? (
          <SkeletonRows n={5} />
        ) : rows.length === 0 ? (
          <p className="pad muted">{api.t('requests.ui.empty')}</p>
        ) : (
          rows.map((r) => (
            <PLink key={r.id} to={`/p/requests/${r.id}`} navigate={api.navigate} className="req-row">
              <div className="vote-box">
                <b>{r.votes}</b>
                <span>{api.t('requests.ui.want')}</span>
              </div>
              <div>
                <b>{r.title}</b>
                <div className="small muted">
                  {typeLabel(api, r.type_id)} · {new Date(r.created_at).toLocaleDateString()}
                  <span className="m-only"> · <span className="gold num">{num(r.bounty_total)}</span> · {api.t(`requests.ui.status.${r.status}`)}</span>
                </div>
              </div>
              <span className="bounty">{num(r.bounty_total)}</span>
              <Status tone={TONE[r.status] ?? 'mute'}>{api.t(`requests.ui.status.${r.status}`)}</Status>
            </PLink>
          ))
        )}
      </section>
    </div>
  );
}

function New({ api }: { api: PluginApi }) {
  const types = useTypes(api);
  const [typeId, setTypeId] = useState('');
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [bounty, setBounty] = useState(100);
  const [reqs, setReqs] = useState('');
  const [err, setErr] = useState<string | null>(null);
  const tid = typeId || types[0]?.type_id || '';
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
      const r = await api.json<{ id: number }>('/api/p/requests/v1/requests', { method: 'POST', json: { type_id: tid, title, description, bounty, requirements }, idempotent: true });
      api.navigate(`/p/requests/${r.id}`);
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  return (
    <div className="stack">
      <PageHead
        title={api.t('requests.ui.new')}
        crumb={
          <>
            <PLink to="/p/requests/" navigate={api.navigate}>
              {api.t('requests.ui.title')}
            </PLink>
            <span>{api.t('requests.ui.new')}</span>
          </>
        }
      />
      <form
        className="panel pad-l form narrow-form"
        data-primary
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <div className="form-2">
          <Field label={api.t('requests.ui.type')}>
            <Select name="request_type" value={tid} onChange={(e) => setTypeId(e.target.value)} options={types.map((t) => ({ value: t.type_id, label: api.t(`term:${t.name_term}`) }))} />
          </Field>
          <Field label={api.t('requests.ui.bounty')} hint={api.t('requests.ui.bounty_freeze')}>
            <Input name="request_bounty" type="number" min={1} value={bounty} onChange={(e) => setBounty(Number(e.target.value) || 0)} />
          </Field>
        </div>
        <Field label={api.t('requests.ui.name')}>
          <Input name="request_title" value={title} maxLength={200} placeholder={api.t('requests.ui.placeholder_title')} onChange={(e) => setTitle(e.target.value)} />
        </Field>
        <Field label={api.t('requests.ui.description')}>
          <MarkdownEditor name="request_description" rows={6} value={description} onChange={setDescription} preview={renderPreview} />
        </Field>
        <Field label={api.t('requests.ui.requirements')}>
          <Textarea className="ta code" rows={3} placeholder='[{"field": "resolution", "op": "in", "value": ["2160p"]}]' value={reqs} onChange={(e) => setReqs(e.target.value)} />
        </Field>
        {err && <Note tone="bad">{err}</Note>}
        <div className="form-acts">
          <Button type="submit" variant="primary" disabled={!title.trim() || bounty < 1}>
            {api.t('requests.ui.submit')}
          </Button>
        </div>
      </form>
    </div>
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

function UserA({ api, id, name }: { api: PluginApi; id: number; name: string | null }) {
  return (
    <PLink to={`/users/${id}`} navigate={api.navigate}>
      {name ?? `#${id}`}
    </PLink>
  );
}

function DetailPage({ api, id }: { api: PluginApi; id: number }) {
  const [d, setD] = useState<Detail | null>(null);
  const [amount, setAmount] = useState(100);
  const [tid, setTid] = useState('');
  const [err, setErr] = useState<string | null>(null);
  const [sheet, setSheet] = useState<'bounty' | 'withdraw' | { reject: number } | null>(null);
  const [reason, setReason] = useState('');
  const load = useCallback(async () => setD(await api.json<Detail>(`/api/p/requests/v1/requests/${id}`)), [api, id]);
  useEffect(() => {
    void load().catch((e) => setErr(errText(api, e)));
  }, [api, load]);
  async function act(path: `/api/${string}`, body: unknown = {}) {
    setErr(null);
    try {
      await api.json(path, { method: 'POST', json: body });
      setSheet(null);
      await load();
      return true;
    } catch (e) {
      api.toast(errText(api, e), 'error');
      return false;
    }
  }
  async function toggleVote() {
    await api.fetch(`/api/p/requests/v1/requests/${id}/vote`, { method: d?.voted ? 'DELETE' : 'POST', json: {} });
    await load();
  }
  if (!d) return err ? <Note tone="bad">{err}</Note> : <SkeletonRows n={6} />;
  const open = d.status === 'open' || d.status === 'fill_pending';
  const cancel = (
    <Button variant="ghost" onClick={() => setSheet(null)}>
      {api.t('requests.ui.cancel')}
    </Button>
  );
  return (
    <div className="stack">
      <PageHead
        crumb={
          <>
            <PLink to="/p/requests/" navigate={api.navigate}>
              {api.t('requests.ui.title')}
            </PLink>
            <span>#{d.id}</span>
          </>
        }
        title={d.title}
        lede={
          <>
            {typeLabel(api, d.type_id)} · {api.t('requests.ui.by', { user: d.user.username ?? `#${d.user.id}` })}
          </>
        }
        actions={
          <>
            <Status tone={TONE[d.status] ?? 'mute'}>{api.t(`requests.ui.status.${d.status}`)}</Status>
            <Button size="sm" variant={d.voted ? 'primary' : undefined} onClick={() => void toggleVote()}>
              {api.t(d.voted ? 'requests.ui.unvote' : 'requests.ui.vote')} · {d.votes}
            </Button>
            {open && (
              <Button size="sm" onClick={() => setSheet('bounty')}>
                {api.t('requests.ui.add_bounty')}
              </Button>
            )}
          </>
        }
      />
      <KeyFacts
        items={[
          { label: api.t('requests.ui.bounty'), value: num(d.bounty_total), hint: d.currency, tone: 'gold' },
          { label: api.t('requests.ui.votes'), value: d.votes },
          { label: api.t('requests.ui.fills'), value: d.fills.length },
          { label: api.t('requests.ui.status'), value: api.t(`requests.ui.status.${d.status}`) },
        ]}
      />
      <SideLayout
        width={320}
        side={
          <>
            <Panel title={api.t('requests.ui.contributors')} flush>
              <Table
                rows={d.contributors}
                rowKey={(c) => c.user_id}
                columns={[
                  { key: 'u', header: api.t('requests.ui.user'), render: (c) => <UserA api={api} id={c.user_id} name={c.username} /> },
                  { key: 'a', header: api.t('requests.ui.bounty'), right: true, render: (c) => <span className="num">{num(c.amount)}</span> },
                ]}
              />
            </Panel>
            {d.can_manage && d.status === 'open' && (
              <section className="panel side-card">
                <h3>{api.t('requests.ui.owner')}</h3>
                <div>
                  <Button size="sm" variant="danger" onClick={() => setSheet('withdraw')}>
                    {api.t('requests.ui.withdraw')}
                  </Button>
                </div>
                <span className="tiny muted">{api.t('requests.ui.withdraw_hint')}</span>
              </section>
            )}
          </>
        }
      >
        <Panel title={api.t('requests.ui.description')} primary>
          <UserContent html={d.description_html} />
        </Panel>
        <Panel title={api.t('requests.ui.fills')} flush>
          <Table
            rows={d.fills}
            rowKey={(f) => f.id}
            empty={api.t('requests.ui.no_fills')}
            columns={[
              { key: 't', header: api.t('requests.ui.torrent_id'), render: (f) => <PLink to={`/torrents/${f.torrent_id}`} navigate={api.navigate}>#{f.torrent_id}</PLink> },
              { key: 'u', header: api.t('requests.ui.filler'), render: (f) => <UserA api={api} id={f.filler.id} name={f.filler.username} /> },
              {
                key: 's',
                header: api.t('requests.ui.status'),
                render: (f) => (
                  <>
                    <Status tone={TONE[f.state] ?? 'mute'}>{api.t(`requests.ui.fill_state.${f.state}`)}</Status>
                    {f.reason && <div className="tiny muted">{f.reason}</div>}
                  </>
                ),
              },
              {
                key: 'x',
                header: '',
                right: true,
                render: (f) =>
                  f.state === 'pending' && d.can_manage ? (
                    <div className="form-acts">
                      <Button size="sm" variant="primary" onClick={() => void act(`/api/p/requests/v1/fills/${f.id}/confirm`)}>
                        {api.t('requests.ui.confirm')}
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        onClick={() => {
                          setReason('');
                          setSheet({ reject: f.id });
                        }}
                      >
                        {api.t('requests.ui.reject')}
                      </Button>
                    </div>
                  ) : null,
              },
            ]}
          />
          {d.status === 'open' && (
            <form
              className="toolbar"
              onSubmit={(e) => {
                e.preventDefault();
                void act(`/api/p/requests/v1/requests/${id}/fills`, { torrent_id: Number(tid) }).then((ok) => ok && setTid(''));
              }}
            >
              <Input name="fill_torrent" inputMode="numeric" aria-label={api.t('requests.ui.torrent_id')} placeholder={api.t('requests.ui.torrent_id')} value={tid} onChange={(e) => setTid(e.target.value)} />
              <Button type="submit" variant="primary" disabled={!Number(tid)}>
                {api.t('requests.ui.fill')}
              </Button>
              <span className="tiny muted">{api.t('requests.ui.fill_hint')}</span>
            </form>
          )}
        </Panel>
        <Slot name="request.detail.sidebar" ctx={{ request: d }} />
      </SideLayout>
      <Sheet
        open={sheet === 'bounty'}
        onClose={() => setSheet(null)}
        title={api.t('requests.ui.add_bounty')}
        footer={
          <>
            {cancel}
            <Button variant="primary" disabled={amount < 1} onClick={() => void act(`/api/p/requests/v1/requests/${id}/bounty`, { amount })}>
              {api.t('requests.ui.add_bounty')}
            </Button>
          </>
        }
      >
        <Field label={api.t('requests.ui.amount')} hint={api.t('requests.ui.bounty_freeze')}>
          <Input name="bounty_amount" type="number" min={1} value={amount} onChange={(e) => setAmount(Number(e.target.value) || 0)} />
        </Field>
      </Sheet>
      <Sheet
        open={sheet === 'withdraw'}
        onClose={() => setSheet(null)}
        title={api.t('requests.ui.withdraw')}
        footer={
          <>
            {cancel}
            <Button variant="danger" onClick={() => void act(`/api/p/requests/v1/requests/${id}/withdraw`)}>
              {api.t('requests.ui.withdraw')}
            </Button>
          </>
        }
      >
        <p>{api.t('requests.ui.withdraw_confirm')}</p>
        <p className="small muted">{api.t('requests.ui.withdraw_hint')}</p>
      </Sheet>
      <Sheet
        open={typeof sheet === 'object' && sheet !== null}
        onClose={() => setSheet(null)}
        title={api.t('requests.ui.reject')}
        footer={
          <>
            {cancel}
            <Button variant="danger" onClick={() => typeof sheet === 'object' && sheet && void act(`/api/p/requests/v1/fills/${sheet.reject}/reject`, { reason })}>
              {api.t('requests.ui.reject')}
            </Button>
          </>
        }
      >
        <Field label={api.t('requests.ui.reject_reason')}>
          <Textarea name="reject_reason" rows={3} maxLength={500} value={reason} onChange={(e) => setReason(e.target.value)} />
        </Field>
      </Sheet>
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

/** 魔力值页的求种条目：应求获得悬赏（如何获得）、悬赏求种（可以用来做什么） */
function Explain({ ctx, api }: SlotProps<{ section: string }>) {
  const earn = ctx.section === 'earn';
  return (
    <RuleItem title={api.t(earn ? 'requests.ui.rule_earn' : 'requests.ui.rule_spend')} value={api.t('requests.ui.rule_val')} to="/p/requests/" navigate={api.navigate} link={api.t('requests.ui.rule_link')}>
      {api.t(earn ? 'requests.ui.rule_earn_body' : 'requests.ui.rule_spend_body')}
    </RuleItem>
  );
}

export default {
  slots: {
    'nav.user_menu': { component: MenuItem, order: 30 },
    'bonus.explain': { component: Explain, order: 40 },
  },
  routes: [{ path: '/p/requests/*', component: Pages }],
} satisfies PluginFrontend;
