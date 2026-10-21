// checkin 前端：用户菜单的签到入口（未签到时一键签到并显示奖励）、首页小部件、签到页（/p/checkin/：连续天数、本月日历、排行）。
import { useCallback, useEffect, useState } from "react";
import { ApiError } from "@pt-studio/sdk";
import type { PluginApi, PluginFrontend, SlotProps } from "@pt-studio/slots";
import { registerMessages } from "@pt-studio/terms";
import {
  Button,
  NavItem,
  PageHead,
  Panel,
  PLink,
  SideLayout,
  SkeletonRows,
  Table,
} from "@pt-studio/ui-kit";

registerMessages("zh-CN", {
  "checkin.ui.menu": "签到",
  "checkin.ui.done_menu": "已签到（{n} 天）",
  "checkin.ui.title": "每日签到",
  "checkin.ui.check_in": "签到",
  "checkin.ui.result": "签到成功：连续 {streak} 天，第 {rank} 位，获得 {total}",
  "checkin.ui.milestone": "（含连续 {days} 天奖励 {amount}）",
  "checkin.ui.streak": "连续天数",
  "checkin.ui.longest": "最长连续",
  "checkin.ui.total": "累计天数",
  "checkin.ui.next": "下次签到可得",
  "checkin.ui.calendar": "本月",
  "checkin.ui.leaderboard": "连续签到排行",
  "checkin.ui.user": "用户",
  "checkin.ui.done_today": "今天已签到",
  "checkin.ui.streak_n": "已连续 {n} 天",
  "checkin.ui.detail": "签到详情",
  "checkin.ui.lede": "每天来一次，连续签到在里程碑日会有额外奖励。",
});
registerMessages("en", {
  "checkin.ui.menu": "Check in",
  "checkin.ui.done_menu": "Checked in ({n} days)",
  "checkin.ui.title": "Daily check-in",
  "checkin.ui.check_in": "Check in",
  "checkin.ui.result":
    "Checked in: {streak}-day streak, #{rank} today, earned {total}",
  "checkin.ui.milestone": " (incl. {days}-day bonus {amount})",
  "checkin.ui.streak": "Streak",
  "checkin.ui.longest": "Longest",
  "checkin.ui.total": "Total days",
  "checkin.ui.next": "Next reward",
  "checkin.ui.calendar": "This month",
  "checkin.ui.leaderboard": "Streak leaderboard",
  "checkin.ui.user": "User",
  "checkin.ui.done_today": "Checked in today",
  "checkin.ui.streak_n": "{n}-day streak",
  "checkin.ui.detail": "Details",
  "checkin.ui.lede": "Come by every day — streak milestones pay extra.",
});

interface Status {
  date: string;
  checked_in_today: boolean;
  streak: number;
  longest: number;
  total_days: number;
  month_days: number[];
  next_reward: {
    daily: number;
    milestone: { days: number; amount: number } | null;
  };
}

interface Result {
  streak: number;
  rank: number;
  reward: { total: number; milestone: number; milestone_days: number | null };
}

function resultText(api: PluginApi, r: Result): string {
  let s = api.t("checkin.ui.result", {
    streak: r.streak,
    rank: r.rank,
    total: r.reward.total,
  });
  if (r.reward.milestone_days)
    s += api.t("checkin.ui.milestone", {
      days: r.reward.milestone_days,
      amount: r.reward.milestone,
    });
  return s;
}

function errText(api: PluginApi, e: unknown): string {
  if (e instanceof ApiError) {
    const s = api.t(e.code);
    return s === e.code ? e.message : s;
  }
  return String(e);
}

/** 签到状态：与宿主首页共用查询缓存（菜单、首页组件同时挂载时只发一次请求） */
const myStatus = (api: PluginApi) =>
  api.queryClient.fetchQuery({ queryKey: ["checkin-me"], queryFn: () => api.json<Status>("/api/p/checkin/v1/me"), staleTime: 30_000 });

function MenuItem({ api }: SlotProps) {
  const [st, setSt] = useState<Status | null>(null);
  useEffect(() => {
    void myStatus(api)
      .then(setSt)
      .catch(() => undefined);
  }, [api]);
  if (!st) return null;
  return (
    <NavItem
      to="/p/checkin/"
      navigate={api.navigate}
      count={st.checked_in_today ? st.streak : "!"}
      alert={!st.checked_in_today}
    >
      {api.t("checkin.ui.menu")}
    </NavItem>
  );
}

const WEEK = ["一", "二", "三", "四", "五", "六", "日"];

/** 本月日历：周一开头；已签到的日子着色，今天描边 */
function Calendar({ st }: { st: Status }) {
  const year = Number(st.date.slice(0, 4));
  const month = Number(st.date.slice(5, 7));
  const today = Number(st.date.slice(8, 10));
  const days = new Date(year, month, 0).getDate();
  const lead = (new Date(year, month - 1, 1).getDay() + 6) % 7;
  return (
    <div className="cal">
      {WEEK.map((w) => (
        <span key={w} className="h">
          {w}
        </span>
      ))}
      {Array.from({ length: lead }, (_, i) => (
        <span key={`b${i}`} />
      ))}
      {Array.from({ length: days }, (_, i) => i + 1).map((d) => (
        <span
          key={d}
          className={
            [st.month_days.includes(d) ? "on" : "", d === today ? "today" : ""]
              .join(" ")
              .trim() || undefined
          }
        >
          {d}
        </span>
      ))}
    </div>
  );
}

/** 首页侧栏：本月签到日历（签到按钮在首页卷首） */
function Widget({ api }: SlotProps) {
  const [st, setSt] = useState<Status | null>(null);
  useEffect(() => {
    if (api.user())
      void myStatus(api)
        .then(setSt)
        .catch(() => undefined);
  }, [api]);
  if (!api.user() || !st) return null;
  return (
    <Panel
      title={api.t("checkin.ui.calendar")}
      sub={api.t("checkin.ui.streak_n", { n: st.streak })}
      more={
        <PLink to="/p/checkin/" navigate={api.navigate}>
          {api.t("checkin.ui.detail")}
        </PLink>
      }
    >
      <Calendar st={st} />
    </Panel>
  );
}

function Page({ api }: { api: PluginApi }) {
  const [st, setSt] = useState<Status | null>(null);
  const [board, setBoard] = useState<
    | {
        user_id: number;
        username: string;
        streak: number;
        total_days: number;
      }[]
    | null
  >(null);
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    setSt(await api.json<Status>("/api/p/checkin/v1/me"));
    setBoard(
      (
        await api.json<{ items: NonNullable<typeof board> }>(
          "/api/p/checkin/v1/leaderboard",
        )
      ).items,
    );
  }, [api]);
  useEffect(() => {
    void load().catch((e: unknown) => api.toast(errText(api, e), "error"));
  }, [api, load]);
  async function go() {
    setBusy(true);
    try {
      const r = await api.json<Result>("/api/p/checkin/v1/check-ins", {
        method: "POST",
        json: {},
      });
      setMsg(resultText(api, r));
      void api.queryClient.invalidateQueries({ queryKey: ["checkin-me"] });
      void api.queryClient.invalidateQueries({ queryKey: ["bonus-summary"] });
      api.toast(resultText(api, r), "success");
      await load();
    } catch (e) {
      setMsg(errText(api, e));
    } finally {
      setBusy(false);
    }
  }
  if (!st) return <SkeletonRows n={6} />;
  const next = st.next_reward.daily + (st.next_reward.milestone?.amount ?? 0);
  const me = api.user()?.id;
  return (
    <div className="stack">
      <PageHead
        title={api.t("checkin.ui.title")}
        lede={api.t("checkin.ui.lede")}
      />
      <SideLayout
        left
        width={340}
        sideFirst
        side={
          <>
            <Panel primary>
              <div className="stack-s">
                <span className="eyebrow">{api.t("checkin.ui.streak")}</span>
                <span className="big-n num">{st.streak}</span>
                <Button
                  variant="primary"
                  size="lg"
                  block
                  disabled={st.checked_in_today || busy}
                  onClick={() => void go()}
                >
                  {api.t(
                    st.checked_in_today
                      ? "checkin.ui.done_today"
                      : "checkin.ui.check_in",
                  )}
                </Button>
                {msg && (
                  <p className="small" data-testid="checkin-result">
                    {msg}
                  </p>
                )}
                <dl className="ledger">
                  <dt>{api.t("checkin.ui.longest")}</dt>
                  <dd className="num">{st.longest}</dd>
                  <dt>{api.t("checkin.ui.total")}</dt>
                  <dd className="num">{st.total_days}</dd>
                  <dt>{api.t("checkin.ui.next")}</dt>
                  <dd className="num gold">{next}</dd>
                </dl>
                {st.next_reward.milestone && (
                  <p className="tiny muted">
                    {api.t("checkin.ui.milestone", {
                      days: st.next_reward.milestone.days,
                      amount: st.next_reward.milestone.amount,
                    })}
                  </p>
                )}
              </div>
            </Panel>
            <Panel
              title={api.t("checkin.ui.calendar")}
              sub={st.date.slice(0, 7)}
            >
              <Calendar st={st} />
            </Panel>
          </>
        }
      >
        <Panel title={api.t("checkin.ui.leaderboard")} flush>
          {board === null ? (
            <SkeletonRows n={5} />
          ) : (
            <Table
              rows={board}
              rowKey={(r) => r.user_id}
              empty="—"
              selected={me ?? null}
              columns={[
                {
                  key: "i",
                  header: "#",
                  render: (r) => (
                    <span className="num muted">{board.indexOf(r) + 1}</span>
                  ),
                },
                {
                  key: "u",
                  header: api.t("checkin.ui.user"),
                  render: (r) => (
                    <PLink to={`/users/${r.user_id}`} navigate={api.navigate}>
                      {r.username}
                    </PLink>
                  ),
                },
                {
                  key: "s",
                  header: api.t("checkin.ui.streak"),
                  right: true,
                  render: (r) => <span className="num">{r.streak}</span>,
                },
                {
                  key: "t",
                  header: api.t("checkin.ui.total"),
                  right: true,
                  render: (r) => (
                    <span className="num muted">{r.total_days}</span>
                  ),
                },
              ]}
            />
          )}
        </Panel>
      </SideLayout>
    </div>
  );
}

export default {
  slots: {
    "nav.user_menu": { component: MenuItem, order: 1 },
    "home.widgets": { component: Widget, order: 10 },
  },
  routes: [{ path: "/p/checkin/*", component: Page }],
} satisfies PluginFrontend;
