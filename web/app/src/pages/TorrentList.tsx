// 种子列表 / 搜索（search-meilisearch）。筛选条件序列化到 URL，可分享（UI-WEB P1：常用条件一行，高级条件折叠）。
import { useInfiniteQuery, useQuery } from '@tanstack/react-query';
import { useEffect, useState, type FormEvent } from 'react';
import { useSearchParams } from 'react-router';
import { api } from '@pt-studio/sdk';
import { Slot } from '@pt-studio/slots';
import { useT } from '@pt-studio/terms';
import { ActiveFilters, Button, FilterBar, FilterRow, LoadMore, PageHead, Panel, Segmented, Select, SkeletonRows } from '@pt-studio/ui-kit';
import { ErrorText } from '../errors';
import { SuggestInput } from '../suggest';
import { TorrentRows, TorrentWall, type SortKey, type TorrentDoc } from '../torrent';
import { useTypes } from '../types';

interface SearchResult {
  hits: { id: number; document: TorrentDoc; highlights: Record<string, string> }[];
  facets: { field: string; buckets: { value: string; count: number }[] }[];
  estimated_total: number;
  next_cursor: string | null;
}

interface SchemaView {
  fields: { name: string; facetable: boolean; label_term: string; generic: boolean }[];
}

const SORTS: Record<string, { field: string; descending: boolean }[]> = {
  relevance: [],
  // 置顶的排在最前
  newest: [
    { field: 'sticky', descending: true },
    { field: 'created_at', descending: true },
  ],
  largest: [{ field: 'size', descending: true }],
};

const FACETS = ['rel_resolution', 'rel_source', 'rel_video_codec', 'rel_hdr', 'type_id'];
const PAGE = 50;
const ADV_KEY = 'pt-adv-filters';

const typeRank = (id: string) => {
  const i = ['movie', 'tv', 'anime', 'music', 'generic'].indexOf(id);
  return i < 0 ? 4 : i;
};

function useSearch(params: URLSearchParams) {
  const q = params.get('q') ?? '';
  const sort = params.get('sort') ?? (q ? 'relevance' : 'newest');
  const filters = [...params.entries()].filter(([k]) => k.startsWith('f.')).map(([k, v]) => ({ field: k.slice(2), op: 'in', values: v.split(',') }));
  const body = { index: 'torrents', query: q, filters, sort: SORTS[sort] ?? [], facets: FACETS, limit: PAGE };
  return useInfiniteQuery({
    queryKey: ['search', body],
    initialPageParam: null as string | null,
    queryFn: ({ pageParam }) => api<SearchResult>('/api/p/search-meilisearch/v1/search', { json: { ...body, cursor: pageParam ?? undefined } }),
    getNextPageParam: (last) => last.next_cursor,
    staleTime: 30_000,
  });
}

function loadAdv(): boolean {
  try {
    return window.matchMedia('(min-width: 761px)').matches && localStorage.getItem(ADV_KEY) === '1';
  } catch {
    return false;
  }
}

export function TorrentList() {
  const t = useT();
  const [params, setParams] = useSearchParams();
  const [q, setQ] = useState(params.get('q') ?? '');
  useEffect(() => setQ(params.get('q') ?? ''), [params]);
  const res = useSearch(params);
  const types = useTypes();
  const schema = useQuery({
    queryKey: ['search-schema', 'torrents'],
    queryFn: () => api<SchemaView>('/api/p/search-meilisearch/v1/indexes/torrents/schema'),
    staleTime: Infinity,
    retry: false,
  });
  const [advOpen, setAdvOpen] = useState(loadAdv);
  const view = params.get('view') === 'wall' ? 'wall' : 'list';
  const sort = params.get('sort') ?? (params.get('q') ? 'relevance' : 'newest');
  const update = (f: (p: URLSearchParams) => void) => {
    const p = new URLSearchParams(params);
    f(p);
    setParams(p, { replace: true });
  };
  const first = res.data?.pages[0];
  const selected = (field: string) => new Set((params.get(`f.${field}`) ?? '').split(',').filter(Boolean));
  const toggle = (field: string, value: string, single = false) =>
    update((p) => {
      const cur = single ? new Set<string>() : selected(field);
      if (selected(field).has(value)) cur.delete(value);
      else cur.add(value);
      if (cur.size) p.set(`f.${field}`, [...cur].join(','));
      else p.delete(`f.${field}`);
    });
  const label = (name: string) => {
    const f = schema.data?.fields.find((x) => x.name === name);
    const v = f?.label_term ? t(`term:${f.label_term}`) : null;
    return v && v !== f?.label_term ? v : t(`ui.torrent.facet.${name}`);
  };
  const curType = params.get('f.type_id') ?? '';
  const facets = (first?.facets ?? []).filter((f) => f.field !== 'type_id' && f.buckets.length > 0);
  const active = [...params.entries()]
    .filter(([k]) => k.startsWith('f.') && k !== 'f.type_id')
    .flatMap(([k, v]) => v.split(',').filter(Boolean).map((val) => ({ field: k.slice(2), val })));
  const resBuckets = first?.facets.find((f) => f.field === 'rel_resolution')?.buckets.slice(0, 4) ?? [];
  const typeCount = new Map((first?.facets.find((f) => f.field === 'type_id')?.buckets ?? []).map((b) => [b.value, b.count]));

  const submit = (e: FormEvent) => {
    e.preventDefault();
    update((p) => {
      if (q.trim()) p.set('q', q.trim());
      else p.delete('q');
      p.delete('sort');
    });
  };
  const openAdv = (v: boolean) => {
    setAdvOpen(v);
    try {
      if (window.matchMedia('(min-width: 761px)').matches) localStorage.setItem(ADV_KEY, v ? '1' : '0');
    } catch {
      /* 隐私模式 */
    }
  };
  const total = first?.estimated_total ?? 0;
  const hits = res.data?.pages.flatMap((p) => p.hits) ?? [];
  const rows = hits.map((h) => h.document);

  return (
    <div className="torrents">
      <PageHead
        title={t('ui.nav.torrents')}
        lede={first ? (params.get('q') ? t('ui.torrent.count_q', { n: total, q: params.get('q') }) : t('ui.torrent.count', { n: total })) : undefined}
      />
      <form className="page-search" role="search" onSubmit={submit}>
        <SuggestInput className="in" name="q" type="search" value={q} placeholder={t('ui.torrent.search_hint')} aria-label={t('ui.torrent.search')} onValue={setQ} />
        <Button type="submit" variant="primary">
          {t('ui.torrent.search_short')}
        </Button>
      </form>
      <div className="cats" role="group" aria-label={t('ui.torrent.type')}>
        <button type="button" aria-pressed={!curType} onClick={() => update((p) => p.delete('f.type_id'))}>
          {t('ui.torrent.all_types')}
        </button>
        {[...(types.data ?? [])]
          .sort((a, b) => typeRank(a.type_id) - typeRank(b.type_id))
          .map((x) => (
            <button key={x.type_id} type="button" aria-pressed={curType === x.type_id} onClick={() => toggle('type_id', x.type_id, true)}>
              {t(`term:${x.name_term}`)}
              {!curType && typeCount.has(x.type_id) && <em>{typeCount.get(x.type_id)}</em>}
            </button>
          ))}
      </div>
      <Panel flush primary>
        <FilterBar
          open={advOpen}
          onOpenChange={openAdv}
          activeCount={active.length}
          onReset={() =>
            update((p) => {
              for (const [k] of [...p.entries()]) if (k.startsWith('f.') && k !== 'f.type_id') p.delete(k);
            })
          }
          applyLabel={t('ui.torrent.show_n', { n: total })}
          title={t('ui.torrent.filters')}
          quick={resBuckets.map((b) => (
            <button key={b.value} type="button" className="chip" aria-pressed={selected('rel_resolution').has(b.value)} onClick={() => toggle('rel_resolution', b.value)}>
              {b.value}
            </button>
          ))}
          right={
            <>
              <Select
                aria-label={t('ui.torrent.sort')}
                value={sort}
                onChange={(e) => update((p) => p.set('sort', e.target.value))}
                options={Object.keys(SORTS)
                  .filter((k) => k !== 'relevance' || params.get('q'))
                  .map((k) => ({ value: k, label: t(`ui.torrent.sort.${k}`) }))}
              />
              <Segmented
                label={t('ui.torrent.view')}
                value={view}
                onChange={(v) => update((p) => (v === 'wall' ? p.set('view', 'wall') : p.delete('view')))}
                options={[
                  { value: 'list', label: t('ui.torrent.view_list') },
                  { value: 'wall', label: t('ui.torrent.view_wall') },
                ]}
              />
            </>
          }
          active={
            <ActiveFilters
              items={active.map((a) => ({ key: `${a.field}:${a.val}`, label: label(a.field), value: a.val, onRemove: () => toggle(a.field, a.val) }))}
              onClear={() =>
                update((p) => {
                  for (const [k] of [...p.entries()]) if (k.startsWith('f.') && k !== 'f.type_id') p.delete(k);
                })
              }
            />
          }
          advanced={
            <>
              {facets.map((f) => (
                <FilterRow key={f.field} label={label(f.field)}>
                  <div className="chips">
                    {f.buckets.map((b) => (
                      <button key={b.value} type="button" className="chip" aria-pressed={selected(f.field).has(b.value)} onClick={() => toggle(f.field, b.value)}>
                        {b.value.replace(/_/g, '-')} <span className="muted tiny">{b.count}</span>
                      </button>
                    ))}
                  </div>
                </FilterRow>
              ))}
              <Slot name="torrent.list.filters" ctx={{ index_schema: schema.data ?? null }} />
            </>
          }
        />
        {res.isLoading ? (
          <SkeletonRows n={8} />
        ) : res.error ? (
          <div className="pad">
            <ErrorText error={res.error} />
          </div>
        ) : rows.length === 0 ? (
          <div className="pad">
            <TorrentRows rows={[]} empty={t('ui.torrent.no_results')} />
            {(params.get('q') || active.length > 0 || curType) && (
              <div className="row center">
                <Button
                  variant="primary"
                  onClick={() => {
                    setQ('');
                    setParams(new URLSearchParams());
                  }}
                >
                  {t('ui.torrent.reset_search')}
                </Button>
              </div>
            )}
          </div>
        ) : view === 'wall' ? (
          <TorrentWall rows={rows} />
        ) : (
          <TorrentRows
            rows={rows}
            highlights={new Map(hits.map((h) => [h.id, h.highlights]))}
            sort={sort as SortKey}
            onSort={(k) => {
              if (k === 'newest' || k === 'largest') update((p) => p.set('sort', k));
            }}
          />
        )}
        {res.hasNextPage && <LoadMore onClick={() => void res.fetchNextPage()} loading={res.isFetchingNextPage} label={t('ui.torrent.load_more')} />}
      </Panel>
    </div>
  );
}
