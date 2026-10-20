// 后台：首页（插件 admin.dashboard.widgets + 常用入口）、插件列表、插件设置（schema 表单 / JSON、未保存提示、恢复默认、插件自定义组件）、插件后台页面。
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Suspense, useEffect, useMemo, useState } from 'react';
import { Link, useLocation, useParams } from 'react-router';
import { api, ApiError } from '@pt-studio/sdk';
import { PluginPage, Slot, usePluginApi, usePluginSettings } from '@pt-studio/slots';
import { JsonField, SchemaForm, same, serverErrors, validate, withDefaults, type Errors, type Schema } from '@pt-studio/schema-form';
import { useT } from '@pt-studio/terms';
import { Button, Empty, Input, Note, PageHead, Panel, Segmented, SkeletonRows, Spinner, Tag, useToast } from '@pt-studio/ui-kit';
import { ErrorText, useErrorText } from '../errors';
import { useManifest } from '../manifest';
import { useSession } from '../session';
import { CATEGORIES, pluginLabel, usePluginList, type PluginInfo as NavPlugin } from '../shell/Layout';

interface PluginInfo extends NavPlugin {
  description?: string;
  summary?: string;
  category?: string;
  version: string;
  kind: string;
  settings?: 'schema' | 'custom' | 'none';
}

const adminHref = (p: PluginInfo): string | null => {
  const r = p.admin_routes?.[0];
  return r ? r.replace(/\*$/, '') : null;
};

export function AdminHome() {
  const t = useT();
  const s = useSession();
  const list = usePluginList();
  const items = (list.data?.items ?? []) as PluginInfo[];
  const withAdmin = items.filter((p) => adminHref(p));
  return (
    <div className="stack">
      <PageHead title={t('ui.admin.dashboard')} lede={t('ui.admin.hello', { name: s.me?.username ?? '' })} />
      <Slot name="admin.dashboard.widgets" ctx={{ user: s.me }} className="g2 dash-widgets" />
      <Panel title={t('ui.admin.shortcuts')} flush primary>
        <div className="plug-grid">
          {s.can('identity.users.read') && (
            <Link className="plug" to="/admin/users">
              <b>{t('ui.admin.users')}</b>
              <span className="tiny muted">{t('ui.admin.users_hint')}</span>
            </Link>
          )}
          {s.can('kernel.audit.read') && (
            <Link className="plug" to="/admin/audit">
              <b>{t('ui.audit.title')}</b>
              <span className="tiny muted">{t('ui.admin.audit_hint')}</span>
            </Link>
          )}
          <Link className="plug" to="/admin/plugins">
            <b>{t('ui.admin.plugins')}</b>
            <span className="tiny muted">{t('ui.admin.plugins_n', { n: items.length })}</span>
          </Link>
          {withAdmin.map((p) => (
            <Link key={p.id} className="plug" to={adminHref(p) as string}>
              <b>{pluginLabel(p, t)}</b>
              <span className="mono">{p.id}</span>
            </Link>
          ))}
        </div>
      </Panel>
    </div>
  );
}

export function AdminPlugins() {
  const t = useT();
  const m = useManifest();
  const list = usePluginList();
  const fe = new Map(m.plugins.map((p) => [p.id, p]));
  const [q, setQ] = useState('');
  const [filter, setFilter] = useState<'configurable' | 'all'>('configurable');
  const all = (list.data?.items ?? []) as PluginInfo[];
  const needle = q.trim().toLowerCase();
  const rows = all
    .filter((p) => filter === 'all' || p.settings !== 'none' || adminHref(p))
    .filter((p) => !needle || [p.id, p.name, pluginLabel(p, t), p.summary ?? '', p.description ?? ''].some((x) => x.toLowerCase().includes(needle)));
  const byCat = CATEGORIES.map((c) => [c, rows.filter((p) => (p.category ?? 'system') === c)] as const).filter(([, ps]) => ps.length > 0);
  return (
    <div className="stack">
      <PageHead title={t('ui.admin.plugins')} lede={t('ui.admin.plugins_lede', { n: all.length })} />
      <Panel flush primary>
        <div className="fbar">
          <Input type="search" placeholder={t('ui.admin.plugin_search')} value={q} onChange={(e) => setQ(e.target.value)} style={{ maxWidth: 320 }} />
          <span className="grow" />
          <Segmented
            value={filter}
            onChange={(v) => setFilter(v)}
            options={[
              { value: 'configurable', label: t('ui.admin.filter_configurable') },
              { value: 'all', label: t('ui.admin.filter_all') },
            ]}
          />
        </div>
        {list.isLoading ? (
          <SkeletonRows n={6} />
        ) : list.error ? (
          <div className="pad">
            <ErrorText error={list.error} />
          </div>
        ) : rows.length === 0 ? (
          <Empty>{t('ui.admin.no_plugins')}</Empty>
        ) : (
          byCat.map(([c, ps]) => (
            <section key={c} className="plug-cat" aria-labelledby={`plug-cat-${c}`}>
              <h3 id={`plug-cat-${c}`}>
                {t(`ui.admin.g_${c}`)} <span className="muted small">{ps.length}</span>
              </h3>
              <div className="plug-grid">
                {ps
                  .sort((a, b) => pluginLabel(a, t).localeCompare(pluginLabel(b, t)))
                  .map((p) => {
                    const admin = adminHref(p);
                    return (
                      <div key={p.id} className="plug">
                        <div className="row-sb">
                          <b>{pluginLabel(p, t)}</b>
                          <span className="row" style={{ gap: 4 }}>
                            <Tag>{p.kind}</Tag>
                            {fe.get(p.id)?.entry && <Tag tone="o">UI</Tag>}
                          </span>
                        </div>
                        {p.summary && <p className="plug-sum">{p.summary}</p>}
                        <span className="mono" title={p.description || undefined}>
                          {p.id} · v{p.version}
                        </span>
                        <div className="row plug-acts">
                          {p.settings !== 'none' ? (
                            <Link className="btn btn-xs" to={`/admin/p/${p.id}/settings`}>
                              {t('ui.admin.settings')}
                            </Link>
                          ) : (
                            <span className="tiny muted">{t('ui.admin.no_settings')}</span>
                          )}
                          {admin && (
                            <Link className="btn btn-xs btn-ghost" to={admin}>
                              {t('ui.admin.manage')}
                            </Link>
                          )}
                        </div>
                      </div>
                    );
                  })}
              </div>
            </section>
          ))
        )}
      </Panel>
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

function SettingsHead({ pluginId, info }: { pluginId: string; info?: PluginInfo }) {
  const t = useT();
  const admin = info && adminHref(info);
  return (
    <PageHead
      crumb={
        <>
          <Link to="/admin/plugins">{t('ui.admin.plugins')}</Link>
          <span>{pluginId}</span>
        </>
      }
      title={
        <>
          {info ? pluginLabel(info, t) : pluginId} <span className="muted h-sub">{t('ui.admin.settings')}</span>
        </>
      }
      lede={
        <>
          <span className="mono">
            {pluginId}
            {info && ` · v${info.version} · ${info.kind}`}
          </span>
          {(info?.summary || info?.description) && <> · {info.summary || info.description}</>}
        </>
      }
      actions={
        admin && (
          <Link className="btn btn-sm" to={admin}>
            {t('ui.admin.manage')}
          </Link>
        )
      }
    />
  );
}

/** 改动的顶层字段（显示名） */
function changedKeys(schema: Schema, draft: unknown, base: unknown): string[] {
  const d = (draft ?? {}) as Record<string, unknown>;
  const b = (base ?? {}) as Record<string, unknown>;
  const keys = new Set([...Object.keys(d), ...Object.keys(b)]);
  return [...keys].filter((k) => !same(d[k], b[k])).map((k) => schema.properties?.[k]?.title ?? k);
}

export function PluginSettings({ pluginId }: { pluginId: string }) {
  const t = useT();
  const toast = useToast();
  const errText = useErrorText();
  const qc = useQueryClient();
  const key = ['plugin-settings', pluginId];
  const q = useQuery({ queryKey: key, queryFn: () => api<SettingsResp>(`/api/kernel/v1/admin/plugins/${pluginId}/settings`) });
  const info = (usePluginList().data?.items as PluginInfo[] | undefined)?.find((p) => p.id === pluginId);
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
  if (q.isLoading || draft === undefined) return q.error ? <ErrorText error={q.error} /> : <SkeletonRows n={6} />;
  const data = q.data as SettingsResp;
  if (data.settings_ui === 'custom') {
    return (
      <div className="stack">
        <SettingsHead pluginId={pluginId} info={info} />
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
    <div className="stack settings-page">
      <SettingsHead pluginId={pluginId} info={info} />
      {noSchema && <Note tone="info">{empty ? t('ui.admin.no_schema_empty') : t('ui.admin.no_schema')}</Note>}
      <Panel
        primary
        title={data.schema && mode === 'form' ? t('ui.admin.mode_form') : 'JSON'}
        actions={
          data.schema && (
            <Segmented
              value={mode}
              onChange={(v) => {
                setMode(v);
                setJsonKey((k) => k + 1);
              }}
              options={[
                { value: 'form', label: t('ui.admin.mode_form') },
                { value: 'json', label: 'JSON' },
              ]}
            />
          )
        }
      >
        {data.schema && mode === 'form' ? (
          <div className="form settings-form">
            <SchemaForm schema={data.schema} value={draft} onChange={setDraft} errors={errors} t={t} baseline={baseline} />
          </div>
        ) : (
          <JsonField key={jsonKey} label={data.schema ? t('ui.admin.json_hint') : 'settings'} value={draft} onChange={setDraft} error={errors['']} />
        )}
      </Panel>
      <div className={dirty ? 'save-bar dirty' : 'save-bar'}>
        <span className="small">
          {dirty ? (
            <>
              <b>{t('ui.admin.unsaved')}</b>
              {changed.length > 0 && <span className="muted"> · {changed.join('、')}</span>}
            </>
          ) : (
            <span className="muted">
              {t('ui.admin.version')} {data.version}
            </span>
          )}
        </span>
        <span className="row">
          <Button variant="ghost" onClick={discard} disabled={!dirty || saving}>
            {t('ui.admin.discard')}
          </Button>
          <Button variant="primary" onClick={save} disabled={!dirty || saving}>
            {saving ? t('ui.common.loading') : t('ui.admin.save')}
          </Button>
        </span>
      </div>
    </div>
  );
}

/** `/admin/p/:plugin/*`：`settings` 为内核设置页，其余交给插件声明的后台路由 */
export function AdminPluginRoute() {
  const { plugin = '' } = useParams();
  const loc = useLocation();
  const sub = loc.pathname.slice(`/admin/p/${plugin}`.length);
  // 插件声明了后台路由时，`/admin/p/x/` 交给插件；设置页固定在 `/settings`
  const hasAdmin = useManifest().plugins.some((p) => p.id === plugin && p.routes.some((r) => r.startsWith(`/admin/p/${plugin}`)));
  if (sub === '/settings' || (!hasAdmin && (sub === '' || sub === '/'))) return <PluginSettings key={plugin} pluginId={plugin} />;
  return <PluginPage pluginId={plugin} path={loc.pathname} fallback={<SkeletonRows n={6} />} />;
}
