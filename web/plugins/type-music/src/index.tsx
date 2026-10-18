// type-music 前端：发布页（音乐类型）抓轨日志预检 —— 对表单里已添加的 log 逐个评分，列出扣分项；发布后由后台任务正式评分。
import { useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'type-music.ui.log_check': '抓轨日志预检',
  'type-music.ui.log_hint': '在上方“抓轨日志”中添加 log 文件后可预先查看评分；发布后会自动正式评分。',
  'type-music.ui.check': '检查 {n} 个 log',
  'type-music.ui.checking': '检查中…',
  'type-music.ui.score': '{n} 分',
  'type-music.ui.log_n': 'log #{n}',
  'type-music.ui.no_issues': '没有扣分项',
  'type-music.ui.track': '第 {n} 轨',
  'type-music.ui.deduction': '−{n}',
  'type-music.log_too_large': '单个 log 不能超过 512 KiB',
});
registerMessages('en', {
  'type-music.ui.log_check': 'Rip log pre-check',
  'type-music.ui.log_hint': 'Add log files under “Rip logs” above to preview their scores; they are scored for real after upload.',
  'type-music.ui.check': 'Check {n} log(s)',
  'type-music.ui.checking': 'Checking…',
  'type-music.ui.score': '{n} points',
  'type-music.ui.log_n': 'log #{n}',
  'type-music.ui.no_issues': 'No deductions',
  'type-music.ui.track': 'track {n}',
  'type-music.ui.deduction': '−{n}',
  'type-music.log_too_large': 'A log must not exceed 512 KiB',
});

interface Report {
  software: string | null;
  version: string | null;
  score: number;
  issues: { code: string; track: number | null; deduction: number; message_term: string }[];
}

const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e);

function LogCheck({ ctx, api }: SlotProps<{ draft: { type_id?: string; fields?: Record<string, unknown> } }>) {
  const [reports, setReports] = useState<(Report | string)[] | null>(null);
  const [busy, setBusy] = useState(false);
  if (ctx.draft.type_id !== 'music') return null;
  const raw = ctx.draft.fields?.logs;
  const logs = Array.isArray(raw) ? raw.filter((l): l is string => typeof l === 'string' && l.length > 0) : [];
  const check = async () => {
    setBusy(true);
    try {
      setReports(
        await Promise.all(
          logs.map((log) =>
            api.json<Report>('/api/p/type-music/v1/log-check', { json: { log } }).catch((e: unknown) => errText(api, e)),
          ),
        ),
      );
    } finally {
      setBusy(false);
    }
  };
  return (
    <Card title={api.t('type-music.ui.log_check')}>
      <p className="pt-muted small">{api.t('type-music.ui.log_hint')}</p>
      <Button disabled={busy || logs.length === 0} onClick={() => void check()}>
        {api.t(busy ? 'type-music.ui.checking' : 'type-music.ui.check', { n: logs.length })}
      </Button>
      {reports?.map((r, i) => (
        <div key={i} className="pt-stack">
          <p className="pt-row">
            <strong>{api.t('type-music.ui.log_n', { n: i + 1 })}</strong>
            {typeof r === 'string' ? (
              <span className="pt-error">{r}</span>
            ) : (
              <>
                <Badge tone={r.score >= 100 ? 'success' : r.score >= 80 ? 'primary' : 'danger'}>{api.t('type-music.ui.score', { n: r.score })}</Badge>
                {r.software && <span className="pt-muted small">{`${r.software} ${r.version ?? ''}`}</span>}
              </>
            )}
          </p>
          {typeof r !== 'string' &&
            (r.issues.length === 0 ? (
              <p className="pt-muted small">{api.t('type-music.ui.no_issues')}</p>
            ) : (
              <ul className="small">
                {r.issues.map((x, j) => (
                  <li key={j}>
                    {api.t(`term:${x.message_term}`)}
                    {x.track !== null && ` · ${api.t('type-music.ui.track', { n: x.track })}`} <span className="pt-muted">{api.t('type-music.ui.deduction', { n: x.deduction })}</span>
                  </li>
                ))}
              </ul>
            ))}
        </div>
      ))}
    </Card>
  );
}

export default {
  slots: {
    'torrent.upload.extra': { component: LogCheck, order: 30 },
  },
} satisfies PluginFrontend;
