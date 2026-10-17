// ratio-policy 前端：个人页分享率（含要求与警告）、“分享率考核”页（我的状态、要求表）、后台用户标签页（警告 / 解除 / 豁免）与警告列表。
import { useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Field, Input, Stat, Table } from '@pt-studio/ui-kit';

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
});

interface Status {
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
const tone = (s: Status['state']) => (s === 'ok' ? 'muted' : 'danger');

function errText(api: PluginApi, e: unknown) {
  return e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e);
}

function Summary({ api, s }: { api: PluginApi; s: Status }) {
  return (
    <>
      <p>
        {api.t('ratio-policy.ui.ratio')} <strong>{fmtRatio(s.ratio)}</strong> · {api.t('ratio-policy.ui.required')} {s.required.toFixed(3)}{' '}
        <Badge tone={tone(s.state)}>{api.t(`ratio-policy.ui.state.${s.state}`)}</Badge>
      </p>
      {s.exempt_reason && <p className="pt-muted small">{api.t(`ratio-policy.ui.exempt.${s.exempt_reason}`)}</p>}
      {s.upload_needed > 0 && <p>{api.t('ratio-policy.ui.need', { n: (s.upload_needed / GIB).toFixed(2) })}</p>}
      {s.warned_until && <p className="pt-field-error">{api.t('ratio-policy.ui.warned_until', { at: new Date(s.warned_until).toLocaleString() })}</p>}
    </>
  );
}

function ProfileStat({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const [s, setS] = useState<Status | null>(null);
  const self = api.user()?.id === ctx.profile.id;
  useEffect(() => {
    if (self) void api.json<Status>('/api/p/ratio-policy/v1/me').then(setS).catch(() => undefined);
  }, [api, self]);
  if (!s) return null;
  return (
    <Stat
      label={api.t('ratio-policy.ui.required')}
      value={<span className={s.state === 'ok' ? undefined : 'pt-field-error'}>{s.required.toFixed(3)}</span>}
      hint={s.state === 'ok' ? undefined : api.t(`ratio-policy.ui.state.${s.state}`)}
    />
  );
}

function MyPage({ api }: { api: PluginApi }) {
  const [s, setS] = useState<Status | null>(null);
  useEffect(() => {
    void api.json<Status>('/api/p/ratio-policy/v1/me').then(setS);
  }, [api]);
  if (!s) return null;
  return (
    <Card title={api.t('ratio-policy.ui.title')}>
      <Summary api={api} s={s} />
      <h3>{api.t('ratio-policy.ui.table')}</h3>
      <Table
        rows={s.table}
        rowKey={(r) => r.min_downloaded_gib}
        columns={[
          { key: 'd', header: api.t('ratio-policy.ui.downloaded'), render: (r) => `${r.min_downloaded_gib} GiB` },
          { key: 'r', header: api.t('ratio-policy.ui.required'), render: (r) => r.required.toFixed(2) },
        ]}
      />
      {s.seeding_reduction_applied > 0 && (
        <p className="pt-muted small">{api.t('ratio-policy.ui.reduction', { p: Math.round(s.seeding_reduction_applied * 100) })}</p>
      )}
    </Card>
  );
}

function UserTab({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const uid = ctx.profile.id;
  const [s, setS] = useState<Status | null>(null);
  const [f, setF] = useState({ days: '7', reason: '' });
  const load = () => api.json<Status>(`/api/p/ratio-policy/v1/admin/users/${uid}`).then(setS);
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [uid]);
  const act = async (op: string, body: Record<string, unknown>) => {
    try {
      await api.json(`/api/p/ratio-policy/v1/admin/users/${uid}/${op}`, { json: body });
      api.toast(api.t('ratio-policy.ui.done'), 'success');
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const until = () => Date.now() + Number(f.days || '7') * 86_400_000;
  const warn = (e: FormEvent) => {
    e.preventDefault();
    void act('warn', { deadline: until(), reason: f.reason });
  };
  if (!s) return null;
  return (
    <Card title={api.t('ratio-policy.ui.title')}>
      <Summary api={api} s={s} />
      <form onSubmit={warn}>
        <Field label={api.t('ratio-policy.ui.deadline_days')}>
          <Input name="ratio_days" type="number" min={1} value={f.days} onChange={(e) => setF({ ...f, days: e.target.value })} />
        </Field>
        <Field label={api.t('ratio-policy.ui.reason')}>
          <Input name="ratio_reason" maxLength={500} value={f.reason} onChange={(e) => setF({ ...f, reason: e.target.value })} />
        </Field>
        <Button type="submit" variant="danger">
          {api.t('ratio-policy.ui.warn')}
        </Button>{' '}
        <Button type="button" onClick={() => void act('lift', { reason: f.reason })}>
          {api.t('ratio-policy.ui.lift')}
        </Button>{' '}
        <Button type="button" onClick={() => void act('exempt', { until: until(), reason: f.reason })}>
          {api.t('ratio-policy.ui.exempt')}
        </Button>
      </form>
    </Card>
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

function AdminList({ api }: { api: PluginApi }) {
  const [state, setState] = useState('warned');
  const [rows, setRows] = useState<Warned[]>([]);
  useEffect(() => {
    void api.json<{ items: Warned[] }>(`/api/p/ratio-policy/v1/admin/warned?state=${state}`).then((r) => setRows(r.items));
  }, [api, state]);
  return (
    <Card title={api.t('ratio-policy.ui.list')}>
      <select className="pt-input" value={state} onChange={(e) => setState(e.target.value)}>
        {['warned', 'restricted', 'banned', 'restricted_skipped'].map((s) => (
          <option key={s} value={s}>
            {s}
          </option>
        ))}
      </select>
      <Table
        rows={rows}
        rowKey={(r) => r.user_id}
        empty="—"
        columns={[
          { key: 'u', header: '', render: (r) => <a href={`/admin/users/${r.user_id}`}>{r.username}</a> },
          { key: 'r', header: api.t('ratio-policy.ui.ratio'), render: (r) => fmtRatio(r.ratio) },
          { key: 'q', header: api.t('ratio-policy.ui.required'), render: (r) => r.required?.toFixed(3) ?? '—' },
          { key: 'd', header: '', render: (r) => (r.deadline ? new Date(r.deadline).toLocaleString() : '') },
          { key: 'm', header: '', render: (r) => (r.manual ? <Badge>manual</Badge> : null) },
        ]}
      />
    </Card>
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
