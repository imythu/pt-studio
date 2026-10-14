// @pt-studio/ui-kit：基础组件（样式见 styles.css，全部基于主题令牌）。

import DOMPurify from 'dompurify';
import { createContext, isValidElement, useCallback, useContext, useMemo, useState, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode } from 'react';

export function cx(...xs: (string | false | null | undefined)[]): string {
  return xs.filter(Boolean).join(' ');
}

export function Button({ variant = 'default', className, ...p }: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: 'default' | 'primary' | 'danger' | 'ghost' }) {
  return <button type="button" {...p} className={cx('pt-btn', `pt-btn-${variant}`, className)} />;
}

export function Card({ title, actions, children, className }: { title?: ReactNode; actions?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={cx('pt-card', className)}>
      {(title || actions) && (
        <header className="pt-card-head">
          {title && <h2 className="pt-card-title">{title}</h2>}
          {actions && <div className="pt-card-actions">{actions}</div>}
        </header>
      )}
      <div className="pt-card-body">{children}</div>
    </section>
  );
}

export function Badge({ tone = 'muted', children, title }: { tone?: 'muted' | 'primary' | 'success' | 'danger' | 'free' | 'double'; children: ReactNode; title?: string }) {
  return (
    <span className={cx('pt-badge', `pt-badge-${tone}`)} title={title}>
      {children}
    </span>
  );
}

export function Stat({ label, value, hint }: { label: ReactNode; value: ReactNode; hint?: ReactNode }) {
  return (
    <div className="pt-stat">
      <div className="pt-stat-label">{label}</div>
      <div className="pt-stat-value">{value}</div>
      {hint && <div className="pt-stat-hint">{hint}</div>}
    </div>
  );
}

export function Progress({ value, max = 1, label }: { value: number; max?: number; label?: string }) {
  const pct = Math.max(0, Math.min(100, (value / (max || 1)) * 100));
  return (
    <div className="pt-progress" role="progressbar" aria-valuenow={Math.round(pct)} aria-valuemin={0} aria-valuemax={100} aria-label={label}>
      <div className="pt-progress-bar" style={{ width: `${pct}%` }} />
    </div>
  );
}

export function Field({ label, error, hint, children }: { label: ReactNode; error?: string | null; hint?: ReactNode; children: ReactNode }) {
  return (
    <label className={cx('pt-field', error && 'pt-field-invalid')}>
      <span className="pt-field-label">{label}</span>
      {children}
      {hint && !error && <span className="pt-field-hint">{hint}</span>}
      {error && <span className="pt-field-error">{error}</span>}
    </label>
  );
}

export function Input(p: InputHTMLAttributes<HTMLInputElement>) {
  return <input {...p} className={cx('pt-input', p.className)} />;
}

export function Spinner() {
  return <span className="pt-spinner" aria-label="loading" />;
}

/** 空状态插画（主题色随令牌变化） */
/** 空状态插画：摊开的魔导书与一朵苍月草 */
export function EmptyArt({ className }: { className?: string }) {
  return (
    <svg className={cx('pt-empty-art', className)} viewBox="0 0 160 120" fill="none" aria-hidden="true">
      <ellipse cx="80" cy="110" rx="56" ry="6" fill="currentColor" opacity=".07" />
      <g transform="translate(80 30)" stroke="var(--pt-color-gold)" fill="none" opacity=".8">
        <circle r="18" strokeWidth="1.3" />
        <circle r="13" strokeWidth=".9" strokeDasharray="2 3" />
      </g>
      <path d="M80 30l2 4.6 4.6 2-4.6 2L80 43l-2-4.4-4.6-2 4.6-2Z" fill="var(--pt-color-gold)" />
      <path d="M24 56Q52 46 80 58Q108 46 136 56V104Q108 96 80 106Q52 96 24 104Z" fill="#26314f" />
      <path d="M30 54Q55 44 80 56V100Q55 90 30 98Z" fill="var(--pt-color-surface)" stroke="var(--pt-color-border-strong)" strokeWidth="1.5" />
      <path d="M130 54Q105 44 80 56V100Q105 90 130 98Z" fill="var(--pt-color-surface)" stroke="var(--pt-color-border-strong)" strokeWidth="1.5" />
      <path d="M40 64q16-5 32 1M40 72q16-5 32 1M40 80q12-4 24 0M88 65q16-6 32-1M88 73q16-6 32-1M88 81q10-4 22-1" stroke="var(--pt-color-muted)" strokeWidth="1.4" strokeLinecap="round" opacity=".45" />
      <g transform="translate(134 96)">
        <path d="M0 2v12" stroke="#4f8f6d" strokeWidth="1.4" />
        {[0, 72, 144, 216, 288].map((r) => (
          <ellipse key={r} cx="0" cy="-4.5" rx="2.7" ry="4.4" fill="#6f9fd8" transform={`rotate(${r})`} />
        ))}
        <circle r="1.8" fill="#f2d48a" />
      </g>
    </svg>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  // 插件常用 `—` 作占位文案：只显示插画
  const blank = children === '—' || children === null || children === undefined;
  return (
    <div className="pt-empty">
      <EmptyArt />
      {!blank && <p>{children}</p>}
    </div>
  );
}

/** 列表加载占位 */
export function SkeletonRows({ n = 5 }: { n?: number }) {
  return (
    <div aria-busy="true" aria-label="loading">
      {Array.from({ length: n }, (_, i) => (
        <span key={i} className="pt-skeleton pt-skeleton-block" />
      ))}
    </div>
  );
}

export interface Column<T> {
  key: string;
  header: ReactNode;
  render(row: T): ReactNode;
  className?: string;
}

export function Table<T>({ rows, columns, rowKey, empty }: { rows: T[]; columns: Column<T>[]; rowKey(row: T): string | number; empty?: ReactNode }) {
  if (rows.length === 0 && empty) return isValidElement(empty) ? <>{empty}</> : <Empty>{empty}</Empty>;
  return (
    <div className="pt-table-wrap">
      <table className="pt-table">
        <thead>
          <tr>
            {columns.map((c) => (
              <th key={c.key} className={c.className}>
                {c.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={rowKey(r)}>
              {columns.map((c) => (
                <td key={c.key} className={c.className}>
                  {c.render(r)}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function Tabs({ tabs, value, onChange }: { tabs: { id: string; label: ReactNode }[]; value: string; onChange(id: string): void }) {
  return (
    <div role="tablist" className="pt-tabs">
      {tabs.map((t) => (
        <button key={t.id} type="button" role="tab" aria-selected={t.id === value} className="pt-tab" onClick={() => onChange(t.id)}>
          {t.label}
        </button>
      ))}
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

// ---------------- 提示 ----------------

type ToastKind = 'info' | 'success' | 'error';
interface ToastItem {
  id: number;
  msg: string;
  kind: ToastKind;
}

const ToastCtx = createContext<(msg: string, kind?: ToastKind) => void>(() => undefined);

export function ToastHost({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const push = useCallback((msg: string, kind: ToastKind = 'info') => {
    const id = Date.now() + Math.random();
    setItems((xs) => [...xs, { id, msg, kind }]);
    setTimeout(() => setItems((xs) => xs.filter((x) => x.id !== id)), 4000);
  }, []);
  return (
    <ToastCtx.Provider value={push}>
      {children}
      <div className="pt-toasts" aria-live="polite">
        {items.map((t) => (
          <div key={t.id} className={cx('pt-toast', `pt-toast-${t.kind}`)}>
            {t.msg}
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}

export function useToast(): (msg: string, kind?: ToastKind) => void {
  return useContext(ToastCtx);
}
