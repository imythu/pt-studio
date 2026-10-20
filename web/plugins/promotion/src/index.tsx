// promotion 前端：种子列表行与详情页的优惠印记（批量查询，带剩余时间）；种子页「使用促销卡」、我的促销卡（/p/promotion/）与导航入口；
// 后台促销管理（/admin/p/promotion/：全站活动、自动规则，新建都在抽屉里）、后台用户详情「促销卡」页签（发放 / 扣除）、
// 种子页管理区「促销」（单种手动促销：设置 / 取消）。
import { useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '@pt-studio/sdk';
import { registerMessages } from '@pt-studio/terms';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { Button, Field, Input, Kpis, NavItem, Note, PageHead, Panel, PLink, Seal, Select, Sheet, SkeletonRows, Status, Table, Tabs, Textarea, type SealKind } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'promotion.ui.until': '至 {at}',
  'promotion.ui.admin': '促销管理',
  'promotion.ui.events': '全站活动',
  'promotion.ui.rules': '自动规则',
  'promotion.ui.new_event': '发起全站活动',
  'promotion.ui.kind': '类型',
  'promotion.ui.starts': '开始（留空立即）',
  'promotion.ui.ends': '结束',
  'promotion.ui.title_zh': '标题（中文）',
  'promotion.ui.title_en': '标题（英文）',
  'promotion.ui.create': '创建',
  'promotion.ui.end': '提前结束',
  'promotion.ui.ended': '已结束',
  'promotion.ui.new_rule': '新建自动规则（JSON）',
  'promotion.ui.enable': '启用',
  'promotion.ui.disable': '停用',
  'promotion.ui.disabled': '已停用',
  'promotion.ui.rule_hint': '字段：size、age_hours、type_id、status、tags、seeders、leechers、completed、file_count；比较：gte、gt、lte、lt、eq、ne、in、nin；duration.mode 为 fixed（hours 小时）或 while_matched（max_hours 上限）。停用后不再匹配新种子，已生效的促销按退出条件或到期结束。',
  'promotion.ui.tab': '促销卡',
  'promotion.ui.card_kind': '卡类型',
  'promotion.ui.delta': '数量（正数发放，负数扣除）',
  'promotion.ui.reason': '原因',
  'promotion.ui.grant': '调整',
  'promotion.ui.balance': '当前余额 {n}',
  'promotion.ui.done': '已保存',
  'promotion.ui.menu': '促销卡',
  'promotion.ui.admin_lede': '全站活动对所有种子生效；自动规则按条件给新种子加促销。',
  'promotion.ui.rules_n': '{n} 条规则',
  'promotion.ui.days_hours': '{d}天{h}时',
  'promotion.ui.hours_left': '{h}时',
  'promotion.ui.minutes_left': '{m}分',
  'promotion.ui.left': '剩余',
  'promotion.ui.no_events': '还没有全站活动',
  'promotion.ui.no_rules': '还没有自动规则',
  'promotion.ui.state': '状态',
  'promotion.ui.active': '进行中',
  'promotion.ui.window': '时间',
  'promotion.ui.tab_torrent': '促销',
  'promotion.ui.my_cards': '我的促销卡',
  'promotion.ui.my_cards_hint': '促销卡可在商店购买，或由管理员发放。对一个种子使用后，只对你本人生效。',
  'promotion.ui.active_cards': '生效中的卡',
  'promotion.ui.torrent': '种子',
  'promotion.ui.no_active': '没有生效中的卡',
  'promotion.ui.use_card': '使用促销卡',
  'promotion.ui.apply': '使用',
  'promotion.ui.applied': '已使用，{at} 前有效',
  'promotion.ui.redundant': '当前倍率已经包含这张卡的效果，仍要使用吗？',
  'promotion.ui.current': '当前促销',
  'promotion.ui.none': '无',
  'promotion.ui.my_rate': '你的实际倍率：上传 {up}× · 下载 {down}×',
  'promotion.ui.set_manual': '设置手动促销',
  'promotion.ui.manual_hint': '手动促销会替换该种子已有的手动促销，并结束自动规则产生的促销。',
  'promotion.ui.up_permille': '上传倍率（‰，1000 = 1×）',
  'promotion.ui.down_permille': '下载倍率（‰，0 = 免费）',
  'promotion.ui.hours': '持续小时（留空为永久）',
  'promotion.ui.set': '设置',
  'promotion.ui.end_manual': '取消手动促销',
  'promotion.no_card': '没有这种促销卡',
  'promotion.card_not_applicable': '该种子不能使用促销卡（未通过审核或已删除）',
  'promotion.card_already_applied': '这张卡已经在该种子上生效',
  'promotion.not_found': '该种子没有手动促销',
  'promotion.rate_invalid': '自定义倍率无效：上传 ≥ 1000‰，下载 0–1000‰',
});
registerMessages('en', {
  'promotion.ui.until': 'until {at}',
  'promotion.ui.admin': 'Promotions',
  'promotion.ui.events': 'Site events',
  'promotion.ui.rules': 'Auto rules',
  'promotion.ui.new_event': 'Start a site event',
  'promotion.ui.kind': 'Kind',
  'promotion.ui.starts': 'Starts (blank = now)',
  'promotion.ui.ends': 'Ends',
  'promotion.ui.title_zh': 'Title (Chinese)',
  'promotion.ui.title_en': 'Title (English)',
  'promotion.ui.create': 'Create',
  'promotion.ui.end': 'End now',
  'promotion.ui.ended': 'ended',
  'promotion.ui.new_rule': 'New auto rule (JSON)',
  'promotion.ui.enable': 'Enable',
  'promotion.ui.disable': 'Disable',
  'promotion.ui.disabled': 'disabled',
  'promotion.ui.rule_hint': 'Fields: size, age_hours, type_id, status, tags, seeders, leechers, completed, file_count; ops: gte, gt, lte, lt, eq, ne, in, nin; duration.mode is fixed (hours) or while_matched (max_hours cap). Disabled rules stop matching new torrents; active promotions end by their exit conditions or expiry.',
  'promotion.ui.tab': 'Promo cards',
  'promotion.ui.card_kind': 'Card',
  'promotion.ui.delta': 'Amount (positive grants, negative deducts)',
  'promotion.ui.reason': 'Reason',
  'promotion.ui.grant': 'Adjust',
  'promotion.ui.balance': 'Balance {n}',
  'promotion.ui.done': 'Saved',
  'promotion.ui.menu': 'Promo cards',
  'promotion.ui.admin_lede': 'Site events apply to every torrent; auto rules add promotions to new torrents that match.',
  'promotion.ui.rules_n': '{n} rules',
  'promotion.ui.days_hours': '{d}d {h}h',
  'promotion.ui.hours_left': '{h}h',
  'promotion.ui.minutes_left': '{m}m',
  'promotion.ui.left': 'left',
  'promotion.ui.no_events': 'No site events yet',
  'promotion.ui.no_rules': 'No auto rules yet',
  'promotion.ui.state': 'State',
  'promotion.ui.active': 'Active',
  'promotion.ui.window': 'Window',
  'promotion.ui.tab_torrent': 'Promotion',
  'promotion.ui.my_cards': 'My promo cards',
  'promotion.ui.my_cards_hint': 'Buy cards in the shop or get them from staff. A card applied to a torrent only affects you.',
  'promotion.ui.active_cards': 'Active cards',
  'promotion.ui.torrent': 'Torrent',
  'promotion.ui.no_active': 'No active cards',
  'promotion.ui.use_card': 'Use a promo card',
  'promotion.ui.apply': 'Use',
  'promotion.ui.applied': 'Applied, valid until {at}',
  'promotion.ui.redundant': 'Your current rates already cover this card. Use it anyway?',
  'promotion.ui.current': 'Current promotion',
  'promotion.ui.none': 'none',
  'promotion.ui.my_rate': 'Your effective rates: upload {up}× · download {down}×',
  'promotion.ui.set_manual': 'Set a manual promotion',
  'promotion.ui.manual_hint': 'A manual promotion replaces any existing manual one on this torrent and ends promotions from auto rules.',
  'promotion.ui.up_permille': 'Upload rate (‰, 1000 = 1×)',
  'promotion.ui.down_permille': 'Download rate (‰, 0 = free)',
  'promotion.ui.hours': 'Duration in hours (blank = permanent)',
  'promotion.ui.set': 'Set',
  'promotion.ui.end_manual': 'Cancel manual promotion',
  'promotion.no_card': 'You have no card of this kind',
  'promotion.card_not_applicable': 'Cards cannot be used on this torrent',
  'promotion.card_already_applied': 'This card is already active on the torrent',
  'promotion.not_found': 'This torrent has no manual promotion',
  'promotion.rate_invalid': 'Invalid custom rates: upload ≥ 1000‰, download 0–1000‰',
});

interface State {
  torrent_id: number;
  primary: { kind: string; name_key: string; ends_at: string | null } | null;
  global: { kind: string; name_key: string }[];
}

const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e);
const KINDS = ['free', 'double_up', 'double_up_free', 'half_down', 'thirty_down', 'double_up_half_down', 'neutral'];
const RULE_TEMPLATE = JSON.stringify(
  { name: '新种 24 小时免费', kind: 'free', enter: [{ field: 'age_hours', op: 'lt', value: 1 }], exit: null, duration: { mode: 'fixed', hours: 24 }, priority: 0 },
  null,
  2,
);

/** 促销类型 → 印记样式 */
function sealKind(kind: string): SealKind {
  if (kind === 'free') return 'free';
  if (kind === 'double_up_free') return '2xfree';
  if (kind.startsWith('double_up')) return '2x';
  if (kind === 'half_down') return 'half';
  if (kind === 'thirty_down') return '30';
  return 'other';
}

/** PostgreSQL 的 timestamptz 文本（`2026-10-25 02:00:00.5+00`）→ Date（统一成 ISO 8601，避免各浏览器解析差异） */
const pgDate = (t: string) => new Date(/^\d{4}-\d\d-\d\d \d/.test(t) ? t.replace(' ', 'T').replace(/([+-]\d{2})$/, '$1:00') : t);

/** 剩余时间：2天05时 / 18时 / 40分 */
function left(api: PluginApi, endsAt: string | null): string | undefined {
  if (!endsAt) return undefined;
  const ms = pgDate(endsAt).getTime() - Date.now();
  if (!Number.isFinite(ms) || ms <= 0) return undefined;
  const m = Math.floor(ms / 60000);
  if (m < 60) return api.t('promotion.ui.minutes_left', { m });
  const h = Math.floor(m / 60);
  if (h < 24) return api.t('promotion.ui.hours_left', { h });
  return api.t('promotion.ui.days_hours', { d: Math.floor(h / 24), h: String(h % 24).padStart(2, '0') });
}

function loader(api: PluginApi) {
  return api.batch<number, State>('states', async (ids) => {
    const r = await api.json<{ items: State[] }>(`/api/p/promotion/v1/torrents?ids=${ids.join(',')}`);
    return new Map(r.items.map((s) => [s.torrent_id, s]));
  });
}

function usePromo(api: PluginApi, id: number): State | null {
  const [s, setS] = useState<State | null>(null);
  useEffect(() => {
    let live = true;
    void loader(api)(id)
      .then((v) => live && setS(v ?? null))
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [api, id]);
  return s;
}

/** 印记：单种促销带剩余时间，全站活动只显示类型 */
function Seals({ s, api }: { s: State | null; api: PluginApi }) {
  if (!s) return null;
  return (
    <>
      {s.primary && (
        <Seal kind={sealKind(s.primary.kind)} left={left(api, s.primary.ends_at)} title={s.primary.ends_at ? api.t('promotion.ui.until', { at: pgDate(s.primary.ends_at).toLocaleString() }) : undefined}>
          {api.t(`term:${s.primary.name_key}`)}
        </Seal>
      )}
      {s.global.map((p, i) => (
        <Seal key={i} kind={sealKind(p.kind)}>
          {api.t(`term:${p.name_key}`)}
        </Seal>
      ))}
    </>
  );
}

function RowBadge({ ctx, api }: SlotProps<{ torrent: { id: number } }>) {
  return <Seals s={usePromo(api, ctx.torrent.id)} api={api} />;
}

function Header({ ctx, api }: SlotProps<{ torrent: { id: number } }>) {
  return <Seals s={usePromo(api, ctx.torrent.id)} api={api} />;
}

interface Event {
  id: number;
  kind: string;
  starts_at: number;
  ends_at: number | null;
  title: { 'zh-CN'?: string; en?: string } | null;
  state: string;
}

interface Rule {
  id: number;
  definition: Record<string, unknown>;
  priority: number;
  active: boolean;
}

function Events({ api, creating, onClose }: { api: PluginApi; creating: boolean; onClose(): void }) {
  const [rows, setRows] = useState<Event[] | null>(null);
  const [f, setF] = useState({ kind: 'free', starts: '', ends: '', title_zh: '', title_en: '' });
  const load = () => void api.json<{ items: Event[] }>('/api/p/promotion/v1/admin/events').then((r) => setRows(r.items));
  useEffect(load, []); // eslint-disable-line react-hooks/exhaustive-deps
  const create = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await api.json('/api/p/promotion/v1/admin/events', {
        json: { kind: f.kind, starts_at_ms: f.starts ? new Date(f.starts).getTime() : undefined, ends_at_ms: new Date(f.ends).getTime(), title_zh: f.title_zh || null, title_en: f.title_en || null },
        idempotent: true,
      });
      api.toast(api.t('promotion.ui.done'), 'success');
      onClose();
      load();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  const end = async (id: number) => {
    try {
      await api.json(`/api/p/promotion/v1/admin/events/${id}`, { method: 'DELETE' });
      load();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  return (
    <>
      <Panel flush primary>
        {rows === null ? (
          <SkeletonRows n={3} />
        ) : (
          <Table
            rows={rows}
            rowKey={(r) => r.id}
            empty={api.t('promotion.ui.no_events')}
            minWidth={640}
            columns={[
              { key: 'k', header: api.t('promotion.ui.kind'), render: (r) => <Seal kind={sealKind(r.kind)}>{api.t(`term:promotion.${r.kind}`)}</Seal> },
              { key: 't', header: api.t('promotion.ui.title_zh'), render: (r) => r.title?.['zh-CN'] ?? r.title?.en ?? '—' },
              { key: 'w', header: api.t('promotion.ui.window'), render: (r) => <span className="small muted nowrap">{new Date(r.starts_at).toLocaleString()} → {r.ends_at ? new Date(r.ends_at).toLocaleString() : '∞'}</span> },
              {
                key: 'x',
                header: api.t('promotion.ui.state'),
                render: (r) =>
                  r.state === 'active' ? (
                    <span className="row" style={{ gap: 6 }}>
                      <Status tone="ok">{api.t('promotion.ui.active')}</Status>
                      <Button size="xs" variant="danger" onClick={() => void end(r.id)}>
                        {api.t('promotion.ui.end')}
                      </Button>
                    </span>
                  ) : (
                    <Status tone="mute">{api.t('promotion.ui.ended')}</Status>
                  ),
              },
            ]}
          />
        )}
      </Panel>
      <Sheet
        open={creating}
        onClose={onClose}
        title={api.t('promotion.ui.new_event')}
        footer={
          <Button type="submit" form="promo-event" variant="primary" disabled={!f.ends}>
            {api.t('promotion.ui.create')}
          </Button>
        }
      >
        <form id="promo-event" className="form" onSubmit={(e) => void create(e)}>
          <Field label={api.t('promotion.ui.kind')}>
            <Select name="event_kind" value={f.kind} onChange={(e) => setF({ ...f, kind: e.target.value })} options={KINDS.map((k) => ({ value: k, label: api.t(`term:promotion.${k}`) }))} />
          </Field>
          <div className="form-2">
            <Field label={api.t('promotion.ui.starts')}>
              <Input name="event_starts" type="datetime-local" value={f.starts} onChange={(e) => setF({ ...f, starts: e.target.value })} />
            </Field>
            <Field label={api.t('promotion.ui.ends')}>
              <Input name="event_ends" type="datetime-local" required value={f.ends} onChange={(e) => setF({ ...f, ends: e.target.value })} />
            </Field>
          </div>
          <Field label={api.t('promotion.ui.title_zh')}>
            <Input name="event_title_zh" maxLength={100} value={f.title_zh} onChange={(e) => setF({ ...f, title_zh: e.target.value })} />
          </Field>
          <Field label={api.t('promotion.ui.title_en')}>
            <Input name="event_title_en" maxLength={100} value={f.title_en} onChange={(e) => setF({ ...f, title_en: e.target.value })} />
          </Field>
        </form>
      </Sheet>
    </>
  );
}

function Rules({ api, creating, onClose }: { api: PluginApi; creating: boolean; onClose(): void }) {
  const [rows, setRows] = useState<Rule[] | null>(null);
  const [def, setDef] = useState(RULE_TEMPLATE);
  const load = () => void api.json<{ items: Rule[] }>('/api/p/promotion/v1/admin/rules').then((r) => setRows(r.items));
  useEffect(load, []); // eslint-disable-line react-hooks/exhaustive-deps
  const run = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      api.toast(api.t('promotion.ui.done'), 'success');
      load();
      return true;
    } catch (err) {
      api.toast(errText(api, err), 'error');
      return false;
    }
  };
  const create = async () => {
    let parsed: unknown;
    try {
      parsed = JSON.parse(def);
    } catch (err) {
      api.toast(String(err), 'error');
      return;
    }
    if (await run(() => api.json('/api/p/promotion/v1/admin/rules', { json: parsed }))) onClose();
  };
  return (
    <>
      <Panel flush primary>
        {rows === null ? (
          <SkeletonRows n={3} />
        ) : (
          <Table
            rows={rows}
            rowKey={(r) => r.id}
            empty={api.t('promotion.ui.no_rules')}
            minWidth={820}
            columns={[
              { key: 'n', header: '', render: (r) => <b>{String(r.definition.name ?? `#${r.id}`)}</b> },
              { key: 'k', header: api.t('promotion.ui.kind'), render: (r) => <Seal kind={sealKind(String(r.definition.kind))}>{api.t(`term:promotion.${String(r.definition.kind)}`)}</Seal> },
              { key: 'd', header: '', render: (r) => <code className="mono">{JSON.stringify(r.definition.enter)}</code> },
              {
                key: 'x',
                header: api.t('promotion.ui.state'),
                render: (r) => (
                  <span className="row" style={{ gap: 6 }}>
                    <Status tone={r.active ? 'ok' : 'mute'}>{r.active ? api.t('promotion.ui.enable') : api.t('promotion.ui.disabled')}</Status>
                    <Button size="xs" variant="ghost" onClick={() => void run(() => api.json(`/api/p/promotion/v1/admin/rules/${r.id}`, { method: 'PATCH', json: { active: !r.active } }))}>
                      {api.t(r.active ? 'promotion.ui.disable' : 'promotion.ui.enable')}
                    </Button>
                  </span>
                ),
              },
            ]}
          />
        )}
      </Panel>
      <Sheet
        open={creating}
        onClose={onClose}
        title={api.t('promotion.ui.new_rule')}
        width={620}
        footer={
          <Button variant="primary" onClick={() => void create()}>
            {api.t('promotion.ui.create')}
          </Button>
        }
      >
        <Note>{api.t('promotion.ui.rule_hint')}</Note>
        <Textarea className="code" name="rule_definition" rows={16} value={def} onChange={(e) => setDef(e.target.value)} />
      </Sheet>
    </>
  );
}

function Admin({ api }: { api: PluginApi }) {
  const [tab, setTab] = useState('events');
  const [creating, setCreating] = useState(false);
  return (
    <div className="stack">
      <PageHead
        title={api.t('promotion.ui.admin')}
        lede={api.t('promotion.ui.admin_lede')}
        actions={
          <Button size="sm" variant="primary" onClick={() => setCreating(true)}>
            {api.t(tab === 'events' ? 'promotion.ui.new_event' : 'promotion.ui.new_rule')}
          </Button>
        }
      />
      <Tabs
        variant="sub"
        value={tab}
        onChange={(v) => {
          setTab(v);
          setCreating(false);
        }}
        tabs={[
          { id: 'events', label: api.t('promotion.ui.events') },
          { id: 'rules', label: api.t('promotion.ui.rules') },
        ]}
      />
      {tab === 'events' ? <Events api={api} creating={creating} onClose={() => setCreating(false)} /> : <Rules api={api} creating={creating} onClose={() => setCreating(false)} />}
    </div>
  );
}

/** 后台用户详情页「促销卡」页签 */
function UserTab({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const [f, setF] = useState({ card_kind: 'free', delta: '1', reason: '' });
  const [bal, setBal] = useState<number | null>(null);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      const r = await api.json<{ balance: number }>(`/api/p/promotion/v1/admin/users/${ctx.profile.id}/cards`, {
        json: { card_kind: f.card_kind, delta: Number(f.delta), reason: f.reason || 'admin' },
        idempotent: true,
      });
      setBal(r.balance);
      api.toast(api.t('promotion.ui.done'), 'success');
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  return (
    <form onSubmit={(e) => void submit(e)} className="form-2 narrow-form">
      <Field label={api.t('promotion.ui.card_kind')}>
        <Select name="card_kind" value={f.card_kind} onChange={(e) => setF({ ...f, card_kind: e.target.value })} options={CARD_KINDS.map((k) => ({ value: k, label: api.t(`term:promotion.card.${k}`) }))} />
      </Field>
      <Field label={api.t('promotion.ui.delta')}>
        <Input name="card_delta" type="number" value={f.delta} onChange={(e) => setF({ ...f, delta: e.target.value })} />
      </Field>
      <Field label={api.t('promotion.ui.reason')} className="span2">
        <Input name="card_reason" maxLength={200} value={f.reason} onChange={(e) => setF({ ...f, reason: e.target.value })} />
      </Field>
      <div className="span2 row">
        <Button type="submit" variant="primary" size="sm" disabled={!Number(f.delta)}>
          {api.t('promotion.ui.grant')}
        </Button>
        {bal !== null && <span className="small muted">{api.t('promotion.ui.balance', { n: bal })}</span>}
      </div>
    </form>
  );
}

const CARD_KINDS = ['free', 'double_up'] as const;

interface Cards {
  balances: { card_kind: string; balance: number }[];
  active: { torrent_id: number; card_kind: string; ends_at: string | null }[];
}

function useCards(api: PluginApi): [Cards | null, () => void] {
  const [c, setC] = useState<Cards | null>(null);
  const load = () =>
    void api
      .json<Cards>('/api/p/promotion/v1/me/cards')
      .then(setC)
      .catch(() => setC(null));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, [api]);
  return [c, load];
}

const owned = (c: Cards | null) => c?.balances.reduce((n, b) => n + b.balance, 0) ?? 0;

/** 导航入口：只在持有或正在使用促销卡时显示 */
function MenuItem({ api }: SlotProps) {
  const [c] = useCards(api);
  if (!c || (owned(c) === 0 && c.active.length === 0)) return null;
  return (
    <NavItem to="/p/promotion/" navigate={api.navigate} count={owned(c)}>
      {api.t('promotion.ui.menu')}
    </NavItem>
  );
}

/** 种子页操作区：对已通过的种子使用个人免费卡 / 2X 卡（没有卡时不显示）；选卡与确认在抽屉里 */
function CardAction({ ctx, api }: SlotProps<{ torrent: { id: number; status?: string } }>) {
  const [c, reload] = useCards(api);
  const [open, setOpen] = useState(false);
  const [kind, setKind] = useState('');
  const [busy, setBusy] = useState(false);
  const [redundant, setRedundant] = useState(false);
  const have = (c?.balances ?? []).filter((b) => b.balance > 0);
  if (ctx.torrent.status !== undefined && ctx.torrent.status !== 'approved') return null;
  if (have.length === 0) return null;
  const pick = have.some((b) => b.card_kind === kind) ? kind : (have[0] as { card_kind: string }).card_kind;
  const apply = async (confirm: boolean) => {
    setBusy(true);
    try {
      const r = await api.json<{ ends_at: number | null }>('/api/p/promotion/v1/me/cards/apply', { json: { torrent_id: ctx.torrent.id, card_kind: pick, confirm }, idempotent: true });
      api.toast(api.t('promotion.ui.applied', { at: r.ends_at ? new Date(r.ends_at).toLocaleString() : '∞' }), 'success');
      setOpen(false);
      setRedundant(false);
      reload();
    } catch (e) {
      if (e instanceof ApiError && e.code === 'promotion.card_redundant' && !confirm) setRedundant(true);
      else api.toast(errText(api, e), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <Button onClick={() => setOpen(true)}>{api.t('promotion.ui.use_card')}</Button>
      <Sheet
        open={open}
        onClose={() => setOpen(false)}
        title={api.t('promotion.ui.use_card')}
        footer={
          <Button variant="primary" disabled={busy} onClick={() => void apply(redundant)}>
            {api.t('promotion.ui.apply')}
          </Button>
        }
      >
        <p className="small muted">{api.t('promotion.ui.my_cards_hint')}</p>
        <div className="chips" role="radiogroup" aria-label={api.t('promotion.ui.card_kind')}>
          {have.map((b) => (
            <button
              key={b.card_kind}
              type="button"
              role="radio"
              className="chip"
              aria-checked={pick === b.card_kind}
              onClick={() => {
                setKind(b.card_kind);
                setRedundant(false);
              }}
            >
              {api.t(`term:promotion.card.${b.card_kind}`)} × {b.balance}
            </button>
          ))}
        </div>
        {redundant && <Note tone="warn">{api.t('promotion.ui.redundant')}</Note>}
      </Sheet>
    </>
  );
}

/** 我的促销卡（/p/promotion/）：各类卡余额与生效中的卡 */
function MyCards({ api }: { api: PluginApi }) {
  const [c] = useCards(api);
  return (
    <div className="stack">
      <PageHead title={api.t('promotion.ui.my_cards')} lede={api.t('promotion.ui.my_cards_hint')} />
      <Kpis items={CARD_KINDS.map((k) => ({ label: api.t(`term:promotion.card.${k}`), value: c ? (c.balances.find((b) => b.card_kind === k)?.balance ?? 0) : '…', tone: 'gold' as const }))} />
      <Panel title={api.t('promotion.ui.active_cards')} flush primary>
        <Table
          rows={c?.active ?? []}
          rowKey={(r) => `${r.torrent_id}:${r.card_kind}`}
          empty={api.t('promotion.ui.no_active')}
          columns={[
            { key: 't', header: api.t('promotion.ui.torrent'), render: (r) => <PLink to={`/torrents/${r.torrent_id}`} navigate={api.navigate} className="link">#{r.torrent_id}</PLink> },
            { key: 'k', header: api.t('promotion.ui.card_kind'), render: (r) => <Seal kind={sealKind(r.card_kind)}>{api.t(`term:promotion.card.${r.card_kind}`)}</Seal> },
            { key: 'e', header: api.t('promotion.ui.ends'), render: (r) => <span className="small nowrap">{r.ends_at ? pgDate(r.ends_at).toLocaleString() : '∞'}</span> },
          ]}
        />
      </Panel>
    </div>
  );
}

interface FullState extends State {
  combined_up_permille: number;
  combined_down_permille: number;
  mine: { up_permille: number; down_permille: number } | null;
}

/** 种子页管理区「促销」：当前促销与单种手动促销（设置 / 取消） */
function TorrentTab({ ctx, api }: SlotProps<{ torrent: { id: number } }>) {
  const tid = ctx.torrent.id;
  const [s, setS] = useState<FullState | null>(null);
  const [f, setF] = useState({ kind: 'free', up: '2000', down: '0', hours: '24', reason: '' });
  const load = () => void api.json<FullState>(`/api/p/promotion/v1/torrents/${tid}`).then(setS).catch(() => setS(null));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, [tid]);
  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      api.toast(api.t('promotion.ui.done'), 'success');
      load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const submit = (e: FormEvent) => {
    e.preventDefault();
    const custom = f.kind === 'custom';
    void act(() =>
      api.json('/api/p/promotion/v1/torrents/promotions', {
        json: {
          torrent_ids: [tid],
          kind: f.kind,
          up_permille: custom && f.up.trim() ? Number(f.up) : undefined,
          down_permille: custom && f.down.trim() ? Number(f.down) : undefined,
          duration_secs: f.hours.trim() ? Math.round(Number(f.hours) * 3600) : undefined,
          reason: f.reason,
        },
        idempotent: true,
      }),
    );
  };
  if (!s) return null;
  const x = (p: number) => (p / 1000).toLocaleString(undefined, { maximumFractionDigits: 2 });
  return (
    <section className="stack-s">
      <h3>{api.t('promotion.ui.tab_torrent')}</h3>
      <div className="row">
        <span className="small muted">{api.t('promotion.ui.current')}</span>
        {s.primary || s.global.length > 0 ? <Seals s={s} api={api} /> : <span className="small">{api.t('promotion.ui.none')}</span>}
      </div>
      {s.mine && <p className="small muted">{api.t('promotion.ui.my_rate', { up: x(s.mine.up_permille), down: x(s.mine.down_permille) })}</p>}
      <p className="small muted">{api.t('promotion.ui.manual_hint')}</p>
      <form onSubmit={submit} className="form-2">
        <Field label={api.t('promotion.ui.kind')}>
          <Select name="manual_kind" value={f.kind} onChange={(e) => setF({ ...f, kind: e.target.value })} options={[...KINDS, 'custom'].map((k) => ({ value: k, label: api.t(`term:promotion.${k}`) }))} />
        </Field>
        <Field label={api.t('promotion.ui.hours')}>
          <Input name="manual_hours" type="number" min={1} value={f.hours} onChange={(e) => setF({ ...f, hours: e.target.value })} />
        </Field>
        {f.kind === 'custom' && (
          <>
            <Field label={api.t('promotion.ui.up_permille')}>
              <Input name="manual_up" type="number" min={1000} value={f.up} onChange={(e) => setF({ ...f, up: e.target.value })} />
            </Field>
            <Field label={api.t('promotion.ui.down_permille')}>
              <Input name="manual_down" type="number" min={0} max={1000} value={f.down} onChange={(e) => setF({ ...f, down: e.target.value })} />
            </Field>
          </>
        )}
        <Field label={api.t('promotion.ui.reason')} className="span2">
          <Input name="manual_reason" maxLength={200} value={f.reason} onChange={(e) => setF({ ...f, reason: e.target.value })} />
        </Field>
        <div className="span2 row">
          <Button type="submit" size="sm" variant="primary">
            {api.t('promotion.ui.set')}
          </Button>
          <Button type="button" size="sm" variant="ghost" onClick={() => void act(() => api.json(`/api/p/promotion/v1/torrents/${tid}/promotion`, { method: 'DELETE' }))}>
            {api.t('promotion.ui.end_manual')}
          </Button>
        </div>
      </form>
    </section>
  );
}

export default {
  slots: {
    'torrent.list.row_badges': { component: RowBadge, order: 10 },
    'torrent.detail.header': { component: Header, order: 10 },
    'torrent.detail.actions': { component: CardAction, order: 30 },
    'nav.user_menu': { component: MenuItem, order: 25 },
    'admin.user.tabs': { component: UserTab, order: 35, title_term: 'promotion.ui.tab' },
    'admin.torrent.tabs': { component: TorrentTab, order: 20, title_term: 'promotion.ui.tab_torrent' },
  },
  routes: [
    { path: '/p/promotion/*', component: MyCards },
    { path: '/admin/p/promotion/*', component: Admin },
  ],
} satisfies PluginFrontend;
