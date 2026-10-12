// levels 前端：个人页显示当前等级与升级进度（仅本人：levels 只提供 /v1/me）。
import { useEffect, useState } from 'react';
import { registerMessages } from '@pt-studio/terms';
import type { PluginFrontend, SlotProps } from '@pt-studio/slots';
import { Progress, Stat } from '@pt-studio/ui-kit';

interface Me {
  class: { id: number; name_key: string };
  next: { id: number; name_key: string; progress: { metric: string; target: string; current: string; met: boolean }[] } | null;
  warning: { deadline: number } | null;
  protected: boolean;
}

function LevelStat({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const [me, setMe] = useState<Me | null>(null);
  const self = api.user()?.id === ctx.profile.id;
  useEffect(() => {
    if (self) void api.json<Me>('/api/p/levels/v1/me').then(setMe).catch(() => undefined);
  }, [api, self]);
  if (!me) return null;
  const met = me.next ? me.next.progress.filter((p) => p.met).length : 0;
  return (
    <Stat
      label={api.t('levels.ui.class')}
      value={api.t(`term:${me.class.name_key}`)}
      hint={
        me.warning ? (
          <span className="pt-field-error">{api.t('levels.ui.warning', { date: new Date(me.warning.deadline).toLocaleDateString() })}</span>
        ) : me.next ? (
          <span title={me.next.progress.map((p) => `${p.metric}: ${p.current} / ${p.target}`).join('\n')}>
            → {api.t(`term:${me.next.name_key}`)} <Progress value={met} max={me.next.progress.length} />
          </span>
        ) : null
      }
    />
  );
}

registerMessages('zh-CN', { 'levels.ui.class': '等级', 'levels.ui.warning': '保级警告：{date} 前未达标将降级' });
registerMessages('en', { 'levels.ui.class': 'Class', 'levels.ui.warning': 'Warning: demotion on {date} unless requirements are met' });

export default {
  slots: { 'user.profile.stats': { component: LevelStat, order: 10 } },
} satisfies PluginFrontend;
