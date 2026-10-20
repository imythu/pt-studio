// dupe 前端：种子页「版本与替代」（被替代 / 替代了 / 同规格版本；申请替代在抽屉里）、后台审核队列、仪表盘待办数。
import { useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Checkbox, Field, Input, Kpis, Note, PageHead, Panel, PLink, Select, Sheet, SkeletonRows, Status, Table } from '@pt-studio/ui-kit';

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
  'dupe.ui.new_old': '新种子 → 被替代的旧种子',
  'dupe.ui.requested_by': '申请人',
  'dupe.ui.lede': '同一作品、同一规格只保留最好的版本；替代后旧种子标记为已替代。',
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
  'dupe.ui.new_old': 'New torrent → torrent it trumps',
  'dupe.ui.requested_by': 'Requested by',
  'dupe.ui.lede': 'Keep only the best release per work and spec; a trumped torrent is marked as such.',
  'dupe.same_content': 'A torrent with the same content already exists',
  'dupe.slot_taken': 'This work already has a torrent with the same specs; request a trump instead',
  'dupe.not_same_work': 'Both torrents must belong to the same work',
  'dupe.not_trumpable': 'The torrent to trump must be published',
  'dupe.already_requested': 'A trump for this torrent is already pending',
});

const errText = (api: PluginApi, e: unknown) => (e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e));

interface TrumpLink {
  trump_id: number;
  torrent_id: number;
  title: string | null;
  reason: string;
  state: string;
}

interface Info {
  trumped_by: TrumpLink | null;
  replaces: TrumpLink[];
  slot_peers: { torrent_id: number; title: string | null }[];
  can_request: boolean;
  candidates: { torrent_id: number; title: string | null }[];
  staff: boolean;
}

const TLink = ({ api, id, title }: { api: PluginApi; id: number; title: string | null }) => (
  <PLink to={`/torrents/${id}`} navigate={api.navigate} className="link">
    {title ?? `#${id}`}
  </PLink>
);

function TorrentCard({ api, ctx }: SlotProps<{ torrent: { id: number } }>) {
  const tid = ctx.torrent.id;
  const [d, setD] = useState<Info | null>(null);
  const [open, setOpen] = useState(false);
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
      setOpen(false);
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <section className="stack-s">
      <div className="row-sb">
        <h3>{api.t('dupe.ui.card')}</h3>
        {d.can_request && (
          <Button size="xs" variant="ghost" onClick={() => setOpen(true)}>
            {api.t('dupe.ui.request')}
          </Button>
        )}
      </div>
      {d.trumped_by && (
        <Note tone={d.trumped_by.state === 'approved' ? 'bad' : 'warn'}>
          <b>{api.t(d.trumped_by.state === 'approved' ? 'dupe.ui.trumped_by' : 'dupe.ui.trump_pending')}</b>
          <TLink api={api} id={d.trumped_by.torrent_id} title={d.trumped_by.title} />
          <span className="small">（{d.trumped_by.reason}）</span>
        </Note>
      )}
      {(d.replaces.length > 0 || d.slot_peers.length > 0) && (
        <dl className="kv">
          {d.replaces.length > 0 && (
            <>
              <dt>{api.t('dupe.ui.replaces')}</dt>
              <dd className="row" style={{ gap: 8 }}>
                {d.replaces.map((r) => (
                  <span key={r.trump_id} className="row" style={{ gap: 4 }}>
                    <TLink api={api} id={r.torrent_id} title={r.title} />
                    {r.state === 'pending' && <Status tone="warn">{api.t('dupe.ui.pending')}</Status>}
                  </span>
                ))}
              </dd>
            </>
          )}
          {d.slot_peers.length > 0 && (
            <>
              <dt>{api.t('dupe.ui.peers')}</dt>
              <dd className="row" style={{ gap: 8 }}>
                {d.slot_peers.map((p) => (
                  <TLink key={p.torrent_id} api={api} id={p.torrent_id} title={p.title} />
                ))}
              </dd>
            </>
          )}
        </dl>
      )}
      <Sheet
        open={open}
        onClose={() => setOpen(false)}
        title={api.t('dupe.ui.request')}
        footer={
          <Button variant="primary" disabled={!f.old || !f.reason.trim()} onClick={() => void submit()}>
            {api.t('dupe.ui.submit')}
          </Button>
        }
      >
        <Field label={api.t('dupe.ui.old')}>
          <Select name="trump_old" value={f.old} onChange={(e) => setF({ ...f, old: e.target.value })} options={[{ value: '', label: '—' }, ...d.candidates.map((c) => ({ value: String(c.torrent_id), label: c.title ?? `#${c.torrent_id}` }))]} />
        </Field>
        <Field label={api.t('dupe.ui.reason')}>
          <Input name="trump_reason" value={f.reason} maxLength={500} required onChange={(e) => setF({ ...f, reason: e.target.value })} />
        </Field>
        {d.staff && <Checkbox label={api.t('dupe.ui.approve_now')} checked={f.approve} onChange={(v) => setF({ ...f, approve: v })} />}
      </Sheet>
    </section>
  );
}

interface Queue {
  trumps: { id: number; old_id: number; old_title: string | null; new_id: number; new_title: string | null; reason: string; requested_by: string | null }[];
  candidates: { id: number; torrent_id: number; title: string | null; matches: number[]; slot: string }[];
}

function Admin({ api }: { api: PluginApi }) {
  const [q, setQ] = useState<Queue | null>(null);
  const [rejecting, setRejecting] = useState<number | null>(null);
  const [note, setNote] = useState('');
  const load = () => api.json<Queue>('/api/p/dupe/v1/admin/queue').then(setQ);
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const run = async (f: () => Promise<unknown>) => {
    try {
      await f();
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const decide = (id: number, decision: 'approve' | 'reject', n = '') => void run(() => api.json(`/api/p/dupe/v1/admin/trumps/${id}/decision`, { json: { decision, note: n } }));
  return (
    <div className="stack">
      <PageHead title={api.t('dupe.ui.queue')} lede={api.t('dupe.ui.lede')} />
      {!q ? (
        <SkeletonRows n={6} />
      ) : (
        <>
          <Kpis
            items={[
              { label: api.t('dupe.ui.pending'), value: q.trumps.length, tone: q.trumps.length ? 'gold' : undefined },
              { label: api.t('dupe.ui.open'), value: q.candidates.length },
            ]}
          />
          <Panel title={api.t('dupe.ui.trumps')} flush primary>
            <Table
              rows={q.trumps}
              rowKey={(r) => r.id}
              empty={api.t('dupe.ui.none')}
              minWidth={820}
              columns={[
                {
                  key: 't',
                  header: api.t('dupe.ui.new_old'),
                  render: (r) => (
                    <div className="tt">
                      <span>
                        <TLink api={api} id={r.new_id} title={r.new_title} /> → <TLink api={api} id={r.old_id} title={r.old_title} />
                      </span>
                      <span className="small muted">{r.reason}</span>
                    </div>
                  ),
                },
                { key: 'u', header: api.t('dupe.ui.requested_by'), render: (r) => <span className="small">{r.requested_by ?? '?'}</span> },
                {
                  key: 'a',
                  header: '',
                  render: (r) => (
                    <div className="acts">
                      <Button size="xs" variant="primary" onClick={() => decide(r.id, 'approve')}>
                        {api.t('dupe.ui.approve')}
                      </Button>
                      <Button size="xs" onClick={() => setRejecting(r.id)}>
                        {api.t('dupe.ui.reject')}
                      </Button>
                    </div>
                  ),
                },
              ]}
            />
          </Panel>
          <Panel title={api.t('dupe.ui.candidates')} flush>
            <Table
              rows={q.candidates}
              rowKey={(r) => r.id}
              empty={api.t('dupe.ui.none')}
              minWidth={820}
              columns={[
                {
                  key: 't',
                  header: '',
                  render: (r) => (
                    <div className="tt">
                      <span>
                        <TLink api={api} id={r.torrent_id} title={r.title} /> <span className="mono muted">{r.slot}</span>
                      </span>
                      <span className="small">
                        {api.t('dupe.ui.matches')}{' '}
                        {r.matches.map((m) => (
                          <span key={m}>
                            <TLink api={api} id={m} title={null} />{' '}
                          </span>
                        ))}
                      </span>
                    </div>
                  ),
                },
                {
                  key: 'a',
                  header: '',
                  render: (r) => (
                    <div className="acts">
                      <Button size="xs" onClick={() => void run(() => api.json(`/api/p/dupe/v1/admin/candidates/${r.id}/dismiss`, { json: {} }))}>
                        {api.t('dupe.ui.dismiss')}
                      </Button>
                    </div>
                  ),
                },
              ]}
            />
          </Panel>
        </>
      )}
      <Sheet
        open={rejecting !== null}
        onClose={() => setRejecting(null)}
        title={api.t('dupe.ui.reject')}
        footer={
          <Button
            variant="danger"
            onClick={() => {
              if (rejecting !== null) decide(rejecting, 'reject', note);
              setRejecting(null);
              setNote('');
            }}
          >
            {api.t('dupe.ui.reject')}
          </Button>
        }
      >
        <Field label={api.t('dupe.ui.note')}>
          <Input value={note} maxLength={500} onChange={(e) => setNote(e.target.value)} />
        </Field>
      </Sheet>
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
    <Panel
      title={api.t('dupe.ui.queue')}
      more={
        <PLink to="/admin/p/dupe/" navigate={api.navigate}>
          {api.t('dupe.ui.approve')}
        </PLink>
      }
    >
      <dl className="ledger">
        <dt>{api.t('dupe.ui.pending')}</dt>
        <dd className="num">{q.trumps.length}</dd>
        <dt>{api.t('dupe.ui.open')}</dt>
        <dd className="num">{q.candidates.length}</dd>
      </dl>
    </Panel>
  );
}

export default {
  slots: {
    'torrent.detail.info': { component: TorrentCard, order: 20 },
    'admin.dashboard.widgets': { component: Widget, order: 40 },
  },
  routes: [{ path: '/admin/p/dupe/*', component: Admin }],
} satisfies PluginFrontend;
