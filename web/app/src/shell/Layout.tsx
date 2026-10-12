// 外壳：顶栏（导航、插件导航插槽、用户菜单、主题切换）与主内容区。
import { Link, NavLink, Outlet, useNavigate } from 'react-router';
import { Slot } from '@pt-studio/slots';
import { useT } from '@pt-studio/terms';
import { Button } from '@pt-studio/ui-kit';
import { useSession } from '../session';
import { useManifest } from '../manifest';

function toggleTheme() {
  const root = document.documentElement;
  const next = root.dataset.theme === 'dark' ? 'light' : 'dark';
  root.dataset.theme = next;
  try {
    localStorage.setItem('pt-theme', next);
  } catch {
    /* 隐私模式 */
  }
}

export function Layout() {
  const t = useT();
  const s = useSession();
  const m = useManifest();
  const nav = useNavigate();
  return (
    <div className="app">
      <header className="topbar">
        <Link to="/" className="brand">
          {m.site.name}
        </Link>
        <nav className="mainnav">
          <NavLink to="/torrents">{t('ui.nav.torrents')}</NavLink>
          {s.me && <NavLink to="/upload">{t('ui.nav.upload')}</NavLink>}
          <Slot name="nav.main" ctx={{ user: s.me }} />
          {s.isStaff && <NavLink to="/admin">{t('ui.nav.admin')}</NavLink>}
        </nav>
        <div className="usernav">
          {s.me ? (
            <>
              <Slot name="nav.user_menu" ctx={{ user: s.me }} />
              <Link to={`/users/${s.me.id}`} className="username">
                {s.me.username}
              </Link>
              <Button
                variant="ghost"
                onClick={async () => {
                  await s.logout();
                  nav('/login');
                }}
              >
                {t('ui.nav.logout')}
              </Button>
            </>
          ) : (
            <>
              <NavLink to="/login">{t('ui.nav.login')}</NavLink>
              <NavLink to="/register">{t('ui.nav.register')}</NavLink>
            </>
          )}
          <Button variant="ghost" onClick={toggleTheme} aria-label={t('ui.theme.toggle')} title={t('ui.theme.toggle')}>
            ◐
          </Button>
        </div>
      </header>
      <main className="content">
        <Outlet />
      </main>
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
