// 外壳：顶栏（导航、插件导航插槽、用户菜单、主题切换）与主内容区。
import { useCallback, useEffect, useRef, useState } from 'react';
import { Link, NavLink, Outlet, useLocation, useNavigate } from 'react-router';
import { Slot } from '@pt-studio/slots';
import { useT } from '@pt-studio/terms';
import { Icon, Logo } from '../art';
import { useSession } from '../session';
import { useManifest } from '../manifest';

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
      <button type="button" className="icon-btn" aria-haspopup="menu" aria-expanded={open} aria-label={t('ui.nav.menu')} onClick={() => setOpen(!open)}>
        <Icon name="chevron" size={16} />
      </button>
      {open && (
        <div className="menu" role="menu">
          <Link role="menuitem" to={`/users/${s.me.id}`} onClick={close}>
            <Icon name="user" size={16} /> {t('ui.nav.profile')}
          </Link>
          <Link role="menuitem" to="/settings" onClick={close}>
            <Icon name="settings" size={16} /> {t('ui.nav.settings')}
          </Link>
          <div className="menu-slot" onClick={close}>
            <Slot name="nav.user_menu" ctx={{ user: s.me }} />
          </div>
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

export function Layout() {
  const t = useT();
  const s = useSession();
  const m = useManifest();
  const loc = useLocation();
  const [menu, setMenu] = useState(false);
  const [theme, setTheme] = useState(currentTheme);
  useEffect(() => setMenu(false), [loc.pathname]);
  const bare = !s.me && (loc.pathname === '/login' || loc.pathname === '/register' || loc.pathname === '/');
  return (
    <div className="app">
      <header className="topbar">
        <div className="topbar-in">
          <Link to="/" className="brand">
            <Logo size={30} />
            <span>{m.site.name}</span>
          </Link>
          <button type="button" className="icon-btn menu-toggle" aria-label={t('ui.nav.menu')} aria-expanded={menu} onClick={() => setMenu(!menu)}>
            <Icon name={menu ? 'close' : 'menu'} />
          </button>
          <div className={`topbar-body${menu ? ' open' : ''}`}>
            {s.me && (
              <nav className="mainnav">
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
                  <Link to={`/users/${s.me.id}`} className="username">
                    <span className="avatar" aria-hidden="true">
                      {s.me.username.slice(0, 1).toUpperCase()}
                    </span>
                    {s.me.username}
                  </Link>
                  <UserMenu />
                </>
              ) : (
                !bare && (
                  <>
                    <NavLink to="/login">{t('ui.nav.login')}</NavLink>
                    <NavLink to="/register" className="nav-cta">
                      {t('ui.nav.register')}
                    </NavLink>
                  </>
                )
              )}
              <button
                type="button"
                className="icon-btn"
                onClick={() => setTheme(toggleTheme())}
                aria-label={t('ui.theme.toggle')}
                title={t('ui.theme.toggle')}
              >
                <Icon name={theme === 'dark' ? 'sun' : 'moon'} />
              </button>
            </div>
          </div>
        </div>
      </header>
      <main className={`content${bare ? ' content-bare' : ''}`}>
        <Outlet />
      </main>
      <footer className="footer">
        <span>
          {m.site.name} · Powered by PT Studio
        </span>
      </footer>
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
