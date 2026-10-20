// accounting 前端：后台用户详情页“流量”标签 —— 当前上传 / 下载 / 分享率、手动调整（GiB，可为负，需填写原因，抽屉填写）与该用户的异常信号；
// 后台“流量异常信号”页（/admin/p/accounting/：按类型筛选、近 30 天，accounting.signals.read）。
import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { formatBytes, registerMessages } from '@pt-studio/terms';
import { Button, Field, Input, KeyFacts, LoadMore, PageHead, Panel, PLink, Sheet, SkeletonRows, Status, Table, Tabs, type Tone } from '@pt-studio/ui-kit';

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
  'accounting.ui.cancel': '取消',
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
  'accounting.ui.cancel': 'Cancel',
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
  const [open, setOpen] = useState(false);
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
      setOpen(false);
      load();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  if (!s) return <SkeletonRows n={3} />;
  const ratio = s.downloaded > 0 ? (s.uploaded / s.downloaded).toFixed(3) : '∞';
  return (
    <div className="stack">
      <div className="row-sb">
        <KeyFacts
          items={[
            { label: api.t('accounting.ui.uploaded'), value: formatBytes(s.uploaded), tone: 'up' },
            { label: api.t('accounting.ui.downloaded'), value: formatBytes(s.downloaded), tone: 'down' },
            { label: api.t('accounting.ui.ratio'), value: ratio },
          ]}
        />
        <Button size="sm" variant="primary" onClick={() => setOpen(true)}>
          {api.t('accounting.ui.apply')}
        </Button>
      </div>
      <Panel title={api.t('accounting.ui.signals')} flush>
        <Signals api={api} uid={uid} />
      </Panel>
      <Sheet
        open={open}
        onClose={() => setOpen(false)}
        title={api.t('accounting.ui.apply')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setOpen(false)}>
              {api.t('accounting.ui.cancel')}
            </Button>
            <Button type="submit" form="traffic-adjust" variant="danger" disabled={!f.note.trim() || (!Number(f.up) && !Number(f.down))}>
              {api.t('accounting.ui.apply')}
            </Button>
          </>
        }
      >
        <form id="traffic-adjust" className="form" onSubmit={(e) => void submit(e)}>
          <div className="form-2">
            <Field label={api.t('accounting.ui.d_up')}>
              <Input name="traffic_up" type="number" step="any" value={f.up} onChange={(e) => setF({ ...f, up: e.target.value })} />
            </Field>
            <Field label={api.t('accounting.ui.d_down')}>
              <Input name="traffic_down" type="number" step="any" value={f.down} onChange={(e) => setF({ ...f, down: e.target.value })} />
            </Field>
          </div>
          <Field label={api.t('accounting.ui.note')}>
            <Input name="traffic_note" required maxLength={500} value={f.note} onChange={(e) => setF({ ...f, note: e.target.value })} />
          </Field>
        </form>
      </Sheet>
    </div>
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
const SEV: Record<string, Tone> = { low: 'mute', medium: 'warn', high: 'bad' };
const dt = (ms: number) => new Date(ms).toLocaleString(undefined, { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });

/** 信号列表（近 30 天）；`uid` 为空时列出全站 */
function Signals({ api, uid }: { api: PluginApi; uid: number | null }) {
  const [kind, setKind] = useState('');
  const [rows, setRows] = useState<Signal[] | null>(null);
  const [next, setNext] = useState<string | null>(null);
  const load = useCallback(
    async (cursor: string | null) => {
      const q = new URLSearchParams({ limit: '50' });
      if (uid !== null) q.set('user_id', String(uid));
      if (kind) q.set('signal', kind);
      if (cursor) q.set('cursor', cursor);
      const r = await api.json<{ items: Signal[]; next_cursor: string | null }>(`/api/p/accounting/v1/admin/signals?${q.toString()}`);
      setRows((old) => (cursor ? [...(old ?? []), ...r.items] : r.items));
      setNext(r.next_cursor);
    },
    [api, uid, kind],
  );
  useEffect(() => {
    void load(null).catch(() => setRows([]));
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
      <Tabs variant="flush" value={kind} onChange={setKind} tabs={[{ id: '', label: api.t('accounting.ui.all') }, ...KINDS.map((k) => ({ id: k, label: label(k) }))]} />
      {rows === null ? (
        <SkeletonRows n={4} />
      ) : (
        <Table
          rows={rows}
          rowKey={(r) => r.id}
          empty="—"
          minWidth={820}
          columns={[
            ...(uid === null
              ? [
                  {
                    key: 'u',
                    header: api.t('accounting.ui.user'),
                    render: (r: Signal) => (
                      <PLink to={`/admin/users/${r.user_id}`} navigate={api.navigate}>
                        {`#${r.user_id}`}
                      </PLink>
                    ),
                  },
                ]
              : []),
            {
              key: 't',
              header: api.t('accounting.ui.torrent'),
              render: (r) =>
                r.torrent_id === null ? (
                  '—'
                ) : (
                  <PLink to={`/torrents/${r.torrent_id}`} navigate={api.navigate}>
                    {`#${r.torrent_id}`}
                  </PLink>
                ),
            },
            {
              key: 'k',
              header: api.t('accounting.ui.kind'),
              render: (r) => (
                <>
                  <Status tone={SEV[r.severity] ?? 'mute'}>{api.t(`accounting.ui.severity.${r.severity}`)}</Status> {label(r.kind)}
                  {reason(r.details) && <div className="tiny muted pt-mono">{reason(r.details)}</div>}
                </>
              ),
            },
            { key: 'h', header: api.t('accounting.ui.hits'), right: true, render: (r) => <span className="num">{r.hits}</span> },
            { key: 'w', header: api.t('accounting.ui.when'), render: (r) => <span className="small muted num nowrap">{`${dt(r.first_at)} → ${dt(r.last_at)}`}</span> },
          ]}
        />
      )}
      {next && <LoadMore onClick={() => void load(next)} label={api.t('accounting.ui.more')} />}
    </>
  );
}

function SignalsPage({ api }: { api: PluginApi }) {
  return (
    <div className="stack">
      <PageHead title={api.t('accounting.ui.signals')} lede={api.t('accounting.ui.signals_hint')} />
      <Panel flush primary>
        <Signals api={api} uid={null} />
      </Panel>
    </div>
  );
}

export default {
  slots: {
    'admin.user.tabs': { component: UserTab, order: 10, title_term: 'accounting.ui.tab' },
  },
  routes: [{ path: '/admin/p/accounting/*', component: SignalsPage }],
} satisfies PluginFrontend;
