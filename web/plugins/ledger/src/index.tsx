// ledger 前端：后台用户详情页“余额”标签 —— 各货币余额（含冻结）与手动调整（正数发放、负数扣除，需填写原因）。
import { useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Field, Input, Table } from '@pt-studio/ui-kit';

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
    </Card>
  );
}

export default {
  slots: {
    'admin.user.tabs': { component: UserTab, order: 11, title_term: 'ledger.ui.tab' },
  },
  routes: [],
} satisfies PluginFrontend;
