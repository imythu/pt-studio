// checkin 前端：用户菜单的签到入口（未签到时一键签到并显示奖励）、首页小部件、签到页（/p/checkin/：连续天数、本月日历、排行）。
import { useCallback, useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Card, Stat, Table } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'checkin.ui.menu': '签到',
  'checkin.ui.done_menu': '已签到（{n} 天）',
  'checkin.ui.title': '每日签到',
  'checkin.ui.check_in': '签到',
  'checkin.ui.result': '签到成功：连续 {streak} 天，第 {rank} 位，获得 {total}',
  'checkin.ui.milestone': '（含连续 {days} 天奖励 {amount}）',
  'checkin.ui.streak': '连续天数',
  'checkin.ui.longest': '最长连续',
  'checkin.ui.total': '累计天数',
  'checkin.ui.next': '下次签到可得',
  'checkin.ui.calendar': '本月',
  'checkin.ui.leaderboard': '连续签到排行',
  'checkin.ui.user': '用户',
  'checkin.ui.done_today': '今天已签到',
});
registerMessages('en', {
  'checkin.ui.menu': 'Check in',
  'checkin.ui.done_menu': 'Checked in ({n} days)',
  'checkin.ui.title': 'Daily check-in',
  'checkin.ui.check_in': 'Check in',
  'checkin.ui.result': 'Checked in: {streak}-day streak, #{rank} today, earned {total}',
  'checkin.ui.milestone': ' (incl. {days}-day bonus {amount})',
  'checkin.ui.streak': 'Streak',
  'checkin.ui.longest': 'Longest',
  'checkin.ui.total': 'Total days',
  'checkin.ui.next': 'Next reward',
  'checkin.ui.calendar': 'This month',
  'checkin.ui.leaderboard': 'Streak leaderboard',
  'checkin.ui.user': 'User',
  'checkin.ui.done_today': 'Checked in today',
});

interface Status {
  date: string;
  checked_in_today: boolean;
  streak: number;
  longest: number;
  total_days: number;
  month_days: number[];
  next_reward: { daily: number; milestone: { days: number; amount: number } | null };
}

interface Result {
  streak: number;
  rank: number;
  reward: { total: number; milestone: number; milestone_days: number | null };
}

function resultText(api: PluginApi, r: Result): string {
  let s = api.t('checkin.ui.result', { streak: r.streak, rank: r.rank, total: r.reward.total });
  if (r.reward.milestone_days) s += api.t('checkin.ui.milestone', { days: r.reward.milestone_days, amount: r.reward.milestone });
  return s;
}

function errText(api: PluginApi, e: unknown): string {
  if (e instanceof ApiError) {
    const s = api.t(e.code);
    return s === e.code ? e.message : s;
  }
  return String(e);
}

function MenuItem({ api }: SlotProps) {
  const [st, setSt] = useState<Status | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  useEffect(() => {
    void api
      .json<Status>('/api/p/checkin/v1/me')
      .then(setSt)
      .catch(() => undefined);
  }, [api]);
  async function go() {
    try {
      const r = await api.json<Result>('/api/p/checkin/v1/check-ins', { method: 'POST', json: {} });
      setMsg(resultText(api, r));
      api.toast(resultText(api, r), 'success');
      setSt(st ? { ...st, checked_in_today: true, streak: r.streak } : st);
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  }
  if (!st) return null;
  if (st.checked_in_today)
    return (
      <a
        href="/p/checkin/"
        title={msg ?? undefined}
        onClick={(e) => {
          e.preventDefault();
          api.navigate('/p/checkin/');
        }}
      >
        {api.t('checkin.ui.done_menu', { n: st.streak })}
      </a>
    );
  return (
    <a
      href="/p/checkin/"
      onClick={(e) => {
        e.preventDefault();
        void go();
      }}
    >
      {api.t('checkin.ui.menu')}
    </a>
  );
}

/** 首页小部件：今日状态与一键签到 */
function Widget({ api }: SlotProps) {
  const [st, setSt] = useState<Status | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const load = useCallback(async () => setSt(await api.json<Status>('/api/p/checkin/v1/me')), [api]);
  useEffect(() => {
    if (api.user()) void load().catch(() => undefined);
  }, [api, load]);
  if (!api.user() || !st) return null;
  async function go() {
    try {
      setMsg(resultText(api, await api.json<Result>('/api/p/checkin/v1/check-ins', { method: 'POST', json: {} })));
      await load();
    } catch (e) {
      setMsg(errText(api, e));
    }
  }
  return (
    <Card title={api.t('checkin.ui.title')}>
      <div className="pt-row">
        <Stat label={api.t('checkin.ui.streak')} value={st.streak} />
        <Stat label={api.t('checkin.ui.next')} value={st.next_reward.daily + (st.next_reward.milestone?.amount ?? 0)} />
      </div>
      <Button variant="primary" disabled={st.checked_in_today} onClick={() => void go()}>
        {api.t(st.checked_in_today ? 'checkin.ui.done_today' : 'checkin.ui.check_in')}
      </Button>
      {msg && <p className="pt-muted">{msg}</p>}
    </Card>
  );
}

function Page({ api }: { api: PluginApi }) {
  const [st, setSt] = useState<Status | null>(null);
  const [board, setBoard] = useState<{ user_id: number; username: string; streak: number; total_days: number }[]>([]);
  const [msg, setMsg] = useState<string | null>(null);
  const load = useCallback(async () => {
    setSt(await api.json<Status>('/api/p/checkin/v1/me'));
    setBoard((await api.json<{ items: typeof board }>('/api/p/checkin/v1/leaderboard')).items);
  }, [api]);
  useEffect(() => {
    void load();
  }, [load]);
  async function go() {
    try {
      const r = await api.json<Result>('/api/p/checkin/v1/check-ins', { method: 'POST', json: {} });
      setMsg(resultText(api, r));
      await load();
    } catch (e) {
      setMsg(errText(api, e));
    }
  }
  if (!st) return null;
  const year = Number(st.date.slice(0, 4));
  const month = Number(st.date.slice(5, 7));
  const days = new Date(year, month, 0).getDate();
  return (
    <div className="pt-stack">
      <Card title={api.t('checkin.ui.title')}>
        <div className="pt-row">
          <Stat label={api.t('checkin.ui.streak')} value={st.streak} />
          <Stat label={api.t('checkin.ui.longest')} value={st.longest} />
          <Stat label={api.t('checkin.ui.total')} value={st.total_days} />
          <Stat label={api.t('checkin.ui.next')} value={st.next_reward.daily + (st.next_reward.milestone?.amount ?? 0)} />
        </div>
        <Button variant="primary" disabled={st.checked_in_today} onClick={() => void go()}>
          {api.t('checkin.ui.check_in')}
        </Button>
        {msg && <p data-testid="checkin-result">{msg}</p>}
      </Card>
      <Card title={api.t('checkin.ui.calendar')}>
        <div className="pt-calendar">
          {Array.from({ length: days }, (_, i) => i + 1).map((d) => (
            <span key={d} className={st.month_days.includes(d) ? 'pt-calendar-day done' : 'pt-calendar-day'}>
              {d}
            </span>
          ))}
        </div>
      </Card>
      <Card title={api.t('checkin.ui.leaderboard')}>
        <Table
          rows={board}
          rowKey={(r) => r.user_id}
          columns={[
            { key: 'u', header: api.t('checkin.ui.user'), render: (r) => r.username },
            { key: 's', header: api.t('checkin.ui.streak'), render: (r) => r.streak },
            { key: 't', header: api.t('checkin.ui.total'), render: (r) => r.total_days },
          ]}
        />
      </Card>
    </div>
  );
}

export default {
  slots: {
    'nav.user_menu': { component: MenuItem, order: 1 },
    'home.widgets': { component: Widget, order: 10 },
  },
  routes: [{ path: '/p/checkin/*', component: Page }],
} satisfies PluginFrontend;
