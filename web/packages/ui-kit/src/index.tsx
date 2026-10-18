// @pt-studio/ui-kit：基础组件（样式见 styles.css，全部基于主题令牌）。

import DOMPurify from 'dompurify';
import { createContext, isValidElement, useCallback, useContext, useEffect, useMemo, useRef, useState, type ButtonHTMLAttributes, type InputHTMLAttributes, type KeyboardEvent, type ReactNode } from 'react';

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

/** 空状态插画：摊开的魔导书与一朵苍月草（SVG 极简矢量版） */
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

/** 空状态精美插画：芙莉莲坐在苍月草花海中静心阅读魔导书 */
export function EmptyIllustration({
  className,
  src = '/frieren/frieren-empty-state-generated.jpg',
  alt = '',
}: {
  className?: string;
  src?: string;
  alt?: string;
}) {
  return (
    <div className={cx('pt-empty-illustration-wrap', className)}>
      <img className="pt-empty-illustration" src={src} alt={alt} loading="lazy" />
    </div>
  );
}

export function Empty({
  children,
  illustration,
}: {
  children?: ReactNode;
  illustration?: boolean | string;
}) {
  // 插件常用 `—` 作占位文案：只显示插画
  const blank = children === '—' || children === null || children === undefined;
  return (
    <div className="pt-empty">
      {illustration ? (
        <EmptyIllustration src={typeof illustration === 'string' ? illustration : undefined} />
      ) : (
        <EmptyArt />
      )}
      {!blank && <p>{children}</p>}
    </div>
  );
}

/** 用户头像：支持用户自定义头像，未配置或加载失败时优雅降级为芙莉莲默认头像或首字母缩写 */
export function Avatar({
  src,
  fallbackSrc = '/frieren/frieren-avatar-generated.jpg',
  name,
  size = 32,
  className,
}: {
  src?: string | null;
  fallbackSrc?: string | null;
  name?: string;
  size?: number;
  className?: string;
}) {
  const [imgError, setImgError] = useState(false);
  const [fallbackError, setFallbackError] = useState(false);
  const initial = (name || '?').slice(0, 1).toUpperCase();
  const effectiveSrc = !imgError && src ? src : (!fallbackError && fallbackSrc ? fallbackSrc : null);

  return (
    <span
      className={cx('pt-avatar', className)}
      style={{ width: size, height: size, minWidth: size, minHeight: size }}
      aria-hidden="true"
    >
      {effectiveSrc ? (
        <img
          src={effectiveSrc}
          alt={name ? `${name}` : ''}
          onError={() => {
            if (src && !imgError) setImgError(true);
            else setFallbackError(true);
          }}
        />
      ) : (
        <span className="pt-avatar-fallback">{initial}</span>
      )}
    </span>
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

// ---------------- Markdown 编辑器 ----------------

const MD_LABELS = {
  'zh-CN': { write: '编辑', preview: '预览', bold: '粗体（Ctrl+B）', italic: '斜体（Ctrl+I）', strike: '删除线', link: '链接（Ctrl+K）', image: '图片', quote: '引用', code: '代码', ul: '无序列表', ol: '有序列表', table: '表格', empty: '没有可预览的内容', hint: '支持 Markdown' },
  en: { write: 'Write', preview: 'Preview', bold: 'Bold (Ctrl+B)', italic: 'Italic (Ctrl+I)', strike: 'Strikethrough', link: 'Link (Ctrl+K)', image: 'Image', quote: 'Quote', code: 'Code', ul: 'Bulleted list', ol: 'Numbered list', table: 'Table', empty: 'Nothing to preview', hint: 'Markdown supported' },
};
type MdLabels = (typeof MD_LABELS)['en'];

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
 * Markdown 编辑器：工具栏（粗体、斜体、删除线、链接、图片、引用、代码、列表、表格）、快捷键、编辑 / 预览切换。
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
  const tools: { key: keyof MdLabels; icon: ReactNode; run(): void }[] = [
    { key: 'bold', icon: <b>B</b>, run: () => apply((v, s, e) => mdWrap(v, s, e, '**', '**', 'text')) },
    { key: 'italic', icon: <i>I</i>, run: () => apply((v, s, e) => mdWrap(v, s, e, '*', '*', 'text')) },
    { key: 'strike', icon: <s>S</s>, run: () => apply((v, s, e) => mdWrap(v, s, e, '~~', '~~', 'text')) },
    { key: 'link', icon: '🔗', run: () => apply((v, s, e) => mdWrap(v, s, e, '[', '](https://)', 'link')) },
    { key: 'image', icon: '🖼', run: () => apply((v, s, e) => mdWrap(v, s, e, '![', '](https://)', 'image')) },
    { key: 'quote', icon: '❝', run: () => apply((v, s, e) => mdPrefix(v, s, e, () => '> ')) },
    { key: 'code', icon: <code>{'</>'}</code>, run: () => apply((v, s, e) => (v.slice(s, e).includes('\n') ? mdWrap(v, s, e, '```\n', '\n```', 'code') : mdWrap(v, s, e, '`', '`', 'code'))) },
    { key: 'ul', icon: '•', run: () => apply((v, s, e) => mdPrefix(v, s, e, () => '- ')) },
    { key: 'ol', icon: '1.', run: () => apply((v, s, e) => mdPrefix(v, s, e, (i) => `${i + 1}. `)) },
    { key: 'table', icon: '▦', run: () => apply((v, s, e) => mdWrap(v, s, e, '\n| A | B |\n| --- | --- |\n| ', ' |  |\n', '1')) },
  ];
  const onKey = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (!(e.ctrlKey || e.metaKey)) return;
    const k = { b: 'bold', i: 'italic', k: 'link' }[e.key.toLowerCase()];
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
    // 输入区在 DOM 中排在工具栏之前：外层 <label>（Field）点击时聚焦输入框而不是第一个工具按钮；工具栏用 CSS order 显示在上方
    <div className="pt-md-editor">
      {/* 预览时输入框保持挂载（隐藏）：外层 <label> 始终关联到它，页签按钮不会被当作被标注的控件 */}
      <textarea ref={ref} className="pt-input" name={name} aria-label={ariaLabel} hidden={tab !== 'write'} rows={rows} placeholder={placeholder ?? L.hint} required={required && tab === 'write'} maxLength={maxLength} value={value} onChange={(e) => onChange(e.target.value)} onKeyDown={onKey} />
      {tab === 'preview' && (
        <div className="pt-md-preview" data-testid="md-preview">
          {!value.trim() ? <span className="pt-muted">{L.empty}</span> : html === null ? <Spinner /> : <UserContent html={html} />}
        </div>
      )}
      <div className="pt-md-bar">
        {preview && (
          <div role="tablist" className="pt-md-tabs">
            <button type="button" role="tab" aria-selected={tab === 'write'} className="pt-tab" onClick={() => setTab('write')}>
              {L.write}
            </button>
            <button type="button" role="tab" aria-selected={tab === 'preview'} className="pt-tab" onClick={showPreview}>
              {L.preview}
            </button>
          </div>
        )}
        {tab === 'write' && (
          <div className="pt-md-tools" role="toolbar">
            {tools.map((t) => (
              <button key={t.key} type="button" className="pt-md-tool" title={L[t.key]} aria-label={L[t.key]} onClick={t.run}>
                {t.icon}
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
  const [hovered, setHovered] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const startTimer = useCallback(() => {
    if (timerRef.current) clearTimeout(timerRef.current);
    timerRef.current = setTimeout(() => onDismiss(item.id), 4000);
  }, [item.id, onDismiss]);

  useEffect(() => {
    if (!hovered) {
      startTimer();
    } else if (timerRef.current) {
      clearTimeout(timerRef.current);
    }
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, [hovered, startTimer]);

  const icon = item.kind === 'success' ? '✓' : item.kind === 'error' ? '!' : 'ℹ';

  return (
    <div
      role="status"
      className={cx('pt-toast', `pt-toast-${item.kind}`)}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      <span className="pt-toast-icon" aria-hidden="true">{icon}</span>
      <div className="pt-toast-msg">{item.msg}</div>
      <button
        type="button"
        className="pt-toast-close"
        onClick={() => onDismiss(item.id)}
        aria-label="关闭提示"
        title="关闭"
      >
        ×
      </button>
    </div>
  );
}

export function ToastHost({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const dismiss = useCallback((id: number) => {
    setItems((xs) => xs.filter((x) => x.id !== id));
  }, []);
  const push = useCallback((msg: string, kind: ToastKind = 'info') => {
    const id = Date.now() + Math.random();
    setItems((xs) => [...xs, { id, msg, kind }]);
  }, []);
  return (
    <ToastCtx.Provider value={push}>
      {children}
      <div className="pt-toasts" aria-live="polite">
        {items.map((t) => (
          <ToastEntry key={t.id} item={t} onDismiss={dismiss} />
        ))}
      </div>
    </ToastCtx.Provider>
  );
}

export function useToast(): (msg: string, kind?: ToastKind) => void {
  return useContext(ToastCtx);
}

// ---------------- 快捷复制 ----------------

export function CopyButton({
  text,
  label,
  copiedLabel,
  title,
  variant = 'default',
  className,
  onCopied,
}: {
  text: string;
  label?: ReactNode;
  copiedLabel?: ReactNode;
  title?: string;
  variant?: 'default' | 'primary' | 'danger' | 'ghost';
  className?: string;
  onCopied?: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const copy = useCallback(async () => {
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(text);
      } else {
        const ta = document.createElement('textarea');
        ta.value = text;
        ta.style.position = 'fixed';
        ta.style.opacity = '0';
        document.body.appendChild(ta);
        ta.focus();
        ta.select();
        document.execCommand('copy');
        document.body.removeChild(ta);
      }
      setCopied(true);
      onCopied?.();
      setTimeout(() => setCopied(false), 2000);
    } catch {
      /* 忽略复制失败 */
    }
  }, [text, onCopied]);

  return (
    <button
      type="button"
      className={cx('pt-btn', 'pt-copy-btn', `pt-btn-${variant}`, copied && 'copied', className)}
      onClick={copy}
      title={title ?? (copied ? '已复制' : '复制')}
      aria-label={title ?? (copied ? '已复制' : '复制')}
      aria-live="polite"
    >
      {copied ? (
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <polyline points="20 6 9 17 4 12" />
        </svg>
      ) : (
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
          <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
        </svg>
      )}
      {copied ? (copiedLabel ?? (label ? '已复制' : null)) : label}
    </button>
  );
}

// ---------------- 回到顶部 ----------------

export function BackToTop({ threshold = 320, title = '回到顶部' }: { threshold?: number; title?: string }) {
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    let ticking = false;
    const onScroll = () => {
      if (!ticking) {
        window.requestAnimationFrame(() => {
          setVisible(window.scrollY > threshold);
          ticking = false;
        });
        ticking = true;
      }
    };
    window.addEventListener('scroll', onScroll, { passive: true });
    onScroll();
    return () => window.removeEventListener('scroll', onScroll);
  }, [threshold]);

  const scrollToTop = () => {
    window.scrollTo({ top: 0, behavior: 'smooth' });
  };

  return (
    <button
      type="button"
      className={cx('pt-back-to-top', visible && 'visible')}
      onClick={scrollToTop}
      title={title}
      aria-label={title}
    >
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
        <path d="M12 19V5M5 12l7-7 7 7" />
      </svg>
    </button>
  );
}

// ---------------- 跳至主要内容（无障碍） ----------------

export function SkipLink({ targetId = 'main-content', children = '跳至主要内容' }: { targetId?: string; children?: ReactNode }) {
  return (
    <a href={`#${targetId}`} className="pt-skip-link">
      {children}
    </a>
  );
}
