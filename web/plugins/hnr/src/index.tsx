// hnr 前端：个人页考核统计（本人）；种子页提示是否需要考核。
import { useEffect, useState } from 'react';
import { registerMessages } from '@pt-studio/terms';
import type { PluginFrontend, SlotProps } from '@pt-studio/slots';
import { Badge, Stat } from '@pt-studio/ui-kit';

interface Status {
  active: number;
  failed: number;
  max: number;
  can_download: boolean;
}

function HnrStat({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const [s, setS] = useState<Status | null>(null);
  const self = api.user()?.id === ctx.profile.id;
  useEffect(() => {
    if (self) void api.json<Status>('/api/p/hnr/v1/me').then(setS).catch(() => undefined);
  }, [api, self]);
  if (!s) return null;
  return (
    <Stat
      label="H&R"
      value={
        <>
          {s.active} / <span className={s.failed > 0 ? 'pt-field-error' : undefined}>{s.failed}</span>
        </>
      }
      hint={api.t('hnr.ui.summary', { max: s.max })}
    />
  );
}

interface Requirement {
  required: boolean;
  exempt_rule: string | null;
  min_seed_hours: number;
  window_days: number;
}

function HnrNotice({ ctx, api }: SlotProps<{ torrent: { id: number } }>) {
  const [r, setR] = useState<Requirement | null>(null);
  useEffect(() => {
    void api.json<Requirement>(`/api/p/hnr/v1/torrents/${ctx.torrent.id}/requirement`).then(setR).catch(() => undefined);
  }, [api, ctx.torrent.id]);
  if (!r) return null;
  return r.required ? (
    <Badge tone="danger" title={api.t('hnr.ui.required_hint', { hours: r.min_seed_hours, days: r.window_days })}>
      {api.t('hnr.ui.required', { hours: r.min_seed_hours })}
    </Badge>
  ) : (
    <Badge tone="success">{api.t('hnr.ui.exempt')}</Badge>
  );
}

registerMessages('zh-CN', { 'hnr.ui.summary': '考核中 / 已计入（上限 {max}）', 'hnr.ui.required': 'H&R：需做种 {hours} 小时', 'hnr.ui.required_hint': '下载完成后 {days} 天内做种满 {hours} 小时', 'hnr.ui.exempt': '免 H&R' });
registerMessages('en', { 'hnr.ui.summary': 'In progress / counted (limit {max})', 'hnr.ui.required': 'H&R: seed {hours}h', 'hnr.ui.required_hint': 'Seed {hours} hours within {days} days after completing', 'hnr.ui.exempt': 'No H&R' });

export default {
  slots: {
    'user.profile.stats': { component: HnrStat, order: 40 },
    'torrent.detail.header': { component: HnrNotice, order: 50 },
  },
} satisfies PluginFrontend;
