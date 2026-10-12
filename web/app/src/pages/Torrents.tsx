// 种子列表 / 搜索（search-meilisearch）、详情、发布（torrent）。筛选条件序列化到 URL，可分享。
import { useQuery } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';
import { Link, useNavigate, useParams, useSearchParams } from 'react-router';
import { api, apiFetch, parse } from '@pt-studio/sdk';
import { Slot, useSlotColumns } from '@pt-studio/slots';
import { formatBytes, useT } from '@pt-studio/terms';
import { Badge, Button, Card, Empty, Field, Input, Spinner, Table, UserContent, type Column } from '@pt-studio/ui-kit';
import { ErrorText } from '../errors';

export interface TorrentDoc {
  id: number;
  title: string;
  subtitle?: string | null;
  size: number;
  type_id?: string;
  created_at?: number;
  rel_resolution?: string | null;
  rel_source?: string | null;
  rel_video_codec?: string | null;
  [k: string]: unknown;
}

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
  newest: [{ field: 'created_at', descending: true }],
  largest: [{ field: 'size', descending: true }],
};

/** 高亮：U+E000 / U+E001 包围的片段渲染为 <mark>，其余按纯文本 */
export function Highlight({ text }: { text: string }) {
  const parts = text.split(/([^]*)/u);
  return (
    <>
      {parts.map((p, i) =>
        p.startsWith('') ? <mark key={i}>{p.slice(1, -1)}</mark> : <span key={i}>{p}</span>,
      )}
    </>
  );
}

function useSearch(params: URLSearchParams, extraFacets: string[]) {
  const q = params.get('q') ?? '';
  const sort = params.get('sort') ?? (q ? 'relevance' : 'newest');
  const filters = [...params.entries()]
    .filter(([k]) => k.startsWith('f.'))
    .map(([k, v]) => ({ field: k.slice(2), op: 'in', values: v.split(',') }));
  const body = { index: 'torrents', query: q, filters, sort: SORTS[sort] ?? [], facets: extraFacets, limit: 50 };
  return useQuery({
    queryKey: ['search', body],
    queryFn: () => api<SearchResult>('/api/p/search-meilisearch/v1/search', { json: body }),
    staleTime: 30_000,
  });
}

function FilterPanel({ facets, params, setParams }: { facets: SearchResult['facets']; params: URLSearchParams; setParams(p: URLSearchParams): void }) {
  const t = useT();
  const schema = useQuery({
    queryKey: ['search-schema', 'torrents'],
    queryFn: () => api<SchemaView>('/api/p/search-meilisearch/v1/indexes/torrents/schema'),
    staleTime: Infinity,
  });
  const label = (name: string) => {
    const f = schema.data?.fields.find((x) => x.name === name);
    return f?.label_term ? t(`term:${f.label_term}`) : name;
  };
  const toggle = (field: string, value: string) => {
    const p = new URLSearchParams(params);
    const cur = new Set((p.get(`f.${field}`) ?? '').split(',').filter(Boolean));
    if (cur.has(value)) cur.delete(value);
    else cur.add(value);
    if (cur.size) p.set(`f.${field}`, [...cur].join(','));
    else p.delete(`f.${field}`);
    setParams(p);
  };
  return (
    <aside className="filters">
      {facets
        .filter((f) => f.buckets.length > 0)
        .map((f) => {
          const sel = new Set((params.get(`f.${f.field}`) ?? '').split(','));
          return (
            <div key={f.field} className="facet">
              <div className="facet-title">{label(f.field)}</div>
              {f.buckets.map((b) => (
                <label key={b.value} className="pt-check">
                  <input type="checkbox" checked={sel.has(b.value)} onChange={() => toggle(f.field, b.value)} /> {b.value}{' '}
                  <span className="pt-muted">{b.count}</span>
                </label>
              ))}
            </div>
          );
        })}
      <Slot name="torrent.list.filters" ctx={{ index_schema: schema.data ?? null }} />
    </aside>
  );
}

export function TorrentTable({ rows, highlights }: { rows: TorrentDoc[]; highlights?: Map<number, Record<string, string>> }) {
  const t = useT();
  const pluginCols = useSlotColumns('torrent.list.columns');
  const cols: Column<TorrentDoc>[] = [
    {
      key: 'title',
      header: t('ui.torrent.title'),
      render: (r) => (
        <div>
          <Link to={`/torrents/${r.id}`}>{highlights?.get(r.id)?.title ? <Highlight text={highlights.get(r.id)?.title ?? ''} /> : r.title}</Link>{' '}
          <Slot name="torrent.list.row_badges" ctx={{ torrent: r }} />
          {r.subtitle && <div className="pt-muted small">{r.subtitle}</div>}
        </div>
      ),
    },
    { key: 'type', header: t('ui.torrent.type'), render: (r) => <Badge>{r.type_id ? t(`term:torrent.type.${r.type_id}`) : '-'}</Badge> },
    ...pluginCols.map((c) => ({ key: `p:${c.key}`, header: c.header, render: (r: TorrentDoc) => c.render({ torrent: r }) })),
    { key: 'size', header: t('ui.torrent.size'), render: (r) => formatBytes(r.size), className: 'num' },
    { key: 'created', header: t('ui.torrent.created'), render: (r) => (r.created_at ? new Date(r.created_at * 1000).toLocaleString() : '-'), className: 'nowrap' },
  ];
  return <Table rows={rows} columns={cols} rowKey={(r) => r.id} empty={t('ui.torrent.no_results')} />;
}

export function TorrentList() {
  const t = useT();
  const [params, setParams] = useSearchParams();
  const [q, setQ] = useState(params.get('q') ?? '');
  const res = useSearch(params, ['rel_resolution', 'rel_source', 'rel_video_codec', 'type_id']);
  const submit = (e: FormEvent) => {
    e.preventDefault();
    const p = new URLSearchParams(params);
    if (q) p.set('q', q);
    else p.delete('q');
    setParams(p);
  };
  const sort = params.get('sort') ?? ((params.get('q') ?? '') ? 'relevance' : 'newest');
  return (
    <div className="with-sidebar">
      {res.data && <FilterPanel facets={res.data.facets} params={params} setParams={setParams} />}
      <div className="main">
        <form onSubmit={submit} className="searchbar">
          <Input name="q" type="search" value={q} placeholder={t('ui.torrent.search')} onChange={(e) => setQ(e.target.value)} />
          <select
            className="pt-input"
            value={sort}
            onChange={(e) => {
              const p = new URLSearchParams(params);
              p.set('sort', e.target.value);
              setParams(p);
            }}
          >
            {Object.keys(SORTS).map((s) => (
              <option key={s} value={s}>
                {t(`ui.torrent.sort.${s}`)}
              </option>
            ))}
          </select>
        </form>
        {res.isLoading && <Spinner />}
        <ErrorText error={res.error} />
        {res.data && (
          <>
            <p className="pt-muted small">≈ {res.data.estimated_total}</p>
            <TorrentTable rows={res.data.hits.map((h) => h.document)} highlights={new Map(res.data.hits.map((h) => [h.id, h.highlights]))} />
          </>
        )}
      </div>
    </div>
  );
}

interface TorrentDetail {
  id: number;
  title: string;
  subtitle: string | null;
  type_id: string;
  size: number;
  file_count: number;
  status: string;
  uploader: { id?: number; anonymous?: boolean };
  description: string;
  created_at: string;
  can_download: boolean;
  info_hash_v1: string | null;
  group_id: number | null;
}

export function TorrentPage() {
  const t = useT();
  const { id } = useParams();
  const tq = useQuery({ queryKey: ['torrent', id], queryFn: () => api<TorrentDetail>(`/api/p/torrent/v1/torrents/${id}`), staleTime: 60_000 });
  const files = useQuery({
    queryKey: ['torrent-files', id],
    queryFn: () => api<{ items: { path: string; size: number }[] | { path: string[]; length: number }[] }>(`/api/p/torrent/v1/torrents/${id}/files`),
    enabled: tq.isSuccess,
  });
  if (tq.isLoading) return <Spinner />;
  if (tq.error) return <ErrorText error={tq.error} />;
  const d = tq.data as TorrentDetail;
  const ctx = { torrent: d };
  return (
    <div className="torrent">
      <h1>{d.title}</h1>
      {d.subtitle && <p className="pt-muted">{d.subtitle}</p>}
      <Slot name="torrent.detail.header" ctx={ctx} />
      <div className="with-sidebar reverse">
        <div className="main">
          <Card
            title={t('ui.torrent.files')}
            actions={
              d.can_download && (
                <a className="pt-btn pt-btn-primary" href={`/api/p/torrent/v1/torrents/${d.id}/download`} download>
                  {t('ui.torrent.download')}
                </a>
              )
            }
          >
            <p>
              {formatBytes(d.size)} · {d.file_count} · {t('ui.torrent.uploader')}:{' '}
              {d.uploader.anonymous ? t('ui.torrent.anonymous') : <Link to={`/users/${d.uploader.id}`}>#{d.uploader.id}</Link>} ·{' '}
              {new Date(d.created_at).toLocaleString()}
            </p>
            <Slot name="torrent.detail.actions" ctx={ctx} />
            <ul className="files">
              {(files.data?.items ?? []).slice(0, 200).map((f, i) => {
                const anyF = f as { path: string | string[]; size?: number; length?: number };
                const path = Array.isArray(anyF.path) ? anyF.path.join('/') : anyF.path;
                return (
                  <li key={i}>
                    <span className="pt-mono">{path}</span> <span className="pt-muted">{formatBytes(anyF.size ?? anyF.length ?? 0)}</span>
                  </li>
                );
              })}
            </ul>
          </Card>
          <Slot name="torrent.detail.info" ctx={ctx} />
          {d.description && (
            <Card title={t('ui.torrent.description')}>
              <UserContent text={d.description} />
            </Card>
          )}
          <Slot name="torrent.detail.tabs" ctx={ctx} />
        </div>
        <aside className="sidebar">
          <Slot name="torrent.detail.sidebar" ctx={ctx} />
        </aside>
      </div>
    </div>
  );
}

export function Upload() {
  const t = useT();
  const nav = useNavigate();
  const [meta, setMeta] = useState({ type_id: 'generic', title: '', subtitle: '', description: '', anonymous: false });
  const [file, setFile] = useState<File | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!file) return;
    setBusy(true);
    setError(null);
    try {
      const fd = new FormData();
      fd.append('meta', JSON.stringify({ ...meta, subtitle: meta.subtitle || undefined }));
      fd.append('torrent', file, file.name);
      const r = await parse<{ id: number }>(await apiFetch('/api/p/torrent/v1/torrents', { method: 'POST', body: fd, idempotent: true }));
      nav(`/torrents/${r.id}`);
    } catch (err) {
      setError(err);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Card title={t('ui.upload.title')} className="narrow">
      <form onSubmit={submit}>
        <Field label={t('ui.upload.file')}>
          <input name="torrent" type="file" accept=".torrent,application/x-bittorrent" onChange={(e) => setFile(e.target.files?.[0] ?? null)} required />
        </Field>
        <Field label={t('ui.torrent.type')}>
          <Input value={meta.type_id} onChange={(e) => setMeta({ ...meta, type_id: e.target.value })} required />
        </Field>
        <Field label={t('ui.torrent.title')}>
          <Input name="title" value={meta.title} onChange={(e) => setMeta({ ...meta, title: e.target.value })} required maxLength={300} />
        </Field>
        <Field label={t('ui.upload.subtitle')}>
          <Input value={meta.subtitle} onChange={(e) => setMeta({ ...meta, subtitle: e.target.value })} maxLength={300} />
        </Field>
        <Slot name="torrent.upload.type_form" ctx={{ type_id: meta.type_id, schema: null }} />
        <Field label={t('ui.torrent.description')}>
          <textarea className="pt-input" rows={6} value={meta.description} onChange={(e) => setMeta({ ...meta, description: e.target.value })} />
        </Field>
        <label className="pt-check">
          <input type="checkbox" checked={meta.anonymous} onChange={(e) => setMeta({ ...meta, anonymous: e.target.checked })} /> {t('ui.upload.anonymous')}
        </label>
        <Slot name="torrent.upload.extra" ctx={{ draft: meta }} />
        <ErrorText error={error} />
        <Button type="submit" variant="primary" disabled={busy || !file}>
          {t('ui.upload.submit')}
        </Button>
      </form>
    </Card>
  );
}

export function Home() {
  const t = useT();
  const res = useQuery({
    queryKey: ['search', 'home-latest'],
    queryFn: () =>
      api<SearchResult>('/api/p/search-meilisearch/v1/search', {
        json: { index: 'torrents', query: '', sort: SORTS.newest, limit: 15 },
      }),
    staleTime: 30_000,
    retry: false,
  });
  return (
    <div>
      <Slot name="home.announcements" ctx={{}} />
      <div className="with-sidebar reverse">
        <div className="main">
          <Card title={t('ui.home.latest')} actions={<Link to="/torrents">→</Link>}>
            {res.isLoading ? <Spinner /> : res.error ? <ErrorText error={res.error} /> : <TorrentTable rows={res.data?.hits.map((h) => h.document) ?? []} />}
          </Card>
        </div>
        <aside className="sidebar">
          <Slot name="home.widgets" ctx={{}} fallback={<Empty>…</Empty>} />
        </aside>
      </div>
    </div>
  );
}
