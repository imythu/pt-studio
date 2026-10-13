// 个人页（identity + accounting）；插件经 user.profile.* 插槽补充（等级、魔力、H&R、邀请…）。
import { useQuery } from '@tanstack/react-query';
import { useParams } from 'react-router';
import { api } from '@pt-studio/sdk';
import { Slot } from '@pt-studio/slots';
import { formatBytes, formatDuration, useT } from '@pt-studio/terms';
import { Badge, Card, Spinner, Stat } from '@pt-studio/ui-kit';
import { ErrorText } from '../errors';
import { useInstalled } from '../manifest';

interface Profile {
  id: number;
  username: string;
  class_id: number;
  status: string;
  title: string | null;
  created_at: number;
}

interface Stats {
  uploaded: number;
  downloaded: number;
  seed_secs: number;
  /** accounting 以十进制字符串返回（如 "1.0000"），没有下载量时为 null */
  ratio: string | number | null;
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
  if (p.isLoading) return <Spinner />;
  if (p.error) return <ErrorText error={p.error} />;
  const profile = p.data as Profile;
  const ctx = { profile };
  const s = stats.data;
  return (
    <div className="profile">
      <h1>
        {profile.username} {profile.status !== 'active' && <Badge tone="danger">{profile.status}</Badge>}
      </h1>
      <Slot name="user.profile.header" ctx={ctx} />
      <Slot name="user.profile.badges" ctx={ctx} />
      <Card>
        {s && (
          <>
            <Stat label={t('ui.profile.uploaded')} value={formatBytes(s.uploaded)} />
            <Stat label={t('ui.profile.downloaded')} value={formatBytes(s.downloaded)} />
            <Stat label={t('ui.profile.ratio')} value={s.ratio === null ? '∞' : Number(s.ratio).toFixed(2)} />
            <Stat label={t('ui.profile.seed_time')} value={formatDuration(s.seed_secs)} />
          </>
        )}
        <Stat label={t('ui.profile.joined')} value={new Date(profile.created_at).toLocaleDateString()} />
        <Slot name="user.profile.stats" ctx={ctx} />
      </Card>
      <Slot name="user.profile.tabs" ctx={ctx} />
    </div>
  );
}
