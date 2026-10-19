// lottery 前端：幸运转盘（转盘、概率公示、最近中奖、我的记录）、首页小组件、后台（开关与参数、奖品与概率、统计）。
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { formatBytes, registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Empty, Field, Input, Stat, Table } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'lottery.ui.nav': '抽奖',
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
const when = (ms: number) => new Date(ms).toLocaleString();

/** 上传量：整 GiB / TiB 去掉小数（10 GiB 而不是 10.00 GiB） */
function shortBytes(b: number): string {
  for (const [unit, size] of [['TiB', 1024 ** 4], ['GiB', 1024 ** 3], ['MiB', 1024 ** 2]] as const) {
    if (b >= size) return `${Number((b / size).toFixed(2))} ${unit}`;
  }
  return formatBytes(b);
}

/** 扇区文字最多两行：优先在空格处断开 */
function lines(text: string): string[] {
  if (text.length <= 7) return [text];
  const sp = text.lastIndexOf(' ', Math.ceil(text.length / 2) + 2);
  if (sp > 0) return [text.slice(0, sp), text.slice(sp + 1)].map((x) => x.slice(0, 9));
  return [text.slice(0, 6), text.slice(6, 12)];
}

function prizeName(api: PluginApi, kind: Kind | null, amount: number, label?: string): string {
  if (label) return label;
  if (!kind) return api.t('lottery.ui.none');
  const n = kind === 'upload' ? shortBytes(amount) : String(amount);
  return api.t(`lottery.ui.k.${kind}`, { n });
}

function A({ api, to, children }: { api: PluginApi; to: string; children: ReactNode }) {
  return (
    <a
      href={to}
      onClick={(e) => {
        e.preventDefault();
        api.navigate(to);
      }}
    >
      {children}
    </a>
  );
}

/** 转盘扇区配色：交替使用主题色，扇区数为奇数时最后一块换色避免与第一块相邻同色 */
const COLORS = ['#f4b860', '#6c8ef5', '#ef7c8e', '#5cc0a5', '#b38cf0', '#4fb3e8', '#f39a6b', '#8fc65a'];

interface Slice {
  key: string;
  prizeId: number | null;
  text: string;
}

function WheelSvg({ slices, rotation }: { slices: Slice[]; rotation: number }) {
  const n = Math.max(slices.length, 1);
  const seg = 360 / n;
  const r = 100;
  const pt = (deg: number, rad: number) => {
    const a = ((deg - 90) * Math.PI) / 180;
    return [r + rad * Math.cos(a), r + rad * Math.sin(a)];
  };
  return (
    <svg className="lw-wheel" viewBox="-6 -6 212 212" style={{ transform: `rotate(${rotation}deg)` }} aria-hidden="true">
      <circle cx={r} cy={r} r={r + 4} fill="#3b2f63" />
      {slices.map((s, i) => {
        const [x1, y1] = pt(i * seg, r);
        const [x2, y2] = pt((i + 1) * seg, r);
        const color = COLORS[(n % 2 === 1 && i === n - 1 && n > 1 ? i + 1 : i) % COLORS.length];
        const mid = i * seg + seg / 2;
        const [tx, ty] = pt(mid, r * 0.64);
        const path = n === 1 ? `M ${r} 0 A ${r} ${r} 0 1 1 ${r - 0.01} 0 Z` : `M ${r} ${r} L ${x1} ${y1} A ${r} ${r} 0 ${seg > 180 ? 1 : 0} 1 ${x2} ${y2} Z`;
        const words = lines(s.text);
        return (
          <g key={s.key}>
            <path d={path} fill={s.prizeId === null ? '#e6e9f2' : color} stroke="#fff" strokeWidth={1.2} />
            <text
              x={tx}
              y={ty}
              transform={`rotate(${mid} ${tx} ${ty})`}
              textAnchor="middle"
              fontSize={n > 10 ? 6.5 : 8}
              fontWeight={700}
              fill={s.prizeId === null ? '#5b6478' : '#fff'}
              style={{ paintOrder: 'stroke' }}
              stroke={s.prizeId === null ? 'none' : 'rgb(0 0 0 / .18)'}
              strokeWidth={1.2}
            >
              {words.map((w, j) => (
                <tspan key={j} x={tx} dy={j === 0 ? (words.length > 1 ? -3.5 : 2.5) : 9}>
                  {w}
                </tspan>
              ))}
            </text>
          </g>
        );
      })}
      {Array.from({ length: n * 2 }, (_, i) => {
        const [x, y] = pt((i * seg) / 2, r + 1.5);
        return <circle key={i} cx={x} cy={y} r={1.6} fill={i % 2 ? '#ffe9a8' : '#fff'} />;
      })}
    </svg>
  );
}

function WheelPage({ api }: { api: PluginApi }) {
  const [w, setW] = useState<Wheel | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [rotation, setRotation] = useState(0);
  const [spinning, setSpinning] = useState(false);
  const [result, setResult] = useState<{ win: boolean; text: string } | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const load = () =>
    api
      .json<Wheel>('/api/p/lottery/v1/wheel')
      .then((r) => {
        setW(r);
        setErr(null);
      })
      .catch((e: unknown) => setErr(errText(api, e)));
  useEffect(() => {
    void load();
    return () => window.clearTimeout(timer.current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  if (err && !w) return <p className="pt-error">{err}</p>;
  if (!w) return null;
  const slices: Slice[] = w.prizes.map((p) => ({ key: `p${p.id}`, prizeId: p.id, text: prizeName(api, p.kind, p.amount, p.label) }));
  if (w.none_chance_ppm > 0 || slices.length === 0) {
    // “谢谢参与”插在中间，与中奖扇区交错，转盘更均衡
    const at = Math.ceil(slices.length / 2);
    slices.splice(at, 0, { key: 'none', prizeId: null, text: api.t('lottery.ui.none') });
  }
  const limited = w.daily_limit > 0 && w.spins_today >= w.daily_limit;
  const canSpin = w.enabled && w.eligible && !limited && !spinning;
  const spin = async () => {
    if (!canSpin) return;
    setSpinning(true);
    setResult(null);
    try {
      const r = await api.json<{ prize_id: number | null; kind: Kind | null; amount: number }>('/api/p/lottery/v1/wheel/spin', { json: {} });
      const idx = Math.max(
        slices.findIndex((s) => s.prizeId === r.prize_id),
        0,
      );
      const seg = 360 / slices.length;
      const jitter = (Math.random() - 0.5) * seg * 0.6;
      const target = 360 - (idx * seg + seg / 2) + jitter;
      setRotation((cur) => cur - (cur % 360) + 360 * 6 + target);
      const label = slices[idx]?.text ?? '';
      timer.current = window.setTimeout(() => {
        setSpinning(false);
        setResult(r.prize_id === null ? { win: false, text: api.t('lottery.ui.lost') } : { win: true, text: api.t('lottery.ui.won', { prize: label }) });
        void load();
      }, 4700);
    } catch (e) {
      setSpinning(false);
      api.toast(errText(api, e), 'error');
      void load();
    }
  };
  return (
    <div>
      <h1 className="page-title">{api.t('lottery.ui.title')}</h1>
      <p className="pt-muted" style={{ marginTop: -8 }}>
        {api.t('lottery.ui.subtitle')}
      </p>
      <div className="lw">
        <Card>
          <div className="lw-stage">
            <div className="lw-box">
              <span className="lw-pointer" />
              <WheelSvg slices={slices} rotation={rotation} />
              <button type="button" className="lw-hub" disabled={!canSpin} onClick={() => void spin()}>
                {spinning ? '…' : api.t('lottery.ui.spin')}
              </button>
            </div>
            <div className={`lw-result${result?.win ? ' win' : ''}`} role="status" aria-live="polite">
              {result?.text ?? (spinning ? api.t('lottery.ui.spinning') : '')}
            </div>
            <div className="lw-meta">
              {!w.enabled ? (
                <Badge tone="danger">{api.t('lottery.ui.disabled')}</Badge>
              ) : !w.eligible ? (
                <Badge tone="danger">{api.t('lottery.ui.not_eligible', { n: w.min_account_days })}</Badge>
              ) : null}
              <span>{w.spin_cost > 0 ? api.t('lottery.ui.cost', { n: w.spin_cost }) : api.t('lottery.ui.free')}</span>
              <span>{w.daily_limit > 0 ? api.t('lottery.ui.today_limit', { n: w.spins_today, limit: w.daily_limit }) : api.t('lottery.ui.today', { n: w.spins_today })}</span>
            </div>
          </div>
        </Card>
        <div className="lw-side">
          <Card title={api.t('lottery.ui.chances')}>
            <ul className="lw-list">
              {w.prizes.map((p) => (
                <li key={p.id}>
                  <span>{prizeName(api, p.kind, p.amount, p.label)}</span>
                  <b>{pct(p.chance_ppm)}</b>
                </li>
              ))}
              {w.none_chance_ppm > 0 && (
                <li>
                  <span className="pt-muted">{api.t('lottery.ui.none')}</span>
                  <span className="pt-muted">{pct(w.none_chance_ppm)}</span>
                </li>
              )}
            </ul>
          </Card>
          <Card title={api.t('lottery.ui.recent')}>
            {w.recent_wins.length === 0 ? (
              <p className="pt-muted small">{api.t('lottery.ui.no_records')}</p>
            ) : (
              <ul className="lw-list">
                {w.recent_wins.map((x, i) => (
                  <li key={i}>
                    <span>
                      <b>{x.username ?? '—'}</b> · {prizeName(api, x.kind, x.amount)}
                    </span>
                    <span className="muted">{new Date(x.at).toLocaleDateString()}</span>
                  </li>
                ))}
              </ul>
            )}
          </Card>
          <Card title={api.t('lottery.ui.mine')}>
            {w.mine.length === 0 ? (
              <p className="pt-muted small">{api.t('lottery.ui.no_records')}</p>
            ) : (
              <ul className="lw-list">
                {w.mine.map((x) => (
                  <li key={x.id}>
                    <span className={x.prize_id === null ? 'pt-muted' : undefined}>{prizeName(api, x.kind, x.amount)}</span>
                    <span className="muted">{when(x.at)}</span>
                  </li>
                ))}
              </ul>
            )}
          </Card>
        </div>
      </div>
    </div>
  );
}

function NavItem({ api }: SlotProps) {
  return (
    <A api={api} to="/p/lottery/">
      {api.t('lottery.ui.nav')}
    </A>
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
    <Card title={api.t('lottery.ui.widget')} actions={<A api={api} to="/p/lottery/">{api.t('lottery.ui.widget_go')}</A>}>
      <p className="pt-muted small" style={{ margin: 0 }}>
        {w.spin_cost > 0 ? api.t('lottery.ui.cost', { n: w.spin_cost }) : api.t('lottery.ui.free')} ·{' '}
        {w.daily_limit > 0 ? api.t('lottery.ui.today_limit', { n: w.spins_today, limit: w.daily_limit }) : api.t('lottery.ui.today', { n: w.spins_today })}
      </p>
    </Card>
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
  if (err && !d) return <p className="pt-error">{err}</p>;
  if (!d || !s) return null;
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
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t('lottery.ui.admin')}</h1>
      <Card title={api.t('lottery.ui.settings')}>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void saveSettings();
          }}
        >
          <label className="pt-row" style={{ gap: 8, marginBottom: 12 }}>
            <input type="checkbox" name="lottery_enabled" checked={cfg.enabled} onChange={(e) => setCfg({ enabled: e.target.checked })} />
            <b>{api.t('lottery.ui.enabled')}</b>
          </label>
          <div className="lw-settings">
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
          <Button type="submit" variant="primary" disabled={busy}>
            {api.t('lottery.ui.save')}
          </Button>
        </form>
      </Card>
      <Card title={api.t('lottery.ui.prizes')}>
        <p className="pt-muted small" style={{ marginTop: 0 }}>
          {api.t('lottery.ui.prizes_hint')}
        </p>
        <div className="lw-admin-rows">
          <div className="lw-admin-row head">
            <span>{api.t('lottery.ui.kind')}</span>
            <span>{api.t('lottery.ui.amount')}</span>
            <span>{api.t('lottery.ui.label')}</span>
            <span>{api.t('lottery.ui.chance_pct')}</span>
            <span>{api.t('lottery.ui.stock')}</span>
            <span>{api.t('lottery.ui.active')}</span>
            <span />
          </div>
          {rows.map((r) => (
            <div className="lw-admin-row" key={r.key}>
              <select className="pt-input" aria-label={api.t('lottery.ui.kind')} value={r.kind} onChange={(e) => upd(r.key, { kind: e.target.value as Kind })}>
                {(['bonus', 'upload', 'vip_days', 'vip_permanent'] as const).map((k) => (
                  <option key={k} value={k}>
                    {api.t(`lottery.ui.kind.${k}`)}
                  </option>
                ))}
              </select>
              <Input
                type="number"
                min={0}
                step={r.kind === 'upload' ? 'any' : 1}
                aria-label={api.t('lottery.ui.amount')}
                placeholder={api.t('lottery.ui.amount')}
                disabled={r.kind === 'vip_permanent'}
                value={r.amount}
                onChange={(e) => upd(r.key, { amount: e.target.value })}
              />
              <Input maxLength={40} aria-label={api.t('lottery.ui.label')} placeholder={api.t('lottery.ui.label')} value={r.label} onChange={(e) => upd(r.key, { label: e.target.value })} />
              <Input type="number" min={0} max={100} step="any" aria-label={api.t('lottery.ui.chance_pct')} value={r.chance} onChange={(e) => upd(r.key, { chance: e.target.value })} />
              <Input type="number" min={0} aria-label={api.t('lottery.ui.stock')} placeholder={api.t('lottery.ui.unlimited')} value={r.stock} onChange={(e) => upd(r.key, { stock: e.target.value })} />
              <input type="checkbox" aria-label={api.t('lottery.ui.active')} checked={r.active} onChange={(e) => upd(r.key, { active: e.target.checked })} />
              <Button variant="ghost" onClick={() => setRows(rows.filter((x) => x.key !== r.key))}>
                {api.t('lottery.ui.remove')}
              </Button>
            </div>
          ))}
        </div>
        <p className={total > 1_000_000 ? 'pt-error' : 'pt-muted small'}>
          {total > 1_000_000 ? api.t('lottery.ui.over') : api.t('lottery.ui.total', { pct: Number((total / 10_000).toFixed(4)), rest: Number(((1_000_000 - total) / 10_000).toFixed(4)) })}
        </p>
        <div className="pt-row">
          <Button
            onClick={() =>
              setRows([...rows, { key: nextKey.current++, kind: 'bonus', amount: '100', label: '', chance: '1', stock: '', active: true }])
            }
          >
            {api.t('lottery.ui.add')}
          </Button>
          <Button variant="primary" disabled={busy || total > 1_000_000} onClick={() => void savePrizes()}>
            {api.t('lottery.ui.save')}
          </Button>
        </div>
      </Card>
      <Card title={api.t('lottery.ui.stats')}>
        <div className="pt-row" style={{ gap: 24, flexWrap: 'wrap' }}>
          <Stat label={api.t('lottery.ui.s_spins')} value={d.today.spins ?? 0} />
          <Stat label={api.t('lottery.ui.s_users')} value={d.today.users ?? 0} />
          <Stat label={api.t('lottery.ui.s_cost')} value={d.today.cost ?? 0} />
          <Stat label={api.t('lottery.ui.s_wins')} value={d.today.wins ?? 0} />
        </div>
      </Card>
      <Card title={api.t('lottery.ui.last30')}>
        {d.last_30_days.length === 0 ? (
          <Empty>{api.t('lottery.ui.no_records')}</Empty>
        ) : (
          <Table
            rows={d.last_30_days}
            rowKey={(x) => x.kind}
            columns={[
              { key: 'k', header: api.t('lottery.ui.kind'), render: (x) => api.t(`lottery.ui.kind.${x.kind}`) },
              { key: 'c', header: api.t('lottery.ui.s_wins'), render: (x) => x.count },
              { key: 'a', header: api.t('lottery.ui.amount'), render: (x) => (x.kind === 'upload' ? shortBytes(x.amount) : x.kind === 'vip_permanent' ? '—' : x.amount) },
            ]}
          />
        )}
      </Card>
    </div>
  );
}

export default {
  slots: {
    'nav.main': { component: NavItem, order: 60 },
    'home.widgets': { component: Widget, order: 30 },
  },
  routes: [
    { path: '/p/lottery/*', component: WheelPage },
    { path: '/admin/p/lottery/*', component: Admin },
  ],
} satisfies PluginFrontend;
