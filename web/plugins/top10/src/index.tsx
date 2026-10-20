// top10 前端：排行榜页（每个榜单按周期切换）、退出用户榜开关、主导航入口。
import { useEffect, useState } from 'react';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { formatBytes, formatDuration, registerMessages } from '@pt-studio/terms';
import { Grid, NavItem as UiNavItem, PageHead, Panel, PLink, Segmented, SkeletonRows, Switch } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'top10.ui.nav': '排行榜',
  'top10.ui.title': '排行榜',
  'top10.ui.updated': '更新于 {at}，每小时更新',
  'top10.ui.not_yet': '排行榜尚未生成，稍后再来',
  'top10.ui.empty': '暂无数据',
  'top10.ui.opt_out': '不在用户榜中显示我',
  'top10.board.torrents.snatched': '完成数最多',
  'top10.board.torrents.active': '最活跃',
  'top10.board.torrents.seeded': '做种最多',
  'top10.board.users.uploaded': '上传量',
  'top10.board.users.seed_time': '做种时长',
  'top10.board.users.uploads': '发布数',
  'top10.period.day': '24 小时',
  'top10.period.week': '7 天',
  'top10.period.month': '30 天',
  'top10.period.all': '全部',
  'top10.period.now': '当前',
});
registerMessages('en', {
  'top10.ui.nav': 'Top 10',
  'top10.ui.title': 'Top 10',
  'top10.ui.updated': 'Updated {at}, refreshed hourly',
  'top10.ui.not_yet': 'The boards have not been generated yet',
  'top10.ui.empty': 'No data yet',
  'top10.ui.opt_out': 'Hide me from user boards',
  'top10.board.torrents.snatched': 'Most snatched',
  'top10.board.torrents.active': 'Most active',
  'top10.board.torrents.seeded': 'Most seeded',
  'top10.board.users.uploaded': 'Uploaded',
  'top10.board.users.seed_time': 'Seeding time',
  'top10.board.users.uploads': 'Uploads',
  'top10.period.day': '24 hours',
  'top10.period.week': '7 days',
  'top10.period.month': '30 days',
  'top10.period.all': 'All time',
  'top10.period.now': 'Now',
});

interface Entry {
  rank: number;
  id: number;
  value: number;
  name: string | null;
}
interface Board {
  key: string;
  kind: 'torrent' | 'user';
  periods: { period: string; items: Entry[] }[];
}
interface Boards {
  boards: Board[];
  refreshed_at: number | null;
  user_boards: boolean;
}

const fmt = (board: string, v: number) => (board === 'users.uploaded' ? formatBytes(v) : board === 'users.seed_time' ? formatDuration(v) : String(v));

function BoardCard({ api, b }: { api: PluginApi; b: Board }) {
  const [period, setPeriod] = useState(b.periods.find((p) => p.period === 'week')?.period ?? b.periods[0]?.period ?? 'now');
  const items = b.periods.find((p) => p.period === period)?.items ?? [];
  return (
    <Panel
      title={api.t(`top10.board.${b.key}`)}
      flush
      actions={
        b.periods.length > 1 ? (
          <Segmented label={api.t(`top10.board.${b.key}`)} value={period} onChange={setPeriod} options={b.periods.map((p) => ({ value: p.period, label: api.t(`top10.period.${p.period}`) }))} />
        ) : (
          <span className="small muted">{api.t(`top10.period.${period}`)}</span>
        )
      }
    >
      <div className="side-list" data-board={b.key}>
        {items.length === 0 && <p className="pad small muted">{api.t('top10.ui.empty')}</p>}
        {items.map((r) => (
          <PLink key={r.rank} to={b.kind === 'torrent' ? `/torrents/${r.id}` : `/users/${r.id}`} navigate={api.navigate} className={r.rank <= 3 ? 'top' : undefined}>
            <span className="rk">{r.rank}</span>
            <span className="nm">{r.name ?? `#${r.id}`}</span>
            <span className="v">{fmt(b.key, r.value)}</span>
          </PLink>
        ))}
      </div>
    </Panel>
  );
}

function Page({ api }: { api: PluginApi }) {
  const [d, setD] = useState<Boards | null>(null);
  const [out, setOut] = useState<boolean | null>(null);
  useEffect(() => {
    void api.json<Boards>('/api/p/top10/v1/boards').then(setD);
    void api.json<{ opted_out: boolean }>('/api/p/top10/v1/me').then((r) => setOut(r.opted_out));
  }, [api]);
  if (!d) return <SkeletonRows n={8} />;
  return (
    <div className="stack top10">
      <PageHead
        title={api.t('top10.ui.title')}
        lede={d.refreshed_at ? api.t('top10.ui.updated', { at: new Date(d.refreshed_at).toLocaleString() }) : api.t('top10.ui.not_yet')}
        actions={
          d.user_boards &&
          out !== null && (
            <Switch showLabel
              checked={out}
              label={api.t('top10.ui.opt_out')}
              onChange={(v) => void api.json<{ opted_out: boolean }>('/api/p/top10/v1/me', { method: 'PUT', json: { opted_out: v } }).then((r) => setOut(r.opted_out))}
            />
          )
        }
      />
      <Grid cols={2}>
        {d.boards.map((b) => (
          <BoardCard key={b.key} api={api} b={b} />
        ))}
      </Grid>
    </div>
  );
}

function NavItem({ api }: SlotProps) {
  return (
    <UiNavItem to="/p/top10/" navigate={api.navigate}>
      {api.t('top10.ui.nav')}
    </UiNavItem>
  );
}

export default {
  slots: { 'nav.main': { component: NavItem, order: 40 } },
  routes: [{ path: '/p/top10/*', component: Page }],
} satisfies PluginFrontend;
