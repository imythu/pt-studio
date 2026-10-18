// ai-upload-review 前端：后台页面（/admin/p/ai-upload-review/）—— 近 30 天的预审结论分布与管理员反馈；
// 种子页员工区“AI 预审”标签（最新报告、重新预审、标记有用 / 误报）。
import { useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Stat, Table } from '@pt-studio/ui-kit';

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
  'ai-upload-review.ui.tab': 'AI 预审',
  'ai-upload-review.ui.none': '这个种子还没有预审报告',
  'ai-upload-review.ui.verdict': '结论：{v}',
  'ai-upload-review.ui.confidence': '置信度 {n}%',
  'ai-upload-review.ui.revision': '第 {n} 次 · {at}',
  'ai-upload-review.ui.model': '模型 {m}',
  'ai-upload-review.ui.skipped': '未调用模型（{r}），只有规则检查结果',
  'ai-upload-review.ui.rerun': '重新预审',
  'ai-upload-review.ui.running': '预审中…',
  'ai-upload-review.ui.mark_useful': '有用',
  'ai-upload-review.ui.mark_false': '误报',
  'ai-upload-review.ui.feedback': '已反馈：{f}',
  'ai-upload-review.ui.thanks': '已记录反馈',
  'ai-upload-review.ui.severity.low': '低',
  'ai-upload-review.ui.severity.medium': '中',
  'ai-upload-review.ui.severity.high': '高',
  'ai-upload-review.ui.source.rule': '规则',
  'ai-upload-review.ui.source.model': '模型',
  'ai-upload-review.budget_exhausted': '今天的模型调用预算已用完',
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
  'ai-upload-review.ui.tab': 'AI review',
  'ai-upload-review.ui.none': 'No review report for this torrent yet',
  'ai-upload-review.ui.verdict': 'Verdict: {v}',
  'ai-upload-review.ui.confidence': 'confidence {n}%',
  'ai-upload-review.ui.revision': 'revision {n} · {at}',
  'ai-upload-review.ui.model': 'model {m}',
  'ai-upload-review.ui.skipped': 'Model not called ({r}); rule checks only',
  'ai-upload-review.ui.rerun': 'Review again',
  'ai-upload-review.ui.running': 'Reviewing…',
  'ai-upload-review.ui.mark_useful': 'Useful',
  'ai-upload-review.ui.mark_false': 'False positive',
  'ai-upload-review.ui.feedback': 'Feedback: {f}',
  'ai-upload-review.ui.thanks': 'Feedback saved',
  'ai-upload-review.ui.severity.low': 'low',
  'ai-upload-review.ui.severity.medium': 'medium',
  'ai-upload-review.ui.severity.high': 'high',
  'ai-upload-review.ui.source.rule': 'rule',
  'ai-upload-review.ui.source.model': 'model',
  'ai-upload-review.budget_exhausted': 'The daily model budget is used up',
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

interface Report {
  report_id: number;
  revision: number;
  verdict: string;
  confidence: number | null;
  findings: { code: string; severity: string; message: string; source: string }[];
  model: string | null;
  model_skipped: string | null;
  feedback: string | null;
  created_at: number;
}

const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e);
const VERDICT_TONE: Record<string, 'success' | 'primary' | 'danger'> = { pass: 'success', warn: 'primary', fail: 'danger' };
const SEV_TONE: Record<string, 'muted' | 'primary' | 'danger'> = { low: 'muted', medium: 'primary', high: 'danger' };

/** 种子页员工区“AI 预审”标签 */
function TorrentTab({ ctx, api }: SlotProps<{ torrent: { id: number } }>) {
  const tid = ctx.torrent.id;
  const [r, setR] = useState<Report | null | undefined>(undefined);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    void api
      .json<Report>(`/api/p/ai-upload-review/v1/torrents/${tid}`)
      .then(setR)
      .catch((e: unknown) => setR(e instanceof ApiError && e.status === 403 ? undefined : null));
  }, [api, tid]);
  const rerun = async () => {
    setBusy(true);
    try {
      setR(await api.json<Report>(`/api/p/ai-upload-review/v1/torrents/${tid}/rerun`, { json: {} }));
    } catch (e) {
      api.toast(errText(api, e), 'error');
    } finally {
      setBusy(false);
    }
  };
  const feedback = async (useful: boolean) => {
    if (!r) return;
    try {
      await api.json(`/api/p/ai-upload-review/v1/reports/${r.report_id}/feedback`, { json: { useful } });
      setR({ ...r, feedback: useful ? 'useful' : 'false_positive' });
      api.toast(api.t('ai-upload-review.ui.thanks'), 'success');
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  if (r === undefined) return null;
  const rerunBtn = (
    <Button disabled={busy} onClick={() => void rerun()}>
      {api.t(busy ? 'ai-upload-review.ui.running' : 'ai-upload-review.ui.rerun')}
    </Button>
  );
  if (r === null)
    return (
      <Card>
        <p className="pt-muted">{api.t('ai-upload-review.ui.none')}</p>
        {rerunBtn}
      </Card>
    );
  return (
    <Card>
      <p className="pt-row">
        <Badge tone={VERDICT_TONE[r.verdict] ?? 'muted'}>{api.t('ai-upload-review.ui.verdict', { v: api.t(`ai-upload-review.ui.${r.verdict}`) })}</Badge>
        {r.confidence !== null && <span className="pt-muted small">{api.t('ai-upload-review.ui.confidence', { n: Math.round(r.confidence * 100) })}</span>}
        <span className="pt-muted small">{api.t('ai-upload-review.ui.revision', { n: r.revision, at: new Date(r.created_at).toLocaleString() })}</span>
        {r.model && <span className="pt-muted small">{api.t('ai-upload-review.ui.model', { m: r.model })}</span>}
      </p>
      {r.model_skipped && <p className="pt-muted small">{api.t('ai-upload-review.ui.skipped', { r: r.model_skipped })}</p>}
      <Table
        rows={r.findings}
        rowKey={(f) => `${f.source}:${f.code}:${f.message}`}
        empty="—"
        columns={[
          { key: 's', header: '', render: (f) => <Badge tone={SEV_TONE[f.severity] ?? 'muted'}>{api.t(`ai-upload-review.ui.severity.${f.severity}`)}</Badge> },
          { key: 'm', header: '', render: (f) => f.message },
          { key: 'o', header: '', render: (f) => <span className="pt-muted small">{`${api.t(`ai-upload-review.ui.source.${f.source}`)} · ${f.code}`}</span> },
        ]}
      />
      <span className="pt-row">
        {rerunBtn}
        {r.feedback ? (
          <span className="pt-muted small">{api.t('ai-upload-review.ui.feedback', { f: api.t(r.feedback === 'useful' ? 'ai-upload-review.ui.mark_useful' : 'ai-upload-review.ui.mark_false') })}</span>
        ) : (
          <>
            <Button variant="ghost" onClick={() => void feedback(true)}>
              {api.t('ai-upload-review.ui.mark_useful')}
            </Button>
            <Button variant="ghost" onClick={() => void feedback(false)}>
              {api.t('ai-upload-review.ui.mark_false')}
            </Button>
          </>
        )}
      </span>
    </Card>
  );
}

export default {
  slots: {
    'admin.torrent.tabs': { component: TorrentTab, order: 10, title_term: 'ai-upload-review.ui.tab' },
  },
  routes: [{ path: '/admin/p/ai-upload-review/*', component: Page }],
} satisfies PluginFrontend;
