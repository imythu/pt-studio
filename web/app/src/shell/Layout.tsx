// 外壳：顶栏（导航、插件导航插槽、用户菜单、主题切换）与主内容区。
import { useCallback, useEffect, useRef, useState } from 'react';
import { Link, NavLink, Outlet, useLocation, useNavigate } from 'react-router';
import { Slot } from '@pt-studio/slots';
import { useT } from '@pt-studio/terms';
import { useQuery } from '@tanstack/react-query';
import { api } from '@pt-studio/sdk';
import { Icon, Logo, type IconName } from '../art';
import { useSession } from '../session';
import { useInstalled, useManifest } from '../manifest';

function currentTheme(): 'dark' | 'light' {
  const set = document.documentElement.dataset.theme;
  if (set === 'dark' || set === 'light') return set;
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

function toggleTheme(): 'dark' | 'light' {
  const next = currentTheme() === 'dark' ? 'light' : 'dark';
  document.documentElement.dataset.theme = next;
  try {
    localStorage.setItem('pt-theme', next);
  } catch {
    /* 隐私模式 */
  }
  return next;
}

/** 点击外部或按 Esc 关闭 */
function useDismiss(open: boolean, close: () => void) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const down = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) close();
    };
    const key = (e: KeyboardEvent) => e.key === 'Escape' && close();
    document.addEventListener('mousedown', down);
    document.addEventListener('keydown', key);
    return () => {
      document.removeEventListener('mousedown', down);
      document.removeEventListener('keydown', key);
    };
  }, [open, close]);
  return ref;
}

function NavSearch() {
  const t = useT();
  const nav = useNavigate();
  const [q, setQ] = useState('');
  return (
    <form
      className="nav-search"
      role="search"
      onSubmit={(e) => {
        e.preventDefault();
        nav(q.trim() ? `/torrents?q=${encodeURIComponent(q.trim())}` : '/torrents');
      }}
    >
      <Icon name="search" size={16} />
      <input type="search" name="nav_q" value={q} onChange={(e) => setQ(e.target.value)} placeholder={t('ui.torrent.search')} aria-label={t('ui.torrent.search')} />
    </form>
  );
}

/** 顶栏常驻入口：通知、消息（带未读数；插件未安装时不显示） */
function QuickLinks() {
  const t = useT();
  const notify = useInstalled('notify');
  const messages = useInstalled('messages');
  const nn = useQuery({
    queryKey: ['unread', 'notify'],
    queryFn: () => api<{ count: number }>('/api/p/notify/v1/unread'),
    enabled: notify,
    refetchInterval: 60_000,
    retry: false,
  });
  const nm = useQuery({
    queryKey: ['unread', 'messages'],
    queryFn: () => api<{ private: number }>('/api/p/messages/v1/unread'),
    enabled: messages,
    refetchInterval: 60_000,
    retry: false,
  });
  const item = (to: string, icon: IconName, label: string, n: number | undefined) => (
    <NavLink to={to} className="icon-btn" aria-label={label} title={label}>
      <Icon name={icon} />
      {!!n && <span className="dot">{n > 99 ? '99+' : n}</span>}
    </NavLink>
  );
  return (
    <>
      {notify && item('/p/notify/', 'bell', t('ui.nav.notifications'), nn.data?.count)}
      {messages && item('/p/messages/', 'mail', t('ui.nav.messages'), nm.data?.private)}
    </>
  );
}

function UserMenu() {
  const t = useT();
  const s = useSession();
  const nav = useNavigate();
  const [open, setOpen] = useState(false);
  const close = useCallback(() => setOpen(false), []);
  const ref = useDismiss(open, close);
  if (!s.me) return null;
  return (
    <div className="user-menu" ref={ref}>
      <button type="button" className="user-chip" aria-haspopup="menu" aria-expanded={open} aria-label={t('ui.nav.menu')} onClick={() => setOpen(!open)}>
        <span className="avatar" aria-hidden="true">
          {s.me.username.slice(0, 1).toUpperCase()}
        </span>
        <span className="user-chip-name">{s.me.username}</span>
        <Icon name="chevron" size={14} />
      </button>
      {open && (
        <div className="menu" role="menu">
          <MeLinks onPick={close} />
          <hr />
          <button
            type="button"
            role="menuitem"
            className="danger"
            onClick={async () => {
              close();
              await s.logout();
              nav('/login');
            }}
          >
            <Icon name="logout" size={16} /> {t('ui.nav.logout')}
          </button>
        </div>
      )}
    </div>
  );
}

/** “我的”入口：个人主页、设置、插件提供的菜单项（菜单与移动端“我的”页共用） */
export function MeLinks({ onPick }: { onPick?: () => void }) {
  const t = useT();
  const s = useSession();
  if (!s.me) return null;
  return (
    <>
      <Link role="menuitem" to={`/users/${s.me.id}`} onClick={onPick}>
        <Icon name="user" size={16} /> {t('ui.nav.profile')}
      </Link>
      <Link role="menuitem" to="/settings" onClick={onPick}>
        <Icon name="settings" size={16} /> {t('ui.nav.settings')}
      </Link>
      <div className="menu-slot" onClick={onPick}>
        <Slot name="nav.user_menu" ctx={{ user: s.me }} />
      </div>
    </>
  );
}

/** 移动端底部标签栏 */
function TabBar() {
  const t = useT();
  const s = useSession();
  const notify = useInstalled('notify');
  const unread = useQuery({
    queryKey: ['unread', 'notify'],
    queryFn: () => api<{ count: number }>('/api/p/notify/v1/unread'),
    enabled: notify && !!s.me,
    refetchInterval: 60_000,
    retry: false,
  }).data?.count;
  if (!s.me) return null;
  return (
    <nav className="tabbar" aria-label={t('ui.nav.menu')}>
      <NavLink to="/" end>
        <Icon name="home" size={22} />
        <span>{t('ui.nav.home')}</span>
      </NavLink>
      <NavLink to="/torrents">
        <Icon name="compass" size={22} />
        <span>{t('ui.nav.torrents')}</span>
      </NavLink>
      <NavLink to="/upload" className="tab-main">
        <span className="tab-main-ico">
          <Icon name="plus" size={24} />
        </span>
        <span>{t('ui.nav.upload_short')}</span>
      </NavLink>
      {notify ? (
        <NavLink to="/p/notify/">
          <span className="tab-ico">
            <Icon name="bell" size={22} />
            {!!unread && <span className="dot">{unread > 99 ? '99+' : unread}</span>}
          </span>
          <span>{t('ui.nav.notifications')}</span>
        </NavLink>
      ) : null}
      <NavLink to="/me">
        <Icon name="user" size={22} />
        <span>{t('ui.nav.me')}</span>
      </NavLink>
    </nav>
  );
}

/** 邮箱未验证提示：重发验证邮件 */
function VerifyBanner() {
  const t = useT();
  const [state, setState] = useState<'idle' | 'sent' | 'error'>('idle');
  const resend = () =>
    api('/api/p/identity/v1/email-verifications', { json: {} })
      .then(() => setState('sent'))
      .catch(() => setState('error'));
  return (
    <p className="notice notice-warn" role="status">
      {t('ui.verify.banner')}{' '}
      {state === 'idle' ? (
        <button type="button" className="linklike" onClick={resend}>
          {t('ui.verify.resend')}
        </button>
      ) : (
        t(state === 'sent' ? 'ui.verify.resent' : 'ui.verify.resend_failed')
      )}
    </p>
  );
}

export function Layout() {
  const t = useT();
  const s = useSession();
  const m = useManifest();
  const loc = useLocation();
  const [theme, setTheme] = useState(currentTheme);
  const bare = !s.me && (loc.pathname === '/login' || loc.pathname === '/register' || loc.pathname === '/');
  const themeBtn = (
    <button type="button" className="icon-btn" onClick={() => setTheme(toggleTheme())} aria-label={t('ui.theme.toggle')} title={t('ui.theme.toggle')}>
      <Icon name={theme === 'dark' ? 'sun' : 'moon'} />
    </button>
  );
  return (
    <div className={`app${s.me ? ' has-tabbar' : ''}`}>
      <header className="topbar">
        <div className="topbar-in">
          <Link to="/" className="brand">
            <Logo size={32} />
            <span>{m.site.name}</span>
          </Link>
          {s.me && (
            <nav className="mainnav">
              <NavLink to="/" end>
                {t('ui.nav.home')}
              </NavLink>
              <NavLink to="/torrents">{t('ui.nav.torrents')}</NavLink>
              <Slot name="nav.main" ctx={{ user: s.me }} />
              {s.isStaff && <NavLink to="/admin">{t('ui.nav.admin')}</NavLink>}
            </nav>
          )}
          {s.me && <NavSearch />}
          <div className="usernav">
            {s.me ? (
              <>
                <NavLink to="/upload" className="nav-upload">
                  <Icon name="upload" size={16} /> {t('ui.nav.upload')}
                </NavLink>
                <QuickLinks />
                <UserMenu />
              </>
            ) : (
              !bare && (
                <>
                  <NavLink to="/login" className="nav-login">
                    {t('ui.nav.login')}
                  </NavLink>
                  <NavLink to="/register" className="nav-cta">
                    {t('ui.nav.register')}
                  </NavLink>
                </>
              )
            )}
            {themeBtn}
          </div>
          <div className="mobile-actions">
            {s.me ? (
              <Link to="/torrents" className="icon-btn" aria-label={t('ui.torrent.search')}>
                <Icon name="search" />
              </Link>
            ) : (
              !bare && (
                <Link to="/login" className="nav-cta">
                  {t('ui.nav.login')}
                </Link>
              )
            )}
            {themeBtn}
          </div>
        </div>
      </header>
      <main className={`content${bare ? ' content-bare' : ''}`}>
        {s.me && !s.emailVerified && <VerifyBanner />}
        <Outlet />
      </main>
      <footer className="footer">
        <span>{m.site.name} · Powered by PT Studio</span>
      </footer>
      <TabBar />
    </div>
  );
}

/** 移动端“我的”页 */
export function MePage() {
  const t = useT();
  const s = useSession();
  const nav = useNavigate();
  if (!s.me) return null;
  return (
    <div className="me-page">
      <Link to={`/users/${s.me.id}`} className="me-card">
        <span className="avatar avatar-lg" aria-hidden="true">
          {s.me.username.slice(0, 1).toUpperCase()}
        </span>
        <span>
          <b>{s.me.username}</b>
          <span className="pt-muted small">{t('ui.me.view_profile')}</span>
        </span>
        <Icon name="arrow" size={18} />
      </Link>
      <div className="me-list">
        <MeLinks />
        {s.isStaff && (
          <Link to="/admin">
            <Icon name="shield" size={16} /> {t('ui.nav.admin')}
          </Link>
        )}
        <Link to="/p/messages/">
          <Icon name="mail" size={16} /> {t('ui.nav.messages')}
        </Link>
      </div>
      <button
        type="button"
        className="pt-btn pt-btn-danger me-logout"
        onClick={async () => {
          await s.logout();
          nav('/login');
        }}
      >
        <Icon name="logout" size={16} /> {t('ui.nav.logout')}
      </button>
    </div>
  );
}

export function RequireLogin({ children }: { children: React.ReactNode }) {
  const s = useSession();
  const t = useT();
  if (s.loading) return <p className="pt-muted">{t('ui.common.loading')}</p>;
  if (!s.me) {
    return (
      <p>
        {t('ui.common.login_required')} · <Link to="/login">{t('ui.nav.login')}</Link>
      </p>
    );
  }
  return <>{children}</>;
}
