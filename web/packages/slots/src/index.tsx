// @pt-studio/slots：插槽、插件前端加载器与 PluginApi（web LLD §3）。
//
// 插件前端是原生 ESM 模块（入口 URL 来自前端清单），共享依赖经 import map 与宿主共用同一份实例。

import {
  Component,
  createContext,
  Suspense,
  use,
  useContext,
  useMemo,
  useState,
  type ComponentType,
  type ErrorInfo,
  type ReactNode,
} from 'react';
import type { QueryClient } from '@tanstack/react-query';
import { api as sdkApi, apiFetch, type FrontendManifest, type FrontendPlugin, type Me, type Path, type RequestOptions } from '@pt-studio/sdk';
import type { TermValue, Translator } from '@pt-studio/terms';
import { SLOT_KINDS, type SlotName } from './catalog.gen';

export { SLOT_KINDS, type SlotName, type SlotKind } from './catalog.gen';

// ---------------- 契约类型 ----------------

export interface PluginApi {
  pluginId: string;
  fetch(path: Path, init?: RequestOptions): Promise<Response>;
  json<T = unknown>(path: Path, init?: RequestOptions): Promise<T>;
  t(key: string, args?: Record<string, unknown>): string;
  term(key: string): TermValue | undefined;
  navigate(to: string): void;
  toast(msg: string, kind?: 'info' | 'success' | 'error'): void;
  user(): Me | null;
  /** 当前用户是否为管理人员（版主 / 管理员）：用于显示管理入口，权限仍以服务端为准 */
  isStaff(): boolean;
  /** 同一微任务内的请求合并为一次（每批 ≤ 100 个键） */
  batch<K, V>(key: string, loader: (keys: K[]) => Promise<Map<K, V>>): (k: K) => Promise<V | undefined>;
  queryClient: QueryClient;
}

export interface SlotProps<C = Record<string, unknown>> {
  ctx: C;
  api: PluginApi;
}

export interface SlotEntry {
  component: ComponentType<SlotProps<any>>;
  order?: number;
  /** tabs 类型插槽的标签页标题（术语键或界面文案键） */
  title_term?: string;
  /** columns 类型插槽的列标题 */
  header_term?: string;
}

export interface PluginRouteDef {
  /** 如 `/p/invite/*` */
  path: string;
  component: ComponentType<{ api: PluginApi; path: string }>;
}

export interface PluginFrontend {
  slots?: Partial<Record<SlotName, SlotEntry>>;
  routes?: PluginRouteDef[];
  settings?: ComponentType<{ api: PluginApi }>;
  onInit?: (api: PluginApi) => void;
}

// ---------------- 宿主 ----------------

export interface HostServices {
  manifest: FrontendManifest;
  translator: Translator;
  navigate(to: string): void;
  toast(msg: string, kind?: 'info' | 'success' | 'error'): void;
  user(): Me | null;
  /** 员工可见插件错误详情 */
  isStaff(): boolean;
  queryClient: QueryClient;
  /** 测试替换用；默认原生 `import()` */
  importer?: (url: string) => Promise<{ default: PluginFrontend }>;
}

interface Registry {
  host: HostServices;
  load(p: FrontendPlugin): Promise<PluginFrontend | null>;
  apiFor(pluginId: string): PluginApi;
}

const RegistryCtx = createContext<Registry | null>(null);

function useRegistry(): Registry {
  const r = useContext(RegistryCtx);
  if (!r) throw new Error('SlotsProvider is missing');
  return r;
}

/** 批量合并器：收集同一微任务内的键 */
export function makeBatcher<K, V>(loader: (keys: K[]) => Promise<Map<K, V>>, max = 100): (k: K) => Promise<V | undefined> {
  let pending: { key: K; resolve: (v: V | undefined) => void; reject: (e: unknown) => void }[] = [];
  let scheduled = false;
  const flush = async () => {
    scheduled = false;
    const all = pending;
    pending = [];
    for (let i = 0; i < all.length; i += max) {
      const chunk = all.slice(i, i + max);
      const keys = [...new Set(chunk.map((c) => c.key))];
      try {
        const m = await loader(keys);
        chunk.forEach((c) => c.resolve(m.get(c.key)));
      } catch (e) {
        chunk.forEach((c) => c.reject(e));
      }
    }
  };
  return (key) =>
    new Promise((resolve, reject) => {
      pending.push({ key, resolve, reject });
      if (!scheduled) {
        scheduled = true;
        queueMicrotask(() => void flush());
      }
    });
}

export function SlotsProvider({ host, children }: { host: HostServices; children: ReactNode }) {
  const registry = useMemo<Registry>(() => {
    const modules = new Map<string, Promise<PluginFrontend | null>>();
    const apis = new Map<string, PluginApi>();
    const batchers = new Map<string, (k: unknown) => Promise<unknown>>();
    const importer = host.importer ?? ((url: string) => import(/* @vite-ignore */ url) as Promise<{ default: PluginFrontend }>);
    const apiFor = (pluginId: string): PluginApi => {
      let a = apis.get(pluginId);
      if (!a) {
        a = {
          pluginId,
          fetch: (path, init) => apiFetch(path, init),
          json: (path, init) => sdkApi(path, init),
          t: (k, args) => host.translator.t(k, args),
          term: (k) => host.translator.term(k),
          navigate: host.navigate,
          toast: host.toast,
          user: host.user,
          isStaff: () => host.isStaff(),
          batch: <K, V>(key: string, loader: (keys: K[]) => Promise<Map<K, V>>) => {
            const full = `${pluginId}:${key}`;
            let b = batchers.get(full);
            if (!b) {
              b = makeBatcher(loader) as (k: unknown) => Promise<unknown>;
              batchers.set(full, b);
            }
            return b as (k: K) => Promise<V | undefined>;
          },
          queryClient: host.queryClient,
        };
        apis.set(pluginId, a);
      }
      return a;
    };
    const load = (p: FrontendPlugin): Promise<PluginFrontend | null> => {
      if (!p.entry) return Promise.resolve(null);
      let m = modules.get(p.id);
      if (!m) {
        // 加载失败（404、版本不兼容）只跳过该插件
        m = importer(p.entry)
          .then((mod) => {
            const f = mod.default ?? null;
            f?.onInit?.(apiFor(p.id));
            return f;
          })
          .catch((e: unknown) => {
            console.warn(`plugin ${p.id} failed to load`, e);
            return null;
          });
        modules.set(p.id, m);
      }
      return m;
    };
    return { host, load, apiFor };
  }, [host]);
  return <RegistryCtx.Provider value={registry}>{children}</RegistryCtx.Provider>;
}

class PluginBoundary extends Component<{ plugin: string; staff: boolean; children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };
  static getDerivedStateFromError(error: Error) {
    return { error };
  }
  componentDidCatch(error: Error, info: ErrorInfo) {
    console.warn(`plugin ${this.props.plugin} crashed`, error, info.componentStack);
  }
  render() {
    if (!this.state.error) return this.props.children;
    if (!this.props.staff) return null;
    return (
      <div className="pt-plugin-error" role="alert">
        {this.props.plugin}: {this.state.error.message}
      </div>
    );
  }
}

export interface ResolvedEntry {
  plugin: string;
  entry: SlotEntry;
}

/** 给 Promise 标上 status/value，`use` 遇到已完成的 Promise 时同步返回，不再挂起（避免登录后反复挂起触发 React #482） */
function track<T>(p: Promise<T>): Promise<T> {
  const t = p as Promise<T> & { status?: string; value?: T; reason?: unknown };
  if (t.status) return p;
  t.status = 'pending';
  p.then(
    (v) => {
      t.status = 'fulfilled';
      t.value = v;
    },
    (e) => {
      t.status = 'rejected';
      t.reason = e;
    },
  );
  return p;
}

const entryCache = new WeakMap<Registry, Map<string, Promise<ResolvedEntry[]>>>();

function entriesPromise(reg: Registry, name: SlotName): Promise<ResolvedEntry[]> {
  let byName = entryCache.get(reg);
  if (!byName) {
    byName = new Map();
    entryCache.set(reg, byName);
  }
  let p = byName.get(name);
  if (!p) {
    const plugins = reg.host.manifest.plugins.filter((x) => x.slots.includes(name));
    p = Promise.all(
      plugins.map(async (pl) => {
        const f = await reg.load(pl);
        const e = f?.slots?.[name];
        return e ? { plugin: pl.id, entry: e } : null;
      }),
    ).then((xs) =>
      xs
        .filter((x): x is ResolvedEntry => x !== null)
        .sort((a, b) => (a.entry.order ?? 100) - (b.entry.order ?? 100) || a.plugin.localeCompare(b.plugin)),
    );
    byName.set(name, track(p));
  }
  return byName.get(name) as Promise<ResolvedEntry[]>;
}

/** 已加载的插槽条目（Suspense） */
export function useSlotEntries(name: SlotName): ResolvedEntry[] {
  return use(entriesPromise(useRegistry(), name));
}

function Rendered({ e, ctx }: { e: ResolvedEntry; ctx: unknown }) {
  const reg = useRegistry();
  const C = e.entry.component;
  return (
    <PluginBoundary plugin={e.plugin} staff={reg.host.isStaff()}>
      <Suspense fallback={<span className="skeleton" />}>
        <C ctx={ctx as Record<string, unknown>} api={reg.apiFor(e.plugin)} />
      </Suspense>
    </PluginBoundary>
  );
}

/** 渲染一个已解析的插槽条目（错误边界内）：宿主把插件标签页并入自己的页签栏时使用 */
export function SlotEntryView({ entry, ctx }: { entry: ResolvedEntry; ctx: unknown }) {
  return <Rendered e={entry} ctx={ctx} />;
}

/** 插槽条目的显示名（tabs 插槽的 title_term、columns 插槽的 header_term） */
export function useSlotEntryTitle(): (e: ResolvedEntry) => string {
  const reg = useRegistry();
  return (e) => reg.host.translator.t(e.entry.title_term ?? e.entry.header_term ?? e.plugin);
}

function SlotInner({ name, ctx, className, empty }: { name: SlotName; ctx: unknown; className?: string; empty?: ReactNode }) {
  const entries = useSlotEntries(name);
  const reg = useRegistry();
  const [tab, setTab] = useState(0);
  if (entries.length === 0) return <>{empty ?? null}</>;
  const kind = SLOT_KINDS[name];
  if (kind === 'single') return <Rendered e={entries[0] as ResolvedEntry} ctx={ctx} />;
  if (kind === 'tabs') {
    const cur = entries[Math.min(tab, entries.length - 1)] as ResolvedEntry;
    return (
      <div className={`pt-slot pt-slot-tabs ${className ?? ''}`} data-slot={name}>
        <div role="tablist" className="tabs">
          {entries.map((e, i) => (
            <button key={e.plugin} role="tab" aria-selected={i === tab} tabIndex={i === tab ? 0 : -1} onClick={() => setTab(i)} type="button">
              {reg.host.translator.t(e.entry.title_term ?? e.plugin)}
            </button>
          ))}
        </div>
        <div role="tabpanel" className="tabp">
          <Rendered e={cur} ctx={ctx} />
        </div>
      </div>
    );
  }
  return (
    <div className={`pt-slot ${className ?? ''}`} data-slot={name}>
      {entries.map((e) => (
        <Rendered key={e.plugin} e={e} ctx={ctx} />
      ))}
    </div>
  );
}

/** `<Slot name="user.profile.stats" ctx={{ profile }} />`；`empty` 为没有插件注册时的宿主默认内容（single 插槽的默认实现） */
export function Slot({ name, ctx, fallback, className, empty }: { name: SlotName; ctx: unknown; fallback?: ReactNode; className?: string; empty?: ReactNode }) {
  return (
    <Suspense fallback={fallback ?? null}>
      <SlotInner name={name} ctx={ctx} className={className} empty={empty} />
    </Suspense>
  );
}

/** columns 类型插槽：返回列定义，由表格自行渲染（每个单元格仍在错误边界内） */
export function useSlotColumns(name: SlotName): { key: string; header: string; render(ctx: unknown): ReactNode }[] {
  const entries = useSlotEntries(name);
  const reg = useRegistry();
  return entries.map((e) => ({
    key: e.plugin,
    header: reg.host.translator.t(e.entry.header_term ?? e.plugin),
    render: (ctx: unknown) => <Rendered e={e} ctx={ctx} />,
  }));
}

// ---------------- 插件页面 ----------------

function routeMatches(pattern: string, path: string): boolean {
  if (pattern.endsWith('/*')) {
    const base = pattern.slice(0, -2);
    return path === base || path.startsWith(`${base}/`);
  }
  return pattern === path;
}

function PluginPageInner({ pluginId, path }: { pluginId: string; path: string }) {
  const reg = useRegistry();
  const pl = reg.host.manifest.plugins.find((p) => p.id === pluginId);
  const f = use(useMemo(() => track(pl ? reg.load(pl) : Promise.resolve(null)), [pl, reg]));
  const route = f?.routes?.find((r) => routeMatches(r.path, path));
  if (!route) return <p className="muted">404</p>;
  const C = route.component;
  return (
    <PluginBoundary plugin={pluginId} staff={reg.host.isStaff()}>
      <C api={reg.apiFor(pluginId)} path={path} />
    </PluginBoundary>
  );
}

/** `/p/<plugin>/*` 与 `/admin/p/<plugin>/*` 页面 */
export function PluginPage({ pluginId, path, fallback }: { pluginId: string; path: string; fallback?: ReactNode }) {
  return (
    <Suspense fallback={fallback ?? null}>
      <PluginPageInner pluginId={pluginId} path={path} />
    </Suspense>
  );
}

/** 插件自定义设置组件（settings_ui = custom） */
export function usePluginSettings(pluginId: string): ComponentType<{ api: PluginApi }> | null {
  const reg = useRegistry();
  const pl = reg.host.manifest.plugins.find((p) => p.id === pluginId);
  const f = use(useMemo(() => track(pl ? reg.load(pl) : Promise.resolve(null)), [pl, reg]));
  return f?.settings ?? null;
}

export function usePluginApi(pluginId: string): PluginApi {
  return useRegistry().apiFor(pluginId);
}

export const __test = { routeMatches };
