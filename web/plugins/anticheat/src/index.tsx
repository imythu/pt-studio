// anticheat 前端：后台复核队列 / 案件工作台 / 申诉 / 观察名单 / 规则集（编辑、校验、回测、激活）/ 规则统计、
// 仪表板小部件、用户详情的风险标签页、员工菜单；用户“我的案件”与申诉、被封禁用户的令牌申诉页。
import { Fragment, useCallback, useEffect, useMemo, useState, type ReactNode } from 'react';
import { ApiError, type Path } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Checkbox, Empty, Field, Input, Kpis, NavItem, Note, PageHead, Panel, PLink, Segmented, Sheet, SideLayout, SkeletonRows, Status, Table, Tabs, Tag, Textarea, Timeline, type Tone } from '@pt-studio/ui-kit';

const zh: Record<string, string> = {
  'anticheat.ui.queue': '复核队列',
  'anticheat.ui.appeals': '申诉',
  'anticheat.ui.watchlist': '观察名单',
  'anticheat.ui.torrent_view': '种子',
  'anticheat.ui.torrents_hint': '交叉校验发现 swarm 上传明显多于下载的种子（基线确认之后的异常）。超额按小时去重累计，被归因用户按次数排序。',
  'anticheat.ui.mismatch_hours': '异常小时',
  'anticheat.ui.excess': '超额上传',
  'anticheat.ui.attributed': '被归因用户',
  'anticheat.ui.last_at': '最近',
  'anticheat.ui.baseline': '确认基线',
  'anticheat.ui.baseline_hint': '确认已复核此种子：之前的异常不再计入种子视角，之后的异常重新计数。不影响用户证据与评分。',
  'anticheat.ui.baseline_note': '备注（可选）',
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
  'anticheat.ui.menu': '反作弊',
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
  'anticheat.ui.cancel': '取消',
  'anticheat.ui.title': '反作弊',
  'anticheat.ui.account_age': '账号天数',
  'anticheat.ui.case': '案件',
  'anticheat.ui.transfer_blocked_tag': '已禁止转账',
  'anticheat.ui.simulation': '模拟结果',
  'anticheat.ui.activate_confirm': '启用 v{v} 后新规则立即生效，当前生效的版本会被替换。确定？',
  'anticheat.ui.errors': '校验错误',
  'anticheat.ui.refresh': '刷新',
  'anticheat.ui.kind_col': '类型',
  'anticheat.ui.until': '截止',
  'anticheat.ui.source': '来源',
  'anticheat.ui.type': '类别',
  'anticheat.ui.severity': '严重度',
  'anticheat.ui.torrent': '种子',
  'anticheat.ui.details': '详情',
  'anticheat.ui.time': '时间',
  'anticheat.ui.client': '客户端',
  'anticheat.ui.via': '渠道',
  'anticheat.ui.verdict': '结论',
  'anticheat.ui.go': '进入',
  'anticheat.ui.cstate.open': '待处理',
  'anticheat.ui.cstate.resolved': '已处理',
  'anticheat.ui.cstate.dismissed': '已驳回',
  'anticheat.ui.cstate.overturned': '已推翻',
  'anticheat.ui.cstate.closed_stale': '超时关闭',
};
registerMessages('zh-CN', zh);
registerMessages('en', {
  'anticheat.ui.queue': 'Review queue',
  'anticheat.ui.appeals': 'Appeals',
  'anticheat.ui.watchlist': 'Watchlist',
  'anticheat.ui.torrent_view': 'Torrents',
  'anticheat.ui.torrents_hint': 'Torrents where cross-checks found swarm upload well above download (since the last confirmed baseline). Excess is summed once per hour; attributed users are ordered by hits.',
  'anticheat.ui.mismatch_hours': 'Mismatch hours',
  'anticheat.ui.excess': 'Excess upload',
  'anticheat.ui.attributed': 'Attributed users',
  'anticheat.ui.last_at': 'Latest',
  'anticheat.ui.baseline': 'Confirm baseline',
  'anticheat.ui.baseline_hint': 'Mark this torrent as reviewed: earlier mismatches stop counting here and later ones are counted afresh. User evidence and scores are not affected.',
  'anticheat.ui.baseline_note': 'Note (optional)',
  'anticheat.ui.rulesets': 'Rulesets',
  'anticheat.ui.stats': 'Rule stats',
  'anticheat.ui.my_cases': 'My cases',
  'anticheat.ui.token_appeal': 'Ban appeal',
  'anticheat.ui.menu': 'Anti-cheat',
  'anticheat.ui.cancel': 'Cancel',
  'anticheat.ui.title': 'Anti-cheat',
  'anticheat.ui.account_age': 'Account age',
  'anticheat.ui.case': 'Case',
  'anticheat.ui.transfer_blocked_tag': 'transfers blocked',
  'anticheat.ui.simulation': 'Simulation',
  'anticheat.ui.activate_confirm': 'Activating v{v} replaces the active ruleset immediately. Continue?',
  'anticheat.ui.errors': 'Validation errors',
  'anticheat.ui.refresh': 'Refresh',
  'anticheat.ui.kind_col': 'Kind',
  'anticheat.ui.until': 'Until',
  'anticheat.ui.source': 'Source',
  'anticheat.ui.type': 'Type',
  'anticheat.ui.severity': 'Severity',
  'anticheat.ui.torrent': 'Torrent',
  'anticheat.ui.details': 'Details',
  'anticheat.ui.time': 'Time',
  'anticheat.ui.client': 'Client',
  'anticheat.ui.via': 'Via',
  'anticheat.ui.verdict': 'Verdict',
  'anticheat.ui.go': 'Open',
  'anticheat.ui.cstate.open': 'Open',
  'anticheat.ui.cstate.resolved': 'Resolved',
  'anticheat.ui.cstate.dismissed': 'Dismissed',
  'anticheat.ui.cstate.overturned': 'Overturned',
  'anticheat.ui.cstate.closed_stale': 'Closed (stale)',
  'anticheat.transfer_blocked': 'Your account is under review; transfers are temporarily disabled',
});

const KINDS = ['warn', 'freeze_traffic', 'lock_wallet', 'adjust_traffic', 'reset_passkey', 'disable_download', 'ban'] as const;
const BASE = '/api/p/anticheat/v1' as const;

function errText(api: PluginApi, e: unknown): string {
  if (e instanceof ApiError) {
    const s = api.t(e.code);
    return s === e.code ? e.message : s;
  }
  return String(e);
}

function bandTone(b: string): Tone {
  return b === 'critical' || b === 'high' ? 'bad' : b === 'watch' ? 'warn' : 'mute';
}
const stateTone = (s: string): Tone => (s === 'active' || s === 'open' ? 'bad' : s === 'proposed' || s === 'pending' || s === 'submitted' ? 'warn' : s === 'resolved' || s === 'upheld' ? 'ok' : 'mute');

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

function Link({ api, to, children, className }: { api: PluginApi; to: string; children: ReactNode; className?: string }) {
  return (
    <PLink to={to} navigate={api.navigate} className={className}>
      {children}
    </PLink>
  );
}

const Mono = ({ children }: { children: ReactNode }) => <span className="pt-mono small">{children}</span>;
const When = ({ at }: { at: unknown }) => <span className="small muted num nowrap">{when(at)}</span>;

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

/** 取消 + 主操作的抽屉底栏 */
function Foot({ api, onCancel, children }: { api: PluginApi; onCancel(): void; children: ReactNode }) {
  return (
    <>
      <Button variant="ghost" onClick={onCancel}>
        {api.t('anticheat.ui.cancel')}
      </Button>
      {children}
    </>
  );
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

const CASE_STATES = ['open', 'resolved', 'dismissed', 'overturned', 'closed_stale'];

function Queue({ api }: { api: PluginApi }) {
  const [state, setState] = useState('open');
  const [d, err] = useLoad<{ items: CaseRow[] }>(api, `${BASE}/admin/cases?states=${state}&limit=100`);
  return (
    <>
      <Tabs variant="flush" value={state} onChange={setState} tabs={CASE_STATES.map((s) => ({ id: s, label: api.t(`anticheat.ui.cstate.${s}`) }))} />
      {err && (
        <div className="pad">
          <Note tone="bad">{err}</Note>
        </div>
      )}
      {!d && !err ? (
        <SkeletonRows n={6} />
      ) : (
        <Table
          rows={d?.items ?? []}
          rowKey={(r) => r.id}
          empty={api.t('anticheat.ui.none')}
          minWidth={820}
          onRowClick={(r) => api.navigate(`/admin/p/anticheat/cases/${r.id}`)}
          columns={[
            {
              key: 'id',
              header: '#',
              render: (r) => (
                <Link api={api} to={`/admin/p/anticheat/cases/${r.id}`}>
                  {`#${r.id}`}
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
            { key: 's', header: api.t('anticheat.ui.score'), right: true, render: (r) => <b className="num">{r.score}</b> },
            { key: 'b', header: api.t('anticheat.ui.band'), render: (r) => <Status tone={bandTone(r.band)}>{r.band}</Status> },
            { key: 'r', header: api.t('anticheat.ui.rule'), render: (r) => <Mono>{r.primary_rule}</Mono> },
            { key: 'w', header: api.t('anticheat.ui.waiting'), right: true, render: (r) => <span className={r.waiting_hours > 48 ? 'num down' : 'num'}>{`${r.waiting_hours} h`}</span> },
            { key: 'a', header: api.t('anticheat.ui.assigned'), render: (r) => <span className="small">{r.assigned_to ? `#${r.assigned_to}` : '—'}</span> },
            {
              key: 'x',
              header: api.t('anticheat.ui.auto'),
              render: (r) => (
                <div className="row">
                  {r.auto_sanctions.map((k) => (
                    <Tag key={k} tone="x">
                      {kindLabel(api, k)}
                    </Tag>
                  ))}
                </div>
              ),
            },
          ]}
        />
      )}
    </>
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

/** 处理案件（抽屉）：建议方案、处理方式、处罚组合或误报证据、备注 */
function Decide({ api, c, open, onClose, done }: { api: PluginApi; c: CaseDetail; open: boolean; onClose(): void; done: () => void }) {
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
      onClose();
      done();
    } catch (e) {
      setMsg(errText(api, e));
    }
  }
  return (
    <Sheet
      open={open}
      onClose={onClose}
      title={api.t('anticheat.ui.decide')}
      width={560}
      footer={
        <Foot api={api} onCancel={onClose}>
          <Button variant={decision === 'sanction' ? 'danger' : 'primary'} disabled={!note.trim()} onClick={() => void submit()}>
            {api.t('anticheat.ui.submit')}
          </Button>
        </Foot>
      }
    >
      <div className="form">
        {c.suggestions.length > 0 && (
          <Field label={api.t('anticheat.ui.suggestions')}>
            <div className="row">
              {c.suggestions.map((s) => (
                <Button key={s.policy_id} size="xs" onClick={() => applySuggestion(s)}>
                  {`${s.policy_id}: ${s.actions.map((a) => kindLabel(api, a.kind)).join(' + ')}`}
                </Button>
              ))}
            </div>
          </Field>
        )}
        <Segmented
          label={api.t('anticheat.ui.decide')}
          value={decision}
          onChange={(v) => setDecision(v as typeof decision)}
          options={(['sanction', 'no_action', 'dismiss'] as const).map((d) => ({ value: d, label: api.t(`anticheat.ui.decision.${d}`) }))}
        />
        {decision === 'sanction' && (
          <div className="stack-s">
            {KINDS.map((k) => {
              const v = acts[k] ?? { on: false, days: '', up: '' };
              const set = (p: Partial<typeof v>) => setActs({ ...acts, [k]: { ...v, ...p } });
              return (
                <div key={k} className="form-acts">
                  <Checkbox checked={v.on} onChange={(on) => set({ on })} label={kindLabel(api, k)} />
                  {v.on && k !== 'adjust_traffic' && k !== 'lock_wallet' && k !== 'reset_passkey' && (
                    <Input type="number" min={1} max={3650} placeholder={api.t('anticheat.ui.days')} value={v.days} onChange={(e) => set({ days: e.target.value })} aria-label={api.t('anticheat.ui.days')} />
                  )}
                  {v.on && k === 'adjust_traffic' && (
                    <>
                      <Input type="number" min={1} placeholder={api.t('anticheat.ui.up_bytes')} value={v.up} onChange={(e) => set({ up: e.target.value })} aria-label={api.t('anticheat.ui.up_bytes')} />
                      {v.up && <span className="small muted num">{bytes(v.up)}</span>}
                    </>
                  )}
                </div>
              );
            })}
          </div>
        )}
        {decision === 'dismiss' && (
          <div className="stack-s">
            {c.evidence.map((e) => (
              <Checkbox key={e.id} checked={fp.includes(e.id)} onChange={(on) => setFp(on ? [...fp, e.id] : fp.filter((i) => i !== e.id))} label={`${api.t('anticheat.ui.fp')} #${e.id} ${e.kind}`} />
            ))}
          </div>
        )}
        <Field label={api.t('anticheat.ui.note')}>
          <Textarea rows={3} maxLength={2000} value={note} onChange={(e) => setNote(e.target.value)} />
        </Field>
        {msg && <Note tone="bad">{msg}</Note>}
      </div>
    </Sheet>
  );
}

/** 处罚列表；解除在抽屉里填写原因 */
function SanctionTable({ api, rows, reload }: { api: PluginApi; rows: Sanction[]; reload: () => void }) {
  const [lifting, setLifting] = useState<Sanction | null>(null);
  const [reason, setReason] = useState('');
  async function act(path: Path, body: unknown) {
    try {
      await api.json(path, { method: 'POST', json: body, idempotent: true });
      reload();
      return true;
    } catch (e) {
      api.toast(errText(api, e), 'error');
      return false;
    }
  }
  return (
    <>
      <Table
        rows={rows}
        rowKey={(r) => r.id}
        empty={api.t('anticheat.ui.none')}
        columns={[
          {
            key: 'k',
            header: api.t('anticheat.ui.kind_col'),
            render: (r) => (
              <>
                {kindLabel(api, r.kind)} {r.external_override && <Tag tone="o">{api.t('anticheat.ui.override')}</Tag>}
              </>
            ),
          },
          { key: 's', header: api.t('anticheat.ui.state'), render: (r) => <Status tone={stateTone(r.state)}>{r.state}</Status> },
          { key: 'u', header: api.t('anticheat.ui.until'), render: (r) => (r.until ? <When at={r.until} /> : '∞') },
          { key: 'p', header: api.t('anticheat.ui.source'), render: (r) => (r.policy_id ? <Mono>{r.policy_id}</Mono> : r.decided_by ? <span className="small">{`#${r.decided_by}`}</span> : '') },
          {
            key: 'a',
            header: '',
            right: true,
            render: (r) => (
              <div className="form-acts">
                {r.state === 'proposed' && (
                  <Button size="xs" variant="primary" onClick={() => void act(`${BASE}/admin/sanctions/${r.id}/approvals`, {})}>
                    {api.t('anticheat.ui.approve')}
                  </Button>
                )}
                {['proposed', 'pending', 'active', 'suspended'].includes(r.state) && (
                  <Button size="xs" variant="ghost" onClick={() => setLifting(r)}>
                    {api.t('anticheat.ui.lift')}
                  </Button>
                )}
              </div>
            ),
          },
        ]}
      />
      <Sheet
        open={lifting !== null}
        onClose={() => setLifting(null)}
        title={lifting ? `${api.t('anticheat.ui.lift')} · ${kindLabel(api, lifting.kind)}` : ''}
        footer={
          <Foot api={api} onCancel={() => setLifting(null)}>
            <Button
              variant="primary"
              disabled={!reason.trim()}
              onClick={() =>
                lifting &&
                void act(`${BASE}/admin/sanctions/${lifting.id}/lifts`, { reason_note: reason.trim() }).then((ok) => {
                  if (ok) {
                    setLifting(null);
                    setReason('');
                  }
                })
              }
            >
              {api.t('anticheat.ui.lift')}
            </Button>
          </Foot>
        }
      >
        <Field label={api.t('anticheat.ui.reason')}>
          <Input name="lift_reason" value={reason} maxLength={500} onChange={(e) => setReason(e.target.value)} />
        </Field>
      </Sheet>
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
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);
  async function ask() {
    try {
      await api.json(`${BASE}/admin/cases/${id}/explanations`, { method: 'POST', json: {} });
      setMsg(api.t('anticheat.ui.explain_queued'));
    } catch (e) {
      setMsg(errText(api, e));
    }
  }
  return (
    <Panel
      title={api.t('anticheat.ui.explain')}
      actions={
        <Button size="xs" variant="ghost" onClick={() => void (text ? load() : ask())}>
          {text ? api.t('anticheat.ui.refresh') : api.t('anticheat.ui.explain')}
        </Button>
      }
    >
      {text ? <p className="pt-prewrap small">{text}</p> : <p className="small muted">{msg ?? '—'}</p>}
    </Panel>
  );
}

function Workbench({ api, id }: { api: PluginApi; id: number }) {
  const [c, err, reload] = useLoad<CaseDetail>(api, `${BASE}/admin/cases/${id}`);
  const [note, setNote] = useState('');
  const [deciding, setDeciding] = useState(false);
  const me = api.user();
  async function post(path: Path, body: unknown, method = 'POST') {
    try {
      await api.fetch(path, { method, json: body });
      reload();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  }
  if (!c) return err ? <Note tone="bad">{err}</Note> : <SkeletonRows n={8} />;
  const mine = c.assigned_to === me?.id;
  return (
    <div className="stack">
      <PageHead
        crumb={
          <>
            <Link api={api} to="/admin/p/anticheat/">
              {api.t('anticheat.ui.queue')}
            </Link>
            <span>{`#${c.id}`}</span>
          </>
        }
        title={
          <>
            {`#${c.id} · `}
            <Link api={api} to={`/admin/users/${c.user.id}`}>
              {c.user.username ?? `#${c.user.id}`}
            </Link>{' '}
            <Status tone={bandTone(c.band)}>{c.band}</Status>
          </>
        }
        lede={
          <>
            <Mono>{c.primary_rule}</Mono>
            {c.resolution_note && ` · ${c.resolution_note}`}
          </>
        }
        actions={
          <>
            {c.state === 'open' &&
              (mine ? (
                <Button onClick={() => void post(`${BASE}/admin/cases/${id}/claims`, undefined, 'DELETE')}>{api.t('anticheat.ui.release')}</Button>
              ) : (
                <Button onClick={() => void post(`${BASE}/admin/cases/${id}/claims`, {})}>{api.t('anticheat.ui.claim')}</Button>
              ))}
            {c.state === 'open' && (
              <Button variant="primary" onClick={() => setDeciding(true)}>
                {api.t('anticheat.ui.decide')}
              </Button>
            )}
          </>
        }
      />
      <Kpis
        items={[
          { label: api.t('anticheat.ui.score'), value: `${c.score} / ${c.peak_score}`, tone: c.band === 'high' || c.band === 'critical' ? 'down' : undefined },
          { label: api.t('anticheat.ui.state'), value: c.resolution ? `${c.state} · ${c.resolution}` : c.state },
          { label: api.t('anticheat.ui.uploaded'), value: bytes(c.traffic.uploaded), tone: 'up' },
          { label: api.t('anticheat.ui.downloaded'), value: bytes(c.traffic.downloaded) },
          { label: api.t('anticheat.ui.assigned'), value: c.assigned_to ? `#${c.assigned_to}` : '—', hint: c.claim_expires_at ? when(c.claim_expires_at) : undefined },
          { label: api.t('anticheat.ui.account_age'), value: c.user.account_age_days ?? '—', hint: api.t('anticheat.ui.days') },
        ]}
      />
      <SideLayout
        width={380}
        side={
          <>
            <Panel title={api.t('anticheat.ui.sanctions')} flush>
              <SanctionTable api={api} rows={c.sanctions} reload={reload} />
            </Panel>
            <Explain api={api} id={c.id} />
            <Panel title={api.t('anticheat.ui.notes')}>
              <div className="stack">
                {c.notes.map((n, i) => (
                  <div key={i} className="stack-s">
                    <span className="tiny muted">{`#${n.by} · ${when(n.at)}`}</span>
                    <span className="small pt-prewrap">{n.text}</span>
                  </div>
                ))}
                <form
                  className="form-acts"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void post(`${BASE}/admin/cases/${id}/notes`, { text: note });
                    setNote('');
                  }}
                >
                  <Input value={note} maxLength={2000} onChange={(e) => setNote(e.target.value)} aria-label={api.t('anticheat.ui.add_note')} placeholder={api.t('anticheat.ui.add_note')} />
                  <Button type="submit" size="sm" disabled={!note.trim()}>
                    {api.t('anticheat.ui.add_note')}
                  </Button>
                </form>
              </div>
            </Panel>
          </>
        }
      >
        <Panel title={api.t('anticheat.ui.breakdown')} flush primary>
          <Table
            rows={c.breakdown}
            rowKey={(r) => r.rule}
            empty="—"
            columns={[
              { key: 'r', header: api.t('anticheat.ui.rule'), render: (r) => <Mono>{r.rule}</Mono> },
              { key: 't', header: api.t('anticheat.ui.type'), render: (r) => <span className="small">{`${r.type}${r.mode !== 'enforce' ? ` (${r.mode})` : ''}`}</span> },
              { key: 's', header: api.t('anticheat.ui.score'), right: true, render: (r) => <b className="num">{r.factor ? `× ${r.factor}` : r.score}</b> },
              { key: 'e', header: api.t('anticheat.ui.evidence'), right: true, render: (r) => <span className="num">{r.evidence ?? ''}</span> },
              {
                key: 'x',
                header: api.t('anticheat.ui.torrents'),
                render: (r) => (
                  <div className="row">
                    {(r.top_torrents ?? []).map((t) => (
                      <Link key={t} api={api} to={`/torrents/${t}`} className="small">
                        {`#${t}`}
                      </Link>
                    ))}
                  </div>
                ),
              },
            ]}
          />
        </Panel>
        <Panel title={api.t('anticheat.ui.evidence')} flush>
          <Table
            rows={c.evidence}
            rowKey={(r) => r.id}
            empty="—"
            minWidth={820}
            columns={[
              { key: 'k', header: api.t('anticheat.ui.kind_col'), render: (r) => <span className={r.false_positive ? 'muted strike' : undefined}>{r.kind}</span> },
              { key: 's', header: api.t('anticheat.ui.severity'), render: (r) => (r.severity ? <Status tone={r.severity === 'high' ? 'bad' : 'mute'}>{r.severity}</Status> : null) },
              { key: 'h', header: '×', right: true, render: (r) => <span className="num">{r.hits}</span> },
              {
                key: 't',
                header: api.t('anticheat.ui.torrent'),
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
                header: api.t('anticheat.ui.details'),
                render: (r) => (
                  <span className="pt-mono tiny muted">
                    {Object.entries(r.details)
                      .map(([k, v]) => `${k}=${typeof v === 'number' && /bytes|uploaded|downloaded|speed/.test(k) ? bytes(v) : String(v)}`)
                      .join(' ')}
                  </span>
                ),
              },
              { key: 'at', header: api.t('anticheat.ui.time'), render: (r) => <When at={r.occurred_at} /> },
            ]}
          />
        </Panel>
        {c.swarm_snapshots.map((s) => (
          <Panel key={`${s.torrent_id}-${s.taken_at}`} title={`${api.t('anticheat.ui.swarm')} #${s.torrent_id}`} sub={when(s.taken_at)} flush>
            <Table
              rows={s.peers}
              rowKey={(p) => `${p.user_id}-${p.client}`}
              selected={`${c.user.id}-${s.peers.find((p) => p.user_id === c.user.id)?.client ?? ''}`}
              columns={[
                { key: 'u', header: api.t('anticheat.ui.user'), render: (p) => (p.user_id === c.user.id ? <b>{`#${p.user_id}`}</b> : `#${p.user_id}`) },
                { key: 'c', header: api.t('anticheat.ui.client'), render: (p) => <Mono>{p.client ?? ''}</Mono> },
                { key: 's', header: '', render: (p) => <Tag tone={p.seeder ? 'g' : undefined}>{p.seeder ? 'seed' : 'leech'}</Tag> },
                { key: 'up', header: api.t('anticheat.ui.uploaded'), right: true, render: (p) => <span className="num up">{bytes(p.uploaded)}</span> },
                { key: 'dn', header: api.t('anticheat.ui.downloaded'), right: true, render: (p) => <span className="num down">{bytes(p.downloaded)}</span> },
              ]}
            />
          </Panel>
        ))}
        <Panel title={api.t('anticheat.ui.timeline')}>
          <Timeline
            items={c.timeline.map((e, i) => ({
              key: i,
              time: when(e.at),
              children: (
                <>
                  <b>{e.kind}</b> {e.actor ? `#${e.actor}` : ''} <span className="pt-mono tiny muted">{JSON.stringify(e.data)}</span>
                </>
              ),
            }))}
          />
        </Panel>
      </SideLayout>
      {c.state === 'open' && <Decide api={api} c={c} open={deciding} onClose={() => setDeciding(false)} done={reload} />}
    </div>
  );
}

// ---------------- 后台：申诉 ----------------

interface AppealRow {
  id: number;
  case_id: number;
  user: { id: number; username: string | null };
  state: string;
  via: string;
  body: string | null;
  created_at: string;
}

function Appeals({ api }: { api: PluginApi }) {
  const [d, err, reload] = useLoad<{ items: AppealRow[] }>(api, `${BASE}/admin/appeals`);
  const [open, setOpen] = useState<AppealRow | null>(null);
  const [outcome, setOutcome] = useState<'upheld' | 'overturned'>('upheld');
  const [note, setNote] = useState('');
  const [comp, setComp] = useState('');
  const [msg, setMsg] = useState<string | null>(null);
  async function decide() {
    if (!open) return;
    setMsg(null);
    try {
      await api.json(`${BASE}/admin/appeals/${open.id}/claims`, { method: 'POST', json: {} });
      await api.json(`${BASE}/admin/appeals/${open.id}/decisions`, {
        method: 'POST',
        json: { outcome, note, ...(outcome === 'overturned' && comp ? { compensate_bytes: Number(comp) } : {}) },
        idempotent: true,
      });
      setOpen(null);
      setNote('');
      setComp('');
      reload();
    } catch (e) {
      setMsg(errText(api, e));
    }
  }
  return (
    <>
      {err && (
        <div className="pad">
          <Note tone="bad">{err}</Note>
        </div>
      )}
      {!d && !err ? (
        <SkeletonRows n={4} />
      ) : (
        <Table
          rows={d?.items ?? []}
          rowKey={(r) => r.id}
          empty={api.t('anticheat.ui.none')}
          minWidth={820}
          columns={[
            {
              key: 'c',
              header: api.t('anticheat.ui.case'),
              render: (a) => (
                <Link api={api} to={`/admin/p/anticheat/cases/${a.case_id}`}>
                  {`#${a.case_id}`}
                </Link>
              ),
            },
            { key: 'u', header: api.t('anticheat.ui.user'), render: (a) => a.user.username ?? `#${a.user.id}` },
            { key: 'b', header: api.t('anticheat.ui.appeal_body'), render: (a) => <span className="small clip2">{a.body}</span> },
            { key: 's', header: api.t('anticheat.ui.state'), render: (a) => <Status tone={stateTone(a.state)}>{a.state}</Status> },
            { key: 'v', header: api.t('anticheat.ui.via'), render: (a) => <Tag>{a.via}</Tag> },
            { key: 't', header: api.t('anticheat.ui.time'), render: (a) => <When at={a.created_at} /> },
            {
              key: 'x',
              header: '',
              right: true,
              render: (a) => (
                <Button size="xs" onClick={() => setOpen(a)}>
                  {api.t('anticheat.ui.decide')}
                </Button>
              ),
            },
          ]}
        />
      )}
      <Sheet
        open={open !== null}
        onClose={() => setOpen(null)}
        title={open ? `${api.t('anticheat.ui.appeals')} · #${open.case_id}` : ''}
        footer={
          <Foot api={api} onCancel={() => setOpen(null)}>
            <Button variant="primary" disabled={!note.trim()} onClick={() => void decide()}>
              {api.t('anticheat.ui.submit')}
            </Button>
          </Foot>
        }
      >
        {open && (
          <div className="form">
            <p className="small muted">{`${open.user.username ?? `#${open.user.id}`} · ${open.via} · ${when(open.created_at)}`}</p>
            <blockquote className="pt-prewrap small">{open.body}</blockquote>
            <Segmented label="outcome" value={outcome} onChange={(v) => setOutcome(v as typeof outcome)} options={(['upheld', 'overturned'] as const).map((o) => ({ value: o, label: api.t(`anticheat.ui.outcome.${o}`) }))} />
            {outcome === 'overturned' && (
              <Field label={api.t('anticheat.ui.compensate')} hint={comp ? bytes(comp) : undefined}>
                <Input type="number" min={0} value={comp} onChange={(e) => setComp(e.target.value)} />
              </Field>
            )}
            <Field label={api.t('anticheat.ui.note')}>
              <Textarea rows={3} value={note} onChange={(e) => setNote(e.target.value)} />
            </Field>
            {msg && <Note tone="bad">{msg}</Note>}
          </div>
        )}
      </Sheet>
    </>
  );
}

function Watchlist({ api }: { api: PluginApi }) {
  const [d] = useLoad<{ items: { user: { id: number; username: string | null }; score: number; band: string }[] }>(api, `${BASE}/admin/watchlist`);
  if (!d) return <SkeletonRows n={4} />;
  return (
    <Table
      rows={d.items}
      rowKey={(r) => r.user.id}
      empty={api.t('anticheat.ui.none')}
      onRowClick={(r) => api.navigate(`/admin/users/${r.user.id}`)}
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
        { key: 'b', header: api.t('anticheat.ui.band'), render: (r) => <Status tone={bandTone(r.band)}>{r.band}</Status> },
        { key: 's', header: api.t('anticheat.ui.score'), right: true, render: (r) => <b className="num">{r.score}</b> },
      ]}
    />
  );
}

interface SuspiciousTorrent {
  torrent: { id: number; title: string | null };
  mismatch_hours: number;
  excess_bytes: number;
  last_at: string;
  baseline_at: string | null;
  user_count: number;
  users: { id: number; username: string | null; hits: number; uploaded: number }[];
}

/** 种子视角（FR-ACH-042）：被刷种子与被归因用户，复核后确认基线 */
function Torrents({ api }: { api: PluginApi }) {
  const [days, setDays] = useState('30');
  const [d, , reload] = useLoad<{ items: SuspiciousTorrent[] }>(api, `${BASE}/admin/torrents/suspicious?days=${days}`);
  const [confirm, setConfirm] = useState<SuspiciousTorrent | null>(null);
  const [note, setNote] = useState('');
  const [msg, setMsg] = useState<string | null>(null);
  async function submit() {
    if (!confirm) return;
    setMsg(null);
    try {
      await api.json(`${BASE}/admin/torrents/${confirm.torrent.id}/baselines`, { method: 'POST', json: note.trim() ? { note: note.trim() } : {}, idempotent: true });
      setConfirm(null);
      setNote('');
      reload();
    } catch (e) {
      setMsg(errText(api, e));
    }
  }
  return (
    <div className="stack-s">
      <div className="row">
        <span className="small muted">{api.t('anticheat.ui.torrents_hint')}</span>
        <Segmented label={api.t('anticheat.ui.days')} value={days} onChange={setDays} options={['7', '30', '90'].map((v) => ({ value: v, label: `${v}d` }))} />
      </div>
      {!d ? (
        <SkeletonRows n={4} />
      ) : (
        <Table
          rows={d.items}
          rowKey={(r) => r.torrent.id}
          empty={api.t('anticheat.ui.none')}
          minWidth={820}
          columns={[
            {
              key: 't',
              header: api.t('anticheat.ui.torrent'),
              render: (r) => (
                <Link api={api} to={`/torrents/${r.torrent.id}`}>
                  {r.torrent.title ?? `#${r.torrent.id}`}
                </Link>
              ),
            },
            { key: 'h', header: api.t('anticheat.ui.mismatch_hours'), right: true, render: (r) => <b className="num">{r.mismatch_hours}</b> },
            { key: 'x', header: api.t('anticheat.ui.excess'), right: true, render: (r) => <span className="num">{bytes(r.excess_bytes)}</span> },
            {
              key: 'u',
              header: api.t('anticheat.ui.attributed'),
              render: (r) => (
                <span className="small">
                  {r.users.slice(0, 5).map((u, i) => (
                    <Fragment key={u.id}>
                      {i > 0 && '、'}
                      <Link api={api} to={`/admin/users/${u.id}`}>
                        {u.username ?? `#${u.id}`}
                      </Link>
                      <span className="muted num"> ×{u.hits}</span>
                    </Fragment>
                  ))}
                  {r.user_count > 5 && <span className="muted"> +{r.user_count - 5}</span>}
                </span>
              ),
            },
            { key: 'l', header: api.t('anticheat.ui.last_at'), render: (r) => <span className="small muted">{when(r.last_at)}</span> },
            {
              key: 'a',
              header: '',
              right: true,
              render: (r) => (
                <Button size="sm" onClick={() => setConfirm(r)}>
                  {api.t('anticheat.ui.baseline')}
                </Button>
              ),
            },
          ]}
        />
      )}
      <Sheet
        open={!!confirm}
        onClose={() => setConfirm(null)}
        title={api.t('anticheat.ui.baseline')}
        width={480}
        footer={
          <Foot api={api} onCancel={() => setConfirm(null)}>
            <Button variant="primary" onClick={() => void submit()}>
              {api.t('anticheat.ui.submit')}
            </Button>
          </Foot>
        }
      >
        <div className="form">
          <p>
            <b>{confirm?.torrent.title ?? `#${confirm?.torrent.id ?? ''}`}</b>
          </p>
          <Note tone="info">{api.t('anticheat.ui.baseline_hint')}</Note>
          <Field label={api.t('anticheat.ui.baseline_note')}>
            <Textarea rows={2} maxLength={500} value={note} onChange={(e) => setNote(e.target.value)} />
          </Field>
          {msg && <Note tone="bad">{msg}</Note>}
        </div>
      </Sheet>
    </div>
  );
}

function Stats({ api }: { api: PluginApi }) {
  const [d] = useLoad<{
    items: { rule_id: string; hits: number; cases_opened: number; confirmed: number; dismissed: number; overturned: number; precision: number | null; low_precision: boolean }[];
  }>(api, `${BASE}/admin/rule-stats?days=90`);
  if (!d) return <SkeletonRows n={4} />;
  const n = (v: number) => <span className="num">{v}</span>;
  return (
    <Table
      rows={d.items}
      rowKey={(r) => r.rule_id}
      empty={api.t('anticheat.ui.none')}
      minWidth={820}
      columns={[
        { key: 'r', header: api.t('anticheat.ui.rule'), render: (r) => <Mono>{r.rule_id}</Mono> },
        { key: 'h', header: api.t('anticheat.ui.hits'), right: true, render: (r) => n(r.hits) },
        { key: 'o', header: api.t('anticheat.ui.opened'), right: true, render: (r) => n(r.cases_opened) },
        { key: 'c', header: api.t('anticheat.ui.confirmed'), right: true, render: (r) => n(r.confirmed) },
        { key: 'd', header: api.t('anticheat.ui.dismissed'), right: true, render: (r) => n(r.dismissed) },
        { key: 'v', header: api.t('anticheat.ui.overturned'), right: true, render: (r) => n(r.overturned) },
        {
          key: 'p',
          header: api.t('anticheat.ui.precision'),
          right: true,
          render: (r) => (r.precision == null ? '—' : <Status tone={r.low_precision ? 'bad' : 'ok'}>{`${(r.precision * 100).toFixed(0)}%`}</Status>),
        },
      ]}
    />
  );
}

// ---------------- 后台：规则集 ----------------

interface VErr {
  path: string;
  code: string;
  message: string;
}

function Rulesets({ api }: { api: PluginApi }) {
  const [d] = useLoad<{ items: { version: number; status: string; note: string | null; created_at: string; activated_at: string | null }[] }>(api, `${BASE}/admin/rulesets`);
  if (!d) return <SkeletonRows n={4} />;
  return (
    <Table
      rows={d.items}
      rowKey={(r) => r.version}
      empty={api.t('anticheat.ui.none')}
      onRowClick={(r) => api.navigate(`/admin/p/anticheat/rulesets/${r.version}`)}
      columns={[
        {
          key: 'v',
          header: api.t('anticheat.ui.version'),
          render: (r) => (
            <Link api={api} to={`/admin/p/anticheat/rulesets/${r.version}`}>
              {`v${r.version}`}
            </Link>
          ),
        },
        { key: 's', header: api.t('anticheat.ui.state'), render: (r) => <Status tone={r.status === 'active' ? 'ok' : r.status === 'draft' ? 'warn' : 'mute'}>{r.status}</Status> },
        { key: 'n', header: api.t('anticheat.ui.note'), render: (r) => <span className="small">{r.note ?? ''}</span> },
        { key: 'c', header: api.t('anticheat.ui.time'), right: true, render: (r) => <When at={r.activated_at ?? r.created_at} /> },
      ]}
    />
  );
}

function RulesetEditor({ api, version }: { api: PluginApi; version: number }) {
  const [rs, err, reload] = useLoad<{ version: number; status: string; definition: unknown; checksum: string; note: string | null }>(api, `${BASE}/admin/rulesets/${version}`);
  const [text, setText] = useState('');
  const [errs, setErrs] = useState<VErr[] | null>(null);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [sim, setSim] = useState<{ id: string; status: string; result: Record<string, unknown> | null } | null>(null);
  const [activating, setActivating] = useState(false);
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
        return await api.json<T>(path, { method, json: body, headers, idempotent: method === 'POST' });
      } catch (e) {
        if (e instanceof ApiError) {
          const d = e.details as { errors?: VErr[] } | undefined;
          if (d?.errors) setErrs(d.errors);
        }
        setMsg({ ok: false, text: errText(api, e) });
        return null;
      }
    },
    [api],
  );
  async function validate() {
    if (!parsed.ok) return setMsg({ ok: false, text: api.t('anticheat.ui.json_error') });
    const r = await call<{ valid: boolean; errors: VErr[] }>(`${BASE}/admin/rulesets:validate`, 'POST', { definition: parsed.v });
    if (r) {
      setErrs(r.errors);
      if (r.valid) setMsg({ ok: true, text: api.t('anticheat.ui.valid') });
    }
  }
  async function save() {
    if (!parsed.ok || !rs) return setMsg({ ok: false, text: api.t('anticheat.ui.json_error') });
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
      const s = await api.json<{ id: string; status: string; result: Record<string, unknown> | null }>(`${BASE}/admin/simulations/${r.id}`);
      setSim(s);
      if (s.status !== 'running') break;
      await new Promise((ok) => setTimeout(ok, 1000));
    }
  }
  async function activate() {
    const r = await call(`${BASE}/admin/rulesets/${version}/activations`, 'POST', { note: '' }, undefined);
    setActivating(false);
    if (r) reload();
  }
  if (!rs) return err ? <Note tone="bad">{err}</Note> : <SkeletonRows n={8} />;
  return (
    <div className="stack">
      <PageHead
        crumb={
          <>
            <Link api={api} to="/admin/p/anticheat/?tab=rulesets">
              {api.t('anticheat.ui.rulesets')}
            </Link>
            <span>{`v${rs.version}`}</span>
          </>
        }
        title={
          <>
            {`v${rs.version} `}
            <Status tone={rs.status === 'active' ? 'ok' : rs.status === 'draft' ? 'warn' : 'mute'}>{rs.status}</Status>
          </>
        }
        lede={rs.note ?? undefined}
        actions={
          <>
            <Button onClick={() => void validate()}>{api.t('anticheat.ui.validate')}</Button>
            {rs.status !== 'active' && <Button onClick={() => void simulate()}>{api.t('anticheat.ui.simulate')}</Button>}
            <Button variant="primary" onClick={() => void save()}>
              {rs.status === 'draft' ? api.t('anticheat.ui.save') : api.t('anticheat.ui.new_draft')}
            </Button>
            {rs.status !== 'active' && (
              <Button variant="danger" onClick={() => setActivating(true)}>
                {api.t('anticheat.ui.activate')}
              </Button>
            )}
          </>
        }
      />
      {!parsed.ok && <Note tone="bad">{api.t('anticheat.ui.json_error')}</Note>}
      {msg && <Note tone={msg.ok ? 'ok' : 'bad'}>{msg.text}</Note>}
      {errs && errs.length > 0 && (
        <Panel title={api.t('anticheat.ui.errors')} flush>
          <Table
            rows={errs.map((e, i) => ({ ...e, i }))}
            rowKey={(e) => e.i}
            columns={[
              { key: 'p', header: 'path', render: (e) => <Mono>{e.path || '/'}</Mono> },
              { key: 'c', header: 'code', render: (e) => <Mono>{e.code}</Mono> },
              { key: 'm', header: '', render: (e) => <span className="small down">{e.message}</span> },
            ]}
          />
        </Panel>
      )}
      <Panel primary>
        <Textarea className="ta pt-mono json-edit" rows={28} spellCheck={false} value={text} onChange={(e) => setText(e.target.value)} aria-label="ruleset JSON" />
      </Panel>
      {sim && (
        <Panel title={api.t('anticheat.ui.simulation')} sub={sim.status}>
          <pre className="json">{JSON.stringify(sim.result, null, 2)}</pre>
        </Panel>
      )}
      <Sheet
        open={activating}
        onClose={() => setActivating(false)}
        title={api.t('anticheat.ui.activate')}
        footer={
          <Foot api={api} onCancel={() => setActivating(false)}>
            <Button variant="danger" onClick={() => void activate()}>
              {api.t('anticheat.ui.activate')}
            </Button>
          </Foot>
        }
      >
        <p>{api.t('anticheat.ui.activate_confirm', { v: rs.version })}</p>
      </Sheet>
    </div>
  );
}

const ADMIN_TABS = ['queue', 'appeals', 'watchlist', 'torrent_view', 'rulesets', 'stats'];

function AdminHome({ api }: { api: PluginApi }) {
  const initial = new URLSearchParams(window.location.search).get('tab') ?? 'queue';
  const [tab, setTab] = useState(ADMIN_TABS.includes(initial) ? initial : 'queue');
  const [d] = useLoad<Dash>(api, `${BASE}/admin/dashboard`);
  return (
    <div className="stack">
      <PageHead title={api.term('anticheat.name')?.name ?? api.t('anticheat.ui.title')} />
      {d && (
        <Kpis
          items={[
            { label: api.t('anticheat.ui.open_cases'), value: d.open_cases, tone: d.open_cases ? 'down' : undefined, hint: d.over_sla ? `${api.t('anticheat.ui.over_sla')} ${d.over_sla}` : undefined },
            { label: api.t('anticheat.ui.oldest'), value: `${d.oldest_waiting_hours} h` },
            { label: api.t('anticheat.ui.appeals_pending'), value: d.appeals_pending },
            { label: '24h', value: `+${d.opened_24h} / ✓${d.resolved_24h}` },
          ]}
        />
      )}
      {d?.breaker.tripped && <Breaker api={api} since={d.breaker.since} />}
      <Panel flush primary>
        <Tabs
          variant="flush"
          tabs={ADMIN_TABS.map((id) => ({ id, label: api.t(`anticheat.ui.${id}`), count: id === 'queue' ? d?.open_cases : id === 'appeals' ? d?.appeals_pending : undefined }))}
          value={tab}
          onChange={setTab}
        />
        {tab === 'queue' && <Queue api={api} />}
        {tab === 'appeals' && <Appeals api={api} />}
        {tab === 'watchlist' && <Watchlist api={api} />}
        {tab === 'torrent_view' && <Torrents api={api} />}
        {tab === 'rulesets' && <Rulesets api={api} />}
        {tab === 'stats' && <Stats api={api} />}
      </Panel>
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

function Breaker({ api, since, onReset }: { api: PluginApi; since: string | null; onReset?(): void }) {
  return (
    <Note
      tone="bad"
      actions={
        <Button
          size="sm"
          variant="danger"
          onClick={() => {
            void api
              .json(`${BASE}/admin/breaker/resets`, { method: 'POST', json: { note: 'dashboard' } })
              .then(() => onReset?.())
              .catch((e) => api.toast(errText(api, e), 'error'));
          }}
        >
          {api.t('anticheat.ui.reset_breaker')}
        </Button>
      }
    >
      {`${api.t('anticheat.ui.breaker')} ${since ? when(since) : ''}`}
    </Note>
  );
}

function Widget({ api }: SlotProps) {
  const [d, err, reload] = useLoad<Dash>(api, `${BASE}/admin/dashboard`);
  if (err || !d) return null;
  return (
    <Panel
      title={api.term('anticheat.name')?.name ?? 'anticheat'}
      more={
        <Link api={api} to="/admin/p/anticheat/">
          {api.t('anticheat.ui.go')}
        </Link>
      }
    >
      <div className="stack">
        <dl className="ledger">
          <dt>{api.t('anticheat.ui.open_cases')}</dt>
          <dd className={d.open_cases ? 'num down' : 'num'}>{d.open_cases}</dd>
          {d.over_sla > 0 && (
            <>
              <dt>{api.t('anticheat.ui.over_sla')}</dt>
              <dd className="num down">{d.over_sla}</dd>
            </>
          )}
          <dt>{api.t('anticheat.ui.oldest')}</dt>
          <dd className="num">{`${d.oldest_waiting_hours} h`}</dd>
          <dt>{api.t('anticheat.ui.appeals_pending')}</dt>
          <dd className="num">{d.appeals_pending}</dd>
          <dt>24h</dt>
          <dd className="num">{`+${d.opened_24h} / ✓${d.resolved_24h}`}</dd>
        </dl>
        {d.breaker.tripped && <Breaker api={api} since={d.breaker.since} onReset={reload} />}
      </div>
    </Panel>
  );
}

function StaffMenu({ api }: SlotProps) {
  const [d] = useLoad<Dash>(api, api.user() ? `${BASE}/admin/dashboard` : null);
  if (!d) return null;
  const n = d.open_cases + d.appeals_pending;
  return (
    <NavItem to="/admin/p/anticheat/" navigate={api.navigate} count={n || undefined} alert={n > 0}>
      {api.t('anticheat.ui.menu')}
    </NavItem>
  );
}

function UserTab({ api, ctx }: SlotProps<{ profile: { id: number } }>) {
  const uid = ctx.profile?.id;
  const [risk] = useLoad<{ score: number; band: string; open_case_id: number | null; transfer_blocked: boolean; breakdown: CaseDetail['breakdown'] }>(api, uid ? `${BASE}/admin/users/${uid}/risk` : null);
  const [ev] = useLoad<{ items: { id: number; kind: string; severity: string | null; hits: number; occurred_at: string; verdict: string | null }[] }>(
    api,
    uid ? `${BASE}/admin/users/${uid}/evidence?limit=50` : null,
  );
  const [sanc, , reload] = useLoad<{ items: Sanction[] }>(api, uid ? `${BASE}/admin/users/${uid}/sanctions` : null);
  const [cases] = useLoad<{ items: CaseRow[] }>(api, uid ? `${BASE}/admin/cases?user_id=${uid}&states=open,resolved,dismissed,overturned,closed_stale` : null);
  const [reason, setReason] = useState('');
  const [opening, setOpening] = useState(false);
  if (!risk) return <SkeletonRows n={4} />;
  async function openCase() {
    try {
      const r = await api.json<{ id: number }>(`${BASE}/admin/cases`, { method: 'POST', json: { user_id: uid, reason_note: reason }, idempotent: true });
      api.navigate(`/admin/p/anticheat/cases/${r.id}`);
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  }
  return (
    <div className="stack">
      <div className="row-sb">
        <div className="row">
          <span className="risk">{risk.score}</span>
          <Status tone={bandTone(risk.band)}>{risk.band}</Status>
          {risk.transfer_blocked && <Tag tone="x">{api.t('anticheat.ui.transfer_blocked_tag')}</Tag>}
          {risk.open_case_id && (
            <Link api={api} to={`/admin/p/anticheat/cases/${risk.open_case_id}`}>
              {`${api.t('anticheat.ui.case')} #${risk.open_case_id}`}
            </Link>
          )}
        </div>
        {!risk.open_case_id && (
          <Button size="sm" variant="danger" onClick={() => setOpening(true)}>
            {api.t('anticheat.ui.open_case')}
          </Button>
        )}
      </div>
      <Panel title={api.t('anticheat.ui.sanctions')} flush>
        <SanctionTable api={api} rows={sanc?.items ?? []} reload={reload} />
      </Panel>
      {(cases?.items ?? []).length > 0 && (
        <Panel title={api.t('anticheat.ui.queue')} flush>
          <div className="side-list">
            {(cases?.items ?? []).map((c) => (
              <Link key={c.id} api={api} to={`/admin/p/anticheat/cases/${c.id}`}>
                <span className="rk">·</span>
                <span className="nm">
                  {`#${c.id} `}
                  <Status tone={stateTone(c.state)}>{c.state}</Status>
                </span>
                <span className="v">{c.score}</span>
              </Link>
            ))}
          </div>
        </Panel>
      )}
      <Panel title={api.t('anticheat.ui.evidence')} flush>
        <Table
          rows={ev?.items ?? []}
          rowKey={(r) => r.id}
          empty="—"
          columns={[
            { key: 'k', header: api.t('anticheat.ui.kind_col'), render: (r) => r.kind },
            { key: 's', header: api.t('anticheat.ui.severity'), render: (r) => (r.severity ? <Status tone={r.severity === 'high' ? 'bad' : 'mute'}>{r.severity}</Status> : '') },
            { key: 'h', header: '×', right: true, render: (r) => <span className="num">{r.hits}</span> },
            { key: 'v', header: api.t('anticheat.ui.verdict'), render: (r) => <span className="small">{r.verdict ?? ''}</span> },
            { key: 'a', header: api.t('anticheat.ui.time'), right: true, render: (r) => <When at={r.occurred_at} /> },
          ]}
        />
      </Panel>
      <Sheet
        open={opening}
        onClose={() => setOpening(false)}
        title={api.t('anticheat.ui.open_case')}
        footer={
          <Foot api={api} onCancel={() => setOpening(false)}>
            <Button variant="danger" disabled={!reason.trim()} onClick={() => void openCase()}>
              {api.t('anticheat.ui.open_case')}
            </Button>
          </Foot>
        }
      >
        <Field label={api.t('anticheat.ui.reason')}>
          <Textarea rows={3} value={reason} onChange={(e) => setReason(e.target.value)} />
        </Field>
      </Sheet>
    </div>
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
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  return (
    <Panel
      title={`${api.term('anticheat.case')?.name ?? api.t('anticheat.ui.case')} #${c.id}`}
      sub={when(c.resolved_at ?? c.opened_at)}
      actions={c.appeal.state && <Status tone={stateTone(c.appeal.state)}>{`${api.term('anticheat.appeal')?.name ?? 'appeal'} · ${c.appeal.state}`}</Status>}
    >
      <div className="stack">
        <div className="row">
          {c.categories.map((k) => (
            <Tag key={k} tone="x">
              {catLabel(api, k)}
            </Tag>
          ))}
        </div>
        {c.sanctions.length > 0 && (
          <dl className="ledger">
            {c.sanctions.map((s, i) => (
              <Fragment key={i}>
                <dt>{kindLabel(api, s.kind)}</dt>
                <dd>
                  <Status tone={stateTone(s.state)}>{s.state}</Status>
                  {s.until ? <span className="small muted num">{` · ${when(s.until)}`}</span> : ''}
                </dd>
              </Fragment>
            ))}
          </dl>
        )}
        {c.torrents.length > 0 && (
          <div className="row small">
            <span className="muted">{api.t('anticheat.ui.torrents')}</span>
            {c.torrents.map((t) => (
              <Link key={t.id} api={api} to={`/torrents/${t.id}`}>
                {t.title ?? `#${t.id}`}
              </Link>
            ))}
          </div>
        )}
        {c.appeal.allowed && onAppeal && (
          <div className="form">
            <Field label={api.t('anticheat.ui.appeal_body')} hint={`${api.t('anticheat.ui.appeal_deadline')}: ${when(c.appeal.deadline)}`}>
              <Textarea rows={4} maxLength={4000} value={body} onChange={(e) => setBody(e.target.value)} aria-label={api.t('anticheat.ui.appeal_body')} />
            </Field>
            <div className="form-acts">
              <Button
                variant="primary"
                disabled={!body.trim()}
                onClick={() =>
                  void onAppeal(body)
                    .then(() => setMsg({ ok: true, text: api.t('anticheat.ui.appeal_sent') }))
                    .catch((e) => setMsg({ ok: false, text: errText(api, e) }))
                }
              >
                {api.t('anticheat.ui.appeal')}
              </Button>
            </div>
          </div>
        )}
        {msg && <Note tone={msg.ok ? 'ok' : 'bad'}>{msg.text}</Note>}
      </div>
    </Panel>
  );
}

function MyCases({ api }: { api: PluginApi }) {
  const [d, err, reload] = useLoad<{ items: UserCase[] }>(api, `${BASE}/users/me/cases`);
  return (
    <div className="stack">
      <PageHead title={api.t('anticheat.ui.my_cases')} />
      {err && <Note tone="bad">{err}</Note>}
      {!d && !err && <SkeletonRows n={4} />}
      {d && d.items.length === 0 && (
        <Panel>
          <Empty compact>{api.t('anticheat.ui.none')}</Empty>
        </Panel>
      )}
      {(d?.items ?? []).map((c) => (
        <UserCaseCard
          key={c.id}
          api={api}
          c={c}
          onAppeal={async (body) => {
            await api.json(`${BASE}/users/me/cases/${c.id}/appeals`, { method: 'POST', json: { body }, idempotent: true });
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
  const [d, setD] = useState<{ case: UserCase; token_used: boolean; appeal: { state: string } | null } | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  useEffect(() => {
    if (window.location.hash) window.history.replaceState(null, '', window.location.pathname);
    if (!token) return;
    void api
      .json<typeof d>(`${BASE}/appeals:lookup`, { method: 'POST', json: { token } })
      .then(setD)
      .catch((e) => setMsg(errText(api, e)));
  }, [api, token]);
  return (
    <div className="stack narrow-form">
      <PageHead title={api.t('anticheat.ui.token_appeal')} />
      {!token && <Note tone="bad">{api.t('anticheat.ui.no_token')}</Note>}
      {msg && <Note tone="bad">{msg}</Note>}
      {d && (
        <UserCaseCard
          api={api}
          c={{ ...d.case, appeal: { ...d.case.appeal, allowed: d.case.appeal.allowed && !d.token_used } }}
          onAppeal={async (body) => {
            await api.json(`${BASE}/appeals:by-token`, { method: 'POST', json: { token, body }, idempotent: true });
            setD({ ...d, token_used: true, appeal: { state: 'submitted' } });
          }}
        />
      )}
    </div>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  const c = /^\/admin\/p\/anticheat\/cases\/(\d+)/.exec(path);
  if (c?.[1]) return <Workbench key={c[1]} api={api} id={Number(c[1])} />;
  const r = /^\/admin\/p\/anticheat\/rulesets\/(\d+)/.exec(path);
  if (r?.[1]) return <RulesetEditor key={r[1]} api={api} version={Number(r[1])} />;
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
