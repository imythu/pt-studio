// accounting 前端：后台用户详情页“流量”标签 —— 当前上传 / 下载 / 分享率、手动调整（GiB，可为负，需填写原因）与该用户的异常信号；
// 后台“流量异常信号”页（/admin/p/accounting/：按类型筛选、近 30 天，accounting.signals.read）。
import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { formatBytes, registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Field, Input, Stat, Table, Tabs } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'accounting.ui.tab': '流量',
  'accounting.ui.uploaded': '上传量',
  'accounting.ui.downloaded': '下载量',
  'accounting.ui.ratio': '分享率',
  'accounting.ui.d_up': '上传量调整（GiB，可为负）',
  'accounting.ui.d_down': '下载量调整（GiB，可为负）',
  'accounting.ui.note': '原因（必填，写入审计）',
  'accounting.ui.apply': '调整流量',
  'accounting.ui.done': '已调整',
  'accounting.ui.signals': '流量异常信号',
  'accounting.ui.signals_hint': '近 30 天 tracker 汇报中的可疑情况（同一用户、种子、类型的重复命中合并为一条）。信号只是线索，需要结合反作弊案件判断。',
  'accounting.ui.all': '全部',
  'accounting.ui.user': '用户',
  'accounting.ui.torrent': '种子',
  'accounting.ui.kind': '类型',
  'accounting.ui.hits': '次数',
  'accounting.ui.when': '时间',
  'accounting.ui.more': '加载更多',
  'accounting.ui.signal.counter_jump': '计数跳变',
  'accounting.ui.signal.negative_delta': '计数回退',
  'accounting.ui.signal.peer_not_in_swarm': '不在种群中却汇报流量',
  'accounting.ui.signal.speed_exceeded': '速度超出上限',
  'accounting.ui.severity.low': '低',
  'accounting.ui.severity.medium': '中',
  'accounting.ui.severity.high': '高',
});
registerMessages('en', {
  'accounting.ui.tab': 'Traffic',
  'accounting.ui.uploaded': 'Uploaded',
  'accounting.ui.downloaded': 'Downloaded',
  'accounting.ui.ratio': 'Ratio',
  'accounting.ui.d_up': 'Upload change (GiB, may be negative)',
  'accounting.ui.d_down': 'Download change (GiB, may be negative)',
  'accounting.ui.note': 'Reason (required, audited)',
  'accounting.ui.apply': 'Adjust traffic',
  'accounting.ui.done': 'Adjusted',
  'accounting.ui.signals': 'Traffic signals',
  'accounting.ui.signals_hint': 'Suspicious tracker reports in the last 30 days (repeats for the same user, torrent and kind are merged). Signals are leads, not verdicts — review them with anticheat cases.',
  'accounting.ui.all': 'All',
  'accounting.ui.user': 'User',
  'accounting.ui.torrent': 'Torrent',
  'accounting.ui.kind': 'Kind',
  'accounting.ui.hits': 'Hits',
  'accounting.ui.when': 'When',
  'accounting.ui.more': 'Load more',
  'accounting.ui.signal.counter_jump': 'Counter jump',
  'accounting.ui.signal.negative_delta': 'Counter went backwards',
  'accounting.ui.signal.peer_not_in_swarm': 'Traffic without a swarm',
  'accounting.ui.signal.speed_exceeded': 'Speed above limit',
  'accounting.ui.severity.low': 'low',
  'accounting.ui.severity.medium': 'medium',
  'accounting.ui.severity.high': 'high',
});

interface Stats {
  uploaded: number;
  downloaded: number;
}

const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e);
const GIB = 1024 ** 3;

function UserTab({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const uid = ctx.profile.id;
  const [s, setS] = useState<Stats | null>(null);
  const [f, setF] = useState({ up: '', down: '', note: '' });
  const load = () => void api.json<Stats>(`/api/p/accounting/v1/users/${uid}/stats`).then(setS);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, [uid]);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await api.json(`/api/p/accounting/v1/admin/users/${uid}/adjustments`, {
        json: { d_up: Math.round(Number(f.up || 0) * GIB), d_down: Math.round(Number(f.down || 0) * GIB), note: f.note },
        idempotent: true,
      });
      api.toast(api.t('accounting.ui.done'), 'success');
      setF({ up: '', down: '', note: '' });
      load();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  if (!s) return null;
  const ratio = s.downloaded > 0 ? (s.uploaded / s.downloaded).toFixed(3) : '∞';
  return (
    <Card>
      <div className="pt-row stats-row">
        <Stat label={api.t('accounting.ui.uploaded')} value={formatBytes(s.uploaded)} />
        <Stat label={api.t('accounting.ui.downloaded')} value={formatBytes(s.downloaded)} />
        <Stat label={api.t('accounting.ui.ratio')} value={ratio} />
      </div>
      <form onSubmit={(e) => void submit(e)} className="upload-grid">
        <Field label={api.t('accounting.ui.d_up')}>
          <Input name="traffic_up" type="number" step="any" value={f.up} onChange={(e) => setF({ ...f, up: e.target.value })} />
        </Field>
        <Field label={api.t('accounting.ui.d_down')}>
          <Input name="traffic_down" type="number" step="any" value={f.down} onChange={(e) => setF({ ...f, down: e.target.value })} />
        </Field>
        <div className="span2">
          <Field label={api.t('accounting.ui.note')}>
            <Input name="traffic_note" required maxLength={500} value={f.note} onChange={(e) => setF({ ...f, note: e.target.value })} />
          </Field>
          <Button type="submit" variant="danger" disabled={!f.note.trim() || (!Number(f.up) && !Number(f.down))}>
            {api.t('accounting.ui.apply')}
          </Button>
        </div>
      </form>
      <h3>{api.t('accounting.ui.signals')}</h3>
      <Signals api={api} uid={uid} />
    </Card>
  );
}

interface Signal {
  id: number;
  user_id: number;
  torrent_id: number | null;
  kind: string;
  severity: string;
  hits: number;
  details: Record<string, unknown> | null;
  first_at: number;
  last_at: number;
}

const KINDS = ['counter_jump', 'negative_delta', 'peer_not_in_swarm', 'speed_exceeded'];
const SEV: Record<string, 'muted' | 'primary' | 'danger'> = { low: 'muted', medium: 'primary', high: 'danger' };

function A({ api, to, children }: { api: PluginApi; to: string; children: string }) {
  return (
    <a
      href={to}
      onClick={(e) => {
        e.preventDefault();
        api.navigate(to);
      }}
    >
      {children}
    </a>
  );
}

/** 信号列表（近 30 天）；`uid` 为空时列出全站 */
function Signals({ api, uid }: { api: PluginApi; uid: number | null }) {
  const [kind, setKind] = useState('');
  const [rows, setRows] = useState<Signal[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const load = useCallback(
    async (cursor: string | null) => {
      const q = new URLSearchParams({ limit: '50' });
      if (uid !== null) q.set('user_id', String(uid));
      if (kind) q.set('signal', kind);
      if (cursor) q.set('cursor', cursor);
      const r = await api.json<{ items: Signal[]; next_cursor: string | null }>(`/api/p/accounting/v1/admin/signals?${q.toString()}`);
      setRows((old) => (cursor ? [...old, ...r.items] : r.items));
      setNext(r.next_cursor);
    },
    [api, uid, kind],
  );
  useEffect(() => {
    void load(null).catch(() => undefined);
  }, [load]);
  const label = (k: string) => {
    const key = `accounting.ui.signal.${k}`;
    return api.t(key) === key ? k : api.t(key);
  };
  const reason = (d: Signal['details']) =>
    d
      ? Object.entries(d)
          .filter(([, v]) => v !== null && typeof v !== 'object')
          .map(([k, v]) => `${k}=${String(v)}`)
          .join(' · ')
      : '';
  return (
    <>
      <Tabs value={kind} onChange={setKind} tabs={[{ id: '', label: api.t('accounting.ui.all') }, ...KINDS.map((k) => ({ id: k, label: label(k) }))]} />
      <Table
        rows={rows}
        rowKey={(r) => r.id}
        empty="—"
        columns={[
          ...(uid === null ? [{ key: 'u', header: api.t('accounting.ui.user'), render: (r: Signal) => <A api={api} to={`/admin/users/${r.user_id}`}>{`#${r.user_id}`}</A> }] : []),
          { key: 't', header: api.t('accounting.ui.torrent'), render: (r) => (r.torrent_id === null ? '—' : <A api={api} to={`/torrents/${r.torrent_id}`}>{`#${r.torrent_id}`}</A>) },
          {
            key: 'k',
            header: api.t('accounting.ui.kind'),
            render: (r) => (
              <>
                <Badge tone={SEV[r.severity] ?? 'muted'}>{api.t(`accounting.ui.severity.${r.severity}`)}</Badge> {label(r.kind)}
                {reason(r.details) && <div className="pt-muted small pt-prewrap">{reason(r.details)}</div>}
              </>
            ),
          },
          { key: 'h', header: api.t('accounting.ui.hits'), render: (r) => r.hits },
          {
            key: 'w',
            header: api.t('accounting.ui.when'),
            render: (r) => <span className="pt-muted small">{`${new Date(r.first_at).toLocaleString()} → ${new Date(r.last_at).toLocaleString()}`}</span>,
          },
        ]}
      />
      {next && <Button onClick={() => void load(next)}>{api.t('accounting.ui.more')}</Button>}
    </>
  );
}

function SignalsPage({ api }: { api: PluginApi }) {
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t('accounting.ui.signals')}</h1>
      <Card>
        <p className="pt-muted small">{api.t('accounting.ui.signals_hint')}</p>
        <Signals api={api} uid={null} />
      </Card>
    </div>
  );
}

export default {
  slots: {
    'admin.user.tabs': { component: UserTab, order: 10, title_term: 'accounting.ui.tab' },
  },
  routes: [{ path: '/admin/p/accounting/*', component: SignalsPage }],
} satisfies PluginFrontend;
