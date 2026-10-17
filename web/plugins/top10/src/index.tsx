// top10 前端：排行榜页（每个榜单按周期切换）、退出用户榜开关、主导航入口。
import { useEffect, useState, type ReactNode } from 'react';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { formatBytes, formatDuration, registerMessages } from '@pt-studio/terms';
import { Card, Table } from '@pt-studio/ui-kit';

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

function A({ api, to, children }: { api: PluginApi; to: string; children: ReactNode }) {
  return (
    <a
      href={to}
      onClick={(e) => {
        e.preventDefault();
        api.navigate(to);
      }}
    >
      {children}
    </a>
  );
}

const fmt = (board: string, v: number) => (board === 'users.uploaded' ? formatBytes(v) : board === 'users.seed_time' ? formatDuration(v) : String(v));

function BoardCard({ api, b }: { api: PluginApi; b: Board }) {
  const [period, setPeriod] = useState(b.periods.find((p) => p.period === 'week')?.period ?? b.periods[0]?.period ?? 'now');
  const items = b.periods.find((p) => p.period === period)?.items ?? [];
  return (
    <Card
      title={api.t(`top10.board.${b.key}`)}
      actions={
        b.periods.length > 1 ? (
          <span className="segmented" role="group" aria-label={api.t(`top10.board.${b.key}`)}>
            {b.periods.map((p) => (
              <button key={p.period} type="button" aria-pressed={p.period === period} onClick={() => setPeriod(p.period)}>
                {api.t(`top10.period.${p.period}`)}
              </button>
            ))}
          </span>
        ) : (
          <span className="pt-muted small">{api.t(`top10.period.${period}`)}</span>
        )
      }
    >
      <div data-board={b.key}>
        <Table
          rows={items}
          rowKey={(r) => r.rank}
          empty={api.t('top10.ui.empty')}
          columns={[
            { key: 'r', header: '#', render: (r) => r.rank },
            {
              key: 'n',
              header: '',
              render: (r) =>
                b.kind === 'torrent' ? (
                  <A api={api} to={`/torrents/${r.id}`}>
                    {r.name ?? `#${r.id}`}
                  </A>
                ) : (
                  <A api={api} to={`/users/${r.id}`}>
                    {r.name ?? `#${r.id}`}
                  </A>
                ),
            },
            { key: 'v', header: '', render: (r) => fmt(b.key, r.value) },
          ]}
        />
      </div>
    </Card>
  );
}

function Page({ api }: { api: PluginApi }) {
  const [d, setD] = useState<Boards | null>(null);
  const [out, setOut] = useState<boolean | null>(null);
  useEffect(() => {
    void api.json<Boards>('/api/p/top10/v1/boards').then(setD);
    void api.json<{ opted_out: boolean }>('/api/p/top10/v1/me').then((r) => setOut(r.opted_out));
  }, [api]);
  if (!d) return null;
  return (
    <div className="pt-stack top10">
      <h1 className="page-title">{api.t('top10.ui.title')}</h1>
      <p className="pt-muted small">{d.refreshed_at ? api.t('top10.ui.updated', { at: new Date(d.refreshed_at).toLocaleString() }) : api.t('top10.ui.not_yet')}</p>
      {d.user_boards && out !== null && (
        <label className="pt-check">
          <input
            type="checkbox"
            checked={out}
            onChange={(e) => {
              const v = e.target.checked;
              void api.json<{ opted_out: boolean }>('/api/p/top10/v1/me', { method: 'PUT', json: { opted_out: v } }).then((r) => setOut(r.opted_out));
            }}
          />{' '}
          {api.t('top10.ui.opt_out')}
        </label>
      )}
      <div className="top10-grid">
        {d.boards.map((b) => (
          <BoardCard key={b.key} api={api} b={b} />
        ))}
      </div>
    </div>
  );
}

function NavItem({ api }: SlotProps) {
  return (
    <A api={api} to="/p/top10/">
      {api.t('top10.ui.nav')}
    </A>
  );
}

export default {
  slots: { 'nav.main': { component: NavItem, order: 40 } },
  routes: [{ path: '/p/top10/*', component: Page }],
} satisfies PluginFrontend;
