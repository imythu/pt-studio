// claims 前端：种子页「认领 / 取消认领」（含认领人数）、导航入口、我的认领（数字带、本月做种进度、上次结算）。
import { useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Kpis, NavItem, PageHead, Panel, PLink, Progress, SkeletonRows, Status, Table } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'claims.ui.claim': '认领',
  'claims.ui.unclaim': '取消认领',
  'claims.ui.count': '{n} 人认领',
  'claims.ui.menu': '我的认领',
  'claims.ui.title': '我的认领',
  'claims.ui.rules': '认领后每个种子每月需要做种 {h} 小时，达标每个奖励 {b} 魔力值。已认领 {n} / {max}。',
  'claims.ui.torrent': '种子',
  'claims.ui.progress': '本月做种',
  'claims.ui.last': '上次结算（{period}）：达标 {passed}、未达标 {failed}、取消 {removed}，奖励 {reward}',
  'claims.ui.passed': '上月达标',
  'claims.ui.failed': '上月未达标',
  'claims.ui.claimed': '已认领',
  'claims.ui.limit': '上限 {n}',
  'claims.ui.qualified': '本月达标',
  'claims.ui.reward': '每个达标奖励',
  'claims.ui.empty': '还没有认领种子',
  'claims.ui.empty_hint': '在下载完成过的种子页点「认领」，承诺长期保种。',
  'claims.ui.status': '上月结果',
  'claims.disabled': '目前不能认领',
  'claims.not_allowed': '这个种子不能认领',
  'claims.not_completed': '只能认领自己下载完成过的种子',
  'claims.limit_reached': '认领数量已达上限',
  'claims.torrent_full': '这个种子的认领人数已满',
  'claims.cooldown': '刚取消过认领，请稍后再试',
  'claims.already_claimed': '已经认领过了',
});
registerMessages('en', {
  'claims.ui.claim': 'Claim',
  'claims.ui.unclaim': 'Unclaim',
  'claims.ui.count': '{n} claimers',
  'claims.ui.menu': 'My claims',
  'claims.ui.title': 'My claims',
  'claims.ui.rules': 'Each claimed torrent needs {h} hours of seeding per month; each one that qualifies earns {b} bonus. Claimed {n} / {max}.',
  'claims.ui.torrent': 'Torrent',
  'claims.ui.progress': 'Seeded this month',
  'claims.ui.last': 'Last settlement ({period}): {passed} passed, {failed} failed, {removed} removed, {reward} bonus',
  'claims.ui.passed': 'Passed last month',
  'claims.ui.failed': 'Failed last month',
  'claims.ui.claimed': 'Claimed',
  'claims.ui.limit': 'limit {n}',
  'claims.ui.qualified': 'Qualified this month',
  'claims.ui.reward': 'Reward each',
  'claims.ui.empty': 'No claimed torrents yet',
  'claims.ui.empty_hint': 'Click “Claim” on a torrent you have completed to commit to seeding it.',
  'claims.ui.status': 'Last month',
  'claims.disabled': 'Claiming is turned off',
  'claims.not_allowed': 'This torrent cannot be claimed',
  'claims.not_completed': 'You can only claim torrents you have downloaded completely',
  'claims.limit_reached': 'You have reached the claim limit',
  'claims.torrent_full': 'This torrent already has enough claimers',
  'claims.cooldown': 'You unclaimed this recently; try again later',
  'claims.already_claimed': 'Already claimed',
});

const errText = (api: PluginApi, e: unknown) => (e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e));

function ClaimButton({ api, ctx }: SlotProps<{ torrent: { id: number } }>) {
  const tid = ctx.torrent.id;
  const [s, setS] = useState<{ claimers: number; mine: boolean; enabled: boolean } | null>(null);
  const load = () => api.json<{ claimers: number; mine: boolean; enabled: boolean }>(`/api/p/claims/v1/torrents/${tid}`).then(setS);
  useEffect(() => {
    void load().catch(() => undefined);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tid]);
  if (!s || (!s.enabled && !s.mine)) return null;
  const toggle = async () => {
    try {
      if (s.mine) await api.fetch(`/api/p/claims/v1/torrents/${tid}/claims`, { method: 'DELETE' });
      else await api.json(`/api/p/claims/v1/torrents/${tid}/claims`, { json: {} });
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <Button aria-pressed={s.mine} title={api.t('claims.ui.count', { n: s.claimers })} onClick={() => void toggle()}>
      {api.t(s.mine ? 'claims.ui.unclaim' : 'claims.ui.claim')}
      <span className="btn-sub">{s.claimers}</span>
    </Button>
  );
}

function MenuItem({ api }: SlotProps) {
  return (
    <NavItem to="/p/claims/" navigate={api.navigate}>
      {api.t('claims.ui.menu')}
    </NavItem>
  );
}

interface Mine {
  items: { torrent_id: number; title: string | null; seed_secs: number; required_secs: number; last_result: string | null }[];
  max_claims: number;
  monthly_seed_hours: number;
  reward_bonus: number;
  last_settlement: { period: string; passed: number; failed: number; removed: number; reward: number } | null;
}

function MyClaims({ api }: { api: PluginApi }) {
  const [d, setD] = useState<Mine | null>(null);
  useEffect(() => {
    void api.json<Mine>('/api/p/claims/v1/me').then(setD);
  }, [api]);
  if (!d) return <SkeletonRows n={6} />;
  const hours = (s: number) => (s / 3600).toFixed(1);
  const qualified = d.items.filter((r) => r.required_secs <= 0 || r.seed_secs >= r.required_secs).length;
  return (
    <div className="stack">
      <PageHead title={api.t('claims.ui.title')} lede={api.t('claims.ui.rules', { h: d.monthly_seed_hours, b: d.reward_bonus, n: d.items.length, max: d.max_claims })} />
      <Kpis
        items={[
          { label: api.t('claims.ui.claimed'), value: d.items.length, hint: api.t('claims.ui.limit', { n: d.max_claims }) },
          { label: api.t('claims.ui.qualified'), value: qualified, tone: 'up' },
          { label: api.t('claims.ui.reward'), value: d.reward_bonus, tone: 'gold' },
          ...(d.last_settlement ? [{ label: d.last_settlement.period, value: `${d.last_settlement.passed} / ${d.last_settlement.failed}`, hint: api.t('claims.ui.last', d.last_settlement) }] : []),
        ]}
      />
      <Panel flush primary>
        <Table
          rows={d.items}
          rowKey={(r) => r.torrent_id}
          empty={
            <div className="empty">
              <b>{api.t('claims.ui.empty')}</b>
              <span>{api.t('claims.ui.empty_hint')}</span>
            </div>
          }
          minWidth={640}
          columns={[
            { key: 't', header: api.t('claims.ui.torrent'), render: (r) => <PLink to={`/torrents/${r.torrent_id}`} navigate={api.navigate} className="tt-name">{r.title ?? `#${r.torrent_id}`}</PLink> },
            {
              key: 'p',
              header: api.t('claims.ui.progress'),
              render: (r) => (
                <div className="hnr-prog">
                  <span className="tiny num">
                    {hours(r.seed_secs)} / {hours(r.required_secs)} h
                  </span>
                  <Progress value={r.required_secs > 0 ? Math.min(1, r.seed_secs / r.required_secs) : 1} tone={r.seed_secs >= r.required_secs ? 'ok' : undefined} />
                </div>
              ),
            },
            { key: 'l', header: api.t('claims.ui.status'), render: (r) => (r.last_result ? <Status tone={r.last_result === 'passed' ? 'ok' : 'bad'}>{api.t(`claims.ui.${r.last_result}`)}</Status> : <span className="muted">—</span>) },
          ]}
        />
      </Panel>
    </div>
  );
}

export default {
  slots: {
    'torrent.detail.actions': { component: ClaimButton, order: 20 },
    'nav.user_menu': { component: MenuItem, order: 55 },
  },
  routes: [{ path: '/p/claims/*', component: MyClaims }],
} satisfies PluginFrontend;
