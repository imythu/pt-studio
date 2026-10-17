// captcha 前端：在登录、注册、找回密码表单中渲染人机验证组件（Turnstile / hCaptcha / reCAPTCHA v2），
// 把令牌写入请求顶层的 `captcha_token`；表单提交失败（attempt 变化）后重置组件。
import { useEffect, useRef } from 'react';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';

interface FormBinding {
  set(key: string, value: unknown): void;
}

interface Config {
  enabled: boolean;
  provider: 'turnstile' | 'hcaptcha' | 'recaptcha';
  site_key: string;
  actions: string[];
}

interface Widgets {
  render(el: HTMLElement, opts: Record<string, unknown>): string | number;
  reset(id?: string | number): void;
}

const SCRIPTS: Record<Config['provider'], { src: string; global: string }> = {
  turnstile: { src: 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit&onload=__ptCaptchaReady', global: 'turnstile' },
  hcaptcha: { src: 'https://js.hcaptcha.com/1/api.js?render=explicit&onload=__ptCaptchaReady', global: 'hcaptcha' },
  recaptcha: { src: 'https://www.google.com/recaptcha/api.js?render=explicit&onload=__ptCaptchaReady', global: 'grecaptcha' },
};

let configP: Promise<Config> | null = null;
const loadConfig = (api: PluginApi) => (configP ??= api.json<Config>('/api/p/captcha/v1/config').catch(() => ({ enabled: false }) as Config));

let scriptP: Promise<Widgets> | null = null;
function loadScript(provider: Config['provider']): Promise<Widgets> {
  const s = SCRIPTS[provider];
  scriptP ??= new Promise((resolve, reject) => {
    const w = window as unknown as Record<string, unknown>;
    w.__ptCaptchaReady = () => resolve(w[s.global] as Widgets);
    const el = document.createElement('script');
    el.src = s.src;
    el.async = true;
    el.onerror = () => reject(new Error('captcha script failed to load'));
    document.head.appendChild(el);
  });
  return scriptP;
}

function Widget({ ctx, api, action }: SlotProps<{ form: FormBinding; attempt?: number }> & { action: string }) {
  const box = useRef<HTMLDivElement>(null);
  const widget = useRef<{ w: Widgets; id: string | number } | null>(null);
  useEffect(() => {
    let cancelled = false;
    void loadConfig(api).then(async (cfg) => {
      if (cancelled || !cfg.enabled || !cfg.actions.includes(action) || !box.current) return;
      const w = await loadScript(cfg.provider);
      if (cancelled || !box.current) return;
      const id = w.render(box.current, {
        sitekey: cfg.site_key,
        action,
        callback: (token: string) => ctx.form.set('captcha_token', token),
        'expired-callback': () => ctx.form.set('captcha_token', undefined),
        'error-callback': () => ctx.form.set('captcha_token', undefined),
      });
      widget.current = { w, id };
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  // 提交失败后令牌已被消耗：重置组件
  useEffect(() => {
    if (!ctx.attempt || !widget.current) return;
    ctx.form.set('captcha_token', undefined);
    widget.current.w.reset(widget.current.id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ctx.attempt]);
  return <div className="captcha" ref={box} data-testid={`captcha-${action}`} />;
}

export default {
  slots: {
    'auth.login.extra': { component: (p: SlotProps<{ form: FormBinding; attempt?: number }>) => <Widget {...p} action="login" />, order: 90 },
    'auth.register.form_extra': { component: (p: SlotProps<{ form: FormBinding; attempt?: number }>) => <Widget {...p} action="register" />, order: 90 },
    'auth.reset.extra': { component: (p: SlotProps<{ form: FormBinding; attempt?: number }>) => <Widget {...p} action="password_reset" />, order: 90 },
  },
} satisfies PluginFrontend;
