// ledger 前端：用户转账页（/p/ledger/transfer）；后台用户详情页“余额”标签 —— 各货币余额（含冻结）、手动调整（正数发放、负数扣除，
// 需填写原因）、流水与冲正；后台货币设置（/admin/p/ledger/：可否转账、手续费、最小额、每日上限、停用）。
import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Checkbox, Field, Input, KeyFacts, Kpis, LoadMore, Note, PageHead, Panel, Select, Sheet, SkeletonRows, Status, Table, Tag } from '@pt-studio/ui-kit';

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
  'ledger.ui.reverse_prompt': '冲正这笔流水会反向记一笔，原因写入审计。',
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
  'ledger.ui.cancel': '取消',
  'ledger.ui.edit': '编辑',
  'ledger.ui.fee': '手续费',
  'ledger.ui.state': '状态',
  'ledger.ui.enabled': '启用',
  'ledger.ui.yes': '可转账',
  'ledger.ui.no': '不可转账',
  'ledger.ui.transfer_lede': '把魔力值等站内货币转给其他用户。转账即时到账、不可撤回，请核对收款人。',
  'ledger.ui.check': '核对转账',
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
  'ledger.ui.reverse_prompt': 'Reversing posts an opposite entry; the reason is audited.',
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
  'ledger.ui.cancel': 'Cancel',
  'ledger.ui.edit': 'Edit',
  'ledger.ui.fee': 'Fee',
  'ledger.ui.state': 'State',
  'ledger.ui.enabled': 'Enabled',
  'ledger.ui.yes': 'Transferable',
  'ledger.ui.no': 'Not transferable',
  'ledger.ui.transfer_lede': 'Send site currency to another user. Transfers are instant and final — double-check the recipient.',
  'ledger.ui.check': 'Review transfer',
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
  const [open, setOpen] = useState(false);
  const [f, setF] = useState({ currency: 'bonus', amount: '', note: '' });
  const [tick, setTick] = useState(0);
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
      setOpen(false);
      load();
      setTick((t) => t + 1);
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  if (!rows) return <SkeletonRows n={3} />;
  return (
    <div className="stack">
      <Panel
        title={api.t('ledger.ui.balance')}
        flush
        actions={
          <Button size="sm" variant="primary" onClick={() => setOpen(true)}>
            {api.t('ledger.ui.apply')}
          </Button>
        }
      >
        <Table
          rows={rows}
          rowKey={(r) => r.currency}
          columns={[
            { key: 'c', header: api.t('ledger.ui.currency'), render: (r) => <>{r.currency} {r.locked && <Status tone="bad">{api.t('ledger.ui.locked')}</Status>}</> },
            { key: 'b', header: api.t('ledger.ui.balance'), right: true, render: (r) => <span className="num">{fmt(r.balance, r.scale)}</span> },
            { key: 'h', header: api.t('ledger.ui.held'), right: true, render: (r) => <span className="num muted">{fmt(r.held, r.scale)}</span> },
          ]}
        />
      </Panel>
      <Panel title={api.t('ledger.ui.entries')} flush>
        <Entries key={tick} api={api} uid={uid} scaleOf={(c) => rows.find((r) => r.currency === c)?.scale ?? 0} onChange={load} />
      </Panel>
      <Sheet
        open={open}
        onClose={() => setOpen(false)}
        title={api.t('ledger.ui.apply')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setOpen(false)}>
              {api.t('ledger.ui.cancel')}
            </Button>
            <Button type="submit" form="ledger-adjust" variant="danger" disabled={!f.note.trim() || !Number(f.amount)}>
              {api.t('ledger.ui.apply')}
            </Button>
          </>
        }
      >
        <form id="ledger-adjust" className="form" onSubmit={(e) => void submit(e)}>
          <Field label={api.t('ledger.ui.currency')}>
            <Select name="ledger_currency" value={f.currency} onChange={(e) => setF({ ...f, currency: e.target.value })} options={[...new Set(['bonus', ...rows.map((r) => r.currency)])]} />
          </Field>
          <Field label={api.t('ledger.ui.amount')}>
            <Input name="ledger_amount" type="number" step="any" value={f.amount} onChange={(e) => setF({ ...f, amount: e.target.value })} />
          </Field>
          <Field label={api.t('ledger.ui.note')}>
            <Input name="ledger_note" required maxLength={500} value={f.note} onChange={(e) => setF({ ...f, note: e.target.value })} />
          </Field>
        </form>
      </Sheet>
    </div>
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

/** 后台：用户流水（倒序分页）与冲正（抽屉里填写原因） */
function Entries({ api, uid, scaleOf, onChange }: { api: PluginApi; uid: number; scaleOf(c: string): number; onChange(): void }) {
  const [rows, setRows] = useState<Entry[] | null>(null);
  const [next, setNext] = useState<string | null>(null);
  const [rev, setRev] = useState<Entry | null>(null);
  const [note, setNote] = useState('');
  const load = useCallback(
    async (cursor: string | null) => {
      const q = new URLSearchParams({ limit: '50' });
      if (cursor) q.set('cursor', cursor);
      const r = await api.json<{ items: Entry[]; next_cursor: string | null }>(`/api/p/ledger/v1/admin/users/${uid}/entries?${q.toString()}`);
      setRows((old) => (cursor ? [...(old ?? []), ...r.items] : r.items));
      setNext(r.next_cursor);
    },
    [api, uid],
  );
  useEffect(() => {
    void load(null).catch(() => setRows([]));
  }, [load]);
  const reverse = async () => {
    if (!rev || !note.trim()) return;
    try {
      await api.json(`/api/p/ledger/v1/admin/entries/${encodeURIComponent(rev.entry_id)}/reversals`, { json: { reason_note: note.trim() }, idempotent: true });
      api.toast(api.t('ledger.ui.done'), 'success');
      setRev(null);
      setNote('');
      await load(null);
      onChange();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  if (rows === null) return <SkeletonRows n={4} />;
  return (
    <>
      <Table
        rows={rows}
        rowKey={(r) => `${r.entry_id}:${r.currency}:${r.amount}`}
        empty="—"
        minWidth={640}
        columns={[
          { key: 't', header: api.t('ledger.ui.time'), render: (r) => <span className="small muted num nowrap">{new Date(r.created_at).toLocaleString()}</span> },
          {
            key: 'r',
            header: api.t('ledger.ui.reason'),
            render: (r) => (
              <>
                <span className="pt-mono small">{r.reason}</span> {r.counterparty_user_id !== null && <span className="small muted">↔ #{r.counterparty_user_id}</span>}
                {r.memo && <div className="tiny muted">{r.memo}</div>}
              </>
            ),
          },
          { key: 'a', header: api.t('ledger.ui.amount_col'), right: true, render: (r) => <span className={r.amount >= 0 ? 'num up' : 'num down'}>{`${r.amount >= 0 ? '+' : ''}${fmt(r.amount, scaleOf(r.currency))} ${r.currency}`}</span> },
          { key: 'b', header: api.t('ledger.ui.after'), right: true, render: (r) => <span className="num">{fmt(r.balance_after, scaleOf(r.currency))}</span> },
          {
            key: 'x',
            header: '',
            right: true,
            render: (r) =>
              r.reversed ? (
                <Status tone="mute">{api.t('ledger.ui.reversed')}</Status>
              ) : (
                <Button size="sm" variant="ghost" onClick={() => setRev(r)}>
                  {api.t('ledger.ui.reverse')}
                </Button>
              ),
          },
        ]}
      />
      {next && <LoadMore onClick={() => void load(next)} label={api.t('ledger.ui.more')} />}
      <Sheet
        open={rev !== null}
        onClose={() => setRev(null)}
        title={api.t('ledger.ui.reverse')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setRev(null)}>
              {api.t('ledger.ui.cancel')}
            </Button>
            <Button variant="danger" disabled={!note.trim()} onClick={() => void reverse()}>
              {api.t('ledger.ui.reverse')}
            </Button>
          </>
        }
      >
        <div className="form">
          <p className="small muted">{api.t('ledger.ui.reverse_prompt')}</p>
          {rev && <span className="pt-mono small">{`${rev.reason} · ${rev.amount >= 0 ? '+' : ''}${fmt(rev.amount, scaleOf(rev.currency))} ${rev.currency}`}</span>}
          <Field label={api.t('ledger.ui.note')}>
            <Input name="reverse_note" maxLength={500} value={note} onChange={(e) => setNote(e.target.value)} />
          </Field>
        </div>
      </Sheet>
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

/** 转账页（/p/ledger/transfer）：可转账货币、可用余额、规则说明；提交前在抽屉里核对 */
function Transfer({ api }: { api: PluginApi }) {
  const [curs, setCurs] = useState<Currency[] | null>(null);
  const [bals, setBals] = useState<MyBal[]>([]);
  const [f, setF] = useState({ currency: '', to: '', amount: '', memo: '' });
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const load = () => {
    void api.json<{ items: Currency[] }>('/api/p/ledger/v1/currencies').then((r) => setCurs(r.items.filter((c) => c.transferable && !c.disabled)));
    void api.json<{ items: MyBal[] }>('/api/p/ledger/v1/users/me/balances').then((r) => setBals(r.items));
  };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, []);
  const cur = curs?.find((c) => c.code === f.currency) ?? curs?.[0];
  const send = async () => {
    if (!cur) return;
    const amount = Math.round(Number(f.amount) * 10 ** cur.scale);
    setBusy(true);
    try {
      const r = await api.json<{ fee: number; balance_after: number }>('/api/p/ledger/v1/transfers', {
        json: { currency: cur.code, to_username: f.to.trim(), amount, memo: f.memo },
        idempotent: true,
      });
      api.toast(api.t('ledger.ui.sent', { fee: fmt(r.fee, cur.scale), after: fmt(r.balance_after, cur.scale) }), 'success');
      setF({ ...f, amount: '', memo: '' });
      setConfirming(false);
      load();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    } finally {
      setBusy(false);
    }
  };
  if (!curs) return <SkeletonRows n={4} />;
  const fee = cur ? (Number(f.amount) || 0) * (cur.fee_bps / 10_000) : 0;
  return (
    <div className="stack narrow-form">
      <PageHead title={api.t('ledger.ui.transfer_title')} lede={api.t('ledger.ui.transfer_lede')} />
      {!cur ? (
        <Note>{api.t('ledger.ui.no_transferable')}</Note>
      ) : (
        <>
          <Kpis
            items={curs.map((c) => ({ label: curName(api, c), value: fmt(bals.find((x) => x.currency === c.code)?.available ?? 0, c.scale), tone: 'gold' as const }))}
          />
          <form
            className="panel pad-l form"
            data-primary
            onSubmit={(e) => {
              e.preventDefault();
              setConfirming(true);
            }}
          >
            <div className="form-2">
              <Field label={api.t('ledger.ui.currency')}>
                <Select name="transfer_currency" value={cur.code} onChange={(e) => setF({ ...f, currency: e.target.value })} options={curs.map((c) => ({ value: c.code, label: curName(api, c) }))} />
              </Field>
              <Field label={api.t('ledger.ui.to')}>
                <Input name="transfer_to" required maxLength={64} autoComplete="off" value={f.to} onChange={(e) => setF({ ...f, to: e.target.value })} />
              </Field>
            </div>
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
            <div className="form-acts">
              <Button type="submit" variant="primary" disabled={busy || !f.to.trim() || !(Number(f.amount) > 0)}>
                {api.t('ledger.ui.transfer')}
              </Button>
            </div>
          </form>
          <Sheet
            open={confirming}
            onClose={() => setConfirming(false)}
            title={api.t('ledger.ui.check')}
            footer={
              <>
                <Button variant="ghost" onClick={() => setConfirming(false)}>
                  {api.t('ledger.ui.cancel')}
                </Button>
                <Button variant="primary" disabled={busy} onClick={() => void send()}>
                  {api.t('ledger.ui.transfer')}
                </Button>
              </>
            }
          >
            <div className="stack">
              <p>{api.t('ledger.ui.confirm', { to: f.to.trim(), amount: f.amount, cur: curName(api, cur) })}</p>
              <KeyFacts
                items={[
                  { label: api.t('ledger.ui.to'), value: f.to.trim() },
                  { label: api.t('ledger.ui.amount_plain'), value: f.amount, tone: 'gold' },
                  { label: api.t('ledger.ui.fee'), value: fee.toLocaleString(undefined, { maximumFractionDigits: cur.scale }) },
                ]}
              />
              {f.memo && <p className="small muted">{f.memo}</p>}
            </div>
          </Sheet>
        </>
      )}
    </div>
  );
}

/** 后台货币设置（/admin/p/ledger/）：表格总览，抽屉编辑 */
function Currencies({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<Currency[] | null>(null);
  const [edit, setEdit] = useState<Currency | null>(null);
  const load = () => void api.json<{ items: Currency[] }>('/api/p/ledger/v1/currencies').then((r) => setRows(r.items));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, []);
  return (
    <div className="stack">
      <PageHead title={api.t('ledger.ui.currencies')} />
      <Panel flush primary>
        {rows === null ? (
          <SkeletonRows n={3} />
        ) : (
          <Table
            rows={rows}
            rowKey={(c) => c.code}
            minWidth={640}
            onRowClick={(c) => setEdit(c)}
            columns={[
              { key: 'n', header: api.t('ledger.ui.currency'), render: (c) => <><b>{curName(api, c)}</b> <span className="pt-mono tiny muted">{c.code}</span></> },
              { key: 't', header: api.t('ledger.ui.transferable'), render: (c) => (c.transferable ? <Tag tone="g">{api.t('ledger.ui.yes')}</Tag> : <Tag>{api.t('ledger.ui.no')}</Tag>) },
              { key: 'f', header: api.t('ledger.ui.fee'), right: true, render: (c) => <span className="num">{(c.fee_bps / 100).toLocaleString()}%</span> },
              { key: 'm', header: api.t('ledger.ui.min_transfer'), right: true, render: (c) => <span className="num">{fmt(c.min_transfer, c.scale)}</span> },
              { key: 'l', header: api.t('ledger.ui.daily_out_limit'), right: true, render: (c) => <span className="num">{c.daily_out_limit > 0 ? fmt(c.daily_out_limit, c.scale) : api.t('ledger.ui.unlimited')}</span> },
              { key: 's', header: api.t('ledger.ui.state'), render: (c) => (c.disabled ? <Status tone="bad">{api.t('ledger.ui.disabled')}</Status> : <Status tone="ok">{api.t('ledger.ui.enabled')}</Status>) },
              {
                key: 'x',
                header: '',
                right: true,
                render: (c) => (
                  <Button size="sm" variant="ghost" onClick={() => setEdit(c)}>
                    {api.t('ledger.ui.edit')}
                  </Button>
                ),
              },
            ]}
          />
        )}
      </Panel>
      {edit && (
        <CurrencySheet
          key={edit.code}
          api={api}
          c={edit}
          onClose={() => setEdit(null)}
          onSaved={() => {
            setEdit(null);
            load();
          }}
        />
      )}
    </div>
  );
}

function CurrencySheet({ api, c, onClose, onSaved }: { api: PluginApi; c: Currency; onClose(): void; onSaved(): void }) {
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
    <Sheet
      open
      onClose={onClose}
      title={`${curName(api, c)} (${c.code})`}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {api.t('ledger.ui.cancel')}
          </Button>
          <Button type="submit" form="ledger-currency" variant="primary">
            {api.t('ledger.ui.save')}
          </Button>
        </>
      }
    >
      <form id="ledger-currency" className="form" onSubmit={(e) => void save(e)}>
        <p className="small muted">{api.t('ledger.ui.units_hint', { scale: c.scale })}</p>
        <div className="row">
          <Checkbox name={`${c.code}_transferable`} label={api.t('ledger.ui.transferable')} checked={f.transferable} onChange={(v) => setF({ ...f, transferable: v })} />
          <Checkbox name={`${c.code}_disabled`} label={api.t('ledger.ui.disabled')} checked={f.disabled} onChange={(v) => setF({ ...f, disabled: v })} />
        </div>
        <Field label={api.t('ledger.ui.fee_bps')}>
          <Input name={`${c.code}_fee_bps`} type="number" min={0} max={10000} value={f.fee_bps} onChange={(e) => setF({ ...f, fee_bps: e.target.value })} />
        </Field>
        <div className="form-2">
          <Field label={api.t('ledger.ui.min_transfer')}>
            <Input name={`${c.code}_min_transfer`} type="number" min={1} value={f.min_transfer} onChange={(e) => setF({ ...f, min_transfer: e.target.value })} />
          </Field>
          <Field label={api.t('ledger.ui.daily_out_limit')}>
            <Input name={`${c.code}_daily_out_limit`} type="number" min={0} value={f.daily_out_limit} onChange={(e) => setF({ ...f, daily_out_limit: e.target.value })} />
          </Field>
        </div>
      </form>
    </Sheet>
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
