// invite 前端：注册页邀请码输入（`?invite=` 预填、失焦校验）、用户菜单入口、“我的邀请”页面；
// 后台用户详情页“邀请”标签（名额调整、暂停邀请权）、后台邀请管理（/admin/p/invite/：邀请码列表与撤销、发放系统 / 活动码、连带处罚复核）。
import { useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Field, Input, Stat, Table, Tabs } from '@pt-studio/ui-kit';

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
  'invite.ui.menu': '邀请（{n}）',
  'invite.ui.title': '我的邀请',
  'invite.ui.balance': '可用名额',
  'invite.ui.outstanding': '未使用',
  'invite.ui.issue': '生成邀请',
  'invite.ui.email': '绑定邮箱（可选）',
  'invite.ui.note': '备注',
  'invite.ui.issued': '邀请已生成，请复制链接（关闭后无法再次查看）：',
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
  'invite.ui.menu': 'Invites ({n})',
  'invite.ui.title': 'My invites',
  'invite.ui.balance': 'Available',
  'invite.ui.outstanding': 'Outstanding',
  'invite.ui.issue': 'Create invite',
  'invite.ui.email': 'Bind email (optional)',
  'invite.ui.note': 'Note',
  'invite.ui.issued': 'Invite created — copy the link now (it will not be shown again):',
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
    <a
      href="/p/invite/"
      onClick={(e) => {
        e.preventDefault();
        api.navigate('/p/invite/');
      }}
    >
      {api.t('invite.ui.menu', { n: q?.balance ?? '…' })}
    </a>
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

function MyInvites({ api }: { api: PluginApi; path: string }) {
  const [quota, setQuota] = useState<Quota | null>(null);
  const [elig, setElig] = useState<Eligibility | null>(null);
  const [list, setList] = useState<Invite[]>([]);
  const [invitees, setInvitees] = useState<{ user_id: number; username: string | null; status: string | null; registered_at: string }[]>([]);
  const [email, setEmail] = useState('');
  const [note, setNote] = useState('');
  const [issued, setIssued] = useState<string | null>(null);
  const reload = async () => {
    const [q, e, l, t] = await Promise.all([
      api.json<Quota>('/api/p/invite/v1/users/me/quota'),
      api.json<Eligibility>('/api/p/invite/v1/users/me/eligibility').catch(() => null),
      api.json<{ items: Invite[] }>('/api/p/invite/v1/users/me/invites'),
      api.json<{ items: { user_id: number; username: string | null; status: string | null; registered_at: string }[] }>('/api/p/invite/v1/users/me/invitees'),
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
  const issue = async () => {
    try {
      const r = await api.json<{ url: string }>('/api/p/invite/v1/users/me/invites', {
        json: { email: email || undefined, note: note || undefined, request_id: crypto.randomUUID() },
      });
      setIssued(`${window.location.origin}${r.url}`);
      setEmail('');
      setNote('');
      await reload();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const revoke = async (id: number) => {
    try {
      await api.json(`/api/p/invite/v1/users/me/invites/${id}`, { method: 'DELETE' });
      await reload();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const blocker = elig?.checks.find((c) => !c.ok);
  return (
    <div>
      <h1>{api.t('invite.ui.title')}</h1>
      <Card>
        <Stat label={api.t('invite.ui.balance')} value={quota?.balance ?? '…'} />
        <Stat label={api.t('invite.ui.outstanding')} value={quota ? `${quota.outstanding} / ${quota.max_outstanding}` : '…'} />
      </Card>
      <Card title={api.t('invite.ui.issue')}>
        {blocker && <p className="pt-field-error">{api.t('invite.ui.not_eligible', { reason: api.t(blocker.code ?? blocker.name) })}</p>}
        <Field label={api.t('invite.ui.email')}>
          <Input type="email" value={email} onChange={(e) => setEmail(e.target.value)} />
        </Field>
        <Field label={api.t('invite.ui.note')}>
          <Input value={note} maxLength={200} onChange={(e) => setNote(e.target.value)} />
        </Field>
        <Button variant="primary" onClick={() => void issue()} disabled={!!blocker}>
          {api.t('invite.ui.issue')}
        </Button>
        {issued && (
          <p className="issued">
            {api.t('invite.ui.issued')} <code className="pt-mono" data-testid="invite-url">{issued}</code>
          </p>
        )}
      </Card>
      <Card title={api.t('invite.ui.list')}>
        <Table
          rows={list}
          rowKey={(r) => r.id}
          columns={[
            { key: 'hint', header: '#', render: (r) => <span className="pt-mono">…{r.code_hint}</span> },
            { key: 'status', header: api.t('invite.ui.status'), render: (r) => <Badge tone={r.status === 'active' ? 'primary' : 'muted'}>{r.status}</Badge> },
            { key: 'exp', header: api.t('invite.ui.expires'), render: (r) => new Date(r.expires_at).toLocaleString() },
            { key: 'note', header: api.t('invite.ui.note'), render: (r) => r.note ?? r.email_domain ?? '' },
            {
              key: 'act',
              header: '',
              render: (r) =>
                r.status === 'active' ? (
                  <Button variant="danger" onClick={() => void revoke(r.id)}>
                    {api.t('invite.ui.revoke')}
                  </Button>
                ) : null,
            },
          ]}
        />
      </Card>
      <Card title={api.t('invite.ui.invitees')}>
        <Table
          rows={invitees}
          rowKey={(r) => r.user_id}
          columns={[
            {
              key: 'u',
              header: '',
              render: (r) => (
                <a
                  href={`/users/${r.user_id}`}
                  onClick={(e) => {
                    e.preventDefault();
                    api.navigate(`/users/${r.user_id}`);
                  }}
                >
                  {r.username ?? `#${r.user_id}`}
                </a>
              ),
            },
            { key: 's', header: api.t('invite.ui.status'), render: (r) => r.status ?? '' },
            { key: 'd', header: '', render: (r) => new Date(r.registered_at).toLocaleDateString() },
          ]}
        />
      </Card>
    </div>
  );
}

interface Quota {
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

/** 后台用户详情页“邀请”标签：名额（调整）与邀请权暂停 */
function UserTab({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const uid = ctx.profile.id;
  const [q, setQ] = useState<Quota | null>(null);
  const [sus, setSus] = useState<Suspension[]>([]);
  const [f, setF] = useState({ delta: '', days: '', reason: '', sdays: '', sreason: '' });
  const load = () => {
    void api.json<Quota>(`/api/p/invite/v1/admin/users/${uid}/quota`).then(setQ);
    void api.json<{ items: Suspension[] }>(`/api/p/invite/v1/admin/users/${uid}/suspensions`).then((r) => setSus(r.items));
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, [uid]);
  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      api.toast(api.t('invite.ui.done'), 'success');
      load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const adjust = (e: FormEvent) => {
    e.preventDefault();
    void act(() =>
      api.json(`/api/p/invite/v1/admin/users/${uid}/quota-adjustments`, {
        json: { delta: Number(f.delta), reason: f.reason, expires_at_ms: day(f.days), request_id: crypto.randomUUID() },
      }),
    ).then(() => setF((o) => ({ ...o, delta: '', reason: '' })));
  };
  const suspend = (e: FormEvent) => {
    e.preventDefault();
    void act(() => api.json(`/api/p/invite/v1/admin/users/${uid}/suspensions`, { json: { until_ms: day(f.sdays), reason: f.sreason }, idempotent: true }));
  };
  if (!q) return null;
  return (
    <Card>
      <p>
        <strong>{api.t('invite.ui.quota', { n: q.balance, p: q.permanent })}</strong>
      </p>
      <form onSubmit={adjust} className="upload-grid">
        <Field label={api.t('invite.ui.delta')}>
          <Input name="invite_delta" type="number" value={f.delta} onChange={(e) => setF({ ...f, delta: e.target.value })} />
        </Field>
        <Field label={api.t('invite.ui.expires_days')}>
          <Input name="invite_days" type="number" min={1} value={f.days} onChange={(e) => setF({ ...f, days: e.target.value })} />
        </Field>
        <div className="span2">
          <Field label={api.t('invite.ui.reason')}>
            <Input name="invite_reason" maxLength={500} value={f.reason} onChange={(e) => setF({ ...f, reason: e.target.value })} />
          </Field>
          <Button type="submit" disabled={!Number(f.delta) || !f.reason.trim()}>
            {api.t('invite.ui.adjust')}
          </Button>
        </div>
      </form>
      <h3>{api.t('invite.ui.suspensions')}</h3>
      <Table
        rows={sus}
        rowKey={(r) => r.id}
        columns={[
          { key: 't', header: '', render: (r) => <span className="pt-muted small">{new Date(r.created_at).toLocaleString()}</span> },
          { key: 'r', header: '', render: (r) => `${r.source} · ${r.reason ?? ''}` },
          { key: 'u', header: '', render: (r) => (r.until ? new Date(r.until).toLocaleString() : '∞') },
          {
            key: 'x',
            header: '',
            render: (r) =>
              r.lifted_at ? (
                <Badge>{api.t('invite.ui.lifted')}</Badge>
              ) : (
                <Button variant="ghost" onClick={() => void act(() => api.json(`/api/p/invite/v1/admin/users/${uid}/suspensions/${r.id}`, { method: 'DELETE' }))}>
                  {api.t('invite.ui.lift')}
                </Button>
              ),
          },
        ]}
      />
      <form onSubmit={suspend} className="searchbar">
        <Input name="suspend_days" type="number" min={1} placeholder={api.t('invite.ui.suspend_days')} value={f.sdays} onChange={(e) => setF({ ...f, sdays: e.target.value })} />
        <Input name="suspend_reason" placeholder={api.t('invite.ui.reason')} maxLength={500} value={f.sreason} onChange={(e) => setF({ ...f, sreason: e.target.value })} />
        <Button type="submit" variant="danger" disabled={!f.sreason.trim()}>
          {api.t('invite.ui.suspend')}
        </Button>
      </form>
    </Card>
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

function Codes({ api }: { api: PluginApi }) {
  const [status, setStatus] = useState('active');
  const [rows, setRows] = useState<AdminInvite[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const [f, setF] = useState({ kind: 'system', max_uses: '1', expires_days: '7', note: '' });
  const [issued, setIssued] = useState<string | null>(null);
  const load = async (cursor: string | null) => {
    const q = new URLSearchParams();
    if (status) q.set('status', status);
    if (cursor) q.set('cursor', cursor);
    const r = await api.json<{ items: AdminInvite[]; next_cursor: string | null }>(`/api/p/invite/v1/admin/invites?${q.toString()}`);
    setRows((old) => (cursor ? [...old, ...r.items] : r.items));
    setNext(r.next_cursor);
  };
  useEffect(() => {
    void load(null);
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
  return (
    <>
      <Card title={api.t('invite.ui.issue')}>
        <form onSubmit={(e) => void issue(e)} className="upload-grid">
          <Field label="">
            <select className="pt-input" name="invite_kind" value={f.kind} onChange={(e) => setF({ ...f, kind: e.target.value, max_uses: e.target.value === 'campaign' ? '50' : '1' })}>
              <option value="system">{api.t('invite.ui.kind_system')}</option>
              <option value="campaign">{api.t('invite.ui.kind_campaign')}</option>
            </select>
          </Field>
          <Field label={api.t('invite.ui.max_uses')}>
            <Input name="invite_max_uses" type="number" min={1} value={f.max_uses} onChange={(e) => setF({ ...f, max_uses: e.target.value })} />
          </Field>
          <Field label={api.t('invite.ui.expires_days')}>
            <Input name="invite_expires" type="number" min={1} value={f.expires_days} onChange={(e) => setF({ ...f, expires_days: e.target.value })} />
          </Field>
          <Field label={api.t('invite.ui.reason')}>
            <Input name="invite_note" maxLength={200} value={f.note} onChange={(e) => setF({ ...f, note: e.target.value })} />
          </Field>
          <div className="span2">
            <Button type="submit" variant="primary">
              {api.t('invite.ui.issue')}
            </Button>
            {issued && (
              <p data-testid="issued-invite">
                {api.t('invite.ui.issued')} 
                <code>{issued}</code>
              </p>
            )}
          </div>
        </form>
      </Card>
      <Card>
        <Tabs value={status} onChange={setStatus} tabs={STATUSES.map((s) => ({ id: s, label: s || api.t('invite.ui.all') }))} />
        <Table
          rows={rows}
          rowKey={(r) => r.id}
          columns={[
            { key: 'c', header: '', render: (r) => <code>{r.code_hint}…</code> },
            { key: 'k', header: '', render: (r) => <><Badge>{r.kind}</Badge> <span className="pt-muted small">{r.source}</span></> },
            { key: 's', header: '', render: (r) => r.status },
            { key: 'u', header: '', render: (r) => api.t('invite.ui.uses', { u: r.use_count, m: r.max_uses }) },
            { key: 'e', header: '', render: (r) => <span className="pt-muted small">{new Date(r.expires_at).toLocaleDateString()}</span> },
            { key: 'n', header: '', render: (r) => <span className="pt-muted small">{r.note}</span> },
            {
              key: 'x',
              header: '',
              render: (r) =>
                r.status === 'active' && (
                  <Button variant="ghost" onClick={() => void revoke(r.id)}>
                    {api.t('invite.ui.revoke')}
                  </Button>
                ),
            },
          ]}
        />
        {next && <Button onClick={() => void load(next)}>{api.t('invite.ui.more')}</Button>}
      </Card>
    </>
  );
}

function Penalties({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<Penalty[]>([]);
  const load = () => void api.json<{ items: Penalty[] }>('/api/p/invite/v1/admin/penalties?status=pending_review,applied').then((r) => setRows(r.items));
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
  return (
    <Card>
      <Table
        rows={rows}
        rowKey={(r) => r.id}
        columns={[
          { key: 't', header: '', render: (r) => <span className="pt-muted small">{new Date(r.created_at).toLocaleString()}</span> },
          { key: 'r', header: '', render: (r) => `${r.rule_id} · ${r.trigger}` },
          { key: 'u', header: '', render: (r) => `#${r.target_user_id} ← #${r.invitee_id} (${r.depth})` },
          { key: 's', header: '', render: (r) => <Badge tone={r.state === 'pending_review' ? 'danger' : 'muted'}>{r.state}</Badge> },
          {
            key: 'x',
            header: '',
            render: (r) => (
              <>
                {r.state === 'pending_review' && <Button onClick={() => void review(r, 'confirm')}>{api.t('invite.ui.confirm')}</Button>}{' '}
                <Button variant="ghost" onClick={() => void review(r, 'reverse')}>
                  {api.t('invite.ui.reverse')}
                </Button>
              </>
            ),
          },
        ]}
      />
    </Card>
  );
}

function Admin({ api }: { api: PluginApi }) {
  const [tab, setTab] = useState('codes');
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t('invite.ui.admin')}</h1>
      <Tabs
        value={tab}
        onChange={setTab}
        tabs={[
          { id: 'codes', label: api.t('invite.ui.codes') },
          { id: 'penalties', label: api.t('invite.ui.penalties') },
        ]}
      />
      {tab === 'codes' ? <Codes api={api} /> : <Penalties api={api} />}
    </div>
  );
}

export default {
  slots: {
    'auth.register.form_extra': { component: CodeField, order: 10 },
    'admin.user.tabs': { component: UserTab, order: 30, title_term: 'invite.ui.tab' },
    'nav.user_menu': { component: MenuItem, order: 50 },
  },
  routes: [
    { path: '/p/invite/*', component: MyInvites },
    { path: '/admin/p/invite/*', component: Admin },
  ],
} satisfies PluginFrontend;
