// medals 前端：个人页头衔与佩戴勋章、勋章墙；“我的勋章”页（佩戴设置、成就进度）；后台（物品、图标、授予与收回、成就规则）。
import { useEffect, useState, type ReactNode } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Field, Input, Progress, Table, Tabs } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'medals.ui.mine': '我的勋章',
  'medals.ui.wall': '勋章墙',
  'medals.ui.none': '还没有勋章',
  'medals.ui.wear': '佩戴（最多 {n} 枚）',
  'medals.ui.title_label': '头衔',
  'medals.ui.no_title': '不使用头衔',
  'medals.ui.save': '保存',
  'medals.ui.saved': '已保存',
  'medals.ui.expires': '到期 {d}',
  'medals.ui.achievements': '成就',
  'medals.ui.held': '已获得',
  'medals.ui.admin': '勋章管理',
  'medals.ui.items': '物品',
  'medals.ui.grants': '授予',
  'medals.ui.rules': '成就规则',
  'medals.ui.new_item': '新增物品',
  'medals.ui.kind': '类型',
  'medals.ui.name': '名称',
  'medals.ui.description': '说明',
  'medals.ui.max_holders': '限量（留空 = 不限）',
  'medals.ui.validity': '有效天数（留空 = 永久）',
  'medals.ui.hidden': '隐藏（不在勋章墙展示）',
  'medals.ui.active': '启用',
  'medals.ui.icon': '图标',
  'medals.ui.holders': '持有者 {n}',
  'medals.ui.item': '物品',
  'medals.ui.usernames': '用户名（每行一个）',
  'medals.ui.reason': '理由',
  'medals.ui.expires_days': '有效天数（可选）',
  'medals.ui.grant': '授予',
  'medals.ui.granted': '已授予 {n} 人',
  'medals.ui.missing': '找不到用户：{names}',
  'medals.ui.revoke': '收回',
  'medals.ui.revoke_reason': '收回理由',
  'medals.ui.rule_kind': '规则类型',
  'medals.ui.event': '事件',
  'medals.ui.measure': '计量（count / sum:字段 / max:字段）',
  'medals.ui.stat_field': '累计字段',
  'medals.ui.threshold': '阈值',
  'medals.ui.add_rule': '添加规则',
  'medals.ui.evaluate': '立即评估',
  'medals.ui.evaluated': '本次授予 {n} 人',
  'medals.ui.enable': '启用',
  'medals.ui.disable': '停用',
  'medals.ui.kind.medal': '勋章',
  'medals.ui.kind.title': '头衔',
  'medals.inactive': '该勋章已停用',
  'medals.limit_reached': '该勋章已达到限量',
  'medals.not_held': '只能佩戴已持有的勋章',
  'medals.bad_icon': '图标须为 PNG、JPEG、GIF 或 WebP 图片',
});
registerMessages('en', {
  'medals.ui.mine': 'My medals',
  'medals.ui.wall': 'Medals',
  'medals.ui.none': 'No medals yet',
  'medals.ui.wear': 'Wear (up to {n})',
  'medals.ui.title_label': 'Title',
  'medals.ui.no_title': 'No title',
  'medals.ui.save': 'Save',
  'medals.ui.saved': 'Saved',
  'medals.ui.expires': 'expires {d}',
  'medals.ui.achievements': 'Achievements',
  'medals.ui.held': 'Earned',
  'medals.ui.admin': 'Medals',
  'medals.ui.items': 'Items',
  'medals.ui.grants': 'Grant',
  'medals.ui.rules': 'Achievement rules',
  'medals.ui.new_item': 'New item',
  'medals.ui.kind': 'Kind',
  'medals.ui.name': 'Name',
  'medals.ui.description': 'Description',
  'medals.ui.max_holders': 'Holder limit (empty = unlimited)',
  'medals.ui.validity': 'Valid for days (empty = permanent)',
  'medals.ui.hidden': 'Hidden (not shown on the wall)',
  'medals.ui.active': 'Enabled',
  'medals.ui.icon': 'Icon',
  'medals.ui.holders': '{n} holders',
  'medals.ui.item': 'Item',
  'medals.ui.usernames': 'Usernames (one per line)',
  'medals.ui.reason': 'Reason',
  'medals.ui.expires_days': 'Valid for days (optional)',
  'medals.ui.grant': 'Grant',
  'medals.ui.granted': 'Granted to {n} users',
  'medals.ui.missing': 'Users not found: {names}',
  'medals.ui.revoke': 'Revoke',
  'medals.ui.revoke_reason': 'Reason for revoking',
  'medals.ui.rule_kind': 'Rule type',
  'medals.ui.event': 'Event',
  'medals.ui.measure': 'Measure (count / sum:field / max:field)',
  'medals.ui.stat_field': 'Total',
  'medals.ui.threshold': 'Threshold',
  'medals.ui.add_rule': 'Add rule',
  'medals.ui.evaluate': 'Evaluate now',
  'medals.ui.evaluated': 'Granted to {n} users',
  'medals.ui.enable': 'Enable',
  'medals.ui.disable': 'Disable',
  'medals.ui.kind.medal': 'Medal',
  'medals.ui.kind.title': 'Title',
  'medals.inactive': 'This medal is disabled',
  'medals.limit_reached': 'This medal has reached its holder limit',
  'medals.not_held': 'You can only wear medals you hold',
  'medals.bad_icon': 'The icon must be a PNG, JPEG, GIF or WebP image',
});

const errText = (api: PluginApi, e: unknown) => (e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e));

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

interface Mini {
  id: number;
  name: string;
  icon: string | null;
}
interface Display {
  worn: Mini[];
  title: Mini | null;
}
interface Holding extends Mini {
  kind: 'medal' | 'title';
  description: string;
  hidden: boolean;
  granted_at: number;
  expires_at: number | null;
  reason: string | null;
}

/** 勋章小图标：有图用图，没有图用名称徽章 */
function MedalChip({ m, size = 24 }: { m: Mini & { description?: string }; size?: number }) {
  return m.icon ? (
    <img className="medal-icon" src={m.icon} alt={m.name} title={m.description ? `${m.name} — ${m.description}` : m.name} width={size} height={size} />
  ) : (
    <Badge tone="primary" title={m.description}>
      {m.name}
    </Badge>
  );
}

// 同一页面的两个插槽共用一次请求
const wallCache = new Map<number, Promise<{ holdings: Holding[]; display: Display }>>();
const wallOf = (api: PluginApi, id: number) => {
  let p = wallCache.get(id);
  if (!p) {
    p = api.json<{ holdings: Holding[]; display: Display }>(`/api/p/medals/v1/users/${id}`);
    wallCache.set(id, p);
    setTimeout(() => wallCache.delete(id), 5000);
  }
  return p;
};

function useWall(api: PluginApi, id: number) {
  const [w, setW] = useState<{ holdings: Holding[]; display: Display } | null>(null);
  useEffect(() => {
    void wallOf(api, id)
      .then(setW)
      .catch(() => undefined);
  }, [api, id]);
  return w;
}

function ProfileHeader({ api, ctx }: SlotProps<{ profile: { id: number } }>) {
  const w = useWall(api, ctx.profile.id);
  if (!w || (!w.display.title && w.display.worn.length === 0)) return null;
  return (
    <p className="medal-row" data-testid="medal-display">
      {w.display.title && <Badge tone="success">{w.display.title.name}</Badge>}
      {w.display.worn.map((m) => (
        <MedalChip key={m.id} m={m} />
      ))}
    </p>
  );
}

/** 用户名旁的装饰（列表中每个用户名一个实例）：头衔与佩戴的勋章图标；同一轮渲染的请求经 api.batch 合并 */
function NameDecoration({ api, ctx }: SlotProps<{ user_id: number; size?: 'sm' | 'md' }>) {
  const [d, setD] = useState<Display | null>(null);
  useEffect(() => {
    let live = true;
    void api
      .batch<number, Display>('show', async (ids) => {
        const r = await api.json<Record<string, Display>>(`/api/p/medals/v1/show?user_ids=${ids.join(',')}`);
        return new Map(Object.entries(r).map(([k, v]) => [Number(k), v]));
      })(ctx.user_id)
      .then((v) => live && setD(v ?? null))
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [api, ctx.user_id]);
  if (!d || (!d.title && d.worn.length === 0)) return null;
  const px = ctx.size === 'md' ? 20 : 16;
  return (
    <>
      {d.title && <Badge tone="success">{d.title.name}</Badge>}
      {d.worn
        .filter((m) => m.icon)
        .map((m) => (
          <MedalChip key={m.id} m={m} size={px} />
        ))}
    </>
  );
}

function ProfileWall({ api, ctx }: SlotProps<{ profile: { id: number } }>) {
  const w = useWall(api, ctx.profile.id);
  const medals = (w?.holdings ?? []).filter((h) => h.kind === 'medal');
  if (medals.length <= (w?.display.worn.length ?? 0)) return null;
  return (
    <details className="medal-wall">
      <summary>
        {api.t('medals.ui.wall')} ({medals.length})
      </summary>
      <div className="medal-row">
        {medals.map((m) => (
          <MedalChip key={m.id} m={m} size={32} />
        ))}
      </div>
    </details>
  );
}

function MenuItem({ api }: SlotProps) {
  return (
    <A api={api} to="/p/medals/">
      {api.t('medals.ui.mine')}
    </A>
  );
}

interface Achievement {
  rule_id: number;
  medal_id: number;
  name: string;
  icon: string | null;
  kind: 'event' | 'stat';
  stat_field: string | null;
  threshold: number;
  description: string;
  value: number;
  held: boolean;
}

function Mine({ api }: { api: PluginApi }) {
  const [d, setD] = useState<{ holdings: Holding[]; display: Display; achievements: Achievement[]; max_worn: number } | null>(null);
  const [worn, setWorn] = useState<number[]>([]);
  const [title, setTitle] = useState('');
  useEffect(() => {
    void api.json<NonNullable<typeof d>>('/api/p/medals/v1/me').then((r) => {
      setD(r);
      setWorn(r.display.worn.map((m) => m.id));
      setTitle(r.display.title ? String(r.display.title.id) : '');
    });
  }, [api]);
  if (!d) return null;
  const medals = d.holdings.filter((h) => h.kind === 'medal');
  const titles = d.holdings.filter((h) => h.kind === 'title');
  const toggle = (id: number) => setWorn((w) => (w.includes(id) ? w.filter((x) => x !== id) : w.length < d.max_worn ? [...w, id] : w));
  const save = async () => {
    try {
      await api.json('/api/p/medals/v1/me/display', { method: 'PUT', json: { worn, title: title ? Number(title) : null } });
      api.toast(api.t('medals.ui.saved'), 'success');
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t('medals.ui.mine')}</h1>
      <Card title={api.t('medals.ui.wear', { n: d.max_worn })}>
        {medals.length === 0 && titles.length === 0 && <p className="pt-muted">{api.t('medals.ui.none')}</p>}
        <div className="medal-grid">
          {medals.map((m) => (
            <label key={m.id} className="medal-pick">
              <input type="checkbox" checked={worn.includes(m.id)} onChange={() => toggle(m.id)} />
              <MedalChip m={m} size={40} />
              <span>
                <strong>{m.name}</strong>
                <span className="pt-muted small">
                  {' '}
                  {m.description}
                  {m.expires_at && ` · ${api.t('medals.ui.expires', { d: new Date(m.expires_at).toLocaleDateString() })}`}
                </span>
              </span>
            </label>
          ))}
        </div>
        {titles.length > 0 && (
          <Field label={api.t('medals.ui.title_label')}>
            <select className="pt-input" name="medal_title" value={title} onChange={(e) => setTitle(e.target.value)}>
              <option value="">{api.t('medals.ui.no_title')}</option>
              {titles.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.name}
                </option>
              ))}
            </select>
          </Field>
        )}
        {(medals.length > 0 || titles.length > 0) && (
          <Button variant="primary" onClick={() => void save()}>
            {api.t('medals.ui.save')}
          </Button>
        )}
      </Card>
      {d.achievements.length > 0 && (
        <Card title={api.t('medals.ui.achievements')}>
          <div className="pt-stack">
            {d.achievements.map((a) => (
              <div key={a.rule_id} className="medal-ach">
                <MedalChip m={{ id: a.medal_id, name: a.name, icon: a.icon }} size={32} />
                <div>
                  <strong>{a.name}</strong> <span className="pt-muted small">{a.description}</span>
                  {a.held ? (
                    <div>
                      <Badge tone="success">{api.t('medals.ui.held')}</Badge>
                    </div>
                  ) : (
                    <Progress value={Math.min(a.value, a.threshold)} max={a.threshold} label={`${a.value} / ${a.threshold}`} />
                  )}
                </div>
              </div>
            ))}
          </div>
        </Card>
      )}
    </div>
  );
}

// ---------------- 后台 ----------------

interface Item extends Mini {
  kind: 'medal' | 'title';
  description: string;
  max_holders: number | null;
  validity_days: number | null;
  hidden: boolean;
  active: boolean;
  sort: number;
  holders: number;
}

type Draft = { kind: string; name: string; description: string; max_holders: string; validity_days: string; hidden: boolean; active: boolean };
const blank: Draft = { kind: 'medal', name: '', description: '', max_holders: '', validity_days: '', hidden: false, active: true };
const optNum = (s: string) => (s.trim() === '' ? null : Number(s));

function ItemForm({ api, init, id, done }: { api: PluginApi; init: Draft; id?: number; done(): void }) {
  const [f, setF] = useState<Draft>(init);
  const save = async () => {
    try {
      const body = { kind: f.kind, name: f.name, description: f.description, max_holders: optNum(f.max_holders), validity_days: optNum(f.validity_days), hidden: f.hidden, active: f.active };
      if (id) await api.json(`/api/p/medals/v1/admin/items/${id}`, { method: 'PATCH', json: body });
      else await api.json('/api/p/medals/v1/admin/items', { json: body });
      api.toast(api.t('medals.ui.saved'), 'success');
      if (!id) setF(blank);
      done();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <Field label={api.t('medals.ui.kind')}>
        <select className="pt-input" name="medal_kind" value={f.kind} disabled={!!id} onChange={(e) => setF({ ...f, kind: e.target.value })}>
          <option value="medal">{api.t('medals.ui.kind.medal')}</option>
          <option value="title">{api.t('medals.ui.kind.title')}</option>
        </select>
      </Field>
      <Field label={api.t('medals.ui.name')}>
        <Input name="medal_name" value={f.name} maxLength={40} required onChange={(e) => setF({ ...f, name: e.target.value })} />
      </Field>
      <Field label={api.t('medals.ui.description')}>
        <Input value={f.description} maxLength={300} onChange={(e) => setF({ ...f, description: e.target.value })} />
      </Field>
      <Field label={api.t('medals.ui.max_holders')}>
        <Input type="number" min={1} value={f.max_holders} onChange={(e) => setF({ ...f, max_holders: e.target.value })} />
      </Field>
      <Field label={api.t('medals.ui.validity')}>
        <Input type="number" min={1} max={3650} value={f.validity_days} onChange={(e) => setF({ ...f, validity_days: e.target.value })} />
      </Field>
      <label className="pt-check">
        <input type="checkbox" checked={f.hidden} onChange={(e) => setF({ ...f, hidden: e.target.checked })} /> {api.t('medals.ui.hidden')}
      </label>
      <label className="pt-check">
        <input type="checkbox" checked={f.active} onChange={(e) => setF({ ...f, active: e.target.checked })} /> {api.t('medals.ui.active')}
      </label>
      <Button type="submit" variant="primary">
        {api.t('medals.ui.save')}
      </Button>
    </form>
  );
}

function Holders({ api, item, done }: { api: PluginApi; item: number; done(): void }) {
  const [rows, setRows] = useState<{ grant_id: number; username: string | null; source: string; reason: string; expires_at: number | null }[]>([]);
  const load = () => void api.json<{ items: typeof rows }>(`/api/p/medals/v1/admin/items/${item}/holders`).then((r) => setRows(r.items));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, [item]);
  const revoke = async (id: number) => {
    const reason = window.prompt(api.t('medals.ui.revoke_reason'));
    if (!reason) return;
    try {
      await api.json(`/api/p/medals/v1/admin/grants/${id}/revoke`, { json: { reason } });
      load();
      done();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <Table
      rows={rows}
      rowKey={(r) => r.grant_id}
      empty="—"
      columns={[
        { key: 'u', header: '', render: (r) => r.username ?? '?' },
        { key: 's', header: '', render: (r) => <span className="pt-muted small">{`${r.source} · ${r.reason}`}</span> },
        { key: 'e', header: '', render: (r) => (r.expires_at ? new Date(r.expires_at).toLocaleDateString() : '∞') },
        { key: 'a', header: '', render: (r) => <Button onClick={() => void revoke(r.grant_id)}>{api.t('medals.ui.revoke')}</Button> },
      ]}
    />
  );
}

function ItemsTab({ api, items, load }: { api: PluginApi; items: Item[]; load(): void }) {
  const [editing, setEditing] = useState<number | null>(null);
  const [open, setOpen] = useState<number | null>(null);
  const icon = async (id: number, f: File | undefined) => {
    if (!f) return;
    try {
      await api.json(`/api/p/medals/v1/admin/items/${id}/icon`, { method: 'POST', body: f, headers: { 'content-type': f.type || 'application/octet-stream' } });
      load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <div className="pt-stack">
      <Card title={api.t('medals.ui.items')}>
        <Table
          rows={items}
          rowKey={(r) => r.id}
          empty="—"
          columns={[
            {
              key: 'i',
              header: '',
              render: (r) =>
                editing === r.id ? (
                  <ItemForm
                    api={api}
                    id={r.id}
                    init={{ kind: r.kind, name: r.name, description: r.description, max_holders: r.max_holders?.toString() ?? '', validity_days: r.validity_days?.toString() ?? '', hidden: r.hidden, active: r.active }}
                    done={() => {
                      setEditing(null);
                      load();
                    }}
                  />
                ) : (
                  <div className="medal-ach">
                    <MedalChip m={r} size={32} />
                    <div>
                      <strong>{r.name}</strong> <Badge tone={r.active ? 'success' : 'muted'}>{api.t(`medals.ui.kind.${r.kind}`)}</Badge>
                      <div className="pt-muted small">
                        {r.description}
                        {r.validity_days ? ` · ${r.validity_days} d` : ''}
                        {r.max_holders ? ` · ≤ ${r.max_holders}` : ''}
                      </div>
                      <button type="button" className="pt-btn pt-btn-ghost small" onClick={() => setOpen(open === r.id ? null : r.id)}>
                        {api.t('medals.ui.holders', { n: r.holders })}
                      </button>
                      {open === r.id && <Holders api={api} item={r.id} done={load} />}
                    </div>
                  </div>
                ),
            },
            {
              key: 'a',
              header: '',
              render: (r) =>
                editing === r.id ? null : (
                  <span className="pt-row">
                    <label className="pt-btn pt-btn-ghost small">
                      {api.t('medals.ui.icon')}
                      <input type="file" accept="image/png,image/jpeg,image/gif,image/webp" hidden onChange={(e) => void icon(r.id, e.target.files?.[0])} />
                    </label>
                    <Button variant="ghost" onClick={() => setEditing(r.id)}>
                      ✎
                    </Button>
                  </span>
                ),
            },
          ]}
        />
      </Card>
      <Card title={api.t('medals.ui.new_item')}>
        <ItemForm api={api} init={blank} done={load} />
      </Card>
    </div>
  );
}

function GrantTab({ api, items, load }: { api: PluginApi; items: Item[]; load(): void }) {
  const [f, setF] = useState({ item: '', users: '', reason: '', days: '' });
  const submit = async () => {
    try {
      const r = await api.json<{ granted: number; missing: string[] }>('/api/p/medals/v1/admin/grants', {
        json: { item_id: Number(f.item), usernames: f.users.split(/\s+/).filter(Boolean), reason: f.reason, expires_days: optNum(f.days) },
      });
      api.toast(api.t('medals.ui.granted', { n: r.granted }), 'success');
      if (r.missing.length) api.toast(api.t('medals.ui.missing', { names: r.missing.join(', ') }), 'error');
      setF({ ...f, users: '' });
      load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <Card title={api.t('medals.ui.grants')}>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <Field label={api.t('medals.ui.item')}>
          <select className="pt-input" name="grant_item" value={f.item} required onChange={(e) => setF({ ...f, item: e.target.value })}>
            <option value="" />
            {items
              .filter((i) => i.active)
              .map((i) => (
                <option key={i.id} value={i.id}>
                  {i.name}
                </option>
              ))}
          </select>
        </Field>
        <Field label={api.t('medals.ui.usernames')}>
          <textarea className="pt-input" name="grant_users" rows={4} value={f.users} required onChange={(e) => setF({ ...f, users: e.target.value })} />
        </Field>
        <Field label={api.t('medals.ui.reason')}>
          <Input name="grant_reason" value={f.reason} maxLength={300} required onChange={(e) => setF({ ...f, reason: e.target.value })} />
        </Field>
        <Field label={api.t('medals.ui.expires_days')}>
          <Input type="number" min={1} max={3650} value={f.days} onChange={(e) => setF({ ...f, days: e.target.value })} />
        </Field>
        <Button type="submit" variant="primary">
          {api.t('medals.ui.grant')}
        </Button>
      </form>
    </Card>
  );
}

interface Rule {
  id: number;
  item_id: number;
  item_name: string;
  kind: 'event' | 'stat';
  event: string | null;
  measure: string;
  stat_field: string | null;
  threshold: number;
  description: string;
  active: boolean;
  granted: number;
}

function RulesTab({ api, items }: { api: PluginApi; items: Item[] }) {
  const [d, setD] = useState<{ items: Rule[]; events: string[]; stat_fields: string[] } | null>(null);
  const [f, setF] = useState({ item: '', kind: 'event', event: '', measure: 'count', stat_field: '', threshold: '1', description: '' });
  const load = () => void api.json<NonNullable<typeof d>>('/api/p/medals/v1/admin/rules').then(setD);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, []);
  if (!d) return null;
  const run = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const create = () =>
    run(() =>
      api.json('/api/p/medals/v1/admin/rules', {
        json: { item_id: Number(f.item), kind: f.kind, event: f.event, measure: f.measure, stat_field: f.stat_field, threshold: Number(f.threshold), description: f.description },
      }),
    );
  const evaluate = (id: number) =>
    run(async () => {
      const r = await api.json<{ granted: number }>(`/api/p/medals/v1/admin/rules/${id}/evaluate`, { json: {} });
      api.toast(api.t('medals.ui.evaluated', { n: r.granted }), 'success');
    });
  return (
    <div className="pt-stack">
      <Card title={api.t('medals.ui.rules')}>
        <Table
          rows={d.items}
          rowKey={(r) => r.id}
          empty="—"
          columns={[
            {
              key: 'r',
              header: '',
              render: (r) => (
                <>
                  <strong>{r.item_name}</strong> {!r.active && <Badge>{api.t('medals.ui.disable')}</Badge>}
                  <div className="pt-muted small pt-mono">{r.kind === 'event' ? `${r.event} ${r.measure} ≥ ${r.threshold}` : `${r.stat_field} ≥ ${r.threshold}`}</div>
                  <div className="small">
                    {r.description} · {api.t('medals.ui.holders', { n: r.granted })}
                  </div>
                </>
              ),
            },
            {
              key: 'a',
              header: '',
              render: (r) => (
                <span className="pt-row">
                  <Button onClick={() => void run(() => api.json(`/api/p/medals/v1/admin/rules/${r.id}`, { method: 'PATCH', json: { active: !r.active } }))}>
                    {api.t(r.active ? 'medals.ui.disable' : 'medals.ui.enable')}
                  </Button>
                  {r.kind === 'stat' && <Button onClick={() => void evaluate(r.id)}>{api.t('medals.ui.evaluate')}</Button>}
                </span>
              ),
            },
          ]}
        />
      </Card>
      <Card title={api.t('medals.ui.add_rule')}>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void create();
          }}
        >
          <Field label={api.t('medals.ui.item')}>
            <select className="pt-input" name="rule_item" value={f.item} required onChange={(e) => setF({ ...f, item: e.target.value })}>
              <option value="" />
              {items.map((i) => (
                <option key={i.id} value={i.id}>
                  {i.name}
                </option>
              ))}
            </select>
          </Field>
          <Field label={api.t('medals.ui.rule_kind')}>
            <select className="pt-input" name="rule_kind" value={f.kind} onChange={(e) => setF({ ...f, kind: e.target.value })}>
              <option value="event">event</option>
              <option value="stat">stat</option>
            </select>
          </Field>
          {f.kind === 'event' ? (
            <>
              <Field label={api.t('medals.ui.event')}>
                <select className="pt-input" name="rule_event" value={f.event} required onChange={(e) => setF({ ...f, event: e.target.value })}>
                  <option value="" />
                  {d.events.map((e) => (
                    <option key={e}>{e}</option>
                  ))}
                </select>
              </Field>
              <Field label={api.t('medals.ui.measure')}>
                <Input className="pt-mono" value={f.measure} onChange={(e) => setF({ ...f, measure: e.target.value })} />
              </Field>
            </>
          ) : (
            <Field label={api.t('medals.ui.stat_field')}>
              <select className="pt-input" name="rule_stat" value={f.stat_field} required onChange={(e) => setF({ ...f, stat_field: e.target.value })}>
                <option value="" />
                {d.stat_fields.map((s) => (
                  <option key={s}>{s}</option>
                ))}
              </select>
            </Field>
          )}
          <Field label={api.t('medals.ui.threshold')}>
            <Input name="rule_threshold" type="number" min={1} value={f.threshold} onChange={(e) => setF({ ...f, threshold: e.target.value })} />
          </Field>
          <Field label={api.t('medals.ui.description')}>
            <Input value={f.description} maxLength={300} onChange={(e) => setF({ ...f, description: e.target.value })} />
          </Field>
          <Button type="submit" variant="primary">
            {api.t('medals.ui.add_rule')}
          </Button>
        </form>
      </Card>
    </div>
  );
}

function Admin({ api }: { api: PluginApi }) {
  const [tab, setTab] = useState('items');
  const [items, setItems] = useState<Item[]>([]);
  const load = () => void api.json<{ items: Item[] }>('/api/p/medals/v1/admin/items').then((r) => setItems(r.items));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, []);
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t('medals.ui.admin')}</h1>
      <Tabs
        value={tab}
        onChange={setTab}
        tabs={[
          { id: 'items', label: api.t('medals.ui.items') },
          { id: 'grant', label: api.t('medals.ui.grants') },
          { id: 'rules', label: api.t('medals.ui.rules') },
        ]}
      />
      {tab === 'items' && <ItemsTab api={api} items={items} load={load} />}
      {tab === 'grant' && <GrantTab api={api} items={items} load={load} />}
      {tab === 'rules' && <RulesTab api={api} items={items} />}
    </div>
  );
}

export default {
  slots: {
    'user.profile.header': { component: ProfileHeader, order: 10 },
    'user.profile.badges': { component: ProfileWall, order: 10 },
    'nav.user_menu': { component: MenuItem, order: 30 },
    'user.name.decorations': { component: NameDecoration, order: 20 },
  },
  routes: [
    { path: '/p/medals/*', component: Mine },
    { path: '/admin/p/medals/*', component: Admin },
  ],
} satisfies PluginFrontend;
