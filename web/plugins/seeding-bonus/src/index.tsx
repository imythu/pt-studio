// seeding-bonus 前端：做种魔力的计算方式说明（当前站点参数）与估算器。参数在后台插件设置中修改。
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Field, Input, NavItem, Note, PageHead, Panel, SkeletonRows } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'seeding-bonus.ui.menu': '做种魔力规则',
  'seeding-bonus.ui.title': '做种魔力怎么算',
  'seeding-bonus.ui.lede': '每小时结算一次，按你这一小时里正在做种的种子计算。',
  'seeding-bonus.ui.calc': '估算器',
  'seeding-bonus.ui.calc_hint': '输入一个典型种子的情况和你在做的种子数，估算每小时能拿多少（假设所有种子都和它一样、整小时在做种）。',
  'seeding-bonus.ui.size': '单个种子体积（GiB）',
  'seeding-bonus.ui.age': '种子发布了几周',
  'seeding-bonus.ui.seeders': '做种人数（含你）',
  'seeding-bonus.ui.count': '你在做的种子数',
  'seeding-bonus.ui.official_n': '其中官种数',
  'seeding-bonus.ui.per_hour': '约 {n} / 小时',
  'seeding-bonus.ui.per_day': '约 {n} / 天',
  'seeding-bonus.ui.formula': '完整公式',
  'seeding-bonus.ui.params': '本站参数',
});
registerMessages('en', {
  'seeding-bonus.ui.menu': 'Seeding bonus rules',
  'seeding-bonus.ui.title': 'How seeding bonus works',
  'seeding-bonus.ui.lede': 'Settled every hour from the torrents you seeded during that hour.',
  'seeding-bonus.ui.calc': 'Estimator',
  'seeding-bonus.ui.calc_hint': 'Describe a typical torrent and how many you seed to estimate your hourly bonus (assumes all torrents are alike and seeded the whole hour).',
  'seeding-bonus.ui.size': 'Torrent size (GiB)',
  'seeding-bonus.ui.age': 'Torrent age (weeks)',
  'seeding-bonus.ui.seeders': 'Seeders (including you)',
  'seeding-bonus.ui.count': 'Torrents you seed',
  'seeding-bonus.ui.official_n': 'Of which official',
  'seeding-bonus.ui.per_hour': '≈ {n} / hour',
  'seeding-bonus.ui.per_day': '≈ {n} / day',
  'seeding-bonus.ui.formula': 'Full formula',
  'seeding-bonus.ui.params': 'Parameters on this site',
});

interface Params {
  b0: number;
  l: number;
  t0: number;
  n0: number;
  p: number;
  max_count: number;
  official_enabled: boolean;
  official_factor: number;
  official_groups: string[];
  donor_factor: number;
  min_seed_minutes: number;
  min_size_mib: number;
  hourly_cap: number;
}

/** 单个种子的做种分 A（与服务端规则一致） */
function torrentScore(p: Params, gib: number, weeks: number, seeders: number): number {
  const age = 1 - Math.exp((Math.log(0.1) / p.t0) * weeks);
  const rare = 1 + Math.SQRT2 * Math.exp((Math.log(0.1) * (Math.max(seeders, 1) - 1)) / (p.n0 - 1));
  return age * gib * rare;
}

/** 每小时魔力（非 VIP） */
function hourly(p: Params, a: number, count: number, officialA: number): number {
  const curve = (x: number) => ((p.b0 * 2) / Math.PI) * Math.atan(x / p.l);
  const official = p.official_enabled ? curve(officialA) * p.official_factor : 0;
  return Math.min(curve(a) + p.p * Math.min(count, p.max_count) + official, p.hourly_cap);
}

const fmt = (n: number) => n.toLocaleString(undefined, { maximumFractionDigits: 2 });

function Calc({ api, p }: { api: PluginApi; p: Params }) {
  const [v, setV] = useState({ gib: '10', weeks: '4', seeders: '5', count: '20', official: '0' });
  const num = (k: keyof typeof v) => Math.max(0, Number(v[k]) || 0);
  const per = torrentScore(p, num('gib'), num('weeks'), num('seeders'));
  const count = Math.floor(num('count'));
  const official = Math.min(Math.floor(num('official')), count);
  const h = hourly(p, per * count, count, per * official);
  const field = (k: keyof typeof v, label: string) => (
    <Field label={api.t(label)}>
      <Input inputMode="decimal" value={v[k]} onChange={(e) => setV({ ...v, [k]: e.target.value })} />
    </Field>
  );
  return (
    <Panel title={api.t('seeding-bonus.ui.calc')} sub={api.t('seeding-bonus.ui.calc_hint')}>
      <div className="form g2">
        {field('gib', 'seeding-bonus.ui.size')}
        {field('weeks', 'seeding-bonus.ui.age')}
        {field('seeders', 'seeding-bonus.ui.seeders')}
        {field('count', 'seeding-bonus.ui.count')}
        {p.official_enabled && field('official', 'seeding-bonus.ui.official_n')}
      </div>
      <p className="sb-result">
        <b>{api.t('seeding-bonus.ui.per_hour', { n: fmt(h) })}</b>
        <span className="muted"> · {api.t('seeding-bonus.ui.per_day', { n: fmt(h * 24) })}</span>
      </p>
      <p className="small muted">
        A = {fmt(per)} × {count} = {fmt(per * count)}
      </p>
    </Panel>
  );
}

function Page({ api }: { api: PluginApi }) {
  const q = useQuery({ queryKey: ['seeding-bonus', 'explain'], queryFn: () => api.json<Params>('/api/p/seeding-bonus/v1/explain'), staleTime: 300_000 });
  const p = q.data;
  return (
    <div className="stack">
      <PageHead title={api.t('seeding-bonus.ui.title')} lede={api.t('seeding-bonus.ui.lede')} />
      {!p ? (
        <SkeletonRows n={6} />
      ) : (
        <>
          <Panel primary>
            <ol className="sb-steps">
              <li>
                <b>每个种子先算一个“做种分”</b>：体积越大分越高（按 GiB 计）；发布越久分越高（约 {fmt(p.t0)} 周达到满分的 90%，鼓励保老种）；做种的人越少分越高（只有你一人做种时 ×2.41，{fmt(p.n0)} 人时降到 ×1.14）。
              </li>
              <li>
                <b>把所有种子的做种分加起来，得到 A</b>，换算成魔力：A 越大魔力越多，但越往后涨得越慢，最多接近 <b>{fmt(p.b0)}</b> / 小时；A = {fmt(p.l)} 时正好拿到一半（{fmt(p.b0 / 2)}）。
              </li>
              <li>
                <b>做种个数奖励</b>：每做一个种子每小时再加 {fmt(p.p)}，最多计 {fmt(p.max_count)} 个。
              </li>
              {p.official_enabled && (
                <li>
                  <b>官种奖励</b>：官种的做种分按同样的曲线再算一份，乘以 {fmt(p.official_factor)} 额外发放
                  {p.official_groups.length > 0 && <>（本站官组：{p.official_groups.join('、')}）</>}。
                </li>
              )}
              <li>
                <b>VIP</b> 的基础部分 ×{fmt(p.donor_factor)}；一小时内做种不足 {fmt(p.min_seed_minutes)} 分钟的种子不计入
                {p.min_size_mib > 0 && <>，小于 {fmt(p.min_size_mib)} MiB 的种子不计入</>}；只做了部分时间按比例折算；每小时最多 {fmt(p.hourly_cap)}。
              </li>
            </ol>
          </Panel>
          <Calc api={api} p={p} />
          <Panel title={api.t('seeding-bonus.ui.formula')}>
            <pre className="sb-formula">
              {`单种做种分  Aᵢ = (1 − 10^(−周数 / ${fmt(p.t0)})) × 体积GiB × (1 + √2 × 10^(−(做种人数 − 1) / ${fmt(p.n0 - 1)}))
总做种分    A  = Σ Aᵢ          官种做种分 OA = Σ 官种 Aᵢ
每小时魔力     = ${fmt(p.b0)} × 2/π × arctan(A / ${fmt(p.l)}) + ${fmt(p.p)} × min(种子数, ${fmt(p.max_count)})${
                p.official_enabled ? `\n               + ${fmt(p.b0)} × 2/π × arctan(OA / ${fmt(p.l)}) × ${fmt(p.official_factor)}` : ''
              }`}
            </pre>
            <Note tone="info">公式来自 NexusPHP 的做种魔力算法，参数由管理员在后台“做种魔力”插件设置中调整。</Note>
          </Panel>
        </>
      )}
    </div>
  );
}

function MenuItem({ api }: SlotProps) {
  return (
    <NavItem to="/p/seeding-bonus/" navigate={api.navigate}>
      {api.t('seeding-bonus.ui.menu')}
    </NavItem>
  );
}

export default {
  slots: { 'nav.user_menu': { component: MenuItem, order: 21 } },
  routes: [{ path: '/p/seeding-bonus/*', component: Page }],
} satisfies PluginFrontend;
