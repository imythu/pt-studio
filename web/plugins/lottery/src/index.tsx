// lottery 前端：抽奖列表、期次详情（购票、我的票、中奖名单、浏览器内复算验证）、首页小组件、后台开期 / 取消 / 立即开奖。
import { useEffect, useState, type ReactNode } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Field, Input, Stat, Table } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'lottery.ui.nav': '抽奖',
  'lottery.ui.title': '抽奖',
  'lottery.ui.disabled': '本站未开启抽奖',
  'lottery.ui.empty': '暂无期次',
  'lottery.ui.pool': '奖池',
  'lottery.ui.price': '票价',
  'lottery.ui.tickets': '已售',
  'lottery.ui.closes': '停售',
  'lottery.ui.tier': '{n} 等奖 × {count}：每注 {amount}（奖池 {pct}%）',
  'lottery.ui.house_cut': '抽水 {n}%',
  'lottery.ui.min_tickets': '少于 {n} 张取消并退款',
  'lottery.ui.per_user': '每人限购 {n} 张',
  'lottery.ui.count': '张数',
  'lottery.ui.buy': '购买',
  'lottery.ui.bought': '已购买 {count} 张，票号 {from}–{to}',
  'lottery.ui.mine': '我的票',
  'lottery.ui.refunded': '已退款',
  'lottery.ui.winners': '中奖名单',
  'lottery.ui.state.open': '销售中',
  'lottery.ui.state.drawn': '已开奖',
  'lottery.ui.state.cancelled': '已取消',
  'lottery.ui.verify': '验证',
  'lottery.ui.verify_hint': '开售前公开 commit = sha256(seed)；开奖后公开 seed。结果由 seed 与全部票据的哈希链决定，可以在浏览器中复算。',
  'lottery.ui.verify_run': '在浏览器中复算',
  'lottery.ui.verify_ok': '验证通过：commit、票据链与中奖号码一致',
  'lottery.ui.verify_bad': '验证失败：{what}',
  'lottery.ui.my_prizes': '我的中奖',
  'lottery.ui.admin': '抽奖管理',
  'lottery.ui.new_round': '新开一期',
  'lottery.ui.f_title': '标题',
  'lottery.ui.f_desc': '说明',
  'lottery.ui.f_price': '票价（魔力值）',
  'lottery.ui.f_max': '每人限购（0 = 不限）',
  'lottery.ui.f_min': '最少票数',
  'lottery.ui.f_house': '抽水 %',
  'lottery.ui.f_tiers': '奖级（中奖数:奖池占比%，逗号分隔，如 1:50,3:30）',
  'lottery.ui.f_closes': '停售时间',
  'lottery.ui.create': '开期',
  'lottery.ui.cancel': '取消并退款',
  'lottery.ui.cancel_reason': '取消理由',
  'lottery.ui.draw_now': '立即开奖',
  'lottery.disabled': '本站未开启抽奖',
  'lottery.not_eligible': '注册时间不足，暂不能购票',
  'lottery.closed': '该期不在销售中',
  'lottery.limit_reached': '已达到本期限购',
  'lottery.insufficient_funds': '魔力值不足',
});
registerMessages('en', {
  'lottery.ui.nav': 'Lottery',
  'lottery.ui.title': 'Lottery',
  'lottery.ui.disabled': 'The lottery is not open on this site',
  'lottery.ui.empty': 'No rounds',
  'lottery.ui.pool': 'Pool',
  'lottery.ui.price': 'Ticket',
  'lottery.ui.tickets': 'Sold',
  'lottery.ui.closes': 'Closes',
  'lottery.ui.tier': 'Tier {n} × {count}: {amount} each ({pct}% of pool)',
  'lottery.ui.house_cut': '{n}% house cut',
  'lottery.ui.min_tickets': 'Cancelled and refunded below {n} tickets',
  'lottery.ui.per_user': 'Up to {n} tickets per user',
  'lottery.ui.count': 'Tickets',
  'lottery.ui.buy': 'Buy',
  'lottery.ui.bought': 'Bought {count} tickets, #{from}–{to}',
  'lottery.ui.mine': 'My tickets',
  'lottery.ui.refunded': 'refunded',
  'lottery.ui.winners': 'Winners',
  'lottery.ui.state.open': 'On sale',
  'lottery.ui.state.drawn': 'Drawn',
  'lottery.ui.state.cancelled': 'Cancelled',
  'lottery.ui.verify': 'Verify',
  'lottery.ui.verify_hint': 'commit = sha256(seed) is published before sales; the seed is revealed after the draw. Results depend on the seed and the hash chain of all tickets and can be recomputed in your browser.',
  'lottery.ui.verify_run': 'Recompute in browser',
  'lottery.ui.verify_ok': 'Verified: commit, ticket chain and winning numbers match',
  'lottery.ui.verify_bad': 'Verification failed: {what}',
  'lottery.ui.my_prizes': 'My prizes',
  'lottery.ui.admin': 'Lottery',
  'lottery.ui.new_round': 'New round',
  'lottery.ui.f_title': 'Title',
  'lottery.ui.f_desc': 'Description',
  'lottery.ui.f_price': 'Ticket price (bonus)',
  'lottery.ui.f_max': 'Per-user limit (0 = none)',
  'lottery.ui.f_min': 'Minimum tickets',
  'lottery.ui.f_house': 'House cut %',
  'lottery.ui.f_tiers': 'Tiers (winners:pool%, comma separated, e.g. 1:50,3:30)',
  'lottery.ui.f_closes': 'Closes at',
  'lottery.ui.create': 'Open round',
  'lottery.ui.cancel': 'Cancel and refund',
  'lottery.ui.cancel_reason': 'Reason',
  'lottery.ui.draw_now': 'Draw now',
  'lottery.disabled': 'The lottery is not open on this site',
  'lottery.not_eligible': 'Your account is too new to buy tickets',
  'lottery.closed': 'This round is not on sale',
  'lottery.limit_reached': 'You reached the ticket limit for this round',
  'lottery.insufficient_funds': 'Not enough bonus',
});

const errText = (api: PluginApi, e: unknown) => (e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e));

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

interface Round {
  id: number;
  title: string;
  description: string;
  ticket_price: number;
  max_per_user: number;
  min_tickets: number;
  house_cut_pct: number;
  tiers: { count: number; pct: number; amount: number }[];
  opens_at: number;
  closes_at: number;
  state: 'open' | 'drawn' | 'cancelled';
  commit: string;
  total_tickets: number;
  pool: number;
  cancel_reason: string | null;
}
interface Detail extends Round {
  my_tickets: { first_no: number; count: number; cost: number; refunded: boolean }[];
  winners: { tier: number; ticket_no: number; amount: number; username: string | null }[];
  enabled: boolean;
}

const when = (ms: number) => new Date(ms).toLocaleString();
const stateTone = (s: string) => (s === 'open' ? 'success' : s === 'drawn' ? 'primary' : 'muted');

function Summary({ api, r }: { api: PluginApi; r: Round }) {
  return (
    <>
      <div className="pt-row stats-row">
        <Stat label={api.t('lottery.ui.pool')} value={r.pool} />
        <Stat label={api.t('lottery.ui.price')} value={r.ticket_price} />
        <Stat label={api.t('lottery.ui.tickets')} value={r.total_tickets} />
        <Stat label={api.t('lottery.ui.closes')} value={when(r.closes_at)} />
      </div>
      <ul className="small">
        {r.tiers.map((t, i) => (
          <li key={i}>{api.t('lottery.ui.tier', { n: i + 1, count: t.count, amount: t.amount, pct: t.pct })}</li>
        ))}
      </ul>
      <p className="pt-muted small">
        {api.t('lottery.ui.house_cut', { n: r.house_cut_pct })} · {api.t('lottery.ui.min_tickets', { n: r.min_tickets })}
        {r.max_per_user > 0 && ` · ${api.t('lottery.ui.per_user', { n: r.max_per_user })}`}
      </p>
    </>
  );
}

function List({ api }: { api: PluginApi }) {
  const [d, setD] = useState<{ items: Round[]; enabled: boolean } | null>(null);
  const [mine, setMine] = useState<{ prizes: { round_id: number; title: string; tier: number; ticket_no: number; amount: number }[] } | null>(null);
  useEffect(() => {
    void api.json<NonNullable<typeof d>>('/api/p/lottery/v1/rounds').then(setD);
    void api.json<NonNullable<typeof mine>>('/api/p/lottery/v1/me').then(setMine);
  }, [api]);
  if (!d) return null;
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t('lottery.ui.title')}</h1>
      {!d.enabled && <p className="pt-muted">{api.t('lottery.ui.disabled')}</p>}
      {d.items.length === 0 && <p className="pt-muted">{api.t('lottery.ui.empty')}</p>}
      {d.items.map((r) => (
        <Card
          key={r.id}
          title={
            <A api={api} to={`/p/lottery/${r.id}`}>
              {r.title}
            </A>
          }
          actions={<Badge tone={stateTone(r.state)}>{api.t(`lottery.ui.state.${r.state}`)}</Badge>}
        >
          <Summary api={api} r={r} />
        </Card>
      ))}
      {mine && mine.prizes.length > 0 && (
        <Card title={api.t('lottery.ui.my_prizes')}>
          <Table
            rows={mine.prizes}
            rowKey={(p) => `${p.round_id}:${p.ticket_no}`}
            columns={[
              { key: 'r', header: '', render: (p) => <A api={api} to={`/p/lottery/${p.round_id}`}>{p.title}</A> },
              { key: 't', header: '', render: (p) => `${p.tier} · #${p.ticket_no}` },
              { key: 'a', header: '', render: (p) => p.amount },
            ]}
          />
        </Card>
      )}
    </div>
  );
}

// ---------------- 浏览器内复算 ----------------

const fromHex = (h: string) => new Uint8Array((h.match(/../g) ?? []).map((x) => parseInt(x, 16)));
const toHex = (b: Uint8Array) => Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
const be = (n: number, bytes: 4 | 8) => {
  const b = new Uint8Array(bytes);
  let v = BigInt(n);
  for (let i = bytes - 1; i >= 0; i--) {
    b[i] = Number(v & 0xffn);
    v >>= 8n;
  }
  return b;
};
async function sha256(...parts: Uint8Array[]): Promise<Uint8Array> {
  const all = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let o = 0;
  for (const p of parts) {
    all.set(p, o);
    o += p.length;
  }
  return new Uint8Array(await crypto.subtle.digest('SHA-256', all));
}

interface VerifyData {
  state: string;
  commit: string;
  seed: string | null;
  tickets_root: string;
  total_tickets: number;
  tiers: { count: number; pct: number }[];
  batches: { batch_id: number; user_id: number; first_no: number; count: number; chain: string }[];
  winners: { ticket_no: number }[];
}

async function recompute(v: VerifyData): Promise<string | null> {
  if (!v.seed) return 'seed';
  const seed = fromHex(v.seed);
  if (toHex(await sha256(seed)) !== v.commit) return 'commit';
  let prev: Uint8Array = new Uint8Array();
  for (const b of v.batches) {
    prev = await sha256(prev, be(b.batch_id, 8), be(b.user_id, 8), be(b.first_no, 8), be(b.count, 8));
    if (toHex(prev) !== b.chain) return `chain #${b.batch_id}`;
  }
  if (toHex(prev) !== v.tickets_root) return 'tickets_root';
  const rand = await sha256(seed, prev);
  const got: number[] = [];
  const total = BigInt(v.total_tickets);
  outer: for (let t = 0; t < v.tiers.length; t++) {
    for (let k = 0; k < (v.tiers[t]?.count ?? 0); k++) {
      if (got.length >= v.total_tickets) break outer;
      for (let a = 0; ; a++) {
        const h = await sha256(rand, be(t, 4), be(k, 8), be(a, 4));
        let x = 0n;
        for (let i = 0; i < 8; i++) x = (x << 8n) | BigInt(h[i] ?? 0);
        const n = Number(x % total) + 1;
        if (!got.includes(n)) {
          got.push(n);
          break;
        }
      }
    }
  }
  return got.join(',') === v.winners.map((w) => w.ticket_no).join(',') ? null : 'winners';
}

function Verify({ api, id }: { api: PluginApi; id: number }) {
  const [v, setV] = useState<VerifyData | null>(null);
  const [res, setRes] = useState<string | null>(null);
  useEffect(() => {
    void api.json<VerifyData>(`/api/p/lottery/v1/rounds/${id}/verify`).then(setV);
  }, [api, id]);
  if (!v) return null;
  return (
    <Card title={api.t('lottery.ui.verify')}>
      <p className="small pt-muted">{api.t('lottery.ui.verify_hint')}</p>
      <p className="small pt-mono" style={{ wordBreak: 'break-all' }}>
        commit: {v.commit}
        <br />
        seed: {v.seed ?? '—'}
        <br />
        tickets_root: {v.tickets_root || '—'}
      </p>
      {v.state === 'drawn' && (
        <Button onClick={() => void recompute(v).then((bad) => setRes(bad === null ? api.t('lottery.ui.verify_ok') : api.t('lottery.ui.verify_bad', { what: bad })))}>
          {api.t('lottery.ui.verify_run')}
        </Button>
      )}
      {res && <p role="status">{res}</p>}
    </Card>
  );
}

function RoundPage({ api, id }: { api: PluginApi; id: number }) {
  const [d, setD] = useState<Detail | null>(null);
  const [count, setCount] = useState(1);
  const load = () => void api.json<Detail>(`/api/p/lottery/v1/rounds/${id}`).then(setD);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, [id]);
  if (!d) return null;
  const buy = async () => {
    try {
      const r = await api.json<{ first_no: number; count: number }>(`/api/p/lottery/v1/rounds/${id}/tickets`, { json: { count } });
      api.toast(api.t('lottery.ui.bought', { count: r.count, from: r.first_no, to: r.first_no + r.count - 1 }), 'success');
      load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const open = d.state === 'open' && Date.now() < d.closes_at;
  return (
    <div className="pt-stack">
      <h1 className="page-title">
        {d.title} <Badge tone={stateTone(d.state)}>{api.t(`lottery.ui.state.${d.state}`)}</Badge>
      </h1>
      {d.description && <p>{d.description}</p>}
      <Card>
        <Summary api={api} r={d} />
        {open && d.enabled && (
          <div className="shop-buy">
            <Input type="number" name="lottery_count" aria-label={api.t('lottery.ui.count')} min={1} value={count} onChange={(e) => setCount(Math.max(1, Number(e.target.value) || 1))} />
            <Button variant="primary" onClick={() => void buy()}>
              {api.t('lottery.ui.buy')} ({count * d.ticket_price})
            </Button>
          </div>
        )}
        {d.state === 'cancelled' && d.cancel_reason && <p className="pt-muted">{d.cancel_reason}</p>}
      </Card>
      {d.my_tickets.length > 0 && (
        <Card title={api.t('lottery.ui.mine')}>
          <p>
            {d.my_tickets.map((t) => (
              <span key={t.first_no} className="pt-mono">
                #{t.first_no}
                {t.count > 1 ? `–${t.first_no + t.count - 1}` : ''}
                {t.refunded ? ` (${api.t('lottery.ui.refunded')})` : ''}{' '}
              </span>
            ))}
          </p>
        </Card>
      )}
      {d.winners.length > 0 && (
        <Card title={api.t('lottery.ui.winners')}>
          <Table
            rows={d.winners}
            rowKey={(w) => w.ticket_no}
            columns={[
              { key: 't', header: '', render: (w) => w.tier },
              { key: 'n', header: '', render: (w) => <span className="pt-mono">#{w.ticket_no}</span> },
              { key: 'u', header: '', render: (w) => w.username ?? '?' },
              { key: 'a', header: '', render: (w) => w.amount },
            ]}
          />
        </Card>
      )}
      <Verify api={api} id={id} />
    </div>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  const m = /^\/p\/lottery\/(\d+)/.exec(path);
  return m ? <RoundPage api={api} id={Number(m[1])} /> : <List api={api} />;
}

function NavItem({ api }: SlotProps) {
  return (
    <A api={api} to="/p/lottery/">
      {api.t('lottery.ui.nav')}
    </A>
  );
}

function Widget({ api }: SlotProps) {
  const [r, setR] = useState<Round | null>(null);
  useEffect(() => {
    if (!api.user()) return;
    void api
      .json<{ items: Round[]; enabled: boolean }>('/api/p/lottery/v1/rounds')
      .then((d) => setR(d.enabled ? (d.items.find((x) => x.state === 'open') ?? null) : null))
      .catch(() => undefined);
  }, [api]);
  if (!r) return null;
  return (
    <Card
      title={
        <A api={api} to={`/p/lottery/${r.id}`}>
          {api.t('lottery.ui.title')} · {r.title}
        </A>
      }
    >
      <div className="pt-row">
        <Stat label={api.t('lottery.ui.pool')} value={r.pool} />
        <Stat label={api.t('lottery.ui.closes')} value={when(r.closes_at)} />
      </div>
    </Card>
  );
}

// ---------------- 后台 ----------------

function Admin({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<Round[]>([]);
  const inAWeek = new Date(Date.now() + 7 * 86_400_000 - new Date().getTimezoneOffset() * 60_000).toISOString().slice(0, 16);
  const [f, setF] = useState({ title: '', description: '', price: '100', max: '0', min: '10', house: '10', tiers: '1:50,3:30', closes: inAWeek });
  const load = () => void api.json<{ items: Round[] }>('/api/p/lottery/v1/admin/rounds').then((r) => setRows(r.items));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, []);
  const run = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const create = () =>
    run(() =>
      api.json('/api/p/lottery/v1/admin/rounds', {
        json: {
          title: f.title,
          description: f.description,
          ticket_price: Number(f.price),
          max_per_user: Number(f.max) || 0,
          min_tickets: Number(f.min) || 1,
          house_cut_pct: Number(f.house) || 0,
          tiers: f.tiers
            .split(',')
            .map((s) => s.split(':').map((x) => Number(x.trim())))
            .map(([count, pct]) => ({ count, pct })),
          closes_at: new Date(f.closes).getTime(),
        },
      }),
    );
  const cancel = (id: number) => {
    const reason = window.prompt(api.t('lottery.ui.cancel_reason'));
    if (reason) void run(() => api.json(`/api/p/lottery/v1/admin/rounds/${id}/cancel`, { json: { reason } }));
  };
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t('lottery.ui.admin')}</h1>
      <Card title={api.t('lottery.ui.title')}>
        <Table
          rows={rows}
          rowKey={(r) => r.id}
          empty={api.t('lottery.ui.empty')}
          columns={[
            {
              key: 't',
              header: '',
              render: (r) => (
                <>
                  <A api={api} to={`/p/lottery/${r.id}`}>
                    {r.title}
                  </A>{' '}
                  <Badge tone={stateTone(r.state)}>{api.t(`lottery.ui.state.${r.state}`)}</Badge>
                  <div className="pt-muted small">
                    {api.t('lottery.ui.pool')} {r.pool} · {api.t('lottery.ui.tickets')} {r.total_tickets} · {when(r.closes_at)}
                  </div>
                </>
              ),
            },
            {
              key: 'a',
              header: '',
              render: (r) =>
                r.state === 'open' ? (
                  <span className="pt-row">
                    <Button onClick={() => void run(() => api.json(`/api/p/lottery/v1/admin/rounds/${r.id}/draw`, { json: {} }))}>{api.t('lottery.ui.draw_now')}</Button>
                    <Button variant="danger" onClick={() => cancel(r.id)}>
                      {api.t('lottery.ui.cancel')}
                    </Button>
                  </span>
                ) : null,
            },
          ]}
        />
      </Card>
      <Card title={api.t('lottery.ui.new_round')}>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void create();
          }}
        >
          <Field label={api.t('lottery.ui.f_title')}>
            <Input name="round_title" value={f.title} maxLength={100} required onChange={(e) => setF({ ...f, title: e.target.value })} />
          </Field>
          <Field label={api.t('lottery.ui.f_desc')}>
            <Input value={f.description} maxLength={1000} onChange={(e) => setF({ ...f, description: e.target.value })} />
          </Field>
          <Field label={api.t('lottery.ui.f_price')}>
            <Input name="round_price" type="number" min={1} value={f.price} onChange={(e) => setF({ ...f, price: e.target.value })} />
          </Field>
          <Field label={api.t('lottery.ui.f_max')}>
            <Input type="number" min={0} value={f.max} onChange={(e) => setF({ ...f, max: e.target.value })} />
          </Field>
          <Field label={api.t('lottery.ui.f_min')}>
            <Input name="round_min" type="number" min={1} value={f.min} onChange={(e) => setF({ ...f, min: e.target.value })} />
          </Field>
          <Field label={api.t('lottery.ui.f_house')}>
            <Input type="number" min={0} max={50} value={f.house} onChange={(e) => setF({ ...f, house: e.target.value })} />
          </Field>
          <Field label={api.t('lottery.ui.f_tiers')}>
            <Input name="round_tiers" className="pt-mono" value={f.tiers} onChange={(e) => setF({ ...f, tiers: e.target.value })} />
          </Field>
          <Field label={api.t('lottery.ui.f_closes')}>
            <Input type="datetime-local" value={f.closes} required onChange={(e) => setF({ ...f, closes: e.target.value })} />
          </Field>
          <Button type="submit" variant="primary">
            {api.t('lottery.ui.create')}
          </Button>
        </form>
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
    { path: '/p/lottery/*', component: Pages },
    { path: '/admin/p/lottery/*', component: Admin },
  ],
} satisfies PluginFrontend;
