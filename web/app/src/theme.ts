// 主题插件（外观）：启动时挂上当前主题的样式表，并采用它替换的页面。
// 样式表地址缓存在 localStorage，下次启动在请求清单之前就挂上，避免先闪一下经典样式。
import type { ComponentType } from 'react';
import type { ActiveTheme } from '@pt-studio/sdk';
import type { PluginFrontend } from '@pt-studio/slots';
import { overrides, PAGE_IDS, type PageId } from './overrides';

const KEY = 'pt-skin';

function link(href: string): Promise<void> {
  let el = document.querySelector<HTMLLinkElement>('link[data-skin]');
  if (el?.getAttribute('href') === href) return Promise.resolve();
  el?.remove();
  el = document.createElement('link');
  el.rel = 'stylesheet';
  el.href = href;
  el.dataset.skin = '';
  const done = new Promise<void>((resolve) => {
    el.onload = el.onerror = () => resolve();
    setTimeout(resolve, 2000);
  });
  document.head.appendChild(el);
  return done;
}

export function preloadTheme() {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? 'null') as { id: string; css: string } | null;
    if (saved?.css) {
      document.documentElement.dataset.skin = saved.id;
      void link(saved.css);
    }
  } catch {
    /* 隐私模式或旧值 */
  }
}

export async function applyTheme(theme: ActiveTheme | null) {
  const root = document.documentElement;
  try {
    if (theme?.css) localStorage.setItem(KEY, JSON.stringify({ id: theme.id, css: theme.css }));
    else localStorage.removeItem(KEY);
  } catch {
    /* 忽略 */
  }
  if (!theme) {
    delete root.dataset.skin;
    document.querySelector('link[data-skin]')?.remove();
    return;
  }
  root.dataset.skin = theme.id;
  const tasks: Promise<unknown>[] = [];
  if (theme.css) tasks.push(link(theme.css));
  else document.querySelector('link[data-skin]')?.remove();
  if (theme.entry) {
    tasks.push(
      (import(/* @vite-ignore */ theme.entry) as Promise<{ default?: PluginFrontend }>)
        .then((m) => {
          for (const [id, C] of Object.entries(m.default?.pages ?? {})) {
            if (C && PAGE_IDS.includes(id as PageId)) {
              overrides[id as PageId] = C as ComponentType;
              root.dataset.skinPages = '1';
            }
          }
        })
        .catch((e: unknown) => console.error(`theme ${theme.id}: failed to load entry`, e)),
    );
  }
  await Promise.all(tasks);
}
