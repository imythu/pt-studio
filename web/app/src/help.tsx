// 新手帮助：术语提示、首次下载引导、首页“新手三步”、帮助页。
import { useCallback, useEffect, useRef, useState } from 'react';
import { Link } from 'react-router';
import { useT } from '@pt-studio/terms';
import { Icon } from './art';

function load(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
function save(key: string, v: string) {
  try {
    localStorage.setItem(key, v);
  } catch {
    /* 隐私模式 */
  }
}

/** 术语旁的小问号：点一下显示解释（手机上没有悬停，所以用点击） */
export function Hint({ text }: { text: string }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (!open) return;
    const down = (e: MouseEvent) => ref.current && !ref.current.contains(e.target as Node) && setOpen(false);
    document.addEventListener('mousedown', down);
    return () => document.removeEventListener('mousedown', down);
  }, [open]);
  return (
    <span className="hint" ref={ref}>
      <button type="button" className="hint-btn" aria-label={text} aria-expanded={open} onClick={() => setOpen(!open)}>
        ?
      </button>
      {open && (
        <span className="hint-pop" role="tooltip">
          {text}
        </span>
      )}
    </span>
  );
}

export interface FirstTime {
  open: boolean;
  /** 首次（未勾选“不再提示”）才弹出 */
  trigger(): void;
  show(): void;
  close(never: boolean): void;
}

export function useFirstTime(key: string): FirstTime {
  const [open, setOpen] = useState(false);
  const trigger = useCallback(() => {
    if (load(key) !== 'off') setTimeout(() => setOpen(true), 300);
  }, [key]);
  const show = useCallback(() => setOpen(true), []);
  const close = useCallback(
    (never: boolean) => {
      if (never) save(key, 'off');
      setOpen(false);
    },
    [key],
  );
  return { open, trigger, show, close };
}

/** 点击“下载种子”后的三步引导 */
export function DownloadGuide({ ctl }: { ctl: FirstTime }) {
  const t = useT();
  const [never, setNever] = useState(false);
  useEffect(() => {
    if (!ctl.open) return;
    const k = (e: KeyboardEvent) => e.key === 'Escape' && ctl.close(never);
    document.addEventListener('keydown', k);
    return () => document.removeEventListener('keydown', k);
  }, [ctl, never]);
  if (!ctl.open) return null;
  return (
    <div className="modal-back" onClick={() => ctl.close(never)}>
      <div className="modal" role="dialog" aria-modal="true" aria-labelledby="dl-guide-title" onClick={(e) => e.stopPropagation()}>
        <h2 id="dl-guide-title">{t('ui.help.dl_title')}</h2>
        <ol className="steps">
          <li>
            <b>{t('ui.help.dl_1_title')}</b>
            <span>{t('ui.help.dl_1')}</span>
          </li>
          <li>
            <b>{t('ui.help.dl_2_title')}</b>
            <span>{t('ui.help.dl_2')}</span>
          </li>
          <li>
            <b>{t('ui.help.dl_3_title')}</b>
            <span>{t('ui.help.dl_3')}</span>
          </li>
        </ol>
        <label className="pt-check">
          <input type="checkbox" checked={never} onChange={(e) => setNever(e.target.checked)} /> {t('ui.help.dont_show')}
        </label>
        <div className="modal-actions">
          <Link to="/help" className="pt-btn" onClick={() => ctl.close(never)}>
            {t('ui.help.more')}
          </Link>
          <button type="button" className="pt-btn pt-btn-primary" onClick={() => ctl.close(never)}>
            {t('ui.help.got_it')}
          </button>
        </div>
      </div>
    </div>
  );
}

/** 首页“新手三步”，可关闭 */
export function GettingStarted() {
  const t = useT();
  const [hidden, setHidden] = useState(() => load('pt-guide-home') === 'off');
  if (hidden) return null;
  return (
    <section className="getting-started">
      <header>
        <h2>{t('ui.help.start_title')}</h2>
        <button
          type="button"
          className="icon-btn"
          aria-label={t('ui.help.hide')}
          title={t('ui.help.hide')}
          onClick={() => {
            save('pt-guide-home', 'off');
            setHidden(true);
          }}
        >
          <Icon name="close" size={16} />
        </button>
      </header>
      <ol className="gs-steps">
        <li>
          <span className="gs-n">1</span>
          <div>
            <b>{t('ui.help.start_1_title')}</b>
            <p>{t('ui.help.start_1')}</p>
          </div>
        </li>
        <li>
          <span className="gs-n">2</span>
          <div>
            <b>{t('ui.help.start_2_title')}</b>
            <p>{t('ui.help.start_2')}</p>
          </div>
        </li>
        <li>
          <span className="gs-n">3</span>
          <div>
            <b>{t('ui.help.start_3_title')}</b>
            <p>{t('ui.help.start_3')}</p>
          </div>
        </li>
      </ol>
      <Link to="/help" className="gs-more">
        {t('ui.help.more')} →
      </Link>
    </section>
  );
}

const FAQ = ['torrent', 'client', 'seed', 'ratio', 'hnr', 'bonus', 'passkey', 'upload'];

/** 新手指南 */
export function HelpPage() {
  const t = useT();
  return (
    <div className="help-page">
      <div className="help-hero">
        <img src="/frieren/frieren-magic-journey.jpg" alt="" className="help-hero-bg" />
        <div className="help-hero-content">
          <h1>{t('ui.help.title')}</h1>
          <p>{t('ui.help.intro')}</p>
        </div>
      </div>
      <div className="faq">
        {FAQ.map((k, i) => (
          <details key={k} open={i < 2}>
            <summary>{t(`ui.help.q_${k}`)}</summary>
            <p>{t(`ui.help.a_${k}`)}</p>
          </details>
        ))}
      </div>
    </div>
  );
}
