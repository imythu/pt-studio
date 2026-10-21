// 后台用户：列表（用户名前缀、状态、游标）、按 IP 查账户（抽屉）；详情（资料、角色、限制、会话、安全记录、IP 与关联账户 + 插件 admin.user.tabs 页签）。
import { useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/react-query';
import { Suspense, useState, type FormEvent } from 'react';
import { Link, useNavigate, useParams } from 'react-router';
import { api } from '@pt-studio/sdk';
import { SlotEntryView, useSlotEntries, useSlotEntryTitle } from '@pt-studio/slots';
import { useT, type Translator } from '@pt-studio/terms';
import { Avatar, Button, Field, Input, Kpis, LoadMore, Note, PageHead, Panel, Select, Sheet, SkeletonRows, Status, Table, Tabs, Tag, useToast, type Tone } from '@pt-studio/ui-kit';
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

export function statusTone(s: string): Tone {
  return s === 'active' ? 'ok' : s === 'pending' ? 'warn' : s === 'banned' ? 'bad' : 'warn';
}

/** 账号状态 / 限制类型的显示名；未知值原样显示 */
export function statusName(t: Translator['t'], s: string): string {
  const k = `ui.admin.status.${s}`;
  const v = t(k);
  return v === k ? s : v;
}

/** 角色显示名：内置角色用 identity.role.<name>，插件角色 <plugin>:<name> 用 <plugin>.role.<name>；没有术语时显示原名 */
export function roleName(t: Translator['t'], r: string): string {
  const [p, n] = r.includes(':') ? r.split(':', 2) : ['identity', r];
  const k = `${p}.role.${n}`;
  const v = t(`term:${k}`);
  return v === k ? r : v;
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
  const t = useT();
  return (
    <div className="side-list plain">
      {rows.map((u) => (
        <Link key={u.user_id} to={`/admin/users/${u.user_id}`}>
          <span className="nm">
            {u.username} <Status tone={statusTone(u.status)}>{statusName(t, u.status)}</Status>
          </span>
          <span className="v">{extra(u)}</span>
        </Link>
      ))}
      {rows.length === 0 && <div className="muted">—</div>}
    </div>
  );
}

function IpLookup({ open, onClose }: { open: boolean; onClose(): void }) {
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
    <Sheet open={open} onClose={onClose} title={t('ui.admin.ip_lookup')}>
      <form className="row" onSubmit={search}>
        <Input name="ip" placeholder="203.0.113.7 / 203.0.113.0/24" value={ip} onChange={(e) => setIp(e.target.value)} required style={{ flex: 1 }} />
        <Button type="submit" variant="primary">
          {t('ui.admin.ip_lookup_go')}
        </Button>
      </form>
      <Note tone="warn">{t('ui.admin.sensitive_hint')}</Note>
      {rows && <UserLinks rows={rows} extra={(u) => new Date(u.last_seen_at).toLocaleString()} />}
    </Sheet>
  );
}

export function AdminUsers() {
  const t = useT();
  const nav = useNavigate();
  const [prefix, setPrefix] = useState('');
  const [filter, setFilter] = useState({ prefix: '', status: '' });
  const [ipOpen, setIpOpen] = useState(false);
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
    <div className="stack">
      <PageHead
        title={t('ui.admin.users')}
        actions={
          sensitive && (
            <Button size="sm" onClick={() => setIpOpen(true)}>
              {t('ui.admin.ip_lookup')}
            </Button>
          )
        }
      />
      <IpLookup open={ipOpen} onClose={() => setIpOpen(false)} />
      <Panel flush primary>
        <form
          className="fbar"
          onSubmit={(e) => {
            e.preventDefault();
            setFilter({ ...filter, prefix });
          }}
        >
          <Input name="username_prefix" placeholder={t('ui.admin.user_search')} value={prefix} onChange={(e) => setPrefix(e.target.value)} style={{ maxWidth: 280 }} />
          <Select name="status" value={filter.status} onChange={(e) => setFilter({ ...filter, status: e.target.value })} options={STATUSES.map((s) => ({ value: s, label: s ? statusName(t, s) : t('ui.admin.all_status') }))} />
          <Button type="submit" size="sm" variant="primary">
            {t('ui.admin.search')}
          </Button>
        </form>
        <ErrorText error={q.error} />
        {q.isLoading ? (
          <SkeletonRows n={8} />
        ) : (
          <Table
            rows={rows}
            rowKey={(r) => r.id}
            minWidth={820}
            empty={t('ui.admin.no_users')}
            onRowClick={(r) => nav(`/admin/users/${r.id}`)}
            columns={[
              { key: 'id', header: 'ID', render: (r) => <span className="mono">{r.id}</span> },
              { key: 'u', header: t('ui.login.username'), render: (r) => <Link className="u" to={`/admin/users/${r.id}`}>{r.username}</Link> },
              { key: 's', header: t('ui.admin.status'), render: (r) => <Status tone={statusTone(r.status)}>{statusName(t, r.status)}</Status> },
              { key: 'r', header: t('ui.admin.roles'), render: (r) => <span className="row" style={{ gap: 4 }}>{r.roles.filter((x) => !x.startsWith('levels:') && x !== 'user' && x !== 'guest').map((x) => <Tag key={x}>{roleName(t, x)}</Tag>)}</span> },
              { key: 'c', header: t('ui.profile.joined'), render: (r) => <span className="small nowrap">{new Date(r.created_at).toLocaleDateString()}</span> },
              { key: 'l', header: t('ui.settings.last_seen'), render: (r) => <span className="small nowrap">{r.last_seen_at ? new Date(r.last_seen_at).toLocaleString() : '—'}</span> },
            ]}
          />
        )}
        {q.hasNextPage && <LoadMore onClick={() => void q.fetchNextPage()} loading={q.isFetchingNextPage} label={t('ui.torrent.load_more')} />}
      </Panel>
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

interface SecurityEvent {
  id: number;
  kind: string;
  details: { ip_prefix?: string; ua?: string; method?: string; new_device?: boolean };
  created_at: number;
}

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
  if (!data)
    return (
      <Note tone="warn" actions={<Button size="xs" onClick={load}>{t('ui.admin.load_sensitive')}</Button>}>
        <span>{t('ui.admin.sensitive_hint')}</span>
      </Note>
    );
  return (
    <div className="g2">
      <Table
        rows={data.ips}
        rowKey={(r) => `${r.ip}-${r.source}-${r.last_seen_at}`}
        empty="—"
        columns={[
          { key: 'ip', header: 'IP', render: (r) => <code className="mono">{r.ip}</code> },
          { key: 's', header: t('ui.admin.source'), render: (r) => t(`ui.security.source.${r.source}`) },
          { key: 'h', header: t('ui.settings.hits'), right: true, render: (r) => <span className="num">{r.hits}</span> },
          { key: 't', header: t('ui.settings.last_seen'), render: (r) => <span className="small nowrap">{new Date(r.last_seen_at).toLocaleString()}</span> },
        ]}
      />
      <div className="stack-s">
        <h3>{t('ui.admin.shared_ip_users')}</h3>
        <UserLinks rows={data.shared} extra={(u) => t('ui.admin.shared_ips_n', { n: u.shared_ips ?? 0 })} />
      </div>
    </div>
  );
}

function UserSecurityEvents({ id }: { id: number }) {
  const t = useT();
  const q = useQuery({ queryKey: ['admin-security-events', id], queryFn: () => api<{ items: SecurityEvent[] }>(`/api/p/identity/v1/admin/users/${id}/security-events`) });
  if (q.error) return <ErrorText error={q.error} />;
  return (
    <Table
      rows={q.data?.items ?? []}
      rowKey={(e) => e.id}
      empty="—"
      minWidth={640}
      columns={[
        { key: 'k', header: t('ui.admin.kind'), render: (e) => <span className="row" style={{ gap: 6 }}>{t(`ui.security.${e.kind}`)} {e.details.new_device && <Tag tone="x">{t('ui.security.new_device')}</Tag>}</span> },
        { key: 'ip', header: 'IP', render: (e) => <span className="mono">{e.details.ip_prefix ?? '—'}</span> },
        { key: 'ua', header: t('ui.settings.device'), render: (e) => <span className="small">{e.details.ua ?? '—'}</span> },
        { key: 't', header: t('ui.settings.time'), render: (e) => <span className="small nowrap">{new Date(e.created_at).toLocaleString()}</span> },
      ]}
    />
  );
}

function Overview({ d, refresh }: { d: Detail; refresh(): Promise<unknown> }) {
  const t = useT();
  const toast = useToast();
  const errText = useErrorText();
  const [ban, setBan] = useState({ kind: 'ban', reason_code: '', days: '' });
  const [role, setRole] = useState('uploader');
  const [title, setTitle] = useState<string | null>(null);
  const act = async (f: () => Promise<unknown>, ok?: string) => {
    try {
      await f();
      if (ok) toast(ok, 'success');
      await refresh();
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
    <div className="g2">
      <div className="stack">
        <section className="stack-s">
          <h3>{t('ui.settings.title_field')}</h3>
          <form
            className="row"
            onSubmit={(e) => {
              e.preventDefault();
              void act(() => api(`/api/p/identity/v1/admin/users/${d.id}`, { method: 'PATCH', json: { title: (title ?? d.title) || null } }), t('ui.admin.saved'));
            }}
          >
            <Input value={title ?? d.title ?? ''} maxLength={30} onChange={(e) => setTitle(e.target.value)} style={{ flex: 1 }} />
            <Button type="submit" size="sm">
              {t('ui.admin.save')}
            </Button>
          </form>
        </section>
        <section className="stack-s">
          <h3>{t('ui.admin.roles')}</h3>
          <div className="row">
            {d.granted_roles.map((r) => (
              <span key={r.role} className="chip-x" title={`${r.role} · ${r.granted_by}`}>
                {roleName(t, r.role)}
                <button type="button" className="txt-btn" aria-label={t('ui.common.remove')} onClick={() => act(() => api(`/api/p/identity/v1/admin/users/${d.id}/roles/${r.role}`, { method: 'DELETE' }))}>
                  ×
                </button>
              </span>
            ))}
            {d.granted_roles.length === 0 && <span className="small muted">—</span>}
          </div>
          <div className="row">
            <Select value={role} onChange={(e) => setRole(e.target.value)} options={ROLES.map((r) => ({ value: r, label: roleName(t, r) }))} style={{ width: 'auto' }} />
            <Button size="sm" onClick={() => act(() => api(`/api/p/identity/v1/admin/users/${d.id}/roles`, { json: { role } }), t('ui.admin.saved'))}>
              {t('ui.admin.grant')}
            </Button>
          </div>
        </section>
      </div>
      <section className="stack-s">
        <div className="row-sb">
          <h3>{t('ui.admin.restrictions')}</h3>
          <Button size="xs" variant="danger" onClick={() => act(() => api(`/api/p/identity/v1/admin/users/${d.id}/session-revocations`, { json: {}, idempotent: true }), t('ui.admin.saved'))}>
            {t('ui.admin.revoke_sessions')}
          </Button>
        </div>
        <Table
          rows={d.restrictions}
          rowKey={(r) => r.id}
          empty="—"
          columns={[
            { key: 'k', header: t('ui.admin.kind'), render: (r) => <Status tone={r.lifted_at ? 'mute' : 'bad'}>{statusName(t, r.kind)}</Status> },
            { key: 'r', header: t('ui.admin.reason'), render: (r) => <span className="small">{r.reason_code}<span className="muted"> · {r.source}</span></span> },
            { key: 'u', header: t('ui.admin.until'), render: (r) => <span className="small nowrap">{r.until ? new Date(r.until).toLocaleString() : '∞'}</span> },
            {
              key: 'a',
              header: '',
              render: (r) =>
                r.lifted_at ? (
                  <span className="tiny muted">{t('ui.admin.lifted')}</span>
                ) : (
                  <div className="acts">
                    <Button size="xs" onClick={() => act(() => api(`/api/p/identity/v1/admin/users/${d.id}/restrictions/${r.id}`, { method: 'DELETE' }), t('ui.admin.saved'))}>
                      {t('ui.admin.lift')}
                    </Button>
                  </div>
                ),
            },
          ]}
        />
        <form onSubmit={restrict} className="form-2">
          <Field label={t('ui.admin.kind')}>
            <Select name="kind" value={ban.kind} onChange={(e) => setBan({ ...ban, kind: e.target.value })} options={['ban', 'leech_disabled'].map((k) => ({ value: k, label: statusName(t, k) }))} />
          </Field>
          <Field label={t('ui.admin.days')}>
            <Input name="days" type="number" min={1} value={ban.days} onChange={(e) => setBan({ ...ban, days: e.target.value })} />
          </Field>
          <Field label={t('ui.admin.reason')} className="span2">
            <Input name="reason_code" value={ban.reason_code} onChange={(e) => setBan({ ...ban, reason_code: e.target.value })} required maxLength={64} />
          </Field>
          <div className="span2">
            <Button type="submit" size="sm" variant="danger">
              {t('ui.admin.apply_restriction')}
            </Button>
          </div>
        </form>
      </section>
    </div>
  );
}

function UserTabs({ d, refresh }: { d: Detail; refresh(): Promise<unknown> }) {
  const t = useT();
  const s = useSession();
  const entries = useSlotEntries('admin.user.tabs');
  const title = useSlotEntryTitle();
  const [tab, setTab] = useState('overview');
  const tabs = [
    { id: 'overview', label: t('ui.admin.overview') },
    ...(s.can('identity.users.read') ? [{ id: 'security', label: t('ui.settings.security_events') }] : []),
    ...(s.can('identity.users.read_sensitive') ? [{ id: 'ips', label: t('ui.admin.ip_and_accounts') }] : []),
    ...entries.map((e) => ({ id: `p:${e.plugin}`, label: title(e) })),
  ];
  const cur = entries.find((e) => `p:${e.plugin}` === tab);
  return (
    <Panel flush primary>
      <Tabs tabs={tabs} value={tab} onChange={setTab} />
      <div className="tabp">
        {tab === 'overview' && <Overview d={d} refresh={refresh} />}
        {tab === 'security' && <UserSecurityEvents id={d.id} />}
        {tab === 'ips' && <UserIps id={d.id} />}
        {cur && <SlotEntryView entry={cur} ctx={{ profile: d, user: s.me }} />}
      </div>
    </Panel>
  );
}

export function AdminUser() {
  const t = useT();
  const qc = useQueryClient();
  const { id } = useParams();
  const key = ['admin-user', id];
  const q = useQuery({ queryKey: key, queryFn: () => api<Detail>(`/api/p/identity/v1/admin/users/${id}`) });
  if (q.isLoading) return <SkeletonRows n={6} />;
  if (q.error) return <ErrorText error={q.error} />;
  const d = q.data as Detail;
  return (
    <div className="stack">
      <PageHead
        crumb={
          <>
            <Link to="/admin/users">{t('ui.admin.users')}</Link>
            <span>#{d.id}</span>
          </>
        }
        title={
          <span className="row" style={{ gap: 12 }}>
            <Avatar name={d.username} size={44} />
            {d.username}
            <Status tone={statusTone(d.status)}>{statusName(t, d.status)}</Status>
          </span>
        }
        actions={
          <Link className="btn btn-sm" to={`/users/${d.id}`}>
            {t('ui.nav.profile')}
          </Link>
        }
      />
      <Kpis
        items={[
          { label: 'ID', value: d.id },
          { label: t('ui.admin.roles'), value: d.roles.filter((x) => !x.startsWith('levels:') && x !== 'guest').length },
          { label: t('ui.settings.sessions'), value: d.sessions },
          { label: t('ui.profile.joined'), value: new Date(d.created_at).toLocaleDateString() },
        ]}
      />
      <Suspense fallback={<SkeletonRows n={4} />}>
        <UserTabs d={d} refresh={() => qc.invalidateQueries({ queryKey: key })} />
      </Suspense>
    </div>
  );
}
