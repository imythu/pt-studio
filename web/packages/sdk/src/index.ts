// @pt-studio/sdk：后端访问的唯一入口（web HLD §6）——会话 cookie、CSRF、problem+json、幂等键。

/** 后端 problem+json 错误 */
export class ApiError extends Error {
  readonly code: string;
  readonly status: number;
  readonly details: unknown;
  constructor(code: string, status: number, message: string, details?: unknown) {
    super(message);
    this.name = 'ApiError';
    this.code = code;
    this.status = status;
    this.details = details;
  }
}

const CSRF_COOKIES = ['__Host-pt_csrf', 'pt_csrf'];

/** CSRF 双提交 cookie（非 HttpOnly，由 identity 登录时写入） */
export function csrfToken(): string | null {
  if (typeof document === 'undefined') return null;
  for (const part of document.cookie.split(';')) {
    const [k, ...v] = part.trim().split('=');
    if (k !== undefined && CSRF_COOKIES.includes(k)) return decodeURIComponent(v.join('='));
  }
  return null;
}

/** 幂等键：同一次用户操作的重试复用同一个键 */
export function idempotencyKey(): string {
  const c = globalThis.crypto;
  if (c && 'randomUUID' in c) return c.randomUUID();
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

export interface RequestOptions extends Omit<RequestInit, 'body'> {
  /** JSON 请求体（与 body 二选一） */
  json?: unknown;
  body?: BodyInit | null;
  /** 为写请求附加 Idempotency-Key；传字符串则使用该键 */
  idempotent?: boolean | string;
}

const UNSAFE = new Set(['POST', 'PUT', 'PATCH', 'DELETE']);

export type Path = `/api/${string}`;

export async function apiFetch(path: Path, opts: RequestOptions = {}): Promise<Response> {
  if (!path.startsWith('/api/')) throw new Error(`only /api/ paths are allowed: ${path}`);
  const method = (opts.method ?? (opts.json !== undefined ? 'POST' : 'GET')).toUpperCase();
  const headers = new Headers(opts.headers);
  headers.set('accept', 'application/json');
  let body = opts.body ?? null;
  if (opts.json !== undefined) {
    headers.set('content-type', 'application/json');
    body = JSON.stringify(opts.json);
  }
  if (UNSAFE.has(method)) {
    const t = csrfToken();
    if (t) headers.set('x-csrf-token', t);
    if (opts.idempotent) headers.set('idempotency-key', typeof opts.idempotent === 'string' ? opts.idempotent : idempotencyKey());
  }
  const { json: _j, idempotent: _i, ...init } = opts;
  return fetch(path, { ...init, method, headers, body, credentials: 'same-origin' });
}

/** problem+json → ApiError；2xx 返回 JSON（204 返回 undefined） */
export async function parse<T>(resp: Response): Promise<T> {
  const text = await resp.text();
  const data: unknown = text ? safeJson(text) : undefined;
  if (!resp.ok) {
    const p = (data ?? {}) as { code?: string; title?: string; detail?: string; message?: string; details?: unknown };
    throw new ApiError(p.code ?? `http.${resp.status}`, resp.status, p.detail ?? p.message ?? p.title ?? resp.statusText, p.details);
  }
  return data as T;
}

function safeJson(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return text;
  }
}

export async function api<T = unknown>(path: Path, opts?: RequestOptions): Promise<T> {
  return parse<T>(await apiFetch(path, opts));
}

// ---------------- 常用接口的类型 ----------------

export interface Me {
  id: number;
  username: string;
  class_id: number;
  status: string;
  [k: string]: unknown;
}

export interface FrontendPlugin {
  id: string;
  name: string;
  version: string;
  entry: string | null;
  slots: string[];
  routes: string[];
  settings_ui: 'schema' | 'custom' | 'none' | string;
  /** 是否主题插件（可在后台“外观”中选用） */
  theme?: boolean;
}

/** 当前生效的主题插件：样式表与（可选）替换页面的入口模块 */
export interface ActiveTheme {
  id: string;
  css: string | null;
  entry: string | null;
  /** 主题覆盖了术语（如等级名称）；切换后需重新加载术语 */
  terms?: boolean;
}

export interface FrontendManifest {
  site: { name: string; /** 字标：主字 + 副字（副字可空） */ mark?: [string, string]; default_locale: string; locales: string[]; themes: string[] };
  plugins: FrontendPlugin[];
  /** null：使用框架自带样式（经典） */
  theme?: ActiveTheme | null;
  installed: string[];
  features: Record<string, boolean>;
}

/** 服务端渲染预览（与最终显示一致）：Markdown / BBCode → 安全 HTML；需登录 */
export async function renderPreview(body: string, markup: 'markdown' | 'bbcode' = 'markdown'): Promise<string> {
  return (await api<{ html: string }>('/api/kernel/v1/markup/preview', { json: { body, markup } })).html;
}
