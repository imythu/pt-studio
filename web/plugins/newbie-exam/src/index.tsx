// newbie-exam 前端：首页小部件（考核中显示当前阶段进度）、考核页（全部阶段与目标）、后台用户标签页（分配 / 延期 / 豁免）。
import { useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Input, Progress } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'newbie-exam.ui.title': '新手考核',
  'newbie-exam.ui.stage': '第 {n} 阶段 · {at} 截止',
  'newbie-exam.ui.metric.uploaded_gib': '上传量（GiB）',
  'newbie-exam.ui.metric.downloaded_gib': '下载量（GiB）',
  'newbie-exam.ui.metric.seed_hours': '做种时长（小时）',
  'newbie-exam.ui.metric.uploads_count': '发布种子数',
  'newbie-exam.ui.metric.bonus': '魔力值',
  'newbie-exam.ui.metric.ratio': '分享率',
  'newbie-exam.ui.state.active': '进行中',
  'newbie-exam.ui.state.passed': '已通过',
  'newbie-exam.ui.state.failed': '未通过',
  'newbie-exam.ui.state.failed_restricted': '未通过，已禁止下载',
  'newbie-exam.ui.state.failed_banned': '未通过，已封禁',
  'newbie-exam.ui.state.remedied': '已补救',
  'newbie-exam.ui.state.exempted': '已豁免',
  'newbie-exam.ui.fail.none': '未通过不处理',
  'newbie-exam.ui.fail.restrict': '未通过将被禁止下载',
  'newbie-exam.ui.fail.ban': '未通过将被封禁',
  'newbie-exam.ui.remedy': '禁止下载后 {n} 天内完成全部目标可自动恢复。',
  'newbie-exam.ui.reward': '通过奖励 {n} 魔力值。',
  'newbie-exam.ui.details': '查看详情',
  'newbie-exam.ui.none': '没有考核。',
  'newbie-exam.ui.assign': '分配考核',
  'newbie-exam.ui.extend': '延期（天）',
  'newbie-exam.ui.exempt': '豁免',
  'newbie-exam.ui.done': '已处理',
  'newbie-exam.already_assigned': '该用户已有考核',
  'newbie-exam.not_assigned': '该用户没有进行中的考核',
  'newbie-exam.restricted': '新手考核通过前不能发邀请',
});
registerMessages('en', {
  'newbie-exam.ui.title': 'Newbie exam',
  'newbie-exam.ui.stage': 'Stage {n} · due {at}',
  'newbie-exam.ui.metric.uploaded_gib': 'Uploaded (GiB)',
  'newbie-exam.ui.metric.downloaded_gib': 'Downloaded (GiB)',
  'newbie-exam.ui.metric.seed_hours': 'Seeding time (hours)',
  'newbie-exam.ui.metric.uploads_count': 'Torrents uploaded',
  'newbie-exam.ui.metric.bonus': 'Bonus points',
  'newbie-exam.ui.metric.ratio': 'Ratio',
  'newbie-exam.ui.state.active': 'In progress',
  'newbie-exam.ui.state.passed': 'Passed',
  'newbie-exam.ui.state.failed': 'Failed',
  'newbie-exam.ui.state.failed_restricted': 'Failed, downloads disabled',
  'newbie-exam.ui.state.failed_banned': 'Failed, banned',
  'newbie-exam.ui.state.remedied': 'Remedied',
  'newbie-exam.ui.state.exempted': 'Exempted',
  'newbie-exam.ui.fail.none': 'No penalty for failing',
  'newbie-exam.ui.fail.restrict': 'Failing disables downloads',
  'newbie-exam.ui.fail.ban': 'Failing bans the account',
  'newbie-exam.ui.remedy': 'Completing every goal within {n} days of the restriction lifts it.',
  'newbie-exam.ui.reward': 'Passing earns {n} bonus points.',
  'newbie-exam.ui.details': 'Details',
  'newbie-exam.ui.none': 'No exam.',
  'newbie-exam.ui.assign': 'Assign exam',
  'newbie-exam.ui.extend': 'Extend (days)',
  'newbie-exam.ui.exempt': 'Exempt',
  'newbie-exam.ui.done': 'Done',
  'newbie-exam.already_assigned': 'The user already has an exam',
  'newbie-exam.not_assigned': 'The user has no open exam',
  'newbie-exam.restricted': 'Members cannot send invites before passing the newbie exam',
});

interface Goal {
  metric: string;
  target: number;
  current: number | null;
  met: boolean;
}
interface StageP {
  index: number;
  deadline: number;
  passed: boolean;
  goals: Goal[];
}
interface Status {
  exam: string;
  state: string;
  current_stage: number;
  action_on_fail: string;
  remedy_days: number;
  reward_bonus: number;
  stages: StageP[];
}

const fmt = (v: number | null) => (v == null ? '∞' : Number.isInteger(v) ? String(v) : v.toFixed(2));

function Goals({ api, stage }: { api: PluginApi; stage: StageP }) {
  return (
    <ul className="exam-goals">
      {stage.goals.map((g) => (
        <li key={g.metric}>
          {api.t(`newbie-exam.ui.metric.${g.metric}`)}：{fmt(g.current)} / {fmt(g.target)} {g.met && <Badge tone="success">✓</Badge>}
          <Progress value={g.current == null ? 1 : Math.min(1, g.target > 0 ? g.current / g.target : 1)} />
        </li>
      ))}
    </ul>
  );
}

function useStatus(api: PluginApi, path: `/api/${string}`) {
  const [s, setS] = useState<Status | null | undefined>(undefined);
  const load = async () => {
    const r = await api.fetch(path);
    setS(r.status === 204 ? null : ((await r.json()) as Status | null));
  };
  useEffect(() => {
    void load().catch(() => setS(null));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path]);
  return [s, load] as const;
}

function HomeWidget({ api }: SlotProps) {
  const [s] = useStatus(api, '/api/p/newbie-exam/v1/me');
  if (!s || s.state !== 'active') return null;
  const st = s.stages[s.current_stage];
  if (!st) return null;
  return (
    <Card title={s.exam}>
      <p className="small">
        {api.t('newbie-exam.ui.stage', { n: st.index + 1, at: new Date(st.deadline).toLocaleString() })} · {api.t(`newbie-exam.ui.fail.${s.action_on_fail}`)}
      </p>
      <Goals api={api} stage={st} />
      <Button variant="ghost" onClick={() => api.navigate('/p/newbie-exam/')}>
        {api.t('newbie-exam.ui.details')}
      </Button>
    </Card>
  );
}

function Details({ api, s }: { api: PluginApi; s: Status }) {
  return (
    <>
      <p>
        <Badge tone={s.state === 'passed' || s.state === 'remedied' || s.state === 'exempted' ? 'success' : s.state === 'active' ? 'primary' : 'danger'}>
          {api.t(`newbie-exam.ui.state.${s.state}`)}
        </Badge>{' '}
        {api.t(`newbie-exam.ui.fail.${s.action_on_fail}`)}
      </p>
      {s.action_on_fail === 'restrict' && s.remedy_days > 0 && <p className="pt-muted small">{api.t('newbie-exam.ui.remedy', { n: s.remedy_days })}</p>}
      {s.reward_bonus > 0 && <p className="pt-muted small">{api.t('newbie-exam.ui.reward', { n: s.reward_bonus })}</p>}
      {s.stages.map((st) => (
        <div key={st.index}>
          <h3>
            {api.t('newbie-exam.ui.stage', { n: st.index + 1, at: new Date(st.deadline).toLocaleString() })} {st.passed && <Badge tone="success">✓</Badge>}
          </h3>
          <Goals api={api} stage={st} />
        </div>
      ))}
    </>
  );
}

function MyPage({ api }: { api: PluginApi }) {
  const [s] = useStatus(api, '/api/p/newbie-exam/v1/me');
  if (s === undefined) return null;
  return <Card title={api.t('newbie-exam.ui.title')}>{s ? <Details api={api} s={s} /> : <p className="pt-muted">{api.t('newbie-exam.ui.none')}</p>}</Card>;
}

function UserTab({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const uid = ctx.profile.id;
  const [s, reload] = useStatus(api, `/api/p/newbie-exam/v1/admin/users/${uid}`);
  const [days, setDays] = useState('7');
  const act = async (path: string, body: Record<string, unknown>) => {
    try {
      const r = await api.json<{ results?: { error?: string }[] }>(`/api/p/newbie-exam/v1/admin/${path}`, { json: body });
      const e = r?.results?.[0]?.error;
      api.toast(e ? api.t(e) : api.t('newbie-exam.ui.done'), e ? 'error' : 'success');
      await reload();
    } catch (err) {
      api.toast(err instanceof ApiError ? api.t(err.code) : String(err), 'error');
    }
  };
  if (s === undefined) return null;
  return (
    <Card title={api.t('newbie-exam.ui.title')}>
      {s ? <Details api={api} s={s} /> : <p className="pt-muted">{api.t('newbie-exam.ui.none')}</p>}
      <div className="searchbar">
        <Button onClick={() => void act('assign', { user_ids: [uid] })}>{api.t('newbie-exam.ui.assign')}</Button>
        <Input name="exam_days" type="number" min={1} max={365} value={days} onChange={(e) => setDays(e.target.value)} />
        <Button onClick={() => void act(`users/${uid}/extend`, { days: Number(days) })}>{api.t('newbie-exam.ui.extend')}</Button>
        <Button variant="danger" onClick={() => void act(`users/${uid}/exempt`, { reason: 'admin' })}>
          {api.t('newbie-exam.ui.exempt')}
        </Button>
      </div>
    </Card>
  );
}

export default {
  slots: {
    'home.widgets': { component: HomeWidget, order: 5 },
    'admin.user.tabs': { component: UserTab, order: 45, title_term: 'newbie-exam.name' },
  },
  routes: [{ path: '/p/newbie-exam/*', component: MyPage }],
} satisfies PluginFrontend;
