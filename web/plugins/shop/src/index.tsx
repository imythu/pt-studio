// shop 前端：魔力值商城（商品、购买、我的订单）、后台商品管理与订单。
import { useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { formatBytes, registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Field, Input, Table } from '@pt-studio/ui-kit';

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
    default:
      return api.t(`shop.kind.${it.kind}`);
  }
}

function BuyBox({ api, it, done }: { api: PluginApi; it: Item; done(): void }) {
  const [qty, setQty] = useState(1);
  const [records, setRecords] = useState<{ id: number; torrent_id: number; state: string }[] | null>(null);
  const [record, setRecord] = useState('');
  useEffect(() => {
    if (it.kind !== 'hnr_removal') return;
    void api
      .json<{ items: { id: number; torrent_id: number; state: string }[] }>('/api/p/hnr/v1/me/records')
      .then((r) => setRecords(r.items.filter((x) => x.state === 'active' || x.state === 'failed')))
      .catch(() => setRecords([]));
  }, [api, it.kind]);
  const soldOut = it.stock !== null && it.stock <= 0;
  const limited = it.per_user_limit > 0 && (it.bought ?? 0) >= it.per_user_limit;
  const buy = async () => {
    try {
      await api.json(`/api/p/shop/v1/items/${it.id}/purchase`, { json: { quantity: qty, ...(record ? { record_id: Number(record) } : {}) } });
      api.toast(api.t('shop.ui.bought'), 'success');
      done();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const single = it.kind === 'hnr_removal';
  return (
    <div className="shop-buy">
      {single ? (
        records && records.length === 0 ? (
          <span className="pt-muted small">{api.t('shop.ui.no_records')}</span>
        ) : (
          <select className="pt-input" aria-label={api.t('shop.ui.record')} value={record} onChange={(e) => setRecord(e.target.value)}>
            <option value="">{api.t('shop.ui.record')}</option>
            {(records ?? []).map((r) => (
              <option key={r.id} value={r.id}>
                #{r.torrent_id} · {r.state}
              </option>
            ))}
          </select>
        )
      ) : (
        <Input type="number" aria-label={api.t('shop.ui.quantity')} min={1} max={10} value={qty} onChange={(e) => setQty(Math.max(1, Math.min(10, Number(e.target.value) || 1)))} />
      )}
      <Button variant="primary" disabled={soldOut || limited || (single && !record)} onClick={() => void buy()}>
        {api.t('shop.ui.buy')}
      </Button>
    </div>
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

function Shop({ api }: { api: PluginApi }) {
  const [items, setItems] = useState<Item[] | null>(null);
  const [orders, setOrders] = useState<Order[]>([]);
  const [balance, setBalance] = useState<number | null>(null);
  const load = () => {
    void api.json<{ items: Item[] }>('/api/p/shop/v1/items').then((r) => setItems(r.items));
    void api.json<{ items: Order[] }>('/api/p/shop/v1/orders').then((r) => setOrders(r.items));
    void api
      .json<{ currency: string; balance: number }[] | { items: { currency: string; balance: number }[] }>('/api/p/ledger/v1/users/me/balances')
      .then((r) => setBalance((Array.isArray(r) ? r : r.items).find((x) => x.currency === 'bonus')?.balance ?? 0))
      .catch(() => undefined);
  };
  useEffect(load, [api]);
  if (!items) return null;
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t('shop.ui.title')}</h1>
      {balance !== null && <p className="pt-muted">{api.t('shop.ui.balance', { n: balance })}</p>}
      {items.length === 0 && <p className="pt-muted">{api.t('shop.ui.empty')}</p>}
      <div className="shop-grid">
        {items.map((it) => (
          <Card key={it.id} title={it.title}>
            <p>
              <Badge tone="primary">{api.t(`shop.kind.${it.kind}`)}</Badge> {summary(api, it)}
            </p>
            {it.description && <p className="small">{it.description}</p>}
            <p>
              <strong>{api.t('shop.ui.price', { n: it.price })}</strong>{' '}
              {it.stock !== null && <span className="pt-muted small">{it.stock > 0 ? api.t('shop.ui.stock', { n: it.stock }) : api.t('shop.ui.sold_out')}</span>}
            </p>
            {it.per_user_limit > 0 && (
              <p className="pt-muted small">{api.t('shop.ui.limit', { n: it.per_user_limit, period: api.t(`shop.ui.period.${it.limit_period}`), used: it.bought ?? 0 })}</p>
            )}
            <BuyBox api={api} it={it} done={load} />
          </Card>
        ))}
      </div>
      <Card title={api.t('shop.ui.orders')}>
        <Table
          rows={orders}
          rowKey={(r) => r.id}
          empty="—"
          columns={[
            { key: 'i', header: '', render: (r) => `${r.item_title} × ${r.quantity}` },
            { key: 'p', header: '', render: (r) => api.t('shop.ui.price', { n: r.total }) },
            { key: 'd', header: '', render: (r) => <span className="pt-muted small">{new Date(r.created_at).toLocaleString()}</span> },
          ]}
        />
      </Card>
    </div>
  );
}

const KINDS = ['upload_credit', 'invite', 'promo_card', 'hnr_removal'];
const PARAM_HINT: Record<string, string> = {
  upload_credit: '{"bytes": 10737418240}',
  invite: '{"count": 1, "expires_days": 30}',
  promo_card: '{"card_kind": "free", "count": 1}',
  hnr_removal: '{}',
};

type Draft = { kind: string; title: string; description: string; price: string; params: string; stock: string; per_user_limit: string; limit_period: string; active: boolean };
const blank: Draft = { kind: 'upload_credit', title: '', description: '', price: '100', params: PARAM_HINT.upload_credit ?? '{}', stock: '', per_user_limit: '0', limit_period: 'none', active: true };

function ItemForm({ api, init, id, done }: { api: PluginApi; init: Draft; id?: number; done(): void }) {
  const [f, setF] = useState<Draft>(init);
  const save = async () => {
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
      if (id) await api.json(`/api/p/shop/v1/admin/items/${id}`, { method: 'PATCH', json: body });
      else await api.json('/api/p/shop/v1/admin/items', { json: body });
      api.toast(api.t('shop.ui.save'), 'success');
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
      <Field label={api.t('shop.ui.kind')}>
        <select className="pt-input" name="item_kind" value={f.kind} disabled={!!id} onChange={(e) => setF({ ...f, kind: e.target.value, params: PARAM_HINT[e.target.value] ?? '{}' })}>
          {KINDS.map((k) => (
            <option key={k} value={k}>
              {api.t(`shop.kind.${k}`)}
            </option>
          ))}
        </select>
      </Field>
      <Field label={api.t('shop.ui.name')}>
        <Input name="item_title" value={f.title} maxLength={100} required onChange={(e) => setF({ ...f, title: e.target.value })} />
      </Field>
      <Field label={api.t('shop.ui.description')}>
        <Input value={f.description} maxLength={500} onChange={(e) => setF({ ...f, description: e.target.value })} />
      </Field>
      <Field label={api.t('shop.ui.price', { n: '' }).trim()}>
        <Input name="item_price" type="number" min={1} value={f.price} onChange={(e) => setF({ ...f, price: e.target.value })} />
      </Field>
      <Field label={api.t('shop.ui.params')}>
        <Input name="item_params" className="pt-mono" value={f.params} onChange={(e) => setF({ ...f, params: e.target.value })} />
      </Field>
      <Field label={api.t('shop.ui.stock_input')}>
        <Input type="number" min={0} value={f.stock} onChange={(e) => setF({ ...f, stock: e.target.value })} />
      </Field>
      <Field label={api.t('shop.ui.per_user')}>
        <Input type="number" min={0} value={f.per_user_limit} onChange={(e) => setF({ ...f, per_user_limit: e.target.value })} />
      </Field>
      <Field label={api.t('shop.ui.period')}>
        <select className="pt-input" value={f.limit_period} onChange={(e) => setF({ ...f, limit_period: e.target.value })}>
          {['none', 'day', 'week', 'month'].map((p) => (
            <option key={p} value={p}>
              {p}
            </option>
          ))}
        </select>
      </Field>
      <label className="pt-check">
        <input type="checkbox" checked={f.active} onChange={(e) => setF({ ...f, active: e.target.checked })} /> {api.t('shop.ui.active')}
      </label>
      <Button type="submit" variant="primary">
        {api.t('shop.ui.save')}
      </Button>
    </form>
  );
}

function Admin({ api }: { api: PluginApi }) {
  const [items, setItems] = useState<Item[]>([]);
  const [orders, setOrders] = useState<Order[]>([]);
  const [editing, setEditing] = useState<number | null>(null);
  const load = () => {
    void api.json<{ items: Item[] }>('/api/p/shop/v1/admin/items').then((r) => setItems(r.items));
    void api.json<{ items: Order[] }>('/api/p/shop/v1/admin/orders').then((r) => setOrders(r.items));
  };
  useEffect(load, [api]);
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
  return (
    <div className="pt-stack">
      <Card title={api.t('shop.ui.items')}>
        <Table
          rows={items}
          rowKey={(r) => r.id}
          empty={api.t('shop.ui.empty')}
          columns={[
            {
              key: 't',
              header: '',
              render: (r) =>
                editing === r.id ? (
                  <ItemForm
                    api={api}
                    init={toDraft(r)}
                    id={r.id}
                    done={() => {
                      setEditing(null);
                      load();
                    }}
                  />
                ) : (
                  <>
                    {r.title} <Badge tone={r.active ? 'success' : 'muted'}>{api.t(`shop.kind.${r.kind}`)}</Badge>
                    <div className="pt-muted small">
                      {api.t('shop.ui.price', { n: r.price })} · {summary(api, r)}
                      {r.stock !== null ? ` · ${api.t('shop.ui.stock', { n: r.stock })}` : ''}
                    </div>
                  </>
                ),
            },
            {
              key: 'e',
              header: '',
              render: (r) =>
                editing === r.id ? null : (
                  <Button variant="ghost" onClick={() => setEditing(r.id)}>
                    ✎
                  </Button>
                ),
            },
          ]}
        />
      </Card>
      <Card title={api.t('shop.ui.new_item')}>
        <ItemForm api={api} init={blank} done={load} />
      </Card>
      <Card title={api.t('shop.ui.all_orders')}>
        <Table
          rows={orders}
          rowKey={(r) => r.id}
          empty="—"
          columns={[
            { key: 'u', header: '', render: (r) => r.username ?? '?' },
            { key: 'i', header: '', render: (r) => `${r.item_title} × ${r.quantity}` },
            { key: 'p', header: '', render: (r) => api.t('shop.ui.price', { n: r.total }) },
            { key: 'd', header: '', render: (r) => <span className="pt-muted small">{new Date(r.created_at).toLocaleString()}</span> },
          ]}
        />
      </Card>
    </div>
  );
}

function NavItem({ api }: SlotProps) {
  return (
    <a
      href="/p/shop/"
      onClick={(e) => {
        e.preventDefault();
        api.navigate('/p/shop/');
      }}
    >
      {api.t('shop.ui.nav')}
    </a>
  );
}

export default {
  slots: { 'nav.user_menu': { component: NavItem, order: 50 } },
  routes: [
    { path: '/p/shop/*', component: Shop },
    { path: '/admin/p/shop/*', component: Admin },
  ],
} satisfies PluginFrontend;
