// 种子公共组件：密集列表行、海报墙、类型标记、批量做种人数、字段展示。
import { useQuery } from '@tanstack/react-query';
import { useState, type ReactNode } from 'react';
import { Link } from 'react-router';
import { api } from '@pt-studio/sdk';
import { Slot, useSlotColumns } from '@pt-studio/slots';
import { formatBytes, useT } from '@pt-studio/terms';
import { Button, Cover, CopyButton, Empty, Tag, cx } from '@pt-studio/ui-kit';
import { age, typeColor } from './lib';
import { fieldSchemas, useTypes } from './types';

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
  rel_hdr?: string[] | null;
  rel_group?: string | null;
  sticky?: number;
  poster?: string | null;
  uploader_id?: number;
  [k: string]: unknown;
}

export interface Swarm {
  seeders: number;
  leechers: number;
  completed: number;
}

const hasCjk = (x: unknown): x is string => typeof x === 'string' && /[㐀-鿿]/.test(x);

/** 本地化（中文）名：副标题优先，其次元数据补全的中文名 */
export function zhName(r: TorrentDoc): string | null {
  if (r.subtitle) return r.subtitle;
  for (const k of ['tv_title_zh', 'anime_title_zh', 'movie_title_zh', 'movie_title', 'title_zh']) {
    if (hasCjk(r[k])) return r[k] as string;
  }
  return null;
}

export const mediaUrl = (v: unknown): string | null => (typeof v === 'string' && v.startsWith('/media/') ? v : null);

/** 列表页批量取做种 / 下载 / 完成人数 */
export function useSwarmBatch(ids: number[]) {
  const key = ids.join(',');
  return useQuery({
    queryKey: ['swarm-batch', key],
    queryFn: () => api<{ items: Record<string, Swarm> }>(`/api/p/accounting/v1/torrents/swarm?ids=${key}`),
    enabled: ids.length > 0,
    staleTime: 20_000,
    retry: false,
  }).data?.items;
}

export function useTypeName(): (id?: string) => string {
  const t = useT();
  const types = useTypes().data;
  return (id) => {
    const info = types?.find((x) => x.type_id === id);
    return info ? t(`term:${info.name_term}`) : (id ?? '-');
  };
}

/** 类型标记：文字 + 识别色下划线（不用图标） */
export function TypeMark({ typeId }: { typeId?: string }) {
  const name = useTypeName();
  return (
    <span className="cat-chip" style={{ ['--c' as string]: typeColor(typeId) }}>
      {name(typeId)}
    </span>
  );
}

export function seedCls(n: number): string {
  return n === 0 ? 'dead' : n <= 3 ? 'thin' : 'up';
}

/** 规格标签：分辨率、来源、编码、HDR */
export function specTags(r: TorrentDoc): string[] {
  const xs = [r.rel_resolution, r.rel_source, r.rel_video_codec, ...(Array.isArray(r.rel_hdr) ? r.rel_hdr : [])];
  return xs.filter((x): x is string => typeof x === 'string' && x !== '').map((x) => x.replace(/_/g, '-'));
}

export type SortKey = 'newest' | 'largest' | 'relevance' | 'seeders' | 'completed';

/**
 * 种子密集列表：类型 · 标题（发布名、中文名、优惠印记、规格标签）· 存活 · 体积 · 做种 · 下载 · 完成。
 * 鼠标停在标题上显示封面浮层；窄屏变为两列（标题 / 体积与人数）。插件列经 `torrent.list.columns` 追加。
 */
export function TorrentRows({
  rows,
  highlights,
  compact,
  sort,
  onSort,
  empty,
}: {
  rows: TorrentDoc[];
  highlights?: Map<number, Record<string, string>>;
  compact?: boolean;
  sort?: SortKey;
  onSort?(k: SortKey): void;
  empty?: ReactNode;
}) {
  const t = useT();
  const swarm = useSwarmBatch(rows.map((r) => r.id));
  const pluginCols = useSlotColumns('torrent.list.columns');
  const [peek, setPeek] = useState<{ r: TorrentDoc; x: number; y: number } | null>(null);
  if (rows.length === 0) return <Empty>{empty ?? t('ui.torrent.no_results')}</Empty>;
  const head = (k: SortKey | null, label: string, cls?: string) => (
    <div className={cx('r', cls)}>
      {k && onSort ? (
        <button type="button" aria-sort={sort === k ? 'descending' : undefined} onClick={() => onSort(k)}>
          {label}
        </button>
      ) : (
        label
      )}
    </div>
  );
  // 列宽经 CSS 变量传入（UI-WEB R6）：插件列插在标题之后
  const grid = ['58px', 'minmax(0, 1fr)', ...pluginCols.map(() => '84px'), ...(compact ? [] : ['52px']), '88px', '54px', '54px', '54px'].join(' ');
  return (
    <div className={cx('tlist', compact && 'compact')} role="table" aria-label={t('ui.torrent.list')} style={{ ['--cols' as string]: grid }}>
      <div className="trow thead" role="row">
        <div className="c-cat">{t('ui.torrent.type')}</div>
        <div>{t('ui.torrent.title')}</div>
        {pluginCols.map((c) => (
          <div key={c.key} className="c-hide">
            {c.header}
          </div>
        ))}
        {!compact && head('newest', t('ui.torrent.age'), 'c-age')}
        {head('largest', t('ui.torrent.size'), 'c-hide')}
        {head(onSort ? 'seeders' : null, t('ui.torrent.seeders'), 'c-hide')}
        {head(null, t('ui.torrent.leechers'), 'c-hide')}
        {head(onSort ? 'completed' : null, t('ui.torrent.completed'), 'c-hide')}
      </div>
      {rows.map((r) => {
        const s = swarm?.[String(r.id)];
        const zh = zhName(r);
        const hl = highlights?.get(r.id)?.title;
        return (
          <div className="trow" role="row" key={r.id}>
            <div className="c-cat">
              <TypeMark typeId={r.type_id} />
            </div>
            <div className="tt">
              <div className="tt-1">
                {r.sticky === 1 && <span className="sticky-flag">{t('ui.torrent.sticky')}</span>}
                <Link
                  className="tt-name"
                  to={`/torrents/${r.id}`}
                  onMouseEnter={(e) => {
                    const b = (e.currentTarget as HTMLElement).getBoundingClientRect();
                    setPeek({ r, x: b.left, y: b.bottom });
                  }}
                  onMouseLeave={() => setPeek(null)}
                >
                  {hl ? <Highlight text={hl} /> : r.title}
                </Link>
              </div>
              <div className="tt-2">
                <Slot name="torrent.list.row_badges" ctx={{ torrent: r }} className="row-badges" />
                {zh && <span className="subt">{zh}</span>}
                {specTags(r).map((x) => (
                  <Tag key={x}>{x}</Tag>
                ))}
              </div>
            </div>
            {pluginCols.map((c) => (
              <div key={c.key} className="c-hide c-plugin">
                {c.render({ torrent: r })}
              </div>
            ))}
            {!compact && <div className="c-age">{r.created_at ? age(r.created_at * 1000) : '-'}</div>}
            <div className="c-num num c-hide">{formatBytes(r.size)}</div>
            <div className={cx('c-num num c-hide c-seed', s && seedCls(s.seeders))}>{s?.seeders ?? '–'}</div>
            <div className="c-num num c-hide down">{s?.leechers ?? '–'}</div>
            <div className="c-num num c-hide">{s?.completed ?? '–'}</div>
            <div className="c-mob">
              <span className="num">{formatBytes(r.size)}</span>
              <span className="num">
                <span className={s ? seedCls(s.seeders) : undefined}>↑{s?.seeders ?? '–'}</span> <span className="down">↓{s?.leechers ?? '–'}</span>
              </span>
            </div>
            <div className="row-acts">
              <a className="btn btn-xs btn-primary" href={`/api/p/torrent/v1/torrents/${r.id}/download`} download>
                {t('ui.torrent.download_short')}
              </a>
              <CopyButton size="xs" text={`${location.origin}/api/p/torrent/v1/torrents/${r.id}/download`} label={t('ui.torrent.copy_link_short')} />
            </div>
          </div>
        );
      })}
      {peek && <Peek r={peek.r} x={peek.x} y={peek.y} s={swarm?.[String(peek.r.id)]} />}
    </div>
  );
}

function Peek({ r, x, y, s }: { r: TorrentDoc; x: number; y: number; s?: Swarm }) {
  const zh = zhName(r);
  const above = y + 220 > window.innerHeight;
  return (
    <div className="peek" style={{ left: Math.max(8, Math.min(x, window.innerWidth - 316)), top: above ? y - 210 : y + 8 }} aria-hidden="true">
      <Cover src={mediaUrl(r.poster)} title={zh ?? r.title} sub={zh ? r.title : null} />
      <div className="peek-b">
        <b>{zh ?? r.title}</b>
        {zh && <span className="muted clip2">{r.title}</span>}
        <span className="num muted">
          {formatBytes(r.size)}
          {s && ` · ↑${s.seeders} ↓${s.leechers}`}
        </span>
      </div>
    </div>
  );
}

/** 海报墙 */
export function TorrentWall({ rows }: { rows: TorrentDoc[] }) {
  const swarm = useSwarmBatch(rows.map((r) => r.id));
  return (
    <div className="wall">
      {rows.map((r) => {
        const zh = zhName(r);
        const s = swarm?.[String(r.id)];
        return (
          <Link key={r.id} to={`/torrents/${r.id}`}>
            <Cover src={mediaUrl(r.poster)} title={zh ?? r.title} sub={zh ? r.title : null} />
            <span className="nm">{zh ?? r.title}</span>
            <span className="mt">
              <span className="num">{formatBytes(r.size)}</span>
              {s && <span className={cx('num', seedCls(s.seeders))}>↑{s.seeders}</span>}
            </span>
          </Link>
        );
      })}
    </div>
  );
}

/** 高亮：U+E000 / U+E001 包围的片段渲染为 <mark>，其余按纯文本 */
export function Highlight({ text }: { text: string }) {
  const parts = text.split(/([^]*)/u);
  return (
    <>
      {parts.map((p, i) => (p.startsWith('') ? <mark key={i}>{p.slice(1, -1)}</mark> : <span key={i}>{p}</span>))}
    </>
  );
}

/** 由元数据面板展示的字段，不在通用字段表中重复 */
const HIDDEN_FIELDS = ['release_name', 'poster', 'overview', 'group_title', 'edition_key', 'logs', 'cover', 'description', 'screenshots', 'title_zh', 'rating', 'genres'];

/** 类型字段的通用展示（显示名来自 schema 的术语或 title；枚举值按术语渲染） */
export function FieldsView({ typeId, fields }: { typeId: string; fields: Record<string, unknown> }) {
  const t = useT();
  const types = useTypes();
  const props = fieldSchemas(types.data?.find((x) => x.type_id === typeId), fields);
  const rows = Object.entries(fields).filter(([k, v]) => {
    if (HIDDEN_FIELDS.includes(k) || v === null || v === undefined || v === '') return false;
    if (Array.isArray(v)) return v.length > 0 && v.every((x) => typeof x !== 'object');
    return typeof v !== 'object';
  });
  if (rows.length === 0) return null;
  const label = (k: string) => {
    const s = props[k];
    const term = s?.['x-pt-label-term'];
    const v = term ? t(`term:${term}`) : null;
    return v && v !== term ? v : (s?.title ?? k);
  };
  const value = (k: string, x: unknown) => {
    const term = props[k]?.['x-pt-label-term'];
    if (!term) return String(x);
    const key = `${term}.${String(x)}`;
    const v = t(`term:${key}`);
    return v && v !== key ? v : String(x);
  };
  return (
    <dl className="kv fields">
      {rows.map(([k, v]) => (
        <div key={k} className="kv-row">
          <dt>{label(k)}</dt>
          <dd>{Array.isArray(v) ? v.map((x) => value(k, x)).join(' · ') : typeof v === 'boolean' ? (v ? '✓' : '✗') : value(k, v)}</dd>
        </div>
      ))}
    </dl>
  );
}

/** 种子状态的显示名（术语 torrent.status.<状态>，缺失时显示原值） */
export function statusName(t: (k: string, a?: Record<string, unknown>) => string, st: string): string {
  const k = `torrent.status.${st}`;
  const v = t(`term:${k}`);
  return v === k ? st : v;
}

export function DownloadButton({ id, size, onClick, label }: { id: number; size?: number; onClick?(): void; label?: ReactNode }) {
  const t = useT();
  return (
    <a className="btn btn-primary" href={`/api/p/torrent/v1/torrents/${id}/download`} download onClick={onClick}>
      {label ?? t('ui.torrent.download')}
      {size !== undefined && <span className="btn-sub">{formatBytes(size)}</span>}
    </a>
  );
}

export { Button };
