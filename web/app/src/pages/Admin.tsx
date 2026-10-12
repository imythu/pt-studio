// 后台：插件列表、插件设置（schema 表单 / 插件自定义组件 / JSON 编辑）、插件后台页面。
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Suspense, useEffect, useState } from 'react';
import { Link, useLocation, useParams } from 'react-router';
import { api } from '@pt-studio/sdk';
import { PluginPage, Slot, usePluginApi, usePluginSettings } from '@pt-studio/slots';
import { JsonField, SchemaForm, serverErrors, validate, withDefaults, type Errors, type Schema } from '@pt-studio/schema-form';
import { useT } from '@pt-studio/terms';
import { Badge, Button, Card, Spinner, Table, useToast } from '@pt-studio/ui-kit';
import { ApiError } from '@pt-studio/sdk';
import { ErrorText, useErrorText } from '../errors';
import { useManifest } from '../manifest';
import { useSession } from '../session';

interface PluginInfo {
  id: string;
  name: string;
  version: string;
  kind: string;
}

export function AdminHome() {
  const t = useT();
  const s = useSession();
  const m = useManifest();
  const list = useQuery({ queryKey: ['kernel-plugins'], queryFn: () => api<{ items: PluginInfo[] }>('/api/kernel/v1/plugins') });
  const fe = new Map(m.plugins.map((p) => [p.id, p]));
  return (
    <div>
      <h1>{t('ui.admin.title')}</h1>
      <nav className="admin-nav">
        {s.can('identity.users.read') && <Link to="/admin/users">{t('ui.admin.users')}</Link>} <Slot name="nav.admin" ctx={{ user: s.me }} />
      </nav>
      <Slot name="admin.dashboard.widgets" ctx={{ user: s.me }} />
      <Card title={t('ui.admin.plugins')}>
        {list.isLoading ? (
          <Spinner />
        ) : (
          <Table
            rows={list.data?.items ?? []}
            rowKey={(r) => r.id}
            columns={[
              { key: 'id', header: 'ID', render: (r) => <span className="pt-mono">{r.id}</span> },
              { key: 'name', header: 'Name', render: (r) => r.name },
              { key: 'v', header: 'Version', render: (r) => r.version },
              { key: 'k', header: 'Kind', render: (r) => <Badge>{r.kind}</Badge> },
              { key: 'ui', header: 'UI', render: (r) => (fe.get(r.id)?.entry ? <Badge tone="primary">ESM</Badge> : null) },
              { key: 's', header: '', render: (r) => <Link to={`/admin/p/${r.id}/settings`}>{t('ui.admin.settings')}</Link> },
            ]}
          />
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

export function PluginSettings({ pluginId }: { pluginId: string }) {
  const t = useT();
  const toast = useToast();
  const errText = useErrorText();
  const qc = useQueryClient();
  const key = ['plugin-settings', pluginId];
  const q = useQuery({ queryKey: key, queryFn: () => api<SettingsResp>(`/api/kernel/v1/admin/plugins/${pluginId}/settings`) });
  const [draft, setDraft] = useState<unknown>(undefined);
  const [errors, setErrors] = useState<Errors>({});
  useEffect(() => {
    if (q.data) setDraft(q.data.schema ? withDefaults(q.data.schema, q.data.settings) : q.data.settings);
  }, [q.data]);
  if (q.isLoading || draft === undefined) return q.error ? <ErrorText error={q.error} /> : <Spinner />;
  const data = q.data as SettingsResp;
  if (data.settings_ui === 'custom') {
    return (
      <Suspense fallback={<Spinner />}>
        <CustomSettings pluginId={pluginId} />
      </Suspense>
    );
  }
  const save = async () => {
    const errs = data.schema ? validate(data.schema, draft) : {};
    setErrors(errs);
    if (Object.keys(errs).length) return;
    try {
      await api(`/api/kernel/v1/admin/plugins/${pluginId}/settings`, { method: 'PUT', json: { settings: draft, version: data.version } });
      toast(t('ui.admin.saved'), 'success');
      await qc.invalidateQueries({ queryKey: key });
    } catch (e) {
      if (e instanceof ApiError) setErrors(serverErrors(e.details));
      toast(errText(e), 'error');
    }
  };
  return (
    <Card title={`${pluginId} · ${t('ui.admin.settings')}`} actions={<Button variant="primary" onClick={save}>{t('ui.admin.save')}</Button>}>
      {data.schema ? (
        <SchemaForm schema={data.schema} value={draft} onChange={setDraft} errors={errors} t={t} />
      ) : (
        <JsonField label="settings (JSON)" value={draft} onChange={setDraft} error={errors['']} />
      )}
    </Card>
  );
}

/** `/admin/p/:plugin/*`：`settings` 为内核设置页，其余交给插件声明的后台路由 */
export function AdminPluginRoute() {
  const { plugin = '' } = useParams();
  const loc = useLocation();
  const sub = loc.pathname.slice(`/admin/p/${plugin}`.length);
  if (sub === '/settings' || sub === '' || sub === '/') return <PluginSettings pluginId={plugin} />;
  return <PluginPage pluginId={plugin} path={loc.pathname} fallback={<Spinner />} />;
}
