// 作品页：海报、评分、简介、字段，以及该作品的全部版本（按分辨率分组）；插件页签经 group.detail.tabs 追加。
import { useQuery } from '@tanstack/react-query';
import { Suspense, useState } from 'react';
import { Link, useParams } from 'react-router';
import { api } from '@pt-studio/sdk';
import { Slot, SlotEntryView, useSlotEntries, useSlotEntryTitle } from '@pt-studio/slots';
import { useT } from '@pt-studio/terms';
import { Cover, Panel, SkeletonRows, Tabs, Tag } from '@pt-studio/ui-kit';
import { ErrorText } from '../errors';
import { FieldsView, TorrentRows, TypeMark, mediaUrl, useTypeName, type TorrentDoc } from '../torrent';

interface GroupDetail {
  id: number;
  type_id: string;
  title: string;
  year: number | null;
  fields: Record<string, unknown>;
  torrent_count: number;
  torrents: { id: number; title: string; subtitle: string | null; size: number; fields: Record<string, unknown>; created_at: string }[];
}

function resolution(f: Record<string, unknown>): string {
  for (const k of Object.keys(f)) if (k.endsWith('resolution') && typeof f[k] === 'string') return f[k] as string;
  return '';
}

function Versions({ g }: { g: GroupDetail }) {
  const t = useT();
  const rows: TorrentDoc[] = g.torrents.map((x) => ({ ...x, type_id: g.type_id, created_at: Date.parse(x.created_at) / 1000, rel_resolution: resolution(x.fields) || null }));
  const groups = new Map<string, TorrentDoc[]>();
  for (const r of rows) {
    const k = (r.rel_resolution as string) || t('ui.group.other');
    groups.set(k, [...(groups.get(k) ?? []), r]);
  }
  return (
    <>
      {[...groups].map(([k, xs]) => (
        <div key={k} className="ed-group">
          <header>
            <b>{k}</b>
            <span className="small muted">{t('ui.group.n_versions', { n: xs.length })}</span>
          </header>
          <TorrentRows rows={xs} compact />
        </div>
      ))}
    </>
  );
}

function GroupTabs({ g }: { g: GroupDetail }) {
  const t = useT();
  const entries = useSlotEntries('group.detail.tabs');
  const title = useSlotEntryTitle();
  const [tab, setTab] = useState('versions');
  const cur = entries.find((e) => `p:${e.plugin}` === tab);
  return (
    <Panel flush primary>
      <Tabs
        tabs={[{ id: 'versions', label: t('ui.group.torrents', { n: g.torrents.length }) }, ...entries.map((e) => ({ id: `p:${e.plugin}`, label: title(e) }))]}
        value={tab}
        onChange={setTab}
      />
      {tab === 'versions' ? <Versions g={g} /> : cur && <div className="tabp"><SlotEntryView entry={cur} ctx={{ group: g }} /></div>}
    </Panel>
  );
}

export function GroupPage() {
  const t = useT();
  const { id } = useParams();
  const typeName = useTypeName();
  const q = useQuery({ queryKey: ['group', id], queryFn: () => api<GroupDetail>(`/api/p/torrent/v1/groups/${id}`), staleTime: 60_000 });
  if (q.isLoading) return <SkeletonRows n={6} />;
  if (q.error) return <ErrorText error={q.error} />;
  const g = q.data as GroupDetail;
  const f = g.fields ?? {};
  const poster = mediaUrl(f.poster);
  const zh = typeof f.title_zh === 'string' ? f.title_zh : '';
  const overview = typeof f.overview === 'string' ? f.overview : null;
  const rating = typeof f.rating === 'number' ? f.rating : null;
  const genres = Array.isArray(f.genres) ? (f.genres as string[]) : [];
  return (
    <div className="group stack">
      <nav className="crumb" aria-label="breadcrumb">
        <Link to="/torrents">{t('ui.nav.torrents')}</Link>
        <Link to={`/torrents?f.type_id=${g.type_id}`}>{typeName(g.type_id)}</Link>
        <span>{t('ui.group.work')}</span>
      </nav>
      <section className="panel group-hero">
        <Cover src={poster} title={zh || g.title} sub={zh ? g.title : null} rating={rating} />
        <div className="dt-main">
          <div className="row">
            <TypeMark typeId={g.type_id} />
            {genres.map((x) => (
              <Tag key={x}>{x}</Tag>
            ))}
          </div>
          <h1>
            {zh || g.title}
            <small>
              {zh ? g.title : ''} {g.year ? `· ${g.year}` : ''}
            </small>
          </h1>
          {rating !== null && (
            <div className="scores">
              <div className="score">
                <b>{rating.toFixed(1)}</b>
                <span>TMDB</span>
              </div>
            </div>
          )}
          {overview && <p className="overview">{overview}</p>}
          <Slot name="group.detail.header" ctx={{ group: g }} className="row" />
          <FieldsView typeId={g.type_id} fields={f} />
          <div className="row">
            <Link className="btn btn-sm" to="/upload">
              {t('ui.group.upload_version')}
            </Link>
            {typeof f.tmdb_id === 'number' && (
              <a className="small muted" href={`https://www.themoviedb.org/movie/${f.tmdb_id}`} target="_blank" rel="noopener noreferrer nofollow">
                {t('ui.group.attribution')} TMDB
              </a>
            )}
          </div>
        </div>
      </section>
      <Suspense fallback={<SkeletonRows n={4} />}>
        <GroupTabs g={g} />
      </Suspense>
    </div>
  );
}
