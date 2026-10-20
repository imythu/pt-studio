// attachments 前端：我的图片（上传、复制 BBCode / Markdown、删除）、后台最近上传、发布页“插入图片”到简介。
import { useEffect, useState, type ReactNode } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { formatBytes, registerMessages } from '@pt-studio/terms';
import { Button, CopyButton, Empty, Kpis, LoadMore, NavItem, PageHead, Panel, Sheet, SkeletonRows } from '@pt-studio/ui-kit';

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
  'attachments.ui.today_count': '今日张数',
  'attachments.ui.today_bytes': '今日容量',
  'attachments.ui.total': '全部图片',
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
  'attachments.ui.today_count': 'Today (images)',
  'attachments.ui.today_bytes': 'Today (size)',
  'attachments.ui.total': 'All images',
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

function Gallery({ api, items, onDelete, showUser }: { api: PluginApi; items: Img[]; onDelete(img: Img): void; showUser?: boolean }) {
  if (items.length === 0) return <Empty compact>{api.t('attachments.ui.empty')}</Empty>;
  return (
    <div className="gallery">
      {items.map((x) => (
        <figure key={x.id}>
          <a href={x.url} target="_blank" rel="noreferrer">
            <img src={x.url} alt="" loading="lazy" />
          </a>
          <figcaption>
            <span className="num">
              {x.width}×{x.height} · {formatBytes(x.size)}
              {showUser && x.username ? ` · ${x.username}` : ''}
            </span>
            <span className="row">
              <CopyButton size="xs" text={x.bbcode} label="BBCode" copiedLabel={api.t('attachments.ui.copied')} />
              <CopyButton size="xs" text={x.markdown} label="Markdown" copiedLabel={api.t('attachments.ui.copied')} />
              <Button size="xs" variant="ghost" className="push-right" onClick={() => onDelete(x)}>
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
  const [items, setItems] = useState<Img[] | null>(null);
  const [d, setD] = useState<Listing | null>(null);
  const load = async (page: number) => {
    const r = await api.json<Listing>(`${path}?page=${page}`);
    setD(r);
    setItems((xs) => (page === 1 ? r.items : [...(xs ?? []), ...r.items]));
  };
  useEffect(() => {
    void load(1).catch(() => setItems([]));
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
  return <LoadMore onClick={() => void load(d.page + 1)} label={api.t('attachments.ui.more')} />;
}

function Picker({ label, onFiles, busy, variant }: { label: ReactNode; onFiles(fs: File[]): void; busy: boolean; variant?: 'primary' }) {
  return (
    <label className={variant === 'primary' ? 'btn btn-primary' : 'btn btn-sm'}>
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

/** 删除确认抽屉 */
function DeleteSheet({ api, img, onClose, onConfirm }: { api: PluginApi; img: Img | null; onClose(): void; onConfirm(id: number): void }) {
  return (
    <Sheet
      open={img !== null}
      onClose={onClose}
      title={api.t('attachments.ui.delete')}
      footer={
        <Button variant="danger" onClick={() => img && onConfirm(img.id)}>
          {api.t('attachments.ui.delete')}
        </Button>
      }
    >
      <div className="stack">
        {img && <img src={img.url} alt="" className="sheet-img" />}
        <p>{api.t('attachments.ui.delete_confirm')}</p>
      </div>
    </Sheet>
  );
}

function ImagesPage({ api, admin }: { api: PluginApi; admin?: boolean }) {
  const { items, d, load, remove } = useListing(api, admin ? '/api/p/attachments/v1/admin/images' : '/api/p/attachments/v1/images');
  const [busy, setBusy] = useState(false);
  const [deleting, setDeleting] = useState<Img | null>(null);
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
    <div className="stack">
      <PageHead
        title={api.t(admin ? 'attachments.ui.admin' : 'attachments.ui.title')}
        lede={q ? api.t('attachments.ui.hint', { size: formatBytes(q.max_bytes) }) : undefined}
        actions={!admin && <Picker variant="primary" label={api.t('attachments.ui.upload')} onFiles={(fs) => void upload(fs)} busy={busy} />}
      />
      {q && (
        <Kpis
          items={[
            { label: api.t('attachments.ui.today_count'), value: `${q.count} / ${q.daily_count}` },
            { label: api.t('attachments.ui.today_bytes'), value: `${formatBytes(q.bytes)} / ${formatBytes(q.daily_bytes)}` },
            { label: api.t('attachments.ui.total'), value: d?.total ?? '…' },
          ]}
        />
      )}
      <Panel primary>{items === null ? <SkeletonRows n={4} /> : <Gallery api={api} items={items} onDelete={setDeleting} showUser={admin} />}</Panel>
      <More api={api} d={d} load={load} />
      <DeleteSheet
        api={api}
        img={deleting}
        onClose={() => setDeleting(null)}
        onConfirm={(id) => {
          setDeleting(null);
          void remove(id);
        }}
      />
    </div>
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
    <div className="form-acts">
      <Picker label={api.t('attachments.ui.insert')} onFiles={(fs) => void run(fs)} busy={busy} />
    </div>
  );
}

function MenuItem({ api }: SlotProps) {
  return (
    <NavItem to="/p/attachments/" navigate={api.navigate}>
      {api.t('attachments.ui.menu')}
    </NavItem>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  return <ImagesPage key={path.startsWith('/admin/') ? 'a' : 'm'} api={api} admin={path.startsWith('/admin/')} />;
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
