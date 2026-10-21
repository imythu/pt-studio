// hnr 前端：个人页考核统计（本人）；种子页考核要求标记；我的 H&R（/p/hnr/，数字带 + 按状态筛选的考核记录）；
// 个人页与后台用户详情页「H&R」页签（他人记录需要 hnr.read，可在抽屉里填写原因免除单条记录）。
import { useCallback, useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import { formatDuration, registerMessages } from '@pt-studio/terms';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { Button, Field, Input, Kpis, LoadMore, NavItem, Note, PageHead, Panel, PLink, Progress, Sheet, SkeletonRows, Stat, Status, Table, Tabs, Tag, type Tone } from '@pt-studio/ui-kit';

interface Status_ {
  active: number;
  failed: number;
  max: number;
  can_download: boolean;
}

interface Rec {
  id: number;
  torrent_id: number;
  title: string | null;
  state: string;
  completed_at: number;
  deadline: number;
  seed_secs: number;
  required_seed_secs: number;
  ratio: number | null;
  required_ratio: number | null;
  exempt_rule: string | null;
}

const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e);

function HnrStat({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const [s, setS] = useState<Status_ | null>(null);
  const self = api.user()?.id === ctx.profile.id;
  useEffect(() => {
    if (self) void api.json<Status_>('/api/p/hnr/v1/me').then(setS).catch(() => undefined);
  }, [api, self]);
  if (!s) return null;
  return (
    <Stat
      label="H&R"
      to="/p/hnr/"
      navigate={api.navigate}
      value={
        <>
          {s.active} / <span className={s.failed > 0 ? 'down' : undefined}>{s.failed}</span>
        </>
      }
      hint={api.t('hnr.ui.summary', { max: s.max })}
    />
  );
}

interface Requirement {
  required: boolean;
  exempt_rule: string | null;
  min_seed_hours: number;
  window_days: number;
}

function HnrNotice({ ctx, api }: SlotProps<{ torrent: { id: number } }>) {
  const [r, setR] = useState<Requirement | null>(null);
  useEffect(() => {
    void api.json<Requirement>(`/api/p/hnr/v1/torrents/${ctx.torrent.id}/requirement`).then(setR).catch(() => undefined);
  }, [api, ctx.torrent.id]);
  if (!r) return null;
  return r.required ? (
    <Status tone="warn" title={api.t('hnr.ui.required_hint', { hours: r.min_seed_hours, days: r.window_days })}>
      {api.t('hnr.ui.required', { hours: r.min_seed_hours })}
    </Status>
  ) : (
    <Tag tone="k">{api.t('hnr.ui.exempt')}</Tag>
  );
}

const STATES = ['', 'active', 'failed', 'satisfied', 'forgiven', 'exempted'];
const TONE: Record<string, Tone> = { active: 'warn', failed: 'bad', satisfied: 'ok', forgiven: 'info', exempted: 'info' };

/** 考核记录表：`uid` 为空时读本人（/me/records）；他人记录可免除 */
function Records({ api, uid, flushTabs }: { api: PluginApi; uid: number | null; flushTabs?: boolean }) {
  const [state, setState] = useState('');
  const [rows, setRows] = useState<Rec[] | null>(null);
  const [next, setNext] = useState<string | null>(null);
  const [denied, setDenied] = useState(false);
  const [forgiving, setForgiving] = useState<Rec | null>(null);
  const [reason, setReason] = useState('');
  const base = uid === null ? '/api/p/hnr/v1/me/records' : `/api/p/hnr/v1/users/${uid}/records`;
  const load = useCallback(
    async (cursor: string | null) => {
      const q = new URLSearchParams();
      if (state) q.set('state', state);
      if (cursor) q.set('cursor', cursor);
      try {
        const r = await api.json<{ items: Rec[]; next_cursor: string | null }>(`${base}?${q.toString()}` as `/api/${string}`);
        setRows((old) => (cursor ? [...(old ?? []), ...r.items] : r.items));
        setNext(r.next_cursor);
      } catch (e) {
        if (e instanceof ApiError && e.status === 403) setDenied(true);
        else api.toast(errText(api, e), 'error');
      }
    },
    [api, base, state],
  );
  useEffect(() => {
    void load(null);
  }, [load]);
  const forgive = async () => {
    if (!forgiving || !reason.trim()) return;
    try {
      await api.json(`/api/p/hnr/v1/records/${forgiving.id}/forgive`, { json: { reason: reason.trim() } });
      api.toast(api.t('hnr.ui.forgiven'), 'success');
      setForgiving(null);
      setReason('');
      await load(null);
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  if (denied) return <Note>{api.t('hnr.ui.denied')}</Note>;
  return (
    <>
      <Tabs variant={flushTabs ? 'flush' : undefined} value={state} onChange={setState} tabs={STATES.map((s) => ({ id: s, label: api.t(`hnr.ui.state.${s || 'all'}`) }))} />
      {rows === null ? (
        <SkeletonRows n={4} />
      ) : (
        <Table
          rows={rows}
          rowKey={(r) => r.id}
          empty={api.t('hnr.ui.empty')}
          minWidth={820}
          columns={[
            { key: 't', header: api.t('hnr.ui.torrent'), render: (r) => <PLink to={`/torrents/${r.torrent_id}`} navigate={api.navigate} className="tt-name">{r.title ?? `#${r.torrent_id}`}</PLink> },
            { key: 's', header: api.t('hnr.ui.state'), render: (r) => <Status tone={TONE[r.state] ?? 'mute'}>{api.t(`hnr.ui.state.${r.state}`)}</Status> },
            {
              key: 'p',
              header: api.t('hnr.ui.seed'),
              render: (r) => (
                <div className="hnr-prog">
                  <span className="tiny num">
                    {formatDuration(r.seed_secs)} / {formatDuration(r.required_seed_secs)}
                  </span>
                  <Progress value={Math.min(r.seed_secs, r.required_seed_secs)} max={Math.max(r.required_seed_secs, 1)} label={api.t('hnr.ui.seed')} tone={r.seed_secs >= r.required_seed_secs ? 'ok' : undefined} />
                </div>
              ),
            },
            { key: 'r', header: api.t('hnr.ui.ratio'), right: true, render: (r) => <span className="num">{r.ratio === null ? '—' : `${r.ratio.toFixed(2)}${r.required_ratio ? ` / ${r.required_ratio}` : ''}`}</span> },
            { key: 'd', header: api.t('hnr.ui.deadline'), render: (r) => <span className="small muted nowrap">{r.state === 'active' ? new Date(r.deadline).toLocaleString() : (r.exempt_rule ?? '')}</span> },
            {
              key: 'x',
              header: '',
              render: (r) =>
                uid !== null && (r.state === 'active' || r.state === 'failed') ? (
                  <div className="acts">
                    <Button size="xs" variant="ghost" onClick={() => setForgiving(r)}>
                      {api.t('hnr.ui.forgive')}
                    </Button>
                  </div>
                ) : null,
            },
          ]}
        />
      )}
      {next && <LoadMore onClick={() => void load(next)} label={api.t('hnr.ui.more')} />}
      <Sheet
        open={!!forgiving}
        onClose={() => setForgiving(null)}
        title={api.t('hnr.ui.forgive')}
        footer={
          <Button variant="primary" disabled={!reason.trim()} onClick={() => void forgive()}>
            {api.t('hnr.ui.forgive')}
          </Button>
        }
      >
        {forgiving && <p className="small">{forgiving.title ?? `#${forgiving.torrent_id}`}</p>}
        <Field label={api.t('hnr.ui.forgive_prompt')}>
          <Input value={reason} maxLength={200} onChange={(e) => setReason(e.target.value)} />
        </Field>
      </Sheet>
    </>
  );
}

type Me = Status_ & { rules: { window_days: number; min_seed_hours: number; min_torrent_ratio: number } };

/** 我的 H&R（/p/hnr/） */
function MyPage({ api }: { api: PluginApi }) {
  const [s, setS] = useState<Me | null>(null);
  useEffect(() => {
    void api.json<Me>('/api/p/hnr/v1/me').then(setS);
  }, [api]);
  return (
    <div className="stack">
      <PageHead
        title={api.t('hnr.ui.title')}
        lede={s ? `${api.t('hnr.ui.rules', { days: s.rules.window_days, hours: s.rules.min_seed_hours })}${s.rules.min_torrent_ratio > 0 ? api.t('hnr.ui.rules_ratio', { ratio: s.rules.min_torrent_ratio }) : ''}。` : undefined}
      />
      {s && (
        <Kpis
          items={[
            { label: api.t('hnr.ui.state.active'), value: s.active, tone: s.active > 0 ? 'gold' : undefined },
            { label: api.t('hnr.ui.state.failed'), value: `${s.failed} / ${s.max}`, tone: s.failed > 0 ? 'down' : undefined, hint: api.t('hnr.ui.limit_hint') },
            { label: api.t('hnr.ui.download'), value: s.can_download ? api.t('hnr.ui.allowed') : api.t('hnr.ui.blocked'), tone: s.can_download ? 'up' : 'down' },
          ]}
        />
      )}
      <Panel flush primary>
        <Records api={api} uid={null} flushTabs />
      </Panel>
    </div>
  );
}

/** 个人页 / 后台用户详情页「H&R」页签 */
function UserTab({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const self = api.user()?.id === ctx.profile.id;
  return <Records key={ctx.profile.id} api={api} uid={self ? null : ctx.profile.id} />;
}

function MenuItem({ api }: SlotProps) {
  const [s, setS] = useState<Status_ | null>(null);
  useEffect(() => {
    void api.json<Status_>('/api/p/hnr/v1/me').then(setS).catch(() => undefined);
  }, [api]);
  return (
    <NavItem to="/p/hnr/" navigate={api.navigate} count={s ? `${s.active} / ${s.failed}` : null}>
      {api.t('hnr.ui.menu')}
    </NavItem>
  );
}

registerMessages('zh-CN', {
  'hnr.ui.summary': '考核中 / 已计入（上限 {max}）',
  'hnr.ui.required': 'H&R：需做种 {hours} 小时',
  'hnr.ui.required_hint': '下载完成后 {days} 天内做种满 {hours} 小时',
  'hnr.ui.exempt': '免 H&R',
  'hnr.ui.title': '我的 H&R',
  'hnr.ui.menu': 'H&R',
  'hnr.ui.tab': 'H&R',
  'hnr.ui.torrent': '种子',
  'hnr.ui.state': '状态',
  'hnr.ui.state.all': '全部',
  'hnr.ui.state.active': '考核中',
  'hnr.ui.state.failed': '已计入',
  'hnr.ui.state.satisfied': '已达标',
  'hnr.ui.state.forgiven': '已免除',
  'hnr.ui.state.exempted': '已豁免',
  'hnr.ui.seed': '做种时长',
  'hnr.ui.ratio': '分享率',
  'hnr.ui.deadline': '截止 / 说明',
  'hnr.ui.download': '下载权限',
  'hnr.ui.allowed': '正常',
  'hnr.ui.blocked': '已禁止',
  'hnr.ui.limit_hint': '达到上限后禁止下载',
  'hnr.ui.rules': '规则：下载完成后 {days} 天内做种满 {hours} 小时',
  'hnr.ui.rules_ratio': '，或单种分享率达到 {ratio}',
  'hnr.ui.empty': '没有记录',
  'hnr.ui.more': '加载更多',
  'hnr.ui.denied': '只有本人和管理员可以查看 H&R 记录',
  'hnr.ui.forgive': '免除',
  'hnr.ui.forgive_prompt': '免除原因',
  'hnr.ui.forgiven': '已免除',
  'hnr.record_not_found': '记录不存在',
});
registerMessages('en', {
  'hnr.ui.summary': 'In progress / counted (limit {max})',
  'hnr.ui.required': 'H&R: seed {hours}h',
  'hnr.ui.required_hint': 'Seed {hours} hours within {days} days after completing',
  'hnr.ui.exempt': 'No H&R',
  'hnr.ui.title': 'My H&R',
  'hnr.ui.menu': 'H&R',
  'hnr.ui.tab': 'H&R',
  'hnr.ui.torrent': 'Torrent',
  'hnr.ui.state': 'State',
  'hnr.ui.state.all': 'All',
  'hnr.ui.state.active': 'In progress',
  'hnr.ui.state.failed': 'Counted',
  'hnr.ui.state.satisfied': 'Satisfied',
  'hnr.ui.state.forgiven': 'Forgiven',
  'hnr.ui.state.exempted': 'Exempted',
  'hnr.ui.seed': 'Seeding',
  'hnr.ui.ratio': 'Ratio',
  'hnr.ui.deadline': 'Deadline / note',
  'hnr.ui.download': 'Downloading',
  'hnr.ui.allowed': 'allowed',
  'hnr.ui.blocked': 'blocked',
  'hnr.ui.limit_hint': 'Downloads stop at the limit',
  'hnr.ui.rules': 'Rule: seed {hours} hours within {days} days after completing',
  'hnr.ui.rules_ratio': ', or reach a ratio of {ratio} on the torrent',
  'hnr.ui.empty': 'No records',
  'hnr.ui.more': 'Load more',
  'hnr.ui.denied': 'Only the user and staff can see H&R records',
  'hnr.ui.forgive': 'Forgive',
  'hnr.ui.forgive_prompt': 'Reason',
  'hnr.ui.forgiven': 'Forgiven',
  'hnr.record_not_found': 'Record not found',
});

export default {
  slots: {
    'user.profile.stats': { component: HnrStat, order: 40 },
    'user.profile.tabs': { component: UserTab, order: 30, title_term: 'hnr.ui.tab' },
    'torrent.detail.header': { component: HnrNotice, order: 50 },
    'nav.user_menu': { component: MenuItem, order: 45 },
    'admin.user.tabs': { component: UserTab, order: 40, title_term: 'hnr.ui.tab' },
  },
  routes: [{ path: '/p/hnr/*', component: MyPage }],
} satisfies PluginFrontend;
