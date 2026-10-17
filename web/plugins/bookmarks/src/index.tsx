// bookmarks 前端：种子页收藏按钮与“加入合集”、收录它的合集；我的收藏页；合集浏览 / 详情 / 编辑。
import { useEffect, useState, type ReactNode } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { formatBytes, registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Field, Input, Table } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'bookmarks.ui.add': '收藏',
  'bookmarks.ui.remove': '取消收藏',
  'bookmarks.ui.count': '{n} 人收藏',
  'bookmarks.ui.mine': '我的收藏',
  'bookmarks.ui.empty': '还没有收藏',
  'bookmarks.ui.collections': '合集',
  'bookmarks.ui.public': '公开合集',
  'bookmarks.ui.my_collections': '我的合集',
  'bookmarks.ui.add_to': '加入合集…',
  'bookmarks.ui.added': '已加入合集',
  'bookmarks.ui.in_collections': '收录于合集',
  'bookmarks.ui.new': '新建合集',
  'bookmarks.ui.title': '标题',
  'bookmarks.ui.description': '说明',
  'bookmarks.ui.is_public': '公开',
  'bookmarks.ui.private': '私有',
  'bookmarks.ui.create': '创建',
  'bookmarks.ui.save': '保存',
  'bookmarks.ui.edit': '编辑',
  'bookmarks.ui.delete': '删除合集',
  'bookmarks.ui.delete_confirm': '确定删除这个合集？',
  'bookmarks.ui.items': '{n} 个种子',
  'bookmarks.ui.by': '{name} 整理',
  'bookmarks.ui.note': '备注',
  'bookmarks.ui.up': '上移',
  'bookmarks.ui.down': '下移',
  'bookmarks.ui.remove_item': '移出',
  'bookmarks.ui.search': '搜索合集',
  'bookmarks.ui.no_collections': '没有合集',
  'bookmarks.ui.prev': '上一页',
  'bookmarks.ui.next': '下一页',
  'bookmarks.limit_reached': '已达到数量上限',
  'bookmarks.collection_full': '合集已满',
});
registerMessages('en', {
  'bookmarks.ui.add': 'Bookmark',
  'bookmarks.ui.remove': 'Remove bookmark',
  'bookmarks.ui.count': '{n} bookmarks',
  'bookmarks.ui.mine': 'My bookmarks',
  'bookmarks.ui.empty': 'No bookmarks yet',
  'bookmarks.ui.collections': 'Collections',
  'bookmarks.ui.public': 'Public collections',
  'bookmarks.ui.my_collections': 'My collections',
  'bookmarks.ui.add_to': 'Add to collection…',
  'bookmarks.ui.added': 'Added to the collection',
  'bookmarks.ui.in_collections': 'In collections',
  'bookmarks.ui.new': 'New collection',
  'bookmarks.ui.title': 'Title',
  'bookmarks.ui.description': 'Description',
  'bookmarks.ui.is_public': 'Public',
  'bookmarks.ui.private': 'Private',
  'bookmarks.ui.create': 'Create',
  'bookmarks.ui.save': 'Save',
  'bookmarks.ui.edit': 'Edit',
  'bookmarks.ui.delete': 'Delete collection',
  'bookmarks.ui.delete_confirm': 'Delete this collection?',
  'bookmarks.ui.items': '{n} torrents',
  'bookmarks.ui.by': 'by {name}',
  'bookmarks.ui.note': 'Note',
  'bookmarks.ui.up': 'Up',
  'bookmarks.ui.down': 'Down',
  'bookmarks.ui.remove_item': 'Remove',
  'bookmarks.ui.search': 'Search collections',
  'bookmarks.ui.no_collections': 'No collections',
  'bookmarks.ui.prev': 'Previous',
  'bookmarks.ui.next': 'Next',
  'bookmarks.limit_reached': 'You have reached the limit',
  'bookmarks.collection_full': 'This collection is full',
});

const errText = (api: PluginApi, e: unknown) => (e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e));
const BASE = '/api/p/bookmarks/v1';

function A({ api, to, children }: { api: PluginApi; to: string; children: ReactNode }) {
  return (
    <a
      href={to}
      onClick={(e) => {
        e.preventDefault();
        api.navigate(to);
      }}
    >
      {children}
    </a>
  );
}

function Pager({ api, page, total, size, go }: { api: PluginApi; page: number; total: number; size: number; go(p: number): void }) {
  if (total <= size) return null;
  return (
    <p>
      <Button disabled={page <= 1} onClick={() => go(page - 1)}>
        {api.t('bookmarks.ui.prev')}
      </Button>{' '}
      <span className="pt-muted small">
        {page} / {Math.ceil(total / size)}
      </span>{' '}
      <Button disabled={page * size >= total} onClick={() => go(page + 1)}>
        {api.t('bookmarks.ui.next')}
      </Button>
    </p>
  );
}

// ---------------- 种子页 ----------------

interface TorrentInfo {
  bookmarks: number;
  bookmarked: boolean;
  collections: { id: number; title: string; item_count: number; owner: string | null }[];
  my_collections: { id: number; title: string; contains: boolean }[];
}

function useTorrentInfo(api: PluginApi, tid: number) {
  const [s, setS] = useState<TorrentInfo | null>(null);
  const load = () =>
    api
      .json<TorrentInfo>(`${BASE}/torrents/${tid}`)
      .then(setS)
      .catch(() => undefined);
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tid]);
  return [s, load] as const;
}

function Actions({ api, ctx }: SlotProps<{ torrent: { id: number } }>) {
  const tid = ctx.torrent.id;
  const [s, load] = useTorrentInfo(api, tid);
  if (!s) return null;
  const run = async (f: () => Promise<unknown>, ok?: string) => {
    try {
      await f();
      if (ok) api.toast(ok, 'success');
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const candidates = s.my_collections.filter((c) => !c.contains);
  return (
    <span className="bookmarks-action">
      <Button
        variant={s.bookmarked ? 'ghost' : 'default'}
        aria-pressed={s.bookmarked}
        onClick={() => void run(() => api.fetch(`${BASE}/bookmarks/${tid}`, { method: s.bookmarked ? 'DELETE' : 'PUT' }))}
      >
        {s.bookmarked ? '★ ' : '☆ '}
        {api.t(s.bookmarked ? 'bookmarks.ui.remove' : 'bookmarks.ui.add')}
      </Button>{' '}
      <span className="pt-muted small">{api.t('bookmarks.ui.count', { n: s.bookmarks })}</span>{' '}
      {candidates.length > 0 && (
        <select
          className="pt-input"
          name="add_to_collection"
          aria-label={api.t('bookmarks.ui.add_to')}
          value=""
          onChange={(e) => {
            const id = e.target.value;
            if (id) void run(() => api.json(`${BASE}/collections/${id}/items`, { json: { torrent_id: tid } }), api.t('bookmarks.ui.added'));
          }}
        >
          <option value="">{api.t('bookmarks.ui.add_to')}</option>
          {candidates.map((c) => (
            <option key={c.id} value={c.id}>
              {c.title}
            </option>
          ))}
        </select>
      )}
    </span>
  );
}

function InCollections({ api, ctx }: SlotProps<{ torrent: { id: number } }>) {
  const [s] = useTorrentInfo(api, ctx.torrent.id);
  if (!s || s.collections.length === 0) return null;
  return (
    <Card title={api.t('bookmarks.ui.in_collections')}>
      <ul className="pt-list">
        {s.collections.map((c) => (
          <li key={c.id}>
            <A api={api} to={`/p/bookmarks/collections/${c.id}`}>
              {c.title}
            </A>{' '}
            <span className="pt-muted small">{api.t('bookmarks.ui.items', { n: c.item_count })}</span>
          </li>
        ))}
      </ul>
    </Card>
  );
}

// ---------------- 我的收藏 ----------------

interface Paged<T> {
  items: T[];
  total: number;
  page: number;
  page_size: number;
}
interface BookmarkRow {
  torrent_id: number;
  title: string;
  size: number;
  bookmarked_at: number;
}

function MyBookmarks({ api }: { api: PluginApi }) {
  const [page, setPage] = useState(1);
  const [d, setD] = useState<Paged<BookmarkRow> | null>(null);
  const load = () => api.json<Paged<BookmarkRow>>(`${BASE}/bookmarks?page=${page}`).then(setD);
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [page]);
  if (!d) return null;
  return (
    <Card title={`${api.t('bookmarks.ui.mine')}（${d.total}）`} actions={<A api={api} to="/p/bookmarks/collections">{api.t('bookmarks.ui.collections')}</A>}>
      <Table
        rows={d.items}
        rowKey={(r) => r.torrent_id}
        empty={api.t('bookmarks.ui.empty')}
        columns={[
          {
            key: 't',
            header: api.t('bookmarks.ui.title'),
            render: (r) => (
              <A api={api} to={`/torrents/${r.torrent_id}`}>
                {r.title}
              </A>
            ),
          },
          { key: 's', header: '', render: (r) => formatBytes(r.size) },
          { key: 'd', header: '', render: (r) => <span className="pt-muted small">{new Date(r.bookmarked_at).toLocaleDateString()}</span> },
          {
            key: 'x',
            header: '',
            render: (r) => (
              <Button variant="ghost" onClick={() => void api.fetch(`${BASE}/bookmarks/${r.torrent_id}`, { method: 'DELETE' }).then(load)}>
                {api.t('bookmarks.ui.remove')}
              </Button>
            ),
          },
        ]}
      />
      <Pager api={api} page={d.page} total={d.total} size={d.page_size} go={setPage} />
    </Card>
  );
}

// ---------------- 合集 ----------------

interface CollectionRow {
  id: number;
  title: string;
  description: string;
  is_public: boolean;
  item_count: number;
  owner: string | null;
  updated_at: number;
}

function CollectionList({ api }: { api: PluginApi }) {
  const [mine, setMine] = useState(false);
  const [q, setQ] = useState('');
  const [page, setPage] = useState(1);
  const [d, setD] = useState<Paged<CollectionRow> | null>(null);
  const [f, setF] = useState({ title: '', description: '', is_public: true });
  const load = () =>
    api.json<Paged<CollectionRow>>(`${BASE}/collections?page=${page}${mine ? '&mine=1' : ''}${q.trim() && !mine ? `&q=${encodeURIComponent(q.trim())}` : ''}`).then(setD);
  useEffect(() => {
    const h = setTimeout(() => void load(), 250);
    return () => clearTimeout(h);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mine, q, page]);
  const create = async () => {
    try {
      const r = await api.json<{ id: number }>(`${BASE}/collections`, { json: f });
      api.navigate(`/p/bookmarks/collections/${r.id}`);
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <div className="pt-stack">
      <Card
        title={api.t('bookmarks.ui.collections')}
        actions={
          <span className="segmented" role="group">
            {[false, true].map((m) => (
              <button
                key={String(m)}
                type="button"
                aria-pressed={mine === m}
                onClick={() => {
                  setMine(m);
                  setPage(1);
                }}
              >
                {api.t(m ? 'bookmarks.ui.my_collections' : 'bookmarks.ui.public')}
              </button>
            ))}
          </span>
        }
      >
        {!mine && <Input name="collection_q" type="search" placeholder={api.t('bookmarks.ui.search')} value={q} onChange={(e) => setQ(e.target.value)} />}
        {d && (
          <Table
            rows={d.items}
            rowKey={(r) => r.id}
            empty={api.t('bookmarks.ui.no_collections')}
            columns={[
              {
                key: 't',
                header: api.t('bookmarks.ui.title'),
                render: (r) => (
                  <>
                    <A api={api} to={`/p/bookmarks/collections/${r.id}`}>
                      {r.title}
                    </A>{' '}
                    {!r.is_public && <Badge>{api.t('bookmarks.ui.private')}</Badge>}
                  </>
                ),
              },
              { key: 'n', header: '', render: (r) => api.t('bookmarks.ui.items', { n: r.item_count }) },
              { key: 'o', header: '', render: (r) => <span className="pt-muted small">{api.t('bookmarks.ui.by', { name: r.owner ?? '?' })}</span> },
            ]}
          />
        )}
        {d && <Pager api={api} page={d.page} total={d.total} size={d.page_size} go={setPage} />}
      </Card>
      <Card title={api.t('bookmarks.ui.new')}>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void create();
          }}
        >
          <Field label={api.t('bookmarks.ui.title')}>
            <Input name="collection_title" value={f.title} maxLength={100} required onChange={(e) => setF({ ...f, title: e.target.value })} />
          </Field>
          <Field label={api.t('bookmarks.ui.description')}>
            <textarea className="pt-input" name="collection_description" rows={3} maxLength={5000} value={f.description} onChange={(e) => setF({ ...f, description: e.target.value })} />
          </Field>
          <label className="pt-check">
            <input type="checkbox" checked={f.is_public} onChange={(e) => setF({ ...f, is_public: e.target.checked })} /> {api.t('bookmarks.ui.is_public')}
          </label>
          <Button type="submit" variant="primary" disabled={!f.title.trim()}>
            {api.t('bookmarks.ui.create')}
          </Button>
        </form>
      </Card>
    </div>
  );
}

interface CollectionDetail extends CollectionRow {
  editable: boolean;
  items: { torrent_id: number; title: string; size: number; note: string }[];
}

function CollectionPage({ api, id }: { api: PluginApi; id: number }) {
  const [d, setD] = useState<CollectionDetail | null>(null);
  const [edit, setEdit] = useState<{ title: string; description: string; is_public: boolean } | null>(null);
  const load = () =>
    api
      .json<CollectionDetail>(`${BASE}/collections/${id}`)
      .then(setD)
      .catch((e) => api.toast(errText(api, e), 'error'));
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);
  if (!d) return null;
  const run = async (f: () => Promise<unknown>) => {
    try {
      await f();
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const move = (i: number, by: number) => {
    const ids = d.items.map((x) => x.torrent_id);
    const j = i + by;
    if (j < 0 || j >= ids.length) return;
    [ids[i], ids[j]] = [ids[j] as number, ids[i] as number];
    void run(() => api.json(`${BASE}/collections/${id}/order`, { method: 'PUT', json: { torrent_ids: ids } }));
  };
  return (
    <div className="pt-stack">
      <Card
        title={d.title}
        actions={
          d.editable && !edit ? (
            <Button variant="ghost" onClick={() => setEdit({ title: d.title, description: d.description, is_public: d.is_public })}>
              {api.t('bookmarks.ui.edit')}
            </Button>
          ) : null
        }
      >
        {edit ? (
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void run(async () => {
                await api.json(`${BASE}/collections/${id}`, { method: 'PATCH', json: edit });
                setEdit(null);
              });
            }}
          >
            <Field label={api.t('bookmarks.ui.title')}>
              <Input name="edit_collection_title" value={edit.title} maxLength={100} required onChange={(e) => setEdit({ ...edit, title: e.target.value })} />
            </Field>
            <Field label={api.t('bookmarks.ui.description')}>
              <textarea className="pt-input" rows={3} maxLength={5000} value={edit.description} onChange={(e) => setEdit({ ...edit, description: e.target.value })} />
            </Field>
            <label className="pt-check">
              <input type="checkbox" checked={edit.is_public} onChange={(e) => setEdit({ ...edit, is_public: e.target.checked })} /> {api.t('bookmarks.ui.is_public')}
            </label>
            <Button type="submit" variant="primary">
              {api.t('bookmarks.ui.save')}
            </Button>{' '}
            <Button
              variant="danger"
              onClick={() => {
                if (!window.confirm(api.t('bookmarks.ui.delete_confirm'))) return;
                void api
                  .fetch(`${BASE}/collections/${id}`, { method: 'DELETE' })
                  .then(() => api.navigate('/p/bookmarks/collections'))
                  .catch((e) => api.toast(errText(api, e), 'error'));
              }}
            >
              {api.t('bookmarks.ui.delete')}
            </Button>
          </form>
        ) : (
          <>
            <p className="pt-muted small">
              {api.t('bookmarks.ui.by', { name: d.owner ?? '?' })} · {api.t('bookmarks.ui.items', { n: d.item_count })}
              {!d.is_public && <> · {api.t('bookmarks.ui.private')}</>}
            </p>
            {d.description && <p style={{ whiteSpace: 'pre-wrap' }}>{d.description}</p>}
          </>
        )}
      </Card>
      <Card>
        <Table
          rows={d.items}
          rowKey={(r) => r.torrent_id}
          empty={api.t('bookmarks.ui.empty')}
          columns={[
            {
              key: 't',
              header: api.t('bookmarks.ui.title'),
              render: (r) => (
                <>
                  <A api={api} to={`/torrents/${r.torrent_id}`}>
                    {r.title}
                  </A>
                  {r.note && <div className="pt-muted small">{r.note}</div>}
                </>
              ),
            },
            { key: 's', header: '', render: (r) => formatBytes(r.size) },
            ...(d.editable
              ? [
                  {
                    key: 'e',
                    header: '',
                    render: (r: CollectionDetail['items'][number]) => {
                      const i = d.items.indexOf(r);
                      return (
                        <span>
                          <Button variant="ghost" aria-label={api.t('bookmarks.ui.up')} disabled={i === 0} onClick={() => move(i, -1)}>
                            ↑
                          </Button>
                          <Button variant="ghost" aria-label={api.t('bookmarks.ui.down')} disabled={i === d.items.length - 1} onClick={() => move(i, 1)}>
                            ↓
                          </Button>
                          <Button
                            variant="ghost"
                            onClick={() => {
                              const note = window.prompt(api.t('bookmarks.ui.note'), r.note);
                              if (note !== null) void run(() => api.json(`${BASE}/collections/${id}/items`, { json: { torrent_id: r.torrent_id, note } }));
                            }}
                          >
                            {api.t('bookmarks.ui.note')}
                          </Button>
                          <Button variant="ghost" onClick={() => void run(() => api.fetch(`${BASE}/collections/${id}/items/${r.torrent_id}`, { method: 'DELETE' }))}>
                            {api.t('bookmarks.ui.remove_item')}
                          </Button>
                        </span>
                      );
                    },
                  },
                ]
              : []),
          ]}
        />
      </Card>
    </div>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  const c = /^\/p\/bookmarks\/collections\/(\d+)/.exec(path);
  if (c?.[1]) return <CollectionPage api={api} id={Number(c[1])} />;
  if (path.startsWith('/p/bookmarks/collections')) return <CollectionList api={api} />;
  return <MyBookmarks api={api} />;
}

function MenuItem({ api }: SlotProps) {
  return (
    <A api={api} to="/p/bookmarks/">
      {api.t('bookmarks.ui.mine')}
    </A>
  );
}

function NavItem({ api }: SlotProps) {
  return (
    <A api={api} to="/p/bookmarks/collections">
      {api.t('bookmarks.ui.collections')}
    </A>
  );
}

export default {
  slots: {
    'torrent.detail.actions': { component: Actions, order: 10 },
    'torrent.detail.sidebar': { component: InCollections, order: 40 },
    'nav.user_menu': { component: MenuItem, order: 20 },
    'nav.main': { component: NavItem, order: 30 },
  },
  routes: [{ path: '/p/bookmarks/*', component: Pages }],
} satisfies PluginFrontend;
