// type-generic 前端：后台页面（/admin/p/type-generic/）—— 自定义资源类型列表，以 JSON 编辑类型定义（字段、校验），新建、保存、停用。
import { useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, MasterDetail, PageHead, Panel, Sheet, SkeletonRows, Textarea } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'type-generic.ui.title': '自定义资源类型',
  'type-generic.ui.hint': '每个类型的定义包含 kind（标识）、中英文名称、图标与字段列表；保存时校验，发布页按定义生成表单。',
  'type-generic.ui.new': '新建类型',
  'type-generic.ui.save': '保存',
  'type-generic.ui.disable': '停用',
  'type-generic.ui.disable_confirm': '停用后不能再选择该类型发布，已发布的种子不受影响。继续？',
  'type-generic.ui.saved': '已保存',
  'type-generic.ui.cancel': '取消',
  'type-generic.ui.pick': '选择左侧的类型编辑定义',
});
registerMessages('en', {
  'type-generic.ui.title': 'Custom resource types',
  'type-generic.ui.hint': 'Each definition has a kind id, Chinese / English names, an icon and a field list; it is validated on save and drives the upload form.',
  'type-generic.ui.new': 'New type',
  'type-generic.ui.save': 'Save',
  'type-generic.ui.disable': 'Disable',
  'type-generic.ui.disable_confirm': 'Disabled types can no longer be chosen for uploads; existing torrents are unaffected. Continue?',
  'type-generic.ui.saved': 'Saved',
  'type-generic.ui.cancel': 'Cancel',
  'type-generic.ui.pick': 'Pick a type on the left',
});

interface Kind {
  kind: string;
  name_zh: string;
  name_en: string;
  definition: Record<string, unknown>;
}

const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code ? `${api.t(e.code)} ${e.message}` : e instanceof Error ? e.message : String(e);

function Editor({ api, kind, initial, onDone, onDisable }: { api: PluginApi; kind: string | null; initial: string; onDone(): void; onDisable?(): void }) {
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
    <div className="form">
      <Textarea className="ta pt-mono" aria-label="JSON" rows={22} spellCheck={false} value={text} onChange={(e) => setText(e.target.value)} />
      <div className="form-acts">
        {onDisable && (
          <Button variant="ghost" onClick={onDisable}>
            {api.t('type-generic.ui.disable')}
          </Button>
        )}
        <Button variant="primary" className="push-right" onClick={() => void save()}>
          {api.t('type-generic.ui.save')}
        </Button>
      </div>
    </div>
  );
}

/** P2 主从：左侧类型列表，右侧 JSON 编辑；停用在抽屉里确认 */
function Page({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<Kind[] | null>(null);
  const [rev, setRev] = useState(0);
  const [cur, setCur] = useState<string | null>(null);
  const [disabling, setDisabling] = useState<string | null>(null);
  useEffect(() => {
    void api.json<{ items: Kind[] }>('/api/p/type-generic/v1/kinds').then((r) => setRows(r.items));
  }, [api, rev]);
  const reload = () => setRev((x) => x + 1);
  const disable = async () => {
    if (!disabling) return;
    try {
      await api.json(`/api/p/type-generic/v1/admin/kinds/${disabling}`, { method: 'DELETE' });
      setDisabling(null);
      setCur(null);
      reload();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  if (!rows) return <SkeletonRows n={6} />;
  const template = rows[0] ? { ...rows[0].definition, kind: 'new_kind', name_zh: '新类型', name_en: 'New type' } : { kind: 'new_kind', name_zh: '新类型', name_en: 'New type', fields: [] };
  const k = rows.find((r) => r.kind === cur);
  return (
    <div className="stack">
      <PageHead
        title={api.t('type-generic.ui.title')}
        lede={api.t('type-generic.ui.hint')}
        actions={
          <Button variant="primary" onClick={() => setCur('')}>
            {api.t('type-generic.ui.new')}
          </Button>
        }
      />
      <MasterDetail
        listWidth={280}
        showDetail={cur !== null}
        onBack={() => setCur(null)}
        list={
          <Panel flush>
            <div className="side-list">
              {rows.map((r) => (
                <button key={r.kind} type="button" aria-current={r.kind === cur ? 'true' : undefined} onClick={() => setCur(r.kind)}>
                  <span className="rk">·</span>
                  <span className="nm">{`${r.name_zh} / ${r.name_en}`}</span>
                  <span className="v">{r.kind}</span>
                </button>
              ))}
            </div>
          </Panel>
        }
        detail={
          <Panel title={cur === '' ? api.t('type-generic.ui.new') : k ? `${k.name_zh} / ${k.name_en}` : ''} sub={k?.kind}>
            {cur === '' ? (
              <Editor key={`new:${rev}`} api={api} kind={null} initial={JSON.stringify(template, null, 2)} onDone={reload} />
            ) : k ? (
              <Editor key={`${k.kind}:${rev}`} api={api} kind={k.kind} initial={JSON.stringify(k.definition, null, 2)} onDone={reload} onDisable={() => setDisabling(k.kind)} />
            ) : (
              <div className="md-empty">{api.t('type-generic.ui.pick')}</div>
            )}
          </Panel>
        }
      />
      <Sheet
        open={disabling !== null}
        onClose={() => setDisabling(null)}
        title={api.t('type-generic.ui.disable')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDisabling(null)}>
              {api.t('type-generic.ui.cancel')}
            </Button>
            <Button variant="danger" onClick={() => void disable()}>
              {api.t('type-generic.ui.disable')}
            </Button>
          </>
        }
      >
        <p>{api.t('type-generic.ui.disable_confirm')}</p>
      </Sheet>
    </div>
  );
}

export default { slots: {}, routes: [{ path: '/admin/p/type-generic/*', component: Page }] } satisfies PluginFrontend;
