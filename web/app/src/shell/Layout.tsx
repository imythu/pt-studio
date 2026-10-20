// 外壳（UI-WEB §2）：左侧常驻栏（字标、我的数据、导航与插件导航插槽）、顶栏（搜索、昼夜、发布）、
// 窄屏的数据条与底部标签栏；后台路径下左栏换成后台导航；未登录时为无侧栏的整页。
import { useQuery } from '@tanstack/react-query';
import { useEffect, useRef, useState, type FormEvent, type ReactNode } from 'react';
import { Link, NavLink, Outlet, useLocation, useNavigate } from 'react-router';
import { api } from '@pt-studio/sdk';
import { Slot } from '@pt-studio/slots';
import { formatBytes, useT } from '@pt-studio/terms';
import { Avatar, BackToTop, Button, Note, Portal, SkipLink, cx } from '@pt-studio/ui-kit';
import { fmtRatio, fromUnits, levelName, shortBytes, shortNum, useMyBonus, useMyHnr, useMyLevel, useMyStats, useUnread } from '../lib';
import { useInstalled, useManifest } from '../manifest';
import { useSession } from '../session';
import { SuggestInput } from '../suggest';

// ---------------- 主题 ----------------

function currentTheme(): 'dark' | 'light' {
  const set = document.documentElement.dataset.theme;
  if (set === 'dark' || set === 'light') return set;
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

function ThemeButton() {
  const t = useT();
  const [theme, setTheme] = useState(currentTheme);
  const flip = () => {
    const next = currentTheme() === 'dark' ? 'light' : 'dark';
    document.documentElement.dataset.theme = next;
    try {
      localStorage.setItem('pt-theme', next);
    } catch {
      /* 隐私模式 */
    }
    setTheme(next);
  };
  return (
    <button type="button" className="txt-btn theme-btn" onClick={flip} aria-label={t('ui.theme.toggle')} title={t('ui.theme.toggle')}>
      {theme === 'dark' ? t('ui.theme.day') : t('ui.theme.night')}
    </button>
  );
}

// ---------------- 字标 ----------------

export function Wordmark({ small }: { small?: boolean }) {
  const m = useManifest();
  // 站点改名时显示站名；默认站名拆成「PT Studio」两段排版
  const [a, b] = m.site.name === 'PT Studio' ? ['PT', 'Studio'] : [m.site.name, ''];
  return (
    <Link to="/" className={cx('mark', small && 'mark-sm')} aria-label={m.site.name}>
      <b>{a}</b>
      {b && <i>{b}</i>}
    </Link>
  );
}

// ---------------- 我的数据 ----------------

function MyCard() {
  const t = useT();
  const s = useSession();
  const st = useMyStats();
  const bonus = useMyBonus();
  const hnr = useMyHnr();
  const lv = useMyLevel();
  const unread = useUnread();
  if (!s.me) return null;
  const cls = levelName(t, lv?.class);
  return (
    <section className="me-card" aria-label={t('ui.me.card')}>
      <Link to={`/users/${s.me.id}`} className="me-head">
        <Avatar src={(s.me as { avatar_url?: string | null }).avatar_url} name={s.me.username} size={42} ring />
        <span className="me-name">
          <b>{s.me.username}</b>
          {cls && <span className="cls">{cls}</span>}
        </span>
      </Link>
      {st && (
        <dl className="ledger">
          <dt>{t('ui.profile.uploaded')}</dt>
          <dd className="num up">{formatBytes(st.uploaded)}</dd>
          <dt>{t('ui.profile.downloaded')}</dt>
          <dd className="num down">{formatBytes(st.downloaded)}</dd>
          <dt>{t('ui.profile.ratio')}</dt>
          <dd className="num">{fmtRatio(st.ratio)}</dd>
          {bonus && (
            <>
              <dt>{t('term:bonus.currency')}</dt>
              <dd className="num gold">{fromUnits(bonus.available, bonus.scale).toLocaleString('en-US', { maximumFractionDigits: 1 })}</dd>
            </>
          )}
          <dt>{t('ui.me.seed_leech')}</dt>
          <dd className="num">
            <span className="up">{st.seeding_count ?? 0}</span> / <span className="down">{st.leeching_count ?? 0}</span>
          </dd>
          {hnr && (
            <>
              <dt>H&amp;R</dt>
              <dd className={cx('num', hnr.failed > 0 && 'down')}>
                {hnr.failed} / {hnr.max}
              </dd>
            </>
          )}
        </dl>
      )}
      <div className="me-foot">
        {unread.messagesOn && (
          <Link to="/p/messages/">
            {t('ui.nav.messages')} {unread.messages > 0 && <b>{unread.messages}</b>}
          </Link>
        )}
        {unread.notifyOn && (
          <Link to="/p/notify/">
            {t('ui.nav.notifications')} {unread.notify > 0 && <b>{unread.notify}</b>}
          </Link>
        )}
        <Link to={`/users/${s.me.id}`}>{t('ui.nav.profile')}</Link>
      </div>
    </section>
  );
}

/** 窄屏的数据条（≤ 1000px 显示，点击进个人页） */
function StatStrip() {
  const t = useT();
  const s = useSession();
  const st = useMyStats();
  const bonus = useMyBonus();
  if (!s.me || !st) return null;
  return (
    <Link to={`/users/${s.me.id}`} className="mstats" aria-label={t('ui.me.card')}>
      <span>
        <i>{t('ui.profile.uploaded')}</i>
        <b className="up">{shortBytes(st.uploaded)}</b>
      </span>
      <span>
        <i>{t('ui.profile.downloaded')}</i>
        <b className="down">{shortBytes(st.downloaded)}</b>
      </span>
      <span>
        <i>{t('ui.profile.ratio')}</i>
        <b>{fmtRatio(st.ratio)}</b>
      </span>
      {bonus && (
        <span>
          <i>{t('term:bonus.currency')}</i>
          <b className="gold">{shortNum(fromUnits(bonus.available, bonus.scale))}</b>
        </span>
      )}
      <span>
        <i>{t('ui.me.seeding')}</i>
        <b>{st.seeding_count ?? 0}</b>
      </span>
    </Link>
  );
}

// ---------------- 导航 ----------------

/** 插件导航插槽渲染的是普通链接：按当前路径给最匹配的一项标记 aria-current */
function useMarkCurrent(ref: React.RefObject<HTMLElement | null>) {
  const loc = useLocation();
  useEffect(() => {
    const root = ref.current;
    if (!root) return;
    const mark = () => {
      const links = [...root.querySelectorAll<HTMLAnchorElement>('a[href]')];
      let best: HTMLAnchorElement | null = null;
      let bestLen = -1;
      for (const a of links) {
        const p = new URL(a.href, location.origin).pathname;
        const hit = p === '/' ? loc.pathname === '/' : loc.pathname === p || loc.pathname.startsWith(p.endsWith('/') ? p : `${p}/`);
        if (hit && p.length > bestLen) {
          best = a;
          bestLen = p.length;
        }
      }
      links.forEach((a) => (a === best ? a.setAttribute('aria-current', 'page') : a.removeAttribute('aria-current')));
    };
    mark();
    // 插件导航异步加载：出现新链接时重新标记
    const mo = new MutationObserver(mark);
    mo.observe(root, { childList: true, subtree: true });
    return () => mo.disconnect();
  }, [loc.pathname, ref]);
}

function UserNav() {
  const t = useT();
  const s = useSession();
  const nav = useNavigate();
  const ref = useRef<HTMLElement>(null);
  useMarkCurrent(ref);
  if (!s.me) return null;
  return (
    <nav className="nav" ref={ref} aria-label={t('ui.nav.menu')}>
      <Link to="/">{t('ui.nav.home')}</Link>
      <Link to="/torrents">{t('ui.nav.torrents')}</Link>
      <Link to="/upload">{t('ui.nav.upload')}</Link>
      <Slot name="nav.main" ctx={{ user: s.me }} />
      <div className="nav-sec">{t('ui.nav.mine')}</div>
      <Slot name="nav.user_menu" ctx={{ user: s.me }} />
      <Link to="/settings">{t('ui.nav.settings')}</Link>
      <Link to="/help">{t('ui.help.title')}</Link>
      <button
        type="button"
        className="nav-btn"
        onClick={async () => {
          await s.logout();
          nav('/login');
        }}
      >
        {t('ui.nav.logout')}
      </button>
      {s.isStaff && (
        <>
          <div className="nav-sec">{t('ui.nav.admin')}</div>
          <Link to="/admin">{t('ui.nav.admin_enter')}</Link>
        </>
      )}
    </nav>
  );
}

interface PluginInfo {
  id: string;
  name: string;
  name_term?: string | null;
  summary?: string;
  category?: string;
  admin_routes?: string[];
  settings?: string;
}

/** 后台分组顺序（插件清单的 `category`） */
export const CATEGORIES = ['content', 'metadata', 'community', 'users', 'play', 'ai', 'system'] as const;

export function pluginLabel(p: PluginInfo, t: (k: string) => string): string {
  if (p.name_term) {
    const v = t(`term:${p.name_term}`);
    if (v !== p.name_term) return v;
  }
  return p.name;
}

export function usePluginList() {
  const s = useSession();
  return useQuery({ queryKey: ['kernel-plugins'], queryFn: () => api<{ items: PluginInfo[] }>('/api/kernel/v1/plugins'), enabled: s.isStaff, staleTime: 300_000 });
}

/** 插件的后台入口：插件自己的后台页优先，其次设置页；都没有时为 null */
export function pluginAdminHref(p: PluginInfo): string | null {
  const r = p.admin_routes?.[0];
  if (r) return r.replace(/\*$/, '');
  return p.settings && p.settings !== 'none' ? `/admin/p/${p.id}/settings` : null;
}

/** 本地存储的小偏好（隐私模式下读写失败时忽略） */
export function usePref<T>(key: string, init: T): [T, (v: T) => void] {
  const [v, setV] = useState<T>(() => {
    try {
      const raw = localStorage.getItem(key);
      return raw === null ? init : (JSON.parse(raw) as T);
    } catch {
      return init;
    }
  });
  const set = (x: T) => {
    setV(x);
    try {
      localStorage.setItem(key, JSON.stringify(x));
    } catch {
      /* 忽略 */
    }
  };
  return [v, set];
}

function AdminNav() {
  const t = useT();
  const s = useSession();
  const ref = useRef<HTMLElement>(null);
  useMarkCurrent(ref);
  const loc = useLocation();
  const [closed, setClosed] = usePref<string[]>('pt-admin-nav-closed', []);
  const list = usePluginList().data?.items ?? [];
  const groups = new Map<string, PluginInfo[]>(CATEGORIES.map((c) => [c, []]));
  for (const p of list) {
    if (pluginAdminHref(p)) (groups.get(p.category ?? 'system') ?? groups.get('system'))?.push(p);
  }
  return (
    <nav className="nav" ref={ref} aria-label={t('ui.nav.admin')}>
      <div className="nav-sec">{t('ui.admin.g_overview')}</div>
      <Link to="/admin">{t('ui.admin.dashboard')}</Link>
      <Link to="/admin/plugins">{t('ui.admin.plugins')}</Link>
      {s.can('kernel.settings.manage') && <Link to="/admin/appearance">{t('ui.appearance.title')}</Link>}
      {s.can('kernel.audit.read') && <Link to="/admin/audit">{t('ui.audit.title')}</Link>}
      {s.can('identity.users.read') && <Link to="/admin/users">{t('ui.admin.users')}</Link>}
      <Slot name="nav.admin" ctx={{ user: s.me }} />
      {[...groups].map(([g, ps]) => {
        if (ps.length === 0) return null;
        // 当前页面所在的分组总是展开
        const here = ps.some((p) => loc.pathname.startsWith(`/admin/p/${p.id}`));
        const open = here || !closed.includes(g);
        return (
          <div key={g} className="nav-grp">
            <button
              type="button"
              className="nav-sec nav-fold"
              aria-expanded={open}
              onClick={() => setClosed(open ? [...closed, g] : closed.filter((x) => x !== g))}
            >
              {t(`ui.admin.g_${g}`)}
              <span className="nav-n">{ps.length}</span>
            </button>
            {open &&
              ps
                .sort((a, b) => pluginLabel(a, t).localeCompare(pluginLabel(b, t)))
                .map((p) => (
                  <Link key={p.id} to={pluginAdminHref(p) as string} title={p.summary || undefined}>
                    {pluginLabel(p, t)}
                  </Link>
                ))}
          </div>
        );
      })}
    </nav>
  );
}

/** 侧栏滚动位置：前台与后台各记一份（换页、前后台切换后恢复） */
function useRailScroll(ref: React.RefObject<HTMLElement | null>, mode: string) {
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const key = `pt-rail-scroll-${mode}`;
    try {
      const y = Number(sessionStorage.getItem(key) ?? 0);
      // 插件导航异步加载，内容撑开后再恢复
      requestAnimationFrame(() => (el.scrollTop = y));
      setTimeout(() => (el.scrollTop = y), 300);
    } catch {
      /* 忽略 */
    }
    const save = () => {
      try {
        sessionStorage.setItem(key, String(el.scrollTop));
      } catch {
        /* 忽略 */
      }
    };
    el.addEventListener('scroll', save, { passive: true });
    return () => el.removeEventListener('scroll', save);
  }, [ref, mode]);
}

function Rail({ admin, open, onClose }: { admin: boolean; open: boolean; onClose(): void }) {
  const t = useT();
  const loc = useLocation();
  // 窄屏抽屉：换页后收起
  useEffect(() => onClose(), [loc.pathname, onClose]);
  const ref = useRef<HTMLElement>(null);
  useRailScroll(ref, admin ? 'admin' : 'front');
  return (
    <aside ref={ref} className={cx('rail', admin && 'admin', open && 'open')} id="rail" aria-label={t('ui.nav.menu')}>
      <Wordmark />
      {admin ? (
        <div className="admin-mark">
          <span>{t('ui.nav.admin')}</span>
          <Link to="/">{t('ui.nav.back_front')}</Link>
        </div>
      ) : (
        <MyCard />
      )}
      {admin ? <AdminNav /> : <UserNav />}
      <div className="rail-foot">
        <a href="https://github.com/imythu/pt-studio" target="_blank" rel="noopener noreferrer">
          Powered by PT Studio
        </a>
      </div>
    </aside>
  );
}

// ---------------- 搜索 ----------------

function useSlashFocus(ref: React.RefObject<HTMLInputElement | null>, onNarrow: () => void) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.isContentEditable)) return;
      if (e.key === '/' || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k')) {
        e.preventDefault();
        if (ref.current && ref.current.offsetParent !== null) {
          ref.current.focus();
          ref.current.select();
        } else onNarrow();
      }
    };
    window.addEventListener('keydown', k);
    return () => window.removeEventListener('keydown', k);
  }, [ref, onNarrow]);
}

function SearchForm({ autoFocus, onDone, inputRef }: { autoFocus?: boolean; onDone?(): void; inputRef?: React.RefObject<HTMLInputElement | null> }) {
  const t = useT();
  const nav = useNavigate();
  const [q, setQ] = useState('');
  const submit = (e: FormEvent) => {
    e.preventDefault();
    nav(q.trim() ? `/torrents?q=${encodeURIComponent(q.trim())}` : '/torrents');
    onDone?.();
  };
  return (
    <form className="search" role="search" onSubmit={submit}>
      <SuggestInput ref={inputRef} type="search" name="nav_q" value={q} onValue={setQ} placeholder={t('ui.torrent.search_hint')} aria-label={t('ui.torrent.search')} autoFocus={autoFocus} onPick={onDone} />
      <kbd aria-hidden="true">/</kbd>
    </form>
  );
}

/** 窄屏搜索：顶部浮层 */
function SearchOverlay({ onClose }: { onClose(): void }) {
  const t = useT();
  useEffect(() => {
    const k = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    document.addEventListener('keydown', k);
    return () => document.removeEventListener('keydown', k);
  }, [onClose]);
  return (
    <Portal>
      <div className="search-ov" role="dialog" aria-modal="true" aria-label={t('ui.torrent.search')} onClick={(e) => e.target === e.currentTarget && onClose()}>
        <div className="search-ov-pn">
          <SearchForm autoFocus onDone={onClose} />
          <Button size="sm" variant="ghost" onClick={onClose}>
            {t('ui.common.cancel')}
          </Button>
        </div>
      </div>
    </Portal>
  );
}

// ---------------- 底部标签栏 ----------------

function TabBar() {
  const t = useT();
  const s = useSession();
  const u = useUnread();
  const loc = useLocation();
  if (!s.me) return null;
  const n = u.notify + u.messages;
  const msgTo = u.messagesOn ? '/p/messages/' : '/p/notify/';
  const inMsg = loc.pathname.startsWith('/p/messages') || loc.pathname.startsWith('/p/notify');
  const inMe = loc.pathname === '/me' || loc.pathname.startsWith(`/users/${s.me.id}`) || loc.pathname.startsWith('/settings');
  return (
    <nav className="tabbar" aria-label={t('ui.nav.tabbar')}>
      <NavLink to="/" end>
        <b>{t('ui.nav.home')}</b>
      </NavLink>
      <NavLink to="/torrents" className={({ isActive }) => (isActive || loc.pathname.startsWith('/groups') ? 'active' : '')}>
        <b>{t('ui.nav.torrents')}</b>
      </NavLink>
      <NavLink to="/upload" className="tab-up">
        <b>{t('ui.nav.upload_short')}</b>
      </NavLink>
      {(u.messagesOn || u.notifyOn) && (
        <Link to={msgTo} aria-current={inMsg ? 'page' : undefined}>
          <b>{t('ui.nav.inbox')}</b>
          {n > 0 && <em>{n > 99 ? '99+' : n}</em>}
        </Link>
      )}
      <Link to="/me" aria-current={inMe ? 'page' : undefined}>
        <b>{t('ui.nav.me')}</b>
      </Link>
    </nav>
  );
}

// ---------------- 提示条 ----------------

function VerifyBanner() {
  const t = useT();
  const [state, setState] = useState<'idle' | 'sent' | 'error'>('idle');
  const resend = () =>
    api('/api/p/identity/v1/email-verifications', { json: {} })
      .then(() => setState('sent'))
      .catch(() => setState('error'));
  return (
    <Note
      tone="warn"
      actions={
        state === 'idle' ? (
          <Button size="xs" onClick={resend}>
            {t('ui.verify.resend')}
          </Button>
        ) : undefined
      }
    >
      <span>{state === 'idle' ? t('ui.verify.banner') : t(state === 'sent' ? 'ui.verify.resent' : 'ui.verify.resend_failed')}</span>
    </Note>
  );
}

// ---------------- 外壳 ----------------

/** 未登录的整页：只有字标与登录 / 注册入口 */
export function GuestBar({ actions = true }: { actions?: boolean }) {
  const t = useT();
  return (
    <div className="guest-bar">
      <Wordmark />
      {actions && (
        <div className="acts">
          <Link className="btn btn-sm btn-ghost" to="/login">
            {t('ui.nav.login')}
          </Link>
          <Link className="btn btn-sm btn-primary" to="/register">
            {t('ui.nav.register')}
          </Link>
        </div>
      )}
      <ThemeButton />
    </div>
  );
}

export function Layout() {
  const t = useT();
  const s = useSession();
  const loc = useLocation();
  const [railOpen, setRailOpen] = useState(false);
  const [searchOpen, setSearchOpen] = useState(false);
  const closeRail = useRef(() => setRailOpen(false)).current;
  const searchRef = useRef<HTMLInputElement>(null);
  useSlashFocus(searchRef, () => setSearchOpen(true));
  useEffect(() => {
    window.scrollTo({ top: 0 });
  }, [loc.pathname]);
  const admin = loc.pathname.startsWith('/admin');
  if (s.loading) return <div className="boot" aria-busy="true" />;
  if (!s.me) {
    return (
      <div className="bare">
        <SkipLink targetId="main-content">{t('ui.common.skip_to_content')}</SkipLink>
        <main id="main-content" className="bare-main">
          <Outlet />
        </main>
      </div>
    );
  }
  return (
    <div className="shell">
      <SkipLink targetId="main-content">{t('ui.common.skip_to_content')}</SkipLink>
      {railOpen && <div className="scrim" onClick={closeRail} />}
      <Rail admin={admin} open={railOpen} onClose={closeRail} />
      <div className="main">
        <header className="top">
          <button type="button" className="menu-btn" aria-controls="rail" aria-expanded={railOpen} onClick={() => setRailOpen(true)}>
            {t('ui.nav.menu')}
          </button>
          <Wordmark small />
          <SearchForm inputRef={searchRef} />
          <div className="top-r">
            <button type="button" className="txt-btn m-search" onClick={() => setSearchOpen(true)}>
              {t('ui.torrent.search_short')}
            </button>
            <ThemeButton />
            <Link className="btn btn-primary top-up" to="/upload">
              {t('ui.nav.upload')}
            </Link>
          </div>
          <StatStrip />
        </header>
        <main id="main-content" className="view">
          {(!s.emailVerified || s.twoFactorNeeded) && (
            <div className="banners">
              {!s.emailVerified && <VerifyBanner />}
              {s.twoFactorNeeded && (
                <Note tone="warn" actions={<Link className="btn btn-xs" to="/settings#security">{t('ui.nav.settings')}</Link>}>
                  <span>{t('ui.two_factor.required_banner')}</span>
                </Note>
              )}
            </div>
          )}
          <Outlet />
        </main>
      </div>
      <TabBar />
      {searchOpen && <SearchOverlay onClose={() => setSearchOpen(false)} />}
      <BackToTop title={t('ui.common.back_to_top')} />
    </div>
  );
}

// ---------------- 手机端「我的」 ----------------

export function MePage() {
  const t = useT();
  const s = useSession();
  const nav = useNavigate();
  const st = useMyStats();
  const bonus = useMyBonus();
  const lv = useMyLevel();
  const ref = useRef<HTMLDivElement>(null);
  if (!s.me) return null;
  const cls = levelName(t, lv?.class);
  return (
    <div className="me-page stack" data-primary>
      <Link to={`/users/${s.me.id}`} className="panel me-hero">
        <Avatar src={(s.me as { avatar_url?: string | null }).avatar_url} name={s.me.username} size={56} ring />
        <span>
          <b>{s.me.username}</b> {cls && <span className="cls">{cls}</span>}
          <span className="small muted">{t('ui.me.view_profile')}</span>
        </span>
        <span className="muted">›</span>
      </Link>
      {st && (
        <div className="kpis" style={{ ['--n' as string]: 4 }}>
          <div>
            <span>{t('ui.profile.uploaded')}</span>
            <b className="up">{formatBytes(st.uploaded)}</b>
          </div>
          <div>
            <span>{t('ui.profile.downloaded')}</span>
            <b className="down">{formatBytes(st.downloaded)}</b>
          </div>
          <div>
            <span>{t('ui.profile.ratio')}</span>
            <b>{fmtRatio(st.ratio)}</b>
          </div>
          <div>
            <span>{t('term:bonus.currency')}</span>
            <b className="gold">{bonus ? shortNum(fromUnits(bonus.available, bonus.scale)) : '-'}</b>
          </div>
        </div>
      )}
      <section className="panel">
        <div className="side-list plain me-links" ref={ref}>
          <Slot name="nav.main" ctx={{ user: s.me }} />
          <Slot name="nav.user_menu" ctx={{ user: s.me }} />
          <Link to="/settings">{t('ui.nav.settings')}</Link>
          <Link to="/help">{t('ui.help.title')}</Link>
          {s.isStaff && <Link to="/admin">{t('ui.nav.admin_enter')}</Link>}
        </div>
      </section>
      <Button
        variant="danger"
        onClick={async () => {
          await s.logout();
          nav('/login');
        }}
      >
        {t('ui.nav.logout')}
      </Button>
    </div>
  );
}

export function RequireLogin({ children }: { children: ReactNode }) {
  const s = useSession();
  const t = useT();
  const loc = useLocation();
  if (s.loading) return null;
  if (!s.me) {
    return (
      <div className="bare-page">
        <GuestBar />
        <section className="panel pad-l login-required" data-primary>
          <h1>{t('ui.common.login_required')}</h1>
          <p className="muted">{t('ui.common.login_required_hint')}</p>
          <div className="row">
            <Link className="btn btn-primary" to="/login" state={{ from: loc.pathname + loc.search }}>
              {t('ui.nav.login')}
            </Link>
            <Link className="btn" to="/register">
              {t('ui.nav.register')}
            </Link>
          </div>
        </section>
      </div>
    );
  }
  return <>{children}</>;
}

export { ThemeButton };
export type { PluginInfo };
export const useInstalledFlag = useInstalled;
