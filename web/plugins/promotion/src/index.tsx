// promotion 前端：种子列表行徽标（批量查询）与详情页促销状态；种子页“使用促销卡”、我的促销卡（/p/promotion/）与用户菜单入口；
// 后台促销管理（/admin/p/promotion/：全站活动、自动规则）、后台用户详情页“促销卡”标签（发放 / 扣除）、
// 种子页员工区“促销”标签（单种手动促销：设置 / 取消）。
import { useEffect, useState, type FormEvent, type ReactNode } from 'react';
import { ApiError } from '@pt-studio/sdk';
import { registerMessages } from '@pt-studio/terms';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { Badge, Button, Card, Field, Input, Stat, Table, Tabs } from '@pt-studio/ui-kit';

interface State {
  torrent_id: number;
  primary: { kind: string; name_key: string; ends_at: string | null } | null;
  global: { kind: string; name_key: string }[];
}

function loader(api: PluginApi) {
  return api.batch<number, State>('states', async (ids) => {
    const r = await api.json<{ items: State[] }>(`/api/p/promotion/v1/torrents?ids=${ids.join(',')}`);
    return new Map(r.items.map((s) => [s.torrent_id, s]));
  });
}

function tone(kind: string): 'free' | 'double' | 'primary' {
  return kind.includes('free') ? 'free' : kind.includes('double') ? 'double' : 'primary';
}

function useState_(api: PluginApi, id: number): State | null {
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

function PromoBadges({ s, api }: { s: State | null; api: PluginApi }) {
  if (!s) return null;
  const all = [...(s.primary ? [s.primary] : []), ...s.global];
  return (
    <>
      {all.map((p, i) => (
        <Badge key={i} tone={tone(p.kind)}>
          {api.t(`term:${p.name_key}`)}
        </Badge>
      ))}
    </>
  );
}

function RowBadge({ ctx, api }: SlotProps<{ torrent: { id: number } }>) {
  return <PromoBadges s={useState_(api, ctx.torrent.id)} api={api} />;
}

function Header({ ctx, api }: SlotProps<{ torrent: { id: number } }>) {
  const s = useState_(api, ctx.torrent.id);
  if (!s?.primary) return <PromoBadges s={s} api={api} />;
  return (
    <span>
      <PromoBadges s={s} api={api} />
      {s.primary.ends_at && <span className="pt-muted small">{api.t('promotion.ui.until', { at: new Date(s.primary.ends_at).toLocaleString() })}</span>}
    </span>
  );
}

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
  'promotion.ui.menu': '促销卡（{n}）',
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
  'promotion.ui.menu': 'Promo cards ({n})',
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

const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e);
const KINDS = ['free', 'double_up', 'double_up_free', 'half_down', 'thirty_down', 'double_up_half_down', 'neutral'];
const RULE_TEMPLATE = JSON.stringify(
  { name: '新种 24 小时免费', kind: 'free', enter: [{ field: 'age_hours', op: 'lt', value: 1 }], exit: null, duration: { mode: 'fixed', hours: 24 }, priority: 0 },
  null,
  2,
);

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

function Events({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<Event[]>([]);
  const [f, setF] = useState({ kind: 'free', starts: '', ends: '', title_zh: '', title_en: '' });
  const load = () => void api.json<{ items: Event[] }>('/api/p/promotion/v1/admin/events').then((r) => setRows(r.items));
  useEffect(load, []); // eslint-disable-line react-hooks/exhaustive-deps
  const create = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await api.json('/api/p/promotion/v1/admin/events', {
        json: {
          kind: f.kind,
          starts_at_ms: f.starts ? new Date(f.starts).getTime() : undefined,
          ends_at_ms: new Date(f.ends).getTime(),
          title_zh: f.title_zh || null,
          title_en: f.title_en || null,
        },
        idempotent: true,
      });
      api.toast(api.t('promotion.ui.done'), 'success');
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
      <Card title={api.t('promotion.ui.new_event')}>
        <form onSubmit={(e) => void create(e)} className="upload-grid">
          <Field label={api.t('promotion.ui.kind')}>
            <select className="pt-input" name="event_kind" value={f.kind} onChange={(e) => setF({ ...f, kind: e.target.value })}>
              {KINDS.map((k) => (
                <option key={k} value={k}>
                  {api.t(`term:promotion.${k}`)}
                </option>
              ))}
            </select>
          </Field>
          <span />
          <Field label={api.t('promotion.ui.starts')}>
            <Input name="event_starts" type="datetime-local" value={f.starts} onChange={(e) => setF({ ...f, starts: e.target.value })} />
          </Field>
          <Field label={api.t('promotion.ui.ends')}>
            <Input name="event_ends" type="datetime-local" required value={f.ends} onChange={(e) => setF({ ...f, ends: e.target.value })} />
          </Field>
          <Field label={api.t('promotion.ui.title_zh')}>
            <Input name="event_title_zh" maxLength={100} value={f.title_zh} onChange={(e) => setF({ ...f, title_zh: e.target.value })} />
          </Field>
          <Field label={api.t('promotion.ui.title_en')}>
            <Input name="event_title_en" maxLength={100} value={f.title_en} onChange={(e) => setF({ ...f, title_en: e.target.value })} />
          </Field>
          <div className="span2">
            <Button type="submit" variant="primary" disabled={!f.ends}>
              {api.t('promotion.ui.create')}
            </Button>
          </div>
        </form>
      </Card>
      <Card>
        <Table
          rows={rows}
          rowKey={(r) => r.id}
          columns={[
            { key: 'k', header: api.t('promotion.ui.kind'), render: (r) => <Badge tone={tone(r.kind)}>{api.t(`term:promotion.${r.kind}`)}</Badge> },
            { key: 't', header: '', render: (r) => r.title?.['zh-CN'] ?? r.title?.en ?? '' },
            {
              key: 'w',
              header: '',
              render: (r) => (
                <span className="pt-muted small">
                  {new Date(r.starts_at).toLocaleString()} → {r.ends_at ? new Date(r.ends_at).toLocaleString() : '∞'}
                </span>
              ),
            },
            {
              key: 'x',
              header: '',
              render: (r) =>
                r.state === 'active' ? (
                  <Button variant="ghost" onClick={() => void end(r.id)}>
                    {api.t('promotion.ui.end')}
                  </Button>
                ) : (
                  <Badge>{api.t('promotion.ui.ended')}</Badge>
                ),
            },
          ]}
        />
      </Card>
    </>
  );
}

function Rules({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<Rule[]>([]);
  const [def, setDef] = useState(RULE_TEMPLATE);
  const load = () => void api.json<{ items: Rule[] }>('/api/p/promotion/v1/admin/rules').then((r) => setRows(r.items));
  useEffect(load, []); // eslint-disable-line react-hooks/exhaustive-deps
  const run = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      api.toast(api.t('promotion.ui.done'), 'success');
      load();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  const create = () => {
    let parsed: unknown;
    try {
      parsed = JSON.parse(def);
    } catch (err) {
      api.toast(String(err), 'error');
      return;
    }
    void run(() => api.json('/api/p/promotion/v1/admin/rules', { json: parsed }));
  };
  return (
    <>
      <Card>
        <Table
          rows={rows}
          rowKey={(r) => r.id}
          columns={[
            { key: 'n', header: '', render: (r) => <strong>{String(r.definition.name ?? `#${r.id}`)}</strong> },
            { key: 'k', header: api.t('promotion.ui.kind'), render: (r) => <Badge tone={tone(String(r.definition.kind))}>{api.t(`term:promotion.${String(r.definition.kind)}`)}</Badge> },
            { key: 'd', header: '', render: (r) => <code className="small">{JSON.stringify(r.definition.enter)}</code> },
            {
              key: 'x',
              header: '',
              render: (r) => (
                <>
                  {!r.active && <Badge>{api.t('promotion.ui.disabled')}</Badge>}{' '}
                  <Button variant="ghost" onClick={() => void run(() => api.json(`/api/p/promotion/v1/admin/rules/${r.id}`, { method: 'PATCH', json: { active: !r.active } }))}>
                    {api.t(r.active ? 'promotion.ui.disable' : 'promotion.ui.enable')}
                  </Button>
                </>
              ),
            },
          ]}
        />
      </Card>
      <Card title={api.t('promotion.ui.new_rule')}>
        <p className="pt-muted small">{api.t('promotion.ui.rule_hint')}</p>
        <textarea className="pt-input pt-mono" name="rule_definition" rows={10} value={def} onChange={(e) => setDef(e.target.value)} />
        <Button variant="primary" onClick={create}>
          {api.t('promotion.ui.create')}
        </Button>
      </Card>
    </>
  );
}

function Admin({ api }: { api: PluginApi }) {
  const [tab, setTab] = useState('events');
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t('promotion.ui.admin')}</h1>
      <Tabs
        value={tab}
        onChange={setTab}
        tabs={[
          { id: 'events', label: api.t('promotion.ui.events') },
          { id: 'rules', label: api.t('promotion.ui.rules') },
        ]}
      />
      {tab === 'events' ? <Events api={api} /> : <Rules api={api} />}
    </div>
  );
}

/** 后台用户详情页“促销卡”标签 */
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
    <Card>
      <form onSubmit={(e) => void submit(e)} className="searchbar">
        <select className="pt-input" name="card_kind" aria-label={api.t('promotion.ui.card_kind')} value={f.card_kind} onChange={(e) => setF({ ...f, card_kind: e.target.value })}>
          <option value="free">{api.t('term:promotion.card.free')}</option>
          <option value="double_up">{api.t('term:promotion.card.double_up')}</option>
        </select>
        <Input name="card_delta" type="number" aria-label={api.t('promotion.ui.delta')} title={api.t('promotion.ui.delta')} value={f.delta} onChange={(e) => setF({ ...f, delta: e.target.value })} />
        <Input name="card_reason" placeholder={api.t('promotion.ui.reason')} maxLength={200} value={f.reason} onChange={(e) => setF({ ...f, reason: e.target.value })} />
        <Button type="submit" disabled={!Number(f.delta)}>
          {api.t('promotion.ui.grant')}
        </Button>
      </form>
      {bal !== null && <p>{api.t('promotion.ui.balance', { n: bal })}</p>}
    </Card>
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
/** PostgreSQL 的 timestamptz 文本（`2026-10-25 02:00:00.5+00`）→ Date（统一成 ISO 8601，避免各浏览器解析差异） */
const pgDate = (t: string) => new Date(t.replace(' ', 'T').replace(/([+-]\d{2})$/, '$1:00'));

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

/** 用户菜单入口：只在持有或正在使用促销卡时显示 */
function MenuItem({ api }: SlotProps) {
  const [c] = useCards(api);
  if (!c || (owned(c) === 0 && c.active.length === 0)) return null;
  return (
    <A api={api} to="/p/promotion/">
      {api.t('promotion.ui.menu', { n: owned(c) })}
    </A>
  );
}

/** 种子页操作区：对已通过的种子使用个人免费卡 / 2x 卡（没有卡时不显示） */
function CardAction({ ctx, api }: SlotProps<{ torrent: { id: number; status?: string } }>) {
  const [c, reload] = useCards(api);
  const [kind, setKind] = useState('');
  const [busy, setBusy] = useState(false);
  const have = (c?.balances ?? []).filter((b) => b.balance > 0);
  if (ctx.torrent.status !== undefined && ctx.torrent.status !== 'approved') return null;
  if (have.length === 0) return null;
  const pick = have.some((b) => b.card_kind === kind) ? kind : (have[0] as { card_kind: string }).card_kind;
  const apply = async (confirm: boolean) => {
    setBusy(true);
    try {
      const r = await api.json<{ ends_at: number | null }>('/api/p/promotion/v1/me/cards/apply', {
        json: { torrent_id: ctx.torrent.id, card_kind: pick, confirm },
        idempotent: true,
      });
      api.toast(api.t('promotion.ui.applied', { at: r.ends_at ? new Date(r.ends_at).toLocaleString() : '∞' }), 'success');
      reload();
    } catch (e) {
      if (e instanceof ApiError && e.code === 'promotion.card_redundant' && !confirm) {
        if (window.confirm(api.t('promotion.ui.redundant'))) await apply(true);
      } else {
        api.toast(errText(api, e), 'error');
      }
    } finally {
      setBusy(false);
    }
  };
  return (
    <span className="pt-row">
      <select className="pt-input" name="promo_card_kind" aria-label={api.t('promotion.ui.use_card')} value={pick} onChange={(e) => setKind(e.target.value)}>
        {have.map((b) => (
          <option key={b.card_kind} value={b.card_kind}>
            {api.t(`term:promotion.card.${b.card_kind}`)} × {b.balance}
          </option>
        ))}
      </select>
      <Button disabled={busy} onClick={() => void apply(false)}>
        {api.t('promotion.ui.use_card')}
      </Button>
    </span>
  );
}

/** 我的促销卡（/p/promotion/）：各类卡余额与生效中的卡 */
function MyCards({ api }: { api: PluginApi }) {
  const [c] = useCards(api);
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t('promotion.ui.my_cards')}</h1>
      <Card>
        <div className="pt-row stats-row">
          {CARD_KINDS.map((k) => (
            <Stat key={k} label={api.t(`term:promotion.card.${k}`)} value={c ? (c.balances.find((b) => b.card_kind === k)?.balance ?? 0) : '…'} />
          ))}
        </div>
        <p className="pt-muted small">{api.t('promotion.ui.my_cards_hint')}</p>
      </Card>
      <Card title={api.t('promotion.ui.active_cards')}>
        <Table
          rows={c?.active ?? []}
          rowKey={(r) => `${r.torrent_id}:${r.card_kind}`}
          empty={api.t('promotion.ui.no_active')}
          columns={[
            {
              key: 't',
              header: api.t('promotion.ui.torrent'),
              render: (r) => (
                <A api={api} to={`/torrents/${r.torrent_id}`}>
                  #{r.torrent_id}
                </A>
              ),
            },
            { key: 'k', header: api.t('promotion.ui.card_kind'), render: (r) => <Badge tone={tone(r.card_kind)}>{api.t(`term:promotion.card.${r.card_kind}`)}</Badge> },
            { key: 'e', header: api.t('promotion.ui.ends'), render: (r) => (r.ends_at ? pgDate(r.ends_at).toLocaleString() : '∞') },
          ]}
        />
      </Card>
    </div>
  );
}

interface FullState extends State {
  combined_up_permille: number;
  combined_down_permille: number;
  mine: { up_permille: number; down_permille: number } | null;
}

/** 种子页员工区“促销”标签：当前促销与单种手动促销（设置 / 取消） */
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
    <Card>
      <p>
        {api.t('promotion.ui.current')}: {s.primary || s.global.length > 0 ? <PromoBadges s={s} api={api} /> : api.t('promotion.ui.none')}
        {s.primary?.ends_at && <span className="pt-muted small"> {api.t('promotion.ui.until', { at: new Date(s.primary.ends_at).toLocaleString() })}</span>}
      </p>
      {s.mine && <p className="pt-muted small">{api.t('promotion.ui.my_rate', { up: x(s.mine.up_permille), down: x(s.mine.down_permille) })}</p>}
      <h3>{api.t('promotion.ui.set_manual')}</h3>
      <p className="pt-muted small">{api.t('promotion.ui.manual_hint')}</p>
      <form onSubmit={submit} className="upload-grid">
        <Field label={api.t('promotion.ui.kind')}>
          <select className="pt-input" name="manual_kind" value={f.kind} onChange={(e) => setF({ ...f, kind: e.target.value })}>
            {[...KINDS, 'custom'].map((k) => (
              <option key={k} value={k}>
                {api.t(`term:promotion.${k}`)}
              </option>
            ))}
          </select>
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
        <div className="span2">
          <Field label={api.t('promotion.ui.reason')}>
            <Input name="manual_reason" maxLength={200} value={f.reason} onChange={(e) => setF({ ...f, reason: e.target.value })} />
          </Field>
          <span className="pt-row">
            <Button type="submit" variant="primary">
              {api.t('promotion.ui.set')}
            </Button>
            <Button type="button" variant="ghost" onClick={() => void act(() => api.json(`/api/p/promotion/v1/torrents/${tid}/promotion`, { method: 'DELETE' }))}>
              {api.t('promotion.ui.end_manual')}
            </Button>
          </span>
        </div>
      </form>
    </Card>
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
