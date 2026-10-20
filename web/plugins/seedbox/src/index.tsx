// seedbox 前端：设置页“盒子”标签（登记 / 删除 IP 段、审核状态），后台审核页。
import { useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Field, Input, PageHead, Panel, Sheet, SkeletonRows, Status, Table, Tabs, Textarea, type Tone } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'seedbox.ui.title': '盒子',
  'seedbox.ui.hint': '在盒子（seedbox / VPS）上做种时，登记它的 IP 段：来自这些地址的做种会标记为盒子，站点规则（如 H&R、魔力值）可能据此区别对待。IPv4 至少 /24、IPv6 至少 /48，每人最多 {n} 个。',
  'seedbox.ui.ranges': 'IP 段（每行一个，如 203.0.113.0/24）',
  'seedbox.ui.provider': '服务商',
  'seedbox.ui.note': '备注',
  'seedbox.ui.add': '登记',
  'seedbox.ui.state.pending': '待审核',
  'seedbox.ui.state.approved': '已批准',
  'seedbox.ui.state.rejected': '已拒绝',
  'seedbox.ui.state.expired': '已过期',
  'seedbox.ui.delete': '删除',
  'seedbox.ui.review': '盒子登记审核',
  'seedbox.ui.approve': '批准',
  'seedbox.ui.reject': '拒绝',
  'seedbox.ui.overlap': '与其他用户重叠：{users}',
  'seedbox.range_invalid': 'IP 段无效',
  'seedbox.too_many_ranges': '登记的 IP 段过多',
  'seedbox.not_pending': '这条登记不在待审核状态',
  'seedbox.ui.cancel': '取消',
  'seedbox.ui.status': '状态',
  'seedbox.ui.user': '用户',
  'seedbox.ui.time': '登记时间',
});
registerMessages('en', {
  'seedbox.ui.title': 'Seedboxes',
  'seedbox.ui.hint': 'If you seed from a seedbox or VPS, register its IP ranges: seeding from them is marked as seedbox, and site rules (H&R, bonus) may treat it differently. IPv4 /24 or narrower, IPv6 /48 or narrower, up to {n} ranges.',
  'seedbox.ui.ranges': 'IP ranges (one per line, e.g. 203.0.113.0/24)',
  'seedbox.ui.provider': 'Provider',
  'seedbox.ui.note': 'Note',
  'seedbox.ui.add': 'Register',
  'seedbox.ui.state.pending': 'Pending',
  'seedbox.ui.state.approved': 'Approved',
  'seedbox.ui.state.rejected': 'Rejected',
  'seedbox.ui.state.expired': 'Expired',
  'seedbox.ui.delete': 'Delete',
  'seedbox.ui.review': 'Seedbox reviews',
  'seedbox.ui.approve': 'Approve',
  'seedbox.ui.reject': 'Reject',
  'seedbox.ui.overlap': 'Overlaps with: {users}',
  'seedbox.range_invalid': 'Invalid IP range',
  'seedbox.too_many_ranges': 'Too many IP ranges',
  'seedbox.not_pending': 'This registration is not pending',
  'seedbox.ui.cancel': 'Cancel',
  'seedbox.ui.status': 'Status',
  'seedbox.ui.user': 'User',
  'seedbox.ui.time': 'Registered',
});

interface Box {
  id: number;
  username: string | null;
  provider: string | null;
  note: string | null;
  state: 'pending' | 'approved' | 'rejected' | 'expired';
  review_reason: string | null;
  overlap_note: string | null;
  ranges: string[];
  created_at: number;
}

const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code ? `${api.t(e.code)}${e.message ? `：${e.message}` : ''}` : e instanceof Error ? e.message : String(e);
const tone = (s: Box['state']): Tone => (s === 'approved' ? 'ok' : s === 'pending' ? 'warn' : s === 'rejected' ? 'bad' : 'mute');
const STATES = ['pending', 'approved', 'rejected', 'expired'] as const;

function Ranges({ b }: { b: Box }) {
  return (
    <div className="row">
      {b.ranges.map((r) => (
        <span key={r} className="pt-mono small">
          {r}
        </span>
      ))}
    </div>
  );
}

/** 设置页“盒子”标签：已登记列表；登记在抽屉里填写 */
function MyBoxes({ api }: { api: PluginApi }) {
  const [data, setData] = useState<{ items: Box[]; max_ranges: number } | null>(null);
  const [f, setF] = useState({ ranges: '', provider: '', note: '' });
  const [open, setOpen] = useState(false);
  const [deleting, setDeleting] = useState<Box | null>(null);
  const load = () => api.json<{ items: Box[]; max_ranges: number }>('/api/p/seedbox/v1/boxes').then(setData);
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const add = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await api.json('/api/p/seedbox/v1/boxes', {
        json: { ranges: f.ranges.split(/[\s,]+/).filter(Boolean), provider: f.provider || null, note: f.note || null },
      });
      setF({ ranges: '', provider: '', note: '' });
      setOpen(false);
      await load();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  const del = async () => {
    if (!deleting) return;
    try {
      await api.fetch(`/api/p/seedbox/v1/boxes/${deleting.id}`, { method: 'DELETE' });
      setDeleting(null);
      await load();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  if (!data) return <SkeletonRows n={3} />;
  return (
    <div className="stack">
      <p className="small muted">{api.t('seedbox.ui.hint', { n: data.max_ranges })}</p>
      <Panel
        title={api.t('seedbox.ui.title')}
        flush
        actions={
          <Button size="sm" variant="primary" onClick={() => setOpen(true)}>
            {api.t('seedbox.ui.add')}
          </Button>
        }
      >
        <Table
          rows={data.items}
          rowKey={(b) => b.id}
          empty="—"
          columns={[
            { key: 'r', header: 'IP', render: (b) => <Ranges b={b} /> },
            { key: 'p', header: api.t('seedbox.ui.provider'), render: (b) => <span className="small">{[b.provider, b.note].filter(Boolean).join(' · ')}</span> },
            {
              key: 's',
              header: api.t('seedbox.ui.status'),
              render: (b) => (
                <>
                  <Status tone={tone(b.state)}>{api.t(`seedbox.ui.state.${b.state}`)}</Status>
                  {b.review_reason && <div className="tiny muted">{b.review_reason}</div>}
                </>
              ),
            },
            {
              key: 'a',
              header: '',
              right: true,
              render: (b) => (
                <Button size="xs" variant="ghost" onClick={() => setDeleting(b)}>
                  {api.t('seedbox.ui.delete')}
                </Button>
              ),
            },
          ]}
        />
      </Panel>
      <Sheet
        open={open}
        onClose={() => setOpen(false)}
        title={api.t('seedbox.ui.add')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setOpen(false)}>
              {api.t('seedbox.ui.cancel')}
            </Button>
            <Button type="submit" form="seedbox-add" variant="primary">
              {api.t('seedbox.ui.add')}
            </Button>
          </>
        }
      >
        <form id="seedbox-add" className="form" onSubmit={(e) => void add(e)}>
          <Field label={api.t('seedbox.ui.ranges')}>
            <Textarea name="ranges" className="ta pt-mono" rows={4} value={f.ranges} onChange={(e) => setF({ ...f, ranges: e.target.value })} required />
          </Field>
          <Field label={api.t('seedbox.ui.provider')}>
            <Input name="provider" maxLength={64} value={f.provider} onChange={(e) => setF({ ...f, provider: e.target.value })} />
          </Field>
          <Field label={api.t('seedbox.ui.note')}>
            <Input name="note" maxLength={200} value={f.note} onChange={(e) => setF({ ...f, note: e.target.value })} />
          </Field>
        </form>
      </Sheet>
      <Sheet
        open={deleting !== null}
        onClose={() => setDeleting(null)}
        title={api.t('seedbox.ui.delete')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDeleting(null)}>
              {api.t('seedbox.ui.cancel')}
            </Button>
            <Button variant="danger" onClick={() => void del()}>
              {api.t('seedbox.ui.delete')}
            </Button>
          </>
        }
      >
        {deleting && <Ranges b={deleting} />}
      </Sheet>
    </div>
  );
}

function Review({ api }: { api: PluginApi }) {
  const [state, setState] = useState('pending');
  const [rows, setRows] = useState<Box[] | null>(null);
  const load = () => api.json<{ items: Box[] }>(`/api/p/seedbox/v1/admin/boxes?state=${state}`).then((r) => setRows(r.items));
  useEffect(() => {
    setRows(null);
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [state]);
  const review = async (b: Box, approve: boolean) => {
    try {
      await api.json(`/api/p/seedbox/v1/admin/boxes/${b.id}/review`, { json: { approve } });
      await load();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  return (
    <div className="stack">
      <PageHead title={api.t('seedbox.ui.review')} />
      <Panel flush primary>
        <Tabs variant="flush" value={state} onChange={setState} tabs={STATES.map((s) => ({ id: s, label: api.t(`seedbox.ui.state.${s}`) }))} />
        {rows === null ? (
          <SkeletonRows n={4} />
        ) : (
          <Table
            rows={rows}
            rowKey={(b) => b.id}
            empty="—"
            minWidth={640}
            columns={[
              { key: 'u', header: api.t('seedbox.ui.user'), render: (b) => b.username ?? '—' },
              {
                key: 'r',
                header: 'IP',
                render: (b) => (
                  <>
                    <Ranges b={b} />
                    {b.overlap_note && <div className="small down">{api.t('seedbox.ui.overlap', { users: b.overlap_note })}</div>}
                  </>
                ),
              },
              { key: 'p', header: api.t('seedbox.ui.provider'), render: (b) => <span className="small">{[b.provider, b.note].filter(Boolean).join(' · ')}</span> },
              { key: 't', header: api.t('seedbox.ui.time'), render: (b) => <span className="small muted num nowrap">{new Date(b.created_at).toLocaleDateString()}</span> },
              {
                key: 'a',
                header: '',
                right: true,
                render: (b) =>
                  b.state === 'pending' ? (
                    <div className="form-acts">
                      <Button size="xs" variant="primary" onClick={() => void review(b, true)}>
                        {api.t('seedbox.ui.approve')}
                      </Button>
                      <Button size="xs" variant="danger" onClick={() => void review(b, false)}>
                        {api.t('seedbox.ui.reject')}
                      </Button>
                    </div>
                  ) : (
                    <Status tone={tone(b.state)}>{api.t(`seedbox.ui.state.${b.state}`)}</Status>
                  ),
              },
            ]}
          />
        )}
      </Panel>
    </div>
  );
}

export default {
  slots: {
    'user.settings.tabs': { component: ({ api }: { api: PluginApi }) => <MyBoxes api={api} />, order: 60, title_term: 'seedbox.name' },
  },
  routes: [{ path: '/admin/p/seedbox/*', component: Review }],
} satisfies PluginFrontend;
