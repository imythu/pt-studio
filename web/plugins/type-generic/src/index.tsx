// type-generic 前端：后台页面（/admin/p/type-generic/）—— 自定义资源类型列表，以 JSON 编辑类型定义（字段、校验），新建、保存、停用。
import { useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Card } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'type-generic.ui.title': '自定义资源类型',
  'type-generic.ui.hint': '每个类型的定义包含 kind（标识）、中英文名称、图标与字段列表；保存时校验，发布页按定义生成表单。',
  'type-generic.ui.new': '新建类型',
  'type-generic.ui.save': '保存',
  'type-generic.ui.disable': '停用',
  'type-generic.ui.disable_confirm': '停用后不能再选择该类型发布，已发布的种子不受影响。继续？',
  'type-generic.ui.saved': '已保存',
});
registerMessages('en', {
  'type-generic.ui.title': 'Custom resource types',
  'type-generic.ui.hint': 'Each definition has a kind id, Chinese / English names, an icon and a field list; it is validated on save and drives the upload form.',
  'type-generic.ui.new': 'New type',
  'type-generic.ui.save': 'Save',
  'type-generic.ui.disable': 'Disable',
  'type-generic.ui.disable_confirm': 'Disabled types can no longer be chosen for uploads; existing torrents are unaffected. Continue?',
  'type-generic.ui.saved': 'Saved',
});

interface Kind {
  kind: string;
  name_zh: string;
  name_en: string;
  definition: Record<string, unknown>;
}

const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code ? `${api.t(e.code)} ${e.message}` : e instanceof Error ? e.message : String(e);

function Editor({ api, kind, initial, onDone }: { api: PluginApi; kind: string | null; initial: string; onDone(): void }) {
  const [text, setText] = useState(initial);
  const save = async () => {
    let v: unknown;
    try {
      v = JSON.parse(text);
    } catch (e) {
      api.toast(String(e), 'error');
      return;
    }
    try {
      if (kind) await api.json(`/api/p/type-generic/v1/admin/kinds/${kind}`, { method: 'PATCH', json: v });
      else await api.json('/api/p/type-generic/v1/admin/kinds', { json: v });
      api.toast(api.t('type-generic.ui.saved'), 'success');
      onDone();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <>
      <textarea className="pt-input pt-mono" rows={14} value={text} onChange={(e) => setText(e.target.value)} />
      <Button variant="primary" onClick={() => void save()}>
        {api.t('type-generic.ui.save')}
      </Button>
    </>
  );
}

function Page({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<Kind[]>([]);
  const [rev, setRev] = useState(0);
  useEffect(() => {
    void api.json<{ items: Kind[] }>('/api/p/type-generic/v1/kinds').then((r) => setRows(r.items));
  }, [api, rev]);
  const reload = () => setRev((x) => x + 1);
  const disable = async (kind: string) => {
    if (!window.confirm(api.t('type-generic.ui.disable_confirm'))) return;
    try {
      await api.json(`/api/p/type-generic/v1/admin/kinds/${kind}`, { method: 'DELETE' });
      reload();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const template = rows[0] ? { ...rows[0].definition, kind: 'new_kind', name_zh: '新类型', name_en: 'New type' } : { kind: 'new_kind', name_zh: '新类型', name_en: 'New type', fields: [] };
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t('type-generic.ui.title')}</h1>
      <p className="pt-muted">{api.t('type-generic.ui.hint')}</p>
      {rows.map((k) => (
        <Card
          key={`${k.kind}:${rev}`}
          title={`${k.name_zh} / ${k.name_en} (${k.kind})`}
          actions={
            <Button variant="ghost" onClick={() => void disable(k.kind)}>
              {api.t('type-generic.ui.disable')}
            </Button>
          }
        >
          <Editor api={api} kind={k.kind} initial={JSON.stringify(k.definition, null, 2)} onDone={reload} />
        </Card>
      ))}
      <Card key={`new:${rev}`} title={api.t('type-generic.ui.new')}>
        <Editor api={api} kind={null} initial={JSON.stringify(template, null, 2)} onDone={reload} />
      </Card>
    </div>
  );
}

export default { slots: {}, routes: [{ path: '/admin/p/type-generic/*', component: Page }] } satisfies PluginFrontend;
