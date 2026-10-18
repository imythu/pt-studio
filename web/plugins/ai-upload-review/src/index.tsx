// ai-upload-review 前端：后台页面（/admin/p/ai-upload-review/）—— 近 30 天的预审结论分布与管理员反馈。
import { useEffect, useState } from 'react';
import type { PluginApi, PluginFrontend } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Card, Stat } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'ai-upload-review.ui.title': 'AI 发布预审',
  'ai-upload-review.ui.period': '近 30 天',
  'ai-upload-review.ui.reports': '预审数',
  'ai-upload-review.ui.pass': '通过',
  'ai-upload-review.ui.warn': '提示',
  'ai-upload-review.ui.fail': '不通过',
  'ai-upload-review.ui.model_skipped': '跳过模型',
  'ai-upload-review.ui.useful': '反馈有用',
  'ai-upload-review.ui.false_positive': '反馈误报',
});
registerMessages('en', {
  'ai-upload-review.ui.title': 'AI upload review',
  'ai-upload-review.ui.period': 'Last 30 days',
  'ai-upload-review.ui.reports': 'Reviews',
  'ai-upload-review.ui.pass': 'Pass',
  'ai-upload-review.ui.warn': 'Warn',
  'ai-upload-review.ui.fail': 'Fail',
  'ai-upload-review.ui.model_skipped': 'Model skipped',
  'ai-upload-review.ui.useful': 'Marked useful',
  'ai-upload-review.ui.false_positive': 'Marked false positive',
});

const KEYS = ['reports', 'pass', 'warn', 'fail', 'model_skipped', 'useful', 'false_positive'] as const;

function Page({ api }: { api: PluginApi }) {
  const [s, setS] = useState<Record<string, number> | null>(null);
  useEffect(() => {
    void api.json<Record<string, number>>('/api/p/ai-upload-review/v1/admin/stats').then(setS);
  }, [api]);
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t('ai-upload-review.ui.title')}</h1>
      <Card title={api.t('ai-upload-review.ui.period')}>
        <div className="pt-row stats-row">
          {s && KEYS.map((k) => <Stat key={k} label={api.t(`ai-upload-review.ui.${k}`)} value={s[k] ?? 0} />)}
        </div>
      </Card>
    </div>
  );
}

export default { slots: {}, routes: [{ path: '/admin/p/ai-upload-review/*', component: Page }] } satisfies PluginFrontend;
