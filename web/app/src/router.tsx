import type { ComponentType, ReactNode } from 'react';
import { createBrowserRouter, Link } from 'react-router';
import { useT } from '@pt-studio/terms';
import { AdminHome, AdminPluginRoute, AdminPlugins } from './pages/Admin';
import { AdminUser, AdminUsers } from './pages/AdminUsers';
import { AdminAudit } from './pages/AdminAudit';
import { SettingsPage } from './pages/Settings';
import { Login, Register, ResetPassword, VerifyEmail } from './pages/Auth';
import { PluginRoute } from './pages/PluginRoute';
import { ProfilePage } from './pages/Profile';
import { Home } from './pages/Home';
import { TorrentList } from './pages/TorrentList';
import { TorrentPage } from './pages/TorrentDetail';
import { GroupPage } from './pages/Group';
import { Upload } from './pages/Upload';
import { HelpPage } from './help';
import { overrides, type PageId } from './overrides';
import { GuestBar, Layout, MePage, RequireLogin } from './shell/Layout';
import { useSession } from './session';

function page(id: PageId, C: ComponentType) {
  const O = overrides[id] ?? C;
  return <O />;
}

export function NotFound() {
  const t = useT();
  const s = useSession();
  return (
    <div className={s.me ? undefined : 'bare-page'}>
      {!s.me && <GuestBar actions={false} />}
      <section className="lost" data-primary>
        <div className="lost-art" aria-hidden="true" />
        <div className="lost-b">
          <span className="lost-code">404</span>
          <h1>{t('ui.common.not_found')}</h1>
          <p className="muted">{t('ui.common.not_found_hint')}</p>
          <div className="row">
            <Link className="btn btn-primary" to="/">
              {t('ui.nav.home')}
            </Link>
            {s.me && (
              <Link className="btn" to="/torrents">
                {t('ui.nav.torrents')}
              </Link>
            )}
          </div>
        </div>
      </section>
    </div>
  );
}

function RequireStaff({ children }: { children: ReactNode }) {
  const s = useSession();
  if (!s.isStaff) return <NotFound />;
  return <>{children}</>;
}

const member = (el: ReactNode) => <RequireLogin>{el}</RequireLogin>;
const staff = (el: ReactNode) => (
  <RequireLogin>
    <RequireStaff>{el}</RequireStaff>
  </RequireLogin>
);

export const router = createBrowserRouter([
  {
    path: '/',
    element: <Layout />,
    children: [
      { index: true, element: page('home', Home) },
      { path: 'login', element: page('login', Login) },
      { path: 'register', element: page('register', Register) },
      { path: 'reset-password', element: <ResetPassword /> },
      { path: 'verify-email', element: <VerifyEmail /> },
      { path: 'torrents', element: member(page('torrents', TorrentList)) },
      { path: 'torrents/:id', element: member(page('torrent', TorrentPage)) },
      { path: 'groups/:id', element: member(page('group', GroupPage)) },
      { path: 'upload', element: member(page('upload', Upload)) },
      { path: 'users/:id', element: member(page('profile', ProfilePage)) },
      { path: 'help', element: <HelpPage /> },
      { path: 'me', element: member(<MePage />) },
      { path: 'settings', element: member(page('settings', SettingsPage)) },
      { path: 'p/:plugin/*', element: <PluginRoute /> },
      { path: 'admin', element: staff(page('admin', AdminHome)) },
      { path: 'admin/plugins', element: staff(<AdminPlugins />) },
      { path: 'admin/users', element: staff(<AdminUsers />) },
      { path: 'admin/users/:id', element: staff(<AdminUser />) },
      { path: 'admin/audit', element: staff(<AdminAudit />) },
      { path: 'admin/p/:plugin/*', element: staff(<AdminPluginRoute />) },
      { path: '*', element: <NotFound /> },
    ],
  },
]);
