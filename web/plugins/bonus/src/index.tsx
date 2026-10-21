// bonus 前端：个人页显示魔力值余额（仅本人）；魔力值流水页（/p/bonus/：各货币余额、收支明细、按货币筛选、加载更多）；用户菜单入口。
import { useCallback, useEffect, useState } from 'react';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Kpis, LoadMore, NavItem, PageHead, Panel, PLink, SkeletonRows, Stat, Status, Table, Tabs } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'bonus.ui.menu': '魔力值',
  'bonus.ui.history': '魔力值流水',
  'bonus.ui.transfer': '转账',
  'bonus.ui.available': '可用 {n}',
  'bonus.ui.all': '全部',
  'bonus.ui.more': '加载更多',
  'bonus.ui.empty': '暂无记录',
  'bonus.ui.reversed': '已冲正',
  'bonus.ui.balance_after': '余额 {n}',
  'bonus.ui.time': '时间',
  'bonus.ui.event': '事项',
  'bonus.ui.amount': '收支',
  'bonus.ui.after': '余额',
  'bonus.reason.bonus.seeding': '做种奖励',
  'bonus.reason.bonus.award': '奖励',
  'bonus.reason.checkin.daily': '每日签到',
  'bonus.reason.checkin.milestone': '连续签到奖励',
  'bonus.reason.shop.purchase': '商城购买',
  'bonus.reason.lottery.spin': '幸运转盘抽奖',
  'bonus.reason.lottery.prize': '幸运转盘中奖',
  'bonus.reason.lottery.refund': '幸运转盘退款',
  'bonus.reason.ledger.admin_adjust': '管理员调整',
  'bonus.reason.ledger.transfer': '用户转账',
  'bonus.reason.ledger.user_gift': '用户转账',
  'bonus.reason.ledger.admin_reversal': '管理员冲正',
  'bonus.reason.donations.reward': '捐赠奖励',
});
registerMessages('en', {
  'bonus.ui.menu': 'Bonus',
  'bonus.ui.history': 'Bonus history',
  'bonus.ui.transfer': 'Transfer',
  'bonus.ui.available': '{n} available',
  'bonus.ui.all': 'All',
  'bonus.ui.more': 'Load more',
  'bonus.ui.empty': 'No entries',
  'bonus.ui.reversed': 'reversed',
  'bonus.ui.balance_after': 'balance {n}',
  'bonus.ui.time': 'Time',
  'bonus.ui.event': 'Event',
  'bonus.ui.amount': 'Amount',
  'bonus.ui.after': 'Balance',
  'bonus.reason.bonus.seeding': 'Seeding reward',
  'bonus.reason.bonus.award': 'Reward',
  'bonus.reason.checkin.daily': 'Daily check-in',
  'bonus.reason.checkin.milestone': 'Check-in streak bonus',
  'bonus.reason.shop.purchase': 'Shop purchase',
  'bonus.reason.lottery.spin': 'Prize wheel spin',
  'bonus.reason.lottery.prize': 'Prize wheel win',
  'bonus.reason.lottery.refund': 'Prize wheel refund',
  'bonus.reason.ledger.admin_adjust': 'Staff adjustment',
  'bonus.reason.ledger.transfer': 'Transfer',
  'bonus.reason.ledger.user_gift': 'Transfer',
  'bonus.reason.ledger.admin_reversal': 'Staff reversal',
  'bonus.reason.donations.reward': 'Donation reward',
});

interface Balance {
  currency: string;
  scale: number;
  name_term: string;
  balance: number | string;
  available: number | string;
}
interface Summary {
  balances: Balance[];
}

function fmt(v: number | string, scale: number): string {
  const n = Number(v) / 10 ** scale;
  return n.toLocaleString(undefined, { maximumFractionDigits: Math.min(scale, 2) });
}

function BonusStat({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const [s, setS] = useState<Summary | null>(null);
  const self = api.user()?.id === ctx.profile.id;
  useEffect(() => {
    if (self) void api.json<Summary>('/api/p/bonus/v1/users/me/summary').then(setS).catch(() => undefined);
  }, [api, self]);
  if (!s) return null;
  return (
    <>
      {s.balances.map((b) => (
        <Stat
          key={b.currency}
          label={api.t(`term:${b.name_term}`)}
          value={fmt(b.balance, b.scale)}
          tone="gold"
          to="/p/bonus/"
          navigate={api.navigate}
        />
      ))}
    </>
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
  reversed: boolean;
  created_at: number;
}

/** 理由显示：已知的给中文名，其余显示原始键 */
function reasonText(api: PluginApi, r: string): string {
  const k = `bonus.reason.${r}`;
  const t = api.t(k);
  return t === k ? r : t;
}

function History({ api }: { api: PluginApi }) {
  const [s, setS] = useState<Summary | null>(null);
  const [cur, setCur] = useState('');
  const [rows, setRows] = useState<Entry[] | null>(null);
  const [next, setNext] = useState<string | null>(null);
  const load = useCallback(
    async (cursor: string | null) => {
      const q = new URLSearchParams({ limit: '50' });
      if (cur) q.set('currency', cur);
      if (cursor) q.set('cursor', cursor);
      const r = await api.json<{ items: Entry[]; next_cursor: string | null }>(`/api/p/ledger/v1/users/me/entries?${q.toString()}`);
      setRows((old) => (cursor ? [...(old ?? []), ...r.items] : r.items));
      setNext(r.next_cursor);
    },
    [api, cur],
  );
  useEffect(() => {
    void api.json<Summary>('/api/p/bonus/v1/users/me/summary').then(setS).catch(() => undefined);
  }, [api]);
  useEffect(() => {
    void load(null).catch(() => setRows([]));
  }, [load]);
  const scaleOf = (c: string) => s?.balances.find((b) => b.currency === c)?.scale ?? 0;
  return (
    <div className="stack">
      <PageHead
        title={api.t('bonus.ui.history')}
        actions={
          <PLink to="/p/ledger/transfer" navigate={api.navigate} className="btn">
            {api.t('bonus.ui.transfer')}
          </PLink>
        }
      />
      {s && (
        <Kpis
          items={s.balances.map((b) => ({ label: api.t(`term:${b.name_term}`), value: fmt(b.balance, b.scale), hint: api.t('bonus.ui.available', { n: fmt(b.available, b.scale) }), tone: 'gold' as const }))}
        />
      )}
      <Panel flush primary>
        {s && s.balances.length > 1 && (
          <Tabs
            variant="flush"
            value={cur}
            onChange={setCur}
            tabs={[{ id: '', label: api.t('bonus.ui.all') }, ...s.balances.map((b) => ({ id: b.currency, label: api.t(`term:${b.name_term}`) }))]}
          />
        )}
        {rows === null ? (
          <SkeletonRows n={6} />
        ) : (
          <Table
            rows={rows}
            rowKey={(r) => `${r.entry_id}:${r.currency}`}
            empty={api.t('bonus.ui.empty')}
            columns={[
              { key: 't', header: api.t('bonus.ui.time'), render: (r) => <span className="small muted nowrap num">{new Date(r.created_at).toLocaleString()}</span> },
              {
                key: 'r',
                header: api.t('bonus.ui.event'),
                render: (r) => (
                  <>
                    {reasonText(api, r.reason)} {r.reversed && <Status tone="mute">{api.t('bonus.ui.reversed')}</Status>}
                    {r.memo && <div className="tiny muted">{r.memo}</div>}
                  </>
                ),
              },
              {
                key: 'a',
                header: api.t('bonus.ui.amount'),
                right: true,
                render: (r) => (
                  <span className={r.amount >= 0 ? 'num up' : 'num down'}>
                    {r.amount >= 0 ? '+' : ''}
                    {fmt(r.amount, scaleOf(r.currency))}
                  </span>
                ),
              },
              { key: 'b', header: api.t('bonus.ui.after'), right: true, render: (r) => <span className="num muted">{fmt(r.balance_after, scaleOf(r.currency))}</span> },
            ]}
          />
        )}
        {next && <LoadMore onClick={() => void load(next)} label={api.t('bonus.ui.more')} />}
      </Panel>
    </div>
  );
}

function MenuItem({ api }: SlotProps) {
  return (
    <NavItem to="/p/bonus/" navigate={api.navigate}>
      {api.t('bonus.ui.menu')}
    </NavItem>
  );
}

export default {
  slots: {
    'user.profile.stats': { component: BonusStat, order: 20 },
    'nav.user_menu': { component: MenuItem, order: 20 },
  },
  routes: [{ path: '/p/bonus/*', component: History }],
} satisfies PluginFrontend;
