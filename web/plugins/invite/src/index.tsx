// invite 前端：注册页邀请码输入（`?invite=` 预填、失焦校验）、用户菜单入口、“我的邀请”页面（统计、邀请树、名额明细、连带处罚）；
// 个人页“邀请树”标签（邀请链与下级，本人或 invite.tree.read）；
// 后台用户详情页“邀请”标签（名额调整、暂停邀请权）、后台邀请管理（/admin/p/invite/：邀请码列表与撤销、发放系统 / 活动码、连带处罚复核）。
import { useEffect, useState, type FormEvent, type ReactNode } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, CodeBox, Field, Input, Kpis, LoadMore, NavItem, Note, PageHead, Panel, PLink, Select, Sheet, SkeletonRows, Status, Table, Tabs, Tag, type Tone } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'invite.ui.tab': '邀请',
  'invite.ui.quota': '邀请名额 {n}（永久 {p}）',
  'invite.ui.delta': '名额变化（正数发放，负数扣除）',
  'invite.ui.expires_days': '有效天数（留空永久）',
  'invite.ui.reason': '原因（必填）',
  'invite.ui.adjust': '调整名额',
  'invite.ui.suspensions': '邀请权暂停记录',
  'invite.ui.suspend_days': '暂停天数（留空永久）',
  'invite.ui.suspend': '暂停邀请权',
  'invite.ui.lift': '解除',
  'invite.ui.lifted': '已解除',
  'invite.ui.done': '已保存',
  'invite.ui.admin': '邀请管理',
  'invite.ui.codes': '邀请码',
  'invite.ui.penalties': '连带处罚',
  'invite.ui.kind_system': '系统码（单人）',
  'invite.ui.kind_campaign': '活动码（多人）',
  'invite.ui.max_uses': '可用次数',
  'invite.ui.all': '全部',
  'invite.ui.confirm': '确认',
  'invite.ui.reverse': '撤销处罚',
  'invite.ui.more': '加载更多',
  'invite.ui.uses': '{u}/{m} 次',
  'invite.ui.code': '邀请码',
  'invite.ui.valid': '有效 · 邀请人 {inviter} · {expires} 到期',
  'invite.ui.menu': '邀请',
  'invite.ui.title': '我的邀请',
  'invite.ui.balance': '可用名额',
  'invite.ui.outstanding': '未使用',
  'invite.ui.issue': '生成邀请',
  'invite.ui.email': '绑定邮箱（可选）',
  'invite.ui.note': '备注',
  'invite.ui.issued': '邀请已生成，请复制链接（关闭后无法再次查看）',
  'invite.ui.list': '已发出的邀请',
  'invite.ui.invitees': '我邀请的人',
  'invite.ui.revoke': '撤销',
  'invite.ui.status': '状态',
  'invite.ui.expires': '到期',
  'invite.ui.not_eligible': '暂不能发邀：{reason}',
  'invite.disabled': '站点已关闭邀请功能',
  'invite.account_inactive': '账号状态异常，不能发邀',
  'invite.account_too_new': '账号注册时间太短，满一定天数后才能发邀',
  'invite.privilege_suspended': '发邀权限已被暂停',
  'invite.outstanding_limit_reached': '未使用的邀请已达上限，请等待被使用或撤销后再发',
  'invite.daily_limit_reached': '今天的发邀次数已用完',
  'invite.quota_exhausted': '邀请名额已用完',
  'invite.ui.stats_issued': '累计发出',
  'invite.ui.stats_expired': '已过期',
  'invite.ui.stats_revoked': '已撤销',
  'invite.ui.stats_invitees': '已注册',
  'invite.ui.tree': '邀请树',
  'invite.ui.tree_levels': '各层人数：{levels}',
  'invite.ui.chain': '邀请链',
  'invite.ui.chain_none': '注册时未使用个人邀请',
  'invite.ui.direct': '直接邀请 {n} 人',
  'invite.ui.purged': '（已注销）',
  'invite.ui.denied': '只有本人和管理员可以查看邀请树',
  'invite.ui.ledger': '名额明细',
  'invite.ui.delta_col': '变化',
  'invite.ui.after': '余额',
  'invite.ui.my_penalties': '连带处罚',
  'invite.ui.no_penalties': '没有连带处罚',
  'invite.ui.penalty_desc': '被邀请人 #{invitee} 触发 {trigger}（第 {depth} 层）',
  'invite.ui.empty': '暂无记录',
  'invite.reason.issue': '生成邀请',
  'invite.reason.lot_expired': '名额过期',
  'invite.reason.initial': '初始名额',
  'invite.reason.level_promotion': '等级提升奖励',
  'invite.reason.periodic': '定期发放',
  'invite.reason.admin': '管理员发放',
  'invite.reason.deduct': '管理员扣除',
  'invite.reason.penalty': '连带处罚扣除',
  'invite.reason.penalty_refund': '处罚撤销退还',
  'invite.reason.revoked': '撤销邀请退还',
  'invite.reason.expired': '邀请过期退还',
  'invite.reason.shop': '商店购买',
  'invite.state.applied': '已执行',
  'invite.state.pending_review': '待复核',
  'invite.state.confirmed': '已确认',
  'invite.state.reversed': '已撤销',
  'invite.state.exempt': '已豁免',
  'invite.ui.close': '完成',
  'invite.ui.cancel': '取消',
  'invite.ui.lede': '邀请朋友加入。每条邀请只显示一次链接，请发出前确认对方可信 —— 被邀请人违规会连带影响你。',
  'invite.ui.user': '用户',
  'invite.ui.registered': '注册时间',
  'invite.ui.time': '时间',
  'invite.ui.event': '事项',
  'invite.ui.kind': '类型',
  'invite.ui.uses_col': '使用',
  'invite.ui.until': '截止',
  'invite.ui.rule': '规则',
  'invite.ui.target': '对象',
  'invite.ui.action': '操作',
  'invite.ui.adjust_hint': '调整会记入名额明细，用户可见原因。',
  'invite.ui.suspend_title': '暂停邀请权',
});
registerMessages('en', {
  'invite.ui.tab': 'Invites',
  'invite.ui.quota': '{n} invites ({p} permanent)',
  'invite.ui.delta': 'Change (positive grants, negative deducts)',
  'invite.ui.expires_days': 'Valid for days (blank = permanent)',
  'invite.ui.reason': 'Reason (required)',
  'invite.ui.adjust': 'Adjust quota',
  'invite.ui.suspensions': 'Invite suspensions',
  'invite.ui.suspend_days': 'Suspend for days (blank = permanent)',
  'invite.ui.suspend': 'Suspend inviting',
  'invite.ui.lift': 'Lift',
  'invite.ui.lifted': 'lifted',
  'invite.ui.done': 'Saved',
  'invite.ui.admin': 'Invites',
  'invite.ui.codes': 'Codes',
  'invite.ui.penalties': 'Penalties',
  'invite.ui.kind_system': 'System code (single use)',
  'invite.ui.kind_campaign': 'Campaign code (multi-use)',
  'invite.ui.max_uses': 'Uses',
  'invite.ui.all': 'All',
  'invite.ui.confirm': 'Confirm',
  'invite.ui.reverse': 'Reverse',
  'invite.ui.more': 'Load more',
  'invite.ui.uses': '{u}/{m} uses',
  'invite.ui.code': 'Invite code',
  'invite.ui.valid': 'Valid · from {inviter} · expires {expires}',
  'invite.ui.menu': 'Invites',
  'invite.ui.title': 'My invites',
  'invite.ui.balance': 'Available',
  'invite.ui.outstanding': 'Outstanding',
  'invite.ui.issue': 'Create invite',
  'invite.ui.email': 'Bind email (optional)',
  'invite.ui.note': 'Note',
  'invite.ui.issued': 'Invite created — copy the link now (it will not be shown again)',
  'invite.ui.list': 'Sent invites',
  'invite.ui.invitees': 'People I invited',
  'invite.ui.revoke': 'Revoke',
  'invite.ui.status': 'Status',
  'invite.ui.expires': 'Expires',
  'invite.ui.not_eligible': 'You cannot invite yet: {reason}',
  'invite.disabled': 'Invitations are turned off',
  'invite.account_inactive': 'Your account is not active',
  'invite.account_too_new': 'Your account is too new to send invitations',
  'invite.privilege_suspended': 'Your invitation privilege is suspended',
  'invite.outstanding_limit_reached': 'You have too many unused invitations',
  'invite.daily_limit_reached': 'Daily invitation limit reached',
  'invite.quota_exhausted': 'No invitation quota left',
  'invite.ui.stats_issued': 'Issued',
  'invite.ui.stats_expired': 'Expired',
  'invite.ui.stats_revoked': 'Revoked',
  'invite.ui.stats_invitees': 'Registered',
  'invite.ui.tree': 'Invite tree',
  'invite.ui.tree_levels': 'Per level: {levels}',
  'invite.ui.chain': 'Invited by',
  'invite.ui.chain_none': 'Did not register with a personal invite',
  'invite.ui.direct': '{n} direct invitees',
  'invite.ui.purged': '(deleted)',
  'invite.ui.denied': 'Only the user and staff can see the invite tree',
  'invite.ui.ledger': 'Quota history',
  'invite.ui.delta_col': 'Change',
  'invite.ui.after': 'Balance',
  'invite.ui.my_penalties': 'Penalties',
  'invite.ui.no_penalties': 'No penalties',
  'invite.ui.penalty_desc': 'Invitee #{invitee} triggered {trigger} (level {depth})',
  'invite.ui.empty': 'Nothing yet',
  'invite.reason.issue': 'Invite created',
  'invite.reason.lot_expired': 'Quota expired',
  'invite.reason.initial': 'Initial quota',
  'invite.reason.level_promotion': 'Promotion reward',
  'invite.reason.periodic': 'Periodic grant',
  'invite.reason.admin': 'Granted by staff',
  'invite.reason.deduct': 'Deducted by staff',
  'invite.reason.penalty': 'Penalty',
  'invite.reason.penalty_refund': 'Penalty reversed',
  'invite.reason.revoked': 'Refund (revoked)',
  'invite.reason.expired': 'Refund (expired)',
  'invite.reason.shop': 'Shop purchase',
  'invite.state.applied': 'applied',
  'invite.state.pending_review': 'pending review',
  'invite.state.confirmed': 'confirmed',
  'invite.state.reversed': 'reversed',
  'invite.state.exempt': 'exempt',
  'invite.ui.close': 'Done',
  'invite.ui.cancel': 'Cancel',
  'invite.ui.lede': 'Invite friends. Each link is shown only once — invite people you trust; their violations affect you too.',
  'invite.ui.user': 'User',
  'invite.ui.registered': 'Registered',
  'invite.ui.time': 'Time',
  'invite.ui.event': 'Event',
  'invite.ui.kind': 'Kind',
  'invite.ui.uses_col': 'Uses',
  'invite.ui.until': 'Until',
  'invite.ui.rule': 'Rule',
  'invite.ui.target': 'Target',
  'invite.ui.action': 'Action',
  'invite.ui.adjust_hint': 'Adjustments appear in the quota history with the reason.',
  'invite.ui.suspend_title': 'Suspend inviting',
});

interface FormBinding {
  get(key: string): unknown;
  set(key: string, value: unknown): void;
}

function errText(api: PluginApi, e: unknown): string {
  if (e instanceof ApiError) {
    const s = api.t(e.code);
    return s === e.code ? e.message : s;
  }
  return String(e);
}

const when = (s: string) => <span className="small muted nowrap num">{new Date(s).toLocaleString()}</span>;
const date = (s: string) => <span className="small muted nowrap num">{new Date(s).toLocaleDateString()}</span>;
const STATUS_TONE: Record<string, Tone> = { active: 'ok', exhausted: 'info', expired: 'mute', revoked: 'bad', suspended: 'bad', banned: 'bad', disabled: 'bad' };

function CodeField({ ctx, api }: SlotProps<{ form: FormBinding }>) {
  const [code, setCode] = useState(() => new URLSearchParams(window.location.search).get('invite') ?? '');
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  useEffect(() => {
    ctx.form.set('invite_code', code.trim() || undefined);
    // 只在码变化时写入绑定
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [code]);
  // 链接带码（?invite=）时立即校验一次
  useEffect(() => {
    if (code.trim()) void check();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  async function check() {
    if (!code.trim()) {
      setMsg(null);
      return;
    }
    try {
      const r = await api.json<{ inviter_username: string | null; expires_at: string }>('/api/p/invite/v1/invite-checks', { json: { code } });
      setMsg({ ok: true, text: api.t('invite.ui.valid', { inviter: r.inviter_username ?? '-', expires: new Date(r.expires_at).toLocaleString() }) });
    } catch (e) {
      setMsg({ ok: false, text: errText(api, e) });
    }
  }
  return (
    <Field label={api.t('invite.ui.code')} error={msg && !msg.ok ? msg.text : null} hint={msg?.ok ? msg.text : undefined}>
      <Input name="invite_code" value={code} onChange={(e) => setCode(e.target.value)} onBlur={() => void check()} autoComplete="off" />
    </Field>
  );
}

interface Quota {
  balance: number;
  outstanding: number;
  max_outstanding: number;
}

function MenuItem({ api }: SlotProps) {
  const [q, setQ] = useState<Quota | null>(null);
  useEffect(() => {
    void api.json<Quota>('/api/p/invite/v1/users/me/quota').then(setQ).catch(() => undefined);
  }, [api]);
  return (
    <NavItem to="/p/invite/" navigate={api.navigate} count={q?.balance}>
      {api.t('invite.ui.menu')}
    </NavItem>
  );
}

interface Invite {
  id: number;
  status: string;
  code_hint: string;
  expires_at: string;
  email_domain: string | null;
  note: string | null;
}

interface Eligibility {
  eligible: boolean;
  checks: { name: string; ok: boolean; code: string | null }[];
}

interface InviteStats {
  issued: number;
  outstanding: number;
  expired: number;
  revoked: number;
  invitees: number;
}

interface Invitee {
  user_id: number;
  username: string | null;
  status: string | null;
  registered_at: string;
}

function UserLink({ api, id, name }: { api: PluginApi; id: number; name: string | null }) {
  return (
    <PLink to={`/users/${id}`} navigate={api.navigate}>
      {name ?? `#${id}`}
    </PLink>
  );
}

/** 生成邀请：抽屉表单，生成后在同一抽屉里展示一次性链接 */
function IssueSheet({ api, open, onClose, blocker, onIssued }: { api: PluginApi; open: boolean; onClose(): void; blocker: string | null; onIssued(): void }) {
  const [email, setEmail] = useState('');
  const [note, setNote] = useState('');
  const [issued, setIssued] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const close = () => {
    setIssued(null);
    onClose();
  };
  const issue = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      const r = await api.json<{ url: string }>('/api/p/invite/v1/users/me/invites', {
        json: { email: email || undefined, note: note || undefined, request_id: crypto.randomUUID() },
      });
      setIssued(`${window.location.origin}${r.url}`);
      setEmail('');
      setNote('');
      onIssued();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <Sheet
      open={open}
      onClose={close}
      title={api.t('invite.ui.issue')}
      footer={
        issued ? (
          <Button variant="primary" onClick={close}>
            {api.t('invite.ui.close')}
          </Button>
        ) : (
          <>
            <Button variant="ghost" onClick={close}>
              {api.t('invite.ui.cancel')}
            </Button>
            <Button variant="primary" type="submit" form="invite-issue" disabled={!!blocker || busy}>
              {api.t('invite.ui.issue')}
            </Button>
          </>
        )
      }
    >
      {issued ? (
        <div className="stack">
          <Note tone="ok">{api.t('invite.ui.issued')}</Note>
          <CodeBox copy={issued}>
            <span data-testid="invite-url">{issued}</span>
          </CodeBox>
        </div>
      ) : (
        <form id="invite-issue" className="form" onSubmit={(e) => void issue(e)}>
          {blocker && <Note tone="warn">{api.t('invite.ui.not_eligible', { reason: blocker })}</Note>}
          <Field label={api.t('invite.ui.email')}>
            <Input type="email" name="invite_email" value={email} onChange={(e) => setEmail(e.target.value)} />
          </Field>
          <Field label={api.t('invite.ui.note')}>
            <Input name="invite_note" value={note} maxLength={200} onChange={(e) => setNote(e.target.value)} />
          </Field>
        </form>
      )}
    </Sheet>
  );
}

function MyInvites({ api }: { api: PluginApi; path: string }) {
  const [quota, setQuota] = useState<Quota | null>(null);
  const [elig, setElig] = useState<Eligibility | null>(null);
  const [list, setList] = useState<Invite[] | null>(null);
  const [invitees, setInvitees] = useState<Invitee[]>([]);
  const [stats, setStats] = useState<InviteStats | null>(null);
  const [tab, setTab] = useState('sent');
  const [issuing, setIssuing] = useState(false);
  const reload = async () => {
    void api.json<InviteStats>('/api/p/invite/v1/users/me/stats').then(setStats).catch(() => undefined);
    const [q, e, l, t] = await Promise.all([
      api.json<Quota>('/api/p/invite/v1/users/me/quota'),
      api.json<Eligibility>('/api/p/invite/v1/users/me/eligibility').catch(() => null),
      api.json<{ items: Invite[] }>('/api/p/invite/v1/users/me/invites'),
      api.json<{ items: Invitee[] }>('/api/p/invite/v1/users/me/invitees'),
    ]);
    setQuota(q);
    setElig(e);
    setList(l.items);
    setInvitees(t.items);
  };
  useEffect(() => {
    void reload().catch((e: unknown) => api.toast(errText(api, e), 'error'));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const revoke = async (id: number) => {
    try {
      await api.json(`/api/p/invite/v1/users/me/invites/${id}`, { method: 'DELETE' });
      await reload();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const b = elig?.checks.find((c) => !c.ok);
  const blocker = b ? api.t(b.code ?? b.name) : null;
  const uid = api.user()?.id ?? 0;
  return (
    <div className="stack">
      <PageHead
        title={api.t('invite.ui.title')}
        lede={api.t('invite.ui.lede')}
        actions={
          <Button variant="primary" onClick={() => setIssuing(true)}>
            {api.t('invite.ui.issue')}
          </Button>
        }
      />
      <Kpis
        items={[
          { label: api.t('invite.ui.balance'), value: quota?.balance ?? '…', tone: quota && quota.balance > 0 ? 'up' : undefined },
          { label: api.t('invite.ui.outstanding'), value: quota ? `${quota.outstanding} / ${quota.max_outstanding}` : '…' },
          { label: api.t('invite.ui.stats_issued'), value: stats?.issued ?? '…', hint: stats ? `${api.t('invite.ui.stats_expired')} ${stats.expired} · ${api.t('invite.ui.stats_revoked')} ${stats.revoked}` : undefined },
          { label: api.t('invite.ui.stats_invitees'), value: stats?.invitees ?? '…' },
        ]}
      />
      <Panel flush primary>
        {blocker && <Note tone="warn">{api.t('invite.ui.not_eligible', { reason: blocker })}</Note>}
        <Tabs
          variant="flush"
          value={tab}
          onChange={setTab}
          tabs={[
            { id: 'sent', label: api.t('invite.ui.list'), count: list?.length },
            { id: 'invitees', label: api.t('invite.ui.invitees'), count: invitees.length },
            { id: 'tree', label: api.t('invite.ui.tree') },
            { id: 'ledger', label: api.t('invite.ui.ledger') },
            { id: 'penalties', label: api.t('invite.ui.my_penalties') },
          ]}
        />
        {tab === 'sent' ? (
          list === null ? (
            <SkeletonRows n={3} />
          ) : (
            <Table
              rows={list}
              rowKey={(r) => r.id}
              empty={api.t('invite.ui.empty')}
              columns={[
                { key: 'hint', header: api.t('invite.ui.code'), render: (r) => <span className="pt-mono">…{r.code_hint}</span> },
                { key: 'status', header: api.t('invite.ui.status'), render: (r) => <Status tone={STATUS_TONE[r.status] ?? 'mute'}>{r.status}</Status> },
                { key: 'exp', header: api.t('invite.ui.expires'), render: (r) => when(r.expires_at) },
                { key: 'note', header: api.t('invite.ui.note'), render: (r) => <span className="small">{r.note ?? r.email_domain ?? ''}</span> },
                {
                  key: 'act',
                  header: '',
                  right: true,
                  render: (r) =>
                    r.status === 'active' ? (
                      <Button size="sm" variant="ghost" onClick={() => void revoke(r.id)}>
                        {api.t('invite.ui.revoke')}
                      </Button>
                    ) : null,
                },
              ]}
            />
          )
        ) : tab === 'invitees' ? (
          <Table
            rows={invitees}
            rowKey={(r) => r.user_id}
            empty={api.t('invite.ui.empty')}
            columns={[
              { key: 'u', header: api.t('invite.ui.user'), render: (r) => <UserLink api={api} id={r.user_id} name={r.username} /> },
              { key: 's', header: api.t('invite.ui.status'), render: (r) => (r.status ? <Status tone={STATUS_TONE[r.status] ?? 'mute'}>{r.status}</Status> : '') },
              { key: 'd', header: api.t('invite.ui.registered'), right: true, render: (r) => date(r.registered_at) },
            ]}
          />
        ) : tab === 'tree' ? (
          <div className="pad">
            <Tree api={api} uid={uid} />
          </div>
        ) : tab === 'ledger' ? (
          <QuotaLedger api={api} />
        ) : (
          <MyPenalties api={api} />
        )}
      </Panel>
      <IssueSheet api={api} open={issuing} onClose={() => setIssuing(false)} blocker={blocker} onIssued={() => void reload()} />
    </div>
  );
}

interface TreeNode {
  user_id: number;
  depth: number;
  inviter_id: number;
  username: string | null;
  status: string | null;
  purged: boolean;
  direct_count: number;
  registered_at: string;
}

/** 邀请树：按层加载（深度 3），按邀请人深度优先展开成缩进列表 */
function Tree({ api, uid }: { api: PluginApi; uid: number }) {
  const [nodes, setNodes] = useState<TreeNode[] | null>(null);
  const [levels, setLevels] = useState<number[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const [denied, setDenied] = useState(false);
  const load = async (cursor: string | null) => {
    const q = new URLSearchParams({ depth: '3' });
    if (cursor) q.set('cursor', cursor);
    try {
      const r = await api.json<{ level_counts: number[]; items: TreeNode[]; next_cursor: string | null }>(`/api/p/invite/v1/users/${uid}/tree?${q.toString()}`);
      setNodes((old) => (cursor ? [...(old ?? []), ...r.items] : r.items));
      setLevels(r.level_counts);
      setNext(r.next_cursor);
    } catch (e) {
      if (e instanceof ApiError && e.status === 403) setDenied(true);
      else setNodes([]);
    }
  };
  useEffect(() => {
    if (uid) void load(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [uid]);
  if (denied) return <Note>{api.t('invite.ui.denied')}</Note>;
  if (nodes === null) return <SkeletonRows n={3} />;
  const flat: ReactNode[] = [];
  const walk = (parent: number, d: number) => {
    for (const n of nodes.filter((x) => x.inviter_id === parent)) {
      flat.push(
        <div key={n.user_id} style={{ '--d': d } as React.CSSProperties}>
          {d > 0 && <span className="muted">└</span>}
          {n.purged ? <span className="muted">{`#${n.user_id} ${api.t('invite.ui.purged')}`}</span> : <UserLink api={api} id={n.user_id} name={n.username} />}
          {n.status && n.status !== 'active' && <Status tone="bad">{n.status}</Status>}
          <span className="tiny muted">
            {new Date(n.registered_at).toLocaleDateString()}
            {n.direct_count > 0 && ` · ${api.t('invite.ui.direct', { n: n.direct_count })}`}
          </span>
        </div>,
      );
      walk(n.user_id, d + 1);
    }
  };
  walk(uid, 0);
  return (
    <div className="stack-s">
      {levels.length > 0 && <p className="small muted">{api.t('invite.ui.tree_levels', { levels: levels.join(' / ') })}</p>}
      {flat.length === 0 ? <p className="muted">{api.t('invite.ui.empty')}</p> : <div className="tree">{flat}</div>}
      {next && <LoadMore onClick={() => void load(next)} label={api.t('invite.ui.more')} />}
    </div>
  );
}

/** 个人页“邀请树”标签：邀请链（上级）与下级 */
function ProfileTab({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const uid = ctx.profile.id;
  const [chain, setChain] = useState<{ user_id: number; depth: number; username: string | null }[] | null>(null);
  const [denied, setDenied] = useState(false);
  useEffect(() => {
    setDenied(false);
    void api
      .json<{ items: { user_id: number; depth: number; username: string | null }[] }>(`/api/p/invite/v1/users/${uid}/ancestors`)
      .then((r) => setChain(r.items))
      .catch((e: unknown) => {
        if (e instanceof ApiError && e.status === 403) setDenied(true);
      });
  }, [api, uid]);
  if (denied) return <Note>{api.t('invite.ui.denied')}</Note>;
  return (
    <div className="stack">
      <dl className="kv">
        <dt>{api.t('invite.ui.chain')}</dt>
        <dd>
          {chain === null ? '…' : chain.length === 0 ? (
            <span className="muted">{api.t('invite.ui.chain_none')}</span>
          ) : (
            [...chain].reverse().map((a, i) => (
              <span key={a.user_id}>
                {i > 0 && ' → '}
                <UserLink api={api} id={a.user_id} name={a.username} />
              </span>
            ))
          )}
        </dd>
      </dl>
      <Tree key={uid} api={api} uid={uid} />
    </div>
  );
}

interface LedgerRow {
  id: number;
  delta: number;
  reason: string;
  ref: string | null;
  balance_after: number;
  note: string | null;
  created_at: string;
}

function QuotaLedger({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<LedgerRow[] | null>(null);
  const [next, setNext] = useState<string | null>(null);
  const load = async (cursor: string | null) => {
    const r = await api.json<{ items: LedgerRow[]; next_cursor: string | null }>(`/api/p/invite/v1/users/me/quota/ledger${cursor ? `?cursor=${cursor}` : ''}`);
    setRows((old) => (cursor ? [...(old ?? []), ...r.items] : r.items));
    setNext(r.next_cursor);
  };
  useEffect(() => {
    void load(null).catch(() => setRows([]));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const reason = (r: string) => {
    const k = `invite.reason.${r}`;
    return api.t(k) === k ? r : api.t(k);
  };
  if (rows === null) return <SkeletonRows n={3} />;
  return (
    <>
      <Table
        rows={rows}
        rowKey={(r) => r.id}
        empty={api.t('invite.ui.empty')}
        columns={[
          { key: 't', header: api.t('invite.ui.time'), render: (r) => when(r.created_at) },
          { key: 'r', header: api.t('invite.ui.event'), render: (r) => <>{reason(r.reason)}{r.note && <div className="tiny muted">{r.note}</div>}</> },
          { key: 'd', header: api.t('invite.ui.delta_col'), right: true, render: (r) => <span className={r.delta >= 0 ? 'num up' : 'num down'}>{r.delta >= 0 ? `+${r.delta}` : r.delta}</span> },
          { key: 'b', header: api.t('invite.ui.after'), right: true, render: (r) => <span className="num">{r.balance_after}</span> },
        ]}
      />
      {next && <LoadMore onClick={() => void load(next)} label={api.t('invite.ui.more')} />}
    </>
  );
}

const PENALTY_TONE: Record<string, Tone> = { applied: 'bad', pending_review: 'warn', confirmed: 'bad', reversed: 'mute', exempt: 'mute' };

function MyPenalties({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<Penalty[] | null>(null);
  useEffect(() => {
    void api
      .json<{ items: Penalty[] }>('/api/p/invite/v1/users/me/penalties')
      .then((r) => setRows(r.items))
      .catch(() => setRows([]));
  }, [api]);
  if (rows === null) return <SkeletonRows n={3} />;
  return (
    <Table
      rows={rows}
      rowKey={(r) => r.id}
      empty={api.t('invite.ui.no_penalties')}
      columns={[
        { key: 't', header: api.t('invite.ui.time'), render: (r) => when(r.created_at) },
        { key: 'd', header: api.t('invite.ui.event'), render: (r) => api.t('invite.ui.penalty_desc', { invitee: r.invitee_id, trigger: r.trigger, depth: r.depth }) },
        { key: 's', header: api.t('invite.ui.status'), right: true, render: (r) => <Status tone={PENALTY_TONE[r.state] ?? 'mute'}>{api.t(`invite.state.${r.state}`)}</Status> },
      ]}
    />
  );
}

interface AdminQuota {
  balance: number;
  permanent: number;
}
interface Suspension {
  id: number;
  source: string;
  reason: string | null;
  until: string | null;
  created_at: string;
  lifted_at: string | null;
}

const day = (n: string) => (n.trim() ? Date.now() + Number(n) * 86_400_000 : null);

/** 后台用户详情页“邀请”标签：名额（调整）与邀请权暂停；两个操作都在抽屉里填写 */
function UserTab({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const uid = ctx.profile.id;
  const [q, setQ] = useState<AdminQuota | null>(null);
  const [sus, setSus] = useState<Suspension[]>([]);
  const [sheet, setSheet] = useState<'adjust' | 'suspend' | null>(null);
  const [f, setF] = useState({ delta: '', days: '', reason: '', sdays: '', sreason: '' });
  const load = () => {
    void api.json<AdminQuota>(`/api/p/invite/v1/admin/users/${uid}/quota`).then(setQ);
    void api.json<{ items: Suspension[] }>(`/api/p/invite/v1/admin/users/${uid}/suspensions`).then((r) => setSus(r.items));
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, [uid]);
  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      api.toast(api.t('invite.ui.done'), 'success');
      setSheet(null);
      load();
      return true;
    } catch (e) {
      api.toast(errText(api, e), 'error');
      return false;
    }
  };
  const adjust = (e: FormEvent) => {
    e.preventDefault();
    void act(() =>
      api.json(`/api/p/invite/v1/admin/users/${uid}/quota-adjustments`, {
        json: { delta: Number(f.delta), reason: f.reason, expires_at_ms: day(f.days), request_id: crypto.randomUUID() },
      }),
    ).then((ok) => ok && setF((o) => ({ ...o, delta: '', days: '', reason: '' })));
  };
  const suspend = (e: FormEvent) => {
    e.preventDefault();
    void act(() => api.json(`/api/p/invite/v1/admin/users/${uid}/suspensions`, { json: { until_ms: day(f.sdays), reason: f.sreason }, idempotent: true })).then(
      (ok) => ok && setF((o) => ({ ...o, sdays: '', sreason: '' })),
    );
  };
  if (!q) return <SkeletonRows n={3} />;
  const cancel = (
    <Button variant="ghost" onClick={() => setSheet(null)}>
      {api.t('invite.ui.cancel')}
    </Button>
  );
  return (
    <div className="stack">
      <div className="row-sb">
        <strong className="num">{api.t('invite.ui.quota', { n: q.balance, p: q.permanent })}</strong>
        <div className="form-acts">
          <Button size="sm" onClick={() => setSheet('adjust')}>
            {api.t('invite.ui.adjust')}
          </Button>
          <Button size="sm" variant="danger" onClick={() => setSheet('suspend')}>
            {api.t('invite.ui.suspend')}
          </Button>
        </div>
      </div>
      <Panel title={api.t('invite.ui.suspensions')} flush>
        <Table
          rows={sus}
          rowKey={(r) => r.id}
          empty={api.t('invite.ui.empty')}
          columns={[
            { key: 't', header: api.t('invite.ui.time'), render: (r) => when(r.created_at) },
            { key: 'r', header: api.t('invite.ui.reason'), render: (r) => <><Tag>{r.source}</Tag> {r.reason ?? ''}</> },
            { key: 'u', header: api.t('invite.ui.until'), render: (r) => (r.until ? when(r.until) : '∞') },
            {
              key: 'x',
              header: '',
              right: true,
              render: (r) =>
                r.lifted_at ? (
                  <Status tone="mute">{api.t('invite.ui.lifted')}</Status>
                ) : (
                  <Button size="sm" variant="ghost" onClick={() => void act(() => api.json(`/api/p/invite/v1/admin/users/${uid}/suspensions/${r.id}`, { method: 'DELETE' }))}>
                    {api.t('invite.ui.lift')}
                  </Button>
                ),
            },
          ]}
        />
      </Panel>
      <Sheet
        open={sheet === 'adjust'}
        onClose={() => setSheet(null)}
        title={api.t('invite.ui.adjust')}
        footer={
          <>
            {cancel}
            <Button type="submit" form="invite-adjust" variant="primary" disabled={!Number(f.delta) || !f.reason.trim()}>
              {api.t('invite.ui.adjust')}
            </Button>
          </>
        }
      >
        <form id="invite-adjust" className="form" onSubmit={adjust}>
          <p className="small muted">{api.t('invite.ui.adjust_hint')}</p>
          <Field label={api.t('invite.ui.delta')}>
            <Input name="invite_delta" type="number" value={f.delta} onChange={(e) => setF({ ...f, delta: e.target.value })} />
          </Field>
          <Field label={api.t('invite.ui.expires_days')}>
            <Input name="invite_days" type="number" min={1} value={f.days} onChange={(e) => setF({ ...f, days: e.target.value })} />
          </Field>
          <Field label={api.t('invite.ui.reason')}>
            <Input name="invite_reason" maxLength={500} value={f.reason} onChange={(e) => setF({ ...f, reason: e.target.value })} />
          </Field>
        </form>
      </Sheet>
      <Sheet
        open={sheet === 'suspend'}
        onClose={() => setSheet(null)}
        title={api.t('invite.ui.suspend_title')}
        footer={
          <>
            {cancel}
            <Button type="submit" form="invite-suspend" variant="danger" disabled={!f.sreason.trim()}>
              {api.t('invite.ui.suspend')}
            </Button>
          </>
        }
      >
        <form id="invite-suspend" className="form" onSubmit={suspend}>
          <Field label={api.t('invite.ui.suspend_days')}>
            <Input name="suspend_days" type="number" min={1} value={f.sdays} onChange={(e) => setF({ ...f, sdays: e.target.value })} />
          </Field>
          <Field label={api.t('invite.ui.reason')}>
            <Input name="suspend_reason" maxLength={500} value={f.sreason} onChange={(e) => setF({ ...f, sreason: e.target.value })} />
          </Field>
        </form>
      </Sheet>
    </div>
  );
}

interface AdminInvite {
  id: number;
  kind: string;
  source: string;
  status: string;
  code_hint: string;
  inviter_id: number | null;
  max_uses: number;
  use_count: number;
  expires_at: string;
  note: string | null;
  created_at: string;
}
interface Penalty {
  id: number;
  rule_id: string;
  trigger: string;
  invitee_id: number;
  target_user_id: number;
  depth: number;
  state: string;
  version: number;
  created_at: string;
}

const STATUSES = ['', 'active', 'exhausted', 'expired', 'revoked'];

function Codes({ api, issuing, setIssuing }: { api: PluginApi; issuing: boolean; setIssuing(v: boolean): void }) {
  const [status, setStatus] = useState('active');
  const [rows, setRows] = useState<AdminInvite[] | null>(null);
  const [next, setNext] = useState<string | null>(null);
  const [f, setF] = useState({ kind: 'system', max_uses: '1', expires_days: '7', note: '' });
  const [issued, setIssued] = useState<string | null>(null);
  const load = async (cursor: string | null) => {
    const q = new URLSearchParams();
    if (status) q.set('status', status);
    if (cursor) q.set('cursor', cursor);
    const r = await api.json<{ items: AdminInvite[]; next_cursor: string | null }>(`/api/p/invite/v1/admin/invites?${q.toString()}`);
    setRows((old) => (cursor ? [...(old ?? []), ...r.items] : r.items));
    setNext(r.next_cursor);
  };
  useEffect(() => {
    void load(null).catch((e: unknown) => api.toast(errText(api, e), 'error'));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status]);
  const issue = async (e: FormEvent) => {
    e.preventDefault();
    try {
      const r = await api.json<{ code: string; url: string }>('/api/p/invite/v1/admin/invites', {
        json: { kind: f.kind, max_uses: Number(f.max_uses), expires_days: Number(f.expires_days), note: f.note || undefined, request_id: crypto.randomUUID() },
      });
      setIssued(`${location.origin}${r.url}`);
      void load(null);
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  const revoke = async (id: number) => {
    try {
      await api.json(`/api/p/invite/v1/admin/invites/${id}`, { method: 'DELETE' });
      void load(null);
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  const close = () => {
    setIssued(null);
    setIssuing(false);
  };
  return (
    <>
      <Tabs variant="flush" value={status} onChange={setStatus} tabs={STATUSES.map((s) => ({ id: s, label: s || api.t('invite.ui.all') }))} />
      {rows === null ? (
        <SkeletonRows n={4} />
      ) : (
        <Table
          rows={rows}
          rowKey={(r) => r.id}
          empty={api.t('invite.ui.empty')}
          minWidth={820}
          columns={[
            { key: 'c', header: api.t('invite.ui.code'), render: (r) => <span className="pt-mono">{r.code_hint}…</span> },
            { key: 'k', header: api.t('invite.ui.kind'), render: (r) => <><Tag tone={r.kind === 'campaign' ? 'o' : undefined}>{r.kind}</Tag> <span className="tiny muted">{r.source}</span></> },
            { key: 's', header: api.t('invite.ui.status'), render: (r) => <Status tone={STATUS_TONE[r.status] ?? 'mute'}>{r.status}</Status> },
            { key: 'u', header: api.t('invite.ui.uses_col'), right: true, render: (r) => <span className="num">{api.t('invite.ui.uses', { u: r.use_count, m: r.max_uses })}</span> },
            { key: 'e', header: api.t('invite.ui.expires'), render: (r) => date(r.expires_at) },
            { key: 'n', header: api.t('invite.ui.note'), render: (r) => <span className="small muted">{r.note}</span> },
            {
              key: 'x',
              header: '',
              right: true,
              render: (r) =>
                r.status === 'active' && (
                  <Button size="sm" variant="ghost" onClick={() => void revoke(r.id)}>
                    {api.t('invite.ui.revoke')}
                  </Button>
                ),
            },
          ]}
        />
      )}
      {next && <LoadMore onClick={() => void load(next)} label={api.t('invite.ui.more')} />}
      <Sheet
        open={issuing}
        onClose={close}
        title={api.t('invite.ui.issue')}
        footer={
          issued ? (
            <Button variant="primary" onClick={close}>
              {api.t('invite.ui.close')}
            </Button>
          ) : (
            <>
              <Button variant="ghost" onClick={close}>
                {api.t('invite.ui.cancel')}
              </Button>
              <Button type="submit" form="invite-admin-issue" variant="primary">
                {api.t('invite.ui.issue')}
              </Button>
            </>
          )
        }
      >
        {issued ? (
          <div className="stack" data-testid="issued-invite">
            <Note tone="ok">{api.t('invite.ui.issued')}</Note>
            <CodeBox copy={issued}>{issued}</CodeBox>
          </div>
        ) : (
          <form id="invite-admin-issue" className="form" onSubmit={(e) => void issue(e)}>
            <Field label={api.t('invite.ui.kind')}>
              <Select
                name="invite_kind"
                value={f.kind}
                onChange={(e) => setF({ ...f, kind: e.target.value, max_uses: e.target.value === 'campaign' ? '50' : '1' })}
                options={[
                  { value: 'system', label: api.t('invite.ui.kind_system') },
                  { value: 'campaign', label: api.t('invite.ui.kind_campaign') },
                ]}
              />
            </Field>
            <div className="form-2">
              <Field label={api.t('invite.ui.max_uses')}>
                <Input name="invite_max_uses" type="number" min={1} value={f.max_uses} onChange={(e) => setF({ ...f, max_uses: e.target.value })} />
              </Field>
              <Field label={api.t('invite.ui.expires_days')}>
                <Input name="invite_expires" type="number" min={1} value={f.expires_days} onChange={(e) => setF({ ...f, expires_days: e.target.value })} />
              </Field>
            </div>
            <Field label={api.t('invite.ui.note')}>
              <Input name="invite_note" maxLength={200} value={f.note} onChange={(e) => setF({ ...f, note: e.target.value })} />
            </Field>
          </form>
        )}
      </Sheet>
    </>
  );
}

function Penalties({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<Penalty[] | null>(null);
  const load = () =>
    void api
      .json<{ items: Penalty[] }>('/api/p/invite/v1/admin/penalties?status=pending_review,applied')
      .then((r) => setRows(r.items))
      .catch(() => setRows([]));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, []);
  const review = async (p: Penalty, decision: string) => {
    try {
      await api.json(`/api/p/invite/v1/admin/penalties/${p.id}/reviews`, { json: { decision, version: p.version }, idempotent: true });
      load();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  if (rows === null) return <SkeletonRows n={4} />;
  return (
    <Table
      rows={rows}
      rowKey={(r) => r.id}
      empty={api.t('invite.ui.no_penalties')}
      minWidth={640}
      columns={[
        { key: 't', header: api.t('invite.ui.time'), render: (r) => when(r.created_at) },
        { key: 'r', header: api.t('invite.ui.rule'), render: (r) => <span className="small">{`${r.rule_id} · ${r.trigger}`}</span> },
        { key: 'u', header: api.t('invite.ui.target'), render: (r) => <span className="num small">{`#${r.target_user_id} ← #${r.invitee_id} (${r.depth})`}</span> },
        { key: 's', header: api.t('invite.ui.status'), render: (r) => <Status tone={PENALTY_TONE[r.state] ?? 'mute'}>{api.t(`invite.state.${r.state}`)}</Status> },
        {
          key: 'x',
          header: '',
          right: true,
          render: (r) => (
            <div className="form-acts">
              {r.state === 'pending_review' && (
                <Button size="sm" onClick={() => void review(r, 'confirm')}>
                  {api.t('invite.ui.confirm')}
                </Button>
              )}
              <Button size="sm" variant="ghost" onClick={() => void review(r, 'reverse')}>
                {api.t('invite.ui.reverse')}
              </Button>
            </div>
          ),
        },
      ]}
    />
  );
}

function Admin({ api }: { api: PluginApi }) {
  const [tab, setTab] = useState('codes');
  const [issuing, setIssuing] = useState(false);
  return (
    <div className="stack">
      <PageHead
        title={api.t('invite.ui.admin')}
        actions={
          tab === 'codes' && (
            <Button variant="primary" onClick={() => setIssuing(true)}>
              {api.t('invite.ui.issue')}
            </Button>
          )
        }
      />
      <Tabs
        variant="sub"
        value={tab}
        onChange={setTab}
        tabs={[
          { id: 'codes', label: api.t('invite.ui.codes') },
          { id: 'penalties', label: api.t('invite.ui.penalties') },
        ]}
      />
      <Panel flush primary>
        {tab === 'codes' ? <Codes api={api} issuing={issuing} setIssuing={setIssuing} /> : <Penalties api={api} />}
      </Panel>
    </div>
  );
}

export default {
  slots: {
    'auth.register.form_extra': { component: CodeField, order: 10 },
    'admin.user.tabs': { component: UserTab, order: 30, title_term: 'invite.ui.tab' },
    'nav.user_menu': { component: MenuItem, order: 50 },
    'user.profile.tabs': { component: ProfileTab, order: 20, title_term: 'invite.ui.tree' },
  },
  routes: [
    { path: '/p/invite/*', component: MyInvites },
    { path: '/admin/p/invite/*', component: Admin },
  ],
} satisfies PluginFrontend;
