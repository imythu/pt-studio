// 启动：读取前端清单 → 术语 → 会话 → 插槽宿主 → 路由（web HLD §2）。
import './locales';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { StrictMode, useMemo } from 'react';
import { createRoot } from 'react-dom/client';
import { RouterProvider } from 'react-router';
import { api, type FrontendManifest } from '@pt-studio/sdk';
import { SlotsProvider, type HostServices } from '@pt-studio/slots';
import { TermsProvider, useTranslator } from '@pt-studio/terms';
import { ToastHost, useToast } from '@pt-studio/ui-kit';
import { ManifestCtx } from './manifest';
import { applyTheme, preloadTheme } from './theme';
import { ReauthHost } from './reauth';
import { router } from './router';
import { SessionProvider, useSession } from './session';

const queryClient = new QueryClient({ defaultOptions: { queries: { refetchOnWindowFocus: false, retry: 1 } } });

function pickLocale(m: FrontendManifest): string {
  let saved: string | null = null;
  try {
    saved = localStorage.getItem('pt-locale');
  } catch {
    /* 隐私模式 */
  }
  const prefs = [saved, ...navigator.languages].filter((x): x is string => !!x);
  for (const p of prefs) {
    const hit = m.site.locales.find((l) => l.toLowerCase() === p.toLowerCase() || l.split('-')[0] === p.split('-')[0]);
    if (hit) return hit;
  }
  return m.site.default_locale;
}

function Host({ manifest }: { manifest: FrontendManifest }) {
  const translator = useTranslator();
  const toast = useToast();
  const session = useSession();
  const host = useMemo<HostServices>(
    () => ({
      manifest,
      translator,
      navigate: (to) => void router.navigate(to),
      toast,
      user: () => session.me,
      isStaff: () => session.isStaff,
      queryClient,
    }),
    [manifest, translator, toast, session.me, session.isStaff],
  );
  return (
    <SlotsProvider host={host}>
      <RouterProvider router={router} />
      <ReauthHost />
    </SlotsProvider>
  );
}

async function boot() {
  try {
    const t = localStorage.getItem('pt-theme');
    if (t) document.documentElement.dataset.theme = t;
  } catch {
    /* 忽略 */
  }
  preloadTheme();
  const manifest = await api<FrontendManifest>('/api/kernel/v1/frontend/manifest');
  await applyTheme(manifest.theme ?? null);
  document.title = manifest.site.name;
  const locale = pickLocale(manifest);
  document.documentElement.lang = locale;
  const el = document.getElementById('root');
  if (!el) return;
  createRoot(el).render(
    <StrictMode>
      <ManifestCtx.Provider value={manifest}>
        <QueryClientProvider client={queryClient}>
          <TermsProvider locale={locale}>
            <ToastHost>
              <SessionProvider>
                <Host manifest={manifest} />
              </SessionProvider>
            </ToastHost>
          </TermsProvider>
        </QueryClientProvider>
      </ManifestCtx.Provider>
    </StrictMode>,
  );
}

void boot();
