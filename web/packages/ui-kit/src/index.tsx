// @pt-studio/ui-kit：基础组件（样式见 styles.css，全部基于主题令牌）。

import DOMPurify from 'dompurify';
import { createContext, useCallback, useContext, useMemo, useState, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode } from 'react';

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

export function Empty({ children }: { children: ReactNode }) {
  return <p className="pt-empty">{children}</p>;
}

export interface Column<T> {
  key: string;
  header: ReactNode;
  render(row: T): ReactNode;
  className?: string;
}

export function Table<T>({ rows, columns, rowKey, empty }: { rows: T[]; columns: Column<T>[]; rowKey(row: T): string | number; empty?: ReactNode }) {
  if (rows.length === 0 && empty) return <Empty>{empty}</Empty>;
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

const ALLOWED_TAGS = ['a', 'b', 'strong', 'i', 'em', 'u', 's', 'del', 'p', 'br', 'hr', 'ul', 'ol', 'li', 'blockquote', 'code', 'pre', 'h1', 'h2', 'h3', 'h4', 'table', 'thead', 'tbody', 'tr', 'th', 'td', 'img', 'span', 'details', 'summary'];
const ALLOWED_ATTR = ['href', 'src', 'alt', 'title', 'colspan', 'rowspan'];

export function sanitize(html: string): string {
  const clean = DOMPurify.sanitize(html, { ALLOWED_TAGS, ALLOWED_ATTR, ALLOW_DATA_ATTR: false });
  // 链接一律 nofollow noopener ugc，新窗口打开
  const doc = new DOMParser().parseFromString(`<div>${clean}</div>`, 'text/html');
  doc.querySelectorAll('a').forEach((a) => {
    a.setAttribute('rel', 'nofollow noopener noreferrer ugc');
    a.setAttribute('target', '_blank');
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
