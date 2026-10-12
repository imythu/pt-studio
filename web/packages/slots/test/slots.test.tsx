import { QueryClient } from '@tanstack/react-query';
import { act, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { FrontendManifest } from '@pt-studio/sdk';
import { makeTranslator } from '@pt-studio/terms';
import { __test, makeBatcher, PluginPage, Slot, SlotsProvider, type HostServices, type PluginFrontend, type SlotProps } from '../src/index';

function host(modules: Record<string, PluginFrontend | 'broken'>): HostServices {
  const plugins = Object.entries(modules).map(([id, m]) => ({
    id,
    name: id,
    version: '0.1.0',
    entry: `/static/plugins/${id}/0.1.0/index.js`,
    slots: m === 'broken' ? ['user.profile.stats'] : (Object.keys(m.slots ?? {}) as string[]),
    routes: m === 'broken' ? [] : (m.routes ?? []).map((r) => r.path),
    settings_ui: 'schema',
  }));
  const manifest: FrontendManifest = { site: { name: 't', default_locale: 'zh-CN', locales: ['zh-CN'], themes: [] }, plugins, installed: Object.keys(modules), features: {} };
  return {
    manifest,
    translator: makeTranslator('zh-CN', {}),
    navigate: () => undefined,
    toast: () => undefined,
    user: () => null,
    isStaff: () => true,
    queryClient: new QueryClient(),
    importer: async (url) => {
      const id = url.split('/')[3] as string;
      const m = modules[id];
      if (!m || m === 'broken') throw new Error('404');
      return { default: m };
    },
  };
}

const Stat = (label: string) =>
  function S({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
    return (
      <span>
        {label}:{ctx.profile.id}:{api.pluginId}
      </span>
    );
  };

function Boom(): never {
  throw new Error('kaboom');
}

describe('slots', () => {
  it('renders plugins in order, isolates failures and skips unloadable plugins', async () => {
    const h = host({
      b: { slots: { 'user.profile.stats': { component: Stat('B'), order: 20 } } },
      a: { slots: { 'user.profile.stats': { component: Stat('A'), order: 10 } } },
      crash: { slots: { 'user.profile.stats': { component: Boom, order: 5 } } },
      gone: 'broken',
    });
    await act(async () => {
      render(
        <SlotsProvider host={h}>
          <Slot name="user.profile.stats" ctx={{ profile: { id: 7 } }} />
        </SlotsProvider>,
      );
    });
    const a = await screen.findByText('A:7:a');
    const b = await screen.findByText('B:7:b');
    expect(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(await screen.findByRole('alert')).toHaveProperty('textContent', 'crash: kaboom');
  });

  it('renders plugin routes', async () => {
    const h = host({ invite: { routes: [{ path: '/p/invite/*', component: ({ path }) => <h1>page {path}</h1> }] } });
    await act(async () => {
      render(
        <SlotsProvider host={h}>
          <PluginPage pluginId="invite" path="/p/invite/tree" />
        </SlotsProvider>,
      );
    });
    expect(await screen.findByText('page /p/invite/tree')).toBeTruthy();
    expect(__test.routeMatches('/p/invite/*', '/p/invite')).toBe(true);
    expect(__test.routeMatches('/p/invite/*', '/p/invitex')).toBe(false);
  });

  it('batches keys within a microtask', async () => {
    const calls: number[][] = [];
    const get = makeBatcher<number, string>(async (keys) => {
      calls.push(keys);
      return new Map(keys.map((k) => [k, `v${k}`]));
    });
    const r = await Promise.all([get(1), get(2), get(1)]);
    expect(r).toEqual(['v1', 'v2', 'v1']);
    expect(calls).toEqual([[1, 2]]);
  });
});
