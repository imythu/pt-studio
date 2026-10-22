// @pt-studio/ui-kit：宿主与插件共用的组件（UI-WEB §4）。样式见 styles.css，全部基于主题令牌；
// 页面里任何两栏结构都经这里的布局模式组件（P1–P5）实现，页面与插件不自写媒体查询。

import DOMPurify from 'dompurify';
import {
  createContext,
  isValidElement,
  useCallback,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type ButtonHTMLAttributes,
  type CSSProperties,
  type InputHTMLAttributes,
  type KeyboardEvent,
  type ReactNode,
  type SelectHTMLAttributes,
  type TextareaHTMLAttributes,
} from 'react';
import { createPortal } from 'react-dom';

export function cx(...xs: (string | false | null | undefined)[]): string {
  return xs.filter(Boolean).join(' ');
}

/** 媒体查询（窄屏行为只用于组件内部，页面不直接判断宽度） */
/** 等级名后附简写：高级用户（PU） / Power User (PU)；没有简写或与名称相同时只显示名称 */
export function withAbbr(name: string, abbr?: string | null): string {
  if (!abbr || abbr === name) return name;
  return /[^\x00-\x7f]/.test(name) ? `${name}（${abbr}）` : `${name} (${abbr})`;
}

export function useMediaQuery(q: string): boolean {
  const get = () => typeof window !== 'undefined' && typeof window.matchMedia === 'function' && window.matchMedia(q).matches;
  const [v, setV] = useState(get);
  useEffect(() => {
    if (typeof window.matchMedia !== 'function') return;
    const m = window.matchMedia(q);
    const on = () => setV(m.matches);
    on();
    m.addEventListener('change', on);
    return () => m.removeEventListener('change', on);
  }, [q]);
  return v;
}

/** 挂到 document.body（UI-WEB R7：浮层不受页面容器的定位与层叠影响） */
export function Portal({ children }: { children: ReactNode }) {
  if (typeof document === 'undefined') return null;
  return createPortal(children, document.body);
}

// ---------------- 按钮 ----------------

type Variant = 'default' | 'primary' | 'gold' | 'danger' | 'ghost';

export function Button({
  variant = 'default',
  size,
  block,
  className,
  ...p
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: 'xs' | 'sm' | 'lg'; block?: boolean }) {
  return <button type="button" {...p} className={cx('btn', variant !== 'default' && `btn-${variant}`, size && `btn-${size}`, block && 'btn-block', className)} />;
}

// ---------------- 站内链接（插件用） ----------------

/** 站内链接：普通点击走单页导航，Ctrl / 中键 / 新窗口保持浏览器默认行为 */
export function PLink({ to, navigate, children, className, title }: { to: string; navigate(to: string): void; children: ReactNode; className?: string; title?: string }) {
  return (
    <a
      href={to}
      className={className}
      title={title}
      onClick={(e) => {
        if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
        e.preventDefault();
        navigate(to);
      }}
    >
      {children}
    </a>
  );
}

/** 导航项（nav.main / nav.user_menu 插槽）：文字 + 可选数字；`alert` 时数字为醒目的未读样式 */
export function NavItem({ to, navigate, children, count, alert }: { to: string; navigate(to: string): void; children: ReactNode; count?: number | string | null; alert?: boolean }) {
  const show = count !== undefined && count !== null && count !== '' && !(alert && Number(count) === 0);
  return (
    <PLink to={to} navigate={navigate}>
      <span>{children}</span>
      {show && (alert ? <b className="n">{Number(count) > 99 ? '99+' : count}</b> : <em>{count}</em>)}
    </PLink>
  );
}

// ---------------- 页面结构 ----------------

/** 页面标题区：标题、说明、操作；`crumb` 为面包屑（链接由调用方提供） */
export function PageHead({ title, lede, actions, crumb }: { title: ReactNode; lede?: ReactNode; actions?: ReactNode; crumb?: ReactNode }) {
  return (
    <>
      {crumb && <nav className="crumb" aria-label="breadcrumb">{crumb}</nav>}
      <div className="page-h">
        <h1>{title}</h1>
        {actions && <div className="acts">{actions}</div>}
        {lede && <p className="lede">{lede}</p>}
      </div>
    </>
  );
}

export function Panel({
  title,
  sub,
  more,
  actions,
  children,
  className,
  flush,
  id,
  primary,
}: {
  title?: ReactNode;
  sub?: ReactNode;
  more?: ReactNode;
  actions?: ReactNode;
  children?: ReactNode;
  className?: string;
  /** 内容不加内边距（表格、列表） */
  flush?: boolean;
  id?: string;
  /** 标记为页面主要内容区（UI-WEB R8） */
  primary?: boolean;
}) {
  return (
    <section className={cx('panel', className)} id={id} data-primary={primary || undefined}>
      {(title || sub || more || actions) && (
        <header className="panel-h">
          {title && <h2>{title}</h2>}
          {sub && <span className="sub">{sub}</span>}
          {more && <span className="more">{more}</span>}
          {actions && <div className="acts">{actions}</div>}
        </header>
      )}
      {children !== undefined && children !== null && children !== false && <div className={cx('panel-b', flush && 'flush')}>{children}</div>}
    </section>
  );
}

/** 旧名兼容：Card = Panel */
export function Card({ title, actions, children, className }: { title?: ReactNode; actions?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <Panel title={title} actions={actions} className={className}>
      {children}
    </Panel>
  );
}

/** 主内容 + 侧栏（P3）：≤ 1240px 变为一栏，侧栏排在正文后；`sideFirst` 时侧栏在前（侧栏是导航或关键信息） */
export function SideLayout({ side, children, left, width, sideFirst }: { side: ReactNode; children: ReactNode; left?: boolean; width?: number; sideFirst?: boolean }) {
  const style = width ? ({ '--side-w': `${width}px` } as CSSProperties) : undefined;
  const sideEl = <div className={cx('stack', sideFirst ? 'm-first' : 'm-after')}>{side}</div>;
  const main = <div className="stack">{children}</div>;
  return (
    <div className={left ? 'side-l' : 'side-r'} style={style}>
      {left ? sideEl : main}
      {left ? main : sideEl}
    </div>
  );
}

export function Grid({ cols = 2, children, className }: { cols?: 2 | 3; children: ReactNode; className?: string }) {
  return <div className={cx(cols === 3 ? 'g3' : 'g2', className)}>{children}</div>;
}

export interface Fact {
  label: ReactNode;
  value: ReactNode;
  hint?: ReactNode;
  tone?: 'up' | 'down' | 'gold' | 'moon';
}

/** 数字带：一行横排，细分割 */
export function Kpis({ items }: { items: Fact[] }) {
  return (
    <div className="kpis" style={{ '--n': items.length } as CSSProperties}>
      {items.map((x, i) => (
        <div key={i}>
          <span>{x.label}</span>
          <b className={x.tone}>{x.value}</b>
          {x.hint && <small>{x.hint}</small>}
        </div>
      ))}
    </div>
  );
}

/** P3 关键信息条：≤ 1240px 显示在正文前（侧栏里同样的卡片加 `kf-hide`） */
export function KeyFacts({ items }: { items: Fact[] }) {
  return (
    <div className="keyfacts" data-primary style={{ '--n': items.length } as CSSProperties}>
      {items.map((x, i) => (
        <div key={i}>
          <span>{x.label}</span>
          <b className={x.tone}>{x.value}</b>
          {x.hint && <small>{x.hint}</small>}
        </div>
      ))}
    </div>
  );
}

/** 统计格；给了 `to` + `navigate` 时整格可点，进入对应详情页 */
export function Stat({ label, value, hint, tone, to, navigate }: { label: ReactNode; value: ReactNode; hint?: ReactNode; tone?: Fact['tone']; to?: string; navigate?(to: string): void }) {
  const body = (
    <>
      <span>{label}</span>
      <b className={tone}>{value}</b>
      {hint && <small>{hint}</small>}
    </>
  );
  return to && navigate ? (
    <PLink to={to} navigate={navigate} className="stat link">
      {body}
    </PLink>
  ) : (
    <div className="stat">{body}</div>
  );
}

/** 规则条目（魔力值页「获取 / 使用」列表）：标题、说明、右侧数值；给了 `to` + `navigate` 时末尾带一个文字链接 */
export function RuleItem({ title, value, children, to, navigate, link }: { title: ReactNode; value?: ReactNode; children?: ReactNode; to?: string; navigate?(to: string): void; link?: ReactNode }) {
  return (
    <div className="rule-item">
      <div>
        <b>{title}</b>
        {children && <div className="ri-body">{children}</div>}
        {to && navigate && link && (
          <PLink to={to} navigate={navigate} className="ri-link">
            {link} →
          </PLink>
        )}
      </div>
      {value !== undefined && value !== null && <span className="ri-val">{value}</span>}
    </div>
  );
}

// ---------------- 状态与标记 ----------------

export type Tone = 'ok' | 'warn' | 'bad' | 'info' | 'mute';

/** 状态胶囊（语义色） */
export function Status({ tone, children, title }: { tone?: Tone; children: ReactNode; title?: string }) {
  return (
    <span className={cx('st', tone && `st-${tone}`)} title={title}>
      {children}
    </span>
  );
}

export function Tag({ tone, children, title }: { tone?: 'x' | 'o' | 'k' | 'g'; children: ReactNode; title?: string }) {
  return (
    <span className={cx('tag', tone && `tag-${tone}`)} title={title}>
      {children}
    </span>
  );
}

/** 旧名兼容：Badge 映射到 Tag 的语气 */
export function Badge({ tone = 'muted', children, title }: { tone?: 'muted' | 'primary' | 'success' | 'danger' | 'free' | 'double'; children: ReactNode; title?: string }) {
  const map = { muted: undefined, primary: 'g', success: 'k', danger: 'x', free: 'g', double: 'k' } as const;
  return (
    <Tag tone={map[tone]} title={title}>
      {children}
    </Tag>
  );
}

export type SealKind = 'free' | '2x' | '2xfree' | 'half' | '30' | 'other';

/** 优惠印记：免费 / 2X / 2X免费 / 50% / 30%；`left` 为剩余时间 */
export function Seal({ kind, children, left, title }: { kind: SealKind; children: ReactNode; left?: ReactNode; title?: string }) {
  return (
    <>
      <span className={`seal seal-${kind}`} title={title}>
        {children}
      </span>
      {left && <span className="seal-left">{left}</span>}
    </>
  );
}

export function Note({ tone, children, actions }: { tone?: 'warn' | 'bad' | 'ok' | 'info'; children: ReactNode; actions?: ReactNode }) {
  return (
    <div className={cx('note', tone && `note-${tone}`)} role={tone === 'bad' ? 'alert' : undefined}>
      {children}
      {actions && <span className="acts">{actions}</span>}
    </div>
  );
}

/** 进度条 */
export function Progress({ value, max = 1, label, tone }: { value: number; max?: number; label?: string; tone?: 'ok' | 'bad' | 'info' }) {
  const pct = Math.max(0, Math.min(100, (value / (max || 1)) * 100));
  return (
    <div className={cx('bar', tone)} role="progressbar" aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100} aria-label={label}>
      <i style={{ width: `${pct}%` }} />
    </div>
  );
}

export function Timeline({ items }: { items: { tone?: 'ok' | 'warn' | 'bad'; children: ReactNode; time?: ReactNode; key?: string | number }[] }) {
  return (
    <div className="tl">
      {items.map((x, i) => (
        <div key={x.key ?? i} className={x.tone ? `tl-${x.tone}` : undefined}>
          {x.children}
          {x.time && <time>{x.time}</time>}
        </div>
      ))}
    </div>
  );
}

// ---------------- 表单 ----------------

export function Field({ label, error, hint, children, className }: { label: ReactNode; error?: string | null; hint?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <label className={cx('fl', error && 'invalid', className)}>
      <span className="fl-label">{label}</span>
      {children}
      {hint && !error && <span className="hint">{hint}</span>}
      {error && <span className="err">{error}</span>}
    </label>
  );
}

export function Input(p: InputHTMLAttributes<HTMLInputElement>) {
  return <input {...p} className={cx('in', p.className)} />;
}

export function Select({ options, className, children, ...p }: SelectHTMLAttributes<HTMLSelectElement> & { options?: (string | { value: string; label: ReactNode })[] }) {
  return (
    <select {...p} className={cx('sel', className)}>
      {options?.map((o) => (typeof o === 'string' ? <option key={o} value={o}>{o}</option> : <option key={o.value} value={o.value}>{o.label}</option>))}
      {children}
    </select>
  );
}

export function Textarea(p: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea {...p} className={cx('ta', p.className)} />;
}

export function Checkbox({ label, checked, onChange, name, disabled }: { label: ReactNode; checked: boolean; onChange(v: boolean): void; name?: string; disabled?: boolean }) {
  return (
    <label className="ck">
      <input type="checkbox" name={name} checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
      {label}
    </label>
  );
}

/** 开关；`showLabel` 时在右侧显示文字（点文字同样切换） */
export function Switch({ checked, onChange, label, disabled, showLabel }: { checked: boolean; onChange(v: boolean): void; label: string; disabled?: boolean; showLabel?: boolean }) {
  const sw = <button type="button" role="switch" className="sw" aria-checked={checked} aria-label={label} disabled={disabled} onClick={() => onChange(!checked)} />;
  if (!showLabel) return sw;
  return (
    <span className="sw-l">
      {sw}
      <span aria-hidden="true" onClick={() => !disabled && onChange(!checked)}>
        {label}
      </span>
    </span>
  );
}

export interface Opt<V extends string = string> {
  value: V;
  label: ReactNode;
}

/** 单选或多选的文字标签组 */
export function Chips<V extends string>({ options, value, onChange, label }: { options: Opt<NoInfer<V>>[]; value: V | V[] | null; onChange(v: V): void; label?: string }) {
  const on = (v: V) => (Array.isArray(value) ? value.includes(v) : value === v);
  return (
    <div className="chips" role="group" aria-label={label}>
      {options.map((o) => (
        <button key={o.value} type="button" className="chip" aria-pressed={on(o.value)} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Segmented<V extends string>({ options, value, onChange, label }: { options: Opt<NoInfer<V>>[]; value: V; onChange(v: V): void; label?: string }) {
  return (
    <div className="seg" role="group" aria-label={label}>
      {options.map((o) => (
        <button key={o.value} type="button" aria-pressed={o.value === value} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}

// ---------------- 加载与空状态 ----------------

export function Spinner() {
  return <span className="spinner" role="status" aria-label="loading" />;
}

export function SkeletonRows({ n = 5 }: { n?: number }) {
  return (
    <div aria-busy="true" aria-label="loading">
      {Array.from({ length: n }, (_, i) => (
        <span key={i} className="skeleton skeleton-block" />
      ))}
    </div>
  );
}

/** 空状态：芙莉莲读书插画 + 说明；插件常用 `—` 作占位文案时只显示插画 */
export function Empty({ children, hint, compact }: { children?: ReactNode; hint?: ReactNode; compact?: boolean; illustration?: boolean | string }) {
  const blank = children === '—' || children === null || children === undefined;
  return (
    <div className="empty">
      {!compact && <img src="/frieren/frieren-empty-state-generated.jpg" alt="" loading="lazy" />}
      {!blank && <b>{children}</b>}
      {hint && <span>{hint}</span>}
    </div>
  );
}

// ---------------- 头像与封面 ----------------

const DEFAULT_AVATAR = '/frieren/frieren-avatar-generated.jpg';

/** 头像：未设置或加载失败时用芙莉莲默认头像，再失败显示首字母 */
export function Avatar({ src, fallbackSrc = DEFAULT_AVATAR, name, size = 32, ring, className }: { src?: string | null; fallbackSrc?: string | null; name?: string; size?: number; ring?: boolean; className?: string }) {
  const [step, setStep] = useState(0);
  useEffect(() => setStep(0), [src]);
  const cands = [src, fallbackSrc].filter((x): x is string => !!x);
  const cur = cands[step];
  return (
    <span className={cx('ava', ring && 'ring', className)} style={{ width: size, height: size, fontSize: Math.max(11, size * 0.4) }} aria-hidden="true">
      {cur ? <img src={cur} alt="" onError={() => setStep((s) => s + 1)} /> : (name || '?').slice(0, 1).toUpperCase()}
    </span>
  );
}

const COVER_COLORS = ['#2c4f86', '#8a5a2a', '#7a2836', '#4a3a2c', '#20604c', '#9a3a3a', '#1f2a44', '#5a6a3a', '#3a4a5c', '#6a3a6a', '#3a6a5a', '#6a4a1a', '#5c2a20', '#3a5a6a'];

/** 封面：有海报用海报；没有时排成一册魔导书封面（颜色由标题稳定决定） */
export function Cover({ src, title, sub, rating, corner, className }: { src?: string | null; title: string; sub?: string | null; rating?: number | string | null; corner?: ReactNode; className?: string }) {
  const [failed, setFailed] = useState(false);
  const color = useMemo(() => {
    let h = 0;
    for (const c of title) h = (h * 31 + c.charCodeAt(0)) >>> 0;
    return COVER_COLORS[h % COVER_COLORS.length];
  }, [title]);
  const img = src && !failed;
  return (
    <span className={cx('cover', !img && 'book', className)} style={{ '--bg-c': color } as CSSProperties}>
      {/* 加载失败时保留（隐藏的）img，改显示书脊式封面 */}
      {src && <img className="poster" src={src} alt="" loading="lazy" hidden={failed} onError={() => setFailed(true)} />}
      {!img && (
        <>
          <span className="bt">{title}</span>
          {sub && <span className="be">{sub}</span>}
        </>
      )}
      {rating !== undefined && rating !== null && rating !== '' && <span className="rt">★ {typeof rating === 'number' ? rating.toFixed(1) : rating}</span>}
      {corner && <span className="sl">{corner}</span>}
    </span>
  );
}

// ---------------- 表格与分页 ----------------

export interface Column<T> {
  key: string;
  header: ReactNode;
  render(row: T): ReactNode;
  className?: string;
  /** 右对齐（数字列） */
  right?: boolean;
}

export function Table<T>({
  rows,
  columns,
  rowKey,
  empty,
  minWidth,
  onRowClick,
  selected,
  label,
}: {
  rows: T[];
  columns: Column<T>[];
  rowKey(row: T): string | number;
  empty?: ReactNode;
  minWidth?: 640 | 820;
  /** 行可点击（P2 主从列表） */
  onRowClick?(row: T): void;
  selected?: string | number | null;
  label?: string;
}) {
  if (rows.length === 0 && empty !== undefined) return isValidElement(empty) ? <>{empty}</> : <Empty compact>{empty}</Empty>;
  return (
    <div className="dtw">
      <table className={cx('dt', minWidth && `min${minWidth}`)} aria-label={label}>
        <thead>
          <tr>
            {columns.map((c) => (
              <th key={c.key} className={cx(c.right && 'r', c.className)}>
                {c.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => {
            const k = rowKey(r);
            return (
              <tr
                key={k}
                data-selected={selected !== undefined && selected === k ? 'true' : undefined}
                data-click={onRowClick ? '' : undefined}
                onClick={onRowClick ? (e) => !(e.target as HTMLElement).closest('a,button,input,select,textarea,label') && onRowClick(r) : undefined}
              >
                {columns.map((c) => (
                  <td key={c.key} className={cx(c.right && 'r', c.className)}>
                    {c.render(r)}
                  </td>
                ))}
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

/** 页码：显示首页、末页与当前页附近 */
export function Pager({ page, pages, onChange, prev = '上一页', next = '下一页' }: { page: number; pages: number; onChange(p: number): void; prev?: string; next?: string }) {
  if (pages <= 1) return null;
  const set = new Set([1, pages, page - 1, page, page + 1].filter((p) => p >= 1 && p <= pages));
  const list = [...set].sort((a, b) => a - b);
  const out: (number | '…')[] = [];
  list.forEach((p, i) => {
    if (i && p - (list[i - 1] as number) > 1) out.push('…');
    out.push(p);
  });
  return (
    <nav className="pager" aria-label="pagination">
      <button type="button" disabled={page <= 1} onClick={() => onChange(page - 1)}>
        {prev}
      </button>
      {out.map((p, i) =>
        p === '…' ? (
          <button key={`e${i}`} type="button" disabled>
            …
          </button>
        ) : (
          <button key={p} type="button" aria-current={p === page ? 'page' : undefined} onClick={() => onChange(p)}>
            {p}
          </button>
        ),
      )}
      <button type="button" disabled={page >= pages} onClick={() => onChange(page + 1)}>
        {next}
      </button>
    </nav>
  );
}

export function LoadMore({ onClick, loading, label = '加载更多' }: { onClick(): void; loading?: boolean; label?: ReactNode }) {
  return (
    <div className="more-row">
      <Button size="sm" onClick={onClick} disabled={loading}>
        {loading ? <Spinner /> : label}
      </Button>
    </div>
  );
}

// ---------------- 页签 ----------------

export interface TabDef {
  id: string;
  label: ReactNode;
  count?: ReactNode;
}

/** 页签：`variant="sub"` 为页面级二级导航，默认为面板内页签 */
export function Tabs({ tabs, value, onChange, variant, label }: { tabs: TabDef[]; value: string; onChange(id: string): void; variant?: 'sub' | 'flush'; label?: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return;
    const i = tabs.findIndex((t) => t.id === value);
    const n = tabs[(i + (e.key === 'ArrowRight' ? 1 : tabs.length - 1)) % tabs.length];
    if (n) {
      onChange(n.id);
      requestAnimationFrame(() => ref.current?.querySelector<HTMLButtonElement>('[aria-selected="true"]')?.focus());
    }
  };
  return (
    <div ref={ref} role="tablist" aria-label={label} className={variant === 'sub' ? 'subnav' : variant === 'flush' ? 'subnav flush' : 'tabs'} onKeyDown={onKey}>
      {tabs.map((t) => (
        <button key={t.id} type="button" role="tab" aria-selected={t.id === value} tabIndex={t.id === value ? 0 : -1} onClick={() => onChange(t.id)}>
          {t.label}
          {t.count !== undefined && t.count !== null && <em>{t.count}</em>}
        </button>
      ))}
    </div>
  );
}

// ---------------- 浮层 ----------------

/** P4 抽屉：创建与编辑表单；桌面从右侧滑出，手机从底部升起；Esc 与背景点击关闭 */
export function Sheet({ open, onClose, title, children, footer, width, flush }: { open: boolean; onClose(): void; title: ReactNode; children: ReactNode; footer?: ReactNode; width?: number; flush?: boolean }) {
  const id = useId();
  const pn = useRef<HTMLDivElement>(null);
  // 调用方多传内联函数：经 ref 读取，避免每次重渲染都重跑下面的 effect（会把焦点拉回第一个输入框）
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    if (!open) return;
    const prev = document.activeElement as HTMLElement | null;
    const key = (e: globalThis.KeyboardEvent) => e.key === 'Escape' && close.current();
    document.addEventListener('keydown', key);
    requestAnimationFrame(() => {
      const f = pn.current?.querySelector<HTMLElement>('input:not([type=hidden]),select,textarea') ?? pn.current?.querySelector<HTMLElement>('button');
      if (window.matchMedia?.('(min-width: 761px)').matches) f?.focus();
    });
    return () => {
      document.removeEventListener('keydown', key);
      prev?.focus?.();
    };
  }, [open]);
  if (!open) return null;
  return (
    <Portal>
      <div className="sheet" role="dialog" aria-modal="true" aria-labelledby={id}>
        <div className="sheet-bd" onClick={onClose} />
        <div className="sheet-pn" ref={pn} style={width ? ({ '--sheet-w': `${width}px` } as CSSProperties) : undefined}>
          <header>
            <h2 id={id}>{title}</h2>
            <Button size="sm" variant="ghost" onClick={onClose}>
              关闭
            </Button>
          </header>
          <div className={cx('sheet-b', flush && 'flush')}>{children}</div>
          {footer && <footer>{footer}</footer>}
        </div>
      </div>
    </Portal>
  );
}

/** P5 手机主操作栏：≤ 760px 固定在底部标签栏上方，桌面不显示 */
export function MobileBar({ children }: { children: ReactNode }) {
  return (
    <Portal>
      <div className="m-bar">{children}</div>
    </Portal>
  );
}

/** P2 主从：宽屏左右并排；≤ 900px 一次只显示一级，详情顶部「返回」 */
export function MasterDetail({
  list,
  detail,
  showDetail,
  onBack,
  backLabel = '返回列表',
  listWidth,
  cols,
  joined,
}: {
  list: ReactNode;
  detail: ReactNode;
  /** 窄屏时当前显示详情 */
  showDetail: boolean;
  onBack(): void;
  backLabel?: ReactNode;
  listWidth?: number;
  /** 完整列定义（如 `minmax(0, 1fr) 360px`），经 CSS 变量传入 */
  cols?: string;
  /** 两级合在一个面板里（站内信、工单对话） */
  joined?: boolean;
}) {
  const style = { ...(listWidth ? { '--md-l': `${listWidth}px` } : {}), ...(cols ? { '--md-cols': cols } : {}) } as CSSProperties;
  return (
    <div className={cx('md', joined && 'joined')} data-md={showDetail ? 'detail' : 'list'} style={style} data-primary>
      <div className="md-list">{list}</div>
      <div className="md-detail">
        <Button size="sm" variant="ghost" className="md-back" onClick={onBack}>
          ‹ {backLabel}
        </Button>
        {detail}
      </div>
    </div>
  );
}

// ---------------- P1 筛选 ----------------

/** 常用条件一行；高级条件放在 `advanced`（桌面列表上方展开，手机底部抽屉） */
export function FilterBar({
  quick,
  right,
  advanced,
  activeCount = 0,
  open,
  onOpenChange,
  onReset,
  applyLabel = '显示结果',
  title = '筛选',
  active,
}: {
  quick?: ReactNode;
  right?: ReactNode;
  advanced?: ReactNode;
  activeCount?: number;
  open?: boolean;
  onOpenChange?(v: boolean): void;
  onReset?(): void;
  applyLabel?: ReactNode;
  title?: string;
  /** 已选条件（可删除的标签），见 ActiveFilters */
  active?: ReactNode;
}) {
  const id = useId();
  const narrow = useMediaQuery('(max-width: 760px)');
  const close = () => onOpenChange?.(false);
  useEffect(() => {
    if (!open || !narrow) return;
    const key = (e: globalThis.KeyboardEvent) => e.key === 'Escape' && close();
    document.addEventListener('keydown', key);
    return () => document.removeEventListener('keydown', key);
  });
  const panel = advanced && (
    <div className={cx('adv', open && 'open')} id={id} onClick={(e) => e.target === e.currentTarget && close()}>
      <div className="adv-pn" role={narrow ? 'dialog' : undefined} aria-modal={narrow || undefined} aria-label={title}>
        <header>
          <h2>{title}</h2>
          <Button size="sm" variant="ghost" onClick={close}>
            关闭
          </Button>
        </header>
        <div className="adv-b">{advanced}</div>
        <footer>
          {onReset && <Button onClick={onReset}>重置</Button>}
          <Button variant="primary" onClick={close}>
            {applyLabel}
          </Button>
        </footer>
      </div>
    </div>
  );
  return (
    <>
      <div className="fbar">
        {quick && <div className="quick">{quick}</div>}
        {advanced && (
          <Button size="sm" className="adv-btn" aria-expanded={!!open} aria-controls={id} onClick={() => onOpenChange?.(!open)}>
            {title} {activeCount > 0 && <em>{activeCount}</em>}
          </Button>
        )}
        <span className="grow" />
        {right}
      </div>
      {active}
      {panel && (narrow && open ? <Portal>{panel}</Portal> : panel)}
    </>
  );
}

/** 筛选面板中的一行：标签 + 选项 */
export function FilterRow({ label, children }: { label: ReactNode; children: ReactNode }) {
  return (
    <div className="frow">
      <span>{label}</span>
      {children}
    </div>
  );
}

export function ActiveFilters({ items, onClear }: { items: { key: string; label: ReactNode; value: ReactNode; onRemove(): void }[]; onClear?(): void }) {
  if (items.length === 0) return null;
  return (
    <div className="act-row">
      {items.map((x) => (
        <button key={x.key} type="button" className="chip-x" onClick={x.onRemove}>
          <span>{x.label}</span>
          {x.value} ×
        </button>
      ))}
      {onClear && (
        <button type="button" className="txt-btn" onClick={onClear}>
          清除全部
        </button>
      )}
    </div>
  );
}

// ---------------- 用户内容（唯一允许插入 HTML 的组件，web LLD §7） ----------------

const ALLOWED_TAGS = ['a', 'b', 'strong', 'i', 'em', 'u', 's', 'del', 'p', 'br', 'hr', 'ul', 'ol', 'li', 'blockquote', 'cite', 'code', 'pre', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'table', 'thead', 'tbody', 'tr', 'th', 'td', 'img', 'span', 'div', 'details', 'summary', 'input'];
const ALLOWED_ATTR = ['href', 'src', 'alt', 'title', 'colspan', 'rowspan', 'width', 'height', 'loading', 'class', 'style', 'type', 'disabled', 'checked'];
/** 服务端渲染器（pt-markup）只生成这些 class 与样式属性 */
const ALLOWED_CLASSES = new Set(['mention', 'center', 'mediainfo', 'emoji']);
const ALLOWED_STYLES = ['color', 'font-size'];

export function sanitize(html: string): string {
  const clean = DOMPurify.sanitize(html, { ALLOWED_TAGS, ALLOWED_ATTR, ALLOW_DATA_ATTR: false });
  // 链接一律 nofollow noopener ugc，新窗口打开
  const doc = new DOMParser().parseFromString(`<div>${clean}</div>`, 'text/html');
  doc.querySelectorAll('a').forEach((a) => {
    a.setAttribute('rel', 'nofollow noopener noreferrer ugc');
    a.setAttribute('target', '_blank');
  });
  // 样式只保留颜色与字号，class 只保留渲染器生成的，输入框只允许只读复选框（任务列表）
  doc.querySelectorAll<HTMLElement>('[style]').forEach((el) => {
    const keep = ALLOWED_STYLES.map((p) => [p, el.style.getPropertyValue(p)] as const).filter(([, v]) => v && !/url\(|expression/i.test(v));
    el.removeAttribute('style');
    for (const [p, v] of keep) el.style.setProperty(p, v);
    if (!el.getAttribute('style')) el.removeAttribute('style');
  });
  doc.querySelectorAll('[class]').forEach((el) => {
    const cls = [...el.classList].filter((c) => ALLOWED_CLASSES.has(c));
    if (cls.length) el.setAttribute('class', cls.join(' '));
    else el.removeAttribute('class');
  });
  doc.querySelectorAll('input').forEach((el) => {
    if (el.getAttribute('type') !== 'checkbox') el.remove();
    else el.setAttribute('disabled', '');
  });
  return doc.body.firstElementChild?.innerHTML ?? '';
}

/** 渲染服务端已净化的 HTML（二次净化）；`text` 时按纯文本渲染 */
export function UserContent({ html, text, className }: { html?: string; text?: string; className?: string }) {
  const safe = useMemo(() => (html !== undefined ? sanitize(html) : null), [html]);
  if (safe === null) return <div className={cx('pt-user-content', 'pt-pre', className)}>{text ?? ''}</div>;
  // eslint-disable-next-line react/no-danger -- 唯一允许的 HTML 出口
  return <div className={cx('pt-user-content', className)} dangerouslySetInnerHTML={{ __html: safe }} />;
}

// ---------------- Markdown 编辑器 ----------------

const MD_LABELS = {
  'zh-CN': { write: '编辑', preview: '预览', bold: '粗体（Ctrl+B）', italic: '斜体（Ctrl+I）', strike: '删除线', link: '链接（Ctrl+K）', image: '图片', quote: '引用', code: '代码', ul: '无序列表', ol: '有序列表', table: '表格', empty: '没有可预览的内容', hint: '支持 Markdown' },
  en: { write: 'Write', preview: 'Preview', bold: 'Bold (Ctrl+B)', italic: 'Italic (Ctrl+I)', strike: 'Strikethrough', link: 'Link (Ctrl+K)', image: 'Image', quote: 'Quote', code: 'Code', ul: 'Bulleted list', ol: 'Numbered list', table: 'Table', empty: 'Nothing to preview', hint: 'Markdown supported' },
};
type MdLabels = (typeof MD_LABELS)['en'];
const MD_SHORT = { 'zh-CN': { bold: 'B', italic: 'I', strike: 'S', link: '链接', image: '图片', quote: '引用', code: '代码', ul: '列表', ol: '编号', table: '表格' }, en: { bold: 'B', italic: 'I', strike: 'S', link: 'Link', image: 'Img', quote: 'Quote', code: 'Code', ul: 'List', ol: '1.', table: 'Table' } };

/** 在选区两侧加标记；无选区时插入占位文字并选中 */
export function mdWrap(v: string, start: number, end: number, before: string, after: string, placeholder: string): [string, number, number] {
  const sel = v.slice(start, end) || placeholder;
  const out = v.slice(0, start) + before + sel + after + v.slice(end);
  return [out, start + before.length, start + before.length + sel.length];
}

/** 给选区涉及的每一行加前缀（引用、列表）；有序列表逐行编号 */
export function mdPrefix(v: string, start: number, end: number, prefix: (i: number) => string): [string, number, number] {
  const ls = v.lastIndexOf('\n', start - 1) + 1;
  const le = v.indexOf('\n', end) === -1 ? v.length : v.indexOf('\n', end);
  const lines = v.slice(ls, le).split('\n').map((l, i) => prefix(i) + l);
  const block = lines.join('\n');
  return [v.slice(0, ls) + block + v.slice(le), ls, ls + block.length];
}

/**
 * Markdown 编辑器：工具栏、快捷键、编辑 / 预览切换。
 * 预览由调用方提供（通常调用服务端渲染接口，与最终显示一致）；不提供时不显示预览页签。
 */
export function MarkdownEditor({
  value,
  onChange,
  preview,
  name,
  rows = 8,
  placeholder,
  required,
  maxLength,
  labels,
  ariaLabel,
}: {
  value: string;
  onChange(v: string): void;
  preview?: (src: string) => Promise<string>;
  name?: string;
  rows?: number;
  placeholder?: string;
  required?: boolean;
  maxLength?: number;
  labels?: Partial<MdLabels>;
  ariaLabel?: string;
}) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const [tab, setTab] = useState<'write' | 'preview'>('write');
  const [html, setHtml] = useState<string | null>(null);
  const lang = typeof document !== 'undefined' && document.documentElement.lang.startsWith('en') ? 'en' : 'zh-CN';
  const L: MdLabels = { ...MD_LABELS[lang], ...labels };
  const S = MD_SHORT[lang];
  const apply = (fn: (v: string, s: number, e: number) => [string, number, number]) => {
    const ta = ref.current;
    if (!ta) return;
    const [out, s, e] = fn(value, ta.selectionStart, ta.selectionEnd);
    onChange(out);
    requestAnimationFrame(() => {
      ta.focus();
      ta.setSelectionRange(s, e);
    });
  };
  const tools: { key: keyof typeof S; run(): void }[] = [
    { key: 'bold', run: () => apply((v, s, e) => mdWrap(v, s, e, '**', '**', 'text')) },
    { key: 'italic', run: () => apply((v, s, e) => mdWrap(v, s, e, '*', '*', 'text')) },
    { key: 'strike', run: () => apply((v, s, e) => mdWrap(v, s, e, '~~', '~~', 'text')) },
    { key: 'link', run: () => apply((v, s, e) => mdWrap(v, s, e, '[', '](https://)', 'link')) },
    { key: 'image', run: () => apply((v, s, e) => mdWrap(v, s, e, '![', '](https://)', 'image')) },
    { key: 'quote', run: () => apply((v, s, e) => mdPrefix(v, s, e, () => '> ')) },
    { key: 'code', run: () => apply((v, s, e) => (v.slice(s, e).includes('\n') ? mdWrap(v, s, e, '```\n', '\n```', 'code') : mdWrap(v, s, e, '`', '`', 'code'))) },
    { key: 'ul', run: () => apply((v, s, e) => mdPrefix(v, s, e, () => '- ')) },
    { key: 'ol', run: () => apply((v, s, e) => mdPrefix(v, s, e, (i) => `${i + 1}. `)) },
    { key: 'table', run: () => apply((v, s, e) => mdWrap(v, s, e, '\n| A | B |\n| --- | --- |\n| ', ' |  |\n', '1')) },
  ];
  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (!(e.ctrlKey || e.metaKey)) return;
    const k = ({ b: 'bold', i: 'italic', k: 'link' } as Record<string, string>)[e.key.toLowerCase()];
    const t = tools.find((x) => x.key === k);
    if (t) {
      e.preventDefault();
      t.run();
    }
  };
  const showPreview = () => {
    setTab('preview');
    setHtml(null);
    if (preview && value.trim()) void preview(value).then(setHtml, (err: unknown) => setHtml(`<p>${String(err).replace(/[<>&]/g, '')}</p>`));
  };
  return (
    // 输入区在 DOM 中排在工具栏之前：外层 <label>（Field）点击时聚焦输入框；工具栏用 CSS order 显示在上方
    <div className="md-editor">
      <textarea ref={ref} name={name} aria-label={ariaLabel} hidden={tab !== 'write'} rows={rows} placeholder={placeholder ?? L.hint} required={required && tab === 'write'} maxLength={maxLength} value={value} onChange={(e) => onChange(e.target.value)} onKeyDown={onKey} />
      {tab === 'preview' && (
        <div className="md-preview" data-testid="md-preview">
          {!value.trim() ? <span className="muted">{L.empty}</span> : html === null ? <Spinner /> : <UserContent html={html} />}
        </div>
      )}
      <div className="md-bar">
        {preview && (
          <div role="tablist">
            <button type="button" role="tab" aria-selected={tab === 'write'} onClick={() => setTab('write')}>
              {L.write}
            </button>
            <button type="button" role="tab" aria-selected={tab === 'preview'} onClick={showPreview}>
              {L.preview}
            </button>
          </div>
        )}
        {tab === 'write' && (
          <div className="md-tools" role="toolbar">
            {tools.map((t) => (
              <button key={t.key} type="button" title={L[t.key]} aria-label={L[t.key]} onClick={t.run}>
                {t.key === 'bold' ? <b>B</b> : t.key === 'italic' ? <i>I</i> : t.key === 'strike' ? <s>S</s> : S[t.key]}
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

// ---------------- 提示 ----------------

type ToastKind = 'info' | 'success' | 'error';
interface ToastItem {
  id: number;
  msg: string;
  kind: ToastKind;
}

const ToastCtx = createContext<(msg: string, kind?: ToastKind) => void>(() => undefined);

function ToastEntry({ item, onDismiss }: { item: ToastItem; onDismiss: (id: number) => void }) {
  const [hover, setHover] = useState(false);
  useEffect(() => {
    if (hover) return;
    const t = setTimeout(() => onDismiss(item.id), item.kind === 'error' ? 6000 : 3500);
    return () => clearTimeout(t);
  }, [hover, item, onDismiss]);
  return (
    <div role={item.kind === 'error' ? 'alert' : 'status'} className={cx('toast', `toast-${item.kind}`)} onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}>
      <span className="msg">{item.msg}</span>
      <button type="button" onClick={() => onDismiss(item.id)} aria-label="关闭提示" title="关闭">
        ×
      </button>
    </div>
  );
}

export function ToastHost({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const dismiss = useCallback((id: number) => setItems((xs) => xs.filter((x) => x.id !== id)), []);
  const push = useCallback((msg: string, kind: ToastKind = 'info') => {
    const id = Date.now() + Math.random();
    setItems((xs) => [...xs.slice(-3), { id, msg, kind }]);
  }, []);
  return (
    <ToastCtx.Provider value={push}>
      {children}
      <Portal>
        <div className="toasts" aria-live="polite">
          {items.map((t) => (
            <ToastEntry key={t.id} item={t} onDismiss={dismiss} />
          ))}
        </div>
      </Portal>
    </ToastCtx.Provider>
  );
}

export function useToast(): (msg: string, kind?: ToastKind) => void {
  return useContext(ToastCtx);
}

// ---------------- 复制 ----------------

export async function copyText(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.style.position = 'fixed';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand('copy');
    document.body.removeChild(ta);
    return ok;
  } catch {
    return false;
  }
}

export function CopyButton({
  text,
  label = '复制',
  copiedLabel = '已复制',
  title,
  variant = 'default',
  size = 'sm',
  className,
  onCopied,
}: {
  text: string;
  label?: ReactNode;
  copiedLabel?: ReactNode;
  title?: string;
  variant?: Variant;
  /** md = 标准按钮高度 */
  size?: 'xs' | 'sm' | 'md';
  className?: string;
  onCopied?: () => void;
}) {
  const [copied, setCopied] = useState(false);
  return (
    <Button
      variant={variant}
      size={size === 'md' ? undefined : size}
      className={cx(copied && 'btn-copied', className)}
      title={title}
      aria-live="polite"
      onClick={async () => {
        if (await copyText(text)) {
          setCopied(true);
          onCopied?.();
          setTimeout(() => setCopied(false), 2000);
        }
      }}
    >
      {copied ? copiedLabel : label}
    </Button>
  );
}

/** 等宽文本框 + 复制按钮（链接、密钥、邀请码） */
export function CodeBox({ children, copy, actions }: { children: ReactNode; copy?: string; actions?: ReactNode }) {
  return (
    <div className="code-box">
      <span>{children}</span>
      {copy !== undefined && <CopyButton text={copy} size="xs" />}
      {actions}
    </div>
  );
}

// ---------------- 回到顶部 / 跳到正文 ----------------

export function BackToTop({ threshold = 600, title = '回到顶部' }: { threshold?: number; title?: string }) {
  const [on, setOn] = useState(false);
  useEffect(() => {
    const f = () => setOn(window.scrollY > threshold);
    window.addEventListener('scroll', f, { passive: true });
    f();
    return () => window.removeEventListener('scroll', f);
  }, [threshold]);
  return (
    <button type="button" className={cx('back-top', on && 'on')} onClick={() => window.scrollTo({ top: 0, behavior: 'smooth' })} aria-label={title} tabIndex={on ? 0 : -1}>
      {title}
    </button>
  );
}

export function SkipLink({ targetId = 'main-content', children = '跳至主要内容' }: { targetId?: string; children?: ReactNode }) {
  return (
    <a href={`#${targetId}`} className="skip-link">
      {children}
    </a>
  );
}
