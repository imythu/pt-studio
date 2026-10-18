// 个人设置：资料、安全（改密码、会话）、passkey；插件经 user.settings.tabs 追加标签页。
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';
import { api, ApiError } from '@pt-studio/sdk';
import { Slot } from '@pt-studio/slots';
import { useT } from '@pt-studio/terms';
import { Badge, Button, Card, CopyButton, Field, Input, Spinner, Table, Tabs, useToast } from '@pt-studio/ui-kit';
import { ErrorText, useErrorText } from '../errors';
import { useManifest } from '../manifest';
import { createCredential, webauthnSupported } from '../webauthn';
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

interface SecurityKey {
  id: number;
  name: string;
  discoverable: boolean;
  created_at: number;
  last_used_at: number | null;
}

interface TwoFactorStatus {
  totp: boolean;
  recovery_remaining: number;
  required: boolean;
  webauthn: SecurityKey[];
}

/**
 * 需要近期重新验证的操作：服务端返回 identity.reauth_required 时显示密码框，验证后重试原操作。
 * 返回 [执行器, 重新验证表单]。
 */
function useReauth(): [(f: () => Promise<void>) => Promise<void>, React.ReactNode] {
  const t = useT();
  const [pending, setPending] = useState<(() => Promise<void>) | null>(null);
  const [pw, setPw] = useState('');
  const [error, setError] = useState<unknown>(null);
  const run = async (f: () => Promise<void>) => {
    try {
      await f();
    } catch (e) {
      if (e instanceof ApiError && e.code === 'identity.reauth_required') {
        setPending(() => f);
        return;
      }
      throw e;
    }
  };
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      await api('/api/p/identity/v1/sessions/current/reauth', { json: { password: pw } });
      const f = pending;
      setPending(null);
      setPw('');
      if (f) await f();
    } catch (err) {
      setError(err);
    }
  };
  const form = pending ? (
    <form onSubmit={submit} className="reauth">
      <p className="pt-muted small">{t('ui.reauth.hint')}</p>
      <Field label={t('ui.settings.current_password')}>
        <Input name="reauth_password" type="password" autoComplete="current-password" autoFocus value={pw} onChange={(e) => setPw(e.target.value)} required />
      </Field>
      <ErrorText error={error} />
      <Button type="submit" variant="primary">
        {t('ui.reauth.confirm')}
      </Button>{' '}
      <Button type="button" onClick={() => setPending(null)}>
        {t('ui.common.cancel')}
      </Button>
    </form>
  ) : null;
  return [run, form];
}

/** 两步验证：TOTP 绑定（二维码由服务端生成）、恢复码、关闭 */
function TwoFactorCard() {
  const t = useT();
  const toast = useToast();
  const errText = useErrorText();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ['my-2fa'], queryFn: () => api<TwoFactorStatus>('/api/p/identity/v1/users/me/two-factor') });
  const [setup, setSetup] = useState<{ secret: string; qr_svg: string } | null>(null);
  const [code, setCode] = useState('');
  const [codes, setCodes] = useState<string[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [withReauth, reauthForm] = useReauth();
  const reload = () => qc.invalidateQueries({ queryKey: ['my-2fa'] });
  const act = (f: () => Promise<void>) => withReauth(f).catch((e: unknown) => toast(errText(e), 'error'));
  const begin = () => act(async () => setSetup(await api<{ secret: string; qr_svg: string }>('/api/p/identity/v1/users/me/totp', { json: {} })));
  const confirm = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      await withReauth(async () => {
        const r = await api<{ recovery_codes: string[] }>('/api/p/identity/v1/users/me/totp/confirm', { json: { code } });
        setCodes(r.recovery_codes);
        setSetup(null);
        setCode('');
        await reload();
      });
    } catch (err) {
      setError(err);
    }
  };
  const disable = () => {
    if (!window.confirm(t('ui.two_factor.disable_confirm'))) return;
    void act(async () => {
      await api('/api/p/identity/v1/users/me/totp', { method: 'DELETE' });
      await reload();
      toast(t('ui.two_factor.disabled'), 'success');
    });
  };
  const regenerate = () =>
    act(async () => {
      setCodes((await api<{ recovery_codes: string[] }>('/api/p/identity/v1/users/me/recovery-codes', { json: {} })).recovery_codes);
      await reload();
    });
  const [keyName, setKeyName] = useState('');
  const addKey = (e: FormEvent) => {
    e.preventDefault();
    void act(async () => {
      const o = await api<{ ceremony_id: string; public_key: Record<string, unknown> }>('/api/p/identity/v1/users/me/webauthn/registration-options', { json: {} });
      const credential = await createCredential(o.public_key);
      const r = await api<{ recovery_codes: string[] | null }>('/api/p/identity/v1/users/me/webauthn-credentials', {
        json: { ceremony_id: o.ceremony_id, name: keyName.trim() || 'Security key', credential },
      });
      if (r.recovery_codes) setCodes(r.recovery_codes);
      setKeyName('');
      await reload();
      toast(t('ui.webauthn.added'), 'success');
    });
  };
  const removeKey = (k: SecurityKey) => {
    if (!window.confirm(t('ui.webauthn.remove_confirm', { name: k.name }))) return;
    void act(async () => {
      await api(`/api/p/identity/v1/users/me/webauthn-credentials/${k.id}`, { method: 'DELETE' });
      await reload();
    });
  };
  const st = q.data;
  const keys = st && (
    <div className="security-keys">
      <h3>{t('ui.webauthn.title')}</h3>
      <p className="pt-muted small">{t('ui.webauthn.hint')}</p>
      <ul>
        {st.webauthn.map((k) => (
          <li key={k.id}>
            {k.name} {k.discoverable && <Badge>{t('ui.webauthn.passwordless_ok')}</Badge>}{' '}
            <span className="pt-muted small">{k.last_used_at ? new Date(k.last_used_at).toLocaleString() : '—'}</span>{' '}
            <Button variant="ghost" onClick={() => removeKey(k)}>
              ✕
            </Button>
          </li>
        ))}
      </ul>
      {webauthnSupported() ? (
        <form className="searchbar" onSubmit={addKey}>
          <Input name="key_name" placeholder={t('ui.webauthn.name')} value={keyName} maxLength={64} onChange={(e) => setKeyName(e.target.value)} />
          <Button type="submit">{t('ui.webauthn.add')}</Button>
        </form>
      ) : (
        <p className="pt-muted small">{t('ui.webauthn.unsupported')}</p>
      )}
    </div>
  );
  return (
    <Card title={t('ui.two_factor.title')}>
      {reauthForm}
      {codes && (
        <div className="recovery">
          <p>{t('ui.two_factor.codes_hint')}</p>
          <pre className="pt-mono" data-testid="recovery-codes">
            {codes.join('\n')}
          </pre>
          <div className="pt-row" style={{ marginTop: '8px', marginBottom: '8px' }}>
            <CopyButton text={codes.join('\n')} label={t('ui.common.copy')} />
            <Button onClick={() => setCodes(null)}>{t('ui.two_factor.codes_saved')}</Button>
          </div>
        </div>
      )}
      {!st ? (
        <Spinner />
      ) : st.totp ? (
        <>
          <p>
            <Badge tone="success">{t('ui.two_factor.on')}</Badge> {t('ui.two_factor.recovery_left', { n: st.recovery_remaining })}
          </p>
          <Button onClick={regenerate}>{t('ui.two_factor.regenerate')}</Button>{' '}
          <Button variant="danger" onClick={disable}>
            {t('ui.two_factor.disable')}
          </Button>
          {keys}
        </>
      ) : setup ? (
        <form onSubmit={confirm}>
          <p>{t('ui.two_factor.scan')}</p>
          {/* 服务端生成的二维码 SVG（只含 otpauth URI） */}
          <div className="qr" dangerouslySetInnerHTML={{ __html: setup.qr_svg }} />
          <p className="small">
            {t('ui.two_factor.manual')} <code className="pt-mono" data-testid="totp-secret">{setup.secret}</code>{' '}
            <CopyButton text={setup.secret} title={t('ui.common.copy')} />
          </p>
          <Field label={t('ui.two_factor.code')}>
            <Input name="totp_code" autoComplete="one-time-code" inputMode="numeric" value={code} onChange={(e) => setCode(e.target.value)} required maxLength={6} />
          </Field>
          <ErrorText error={error} />
          <Button type="submit" variant="primary">
            {t('ui.two_factor.enable')}
          </Button>
        </form>
      ) : (
        <>
          <p className="pt-muted">{st.required ? t('ui.two_factor.required_hint') : t('ui.two_factor.off_hint')}</p>
          <Button variant="primary" onClick={begin}>
            {t('ui.two_factor.setup')}
          </Button>
          {st.webauthn.length > 0 && (
            <p className="small">
              {t('ui.two_factor.recovery_left', { n: st.recovery_remaining })} <Button onClick={regenerate}>{t('ui.two_factor.regenerate')}</Button>
            </p>
          )}
          {keys}
        </>
      )}
    </Card>
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
      <TwoFactorCard />
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
      {passkey ? (
        <CopyButton text={passkey} label={t('ui.common.copy')} />
      ) : (
        <Button onClick={reveal}>{t('ui.settings.reveal')}</Button>
      )}{' '}
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
