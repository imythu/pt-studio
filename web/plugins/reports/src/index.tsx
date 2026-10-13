// reports 前端：种子页举报按钮、员工菜单的待处理工单数、我的举报（/p/reports/）、工单处理（/admin/p/reports/[<id>]）。
import { useCallback, useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Empty, Field, Input, Table } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'reports.ui.report': '举报',
  'reports.ui.reason': '理由',
  'reports.ui.note': '说明',
  'reports.ui.submit': '提交举报',
  'reports.ui.sent': '已提交，感谢举报',
  'reports.ui.menu_cases': '举报工单（{n}）',
  'reports.ui.my': '我的举报',
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
  'reports.ui.note_public': '对举报者的说明',
  'reports.ui.note_internal': '内部备注',
  'reports.ui.reporters': '举报记录',
  'reports.ui.back': '返回列表',
});
registerMessages('en', {
  'reports.ui.report': 'Report',
  'reports.ui.reason': 'Reason',
  'reports.ui.note': 'Note',
  'reports.ui.submit': 'Submit report',
  'reports.ui.sent': 'Report submitted, thanks',
  'reports.ui.menu_cases': 'Report cases ({n})',
  'reports.ui.my': 'My reports',
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
  'reports.ui.note_public': 'Note to reporters',
  'reports.ui.note_internal': 'Internal note',
  'reports.ui.reporters': 'Reports',
  'reports.ui.back': 'Back to list',
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

function ReportTorrent({ api, ctx }: SlotProps<{ torrent: { id: number } }>) {
  const [open, setOpen] = useState(false);
  const [reasons, setReasons] = useState<{ code: string; requires_note: boolean }[]>([]);
  const [code, setCode] = useState('');
  const [note, setNote] = useState('');
  const [msg, setMsg] = useState<string | null>(null);
  async function show() {
    const r = await api.json<{ items: { code: string; requires_note: boolean }[] }>('/api/p/reports/v1/reasons?kind=torrent');
    setReasons(r.items);
    setCode(r.items[0]?.code ?? '');
    setOpen(true);
  }
  async function submit() {
    try {
      await api.json('/api/p/reports/v1/reports', { method: 'POST', json: { target: `torrent:${ctx.torrent.id}`, reason_code: code, note } });
      setMsg(api.t('reports.ui.sent'));
      setOpen(false);
    } catch (e) {
      setMsg(errText(api, e));
    }
  }
  if (!api.user()) return null;
  return (
    <span className="pt-row">
      {!open && (
        <Button variant="ghost" onClick={() => void show()}>
          {api.t('reports.ui.report')}
        </Button>
      )}
      {open && (
        <>
          <select className="pt-input" aria-label={api.t('reports.ui.reason')} value={code} onChange={(e) => setCode(e.target.value)}>
            {reasons.map((r) => (
              <option key={r.code} value={r.code}>
                {reasonLabel(api, 'torrent', r.code)}
              </option>
            ))}
          </select>
          <Input aria-label={api.t('reports.ui.note')} placeholder={api.t('reports.ui.note')} value={note} maxLength={2000} onChange={(e) => setNote(e.target.value)} />
          <Button variant="danger" onClick={() => void submit()}>
            {api.t('reports.ui.submit')}
          </Button>
        </>
      )}
      {msg && <span className="pt-muted">{msg}</span>}
    </span>
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
    <a
      href="/admin/p/reports/"
      onClick={(e) => {
        e.preventDefault();
        api.navigate('/admin/p/reports/');
      }}
    >
      {api.t('reports.ui.menu_cases', { n })}
    </a>
  );
}

function MyReports({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<{ id: number; target: string; reason_code: string; state: string; resolution: string | null; note_public: string | null }[]>([]);
  useEffect(() => {
    void api.json<{ items: typeof rows }>('/api/p/reports/v1/my-reports').then((r) => setRows(r.items));
  }, [api]);
  return (
    <Card title={api.t('reports.ui.my')}>
      <Table
        rows={rows}
        rowKey={(r) => r.id}
        empty={<Empty>—</Empty>}
        columns={[
          { key: 't', header: api.t('reports.ui.target'), render: (r) => r.target },
          { key: 'r', header: api.t('reports.ui.reason'), render: (r) => reasonLabel(api, r.target.split(':')[0] ?? '', r.reason_code) },
          { key: 's', header: api.t('reports.ui.state'), render: (r) => r.state },
          { key: 'x', header: api.t('reports.ui.result'), render: (r) => (r.resolution ? `${api.t(`reports.ui.code.${r.resolution}`)} ${r.note_public ?? ''}` : '—') },
        ]}
      />
    </Card>
  );
}

function CaseDetail({ api, id }: { api: PluginApi; id: number }) {
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
  const [c, setC] = useState<Case | null>(null);
  const [code, setCode] = useState('resolved');
  const [actions, setActions] = useState<string[]>([]);
  const [notePublic, setNotePublic] = useState('');
  const [noteInternal, setNoteInternal] = useState('');
  const [msg, setMsg] = useState<string | null>(null);
  const load = useCallback(async () => setC(await api.json<Case>(`/api/p/reports/v1/cases/${id}`)), [api, id]);
  useEffect(() => {
    void load().catch((e) => setMsg(errText(api, e)));
  }, [api, load]);
  async function claim() {
    try {
      await api.json(`/api/p/reports/v1/cases/${id}/claim`, { method: 'POST', json: {} });
      await load();
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
      await load();
    } catch (e) {
      setMsg(errText(api, e));
    }
  }
  if (!c) return msg ? <p className="pt-error">{msg}</p> : null;
  return (
    <Card title={`#${c.id} ${c.target}`} actions={<Button variant="ghost" onClick={() => api.navigate('/admin/p/reports/')}>{api.t('reports.ui.back')}</Button>}>
      <p>
        <Badge>{c.state}</Badge> <code>{JSON.stringify(c.target_preview)}</code>
      </p>
      <Table
        rows={c.reports}
        rowKey={(r) => `${r.reporter.id}-${r.reason_code}`}
        columns={[
          { key: 'u', header: '', render: (r) => r.reporter.username ?? `#${r.reporter.id}` },
          { key: 'r', header: api.t('reports.ui.reason'), render: (r) => reasonLabel(api, c.kind, r.reason_code) },
          { key: 'n', header: api.t('reports.ui.note'), render: (r) => r.note },
        ]}
      />
      {c.state !== 'closed' && (
        <div className="pt-stack">
          <Button variant="ghost" onClick={() => void claim()}>
            {api.t('reports.ui.claim')}
          </Button>
          <Field label={api.t('reports.ui.resolve')}>
            <select className="pt-input" value={code} onChange={(e) => setCode(e.target.value)}>
              {['resolved', 'invalid', 'escalated'].map((x) => (
                <option key={x} value={x}>
                  {api.t(`reports.ui.code.${x}`)}
                </option>
              ))}
            </select>
          </Field>
          {code === 'resolved' && (
            <div className="pt-checks">
              {c.available_actions
                .filter((a) => a !== 'none')
                .map((a) => (
                  <label key={a} className="pt-check">
                    <input type="checkbox" checked={actions.includes(a)} onChange={(e) => setActions(e.target.checked ? [...actions, a] : actions.filter((x) => x !== a))} />{' '}
                    {api.t(`reports.ui.action.${a}`)}
                  </label>
                ))}
            </div>
          )}
          <Field label={api.t('reports.ui.note_public')}>
            <Input value={notePublic} onChange={(e) => setNotePublic(e.target.value)} />
          </Field>
          <Field label={api.t('reports.ui.note_internal')}>
            <Input value={noteInternal} onChange={(e) => setNoteInternal(e.target.value)} />
          </Field>
          <Button variant="primary" onClick={() => void resolve()}>
            {api.t('reports.ui.resolve')}
          </Button>
        </div>
      )}
      {msg && <p className="pt-error">{msg}</p>}
    </Card>
  );
}

function Cases({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<CaseRow[]>([]);
  useEffect(() => {
    void api.json<{ items: CaseRow[] }>('/api/p/reports/v1/cases').then((r) => setRows(r.items));
  }, [api]);
  return (
    <Card title={api.t('reports.ui.cases')}>
      <Table
        rows={rows}
        rowKey={(r) => r.id}
        empty={<Empty>—</Empty>}
        columns={[
          {
            key: 't',
            header: api.t('reports.ui.target'),
            render: (r) => (
              <a
                href={`/admin/p/reports/${r.id}`}
                onClick={(e) => {
                  e.preventDefault();
                  api.navigate(`/admin/p/reports/${r.id}`);
                }}
              >
                {r.target}
              </a>
            ),
          },
          { key: 's', header: api.t('reports.ui.state'), render: (r) => r.state },
          { key: 'p', header: api.t('reports.ui.priority'), render: (r) => r.priority },
          { key: 'c', header: api.t('reports.ui.count'), render: (r) => r.report_count },
        ]}
      />
    </Card>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  const m = /^\/admin\/p\/reports\/(\d+)/.exec(path);
  if (m?.[1]) return <CaseDetail api={api} id={Number(m[1])} />;
  if (path.startsWith('/admin/p/reports')) return <Cases api={api} />;
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
