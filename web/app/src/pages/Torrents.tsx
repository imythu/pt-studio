// 种子列表 / 搜索（search-meilisearch）、详情、发布（torrent）。筛选条件序列化到 URL，可分享。
import { useQuery } from '@tanstack/react-query';
import { useState, type FormEvent } from 'react';
import { Link, useNavigate, useParams, useSearchParams } from 'react-router';
import { api, apiFetch, parse } from '@pt-studio/sdk';
import { Slot, useSlotColumns } from '@pt-studio/slots';
import { formatBytes, useT } from '@pt-studio/terms';
import { SchemaForm, serverErrors, validate, withDefaults, type Errors, type Schema } from '@pt-studio/schema-form';
import { Badge, Button, Card, Empty, Field, Input, Spinner, Table, UserContent, type Column } from '@pt-studio/ui-kit';
import { ApiError } from '@pt-studio/sdk';
import { fieldSchemas, useTypes, type TypeInfo } from '../types';
import { useSession } from '../session';
import { useInstalled } from '../manifest';
import { useQueryClient } from '@tanstack/react-query';
import { useToast } from '@pt-studio/ui-kit';
import { useErrorText } from '../errors';
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

export function TypeBadge({ typeId }: { typeId?: string }) {
  const t = useT();
  const types = useTypes();
  const info = types.data?.find((x) => x.type_id === typeId);
  return <Badge>{info ? t(`term:${info.name_term}`) : (typeId ?? '-')}</Badge>;
}

/** 由元数据面板展示的字段，不在通用字段表中重复 */
const HIDDEN_FIELDS = ['release_name', 'poster', 'overview', 'group_title'];

/** 作品元数据：海报、简介、类型、评分（来自元数据插件写入的 Group 字段） */
export function GroupMeta({ fields }: { fields: Record<string, unknown> }) {
  const t = useT();
  const poster = typeof fields.poster === 'string' && fields.poster.startsWith('/media/') ? fields.poster : null;
  const overview = typeof fields.overview === 'string' ? fields.overview : null;
  if (!poster && !overview) return null;
  const genres = Array.isArray(fields.genres) ? (fields.genres as string[]) : [];
  return (
    <div className="group-meta">
      {poster && <img className="poster" src={poster} alt="" loading="lazy" />}
      <div>
        {genres.length > 0 && (
          <p>
            {genres.map((g) => (
              <Badge key={g}>{g}</Badge>
            ))}
          </p>
        )}
        {typeof fields.rating === 'number' && <p>TMDB ★ {fields.rating.toFixed(1)}</p>}
        {overview && <UserContent text={overview} />}
        {typeof fields.tmdb_id === 'number' && (
          <p className="pt-muted small attribution">
            {t('ui.group.attribution')}{' '}
            <a href={`https://www.themoviedb.org/movie/${fields.tmdb_id}`} target="_blank" rel="noopener noreferrer nofollow">
              TMDB
            </a>
          </p>
        )}
      </div>
    </div>
  );
}

/** 类型字段的通用展示（显示名来自 schema 的术语或 title；枚举值按术语渲染） */
export function FieldsView({ typeId, fields }: { typeId: string; fields: Record<string, unknown> }) {
  const t = useT();
  const types = useTypes();
  const props = fieldSchemas(types.data?.find((x) => x.type_id === typeId), fields);
  const rows = Object.entries(fields).filter(
    ([k, v]) => !HIDDEN_FIELDS.includes(k) && v !== null && v !== '' && !(Array.isArray(v) && v.length === 0),
  );
  if (rows.length === 0) return null;
  const label = (k: string) => {
    const s = props[k];
    const term = s?.['x-pt-label-term'];
    const v = term ? t(`term:${term}`) : null;
    return v && v !== term ? v : (s?.title ?? k);
  };
  // 枚举值的术语：`<字段术语>.<值>`（与 schema-form 一致），没有则显示原值
  const value = (k: string, x: unknown) => {
    const term = props[k]?.['x-pt-label-term'];
    if (!term) return String(x);
    const key = `${term}.${String(x)}`;
    const v = t(`term:${key}`);
    return v && v !== key ? v : String(x);
  };
  return (
    <dl className="fields">
      {rows.map(([k, v]) => (
        <div key={k}>
          <dt>{label(k)}</dt>
          <dd>{Array.isArray(v) ? v.map((x) => value(k, x)).join(', ') : typeof v === 'boolean' ? (v ? '✓' : '✗') : value(k, v)}</dd>
        </div>
      ))}
    </dl>
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
    { key: 'type', header: t('ui.torrent.type'), render: (r) => <TypeBadge typeId={r.type_id} /> },
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
  fields: Record<string, unknown>;
}

/** 编辑（本人或 torrent.update）与审核（torrent.update / torrent.delete） */
function ManageTorrent({ d }: { d: TorrentDetail }) {
  const t = useT();
  const s = useSession();
  const qc = useQueryClient();
  const toast = useToast();
  const errText = useErrorText();
  const nav = useNavigate();
  const [edit, setEdit] = useState<{ title: string; subtitle: string; description: string } | null>(null);
  const [status, setStatus] = useState({ to: d.status, reason: '' });
  const owner = s.me?.id === d.uploader.id;
  const canEdit = owner || s.can('torrent.update');
  if (!canEdit && !s.can('torrent.delete')) return null;
  const key = ['torrent', String(d.id)];
  const act = async (f: () => Promise<unknown>) => {
    try {
      await f();
      toast(t('ui.admin.saved'), 'success');
      await qc.invalidateQueries({ queryKey: key });
    } catch (e) {
      toast(errText(e), 'error');
    }
  };
  return (
    <Card title={t('ui.torrent.manage')}>
      {canEdit &&
        (edit ? (
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void act(async () => {
                await api(`/api/p/torrent/v1/torrents/${d.id}`, { method: 'PATCH', json: { ...edit, subtitle: edit.subtitle || null } });
                setEdit(null);
              });
            }}
          >
            <Field label={t('ui.torrent.title')}>
              <Input name="edit_title" value={edit.title} maxLength={300} onChange={(e) => setEdit({ ...edit, title: e.target.value })} required />
            </Field>
            <Field label={t('ui.upload.subtitle')}>
              <Input value={edit.subtitle} maxLength={300} onChange={(e) => setEdit({ ...edit, subtitle: e.target.value })} />
            </Field>
            <Field label={t('ui.torrent.description')}>
              <textarea className="pt-input" rows={6} value={edit.description} onChange={(e) => setEdit({ ...edit, description: e.target.value })} />
            </Field>
            <Button type="submit" variant="primary">
              {t('ui.admin.save')}
            </Button>{' '}
            <Button onClick={() => setEdit(null)}>{t('ui.common.cancel')}</Button>
          </form>
        ) : (
          <Button onClick={() => setEdit({ title: d.title, subtitle: d.subtitle ?? '', description: d.description })}>{t('ui.torrent.edit')}</Button>
        ))}
      {s.can('torrent.update') && (
        <form
          className="searchbar moderation"
          onSubmit={(e) => {
            e.preventDefault();
            void act(() => api(`/api/p/torrent/v1/admin/torrents/${d.id}/status`, { json: { to: status.to, reason: status.reason || undefined } }));
          }}
        >
          <select className="pt-input" name="status" value={status.to} onChange={(e) => setStatus({ ...status, to: e.target.value })}>
            {['candidate', 'pending', 'approved', 'rejected', 'disabled'].map((x) => (
              <option key={x}>{x}</option>
            ))}
          </select>
          <Input placeholder={t('ui.admin.reason')} value={status.reason} maxLength={500} onChange={(e) => setStatus({ ...status, reason: e.target.value })} />
          <Button type="submit">{t('ui.torrent.set_status')}</Button>
        </form>
      )}
      {s.can('torrent.delete') && (
        <Button
          variant="danger"
          onClick={() => {
            if (!window.confirm(t('ui.torrent.delete_confirm'))) return;
            void act(async () => {
              await api(`/api/p/torrent/v1/admin/torrents/${d.id}?reason=admin`, { method: 'DELETE' });
              nav('/torrents');
            });
          }}
        >
          {t('ui.torrent.delete')}
        </Button>
      )}
      {s.isStaff && <Slot name="admin.torrent.tabs" ctx={{ torrent: d }} />}
    </Card>
  );
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
      {d.status !== 'approved' && <Badge tone="danger">{d.status}</Badge>}
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
          {d.fields && Object.keys(d.fields).length > 0 && (
            <Card title={t('ui.torrent.info')} actions={d.group_id ? <Link to={`/groups/${d.group_id}`}>{t('ui.group.view')}</Link> : null}>
              <FieldsView typeId={d.type_id} fields={d.fields} />
            </Card>
          )}
          <Slot name="torrent.detail.info" ctx={ctx} />
          {d.description && (
            <Card title={t('ui.torrent.description')}>
              <UserContent text={d.description} />
            </Card>
          )}
          <Slot name="torrent.detail.tabs" ctx={ctx} />
        </div>
        <aside className="sidebar">
          <ManageTorrent d={d} />
          <Slot name="torrent.detail.sidebar" ctx={ctx} />
        </aside>
      </div>
    </div>
  );
}

interface MetaHit {
  external_id: string;
  title: string | null;
  original: string | null;
  year: number | null;
}

/** 上传表单：从 TMDB 搜索并预填（metadata-tmdb 已安装且类型为电影时） */
function MetadataSearch({ onPick }: { onPick(h: MetaHit): void }) {
  const t = useT();
  const [q, setQ] = useState('');
  const [hits, setHits] = useState<MetaHit[]>([]);
  const [error, setError] = useState<unknown>(null);
  const run = async () => {
    setError(null);
    try {
      setHits((await api<{ items: MetaHit[] }>(`/api/p/metadata-tmdb/v1/search?q=${encodeURIComponent(q)}&type=movie`)).items);
    } catch (e) {
      setError(e);
    }
  };
  return (
    <div className="meta-search">
      <div className="searchbar">
        <Input name="meta_q" placeholder={t('ui.upload.meta_search')} value={q} onChange={(e) => setQ(e.target.value)} />
        <Button onClick={() => void run()} disabled={!q.trim()}>
          TMDB
        </Button>
      </div>
      <ErrorText error={error} />
      {hits.map((h) => (
        <button
          type="button"
          key={h.external_id}
          className="pt-btn pt-btn-ghost meta-hit"
          onClick={() => {
            onPick(h);
            setHits([]);
          }}
        >
          {h.title} {h.original && h.original !== h.title && <span className="pt-muted">/ {h.original}</span>} {h.year && `(${h.year})`}
        </button>
      ))}
    </div>
  );
}

function TypeForm({ info, value, onChange, errors }: { info: TypeInfo; value: Record<string, unknown>; onChange(v: Record<string, unknown>): void; errors: Errors }) {
  const t = useT();
  if (!info.field_schema) return null;
  return <SchemaForm schema={info.field_schema} value={value} onChange={(v) => onChange((v as Record<string, unknown>) ?? {})} errors={errors} t={t} />;
}

export function Upload() {
  const t = useT();
  const nav = useNavigate();
  const types = useTypes();
  const [typeId, setTypeId] = useState<string>('');
  const [meta, setMeta] = useState({ title: '', subtitle: '', description: '', anonymous: false });
  const [fields, setFields] = useState<Record<string, unknown>>({});
  const [file, setFile] = useState<File | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [errors, setErrors] = useState<Errors>({});
  const [busy, setBusy] = useState(false);
  const list = types.data ?? [];
  const info = list.find((x) => x.type_id === typeId) ?? list[0];
  const hasTmdb = useInstalled('metadata-tmdb');
  const pick = (id: string) => {
    setTypeId(id);
    const i = list.find((x) => x.type_id === id);
    setFields(i?.field_schema ? ((withDefaults(i.field_schema as Schema, {}) as Record<string, unknown>) ?? {}) : {});
    setErrors({});
  };
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!file || !info) return;
    // 客户端预校验（发布名可补全的字段由服务端判定，这里只查类型与范围）
    const errs = info.field_schema ? Object.fromEntries(Object.entries(validate(info.field_schema, fields)).filter(([, m]) => m !== 'required')) : {};
    setErrors(errs);
    if (Object.keys(errs).length) return;
    setBusy(true);
    setError(null);
    try {
      const fd = new FormData();
      fd.append('meta', JSON.stringify({ ...meta, type_id: info.type_id, fields, subtitle: meta.subtitle || undefined }));
      fd.append('torrent', file, file.name);
      const r = await parse<{ id: number }>(await apiFetch('/api/p/torrent/v1/torrents', { method: 'POST', body: fd, idempotent: true }));
      nav(`/torrents/${r.id}`);
    } catch (err) {
      if (err instanceof ApiError) setErrors(serverErrors(err.details));
      setError(err);
    } finally {
      setBusy(false);
    }
  };
  return (
    <Card title={t('ui.upload.title')} className="narrow wide">
      <form onSubmit={submit}>
        <Field label={t('ui.upload.file')}>
          <input name="torrent" type="file" accept=".torrent,application/x-bittorrent" onChange={(e) => setFile(e.target.files?.[0] ?? null)} required />
        </Field>
        <Field label={t('ui.torrent.type')}>
          <select className="pt-input" name="type_id" value={info?.type_id ?? ''} onChange={(e) => pick(e.target.value)}>
            {list.map((x) => (
              <option key={x.type_id} value={x.type_id}>
                {t(`term:${x.name_term}`)}
              </option>
            ))}
          </select>
        </Field>
        <Field label={t('ui.torrent.title')} hint={info?.plugin ? t('ui.upload.title_hint') : undefined}>
          <Input name="title" value={meta.title} onChange={(e) => setMeta({ ...meta, title: e.target.value })} required={!info?.plugin} maxLength={300} />
        </Field>
        <Field label={t('ui.upload.subtitle')}>
          <Input value={meta.subtitle} onChange={(e) => setMeta({ ...meta, subtitle: e.target.value })} maxLength={300} />
        </Field>
        {info?.type_id === 'movie' && hasTmdb && (
          <MetadataSearch
            onPick={(h) => {
              const id = Number(h.external_id.split(':')[1]);
              setFields({ ...fields, tmdb_id: id, ...(h.title ? { title: h.original ?? h.title } : {}), ...(h.year ? { year: h.year } : {}) });
            }}
          />
        )}
        {info && (
          <Slot
            name="torrent.upload.type_form"
            ctx={{ type_id: info.type_id, schema: info.field_schema, value: fields, onChange: setFields }}
            fallback={<TypeForm info={info} value={fields} onChange={setFields} errors={errors} />}
            empty={<TypeForm info={info} value={fields} onChange={setFields} errors={errors} />}
          />
        )}
        <Field label={t('ui.torrent.description')}>
          <textarea className="pt-input" rows={6} value={meta.description} onChange={(e) => setMeta({ ...meta, description: e.target.value })} />
        </Field>
        <label className="pt-check">
          <input type="checkbox" checked={meta.anonymous} onChange={(e) => setMeta({ ...meta, anonymous: e.target.checked })} /> {t('ui.upload.anonymous')}
        </label>
        <Slot name="torrent.upload.extra" ctx={{ draft: { ...meta, type_id: info?.type_id, fields } }} />
        <ErrorText error={error} />
        <Button type="submit" variant="primary" disabled={busy || !file || !info}>
          {t('ui.upload.submit')}
        </Button>
      </form>
    </Card>
  );
}

interface GroupDetail {
  id: number;
  type_id: string;
  title: string;
  year: number | null;
  fields: Record<string, unknown>;
  torrent_count: number;
  torrents: { id: number; title: string; subtitle: string | null; size: number; fields: Record<string, unknown>; created_at: string }[];
}

export function GroupPage() {
  const t = useT();
  const { id } = useParams();
  const g = useQuery({ queryKey: ['group', id], queryFn: () => api<GroupDetail>(`/api/p/torrent/v1/groups/${id}`), staleTime: 60_000 });
  if (g.isLoading) return <Spinner />;
  if (g.error) return <ErrorText error={g.error} />;
  const d = g.data as GroupDetail;
  return (
    <div className="group">
      <h1>
        {d.title} {d.year && <span className="pt-muted">({d.year})</span>} <TypeBadge typeId={d.type_id} />
      </h1>
      <Slot name="group.detail.header" ctx={{ group: d }} />
      <Card>
        <GroupMeta fields={d.fields} />
        <FieldsView typeId={d.type_id} fields={d.fields} />
      </Card>
      <Card title={t('ui.group.torrents', { n: d.torrents.length })}>
        <TorrentTable rows={d.torrents.map((x) => ({ ...x, type_id: d.type_id, created_at: Date.parse(x.created_at) / 1000 }))} />
      </Card>
      <Slot name="group.detail.tabs" ctx={{ group: d }} />
    </div>
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
