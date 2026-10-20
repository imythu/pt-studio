// 新手帮助：首次下载引导（抽屉）、帮助页。
import { useCallback, useState } from 'react';
import { Link } from 'react-router';
import { useT } from '@pt-studio/terms';
import { Button, Checkbox, PageHead, Panel, Sheet, SideLayout } from '@pt-studio/ui-kit';

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
  return (
    <Sheet
      open={ctl.open}
      onClose={() => ctl.close(never)}
      title={t('ui.help.dl_title')}
      footer={
        <>
          <Link to="/help" className="btn" onClick={() => ctl.close(never)}>
            {t('ui.help.more')}
          </Link>
          <Button variant="primary" onClick={() => ctl.close(never)}>
            {t('ui.help.got_it')}
          </Button>
        </>
      }
    >
      <ol className="guide-steps">
        {[1, 2, 3].map((i) => (
          <li key={i}>
            <b>{t(`ui.help.dl_${i}_title`)}</b>
            <span>{t(`ui.help.dl_${i}`)}</span>
          </li>
        ))}
      </ol>
      <Checkbox label={t('ui.help.dont_show')} checked={never} onChange={setNever} />
    </Sheet>
  );
}

const FAQ = ['torrent', 'client', 'seed', 'ratio', 'hnr', 'bonus', 'passkey', 'upload'];
const TERMS = ['seeders', 'leechers', 'completed', 'ratio'];

/** 新手指南 */
export function HelpPage() {
  const t = useT();
  return (
    <div className="help-page">
      <PageHead title={t('ui.help.title')} lede={t('ui.help.intro')} />
      <SideLayout
        side={
          <>
            <Panel title={t('ui.help.glossary')}>
              <dl className="kv">
                {TERMS.map((k) => (
                  <div key={k} className="kv-row">
                    <dt>{t(`ui.torrent.${k === 'ratio' ? 'ratio_term' : k}`)}</dt>
                    <dd>{t(`ui.help.${k}`)}</dd>
                  </div>
                ))}
              </dl>
            </Panel>
          </>
        }
      >
        <Panel primary>
          <ol className="guide-steps start">
            {[1, 2, 3].map((i) => (
              <li key={i}>
                <b>{t(`ui.help.start_${i}_title`)}</b>
                <span>{t(`ui.help.start_${i}`)}</span>
              </li>
            ))}
          </ol>
        </Panel>
        <Panel flush>
          <div className="faq">
            {FAQ.map((k, i) => (
              <details key={k} open={i < 2}>
                <summary>{t(`ui.help.q_${k}`)}</summary>
                <p>{t(`ui.help.a_${k}`)}</p>
              </details>
            ))}
          </div>
        </Panel>
      </SideLayout>
    </div>
  );
}
