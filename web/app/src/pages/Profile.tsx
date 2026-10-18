// 个人页（identity + accounting）；插件经 user.profile.* 插槽补充（等级、魔力、H&R、邀请…）。
import { useQueries, useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { Link, useParams } from 'react-router';
import { api } from '@pt-studio/sdk';
import { Slot } from '@pt-studio/slots';
import { formatBytes, formatDuration, useT } from '@pt-studio/terms';
import { Avatar, Badge, Card, Empty, SkeletonRows, Stat, Tabs } from '@pt-studio/ui-kit';
import { SwarmCell } from './Torrents';
import { ErrorText } from '../errors';
import { useInstalled } from '../manifest';

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
  /** accounting 以十进制字符串返回（如 "1.0000"），没有下载量时为 null */
  ratio: string | number | null;
}

interface SnatchRow {
  torrent_id: number;
  uploaded: number;
  downloaded: number;
  seed_secs: number;
  completed_at: string | null;
  last_action_at: string;
}

/** 做种中 / 下载中 / 已完成列表（accounting snatches + 种子标题） */
function UserTorrents({ uid }: { uid: string }) {
  const t = useT();
  const [tab, setTab] = useState('seeding');
  const q = useQuery({
    queryKey: ['snatches', uid, tab],
    queryFn: () => api<{ items: SnatchRow[] }>(`/api/p/accounting/v1/users/${uid}/snatches?state=${tab}&limit=30`),
    retry: false,
  });
  const rows = q.data?.items ?? [];
  const titles = useQueries({
    queries: rows.map((r) => ({
      queryKey: ['torrent-title', r.torrent_id],
      queryFn: () => api<{ id: number; title: string; size: number }>(`/api/p/torrent/v1/torrents/${r.torrent_id}`),
      staleTime: 300_000,
      retry: false,
    })),
  });
  return (
    <Card className="list-card" title={t('ui.profile.activity')}>
      <div className="card-pad">
        <Tabs
          value={tab}
          onChange={setTab}
          tabs={[
            { id: 'seeding', label: t('ui.profile.seeding') },
            { id: 'leeching', label: t('ui.profile.leeching') },
            { id: 'completed', label: t('ui.profile.completed') },
          ]}
        />
      </div>
      {q.isLoading ? (
        <div className="card-pad">
          <SkeletonRows n={3} />
        </div>
      ) : rows.length === 0 ? (
        <Empty>{t('ui.profile.none')}</Empty>
      ) : (
        <div className="pt-table-wrap">
          <table className="pt-table">
            <thead>
              <tr>
                <th>{t('ui.torrent.title')}</th>
                <th>{t('ui.torrent.size')}</th>
                <th>{t('ui.profile.uploaded')}</th>
                <th>{t('ui.profile.downloaded')}</th>
                <th>{t('ui.profile.seed_time')}</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r, i) => {
                const d = titles[i]?.data;
                return (
                  <tr key={r.torrent_id}>
                    <td>
                      <Link className="t-title" to={`/torrents/${r.torrent_id}`}>
                        {d?.title ?? `#${r.torrent_id}`}
                      </Link>
                    </td>
                    <td className="num">{d ? formatBytes(d.size) : '-'}</td>
                    <td className="num">{formatBytes(r.uploaded)}</td>
                    <td className="num">{formatBytes(r.downloaded)}</td>
                    <td className="num">{formatDuration(r.seed_secs)}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </Card>
  );
}

export function ProfilePage() {
  const t = useT();
  const { id } = useParams();
  const hasAccounting = useInstalled('accounting');
  const p = useQuery({ queryKey: ['user', id], queryFn: () => api<Profile>(`/api/p/identity/v1/users/${id}`) });
  const stats = useQuery({
    queryKey: ['user-stats', id],
    queryFn: () => api<Stats>(`/api/p/accounting/v1/users/${id}/stats`),
    enabled: hasAccounting,
    retry: false,
  });
  if (p.isLoading) return <SkeletonRows n={4} />;
  if (p.error) return <ErrorText error={p.error} />;
  const profile = p.data as Profile;
  const ctx = { profile };
  const s = stats.data;
  return (
    <div className="profile">
      <div className="p-cover" style={{ backgroundImage: 'url(/frieren/frieren-sky-landscape.jpg)' }}>
        <div className="p-cover-overlay" />
      </div>
      <section className="p-head">
        <Avatar src={profile.avatar_url} name={profile.username} size={76} className="p-avatar" />
        <div className="p-main">
          <h1>
            {profile.username} {profile.status !== 'active' && <Badge tone="danger">{profile.status}</Badge>}
          </h1>
          <p className="pt-muted">
            {profile.title ? `${profile.title} · ` : ''}
            {t('ui.profile.joined')} {new Date(profile.created_at).toLocaleDateString()}
          </p>
          {profile.bio && <p className="p-bio">{profile.bio}</p>}
          <Slot name="user.profile.header" ctx={ctx} />
          <Slot name="user.profile.badges" ctx={ctx} />
        </div>
      </section>
      <Card>
        <div className="pt-row stats-row">
          {s && (
            <>
              <Stat label={t('ui.profile.uploaded')} value={formatBytes(s.uploaded)} />
              <Stat label={t('ui.profile.downloaded')} value={formatBytes(s.downloaded)} />
              <Stat label={t('ui.profile.ratio')} value={s.ratio === null ? '∞' : Number(s.ratio).toFixed(2)} />
              <Stat label={t('ui.profile.seed_time')} value={formatDuration(s.seed_secs)} />
            </>
          )}
          <Slot name="user.profile.stats" ctx={ctx} />
        </div>
      </Card>
      {hasAccounting && <UserTorrents uid={String(profile.id)} />}
      <Slot name="user.profile.tabs" ctx={ctx} />
    </div>
  );
}
