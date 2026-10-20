// metadata-douban 前端：发布页“从豆瓣生成简介”（PT-Gen 版式，回填描述与标题）。启用与配置在后台插件设置（schema 表单）。
import { useQuery } from '@tanstack/react-query';
import { useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Field, Input } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'metadata-douban.ui.link': '豆瓣链接或 ID',
  'metadata-douban.ui.generate': '生成简介',
  'metadata-douban.ui.done': '简介已填入描述',
  'metadata-douban.not_enabled': '豆瓣数据源未启用',
  'metadata-douban.not_configured': '豆瓣数据源未配置',
  'metadata-douban.id_invalid': '不是有效的豆瓣链接或 ID',
  'metadata-douban.not_found': '豆瓣条目不存在',
  'metadata-douban.upstream_error': '豆瓣数据源暂时不可用',
  'metadata-douban.blocked': '豆瓣拒绝了请求（限流），请稍后重试或在插件设置中填写 Cookie',
});
registerMessages('en', {
  'metadata-douban.ui.link': 'Douban link or ID',
  'metadata-douban.ui.generate': 'Generate intro',
  'metadata-douban.ui.done': 'The intro was added to the description',
  'metadata-douban.not_enabled': 'The Douban source is not enabled',
  'metadata-douban.not_configured': 'The Douban source is not configured',
  'metadata-douban.id_invalid': 'Not a valid Douban link or ID',
  'metadata-douban.not_found': 'Douban subject not found',
  'metadata-douban.upstream_error': 'The Douban source is temporarily unavailable',
  'metadata-douban.blocked': 'Douban rejected the request (rate limited); retry later or set a cookie in the plugin settings',
});

const errText = (api: PluginApi, e: unknown) => (e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e));

type Draft = { title: string; subtitle: string; description: string };

function Generate({ api, ctx }: SlotProps<{ draft: Draft; update?: (p: Partial<Draft>) => void }>) {
  const [link, setLink] = useState('');
  const [busy, setBusy] = useState(false);
  const st = useQuery({
    queryKey: ['metadata-douban', 'status'],
    queryFn: () => api.json<{ enabled: boolean }>('/api/p/metadata-douban/v1/status'),
    staleTime: 300_000,
  });
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
  if (!ctx.update || !st.data?.enabled) return null;
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

export default {
  slots: {
    'torrent.upload.extra': { component: Generate, order: 20 },
  },
} satisfies PluginFrontend;
