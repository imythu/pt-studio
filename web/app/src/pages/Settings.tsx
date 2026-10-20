// 个人设置：资料与外观、安全（改密、两步验证、安全密钥、登录设备、安全记录）、Passkey；
// 插件经 user.settings.tabs 追加分区，并入左侧分区导航（窄屏为顶部横向标签）。分区由地址 #hash 选定。
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Suspense, useEffect, useState, type FormEvent, type ReactNode } from 'react';
import { useLocation, useNavigate } from 'react-router';
import { api, ApiError } from '@pt-studio/sdk';
import { SlotEntryView, useSlotEntries, useSlotEntryTitle } from '@pt-studio/slots';
import { useT } from '@pt-studio/terms';
import { Avatar, Button, CodeBox, CopyButton, Field, Input, Note, PageHead, Panel, Segmented, Select, SkeletonRows, Status, Table, Tag, Textarea, useToast } from '@pt-studio/ui-kit';
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

interface SessionRow {
  id: string;
  method: string;
  user_agent: string | null;
  created_at: number;
  last_seen_at: number;
  current: boolean;
}

function Sec({ title, desc, children, actions }: { title: ReactNode; desc?: ReactNode; children: ReactNode; actions?: ReactNode }) {
  return (
    <section className="set-sec">
      <div className="row-sb">
        <h2>{title}</h2>
        {actions}
      </div>
      {desc && <p className="muted small set-desc">{desc}</p>}
      {children}
    </section>
  );
}

function getTheme(): string {
  try {
    return localStorage.getItem('pt-theme') ?? 'system';
  } catch {
    return 'system';
  }
}

function Appearance() {
  const t = useT();
  const [theme, setTheme] = useState(getTheme);
  const apply = (v: string) => {
    setTheme(v);
    try {
      if (v === 'system') {
        localStorage.removeItem('pt-theme');
        delete document.documentElement.dataset.theme;
      } else {
        localStorage.setItem('pt-theme', v);
        document.documentElement.dataset.theme = v;
      }
    } catch {
      /* 隐私模式 */
    }
  };
  return (
    <Sec title={t('ui.settings.appearance')}>
      <div className="set-row">
        <div>
          <b>{t('ui.settings.theme')}</b>
          <p>{t('ui.settings.theme_hint')}</p>
        </div>
        <Segmented
          value={theme}
          onChange={apply}
          label={t('ui.settings.theme')}
          options={[
            { value: 'system', label: t('ui.settings.theme_system') },
            { value: 'light', label: t('ui.theme.day') },
            { value: 'dark', label: t('ui.theme.night') },
          ]}
        />
      </div>
    </Sec>
  );
}

function ProfileSection() {
  const t = useT();
  const toast = useToast();
  const m = useManifest();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ['my-profile'], queryFn: () => api<Profile>('/api/p/identity/v1/users/me/profile') });
  const [draft, setDraft] = useState<Partial<Profile> | null>(null);
  const [error, setError] = useState<unknown>(null);
  if (q.isLoading) return <SkeletonRows n={4} />;
  const p = { ...(q.data as Profile), ...(draft ?? {}) };
  const save = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    try {
      await api('/api/p/identity/v1/users/me', { method: 'PATCH', json: { title: p.title || null, bio: p.bio || null, avatar_url: p.avatar_url || null, locale: p.locale } });
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
    <>
      <Sec title={t('ui.settings.profile')}>
        <form className="form" onSubmit={save}>
          <div className="row" style={{ gap: 16 }}>
            <Avatar src={p.avatar_url} name={p.username} size={64} ring />
            <div className="stack-s grow">
              <Field label={t('ui.settings.avatar')} hint={t('ui.settings.avatar_hint')}>
                <Input name="avatar_url" type="url" placeholder="https://" value={p.avatar_url ?? ''} onChange={set('avatar_url')} />
              </Field>
            </div>
          </div>
          <div className="form-2">
            <Field label={t('ui.settings.title_field')} hint={t('ui.settings.title_hint')}>
              <Input name="title" value={p.title ?? ''} maxLength={30} onChange={set('title')} />
            </Field>
            <Field label={t('ui.settings.locale')}>
              <Select name="locale" value={p.locale} onChange={set('locale')} options={[...new Set([...m.site.locales, 'zh-CN', 'en'])]} />
            </Field>
          </div>
          <Field label={t('ui.settings.bio')}>
            <Textarea name="bio" rows={4} maxLength={2000} value={p.bio ?? ''} onChange={set('bio')} />
          </Field>
          <ErrorText error={error} />
          <div className="form-acts">
            <Button type="submit" variant="primary">
              {t('ui.admin.save')}
            </Button>
          </div>
        </form>
      </Sec>
      <Appearance />
    </>
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

function SecurityLog() {
  const t = useT();
  const events = useQuery({ queryKey: ['my-security-events'], queryFn: () => api<{ items: SecurityEvent[] }>('/api/p/identity/v1/users/me/security-events') });
  const ips = useQuery({ queryKey: ['my-ip-history'], queryFn: () => api<{ items: IpRecord[] }>('/api/p/identity/v1/users/me/ip-history') });
  return (
    <>
      <Sec title={t('ui.settings.security_events')}>
        <Table
          rows={events.data?.items ?? []}
          rowKey={(e) => e.id}
          empty="—"
          minWidth={640}
          columns={[
            { key: 'k', header: t('ui.admin.kind'), render: (e) => <span className="row" style={{ gap: 6 }}>{t(`ui.security.${e.kind}`)} {e.details.new_device && <Tag tone="x">{t('ui.security.new_device')}</Tag>}</span> },
            { key: 'ip', header: 'IP', render: (e) => <span className="mono">{e.details.ip_prefix ?? '—'}</span> },
            { key: 'ua', header: t('ui.settings.device'), render: (e) => <span className="small">{e.details.ua ?? '—'}</span> },
            { key: 't', header: t('ui.settings.time'), render: (e) => <span className="small nowrap">{new Date(e.created_at).toLocaleString()}</span> },
          ]}
        />
      </Sec>
      <Sec title={t('ui.settings.ip_history')}>
        <Table
          rows={ips.data?.items ?? []}
          rowKey={(r) => `${r.ip}-${r.source}-${r.last_seen_at}`}
          empty="—"
          columns={[
            { key: 'ip', header: 'IP', render: (r) => <code className="mono">{r.ip}</code> },
            { key: 's', header: t('ui.admin.source'), render: (r) => t(`ui.security.source.${r.source}`) },
            { key: 'h', header: t('ui.settings.hits'), right: true, render: (r) => <span className="num">{r.hits}</span> },
            { key: 't', header: t('ui.settings.last_seen'), render: (r) => <span className="small nowrap">{new Date(r.last_seen_at).toLocaleString()}</span> },
          ]}
        />
      </Sec>
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

/** 需要近期重新验证的操作：服务端返回 identity.reauth_required 时显示密码框，验证后重试原操作 */
function useReauth(): [(f: () => Promise<void>) => Promise<void>, ReactNode] {
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
    <form onSubmit={submit} className="form reauth">
      <Note tone="warn">{t('ui.reauth.hint')}</Note>
      <Field label={t('ui.settings.current_password')}>
        <Input name="reauth_password" type="password" autoComplete="current-password" autoFocus value={pw} onChange={(e) => setPw(e.target.value)} required />
      </Field>
      <ErrorText error={error} />
      <div className="form-acts">
        <Button type="submit" variant="primary">
          {t('ui.reauth.confirm')}
        </Button>
        <Button onClick={() => setPending(null)}>{t('ui.common.cancel')}</Button>
      </div>
    </form>
  ) : null;
  return [run, form];
}

function TwoFactor() {
  const t = useT();
  const toast = useToast();
  const errText = useErrorText();
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ['my-2fa'], queryFn: () => api<TwoFactorStatus>('/api/p/identity/v1/users/me/two-factor') });
  const [setup, setSetup] = useState<{ secret: string; qr_svg: string } | null>(null);
  const [code, setCode] = useState('');
  const [codes, setCodes] = useState<string[] | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [confirmOff, setConfirmOff] = useState(false);
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
  const disable = () =>
    void act(async () => {
      await api('/api/p/identity/v1/users/me/totp', { method: 'DELETE' });
      setConfirmOff(false);
      await reload();
      toast(t('ui.two_factor.disabled'), 'success');
    });
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
      const r = await api<{ recovery_codes: string[] | null }>('/api/p/identity/v1/users/me/webauthn-credentials', { json: { ceremony_id: o.ceremony_id, name: keyName.trim() || 'Security key', credential } });
      if (r.recovery_codes) setCodes(r.recovery_codes);
      setKeyName('');
      await reload();
      toast(t('ui.webauthn.added'), 'success');
    });
  };
  const removeKey = (k: SecurityKey) =>
    void act(async () => {
      await api(`/api/p/identity/v1/users/me/webauthn-credentials/${k.id}`, { method: 'DELETE' });
      await reload();
    });
  const st = q.data;
  return (
    <>
      <Sec title={<>{t('ui.two_factor.title')} {st?.totp && <Status tone="ok">{t('ui.two_factor.on')}</Status>}</>} desc={st && !st.totp ? (st.required ? t('ui.two_factor.required_hint') : t('ui.two_factor.off_hint')) : undefined}>
        {reauthForm}
        {codes && (
          <div className="stack-s">
            <Note tone="warn">{t('ui.two_factor.codes_hint')}</Note>
            <pre className="codes" data-testid="recovery-codes">
              {codes.join('\n')}
            </pre>
            <div className="row">
              <CopyButton text={codes.join('\n')} label={t('ui.common.copy')} />
              <Button size="sm" onClick={() => setCodes(null)}>
                {t('ui.two_factor.codes_saved')}
              </Button>
            </div>
          </div>
        )}
        {!st ? (
          <SkeletonRows n={2} />
        ) : st.totp ? (
          <div className="row">
            <span className="small muted">{t('ui.two_factor.recovery_left', { n: st.recovery_remaining })}</span>
            <Button size="sm" onClick={regenerate}>
              {t('ui.two_factor.regenerate')}
            </Button>
            {confirmOff ? (
              <>
                <span className="small down">{t('ui.two_factor.disable_confirm')}</span>
                <Button size="sm" variant="danger" onClick={disable}>
                  {t('ui.two_factor.disable')}
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setConfirmOff(false)}>
                  {t('ui.common.cancel')}
                </Button>
              </>
            ) : (
              <Button size="sm" variant="danger" onClick={() => setConfirmOff(true)}>
                {t('ui.two_factor.disable')}
              </Button>
            )}
          </div>
        ) : setup ? (
          <form className="form" onSubmit={confirm}>
            <p className="small">{t('ui.two_factor.scan')}</p>
            <div className="row" style={{ alignItems: 'flex-start', gap: 20 }}>
              {/* 服务端生成的二维码 SVG（只含 otpauth URI） */}
              <div className="qr" dangerouslySetInnerHTML={{ __html: setup.qr_svg }} />
              <div className="stack-s grow">
                <span className="small muted">{t('ui.two_factor.manual')}</span>
                <CodeBox copy={setup.secret}>
                  <span data-testid="totp-secret">{setup.secret}</span>
                </CodeBox>
                <Field label={t('ui.two_factor.code')}>
                  <Input name="totp_code" autoComplete="one-time-code" inputMode="numeric" value={code} onChange={(e) => setCode(e.target.value)} required maxLength={6} style={{ maxWidth: 200 }} />
                </Field>
                <ErrorText error={error} />
                <div className="form-acts">
                  <Button type="submit" variant="primary">
                    {t('ui.two_factor.enable')}
                  </Button>
                </div>
              </div>
            </div>
          </form>
        ) : (
          <div className="row">
            <Button variant="primary" onClick={begin}>
              {t('ui.two_factor.setup')}
            </Button>
            {st.webauthn.length > 0 && (
              <>
                <span className="small muted">{t('ui.two_factor.recovery_left', { n: st.recovery_remaining })}</span>
                <Button size="sm" onClick={regenerate}>
                  {t('ui.two_factor.regenerate')}
                </Button>
              </>
            )}
          </div>
        )}
      </Sec>
      {st && (
        <Sec title={t('ui.webauthn.title')} desc={t('ui.webauthn.hint')}>
          {st.webauthn.length > 0 && (
            <Table
              rows={st.webauthn}
              rowKey={(k) => k.id}
              columns={[
                { key: 'n', header: t('ui.webauthn.name_col'), render: (k) => <span className="row" style={{ gap: 6 }}>{k.name} {k.discoverable && <Tag tone="k">{t('ui.webauthn.passwordless_ok')}</Tag>}</span> },
                { key: 'u', header: t('ui.settings.last_seen'), render: (k) => <span className="small">{k.last_used_at ? new Date(k.last_used_at).toLocaleString() : '—'}</span> },
                { key: 'x', header: '', render: (k) => <div className="acts"><Button size="xs" variant="ghost" onClick={() => removeKey(k)}>{t('ui.common.remove')}</Button></div> },
              ]}
            />
          )}
          {webauthnSupported() ? (
            <form className="row" onSubmit={addKey}>
              <Input name="key_name" placeholder={t('ui.webauthn.name')} value={keyName} maxLength={64} onChange={(e) => setKeyName(e.target.value)} style={{ maxWidth: 320 }} />
              <Button type="submit">{t('ui.webauthn.add')}</Button>
            </form>
          ) : (
            <p className="small muted">{t('ui.webauthn.unsupported')}</p>
          )}
        </Sec>
      )}
    </>
  );
}

function SecuritySection() {
  const t = useT();
  const toast = useToast();
  const errText = useErrorText();
  const qc = useQueryClient();
  const sessions = useQuery({ queryKey: ['my-sessions'], queryFn: () => api<{ items: SessionRow[] }>('/api/p/identity/v1/sessions') });
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
      <Sec title={t('ui.settings.password')}>
        <form className="form" onSubmit={change}>
          <div className="form-2">
            <Field label={t('ui.settings.current_password')}>
              <Input name="current_password" type="password" autoComplete="current-password" value={pw.current_password} onChange={(e) => setPw({ ...pw, current_password: e.target.value })} required />
            </Field>
            <Field label={t('ui.settings.new_password')}>
              <Input name="new_password" type="password" autoComplete="new-password" value={pw.new_password} onChange={(e) => setPw({ ...pw, new_password: e.target.value })} required minLength={8} />
            </Field>
          </div>
          <ErrorText error={error} />
          <div className="form-acts">
            <Button type="submit" variant="primary">
              {t('ui.settings.change_password')}
            </Button>
          </div>
        </form>
      </Sec>
      <TwoFactor />
      <Sec
        title={t('ui.settings.sessions')}
        actions={
          <Button size="sm" variant="danger" onClick={() => act(() => api('/api/p/identity/v1/sessions/revocations', { json: { scope: 'others' }, idempotent: true }))}>
            {t('ui.settings.revoke_others')}
          </Button>
        }
      >
        <Table
          rows={sessions.data?.items ?? []}
          rowKey={(s) => s.id}
          minWidth={640}
          columns={[
            { key: 'ua', header: t('ui.settings.device'), render: (s) => <span className="row" style={{ gap: 6 }}>{s.user_agent ?? s.method} {s.current && <Tag tone="o">{t('ui.settings.current')}</Tag>}</span> },
            { key: 'seen', header: t('ui.settings.last_seen'), render: (s) => <span className="small nowrap">{new Date(s.last_seen_at).toLocaleString()}</span> },
            { key: 'created', header: t('ui.settings.signed_in'), render: (s) => <span className="small nowrap">{new Date(s.created_at).toLocaleString()}</span> },
            {
              key: 'act',
              header: '',
              render: (s) =>
                s.current ? null : (
                  <div className="acts">
                    <Button size="xs" variant="ghost" onClick={() => act(() => api(`/api/p/identity/v1/sessions/${s.id}`, { method: 'DELETE' }))}>
                      {t('ui.settings.revoke')}
                    </Button>
                  </div>
                ),
            },
          ]}
        />
      </Sec>
      <SecurityLog />
    </>
  );
}

function PasskeySection() {
  const t = useT();
  const toast = useToast();
  const errText = useErrorText();
  const [passkey, setPasskey] = useState<string | null>(null);
  const [confirm, setConfirm] = useState(false);
  const reveal = async () => {
    try {
      setPasskey((await api<{ passkey: string }>('/api/p/identity/v1/users/me/passkey')).passkey);
    } catch (e) {
      toast(errText(e), 'error');
    }
  };
  const reset = async () => {
    try {
      setPasskey((await api<{ passkey: string }>('/api/p/identity/v1/users/me/passkey-resets', { json: {} })).passkey);
      setConfirm(false);
      toast(t('ui.settings.passkey_reset_done'), 'success');
    } catch (e) {
      toast(errText(e), 'error');
    }
  };
  return (
    <Sec title="Passkey" desc={t('ui.settings.passkey_hint')}>
      <CodeBox
        copy={passkey ?? undefined}
        actions={
          !passkey && (
            <Button size="xs" onClick={reveal}>
              {t('ui.settings.reveal')}
            </Button>
          )
        }
      >
        <span data-testid="passkey">{passkey ?? '••••••••••••••••••••••••••••••••'}</span>
      </CodeBox>
      <div className="row">
        {confirm ? (
          <>
            <span className="small down">{t('ui.settings.passkey_confirm')}</span>
            <Button size="sm" variant="danger" onClick={reset}>
              {t('ui.settings.passkey_reset')}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setConfirm(false)}>
              {t('ui.common.cancel')}
            </Button>
          </>
        ) : (
          <Button size="sm" variant="danger" onClick={() => setConfirm(true)}>
            {t('ui.settings.passkey_reset')}
          </Button>
        )}
      </div>
    </Sec>
  );
}

function SettingsBody() {
  const t = useT();
  const s = useSession();
  const loc = useLocation();
  const nav = useNavigate();
  const entries = useSlotEntries('user.settings.tabs');
  const title = useSlotEntryTitle();
  const sections = [
    { id: 'profile', label: t('ui.settings.profile') },
    { id: 'security', label: t('ui.settings.security') },
    { id: 'passkey', label: 'Passkey' },
    ...entries.map((e) => ({ id: e.plugin, label: title(e) })),
  ];
  const fromHash = loc.hash.slice(1);
  const [cur, setCur] = useState(sections.some((x) => x.id === fromHash) ? fromHash : 'profile');
  useEffect(() => {
    if (fromHash && sections.some((x) => x.id === fromHash)) setCur(fromHash);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fromHash]);
  const pick = (id: string) => {
    setCur(id);
    nav({ hash: id }, { replace: true });
  };
  const entry = entries.find((e) => e.plugin === cur);
  return (
    <div className="set-layout">
      <nav className="set-nav" role="tablist" aria-label={t('ui.settings.heading')}>
        {sections.map((x) => (
          <button key={x.id} type="button" role="tab" aria-selected={x.id === cur} onClick={() => pick(x.id)}>
            {x.label}
          </button>
        ))}
      </nav>
      <section className="panel set-body" role="tabpanel" data-primary>
        {cur === 'profile' && <ProfileSection />}
        {cur === 'security' && <SecuritySection />}
        {cur === 'passkey' && <PasskeySection />}
        {entry && (
          <div className="set-sec">
            <h2>{title(entry)}</h2>
            <SlotEntryView entry={entry} ctx={{ user: s.me }} />
          </div>
        )}
      </section>
    </div>
  );
}

export function SettingsPage() {
  const t = useT();
  return (
    <div className="settings">
      <PageHead title={t('ui.settings.heading')} />
      <Suspense fallback={<SkeletonRows n={6} />}>
        <SettingsBody />
      </Suspense>
    </div>
  );
}
