// ratio-policy 前端：个人页分享率（含要求与警告）、“分享率考核”页（我的状态、要求表）、后台用户标签页（警告 / 解除 / 豁免）与警告列表。
import { useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Field, Input, Kpis, Note, PageHead, Panel, PLink, Sheet, SkeletonRows, Stat, Status, Table, Tabs, Tag, type Tone } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'ratio-policy.ui.title': '分享率考核',
  'ratio-policy.ui.ratio': '分享率',
  'ratio-policy.ui.required': '要求',
  'ratio-policy.ui.need': '还需上传 {n} GiB',
  'ratio-policy.ui.state.ok': '正常',
  'ratio-policy.ui.state.warned': '警告中',
  'ratio-policy.ui.state.restricted': '已禁止下载',
  'ratio-policy.ui.state.banned': '已封禁',
  'ratio-policy.ui.warned_until': '请在 {at} 前达标，否则将被禁止下载',
  'ratio-policy.ui.exempt.manual': '管理员豁免',
  'ratio-policy.ui.exempt.low_download': '下载量较少，暂不考核',
  'ratio-policy.ui.exempt.grace': '新用户宽限期',
  'ratio-policy.ui.exempt.class': '所在等级不考核',
  'ratio-policy.ui.table': '要求分享率',
  'ratio-policy.ui.downloaded': '下载量 ≥',
  'ratio-policy.ui.reduction': '做种减免 {p}%：仍在做种的已完成种子越多，要求越低。',
  'ratio-policy.ui.deadline_days': '警告期（天）',
  'ratio-policy.ui.reason': '原因',
  'ratio-policy.ui.warn': '发出警告',
  'ratio-policy.ui.lift': '解除',
  'ratio-policy.ui.exempt_days': '豁免天数',
  'ratio-policy.ui.exempt': '豁免',
  'ratio-policy.ui.done': '已处理',
  'ratio-policy.ui.list': '分享率警告',
  'ratio-policy.download_restricted': '分享率警告期间不能下载新种子',
  'ratio-policy.restricted': '分享率警告或限制中，不能执行该操作',
  'ratio-policy.not_warned': '该用户不在警告或限制中',
  'ratio-policy.ui.status': '状态',
  'ratio-policy.ui.user': '用户',
  'ratio-policy.ui.deadline': '截止',
  'ratio-policy.ui.manual': '手动',
  'ratio-policy.ui.state.restricted_skipped': '已跳过限制',
});
registerMessages('en', {
  'ratio-policy.ui.title': 'Ratio policy',
  'ratio-policy.ui.ratio': 'Ratio',
  'ratio-policy.ui.required': 'Required',
  'ratio-policy.ui.need': 'Upload {n} GiB more',
  'ratio-policy.ui.state.ok': 'OK',
  'ratio-policy.ui.state.warned': 'Warned',
  'ratio-policy.ui.state.restricted': 'Downloads disabled',
  'ratio-policy.ui.state.banned': 'Banned',
  'ratio-policy.ui.warned_until': 'Meet the requirement by {at} or downloading will be disabled',
  'ratio-policy.ui.exempt.manual': 'Exempted by staff',
  'ratio-policy.ui.exempt.low_download': 'Not enough downloaded to be checked yet',
  'ratio-policy.ui.exempt.grace': 'New-member grace period',
  'ratio-policy.ui.exempt.class': 'Your class is exempt',
  'ratio-policy.ui.table': 'Required ratio',
  'ratio-policy.ui.downloaded': 'Downloaded ≥',
  'ratio-policy.ui.reduction': 'Seeding reduction {p}%: the more completed torrents you still seed, the lower the requirement.',
  'ratio-policy.ui.deadline_days': 'Warning period (days)',
  'ratio-policy.ui.reason': 'Reason',
  'ratio-policy.ui.warn': 'Warn',
  'ratio-policy.ui.lift': 'Lift',
  'ratio-policy.ui.exempt_days': 'Exempt for (days)',
  'ratio-policy.ui.exempt': 'Exempt',
  'ratio-policy.ui.done': 'Done',
  'ratio-policy.ui.list': 'Ratio warnings',
  'ratio-policy.download_restricted': 'New downloads are disabled during a ratio warning',
  'ratio-policy.restricted': 'Not allowed while under a ratio warning or restriction',
  'ratio-policy.not_warned': 'The user is not under a ratio warning',
  'ratio-policy.ui.status': 'Status',
  'ratio-policy.ui.user': 'User',
  'ratio-policy.ui.deadline': 'Deadline',
  'ratio-policy.ui.manual': 'manual',
  'ratio-policy.ui.state.restricted_skipped': 'Restriction skipped',
});

interface PolicyStatus {
  ratio: number | null;
  required: number;
  upload_needed: number;
  state: 'ok' | 'warned' | 'restricted' | 'banned';
  warned_until: number | null;
  exempt_reason: string | null;
  table: { min_downloaded_gib: number; required: number }[];
  seeding_reduction_applied: number;
}

const GIB = 1024 ** 3;
const fmtRatio = (r: number | null) => (r == null ? '∞' : r.toFixed(3));
const tone = (s: string): Tone => (s === 'ok' ? 'ok' : s === 'warned' ? 'warn' : 'bad');

function errText(api: PluginApi, e: unknown) {
  return e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e);
}

function Summary({ api, s }: { api: PluginApi; s: PolicyStatus }) {
  return (
    <div className="stack">
      <Kpis
        items={[
          { label: api.t('ratio-policy.ui.ratio'), value: fmtRatio(s.ratio), tone: s.ratio != null && s.ratio < s.required ? 'down' : 'up' },
          { label: api.t('ratio-policy.ui.required'), value: s.required.toFixed(3) },
          { label: api.t('ratio-policy.ui.status'), value: <Status tone={tone(s.state)}>{api.t(`ratio-policy.ui.state.${s.state}`)}</Status>, hint: s.upload_needed > 0 ? api.t('ratio-policy.ui.need', { n: (s.upload_needed / GIB).toFixed(2) }) : undefined },
        ]}
      />
      {s.warned_until && <Note tone="bad">{api.t('ratio-policy.ui.warned_until', { at: new Date(s.warned_until).toLocaleString() })}</Note>}
      {s.exempt_reason && <Note tone="info">{api.t(`ratio-policy.ui.exempt.${s.exempt_reason}`)}</Note>}
    </div>
  );
}

function ProfileStat({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const [s, setS] = useState<PolicyStatus | null>(null);
  const self = api.user()?.id === ctx.profile.id;
  useEffect(() => {
    if (self) void api.json<PolicyStatus>('/api/p/ratio-policy/v1/me').then(setS).catch(() => undefined);
  }, [api, self]);
  if (!s) return null;
  return (
    <Stat
      label={api.t('ratio-policy.ui.required')}
      to="/p/ratio-policy/"
      navigate={api.navigate}
      value={<span className={s.state === 'ok' ? undefined : 'down'}>{s.required.toFixed(3)}</span>}
      hint={s.state === 'ok' ? undefined : api.t(`ratio-policy.ui.state.${s.state}`)}
    />
  );
}

function MyPage({ api }: { api: PluginApi }) {
  const [s, setS] = useState<PolicyStatus | null>(null);
  useEffect(() => {
    void api.json<PolicyStatus>('/api/p/ratio-policy/v1/me').then(setS);
  }, [api]);
  if (!s) return <SkeletonRows n={5} />;
  return (
    <div className="stack">
      <PageHead title={api.t('ratio-policy.ui.title')} lede={s.seeding_reduction_applied > 0 ? api.t('ratio-policy.ui.reduction', { p: Math.round(s.seeding_reduction_applied * 100) }) : undefined} />
      <Summary api={api} s={s} />
      <Panel title={api.t('ratio-policy.ui.table')} flush primary>
        <Table
          rows={s.table}
          rowKey={(r) => r.min_downloaded_gib}
          selected={[...s.table].reverse().find((r) => r.required <= s.required + 1e-9)?.min_downloaded_gib ?? null}
          columns={[
            { key: 'd', header: api.t('ratio-policy.ui.downloaded'), render: (r) => <span className="num">{`${r.min_downloaded_gib} GiB`}</span> },
            { key: 'r', header: api.t('ratio-policy.ui.required'), right: true, render: (r) => <span className="num">{r.required.toFixed(2)}</span> },
          ]}
        />
      </Panel>
    </div>
  );
}

function UserTab({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const uid = ctx.profile.id;
  const [s, setS] = useState<PolicyStatus | null>(null);
  const [f, setF] = useState({ days: '7', reason: '' });
  const [op, setOp] = useState<'warn' | 'lift' | 'exempt' | null>(null);
  const load = () => api.json<PolicyStatus>(`/api/p/ratio-policy/v1/admin/users/${uid}`).then(setS);
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [uid]);
  const until = () => Date.now() + Number(f.days || '7') * 86_400_000;
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!op) return;
    const body = op === 'warn' ? { deadline: until(), reason: f.reason } : op === 'exempt' ? { until: until(), reason: f.reason } : { reason: f.reason };
    try {
      await api.json(`/api/p/ratio-policy/v1/admin/users/${uid}/${op}`, { json: body });
      api.toast(api.t('ratio-policy.ui.done'), 'success');
      setOp(null);
      await load();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  if (!s) return <SkeletonRows n={3} />;
  return (
    <div className="stack">
      <Summary api={api} s={s} />
      <div className="form-acts">
        <Button size="sm" variant="danger" onClick={() => setOp('warn')}>
          {api.t('ratio-policy.ui.warn')}
        </Button>
        {s.state !== 'ok' && (
          <Button size="sm" onClick={() => setOp('lift')}>
            {api.t('ratio-policy.ui.lift')}
          </Button>
        )}
        <Button size="sm" onClick={() => setOp('exempt')}>
          {api.t('ratio-policy.ui.exempt')}
        </Button>
      </div>
      <Sheet
        open={op !== null}
        onClose={() => setOp(null)}
        title={op ? api.t(`ratio-policy.ui.${op}`) : ''}
        footer={
          <Button type="submit" form="ratio-op" variant={op === 'warn' ? 'danger' : 'primary'}>
            {op ? api.t(`ratio-policy.ui.${op}`) : ''}
          </Button>
        }
      >
        <form id="ratio-op" className="form" onSubmit={(e) => void submit(e)}>
          {op !== 'lift' && (
            <Field label={api.t(op === 'exempt' ? 'ratio-policy.ui.exempt_days' : 'ratio-policy.ui.deadline_days')}>
              <Input name="ratio_days" type="number" min={1} value={f.days} onChange={(e) => setF({ ...f, days: e.target.value })} />
            </Field>
          )}
          <Field label={api.t('ratio-policy.ui.reason')}>
            <Input name="ratio_reason" maxLength={500} value={f.reason} onChange={(e) => setF({ ...f, reason: e.target.value })} />
          </Field>
        </form>
      </Sheet>
    </div>
  );
}

interface Warned {
  user_id: number;
  username: string;
  state: string;
  ratio: number | null;
  required: number | null;
  deadline: number | null;
  manual: boolean;
}

const LIST_STATES = ['warned', 'restricted', 'banned', 'restricted_skipped'];

function AdminList({ api }: { api: PluginApi }) {
  const [state, setState] = useState('warned');
  const [rows, setRows] = useState<Warned[] | null>(null);
  useEffect(() => {
    setRows(null);
    void api.json<{ items: Warned[] }>(`/api/p/ratio-policy/v1/admin/warned?state=${state}`).then((r) => setRows(r.items));
  }, [api, state]);
  return (
    <div className="stack">
      <PageHead title={api.t('ratio-policy.ui.list')} />
      <Panel flush primary>
        <Tabs variant="flush" value={state} onChange={setState} tabs={LIST_STATES.map((s) => ({ id: s, label: api.t(`ratio-policy.ui.state.${s}`) === `ratio-policy.ui.state.${s}` ? s : api.t(`ratio-policy.ui.state.${s}`) }))} />
        {rows === null ? (
          <SkeletonRows n={5} />
        ) : (
          <Table
            rows={rows}
            rowKey={(r) => r.user_id}
            empty="—"
            onRowClick={(r) => api.navigate(`/admin/users/${r.user_id}`)}
            columns={[
              {
                key: 'u',
                header: api.t('ratio-policy.ui.user'),
                render: (r) => (
                  <PLink to={`/admin/users/${r.user_id}`} navigate={api.navigate}>
                    {r.username}
                  </PLink>
                ),
              },
              { key: 'r', header: api.t('ratio-policy.ui.ratio'), right: true, render: (r) => <span className="num down">{fmtRatio(r.ratio)}</span> },
              { key: 'q', header: api.t('ratio-policy.ui.required'), right: true, render: (r) => <span className="num">{r.required?.toFixed(3) ?? '—'}</span> },
              { key: 'd', header: api.t('ratio-policy.ui.deadline'), render: (r) => <span className="small muted num nowrap">{r.deadline ? new Date(r.deadline).toLocaleString() : ''}</span> },
              { key: 'm', header: '', render: (r) => (r.manual ? <Tag>{api.t('ratio-policy.ui.manual')}</Tag> : null) },
            ]}
          />
        )}
      </Panel>
    </div>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  return path.startsWith('/admin/') ? <AdminList api={api} /> : <MyPage api={api} />;
}

export default {
  slots: {
    'user.profile.stats': { component: ProfileStat, order: 45 },
    'admin.user.tabs': { component: UserTab, order: 40, title_term: 'ratio-policy.name' },
  },
  routes: [
    { path: '/p/ratio-policy/*', component: Pages },
    { path: '/admin/p/ratio-policy/*', component: Pages },
  ],
} satisfies PluginFrontend;
