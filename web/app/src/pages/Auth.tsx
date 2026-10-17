// 登录 / 注册（identity）。插件经 `auth.*` 插槽向请求写入字段（FormBinding）。
import { useEffect, useMemo, useRef, useState, type FormEvent } from 'react';
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
  // 默认保持登录：会话 cookie 带 Max-Age，关闭浏览器后再打开不需要重新登录
  const [remember, setRemember] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const st = useLocation().state as { registered?: boolean; reset?: boolean } | null;
  const registered = st?.registered;
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api('/api/p/identity/v1/sessions', { json: { username, password, remember, ...splitExtra(extra) } });
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
      {st?.reset && <p className="notice">{t('ui.reset.done')}</p>}
      <form onSubmit={submit}>
        <Field label={t('ui.login.username')}>
          <Input name="username" autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} required />
        </Field>
        <Field label={t('ui.login.password')}>
          <Input name="password" type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} required />
        </Field>
        <Slot name="auth.login.extra" ctx={{ form: binding }} />
        <label className="pt-check">
          <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} /> {t('ui.login.remember')}
        </label>
        <ErrorText error={error} />
        <Button type="submit" variant="primary" disabled={busy}>
          {t('ui.login.submit')}
        </Button>
        <p className="small">
          <Link to="/reset-password">{t('ui.reset.forgot')}</Link>
        </p>
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

/** 邮件链接 `?c=<挑战 ID>&t=<令牌>`：读出后立即从地址栏去掉令牌 */
function useChallengeParams(): { c: string; t: string } | null {
  const loc = useLocation();
  const [p] = useState(() => {
    const q = new URLSearchParams(loc.search);
    const c = q.get('c');
    const t = q.get('t');
    return c && t ? { c, t } : null;
  });
  useEffect(() => {
    if (p) window.history.replaceState(null, '', loc.pathname);
  }, [p, loc.pathname]);
  return p;
}

export function ResetPassword() {
  const t = useT();
  const nav = useNavigate();
  const challenge = useChallengeParams();
  const [login, setLogin] = useState('');
  const [pw, setPw] = useState('');
  const [sent, setSent] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const run = async (e: FormEvent, f: () => Promise<void>) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await f();
    } catch (err) {
      setError(err);
    } finally {
      setBusy(false);
    }
  };
  return (
    <AuthShell
      title={t('ui.reset.title')}
      sub={challenge ? t('ui.reset.sub_new') : t('ui.reset.sub_request')}
      foot={<Link to="/login">{t('ui.nav.login')}</Link>}
    >
      {challenge ? (
        <form
          onSubmit={(e) =>
            run(e, async () => {
              await api('/api/p/identity/v1/password-resets/confirm', { json: { challenge_id: challenge.c, token: challenge.t, new_password: pw } });
              nav('/login', { state: { reset: true } });
            })
          }
        >
          <Field label={t('ui.settings.new_password')}>
            <Input name="new_password" type="password" autoComplete="new-password" value={pw} onChange={(e) => setPw(e.target.value)} required minLength={8} />
          </Field>
          <ErrorText error={error} />
          <Button type="submit" variant="primary" disabled={busy}>
            {t('ui.reset.set')}
          </Button>
        </form>
      ) : sent ? (
        <p className="notice">{t('ui.reset.sent')}</p>
      ) : (
        <form
          onSubmit={(e) =>
            run(e, async () => {
              await api('/api/p/identity/v1/password-resets', { json: { login } });
              setSent(true);
            })
          }
        >
          <Field label={t('ui.reset.login')}>
            <Input name="login" autoComplete="username" value={login} onChange={(e) => setLogin(e.target.value)} required />
          </Field>
          <ErrorText error={error} />
          <Button type="submit" variant="primary" disabled={busy}>
            {t('ui.reset.send')}
          </Button>
        </form>
      )}
    </AuthShell>
  );
}

export function VerifyEmail() {
  const t = useT();
  const s = useSession();
  const challenge = useChallengeParams();
  const [state, setState] = useState<'busy' | 'ok' | unknown>(challenge ? 'busy' : null);
  const started = useRef(false);
  useEffect(() => {
    if (!challenge || started.current) return;
    started.current = true;
    api('/api/p/identity/v1/email-verifications/confirm', { json: { challenge_id: challenge.c, token: challenge.t } })
      .then(async () => {
        setState('ok');
        await s.refresh();
      })
      .catch((e: unknown) => setState(e));
  }, [challenge, s]);
  return (
    <AuthShell title={t('ui.verify.title')} sub="" foot={<Link to="/">{t('ui.nav.home')}</Link>}>
      {state === 'busy' && <p>…</p>}
      {state === 'ok' && <p className="notice">{t('ui.verify.done')}</p>}
      {state !== 'busy' && state !== 'ok' && <ErrorText error={state ?? new Error(t('identity.challenge_invalid'))} />}
    </AuthShell>
  );
}
