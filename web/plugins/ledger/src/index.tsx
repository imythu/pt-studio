// ledger 前端：用户转账页（/p/ledger/transfer）；后台用户详情页“余额”标签 —— 各货币余额（含冻结）、手动调整（正数发放、负数扣除，
// 需填写原因）、流水与冲正；后台货币设置（/admin/p/ledger/：可否转账、手续费、最小额、每日上限、停用）。
import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Field, Input, Stat, Table } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'ledger.ui.tab': '余额',
  'ledger.ui.currency': '货币',
  'ledger.ui.balance': '余额',
  'ledger.ui.held': '冻结',
  'ledger.ui.locked': '已锁定',
  'ledger.ui.amount': '数量（正数发放，负数扣除）',
  'ledger.ui.note': '原因（必填，写入审计）',
  'ledger.ui.apply': '调整余额',
  'ledger.ui.done': '已调整',
  'ledger.ui.saved': '已保存',
  'ledger.ui.entries': '流水',
  'ledger.ui.time': '时间',
  'ledger.ui.reason': '原因',
  'ledger.ui.amount_col': '数量',
  'ledger.ui.after': '余额',
  'ledger.ui.reversed': '已冲正',
  'ledger.ui.reverse': '冲正',
  'ledger.ui.reverse_prompt': '冲正这笔流水（反向记一笔）。请填写原因：',
  'ledger.ui.more': '加载更多',
  'ledger.ui.transfer': '转账',
  'ledger.ui.transfer_title': '转账给其他用户',
  'ledger.ui.to': '收款人用户名',
  'ledger.ui.amount_plain': '数量',
  'ledger.ui.memo': '附言（可选，对方可见）',
  'ledger.ui.available': '可用 {n}',
  'ledger.ui.rules': '手续费 {fee}% · 最少 {min} · 每日转出上限 {limit}',
  'ledger.ui.unlimited': '不限',
  'ledger.ui.no_transferable': '目前没有可以转账的货币',
  'ledger.ui.confirm': '确认转给 {to} {amount} {cur}？',
  'ledger.ui.sent': '已转出，手续费 {fee}，余额 {after}',
  'ledger.ui.currencies': '货币设置',
  'ledger.ui.transferable': '允许转账',
  'ledger.ui.fee_bps': '手续费（万分比）',
  'ledger.ui.min_transfer': '最小转账额',
  'ledger.ui.daily_out_limit': '每日转出上限（0 不限）',
  'ledger.ui.disabled': '停用',
  'ledger.ui.save': '保存',
  'ledger.ui.units_hint': '数量均为最小单位（小数位 {scale}）',
  'ledger.transfer_disabled': '这种货币不允许转账',
  'ledger.amount_below_minimum': '低于最小转账额',
  'ledger.recipient_invalid': '收款人不存在或不可用',
  'ledger.insufficient_balance': '余额不足',
  'ledger.self_transfer': '不能转给自己',
  'ledger.currency_disabled': '这种货币已停用',
  'ledger.already_reversed': '这笔流水已经冲正过',
  'ledger.negative_not_allowed': '冲正后余额会为负',
  'ledger.daily_limit_exceeded': '超过今日转出上限',
  'ledger.account_locked': '账户已锁定',
});
registerMessages('en', {
  'ledger.ui.tab': 'Balances',
  'ledger.ui.currency': 'Currency',
  'ledger.ui.balance': 'Balance',
  'ledger.ui.held': 'Held',
  'ledger.ui.locked': 'locked',
  'ledger.ui.amount': 'Amount (positive credits, negative debits)',
  'ledger.ui.note': 'Reason (required, audited)',
  'ledger.ui.apply': 'Adjust balance',
  'ledger.ui.done': 'Adjusted',
  'ledger.ui.saved': 'Saved',
  'ledger.ui.entries': 'Entries',
  'ledger.ui.time': 'Time',
  'ledger.ui.reason': 'Reason',
  'ledger.ui.amount_col': 'Amount',
  'ledger.ui.after': 'Balance',
  'ledger.ui.reversed': 'reversed',
  'ledger.ui.reverse': 'Reverse',
  'ledger.ui.reverse_prompt': 'Reverse this entry (posts an opposite entry). Reason:',
  'ledger.ui.more': 'Load more',
  'ledger.ui.transfer': 'Transfer',
  'ledger.ui.transfer_title': 'Send to another user',
  'ledger.ui.to': 'Recipient username',
  'ledger.ui.amount_plain': 'Amount',
  'ledger.ui.memo': 'Memo (optional, visible to the recipient)',
  'ledger.ui.available': 'Available {n}',
  'ledger.ui.rules': 'Fee {fee}% · minimum {min} · daily limit {limit}',
  'ledger.ui.unlimited': 'none',
  'ledger.ui.no_transferable': 'No currency can be transferred right now',
  'ledger.ui.confirm': 'Send {amount} {cur} to {to}?',
  'ledger.ui.sent': 'Sent; fee {fee}, balance {after}',
  'ledger.ui.currencies': 'Currencies',
  'ledger.ui.transferable': 'Transferable',
  'ledger.ui.fee_bps': 'Fee (basis points)',
  'ledger.ui.min_transfer': 'Minimum transfer',
  'ledger.ui.daily_out_limit': 'Daily outgoing limit (0 = none)',
  'ledger.ui.disabled': 'Disabled',
  'ledger.ui.save': 'Save',
  'ledger.ui.units_hint': 'Amounts are in minor units ({scale} decimals)',
  'ledger.transfer_disabled': 'This currency cannot be transferred',
  'ledger.amount_below_minimum': 'The amount is below the minimum',
  'ledger.recipient_invalid': 'The recipient does not exist or cannot receive',
  'ledger.insufficient_balance': 'Insufficient balance',
  'ledger.self_transfer': 'You cannot send to yourself',
  'ledger.currency_disabled': 'This currency is disabled',
  'ledger.already_reversed': 'This entry was already reversed',
  'ledger.negative_not_allowed': 'The reversal would make the balance negative',
  'ledger.daily_limit_exceeded': 'Daily outgoing limit exceeded',
  'ledger.account_locked': 'The account is locked',
});

interface Bal {
  currency: string;
  scale: number;
  balance: number;
  held: number;
  locked: boolean;
}

const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e);
const fmt = (v: number, scale: number) => (v / 10 ** scale).toLocaleString(undefined, { maximumFractionDigits: scale });

function UserTab({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const uid = ctx.profile.id;
  const [rows, setRows] = useState<Bal[] | null>(null);
  const [f, setF] = useState({ currency: 'bonus', amount: '', note: '' });
  const load = () => void api.json<{ items: Bal[] }>(`/api/p/ledger/v1/admin/users/${uid}/balances`).then((r) => setRows(r.items));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, [uid]);
  const scale = rows?.find((r) => r.currency === f.currency)?.scale ?? 0;
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await api.json('/api/p/ledger/v1/admin/adjustments', {
        json: { user_id: uid, currency: f.currency, amount: Math.round(Number(f.amount) * 10 ** scale), reason_note: f.note },
        idempotent: true,
      });
      api.toast(api.t('ledger.ui.done'), 'success');
      setF({ ...f, amount: '', note: '' });
      load();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  if (!rows) return null;
  return (
    <Card>
      <Table
        rows={rows}
        rowKey={(r) => r.currency}
        columns={[
          { key: 'c', header: api.t('ledger.ui.currency'), render: (r) => <>{r.currency} {r.locked && <Badge tone="danger">{api.t('ledger.ui.locked')}</Badge>}</> },
          { key: 'b', header: api.t('ledger.ui.balance'), render: (r) => fmt(r.balance, r.scale) },
          { key: 'h', header: api.t('ledger.ui.held'), render: (r) => fmt(r.held, r.scale) },
        ]}
      />
      <form onSubmit={(e) => void submit(e)} className="upload-grid">
        <Field label={api.t('ledger.ui.currency')}>
          <select className="pt-input" name="ledger_currency" value={f.currency} onChange={(e) => setF({ ...f, currency: e.target.value })}>
            {[...new Set(['bonus', ...rows.map((r) => r.currency)])].map((c) => (
              <option key={c} value={c}>
                {c}
              </option>
            ))}
          </select>
        </Field>
        <Field label={api.t('ledger.ui.amount')}>
          <Input name="ledger_amount" type="number" step="any" value={f.amount} onChange={(e) => setF({ ...f, amount: e.target.value })} />
        </Field>
        <div className="span2">
          <Field label={api.t('ledger.ui.note')}>
            <Input name="ledger_note" required maxLength={500} value={f.note} onChange={(e) => setF({ ...f, note: e.target.value })} />
          </Field>
          <Button type="submit" variant="danger" disabled={!f.note.trim() || !Number(f.amount)}>
            {api.t('ledger.ui.apply')}
          </Button>
        </div>
      </form>
      <h3>{api.t('ledger.ui.entries')}</h3>
      <Entries api={api} uid={uid} scaleOf={(c) => rows.find((r) => r.currency === c)?.scale ?? 0} onChange={load} />
    </Card>
  );
}

interface Entry {
  entry_id: string;
  currency: string;
  kind: string;
  amount: number;
  balance_after: number;
  reason: string;
  memo: string | null;
  counterparty_user_id: number | null;
  reversed: boolean;
  created_at: number;
}

/** 后台：用户流水（倒序分页）与冲正 */
function Entries({ api, uid, scaleOf, onChange }: { api: PluginApi; uid: number; scaleOf(c: string): number; onChange(): void }) {
  const [rows, setRows] = useState<Entry[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const load = useCallback(
    async (cursor: string | null) => {
      const q = new URLSearchParams({ limit: '50' });
      if (cursor) q.set('cursor', cursor);
      const r = await api.json<{ items: Entry[]; next_cursor: string | null }>(`/api/p/ledger/v1/admin/users/${uid}/entries?${q.toString()}`);
      setRows((old) => (cursor ? [...old, ...r.items] : r.items));
      setNext(r.next_cursor);
    },
    [api, uid],
  );
  useEffect(() => {
    void load(null).catch(() => undefined);
  }, [load]);
  const reverse = async (e: Entry) => {
    const note = window.prompt(api.t('ledger.ui.reverse_prompt'));
    if (!note?.trim()) return;
    try {
      await api.json(`/api/p/ledger/v1/admin/entries/${encodeURIComponent(e.entry_id)}/reversals`, { json: { reason_note: note.trim() }, idempotent: true });
      api.toast(api.t('ledger.ui.done'), 'success');
      await load(null);
      onChange();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  return (
    <>
      <Table
        rows={rows}
        rowKey={(r) => `${r.entry_id}:${r.currency}:${r.amount}`}
        empty="—"
        columns={[
          { key: 't', header: api.t('ledger.ui.time'), render: (r) => <span className="pt-muted small">{new Date(r.created_at).toLocaleString()}</span> },
          {
            key: 'r',
            header: api.t('ledger.ui.reason'),
            render: (r) => (
              <>
                <code className="small">{r.reason}</code> {r.counterparty_user_id !== null && <span className="pt-muted small">↔ #{r.counterparty_user_id}</span>}
                {r.memo && <div className="pt-muted small">{r.memo}</div>}
              </>
            ),
          },
          { key: 'a', header: api.t('ledger.ui.amount_col'), render: (r) => <span className={r.amount >= 0 ? 'ledger-in' : 'ledger-out'}>{`${r.amount >= 0 ? '+' : ''}${fmt(r.amount, scaleOf(r.currency))} ${r.currency}`}</span> },
          { key: 'b', header: api.t('ledger.ui.after'), render: (r) => fmt(r.balance_after, scaleOf(r.currency)) },
          {
            key: 'x',
            header: '',
            render: (r) =>
              r.reversed ? (
                <Badge>{api.t('ledger.ui.reversed')}</Badge>
              ) : (
                <Button variant="ghost" onClick={() => void reverse(r)}>
                  {api.t('ledger.ui.reverse')}
                </Button>
              ),
          },
        ]}
      />
      {next && <Button onClick={() => void load(next)}>{api.t('ledger.ui.more')}</Button>}
    </>
  );
}

interface Currency {
  code: string;
  scale: number;
  name_term: string;
  transferable: boolean;
  fee_bps: number;
  min_transfer: number;
  daily_out_limit: number;
  disabled: boolean;
}
interface MyBal {
  currency: string;
  scale: number;
  available: number;
}

const curName = (api: PluginApi, c: Currency) => {
  const n = api.t(`term:${c.name_term}`);
  return n === c.name_term ? c.code : n;
};

/** 转账页（/p/ledger/transfer）：可转账货币、可用余额、规则说明 */
function Transfer({ api }: { api: PluginApi }) {
  const [curs, setCurs] = useState<Currency[] | null>(null);
  const [bals, setBals] = useState<MyBal[]>([]);
  const [f, setF] = useState({ currency: '', to: '', amount: '', memo: '' });
  const [busy, setBusy] = useState(false);
  const load = () => {
    void api.json<{ items: Currency[] }>('/api/p/ledger/v1/currencies').then((r) => setCurs(r.items.filter((c) => c.transferable && !c.disabled)));
    void api.json<{ items: MyBal[] }>('/api/p/ledger/v1/users/me/balances').then((r) => setBals(r.items));
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, []);
  const cur = curs?.find((c) => c.code === f.currency) ?? curs?.[0];
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!cur) return;
    const amount = Math.round(Number(f.amount) * 10 ** cur.scale);
    if (!window.confirm(api.t('ledger.ui.confirm', { to: f.to.trim(), amount: f.amount, cur: curName(api, cur) }))) return;
    setBusy(true);
    try {
      const r = await api.json<{ fee: number; balance_after: number }>('/api/p/ledger/v1/transfers', {
        json: { currency: cur.code, to_username: f.to.trim(), amount, memo: f.memo },
        idempotent: true,
      });
      api.toast(api.t('ledger.ui.sent', { fee: fmt(r.fee, cur.scale), after: fmt(r.balance_after, cur.scale) }), 'success');
      setF({ ...f, amount: '', memo: '' });
      load();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    } finally {
      setBusy(false);
    }
  };
  if (!curs) return null;
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t('ledger.ui.transfer_title')}</h1>
      {!cur ? (
        <Card>
          <p className="pt-muted">{api.t('ledger.ui.no_transferable')}</p>
        </Card>
      ) : (
        <Card>
          <div className="pt-row stats-row">
            {curs.map((c) => {
              const b = bals.find((x) => x.currency === c.code);
              return <Stat key={c.code} label={curName(api, c)} value={fmt(b?.available ?? 0, c.scale)} />;
            })}
          </div>
          <form onSubmit={(e) => void submit(e)} className="upload-grid">
            <Field label={api.t('ledger.ui.currency')}>
              <select className="pt-input" name="transfer_currency" value={cur.code} onChange={(e) => setF({ ...f, currency: e.target.value })}>
                {curs.map((c) => (
                  <option key={c.code} value={c.code}>
                    {curName(api, c)}
                  </option>
                ))}
              </select>
            </Field>
            <Field label={api.t('ledger.ui.to')}>
              <Input name="transfer_to" required maxLength={64} autoComplete="off" value={f.to} onChange={(e) => setF({ ...f, to: e.target.value })} />
            </Field>
            <Field
              label={api.t('ledger.ui.amount_plain')}
              hint={api.t('ledger.ui.rules', {
                fee: (cur.fee_bps / 100).toLocaleString(),
                min: fmt(cur.min_transfer, cur.scale),
                limit: cur.daily_out_limit > 0 ? fmt(cur.daily_out_limit, cur.scale) : api.t('ledger.ui.unlimited'),
              })}
            >
              <Input name="transfer_amount" type="number" min={0} step="any" required value={f.amount} onChange={(e) => setF({ ...f, amount: e.target.value })} />
            </Field>
            <Field label={api.t('ledger.ui.memo')}>
              <Input name="transfer_memo" maxLength={200} value={f.memo} onChange={(e) => setF({ ...f, memo: e.target.value })} />
            </Field>
            <div className="span2">
              <Button type="submit" variant="primary" disabled={busy || !f.to.trim() || !(Number(f.amount) > 0)}>
                {api.t('ledger.ui.transfer')}
              </Button>
            </div>
          </form>
        </Card>
      )}
    </div>
  );
}

/** 后台货币设置（/admin/p/ledger/） */
function Currencies({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<Currency[]>([]);
  const load = () => void api.json<{ items: Currency[] }>('/api/p/ledger/v1/currencies').then((r) => setRows(r.items));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, []);
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t('ledger.ui.currencies')}</h1>
      {rows.map((c) => (
        <CurrencyForm key={c.code} api={api} c={c} onSaved={load} />
      ))}
    </div>
  );
}

function CurrencyForm({ api, c, onSaved }: { api: PluginApi; c: Currency; onSaved(): void }) {
  const [f, setF] = useState({ transferable: c.transferable, fee_bps: String(c.fee_bps), min_transfer: String(c.min_transfer), daily_out_limit: String(c.daily_out_limit), disabled: c.disabled });
  const save = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await api.json(`/api/p/ledger/v1/admin/currencies/${encodeURIComponent(c.code)}`, {
        method: 'PATCH',
        json: { transferable: f.transferable, fee_bps: Number(f.fee_bps), min_transfer: Number(f.min_transfer), daily_out_limit: Number(f.daily_out_limit), disabled: f.disabled },
      });
      api.toast(api.t('ledger.ui.saved'), 'success');
      onSaved();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  return (
    <Card title={`${curName(api, c)} (${c.code})`}>
      <p className="pt-muted small">{api.t('ledger.ui.units_hint', { scale: c.scale })}</p>
      <form onSubmit={(e) => void save(e)} className="upload-grid">
        <label className="pt-check">
          <input type="checkbox" name={`${c.code}_transferable`} checked={f.transferable} onChange={(e) => setF({ ...f, transferable: e.target.checked })} /> {api.t('ledger.ui.transferable')}
        </label>
        <label className="pt-check">
          <input type="checkbox" name={`${c.code}_disabled`} checked={f.disabled} onChange={(e) => setF({ ...f, disabled: e.target.checked })} /> {api.t('ledger.ui.disabled')}
        </label>
        <Field label={api.t('ledger.ui.fee_bps')}>
          <Input name={`${c.code}_fee_bps`} type="number" min={0} max={10000} value={f.fee_bps} onChange={(e) => setF({ ...f, fee_bps: e.target.value })} />
        </Field>
        <Field label={api.t('ledger.ui.min_transfer')}>
          <Input name={`${c.code}_min_transfer`} type="number" min={1} value={f.min_transfer} onChange={(e) => setF({ ...f, min_transfer: e.target.value })} />
        </Field>
        <Field label={api.t('ledger.ui.daily_out_limit')}>
          <Input name={`${c.code}_daily_out_limit`} type="number" min={0} value={f.daily_out_limit} onChange={(e) => setF({ ...f, daily_out_limit: e.target.value })} />
        </Field>
        <div className="span2">
          <Button type="submit" variant="primary">
            {api.t('ledger.ui.save')}
          </Button>
        </div>
      </form>
    </Card>
  );
}

export default {
  slots: {
    'admin.user.tabs': { component: UserTab, order: 11, title_term: 'ledger.ui.tab' },
  },
  routes: [
    { path: '/p/ledger/*', component: Transfer },
    { path: '/admin/p/ledger/*', component: Currencies },
  ],
} satisfies PluginFrontend;
