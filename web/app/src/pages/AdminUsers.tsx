// 后台用户管理：列表（筛选、游标）、详情（资料、角色、限制、会话、安全记录）；插件经 admin.user.tabs 追加标签页。
import { useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';
import { Link, useParams } from 'react-router';
import { api } from '@pt-studio/sdk';
import { Slot } from '@pt-studio/slots';
import { useT } from '@pt-studio/terms';
import { Badge, Button, Card, Field, Input, Spinner, Stat, Table, useToast } from '@pt-studio/ui-kit';
import { ErrorText, useErrorText } from '../errors';
import { useSession } from '../session';

interface Row {
  id: number;
  username: string;
  class_id: number;
  status: string;
  created_at: number;
  last_seen_at: number | null;
  roles: string[];
}

const STATUSES = ['', 'active', 'pending', 'leech_disabled', 'banned'];
const ROLES = ['uploader', 'moderator', 'admin'];

export function AdminUsers() {
  const t = useT();
  const [prefix, setPrefix] = useState('');
  const [filter, setFilter] = useState({ prefix: '', status: '' });
  const q = useInfiniteQuery({
    queryKey: ['admin-users', filter],
    initialPageParam: null as number | null,
    queryFn: ({ pageParam }) => {
      const p = new URLSearchParams();
      if (filter.prefix) p.set('username_prefix', filter.prefix);
      if (filter.status) p.set('status', filter.status);
      if (pageParam) p.set('cursor', String(pageParam));
      return api<{ items: Row[]; next_cursor: number | null }>(`/api/p/identity/v1/admin/users?${p.toString()}`);
    },
    getNextPageParam: (last) => last.next_cursor,
  });
  const rows = q.data?.pages.flatMap((p) => p.items) ?? [];
  const sensitive = useSession().can('identity.users.read_sensitive');
  return (
    <div>
      <h1>{t('ui.admin.users')}</h1>
      {sensitive && <IpLookup />}
      <form
        className="searchbar"
        onSubmit={(e) => {
          e.preventDefault();
          setFilter({ ...filter, prefix });
        }}
      >
        <Input name="username_prefix" placeholder={t('ui.login.username')} value={prefix} onChange={(e) => setPrefix(e.target.value)} />
        <select className="pt-input" name="status" value={filter.status} onChange={(e) => setFilter({ ...filter, status: e.target.value })}>
          {STATUSES.map((s) => (
            <option key={s} value={s}>
              {s || '—'}
            </option>
          ))}
        </select>
        <Button type="submit">{t('ui.admin.search')}</Button>
      </form>
      <ErrorText error={q.error} />
      <Table
        rows={rows}
        rowKey={(r) => r.id}
        columns={[
          { key: 'id', header: 'ID', render: (r) => r.id },
          { key: 'u', header: t('ui.login.username'), render: (r) => <Link to={`/admin/users/${r.id}`}>{r.username}</Link> },
          { key: 's', header: t('ui.admin.status'), render: (r) => <Badge tone={r.status === 'active' ? 'muted' : 'danger'}>{r.status}</Badge> },
          { key: 'r', header: t('ui.admin.roles'), render: (r) => r.roles.join(', ') },
          { key: 'c', header: t('ui.profile.joined'), render: (r) => new Date(r.created_at).toLocaleDateString() },
        ]}
      />
      {q.hasNextPage && <Button onClick={() => void q.fetchNextPage()}>{t('ui.torrent.load_more')}</Button>}
    </div>
  );
}

interface Detail {
  id: number;
  username: string;
  status: string;
  class_id: number;
  title: string | null;
  created_at: number;
  roles: string[];
  granted_roles: { role: string; granted_by: string }[];
  sessions: number;
  restrictions: { id: number; kind: string; source: string; reason_code: string; until: number | null; lifted_at: number | null }[];
}

interface IpRow {
  ip: string;
  source: string;
  hits: number;
  last_seen_at: number;
}

interface IpUser {
  user_id: number;
  username: string;
  status: string;
  last_seen_at: number;
  shared_ips?: number;
  hits?: number;
}

function UserLinks({ rows, extra }: { rows: IpUser[]; extra: (u: IpUser) => string | number }) {
  return (
    <ul>
      {rows.map((u) => (
        <li key={u.user_id}>
          <Link to={`/admin/users/${u.user_id}`}>{u.username}</Link> <Badge tone={u.status === 'active' ? 'muted' : 'danger'}>{u.status}</Badge>{' '}
          <span className="pt-muted small">{extra(u)}</span>
        </li>
      ))}
      {rows.length === 0 && <li className="pt-muted">—</li>}
    </ul>
  );
}

/** IP 历史与同 IP 账户：敏感数据，点击后加载（服务端记审计） */
function UserIps({ id }: { id: number }) {
  const t = useT();
  const toast = useToast();
  const errText = useErrorText();
  const [data, setData] = useState<{ ips: IpRow[]; shared: IpUser[] } | null>(null);
  const load = async () => {
    try {
      const [ips, shared] = await Promise.all([
        api<{ items: IpRow[] }>(`/api/p/identity/v1/admin/users/${id}/ip-history`),
        api<{ items: IpUser[] }>(`/api/p/identity/v1/admin/users/${id}/shared-ips`),
      ]);
      setData({ ips: ips.items, shared: shared.items });
    } catch (e) {
      toast(errText(e), 'error');
    }
  };
  return (
    <Card title={t('ui.admin.ip_and_accounts')} actions={data ? null : <Button onClick={load}>{t('ui.admin.load_sensitive')}</Button>}>
      {data ? (
        <>
          <Table
            rows={data.ips}
            rowKey={(r) => `${r.ip}-${r.source}-${r.last_seen_at}`}
            empty="—"
            columns={[
              { key: 'ip', header: 'IP', render: (r) => <code className="pt-mono">{r.ip}</code> },
              { key: 's', header: t('ui.admin.source'), render: (r) => t(`ui.security.source.${r.source}`) },
              { key: 'h', header: t('ui.settings.hits'), render: (r) => r.hits },
              { key: 't', header: t('ui.settings.last_seen'), render: (r) => new Date(r.last_seen_at).toLocaleString() },
            ]}
          />
          <h3>{t('ui.admin.shared_ip_users')}</h3>
          <UserLinks rows={data.shared} extra={(u) => t('ui.admin.shared_ips_n', { n: u.shared_ips ?? 0 })} />
        </>
      ) : (
        <p className="pt-muted small">{t('ui.admin.sensitive_hint')}</p>
      )}
    </Card>
  );
}

interface SecurityEvent {
  id: number;
  kind: string;
  details: { ip_prefix?: string; ua?: string; method?: string; new_device?: boolean };
  created_at: number;
}

/** 安全记录（登录、改密、两步验证、新设备等，最近 200 条） */
function UserSecurityEvents({ id }: { id: number }) {
  const t = useT();
  const q = useQuery({ queryKey: ['admin-security-events', id], queryFn: () => api<{ items: SecurityEvent[] }>(`/api/p/identity/v1/admin/users/${id}/security-events`) });
  return (
    <Card title={t('ui.settings.security_events')}>
      {q.error ? (
        <ErrorText error={q.error} />
      ) : (
        <Table
          rows={q.data?.items ?? []}
          rowKey={(e) => e.id}
          empty="—"
          columns={[
            { key: 'k', header: t('ui.admin.kind'), render: (e) => <>{t(`ui.security.${e.kind}`)} {e.details.new_device && <Badge tone="danger">{t('ui.security.new_device')}</Badge>}</> },
            { key: 'ip', header: 'IP', render: (e) => e.details.ip_prefix ?? '—' },
            { key: 'ua', header: t('ui.settings.device'), render: (e) => <span className="small">{e.details.ua ?? '—'}</span> },
            { key: 't', header: t('ui.settings.time'), render: (e) => new Date(e.created_at).toLocaleString() },
          ]}
        />
      )}
    </Card>
  );
}

/** 按 IP / CIDR 查账户 */
export function IpLookup() {
  const t = useT();
  const toast = useToast();
  const errText = useErrorText();
  const [ip, setIp] = useState('');
  const [rows, setRows] = useState<IpUser[] | null>(null);
  const search = async (e: FormEvent) => {
    e.preventDefault();
    try {
      setRows((await api<{ items: IpUser[] }>(`/api/p/identity/v1/admin/ip-lookups?ip=${encodeURIComponent(ip.trim())}`)).items);
    } catch (err) {
      toast(errText(err), 'error');
    }
  };
  return (
    <Card title={t('ui.admin.ip_lookup')}>
      <form className="searchbar" onSubmit={search}>
        <Input name="ip" placeholder="203.0.113.7 / 203.0.113.0/24" value={ip} onChange={(e) => setIp(e.target.value)} required />
        <Button type="submit">{t('ui.admin.ip_lookup_go')}</Button>
      </form>
      {rows && <UserLinks rows={rows} extra={(u) => new Date(u.last_seen_at).toLocaleString()} />}
    </Card>
  );
}

export function AdminUser() {
  const t = useT();
  const toast = useToast();
  const errText = useErrorText();
  const qc = useQueryClient();
  const s = useSession();
  const { id } = useParams();
  const key = ['admin-user', id];
  const q = useQuery({ queryKey: key, queryFn: () => api<Detail>(`/api/p/identity/v1/admin/users/${id}`) });
  const [ban, setBan] = useState({ kind: 'ban', reason_code: '', days: '' });
  const [role, setRole] = useState('uploader');
  const [title, setTitle] = useState<string | null>(null);
  if (q.isLoading) return <Spinner />;
  if (q.error) return <ErrorText error={q.error} />;
  const d = q.data as Detail;
  const act = async (f: () => Promise<unknown>, ok?: string) => {
    try {
      await f();
      if (ok) toast(ok, 'success');
      await qc.invalidateQueries({ queryKey: key });
    } catch (e) {
      toast(errText(e), 'error');
    }
  };
  const restrict = (e: FormEvent) => {
    e.preventDefault();
    const until = ban.days ? Date.now() + Number(ban.days) * 86_400_000 : undefined;
    void act(() => api(`/api/p/identity/v1/admin/users/${d.id}/restrictions`, { json: { kind: ban.kind, reason_code: ban.reason_code, until }, idempotent: true }), t('ui.admin.saved'));
  };
  return (
    <div>
      <h1>
        {d.username} <Badge tone={d.status === 'active' ? 'muted' : 'danger'}>{d.status}</Badge>
      </h1>
      <Card>
        <Stat label="ID" value={d.id} />
        <Stat label={t('ui.admin.roles')} value={d.roles.join(', ')} />
        <Stat label={t('ui.settings.sessions')} value={d.sessions} />
        <Stat label={t('ui.profile.joined')} value={new Date(d.created_at).toLocaleDateString()} />
        <p>
          <Link to={`/users/${d.id}`}>{t('ui.nav.profile')}</Link>
        </p>
      </Card>
      <Card title={t('ui.settings.title_field')}>
        <form
          className="searchbar"
          onSubmit={(e) => {
            e.preventDefault();
            void act(() => api(`/api/p/identity/v1/admin/users/${d.id}`, { method: 'PATCH', json: { title: (title ?? d.title) || null } }), t('ui.admin.saved'));
          }}
        >
          <Input value={title ?? d.title ?? ''} maxLength={30} onChange={(e) => setTitle(e.target.value)} />
          <Button type="submit">{t('ui.admin.save')}</Button>
        </form>
      </Card>
      <Card title={t('ui.admin.roles')}>
        <ul>
          {d.granted_roles.map((r) => (
            <li key={r.role}>
              {r.role} <span className="pt-muted small">({r.granted_by})</span>{' '}
              <Button variant="ghost" onClick={() => act(() => api(`/api/p/identity/v1/admin/users/${d.id}/roles/${r.role}`, { method: 'DELETE' }))}>
                ✕
              </Button>
            </li>
          ))}
        </ul>
        <div className="searchbar">
          <select className="pt-input" value={role} onChange={(e) => setRole(e.target.value)}>
            {ROLES.map((r) => (
              <option key={r}>{r}</option>
            ))}
          </select>
          <Button onClick={() => act(() => api(`/api/p/identity/v1/admin/users/${d.id}/roles`, { json: { role } }), t('ui.admin.saved'))}>{t('ui.admin.grant')}</Button>
        </div>
      </Card>
      <Card
        title={t('ui.admin.restrictions')}
        actions={
          <Button variant="danger" onClick={() => act(() => api(`/api/p/identity/v1/admin/users/${d.id}/session-revocations`, { json: {}, idempotent: true }), t('ui.admin.saved'))}>
            {t('ui.admin.revoke_sessions')}
          </Button>
        }
      >
        <Table
          rows={d.restrictions}
          rowKey={(r) => r.id}
          empty="—"
          columns={[
            { key: 'k', header: t('ui.admin.kind'), render: (r) => <Badge tone={r.lifted_at ? 'muted' : 'danger'}>{r.kind}</Badge> },
            { key: 's', header: t('ui.admin.source'), render: (r) => r.source },
            { key: 'r', header: t('ui.admin.reason'), render: (r) => r.reason_code },
            { key: 'u', header: t('ui.admin.until'), render: (r) => (r.until ? new Date(r.until).toLocaleString() : '∞') },
            {
              key: 'a',
              header: '',
              render: (r) =>
                r.lifted_at ? (
                  <span className="pt-muted small">{t('ui.admin.lifted')}</span>
                ) : (
                  <Button onClick={() => act(() => api(`/api/p/identity/v1/admin/users/${d.id}/restrictions/${r.id}`, { method: 'DELETE' }), t('ui.admin.saved'))}>
                    {t('ui.admin.lift')}
                  </Button>
                ),
            },
          ]}
        />
        <form onSubmit={restrict} className="searchbar">
          <select className="pt-input" name="kind" value={ban.kind} onChange={(e) => setBan({ ...ban, kind: e.target.value })}>
            <option value="ban">ban</option>
            <option value="leech_disabled">leech_disabled</option>
          </select>
          <Input name="reason_code" placeholder={t('ui.admin.reason')} value={ban.reason_code} onChange={(e) => setBan({ ...ban, reason_code: e.target.value })} required maxLength={64} />
          <Input name="days" type="number" min={1} placeholder={t('ui.admin.days')} value={ban.days} onChange={(e) => setBan({ ...ban, days: e.target.value })} />
          <Button type="submit" variant="danger">
            {t('ui.admin.apply_restriction')}
          </Button>
        </form>
      </Card>
      {s.can('identity.users.read') && <UserSecurityEvents id={d.id} />}
      {s.can('identity.users.read_sensitive') && <UserIps id={d.id} />}
      <Slot name="admin.user.tabs" ctx={{ profile: d, user: s.me }} />
    </div>
  );
}
