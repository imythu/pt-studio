// 个人设置：资料、安全（改密码、会话）、passkey；插件经 user.settings.tabs 追加标签页。
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';
import { api } from '@pt-studio/sdk';
import { Slot } from '@pt-studio/slots';
import { useT } from '@pt-studio/terms';
import { Badge, Button, Card, Field, Input, Spinner, Table, Tabs, useToast } from '@pt-studio/ui-kit';
import { ErrorText, useErrorText } from '../errors';
import { useManifest } from '../manifest';
import { useSession } from '../session';

interface Profile {
  id: number;
  username: string;
  title: string | null;
  bio: string | null;
  avatar_url: string | null;
  locale: string;
}

interface Session {
  id: string;
  method: string;
  user_agent: string | null;
  created_at: number;
  last_seen_at: number;
  current: boolean;
}

function ProfileTab() {
  const t = useT();
  const toast = useToast();
  const m = useManifest();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ['my-profile'], queryFn: () => api<Profile>('/api/p/identity/v1/users/me/profile') });
  const [draft, setDraft] = useState<Partial<Profile> | null>(null);
  const [error, setError] = useState<unknown>(null);
  if (q.isLoading) return <Spinner />;
  const p = { ...(q.data as Profile), ...(draft ?? {}) };
  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      await api('/api/p/identity/v1/users/me', {
        method: 'PATCH',
        json: { title: p.title || null, bio: p.bio || null, avatar_url: p.avatar_url || null, locale: p.locale },
      });
      try {
        localStorage.setItem('pt-locale', p.locale);
      } catch {
        /* 隐私模式 */
      }
      setDraft(null);
      await qc.invalidateQueries({ queryKey: ['my-profile'] });
      toast(t('ui.admin.saved'), 'success');
    } catch (err) {
      setError(err);
    }
  };
  const set = (k: keyof Profile) => (e: { target: { value: string } }) => setDraft({ ...(draft ?? {}), [k]: e.target.value });
  return (
    <form onSubmit={save}>
      <Field label={t('ui.settings.title_field')} hint={t('ui.settings.title_hint')}>
        <Input name="title" value={p.title ?? ''} maxLength={30} onChange={set('title')} />
      </Field>
      <Field label={t('ui.settings.avatar')}>
        <Input name="avatar_url" type="url" placeholder="https://" value={p.avatar_url ?? ''} onChange={set('avatar_url')} />
      </Field>
      <Field label={t('ui.settings.bio')}>
        <textarea className="pt-input" name="bio" rows={4} maxLength={2000} value={p.bio ?? ''} onChange={set('bio')} />
      </Field>
      <Field label={t('ui.settings.locale')}>
        <select className="pt-input" name="locale" value={p.locale} onChange={set('locale')}>
          {[...new Set([...m.site.locales, 'zh-CN', 'en'])].map((l) => (
            <option key={l} value={l}>
              {l}
            </option>
          ))}
        </select>
      </Field>
      <ErrorText error={error} />
      <Button type="submit" variant="primary">
        {t('ui.admin.save')}
      </Button>
    </form>
  );
}

interface SecurityEvent {
  id: number;
  kind: string;
  details: { ip_prefix?: string; ua?: string; method?: string; new_device?: boolean };
  created_at: number;
}

interface IpRecord {
  ip: string;
  source: string;
  hits: number;
  last_seen_at: number;
}

/** 本人的安全事件与 IP 记录（IP 默认只显示网段） */
function SecurityLog() {
  const t = useT();
  const events = useQuery({ queryKey: ['my-security-events'], queryFn: () => api<{ items: SecurityEvent[] }>('/api/p/identity/v1/users/me/security-events') });
  const ips = useQuery({ queryKey: ['my-ip-history'], queryFn: () => api<{ items: IpRecord[] }>('/api/p/identity/v1/users/me/ip-history') });
  return (
    <>
      <Card title={t('ui.settings.security_events')}>
        <Table
          rows={events.data?.items ?? []}
          rowKey={(e) => e.id}
          empty="—"
          columns={[
            { key: 'k', header: t('ui.admin.kind'), render: (e) => <>{t(`ui.security.${e.kind}`)} {e.details.new_device && <Badge tone="danger">{t('ui.security.new_device')}</Badge>}</> },
            { key: 'ip', header: 'IP', render: (e) => e.details.ip_prefix ?? '—' },
            { key: 'ua', header: t('ui.settings.device'), render: (e) => <span className="small">{e.details.ua ?? '—'}</span> },
            { key: 't', header: t('ui.settings.time'), render: (e) => new Date(e.created_at).toLocaleString() },
          ]}
        />
      </Card>
      <Card title={t('ui.settings.ip_history')}>
        <Table
          rows={ips.data?.items ?? []}
          rowKey={(r) => `${r.ip}-${r.source}-${r.last_seen_at}`}
          empty="—"
          columns={[
            { key: 'ip', header: 'IP', render: (r) => <code className="pt-mono">{r.ip}</code> },
            { key: 's', header: t('ui.admin.source'), render: (r) => t(`ui.security.source.${r.source}`) },
            { key: 'h', header: t('ui.settings.hits'), render: (r) => r.hits },
            { key: 't', header: t('ui.settings.last_seen'), render: (r) => new Date(r.last_seen_at).toLocaleString() },
          ]}
        />
      </Card>
    </>
  );
}

function SecurityTab() {
  const t = useT();
  const toast = useToast();
  const errText = useErrorText();
  const qc = useQueryClient();
  const sessions = useQuery({ queryKey: ['my-sessions'], queryFn: () => api<{ items: Session[] }>('/api/p/identity/v1/sessions') });
  const [pw, setPw] = useState({ current_password: '', new_password: '' });
  const [error, setError] = useState<unknown>(null);
  const reload = () => qc.invalidateQueries({ queryKey: ['my-sessions'] });
  const change = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      await api('/api/p/identity/v1/users/me/password', { method: 'PUT', json: pw });
      setPw({ current_password: '', new_password: '' });
      toast(t('ui.settings.password_changed'), 'success');
      await reload();
    } catch (err) {
      setError(err);
    }
  };
  const act = async (f: () => Promise<unknown>) => {
    try {
      await f();
      await reload();
    } catch (e) {
      toast(errText(e), 'error');
    }
  };
  return (
    <>
      <Card title={t('ui.settings.password')}>
        <form onSubmit={change}>
          <Field label={t('ui.settings.current_password')}>
            <Input name="current_password" type="password" autoComplete="current-password" value={pw.current_password} onChange={(e) => setPw({ ...pw, current_password: e.target.value })} required />
          </Field>
          <Field label={t('ui.settings.new_password')}>
            <Input name="new_password" type="password" autoComplete="new-password" value={pw.new_password} onChange={(e) => setPw({ ...pw, new_password: e.target.value })} required minLength={8} />
          </Field>
          <ErrorText error={error} />
          <Button type="submit" variant="primary">
            {t('ui.settings.change_password')}
          </Button>
        </form>
      </Card>
      <Card
        title={t('ui.settings.sessions')}
        actions={
          <Button variant="danger" onClick={() => act(() => api('/api/p/identity/v1/sessions/revocations', { json: { scope: 'others' }, idempotent: true }))}>
            {t('ui.settings.revoke_others')}
          </Button>
        }
      >
        <Table
          rows={sessions.data?.items ?? []}
          rowKey={(s) => s.id}
          columns={[
            { key: 'ua', header: t('ui.settings.device'), render: (s) => <>{s.user_agent ?? s.method} {s.current && <Badge tone="primary">{t('ui.settings.current')}</Badge>}</> },
            { key: 'seen', header: t('ui.settings.last_seen'), render: (s) => new Date(s.last_seen_at).toLocaleString() },
            { key: 'created', header: t('ui.torrent.created'), render: (s) => new Date(s.created_at).toLocaleString() },
            {
              key: 'act',
              header: '',
              render: (s) =>
                s.current ? null : (
                  <Button variant="danger" onClick={() => act(() => api(`/api/p/identity/v1/sessions/${s.id}`, { method: 'DELETE' }))}>
                    {t('ui.settings.revoke')}
                  </Button>
                ),
            },
          ]}
        />
      </Card>
      <SecurityLog />
    </>
  );
}

function PasskeyTab() {
  const t = useT();
  const toast = useToast();
  const errText = useErrorText();
  const [passkey, setPasskey] = useState<string | null>(null);
  const reveal = async () => {
    try {
      setPasskey((await api<{ passkey: string }>('/api/p/identity/v1/users/me/passkey')).passkey);
    } catch (e) {
      toast(errText(e), 'error');
    }
  };
  const reset = async () => {
    if (!window.confirm(t('ui.settings.passkey_confirm'))) return;
    try {
      setPasskey((await api<{ passkey: string }>('/api/p/identity/v1/users/me/passkey-resets', { json: {} })).passkey);
      toast(t('ui.settings.passkey_reset_done'), 'success');
    } catch (e) {
      toast(errText(e), 'error');
    }
  };
  return (
    <Card title="Passkey">
      <p className="pt-muted">{t('ui.settings.passkey_hint')}</p>
      <p>
        <code className="pt-mono" data-testid="passkey">
          {passkey ?? '••••••••••••••••••••••••••••••••'}
        </code>
      </p>
      <Button onClick={reveal}>{t('ui.settings.reveal')}</Button>{' '}
      <Button variant="danger" onClick={reset}>
        {t('ui.settings.passkey_reset')}
      </Button>
    </Card>
  );
}

export function SettingsPage() {
  const t = useT();
  const s = useSession();
  const [tab, setTab] = useState('profile');
  return (
    <div className="settings">
      <h1>{t('ui.settings.heading')}</h1>
      <Tabs
        value={tab}
        onChange={setTab}
        tabs={[
          { id: 'profile', label: t('ui.settings.profile') },
          { id: 'security', label: t('ui.settings.security') },
          { id: 'passkey', label: 'Passkey' },
          { id: 'plugins', label: t('ui.settings.more') },
        ]}
      />
      {tab === 'profile' && <ProfileTab />}
      {tab === 'security' && <SecurityTab />}
      {tab === 'passkey' && <PasskeyTab />}
      {tab === 'plugins' && <Slot name="user.settings.tabs" ctx={{ user: s.me }} empty={<p className="pt-muted">—</p>} />}
    </div>
  );
}
