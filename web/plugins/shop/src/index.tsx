// shop 前端：魔力值商城（商品、购买、我的订单）、后台商品管理与订单。
import { useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { formatBytes, registerMessages } from '@pt-studio/terms';
import { Button, Checkbox, Empty, Field, Input, NavItem, Note, PageHead, Panel, Select, Sheet, SkeletonRows, Status, Table, Tabs, Tag, type Column } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'shop.ui.nav': '商城',
  'shop.ui.title': '魔力值商城',
  'shop.ui.balance': '我的魔力值：{n}',
  'shop.ui.price': '{n} 魔力值',
  'shop.ui.buy': '购买',
  'shop.ui.bought': '购买成功',
  'shop.ui.quantity': '数量',
  'shop.ui.stock': '剩余 {n}',
  'shop.ui.sold_out': '已售罄',
  'shop.ui.limit': '每{period}限购 {n}，已购 {used}',
  'shop.ui.period.none': '人',
  'shop.ui.period.day': '天',
  'shop.ui.period.week': '周',
  'shop.ui.period.month': '月',
  'shop.ui.record': '要消除的 H&R 记录',
  'shop.ui.no_records': '没有可消除的 H&R 记录',
  'shop.ui.orders': '我的订单',
  'shop.ui.empty': '商城暂无商品',
  'shop.ui.admin': '商城管理',
  'shop.ui.items': '商品',
  'shop.ui.new_item': '新增商品',
  'shop.ui.kind': '类型',
  'shop.ui.name': '名称',
  'shop.ui.description': '说明',
  'shop.ui.params': '参数（JSON）',
  'shop.ui.stock_input': '库存（留空 = 不限）',
  'shop.ui.per_user': '每人限购（0 = 不限）',
  'shop.ui.period': '限购周期',
  'shop.ui.active': '上架',
  'shop.ui.save': '保存',
  'shop.ui.all_orders': '最近订单',
  'shop.kind.upload_credit': '上传量',
  'shop.kind.invite': '邀请名额',
  'shop.kind.promo_card': '促销卡',
  'shop.kind.hnr_removal': '消除 H&R',
  'shop.kind.medal': '勋章 / 头衔',
  'shop.kind.vip': 'VIP',
  'shop.ui.days': '{n} 天',
  'shop.ui.total': '合计',
  'shop.ui.my_balance': '我的魔力值',
  'shop.ui.user': '用户',
  'shop.ui.item': '商品',
  'shop.ui.paid': '花费',
  'shop.ui.state': '状态',
  'shop.ui.time': '时间',
  'shop.ui.saved': '已保存',
  'shop.ui.cancel': '取消',
  'shop.ui.price_label': '价格（魔力值）',
  'shop.ui.stock_col': '库存',
  'shop.ui.on_sale': '上架中',
  'shop.ui.off_sale': '已下架',
  'shop.ui.edit': '编辑',
  'shop.ui.period_opt.none': '不按周期（总计）',
  'shop.ui.period_opt.day': '每天',
  'shop.ui.period_opt.week': '每周',
  'shop.ui.period_opt.month': '每月',
  'shop.unavailable': '商品不可购买',
  'shop.sold_out': '已售罄',
  'shop.limit_reached': '已达到限购数量',
  'shop.insufficient_funds': '魔力值不足',
  'shop.invalid_target': '选择的对象不可用',
  'shop.fulfillment_failed': '发放失败，魔力值已退回',
});
registerMessages('en', {
  'shop.ui.nav': 'Shop',
  'shop.ui.title': 'Bonus shop',
  'shop.ui.balance': 'My bonus: {n}',
  'shop.ui.price': '{n} bonus',
  'shop.ui.buy': 'Buy',
  'shop.ui.bought': 'Purchased',
  'shop.ui.quantity': 'Quantity',
  'shop.ui.stock': '{n} left',
  'shop.ui.sold_out': 'Sold out',
  'shop.ui.limit': 'Limit {n} per {period}, bought {used}',
  'shop.ui.period.none': 'user',
  'shop.ui.period.day': 'day',
  'shop.ui.period.week': 'week',
  'shop.ui.period.month': 'month',
  'shop.ui.record': 'H&R record to clear',
  'shop.ui.no_records': 'No H&R records can be cleared',
  'shop.ui.orders': 'My orders',
  'shop.ui.empty': 'Nothing for sale yet',
  'shop.ui.admin': 'Shop',
  'shop.ui.items': 'Items',
  'shop.ui.new_item': 'New item',
  'shop.ui.kind': 'Kind',
  'shop.ui.name': 'Name',
  'shop.ui.description': 'Description',
  'shop.ui.params': 'Parameters (JSON)',
  'shop.ui.stock_input': 'Stock (empty = unlimited)',
  'shop.ui.per_user': 'Per-user limit (0 = none)',
  'shop.ui.period': 'Limit period',
  'shop.ui.active': 'On sale',
  'shop.ui.save': 'Save',
  'shop.ui.all_orders': 'Recent orders',
  'shop.kind.upload_credit': 'Upload credit',
  'shop.kind.invite': 'Invites',
  'shop.kind.promo_card': 'Promotion cards',
  'shop.kind.hnr_removal': 'Clear an H&R',
  'shop.kind.medal': 'Medal / title',
  'shop.kind.vip': 'VIP',
  'shop.ui.days': '{n} days',
  'shop.ui.total': 'Total',
  'shop.ui.my_balance': 'My bonus',
  'shop.ui.user': 'User',
  'shop.ui.item': 'Item',
  'shop.ui.paid': 'Paid',
  'shop.ui.state': 'State',
  'shop.ui.time': 'Time',
  'shop.ui.saved': 'Saved',
  'shop.ui.cancel': 'Cancel',
  'shop.ui.price_label': 'Price (bonus)',
  'shop.ui.stock_col': 'Stock',
  'shop.ui.on_sale': 'on sale',
  'shop.ui.off_sale': 'off sale',
  'shop.ui.edit': 'Edit',
  'shop.ui.period_opt.none': 'Lifetime',
  'shop.ui.period_opt.day': 'Per day',
  'shop.ui.period_opt.week': 'Per week',
  'shop.ui.period_opt.month': 'Per month',
  'shop.unavailable': 'This item is not available',
  'shop.sold_out': 'Sold out',
  'shop.limit_reached': 'You have reached the purchase limit',
  'shop.insufficient_funds': 'Not enough bonus',
  'shop.invalid_target': 'The selected target is not available',
  'shop.fulfillment_failed': 'Delivery failed; your bonus was returned',
});

const errText = (api: PluginApi, e: unknown) => (e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e));

interface Item {
  id: number;
  kind: string;
  title: string;
  description: string;
  price: number;
  params: Record<string, unknown>;
  stock: number | null;
  per_user_limit: number;
  limit_period: string;
  active: boolean;
  sort: number;
  bought?: number;
  summary?: string;
}

/** 商品内容的一句话说明（按类型与参数） */
function summary(api: PluginApi, it: Item): string {
  const p = it.params;
  switch (it.kind) {
    case 'upload_credit':
      return formatBytes(Number(p.bytes ?? 0));
    case 'invite':
      return `× ${Number(p.count ?? 1)}`;
    case 'promo_card':
      return `${String(p.card_kind ?? '')} × ${Number(p.count ?? 1)}`;
    case 'vip':
      return `VIP · ${api.t('shop.ui.days', { n: Number(p.days ?? 0) })}`;
    case 'medal':
      return p.days ? `${Number(p.days)} d` : api.t('shop.kind.medal');
    default:
      return api.t(`shop.kind.${it.kind}`);
  }
}

/** 购买抽屉：数量（或要消除的 H&R 记录）与合计，确认后下单 */
function BuySheet({ api, it, balance, onClose, done }: { api: PluginApi; it: Item | null; balance: number | null; onClose(): void; done(): void }) {
  const [qty, setQty] = useState(1);
  const [records, setRecords] = useState<{ id: number; torrent_id: number; title?: string | null; state: string }[] | null>(null);
  const [record, setRecord] = useState('');
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    setQty(1);
    setRecord('');
    if (it?.kind !== 'hnr_removal') return;
    void api
      .json<{ items: { id: number; torrent_id: number; title?: string | null; state: string }[] }>('/api/p/hnr/v1/me/records')
      .then((r) => setRecords(r.items.filter((x) => x.state === 'active' || x.state === 'failed')))
      .catch(() => setRecords([]));
  }, [api, it]);
  if (!it) return null;
  const single = it.kind === 'hnr_removal' || it.kind === 'medal';
  const total = it.price * (single ? 1 : qty);
  const short = balance !== null && balance < total;
  const buy = async () => {
    setBusy(true);
    try {
      await api.json(`/api/p/shop/v1/items/${it.id}/purchase`, { json: { quantity: single ? 1 : qty, ...(record ? { record_id: Number(record) } : {}) } });
      api.toast(api.t('shop.ui.bought'), 'success');
      onClose();
      done();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <Sheet
      open
      onClose={onClose}
      title={it.title}
      footer={
        <>
          <span className="price">{total.toLocaleString()}</span>
          <Button variant="primary" className="push-right" disabled={busy || short || (it.kind === 'hnr_removal' && !record)} onClick={() => void buy()}>
            {api.t('shop.ui.buy')}
          </Button>
        </>
      }
    >
      <div className="form">
        <p className="small">
          <Tag tone="o">{api.t(`shop.kind.${it.kind}`)}</Tag> {summary(api, it)}
        </p>
        {it.description && <p className="small muted">{it.description}</p>}
        {it.kind === 'hnr_removal' ? (
          records && records.length === 0 ? (
            <Note>{api.t('shop.ui.no_records')}</Note>
          ) : (
            <Field label={api.t('shop.ui.record')}>
              <Select
                name="shop_record"
                value={record}
                onChange={(e) => setRecord(e.target.value)}
                options={[{ value: '', label: '—' }, ...(records ?? []).map((r) => ({ value: String(r.id), label: `${r.title ?? `#${r.torrent_id}`} · ${r.state}` }))]}
              />
            </Field>
          )
        ) : (
          !single && (
            <Field label={api.t('shop.ui.quantity')}>
              <Input name="shop_qty" type="number" min={1} max={10} value={qty} onChange={(e) => setQty(Math.max(1, Math.min(10, Number(e.target.value) || 1)))} />
            </Field>
          )
        )}
        <dl className="ledger">
          <dt>{api.t('shop.ui.total')}</dt>
          <dd className="num">{total.toLocaleString()}</dd>
          {balance !== null && (
            <>
              <dt>{api.t('shop.ui.my_balance')}</dt>
              <dd className={short ? 'num down' : 'num'}>{balance.toLocaleString()}</dd>
            </>
          )}
        </dl>
        {short && <Note tone="bad">{api.t('shop.insufficient_funds')}</Note>}
      </div>
    </Sheet>
  );
}

interface Order {
  id: number;
  item_title: string;
  quantity: number;
  total: number;
  state: string;
  created_at: number;
  username?: string | null;
}

const orderCols = (api: PluginApi, withUser?: boolean): Column<Order>[] => [
  ...(withUser ? [{ key: 'u', header: api.t('shop.ui.user'), render: (r: Order) => r.username ?? '?' }] : []),
  { key: 'i', header: api.t('shop.ui.item'), render: (r) => `${r.item_title} × ${r.quantity}` },
  { key: 'p', header: api.t('shop.ui.paid'), right: true, render: (r) => <span className="num gold">{r.total.toLocaleString()}</span> },
  { key: 's', header: api.t('shop.ui.state'), render: (r) => <Status tone={r.state === 'fulfilled' || r.state === 'done' ? 'ok' : r.state === 'refunded' || r.state === 'failed' ? 'bad' : 'mute'}>{r.state}</Status> },
  { key: 'd', header: api.t('shop.ui.time'), right: true, render: (r) => <span className="small muted num nowrap">{new Date(r.created_at).toLocaleString()}</span> },
];

function Shop({ api }: { api: PluginApi }) {
  const [items, setItems] = useState<Item[] | null>(null);
  const [orders, setOrders] = useState<Order[]>([]);
  const [balance, setBalance] = useState<number | null>(null);
  const [buying, setBuying] = useState<Item | null>(null);
  const [tab, setTab] = useState('goods');
  const load = () => {
    void api.json<{ items: Item[] }>('/api/p/shop/v1/items').then((r) => setItems(r.items));
    void api.json<{ items: Order[] }>('/api/p/shop/v1/orders').then((r) => setOrders(r.items));
    void api
      .json<{ currency: string; balance: number }[] | { items: { currency: string; balance: number }[] }>('/api/p/ledger/v1/users/me/balances')
      .then((r) => setBalance((Array.isArray(r) ? r : r.items).find((x) => x.currency === 'bonus')?.balance ?? 0))
      .catch(() => undefined);
  };
  useEffect(load, [api]);
  if (!items) return <SkeletonRows n={6} />;
  return (
    <div className="stack">
      <PageHead title={api.t('shop.ui.title')} lede={balance !== null ? api.t('shop.ui.balance', { n: balance.toLocaleString() }) : undefined} />
      <Tabs
        variant="sub"
        value={tab}
        onChange={setTab}
        tabs={[
          { id: 'goods', label: api.t('shop.ui.items'), count: items.length },
          { id: 'orders', label: api.t('shop.ui.orders'), count: orders.length },
        ]}
      />
      {tab === 'goods' ? (
        items.length === 0 ? (
          <Panel>
            <Empty compact>{api.t('shop.ui.empty')}</Empty>
          </Panel>
        ) : (
          <div className="goods" data-primary>
            {items.map((it) => {
              const soldOut = it.stock !== null && it.stock <= 0;
              const limited = it.per_user_limit > 0 && (it.bought ?? 0) >= it.per_user_limit;
              return (
                <article key={it.id} className={soldOut || limited ? 'good out' : 'good'}>
                  <div className="good-top">
                    <span className="good-k">{api.t(`shop.kind.${it.kind}`)}</span>
                    <h3>{it.title}</h3>
                    <span className="small muted">{summary(api, it)}</span>
                  </div>
                  <p className="good-d">{it.description}</p>
                  {it.per_user_limit > 0 && <p className="good-lim">{api.t('shop.ui.limit', { n: it.per_user_limit, period: api.t(`shop.ui.period.${it.limit_period}`), used: it.bought ?? 0 })}</p>}
                  <div className="good-f">
                    <span>
                      <span className="price">{it.price.toLocaleString()}</span>
                      {it.stock !== null && <span className="tiny muted">{` · ${it.stock > 0 ? api.t('shop.ui.stock', { n: it.stock }) : api.t('shop.ui.sold_out')}`}</span>}
                    </span>
                    <Button size="sm" variant="primary" disabled={soldOut || limited} onClick={() => setBuying(it)}>
                      {soldOut ? api.t('shop.ui.sold_out') : api.t('shop.ui.buy')}
                    </Button>
                  </div>
                </article>
              );
            })}
          </div>
        )
      ) : (
        <Panel flush primary>
          <Table rows={orders} rowKey={(r) => r.id} empty="—" columns={orderCols(api)} />
        </Panel>
      )}
      <BuySheet api={api} it={buying} balance={balance} onClose={() => setBuying(null)} done={load} />
    </div>
  );
}

const KINDS = ['upload_credit', 'invite', 'promo_card', 'hnr_removal', 'medal', 'vip'];
const PARAM_HINT: Record<string, string> = {
  upload_credit: '{"bytes": 10737418240}',
  invite: '{"count": 1, "expires_days": 30}',
  promo_card: '{"card_kind": "free", "count": 1}',
  hnr_removal: '{}',
  medal: '{"medal_id": 1, "days": 30}',
  vip: '{"days": 30}',
};

type Draft = { kind: string; title: string; description: string; price: string; params: string; stock: string; per_user_limit: string; limit_period: string; active: boolean };
const blank: Draft = { kind: 'upload_credit', title: '', description: '', price: '100', params: PARAM_HINT.upload_credit ?? '{}', stock: '', per_user_limit: '0', limit_period: 'none', active: true };
const toDraft = (it: Item): Draft => ({
  kind: it.kind,
  title: it.title,
  description: it.description,
  price: String(it.price),
  params: JSON.stringify(it.params),
  stock: it.stock === null ? '' : String(it.stock),
  per_user_limit: String(it.per_user_limit),
  limit_period: it.limit_period,
  active: it.active,
});

/** 商品新建 / 编辑抽屉 */
function ItemSheet({ api, item, open, onClose, done }: { api: PluginApi; item?: Item; open: boolean; onClose(): void; done(): void }) {
  const [f, setF] = useState<Draft>(item ? toDraft(item) : blank);
  const formId = item ? 'shop-item-edit' : 'shop-item-new';
  const save = async (e: FormEvent) => {
    e.preventDefault();
    try {
      const body = {
        kind: f.kind,
        title: f.title,
        description: f.description,
        price: Number(f.price),
        params: JSON.parse(f.params || '{}') as unknown,
        stock: f.stock.trim() === '' ? null : Number(f.stock),
        per_user_limit: Number(f.per_user_limit) || 0,
        limit_period: f.limit_period,
        active: f.active,
      };
      if (item) await api.json(`/api/p/shop/v1/admin/items/${item.id}`, { method: 'PATCH', json: body });
      else await api.json('/api/p/shop/v1/admin/items', { json: body });
      api.toast(api.t('shop.ui.saved'), 'success');
      if (!item) setF(blank);
      onClose();
      done();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  return (
    <Sheet
      open={open}
      onClose={onClose}
      title={item ? item.title : api.t('shop.ui.new_item')}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {api.t('shop.ui.cancel')}
          </Button>
          <Button type="submit" form={formId} variant="primary">
            {api.t('shop.ui.save')}
          </Button>
        </>
      }
    >
      <form id={formId} className="form" onSubmit={(e) => void save(e)}>
        <Field label={api.t('shop.ui.kind')}>
          <Select
            name="item_kind"
            value={f.kind}
            disabled={!!item}
            onChange={(e) => setF({ ...f, kind: e.target.value, params: PARAM_HINT[e.target.value] ?? '{}' })}
            options={KINDS.map((k) => ({ value: k, label: api.t(`shop.kind.${k}`) }))}
          />
        </Field>
        <Field label={api.t('shop.ui.name')}>
          <Input name="item_title" value={f.title} maxLength={100} required onChange={(e) => setF({ ...f, title: e.target.value })} />
        </Field>
        <Field label={api.t('shop.ui.description')}>
          <Input name="item_description" value={f.description} maxLength={500} onChange={(e) => setF({ ...f, description: e.target.value })} />
        </Field>
        <div className="form-2">
          <Field label={api.t('shop.ui.price_label')}>
            <Input name="item_price" type="number" min={1} value={f.price} onChange={(e) => setF({ ...f, price: e.target.value })} />
          </Field>
          <Field label={api.t('shop.ui.stock_input')}>
            <Input name="item_stock" type="number" min={0} value={f.stock} onChange={(e) => setF({ ...f, stock: e.target.value })} />
          </Field>
          <Field label={api.t('shop.ui.per_user')}>
            <Input name="item_limit" type="number" min={0} value={f.per_user_limit} onChange={(e) => setF({ ...f, per_user_limit: e.target.value })} />
          </Field>
          <Field label={api.t('shop.ui.period')}>
            <Select name="item_period" value={f.limit_period} onChange={(e) => setF({ ...f, limit_period: e.target.value })} options={['none', 'day', 'week', 'month'].map((p) => ({ value: p, label: api.t(`shop.ui.period_opt.${p}`) }))} />
          </Field>
        </div>
        <Field label={api.t('shop.ui.params')}>
          <Input name="item_params" className="in pt-mono" value={f.params} onChange={(e) => setF({ ...f, params: e.target.value })} />
        </Field>
        <Checkbox checked={f.active} onChange={(v) => setF({ ...f, active: v })} label={api.t('shop.ui.active')} />
      </form>
    </Sheet>
  );
}

function Admin({ api }: { api: PluginApi }) {
  const [items, setItems] = useState<Item[] | null>(null);
  const [orders, setOrders] = useState<Order[]>([]);
  const [editing, setEditing] = useState<Item | null>(null);
  const [creating, setCreating] = useState(false);
  const [tab, setTab] = useState('items');
  const load = () => {
    void api.json<{ items: Item[] }>('/api/p/shop/v1/admin/items').then((r) => setItems(r.items));
    void api.json<{ items: Order[] }>('/api/p/shop/v1/admin/orders').then((r) => setOrders(r.items));
  };
  useEffect(load, [api]);
  return (
    <div className="stack">
      <PageHead
        title={api.t('shop.ui.admin')}
        actions={
          <Button variant="primary" onClick={() => setCreating(true)}>
            {api.t('shop.ui.new_item')}
          </Button>
        }
      />
      <Panel flush primary>
        <Tabs
          variant="flush"
          value={tab}
          onChange={setTab}
          tabs={[
            { id: 'items', label: api.t('shop.ui.items'), count: items?.length },
            { id: 'orders', label: api.t('shop.ui.all_orders') },
          ]}
        />
        {tab === 'orders' ? (
          <Table rows={orders} rowKey={(r) => r.id} empty="—" columns={orderCols(api, true)} />
        ) : items === null ? (
          <SkeletonRows n={5} />
        ) : (
          <Table
            rows={items}
            rowKey={(r) => r.id}
            empty={api.t('shop.ui.empty')}
            onRowClick={(r) => setEditing(r)}
            minWidth={640}
            columns={[
              {
                key: 't',
                header: api.t('shop.ui.name'),
                render: (r) => (
                  <>
                    <b>{r.title}</b>
                    <div className="tiny muted">{summary(api, r)}</div>
                  </>
                ),
              },
              { key: 'k', header: api.t('shop.ui.kind'), render: (r) => <Tag>{api.t(`shop.kind.${r.kind}`)}</Tag> },
              { key: 'p', header: api.t('shop.ui.price_label'), right: true, render: (r) => <span className="num gold">{r.price.toLocaleString()}</span> },
              { key: 's', header: api.t('shop.ui.stock_col'), right: true, render: (r) => <span className="num">{r.stock ?? '∞'}</span> },
              { key: 'a', header: api.t('shop.ui.state'), render: (r) => <Status tone={r.active ? 'ok' : 'mute'}>{api.t(r.active ? 'shop.ui.on_sale' : 'shop.ui.off_sale')}</Status> },
              {
                key: 'e',
                header: '',
                right: true,
                render: (r) => (
                  <Button size="xs" variant="ghost" onClick={() => setEditing(r)}>
                    {api.t('shop.ui.edit')}
                  </Button>
                ),
              },
            ]}
          />
        )}
      </Panel>
      <ItemSheet key={editing?.id ?? 'none'} api={api} item={editing ?? undefined} open={editing !== null} onClose={() => setEditing(null)} done={load} />
      <ItemSheet api={api} open={creating} onClose={() => setCreating(false)} done={load} />
    </div>
  );
}

function MenuItem({ api }: SlotProps) {
  return (
    <NavItem to="/p/shop/" navigate={api.navigate}>
      {api.t('shop.ui.nav')}
    </NavItem>
  );
}

export default {
  slots: { 'nav.user_menu': { component: MenuItem, order: 50 } },
  routes: [
    { path: '/p/shop/*', component: Shop },
    { path: '/admin/p/shop/*', component: Admin },
  ],
} satisfies PluginFrontend;
