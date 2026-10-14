// 登录 / 注册（identity）。插件经 `auth.*` 插槽向请求写入字段（FormBinding）。
import { useMemo, useState, type FormEvent } from 'react';
import { Link, useLocation, useNavigate } from 'react-router';
import { api } from '@pt-studio/sdk';
import { Slot } from '@pt-studio/slots';
import { useT } from '@pt-studio/terms';
import { Button, Field, Input } from '@pt-studio/ui-kit';
import { HeroArt, Logo } from '../art';
import { useManifest } from '../manifest';
import { ErrorText } from '../errors';
import { useSession } from '../session';

/** 插件写入注册 / 登录请求的绑定：顶层字段（如 `invite_code`）或 `form.<key>` */
export interface FormBinding {
  get(key: string): unknown;
  set(key: string, value: unknown): void;
}

function useBinding(): [FormBinding, Record<string, unknown>] {
  const [extra, setExtra] = useState<Record<string, unknown>>({});
  const binding = useMemo<FormBinding>(
    () => ({ get: (k) => extra[k], set: (k, v) => setExtra((x) => ({ ...x, [k]: v })) }),
    [extra],
  );
  return [binding, extra];
}

/** `form.x` 键收进 form 对象，其余为顶层字段 */
function splitExtra(extra: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  const form: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(extra)) {
    if (v === undefined || v === '') continue;
    if (k.startsWith('form.')) form[k.slice(5)] = v;
    else out[k] = v;
  }
  if (Object.keys(form).length) out.form = form;
  return out;
}

function AuthShell({ title, sub, children, foot }: { title: string; sub: string; children: React.ReactNode; foot: React.ReactNode }) {
  const t = useT();
  const m = useManifest();
  return (
    <div className="auth">
      <aside className="auth-side">
        <HeroArt />
        <h2>{t('ui.home.tagline', { site: m.site.name })}</h2>
        <p>{t('ui.home.guest_body')}</p>
      </aside>
      <div className="auth-form">
        <div className="brand-sm">
          <Logo size={34} />
          <strong>{m.site.name}</strong>
        </div>
        <h1>{title}</h1>
        <p className="sub">{sub}</p>
        {children}
        <p className="switch">{foot}</p>
      </div>
    </div>
  );
}

export function Login() {
  const t = useT();
  const s = useSession();
  const nav = useNavigate();
  const [binding, extra] = useBinding();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const registered = (useLocation().state as { registered?: boolean } | null)?.registered;
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api('/api/p/identity/v1/sessions', { json: { username, password, ...splitExtra(extra) } });
      await s.refresh();
      nav('/');
    } catch (err) {
      setError(err);
    } finally {
      setBusy(false);
    }
  };
  return (
    <AuthShell
      title={t('ui.login.title')}
      sub={t('ui.login.sub')}
      foot={
        <>
          {t('ui.login.no_account')} <Link to="/register">{t('ui.nav.register')}</Link>
        </>
      }
    >
      {registered && <p className="notice">{t('ui.register.done')}</p>}
      <form onSubmit={submit}>
        <Field label={t('ui.login.username')}>
          <Input name="username" autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} required />
        </Field>
        <Field label={t('ui.login.password')}>
          <Input name="password" type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} required />
        </Field>
        <Slot name="auth.login.extra" ctx={{ form: binding }} />
        <ErrorText error={error} />
        <Button type="submit" variant="primary" disabled={busy}>
          {t('ui.login.submit')}
        </Button>
      </form>
    </AuthShell>
  );
}

export function Register() {
  const t = useT();
  const nav = useNavigate();
  const [binding, extra] = useBinding();
  const [f, setF] = useState({ username: '', email: '', password: '' });
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api('/api/p/identity/v1/users', { json: { ...f, ...splitExtra(extra) } });
      nav('/login', { state: { registered: true } });
    } catch (err) {
      setError(err);
    } finally {
      setBusy(false);
    }
  };
  const field = (k: keyof typeof f, label: string, type = 'text', ac?: string) => (
    <Field label={label}>
      <Input name={k} type={type} autoComplete={ac} value={f[k]} onChange={(e) => setF({ ...f, [k]: e.target.value })} required />
    </Field>
  );
  return (
    <AuthShell
      title={t('ui.register.title')}
      sub={t('ui.register.sub')}
      foot={
        <>
          {t('ui.register.have_account')} <Link to="/login">{t('ui.nav.login')}</Link>
        </>
      }
    >
      <form onSubmit={submit}>
        {field('username', t('ui.login.username'), 'text', 'username')}
        {field('email', t('ui.register.email'), 'email', 'email')}
        {field('password', t('ui.login.password'), 'password', 'new-password')}
        <Slot name="auth.register.form_extra" ctx={{ form: binding }} />
        <ErrorText error={error} />
        <Button type="submit" variant="primary" disabled={busy}>
          {t('ui.register.submit')}
        </Button>
      </form>
    </AuthShell>
  );
}
