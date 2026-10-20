// lottery 前端：幸运转盘（canvas 转盘、概率公示、最近中奖、我的记录）、首页小组件、后台（开关与参数、奖品与概率、统计）。
import { useCallback, useEffect, useRef, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { formatBytes, registerMessages } from '@pt-studio/terms';
import { Button, Checkbox, Field, Input, Kpis, NavItem as Nav, Note, PageHead, Panel, PLink, Select, SideLayout, SkeletonRows, Status, Switch, Table } from '@pt-studio/ui-kit';


registerMessages('zh-CN', {
  'lottery.ui.nav': '抽奖',
  'lottery.ui.time': '时间',
  'lottery.ui.result': '结果',
  'lottery.ui.spent': '消耗',
  'lottery.ui.again': '再抽一次',
  'lottery.ui.won_by': '{user} 抽中 {prize}',
  'lottery.ui.title': '幸运转盘',
  'lottery.ui.subtitle': '转动转盘，赢取魔力值、上传量与 VIP',
  'lottery.ui.disabled': '幸运转盘暂未开放',
  'lottery.ui.spin': '抽奖',
  'lottery.ui.spinning': '转动中…',
  'lottery.ui.cost': '每次消耗 {n} 魔力值',
  'lottery.ui.free': '免费抽奖',
  'lottery.ui.today': '今日已抽 {n} 次',
  'lottery.ui.today_limit': '今日已抽 {n} / {limit} 次',
  'lottery.ui.not_eligible': '注册满 {n} 天后才能抽奖',
  'lottery.ui.won': '恭喜！你抽中了 {prize}',
  'lottery.ui.lost': '谢谢参与，再接再厉！',
  'lottery.ui.chances': '奖品与概率',
  'lottery.ui.prize': '奖品',
  'lottery.ui.chance': '概率',
  'lottery.ui.recent': '最近中奖',
  'lottery.ui.mine': '我的记录',
  'lottery.ui.none': '谢谢参与',
  'lottery.ui.no_records': '暂无记录',
  'lottery.ui.k.bonus': '{n} 魔力值',
  'lottery.ui.k.upload': '{n} 上传量',
  'lottery.ui.k.vip_days': '{n} 天 VIP',
  'lottery.ui.k.vip_permanent': '永久 VIP',
  'lottery.ui.kind.bonus': '魔力值',
  'lottery.ui.kind.upload': '上传量（GiB）',
  'lottery.ui.kind.vip_days': 'VIP 天数',
  'lottery.ui.kind.vip_permanent': '永久 VIP',
  'lottery.ui.widget': '幸运转盘',
  'lottery.ui.widget_go': '去抽奖 →',
  'lottery.ui.admin': '抽奖管理',
  'lottery.ui.settings': '开关与参数',
  'lottery.ui.enabled': '开启幸运转盘',
  'lottery.ui.spin_cost': '每次消耗魔力值（0 为免费）',
  'lottery.ui.daily_limit': '每人每天次数（0 为不限）',
  'lottery.ui.min_days': '注册满多少天才能抽奖',
  'lottery.ui.save': '保存',
  'lottery.ui.saved': '已保存',
  'lottery.ui.prizes': '奖品与概率',
  'lottery.ui.prizes_hint': '概率按百分比填写，最多 4 位小数；启用奖品的概率之和不超过 100%，剩余部分为“谢谢参与”。库存留空为不限，抽完后自动从转盘移除。',
  'lottery.ui.kind': '类型',
  'lottery.ui.amount': '数量',
  'lottery.ui.label': '显示名称（可选）',
  'lottery.ui.chance_pct': '概率 %',
  'lottery.ui.stock': '库存',
  'lottery.ui.unlimited': '不限',
  'lottery.ui.active': '启用',
  'lottery.ui.remove': '删除',
  'lottery.ui.add': '添加奖品',
  'lottery.ui.total': '已分配 {pct}%，谢谢参与 {rest}%',
  'lottery.ui.over': '概率之和超过 100%',
  'lottery.ui.stats': '今日统计',
  'lottery.ui.s_spins': '抽奖次数',
  'lottery.ui.s_users': '参与人数',
  'lottery.ui.s_cost': '消耗魔力值',
  'lottery.ui.s_wins': '中奖次数',
  'lottery.ui.last30': '近 30 天发放',
  'lottery.disabled': '幸运转盘暂未开放',
  'lottery.not_eligible': '账号注册时间不足，暂不能抽奖',
  'lottery.limit_reached': '今天的抽奖次数已用完',
  'lottery.insufficient_funds': '魔力值不足',
  'lottery.grant_failed': '奖品发放失败，本次消耗已退回',
  'lottery.unavailable': '奖品暂不可用',
});
registerMessages('en', {
  'lottery.ui.nav': 'Lottery',
  'lottery.ui.time': 'Time',
  'lottery.ui.result': 'Result',
  'lottery.ui.spent': 'Cost',
  'lottery.ui.again': 'Spin again',
  'lottery.ui.won_by': '{user} won {prize}',
  'lottery.ui.title': 'Prize wheel',
  'lottery.ui.subtitle': 'Spin to win bonus, upload credit and VIP',
  'lottery.ui.disabled': 'The prize wheel is not open',
  'lottery.ui.spin': 'Spin',
  'lottery.ui.spinning': 'Spinning…',
  'lottery.ui.cost': '{n} bonus per spin',
  'lottery.ui.free': 'Free spins',
  'lottery.ui.today': '{n} spins today',
  'lottery.ui.today_limit': '{n} / {limit} spins today',
  'lottery.ui.not_eligible': 'Accounts can spin after {n} days',
  'lottery.ui.won': 'Congratulations! You won {prize}',
  'lottery.ui.lost': 'No prize this time. Good luck next spin!',
  'lottery.ui.chances': 'Prizes and odds',
  'lottery.ui.prize': 'Prize',
  'lottery.ui.chance': 'Chance',
  'lottery.ui.recent': 'Recent winners',
  'lottery.ui.mine': 'My spins',
  'lottery.ui.none': 'No prize',
  'lottery.ui.no_records': 'Nothing yet',
  'lottery.ui.k.bonus': '{n} bonus',
  'lottery.ui.k.upload': '{n} upload',
  'lottery.ui.k.vip_days': '{n} days VIP',
  'lottery.ui.k.vip_permanent': 'Permanent VIP',
  'lottery.ui.kind.bonus': 'Bonus',
  'lottery.ui.kind.upload': 'Upload (GiB)',
  'lottery.ui.kind.vip_days': 'VIP days',
  'lottery.ui.kind.vip_permanent': 'Permanent VIP',
  'lottery.ui.widget': 'Prize wheel',
  'lottery.ui.widget_go': 'Spin →',
  'lottery.ui.admin': 'Lottery admin',
  'lottery.ui.settings': 'Switch and limits',
  'lottery.ui.enabled': 'Enable the prize wheel',
  'lottery.ui.spin_cost': 'Bonus per spin (0 = free)',
  'lottery.ui.daily_limit': 'Spins per user per day (0 = unlimited)',
  'lottery.ui.min_days': 'Minimum account age (days)',
  'lottery.ui.save': 'Save',
  'lottery.ui.saved': 'Saved',
  'lottery.ui.prizes': 'Prizes and odds',
  'lottery.ui.prizes_hint': 'Chances are percentages with up to 4 decimals; active prizes may add up to at most 100%, the rest is "no prize". Leave stock empty for unlimited; prizes that run out leave the wheel.',
  'lottery.ui.kind': 'Kind',
  'lottery.ui.amount': 'Amount',
  'lottery.ui.label': 'Label (optional)',
  'lottery.ui.chance_pct': 'Chance %',
  'lottery.ui.stock': 'Stock',
  'lottery.ui.unlimited': 'Unlimited',
  'lottery.ui.active': 'Active',
  'lottery.ui.remove': 'Remove',
  'lottery.ui.add': 'Add prize',
  'lottery.ui.total': '{pct}% assigned, {rest}% no prize',
  'lottery.ui.over': 'Chances add up to more than 100%',
  'lottery.ui.stats': 'Today',
  'lottery.ui.s_spins': 'Spins',
  'lottery.ui.s_users': 'Players',
  'lottery.ui.s_cost': 'Bonus spent',
  'lottery.ui.s_wins': 'Wins',
  'lottery.ui.last30': 'Granted in the last 30 days',
  'lottery.disabled': 'The prize wheel is not open',
  'lottery.not_eligible': 'Your account is too new to spin',
  'lottery.limit_reached': 'No spins left today',
  'lottery.insufficient_funds': 'Not enough bonus',
  'lottery.grant_failed': 'The prize could not be granted; your bonus was refunded',
  'lottery.unavailable': 'Prize unavailable',
});


type Kind = 'bonus' | 'upload' | 'vip_days' | 'vip_permanent';
interface Prize {
  id: number;
  kind: Kind;
  amount: number;
  label: string;
  chance_ppm: number;
}
interface Win {
  kind: Kind;
  amount: number;
  username: string | null;
  at: number;
}
interface MySpin {
  id: number;
  cost: number;
  prize_id: number | null;
  kind: Kind | null;
  amount: number;
  at: number;
}
interface Wheel {
  enabled: boolean;
  spin_cost: number;
  daily_limit: number;
  min_account_days: number;
  prizes: Prize[];
  none_chance_ppm: number;
  recent_wins: Win[];
  spins_today: number;
  eligible: boolean;
  mine: MySpin[];
}

const errText = (api: PluginApi, e: unknown) => (e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e));
const pct = (ppm: number) => `${Number((ppm / 10_000).toFixed(4))}%`;
const when = (ms: number) => new Date(ms).toLocaleString(undefined, { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });

/** 上传量：整 GiB / TiB 去掉小数（10 GiB 而不是 10.00 GiB） */
function shortBytes(b: number): string {
  for (const [unit, size] of [['TiB', 1024 ** 4], ['GiB', 1024 ** 3], ['MiB', 1024 ** 2]] as const) {
    if (b >= size) return `${Number((b / size).toFixed(2))} ${unit}`;
  }
  return formatBytes(b);
}

function prizeName(api: PluginApi, kind: Kind | null, amount: number, label?: string): string {
  if (label) return label;
  if (!kind) return api.t('lottery.ui.none');
  const n = kind === 'upload' ? shortBytes(amount) : String(amount);
  return api.t(`lottery.ui.k.${kind}`, { n });
}

/** 扇区底色：深而沉的矿物色，白字可读；“谢谢参与”固定灰 */
const COLORS = ['#9a7428', '#3f6fb2', '#19735a', '#b2402f', '#5a6a3a', '#6a3a6a', '#20604c', '#7a4f2a'];
const NONE = '#7a8590';

interface Slice {
  key: string;
  prizeId: number | null;
  text: string;
  color: string;
}

const SIZE = 840;
const R = 400;

/** canvas 转盘：rot 为弧度，第 i 个扇区中心位于 i/N 圈（指针在正上方） */
function drawWheel(cv: HTMLCanvasElement, slices: Slice[], rot: number, hub: string) {
  const cx = cv.getContext('2d');
  if (!cx) return;
  const n = Math.max(slices.length, 1);
  const font = getComputedStyle(cv).fontFamily || 'sans-serif';
  cx.clearRect(0, 0, SIZE, SIZE);
  cx.save();
  cx.translate(SIZE / 2, SIZE / 2);
  cx.rotate(rot);
  slices.forEach((s, i) => {
    const a0 = (i / n) * Math.PI * 2 - Math.PI / 2 - Math.PI / n;
    const a1 = a0 + (Math.PI * 2) / n;
    cx.beginPath();
    cx.moveTo(0, 0);
    cx.arc(0, 0, R, a0, a1);
    cx.closePath();
    cx.fillStyle = s.color;
    cx.globalAlpha = i % 2 ? 0.86 : 1;
    cx.fill();
    cx.globalAlpha = 1;
    cx.strokeStyle = 'rgba(255,240,210,.7)';
    cx.lineWidth = 3;
    cx.stroke();
    cx.save();
    cx.rotate((a0 + a1) / 2);
    cx.fillStyle = '#fff8e8';
    cx.font = `600 ${n > 10 ? 24 : 30}px ${font}`;
    cx.textAlign = 'right';
    cx.fillText(s.text.length > 10 ? `${s.text.slice(0, 9)}…` : s.text, R - 34, 10);
    cx.restore();
  });
  cx.beginPath();
  cx.arc(0, 0, R, 0, Math.PI * 2);
  cx.strokeStyle = '#d9b56a';
  cx.lineWidth = 10;
  cx.stroke();
  for (let k = 0; k < 48; k++) {
    cx.save();
    cx.rotate((k / 48) * Math.PI * 2);
    cx.beginPath();
    cx.arc(0, -R + 2, 5, 0, Math.PI * 2);
    cx.fillStyle = k % 2 ? '#fff4d6' : '#d9b56a';
    cx.fill();
    cx.restore();
  }
  cx.restore();
  // 轮毂不随转盘旋转
  cx.save();
  cx.translate(SIZE / 2, SIZE / 2);
  cx.beginPath();
  cx.arc(0, 0, 78, 0, Math.PI * 2);
  cx.fillStyle = '#17212a';
  cx.fill();
  cx.strokeStyle = '#d9b56a';
  cx.lineWidth = 6;
  cx.stroke();
  cx.fillStyle = '#d9b56a';
  cx.font = `italic 700 40px "Cormorant Garamond", serif`;
  cx.textAlign = 'center';
  cx.fillText(hub, 0, 14);
  cx.restore();
}

function WheelPage({ api }: { api: PluginApi }) {
  const [w, setW] = useState<Wheel | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [spinning, setSpinning] = useState(false);
  const [result, setResult] = useState<{ win: boolean; text: string } | null>(null);
  const cv = useRef<HTMLCanvasElement | null>(null);
  const rot = useRef(0);
  const raf = useRef(0);
  const load = useCallback(
    () =>
      api
        .json<Wheel>('/api/p/lottery/v1/wheel')
        .then((r) => {
          setW(r);
          setErr(null);
        })
        .catch((e: unknown) => setErr(errText(api, e))),
    [api],
  );
  useEffect(() => {
    void load();
    return () => cancelAnimationFrame(raf.current);
  }, [load]);
  const slices: Slice[] = (w?.prizes ?? []).map((p, i) => ({ key: `p${p.id}`, prizeId: p.id, text: prizeName(api, p.kind, p.amount, p.label), color: COLORS[i % COLORS.length] ?? NONE }));
  if (w && (w.none_chance_ppm > 0 || slices.length === 0)) {
    // “谢谢参与”插在中间，与中奖扇区交错，转盘更均衡
    slices.splice(Math.ceil(slices.length / 2), 0, { key: 'none', prizeId: null, text: api.t('lottery.ui.none'), color: NONE });
  }
  const sig = slices.map((s) => s.key + s.text).join('|');
  useEffect(() => {
    if (cv.current && !spinning) drawWheel(cv.current, slices, rot.current, 'Spin');
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sig, spinning]);
  if (err && !w) return <Note tone="bad">{err}</Note>;
  if (!w) return <SkeletonRows n={6} />;
  const limited = w.daily_limit > 0 && w.spins_today >= w.daily_limit;
  const canSpin = w.enabled && w.eligible && !limited && !spinning;
  const spin = async () => {
    if (!canSpin) return;
    setSpinning(true);
    setResult(null);
    try {
      const r = await api.json<{ prize_id: number | null; kind: Kind | null; amount: number }>('/api/p/lottery/v1/wheel/spin', { json: {} });
      const n = slices.length;
      const idx = Math.max(
        slices.findIndex((s) => s.prizeId === r.prize_id),
        0,
      );
      const seg = (Math.PI * 2) / n;
      const jitter = (Math.random() - 0.5) * seg * 0.6;
      const start = rot.current % (Math.PI * 2);
      const target = Math.PI * 2 * 6 - idx * seg + jitter;
      const t0 = performance.now();
      const dur = matchMedia('(prefers-reduced-motion: reduce)').matches ? 1 : 4200;
      const label = slices[idx]?.text ?? '';
      const step = (t: number) => {
        const k = Math.min(1, (t - t0) / dur);
        rot.current = start + (target - start) * (1 - Math.pow(1 - k, 4));
        if (cv.current) drawWheel(cv.current, slices, rot.current, 'Spin');
        if (k < 1) raf.current = requestAnimationFrame(step);
        else {
          setSpinning(false);
          setResult(r.prize_id === null ? { win: false, text: api.t('lottery.ui.lost') } : { win: true, text: api.t('lottery.ui.won', { prize: label }) });
          void load();
        }
      };
      raf.current = requestAnimationFrame(step);
    } catch (e) {
      setSpinning(false);
      api.toast(errText(api, e), 'error');
      void load();
    }
  };
  const costText = w.spin_cost > 0 ? api.t('lottery.ui.cost', { n: w.spin_cost }) : api.t('lottery.ui.free');
  const todayText = w.daily_limit > 0 ? api.t('lottery.ui.today_limit', { n: w.spins_today, limit: w.daily_limit }) : api.t('lottery.ui.today', { n: w.spins_today });
  return (
    <div className="stack">
      <PageHead title={api.t('lottery.ui.title')} lede={`${api.t('lottery.ui.subtitle')} · ${costText} · ${todayText}`} />
      {!w.enabled ? <Note tone="warn">{api.t('lottery.ui.disabled')}</Note> : !w.eligible ? <Note tone="warn">{api.t('lottery.ui.not_eligible', { n: w.min_account_days })}</Note> : null}
      <SideLayout
        width={360}
        side={
          <>
            <Panel title={api.t('lottery.ui.chances')} flush>
              <Table
                rows={[...w.prizes.map((p, i) => ({ k: `p${p.id}`, name: prizeName(api, p.kind, p.amount, p.label), ppm: p.chance_ppm, color: COLORS[i % COLORS.length] })), ...(w.none_chance_ppm > 0 ? [{ k: 'none', name: api.t('lottery.ui.none'), ppm: w.none_chance_ppm, color: NONE }] : [])]}
                rowKey={(r) => r.k}
                columns={[
                  {
                    key: 'n',
                    header: api.t('lottery.ui.prize'),
                    render: (r) => (
                      <span className="row lw-name">
                        <i style={{ background: r.color }} />
                        {r.name}
                      </span>
                    ),
                  },
                  { key: 'p', header: api.t('lottery.ui.chance'), right: true, render: (r) => <span className="num">{pct(r.ppm)}</span> },
                ]}
              />
            </Panel>
            <Panel title={api.t('lottery.ui.recent')} flush>
              {w.recent_wins.length === 0 ? (
                <p className="pad small muted">{api.t('lottery.ui.no_records')}</p>
              ) : (
                <div className="side-list">
                  {w.recent_wins.map((x, i) => (
                    <div key={i}>
                      <span className="rk">·</span>
                      <span className="nm">
                        {x.username ?? '—'} · <b>{prizeName(api, x.kind, x.amount)}</b>
                      </span>
                      <span className="v">{when(x.at)}</span>
                    </div>
                  ))}
                </div>
              )}
            </Panel>
            <Panel title={api.t('lottery.ui.mine')} flush>
              <Table
                rows={w.mine}
                rowKey={(x) => x.id}
                empty={api.t('lottery.ui.no_records')}
                columns={[
                  { key: 't', header: api.t('lottery.ui.time'), render: (x) => <span className="small muted num nowrap">{when(x.at)}</span> },
                  { key: 'r', header: api.t('lottery.ui.result'), render: (x) => (x.prize_id === null ? <span className="muted">{api.t('lottery.ui.none')}</span> : <b className="gold">{prizeName(api, x.kind, x.amount)}</b>) },
                  { key: 'c', header: api.t('lottery.ui.spent'), right: true, render: (x) => <span className="num">{x.cost}</span> },
                ]}
              />
            </Panel>
          </>
        }
      >
        <section className="panel wheel-box" data-primary>
          <span className="wheel-pin" />
          <canvas ref={cv} width={SIZE} height={SIZE} aria-label={api.t('lottery.ui.title')} />
          <div className="row">
            <Button variant="gold" size="lg" disabled={!canSpin} onClick={() => void spin()}>
              {spinning ? api.t('lottery.ui.spinning') : `${result ? api.t('lottery.ui.again') : api.t('lottery.ui.spin')}${w.spin_cost > 0 ? ` · ${w.spin_cost}` : ''}`}
            </Button>
          </div>
          <span className={result?.win ? 'small gold' : 'small'} role="status" aria-live="polite">
            {result?.text ?? ''}
          </span>
        </section>
      </SideLayout>
    </div>
  );
}

function NavLink({ api }: SlotProps) {
  return (
    <Nav to="/p/lottery/" navigate={api.navigate}>
      {api.t('lottery.ui.nav')}
    </Nav>
  );
}

function Widget({ api }: SlotProps) {
  const [w, setW] = useState<Wheel | null>(null);
  useEffect(() => {
    api
      .json<Wheel>('/api/p/lottery/v1/wheel')
      .then(setW)
      .catch(() => setW(null));
  }, [api]);
  if (!w?.enabled) return null;
  return (
    <Panel
      title={api.t('lottery.ui.widget')}
      more={
        <PLink to="/p/lottery/" navigate={api.navigate}>
          {api.t('lottery.ui.widget_go')}
        </PLink>
      }
    >
      <p className="small muted">
        {w.spin_cost > 0 ? api.t('lottery.ui.cost', { n: w.spin_cost }) : api.t('lottery.ui.free')} ·{' '}
        {w.daily_limit > 0 ? api.t('lottery.ui.today_limit', { n: w.spins_today, limit: w.daily_limit }) : api.t('lottery.ui.today', { n: w.spins_today })}
      </p>
    </Panel>
  );
}

// ---------------- 后台 ----------------

interface AdminPrize {
  id?: number;
  kind: Kind;
  amount: number;
  label: string;
  chance_ppm: number;
  stock: number | null;
  active: boolean;
}
interface AdminData {
  prizes: AdminPrize[];
  today: { spins: number; cost: number; wins: number; users: number };
  last_30_days: { kind: Kind; count: number; amount: number }[];
}
interface SettingsDoc {
  settings: { enabled?: boolean; spin_cost?: number; daily_limit?: number; min_account_days?: number };
  version: number;
}
/** 编辑中的行：数量与概率保留输入框原文，保存时再换算 */
interface Row {
  key: number;
  id?: number;
  kind: Kind;
  amount: string;
  label: string;
  chance: string;
  stock: string;
  active: boolean;
}

const GIB = 1024 ** 3;
const SETTINGS = '/api/kernel/v1/admin/plugins/lottery/settings';
const KINDS = ['bonus', 'upload', 'vip_days', 'vip_permanent'] as const;

const toRow = (p: AdminPrize, key: number): Row => ({
  key,
  id: p.id,
  kind: p.kind,
  amount: p.kind === 'upload' ? String(Number((p.amount / GIB).toFixed(3))) : p.kind === 'vip_permanent' ? '' : String(p.amount),
  label: p.label,
  chance: String(p.chance_ppm / 10_000),
  stock: p.stock === null ? '' : String(p.stock),
  active: p.active,
});

const ppmOf = (r: Row) => Math.round((Number(r.chance) || 0) * 10_000);

function Admin({ api }: { api: PluginApi }) {
  const [d, setD] = useState<AdminData | null>(null);
  const [rows, setRows] = useState<Row[]>([]);
  const [s, setS] = useState<SettingsDoc | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const nextKey = useRef(1000);
  const apply = (x: AdminData) => {
    setD(x);
    setRows(x.prizes.map((p, i) => toRow(p, i)));
  };
  const load = async () => {
    try {
      const [a, b] = await Promise.all([api.json<AdminData>('/api/p/lottery/v1/admin/wheel'), api.json<SettingsDoc>(SETTINGS)]);
      apply(a);
      setS(b);
      setErr(null);
    } catch (e) {
      setErr(errText(api, e));
    }
  };
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  if (err && !d) return <Note tone="bad">{err}</Note>;
  if (!d || !s) return <SkeletonRows n={6} />;
  const cfg = {
    enabled: s.settings.enabled ?? false,
    spin_cost: s.settings.spin_cost ?? 50,
    daily_limit: s.settings.daily_limit ?? 10,
    min_account_days: s.settings.min_account_days ?? 7,
  };
  const setCfg = (patch: Partial<typeof cfg>) => setS({ ...s, settings: { ...cfg, ...patch } });
  const saveSettings = async () => {
    setBusy(true);
    try {
      await api.json(SETTINGS, { method: 'PUT', json: { settings: cfg, version: s.version } });
      setS(await api.json<SettingsDoc>(SETTINGS));
      api.toast(api.t('lottery.ui.saved'), 'success');
    } catch (e) {
      api.toast(errText(api, e), 'error');
    } finally {
      setBusy(false);
    }
  };
  const total = rows.filter((r) => r.active).reduce((n, r) => n + ppmOf(r), 0);
  const upd = (key: number, patch: Partial<Row>) => setRows(rows.map((r) => (r.key === key ? { ...r, ...patch } : r)));
  const savePrizes = async () => {
    setBusy(true);
    try {
      const prizes = rows.map((r) => ({
        id: r.id,
        kind: r.kind,
        amount: r.kind === 'upload' ? Math.round((Number(r.amount) || 0) * GIB) : r.kind === 'vip_permanent' ? 0 : Math.round(Number(r.amount) || 0),
        label: r.label.trim(),
        chance_ppm: ppmOf(r),
        stock: r.stock.trim() === '' ? null : Math.max(0, Math.round(Number(r.stock) || 0)),
        active: r.active,
      }));
      apply(await api.json<AdminData>('/api/p/lottery/v1/admin/wheel/prizes', { method: 'PUT', json: { prizes } }));
      api.toast(api.t('lottery.ui.saved'), 'success');
    } catch (e) {
      api.toast(errText(api, e), 'error');
    } finally {
      setBusy(false);
    }
  };
  const over = total > 1_000_000;
  return (
    <div className="stack">
      <PageHead title={api.t('lottery.ui.admin')} />
      <Kpis
        items={[
          { label: api.t('lottery.ui.s_spins'), value: d.today.spins ?? 0, hint: api.t('lottery.ui.stats') },
          { label: api.t('lottery.ui.s_users'), value: d.today.users ?? 0 },
          { label: api.t('lottery.ui.s_cost'), value: d.today.cost ?? 0, tone: 'gold' },
          { label: api.t('lottery.ui.s_wins'), value: d.today.wins ?? 0 },
        ]}
      />
      <Panel
        title={api.t('lottery.ui.prizes')}
        sub={api.t('lottery.ui.prizes_hint')}
        flush
        primary
        actions={
          <>
            <Button size="sm" onClick={() => setRows([...rows, { key: nextKey.current++, kind: 'bonus', amount: '100', label: '', chance: '1', stock: '', active: true }])}>
              {api.t('lottery.ui.add')}
            </Button>
            <Button size="sm" variant="primary" disabled={busy || over} onClick={() => void savePrizes()}>
              {api.t('lottery.ui.save')}
            </Button>
          </>
        }
      >
        <Table
          rows={rows}
          rowKey={(r) => r.key}
          minWidth={820}
          columns={[
            {
              key: 'k',
              header: api.t('lottery.ui.kind'),
              render: (r) => <Select aria-label={api.t('lottery.ui.kind')} value={r.kind} onChange={(e) => upd(r.key, { kind: e.target.value as Kind })} options={KINDS.map((k) => ({ value: k, label: api.t(`lottery.ui.kind.${k}`) }))} />,
            },
            {
              key: 'a',
              header: api.t('lottery.ui.amount'),
              render: (r) => (
                <Input type="number" min={0} step={r.kind === 'upload' ? 'any' : 1} aria-label={api.t('lottery.ui.amount')} disabled={r.kind === 'vip_permanent'} value={r.amount} onChange={(e) => upd(r.key, { amount: e.target.value })} />
              ),
            },
            { key: 'l', header: api.t('lottery.ui.label'), render: (r) => <Input maxLength={40} aria-label={api.t('lottery.ui.label')} value={r.label} onChange={(e) => upd(r.key, { label: e.target.value })} /> },
            { key: 'c', header: api.t('lottery.ui.chance_pct'), render: (r) => <Input type="number" min={0} max={100} step="any" aria-label={api.t('lottery.ui.chance_pct')} value={r.chance} onChange={(e) => upd(r.key, { chance: e.target.value })} /> },
            { key: 's', header: api.t('lottery.ui.stock'), render: (r) => <Input type="number" min={0} aria-label={api.t('lottery.ui.stock')} placeholder={api.t('lottery.ui.unlimited')} value={r.stock} onChange={(e) => upd(r.key, { stock: e.target.value })} /> },
            { key: 'on', header: api.t('lottery.ui.active'), render: (r) => <Checkbox label="" checked={r.active} onChange={(v) => upd(r.key, { active: v })} /> },
            {
              key: 'x',
              header: '',
              right: true,
              render: (r) => (
                <Button size="sm" variant="ghost" onClick={() => setRows(rows.filter((x) => x.key !== r.key))}>
                  {api.t('lottery.ui.remove')}
                </Button>
              ),
            },
          ]}
        />
        <div className="toolbar">
          {over ? (
            <Status tone="bad">{api.t('lottery.ui.over')}</Status>
          ) : (
            <span className="small muted">{api.t('lottery.ui.total', { pct: Number((total / 10_000).toFixed(4)), rest: Number(((1_000_000 - total) / 10_000).toFixed(4)) })}</span>
          )}
        </div>
      </Panel>
      <Panel
        title={api.t('lottery.ui.settings')}
        actions={
          <Button size="sm" variant="primary" type="submit" form="lottery-settings" disabled={busy}>
            {api.t('lottery.ui.save')}
          </Button>
        }
      >
        <form
          id="lottery-settings"
          className="form"
          onSubmit={(e) => {
            e.preventDefault();
            void saveSettings();
          }}
        >
          <div className="row">
            <Switch label={api.t('lottery.ui.enabled')} checked={cfg.enabled} onChange={(v) => setCfg({ enabled: v })} />
            <b>{api.t('lottery.ui.enabled')}</b>
          </div>
          <div className="g3">
            <Field label={api.t('lottery.ui.spin_cost')}>
              <Input type="number" min={0} max={1_000_000} value={cfg.spin_cost} onChange={(e) => setCfg({ spin_cost: Number(e.target.value) })} />
            </Field>
            <Field label={api.t('lottery.ui.daily_limit')}>
              <Input type="number" min={0} max={1000} value={cfg.daily_limit} onChange={(e) => setCfg({ daily_limit: Number(e.target.value) })} />
            </Field>
            <Field label={api.t('lottery.ui.min_days')}>
              <Input type="number" min={0} max={365} value={cfg.min_account_days} onChange={(e) => setCfg({ min_account_days: Number(e.target.value) })} />
            </Field>
          </div>
        </form>
      </Panel>
      <Panel title={api.t('lottery.ui.last30')} flush>
        <Table
          rows={d.last_30_days}
          rowKey={(x) => x.kind}
          empty={api.t('lottery.ui.no_records')}
          columns={[
            { key: 'k', header: api.t('lottery.ui.kind'), render: (x) => api.t(`lottery.ui.kind.${x.kind}`) },
            { key: 'c', header: api.t('lottery.ui.s_wins'), right: true, render: (x) => <span className="num">{x.count}</span> },
            { key: 'a', header: api.t('lottery.ui.amount'), right: true, render: (x) => <span className="num">{x.kind === 'upload' ? shortBytes(x.amount) : x.kind === 'vip_permanent' ? '—' : x.amount}</span> },
          ]}
        />
      </Panel>
    </div>
  );
}

export default {
  slots: {
    'nav.main': { component: NavLink, order: 60 },
    'home.widgets': { component: Widget, order: 30 },
  },
  routes: [
    { path: '/p/lottery/*', component: WheelPage },
    { path: '/admin/p/lottery/*', component: Admin },
  ],
} satisfies PluginFrontend;
