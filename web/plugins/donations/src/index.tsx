// donations 前端：捐赠页（/p/donations/：说明、本月进度、捐赠名单、我的捐赠与 VIP 到期时间）、用户菜单入口（开放时显示）、
// 后台登记与记录（/admin/p/donations/）。
import { useEffect, useState, type FormEvent, type ReactNode } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Checkbox, Field, Input, Kpis, NavItem, Note, PageHead, Panel, Progress, Sheet, SideLayout, SkeletonRows, Status, Table, Tag, UserContent } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'donations.ui.menu': '捐赠',
  'donations.ui.title': '支持本站',
  'donations.ui.closed': '捐赠暂未开放',
  'donations.ui.month': '本月已收到 {got} / 目标 {goal}',
  'donations.ui.donors': '感谢以下捐赠者',
  'donations.ui.mine': '我的捐赠',
  'donations.ui.vip_until': '你的 VIP 有效期至 {t}',
  'donations.ui.amount': '金额',
  'donations.ui.rewards': '奖励',
  'donations.ui.vip_days': 'VIP {n} 天',
  'donations.ui.bonus': '魔力值 {n}',
  'donations.ui.revoked': '已撤销',
  'donations.ui.admin': '捐赠登记',
  'donations.ui.record': '登记一笔捐赠',
  'donations.ui.username': '用户名',
  'donations.ui.amount_yuan': '金额（元）',
  'donations.ui.method': '付款方式',
  'donations.ui.note': '备注（仅管理组可见）',
  'donations.ui.vip_days_field': 'VIP 天数（留空按设置计算）',
  'donations.ui.bonus_field': '魔力值（留空按设置计算）',
  'donations.ui.medal_field': '勋章 ID（可选）',
  'donations.ui.public': '用户同意出现在捐赠名单',
  'donations.ui.submit': '登记并发放奖励',
  'donations.ui.recorded': '已登记，奖励已发放',
  'donations.ui.partial': '已登记，但部分奖励发放失败：{list}',
  'donations.ui.revoke': '撤销',
  'donations.ui.revoke_confirm': '撤销只把这笔捐赠排除出统计，已发放的奖励不会收回。继续？',
  'donations.ui.totals': '本月 {m} · 累计 {t}',
  'donations.user_not_found': '找不到该用户',
  'donations.ui.goal': '本月目标',
  'donations.ui.date': '日期',
  'donations.ui.this_month': '本月',
  'donations.ui.total': '累计',
  'donations.ui.count': '笔数',
  'donations.ui.by': '登记人',
  'donations.ui.cancel': '取消',
});
registerMessages('en', {
  'donations.ui.menu': 'Donate',
  'donations.ui.title': 'Support the site',
  'donations.ui.closed': 'Donations are not open',
  'donations.ui.month': '{got} of {goal} received this month',
  'donations.ui.donors': 'Thanks to our donors',
  'donations.ui.mine': 'My donations',
  'donations.ui.vip_until': 'Your VIP is valid until {t}',
  'donations.ui.amount': 'Amount',
  'donations.ui.rewards': 'Rewards',
  'donations.ui.vip_days': 'VIP {n} days',
  'donations.ui.bonus': '{n} bonus',
  'donations.ui.revoked': 'revoked',
  'donations.ui.admin': 'Donations',
  'donations.ui.record': 'Record a donation',
  'donations.ui.username': 'Username',
  'donations.ui.amount_yuan': 'Amount',
  'donations.ui.method': 'Payment method',
  'donations.ui.note': 'Note (staff only)',
  'donations.ui.vip_days_field': 'VIP days (blank = from settings)',
  'donations.ui.bonus_field': 'Bonus (blank = from settings)',
  'donations.ui.medal_field': 'Medal ID (optional)',
  'donations.ui.public': 'User agrees to be listed as a donor',
  'donations.ui.submit': 'Record and reward',
  'donations.ui.recorded': 'Recorded and rewarded',
  'donations.ui.partial': 'Recorded, but some rewards failed: {list}',
  'donations.ui.revoke': 'Revoke',
  'donations.ui.revoke_confirm': 'Revoking only excludes this donation from totals; rewards are not taken back. Continue?',
  'donations.ui.totals': 'This month {m} · total {t}',
  'donations.user_not_found': 'User not found',
  'donations.ui.goal': 'Monthly goal',
  'donations.ui.date': 'Date',
  'donations.ui.this_month': 'This month',
  'donations.ui.total': 'Total',
  'donations.ui.count': 'Records',
  'donations.ui.by': 'By',
  'donations.ui.cancel': 'Cancel',
});

interface Mine {
  id: number;
  amount: number;
  vip_days: number;
  bonus: number;
  revoked: boolean;
  created_at: number;
}
interface Summary {
  enabled: boolean;
  instructions_html: string;
  currency_symbol: string;
  monthly_goal: number;
  month_total: number;
  donors: string[];
  mine: Mine[];
  vip_until: number | null;
}

const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e);
const money = (sym: string, cents: number) => `${sym}${(cents / 100).toLocaleString(undefined, { maximumFractionDigits: 2 })}`;

function rewards(api: PluginApi, r: { vip_days: number; bonus: number }): ReactNode {
  const parts = [];
  if (r.vip_days > 0) parts.push(api.t('donations.ui.vip_days', { n: r.vip_days }));
  if (r.bonus > 0) parts.push(api.t('donations.ui.bonus', { n: r.bonus }));
  return parts.join(' · ') || '—';
}

function Page({ api }: { api: PluginApi }) {
  const [s, setS] = useState<Summary | null>(null);
  useEffect(() => {
    void api
      .json<Summary>('/api/p/donations/v1/summary')
      .then(setS)
      .catch((e: unknown) => api.toast(errText(api, e), 'error'));
  }, [api]);
  if (!s) return <SkeletonRows n={6} />;
  return (
    <div className="stack">
      <PageHead title={api.t('donations.ui.title')} />
      {!s.enabled && <Note tone="warn">{api.t('donations.ui.closed')}</Note>}
      <SideLayout
        width={340}
        side={
          <>
            {s.enabled && s.monthly_goal > 0 && (
              <Panel title={api.t('donations.ui.goal')} primary>
                <div className="stack-s" data-testid="donation-goal">
                  <span className="big-n num">{Math.min(100, Math.round((s.month_total * 100) / s.monthly_goal))}%</span>
                  <Progress value={s.month_total} max={s.monthly_goal} label={api.t('donations.ui.goal')} tone={s.month_total >= s.monthly_goal ? 'ok' : undefined} />
                  <span className="small muted">{api.t('donations.ui.month', { got: money(s.currency_symbol, s.month_total), goal: money(s.currency_symbol, s.monthly_goal) })}</span>
                </div>
              </Panel>
            )}
            {s.vip_until && <Note tone="ok">{api.t('donations.ui.vip_until', { t: new Date(s.vip_until).toLocaleString() })}</Note>}
            {s.donors.length > 0 && (
              <Panel title={api.t('donations.ui.donors')}>
                <div className="row" data-testid="donor-list">
                  {s.donors.map((d) => (
                    <Tag key={d} tone="o">
                      {d}
                    </Tag>
                  ))}
                </div>
              </Panel>
            )}
          </>
        }
      >
        {s.enabled && (
          <Panel>
            <UserContent html={s.instructions_html} />
          </Panel>
        )}
        {s.mine.length > 0 && (
          <Panel title={api.t('donations.ui.mine')} flush>
            <Table
              rows={s.mine}
              rowKey={(r) => r.id}
              columns={[
                { key: 't', header: api.t('donations.ui.date'), render: (r) => <span className="small muted num">{new Date(r.created_at).toLocaleDateString()}</span> },
                {
                  key: 'a',
                  header: api.t('donations.ui.amount'),
                  right: true,
                  render: (r) => (
                    <>
                      <span className="num">{money(s.currency_symbol, r.amount)}</span> {r.revoked && <Status tone="mute">{api.t('donations.ui.revoked')}</Status>}
                    </>
                  ),
                },
                { key: 'r', header: api.t('donations.ui.rewards'), render: (r) => <span className="small">{rewards(api, r)}</span> },
              ]}
            />
          </Panel>
        )}
      </SideLayout>
    </div>
  );
}

interface Row {
  id: number;
  username: string | null;
  amount: number;
  method: string;
  note: string;
  public: boolean;
  vip_days: number;
  bonus: number;
  vip_until: number | null;
  created_by: string | null;
  revoked_at: number | null;
  created_at: number;
}

const EMPTY = { username: '', amount: '', method: '', note: '', vip_days: '', bonus: '', medal_id: '', public: false };

function Admin({ api }: { api: PluginApi }) {
  const [d, setD] = useState<{ items: Row[]; month_total: number; total: number } | null>(null);
  const [f, setF] = useState(EMPTY);
  const [recording, setRecording] = useState(false);
  const [revoking, setRevoking] = useState<Row | null>(null);
  const load = () => void api.json<{ items: Row[]; month_total: number; total: number }>('/api/p/donations/v1/admin/donations').then(setD);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, []);
  const num = (v: string) => (v.trim() === '' ? undefined : Number(v));
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      const r = await api.json<{ failed: { reward: string }[] }>('/api/p/donations/v1/admin/donations', {
        json: {
          username: f.username,
          amount: Math.round(Number(f.amount) * 100),
          method: f.method,
          note: f.note,
          vip_days: num(f.vip_days),
          bonus: num(f.bonus),
          medal_id: num(f.medal_id),
          public: f.public,
        },
      });
      if (r.failed.length) api.toast(api.t('donations.ui.partial', { list: r.failed.map((x) => x.reward).join(', ') }), 'error');
      else api.toast(api.t('donations.ui.recorded'), 'success');
      setF(EMPTY);
      setRecording(false);
      load();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  const revoke = async () => {
    if (!revoking) return;
    try {
      await api.json(`/api/p/donations/v1/admin/donations/${revoking.id}/revoke`, { json: {} });
      setRevoking(null);
      load();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  const set = (k: keyof typeof EMPTY) => (e: { target: { value: string } }) => setF({ ...f, [k]: e.target.value });
  return (
    <div className="stack">
      <PageHead
        title={api.t('donations.ui.admin')}
        actions={
          <Button variant="primary" onClick={() => setRecording(true)}>
            {api.t('donations.ui.record')}
          </Button>
        }
      />
      {d && (
        <Kpis
          items={[
            { label: api.t('donations.ui.this_month'), value: money('', d.month_total), tone: 'gold' },
            { label: api.t('donations.ui.total'), value: money('', d.total) },
            { label: api.t('donations.ui.count'), value: d.items.length },
          ]}
        />
      )}
      <Panel flush primary>
        {!d ? (
          <SkeletonRows n={6} />
        ) : (
          <Table
            rows={d.items}
            rowKey={(r) => r.id}
            minWidth={820}
            columns={[
              { key: 't', header: api.t('donations.ui.date'), render: (r) => <span className="small muted num nowrap">{new Date(r.created_at).toLocaleString()}</span> },
              { key: 'u', header: api.t('donations.ui.username'), render: (r) => r.username ?? '—' },
              {
                key: 'a',
                header: api.t('donations.ui.amount'),
                right: true,
                render: (r) => (
                  <>
                    <span className="num">{money('', r.amount)}</span> {r.revoked_at && <Status tone="mute">{api.t('donations.ui.revoked')}</Status>}
                  </>
                ),
              },
              {
                key: 'm',
                header: api.t('donations.ui.method'),
                render: (r) => (
                  <>
                    <span className="small">{r.method}</span>
                    {r.note && <div className="tiny muted">{r.note}</div>}
                  </>
                ),
              },
              { key: 'r', header: api.t('donations.ui.rewards'), render: (r) => <span className="small">{rewards(api, r)}</span> },
              { key: 'b', header: api.t('donations.ui.by'), render: (r) => <span className="small muted">{r.created_by}</span> },
              {
                key: 'x',
                header: '',
                right: true,
                render: (r) =>
                  !r.revoked_at && (
                    <Button size="xs" variant="ghost" onClick={() => setRevoking(r)}>
                      {api.t('donations.ui.revoke')}
                    </Button>
                  ),
              },
            ]}
          />
        )}
      </Panel>
      <Sheet
        open={recording}
        onClose={() => setRecording(false)}
        title={api.t('donations.ui.record')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setRecording(false)}>
              {api.t('donations.ui.cancel')}
            </Button>
            <Button type="submit" form="donation-record" variant="primary" disabled={!f.username || !(Number(f.amount) > 0)}>
              {api.t('donations.ui.submit')}
            </Button>
          </>
        }
      >
        <form id="donation-record" className="form" onSubmit={(e) => void submit(e)}>
          <div className="form-2">
            <Field label={api.t('donations.ui.username')}>
              <Input name="donation_username" value={f.username} onChange={set('username')} />
            </Field>
            <Field label={api.t('donations.ui.amount_yuan')}>
              <Input name="donation_amount" type="number" min={0} step="0.01" value={f.amount} onChange={set('amount')} />
            </Field>
            <Field label={api.t('donations.ui.method')}>
              <Input name="donation_method" value={f.method} onChange={set('method')} />
            </Field>
            <Field label={api.t('donations.ui.medal_field')}>
              <Input name="donation_medal" type="number" min={1} value={f.medal_id} onChange={set('medal_id')} />
            </Field>
            <Field label={api.t('donations.ui.vip_days_field')}>
              <Input name="donation_vip" type="number" min={0} value={f.vip_days} onChange={set('vip_days')} />
            </Field>
            <Field label={api.t('donations.ui.bonus_field')}>
              <Input name="donation_bonus" type="number" min={0} value={f.bonus} onChange={set('bonus')} />
            </Field>
          </div>
          <Field label={api.t('donations.ui.note')}>
            <Input name="donation_note" value={f.note} onChange={set('note')} />
          </Field>
          <Checkbox name="donation_public" checked={f.public} onChange={(v) => setF({ ...f, public: v })} label={api.t('donations.ui.public')} />
        </form>
      </Sheet>
      <Sheet
        open={revoking !== null}
        onClose={() => setRevoking(null)}
        title={api.t('donations.ui.revoke')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setRevoking(null)}>
              {api.t('donations.ui.cancel')}
            </Button>
            <Button variant="danger" onClick={() => void revoke()}>
              {api.t('donations.ui.revoke')}
            </Button>
          </>
        }
      >
        <p>{api.t('donations.ui.revoke_confirm')}</p>
      </Sheet>
    </div>
  );
}

// 菜单入口：每次加载页面只查询一次是否开放
let enabledP: Promise<boolean> | null = null;
function MenuItem({ api }: SlotProps) {
  const [on, setOn] = useState(false);
  useEffect(() => {
    enabledP ??= api
      .json<Summary>('/api/p/donations/v1/summary')
      .then((s) => s.enabled)
      .catch(() => false);
    void enabledP.then(setOn);
  }, [api]);
  if (!on) return null;
  return (
    <NavItem to="/p/donations/" navigate={api.navigate}>
      {api.t('donations.ui.menu')}
    </NavItem>
  );
}

export default {
  slots: {
    'nav.user_menu': { component: MenuItem, order: 95 },
  },
  routes: [
    { path: '/p/donations/*', component: Page },
    { path: '/admin/p/donations/*', component: Admin },
  ],
} satisfies PluginFrontend;
