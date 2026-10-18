// promotion 前端：种子列表行徽标（批量查询）与详情页促销状态；后台促销管理（/admin/p/promotion/：全站活动、自动规则）、
// 后台用户详情页“促销卡”标签（发放 / 扣除）。
import { useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '@pt-studio/sdk';
import { registerMessages } from '@pt-studio/terms';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { Badge, Button, Card, Field, Input, Table, Tabs } from '@pt-studio/ui-kit';

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

export default {
  slots: {
    'torrent.list.row_badges': { component: RowBadge, order: 10 },
    'torrent.detail.header': { component: Header, order: 10 },
    'admin.user.tabs': { component: UserTab, order: 35, title_term: 'promotion.ui.tab' },
  },
  routes: [{ path: '/admin/p/promotion/*', component: Admin }],
} satisfies PluginFrontend;
