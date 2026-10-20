// metadata-douban 前端：发布页“从豆瓣生成简介”（PT-Gen 版式，回填描述与标题），后台启用页（风险声明）。
import { useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Field, Input, Note, PageHead, Panel, SkeletonRows, Status } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'metadata-douban.ui.link': '豆瓣链接或 ID',
  'metadata-douban.ui.generate': '生成简介',
  'metadata-douban.ui.done': '简介已填入描述',
  'metadata-douban.ui.admin': '豆瓣元数据',
  'metadata-douban.ui.enable': '我已了解并启用',
  'metadata-douban.ui.disable': '停用',
  'metadata-douban.ui.on': '已启用',
  'metadata-douban.ui.off': '未启用',
  'metadata-douban.ui.unconfigured': '还没有在 site.toml 中配置代理或 PT-Gen 地址。',
  'metadata-douban.not_enabled': '豆瓣数据源未启用',
  'metadata-douban.not_configured': '豆瓣数据源未配置',
  'metadata-douban.id_invalid': '不是有效的豆瓣链接或 ID',
  'metadata-douban.not_found': '豆瓣条目不存在',
  'metadata-douban.upstream_error': '豆瓣数据源暂时不可用',
});
registerMessages('en', {
  'metadata-douban.ui.link': 'Douban link or ID',
  'metadata-douban.ui.generate': 'Generate intro',
  'metadata-douban.ui.done': 'The intro was added to the description',
  'metadata-douban.ui.admin': 'Douban metadata',
  'metadata-douban.ui.enable': 'I understand, enable',
  'metadata-douban.ui.disable': 'Disable',
  'metadata-douban.ui.on': 'Enabled',
  'metadata-douban.ui.off': 'Disabled',
  'metadata-douban.ui.unconfigured': 'No proxy or PT-Gen endpoint is configured in site.toml yet.',
  'metadata-douban.not_enabled': 'The Douban source is not enabled',
  'metadata-douban.not_configured': 'The Douban source is not configured',
  'metadata-douban.id_invalid': 'Not a valid Douban link or ID',
  'metadata-douban.not_found': 'Douban subject not found',
  'metadata-douban.upstream_error': 'The Douban source is temporarily unavailable',
});

const errText = (api: PluginApi, e: unknown) => (e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e));

type Draft = { title: string; subtitle: string; description: string };

function Generate({ api, ctx }: SlotProps<{ draft: Draft; update?: (p: Partial<Draft>) => void }>) {
  const [link, setLink] = useState('');
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    try {
      const r = await api.json<{ format: string; title: string | null }>(`/api/p/metadata-douban/v1/ptgen?markup=text&id=${encodeURIComponent(link.trim())}`);
      const d = ctx.draft;
      ctx.update?.({
        description: d.description.trim() ? `${d.description.trim()}\n\n${r.format}` : r.format,
        ...(d.subtitle.trim() || !r.title ? {} : { subtitle: r.title }),
      });
      api.toast(api.t('metadata-douban.ui.done'), 'success');
    } catch (e) {
      api.toast(errText(api, e), 'error');
    } finally {
      setBusy(false);
    }
  };
  if (!ctx.update) return null;
  return (
    <Field label={api.t('metadata-douban.ui.link')}>
      <span className="form-acts">
        <Input name="douban_link" placeholder="https://movie.douban.com/subject/1292052/" value={link} onChange={(e) => setLink(e.target.value)} />
        <Button type="button" disabled={busy || !link.trim()} onClick={() => void run()}>
          {api.t('metadata-douban.ui.generate')}
        </Button>
      </span>
    </Field>
  );
}

function Admin({ api }: { api: PluginApi }) {
  const [s, setS] = useState<{ enabled: boolean; configured: boolean; mode: string; statement: string; statement_version: number } | null>(null);
  const load = () => api.json<typeof s>('/api/p/metadata-douban/v1/admin/status').then(setS);
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  if (!s) return <SkeletonRows n={3} />;
  const act = async (path: string, body: unknown) => {
    try {
      await api.json(`/api/p/metadata-douban/v1/admin/${path}`, { json: body });
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <div className="stack">
      <PageHead
        title={api.t('metadata-douban.ui.admin')}
        actions={
          s.enabled ? (
            <Button variant="danger" onClick={() => void act('disable', {})}>
              {api.t('metadata-douban.ui.disable')}
            </Button>
          ) : (
            <Button variant="primary" onClick={() => void act('enable', { ack: true, statement_version: s.statement_version })}>
              {api.t('metadata-douban.ui.enable')}
            </Button>
          )
        }
      />
      {!s.configured && <Note tone="bad">{api.t('metadata-douban.ui.unconfigured')}</Note>}
      <Panel
        primary
        title={
          <>
            <Status tone={s.enabled ? 'ok' : 'mute'}>{api.t(s.enabled ? 'metadata-douban.ui.on' : 'metadata-douban.ui.off')}</Status> <span className="pt-mono small">{s.mode}</span>
          </>
        }
      >
        <p className="pt-prewrap">{s.statement}</p>
      </Panel>
    </div>
  );
}

export default {
  slots: {
    'torrent.upload.extra': { component: Generate, order: 20 },
  },
  routes: [{ path: '/admin/p/metadata-douban/*', component: Admin }],
} satisfies PluginFrontend;
