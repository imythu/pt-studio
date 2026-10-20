// medals 前端：个人页头衔与佩戴勋章、勋章墙；“我的勋章”页（佩戴设置、成就进度）；后台（物品、图标、授予与收回、成就规则）。
import { useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Checkbox, Empty, Field, Input, NavItem, PageHead, Panel, Progress, Segmented, Select, Sheet, SkeletonRows, Status, Switch, Table, Tabs, Tag, Textarea } from '@pt-studio/ui-kit';

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
  'medals.ui.cancel': '取消',
  'medals.ui.worn': '佩戴中',
  'medals.ui.user': '用户',
  'medals.ui.expires_col': '到期',
  'medals.ui.hidden_short': '隐藏',
  'medals.ui.limits': '有效期 · 限量',
  'medals.ui.edit': '编辑',
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
  'medals.ui.cancel': 'Cancel',
  'medals.ui.worn': 'Wearing',
  'medals.ui.user': 'User',
  'medals.ui.expires_col': 'Expires',
  'medals.ui.hidden_short': 'hidden',
  'medals.ui.limits': 'Validity · limit',
  'medals.ui.edit': 'Edit',
  'medals.inactive': 'This medal is disabled',
  'medals.limit_reached': 'This medal has reached its holder limit',
  'medals.not_held': 'You can only wear medals you hold',
  'medals.bad_icon': 'The icon must be a PNG, JPEG, GIF or WebP image',
});

const errText = (api: PluginApi, e: unknown) => (e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e));

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

/** 勋章章面：有图用图，没有图用名称首字 */
function MedalFace({ m }: { m: Mini }) {
  return <span className="medal-i">{m.icon ? <img src={m.icon} alt="" /> : m.name.slice(0, 1)}</span>;
}

/** 名字旁的小图标：只显示有图的勋章 */
function MedalMini({ m, size = 16 }: { m: Mini & { description?: string }; size?: number }) {
  return m.icon ? <img src={m.icon} alt={m.name} title={m.description ? `${m.name} — ${m.description}` : m.name} width={size} height={size} /> : null;
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
    <span className="user-deco" data-testid="medal-display">
      {w.display.title && <Tag tone="o">{w.display.title.name}</Tag>}
      {w.display.worn.map((m) => (
        <MedalMini key={m.id} m={m} size={20} />
      ))}
    </span>
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
      {d.title && <Tag tone="o">{d.title.name}</Tag>}
      {d.worn.map((m) => (
        <MedalMini key={m.id} m={m} size={px} />
      ))}
    </>
  );
}

function ProfileWall({ api, ctx }: SlotProps<{ profile: { id: number } }>) {
  const w = useWall(api, ctx.profile.id);
  const medals = (w?.holdings ?? []).filter((h) => h.kind === 'medal');
  if (medals.length === 0) return null;
  return (
    <Panel title={api.t('medals.ui.wall')} sub={String(medals.length)}>
      <div className="medals">
        {medals.map((m) => (
          <div key={m.id} className="medal" title={m.description}>
            <MedalFace m={m} />
            <span>{m.name}</span>
          </div>
        ))}
      </div>
    </Panel>
  );
}

function MenuItem({ api }: SlotProps) {
  return (
    <NavItem to="/p/medals/" navigate={api.navigate}>
      {api.t('medals.ui.mine')}
    </NavItem>
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
    void api
      .json<NonNullable<typeof d>>('/api/p/medals/v1/me')
      .then((r) => {
        setD(r);
        setWorn(r.display.worn.map((m) => m.id));
        setTitle(r.display.title ? String(r.display.title.id) : '');
      })
      .catch((e: unknown) => api.toast(errText(api, e), 'error'));
  }, [api]);
  if (!d) return <SkeletonRows n={6} />;
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
  const has = medals.length > 0 || titles.length > 0;
  return (
    <div className="stack">
      <PageHead
        title={api.t('medals.ui.mine')}
        lede={api.t('medals.ui.wear', { n: d.max_worn })}
        actions={
          has && (
            <Button variant="primary" onClick={() => void save()}>
              {api.t('medals.ui.save')}
            </Button>
          )
        }
      />
      <Panel
        title={api.t('medals.ui.wall')}
        sub={`${worn.length} / ${d.max_worn}`}
        primary
        actions={
          titles.length > 0 && (
            <Select
              name="medal_title"
              aria-label={api.t('medals.ui.title_label')}
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              options={[{ value: '', label: api.t('medals.ui.no_title') }, ...titles.map((t) => ({ value: String(t.id), label: t.name }))]}
            />
          )
        }
      >
        {medals.length === 0 ? (
          <Empty compact>{api.t('medals.ui.none')}</Empty>
        ) : (
          <div className="medal-wall">
            {medals.map((m) => (
              <button key={m.id} type="button" className={worn.includes(m.id) ? 'mcard held' : 'mcard'} aria-pressed={worn.includes(m.id)} onClick={() => toggle(m.id)}>
                <MedalFace m={m} />
                <b>{m.name}</b>
                <span>{m.description}</span>
                {m.expires_at && <span className="tiny">{api.t('medals.ui.expires', { d: new Date(m.expires_at).toLocaleDateString() })}</span>}
                {worn.includes(m.id) && <Tag tone="g">{api.t('medals.ui.worn')}</Tag>}
              </button>
            ))}
          </div>
        )}
      </Panel>
      {d.achievements.length > 0 && (
        <Panel title={api.t('medals.ui.achievements')}>
          <div className="medal-wall">
            {d.achievements.map((a) => (
              <div key={a.rule_id} className={a.held ? 'mcard held' : 'mcard'}>
                <span className={a.held ? 'medal' : 'medal locked'}>
                  <MedalFace m={{ id: a.medal_id, name: a.name, icon: a.icon }} />
                </span>
                <b>{a.name}</b>
                <span>{a.description}</span>
                {a.held ? (
                  <Status tone="ok">{api.t('medals.ui.held')}</Status>
                ) : (
                  <>
                    <Progress value={Math.min(a.value, a.threshold)} max={a.threshold} label={a.name} />
                    <span className="num tiny">{`${a.value} / ${a.threshold}`}</span>
                  </>
                )}
              </div>
            ))}
          </div>
        </Panel>
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
const draftOf = (r: Item): Draft => ({ kind: r.kind, name: r.name, description: r.description, max_holders: r.max_holders?.toString() ?? '', validity_days: r.validity_days?.toString() ?? '', hidden: r.hidden, active: r.active });

function SheetFoot({ api, form, label, onCancel }: { api: PluginApi; form: string; label: string; onCancel(): void }) {
  return (
    <>
      <Button variant="ghost" onClick={onCancel}>
        {api.t('medals.ui.cancel')}
      </Button>
      <Button type="submit" form={form} variant="primary">
        {label}
      </Button>
    </>
  );
}

/** 物品新建 / 编辑抽屉 */
function ItemSheet({ api, item, open, onClose, done }: { api: PluginApi; item?: Item; open: boolean; onClose(): void; done(): void }) {
  const [f, setF] = useState<Draft>(item ? draftOf(item) : blank);
  const formId = item ? 'medal-item-edit' : 'medal-item-new';
  const save = async (e: FormEvent) => {
    e.preventDefault();
    try {
      const body = { kind: f.kind, name: f.name, description: f.description, max_holders: optNum(f.max_holders), validity_days: optNum(f.validity_days), hidden: f.hidden, active: f.active };
      if (item) await api.json(`/api/p/medals/v1/admin/items/${item.id}`, { method: 'PATCH', json: body });
      else await api.json('/api/p/medals/v1/admin/items', { json: body });
      api.toast(api.t('medals.ui.saved'), 'success');
      if (!item) setF(blank);
      done();
      onClose();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  return (
    <Sheet open={open} onClose={onClose} title={item ? item.name : api.t('medals.ui.new_item')} footer={<SheetFoot api={api} form={formId} label={api.t('medals.ui.save')} onCancel={onClose} />}>
      <form id={formId} className="form" onSubmit={(e) => void save(e)}>
        <Field label={api.t('medals.ui.kind')}>
          <Segmented
            label={api.t('medals.ui.kind')}
            value={f.kind}
            onChange={(v) => !item && setF({ ...f, kind: v })}
            options={[
              { value: 'medal', label: api.t('medals.ui.kind.medal') },
              { value: 'title', label: api.t('medals.ui.kind.title') },
            ]}
          />
        </Field>
        <Field label={api.t('medals.ui.name')}>
          <Input name="medal_name" value={f.name} maxLength={40} required onChange={(e) => setF({ ...f, name: e.target.value })} />
        </Field>
        <Field label={api.t('medals.ui.description')}>
          <Input name="medal_description" value={f.description} maxLength={300} onChange={(e) => setF({ ...f, description: e.target.value })} />
        </Field>
        <div className="form-2">
          <Field label={api.t('medals.ui.max_holders')}>
            <Input name="medal_max" type="number" min={1} value={f.max_holders} onChange={(e) => setF({ ...f, max_holders: e.target.value })} />
          </Field>
          <Field label={api.t('medals.ui.validity')}>
            <Input name="medal_validity" type="number" min={1} max={3650} value={f.validity_days} onChange={(e) => setF({ ...f, validity_days: e.target.value })} />
          </Field>
        </div>
        <Checkbox checked={f.hidden} onChange={(v) => setF({ ...f, hidden: v })} label={api.t('medals.ui.hidden')} />
        <Checkbox checked={f.active} onChange={(v) => setF({ ...f, active: v })} label={api.t('medals.ui.active')} />
      </form>
    </Sheet>
  );
}

type HolderRow = { grant_id: number; username: string | null; source: string; reason: string; expires_at: number | null };

/** 持有者抽屉：列表与收回（收回理由在同一抽屉底部填写） */
function HoldersSheet({ api, item, onClose, done }: { api: PluginApi; item: Item | null; onClose(): void; done(): void }) {
  const [rows, setRows] = useState<HolderRow[] | null>(null);
  const [revoking, setRevoking] = useState<HolderRow | null>(null);
  const [reason, setReason] = useState('');
  const load = () => {
    if (item) void api.json<{ items: HolderRow[] }>(`/api/p/medals/v1/admin/items/${item.id}/holders`).then((r) => setRows(r.items));
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, [item?.id]);
  const revoke = async (e: FormEvent) => {
    e.preventDefault();
    if (!revoking || !reason.trim()) return;
    try {
      await api.json(`/api/p/medals/v1/admin/grants/${revoking.grant_id}/revoke`, { json: { reason: reason.trim() } });
      setRevoking(null);
      setReason('');
      load();
      done();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  return (
    <Sheet open={item !== null} onClose={onClose} title={item ? `${item.name} · ${api.t('medals.ui.holders', { n: item.holders })}` : ''} width={640} flush>
      {rows === null ? (
        <SkeletonRows n={4} />
      ) : (
        <Table
          rows={rows}
          rowKey={(r) => r.grant_id}
          empty="—"
          selected={revoking?.grant_id ?? null}
          columns={[
            { key: 'u', header: api.t('medals.ui.user'), render: (r) => r.username ?? '?' },
            { key: 's', header: api.t('medals.ui.reason'), render: (r) => <span className="small muted">{`${r.source} · ${r.reason}`}</span> },
            { key: 'e', header: api.t('medals.ui.expires_col'), render: (r) => <span className="num small">{r.expires_at ? new Date(r.expires_at).toLocaleDateString() : '∞'}</span> },
            {
              key: 'a',
              header: '',
              right: true,
              render: (r) => (
                <Button size="xs" variant="ghost" onClick={() => setRevoking(r)}>
                  {api.t('medals.ui.revoke')}
                </Button>
              ),
            },
          ]}
        />
      )}
      {revoking && (
        <form className="toolbar" onSubmit={(e) => void revoke(e)}>
          <span className="small">{`${api.t('medals.ui.revoke')} · ${revoking.username ?? '?'}`}</span>
          <Input name="revoke_reason" aria-label={api.t('medals.ui.revoke_reason')} placeholder={api.t('medals.ui.revoke_reason')} value={reason} onChange={(e) => setReason(e.target.value)} required />
          <Button type="submit" size="sm" variant="danger" disabled={!reason.trim()}>
            {api.t('medals.ui.revoke')}
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setRevoking(null)}>
            {api.t('medals.ui.cancel')}
          </Button>
        </form>
      )}
    </Sheet>
  );
}

function ItemsTab({ api, items, load }: { api: PluginApi; items: Item[]; load(): void }) {
  const [editing, setEditing] = useState<Item | null>(null);
  const [holders, setHolders] = useState<Item | null>(null);
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
    <>
      <Table
        rows={items}
        rowKey={(r) => r.id}
        empty="—"
        minWidth={820}
        columns={[
          {
            key: 'i',
            header: api.t('medals.ui.item'),
            render: (r) => (
              <div className="row">
                <span className="medal">
                  <MedalFace m={r} />
                </span>
                <div>
                  <b>{r.name}</b>
                  <div className="tiny muted">{r.description}</div>
                </div>
              </div>
            ),
          },
          {
            key: 'k',
            header: api.t('medals.ui.kind'),
            render: (r) => (
              <div className="row">
                <Tag tone={r.kind === 'title' ? 'o' : undefined}>{api.t(`medals.ui.kind.${r.kind}`)}</Tag>
                {!r.active && <Status tone="mute">{api.t('medals.ui.disable')}</Status>}
                {r.hidden && <Tag>{api.t('medals.ui.hidden_short')}</Tag>}
              </div>
            ),
          },
          { key: 'l', header: api.t('medals.ui.limits'), render: (r) => <span className="num small">{[r.validity_days ? `${r.validity_days} d` : '∞', r.max_holders ? `≤ ${r.max_holders}` : ''].filter(Boolean).join(' · ')}</span> },
          {
            key: 'h',
            header: '',
            render: (r) => (
              <Button size="xs" variant="ghost" onClick={() => setHolders(r)}>
                {api.t('medals.ui.holders', { n: r.holders })}
              </Button>
            ),
          },
          {
            key: 'a',
            header: '',
            right: true,
            render: (r) => (
              <div className="form-acts">
                <label className="btn btn-xs btn-ghost">
                  {api.t('medals.ui.icon')}
                  <input type="file" accept="image/png,image/jpeg,image/gif,image/webp" hidden onChange={(e) => void icon(r.id, e.target.files?.[0])} />
                </label>
                <Button size="xs" variant="ghost" onClick={() => setEditing(r)}>
                  {api.t('medals.ui.edit')}
                </Button>
              </div>
            ),
          },
        ]}
      />
      <ItemSheet key={editing?.id ?? 'none'} api={api} item={editing ?? undefined} open={editing !== null} onClose={() => setEditing(null)} done={load} />
      <HoldersSheet api={api} item={holders} onClose={() => setHolders(null)} done={load} />
    </>
  );
}

function GrantSheet({ api, items, open, onClose, load }: { api: PluginApi; items: Item[]; open: boolean; onClose(): void; load(): void }) {
  const [f, setF] = useState({ item: '', users: '', reason: '', days: '' });
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      const r = await api.json<{ granted: number; missing: string[] }>('/api/p/medals/v1/admin/grants', {
        json: { item_id: Number(f.item), usernames: f.users.split(/\s+/).filter(Boolean), reason: f.reason, expires_days: optNum(f.days) },
      });
      api.toast(api.t('medals.ui.granted', { n: r.granted }), 'success');
      if (r.missing.length) api.toast(api.t('medals.ui.missing', { names: r.missing.join(', ') }), 'error');
      setF({ ...f, users: '' });
      load();
      if (!r.missing.length) onClose();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  return (
    <Sheet open={open} onClose={onClose} title={api.t('medals.ui.grants')} footer={<SheetFoot api={api} form="medal-grant" label={api.t('medals.ui.grant')} onCancel={onClose} />}>
      <form id="medal-grant" className="form" onSubmit={(e) => void submit(e)}>
        <Field label={api.t('medals.ui.item')}>
          <Select
            name="grant_item"
            value={f.item}
            required
            onChange={(e) => setF({ ...f, item: e.target.value })}
            options={[{ value: '', label: '—' }, ...items.filter((i) => i.active).map((i) => ({ value: String(i.id), label: `${i.name} · ${api.t(`medals.ui.kind.${i.kind}`)}` }))]}
          />
        </Field>
        <Field label={api.t('medals.ui.usernames')}>
          <Textarea name="grant_users" rows={5} value={f.users} required onChange={(e) => setF({ ...f, users: e.target.value })} />
        </Field>
        <Field label={api.t('medals.ui.reason')}>
          <Input name="grant_reason" value={f.reason} maxLength={300} required onChange={(e) => setF({ ...f, reason: e.target.value })} />
        </Field>
        <Field label={api.t('medals.ui.expires_days')}>
          <Input name="grant_days" type="number" min={1} max={3650} value={f.days} onChange={(e) => setF({ ...f, days: e.target.value })} />
        </Field>
      </form>
    </Sheet>
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

function RulesTab({ api, items, adding, setAdding }: { api: PluginApi; items: Item[]; adding: boolean; setAdding(v: boolean): void }) {
  const [d, setD] = useState<{ items: Rule[]; events: string[]; stat_fields: string[] } | null>(null);
  const [f, setF] = useState({ item: '', kind: 'event', event: '', measure: 'count', stat_field: '', threshold: '1', description: '' });
  const load = () => void api.json<NonNullable<typeof d>>('/api/p/medals/v1/admin/rules').then(setD);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, []);
  if (!d) return <SkeletonRows n={4} />;
  const run = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      load();
      return true;
    } catch (e) {
      api.toast(errText(api, e), 'error');
      return false;
    }
  };
  const create = (e: FormEvent) => {
    e.preventDefault();
    void run(() =>
      api.json('/api/p/medals/v1/admin/rules', {
        json: { item_id: Number(f.item), kind: f.kind, event: f.event, measure: f.measure, stat_field: f.stat_field, threshold: Number(f.threshold), description: f.description },
      }),
    ).then((ok) => ok && setAdding(false));
  };
  const evaluate = (id: number) =>
    run(async () => {
      const r = await api.json<{ granted: number }>(`/api/p/medals/v1/admin/rules/${id}/evaluate`, { json: {} });
      api.toast(api.t('medals.ui.evaluated', { n: r.granted }), 'success');
    });
  return (
    <>
      <Table
        rows={d.items}
        rowKey={(r) => r.id}
        empty="—"
        minWidth={820}
        columns={[
          { key: 'n', header: api.t('medals.ui.item'), render: (r) => <b>{r.item_name}</b> },
          { key: 'r', header: api.t('medals.ui.rule_kind'), render: (r) => <span className="pt-mono small">{r.kind === 'event' ? `${r.event} ${r.measure} ≥ ${r.threshold}` : `${r.stat_field} ≥ ${r.threshold}`}</span> },
          { key: 'd', header: api.t('medals.ui.description'), render: (r) => <span className="small">{r.description}</span> },
          { key: 'g', header: '', right: true, render: (r) => <span className="num small muted">{api.t('medals.ui.holders', { n: r.granted })}</span> },
          {
            key: 'a',
            header: '',
            right: true,
            render: (r) => (
              <div className="form-acts">
                <Switch checked={r.active} label={api.t('medals.ui.active')} onChange={() => void run(() => api.json(`/api/p/medals/v1/admin/rules/${r.id}`, { method: 'PATCH', json: { active: !r.active } }))} />
                {r.kind === 'stat' && (
                  <Button size="xs" onClick={() => void evaluate(r.id)}>
                    {api.t('medals.ui.evaluate')}
                  </Button>
                )}
              </div>
            ),
          },
        ]}
      />
      <Sheet open={adding} onClose={() => setAdding(false)} title={api.t('medals.ui.add_rule')} footer={<SheetFoot api={api} form="medal-rule" label={api.t('medals.ui.add_rule')} onCancel={() => setAdding(false)} />}>
        <form id="medal-rule" className="form" onSubmit={create}>
          <Field label={api.t('medals.ui.item')}>
            <Select name="rule_item" value={f.item} required onChange={(e) => setF({ ...f, item: e.target.value })} options={[{ value: '', label: '—' }, ...items.map((i) => ({ value: String(i.id), label: i.name }))]} />
          </Field>
          <Field label={api.t('medals.ui.rule_kind')}>
            <Segmented
              label={api.t('medals.ui.rule_kind')}
              value={f.kind}
              onChange={(v) => setF({ ...f, kind: v })}
              options={[
                { value: 'event', label: 'event' },
                { value: 'stat', label: 'stat' },
              ]}
            />
          </Field>
          {f.kind === 'event' ? (
            <div className="form-2">
              <Field label={api.t('medals.ui.event')}>
                <Select name="rule_event" value={f.event} required onChange={(e) => setF({ ...f, event: e.target.value })} options={['', ...d.events]} />
              </Field>
              <Field label={api.t('medals.ui.measure')}>
                <Input name="rule_measure" className="in pt-mono" value={f.measure} onChange={(e) => setF({ ...f, measure: e.target.value })} />
              </Field>
            </div>
          ) : (
            <Field label={api.t('medals.ui.stat_field')}>
              <Select name="rule_stat" value={f.stat_field} required onChange={(e) => setF({ ...f, stat_field: e.target.value })} options={['', ...d.stat_fields]} />
            </Field>
          )}
          <Field label={api.t('medals.ui.threshold')}>
            <Input name="rule_threshold" type="number" min={1} value={f.threshold} onChange={(e) => setF({ ...f, threshold: e.target.value })} />
          </Field>
          <Field label={api.t('medals.ui.description')}>
            <Input name="rule_description" value={f.description} maxLength={300} onChange={(e) => setF({ ...f, description: e.target.value })} />
          </Field>
        </form>
      </Sheet>
    </>
  );
}

function Admin({ api }: { api: PluginApi }) {
  const [tab, setTab] = useState('items');
  const [items, setItems] = useState<Item[]>([]);
  const [sheet, setSheet] = useState<'item' | 'grant' | 'rule' | null>(null);
  const load = () => void api.json<{ items: Item[] }>('/api/p/medals/v1/admin/items').then((r) => setItems(r.items));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, []);
  return (
    <div className="stack">
      <PageHead
        title={api.t('medals.ui.admin')}
        actions={
          <>
            <Button onClick={() => setSheet('grant')}>{api.t('medals.ui.grants')}</Button>
            {tab === 'items' ? (
              <Button variant="primary" onClick={() => setSheet('item')}>
                {api.t('medals.ui.new_item')}
              </Button>
            ) : (
              <Button variant="primary" onClick={() => setSheet('rule')}>
                {api.t('medals.ui.add_rule')}
              </Button>
            )}
          </>
        }
      />
      <Panel flush primary>
        <Tabs
          variant="flush"
          value={tab}
          onChange={setTab}
          tabs={[
            { id: 'items', label: api.t('medals.ui.items'), count: items.length },
            { id: 'rules', label: api.t('medals.ui.rules') },
          ]}
        />
        {tab === 'items' ? <ItemsTab api={api} items={items} load={load} /> : <RulesTab api={api} items={items} adding={sheet === 'rule'} setAdding={(v) => setSheet(v ? 'rule' : null)} />}
      </Panel>
      <ItemSheet api={api} open={sheet === 'item'} onClose={() => setSheet(null)} done={load} />
      <GrantSheet api={api} items={items} open={sheet === 'grant'} onClose={() => setSheet(null)} load={load} />
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
