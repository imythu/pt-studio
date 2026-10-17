// dupe 前端：种子页“替代”信息卡（被替代 / 替代了 / 同规格版本 / 申请替代）、后台审核队列、仪表盘待办数。
import { useEffect, useState, type ReactNode } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Field, Input, Table } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'dupe.ui.card': '版本与替代',
  'dupe.ui.trumped_by': '此种子已被替代：',
  'dupe.ui.trump_pending': '已申请由以下种子替代（待审核）：',
  'dupe.ui.replaces': '替代了：',
  'dupe.ui.peers': '同规格的其他版本：',
  'dupe.ui.request': '申请替代旧种子',
  'dupe.ui.old': '被替代的种子',
  'dupe.ui.reason': '理由（如“修正音画不同步”“更好的压制”）',
  'dupe.ui.approve_now': '直接批准',
  'dupe.ui.submit': '提交申请',
  'dupe.ui.requested': '替代申请已提交',
  'dupe.ui.queue': '重复与替代',
  'dupe.ui.trumps': '待审核的替代申请',
  'dupe.ui.candidates': '疑似重复（同作品同规格）',
  'dupe.ui.approve': '批准',
  'dupe.ui.reject': '驳回',
  'dupe.ui.dismiss': '保留（忽略）',
  'dupe.ui.note': '备注',
  'dupe.ui.matches': '与以下种子同规格：',
  'dupe.ui.none': '没有待处理项',
  'dupe.ui.pending': '待审核替代',
  'dupe.ui.open': '疑似重复',
  'dupe.same_content': '已有内容相同的种子',
  'dupe.slot_taken': '该作品已有相同规格的种子，请申请替代',
  'dupe.not_same_work': '两个种子必须属于同一作品',
  'dupe.not_trumpable': '被替代的种子必须是已发布状态',
  'dupe.already_requested': '这个种子已有待处理的替代申请',
});
registerMessages('en', {
  'dupe.ui.card': 'Versions & trumps',
  'dupe.ui.trumped_by': 'This torrent was trumped by:',
  'dupe.ui.trump_pending': 'A trump by this torrent is pending review:',
  'dupe.ui.replaces': 'Replaces:',
  'dupe.ui.peers': 'Other torrents with the same specs:',
  'dupe.ui.request': 'Request to trump an older torrent',
  'dupe.ui.old': 'Torrent to trump',
  'dupe.ui.reason': 'Reason (e.g. "fixed A/V sync", "better encode")',
  'dupe.ui.approve_now': 'Approve now',
  'dupe.ui.submit': 'Submit',
  'dupe.ui.requested': 'Trump request submitted',
  'dupe.ui.queue': 'Dupes & trumps',
  'dupe.ui.trumps': 'Pending trump requests',
  'dupe.ui.candidates': 'Possible dupes (same work, same specs)',
  'dupe.ui.approve': 'Approve',
  'dupe.ui.reject': 'Reject',
  'dupe.ui.dismiss': 'Keep (dismiss)',
  'dupe.ui.note': 'Note',
  'dupe.ui.matches': 'Same specs as:',
  'dupe.ui.none': 'Nothing to review',
  'dupe.ui.pending': 'Pending trumps',
  'dupe.ui.open': 'Possible dupes',
  'dupe.same_content': 'A torrent with the same content already exists',
  'dupe.slot_taken': 'This work already has a torrent with the same specs; request a trump instead',
  'dupe.not_same_work': 'Both torrents must belong to the same work',
  'dupe.not_trumpable': 'The torrent to trump must be published',
  'dupe.already_requested': 'A trump for this torrent is already pending',
});

const errText = (api: PluginApi, e: unknown) => (e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e));

function A({ api, to, children }: { api: PluginApi; to: string; children: ReactNode }) {
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

interface Link {
  trump_id: number;
  torrent_id: number;
  title: string | null;
  reason: string;
  state: string;
}
interface Info {
  trumped_by: Link | null;
  replaces: Link[];
  slot_peers: { torrent_id: number; title: string | null }[];
  can_request: boolean;
  candidates: { torrent_id: number; title: string | null }[];
  staff: boolean;
}

const TLink = ({ api, id, title }: { api: PluginApi; id: number; title: string | null }) => (
  <A api={api} to={`/torrents/${id}`}>
    {title ?? `#${id}`}
  </A>
);

function TorrentCard({ api, ctx }: SlotProps<{ torrent: { id: number } }>) {
  const tid = ctx.torrent.id;
  const [d, setD] = useState<Info | null>(null);
  const [f, setF] = useState({ old: '', reason: '', approve: false });
  const load = () =>
    api
      .json<Info>(`/api/p/dupe/v1/torrents/${tid}`)
      .then(setD)
      .catch(() => undefined);
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tid]);
  if (!d || (!d.trumped_by && d.replaces.length === 0 && d.slot_peers.length === 0 && !d.can_request)) return null;
  const submit = async () => {
    try {
      await api.json('/api/p/dupe/v1/trumps', { json: { old_id: Number(f.old), new_id: tid, reason: f.reason, approve: f.approve } });
      api.toast(api.t('dupe.ui.requested'), 'success');
      setF({ old: '', reason: '', approve: false });
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <Card title={api.t('dupe.ui.card')}>
      {d.trumped_by && (
        <p className={d.trumped_by.state === 'approved' ? 'pt-field-error' : ''} role="status">
          {api.t(d.trumped_by.state === 'approved' ? 'dupe.ui.trumped_by' : 'dupe.ui.trump_pending')} <TLink api={api} id={d.trumped_by.torrent_id} title={d.trumped_by.title} />
          <span className="pt-muted small">（{d.trumped_by.reason}）</span>
        </p>
      )}
      {d.replaces.length > 0 && (
        <p>
          {api.t('dupe.ui.replaces')}
          {d.replaces.map((r) => (
            <span key={r.trump_id}>
              {' '}
              <TLink api={api} id={r.torrent_id} title={r.title} /> {r.state === 'pending' && <Badge>{api.t('dupe.ui.pending')}</Badge>}
            </span>
          ))}
        </p>
      )}
      {d.slot_peers.length > 0 && (
        <p className="small">
          {api.t('dupe.ui.peers')}
          {d.slot_peers.map((p) => (
            <span key={p.torrent_id}>
              {' '}
              <TLink api={api} id={p.torrent_id} title={p.title} />
            </span>
          ))}
        </p>
      )}
      {d.can_request && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <h4>{api.t('dupe.ui.request')}</h4>
          <Field label={api.t('dupe.ui.old')}>
            <select className="pt-input" name="trump_old" value={f.old} onChange={(e) => setF({ ...f, old: e.target.value })} required>
              <option value="" />
              {d.candidates.map((c) => (
                <option key={c.torrent_id} value={c.torrent_id}>
                  {c.title ?? `#${c.torrent_id}`}
                </option>
              ))}
            </select>
          </Field>
          <Field label={api.t('dupe.ui.reason')}>
            <Input name="trump_reason" value={f.reason} maxLength={500} required onChange={(e) => setF({ ...f, reason: e.target.value })} />
          </Field>
          {d.staff && (
            <label className="pt-check">
              <input type="checkbox" checked={f.approve} onChange={(e) => setF({ ...f, approve: e.target.checked })} /> {api.t('dupe.ui.approve_now')}
            </label>
          )}
          <Button type="submit" disabled={!f.old || !f.reason.trim()}>
            {api.t('dupe.ui.submit')}
          </Button>
        </form>
      )}
    </Card>
  );
}

interface Queue {
  trumps: { id: number; old_id: number; old_title: string | null; new_id: number; new_title: string | null; reason: string; requested_by: string | null }[];
  candidates: { id: number; torrent_id: number; title: string | null; matches: number[]; slot: string }[];
}

function Admin({ api }: { api: PluginApi }) {
  const [q, setQ] = useState<Queue | null>(null);
  const load = () => api.json<Queue>('/api/p/dupe/v1/admin/queue').then(setQ);
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  if (!q) return null;
  const run = async (f: () => Promise<unknown>) => {
    try {
      await f();
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const decide = (id: number, decision: 'approve' | 'reject') => {
    const note = decision === 'reject' ? window.prompt(api.t('dupe.ui.note')) : '';
    if (note === null) return;
    void run(() => api.json(`/api/p/dupe/v1/admin/trumps/${id}/decision`, { json: { decision, note } }));
  };
  return (
    <div className="pt-stack">
      <Card title={api.t('dupe.ui.trumps')}>
        <Table
          rows={q.trumps}
          rowKey={(r) => r.id}
          empty={api.t('dupe.ui.none')}
          columns={[
            {
              key: 't',
              header: '',
              render: (r) => (
                <>
                  <TLink api={api} id={r.new_id} title={r.new_title} /> → <TLink api={api} id={r.old_id} title={r.old_title} />
                  <div className="pt-muted small">
                    {r.reason} · {r.requested_by ?? '?'}
                  </div>
                </>
              ),
            },
            {
              key: 'a',
              header: '',
              render: (r) => (
                <span>
                  <Button variant="primary" onClick={() => decide(r.id, 'approve')}>
                    {api.t('dupe.ui.approve')}
                  </Button>{' '}
                  <Button onClick={() => decide(r.id, 'reject')}>{api.t('dupe.ui.reject')}</Button>
                </span>
              ),
            },
          ]}
        />
      </Card>
      <Card title={api.t('dupe.ui.candidates')}>
        <Table
          rows={q.candidates}
          rowKey={(r) => r.id}
          empty={api.t('dupe.ui.none')}
          columns={[
            {
              key: 't',
              header: '',
              render: (r) => (
                <>
                  <TLink api={api} id={r.torrent_id} title={r.title} /> <span className="pt-muted small pt-mono">{r.slot}</span>
                  <div className="small">
                    {api.t('dupe.ui.matches')}
                    {r.matches.map((m) => (
                      <span key={m}>
                        {' '}
                        <TLink api={api} id={m} title={null} />
                      </span>
                    ))}
                  </div>
                </>
              ),
            },
            {
              key: 'a',
              header: '',
              render: (r) => <Button onClick={() => void run(() => api.json(`/api/p/dupe/v1/admin/candidates/${r.id}/dismiss`, { json: {} }))}>{api.t('dupe.ui.dismiss')}</Button>,
            },
          ]}
        />
      </Card>
    </div>
  );
}

function Widget({ api }: SlotProps) {
  const [q, setQ] = useState<Queue | null>(null);
  useEffect(() => {
    void api
      .json<Queue>('/api/p/dupe/v1/admin/queue')
      .then(setQ)
      .catch(() => undefined);
  }, [api]);
  if (!q || (q.trumps.length === 0 && q.candidates.length === 0)) return null;
  return (
    <Card title={<A api={api} to="/admin/p/dupe/">{api.t('dupe.ui.queue')}</A>}>
      <p>
        {api.t('dupe.ui.pending')} {q.trumps.length} · {api.t('dupe.ui.open')} {q.candidates.length}
      </p>
    </Card>
  );
}

export default {
  slots: {
    'torrent.detail.info': { component: TorrentCard, order: 20 },
    'admin.dashboard.widgets': { component: Widget, order: 40 },
  },
  routes: [{ path: '/admin/p/dupe/*', component: Admin }],
} satisfies PluginFrontend;
