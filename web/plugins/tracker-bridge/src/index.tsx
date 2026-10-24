// tracker-bridge 前端：后台「Tracker」——
// 节点页：各 pt-tracker 节点的版本、运行时长、同步落后、缓存规模与 announce 速率，以及配置告警（announce 间隔不小于节点 peer 超时、
// 节点失联、无在线节点）；
// 客户端规则页：有序规则（上移 / 下移、启用开关、编辑抽屉）、默认动作、从预设添加、规则测试（对未保存的草稿生效），整体保存带 If-Match；
// 被拒客户端页：按日期的排行，一键生成放行规则草稿。设置仍用内核的设置表单。
import { useEffect, useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend } from '@pt-studio/slots';
import { formatDuration, registerMessages } from '@pt-studio/terms';
import { Button, Card, Field, Input, Kpis, Note, PageHead, Panel, Segmented, Sheet, SideLayout, SkeletonRows, Status, Switch, Table, Tabs, Tag, Textarea } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'tracker-bridge.ui.title': 'Tracker 节点',
  'tracker-bridge.ui.lede': 'pt-tracker 每 10 秒上报一次状态，30 秒未上报视为离线',
  'tracker-bridge.ui.settings': '设置',
  'tracker-bridge.ui.refresh': '刷新',
  'tracker-bridge.ui.online': '在线节点',
  'tracker-bridge.ui.stream': '变更流条目',
  'tracker-bridge.ui.interval': 'announce 间隔',
  'tracker-bridge.ui.node': '节点',
  'tracker-bridge.ui.version': '版本',
  'tracker-bridge.ui.uptime': '运行时长',
  'tracker-bridge.ui.sync': '同步',
  'tracker-bridge.ui.synced': '已同步',
  'tracker-bridge.ui.behind': '落后 {s} 秒',
  'tracker-bridge.ui.lag': '应用延迟 {ms} ms',
  'tracker-bridge.ui.cache': '缓存（passkey / 种子）',
  'tracker-bridge.ui.peer_timeout': 'peer 超时',
  'tracker-bridge.ui.rps': 'announce/秒',
  'tracker-bridge.ui.seen': '上报时间',
  'tracker-bridge.ui.w.no_nodes': '没有在线的 tracker 节点：确认 pt-tracker 已启动且与站点使用同一个 Redis',
  'tracker-bridge.ui.w.node_lag': '节点 {node} 同步落后超过 5 秒：检查它与 Redis 的连接',
  'tracker-bridge.ui.w.interval_exceeds_peer_timeout': 'announce 间隔不小于节点 {node} 的 peer 超时：正常客户端会在两次汇报之间被当作离线。请调小间隔或调大节点的 max_peer_timeout',
  'tracker-bridge.ui.w.node_lost': '节点 {node} 已停止上报（最后一次 {time}）：检查进程是否退出；已下线的节点 24 小时后不再提示',
  'tracker-bridge.ui.tab.nodes': '节点',
  'tracker-bridge.ui.tab.clients': '客户端规则',
  'tracker-bridge.ui.tab.denied': '被拒客户端',
  'tracker-bridge.ui.rules.title': '客户端规则',
  'tracker-bridge.ui.rules.lede': '从上到下逐条匹配，第一条全部条件都满足的规则决定放行或拒绝；保存后 1 秒内各 tracker 节点生效',
  'tracker-bridge.ui.rules.default': '未匹配任何规则的客户端',
  'tracker-bridge.ui.allow': '放行',
  'tracker-bridge.ui.deny': '拒绝',
  'tracker-bridge.ui.rules.add': '新建规则',
  'tracker-bridge.ui.rules.presets': '从预设添加',
  'tracker-bridge.ui.rules.save': '保存',
  'tracker-bridge.ui.rules.discard': '放弃更改',
  'tracker-bridge.ui.rules.saved': '客户端规则已保存',
  'tracker-bridge.ui.rules.dirty': '有未保存的更改',
  'tracker-bridge.ui.rules.not_synced': 'Redis 中的规则与数据库不一致：Redis 恢复后会在 10 分钟内自动重新同步',
  'tracker-bridge.ui.rules.conflict': '规则已被其他管理员修改。请放弃更改、重新加载后再编辑',
  'tracker-bridge.ui.rules.invalid': '有 {n} 处错误，修正后再保存',
  'tracker-bridge.ui.rules.empty': '还没有规则：所有客户端都按默认动作处理',
  'tracker-bridge.ui.rules.deny_all': '没有放行规则且默认拒绝：所有客户端都无法汇报',
  'tracker-bridge.ui.col.order': '顺序',
  'tracker-bridge.ui.col.enabled': '启用',
  'tracker-bridge.ui.col.name': '名称',
  'tracker-bridge.ui.col.action': '动作',
  'tracker-bridge.ui.col.conditions': '条件',
  'tracker-bridge.ui.up': '上移',
  'tracker-bridge.ui.down': '下移',
  'tracker-bridge.ui.edit': '编辑',
  'tracker-bridge.ui.remove': '删除',
  'tracker-bridge.ui.cond.pid_prefix': 'peer_id 以 {v} 开头',
  'tracker-bridge.ui.cond.pid_regex': 'peer_id 匹配 {v}',
  'tracker-bridge.ui.cond.min_ver': '版本 ≥ {v}',
  'tracker-bridge.ui.cond.max_ver': '版本 ≤ {v}',
  'tracker-bridge.ui.cond.ua_prefix': 'UA 以 {v} 开头',
  'tracker-bridge.ui.cond.ua_regex': 'UA 匹配 {v}',
  'tracker-bridge.ui.edit.new': '新建规则',
  'tracker-bridge.ui.edit.title': '编辑规则',
  'tracker-bridge.ui.edit.done': '确定',
  'tracker-bridge.ui.edit.cancel': '取消',
  'tracker-bridge.ui.f.name': '名称',
  'tracker-bridge.ui.f.action': '动作',
  'tracker-bridge.ui.f.pid_prefix': 'peer_id 前缀',
  'tracker-bridge.ui.f.pid_prefix_hint': '如 -qB；任意字节用 %XX 表示（如 -UT%32%32）',
  'tracker-bridge.ui.f.pid_regex': 'peer_id 正则',
  'tracker-bridge.ui.f.pid_regex_hint': '用 (?P<ver>…) 捕获版本号字符，如 ^-qB(?P<ver>[0-9A-Z]{4})-；每个字符按 0-9、A-Z 依次为 0～35 解析为一段版本',
  'tracker-bridge.ui.f.min_ver': '最低版本',
  'tracker-bridge.ui.f.max_ver': '最高版本',
  'tracker-bridge.ui.f.ver_hint': '如 4.3、0.13.8；需要正则中有 ver 组',
  'tracker-bridge.ui.f.ua_prefix': 'User-Agent 前缀',
  'tracker-bridge.ui.f.ua_regex': 'User-Agent 正则',
  'tracker-bridge.ui.f.note': '备注',
  'tracker-bridge.ui.f.enabled': '启用',
  'tracker-bridge.ui.f.need_condition': '至少填一个 peer_id 或 User-Agent 条件',
  'tracker-bridge.ui.test.title': '规则测试',
  'tracker-bridge.ui.test.lede': '对当前编辑中的规则（含未保存的更改）测试',
  'tracker-bridge.ui.test.peer_id': 'peer_id',
  'tracker-bridge.ui.test.peer_id_hint': '20 字节文本、%XX 编码或 40 位十六进制',
  'tracker-bridge.ui.test.ua': 'User-Agent',
  'tracker-bridge.ui.test.run': '测试',
  'tracker-bridge.ui.test.hit': '命中第 {n} 条：{name}',
  'tracker-bridge.ui.test.default': '没有命中任何规则，按默认动作处理',
  'tracker-bridge.ui.test.version': '解析出的版本：{v}',
  'tracker-bridge.ui.presets.title': '预设规则',
  'tracker-bridge.ui.presets.lede': '加入后排在列表末尾，可再调整顺序与条件',
  'tracker-bridge.ui.presets.add': '加入',
  'tracker-bridge.ui.presets.added': '已加入',
  'tracker-bridge.ui.denied.title': '被拒客户端',
  'tracker-bridge.ui.denied.lede': '各 tracker 节点因客户端规则拒绝的请求，按 peer_id 前 8 字节与 User-Agent 分组；保留 8 天（UTC 日期）',
  'tracker-bridge.ui.denied.date': '日期',
  'tracker-bridge.ui.denied.total': '当天被拒次数',
  'tracker-bridge.ui.denied.kinds': '不同客户端',
  'tracker-bridge.ui.denied.prefix': 'peer_id 前缀',
  'tracker-bridge.ui.denied.ua': 'User-Agent',
  'tracker-bridge.ui.denied.count': '次数',
  'tracker-bridge.ui.denied.overflow': '其他（超过统计上限）',
  'tracker-bridge.ui.denied.allow': '生成放行规则',
  'tracker-bridge.ui.denied.empty': '这一天没有被拒的客户端',
  'tracker-bridge.ui.denied.drafted': '已生成规则草稿，确认后记得保存',
});
registerMessages('en', {
  'tracker-bridge.ui.title': 'Tracker nodes',
  'tracker-bridge.ui.lede': 'Each pt-tracker node reports every 10 seconds and counts as offline after 30 seconds of silence',
  'tracker-bridge.ui.settings': 'Settings',
  'tracker-bridge.ui.refresh': 'Refresh',
  'tracker-bridge.ui.online': 'Online nodes',
  'tracker-bridge.ui.stream': 'Change stream entries',
  'tracker-bridge.ui.interval': 'Announce interval',
  'tracker-bridge.ui.node': 'Node',
  'tracker-bridge.ui.version': 'Version',
  'tracker-bridge.ui.uptime': 'Uptime',
  'tracker-bridge.ui.sync': 'Sync',
  'tracker-bridge.ui.synced': 'In sync',
  'tracker-bridge.ui.behind': '{s}s behind',
  'tracker-bridge.ui.lag': 'Apply delay {ms} ms',
  'tracker-bridge.ui.cache': 'Cache (passkeys / torrents)',
  'tracker-bridge.ui.peer_timeout': 'Peer timeout',
  'tracker-bridge.ui.rps': 'Announces/s',
  'tracker-bridge.ui.seen': 'Last report',
  'tracker-bridge.ui.w.no_nodes': 'No tracker node is online. Check that pt-tracker is running and uses the same Redis as the site',
  'tracker-bridge.ui.w.node_lag': 'Node {node} is more than 5 seconds behind. Check its connection to Redis',
  'tracker-bridge.ui.w.interval_exceeds_peer_timeout': 'The announce interval is not shorter than the peer timeout of node {node}, so healthy clients drop out between announces. Lower the interval or raise max_peer_timeout on the node',
  'tracker-bridge.ui.w.node_lost': 'Node {node} stopped reporting (last seen {time}). Check whether the process exited; decommissioned nodes stop showing after 24 hours',
  'tracker-bridge.ui.tab.nodes': 'Nodes',
  'tracker-bridge.ui.tab.clients': 'Client rules',
  'tracker-bridge.ui.tab.denied': 'Denied clients',
  'tracker-bridge.ui.rules.title': 'Client rules',
  'tracker-bridge.ui.rules.lede': 'Rules are checked from top to bottom; the first rule whose conditions all match decides. Tracker nodes apply changes within a second of saving',
  'tracker-bridge.ui.rules.default': 'Clients that match no rule',
  'tracker-bridge.ui.allow': 'Allow',
  'tracker-bridge.ui.deny': 'Deny',
  'tracker-bridge.ui.rules.add': 'New rule',
  'tracker-bridge.ui.rules.presets': 'Add from presets',
  'tracker-bridge.ui.rules.save': 'Save',
  'tracker-bridge.ui.rules.discard': 'Discard changes',
  'tracker-bridge.ui.rules.saved': 'Client rules saved',
  'tracker-bridge.ui.rules.dirty': 'You have unsaved changes',
  'tracker-bridge.ui.rules.not_synced': 'The rules in Redis differ from the database. They resync automatically within 10 minutes once Redis is back',
  'tracker-bridge.ui.rules.conflict': 'Another admin changed the rules. Discard your changes and reload before editing',
  'tracker-bridge.ui.rules.invalid': '{n} problems to fix before saving',
  'tracker-bridge.ui.rules.empty': 'No rules yet: every client gets the default action',
  'tracker-bridge.ui.rules.deny_all': 'There is no allow rule and the default is deny, so no client can announce',
  'tracker-bridge.ui.col.order': 'Order',
  'tracker-bridge.ui.col.enabled': 'On',
  'tracker-bridge.ui.col.name': 'Name',
  'tracker-bridge.ui.col.action': 'Action',
  'tracker-bridge.ui.col.conditions': 'Conditions',
  'tracker-bridge.ui.up': 'Move up',
  'tracker-bridge.ui.down': 'Move down',
  'tracker-bridge.ui.edit': 'Edit',
  'tracker-bridge.ui.remove': 'Remove',
  'tracker-bridge.ui.cond.pid_prefix': 'peer_id starts with {v}',
  'tracker-bridge.ui.cond.pid_regex': 'peer_id matches {v}',
  'tracker-bridge.ui.cond.min_ver': 'version ≥ {v}',
  'tracker-bridge.ui.cond.max_ver': 'version ≤ {v}',
  'tracker-bridge.ui.cond.ua_prefix': 'UA starts with {v}',
  'tracker-bridge.ui.cond.ua_regex': 'UA matches {v}',
  'tracker-bridge.ui.edit.new': 'New rule',
  'tracker-bridge.ui.edit.title': 'Edit rule',
  'tracker-bridge.ui.edit.done': 'Done',
  'tracker-bridge.ui.edit.cancel': 'Cancel',
  'tracker-bridge.ui.f.name': 'Name',
  'tracker-bridge.ui.f.action': 'Action',
  'tracker-bridge.ui.f.pid_prefix': 'peer_id prefix',
  'tracker-bridge.ui.f.pid_prefix_hint': 'For example -qB; write any byte as %XX (such as -UT%32%32)',
  'tracker-bridge.ui.f.pid_regex': 'peer_id regex',
  'tracker-bridge.ui.f.pid_regex_hint': 'Capture the version characters with (?P<ver>…), as in ^-qB(?P<ver>[0-9A-Z]{4})-. Each character is one version part, 0-9 then A-Z for 0 to 35',
  'tracker-bridge.ui.f.min_ver': 'Minimum version',
  'tracker-bridge.ui.f.max_ver': 'Maximum version',
  'tracker-bridge.ui.f.ver_hint': 'Such as 4.3 or 0.13.8; needs a ver group in the regex',
  'tracker-bridge.ui.f.ua_prefix': 'User-Agent prefix',
  'tracker-bridge.ui.f.ua_regex': 'User-Agent regex',
  'tracker-bridge.ui.f.note': 'Note',
  'tracker-bridge.ui.f.enabled': 'Enabled',
  'tracker-bridge.ui.f.need_condition': 'Fill in at least one peer_id or User-Agent condition',
  'tracker-bridge.ui.test.title': 'Test a client',
  'tracker-bridge.ui.test.lede': 'Runs against the rules as currently edited, including unsaved changes',
  'tracker-bridge.ui.test.peer_id': 'peer_id',
  'tracker-bridge.ui.test.peer_id_hint': '20-byte text, %XX escapes or 40 hex digits',
  'tracker-bridge.ui.test.ua': 'User-Agent',
  'tracker-bridge.ui.test.run': 'Test',
  'tracker-bridge.ui.test.hit': 'Matches rule {n}: {name}',
  'tracker-bridge.ui.test.default': 'No rule matches, so the default action applies',
  'tracker-bridge.ui.test.version': 'Parsed version: {v}',
  'tracker-bridge.ui.presets.title': 'Preset rules',
  'tracker-bridge.ui.presets.lede': 'Added rules go to the end of the list; reorder and adjust them afterwards',
  'tracker-bridge.ui.presets.add': 'Add',
  'tracker-bridge.ui.presets.added': 'Added',
  'tracker-bridge.ui.denied.title': 'Denied clients',
  'tracker-bridge.ui.denied.lede': 'Requests the tracker nodes refused because of client rules, grouped by the first 8 bytes of peer_id and the User-Agent. Kept for 8 days (UTC dates)',
  'tracker-bridge.ui.denied.date': 'Date',
  'tracker-bridge.ui.denied.total': 'Denied requests',
  'tracker-bridge.ui.denied.kinds': 'Distinct clients',
  'tracker-bridge.ui.denied.prefix': 'peer_id prefix',
  'tracker-bridge.ui.denied.ua': 'User-Agent',
  'tracker-bridge.ui.denied.count': 'Count',
  'tracker-bridge.ui.denied.overflow': 'Others (over the tracking limit)',
  'tracker-bridge.ui.denied.allow': 'Draft an allow rule',
  'tracker-bridge.ui.denied.empty': 'No client was denied on this day',
  'tracker-bridge.ui.denied.drafted': 'Rule drafted. Remember to save after reviewing it',
});

interface Node {
  node_id: string;
  version: string | null;
  boot_at: number | null;
  sync_id: string;
  lag_ms: number;
  behind_ms: number;
  passkeys: number;
  torrents_cached: number;
  max_peer_timeout: number | null;
  rps: number;
  updated_at: number | null;
}
interface StatusResp {
  nodes: Node[];
  stream: { length: number; last_id: string | null };
  interval: number;
  warnings: { code: string; node_id?: string; last_seen_at?: number }[];
}

const LAG_ALERT_MS = 5000;

const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e);

function Nodes({ api }: { api: PluginApi }) {
  const q = useQuery({
    queryKey: ['tracker-bridge', 'status'],
    queryFn: () => api.json<StatusResp>('/api/p/tracker-bridge/v1/admin/status'),
    refetchInterval: 15_000,
  });
  const d = q.data;
  return (
    <div className="stack">
      <PageHead
        title={api.t('tracker-bridge.ui.title')}
        lede={api.t('tracker-bridge.ui.lede')}
        actions={
          <>
            <Button size="sm" onClick={() => void q.refetch()} disabled={q.isFetching}>
              {api.t('tracker-bridge.ui.refresh')}
            </Button>
            <Button size="sm" onClick={() => api.navigate('/admin/p/tracker-bridge/settings')}>
              {api.t('tracker-bridge.ui.settings')}
            </Button>
          </>
        }
      />
      <Nav api={api} tab="nodes" />
      {q.error && <Note tone="bad">{errText(api, q.error)}</Note>}
      {d?.warnings.map((w, i) => (
        <Note key={i} tone={w.code === 'node_lag' || w.code === 'node_lost' ? 'warn' : 'bad'}>
          {api.t(`tracker-bridge.ui.w.${w.code}`, { node: w.node_id ?? '', time: w.last_seen_at ? new Date(w.last_seen_at).toLocaleString() : '' })}
        </Note>
      ))}
      {!d ? (
        !q.error && <SkeletonRows n={4} />
      ) : (
        <>
          <Kpis
            items={[
              { label: api.t('tracker-bridge.ui.online'), value: d.nodes.length, tone: d.nodes.length ? 'up' : 'down' },
              { label: api.t('tracker-bridge.ui.stream'), value: d.stream.length.toLocaleString(), hint: d.stream.last_id ?? undefined },
              { label: api.t('tracker-bridge.ui.interval'), value: formatDuration(d.interval) },
            ]}
          />
          <Panel flush primary>
            <Table
              rows={d.nodes}
              rowKey={(n) => n.node_id}
              empty="—"
              minWidth={820}
              columns={[
                { key: 'n', header: api.t('tracker-bridge.ui.node'), render: (n) => <b>{n.node_id}</b> },
                { key: 'v', header: api.t('tracker-bridge.ui.version'), render: (n) => <span className="num">{n.version ?? '—'}</span> },
                {
                  key: 'u',
                  header: api.t('tracker-bridge.ui.uptime'),
                  render: (n) => <span className="num nowrap">{n.boot_at ? formatDuration(Math.max(0, (Date.now() - n.boot_at) / 1000)) : '—'}</span>,
                },
                {
                  key: 's',
                  header: api.t('tracker-bridge.ui.sync'),
                  render: (n) => (
                    <span title={n.sync_id}>
                      {n.behind_ms > LAG_ALERT_MS ? (
                        <Status tone="warn">{api.t('tracker-bridge.ui.behind', { s: Math.round(n.behind_ms / 1000) })}</Status>
                      ) : (
                        <Status tone="ok">{api.t('tracker-bridge.ui.synced')}</Status>
                      )}
                      <div className="small muted">{api.t('tracker-bridge.ui.lag', { ms: n.lag_ms })}</div>
                    </span>
                  ),
                },
                {
                  key: 'c',
                  header: api.t('tracker-bridge.ui.cache'),
                  right: true,
                  render: (n) => <span className="num">{`${n.passkeys.toLocaleString()} / ${n.torrents_cached.toLocaleString()}`}</span>,
                },
                {
                  key: 'm',
                  header: api.t('tracker-bridge.ui.peer_timeout'),
                  right: true,
                  render: (n) => <span className="num">{n.max_peer_timeout ? formatDuration(n.max_peer_timeout) : '—'}</span>,
                },
                { key: 'r', header: api.t('tracker-bridge.ui.rps'), right: true, render: (n) => <span className="num">{n.rps.toFixed(1)}</span> },
                {
                  key: 't',
                  header: api.t('tracker-bridge.ui.seen'),
                  render: (n) => <span className="small muted num nowrap">{n.updated_at ? new Date(n.updated_at).toLocaleTimeString() : '—'}</span>,
                },
              ]}
            />
          </Panel>
        </>
      )}
    </div>
  );
}


type Tab = 'nodes' | 'clients' | 'denied';
const TAB_PATH: Record<Tab, string> = {
  nodes: '/admin/p/tracker-bridge/',
  clients: '/admin/p/tracker-bridge/clients',
  denied: '/admin/p/tracker-bridge/denied',
};

function Nav({ api, tab }: { api: PluginApi; tab: Tab }) {
  return (
    <Tabs
      variant="sub"
      value={tab}
      onChange={(t) => api.navigate(TAB_PATH[t as Tab])}
      tabs={(['nodes', 'clients', 'denied'] as const).map((id) => ({ id, label: api.t(`tracker-bridge.ui.tab.${id}`) }))}
    />
  );
}

// ---------------- 客户端规则 ----------------

type Action = 'allow' | 'deny';
const COND_FIELDS = ['pid_prefix', 'pid_regex', 'min_ver', 'max_ver', 'ua_prefix', 'ua_regex'] as const;
type CondField = (typeof COND_FIELDS)[number];

interface Rule {
  id?: number;
  name: string;
  action: Action;
  pid_prefix?: string;
  pid_regex?: string;
  min_ver?: string;
  max_ver?: string;
  ua_prefix?: string;
  ua_regex?: string;
  enabled: boolean;
  note: string;
}
interface RulesResp {
  version: number;
  default_action: Action;
  rules: Rule[];
  redis_ver: number | null;
  synced: boolean;
}
interface FieldErr {
  path: string;
  code: string;
  message: string;
}
interface TestResp {
  action: Action;
  rule_index: number | null;
  rule_name: string | null;
  extracted_ver: number[] | null;
}
interface Draft {
  version: number;
  default_action: Action;
  /** 列表内的稳定键（新规则还没有 ID） */
  rules: (Rule & { key: string })[];
}

let seq = 0;
const keyed = (r: Rule) => ({ ...r, key: r.id ? `r${r.id}` : `n${++seq}` });
const strip = ({ key: _k, ...r }: Rule & { key: string }): Rule => r;
const fieldErrors = (e: unknown): FieldErr[] =>
  e instanceof ApiError && Array.isArray((e.details as { errors?: unknown })?.errors) ? (e.details as { errors: FieldErr[] }).errors : [];

function conditions(api: PluginApi, r: Rule): string[] {
  return COND_FIELDS.filter((f) => r[f]).map((f) => api.t(`tracker-bridge.ui.cond.${f}`, { v: r[f] }));
}

/** 规则编辑抽屉：确定前用规则测试接口在服务端校验（正则方言与 pt-tracker 一致） */
function RuleSheet({ api, rule, onDone, onClose }: { api: PluginApi; rule: Rule | null; onDone(r: Rule): void; onClose(): void }) {
  const [r, setR] = useState<Rule | null>(rule);
  const [errs, setErrs] = useState<FieldErr[]>([]);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    setR(rule);
    setErrs([]);
  }, [rule]);
  if (!r) return null;
  const set = (patch: Partial<Rule>) => setR({ ...r, ...patch });
  const err = (f: string) => errs.find((e) => e.path.endsWith(`/${f}`))?.message ?? null;
  const done = async () => {
    const clean: Rule = { ...r, name: r.name.trim() };
    for (const f of COND_FIELDS) if (!clean[f]?.trim()) delete clean[f];
    setBusy(true);
    try {
      await api.json('/api/p/tracker-bridge/v1/admin/client-rules:test', {
        method: 'POST',
        json: { peer_id: '-XX0000-', rules: { default_action: 'deny', rules: [clean] } },
      });
      onDone(clean);
    } catch (e) {
      const fe = fieldErrors(e);
      if (fe.length) setErrs(fe);
      else api.toast(errText(api, e), 'error');
    } finally {
      setBusy(false);
    }
  };
  const text = (f: CondField | 'name', hint?: string) => (
    <Field label={api.t(`tracker-bridge.ui.f.${f}`)} error={f === 'pid_prefix' && err('pid_prefix')?.includes('condition') ? api.t('tracker-bridge.ui.f.need_condition') : err(f)} hint={hint}>
      <Input value={r[f] ?? ''} onChange={(e) => set({ [f]: e.target.value })} spellCheck={false} />
    </Field>
  );
  return (
    <Sheet
      open
      onClose={onClose}
      title={api.t(r.id ? 'tracker-bridge.ui.edit.title' : 'tracker-bridge.ui.edit.new')}
      footer={
        <>
          <Button onClick={onClose}>{api.t('tracker-bridge.ui.edit.cancel')}</Button>
          <Button variant="primary" onClick={() => void done()} disabled={busy || !r.name.trim()}>
            {api.t('tracker-bridge.ui.edit.done')}
          </Button>
        </>
      }
    >
      <div className="stack-s">
        {text('name')}
        <Field label={api.t('tracker-bridge.ui.f.action')}>
          <Segmented<Action>
            value={r.action}
            onChange={(action) => set({ action })}
            options={[
              { value: 'allow', label: api.t('tracker-bridge.ui.allow') },
              { value: 'deny', label: api.t('tracker-bridge.ui.deny') },
            ]}
          />
        </Field>
        {text('pid_prefix', api.t('tracker-bridge.ui.f.pid_prefix_hint'))}
        {text('pid_regex', api.t('tracker-bridge.ui.f.pid_regex_hint'))}
        {text('min_ver', api.t('tracker-bridge.ui.f.ver_hint'))}
        {text('max_ver')}
        {text('ua_prefix')}
        {text('ua_regex')}
        <Field label={api.t('tracker-bridge.ui.f.note')} error={err('note')}>
          <Textarea rows={2} value={r.note} onChange={(e) => set({ note: e.target.value })} />
        </Field>
        <Switch showLabel label={api.t('tracker-bridge.ui.f.enabled')} checked={r.enabled} onChange={(enabled) => set({ enabled })} />
      </div>
    </Sheet>
  );
}

function TestPanel({ api, draft }: { api: PluginApi; draft: Draft }) {
  const [pid, setPid] = useState('-qB5120-a1b2c3d4e5f6');
  const [ua, setUa] = useState('qBittorrent/5.1.2');
  const [res, setRes] = useState<TestResp | null>(null);
  const [error, setError] = useState<string | null>(null);
  const run = async () => {
    setError(null);
    try {
      setRes(
        await api.json<TestResp>('/api/p/tracker-bridge/v1/admin/client-rules:test', {
          method: 'POST',
          json: { peer_id: pid, user_agent: ua, rules: { default_action: draft.default_action, rules: draft.rules.map(strip) } },
        }),
      );
    } catch (e) {
      setRes(null);
      setError(errText(api, e));
    }
  };
  return (
    <Card title={api.t('tracker-bridge.ui.test.title')}>
      <div className="stack-s">
        <p className="small muted">{api.t('tracker-bridge.ui.test.lede')}</p>
        <Field label={api.t('tracker-bridge.ui.test.peer_id')} hint={api.t('tracker-bridge.ui.test.peer_id_hint')}>
          <Input value={pid} onChange={(e) => setPid(e.target.value)} spellCheck={false} />
        </Field>
        <Field label={api.t('tracker-bridge.ui.test.ua')}>
          <Input value={ua} onChange={(e) => setUa(e.target.value)} spellCheck={false} />
        </Field>
        <Button onClick={() => void run()} disabled={!pid.trim()}>
          {api.t('tracker-bridge.ui.test.run')}
        </Button>
        {error && <Note tone="bad">{error}</Note>}
        {res && (
          <div data-testid="rule-test-result">
            <Status tone={res.action === 'allow' ? 'ok' : 'bad'}>{api.t(`tracker-bridge.ui.${res.action}`)}</Status>{' '}
            <span className="small">
              {res.rule_index === null ? api.t('tracker-bridge.ui.test.default') : api.t('tracker-bridge.ui.test.hit', { n: res.rule_index + 1, name: res.rule_name ?? '' })}
            </span>
            {res.extracted_ver && <div className="small muted">{api.t('tracker-bridge.ui.test.version', { v: res.extracted_ver.join('.') })}</div>}
          </div>
        )}
      </div>
    </Card>
  );
}

function PresetSheet({ api, open, draft, onAdd, onClose }: { api: PluginApi; open: boolean; draft: Draft; onAdd(r: Rule): void; onClose(): void }) {
  const q = useQuery({
    queryKey: ['tracker-bridge', 'presets'],
    queryFn: () => api.json<{ items: Rule[] }>('/api/p/tracker-bridge/v1/admin/client-rule-presets'),
    enabled: open,
    staleTime: Infinity,
  });
  return (
    <Sheet open={open} onClose={onClose} title={api.t('tracker-bridge.ui.presets.title')}>
      <div className="stack-s">
        <p className="small muted">{api.t('tracker-bridge.ui.presets.lede')}</p>
        {!q.data ? (
          <SkeletonRows n={6} />
        ) : (
          q.data.items.map((p) => {
            const added = draft.rules.some((r) => r.name === p.name);
            return (
              <div key={p.name} className="row-sb">
                <div>
                  <b>{p.name}</b> <Tag tone={p.action === 'allow' ? 'k' : 'x'}>{api.t(`tracker-bridge.ui.${p.action}`)}</Tag>
                  <div className="small muted">{conditions(api, p).join(' · ')}</div>
                  {p.note && <div className="small muted">{p.note}</div>}
                </div>
                <Button size="sm" disabled={added} onClick={() => onAdd(p)}>
                  {api.t(added ? 'tracker-bridge.ui.presets.added' : 'tracker-bridge.ui.presets.add')}
                </Button>
              </div>
            );
          })
        )}
      </div>
    </Sheet>
  );
}

const RULES_KEY = ['tracker-bridge', 'client-rules'];

function Clients({ api, draft, setDraft, editing, setEditing }: { api: PluginApi; draft: Draft | null; setDraft(d: Draft | null): void; editing: { index: number | null; rule: Rule } | null; setEditing(e: { index: number | null; rule: Rule } | null): void }) {
  const qc = useQueryClient();
  const q = useQuery({ queryKey: RULES_KEY, queryFn: () => api.json<RulesResp>('/api/p/tracker-bridge/v1/admin/client-rules') });
  const [presets, setPresets] = useState(false);
  const [errs, setErrs] = useState<FieldErr[]>([]);
  const [conflict, setConflict] = useState(false);
  const [saving, setSaving] = useState(false);
  const saved = q.data;
  // 草稿为空时以服务端规则集为准
  const d: Draft | null = draft ?? (saved ? { version: saved.version, default_action: saved.default_action, rules: saved.rules.map(keyed) } : null);
  const dirty = draft !== null;
  const update = (patch: Partial<Draft>) => d && setDraft({ ...d, ...patch });
  const setRules = (rules: Draft['rules']) => {
    setErrs([]);
    update({ rules });
  };
  const move = (i: number, by: number) => {
    if (!d) return;
    const rules = [...d.rules];
    const [r] = rules.splice(i, 1);
    if (r) rules.splice(i + by, 0, r);
    setRules(rules);
  };
  const save = async () => {
    if (!d) return;
    setSaving(true);
    try {
      const r = await api.json<RulesResp>('/api/p/tracker-bridge/v1/admin/client-rules', {
        method: 'PUT',
        headers: { 'if-match': `"${d.version}"` },
        json: { default_action: d.default_action, rules: d.rules.map(strip) },
      });
      qc.setQueryData(RULES_KEY, r);
      setDraft(null);
      setErrs([]);
      api.toast(api.t('tracker-bridge.ui.rules.saved'), 'success');
    } catch (e) {
      if (e instanceof ApiError && e.code === 'kernel.precondition_failed') setConflict(true);
      else if (fieldErrors(e).length) setErrs(fieldErrors(e));
      else api.toast(errText(api, e), 'error');
    } finally {
      setSaving(false);
    }
  };
  const discard = () => {
    setDraft(null);
    setErrs([]);
    setConflict(false);
    void q.refetch();
  };
  const rowErr = (i: number) => errs.filter((e) => e.path.startsWith(`/rules/${i}/`));
  const noAllow = d && d.default_action === 'deny' && !d.rules.some((r) => r.enabled && r.action === 'allow');
  return (
    <div className="stack">
      <PageHead
        title={api.t('tracker-bridge.ui.rules.title')}
        lede={api.t('tracker-bridge.ui.rules.lede')}
        actions={
          <>
            <Button size="sm" onClick={() => setPresets(true)} disabled={!d}>
              {api.t('tracker-bridge.ui.rules.presets')}
            </Button>
            <Button size="sm" onClick={() => setEditing({ index: null, rule: { name: '', action: 'allow', enabled: true, note: '' } })} disabled={!d}>
              {api.t('tracker-bridge.ui.rules.add')}
            </Button>
            {dirty && (
              <Button size="sm" onClick={discard}>
                {api.t('tracker-bridge.ui.rules.discard')}
              </Button>
            )}
            <Button size="sm" variant="primary" onClick={() => void save()} disabled={!dirty || saving}>
              {api.t('tracker-bridge.ui.rules.save')}
            </Button>
          </>
        }
      />
      <Nav api={api} tab="clients" />
      {q.error && <Note tone="bad">{errText(api, q.error)}</Note>}
      {conflict && <Note tone="bad">{api.t('tracker-bridge.ui.rules.conflict')}</Note>}
      {errs.length > 0 && <Note tone="bad">{api.t('tracker-bridge.ui.rules.invalid', { n: errs.length })}</Note>}
      {dirty && !conflict && <Note tone="info">{api.t('tracker-bridge.ui.rules.dirty')}</Note>}
      {saved && !saved.synced && <Note tone="warn">{api.t('tracker-bridge.ui.rules.not_synced')}</Note>}
      {noAllow && <Note tone="warn">{api.t('tracker-bridge.ui.rules.deny_all')}</Note>}
      {!d ? (
        !q.error && <SkeletonRows n={6} />
      ) : (
        <SideLayout side={<TestPanel api={api} draft={d} />} width={320}>
          <div className="stack">
            <Field label={api.t('tracker-bridge.ui.rules.default')}>
              <Segmented<Action>
                value={d.default_action}
                onChange={(default_action) => update({ default_action })}
                options={[
                  { value: 'allow', label: api.t('tracker-bridge.ui.allow') },
                  { value: 'deny', label: api.t('tracker-bridge.ui.deny') },
                ]}
              />
            </Field>
            <Panel flush primary>
              <Table
                rows={d.rules.map((r, i) => ({ r, i }))}
                rowKey={(x) => x.r.key}
                empty={api.t('tracker-bridge.ui.rules.empty')}
                minWidth={820}
                columns={[
                  {
                    key: 'o',
                    header: api.t('tracker-bridge.ui.col.order'),
                    render: ({ i }) => (
                      <span className="row nowrap">
                        <span className="num muted">{i + 1}</span>
                        <Button size="xs" variant="ghost" aria-label={api.t('tracker-bridge.ui.up')} title={api.t('tracker-bridge.ui.up')} disabled={i === 0} onClick={() => move(i, -1)}>
                          ↑
                        </Button>
                        <Button size="xs" variant="ghost" aria-label={api.t('tracker-bridge.ui.down')} title={api.t('tracker-bridge.ui.down')} disabled={i === d.rules.length - 1} onClick={() => move(i, 1)}>
                          ↓
                        </Button>
                      </span>
                    ),
                  },
                  {
                    key: 'e',
                    header: api.t('tracker-bridge.ui.col.enabled'),
                    render: ({ r, i }) => (
                      <Switch label={r.name} checked={r.enabled} onChange={(enabled) => setRules(d.rules.map((x, j) => (j === i ? { ...x, enabled } : x)))} />
                    ),
                  },
                  {
                    key: 'n',
                    header: api.t('tracker-bridge.ui.col.name'),
                    render: ({ r, i }) => (
                      <div>
                        <b className={r.enabled ? undefined : 'muted'}>{r.name}</b>
                        {r.note && <div className="small muted">{r.note}</div>}
                        {rowErr(i).map((e) => (
                          <div key={e.path} className="pt-field-error">
                            {e.message}
                          </div>
                        ))}
                      </div>
                    ),
                  },
                  {
                    key: 'a',
                    header: api.t('tracker-bridge.ui.col.action'),
                    render: ({ r }) => <Tag tone={r.action === 'allow' ? 'k' : 'x'}>{api.t(`tracker-bridge.ui.${r.action}`)}</Tag>,
                  },
                  {
                    key: 'c',
                    header: api.t('tracker-bridge.ui.col.conditions'),
                    render: ({ r }) => <span className="small">{conditions(api, r).join(' · ')}</span>,
                  },
                  {
                    key: 'x',
                    header: '',
                    right: true,
                    render: ({ r, i }) => (
                      <span className="row nowrap">
                        <Button size="xs" onClick={() => setEditing({ index: i, rule: strip(r) })}>
                          {api.t('tracker-bridge.ui.edit')}
                        </Button>
                        <Button size="xs" variant="danger" onClick={() => setRules(d.rules.filter((_, j) => j !== i))}>
                          {api.t('tracker-bridge.ui.remove')}
                        </Button>
                      </span>
                    ),
                  },
                ]}
              />
            </Panel>
          </div>
        </SideLayout>
      )}
      <RuleSheet
        api={api}
        rule={editing?.rule ?? null}
        onClose={() => setEditing(null)}
        onDone={(rule) => {
          if (!d || !editing) return;
          const i = editing.index;
          setRules(i === null ? [...d.rules, keyed(rule)] : d.rules.map((x, j) => (j === i ? { ...rule, key: x.key } : x)));
          setEditing(null);
        }}
      />
      {d && <PresetSheet api={api} open={presets} draft={d} onClose={() => setPresets(false)} onAdd={(r) => setRules([...d.rules, keyed(r)])} />}
    </div>
  );
}

// ---------------- 被拒客户端 ----------------

interface DeniedResp {
  date: string;
  total: number;
  items: { peer_id_prefix: string; user_agent: string; count: number; overflow: boolean }[];
}

const utcDay = (offset: number) => new Date(Date.now() - offset * 86_400_000).toISOString().slice(0, 10);

function Denied({ api, onDraft }: { api: PluginApi; onDraft(r: Rule): void }) {
  const [date, setDate] = useState(utcDay(0));
  const q = useQuery({
    queryKey: ['tracker-bridge', 'denied', date],
    queryFn: () => api.json<DeniedResp>(`/api/p/tracker-bridge/v1/admin/denied-clients?date=${date}&limit=100`),
  });
  const d = q.data;
  return (
    <div className="stack">
      <PageHead title={api.t('tracker-bridge.ui.denied.title')} lede={api.t('tracker-bridge.ui.denied.lede')} />
      <Nav api={api} tab="denied" />
      <Field label={api.t('tracker-bridge.ui.denied.date')}>
        <Input type="date" value={date} min={utcDay(7)} max={utcDay(0)} onChange={(e) => e.target.value && setDate(e.target.value)} />
      </Field>
      {q.error && <Note tone="bad">{errText(api, q.error)}</Note>}
      {!d ? (
        !q.error && <SkeletonRows n={5} />
      ) : (
        <>
          <Kpis
            items={[
              { label: api.t('tracker-bridge.ui.denied.total'), value: d.total.toLocaleString() },
              { label: api.t('tracker-bridge.ui.denied.kinds'), value: d.items.length },
            ]}
          />
          <Panel flush primary>
            <Table
              rows={d.items}
              rowKey={(x) => `${x.peer_id_prefix}|${x.user_agent}`}
              empty={api.t('tracker-bridge.ui.denied.empty')}
              columns={[
                {
                  key: 'p',
                  header: api.t('tracker-bridge.ui.denied.prefix'),
                  render: (x) => (x.overflow ? <span className="muted">{api.t('tracker-bridge.ui.denied.overflow')}</span> : <code>{x.peer_id_prefix}</code>),
                },
                { key: 'u', header: api.t('tracker-bridge.ui.denied.ua'), render: (x) => <span className="small">{x.user_agent || '—'}</span> },
                { key: 'c', header: api.t('tracker-bridge.ui.denied.count'), right: true, render: (x) => <span className="num">{x.count.toLocaleString()}</span> },
                {
                  key: 'a',
                  header: '',
                  right: true,
                  render: (x) =>
                    !x.overflow && (
                      <Button
                        size="xs"
                        onClick={() =>
                          onDraft({
                            name: (x.user_agent || x.peer_id_prefix).slice(0, 64),
                            action: 'allow',
                            pid_prefix: x.peer_id_prefix,
                            enabled: true,
                            note: '',
                          })
                        }
                      >
                        {api.t('tracker-bridge.ui.denied.allow')}
                      </Button>
                    ),
                },
              ]}
            />
          </Panel>
        </>
      )}
    </div>
  );
}

/** 后台入口：三个页签共用规则草稿（在“被拒客户端”生成的规则带到“客户端规则”继续编辑） */
function Admin({ api, path }: { api: PluginApi; path: string }) {
  const [draft, setDraft] = useState<Draft | null>(null);
  const [editing, setEditing] = useState<{ index: number | null; rule: Rule } | null>(null);
  if (path.startsWith(TAB_PATH.clients)) return <Clients api={api} draft={draft} setDraft={setDraft} editing={editing} setEditing={setEditing} />;
  if (path.startsWith(TAB_PATH.denied))
    return (
      <Denied
        api={api}
        onDraft={(rule) => {
          setEditing({ index: null, rule });
          api.toast(api.t('tracker-bridge.ui.denied.drafted'));
          api.navigate(TAB_PATH.clients);
        }}
      />
    );
  return <Nodes api={api} />;
}

export default {
  routes: [{ path: '/admin/p/tracker-bridge/*', component: Admin }],
} satisfies PluginFrontend;
