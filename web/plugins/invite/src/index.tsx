// invite 前端：注册页邀请码输入（`?invite=` 预填、失焦校验）、用户菜单入口、“我的邀请”页面。
import { useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Field, Input, Stat, Table } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
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

export default {
  slots: {
    'auth.register.form_extra': { component: CodeField, order: 10 },
    'nav.user_menu': { component: MenuItem, order: 50 },
  },
  routes: [{ path: '/p/invite/*', component: MyInvites }],
} satisfies PluginFrontend;
