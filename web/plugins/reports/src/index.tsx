// reports 前端：种子页「举报」（理由与说明在抽屉里填写）、员工导航的待处理工单数、我的举报（/p/reports/）、
// 工单处理（/admin/p/reports/[<id>]，主从两级：左侧工单列表，右侧举报记录与处理表单）。
import { useCallback, useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Checkbox, Empty, Field, Input, MasterDetail, NavItem, Note, PageHead, Panel, Select, Sheet, SkeletonRows, Status, Table, Textarea, type Tone } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'reports.ui.report': '举报',
  'reports.ui.report_title': '举报这个种子',
  'reports.ui.report_hint': '举报只有管理组能看到。请写清楚问题，方便核实。',
  'reports.ui.reason': '理由',
  'reports.ui.note': '说明',
  'reports.ui.submit': '提交举报',
  'reports.ui.sent': '已提交，感谢举报',
  'reports.ui.menu_cases': '举报工单',
  'reports.ui.my': '我的举报',
  'reports.ui.my_lede': '感谢你帮助维护站点，处理结果会显示在这里。',
  'reports.ui.cases': '举报工单',
  'reports.ui.target': '对象',
  'reports.ui.state': '状态',
  'reports.ui.priority': '优先级',
  'reports.ui.count': '举报数',
  'reports.ui.result': '处理结果',
  'reports.ui.claim': '认领',
  'reports.ui.resolve': '处理',
  'reports.ui.code.resolved': '属实，已处理',
  'reports.ui.code.invalid': '无效举报',
  'reports.ui.code.escalated': '升级给管理员',
  'reports.ui.action.delete_content': '删除内容',
  'reports.ui.action.disable_torrent': '禁用种子',
  'reports.ui.action.restrict_user': '限制用户（禁止下载 7 天）',
  'reports.ui.actions': '同时执行',
  'reports.ui.note_public': '对举报者的说明',
  'reports.ui.note_internal': '内部备注',
  'reports.ui.reporters': '举报记录',
  'reports.ui.preview': '对象内容',
  'reports.ui.back': '工单列表',
  'reports.ui.pick': '选择左侧的工单处理',
  'reports.ui.none': '没有待处理的工单',
  'reports.ui.empty_my': '你还没有举报过内容',
  'reports.ui.state.open': '待处理',
  'reports.ui.state.claimed': '已认领',
  'reports.ui.state.closed': '已处理',
  'reports.ui.state.escalated': '已升级',
});
registerMessages('en', {
  'reports.ui.report': 'Report',
  'reports.ui.report_title': 'Report this torrent',
  'reports.ui.report_hint': 'Only staff can see reports. Describe the problem so it can be checked.',
  'reports.ui.reason': 'Reason',
  'reports.ui.note': 'Note',
  'reports.ui.submit': 'Submit report',
  'reports.ui.sent': 'Report submitted, thanks',
  'reports.ui.menu_cases': 'Report cases',
  'reports.ui.my': 'My reports',
  'reports.ui.my_lede': 'Thanks for helping keep the site tidy. Outcomes show up here.',
  'reports.ui.cases': 'Report cases',
  'reports.ui.target': 'Target',
  'reports.ui.state': 'State',
  'reports.ui.priority': 'Priority',
  'reports.ui.count': 'Reports',
  'reports.ui.result': 'Result',
  'reports.ui.claim': 'Claim',
  'reports.ui.resolve': 'Resolve',
  'reports.ui.code.resolved': 'Valid, handled',
  'reports.ui.code.invalid': 'Invalid report',
  'reports.ui.code.escalated': 'Escalate to admins',
  'reports.ui.action.delete_content': 'Delete content',
  'reports.ui.action.disable_torrent': 'Disable torrent',
  'reports.ui.action.restrict_user': 'Restrict user (no downloads for 7 days)',
  'reports.ui.actions': 'Also do',
  'reports.ui.note_public': 'Note to reporters',
  'reports.ui.note_internal': 'Internal note',
  'reports.ui.reporters': 'Reports',
  'reports.ui.preview': 'Target content',
  'reports.ui.back': 'Cases',
  'reports.ui.pick': 'Pick a case on the left',
  'reports.ui.none': 'No open cases',
  'reports.ui.empty_my': 'You have not reported anything',
  'reports.ui.state.open': 'Open',
  'reports.ui.state.claimed': 'Claimed',
  'reports.ui.state.closed': 'Closed',
  'reports.ui.state.escalated': 'Escalated',
});

function errText(api: PluginApi, e: unknown): string {
  if (e instanceof ApiError) {
    const s = api.t(e.code);
    return s === e.code ? e.message : s;
  }
  return String(e);
}

function reasonLabel(api: PluginApi, kind: string, code: string): string {
  const k = `reports.reason.${kind}.${code}`;
  const v = api.t(`term:${k}`);
  return v === k ? code : v;
}

function stateLabel(api: PluginApi, s: string): string {
  const k = `reports.ui.state.${s}`;
  const v = api.t(k);
  return v === k ? s : v;
}

const STATE_TONE: Record<string, Tone> = { open: 'warn', claimed: 'info', closed: 'ok', escalated: 'bad' };

function ReportTorrent({ api, ctx }: SlotProps<{ torrent: { id: number } }>) {
  const [open, setOpen] = useState(false);
  const [reasons, setReasons] = useState<{ code: string; requires_note: boolean }[]>([]);
  const [code, setCode] = useState('');
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  async function show() {
    const r = await api.json<{ items: { code: string; requires_note: boolean }[] }>('/api/p/reports/v1/reasons?kind=torrent');
    setReasons(r.items);
    setCode(r.items[0]?.code ?? '');
    setOpen(true);
  }
  async function submit() {
    setBusy(true);
    try {
      await api.json('/api/p/reports/v1/reports', { method: 'POST', json: { target: `torrent:${ctx.torrent.id}`, reason_code: code, note } });
      api.toast(api.t('reports.ui.sent'), 'success');
      setOpen(false);
      setNote('');
    } catch (e) {
      api.toast(errText(api, e), 'error');
    } finally {
      setBusy(false);
    }
  }
  if (!api.user()) return null;
  const needNote = reasons.find((r) => r.code === code)?.requires_note ?? false;
  return (
    <>
      <Button variant="ghost" onClick={() => void show()}>
        {api.t('reports.ui.report')}
      </Button>
      <Sheet
        open={open}
        onClose={() => setOpen(false)}
        title={api.t('reports.ui.report_title')}
        footer={
          <Button variant="danger" disabled={busy || !code || (needNote && !note.trim())} onClick={() => void submit()}>
            {api.t('reports.ui.submit')}
          </Button>
        }
      >
        <p className="small muted">{api.t('reports.ui.report_hint')}</p>
        <Field label={api.t('reports.ui.reason')}>
          <Select value={code} onChange={(e) => setCode(e.target.value)} options={reasons.map((r) => ({ value: r.code, label: reasonLabel(api, 'torrent', r.code) }))} />
        </Field>
        <Field label={api.t('reports.ui.note')}>
          <Textarea value={note} maxLength={2000} rows={4} required={needNote} onChange={(e) => setNote(e.target.value)} />
        </Field>
      </Sheet>
    </>
  );
}

interface CaseRow {
  id: number;
  target: string;
  kind: string;
  state: string;
  priority: number;
  report_count: number;
}

function StaffMenu({ api }: SlotProps) {
  const [n, setN] = useState<number | null>(null);
  useEffect(() => {
    void api
      .json<{ items: CaseRow[] }>('/api/p/reports/v1/cases')
      .then((r) => setN(r.items.length))
      .catch(() => setN(null));
  }, [api]);
  if (n === null) return null;
  return (
    <NavItem to="/admin/p/reports/" navigate={api.navigate} count={n} alert>
      {api.t('reports.ui.menu_cases')}
    </NavItem>
  );
}

function MyReports({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<{ id: number; target: string; reason_code: string; state: string; resolution: string | null; note_public: string | null }[] | null>(null);
  useEffect(() => {
    void api.json<{ items: NonNullable<typeof rows> }>('/api/p/reports/v1/my-reports').then((r) => setRows(r.items));
  }, [api]);
  return (
    <div className="stack">
      <PageHead title={api.t('reports.ui.my')} lede={api.t('reports.ui.my_lede')} />
      <Panel flush primary>
        {rows === null ? (
          <SkeletonRows n={4} />
        ) : (
          <Table
            rows={rows}
            rowKey={(r) => r.id}
            empty={<Empty>{api.t('reports.ui.empty_my')}</Empty>}
            minWidth={640}
            columns={[
              { key: 't', header: api.t('reports.ui.target'), render: (r) => <code className="mono">{r.target}</code> },
              { key: 'r', header: api.t('reports.ui.reason'), render: (r) => reasonLabel(api, r.target.split(':')[0] ?? '', r.reason_code) },
              { key: 's', header: api.t('reports.ui.state'), render: (r) => <Status tone={STATE_TONE[r.state]}>{stateLabel(api, r.state)}</Status> },
              { key: 'x', header: api.t('reports.ui.result'), render: (r) => (r.resolution ? <span className="small">{api.t(`reports.ui.code.${r.resolution}`)}{r.note_public ? ` · ${r.note_public}` : ''}</span> : '—') },
            ]}
          />
        )}
      </Panel>
    </div>
  );
}

interface Case {
  id: number;
  target: string;
  kind: string;
  state: string;
  target_preview: Record<string, unknown> | null;
  available_actions: string[];
  reports: { reporter: { id: number; username: string | null }; reason_code: string; note: string }[];
  action_results: { kind: string; ok: boolean; error?: string }[] | null;
}

function CaseDetail({ api, id, onDone }: { api: PluginApi; id: number; onDone(): void }) {
  const [c, setC] = useState<Case | null>(null);
  const [code, setCode] = useState('resolved');
  const [actions, setActions] = useState<string[]>([]);
  const [notePublic, setNotePublic] = useState('');
  const [noteInternal, setNoteInternal] = useState('');
  const [msg, setMsg] = useState<string | null>(null);
  const load = useCallback(async () => setC(await api.json<Case>(`/api/p/reports/v1/cases/${id}`)), [api, id]);
  useEffect(() => {
    setC(null);
    void load().catch((e) => setMsg(errText(api, e)));
  }, [api, load]);
  async function claim() {
    try {
      await api.json(`/api/p/reports/v1/cases/${id}/claim`, { method: 'POST', json: {} });
      await load();
      onDone();
    } catch (e) {
      setMsg(errText(api, e));
    }
  }
  async function resolve() {
    setMsg(null);
    const body = {
      code,
      note_public: notePublic,
      note_internal: noteInternal,
      actions: actions.map((k) => (k === 'restrict_user' ? { kind: k, restriction: 'leech_disabled', days: 7 } : { kind: k })),
    };
    try {
      const r = await api.fetch(`/api/p/reports/v1/cases/${id}/resolve`, { method: 'POST', json: body });
      const v = (await r.json()) as { results?: { kind: string; ok: boolean; error?: string }[] };
      if (r.status === 207) setMsg((v.results ?? []).map((x) => `${x.kind}: ${x.ok ? 'ok' : x.error}`).join('; '));
      else api.toast(api.t('reports.ui.code.' + code), 'success');
      await load();
      onDone();
    } catch (e) {
      setMsg(errText(api, e));
    }
  }
  if (!c) return msg ? <Note tone="bad">{msg}</Note> : <SkeletonRows n={4} />;
  return (
    <Panel
      title={<span className="mono">{c.target}</span>}
      sub={<Status tone={STATE_TONE[c.state]}>{stateLabel(api, c.state)}</Status>}
      actions={
        c.state !== 'closed' && (
          <Button size="xs" onClick={() => void claim()}>
            {api.t('reports.ui.claim')}
          </Button>
        )
      }
    >
      <div className="stack">
        {c.target_preview && (
          <section className="stack-s">
            <h3>{api.t('reports.ui.preview')}</h3>
            <pre className="json">{JSON.stringify(c.target_preview, null, 2)}</pre>
          </section>
        )}
        <section className="stack-s">
          <h3>{api.t('reports.ui.reporters')}</h3>
          <div className="tl">
            {c.reports.map((r, i) => (
              <div key={i}>
                <b>{r.reporter.username ?? `#${r.reporter.id}`}</b> · {reasonLabel(api, c.kind, r.reason_code)}
                {r.note && <div className="small muted">{r.note}</div>}
              </div>
            ))}
          </div>
        </section>
        {c.state !== 'closed' && (
          <form
            className="form"
            onSubmit={(e) => {
              e.preventDefault();
              void resolve();
            }}
          >
            <Field label={api.t('reports.ui.resolve')}>
              <Select value={code} onChange={(e) => setCode(e.target.value)} options={['resolved', 'invalid', 'escalated'].map((x) => ({ value: x, label: api.t(`reports.ui.code.${x}`) }))} />
            </Field>
            {code === 'resolved' && c.available_actions.some((a) => a !== 'none') && (
              <div className="fl">
                <span className="fl-label">{api.t('reports.ui.actions')}</span>
                <div className="stack-s">
                  {c.available_actions
                    .filter((a) => a !== 'none')
                    .map((a) => (
                      <Checkbox key={a} label={api.t(`reports.ui.action.${a}`)} checked={actions.includes(a)} onChange={(on) => setActions(on ? [...actions, a] : actions.filter((x) => x !== a))} />
                    ))}
                </div>
              </div>
            )}
            <Field label={api.t('reports.ui.note_public')}>
              <Input value={notePublic} onChange={(e) => setNotePublic(e.target.value)} />
            </Field>
            <Field label={api.t('reports.ui.note_internal')}>
              <Input value={noteInternal} onChange={(e) => setNoteInternal(e.target.value)} />
            </Field>
            {msg && <Note tone="warn">{msg}</Note>}
            <div className="form-acts">
              <Button type="submit" variant="primary">
                {api.t('reports.ui.resolve')}
              </Button>
            </div>
          </form>
        )}
      </div>
    </Panel>
  );
}

function Cases({ api, id }: { api: PluginApi; id: number }) {
  const [rows, setRows] = useState<CaseRow[] | null>(null);
  const load = useCallback(() => void api.json<{ items: CaseRow[] }>('/api/p/reports/v1/cases').then((r) => setRows(r.items)), [api]);
  useEffect(load, [load]);
  return (
    <div className="stack">
      <PageHead title={api.t('reports.ui.cases')} />
      <MasterDetail
        cols="minmax(0, 1fr) minmax(360px, 440px)"
        showDetail={id > 0}
        onBack={() => api.navigate('/admin/p/reports/')}
        backLabel={api.t('reports.ui.back')}
        list={
          <Panel flush>
            {rows === null ? (
              <SkeletonRows n={5} />
            ) : (
              <Table
                rows={rows}
                rowKey={(r) => r.id}
                selected={id}
                empty={<Empty>{api.t('reports.ui.none')}</Empty>}
                onRowClick={(r) => api.navigate(`/admin/p/reports/${r.id}`)}
                columns={[
                  { key: 't', header: api.t('reports.ui.target'), render: (r) => <code className="mono">{r.target}</code> },
                  { key: 's', header: api.t('reports.ui.state'), render: (r) => <Status tone={STATE_TONE[r.state]}>{stateLabel(api, r.state)}</Status> },
                  { key: 'p', header: api.t('reports.ui.priority'), right: true, render: (r) => <span className="num">{r.priority}</span> },
                  { key: 'c', header: api.t('reports.ui.count'), right: true, render: (r) => <span className="num">{r.report_count}</span> },
                ]}
              />
            )}
          </Panel>
        }
        detail={id > 0 ? <CaseDetail key={id} api={api} id={id} onDone={load} /> : <Panel><div className="md-empty">{api.t('reports.ui.pick')}</div></Panel>}
      />
    </div>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  if (path.startsWith('/admin/p/reports')) {
    const m = /^\/admin\/p\/reports\/(\d+)/.exec(path);
    return <Cases api={api} id={m?.[1] ? Number(m[1]) : 0} />;
  }
  return <MyReports api={api} />;
}

export default {
  slots: {
    'torrent.detail.actions': { component: ReportTorrent, order: 90 },
    'nav.user_menu': { component: StaffMenu, order: 80 },
  },
  routes: [
    { path: '/p/reports/*', component: Pages },
    { path: '/admin/p/reports/*', component: Pages },
  ],
} satisfies PluginFrontend;
