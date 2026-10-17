// seedbox 前端：设置页“盒子”标签（登记 / 删除 IP 段、审核状态），后台审核页。
import { useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Field, Input, Table } from '@pt-studio/ui-kit';

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
const tone = (s: Box['state']) => (s === 'approved' ? 'success' : s === 'pending' ? 'primary' : 'muted');

function MyBoxes({ api }: { api: PluginApi }) {
  const [data, setData] = useState<{ items: Box[]; max_ranges: number } | null>(null);
  const [f, setF] = useState({ ranges: '', provider: '', note: '' });
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
      await load();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  const del = async (b: Box) => {
    await api.fetch(`/api/p/seedbox/v1/boxes/${b.id}`, { method: 'DELETE' });
    await load();
  };
  if (!data) return null;
  return (
    <Card title={api.t('seedbox.ui.title')}>
      <p className="pt-muted small">{api.t('seedbox.ui.hint', { n: data.max_ranges })}</p>
      <Table
        rows={data.items}
        rowKey={(b) => b.id}
        empty="—"
        columns={[
          { key: 'r', header: 'IP', render: (b) => <code className="pt-mono">{b.ranges.join(', ')}</code> },
          { key: 'p', header: api.t('seedbox.ui.provider'), render: (b) => b.provider ?? '' },
          { key: 's', header: '', render: (b) => <Badge tone={tone(b.state)}>{api.t(`seedbox.ui.state.${b.state}`)}</Badge> },
          { key: 'a', header: '', render: (b) => <Button variant="ghost" onClick={() => void del(b)}>{api.t('seedbox.ui.delete')}</Button> },
        ]}
      />
      <form onSubmit={add}>
        <Field label={api.t('seedbox.ui.ranges')}>
          <textarea className="pt-input" name="ranges" rows={2} value={f.ranges} onChange={(e) => setF({ ...f, ranges: e.target.value })} required />
        </Field>
        <Field label={api.t('seedbox.ui.provider')}>
          <Input name="provider" maxLength={64} value={f.provider} onChange={(e) => setF({ ...f, provider: e.target.value })} />
        </Field>
        <Field label={api.t('seedbox.ui.note')}>
          <Input name="note" maxLength={200} value={f.note} onChange={(e) => setF({ ...f, note: e.target.value })} />
        </Field>
        <Button type="submit" variant="primary">
          {api.t('seedbox.ui.add')}
        </Button>
      </form>
    </Card>
  );
}

function Review({ api }: { api: PluginApi }) {
  const [state, setState] = useState('pending');
  const [rows, setRows] = useState<Box[]>([]);
  const load = () => api.json<{ items: Box[] }>(`/api/p/seedbox/v1/admin/boxes?state=${state}`).then((r) => setRows(r.items));
  useEffect(() => {
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
    <Card title={api.t('seedbox.ui.review')}>
      <select className="pt-input" value={state} onChange={(e) => setState(e.target.value)}>
        {['pending', 'approved', 'rejected', 'expired'].map((s) => (
          <option key={s} value={s}>
            {api.t(`seedbox.ui.state.${s}`)}
          </option>
        ))}
      </select>
      <Table
        rows={rows}
        rowKey={(b) => b.id}
        empty="—"
        columns={[
          { key: 'u', header: '', render: (b) => b.username ?? '—' },
          {
            key: 'r',
            header: 'IP',
            render: (b) => (
              <>
                <code className="pt-mono">{b.ranges.join(', ')}</code>
                {b.overlap_note && <div className="pt-field-error small">{api.t('seedbox.ui.overlap', { users: b.overlap_note })}</div>}
              </>
            ),
          },
          { key: 'p', header: api.t('seedbox.ui.provider'), render: (b) => [b.provider, b.note].filter(Boolean).join(' · ') },
          {
            key: 'a',
            header: '',
            render: (b) =>
              b.state === 'pending' ? (
                <>
                  <Button onClick={() => void review(b, true)}>{api.t('seedbox.ui.approve')}</Button>{' '}
                  <Button variant="danger" onClick={() => void review(b, false)}>
                    {api.t('seedbox.ui.reject')}
                  </Button>
                </>
              ) : (
                <Badge tone={tone(b.state)}>{api.t(`seedbox.ui.state.${b.state}`)}</Badge>
              ),
          },
        ]}
      />
    </Card>
  );
}

export default {
  slots: {
    'user.settings.tabs': { component: ({ api }: { api: PluginApi }) => <MyBoxes api={api} />, order: 60, title_term: 'seedbox.name' },
  },
  routes: [{ path: '/admin/p/seedbox/*', component: Review }],
} satisfies PluginFrontend;
