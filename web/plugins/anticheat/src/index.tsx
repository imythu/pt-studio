// anticheat 前端：后台复核队列 / 案件工作台 / 申诉 / 观察名单 / 规则集（编辑、校验、回测、激活）/ 规则统计、
// 仪表板小部件、用户详情的风险标签页、员工菜单；用户“我的案件”与申诉、被封禁用户的令牌申诉页。
import { useCallback, useEffect, useMemo, useState } from 'react';
import { ApiError, type Path } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Empty, Field, Input, Stat, Table, Tabs } from '@pt-studio/ui-kit';

const zh: Record<string, string> = {
  'anticheat.ui.queue': '复核队列',
  'anticheat.ui.appeals': '申诉',
  'anticheat.ui.watchlist': '观察名单',
  'anticheat.ui.rulesets': '规则集',
  'anticheat.ui.stats': '规则统计',
  'anticheat.ui.user': '用户',
  'anticheat.ui.score': '分数',
  'anticheat.ui.band': '分段',
  'anticheat.ui.rule': '主规则',
  'anticheat.ui.waiting': '等待（小时）',
  'anticheat.ui.assigned': '认领人',
  'anticheat.ui.auto': '自动处置',
  'anticheat.ui.state': '状态',
  'anticheat.ui.back': '返回',
  'anticheat.ui.claim': '认领',
  'anticheat.ui.release': '释放',
  'anticheat.ui.breakdown': '分数构成',
  'anticheat.ui.evidence': '证据',
  'anticheat.ui.sanctions': '处置',
  'anticheat.ui.timeline': '时间线',
  'anticheat.ui.swarm': 'swarm 快照',
  'anticheat.ui.notes': '员工备注',
  'anticheat.ui.add_note': '添加备注',
  'anticheat.ui.decide': '作出决定',
  'anticheat.ui.decision.sanction': '处置',
  'anticheat.ui.decision.no_action': '成立但不处置',
  'anticheat.ui.decision.dismiss': '误判',
  'anticheat.ui.note': '决定说明（必填）',
  'anticheat.ui.days': '天数',
  'anticheat.ui.up_bytes': '回收上传（字节）',
  'anticheat.ui.submit': '提交',
  'anticheat.ui.approve': '批准',
  'anticheat.ui.lift': '解除',
  'anticheat.ui.suggestions': '策略建议',
  'anticheat.ui.traffic': '流量概况',
  'anticheat.ui.uploaded': '上传',
  'anticheat.ui.downloaded': '下载',
  'anticheat.ui.explain': 'AI 解释（仅供参考）',
  'anticheat.ui.explain_queued': '已排队，稍后刷新',
  'anticheat.ui.fp': '标记为误判',
  'anticheat.ui.override': '已被管理员在外部解除',
  'anticheat.ui.my_cases': '我的案件',
  'anticheat.ui.appeal': '申诉',
  'anticheat.ui.appeal_body': '申诉说明',
  'anticheat.ui.appeal_deadline': '申诉截止',
  'anticheat.ui.appeal_sent': '申诉已提交',
  'anticheat.ui.token_appeal': '封禁申诉',
  'anticheat.ui.no_token': '链接无效：缺少申诉令牌',
  'anticheat.ui.torrents': '涉及的种子',
  'anticheat.ui.outcome.upheld': '维持',
  'anticheat.ui.outcome.reduced': '减轻',
  'anticheat.ui.outcome.overturned': '撤销',
  'anticheat.ui.compensate': '补偿上传（字节，可选）',
  'anticheat.ui.version': '版本',
  'anticheat.ui.new_draft': '基于此版本新建草案',
  'anticheat.ui.validate': '校验',
  'anticheat.ui.save': '保存草案',
  'anticheat.ui.simulate': '回测（30 天）',
  'anticheat.ui.activate': '激活',
  'anticheat.ui.valid': '校验通过',
  'anticheat.ui.json_error': 'JSON 格式错误',
  'anticheat.ui.precision': '准确率',
  'anticheat.ui.hits': '命中',
  'anticheat.ui.opened': '开案',
  'anticheat.ui.confirmed': '确认',
  'anticheat.ui.dismissed': '误判',
  'anticheat.ui.overturned': '撤销',
  'anticheat.ui.open_cases': '待复核案件',
  'anticheat.ui.over_sla': '超时',
  'anticheat.ui.oldest': '最久等待（小时）',
  'anticheat.ui.appeals_pending': '待处理申诉',
  'anticheat.ui.breaker': '自动处置已熔断',
  'anticheat.ui.reset_breaker': '复位熔断',
  'anticheat.ui.menu': '反作弊（{n}）',
  'anticheat.ui.risk': '风险',
  'anticheat.ui.open_case': '手动开案',
  'anticheat.ui.reason': '原因',
  'anticheat.ui.none': '无',
  'anticheat.ui.kind.warn': '警告',
  'anticheat.ui.kind.freeze_traffic': '冻结上传流量',
  'anticheat.ui.kind.lock_wallet': '锁定钱包转出',
  'anticheat.ui.kind.disable_download': '禁止下载',
  'anticheat.ui.kind.ban': '封禁',
  'anticheat.ui.kind.adjust_traffic': '回收虚报流量',
  'anticheat.ui.kind.reset_passkey': '重置 passkey',
  'anticheat.case_not_found': '案件不存在',
  'anticheat.case_claimed': '案件已被他人认领',
  'anticheat.claim_required': '请先认领案件',
  'anticheat.action_not_allowed': '没有该处置所需的权限',
  'anticheat.target_protected': '受保护用户不能施加该处置',
  'anticheat.four_eyes_required': '需要另一名管理员处理（四眼原则）',
  'anticheat.simulation_required': '激活前需要先回测该草案',
  'anticheat.ruleset_invalid': '规则集未通过校验',
  'anticheat.appeal_window_closed': '已过申诉期限',
  'anticheat.appeal_exists': '已有处理中的申诉',
  'anticheat.appeal_limit_reached': '申诉次数已用完',
  'anticheat.appeal_token_invalid': '申诉链接无效或已过期',
  'anticheat.transfer_blocked': '账户正在接受审核，暂时无法转出',
};
registerMessages('zh-CN', zh);
registerMessages('en', {
  'anticheat.ui.queue': 'Review queue',
  'anticheat.ui.appeals': 'Appeals',
  'anticheat.ui.watchlist': 'Watchlist',
  'anticheat.ui.rulesets': 'Rulesets',
  'anticheat.ui.stats': 'Rule stats',
  'anticheat.ui.my_cases': 'My cases',
  'anticheat.ui.token_appeal': 'Ban appeal',
  'anticheat.ui.menu': 'Anti-cheat ({n})',
  'anticheat.transfer_blocked': 'Your account is under review; transfers are temporarily disabled',
});

const KINDS = ['warn', 'freeze_traffic', 'lock_wallet', 'adjust_traffic', 'disable_download', 'ban'] as const;
const BASE = '/api/p/anticheat/v1' as const;

function errText(api: PluginApi, e: unknown): string {
  if (e instanceof ApiError) {
    const s = api.t(e.code);
    return s === e.code ? e.message : s;
  }
  return String(e);
}

function bandTone(b: string): 'muted' | 'primary' | 'danger' | 'success' {
  return b === 'critical' || b === 'high' ? 'danger' : b === 'watch' ? 'primary' : 'muted';
}

function bytes(n: unknown): string {
  const v = Number(n ?? 0);
  if (!Number.isFinite(v)) return '—';
  const u = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB'];
  let x = Math.abs(v);
  let i = 0;
  while (x >= 1024 && i < u.length - 1) {
    x /= 1024;
    i++;
  }
  return `${v < 0 ? '-' : ''}${x.toFixed(i ? 2 : 0)} ${u[i]}`;
}

function when(s: unknown): string {
  return typeof s === 'string' ? new Date(s).toLocaleString() : '—';
}

function kindLabel(api: PluginApi, k: string): string {
  const key = `anticheat.ui.kind.${k}`;
  const v = api.t(key);
  return v === key ? k : v;
}

function Link({ api, to, children }: { api: PluginApi; to: string; children: React.ReactNode }) {
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

function useLoad<T>(api: PluginApi, path: Path | null): [T | null, string | null, () => void] {
  const [d, setD] = useState<T | null>(null);
  const [e, setE] = useState<string | null>(null);
  const [n, setN] = useState(0);
  useEffect(() => {
    if (!path) return;
    let live = true;
    api
      .json<T>(path)
      .then((v) => live && (setD(v), setE(null)))
      .catch((x) => live && setE(errText(api, x)));
    return () => {
      live = false;
    };
  }, [api, path, n]);
  return [d, e, () => setN((x) => x + 1)];
}

// ---------------- 后台：队列 ----------------

interface CaseRow {
  id: number;
  user: { id: number; username: string | null };
  state: string;
  score: number;
  band: string;
  primary_rule: string;
  waiting_hours: number;
  assigned_to: number | null;
  auto_sanctions: string[];
}

function Queue({ api }: { api: PluginApi }) {
  const [state, setState] = useState('open');
  const [d, err] = useLoad<{ items: CaseRow[] }>(api, `${BASE}/admin/cases?states=${state}&limit=100`);
  return (
    <Card
      title={api.t('anticheat.ui.queue')}
      actions={
        <select className="pt-input" value={state} onChange={(e) => setState(e.target.value)} aria-label={api.t('anticheat.ui.state')}>
          {['open', 'resolved', 'dismissed', 'overturned', 'closed_stale'].map((s) => (
            <option key={s} value={s}>
              {s}
            </option>
          ))}
        </select>
      }
    >
      {err && <p className="pt-error">{err}</p>}
      <Table
        rows={d?.items ?? []}
        rowKey={(r) => r.id}
        empty={<Empty>—</Empty>}
        columns={[
          {
            key: 'id',
            header: '#',
            render: (r) => (
              <Link api={api} to={`/admin/p/anticheat/cases/${r.id}`}>
                {r.id}
              </Link>
            ),
          },
          {
            key: 'u',
            header: api.t('anticheat.ui.user'),
            render: (r) => (
              <Link api={api} to={`/admin/users/${r.user.id}`}>
                {r.user.username ?? `#${r.user.id}`}
              </Link>
            ),
          },
          {
            key: 's',
            header: api.t('anticheat.ui.score'),
            render: (r) => r.score,
          },
          {
            key: 'b',
            header: api.t('anticheat.ui.band'),
            render: (r) => <Badge tone={bandTone(r.band)}>{r.band}</Badge>,
          },
          {
            key: 'r',
            header: api.t('anticheat.ui.rule'),
            render: (r) => <code>{r.primary_rule}</code>,
          },
          {
            key: 'w',
            header: api.t('anticheat.ui.waiting'),
            render: (r) => r.waiting_hours,
          },
          {
            key: 'a',
            header: api.t('anticheat.ui.assigned'),
            render: (r) => (r.assigned_to ? `#${r.assigned_to}` : '—'),
          },
          {
            key: 'x',
            header: api.t('anticheat.ui.auto'),
            render: (r) =>
              r.auto_sanctions.map((k) => (
                <Badge key={k} tone="danger">
                  {kindLabel(api, k)}
                </Badge>
              )),
          },
        ]}
      />
    </Card>
  );
}

// ---------------- 后台：案件工作台 ----------------

interface Sanction {
  id: number;
  kind: string;
  state: string;
  until: string | null;
  policy_id: string | null;
  decided_by: number | null;
  external_override: boolean;
}

interface CaseDetail {
  id: number;
  user: {
    id: number;
    username: string | null;
    account_age_days: number | null;
  };
  state: string;
  score: number;
  peak_score: number;
  band: string;
  primary_rule: string;
  assigned_to: number | null;
  claim_expires_at: string | null;
  breakdown: {
    rule: string;
    type: string;
    mode: string;
    score?: number;
    evidence?: number;
    factor?: number;
    top_torrents?: number[];
  }[];
  evidence: {
    id: number;
    kind: string;
    severity: string | null;
    hits: number;
    details: Record<string, unknown>;
    torrent: { id: number; title: string | null } | null;
    occurred_at: string;
    false_positive: boolean;
  }[];
  sanctions: Sanction[];
  suggestions: {
    policy_id: string;
    actions: { kind: string; days: number | null; up_bytes?: number }[];
  }[];
  traffic: { uploaded: number | null; downloaded: number | null };
  timeline: {
    at: string;
    kind: string;
    actor: number | null;
    data: Record<string, unknown>;
  }[];
  swarm_snapshots: {
    torrent_id: number;
    taken_at: string;
    peers: {
      user_id: number;
      client: string | null;
      seeder: boolean;
      uploaded: number;
      downloaded: number;
    }[];
  }[];
  notes: { by: number; at: string; text: string }[];
  appeals: { id: number; state: string; body: string | null }[];
  resolution: string | null;
  resolution_note: string | null;
}

function Decide({ api, c, done }: { api: PluginApi; c: CaseDetail; done: () => void }) {
  const [decision, setDecision] = useState<'sanction' | 'no_action' | 'dismiss'>('sanction');
  const [acts, setActs] = useState<Record<string, { on: boolean; days: string; up: string }>>({});
  const [fp, setFp] = useState<number[]>([]);
  const [note, setNote] = useState('');
  const [msg, setMsg] = useState<string | null>(null);
  function applySuggestion(s: CaseDetail['suggestions'][number]) {
    const next: typeof acts = {};
    for (const a of s.actions)
      next[a.kind] = {
        on: true,
        days: a.days ? String(a.days) : '',
        up: a.up_bytes ? String(a.up_bytes) : '',
      };
    setDecision('sanction');
    setActs(next);
  }
  async function submit() {
    setMsg(null);
    const actions = Object.entries(acts)
      .filter(([, v]) => v.on)
      .map(([kind, v]) => ({
        kind,
        ...(v.days ? { days: Number(v.days) } : {}),
        ...(kind === 'adjust_traffic' ? { up_bytes: Number(v.up) } : {}),
      }));
    try {
      await api.json(`${BASE}/admin/cases/${c.id}/resolutions`, {
        method: 'POST',
        json: {
          decision,
          actions: decision === 'sanction' ? actions : [],
          false_positive_evidence_ids: decision === 'dismiss' ? fp : [],
          note,
        },
        idempotent: true,
      });
      done();
    } catch (e) {
      setMsg(errText(api, e));
    }
  }
  return (
    <Card title={api.t('anticheat.ui.decide')}>
      {c.suggestions.length > 0 && (
        <p>
          {api.t('anticheat.ui.suggestions')}:{' '}
          {c.suggestions.map((s) => (
            <Button key={s.policy_id} variant="ghost" onClick={() => applySuggestion(s)}>
              {s.policy_id}: {s.actions.map((a) => kindLabel(api, a.kind)).join(' + ')}
            </Button>
          ))}
        </p>
      )}
      <div className="pt-row">
        {(['sanction', 'no_action', 'dismiss'] as const).map((d) => (
          <label key={d} className="pt-check">
            <input type="radio" name="decision" checked={decision === d} onChange={() => setDecision(d)} /> {api.t(`anticheat.ui.decision.${d}`)}
          </label>
        ))}
      </div>
      {decision === 'sanction' && (
        <div className="pt-stack">
          {KINDS.map((k) => {
            const v = acts[k] ?? { on: false, days: '', up: '' };
            const set = (p: Partial<typeof v>) => setActs({ ...acts, [k]: { ...v, ...p } });
            return (
              <div key={k} className="pt-row">
                <label className="pt-check">
                  <input type="checkbox" checked={v.on} onChange={(e) => set({ on: e.target.checked })} /> {kindLabel(api, k)}
                </label>
                {v.on && k !== 'adjust_traffic' && k !== 'lock_wallet' && (
                  <Input
                    type="number"
                    min={1}
                    max={3650}
                    placeholder={api.t('anticheat.ui.days')}
                    value={v.days}
                    onChange={(e) => set({ days: e.target.value })}
                    aria-label={api.t('anticheat.ui.days')}
                  />
                )}
                {v.on && k === 'adjust_traffic' && (
                  <Input
                    type="number"
                    min={1}
                    placeholder={api.t('anticheat.ui.up_bytes')}
                    value={v.up}
                    onChange={(e) => set({ up: e.target.value })}
                    aria-label={api.t('anticheat.ui.up_bytes')}
                  />
                )}
                {v.on && k === 'adjust_traffic' && v.up && <span className="pt-muted">{bytes(v.up)}</span>}
              </div>
            );
          })}
        </div>
      )}
      {decision === 'dismiss' && (
        <div className="pt-checks">
          {c.evidence.map((e) => (
            <label key={e.id} className="pt-check">
              <input type="checkbox" checked={fp.includes(e.id)} onChange={(x) => setFp(x.target.checked ? [...fp, e.id] : fp.filter((i) => i !== e.id))} />{' '}
              {api.t('anticheat.ui.fp')} #{e.id} {e.kind}
            </label>
          ))}
        </div>
      )}
      <Field label={api.t('anticheat.ui.note')}>
        <textarea className="pt-input" rows={3} maxLength={2000} value={note} onChange={(e) => setNote(e.target.value)} />
      </Field>
      <Button variant="primary" disabled={!note.trim()} onClick={() => void submit()}>
        {api.t('anticheat.ui.submit')}
      </Button>
      {msg && <p className="pt-error">{msg}</p>}
    </Card>
  );
}

function SanctionTable({ api, rows, reload }: { api: PluginApi; rows: Sanction[]; reload: () => void }) {
  const [msg, setMsg] = useState<string | null>(null);
  async function act(path: Path, body: unknown) {
    try {
      await api.json(path, { method: 'POST', json: body, idempotent: true });
      reload();
    } catch (e) {
      setMsg(errText(api, e));
    }
  }
  return (
    <>
      <Table
        rows={rows}
        rowKey={(r) => r.id}
        empty={<Empty>{api.t('anticheat.ui.none')}</Empty>}
        columns={[
          { key: 'k', header: '', render: (r) => kindLabel(api, r.kind) },
          {
            key: 's',
            header: api.t('anticheat.ui.state'),
            render: (r) => <Badge tone={r.state === 'active' ? 'danger' : 'muted'}>{r.state}</Badge>,
          },
          {
            key: 'u',
            header: '',
            render: (r) => (r.until ? when(r.until) : '∞'),
          },
          {
            key: 'p',
            header: '',
            render: (r) => (r.policy_id ? <code>{r.policy_id}</code> : r.decided_by ? `#${r.decided_by}` : ''),
          },
          {
            key: 'o',
            header: '',
            render: (r) => (r.external_override ? <Badge>{api.t('anticheat.ui.override')}</Badge> : null),
          },
          {
            key: 'a',
            header: '',
            render: (r) => (
              <span className="pt-row">
                {r.state === 'proposed' && (
                  <Button onClick={() => void act(`${BASE}/admin/sanctions/${r.id}/approvals`, {})}>{api.t('anticheat.ui.approve')}</Button>
                )}
                {['proposed', 'pending', 'active', 'suspended'].includes(r.state) && (
                  <Button
                    variant="ghost"
                    onClick={() => {
                      const reason = window.prompt(api.t('anticheat.ui.reason'));
                      if (reason)
                        void act(`${BASE}/admin/sanctions/${r.id}/lifts`, {
                          reason_note: reason,
                        });
                    }}
                  >
                    {api.t('anticheat.ui.lift')}
                  </Button>
                )}
              </span>
            ),
          },
        ]}
      />
      {msg && <p className="pt-error">{msg}</p>}
    </>
  );
}

function Explain({ api, id }: { api: PluginApi; id: number }) {
  const [text, setText] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  async function load() {
    try {
      const r = await api.json<{ text: string; stale: boolean }>(`${BASE}/admin/cases/${id}/explanations/latest`);
      setText(r.text);
    } catch {
      setText(null);
    }
  }
  useEffect(() => {
    void load();
  }, [id]);
  async function ask() {
    try {
      await api.json(`${BASE}/admin/cases/${id}/explanations`, {
        method: 'POST',
        json: {},
      });
      setMsg(api.t('anticheat.ui.explain_queued'));
    } catch (e) {
      setMsg(errText(api, e));
    }
  }
  return (
    <Card
      title={api.t('anticheat.ui.explain')}
      actions={
        <Button variant="ghost" onClick={() => void (text ? load() : ask())}>
          {text ? '↻' : api.t('anticheat.ui.explain')}
        </Button>
      }
    >
      {text ? <p style={{ whiteSpace: 'pre-wrap' }}>{text}</p> : <p className="pt-muted">{msg ?? '—'}</p>}
    </Card>
  );
}

function Workbench({ api, id }: { api: PluginApi; id: number }) {
  const [c, err, reload] = useLoad<CaseDetail>(api, `${BASE}/admin/cases/${id}`);
  const [note, setNote] = useState('');
  const [msg, setMsg] = useState<string | null>(null);
  const me = api.user();
  async function post(path: Path, body: unknown, method = 'POST') {
    setMsg(null);
    try {
      await api.fetch(path, { method, json: body });
      reload();
    } catch (e) {
      setMsg(errText(api, e));
    }
  }
  if (!c) return err ? <p className="pt-error">{err}</p> : null;
  const mine = c.assigned_to === me?.id;
  return (
    <div className="pt-stack">
      <Card
        title={
          <>
            #{c.id}{' '}
            <Link api={api} to={`/admin/users/${c.user.id}`}>
              {c.user.username ?? `#${c.user.id}`}
            </Link>{' '}
            <Badge tone={bandTone(c.band)}>{c.band}</Badge>
          </>
        }
        actions={
          <span className="pt-row">
            {c.state === 'open' &&
              (mine ? (
                <Button variant="ghost" onClick={() => void post(`${BASE}/admin/cases/${id}/claims`, undefined, 'DELETE')}>
                  {api.t('anticheat.ui.release')}
                </Button>
              ) : (
                <Button onClick={() => void post(`${BASE}/admin/cases/${id}/claims`, {})}>{api.t('anticheat.ui.claim')}</Button>
              ))}
            <Button variant="ghost" onClick={() => api.navigate('/admin/p/anticheat/')}>
              {api.t('anticheat.ui.back')}
            </Button>
          </span>
        }
      >
        <div className="pt-row">
          <Stat label={api.t('anticheat.ui.score')} value={`${c.score} / ${c.peak_score}`} />
          <Stat label={api.t('anticheat.ui.state')} value={c.resolution ? `${c.state} · ${c.resolution}` : c.state} />
          <Stat label={api.t('anticheat.ui.uploaded')} value={bytes(c.traffic.uploaded)} />
          <Stat label={api.t('anticheat.ui.downloaded')} value={bytes(c.traffic.downloaded)} />
          <Stat
            label={api.t('anticheat.ui.assigned')}
            value={c.assigned_to ? `#${c.assigned_to}` : '—'}
            hint={c.claim_expires_at ? when(c.claim_expires_at) : undefined}
          />
          <Stat label="account" value={c.user.account_age_days ?? '—'} hint="days" />
        </div>
        {c.resolution_note && <p className="pt-muted">{c.resolution_note}</p>}
        {msg && <p className="pt-error">{msg}</p>}
      </Card>
      <Card title={api.t('anticheat.ui.breakdown')}>
        <Table
          rows={c.breakdown}
          rowKey={(r) => r.rule}
          empty={<Empty>—</Empty>}
          columns={[
            {
              key: 'r',
              header: api.t('anticheat.ui.rule'),
              render: (r) => <code>{r.rule}</code>,
            },
            {
              key: 't',
              header: '',
              render: (r) => `${r.type}${r.mode !== 'enforce' ? ` (${r.mode})` : ''}`,
            },
            {
              key: 's',
              header: api.t('anticheat.ui.score'),
              render: (r) => (r.factor ? `× ${r.factor}` : r.score),
            },
            {
              key: 'e',
              header: api.t('anticheat.ui.evidence'),
              render: (r) => r.evidence ?? '',
            },
            {
              key: 'x',
              header: '',
              render: (r) => (r.top_torrents ?? []).map((t) => `#${t}`).join(' '),
            },
          ]}
        />
      </Card>
      <Card title={api.t('anticheat.ui.evidence')}>
        <Table
          rows={c.evidence}
          rowKey={(r) => r.id}
          empty={<Empty>—</Empty>}
          columns={[
            {
              key: 'k',
              header: '',
              render: (r) => (
                <span
                  style={{
                    textDecoration: r.false_positive ? 'line-through' : undefined,
                  }}
                >
                  {r.kind}
                </span>
              ),
            },
            {
              key: 's',
              header: '',
              render: (r) => (r.severity ? <Badge tone={r.severity === 'high' ? 'danger' : 'muted'}>{r.severity}</Badge> : null),
            },
            { key: 'h', header: '×', render: (r) => r.hits },
            {
              key: 't',
              header: '',
              render: (r) =>
                r.torrent ? (
                  <Link api={api} to={`/torrents/${r.torrent.id}`}>
                    {r.torrent.title ?? `#${r.torrent.id}`}
                  </Link>
                ) : (
                  '—'
                ),
            },
            {
              key: 'd',
              header: '',
              render: (r) => (
                <code className="pt-muted">
                  {Object.entries(r.details)
                    .map(([k, v]) => `${k}=${typeof v === 'number' && /bytes|uploaded|downloaded|speed/.test(k) ? bytes(v) : String(v)}`)
                    .join(' ')}
                </code>
              ),
            },
            { key: 'at', header: '', render: (r) => when(r.occurred_at) },
          ]}
        />
      </Card>
      <Card title={api.t('anticheat.ui.sanctions')}>
        <SanctionTable api={api} rows={c.sanctions} reload={reload} />
      </Card>
      {c.state === 'open' && <Decide api={api} c={c} done={reload} />}
      {c.swarm_snapshots.map((s) => (
        <Card key={`${s.torrent_id}-${s.taken_at}`} title={`${api.t('anticheat.ui.swarm')} #${s.torrent_id} · ${when(s.taken_at)}`}>
          <Table
            rows={s.peers}
            rowKey={(p) => `${p.user_id}-${p.client}`}
            columns={[
              {
                key: 'u',
                header: api.t('anticheat.ui.user'),
                render: (p) => (p.user_id === c.user.id ? <strong>#{p.user_id}</strong> : `#${p.user_id}`),
              },
              { key: 'c', header: '', render: (p) => p.client ?? '' },
              {
                key: 's',
                header: '',
                render: (p) => (p.seeder ? 'seed' : 'leech'),
              },
              {
                key: 'up',
                header: api.t('anticheat.ui.uploaded'),
                render: (p) => bytes(p.uploaded),
              },
              {
                key: 'dn',
                header: api.t('anticheat.ui.downloaded'),
                render: (p) => bytes(p.downloaded),
              },
            ]}
          />
        </Card>
      ))}
      <Explain api={api} id={c.id} />
      <Card title={api.t('anticheat.ui.notes')}>
        {c.notes.map((n, i) => (
          <p key={i}>
            <span className="pt-muted">
              #{n.by} · {when(n.at)}
            </span>{' '}
            <span style={{ whiteSpace: 'pre-wrap' }}>{n.text}</span>
          </p>
        ))}
        <div className="pt-row">
          <Input
            value={note}
            maxLength={2000}
            onChange={(e) => setNote(e.target.value)}
            aria-label={api.t('anticheat.ui.add_note')}
            placeholder={api.t('anticheat.ui.add_note')}
          />
          <Button
            disabled={!note.trim()}
            onClick={() => {
              void post(`${BASE}/admin/cases/${id}/notes`, { text: note });
              setNote('');
            }}
          >
            {api.t('anticheat.ui.add_note')}
          </Button>
        </div>
      </Card>
      <Card title={api.t('anticheat.ui.timeline')}>
        {c.timeline.map((e, i) => (
          <p key={i} className="pt-muted">
            {when(e.at)} · <strong>{e.kind}</strong> {e.actor ? `#${e.actor}` : ''} <code>{JSON.stringify(e.data)}</code>
          </p>
        ))}
      </Card>
    </div>
  );
}

// ---------------- 后台：申诉 ----------------

function Appeals({ api }: { api: PluginApi }) {
  const [d, err, reload] = useLoad<{
    items: {
      id: number;
      case_id: number;
      user: { id: number; username: string | null };
      state: string;
      via: string;
      body: string | null;
      created_at: string;
    }[];
  }>(api, `${BASE}/admin/appeals`);
  const [open, setOpen] = useState<number | null>(null);
  const [outcome, setOutcome] = useState('upheld');
  const [note, setNote] = useState('');
  const [comp, setComp] = useState('');
  const [msg, setMsg] = useState<string | null>(null);
  async function decide(id: number) {
    setMsg(null);
    try {
      await api.json(`${BASE}/admin/appeals/${id}/claims`, {
        method: 'POST',
        json: {},
      });
      await api.json(`${BASE}/admin/appeals/${id}/decisions`, {
        method: 'POST',
        json: {
          outcome,
          note,
          ...(outcome === 'overturned' && comp ? { compensate_bytes: Number(comp) } : {}),
        },
        idempotent: true,
      });
      setOpen(null);
      reload();
    } catch (e) {
      setMsg(errText(api, e));
    }
  }
  return (
    <Card title={api.t('anticheat.ui.appeals')}>
      {err && <p className="pt-error">{err}</p>}
      {(d?.items ?? []).length === 0 && <Empty>—</Empty>}
      {(d?.items ?? []).map((a) => (
        <div
          key={a.id}
          className="pt-stack"
          style={{
            borderBottom: '1px solid var(--pt-border, #ddd)',
            paddingBottom: 8,
          }}
        >
          <p>
            <Link api={api} to={`/admin/p/anticheat/cases/${a.case_id}`}>
              #{a.case_id}
            </Link>{' '}
            · {a.user.username ?? `#${a.user.id}`} · <Badge>{a.state}</Badge> · {a.via} · {when(a.created_at)}
          </p>
          <p style={{ whiteSpace: 'pre-wrap' }}>{a.body}</p>
          {open === a.id ? (
            <div className="pt-stack">
              <select className="pt-input" value={outcome} onChange={(e) => setOutcome(e.target.value)} aria-label="outcome">
                {['upheld', 'overturned'].map((o) => (
                  <option key={o} value={o}>
                    {api.t(`anticheat.ui.outcome.${o}`)}
                  </option>
                ))}
              </select>
              {outcome === 'overturned' && (
                <Input type="number" min={0} placeholder={api.t('anticheat.ui.compensate')} value={comp} onChange={(e) => setComp(e.target.value)} />
              )}
              <Input placeholder={api.t('anticheat.ui.note')} value={note} onChange={(e) => setNote(e.target.value)} />
              <Button variant="primary" disabled={!note.trim()} onClick={() => void decide(a.id)}>
                {api.t('anticheat.ui.submit')}
              </Button>
            </div>
          ) : (
            <Button onClick={() => setOpen(a.id)}>{api.t('anticheat.ui.decide')}</Button>
          )}
        </div>
      ))}
      {msg && <p className="pt-error">{msg}</p>}
    </Card>
  );
}

function Watchlist({ api }: { api: PluginApi }) {
  const [d] = useLoad<{
    items: {
      user: { id: number; username: string | null };
      score: number;
      band: string;
    }[];
  }>(api, `${BASE}/admin/watchlist`);
  return (
    <Card title={api.t('anticheat.ui.watchlist')}>
      <Table
        rows={d?.items ?? []}
        rowKey={(r) => r.user.id}
        empty={<Empty>—</Empty>}
        columns={[
          {
            key: 'u',
            header: api.t('anticheat.ui.user'),
            render: (r) => (
              <Link api={api} to={`/admin/users/${r.user.id}`}>
                {r.user.username ?? `#${r.user.id}`}
              </Link>
            ),
          },
          {
            key: 's',
            header: api.t('anticheat.ui.score'),
            render: (r) => r.score,
          },
        ]}
      />
    </Card>
  );
}

function Stats({ api }: { api: PluginApi }) {
  const [d] = useLoad<{
    items: {
      rule_id: string;
      hits: number;
      cases_opened: number;
      confirmed: number;
      dismissed: number;
      overturned: number;
      precision: number | null;
      low_precision: boolean;
    }[];
  }>(api, `${BASE}/admin/rule-stats?days=90`);
  return (
    <Card title={api.t('anticheat.ui.stats')}>
      <Table
        rows={d?.items ?? []}
        rowKey={(r) => r.rule_id}
        empty={<Empty>—</Empty>}
        columns={[
          {
            key: 'r',
            header: api.t('anticheat.ui.rule'),
            render: (r) => <code>{r.rule_id}</code>,
          },
          {
            key: 'h',
            header: api.t('anticheat.ui.hits'),
            render: (r) => r.hits,
          },
          {
            key: 'o',
            header: api.t('anticheat.ui.opened'),
            render: (r) => r.cases_opened,
          },
          {
            key: 'c',
            header: api.t('anticheat.ui.confirmed'),
            render: (r) => r.confirmed,
          },
          {
            key: 'd',
            header: api.t('anticheat.ui.dismissed'),
            render: (r) => r.dismissed,
          },
          {
            key: 'v',
            header: api.t('anticheat.ui.overturned'),
            render: (r) => r.overturned,
          },
          {
            key: 'p',
            header: api.t('anticheat.ui.precision'),
            render: (r) => (r.precision == null ? '—' : <Badge tone={r.low_precision ? 'danger' : 'success'}>{(r.precision * 100).toFixed(0)}%</Badge>),
          },
        ]}
      />
    </Card>
  );
}

// ---------------- 后台：规则集 ----------------

interface VErr {
  path: string;
  code: string;
  message: string;
}

function Rulesets({ api }: { api: PluginApi }) {
  const [d] = useLoad<{
    items: {
      version: number;
      status: string;
      note: string | null;
      created_at: string;
      activated_at: string | null;
    }[];
  }>(api, `${BASE}/admin/rulesets`);
  return (
    <Card title={api.t('anticheat.ui.rulesets')}>
      <Table
        rows={d?.items ?? []}
        rowKey={(r) => r.version}
        empty={<Empty>—</Empty>}
        columns={[
          {
            key: 'v',
            header: api.t('anticheat.ui.version'),
            render: (r) => (
              <Link api={api} to={`/admin/p/anticheat/rulesets/${r.version}`}>
                v{r.version}
              </Link>
            ),
          },
          {
            key: 's',
            header: api.t('anticheat.ui.state'),
            render: (r) => <Badge tone={r.status === 'active' ? 'success' : 'muted'}>{r.status}</Badge>,
          },
          { key: 'n', header: '', render: (r) => r.note ?? '' },
          {
            key: 'c',
            header: '',
            render: (r) => when(r.activated_at ?? r.created_at),
          },
        ]}
      />
    </Card>
  );
}

function RulesetEditor({ api, version }: { api: PluginApi; version: number }) {
  const [rs, err, reload] = useLoad<{
    version: number;
    status: string;
    definition: unknown;
    checksum: string;
    note: string | null;
  }>(api, `${BASE}/admin/rulesets/${version}`);
  const [text, setText] = useState('');
  const [errs, setErrs] = useState<VErr[] | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [sim, setSim] = useState<{
    id: string;
    status: string;
    result: Record<string, unknown> | null;
  } | null>(null);
  useEffect(() => {
    if (rs) setText(JSON.stringify(rs.definition, null, 2));
  }, [rs]);
  const parsed = useMemo(() => {
    try {
      return { ok: true as const, v: JSON.parse(text) as unknown };
    } catch {
      return { ok: false as const };
    }
  }, [text]);
  const call = useCallback(
    async <T,>(path: Path, method: string, body: unknown, headers?: Record<string, string>) => {
      setMsg(null);
      try {
        return await api.json<T>(path, {
          method,
          json: body,
          headers,
          idempotent: method === 'POST',
        });
      } catch (e) {
        if (e instanceof ApiError) {
          const d = e.details as { errors?: VErr[] } | undefined;
          if (d?.errors) setErrs(d.errors);
        }
        setMsg(errText(api, e));
        return null;
      }
    },
    [api],
  );
  async function validate() {
    if (!parsed.ok) return setMsg(api.t('anticheat.ui.json_error'));
    const r = await call<{ valid: boolean; errors: VErr[] }>(`${BASE}/admin/rulesets:validate`, 'POST', { definition: parsed.v });
    if (r) {
      setErrs(r.errors);
      if (r.valid) setMsg(api.t('anticheat.ui.valid'));
    }
  }
  async function save() {
    if (!parsed.ok || !rs) return setMsg(api.t('anticheat.ui.json_error'));
    if (rs.status === 'draft') {
      const r = await call(`${BASE}/admin/rulesets/${version}`, 'PUT', { definition: parsed.v }, { 'If-Match': `"${rs.checksum}"` });
      if (r) reload();
    } else {
      const r = await call<{ version: number }>(`${BASE}/admin/rulesets`, 'POST', { definition: parsed.v, based_on: version }, undefined);
      if (r) api.navigate(`/admin/p/anticheat/rulesets/${r.version}`);
    }
  }
  async function simulate() {
    const r = await call<{ id: string }>(`${BASE}/admin/rulesets/${version}/simulations`, 'POST', { window_days: 30 });
    if (!r) return;
    for (let i = 0; i < 120; i++) {
      const s = await api.json<{
        id: string;
        status: string;
        result: Record<string, unknown> | null;
      }>(`${BASE}/admin/simulations/${r.id}`);
      setSim(s);
      if (s.status !== 'running') break;
      await new Promise((ok) => setTimeout(ok, 1000));
    }
  }
  async function activate() {
    if (!window.confirm(`${api.t('anticheat.ui.activate')} v${version}?`)) return;
    const r = await call(`${BASE}/admin/rulesets/${version}/activations`, 'POST', { note: '' }, undefined);
    if (r) reload();
  }
  if (!rs) return err ? <p className="pt-error">{err}</p> : null;
  return (
    <Card
      title={
        <>
          v{rs.version} <Badge tone={rs.status === 'active' ? 'success' : 'muted'}>{rs.status}</Badge>
        </>
      }
      actions={
        <Button variant="ghost" onClick={() => api.navigate('/admin/p/anticheat/?tab=rulesets')}>
          {api.t('anticheat.ui.back')}
        </Button>
      }
    >
      <textarea
        className="pt-input"
        style={{
          width: '100%',
          minHeight: 480,
          fontFamily: 'monospace',
          fontSize: 12,
        }}
        spellCheck={false}
        value={text}
        onChange={(e) => setText(e.target.value)}
        aria-label="ruleset JSON"
      />
      {!parsed.ok && <p className="pt-error">{api.t('anticheat.ui.json_error')}</p>}
      <div className="pt-row">
        <Button onClick={() => void validate()}>{api.t('anticheat.ui.validate')}</Button>
        <Button onClick={() => void save()}>{rs.status === 'draft' ? api.t('anticheat.ui.save') : api.t('anticheat.ui.new_draft')}</Button>
        {rs.status !== 'active' && <Button onClick={() => void simulate()}>{api.t('anticheat.ui.simulate')}</Button>}
        {rs.status !== 'active' && (
          <Button variant="danger" onClick={() => void activate()}>
            {api.t('anticheat.ui.activate')}
          </Button>
        )}
      </div>
      {msg && <p className={errs?.length ? 'pt-error' : 'pt-muted'}>{msg}</p>}
      {errs && errs.length > 0 && (
        <ul>
          {errs.map((e, i) => (
            <li key={i} className="pt-error">
              <code>{e.path || '/'}</code> {e.code}: {e.message}
            </li>
          ))}
        </ul>
      )}
      {sim && (
        <Card title={`simulation ${sim.status}`}>
          <pre style={{ whiteSpace: 'pre-wrap', fontSize: 12 }}>{JSON.stringify(sim.result, null, 2)}</pre>
        </Card>
      )}
    </Card>
  );
}

function AdminHome({ api }: { api: PluginApi }) {
  const initial = new URLSearchParams(window.location.search).get('tab') ?? 'queue';
  const [tab, setTab] = useState(initial);
  return (
    <div className="pt-stack">
      <Tabs
        tabs={['queue', 'appeals', 'watchlist', 'rulesets', 'stats'].map((id) => ({ id, label: api.t(`anticheat.ui.${id}`) }))}
        value={tab}
        onChange={setTab}
      />
      {tab === 'queue' && <Queue api={api} />}
      {tab === 'appeals' && <Appeals api={api} />}
      {tab === 'watchlist' && <Watchlist api={api} />}
      {tab === 'rulesets' && <Rulesets api={api} />}
      {tab === 'stats' && <Stats api={api} />}
    </div>
  );
}

// ---------------- 插槽 ----------------

interface Dash {
  open_cases: number;
  over_sla: number;
  oldest_waiting_hours: number;
  opened_24h: number;
  resolved_24h: number;
  appeals_pending: number;
  effects_failed: number;
  breaker: { tripped: boolean; since: string | null };
}

function Widget({ api }: SlotProps) {
  const [d, err, reload] = useLoad<Dash>(api, `${BASE}/admin/dashboard`);
  if (err || !d) return null;
  return (
    <Card
      title={
        <Link api={api} to="/admin/p/anticheat/">
          {api.term('anticheat.name')?.name ?? 'anticheat'}
        </Link>
      }
    >
      <div className="pt-row">
        <Stat label={api.t('anticheat.ui.open_cases')} value={d.open_cases} hint={d.over_sla ? `${api.t('anticheat.ui.over_sla')} ${d.over_sla}` : undefined} />
        <Stat label={api.t('anticheat.ui.oldest')} value={d.oldest_waiting_hours} />
        <Stat label={api.t('anticheat.ui.appeals_pending')} value={d.appeals_pending} />
        <Stat label="24h" value={`+${d.opened_24h} / ✓${d.resolved_24h}`} />
      </div>
      {d.breaker.tripped && (
        <p className="pt-error">
          {api.t('anticheat.ui.breaker')} {d.breaker.since ? when(d.breaker.since) : ''}{' '}
          <Button
            variant="danger"
            onClick={() => {
              void api
                .json(`${BASE}/admin/breaker/resets`, {
                  method: 'POST',
                  json: { note: 'dashboard' },
                })
                .then(reload)
                .catch((e) => api.toast(errText(api, e), 'error'));
            }}
          >
            {api.t('anticheat.ui.reset_breaker')}
          </Button>
        </p>
      )}
    </Card>
  );
}

function StaffMenu({ api }: SlotProps) {
  const [d] = useLoad<Dash>(api, api.user() ? `${BASE}/admin/dashboard` : null);
  if (!d) return null;
  return (
    <Link api={api} to="/admin/p/anticheat/">
      {api.t('anticheat.ui.menu', { n: d.open_cases + d.appeals_pending })}
    </Link>
  );
}

function UserTab({ api, ctx }: SlotProps<{ profile: { id: number } }>) {
  const uid = ctx.profile?.id;
  const [risk] = useLoad<{
    score: number;
    band: string;
    open_case_id: number | null;
    transfer_blocked: boolean;
    breakdown: CaseDetail['breakdown'];
  }>(api, uid ? `${BASE}/admin/users/${uid}/risk` : null);
  const [ev] = useLoad<{
    items: {
      id: number;
      kind: string;
      severity: string | null;
      hits: number;
      occurred_at: string;
      verdict: string | null;
    }[];
  }>(api, uid ? `${BASE}/admin/users/${uid}/evidence?limit=50` : null);
  const [sanc, , reload] = useLoad<{ items: Sanction[] }>(api, uid ? `${BASE}/admin/users/${uid}/sanctions` : null);
  const [cases] = useLoad<{ items: CaseRow[] }>(api, uid ? `${BASE}/admin/cases?user_id=${uid}&states=open,resolved,dismissed,overturned,closed_stale` : null);
  const [reason, setReason] = useState('');
  if (!risk) return null;
  async function openCase() {
    try {
      const r = await api.json<{ id: number }>(`${BASE}/admin/cases`, {
        method: 'POST',
        json: { user_id: uid, reason_note: reason },
        idempotent: true,
      });
      api.navigate(`/admin/p/anticheat/cases/${r.id}`);
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  }
  return (
    <Card title={api.t('anticheat.ui.risk')}>
      <div className="pt-row">
        <Stat label={api.t('anticheat.ui.score')} value={risk.score} />
        <Stat label={api.t('anticheat.ui.band')} value={<Badge tone={bandTone(risk.band)}>{risk.band}</Badge>} />
        {risk.open_case_id && (
          <Stat
            label="case"
            value={
              <Link api={api} to={`/admin/p/anticheat/cases/${risk.open_case_id}`}>
                #{risk.open_case_id}
              </Link>
            }
          />
        )}
        {risk.transfer_blocked && <Badge tone="danger">transfer blocked</Badge>}
      </div>
      <h4>{api.t('anticheat.ui.sanctions')}</h4>
      <SanctionTable api={api} rows={sanc?.items ?? []} reload={reload} />
      <h4>{api.t('anticheat.ui.queue')}</h4>
      {(cases?.items ?? []).map((c) => (
        <p key={c.id}>
          <Link api={api} to={`/admin/p/anticheat/cases/${c.id}`}>
            #{c.id}
          </Link>{' '}
          <Badge>{c.state}</Badge> {c.score}
        </p>
      ))}
      <h4>{api.t('anticheat.ui.evidence')}</h4>
      <Table
        rows={ev?.items ?? []}
        rowKey={(r) => r.id}
        empty={<Empty>—</Empty>}
        columns={[
          { key: 'k', header: '', render: (r) => r.kind },
          { key: 's', header: '', render: (r) => r.severity ?? '' },
          { key: 'h', header: '×', render: (r) => r.hits },
          { key: 'v', header: '', render: (r) => r.verdict ?? '' },
          { key: 'a', header: '', render: (r) => when(r.occurred_at) },
        ]}
      />
      {!risk.open_case_id && (
        <div className="pt-row">
          <Input
            value={reason}
            onChange={(e) => setReason(e.target.value)}
            placeholder={api.t('anticheat.ui.reason')}
            aria-label={api.t('anticheat.ui.reason')}
          />
          <Button disabled={!reason.trim()} onClick={() => void openCase()}>
            {api.t('anticheat.ui.open_case')}
          </Button>
        </div>
      )}
    </Card>
  );
}

// ---------------- 用户 ----------------

interface UserCase {
  id: number;
  state: string;
  opened_at: string;
  resolved_at: string | null;
  categories: string[];
  torrents: { id: number; title: string | null }[];
  sanctions: { kind: string; state: string; until: string | null }[];
  appeal: { allowed: boolean; deadline: string | null; state: string | null };
}

function catLabel(api: PluginApi, k: string): string {
  return api.term(k)?.name ?? k;
}

function UserCaseCard({ api, c, onAppeal }: { api: PluginApi; c: UserCase; onAppeal?: (body: string) => Promise<void> }) {
  const [body, setBody] = useState('');
  const [msg, setMsg] = useState<string | null>(null);
  return (
    <Card title={`${api.term('anticheat.case')?.name ?? 'case'} #${c.id}`}>
      <p>
        {c.categories.map((k) => (
          <Badge key={k} tone="danger">
            {catLabel(api, k)}
          </Badge>
        ))}{' '}
        <span className="pt-muted">{when(c.resolved_at ?? c.opened_at)}</span>
      </p>
      <ul>
        {c.sanctions.map((s, i) => (
          <li key={i}>
            {kindLabel(api, s.kind)} · {s.state}
            {s.until ? ` · ${when(s.until)}` : ''}
          </li>
        ))}
      </ul>
      {c.torrents.length > 0 && (
        <p>
          {api.t('anticheat.ui.torrents')}:{' '}
          {c.torrents.map((t) => (
            <Link key={t.id} api={api} to={`/torrents/${t.id}`}>
              {t.title ?? `#${t.id}`}{' '}
            </Link>
          ))}
        </p>
      )}
      {c.appeal.state && (
        <p>
          {api.term('anticheat.appeal')?.name ?? 'appeal'}: <Badge>{c.appeal.state}</Badge>
        </p>
      )}
      {c.appeal.allowed && onAppeal && (
        <div className="pt-stack">
          <span className="pt-muted">
            {api.t('anticheat.ui.appeal_deadline')}: {when(c.appeal.deadline)}
          </span>
          <textarea
            className="pt-input"
            rows={4}
            maxLength={4000}
            value={body}
            onChange={(e) => setBody(e.target.value)}
            aria-label={api.t('anticheat.ui.appeal_body')}
            placeholder={api.t('anticheat.ui.appeal_body')}
          />
          <Button
            variant="primary"
            disabled={!body.trim()}
            onClick={() =>
              void onAppeal(body)
                .then(() => setMsg(api.t('anticheat.ui.appeal_sent')))
                .catch((e) => setMsg(errText(api, e)))
            }
          >
            {api.t('anticheat.ui.appeal')}
          </Button>
        </div>
      )}
      {msg && <p className="pt-muted">{msg}</p>}
    </Card>
  );
}

function MyCases({ api }: { api: PluginApi }) {
  const [d, err, reload] = useLoad<{ items: UserCase[] }>(api, `${BASE}/users/me/cases`);
  return (
    <div className="pt-stack">
      <h2>{api.t('anticheat.ui.my_cases')}</h2>
      {err && <p className="pt-error">{err}</p>}
      {d && d.items.length === 0 && <Empty>—</Empty>}
      {(d?.items ?? []).map((c) => (
        <UserCaseCard
          key={c.id}
          api={api}
          c={c}
          onAppeal={async (body) => {
            await api.json(`${BASE}/users/me/cases/${c.id}/appeals`, {
              method: 'POST',
              json: { body },
              idempotent: true,
            });
            reload();
          }}
        />
      ))}
    </div>
  );
}

function TokenAppeal({ api }: { api: PluginApi }) {
  // 令牌只在 URL fragment 中：读取后立即从地址栏清除
  const [token] = useState(() => new URLSearchParams(window.location.hash.slice(1)).get('t') ?? '');
  const [d, setD] = useState<{
    case: UserCase;
    token_used: boolean;
    appeal: { state: string } | null;
  } | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  useEffect(() => {
    if (window.location.hash) window.history.replaceState(null, '', window.location.pathname);
    if (!token) return;
    void api
      .json<typeof d>(`${BASE}/appeals:lookup`, {
        method: 'POST',
        json: { token },
      })
      .then(setD)
      .catch((e) => setMsg(errText(api, e)));
  }, [api, token]);
  if (!token) return <p className="pt-error">{api.t('anticheat.ui.no_token')}</p>;
  return (
    <div className="pt-stack">
      <h2>{api.t('anticheat.ui.token_appeal')}</h2>
      {msg && <p className="pt-error">{msg}</p>}
      {d && (
        <UserCaseCard
          api={api}
          c={{
            ...d.case,
            appeal: {
              ...d.case.appeal,
              allowed: d.case.appeal.allowed && !d.token_used,
            },
          }}
          onAppeal={async (body) => {
            await api.json(`${BASE}/appeals:by-token`, {
              method: 'POST',
              json: { token, body },
              idempotent: true,
            });
            setD({ ...d, token_used: true, appeal: { state: 'submitted' } });
          }}
        />
      )}
    </div>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  const c = /^\/admin\/p\/anticheat\/cases\/(\d+)/.exec(path);
  if (c?.[1]) return <Workbench api={api} id={Number(c[1])} />;
  const r = /^\/admin\/p\/anticheat\/rulesets\/(\d+)/.exec(path);
  if (r?.[1]) return <RulesetEditor api={api} version={Number(r[1])} />;
  if (path.startsWith('/admin/p/anticheat')) return <AdminHome api={api} />;
  if (path.startsWith('/p/anticheat/appeal')) return <TokenAppeal api={api} />;
  return <MyCases api={api} />;
}

export default {
  slots: {
    'admin.dashboard.widgets': { component: Widget, order: 20 },
    'admin.user.tabs': {
      component: UserTab,
      order: 30,
      title_term: 'anticheat.name',
    },
    'nav.user_menu': { component: StaffMenu, order: 82 },
  },
  routes: [
    { path: '/p/anticheat/*', component: Pages },
    { path: '/admin/p/anticheat/*', component: Pages },
  ],
} satisfies PluginFrontend;
