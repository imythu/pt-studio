// 登录 / 注册（identity）。插件经 `auth.*` 插槽向请求写入字段（FormBinding）。
import { useMemo, useState, type FormEvent } from 'react';
import { Link, useNavigate } from 'react-router';
import { api } from '@pt-studio/sdk';
import { Slot } from '@pt-studio/slots';
import { useT } from '@pt-studio/terms';
import { Button, Card, Field, Input } from '@pt-studio/ui-kit';
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

export function Login() {
  const t = useT();
  const s = useSession();
  const nav = useNavigate();
  const [binding, extra] = useBinding();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
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
    <Card title={t('ui.login.title')} className="narrow">
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
        </Button>{' '}
        <Link to="/register">{t('ui.nav.register')}</Link>
      </form>
    </Card>
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
    <Card title={t('ui.register.title')} className="narrow">
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
    </Card>
  );
}
