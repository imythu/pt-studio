// attachments 前端：我的图片（上传、复制 BBCode / Markdown、删除）、后台最近上传、发布页“插入图片”到简介。
import { useEffect, useState, type ReactNode } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { formatBytes, registerMessages } from '@pt-studio/terms';
import { Button, Card, Field } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'attachments.ui.menu': '我的图片',
  'attachments.ui.title': '我的图片',
  'attachments.ui.admin': '用户上传的图片',
  'attachments.ui.upload': '上传图片',
  'attachments.ui.hint': 'PNG / JPEG / GIF / WebP，单张不超过 {size}；会自动去除照片中的位置等元数据',
  'attachments.ui.quota': '今天已上传 {count} / {max} 张，{bytes} / {maxBytes}',
  'attachments.ui.copy_bbcode': '复制 BBCode',
  'attachments.ui.copy_markdown': '复制 Markdown',
  'attachments.ui.copied': '已复制',
  'attachments.ui.delete': '删除',
  'attachments.ui.delete_confirm': '删除后引用这张图片的地方将无法显示，确定删除？',
  'attachments.ui.empty': '还没有上传图片',
  'attachments.ui.insert': '插入图片到简介',
  'attachments.ui.inserted': '图片已插入简介',
  'attachments.ui.more': '加载更多',
  'attachments.too_large': '图片太大',
  'attachments.unsupported': '只支持 PNG、JPEG、GIF、WebP 图片',
  'attachments.invalid': '图片文件已损坏',
  'attachments.quota_exceeded': '已达到今天的上传上限',
});
registerMessages('en', {
  'attachments.ui.menu': 'My images',
  'attachments.ui.title': 'My images',
  'attachments.ui.admin': 'Uploaded images',
  'attachments.ui.upload': 'Upload images',
  'attachments.ui.hint': 'PNG / JPEG / GIF / WebP, up to {size} each; location and other metadata are removed',
  'attachments.ui.quota': 'Today: {count} / {max} images, {bytes} / {maxBytes}',
  'attachments.ui.copy_bbcode': 'Copy BBCode',
  'attachments.ui.copy_markdown': 'Copy Markdown',
  'attachments.ui.copied': 'Copied',
  'attachments.ui.delete': 'Delete',
  'attachments.ui.delete_confirm': 'Places that use this image will no longer show it. Delete?',
  'attachments.ui.empty': 'No images yet',
  'attachments.ui.insert': 'Insert image into the description',
  'attachments.ui.inserted': 'The image was added to the description',
  'attachments.ui.more': 'Load more',
  'attachments.too_large': 'The image is too large',
  'attachments.unsupported': 'Only PNG, JPEG, GIF and WebP images are accepted',
  'attachments.invalid': 'The image file is damaged',
  'attachments.quota_exceeded': "You have reached today's upload limit",
});

const errText = (api: PluginApi, e: unknown) => (e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e));

interface Img {
  id: number;
  url: string;
  size: number;
  width: number;
  height: number;
  markdown: string;
  bbcode: string;
  username?: string | null;
}
interface Listing {
  items: Img[];
  total: number;
  page: number;
  page_size: number;
  quota?: { count: number; bytes: number; daily_count: number; daily_bytes: number; max_bytes: number };
}

const uploadOne = (api: PluginApi, f: File) => api.json<Img>('/api/p/attachments/v1/images', { method: 'POST', body: f, headers: { 'content-type': f.type || 'application/octet-stream' } });

function copy(api: PluginApi, text: string) {
  void navigator.clipboard
    ?.writeText(text)
    .then(() => api.toast(api.t('attachments.ui.copied'), 'success'))
    .catch(() => window.prompt('', text));
}

function Grid({ api, items, onDelete, showUser }: { api: PluginApi; items: Img[]; onDelete(id: number): void; showUser?: boolean }) {
  if (items.length === 0) return <p className="pt-muted">{api.t('attachments.ui.empty')}</p>;
  return (
    <div className="attachments-grid">
      {items.map((x) => (
        <figure key={x.id} className="attachments-item">
          <a href={x.url} target="_blank" rel="noreferrer">
            <img src={x.url} alt="" loading="lazy" />
          </a>
          <figcaption className="small">
            <span className="pt-muted">
              {x.width}×{x.height} · {formatBytes(x.size)}
              {showUser && x.username ? ` · ${x.username}` : ''}
            </span>
            <span>
              <Button variant="ghost" onClick={() => copy(api, x.bbcode)}>
                {api.t('attachments.ui.copy_bbcode')}
              </Button>
              <Button variant="ghost" onClick={() => copy(api, x.markdown)}>
                {api.t('attachments.ui.copy_markdown')}
              </Button>
              <Button
                variant="ghost"
                onClick={() => {
                  if (window.confirm(api.t('attachments.ui.delete_confirm'))) onDelete(x.id);
                }}
              >
                {api.t('attachments.ui.delete')}
              </Button>
            </span>
          </figcaption>
        </figure>
      ))}
    </div>
  );
}

function useListing(api: PluginApi, path: `/api/${string}`) {
  const [items, setItems] = useState<Img[]>([]);
  const [d, setD] = useState<Listing | null>(null);
  const load = async (page: number) => {
    const r = await api.json<Listing>(`${path}?page=${page}`);
    setD(r);
    setItems((xs) => (page === 1 ? r.items : [...xs, ...r.items]));
  };
  useEffect(() => {
    void load(1);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path]);
  const remove = async (id: number) => {
    try {
      await api.fetch(`/api/p/attachments/v1/images/${id}`, { method: 'DELETE' });
      await load(1);
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return { items, d, load, remove };
}

function More({ api, d, load }: { api: PluginApi; d: Listing | null; load(p: number): Promise<void> }) {
  if (!d || d.page * d.page_size >= d.total) return null;
  return <Button onClick={() => void load(d.page + 1)}>{api.t('attachments.ui.more')}</Button>;
}

function Picker({ api, label, onFiles, busy }: { api: PluginApi; label: ReactNode; onFiles(fs: File[]): void; busy: boolean }) {
  return (
    <label className="pt-btn attachments-picker">
      <input
        type="file"
        name="image_upload"
        accept="image/png,image/jpeg,image/gif,image/webp"
        multiple
        disabled={busy}
        hidden
        onChange={(e) => {
          const fs = [...(e.target.files ?? [])];
          e.target.value = '';
          if (fs.length) onFiles(fs);
        }}
      />
      {busy ? '…' : label}
    </label>
  );
}

function MyImages({ api }: { api: PluginApi }) {
  const { items, d, load, remove } = useListing(api, '/api/p/attachments/v1/images');
  const [busy, setBusy] = useState(false);
  const upload = async (fs: File[]) => {
    setBusy(true);
    for (const f of fs) {
      try {
        await uploadOne(api, f);
      } catch (e) {
        api.toast(`${f.name}: ${errText(api, e)}`, 'error');
      }
    }
    setBusy(false);
    await load(1);
  };
  const q = d?.quota;
  return (
    <Card title={api.t('attachments.ui.title')} actions={<Picker api={api} label={api.t('attachments.ui.upload')} onFiles={(fs) => void upload(fs)} busy={busy} />}>
      {q && (
        <p className="pt-muted small">
          {api.t('attachments.ui.hint', { size: formatBytes(q.max_bytes) })}
          <br />
          {api.t('attachments.ui.quota', { count: q.count, max: q.daily_count, bytes: formatBytes(q.bytes), maxBytes: formatBytes(q.daily_bytes) })}
        </p>
      )}
      <Grid api={api} items={items} onDelete={(id) => void remove(id)} />
      <More api={api} d={d} load={load} />
    </Card>
  );
}

function Admin({ api }: { api: PluginApi }) {
  const { items, d, load, remove } = useListing(api, '/api/p/attachments/v1/admin/images');
  return (
    <Card title={api.t('attachments.ui.admin')}>
      <Grid api={api} items={items} onDelete={(id) => void remove(id)} showUser />
      <More api={api} d={d} load={load} />
    </Card>
  );
}

type Draft = { description: string };

function InsertImage({ api, ctx }: SlotProps<{ draft: Draft; update?: (p: Partial<Draft>) => void }>) {
  const [busy, setBusy] = useState(false);
  if (!ctx.update) return null;
  const run = async (fs: File[]) => {
    setBusy(true);
    const codes: string[] = [];
    for (const f of fs) {
      try {
        codes.push((await uploadOne(api, f)).bbcode);
      } catch (e) {
        api.toast(`${f.name}: ${errText(api, e)}`, 'error');
      }
    }
    setBusy(false);
    if (codes.length) {
      const d = ctx.draft.description.trim();
      ctx.update?.({ description: d ? `${d}\n${codes.join('\n')}` : codes.join('\n') });
      api.toast(api.t('attachments.ui.inserted'), 'success');
    }
  };
  return (
    <Field label="">
      <Picker api={api} label={api.t('attachments.ui.insert')} onFiles={(fs) => void run(fs)} busy={busy} />
    </Field>
  );
}

function MenuItem({ api }: SlotProps) {
  return (
    <a
      href="/p/attachments/"
      onClick={(e) => {
        e.preventDefault();
        api.navigate('/p/attachments/');
      }}
    >
      {api.t('attachments.ui.menu')}
    </a>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  return path.startsWith('/admin/') ? <Admin api={api} /> : <MyImages api={api} />;
}

export default {
  slots: {
    'torrent.upload.extra': { component: InsertImage, order: 30 },
    'nav.user_menu': { component: MenuItem, order: 25 },
  },
  routes: [
    { path: '/p/attachments/*', component: Pages },
    { path: '/admin/p/attachments/*', component: Pages },
  ],
} satisfies PluginFrontend;
