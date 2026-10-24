// 近期重新验证（identity `reauth_secs` 路由返回 identity.reauth_required）：全局对话框，验证后由 SDK 重试原请求。
import { useEffect, useRef, useState, type FormEvent } from 'react';
import { api, setReauthHandler } from '@pt-studio/sdk';
import { useT } from '@pt-studio/terms';
import { Button, Field, Input, Note, Sheet } from '@pt-studio/ui-kit';
import { ErrorText } from './errors';

export function ReauthHost() {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [useCode, setUseCode] = useState(false);
  const [secret, setSecret] = useState('');
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const settle = useRef<((ok: boolean) => void) | null>(null);

  useEffect(() => {
    setReauthHandler(
      () =>
        new Promise<boolean>((resolve) => {
          settle.current = resolve;
          setSecret('');
          setError(null);
          setOpen(true);
        }),
    );
    return () => setReauthHandler(null);
  }, []);

  const finish = (ok: boolean) => {
    setOpen(false);
    setSecret('');
    settle.current?.(ok);
    settle.current = null;
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      await api('/api/p/identity/v1/sessions/current/reauth', { json: useCode ? { code: secret.trim() } : { password: secret } });
      finish(true);
    } catch (err) {
      setError(err);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Sheet
      open={open}
      onClose={() => finish(false)}
      title={t('identity.reauth_required')}
      width={420}
      footer={
        <div className="form-acts">
          <Button type="submit" form="reauth-form" variant="primary" disabled={busy || !secret}>
            {t('ui.reauth.confirm')}
          </Button>
          <Button onClick={() => finish(false)}>{t('ui.common.cancel')}</Button>
        </div>
      }
    >
      <form id="reauth-form" onSubmit={submit} className="form">
        <Note tone="warn">{t(useCode ? 'ui.reauth.hint_code' : 'ui.reauth.hint')}</Note>
        {useCode ? (
          <Field label={t('ui.two_factor.code')}>
            <Input key="code" name="reauth_code" inputMode="numeric" autoComplete="one-time-code" autoFocus value={secret} onChange={(e) => setSecret(e.target.value)} required />
          </Field>
        ) : (
          <Field label={t('ui.settings.current_password')}>
            <Input key="pw" name="reauth_password" type="password" autoComplete="current-password" autoFocus value={secret} onChange={(e) => setSecret(e.target.value)} required />
          </Field>
        )}
        <ErrorText error={error} />
        <Button
          size="sm"
          variant="ghost"
          onClick={() => {
            setUseCode(!useCode);
            setSecret('');
            setError(null);
          }}
        >
          {t(useCode ? 'ui.reauth.use_password' : 'ui.reauth.use_code')}
        </Button>
      </form>
    </Sheet>
  );
}
