// bonus 前端：个人页显示魔力值余额（仅本人）；魔力值页（/p/bonus/：各币种余额；「获取与使用」说明各种魔力值的来源与用途，
// 其他插件经 bonus.explain 插槽补充自己的规则；「明细」分页查看收支流水）；用户菜单入口。
import { useEffect, useState } from 'react';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Slot } from '@pt-studio/slots';
import { Kpis, NavItem, PageHead, Pager, Panel, PLink, RuleItem, SkeletonRows, Stat, Status, Table, Tabs } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'bonus.ui.menu': '魔力值',
  'bonus.ui.history': '魔力值流水',
  'bonus.ui.title': '魔力值',
  'bonus.ui.lede': '魔力值靠做种、签到、发布资源等方式获得，可以在商城兑换、抽奖、悬赏求种或转给其他用户。',
  'bonus.ui.tab_rules': '获取与使用',
  'bonus.ui.tab_history': '魔力明细',
  'bonus.ui.earn': '如何获得',
  'bonus.ui.spend': '可以用来做什么',
  'bonus.ui.last_hour': '上一小时做种',
  'bonus.ui.last_hour_hint': '{n} 个种子 · {t}',
  'bonus.ui.last_hour_none': '最近没有做种结算',
  'bonus.ui.last_hour_sp': '做种积分 +{n}',
  'bonus.ui.sp_title': '魔力值和做种积分有什么不同',
  'bonus.ui.sp_bonus': '用来花的：商城兑换、抽奖、悬赏、转账都扣它。做种之外，签到、发布、捐赠等也能获得；VIP 的做种收益有加成，官种另有奖励。',
  'bonus.ui.sp_points': '记录你的做种贡献：只随做种增长，不能消费、不能转账，也不会因为花魔力而减少。每小时和做种魔力一起结算，数值等于做种魔力的基础部分（不含 VIP 加成和官种奖励），所以同样做种的人做种积分相同。',
  'bonus.ui.sp_current': '当前 {n}',
  'bonus.ui.sp_kpi': '做种贡献，不能消费',
  'bonus.ui.per_times': '+{n} / 次',
  'bonus.ui.daily_limit': '每天最多计 {n} 次。',
  'bonus.ui.transfer_rule': '把魔力值转给其他用户，用于感谢、互助等。',
  'bonus.ui.transfer_go': '去转账',
  'bonus.ui.total': '共 {n} 条',
  'bonus.trigger.torrent_approved': '发布资源',
  'bonus.trigger_hint.torrent_approved': '发布的种子通过审核后奖励发布者。',
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
  'bonus.ui.title': 'Bonus',
  'bonus.ui.last_hour_sp': 'seeding points +{n}',
  'bonus.ui.sp_title': 'Bonus vs. seeding points',
  'bonus.ui.sp_bonus': 'The spendable one: shop, prize wheel, bounties and transfers all use it. Besides seeding you also earn it from check-ins, uploads and donations; VIPs get a seeding multiplier and official releases pay extra.',
  'bonus.ui.sp_points': 'A record of your seeding contribution: it only grows with seeding and cannot be spent or transferred. Settled hourly together with seeding bonus, it equals the base part of seeding bonus (without the VIP multiplier or official-release reward), so equal seeding earns equal points.',
  'bonus.ui.sp_current': 'now {n}',
  'bonus.ui.sp_kpi': 'seeding record, not spendable',
  'bonus.ui.lede': 'Earn bonus by seeding, checking in and uploading; spend it in the shop, on the prize wheel, on request bounties or send it to other users.',
  'bonus.ui.tab_rules': 'Earning & spending',
  'bonus.ui.tab_history': 'History',
  'bonus.ui.earn': 'How to earn',
  'bonus.ui.spend': 'What you can spend it on',
  'bonus.ui.last_hour': 'Last seeding hour',
  'bonus.ui.last_hour_hint': '{n} torrents · {t}',
  'bonus.ui.last_hour_none': 'No recent seeding settlement',
  'bonus.ui.per_times': '+{n} each',
  'bonus.ui.daily_limit': 'Counted at most {n} times a day.',
  'bonus.ui.transfer_rule': 'Send bonus to another user as thanks or help.',
  'bonus.ui.transfer_go': 'Transfer',
  'bonus.ui.total': '{n} entries',
  'bonus.trigger.torrent_approved': 'Uploading',
  'bonus.trigger_hint.torrent_approved': 'Awarded to the uploader once the torrent is approved.',
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

interface Explain {
  currencies: { currency: string; scale: number; name_term: string }[];
  actions: { code: string; trigger: string; currency: string; amount: number; daily_count_limit: number }[];
  last_hour: { period_start: number; torrent_count: number; amounts: Record<string, number> | null; skipped_reason: string | null } | null;
}

const PAGE_SIZE = 20;

/** 页签与页码放在查询串里（?tab=history&page=2），刷新与分享链接时保持 */
function readQuery(): { tab: string; page: number } {
  const q = new URLSearchParams(window.location.search);
  return { tab: q.get('tab') === 'history' ? 'history' : 'rules', page: Math.max(1, Number(q.get('page')) || 1) };
}

function writeQuery(tab: string, page: number) {
  const q = new URLSearchParams(window.location.search);
  if (tab === 'history') q.set('tab', tab);
  else q.delete('tab');
  if (tab === 'history' && page > 1) q.set('page', String(page));
  else q.delete('page');
  const qs = q.toString();
  window.history.replaceState(window.history.state, '', `${window.location.pathname}${qs ? `?${qs}` : ''}`);
}

function Rules({ api, ex, s }: { api: PluginApi; ex: Explain | null; s: Summary | null }) {
  const user = api.user();
  const scaleOf = (c: string) => ex?.currencies.find((x) => x.currency === c)?.scale ?? 0;
  const nameOf = (c: string) => {
    const t = ex?.currencies.find((x) => x.currency === c)?.name_term;
    return t ? api.t(`term:${t}`) : c;
  };
  return (
    <>
      <Panel title={api.t('bonus.ui.earn')} flush primary>
        <div className="rule-list">
          <Slot name="bonus.explain" ctx={{ user, section: 'earn' }} className="rule-list" />
          {ex?.actions.map((a) => (
            <RuleItem key={a.code} title={api.t(`bonus.trigger.${a.trigger}`)} value={api.t('bonus.ui.per_times', { n: `${fmt(a.amount, scaleOf(a.currency))}${a.currency === 'bonus' ? '' : ` ${nameOf(a.currency)}`}` })}>
              {api.t(`bonus.trigger_hint.${a.trigger}`)}
              {a.daily_count_limit > 0 && ` ${api.t('bonus.ui.daily_limit', { n: a.daily_count_limit })}`}
            </RuleItem>
          ))}
        </div>
      </Panel>
      <Panel title={api.t('bonus.ui.spend')} flush>
        <div className="rule-list">
          <Slot name="bonus.explain" ctx={{ user, section: 'spend' }} className="rule-list" />
          <RuleItem title={api.t('bonus.ui.transfer')} to="/p/ledger/transfer" navigate={api.navigate} link={api.t('bonus.ui.transfer_go')}>
            {api.t('bonus.ui.transfer_rule')}
          </RuleItem>
        </div>
      </Panel>
      {ex?.currencies.some((c) => c.currency === 'seed_points') && (
        <Panel title={api.t('bonus.ui.sp_title')} flush>
          <div className="rule-list">
            {(['bonus', 'seed_points'] as const).map((c) => {
              const b = s?.balances.find((x) => x.currency === c);
              return (
                <RuleItem key={c} title={nameOf(c)} value={b ? api.t('bonus.ui.sp_current', { n: fmt(b.balance, b.scale) }) : undefined}>
                  {api.t(c === 'bonus' ? 'bonus.ui.sp_bonus' : 'bonus.ui.sp_points')}
                </RuleItem>
              );
            })}
          </div>
        </Panel>
      )}
    </>
  );
}

function History({ api, s }: { api: PluginApi; s: Summary | null }) {
  const [cur, setCur] = useState('');
  const [page, setPage] = useState(() => readQuery().page);
  const [data, setData] = useState<{ items: Entry[]; total: number } | null>(null);
  useEffect(() => {
    let live = true;
    const q = new URLSearchParams({ page: String(page), limit: String(PAGE_SIZE) });
    if (cur) q.set('currency', cur);
    setData(null);
    api
      .json<{ items: Entry[]; total: number }>(`/api/p/ledger/v1/users/me/entries?${q.toString()}`)
      .then((r) => live && setData(r))
      .catch(() => live && setData({ items: [], total: 0 }));
    writeQuery('history', page);
    return () => {
      live = false;
    };
  }, [api, cur, page]);
  const scaleOf = (c: string) => s?.balances.find((b) => b.currency === c)?.scale ?? 0;
  const pages = data ? Math.max(1, Math.ceil(data.total / PAGE_SIZE)) : 1;
  return (
    <Panel flush primary>
      {s && s.balances.length > 1 && (
        <Tabs
          variant="flush"
          value={cur}
          onChange={(c) => {
            setCur(c);
            setPage(1);
          }}
          tabs={[{ id: '', label: api.t('bonus.ui.all') }, ...s.balances.map((b) => ({ id: b.currency, label: api.t(`term:${b.name_term}`) }))]}
        />
      )}
      {data === null ? (
        <SkeletonRows n={8} />
      ) : (
        <Table
          rows={data.items}
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
      {data && data.total > 0 && (
        <div className="pager-row">
          <span className="small muted">{api.t('bonus.ui.total', { n: data.total.toLocaleString() })}</span>
          <Pager page={page} pages={pages} onChange={setPage} />
        </div>
      )}
    </Panel>
  );
}

function BonusPage({ api }: { api: PluginApi }) {
  const [s, setS] = useState<Summary | null>(null);
  const [ex, setEx] = useState<Explain | null>(null);
  const [tab, setTab] = useState(() => readQuery().tab);
  useEffect(() => {
    void api.json<Summary>('/api/p/bonus/v1/users/me/summary').then(setS).catch(() => undefined);
    void api.json<Explain>('/api/p/bonus/v1/explain').then(setEx).catch(() => undefined);
  }, [api]);
  const last = ex?.last_hour;
  const lastAmount = last?.amounts?.bonus;
  const lastPoints = last?.amounts?.seed_points;
  const scaleOf = (c: string) => s?.balances.find((b) => b.currency === c)?.scale ?? 0;
  return (
    <div className="stack">
      <PageHead
        title={api.t('bonus.ui.title')}
        lede={api.t('bonus.ui.lede')}
        actions={
          <PLink to="/p/ledger/transfer" navigate={api.navigate} className="btn">
            {api.t('bonus.ui.transfer')}
          </PLink>
        }
      />
      {s && (
        <Kpis
          items={[
            ...s.balances.map((b) => ({ label: api.t(`term:${b.name_term}`), value: fmt(b.balance, b.scale), hint: b.currency === 'seed_points' ? api.t('bonus.ui.sp_kpi') : api.t('bonus.ui.available', { n: fmt(b.available, b.scale) }), tone: 'gold' as const })),
            {
              label: api.t('bonus.ui.last_hour'),
              value: lastAmount !== undefined ? `+${fmt(lastAmount, scaleOf('bonus'))}` : '—',
              hint: last
                ? [
                    api.t('bonus.ui.last_hour_hint', { n: last.torrent_count, t: new Date(last.period_start).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) }),
                    lastPoints !== undefined ? api.t('bonus.ui.last_hour_sp', { n: fmt(lastPoints, scaleOf('seed_points')) }) : '',
                  ]
                    .filter(Boolean)
                    .join(' · ')
                : api.t('bonus.ui.last_hour_none'),
            },
          ]}
        />
      )}
      <Tabs
        variant="sub"
        value={tab}
        onChange={(t) => {
          setTab(t);
          writeQuery(t, 1);
        }}
        tabs={[
          { id: 'rules', label: api.t('bonus.ui.tab_rules') },
          { id: 'history', label: api.t('bonus.ui.tab_history') },
        ]}
      />
      {tab === 'history' ? <History api={api} s={s} /> : <Rules api={api} ex={ex} s={s} />}
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
  routes: [{ path: '/p/bonus/*', component: BonusPage }],
} satisfies PluginFrontend;
