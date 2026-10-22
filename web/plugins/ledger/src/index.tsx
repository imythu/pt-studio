// ledger 前端：用户转账页（/p/ledger/transfer）；后台用户详情页“余额”标签 —— 各货币余额（含冻结）、手动调整（正数发放、负数扣除，
// 需填写原因）、流水与冲正；后台（/admin/p/ledger/）：批量发放 / 扣除（按状态 / 等级或用户名单，预览后确认，后台分块记账，
// 扣除时余额不足可跳过或扣到 0，可逐人发站内信）与货币设置（可否转账、手续费、最小额、每日上限、停用）。
import { useCallback, useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Checkbox, Field, Input, Kpis, LoadMore, Note, PageHead, Panel, Progress, Segmented, Select, Sheet, SkeletonRows, Status, Table, Tabs, Tag, Textarea } from '@pt-studio/ui-kit';

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
  'ledger.ui.admin': '账本',
  'ledger.ui.grants': '批量发放 / 扣除',
  'ledger.ui.grant_new': '批量发放 / 扣除',
  'ledger.ui.grant_lede': '给一批用户各发或各扣同一数量的货币。确认前会先算出人数和总额；提交后在后台分批记账，可同时给每人发一封站内信。',
  'ledger.ui.grant_direction': '操作',
  'ledger.ui.dir.grant': '发放',
  'ledger.ui.dir.deduct': '扣除',
  'ledger.ui.grant_shortfall': '余额不足时',
  'ledger.ui.shortfall.skip': '跳过此人',
  'ledger.ui.shortfall.partial': '有多少扣多少',
  'ledger.ui.grant_shortfall_hint': '可用余额 = 余额 − 冻结；不会扣成负数',
  'ledger.ui.grant_insufficient_skip': '{n} 人可用余额不足，会被跳过（不扣、不通知）',
  'ledger.ui.grant_insufficient_partial': '{n} 人可用余额不足，会扣到 0 为止',
  'ledger.ui.grant_total_est': '预计总额',
  'ledger.ui.grant_moved': '合计 {n}',
  'ledger.ui.grant_partial': '只扣了 {n}',
  'ledger.ui.deduct_confirm': '确认扣除',
  'ledger.ui.deduct_started': '已提交，正在后台扣除',
  'ledger.ui.deduct_state.granting': '扣除中',
  'ledger.ui.deduct_state.notifying': '通知中',
  'ledger.ui.deduct_state.done': '已完成',
  'ledger.ui.deduct_detail': '扣除 #{id}',
  'ledger.ui.deduct_granted': '已扣除',
  'ledger.ui.deduct_failed': '未扣除',
  'ledger.ui.grant_amount': '每人数量',
  'ledger.ui.grant_target': '发放对象',
  'ledger.ui.grant_by_filter': '按条件',
  'ledger.ui.grant_by_users': '指定用户',
  'ledger.ui.grant_statuses': '用户状态',
  'ledger.ui.grant_statuses_hint': '封禁和已删除的用户不会收到',
  'ledger.ui.status.active': '正常',
  'ledger.ui.status.leech_disabled': '禁止下载',
  'ledger.ui.status.pending': '待验证',
  'ledger.ui.grant_classes': '等级',
  'ledger.ui.grant_classes_hint': '不勾选 = 所有等级',
  'ledger.ui.grant_usernames': '用户名',
  'ledger.ui.grant_usernames_hint': '每行一个，也可用逗号或空格分隔；不区分大小写；最多 5000 个',
  'ledger.ui.grant_memo': '原因（必填，用户在流水里能看到）',
  'ledger.ui.grant_amount_hint': '扣除时为每人最多扣的数量',
  'ledger.ui.grant_notify': '给每个人发站内信通知',
  'ledger.ui.grant_notify_off': '站内信插件未启用，无法通知',
  'ledger.ui.grant_subject': '站内信标题',
  'ledger.ui.grant_body': '站内信正文（Markdown）',
  'ledger.ui.grant_vars': '可用变量：{username} 用户名、{amount} 数量、{currency} 货币名、{balance} 记账后余额、{memo} 原因',
  'ledger.ui.grant_next': '计算人数',
  'ledger.ui.grant_back': '返回修改',
  'ledger.ui.grant_confirm': '确认发放',
  'ledger.ui.grant_count': '人数',
  'ledger.ui.grant_total': '总额',
  'ledger.ui.grant_missing': '以下用户名不存在或已封禁，会被跳过：{names}',
  'ledger.ui.grant_none': '没有符合条件的用户',
  'ledger.ui.grant_too_many': '人数超过上限 {max}，请缩小范围',
  'ledger.ui.grant_started': '已提交，正在后台发放',
  'ledger.ui.grant_empty': '还没有批量发放记录',
  'ledger.ui.grant_progress': '进度',
  'ledger.ui.grant_notified': '已通知 {n}',
  'ledger.ui.grant_by': '发起人',
  'ledger.ui.grant_state.granting': '发放中',
  'ledger.ui.grant_state.notifying': '通知中',
  'ledger.ui.grant_state.done': '已完成',
  'ledger.ui.grant_target_all': '{statuses}',
  'ledger.ui.grant_target_classes': '{statuses} · {n} 个等级',
  'ledger.ui.grant_target_users': '{n} 个指定用户',
  'ledger.ui.grant_detail': '发放 #{id}',
  'ledger.ui.grant_granted': '已发放',
  'ledger.ui.grant_failed': '发放失败',
  'ledger.ui.grant_notify_failed': '通知失败',
  'ledger.ui.grant_notify_col': '已通知',
  'ledger.ui.grant_failures': '异常名单',
  'ledger.ui.grant_error': '错误',
  'ledger.ui.grant_last_error': '最近错误：{e}',
  'ledger.ui.grant_resume': '继续执行',
  'ledger.ui.grant_resumed': '已重新排队',
  'ledger.ui.grant_no_notify': '不通知',
  'ledger.grant_no_recipients': '没有符合条件的用户',
  'ledger.grant_too_many': '人数超过上限',
  'ledger.grant_finished': '这批已经完成',
  'ledger.amount_out_of_range': '数量超出范围',
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
  'ledger.ui.admin': 'Ledger',
  'ledger.ui.grants': 'Bulk grant / deduct',
  'ledger.ui.grant_new': 'Bulk grant / deduct',
  'ledger.ui.grant_lede': 'Give or take the same amount for a group of users. You see the head count and total before confirming; entries are posted in the background, optionally with a site message to each person.',
  'ledger.ui.grant_direction': 'Operation',
  'ledger.ui.dir.grant': 'Grant',
  'ledger.ui.dir.deduct': 'Deduct',
  'ledger.ui.grant_shortfall': 'If the balance is too low',
  'ledger.ui.shortfall.skip': 'Skip the user',
  'ledger.ui.shortfall.partial': 'Take what is there',
  'ledger.ui.grant_shortfall_hint': 'Available = balance − held; balances never go negative',
  'ledger.ui.grant_insufficient_skip': '{n} users do not have enough and will be skipped (no deduction, no message)',
  'ledger.ui.grant_insufficient_partial': '{n} users do not have enough and will be taken down to 0',
  'ledger.ui.grant_total_est': 'Estimated total',
  'ledger.ui.grant_moved': 'Total {n}',
  'ledger.ui.grant_partial': 'Only {n} taken',
  'ledger.ui.deduct_confirm': 'Deduct now',
  'ledger.ui.deduct_started': 'Submitted; deducting in the background',
  'ledger.ui.deduct_state.granting': 'Deducting',
  'ledger.ui.deduct_state.notifying': 'Notifying',
  'ledger.ui.deduct_state.done': 'Done',
  'ledger.ui.deduct_detail': 'Deduction #{id}',
  'ledger.ui.deduct_granted': 'Deducted',
  'ledger.ui.deduct_failed': 'Not deducted',
  'ledger.ui.grant_amount': 'Amount per user',
  'ledger.ui.grant_target': 'Recipients',
  'ledger.ui.grant_by_filter': 'By filter',
  'ledger.ui.grant_by_users': 'By username',
  'ledger.ui.grant_statuses': 'User status',
  'ledger.ui.grant_statuses_hint': 'Banned and deleted users never receive grants',
  'ledger.ui.status.active': 'Active',
  'ledger.ui.status.leech_disabled': 'Download disabled',
  'ledger.ui.status.pending': 'Pending',
  'ledger.ui.grant_classes': 'Classes',
  'ledger.ui.grant_classes_hint': 'None checked = all classes',
  'ledger.ui.grant_usernames': 'Usernames',
  'ledger.ui.grant_usernames_hint': 'One per line, or separated by commas or spaces; case-insensitive; up to 5000',
  'ledger.ui.grant_memo': 'Reason (required, shown in the user\'s history)',
  'ledger.ui.grant_amount_hint': 'For deductions: the most taken from each user',
  'ledger.ui.grant_notify': 'Send each user a site message',
  'ledger.ui.grant_notify_off': 'The messages plugin is not enabled; no notification can be sent',
  'ledger.ui.grant_subject': 'Message subject',
  'ledger.ui.grant_body': 'Message body (Markdown)',
  'ledger.ui.grant_vars': 'Variables: {username}, {amount}, {currency}, {balance} (after posting), {memo}',
  'ledger.ui.grant_next': 'Count recipients',
  'ledger.ui.grant_back': 'Back',
  'ledger.ui.grant_confirm': 'Grant now',
  'ledger.ui.grant_count': 'Recipients',
  'ledger.ui.grant_total': 'Total',
  'ledger.ui.grant_missing': 'These usernames do not exist or are banned and will be skipped: {names}',
  'ledger.ui.grant_none': 'No user matches',
  'ledger.ui.grant_too_many': 'More than {max} recipients; narrow the selection',
  'ledger.ui.grant_started': 'Submitted; granting in the background',
  'ledger.ui.grant_empty': 'No bulk grants yet',
  'ledger.ui.grant_progress': 'Progress',
  'ledger.ui.grant_notified': '{n} notified',
  'ledger.ui.grant_by': 'By',
  'ledger.ui.grant_state.granting': 'Granting',
  'ledger.ui.grant_state.notifying': 'Notifying',
  'ledger.ui.grant_state.done': 'Done',
  'ledger.ui.grant_target_all': '{statuses}',
  'ledger.ui.grant_target_classes': '{statuses} · {n} classes',
  'ledger.ui.grant_target_users': '{n} named users',
  'ledger.ui.grant_detail': 'Grant #{id}',
  'ledger.ui.grant_granted': 'Granted',
  'ledger.ui.grant_failed': 'Grant failed',
  'ledger.ui.grant_notify_failed': 'Notify failed',
  'ledger.ui.grant_notify_col': 'Notified',
  'ledger.ui.grant_failures': 'Exceptions',
  'ledger.ui.grant_error': 'Error',
  'ledger.ui.grant_last_error': 'Last error: {e}',
  'ledger.ui.grant_resume': 'Resume',
  'ledger.ui.grant_resumed': 'Queued again',
  'ledger.ui.grant_no_notify': 'No message',
  'ledger.grant_no_recipients': 'No user matches',
  'ledger.grant_too_many': 'Too many recipients',
  'ledger.grant_finished': 'This batch has already finished',
  'ledger.amount_out_of_range': 'Amount out of range',
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
      if (uid === api.user()?.id) api.refreshMe();
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
      api.refreshMe();
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
              <Kpis
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
    <>
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
    </>
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

// ---------------- 批量发放（/admin/p/ledger/） ----------------

type Target = { kind: 'filter'; statuses: string[]; class_ids: number[] } | { kind: 'users'; usernames: string[] };
type Direction = 'grant' | 'deduct';
interface Grant {
  id: number;
  currency: string;
  amount: number;
  direction: Direction;
  shortfall: 'skip' | 'partial';
  moved: number;
  memo: string;
  target: Target;
  notify: boolean;
  state: 'granting' | 'notifying' | 'done';
  total: number;
  granted: number;
  failed: number;
  notified: number;
  notify_failed: number;
  last_error: string | null;
  created_by_name: string | null;
  created_at: number;
  scale: number;
  failures?: { user_id: number; username: string | null; error: string | null; notify_error: string | null; amount: number | null }[];
}
interface Preview {
  count: number;
  insufficient: number;
  total_amount: number | null;
  scale: number;
  missing: string[];
  max_recipients: number;
  notify_available: boolean;
  default_subject: string;
  default_body: string;
}
interface LevelClass {
  id: number;
  key: string;
  rank: number;
  name?: string | null;
  name_key: string;
}

const STATUSES = ['active', 'leech_disabled', 'pending'];
const GRANTS = '/api/p/ledger/v1/admin/grants';

const levelName = (api: PluginApi, c: LevelClass) => {
  if (c.name) return c.name;
  const t = api.t(`term:${c.name_key}`);
  return t && t !== c.name_key && t !== `term:${c.name_key}` ? t : c.key;
};

const targetText = (api: PluginApi, t: Target) => {
  if (t.kind === 'users') return api.t('ledger.ui.grant_target_users', { n: t.usernames.length });
  const statuses = t.statuses.map((x) => api.t(`ledger.ui.status.${x}`)).join(' / ');
  return t.class_ids.length ? api.t('ledger.ui.grant_target_classes', { statuses, n: t.class_ids.length }) : api.t('ledger.ui.grant_target_all', { statuses });
};

const stateTone = (s: Grant['state']) => (s === 'done' ? 'ok' : 'info');
/** 扣除批次用 `ledger.ui.deduct_*` 文案 */
const dt = (api: PluginApi, d: Direction, key: string, args?: Record<string, unknown>) => api.t(d === 'deduct' ? `ledger.ui.deduct_${key}` : `ledger.ui.grant_${key}`, args);
const signed = (g: Pick<Grant, 'direction' | 'amount' | 'scale'>) => `${g.direction === 'deduct' ? '−' : '+'}${fmt(g.amount, g.scale)}`;

function Admin({ api }: { api: PluginApi }) {
  const [tab, setTab] = useState('grants');
  const [creating, setCreating] = useState(false);
  return (
    <div className="stack">
      <PageHead
        title={api.t('ledger.ui.admin')}
        actions={
          tab === 'grants' && (
            <Button variant="primary" onClick={() => setCreating(true)}>
              {api.t('ledger.ui.grant_new')}
            </Button>
          )
        }
      />
      <Tabs
        variant="sub"
        value={tab}
        onChange={setTab}
        tabs={[
          { id: 'grants', label: api.t('ledger.ui.grants') },
          { id: 'currencies', label: api.t('ledger.ui.currencies') },
        ]}
      />
      {tab === 'grants' ? <Grants api={api} creating={creating} setCreating={setCreating} /> : <Currencies api={api} />}
    </div>
  );
}

/** 发放记录：未完成时每 2 秒刷新；点行看详情 */
function Grants({ api, creating, setCreating }: { api: PluginApi; creating: boolean; setCreating(v: boolean): void }) {
  const [rows, setRows] = useState<Grant[] | null>(null);
  const [next, setNext] = useState<number | null>(null);
  const [open, setOpen] = useState<number | null>(null);
  const load = useCallback(async () => {
    const r = await api.json<{ items: Grant[]; next_before: number | null }>(`${GRANTS}?limit=20`);
    setRows(r.items);
    setNext(r.next_before);
  }, [api]);
  useEffect(() => {
    void load().catch(() => setRows([]));
  }, [load]);
  const running = rows?.some((g) => g.state !== 'done') ?? false;
  useEffect(() => {
    if (!running) return;
    const t = setInterval(() => void load().catch(() => undefined), 2000);
    return () => clearInterval(t);
  }, [running, load]);
  const more = async () => {
    if (next === null) return;
    const r = await api.json<{ items: Grant[]; next_before: number | null }>(`${GRANTS}?limit=20&before=${next}`);
    setRows((old) => [...(old ?? []), ...r.items]);
    setNext(r.next_before);
  };
  return (
    <>
      <Panel flush primary>
        {rows === null ? (
          <SkeletonRows n={4} />
        ) : (
          <Table
            rows={rows}
            rowKey={(g) => g.id}
            empty={api.t('ledger.ui.grant_empty')}
            minWidth={640}
            onRowClick={(g) => setOpen(g.id)}
            columns={[
              // 窄屏横向滚动时先看到“发给谁、进行到哪”：对象与进度在前，数量与发起人在后
              {
                key: 'w',
                header: api.t('ledger.ui.grant_target'),
                render: (g) => (
                  <>
                    <div className="small">
                      <Tag tone={g.direction === 'deduct' ? 'x' : 'k'}>{api.t(`ledger.ui.dir.${g.direction}`)}</Tag> {targetText(api, g.target)}
                    </div>
                    <div className="tiny muted">{g.memo}</div>
                    <div className="tiny muted num nowrap">{new Date(g.created_at).toLocaleString()}</div>
                  </>
                ),
              },
              {
                key: 'p',
                header: api.t('ledger.ui.grant_progress'),
                render: (g) => (
                  <div style={{ minWidth: 140 }}>
                    <div className="row">
                      <Status tone={stateTone(g.state)}>{dt(api, g.direction, `state.${g.state}`)}</Status>
                      <span className="tiny muted num">
                        {g.granted + g.failed} / {g.total}
                      </span>
                    </div>
                    <Progress value={g.granted + g.failed} max={g.total} tone={g.failed ? 'bad' : g.state === 'done' ? 'ok' : 'info'} label={api.t('ledger.ui.grant_progress')} />
                    <div className="tiny muted">{g.notify ? api.t('ledger.ui.grant_notified', { n: g.notified }) : api.t('ledger.ui.grant_no_notify')}</div>
                  </div>
                ),
              },
              {
                key: 'a',
                header: api.t('ledger.ui.grant_amount'),
                right: true,
                render: (g) => (
                  <span className="nowrap">
                    <span className={g.direction === 'deduct' ? 'num down' : 'num up'}>{signed(g)}</span> <span className="pt-mono tiny muted">{g.currency}</span>
                  </span>
                ),
              },
              { key: 'u', header: api.t('ledger.ui.grant_by'), render: (g) => <span className="small">{g.created_by_name ?? '—'}</span> },
            ]}
          />
        )}
      </Panel>
      {next !== null && <LoadMore onClick={() => void more()} label={api.t('ledger.ui.more')} />}
      {open !== null && <GrantSheet key={open} api={api} id={open} onClose={() => setOpen(null)} onChange={() => void load()} />}
      {creating && (
        <NewGrant
          api={api}
          onClose={() => setCreating(false)}
          onCreated={(id) => {
            setCreating(false);
            void load();
            setOpen(id);
          }}
        />
      )}
    </>
  );
}

function GrantSheet({ api, id, onClose, onChange }: { api: PluginApi; id: number; onClose(): void; onChange(): void }) {
  const [g, setG] = useState<Grant | null>(null);
  const load = useCallback(() => api.json<Grant>(`${GRANTS}/${id}`).then(setG), [api, id]);
  useEffect(() => {
    void load().catch((e: unknown) => api.toast(errText(api, e), 'error'));
  }, [api, load]);
  const running = g !== null && g.state !== 'done';
  useEffect(() => {
    if (!running) return;
    const t = setInterval(() => void load().catch(() => undefined), 2000);
    return () => clearInterval(t);
  }, [running, load]);
  const resume = async () => {
    try {
      setG(await api.json<Grant>(`${GRANTS}/${id}/resume`, { method: 'POST' }));
      api.toast(api.t('ledger.ui.grant_resumed'), 'success');
      onChange();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <Sheet
      open
      onClose={onClose}
      title={g ? dt(api, g.direction, 'detail', { id }) : api.t('ledger.ui.grant_detail', { id })}
      footer={
        g &&
        g.state !== 'done' &&
        g.last_error && (
          <Button variant="primary" onClick={() => void resume()}>
            {api.t('ledger.ui.grant_resume')}
          </Button>
        )
      }
    >
      {!g ? (
        <SkeletonRows n={4} />
      ) : (
        <div className="stack">
          <div className="row">
            <Status tone={stateTone(g.state)}>{dt(api, g.direction, `state.${g.state}`)}</Status>
            <span className="small muted">{targetText(api, g.target)}</span>
          </div>
          <Progress value={g.granted + g.failed} max={g.total} tone={g.failed ? 'bad' : g.state === 'done' ? 'ok' : 'info'} label={api.t('ledger.ui.grant_progress')} />
          <Kpis
            items={[
              { label: api.t('ledger.ui.grant_amount'), value: signed(g), hint: `${g.currency} · ${api.t('ledger.ui.grant_moved', { n: fmt(g.moved, g.scale) })}`, tone: g.direction === 'deduct' ? 'down' : 'gold' },
              { label: dt(api, g.direction, 'granted'), value: `${g.granted} / ${g.total}`, hint: g.failed ? `${dt(api, g.direction, 'failed')} ${g.failed}` : undefined },
              {
                label: api.t('ledger.ui.grant_notify_col'),
                value: g.notify ? `${g.notified} / ${g.granted}` : '—',
                hint: g.notify_failed ? `${api.t('ledger.ui.grant_notify_failed')} ${g.notify_failed}` : undefined,
              },
            ]}
          />
          <p className="small">{g.memo}</p>
          {g.last_error && <Note tone="warn">{api.t('ledger.ui.grant_last_error', { e: api.t(g.last_error) })}</Note>}
          {(g.failures?.length ?? 0) > 0 && (
            <Panel title={api.t('ledger.ui.grant_failures')} flush>
              <Table
                rows={g.failures ?? []}
                rowKey={(f) => f.user_id}
                columns={[
                  { key: 'u', header: api.t('ledger.ui.grant_by_users'), render: (f) => <span className="small">{f.username ?? `#${f.user_id}`}</span> },
                  {
                    key: 'e',
                    header: api.t('ledger.ui.grant_error'),
                    render: (f) => (
                      <span className="small">
                        {[
                          f.error && api.t(f.error),
                          !f.error && f.amount !== null && f.amount !== g.amount && api.t('ledger.ui.grant_partial', { n: fmt(f.amount, g.scale) }),
                          f.notify_error && `${api.t('ledger.ui.grant_notify_failed')}: ${f.notify_error}`,
                        ]
                          .filter(Boolean)
                          .join(' · ')}
                      </span>
                    ),
                  },
                ]}
              />
            </Panel>
          )}
        </div>
      )}
    </Sheet>
  );
}

/** 新建：先填表并计算人数（preview），在同一抽屉里核对后确认 */
function NewGrant({ api, onClose, onCreated }: { api: PluginApi; onClose(): void; onCreated(id: number): void }) {
  const [curs, setCurs] = useState<Currency[]>([]);
  const [classes, setClasses] = useState<LevelClass[] | null>(null);
  const [f, setF] = useState({ direction: 'grant' as Direction, shortfall: 'skip' as 'skip' | 'partial', currency: 'bonus', amount: '', memo: '', mode: 'filter' as 'filter' | 'users', statuses: ['active'], class_ids: [] as number[], usernames: '', notify: true, subject: '', body: '' });
  const [pv, setPv] = useState<Preview | null>(null);
  const [busy, setBusy] = useState(false);
  const [key] = useState(() => `grant-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`);
  useEffect(() => {
    void api.json<{ items: Currency[] }>('/api/p/ledger/v1/currencies').then((r) => setCurs(r.items.filter((c) => !c.disabled)));
    api
      .json<{ items: LevelClass[] }>('/api/p/levels/v1/classes')
      .then((r) => setClasses([...r.items].sort((a, b) => a.rank - b.rank)))
      .catch(() => setClasses([]));
  }, [api]);
  const cur = curs.find((c) => c.code === f.currency);
  const scale = cur?.scale ?? 0;
  const units = Math.round(Number(f.amount) * 10 ** scale);
  const names = f.usernames
    .split(/[\s,，]+/)
    .map((x) => x.trim())
    .filter(Boolean);
  const target: Target = f.mode === 'users' ? { kind: 'users', usernames: names } : { kind: 'filter', statuses: f.statuses, class_ids: f.class_ids };
  const valid = units > 0 && f.memo.trim() !== '' && (f.mode === 'users' ? names.length > 0 : f.statuses.length > 0);
  const toggle = <T,>(list: T[], v: T, on: boolean) => (on ? [...list.filter((x) => x !== v), v] : list.filter((x) => x !== v));
  const set = (patch: Partial<typeof f>) => {
    // 换方向时清空模板，让预览带回对应方向的默认模板
    setF({ ...f, ...patch, ...(patch.direction && patch.direction !== f.direction ? { subject: '', body: '' } : {}) });
    setPv(null);
  };
  const preview = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      const r = await api.json<Preview>(`${GRANTS}/preview`, { method: 'POST', json: { currency: f.currency, amount: units, target, direction: f.direction, shortfall: f.shortfall } });
      setPv(r);
      if (!f.subject && !f.body) setF((x) => ({ ...x, subject: r.default_subject, body: r.default_body }));
    } catch (err) {
      api.toast(errText(api, err), 'error');
    } finally {
      setBusy(false);
    }
  };
  const create = async () => {
    if (!pv) return;
    setBusy(true);
    try {
      const notify = f.notify && pv.notify_available ? { subject: f.subject, body: f.body } : null;
      const r = await api.json<Grant>(GRANTS, { method: 'POST', json: { currency: f.currency, amount: units, memo: f.memo.trim(), target, notify, direction: f.direction, shortfall: f.shortfall }, idempotent: key });
      api.toast(dt(api, f.direction, 'started'), 'success');
      onCreated(r.id);
    } catch (err) {
      api.toast(errText(api, err), 'error');
    } finally {
      setBusy(false);
    }
  };
  const blocked = pv !== null && (pv.count === 0 || pv.count > pv.max_recipients || pv.total_amount === null);
  return (
    <Sheet
      open
      onClose={onClose}
      width={560}
      title={api.t('ledger.ui.grant_new')}
      footer={
        <>
          <Button variant="ghost" onClick={pv ? () => setPv(null) : onClose}>
            {pv ? api.t('ledger.ui.grant_back') : api.t('ledger.ui.cancel')}
          </Button>
          {pv ? (
            <Button variant="danger" disabled={busy || blocked} onClick={() => void create()}>
              {dt(api, f.direction, 'confirm')}
            </Button>
          ) : (
            <Button type="submit" form="ledger-grant" variant="primary" disabled={busy || !valid}>
              {api.t('ledger.ui.grant_next')}
            </Button>
          )}
        </>
      }
    >
      {pv ? (
        <div className="stack">
          <Kpis
            items={[
              { label: api.t('ledger.ui.grant_count'), value: pv.count.toLocaleString() },
              { label: api.t('ledger.ui.grant_amount'), value: `${f.direction === 'deduct' ? '−' : '+'}${f.amount}`, hint: cur ? curName(api, cur) : f.currency },
              {
                label: api.t(f.direction === 'deduct' ? 'ledger.ui.grant_total_est' : 'ledger.ui.grant_total'),
                value: pv.total_amount === null ? '—' : fmt(pv.total_amount, scale),
                tone: f.direction === 'deduct' ? 'down' : 'gold',
              },
            ]}
          />
          <p className="small">
            {api.t(`ledger.ui.dir.${f.direction}`)} · {targetText(api, target)} · {f.memo}
          </p>
          {f.direction === 'deduct' && pv.insufficient > 0 && <Note tone="warn">{api.t(`ledger.ui.grant_insufficient_${f.shortfall}`, { n: pv.insufficient })}</Note>}
          {pv.count === 0 && <Note tone="bad">{api.t('ledger.ui.grant_none')}</Note>}
          {pv.count > pv.max_recipients && <Note tone="bad">{api.t('ledger.ui.grant_too_many', { max: pv.max_recipients })}</Note>}
          {pv.missing.length > 0 && <Note tone="warn">{api.t('ledger.ui.grant_missing', { names: pv.missing.join(', ') })}</Note>}
          {pv.notify_available ? (
            <div className="form">
              <Checkbox name="grant_notify" label={api.t('ledger.ui.grant_notify')} checked={f.notify} onChange={(v) => setF({ ...f, notify: v })} />
              {f.notify && (
                <>
                  <Field label={api.t('ledger.ui.grant_subject')}>
                    <Input name="grant_subject" maxLength={120} value={f.subject} onChange={(e) => setF({ ...f, subject: e.target.value })} />
                  </Field>
                  <Field label={api.t('ledger.ui.grant_body')} hint={api.t('ledger.ui.grant_vars')}>
                    <Textarea name="grant_body" rows={6} maxLength={5000} value={f.body} onChange={(e) => setF({ ...f, body: e.target.value })} />
                  </Field>
                </>
              )}
            </div>
          ) : (
            <Note>{api.t('ledger.ui.grant_notify_off')}</Note>
          )}
        </div>
      ) : (
        <form id="ledger-grant" className="form" onSubmit={(e) => void preview(e)}>
          <p className="small muted">{api.t('ledger.ui.grant_lede')}</p>
          <div className="fl">
            <span className="fl-label">{api.t('ledger.ui.grant_direction')}</span>
            <Segmented
              label={api.t('ledger.ui.grant_direction')}
              value={f.direction}
              onChange={(v) => set({ direction: v })}
              options={[
                { value: 'grant', label: api.t('ledger.ui.dir.grant') },
                { value: 'deduct', label: api.t('ledger.ui.dir.deduct') },
              ]}
            />
          </div>
          <div className="form-2">
            <Field label={api.t('ledger.ui.currency')}>
              <Select name="grant_currency" value={f.currency} onChange={(e) => set({ currency: e.target.value })} options={curs.map((c) => ({ value: c.code, label: curName(api, c) }))} />
            </Field>
            <Field label={api.t('ledger.ui.grant_amount')} hint={f.direction === 'deduct' ? api.t('ledger.ui.grant_amount_hint') : undefined}>
              <Input name="grant_amount" type="number" min={0} step="any" required value={f.amount} onChange={(e) => set({ amount: e.target.value })} />
            </Field>
          </div>
          {f.direction === 'deduct' && (
            <div className="fl">
              <span className="fl-label">{api.t('ledger.ui.grant_shortfall')}</span>
              <Segmented
                label={api.t('ledger.ui.grant_shortfall')}
                value={f.shortfall}
                onChange={(v) => set({ shortfall: v })}
                options={[
                  { value: 'skip', label: api.t('ledger.ui.shortfall.skip') },
                  { value: 'partial', label: api.t('ledger.ui.shortfall.partial') },
                ]}
              />
              <span className="hint">{api.t('ledger.ui.grant_shortfall_hint')}</span>
            </div>
          )}
          <div className="fl">
            <span className="fl-label">{api.t('ledger.ui.grant_target')}</span>
            <Segmented
              label={api.t('ledger.ui.grant_target')}
              value={f.mode}
              onChange={(v) => set({ mode: v })}
              options={[
                { value: 'filter', label: api.t('ledger.ui.grant_by_filter') },
                { value: 'users', label: api.t('ledger.ui.grant_by_users') },
              ]}
            />
          </div>
          {f.mode === 'filter' ? (
            <>
              <div className="fl">
                <span className="fl-label">{api.t('ledger.ui.grant_statuses')}</span>
                <div className="pt-checks">
                  {STATUSES.map((st) => (
                    <Checkbox key={st} name={`grant_status_${st}`} label={api.t(`ledger.ui.status.${st}`)} checked={f.statuses.includes(st)} onChange={(on) => set({ statuses: toggle(f.statuses, st, on) })} />
                  ))}
                </div>
                <span className="hint">{api.t('ledger.ui.grant_statuses_hint')}</span>
              </div>
              {classes === null ? (
                <SkeletonRows n={1} />
              ) : (
                classes.length > 0 && (
                  <div className="fl">
                    <span className="fl-label">{api.t('ledger.ui.grant_classes')}</span>
                    <div className="pt-checks">
                      {classes.map((c) => (
                        <Checkbox key={c.id} name={`grant_class_${c.id}`} label={levelName(api, c)} checked={f.class_ids.includes(c.id)} onChange={(on) => set({ class_ids: toggle(f.class_ids, c.id, on) })} />
                      ))}
                    </div>
                    <span className="hint">{api.t('ledger.ui.grant_classes_hint')}</span>
                  </div>
                )
              )}
            </>
          ) : (
            <Field label={api.t('ledger.ui.grant_usernames')} hint={api.t('ledger.ui.grant_usernames_hint')}>
              <Textarea name="grant_usernames" rows={6} value={f.usernames} onChange={(e) => set({ usernames: e.target.value })} />
            </Field>
          )}
          <Field label={api.t('ledger.ui.grant_memo')}>
            <Input name="grant_memo" required maxLength={200} value={f.memo} onChange={(e) => set({ memo: e.target.value })} />
          </Field>
        </form>
      )}
    </Sheet>
  );
}

export default {
  slots: {
    'admin.user.tabs': { component: UserTab, order: 11, title_term: 'ledger.ui.tab' },
  },
  routes: [
    { path: '/p/ledger/*', component: Transfer },
    { path: '/admin/p/ledger/*', component: Admin },
  ],
} satisfies PluginFrontend;
