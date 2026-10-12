import { afterEach, describe, expect, it, vi } from 'vitest';
import { api, ApiError, apiFetch, csrfToken } from '../src/index';

afterEach(() => {
  vi.restoreAllMocks();
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
});
