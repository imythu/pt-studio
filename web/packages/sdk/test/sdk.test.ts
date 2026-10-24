import { afterEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError, apiFetch, csrfToken, setReauthHandler } from '../src/index';

afterEach(() => {
  vi.restoreAllMocks();
  setReauthHandler(null);
});

describe('sdk', () => {
  it('reads the CSRF cookie and sends it with unsafe methods only', async () => {
    // jsdom 拒绝非 Secure 的 __Host- cookie（正确行为），这里直接替换 getter
    vi.spyOn(document, 'cookie', 'get').mockReturnValue('a=1; __Host-pt_csrf=tok%3D1');
    expect(csrfToken()).toBe('tok=1');
    const f = vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response('{}', { status: 200 }));
    await apiFetch('/api/p/x/v1/a', { json: { a: 1 }, idempotent: 'k1' });
    const h = (f.mock.calls[0]?.[1] as RequestInit).headers as Headers;
    expect(h.get('x-csrf-token')).toBe('tok=1');
    expect(h.get('idempotency-key')).toBe('k1');
    expect(h.get('content-type')).toBe('application/json');
    await apiFetch('/api/p/x/v1/a');
    const h2 = (f.mock.calls[1]?.[1] as RequestInit).headers as Headers;
    expect(h2.get('x-csrf-token')).toBeNull();
  });

  it('rejects non-api paths', async () => {
    await expect(apiFetch('/etc/passwd' as `/api/${string}`)).rejects.toThrow(/only \/api\//);
  });

  it('maps problem+json to ApiError', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(JSON.stringify({ code: 'invite.code_invalid', title: 'bad code', status: 422, details: { reason: 'expired' } }), { status: 422 }),
    );
    const e = await api('/api/p/invite/v1/invite-checks', { json: {} }).catch((x: unknown) => x);
    expect(e).toBeInstanceOf(ApiError);
    expect((e as ApiError).code).toBe('invite.code_invalid');
    expect((e as ApiError).details).toEqual({ reason: 'expired' });
  });

  it('re-authenticates once on identity.reauth_required and retries with the same idempotency key', async () => {
    // 前两次（a、b 首次请求）要求重新验证，之后成功；每次返回新的 Response（body 只能读一次）
    let n = 0;
    const f = vi.spyOn(globalThis, 'fetch').mockImplementation(() =>
      Promise.resolve(
        n++ < 2 ? new Response(JSON.stringify({ code: 'identity.reauth_required', status: 403 }), { status: 403 }) : new Response('{"ok":true}', { status: 200 }),
      ),
    );
    const handler = vi.fn(() => Promise.resolve(true));
    setReauthHandler(handler);
    // 两个并发请求共用一次验证
    const [a, b] = await Promise.all([api('/api/p/x/v1/a', { json: {}, idempotent: true }), api('/api/p/x/v1/b', { json: {} })]);
    expect(a).toEqual({ ok: true });
    expect(b).toEqual({ ok: true });
    expect(handler).toHaveBeenCalledTimes(1);
    const keys = f.mock.calls.filter((c) => c[0] === '/api/p/x/v1/a').map((c) => ((c[1] as RequestInit).headers as Headers).get('idempotency-key'));
    expect(keys).toHaveLength(2);
    expect(keys[0]).toBeTruthy();
    expect(keys[0]).toBe(keys[1]);
  });

  it('surfaces identity.reauth_required when the user cancels', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(new Response(JSON.stringify({ code: 'identity.reauth_required', status: 403 }), { status: 403 }));
    setReauthHandler(() => Promise.resolve(false));
    const e = await api('/api/p/x/v1/a', { json: {} }).catch((x: unknown) => x);
    expect((e as ApiError).code).toBe('identity.reauth_required');
  });
});
