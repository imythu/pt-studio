import type { ComponentType } from 'react';
import { createBrowserRouter, Link } from 'react-router';
import { useT } from '@pt-studio/terms';
import { AdminHome, AdminPluginRoute } from './pages/Admin';
import { Login, Register } from './pages/Auth';
import { PluginRoute } from './pages/PluginRoute';
import { ProfilePage } from './pages/Profile';
import { GroupPage, Home, TorrentList, TorrentPage, Upload } from './pages/Torrents';
import { overrides, type PageId } from './overrides';
import { Layout, RequireLogin } from './shell/Layout';
import { useSession } from './session';

function page(id: PageId, C: ComponentType) {
  const O = overrides[id] ?? C;
  return <O />;
}

function NotFound() {
  const t = useT();
  return (
    <p>
      {t('ui.common.not_found')} · <Link to="/">{t('ui.nav.home')}</Link>
    </p>
  );
}

function RequireStaff({ children }: { children: React.ReactNode }) {
  const s = useSession();
  if (!s.isStaff) return <NotFound />;
  return <>{children}</>;
}

export const router = createBrowserRouter([
  {
    path: '/',
    element: <Layout />,
    children: [
      { index: true, element: page('home', Home) },
      { path: 'login', element: page('login', Login) },
      { path: 'register', element: page('register', Register) },
      { path: 'torrents', element: <RequireLogin>{page('torrents', TorrentList)}</RequireLogin> },
      { path: 'torrents/:id', element: <RequireLogin>{page('torrent', TorrentPage)}</RequireLogin> },
      { path: 'groups/:id', element: <RequireLogin>{page('group', GroupPage)}</RequireLogin> },
      { path: 'upload', element: <RequireLogin>{page('upload', Upload)}</RequireLogin> },
      { path: 'users/:id', element: <RequireLogin>{page('profile', ProfilePage)}</RequireLogin> },
      { path: 'p/:plugin/*', element: <PluginRoute /> },
      {
        path: 'admin',
        element: (
          <RequireLogin>
            <RequireStaff>{page('admin', AdminHome)}</RequireStaff>
          </RequireLogin>
        ),
      },
      {
        path: 'admin/p/:plugin/*',
        element: (
          <RequireLogin>
            <RequireStaff>
              <AdminPluginRoute />
            </RequireStaff>
          </RequireLogin>
        ),
      },
      { path: '*', element: <NotFound /> },
    ],
  },
]);
