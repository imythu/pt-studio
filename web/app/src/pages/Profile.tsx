// 个人页（identity + accounting）：封面、头像与等级、数据带（插件 user.profile.stats 并入）、动态页签（做种中 / 下载中 / 已完成 + 插件页签）、
// 侧栏（简介、插件 header / badges）；窄屏侧栏排在动态之后（P3）。
import { useQueries, useQuery } from '@tanstack/react-query';
import { Suspense, useEffect, useRef, useState } from 'react';
import { Link, useNavigate, useParams, useSearchParams } from 'react-router';
import { api } from '@pt-studio/sdk';
import { Slot, SlotEntryView, useSlotEntries, useSlotEntryTitle } from '@pt-studio/slots';
import { formatBytes, formatDuration, useT } from '@pt-studio/terms';
import { Avatar, Panel, SkeletonRows, Stat, Status, Table, Tabs } from '@pt-studio/ui-kit';
import { ErrorText } from '../errors';
import { fmtRatio, fmtSeed, levelName, type LevelClass } from '../lib';
import { useInstalled } from '../manifest';
import { useSession } from '../session';
import { statusName } from './AdminUsers';

interface Profile {
  id: number;
  username: string;
  class_id: number;
  status: string;
  title: string | null;
  created_at: number;
  avatar_url?: string | null;
  bio?: string | null;
}

interface Stats {
  uploaded: number;
  downloaded: number;
  seed_secs: number;
  ratio: string | number | null;
  uploaded_raw?: number;
  downloaded_raw?: number;
  seeding_count?: number;
  leeching_count?: number;
  completed_count?: number;
}

interface SnatchRow {
  torrent_id: number;
  uploaded: number;
  downloaded: number;
  seed_secs: number;
  completed_at: string | null;
  last_action_at: string;
}

function Snatches({ uid, state }: { uid: number; state: string }) {
  const t = useT();
  const q = useQuery({
    queryKey: ['snatches', uid, state],
    queryFn: () => api<{ items: SnatchRow[] }>(`/api/p/accounting/v1/users/${uid}/snatches?state=${state}&limit=50`),
    retry: false,
  });
  const rows = q.data?.items ?? [];
  const titles = useQueries({
    queries: rows.map((r) => ({
      queryKey: ['torrent-title', r.torrent_id],
      queryFn: () => api<{ id: number; title: string; subtitle: string | null; size: number }>(`/api/p/torrent/v1/torrents/${r.torrent_id}`),
      staleTime: 300_000,
      retry: false,
    })),
  });
  if (q.isLoading) return <SkeletonRows n={3} />;
  if (q.error) return <div className="pad"><ErrorText error={q.error} /></div>;
  return (
    <Table
      rows={rows}
      rowKey={(r) => r.torrent_id}
      empty={t('ui.profile.none')}
      minWidth={640}
      columns={[
        {
          key: 't',
          header: t('ui.torrent.title'),
          render: (r) => {
            const d = titles[rows.indexOf(r)]?.data;
            return (
              <Link className="tt-name" to={`/torrents/${r.torrent_id}`}>
                {d?.subtitle || d?.title || `#${r.torrent_id}`}
              </Link>
            );
          },
        },
        { key: 's', header: t('ui.torrent.size'), right: true, render: (r) => <span className="num">{titles[rows.indexOf(r)]?.data ? formatBytes((titles[rows.indexOf(r)]?.data as { size: number }).size) : '-'}</span> },
        { key: 'u', header: t('ui.profile.uploaded'), right: true, render: (r) => <span className="num up">{formatBytes(r.uploaded)}</span> },
        { key: 'd', header: t('ui.profile.downloaded'), right: true, render: (r) => <span className="num down">{formatBytes(r.downloaded)}</span> },
        { key: 'st', header: t('ui.profile.seed_time'), right: true, render: (r) => <span className="num">{formatDuration(r.seed_secs)}</span> },
      ]}
    />
  );
}

function ActivityTabs({ profile, stats }: { profile: Profile; stats?: Stats }) {
  const t = useT();
  const hasAccounting = useInstalled('accounting');
  const entries = useSlotEntries('user.profile.tabs');
  const title = useSlotEntryTitle();
  const own = [
    { id: 'seeding', label: t('ui.profile.seeding'), count: stats?.seeding_count },
    { id: 'leeching', label: t('ui.profile.leeching'), count: stats?.leeching_count },
    { id: 'completed', label: t('ui.profile.completed'), count: stats?.completed_count },
  ];
  const tabs = [...(hasAccounting ? own : []), ...entries.map((e) => ({ id: `p:${e.plugin}`, label: title(e) }))];
  // ?tab=seeding 等：从「我的数据」点进来直接打开对应页签
  const want = useSearchParams()[0].get('tab');
  const [tab, setTab] = useState(want ?? tabs[0]?.id ?? '');
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!want) return;
    setTab(want);
    box.current?.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }, [want]);
  if (tabs.length === 0) return null;
  const cur = entries.find((e) => `p:${e.plugin}` === tab);
  return (
    <div ref={box} className="scroll-mt">
      <Panel flush>
        <Tabs tabs={tabs} value={tab} onChange={setTab} label={t('ui.profile.activity')} />
        {cur ? (
          <div className="tabp">
            <SlotEntryView entry={cur} ctx={{ profile }} />
          </div>
        ) : (
          <Snatches uid={profile.id} state={tab} />
        )}
      </Panel>
    </div>
  );
}

export function ProfilePage() {
  const t = useT();
  const { id } = useParams();
  const s = useSession();
  const hasAccounting = useInstalled('accounting');
  const hasMessages = useInstalled('messages');
  const hasLevels = useInstalled('levels');
  const hasRatioPolicy = useInstalled('ratio-policy');
  const nav = useNavigate();
  const p = useQuery({ queryKey: ['user', id], queryFn: () => api<Profile>(`/api/p/identity/v1/users/${id}`) });
  const stats = useQuery({ queryKey: ['user-stats', id], queryFn: () => api<Stats>(`/api/p/accounting/v1/users/${id}/stats`), enabled: hasAccounting, retry: false });
  const cls = useQuery({
    queryKey: ['level-classes'],
    queryFn: () => api<{ items: LevelClass[] }>('/api/p/levels/v1/classes'),
    enabled: hasLevels,
    staleTime: 600_000,
    retry: false,
  });
  if (p.isLoading) return <SkeletonRows n={5} />;
  if (p.error) return <ErrorText error={p.error} />;
  const profile = p.data as Profile;
  const ctx = { profile };
  const st = stats.data;
  const seed = st ? fmtSeed(st.seed_secs) : null;
  const level = cls.data?.items.find((c) => c.id === profile.class_id);
  const mine = s.me?.id === profile.id;
  const days = Math.max(1, Math.ceil((Date.now() - profile.created_at) / 86_400_000));
  // ?tab= 只替换查询串，不新增历史记录
  const go = (to: string) => void nav(to, { replace: to.startsWith('?') });
  return (
    <div className="profile">
      <div className="pf-cover" aria-hidden="true" />
      <section className="pf-wrap" data-primary>
        <div className="pf-head">
          <Avatar src={profile.avatar_url} name={profile.username} size={112} ring className="pf-ava" />
          <div className="pf-name">
            <h1>
              {profile.username}
              {level && (
                <span className="cls" style={level.color ? { color: level.color, borderColor: level.color } : undefined}>
                  {levelName(t, level)}
                </span>
              )}
              {profile.title && <span className="tag tag-g">{profile.title}</span>}
              {profile.status !== 'active' && <Status tone="bad">{statusName(t, profile.status)}</Status>}
            </h1>
            <p>{t('ui.profile.joined_line', { date: new Date(profile.created_at).toLocaleDateString(), n: days })}</p>
            <Slot name="user.profile.header" ctx={ctx} className="row" />
          </div>
          <div className="pf-acts row">
            {mine ? (
              <Link className="btn btn-sm" to="/settings">
                {t('ui.profile.edit')}
              </Link>
            ) : (
              hasMessages && (
                <Link className="btn btn-sm" to={`/p/messages/compose?to=${encodeURIComponent(profile.username)}`}>
                  {t('ui.profile.message')}
                </Link>
              )
            )}
            {s.isStaff && !mine && (
              <Link className="btn btn-sm btn-ghost" to={`/admin/users/${profile.id}`}>
                {t('ui.nav.admin')}
              </Link>
            )}
          </div>
        </div>
        <div className="pf-stats">
          {st && (
            <>
              <Stat
                label={t('ui.profile.uploaded')}
                value={formatBytes(st.uploaded)}
                tone="up"
                hint={st.uploaded_raw !== undefined && t('ui.profile.real', { v: formatBytes(st.uploaded_raw) })}
                to="?tab=completed"
                navigate={go}
              />
              <Stat
                label={t('ui.profile.downloaded')}
                value={formatBytes(st.downloaded)}
                tone="down"
                hint={st.downloaded_raw !== undefined && t('ui.profile.real', { v: formatBytes(st.downloaded_raw) })}
                to="?tab=completed"
                navigate={go}
              />
              <Stat label={t('ui.profile.ratio')} value={fmtRatio(st.ratio)} to={mine && hasRatioPolicy ? '/p/ratio-policy/' : '?tab=completed'} navigate={go} />
              <Stat
                label={t('ui.profile.seed_time')}
                value={
                  <>
                    {seed?.v}
                    <small> {seed?.u}</small>
                  </>
                }
                hint={st.seeding_count !== undefined && t('ui.profile.seeding_n', { n: st.seeding_count })}
                to="?tab=seeding"
                navigate={go}
              />
            </>
          )}
          <Slot name="user.profile.stats" ctx={ctx} className="pf-slot" />
        </div>
      </section>
      <div className="pf-grid">
        <div className="stack m-after">
          {profile.bio && (
            <Panel title={t('ui.profile.about')}>
              <p className="pt-prewrap small">{profile.bio}</p>
            </Panel>
          )}
          <Slot name="user.profile.badges" ctx={ctx} className="stack" />
        </div>
        <div className="stack">
          <Suspense fallback={<SkeletonRows n={4} />}>
            <ActivityTabs profile={profile} stats={st} />
          </Suspense>
        </div>
      </div>
    </div>
  );
}
