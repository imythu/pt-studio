// mcp 前端：个人设置“AI 客户端”标签页（MCP 地址与配置示例、个人 MCP token、已授权的客户端）、用户菜单的待确认提示、
// OAuth 同意页（/p/mcp/consent）、待确认操作页（/p/mcp/confirmations[/<id>]）。
import { Fragment, useCallback, useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Checkbox, CodeBox, Field, Input, NavItem, Note, PageHead, Panel, SkeletonRows, Status, Table, Tag } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'mcp.ui.tab': 'AI 客户端',
  'mcp.ui.endpoint': 'MCP 地址',
  'mcp.ui.endpoint_hint': '在支持 MCP 的 AI 客户端中添加远程服务器，填写此地址；支持 OAuth 的客户端会自动跳转到本站授权，其他客户端使用下方的个人 token。',
  'mcp.ui.tokens': '个人 MCP token',
  'mcp.ui.name': '名称',
  'mcp.ui.scopes': '权限',
  'mcp.ui.scope.read': '读取',
  'mcp.ui.scope.write': '写操作（每次需在站内确认）',
  'mcp.ui.scope.admin': '管理员工具',
  'mcp.ui.days': '有效期（天）',
  'mcp.ui.create': '生成 token',
  'mcp.ui.created': 'token 已生成，请立即复制（离开页面后无法再次查看）',
  'mcp.ui.revoke': '撤销',
  'mcp.ui.expires': '到期',
  'mcp.ui.last_used': '最近使用',
  'mcp.ui.never': '从未',
  'mcp.ui.clients': '已授权的客户端',
  'mcp.ui.example': '配置示例',
  'mcp.ui.pending': '待确认的 AI 操作',
  'mcp.ui.confirm_title': '待确认的 AI 操作',
  'mcp.ui.confirm_hint': 'AI 客户端请求执行以下写操作。确认前请核对参数；确认后立即以你的身份执行。',
  'mcp.ui.approve': '确认执行',
  'mcp.ui.reject': '拒绝',
  'mcp.ui.tool': '工具',
  'mcp.ui.args': '参数',
  'mcp.ui.remaining': '剩余 {m} 分钟',
  'mcp.ui.done': '已执行',
  'mcp.ui.rejected': '已拒绝',
  'mcp.ui.consent_title': '授权 AI 客户端',
  'mcp.ui.consent_client': '“{name}” 请求以你的身份访问本站',
  'mcp.ui.consent_unverified': '客户端名称由客户端自行声明，未经验证。只授权你信任的客户端。',
  'mcp.ui.consent_redirect': '授权后返回：{uri}',
  'mcp.ui.allow': '授权',
  'mcp.ui.deny': '拒绝',
  'mcp.ui.login_required': '请先登录，再回到此页面完成授权。',
  'mcp.ui.empty': '暂无',
  'mcp.ui.no_pending': '没有待确认的操作',
});
registerMessages('en', {
  'mcp.ui.tab': 'AI clients',
  'mcp.ui.endpoint': 'MCP endpoint',
  'mcp.ui.endpoint_hint': 'Add this URL as a remote MCP server in your AI client. Clients that support OAuth will send you here to authorize; others can use a personal token below.',
  'mcp.ui.tokens': 'Personal MCP tokens',
  'mcp.ui.name': 'Name',
  'mcp.ui.scopes': 'Scopes',
  'mcp.ui.scope.read': 'Read',
  'mcp.ui.scope.write': 'Write (each call confirmed on site)',
  'mcp.ui.scope.admin': 'Admin tools',
  'mcp.ui.days': 'Valid for (days)',
  'mcp.ui.create': 'Create token',
  'mcp.ui.created': 'Token created — copy it now (it will not be shown again)',
  'mcp.ui.revoke': 'Revoke',
  'mcp.ui.expires': 'Expires',
  'mcp.ui.last_used': 'Last used',
  'mcp.ui.never': 'never',
  'mcp.ui.clients': 'Authorized clients',
  'mcp.ui.example': 'Configuration example',
  'mcp.ui.pending': 'AI actions to confirm',
  'mcp.ui.confirm_title': 'AI actions awaiting confirmation',
  'mcp.ui.confirm_hint': 'An AI client asked to perform these write operations. Check the arguments; once confirmed they run as you.',
  'mcp.ui.approve': 'Confirm',
  'mcp.ui.reject': 'Reject',
  'mcp.ui.tool': 'Tool',
  'mcp.ui.args': 'Arguments',
  'mcp.ui.remaining': '{m} min left',
  'mcp.ui.done': 'Done',
  'mcp.ui.rejected': 'Rejected',
  'mcp.ui.consent_title': 'Authorize an AI client',
  'mcp.ui.consent_client': '“{name}” wants to access this site as you',
  'mcp.ui.consent_unverified': 'The client name is self-declared and not verified. Only authorize clients you trust.',
  'mcp.ui.consent_redirect': 'You will be returned to: {uri}',
  'mcp.ui.allow': 'Authorize',
  'mcp.ui.deny': 'Deny',
  'mcp.ui.login_required': 'Log in first, then come back to this page to authorize.',
  'mcp.ui.empty': 'None',
  'mcp.ui.no_pending': 'Nothing to confirm',
});

const SCOPES = ['read', 'write', 'admin'] as const;

function errText(api: PluginApi, e: unknown): string {
  if (e instanceof ApiError) {
    const s = api.t(e.code);
    return s === e.code ? e.message : s;
  }
  return String(e);
}

interface Token {
  id: string;
  kind: string;
  name: string;
  scopes: string[];
  issued_by: string;
  expires_at: number | null;
  last_used_at: number | null;
}

interface Grant {
  client_id: string;
  client_name: string;
  scopes: string[];
  last_used_at: number | null;
}

const scopeTags = (xs: string[]) => (
  <div className="row">
    {xs.map((s) => (
      <Tag key={s} tone={s === 'admin' ? 'x' : s === 'write' ? 'o' : undefined}>
        {s}
      </Tag>
    ))}
  </div>
);

function SettingsTab({ api }: SlotProps) {
  const endpoint = `${window.location.origin}/mcp`;
  const [tokens, setTokens] = useState<Token[]>([]);
  const [grants, setGrants] = useState<Grant[]>([]);
  const [name, setName] = useState('Claude');
  const [scopes, setScopes] = useState<string[]>(['read']);
  const [days, setDays] = useState(30);
  const [created, setCreated] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(async () => {
    const [t, g] = await Promise.all([
      api.json<{ items: Token[] }>('/api/p/identity/v1/users/me/tokens'),
      api.json<{ items: Grant[] }>('/api/p/mcp/v1/me/grants'),
    ]);
    // OAuth 签发的访问令牌在“已授权的客户端”中管理
    setTokens(t.items.filter((x) => x.kind === 'mcp' && x.issued_by === 'user'));
    setGrants(g.items);
  }, [api]);
  useEffect(() => {
    void load().catch((e) => setErr(errText(api, e)));
  }, [api, load]);
  async function create() {
    setErr(null);
    try {
      const r = await api.json<{ token: string }>('/api/p/identity/v1/users/me/tokens', {
        method: 'POST',
        json: { kind: 'mcp', name, scopes, ttl_days: days },
        idempotent: true,
      });
      setCreated(r.token);
      await load();
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  async function revoke(id: string) {
    await api.fetch(`/api/p/identity/v1/users/me/tokens/${id}`, { method: 'DELETE' });
    await load();
  }
  async function revokeGrant(id: string) {
    await api.fetch(`/api/p/mcp/v1/me/grants/${encodeURIComponent(id)}`, { method: 'DELETE' });
    await load();
  }
  const when = (t: number | null) => <span className="small muted nowrap">{t ? new Date(t).toLocaleString() : api.t('mcp.ui.never')}</span>;
  const example = JSON.stringify({ mcpServers: { 'pt-studio': { type: 'http', url: endpoint, headers: { Authorization: 'Bearer ptm_…' } } } }, null, 2);
  return (
    <div className="stack">
      <div className="stack-s">
        <h3>{api.t('mcp.ui.endpoint')}</h3>
        <p className="small muted">{api.t('mcp.ui.endpoint_hint')}</p>
        <CodeBox copy={endpoint}>{endpoint}</CodeBox>
      </div>
      <div className="stack-s">
        <h3>{api.t('mcp.ui.tokens')}</h3>
        <form
          className="form"
          onSubmit={(e) => {
            e.preventDefault();
            void create();
          }}
        >
          <div className="form-2">
            <Field label={api.t('mcp.ui.name')}>
              <Input name="mcp_name" value={name} maxLength={64} onChange={(e) => setName(e.target.value)} />
            </Field>
            <Field label={api.t('mcp.ui.days')}>
              <Input name="mcp_days" type="number" min={1} max={90} value={days} onChange={(e) => setDays(Number(e.target.value) || 30)} />
            </Field>
          </div>
          <Field label={api.t('mcp.ui.scopes')}>
            <div className="row">
              {SCOPES.map((s) => (
                <Checkbox key={s} label={api.t(`mcp.ui.scope.${s}`)} checked={scopes.includes(s)} onChange={(v) => setScopes(v ? [...scopes, s] : scopes.filter((x) => x !== s))} />
              ))}
            </div>
          </Field>
          <div className="form-acts">
            <Button type="submit" variant="primary" disabled={!name.trim() || scopes.length === 0}>
              {api.t('mcp.ui.create')}
            </Button>
          </div>
        </form>
        {err && <Note tone="bad">{err}</Note>}
        {created && (
          <>
            <Note tone="warn">{api.t('mcp.ui.created')}</Note>
            <CodeBox copy={created}>
              <span data-testid="mcp-token">{created}</span>
            </CodeBox>
          </>
        )}
        {tokens.length > 0 && (
          <div className="panel">
            <Table
              rows={tokens}
              rowKey={(r) => r.id}
              columns={[
                { key: 'n', header: api.t('mcp.ui.name'), render: (r) => <b>{r.name}</b> },
                { key: 's', header: api.t('mcp.ui.scopes'), render: (r) => scopeTags(r.scopes) },
                { key: 'e', header: api.t('mcp.ui.expires'), render: (r) => when(r.expires_at) },
                { key: 'u', header: api.t('mcp.ui.last_used'), render: (r) => when(r.last_used_at) },
                {
                  key: 'x',
                  header: '',
                  right: true,
                  render: (r) => (
                    <Button size="sm" variant="ghost" onClick={() => void revoke(r.id)}>
                      {api.t('mcp.ui.revoke')}
                    </Button>
                  ),
                },
              ]}
            />
          </div>
        )}
      </div>
      <div className="stack-s">
        <h3>{api.t('mcp.ui.clients')}</h3>
        {grants.length === 0 ? (
          <p className="small muted">{api.t('mcp.ui.empty')}</p>
        ) : (
          <div className="panel">
            <Table
              rows={grants}
              rowKey={(r) => r.client_id}
              columns={[
                { key: 'n', header: api.t('mcp.ui.name'), render: (r) => <b>{r.client_name}</b> },
                { key: 's', header: api.t('mcp.ui.scopes'), render: (r) => scopeTags(r.scopes) },
                { key: 'u', header: api.t('mcp.ui.last_used'), render: (r) => when(r.last_used_at) },
                {
                  key: 'x',
                  header: '',
                  right: true,
                  render: (r) => (
                    <Button size="sm" variant="ghost" onClick={() => void revokeGrant(r.client_id)}>
                      {api.t('mcp.ui.revoke')}
                    </Button>
                  ),
                },
              ]}
            />
          </div>
        )}
      </div>
      <div className="stack-s">
        <h3>{api.t('mcp.ui.example')}</h3>
        <pre className="json">{example}</pre>
      </div>
    </div>
  );
}

interface Confirmation {
  id: string;
  tool: string;
  summary: string;
  args: Record<string, unknown>;
  expires_at: number;
}

function MenuItem({ api }: SlotProps) {
  const [n, setN] = useState(0);
  useEffect(() => {
    void api
      .json<{ items: Confirmation[] }>('/api/p/mcp/v1/me/confirmations')
      .then((r) => setN(r.items.length))
      .catch(() => undefined);
  }, [api]);
  if (n === 0) return null;
  return (
    <NavItem to="/p/mcp/confirmations" navigate={api.navigate} count={n} alert>
      {api.t('mcp.ui.pending')}
    </NavItem>
  );
}

function Confirmations({ api, only }: { api: PluginApi; only?: string }) {
  const [items, setItems] = useState<Confirmation[] | null>(null);
  const [done, setDone] = useState<Record<string, string>>({});
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    void api
      .json<{ items: Confirmation[] }>('/api/p/mcp/v1/me/confirmations')
      .then((r) => setItems(only ? r.items.filter((c) => c.id === only) : r.items))
      .catch((e) => {
        setErr(errText(api, e));
        setItems([]);
      });
  }, [api, only]);
  async function resolve(id: string, approve: boolean) {
    setErr(null);
    try {
      await api.json(`/api/p/mcp/v1/me/confirmations/${id}/${approve ? 'approve' : 'reject'}`, { method: 'POST', json: {} });
      setDone({ ...done, [id]: approve ? 'mcp.ui.done' : 'mcp.ui.rejected' });
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  return (
    <div className="stack">
      <PageHead title={api.t('mcp.ui.confirm_title')} lede={api.t('mcp.ui.confirm_hint')} />
      {err && <Note tone="bad">{err}</Note>}
      {items === null ? (
        <SkeletonRows n={4} />
      ) : items.length === 0 ? (
        <Note>{api.t('mcp.ui.no_pending')}</Note>
      ) : (
        items.map((c) => (
          <section key={c.id} className="panel" data-testid="mcp-confirmation" data-primary>
            <div className="panel-h">
              <h2 className="pt-mono">{c.tool}</h2>
              <div className="acts">
                <Status tone="warn">{api.t('mcp.ui.remaining', { m: Math.max(0, Math.round((c.expires_at - Date.now()) / 60000)) })}</Status>
              </div>
            </div>
            <div className="panel-b stack">
              {c.summary && <p>{c.summary}</p>}
              <dl className="kv">
                {Object.entries(c.args).map(([k, v]) => (
                  <Fragment key={k}>
                    <dt>{k}</dt>
                    <dd className="pt-mono">{typeof v === 'string' ? v : JSON.stringify(v)}</dd>
                  </Fragment>
                ))}
              </dl>
              {done[c.id] ? (
                <Status tone={done[c.id] === 'mcp.ui.done' ? 'ok' : 'mute'}>{api.t(done[c.id] ?? '')}</Status>
              ) : (
                <div className="form-acts">
                  <Button variant="danger" onClick={() => void resolve(c.id, true)}>
                    {api.t('mcp.ui.approve')}
                  </Button>
                  <Button variant="ghost" onClick={() => void resolve(c.id, false)}>
                    {api.t('mcp.ui.reject')}
                  </Button>
                </div>
              )}
            </div>
          </section>
        ))
      )}
    </div>
  );
}

function Consent({ api }: { api: PluginApi }) {
  const params = Object.fromEntries(new URLSearchParams(window.location.search));
  const [info, setInfo] = useState<{ client_name: string; scopes: string[]; redirect_uri: string } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    if (!api.user()) return;
    const q = new URLSearchParams(params).toString();
    void api
      .json<{ client_name: string; scopes: string[]; redirect_uri: string }>(`/api/p/mcp/v1/oauth/authorize-info?${q}`)
      .then(setInfo)
      .catch((e) => setErr(errText(api, e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api]);
  async function decide(approve: boolean) {
    try {
      const r = await api.json<{ redirect: string }>('/api/p/mcp/v1/oauth/consent', { method: 'POST', json: { params, approve } });
      window.location.assign(r.redirect);
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  return (
    <div className="stack narrow-form">
      <PageHead title={api.t('mcp.ui.consent_title')} />
      {!api.user() ? (
        <Note tone="warn">{api.t('mcp.ui.login_required')}</Note>
      ) : (
        <Panel primary>
          {err && <Note tone="bad">{err}</Note>}
          {info ? (
            <div className="stack">
              <h3>{api.t('mcp.ui.consent_client', { name: info.client_name })}</h3>
              <Note tone="warn">{api.t('mcp.ui.consent_unverified')}</Note>
              <ul className="stack-s">
                {info.scopes.map((s) => (
                  <li key={s}>{api.t(`mcp.ui.scope.${s}`)}</li>
                ))}
              </ul>
              <p className="small muted">{api.t('mcp.ui.consent_redirect', { uri: info.redirect_uri })}</p>
              <div className="form-acts">
                <Button variant="primary" onClick={() => void decide(true)}>
                  {api.t('mcp.ui.allow')}
                </Button>
                <Button variant="ghost" onClick={() => void decide(false)}>
                  {api.t('mcp.ui.deny')}
                </Button>
              </div>
            </div>
          ) : (
            !err && <SkeletonRows n={3} />
          )}
        </Panel>
      )}
    </div>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  const rest = path.replace(/^\/p\/mcp\/?/, '');
  if (rest.startsWith('consent')) return <Consent api={api} />;
  if (rest.startsWith('confirmations')) {
    const id = rest.split('/')[1];
    return <Confirmations api={api} only={id || undefined} />;
  }
  return (
    <div className="stack">
      <PageHead title={api.t('mcp.ui.tab')} />
      <Panel primary>
        <SettingsTab api={api} ctx={{}} />
      </Panel>
    </div>
  );
}

export default {
  slots: {
    'user.settings.tabs': { component: SettingsTab, order: 40, title_term: 'mcp.ui.tab' },
    'nav.user_menu': { component: MenuItem, order: 60 },
  },
  routes: [{ path: '/p/mcp/*', component: Pages }],
} satisfies PluginFrontend;
