// 登录 / 注册 / 找回密码 / 验证邮箱（identity）：左侧插画与引言，右侧表单。插件经 `auth.*` 插槽向请求写入字段（FormBinding）。
import { useEffect, useMemo, useRef, useState, type FormEvent, type ReactNode } from 'react';
import { Link, useLocation, useNavigate } from 'react-router';
import { api, ApiError } from '@pt-studio/sdk';
import { Slot } from '@pt-studio/slots';
import { useT } from '@pt-studio/terms';
import { Button, Checkbox, Field, Input, Note } from '@pt-studio/ui-kit';
import { ErrorText } from '../errors';
import { useSession } from '../session';
import { Wordmark, ThemeButton } from '../shell/Layout';
import { getAssertion, webauthnSupported } from '../webauthn';

/** 插件写入注册 / 登录请求的绑定：顶层字段（如 `invite_code`）或 `form.<key>` */
export interface FormBinding {
  get(key: string): unknown;
  set(key: string, value: unknown): void;
}

function useBinding(): [FormBinding, Record<string, unknown>] {
  const [extra, setExtra] = useState<Record<string, unknown>>({});
  const binding = useMemo<FormBinding>(() => ({ get: (k) => extra[k], set: (k, v) => setExtra((x) => ({ ...x, [k]: v })) }), [extra]);
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

const ART: Record<string, { img: string; quote: string; who: string }> = {
  login: { img: '/frieren/frieren-sky-landscape.jpg', quote: 'ui.auth.quote_1', who: 'Frieren' },
  register: { img: '/frieren/frieren-hero-banner-generated.jpg', quote: 'ui.auth.quote_2', who: 'Himmel' },
  reset: { img: '/frieren/frieren-wide-scenery.jpg', quote: 'ui.auth.quote_3', who: 'Heiter' },
};

export function AuthShell({ title, sub, children, foot, art = 'login' }: { title: string; sub?: ReactNode; children: ReactNode; foot?: ReactNode; art?: keyof typeof ART }) {
  const t = useT();
  const a = ART[art] ?? ART.login;
  return (
    <div className="gate" data-primary>
      <div className="gate-art" style={{ backgroundImage: `url(${a?.img})` }}>
        <blockquote>
          {t(a?.quote ?? '')}
          <cite>— {a?.who}</cite>
        </blockquote>
      </div>
      <div className="gate-form">
        <div className="row-sb">
          <Wordmark />
          <ThemeButton />
        </div>
        <div>
          <h1>{title}</h1>
          {sub && <p className="muted gate-sub">{sub}</p>}
        </div>
        {children}
        {foot && <div className="gate-alt">{foot}</div>}
      </div>
    </div>
  );
}

export function Login() {
  const t = useT();
  const s = useSession();
  const nav = useNavigate();
  const loc = useLocation();
  const [binding, extra] = useBinding();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  // 默认保持登录：会话 cookie 带 Max-Age，关闭浏览器后再打开不需要重新登录
  const [remember, setRemember] = useState(true);
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  // 两步验证：密码正确后服务端返回票据，再提交 TOTP / 恢复码
  const [ticket, setTicket] = useState<string | null>(null);
  const [methods, setMethods] = useState<string[]>([]);
  // 每次提交失败加 1：验证码等插槽据此重置（令牌只能用一次）
  const [attempt, setAttempt] = useState(0);
  const [code, setCode] = useState('');
  const st = loc.state as { registered?: boolean; reset?: boolean; from?: string } | null;
  const done = async () => {
    await s.refresh();
    nav(st?.from && st.from.startsWith('/') ? st.from : '/');
  };
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      if (ticket) {
        await api('/api/p/identity/v1/sessions/two-factor', { json: { ticket, code } });
      } else {
        const r = await api<{ two_factor_required?: boolean; ticket?: string; methods?: string[] }>('/api/p/identity/v1/sessions', {
          json: { username, password, remember, ...splitExtra(extra) },
        });
        if (r.two_factor_required && r.ticket) {
          setTicket(r.ticket);
          setMethods(r.methods ?? []);
          return;
        }
      }
      await done();
    } catch (err) {
      if (err instanceof ApiError && err.code === 'identity.ticket_invalid') {
        setTicket(null);
        setCode('');
      }
      setAttempt((n) => n + 1);
      setError(err);
    } finally {
      setBusy(false);
    }
  };
  /** 安全密钥：第二步（有票据）或免密码登录 */
  const withKey = async () => {
    setBusy(true);
    setError(null);
    try {
      if (ticket) {
        const o = await api<{ public_key: Record<string, unknown> }>('/api/p/identity/v1/sessions/two-factor/webauthn-options', { json: { ticket } });
        await api('/api/p/identity/v1/sessions/two-factor', { json: { ticket, assertion: await getAssertion(o.public_key) } });
      } else {
        const o = await api<{ ceremony_id: string; public_key: Record<string, unknown> }>('/api/p/identity/v1/webauthn/assertion-options', { json: {} });
        await api('/api/p/identity/v1/sessions/webauthn', { json: { ceremony_id: o.ceremony_id, assertion: await getAssertion(o.public_key) } });
      }
      await done();
    } catch (err) {
      setError(err);
    } finally {
      setBusy(false);
    }
  };
  if (ticket) {
    return (
      <AuthShell title={t('ui.two_factor.title')} sub={t('ui.two_factor.code_hint')}>
        <form className="form" onSubmit={submit}>
          <Field label={t('ui.two_factor.code')}>
            <Input name="code" className="code-in" autoComplete="one-time-code" inputMode="text" autoFocus value={code} onChange={(e) => setCode(e.target.value)} required maxLength={16} />
          </Field>
          <ErrorText error={error} />
          <Button type="submit" variant="primary" size="lg" disabled={busy}>
            {t('ui.two_factor.verify')}
          </Button>
          {methods.includes('webauthn') && webauthnSupported() && (
            <Button size="lg" onClick={withKey} disabled={busy}>
              {t('ui.webauthn.use_key')}
            </Button>
          )}
          <button type="button" className="txt-btn" onClick={() => setTicket(null)}>
            ‹ {t('ui.login.back')}
          </button>
        </form>
      </AuthShell>
    );
  }
  return (
    <AuthShell
      title={t('ui.login.title')}
      sub={t('ui.login.sub')}
      foot={
        <>
          <span className="muted">{t('ui.login.no_account')}</span>
          <Link to="/register">{t('ui.nav.register')}</Link>
        </>
      }
    >
      {st?.registered && <Note tone="ok">{t('ui.register.done')}</Note>}
      {st?.reset && <Note tone="ok">{t('ui.reset.done')}</Note>}
      <form className="form" onSubmit={submit}>
        <Field label={t('ui.login.username')}>
          <Input name="username" autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} required />
        </Field>
        <Field label={t('ui.login.password')}>
          <Input name="password" type="password" autoComplete="current-password" value={password} onChange={(e) => setPassword(e.target.value)} required />
        </Field>
        <Slot name="auth.login.extra" ctx={{ form: binding, attempt }} />
        <div className="row-sb">
          <Checkbox label={t('ui.login.remember')} checked={remember} onChange={setRemember} />
          <Link className="link small" to="/reset-password">
            {t('ui.reset.forgot')}
          </Link>
        </div>
        <ErrorText error={error} />
        <Button type="submit" variant="primary" size="lg" disabled={busy}>
          {t('ui.login.submit')}
        </Button>
        {webauthnSupported() && (
          <>
            <div className="or">{t('ui.login.or')}</div>
            <Button size="lg" onClick={withKey} disabled={busy}>
              {t('ui.webauthn.passwordless')}
            </Button>
          </>
        )}
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
  const [attempt, setAttempt] = useState(0);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await api('/api/p/identity/v1/users', { json: { ...f, ...splitExtra(extra) } });
      nav('/login', { state: { registered: true } });
    } catch (err) {
      setError(err);
      setAttempt((n) => n + 1);
    } finally {
      setBusy(false);
    }
  };
  const field = (k: keyof typeof f, label: string, type = 'text', ac?: string, hint?: string) => (
    <Field label={label} hint={hint}>
      <Input name={k} type={type} autoComplete={ac} value={f[k]} onChange={(e) => setF({ ...f, [k]: e.target.value })} required minLength={k === 'password' ? 8 : undefined} />
    </Field>
  );
  return (
    <AuthShell
      art="register"
      title={t('ui.register.title')}
      sub={t('ui.register.sub')}
      foot={
        <>
          <span className="muted">{t('ui.register.have_account')}</span>
          <Link to="/login">{t('ui.nav.login')}</Link>
        </>
      }
    >
      <form className="form" onSubmit={submit}>
        <Slot name="auth.register.form_extra" ctx={{ form: binding, attempt }} className="stack-s" />
        {field('username', t('ui.login.username'), 'text', 'username')}
        {field('email', t('ui.register.email'), 'email', 'email')}
        {field('password', t('ui.login.password'), 'password', 'new-password', t('ui.register.password_hint'))}
        <ErrorText error={error} />
        <Button type="submit" variant="primary" size="lg" disabled={busy}>
          {t('ui.register.submit')}
        </Button>
        <span className="tiny muted">{t('ui.register.after_hint')}</span>
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
  const [binding, extra] = useBinding();
  const [attempt, setAttempt] = useState(0);
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
      setAttempt((n) => n + 1);
    } finally {
      setBusy(false);
    }
  };
  return (
    <AuthShell art="reset" title={t('ui.reset.title')} sub={challenge ? t('ui.reset.sub_new') : t('ui.reset.sub_request')} foot={<Link to="/login">‹ {t('ui.login.back_login')}</Link>}>
      {challenge ? (
        <form
          className="form"
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
          <Button type="submit" variant="primary" size="lg" disabled={busy}>
            {t('ui.reset.set')}
          </Button>
        </form>
      ) : sent ? (
        <Note tone="ok">{t('ui.reset.sent')}</Note>
      ) : (
        <form
          className="form"
          onSubmit={(e) =>
            run(e, async () => {
              await api('/api/p/identity/v1/password-resets', { json: { login, ...splitExtra(extra) } });
              setSent(true);
            })
          }
        >
          <Field label={t('ui.reset.login')}>
            <Input name="login" autoComplete="username" value={login} onChange={(e) => setLogin(e.target.value)} required />
          </Field>
          <Slot name="auth.reset.extra" ctx={{ form: binding, attempt }} />
          <ErrorText error={error} />
          <Button type="submit" variant="primary" size="lg" disabled={busy}>
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
    <AuthShell art="register" title={t('ui.verify.title')} foot={<Link to="/">{t('ui.nav.home')}</Link>}>
      {state === 'busy' && <p className="muted">…</p>}
      {state === 'ok' && (
        <>
          <Note tone="ok">{t('ui.verify.done')}</Note>
          <div className="row">
            <Link className="btn btn-primary" to="/help">
              {t('ui.help.title')}
            </Link>
            <Link className="btn" to="/">
              {t('ui.nav.home')}
            </Link>
          </div>
        </>
      )}
      {state !== 'busy' && state !== 'ok' && <ErrorText error={state ?? new Error(t('identity.challenge_invalid'))} />}
    </AuthShell>
  );
}
