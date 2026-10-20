// bookmarks 前端：种子页「收藏」与「加入合集」（抽屉）、侧栏收录它的合集；我的收藏；合集（卡片列表、详情、排序与备注；新建 / 编辑在抽屉里）。
import { useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { formatBytes, registerMessages } from '@pt-studio/terms';
import { Button, Checkbox, Empty, Field, Input, NavItem, PageHead, Pager, Panel, PLink, Segmented, Sheet, SkeletonRows, Table, Tag, Textarea } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'bookmarks.ui.add': '收藏',
  'bookmarks.ui.remove': '取消收藏',
  'bookmarks.ui.bookmarked': '已收藏',
  'bookmarks.ui.count': '{n} 人收藏',
  'bookmarks.ui.mine': '我的收藏',
  'bookmarks.ui.mine_lede': '共 {n} 个种子',
  'bookmarks.ui.empty': '还没有收藏',
  'bookmarks.ui.empty_hint': '在种子页点「收藏」，以后在这里找到它。',
  'bookmarks.ui.collections': '合集',
  'bookmarks.ui.collections_lede': '旅人们整理的片单。设为公开后所有人可见。',
  'bookmarks.ui.public': '公开合集',
  'bookmarks.ui.my_collections': '我的合集',
  'bookmarks.ui.add_to': '加入合集',
  'bookmarks.ui.add_to_hint': '勾选要收录这个种子的合集。',
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
  'bookmarks.ui.edit_title': '编辑合集',
  'bookmarks.ui.delete': '删除合集',
  'bookmarks.ui.delete_confirm': '删除后不能恢复，合集里的种子不受影响。',
  'bookmarks.ui.items': '{n} 个种子',
  'bookmarks.ui.by': '{name} 整理',
  'bookmarks.ui.note': '备注',
  'bookmarks.ui.up': '上移',
  'bookmarks.ui.down': '下移',
  'bookmarks.ui.remove_item': '移出',
  'bookmarks.ui.search': '搜索合集',
  'bookmarks.ui.no_collections': '没有合集',
  'bookmarks.ui.bookmarked_at': '收藏于',
  'bookmarks.ui.size': '体积',
  'bookmarks.ui.prev': '上一页',
  'bookmarks.ui.next': '下一页',
  'bookmarks.limit_reached': '已达到数量上限',
  'bookmarks.collection_full': '合集已满',
});
registerMessages('en', {
  'bookmarks.ui.add': 'Bookmark',
  'bookmarks.ui.remove': 'Remove bookmark',
  'bookmarks.ui.bookmarked': 'Bookmarked',
  'bookmarks.ui.count': '{n} bookmarks',
  'bookmarks.ui.mine': 'My bookmarks',
  'bookmarks.ui.mine_lede': '{n} torrents',
  'bookmarks.ui.empty': 'No bookmarks yet',
  'bookmarks.ui.empty_hint': 'Click “Bookmark” on a torrent to find it here later.',
  'bookmarks.ui.collections': 'Collections',
  'bookmarks.ui.collections_lede': 'Lists curated by members. Public collections are visible to everyone.',
  'bookmarks.ui.public': 'Public collections',
  'bookmarks.ui.my_collections': 'My collections',
  'bookmarks.ui.add_to': 'Add to collection',
  'bookmarks.ui.add_to_hint': 'Tick the collections that should include this torrent.',
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
  'bookmarks.ui.edit_title': 'Edit collection',
  'bookmarks.ui.delete': 'Delete collection',
  'bookmarks.ui.delete_confirm': 'This cannot be undone. The torrents themselves are not affected.',
  'bookmarks.ui.items': '{n} torrents',
  'bookmarks.ui.by': 'by {name}',
  'bookmarks.ui.note': 'Note',
  'bookmarks.ui.up': 'Up',
  'bookmarks.ui.down': 'Down',
  'bookmarks.ui.remove_item': 'Remove',
  'bookmarks.ui.search': 'Search collections',
  'bookmarks.ui.no_collections': 'No collections',
  'bookmarks.ui.bookmarked_at': 'Bookmarked',
  'bookmarks.ui.size': 'Size',
  'bookmarks.ui.prev': 'Previous',
  'bookmarks.ui.next': 'Next',
  'bookmarks.limit_reached': 'You have reached the limit',
  'bookmarks.collection_full': 'This collection is full',
});

const errText = (api: PluginApi, e: unknown) => (e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e));
const BASE = '/api/p/bookmarks/v1';

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
  const [open, setOpen] = useState(false);
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
  return (
    <>
      <Button aria-pressed={s.bookmarked} title={api.t('bookmarks.ui.count', { n: s.bookmarks })} onClick={() => void run(() => api.fetch(`${BASE}/bookmarks/${tid}`, { method: s.bookmarked ? 'DELETE' : 'PUT' }))}>
        {api.t(s.bookmarked ? 'bookmarks.ui.bookmarked' : 'bookmarks.ui.add')}
        <span className="btn-sub">{s.bookmarks}</span>
      </Button>
      {s.my_collections.length > 0 && <Button onClick={() => setOpen(true)}>{api.t('bookmarks.ui.add_to')}</Button>}
      <Sheet open={open} onClose={() => setOpen(false)} title={api.t('bookmarks.ui.add_to')}>
        <p className="small muted">{api.t('bookmarks.ui.add_to_hint')}</p>
        <div className="stack-s">
          {s.my_collections.map((c) => (
            <Checkbox
              key={c.id}
              label={c.title}
              checked={c.contains}
              onChange={(on) =>
                void run(
                  () => (on ? api.json(`${BASE}/collections/${c.id}/items`, { json: { torrent_id: tid } }) : api.fetch(`${BASE}/collections/${c.id}/items/${tid}`, { method: 'DELETE' })),
                  on ? api.t('bookmarks.ui.added') : undefined,
                )
              }
            />
          ))}
        </div>
        <PLink to="/p/bookmarks/collections?new=1" navigate={api.navigate} className="link small">
          + {api.t('bookmarks.ui.new')}
        </PLink>
      </Sheet>
    </>
  );
}

function InCollections({ api, ctx }: SlotProps<{ torrent: { id: number } }>) {
  const [s] = useTorrentInfo(api, ctx.torrent.id);
  if (!s || s.collections.length === 0) return null;
  return (
    <Panel title={api.t('bookmarks.ui.in_collections')} flush>
      <div className="side-list plain">
        {s.collections.map((c) => (
          <PLink key={c.id} to={`/p/bookmarks/collections/${c.id}`} navigate={api.navigate}>
            <span className="nm">{c.title}</span>
            <span className="v">{c.item_count}</span>
          </PLink>
        ))}
      </div>
    </Panel>
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
  return (
    <div className="stack">
      <PageHead
        title={api.t('bookmarks.ui.mine')}
        lede={d ? api.t('bookmarks.ui.mine_lede', { n: d.total }) : undefined}
        actions={
          <PLink to="/p/bookmarks/collections" navigate={api.navigate} className="btn btn-sm">
            {api.t('bookmarks.ui.collections')}
          </PLink>
        }
      />
      <Panel flush primary>
        {!d ? (
          <SkeletonRows n={6} />
        ) : d.items.length === 0 ? (
          <Empty hint={api.t('bookmarks.ui.empty_hint')}>{api.t('bookmarks.ui.empty')}</Empty>
        ) : (
          <Table
            rows={d.items}
            rowKey={(r) => r.torrent_id}
            minWidth={640}
            columns={[
              { key: 't', header: api.t('bookmarks.ui.title'), render: (r) => <PLink to={`/torrents/${r.torrent_id}`} navigate={api.navigate} className="tt-name">{r.title}</PLink> },
              { key: 's', header: api.t('bookmarks.ui.size'), right: true, render: (r) => <span className="num">{formatBytes(r.size)}</span> },
              { key: 'd', header: api.t('bookmarks.ui.bookmarked_at'), render: (r) => <span className="small muted nowrap">{new Date(r.bookmarked_at).toLocaleDateString()}</span> },
              {
                key: 'x',
                header: '',
                render: (r) => (
                  <div className="acts">
                    <Button size="xs" variant="ghost" onClick={() => void api.fetch(`${BASE}/bookmarks/${r.torrent_id}`, { method: 'DELETE' }).then(load)}>
                      {api.t('bookmarks.ui.remove')}
                    </Button>
                  </div>
                ),
              },
            ]}
          />
        )}
        {d && <Pager page={d.page} pages={Math.ceil(d.total / d.page_size)} onChange={setPage} prev={api.t('bookmarks.ui.prev')} next={api.t('bookmarks.ui.next')} />}
      </Panel>
    </div>
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

type Draft = { title: string; description: string; is_public: boolean };

function CollectionForm({ api, f, setF }: { api: PluginApi; f: Draft; setF(d: Draft): void }) {
  return (
    <>
      <Field label={api.t('bookmarks.ui.title')}>
        <Input name="collection_title" value={f.title} maxLength={100} required onChange={(e) => setF({ ...f, title: e.target.value })} />
      </Field>
      <Field label={api.t('bookmarks.ui.description')}>
        <Textarea name="collection_description" rows={4} maxLength={5000} value={f.description} onChange={(e) => setF({ ...f, description: e.target.value })} />
      </Field>
      <Checkbox label={api.t('bookmarks.ui.is_public')} checked={f.is_public} onChange={(v) => setF({ ...f, is_public: v })} />
    </>
  );
}

function CollectionList({ api }: { api: PluginApi }) {
  const [mine, setMine] = useState<'public' | 'mine'>('public');
  const [q, setQ] = useState('');
  const [page, setPage] = useState(1);
  const [d, setD] = useState<Paged<CollectionRow> | null>(null);
  const [creating, setCreating] = useState(() => new URLSearchParams(window.location.search).has('new'));
  const [f, setF] = useState<Draft>({ title: '', description: '', is_public: true });
  const load = () => api.json<Paged<CollectionRow>>(`${BASE}/collections?page=${page}${mine === 'mine' ? '&mine=1' : ''}${q.trim() && mine === 'public' ? `&q=${encodeURIComponent(q.trim())}` : ''}`).then(setD);
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
    <div className="stack">
      <PageHead
        title={api.t('bookmarks.ui.collections')}
        lede={api.t('bookmarks.ui.collections_lede')}
        actions={
          <Button size="sm" variant="primary" onClick={() => setCreating(true)}>
            {api.t('bookmarks.ui.new')}
          </Button>
        }
      />
      <div className="row">
        <Segmented
          value={mine}
          onChange={(v) => {
            setMine(v);
            setPage(1);
          }}
          options={[
            { value: 'public', label: api.t('bookmarks.ui.public') },
            { value: 'mine', label: api.t('bookmarks.ui.my_collections') },
          ]}
        />
        {mine === 'public' && <Input name="collection_q" type="search" placeholder={api.t('bookmarks.ui.search')} value={q} onChange={(e) => setQ(e.target.value)} style={{ maxWidth: 320 }} />}
      </div>
      {!d ? (
        <SkeletonRows n={4} />
      ) : d.items.length === 0 ? (
        <Panel primary>
          <Empty>{api.t('bookmarks.ui.no_collections')}</Empty>
        </Panel>
      ) : (
        <div className="colls" data-primary>
          {d.items.map((c) => (
            <PLink key={c.id} to={`/p/bookmarks/collections/${c.id}`} navigate={api.navigate} className="coll">
              <h3>{c.title}</h3>
              <span className="small muted">
                {api.t('bookmarks.ui.items', { n: c.item_count })} · {api.t('bookmarks.ui.by', { name: c.owner ?? '?' })} {!c.is_public && <Tag>{api.t('bookmarks.ui.private')}</Tag>}
              </span>
              {c.description && <p className="small coll-d">{c.description}</p>}
            </PLink>
          ))}
        </div>
      )}
      {d && <Pager page={d.page} pages={Math.ceil(d.total / d.page_size)} onChange={setPage} prev={api.t('bookmarks.ui.prev')} next={api.t('bookmarks.ui.next')} />}
      <Sheet
        open={creating}
        onClose={() => setCreating(false)}
        title={api.t('bookmarks.ui.new')}
        footer={
          <Button variant="primary" disabled={!f.title.trim()} onClick={() => void create()}>
            {api.t('bookmarks.ui.create')}
          </Button>
        }
      >
        <CollectionForm api={api} f={f} setF={setF} />
      </Sheet>
    </div>
  );
}

interface CollectionDetail extends CollectionRow {
  editable: boolean;
  items: { torrent_id: number; title: string; size: number; note: string }[];
}

function CollectionPage({ api, id }: { api: PluginApi; id: number }) {
  const [d, setD] = useState<CollectionDetail | null>(null);
  const [edit, setEdit] = useState<Draft | null>(null);
  const [confirmDel, setConfirmDel] = useState(false);
  const [note, setNote] = useState<{ torrent_id: number; note: string } | null>(null);
  const load = () =>
    api
      .json<CollectionDetail>(`${BASE}/collections/${id}`)
      .then(setD)
      .catch((e) => api.toast(errText(api, e), 'error'));
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);
  if (!d) return <SkeletonRows n={6} />;
  const run = async (f: () => Promise<unknown>) => {
    try {
      await f();
      await load();
      return true;
    } catch (e) {
      api.toast(errText(api, e), 'error');
      return false;
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
    <div className="stack">
      <PageHead
        crumb={
          <>
            <PLink to="/p/bookmarks/collections" navigate={api.navigate}>
              {api.t('bookmarks.ui.collections')}
            </PLink>
            <span>{api.t('bookmarks.ui.by', { name: d.owner ?? '?' })}</span>
          </>
        }
        title={d.title}
        lede={
          <>
            {api.t('bookmarks.ui.items', { n: d.item_count })}
            {!d.is_public && <> · {api.t('bookmarks.ui.private')}</>}
            {d.description && <span className="pt-prewrap coll-desc">{d.description}</span>}
          </>
        }
        actions={
          d.editable && (
            <Button size="sm" onClick={() => setEdit({ title: d.title, description: d.description, is_public: d.is_public })}>
              {api.t('bookmarks.ui.edit')}
            </Button>
          )
        }
      />
      <Panel flush primary>
        <Table
          rows={d.items}
          rowKey={(r) => r.torrent_id}
          empty={api.t('bookmarks.ui.empty')}
          minWidth={640}
          columns={[
            { key: 'n', header: '#', render: (r) => <span className="rk-n">{d.items.indexOf(r) + 1}</span> },
            {
              key: 't',
              header: api.t('bookmarks.ui.title'),
              render: (r) => (
                <div className="tt">
                  <PLink to={`/torrents/${r.torrent_id}`} navigate={api.navigate} className="tt-name">
                    {r.title}
                  </PLink>
                  {r.note && <span className="small muted">{r.note}</span>}
                </div>
              ),
            },
            { key: 's', header: api.t('bookmarks.ui.size'), right: true, render: (r) => <span className="num">{formatBytes(r.size)}</span> },
            ...(d.editable
              ? [
                  {
                    key: 'e',
                    header: '',
                    render: (r: CollectionDetail['items'][number]) => {
                      const i = d.items.indexOf(r);
                      return (
                        <div className="acts">
                          <Button size="xs" variant="ghost" aria-label={api.t('bookmarks.ui.up')} disabled={i === 0} onClick={() => move(i, -1)}>
                            {api.t('bookmarks.ui.up')}
                          </Button>
                          <Button size="xs" variant="ghost" aria-label={api.t('bookmarks.ui.down')} disabled={i === d.items.length - 1} onClick={() => move(i, 1)}>
                            {api.t('bookmarks.ui.down')}
                          </Button>
                          <Button size="xs" variant="ghost" onClick={() => setNote({ torrent_id: r.torrent_id, note: r.note })}>
                            {api.t('bookmarks.ui.note')}
                          </Button>
                          <Button size="xs" variant="ghost" onClick={() => void run(() => api.fetch(`${BASE}/collections/${id}/items/${r.torrent_id}`, { method: 'DELETE' }))}>
                            {api.t('bookmarks.ui.remove_item')}
                          </Button>
                        </div>
                      );
                    },
                  },
                ]
              : []),
          ]}
        />
      </Panel>
      <Sheet
        open={!!edit}
        onClose={() => {
          setEdit(null);
          setConfirmDel(false);
        }}
        title={api.t('bookmarks.ui.edit_title')}
        footer={
          <>
            {confirmDel ? (
              <Button
                variant="danger"
                onClick={() =>
                  void api
                    .fetch(`${BASE}/collections/${id}`, { method: 'DELETE' })
                    .then(() => api.navigate('/p/bookmarks/collections'))
                    .catch((e) => api.toast(errText(api, e), 'error'))
                }
              >
                {api.t('bookmarks.ui.delete')}
              </Button>
            ) : (
              <Button variant="ghost" onClick={() => setConfirmDel(true)}>
                {api.t('bookmarks.ui.delete')}
              </Button>
            )}
            <Button
              variant="primary"
              onClick={() =>
                edit &&
                void run(() => api.json(`${BASE}/collections/${id}`, { method: 'PATCH', json: edit })).then((ok) => ok && setEdit(null))
              }
            >
              {api.t('bookmarks.ui.save')}
            </Button>
          </>
        }
      >
        {edit && <CollectionForm api={api} f={edit} setF={setEdit} />}
        {confirmDel && <p className="small down">{api.t('bookmarks.ui.delete_confirm')}</p>}
      </Sheet>
      <Sheet
        open={!!note}
        onClose={() => setNote(null)}
        title={api.t('bookmarks.ui.note')}
        footer={
          <Button
            variant="primary"
            onClick={() =>
              note && void run(() => api.json(`${BASE}/collections/${id}/items`, { json: { torrent_id: note.torrent_id, note: note.note } })).then((ok) => ok && setNote(null))
            }
          >
            {api.t('bookmarks.ui.save')}
          </Button>
        }
      >
        {note && <Textarea rows={4} maxLength={500} value={note.note} onChange={(e) => setNote({ ...note, note: e.target.value })} />}
      </Sheet>
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
    <NavItem to="/p/bookmarks/" navigate={api.navigate}>
      {api.t('bookmarks.ui.mine')}
    </NavItem>
  );
}

function MainNav({ api }: SlotProps) {
  return (
    <NavItem to="/p/bookmarks/collections" navigate={api.navigate}>
      {api.t('bookmarks.ui.collections')}
    </NavItem>
  );
}

export default {
  slots: {
    'torrent.detail.actions': { component: Actions, order: 10 },
    'torrent.detail.sidebar': { component: InCollections, order: 40 },
    'nav.user_menu': { component: MenuItem, order: 20 },
    'nav.main': { component: MainNav, order: 30 },
  },
  routes: [{ path: '/p/bookmarks/*', component: Pages }],
} satisfies PluginFrontend;
