// 后台：插件列表（卡片、搜索、按可配置筛选）、插件设置（schema 可视化表单 + JSON 视图、未保存提示 / 恢复默认 / 插件自定义组件）、插件后台页面。
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Suspense, useEffect, useMemo, useState } from 'react';
import { Link, useLocation, useParams } from 'react-router';
import { api } from '@pt-studio/sdk';
import { PluginPage, Slot, usePluginApi, usePluginSettings } from '@pt-studio/slots';
import { JsonField, SchemaForm, same, serverErrors, validate, withDefaults, type Errors, type Schema } from '@pt-studio/schema-form';
import { useT } from '@pt-studio/terms';
import { Badge, Button, Card, Empty, Spinner, Tabs, useToast } from '@pt-studio/ui-kit';
import { ApiError } from '@pt-studio/sdk';
import { ErrorText, useErrorText } from '../errors';
import { useManifest } from '../manifest';
import { useSession } from '../session';

interface PluginInfo {
  id: string;
  name: string;
  name_term?: string | null;
  description?: string;
  version: string;
  kind: string;
  /** 后台设置入口：`schema` 可视化表单、`custom` 插件自带组件、`none` 未声明 */
  settings?: 'schema' | 'custom' | 'none';
  admin_routes?: string[];
}

function usePlugins() {
  return useQuery({ queryKey: ['kernel-plugins'], queryFn: () => api<{ items: PluginInfo[] }>('/api/kernel/v1/plugins') });
}

/** 插件显示名：术语优先 */
function displayName(p: PluginInfo, t: (k: string) => string): string {
  if (p.name_term) {
    const v = t(`term:${p.name_term}`);
    if (v !== p.name_term) return v;
  }
  return p.name;
}

/** 插件后台页入口：`/admin/p/x/*` → `/admin/p/x/` */
function adminHref(p: PluginInfo): string | null {
  const r = p.admin_routes?.[0];
  return r ? r.replace(/\*$/, '') : null;
}

type Filter = 'configurable' | 'all';

export function AdminHome() {
  const t = useT();
  const s = useSession();
  const m = useManifest();
  const list = usePlugins();
  const fe = new Map(m.plugins.map((p) => [p.id, p]));
  const [q, setQ] = useState('');
  const [filter, setFilter] = useState<Filter>('configurable');
  const all = list.data?.items ?? [];
  const needle = q.trim().toLowerCase();
  const rows = all
    .filter((p) => filter === 'all' || p.settings !== 'none' || adminHref(p))
    .filter((p) => !needle || [p.id, p.name, displayName(p, t), p.description ?? ''].some((x) => x.toLowerCase().includes(needle)));
  return (
    <div>
      <h1>{t('ui.admin.title')}</h1>
      <nav className="admin-nav">
        {s.can('identity.users.read') && <Link to="/admin/users">{t('ui.admin.users')}</Link>} <Slot name="nav.admin" ctx={{ user: s.me }} />
      </nav>
      <Slot name="admin.dashboard.widgets" ctx={{ user: s.me }} />
      <Card
        title={`${t('ui.admin.plugins')} · ${all.length}`}
        actions={
          <div className="plugin-toolbar">
            <input className="pt-input" type="search" placeholder={t('ui.admin.plugin_search')} value={q} onChange={(e) => setQ(e.target.value)} />
            <Tabs
              value={filter}
              onChange={(v) => setFilter(v as Filter)}
              tabs={[
                { id: 'configurable', label: t('ui.admin.filter_configurable') },
                { id: 'all', label: t('ui.admin.filter_all') },
              ]}
            />
          </div>
        }
      >
        {list.isLoading ? (
          <Spinner />
        ) : list.error ? (
          <ErrorText error={list.error} />
        ) : rows.length === 0 ? (
          <Empty>{t('ui.admin.no_plugins')}</Empty>
        ) : (
          <div className="plugin-grid">
            {rows.map((p) => {
              const admin = adminHref(p);
              return (
                <div key={p.id} className="plugin-card">
                  <div className="plugin-card-head">
                    <div>
                      <div className="plugin-card-name">{displayName(p, t)}</div>
                      <div className="pt-muted pt-mono">
                        {p.id} · v{p.version}
                      </div>
                    </div>
                    <span>
                      <Badge>{p.kind}</Badge>
                      {fe.get(p.id)?.entry && <Badge tone="primary">UI</Badge>}
                    </span>
                  </div>
                  {p.description && <p className="plugin-card-desc">{p.description}</p>}
                  <div className="plugin-card-foot">
                    {p.settings === 'none' ? (
                      <span className="pt-muted">{t('ui.admin.no_settings')}</span>
                    ) : (
                      <Link className="pt-btn" to={`/admin/p/${p.id}/settings`}>
                        {t('ui.admin.settings')}
                      </Link>
                    )}
                    {admin && (
                      <Link className="pt-btn pt-btn-ghost" to={admin}>
                        {t('ui.admin.manage')} →
                      </Link>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </Card>
    </div>
  );
}

interface SettingsResp {
  settings: Record<string, unknown>;
  version: number;
  schema: Schema | null;
  settings_ui: string;
}

function CustomSettings({ pluginId }: { pluginId: string }) {
  const C = usePluginSettings(pluginId);
  const apiFor = usePluginApi(pluginId);
  return C ? <C api={apiFor} /> : null;
}

function SettingsHeader({ pluginId, info }: { pluginId: string; info?: PluginInfo }) {
  const t = useT();
  const admin = info && adminHref(info);
  return (
    <div className="settings-head">
      <Link to="/admin" className="pt-muted">
        ← {t('ui.admin.plugins')}
      </Link>
      <h1>
        {info ? displayName(info, t) : pluginId} <span className="pt-muted settings-head-sub">{t('ui.admin.settings')}</span>
      </h1>
      <div className="pt-muted">
        <span className="pt-mono">
          {pluginId}
          {info && ` · v${info.version}`}
        </span>
        {info && <Badge>{info.kind}</Badge>}
        {admin && <Link to={admin}>{t('ui.admin.manage')} →</Link>}
      </div>
      {info?.description && <p className="settings-head-desc">{info.description}</p>}
    </div>
  );
}

export function PluginSettings({ pluginId }: { pluginId: string }) {
  const t = useT();
  const toast = useToast();
  const errText = useErrorText();
  const qc = useQueryClient();
  const key = ['plugin-settings', pluginId];
  const q = useQuery({ queryKey: key, queryFn: () => api<SettingsResp>(`/api/kernel/v1/admin/plugins/${pluginId}/settings`) });
  const info = usePlugins().data?.items.find((p) => p.id === pluginId);
  const [draft, setDraft] = useState<unknown>(undefined);
  const [errors, setErrors] = useState<Errors>({});
  const [mode, setMode] = useState<'form' | 'json'>('form');
  const [jsonKey, setJsonKey] = useState(0);
  const [saving, setSaving] = useState(false);
  const baseline = useMemo(() => (q.data ? (q.data.schema ? withDefaults(q.data.schema, q.data.settings) : q.data.settings) : undefined), [q.data]);
  useEffect(() => {
    if (baseline !== undefined) setDraft(baseline);
  }, [baseline]);
  const dirty = baseline !== undefined && draft !== undefined && !same(draft, baseline);
  useEffect(() => {
    if (!dirty) return;
    const warn = (e: BeforeUnloadEvent) => e.preventDefault();
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [dirty]);
  if (q.isLoading || draft === undefined) return q.error ? <ErrorText error={q.error} /> : <Spinner />;
  const data = q.data as SettingsResp;
  if (data.settings_ui === 'custom') {
    return (
      <div className="settings-page">
        <SettingsHeader pluginId={pluginId} info={info} />
        <Suspense fallback={<Spinner />}>
          <CustomSettings pluginId={pluginId} />
        </Suspense>
      </div>
    );
  }
  const changed = data.schema ? changedKeys(data.schema, draft, baseline) : [];
  const save = async () => {
    const errs = data.schema ? validate(data.schema, draft) : {};
    setErrors(errs);
    if (Object.keys(errs).length) {
      toast(t('ui.admin.fix_errors'), 'error');
      return;
    }
    setSaving(true);
    try {
      await api(`/api/kernel/v1/admin/plugins/${pluginId}/settings`, { method: 'PUT', json: { settings: draft, version: data.version } });
      toast(t('ui.admin.saved'), 'success');
      await qc.invalidateQueries({ queryKey: key });
    } catch (e) {
      if (e instanceof ApiError) setErrors(serverErrors(e.details));
      toast(errText(e), 'error');
    } finally {
      setSaving(false);
    }
  };
  const discard = () => {
    setDraft(baseline);
    setErrors({});
    setJsonKey((k) => k + 1);
  };
  const noSchema = !data.schema;
  const empty = noSchema && same(data.settings, {});
  return (
    <div className="settings-page">
      <SettingsHeader pluginId={pluginId} info={info} />
      {noSchema && (
        <div className="settings-note">
          {empty ? t('ui.admin.no_schema_empty') : t('ui.admin.no_schema')}
        </div>
      )}
      <Card
        title={
          data.schema ? (
            <Tabs
              value={mode}
              onChange={(v) => {
                setMode(v as 'form' | 'json');
                setJsonKey((k) => k + 1);
              }}
              tabs={[
                { id: 'form', label: t('ui.admin.mode_form') },
                { id: 'json', label: 'JSON' },
              ]}
            />
          ) : (
            'JSON'
          )
        }
      >
        {data.schema && mode === 'form' ? (
          <div className="settings-form">
            <SchemaForm schema={data.schema} value={draft} onChange={setDraft} errors={errors} t={t} baseline={baseline} />
          </div>
        ) : (
          <JsonField key={jsonKey} label={data.schema ? t('ui.admin.json_hint') : 'settings'} value={draft} onChange={setDraft} error={errors['']} />
        )}
      </Card>
      <div className={`settings-bar${dirty ? ' settings-bar-dirty' : ''}`}>
        <span className="settings-bar-msg">
          {dirty ? (
            <>
              {t('ui.admin.unsaved')}
              {changed.length > 0 && <span className="pt-muted"> · {changed.join(', ')}</span>}
            </>
          ) : (
            <span className="pt-muted">
              {t('ui.admin.version')} {data.version}
            </span>
          )}
        </span>
        <Button variant="ghost" onClick={discard} disabled={!dirty || saving}>
          {t('ui.admin.discard')}
        </Button>
        <Button variant="primary" onClick={save} disabled={!dirty || saving}>
          {saving ? t('ui.common.loading') : t('ui.admin.save')}
        </Button>
      </div>
    </div>
  );
}

/** 改动的顶层字段（显示名） */
function changedKeys(schema: Schema, draft: unknown, base: unknown): string[] {
  const d = (draft ?? {}) as Record<string, unknown>;
  const b = (base ?? {}) as Record<string, unknown>;
  const keys = new Set([...Object.keys(d), ...Object.keys(b)]);
  return [...keys].filter((k) => !same(d[k], b[k])).map((k) => schema.properties?.[k]?.title ?? k);
}

/** `/admin/p/:plugin/*`：`settings` 为内核设置页，其余交给插件声明的后台路由 */
export function AdminPluginRoute() {
  const { plugin = '' } = useParams();
  const loc = useLocation();
  const sub = loc.pathname.slice(`/admin/p/${plugin}`.length);
  // 插件声明了后台路由时，`/admin/p/x/` 交给插件；设置页固定在 `/settings`
  const hasAdmin = useManifest().plugins.some((p) => p.id === plugin && p.routes.some((r) => r.startsWith(`/admin/p/${plugin}`)));
  if (sub === '/settings' || (!hasAdmin && (sub === '' || sub === '/'))) return <PluginSettings key={plugin} pluginId={plugin} />;
  return <PluginPage pluginId={plugin} path={loc.pathname} fallback={<Spinner />} />;
}
