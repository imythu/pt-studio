// 种子列表 / 搜索（search-meilisearch）、详情、发布（torrent）。筛选条件序列化到 URL，可分享。
import { useQuery } from '@tanstack/react-query';
import { useEffect, useMemo, useRef, useState, type FormEvent } from 'react';
import { Link, useNavigate, useParams, useSearchParams } from 'react-router';
import { api, apiFetch, parse, renderPreview } from '@pt-studio/sdk';
import { Slot, useSlotColumns } from '@pt-studio/slots';
import { formatBytes, useT } from '@pt-studio/terms';
import { SchemaForm, serverErrors, validate, withDefaults, type Errors, type Schema } from '@pt-studio/schema-form';
import { Badge, Button, Card, CopyButton, Empty, Field, Input, MarkdownEditor, SkeletonRows, Spinner, Table, UserContent, type Column } from '@pt-studio/ui-kit';
import { HeroArt, Icon, Logo } from '../art';
import { Hint, DownloadGuide, GettingStarted, useFirstTime } from '../help';
import { ApiError } from '@pt-studio/sdk';
import { fieldSchemas, useTypes, type TypeInfo } from '../types';
import { useManifest } from '../manifest';
import { useSession } from '../session';
import { useInstalled } from '../manifest';
import { useQueryClient } from '@tanstack/react-query';
import { useToast } from '@pt-studio/ui-kit';
import { useErrorText } from '../errors';
import { ErrorText } from '../errors';
import { SuggestInput } from '../suggest';

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
  // 置顶的排在最前
  newest: [
    { field: 'sticky', descending: true },
    { field: 'created_at', descending: true },
  ],
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
  return <span className={`pt-badge type-badge type-${typeId ?? 'other'}`}>{info ? t(`term:${info.name_term}`) : (typeId ?? '-')}</span>;
}

/** 由元数据面板展示的字段，不在通用字段表中重复 */
const HIDDEN_FIELDS = ['release_name', 'poster', 'overview', 'group_title', 'edition_key', 'logs', 'cover', 'description'];

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

const hasCjk = (x: unknown): x is string => typeof x === 'string' && /[\u3400-\u9fff]/.test(x);

/** 本地化（中文）名：副标题优先，其次元数据补全的中文名 */
export function zhName(r: TorrentDoc): string | null {
  if (r.subtitle) return r.subtitle;
  for (const k of ['tv_title_zh', 'anime_title_zh', 'movie_title_zh', 'movie_title']) {
    if (hasCjk(r[k])) return r[k] as string;
  }
  return null;
}

/** 相对时间（一周内），更早显示日期 */
function timeAgo(ms: number): string {
  const secs = Math.round((ms - Date.now()) / 1000);
  const rtf = new Intl.RelativeTimeFormat(document.documentElement.lang || navigator.language, { numeric: 'auto' });
  const abs = Math.abs(secs);
  if (abs < 60) return rtf.format(Math.trunc(secs / 1), 'second');
  if (abs < 3600) return rtf.format(Math.trunc(secs / 60), 'minute');
  if (abs < 86400) return rtf.format(Math.trunc(secs / 3600), 'hour');
  if (abs < 7 * 86400) return rtf.format(Math.trunc(secs / 86400), 'day');
  return new Date(ms).toLocaleDateString();
}

interface Swarm {
  seeders: number;
  leechers: number;
  completed: number;
}

/** 列表页批量取做种 / 下载 / 完成人数 */
function useSwarmBatch(ids: number[]) {
  const key = ids.join(',');
  return useQuery({
    queryKey: ['swarm-batch', key],
    queryFn: () => api<{ items: Record<string, Swarm> }>(`/api/p/accounting/v1/torrents/swarm?ids=${key}`),
    enabled: ids.length > 0,
    staleTime: 20_000,
    retry: false,
  }).data?.items;
}

export function SwarmCell({ s }: { s?: Swarm }) {
  const n = s ?? { seeders: 0, leechers: 0, completed: 0 };
  return (
    <span className="swarm" title={`↑ ${n.seeders}  ↓ ${n.leechers}  ✓ ${n.completed}`}>
      <b className={n.seeders > 0 ? 'seed' : 'seed zero'}>{n.seeders}</b>
      <i>/</i>
      <b className={n.leechers > 0 ? 'leech' : 'leech zero'}>{n.leechers}</b>
      <i>/</i>
      <b className="done">{n.completed}</b>
    </span>
  );
}

export function TorrentTable({ rows, highlights }: { rows: TorrentDoc[]; highlights?: Map<number, Record<string, string>> }) {
  const t = useT();
  const pluginCols = useSlotColumns('torrent.list.columns');
  const swarm = useSwarmBatch(rows.map((r) => r.id));
  const cols: Column<TorrentDoc>[] = [
    {
      key: 'title',
      header: t('ui.torrent.title'),
      render: (r) => {
        const tags = [r.rel_resolution, r.rel_source, r.rel_video_codec].filter((x): x is string => typeof x === 'string' && x !== '');
        const poster = typeof r.poster === 'string' && r.poster.startsWith('/media/') ? r.poster : null;
        return (
          <div className="t-row">
            {poster && <img className="t-thumb" src={poster} alt="" loading="lazy" />}
            <div>
            <Link className="t-title" to={`/torrents/${r.id}`}>
              {highlights?.get(r.id)?.title ? <Highlight text={highlights.get(r.id)?.title ?? ''} /> : r.title}
            </Link>
            {zhName(r) && <div className="t-zh">{zhName(r)}</div>}
            <div className="t-tags">
              {r.sticky === 1 && <span className="pt-badge pt-badge-primary">{t('ui.torrent.sticky')}</span>}
              {tags.map((x, i) => (
                <span key={x} className={`pt-badge${i === 0 ? ' tag-res' : ''}`}>
                  {x}
                </span>
              ))}
              <Slot name="torrent.list.row_badges" ctx={{ torrent: r }} />
            </div>
            <div className="t-quick-actions">
              <a
                className="pt-copy-btn"
                href={`/api/p/torrent/v1/torrents/${r.id}/download`}
                download
                title={t('ui.torrent.download')}
                onClick={(e) => e.stopPropagation()}
              >
                <Icon name="download" size={13} /> {t('ui.torrent.download')}
              </a>
              <CopyButton
                text={typeof window !== 'undefined' ? `${window.location.origin}/api/p/torrent/v1/torrents/${r.id}/download` : `/api/p/torrent/v1/torrents/${r.id}/download`}
                title={t('ui.torrent.copy_link')}
              />
            </div>
            </div>
          </div>
        );
      },
    },
    { key: 'type', header: t('ui.torrent.type'), render: (r) => <TypeBadge typeId={r.type_id} /> },
    ...pluginCols.map((c) => ({ key: `p:${c.key}`, header: c.header, render: (r: TorrentDoc) => c.render({ torrent: r }) })),
    { key: 'swarm', header: t('ui.torrent.swarm_short'), render: (r) => <SwarmCell s={swarm?.[String(r.id)]} />, className: 'num' },
    { key: 'size', header: t('ui.torrent.size'), render: (r) => formatBytes(r.size), className: 'num' },
    { key: 'created', header: t('ui.torrent.created'), render: (r) => (r.created_at ? timeAgo(r.created_at * 1000) : '-'), className: 'nowrap' },
  ];
  if (rows.length === 0) return <Empty>{t('ui.torrent.no_results')}</Empty>;
  return (
    <div className="torrent-table">
      <div className="t-desktop">
        <Table rows={rows} columns={cols} rowKey={(r) => r.id} empty={t('ui.torrent.no_results')} />
      </div>
      <ul className="t-cards">
        {rows.map((r) => {
          const tags = [r.rel_resolution, r.rel_source, r.rel_video_codec].filter((x): x is string => typeof x === 'string' && x !== '');
          const poster = typeof r.poster === 'string' && r.poster.startsWith('/media/') ? r.poster : null;
          return (
            <li key={r.id}>
              <Link to={`/torrents/${r.id}`} className="t-card">
                {poster && <img className="t-thumb" src={poster} alt="" loading="lazy" />}
                <div className="t-card-main">
                  <div className="t-card-top">
                    <TypeBadge typeId={r.type_id} />
                    {tags.slice(0, 2).map((x) => (
                      <span key={x} className="pt-badge">
                        {x}
                      </span>
                    ))}
                    <a
                      className="pt-copy-btn"
                      style={{ marginLeft: 'auto', padding: '2px 8px' }}
                      href={`/api/p/torrent/v1/torrents/${r.id}/download`}
                      download
                      title={t('ui.torrent.download')}
                      onClick={(e) => e.stopPropagation()}
                    >
                      <Icon name="download" size={13} />
                    </a>
                  </div>
                  <b className="t-card-title">{zhName(r) ?? r.title}</b>
                  {zhName(r) && <span className="t-card-sub">{r.title}</span>}
                  <span className="t-card-meta">
                    <SwarmCell s={swarm?.[String(r.id)]} /> · {formatBytes(r.size)} · {r.created_at ? timeAgo(r.created_at * 1000) : ''}
                  </span>
                </div>
              </Link>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

const typeRank = (id: string) => {
  const i = ['movie', 'tv', 'anime', 'music', 'generic'].indexOf(id);
  return i < 0 ? 4 : i;
};

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
  const [showFilters, setShowFilters] = useState(false);
  const types = useTypes();
  const curType = params.get('f.type_id') ?? '';
  const setType = (id: string) => {
    const p = new URLSearchParams(params);
    if (id) p.set('f.type_id', id);
    else p.delete('f.type_id');
    setParams(p);
  };
  const active = [...params.entries()].filter(([k]) => k.startsWith('f.')).flatMap(([k, v]) => v.split(',').filter(Boolean).map((val) => ({ field: k.slice(2), val })));
  const unfilter = (field: string, val: string) => {
    const p = new URLSearchParams(params);
    const rest = (p.get(`f.${field}`) ?? '').split(',').filter((x) => x && x !== val);
    if (rest.length) p.set(`f.${field}`, rest.join(','));
    else p.delete(`f.${field}`);
    setParams(p);
  };
  return (
    <div>
      <form onSubmit={submit} className="page-search">
        <div className="search-box">
          <Icon name="search" size={18} />
          <SuggestInput className="pt-input" name="q" type="search" value={q} placeholder={t('ui.torrent.search')} aria-label={t('ui.torrent.search')} onValue={setQ} />
        </div>
        <div className="segmented" role="group" aria-label={t('ui.torrent.sort')}>
          {Object.keys(SORTS).map((k) => (
            <button
              key={k}
              type="button"
              aria-pressed={sort === k}
              onClick={() => {
                const p = new URLSearchParams(params);
                p.set('sort', k);
                setParams(p);
              }}
            >
              {t(`ui.torrent.sort.${k}`)}
            </button>
          ))}
        </div>
      </form>
      <div className="type-chips" role="tablist">
        <button type="button" role="tab" aria-selected={!curType} onClick={() => setType('')}>
          {t('ui.torrent.all_types')}
        </button>
        {[...(types.data ?? [])].sort((a, b) => typeRank(a.type_id) - typeRank(b.type_id)).map((x) => (
          <button key={x.type_id} type="button" role="tab" aria-selected={curType === x.type_id} onClick={() => setType(x.type_id)}>
            {t(`term:${x.name_term}`)}
          </button>
        ))}
        <button type="button" className={`filter-toggle${showFilters ? ' on' : ''}`} onClick={() => setShowFilters(!showFilters)}>
          <Icon name="filter" size={16} /> {t('ui.torrent.filters')}
        </button>
      </div>
      {active.filter((a) => a.field !== 'type_id').length > 0 && (
        <div className="chips">
          {active.filter((a) => a.field !== 'type_id').map((a) => (
            <button key={`${a.field}:${a.val}`} type="button" className="chip" onClick={() => unfilter(a.field, a.val)}>
              {a.val} <Icon name="close" size={14} />
            </button>
          ))}
          <button
            type="button"
            className="filter-clear-all"
            onClick={() => {
              const p = new URLSearchParams(params);
              for (const [k] of params.entries()) {
                if (k.startsWith('f.') && k !== 'f.type_id') p.delete(k);
              }
              setParams(p);
            }}
          >
            {t('ui.torrent.clear_filters')}
          </button>
        </div>
      )}
      <div className={`with-sidebar list-layout${showFilters ? ' filters-open' : ''}`}>
        {res.data && <FilterPanel facets={res.data.facets.filter((f) => f.field !== 'type_id')} params={params} setParams={setParams} />}
        <div className="main">
          {res.isLoading && <SkeletonRows n={8} />}
          <ErrorText error={res.error} />
          {res.data && (
            <>
              <p className="result-count">{t('ui.torrent.count', { n: res.data.estimated_total })}</p>
              {res.data.hits.length === 0 ? (
                <Card className="list-card">
                  <Empty>
                    <span>{t('ui.torrent.no_results')}</span>
                    {(q || active.length > 0) && (
                      <div className="empty-actions">
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
                  </Empty>
                </Card>
              ) : (
                <Card className="list-card">
                  <TorrentTable rows={res.data.hits.map((h) => h.document)} highlights={new Map(res.data.hits.map((h) => [h.id, h.highlights]))} />
                </Card>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}

interface TorrentDetail {
  id: number;
  description_html?: string;
  sticky_until?: string | null;
  title: string;
  subtitle: string | null;
  type_id: string;
  size: number;
  file_count: number;
  status: string;
  uploader: { id?: number; username?: string | null; anonymous?: boolean };
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
  const [stickyDays, setStickyDays] = useState('7');
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
              <MarkdownEditor name="edit_description" rows={6} value={edit.description} onChange={(d) => setEdit({ ...edit, description: d })} preview={renderPreview} />
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
          className="pt-stack moderation"
          onSubmit={(e) => {
            e.preventDefault();
            void act(() => api(`/api/p/torrent/v1/admin/torrents/${d.id}/status`, { json: { to: status.to, reason: status.reason || undefined } }));
          }}
        >
          <select className="pt-input" name="status" value={status.to} onChange={(e) => setStatus({ ...status, to: e.target.value })}>
            {['candidate', 'pending', 'approved', 'rejected', 'disabled'].map((x) => (
              <option key={x} value={x}>
                {statusName(t, x)}
              </option>
            ))}
          </select>
          <Input placeholder={t('ui.admin.reason')} value={status.reason} maxLength={500} onChange={(e) => setStatus({ ...status, reason: e.target.value })} />
          <Button type="submit">{t('ui.torrent.set_status')}</Button>
        </form>
      )}
      {s.can('torrent.update') && (
        <p className="pt-stack">
          {d.sticky_until && <span className="pt-muted small">{t('ui.torrent.sticky_until', { at: d.sticky_until.startsWith('9999') ? t('ui.torrent.sticky_forever') : new Date(d.sticky_until).toLocaleString() })}</span>}
          <select className="pt-input" name="sticky_days" value={stickyDays} onChange={(e) => setStickyDays(e.target.value)}>
            {['1', '3', '7', '30', '0'].map((x) => (
              <option key={x} value={x}>
                {x === '0' ? t('ui.torrent.sticky_forever') : t('ui.torrent.sticky_days', { n: x })}
              </option>
            ))}
          </select>
          <Button onClick={() => void act(() => api(`/api/p/torrent/v1/admin/torrents/${d.id}/sticky`, { json: { sticky: true, days: Number(stickyDays) } }))}>
            {t('ui.torrent.make_sticky')}
          </Button>
          {d.sticky_until && <Button onClick={() => void act(() => api(`/api/p/torrent/v1/admin/torrents/${d.id}/sticky`, { json: { sticky: false } }))}>{t('ui.torrent.unsticky')}</Button>}
        </p>
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

function Lightbox({ src, onClose }: { src: string | null; onClose(): void }) {
  useEffect(() => {
    if (!src) return;
    const k = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    document.addEventListener('keydown', k);
    return () => document.removeEventListener('keydown', k);
  }, [src, onClose]);
  if (!src) return null;
  return (
    <div className="lightbox" role="dialog" aria-modal="true" onClick={onClose}>
      <img src={src} alt="" onClick={(e) => e.stopPropagation()} />
      <button type="button" className="lightbox-close" onClick={onClose} aria-label="关闭">
        <Icon name="close" size={20} />
      </button>
      <span className="lightbox-hint">ESC 退出</span>
    </div>
  );
}

function fmtDuration(secs: number): string {
  if (secs < 3600) return `${Math.floor(secs / 60)}m`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h`;
  return `${Math.floor(secs / 86400)}d`;
}

interface SwarmDetail extends Swarm {
  peers: { user_id: number | null; mine: boolean; seeder: boolean; client: string | null; since: string; last_announce: string; seedbox?: boolean }[];
  mine: { uploaded: number; downloaded: number; seed_secs: number; completed_at: string | null } | null;
}

/** 做种 / 下载状态：人数、我的记录、活跃 peer 列表 */
function SwarmPanel({ id }: { id: number }) {
  const t = useT();
  const q = useQuery({
    queryKey: ['swarm', id],
    queryFn: () => api<SwarmDetail>(`/api/p/accounting/v1/torrents/${id}/swarm`),
    refetchInterval: 30_000,
    retry: false,
  });
  const d = q.data;
  return (
    <Card title={t('ui.torrent.swarm')}>
      <div className="swarm-tiles">
        <div className="tile seed">
          <b>{d?.seeders ?? '–'}</b>
          <span>
            {t('ui.torrent.seeders')} <Hint text={t('ui.help.seeders')} />
          </span>
        </div>
        <div className="tile leech">
          <b>{d?.leechers ?? '–'}</b>
          <span>
            {t('ui.torrent.leechers')} <Hint text={t('ui.help.leechers')} />
          </span>
        </div>
        <div className="tile done">
          <b>{d?.completed ?? '–'}</b>
          <span>
            {t('ui.torrent.completed')} <Hint text={t('ui.help.completed')} />
          </span>
        </div>
      </div>
      {d?.mine && (
        <p className="pt-muted small mine">
          {t('ui.torrent.my_record', { up: formatBytes(d.mine.uploaded), down: formatBytes(d.mine.downloaded), seed: fmtDuration(d.mine.seed_secs) })}
          {d.mine.completed_at ? ` · ${t('ui.torrent.my_done')}` : ''}
        </p>
      )}
      {d && d.peers.length > 0 ? (
        <div className="pt-table-wrap">
          <table className="pt-table peers">
            <thead>
              <tr>
                <th>{t('ui.torrent.peer_role')}</th>
                <th>{t('ui.torrent.peer_client')}</th>
                <th>{t('ui.torrent.peer_since')}</th>
                <th>{t('ui.torrent.peer_last')}</th>
              </tr>
            </thead>
            <tbody>
              {d.peers.slice(0, 30).map((p, i) => (
                <tr key={i} className={p.mine ? 'me' : undefined}>
                  <td>
                    <span className={`pt-badge ${p.seeder ? 'pt-badge-success' : 'pt-badge-primary'}`}>{p.seeder ? t('ui.torrent.role_seed') : t('ui.torrent.role_leech')}</span>
                    {p.mine && <span className="pt-badge">{t('ui.torrent.me')}</span>}
                    {p.seedbox && <span className="pt-badge">{t('ui.torrent.seedbox')}</span>}
                  </td>
                  <td>{p.client ?? '-'}</td>
                  <td className="nowrap">{timeAgo(Date.parse(p.since))}</td>
                  <td className="nowrap">{timeAgo(Date.parse(p.last_announce))}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <p className="pt-muted small">{q.isLoading ? '…' : t('ui.torrent.no_peers')}</p>
      )}
    </Card>
  );
}

export function TorrentPage() {
  const t = useT();
  const { id } = useParams();
  const [zoom, setZoom] = useState<string | null>(null);
  const guide = useFirstTime('pt-dl-guide');
  const tq = useQuery({ queryKey: ['torrent', id], queryFn: () => api<TorrentDetail>(`/api/p/torrent/v1/torrents/${id}`), staleTime: 60_000 });
  const files = useQuery({
    queryKey: ['torrent-files', id],
    queryFn: () => api<{ items: { path: string; size: number }[] | { path: string[]; length: number }[] }>(`/api/p/torrent/v1/torrents/${id}/files`),
    enabled: tq.isSuccess,
  });
  const groupId = tq.data?.group_id;
  const group = useQuery({
    queryKey: ['group', String(groupId)],
    queryFn: () => api<GroupDetail>(`/api/p/torrent/v1/groups/${groupId}`),
    enabled: !!groupId,
    staleTime: 60_000,
  });
  if (tq.isLoading) return <SkeletonRows n={6} />;
  if (tq.error) return <ErrorText error={tq.error} />;
  const d = tq.data as TorrentDetail;
  const ctx = { torrent: d };
  const media = (v: unknown) => (typeof v === 'string' && v.startsWith('/media/') ? v : null);
  const poster = media(d.fields?.poster) ?? media(group.data?.fields?.poster);
  const shots = Array.isArray(d.fields?.screenshots) ? (d.fields.screenshots as unknown[]).map(media).filter((x): x is string => !!x) : [];
  const overview = typeof group.data?.fields?.overview === 'string' ? group.data.fields.overview : null;
  const zhGroup = typeof group.data?.fields?.title_zh === 'string' ? group.data.fields.title_zh : '';
  const rating = typeof group.data?.fields?.rating === 'number' ? group.data.fields.rating : null;
  return (
    <div className="torrent">
      <Lightbox src={zoom} onClose={() => setZoom(null)} />
      <DownloadGuide ctl={guide} />
      {d.can_download && (
        <div className="sticky-dl">
          <a className="pt-btn pt-btn-primary" href={`/api/p/torrent/v1/torrents/${d.id}/download`} download onClick={() => guide.trigger()}>
            <Icon name="download" size={18} /> {t('ui.torrent.download')} · {formatBytes(d.size)}
          </a>
        </div>
      )}
      <section className={`t-head${poster ? ' has-poster' : ''}`}>
        {poster && (
          <button type="button" className="t-poster" onClick={() => setZoom(poster)} aria-label={t('ui.torrent.zoom')}>
            <img src={poster} alt="" />
          </button>
        )}
        <div className="t-head-main">
          <div className="t-badges">
            <TypeBadge typeId={d.type_id} />
            {d.status !== 'approved' && <Badge tone="danger">{statusName(t, d.status)}</Badge>}
            {rating !== null && <span className="pt-badge pt-badge-primary">★ {rating.toFixed(1)}</span>}
            <Slot name="torrent.detail.header" ctx={ctx} />
          </div>
          <h1>{d.title}</h1>
          {(d.subtitle || zhGroup) && <p className="sub">{d.subtitle || zhGroup}</p>}
          <div className="t-meta">
            <span>
              <b>{formatBytes(d.size)}</b> · {d.file_count} {t('ui.torrent.files')}
            </span>
            <span>
              {t('ui.torrent.uploader')}:{' '}
              {d.uploader.anonymous || d.uploader.id === undefined ? (
                t('ui.torrent.anonymous')
              ) : (
                <>
                  <Link to={`/users/${d.uploader.id}`}>{d.uploader.username ?? `#${d.uploader.id}`}</Link>
                  <Slot name="user.name.decorations" ctx={{ user_id: d.uploader.id, size: 'sm' }} className="user-deco" />
                </>
              )}
            </span>
            <span>{new Date(d.created_at).toLocaleString()}</span>
            {d.info_hash_v1 && (
              <span className="infohash-meta">
                <span className="pt-muted">Hash:</span>
                <code className="pt-mono">{d.info_hash_v1.slice(0, 8)}…{d.info_hash_v1.slice(-6)}</code>
                <CopyButton text={d.info_hash_v1} title={t('ui.torrent.copy_hash')} />
              </span>
            )}
          </div>
          {overview && <p className="t-overview">{overview}</p>}
          <div className="t-actions">
            {d.can_download ? (
              <>
                <a className="pt-btn pt-btn-primary btn-lg" href={`/api/p/torrent/v1/torrents/${d.id}/download`} download onClick={() => guide.trigger()}>
                  <Icon name="download" size={18} /> {t('ui.torrent.download')}
                </a>
                <CopyButton
                  variant="default"
                  className="btn-lg"
                  text={typeof window !== 'undefined' ? `${window.location.origin}/api/p/torrent/v1/torrents/${d.id}/download` : `/api/p/torrent/v1/torrents/${d.id}/download`}
                  label={t('ui.torrent.copy_link')}
                />
              </>
            ) : (
              <span className="pt-muted small">{t('ui.torrent.cannot_download')}</span>
            )}
            <Slot name="torrent.detail.actions" ctx={ctx} />
          </div>
          {d.can_download && (
            <p className="pt-muted small t-hint">
              {t('ui.torrent.download_hint')}{' '}
              <button type="button" className="link-btn" onClick={() => guide.show()}>
                {t('ui.help.how_to_download')}
              </button>
            </p>
          )}
        </div>
      </section>
      <div className="with-sidebar reverse">
        <div className="main">
          <SwarmPanel id={d.id} />
          {shots.length > 0 && (
            <Card title={t('ui.torrent.screenshots')}>
              <div className="shots">
                {shots.map((u) => (
                  <button key={u} type="button" onClick={() => setZoom(u)} aria-label={t('ui.torrent.zoom')}>
                    <img src={u} alt="" loading="lazy" />
                  </button>
                ))}
              </div>
            </Card>
          )}
          {d.fields && Object.keys(d.fields).length > 0 && (
            <Card title={t('ui.torrent.info')} actions={d.group_id ? <Link to={`/groups/${d.group_id}`}>{t('ui.group.view')}</Link> : null}>
              <FieldsView typeId={d.type_id} fields={d.fields} />
            </Card>
          )}
          <Slot name="torrent.detail.info" ctx={ctx} />
          {d.description && (
            <Card title={t('ui.torrent.description')}>
              {d.description_html !== undefined ? <UserContent html={d.description_html} /> : <UserContent text={d.description} />}
            </Card>
          )}
          <Card title={`${t('ui.torrent.files')} (${d.file_count})`}>
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

/** 发布名 → 片名 / 年份 / 季号（`The.Last.of.Us.S02E05.1080p…` → The Last of Us、S2） */
export function parseRelease(raw: string): { title: string; year?: number; season?: number } {
  const name = raw.replace(/\.torrent$/i, '').replace(/^\[[^\]]*\]\s*/, '');
  const cut = name.search(/[.\s_](?:19|20)\d{2}(?:[.\s_]|$)|[.\s_]S\d{1,2}(?:E\d+)?(?:[.\s_]|$)|[.\s_](?:Complete|2160p|1080p|1080i|720p|480p)/i);
  const title = (cut > 0 ? name.slice(0, cut) : name).replace(/[._]+/g, ' ').replace(/\s+/g, ' ').trim();
  const y = /[.\s_]((?:19|20)\d{2})(?:[.\s_]|$)/.exec(name);
  const sn = /[.\s_]S(\d{1,2})(?:E\d+)?(?:[.\s_]|$)/i.exec(name);
  return { title, year: y ? Number(y[1]) : undefined, season: sn ? Number(sn[1]) : undefined };
}

interface MetaHit {
  external_id: string;
  title: string | null;
  original: string | null;
  year: number | null;
}

export interface MetaPick {
  hit: MetaHit;
  fields: Record<string, unknown>;
  subtitle: string;
}

/** 上传表单：按发布名自动搜 TMDB（中文名优先），可换选；选中后写入 TMDB ID（上传后自动补全海报、简介、中文名） */
function MetadataMatch({ typeId, name, onPick }: { typeId: string; name: string; onPick(p: MetaPick | null): void }) {
  const t = useT();
  const parsed = useMemo(() => parseRelease(name), [name]);
  const [q, setQ] = useState('');
  const [hits, setHits] = useState<MetaHit[]>([]);
  const [chosen, setChosen] = useState<MetaHit | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const kind = typeId === 'movie' ? 'movie' : 'tv';
  const query = q || parsed.title;

  const pick = (h: MetaHit) => {
    setChosen(h);
    const id = Number(h.external_id.split(':')[1]);
    const fields: Record<string, unknown> = kind === 'movie' ? { tmdb_id: id } : { tmdb_tv_id: id };
    if (kind === 'tv' && parsed.season) fields.season = parsed.season;
    onPick({ hit: h, fields, subtitle: h.title && h.title !== h.original ? h.title : (h.original ?? '') });
  };

  useEffect(() => {
    setChosen(null);
    onPick(null);
    if (query.length < 2) {
      setHits([]);
      return;
    }
    const h = setTimeout(async () => {
      setBusy(true);
      setError(null);
      try {
        const year = parsed.year && kind === 'movie' ? `&year=${parsed.year}` : '';
        const r = await api<{ items: MetaHit[] }>(`/api/p/metadata-tmdb/v1/search?q=${encodeURIComponent(query)}&type=${kind}${year}`);
        setHits(r.items);
        const best = r.items.find((x) => parsed.year && x.year === parsed.year) ?? (r.items.length === 1 ? r.items[0] : undefined);
        if (best) pick(best);
      } catch (e) {
        setError(e);
      } finally {
        setBusy(false);
      }
    }, 600);
    return () => clearTimeout(h);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, kind]);

  return (
    <div className="meta-match">
      <div className="mm-head">
        <b>{t('ui.upload.match_title')}</b>
        {busy && <Spinner />}
        {chosen && (
          <span className="pt-badge pt-badge-success">
            {t('ui.upload.matched')}
          </span>
        )}
      </div>
      <div className="searchbar">
        <Input name="meta_q" placeholder={parsed.title || t('ui.upload.meta_search')} value={q} onChange={(e) => setQ(e.target.value)} />
      </div>
      <ErrorText error={error} />
      <div className="mm-hits">
        {hits.slice(0, 5).map((h) => (
          <button type="button" key={h.external_id} className={`mm-hit${chosen?.external_id === h.external_id ? ' on' : ''}`} onClick={() => pick(h)}>
            <b>{h.title}</b>
            {h.original && h.original !== h.title && <span className="pt-muted"> / {h.original}</span>}
            {h.year && <span className="pt-muted"> ({h.year})</span>}
          </button>
        ))}
        {!busy && hits.length === 0 && query.length >= 2 && <p className="pt-muted small">{t('ui.upload.no_match')}</p>}
      </div>
    </div>
  );
}

/** 图片选择：带缩略图预览与移除；`multiple` 时最多 `max` 张 */
function ImagePicker({ label, hint, files, onChange, max = 1 }: { label: string; hint?: string; files: File[]; onChange(f: File[]): void; max?: number }) {
  const t = useT();
  const urls = useMemo(() => files.map((f) => URL.createObjectURL(f)), [files]);
  useEffect(() => () => urls.forEach((u) => URL.revokeObjectURL(u)), [urls]);
  return (
    <div className="pt-field">
      <span className="pt-field-label">{label}</span>
      <div className="img-picker">
        {urls.map((u, i) => (
          <div key={u} className="img-thumb">
            <img src={u} alt="" />
            <button type="button" aria-label={t('ui.common.remove')} onClick={() => onChange(files.filter((_, j) => j !== i))}>
              ×
            </button>
          </div>
        ))}
        {files.length < max && (
          <label className="img-add">
            <input
              type="file"
              accept="image/jpeg,image/png,image/webp"
              multiple={max > 1}
              onChange={(e) => {
                const picked = [...(e.target.files ?? [])].filter((f) => f.size <= 3 << 20);
                onChange([...files, ...picked].slice(0, max));
                e.target.value = '';
              }}
            />
            <Icon name="upload" size={20} />
            <span>{t('ui.upload.add_image')}</span>
          </label>
        )}
      </div>
      {hint && <span className="pt-field-hint">{hint}</span>}
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
  const [match, setMatch] = useState<MetaPick | null>(null);
  const [poster, setPoster] = useState<File[]>([]);
  const [shots, setShots] = useState<File[]>([]);
  const [error, setError] = useState<unknown>(null);
  const [errors, setErrors] = useState<Errors>({});
  const [busy, setBusy] = useState(false);
  const list = types.data ?? [];
  // 通用类型（other）放最后，默认选第一个具体类型
  const ordered = [...list].sort((x, y) => typeRank(x.type_id) - typeRank(y.type_id));
  const info = ordered.find((x) => x.type_id === typeId) ?? ordered[0];
  const hasTmdb = useInstalled('metadata-tmdb');
  const pick = (id: string) => {
    setTypeId(id);
    const i = list.find((x) => x.type_id === id);
    setFields(i?.field_schema ? ((withDefaults(i.field_schema as Schema, {}) as Record<string, unknown>) ?? {}) : {});
    setErrors({});
  };
  // 插件登记的发布后回调（如 mediainfo 保存粘贴的报告）；失败只提示，不影响跳转
  const afterUpload = useRef<((id: number) => Promise<void> | void)[]>([]);
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
      fd.append('meta', JSON.stringify({ ...meta, type_id: info.type_id, fields: { ...fields, ...(match?.fields ?? {}) }, subtitle: meta.subtitle || undefined }));
      fd.append('torrent', file, file.name);
      for (const f of poster) fd.append('poster', f, f.name);
      for (const f of shots) fd.append('screenshot', f, f.name);
      const r = await parse<{ id: number }>(await apiFetch('/api/p/torrent/v1/torrents', { method: 'POST', body: fd, idempotent: true }));
      await Promise.allSettled(afterUpload.current.map((cb) => Promise.resolve().then(() => cb(r.id))));
      nav(`/torrents/${r.id}`);
    } catch (err) {
      if (err instanceof ApiError) setErrors(serverErrors(err.details));
      setError(err);
    } finally {
      setBusy(false);
    }
  };
  const onFile = (f: File | null) => {
    setFile(f);
    if (!f) return;
    const base = f.name.replace(/\.torrent$/i, '');
    if (!meta.title) setMeta((m) => ({ ...m, title: m.title || base }));
    // 没手动选过类型时按文件名猜：有季 / 集号的是剧集，否则电影
    if (!typeId) {
      const guess = /(?:^|[.\s_])S\d{1,2}(?:E\d+)?(?:[.\s_]|$)|Complete|第.+[季集]/i.test(base) ? 'tv' : 'movie';
      if (list.some((x) => x.type_id === guess)) pick(guess);
    }
  };
  const step = (n: number, title: string) => (
    <h3 className="step-h">
      <span className="step-n">{n}</span>
      {title}
    </h3>
  );
  return (
    <div className="upload">
      <h1 className="page-title">{t('ui.upload.title')}</h1>
      <form onSubmit={submit}>
        <section className="step">
          {step(1, t('ui.upload.step_file'))}
          <label className={`dropzone${file ? ' has-file' : ''}`}>
            <input name="torrent" type="file" accept=".torrent,application/x-bittorrent" onChange={(e) => onFile(e.target.files?.[0] ?? null)} required />
            <Icon name={file ? 'book' : 'upload'} size={26} />
            <b>{file ? file.name : t('ui.upload.file')}</b>
            <span className="pt-muted small">{t('ui.upload.file_hint')}</span>
          </label>
        </section>

        <section className="step">
          {step(2, t('ui.upload.step_info'))}
          <div className="pt-field">
            <span className="pt-field-label">{t('ui.torrent.type')}</span>
            <div className="type-chips type-pick" role="radiogroup">
              {ordered.map((x) => (
                <button key={x.type_id} type="button" role="radio" aria-checked={info?.type_id === x.type_id} onClick={() => pick(x.type_id)}>
                  {t(`term:${x.name_term}`)}
                </button>
              ))}
            </div>
          </div>
          <div className="upload-grid">
            <Field label={t('ui.torrent.title')} hint={info?.plugin ? t('ui.upload.title_hint') : undefined}>
              <Input name="title" value={meta.title} onChange={(e) => setMeta({ ...meta, title: e.target.value })} required={!info?.plugin} maxLength={300} />
            </Field>
            <Field label={t('ui.upload.subtitle')}>
              <Input value={meta.subtitle} onChange={(e) => setMeta({ ...meta, subtitle: e.target.value })} maxLength={300} />
            </Field>
          </div>
          {info && ['movie', 'tv', 'anime'].includes(info.type_id) && hasTmdb && (
            <MetadataMatch
              typeId={info.type_id}
              name={meta.title || file?.name || ''}
              onPick={(p) => {
                setMatch(p);
                if (p && !meta.subtitle) setMeta((m) => ({ ...m, subtitle: m.subtitle || p.subtitle }));
              }}
            />
          )}
          {info && (
            <details className="more-fields" open={Object.keys(errors).length > 0 || info.type_id === 'generic' || undefined}>
              <summary>{t('ui.upload.more_fields')}</summary>
              <Slot
                name="torrent.upload.type_form"
                ctx={{ type_id: info.type_id, schema: info.field_schema, value: fields, onChange: setFields }}
                fallback={<TypeForm info={info} value={fields} onChange={setFields} errors={errors} />}
                empty={<TypeForm info={info} value={fields} onChange={setFields} errors={errors} />}
              />
            </details>
          )}
        </section>

        <section className="step">
          {step(3, t('ui.upload.step_media'))}
          <div className="upload-grid">
            <ImagePicker label={t('ui.upload.poster')} hint={t('ui.upload.image_hint')} files={poster} onChange={setPoster} />
            <ImagePicker label={t('ui.upload.screenshots')} hint={t('ui.upload.image_hint')} files={shots} onChange={setShots} max={6} />
          </div>
        </section>

        <section className="step">
          {step(4, t('ui.upload.step_more'))}
          <Field label={t('ui.torrent.description')}>
            <MarkdownEditor name="description" rows={10} value={meta.description} onChange={(d) => setMeta({ ...meta, description: d })} preview={renderPreview} />
          </Field>
          <label className="pt-check">
            <input type="checkbox" checked={meta.anonymous} onChange={(e) => setMeta({ ...meta, anonymous: e.target.checked })} /> {t('ui.upload.anonymous')}
          </label>
          <Slot
            name="torrent.upload.extra"
            ctx={{
              draft: { ...meta, type_id: info?.type_id, fields },
              // 插件回填标题 / 副标题 / 描述（如豆瓣简介）
              update: (patch: Partial<typeof meta>) => setMeta((m) => ({ ...m, ...patch })),
              // 发布成功后以种子 ID 回调；返回取消登记的函数
              afterUpload: (cb: (id: number) => Promise<void> | void) => {
                afterUpload.current.push(cb);
                return () => {
                  afterUpload.current = afterUpload.current.filter((x) => x !== cb);
                };
              },
            }}
          />
          <ErrorText error={error} />
          <Button type="submit" variant="primary" className="btn-lg submit-btn" disabled={busy || !file || !info}>
            {busy ? <Spinner /> : <Icon name="upload" size={18} />} {t('ui.upload.submit')}
          </Button>
        </section>
      </form>
    </div>
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

/** 种子状态的显示名（术语 torrent.status.<状态>，缺失时显示原值） */
function statusName(t: (k: string, a?: Record<string, unknown>) => string, st: string): string {
  const k = `torrent.status.${st}`;
  const v = t(`term:${k}`);
  return v === k ? st : v;
}

export function Home() {
  const t = useT();
  const s = useSession();
  const m = useManifest();
  if (s.loading) return <SkeletonRows n={4} />;
  if (!s.me) {
    // 私有站点：未登录时只显示欢迎与登录 / 注册入口
    return (
      <div>
        <section className="hero">
          <div>
            <h1>
              {t('ui.home.hero_a')} <span className="gradient-text">{t('ui.home.hero_b')}</span>
            </h1>
            <p className="lead">{t('ui.home.guest_body')}</p>
            <div className="pt-row">
              <Link className="pt-btn pt-btn-primary" to="/login">
                {t('ui.nav.login')}
              </Link>
              <Link className="pt-btn" to="/register">
                {t('ui.nav.register')}
              </Link>
            </div>
          </div>
          <HeroArt className="hero-art" />
        </section>
        <section className="features">
          <div className="feature">
            <div className="ico">
              <Icon name="film" size={22} />
            </div>
            <h3>{t('ui.home.f1_title')}</h3>
            <p>{t('ui.home.f1_body')}</p>
          </div>
          <div className="feature">
            <div className="ico">
              <Icon name="zap" size={22} />
            </div>
            <h3>{t('ui.home.f2_title')}</h3>
            <p>{t('ui.home.f2_body')}</p>
          </div>
          <div className="feature">
            <div className="ico">
              <Icon name="users" size={22} />
            </div>
            <h3>{t('ui.home.f3_title')}</h3>
            <p>{t('ui.home.f3_body', { site: m.site.name })}</p>
          </div>
        </section>
      </div>
    );
  }
  return <HomeFeed />;
}

function HomeFeed() {
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
  const me = useSession().me;
  return (
    <div>
      <section className="greeting">
        <span className="mascot">
          <Logo size={48} />
        </span>
        <div>
          <h1>{t('ui.home.welcome', { name: me?.username ?? '' })}</h1>
          <p>{t('ui.home.welcome_sub')}</p>
        </div>
      </section>
      <Slot name="home.announcements" ctx={{}} />
      <GettingStarted />
      <div className="with-sidebar reverse">
        <div className="main">
          <Slot name="home.sections" ctx={{ user: me }} />
          <Card className="list-card" title={t('ui.home.latest')} actions={<Link to="/torrents">{t('ui.home.view_all')} →</Link>}>
            {res.isLoading ? <SkeletonRows n={6} /> : res.error ? <ErrorText error={res.error} /> : <TorrentTable rows={res.data?.hits.map((h) => h.document) ?? []} />}
          </Card>
        </div>
        <aside className="sidebar">
          <Slot name="home.widgets" ctx={{}} />
        </aside>
      </div>
    </div>
  );
}
