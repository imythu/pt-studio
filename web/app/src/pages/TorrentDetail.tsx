// 种子详情：卷首（封面、标题、发布名、元数据、操作）、关键信息条（P3）、页签（简介 / 文件 / 同伴 / 插件页签 / 管理）、
// 侧栏（健康度、我的记录、插件侧栏、同作品版本）；手机端下载固定在底部（P5），其余操作进「更多」抽屉。
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { Suspense, useEffect, useState, type FormEvent, type ReactNode } from 'react';
import { Link, useNavigate, useParams } from 'react-router';
import { api, renderPreview } from '@pt-studio/sdk';
import { Slot, SlotEntryView, useSlotEntries, useSlotEntryTitle } from '@pt-studio/slots';
import { formatBytes, useT } from '@pt-studio/terms';
import {
  Button,
  CopyButton,
  Cover,
  Empty,
  Field,
  Input,
  KeyFacts,
  MarkdownEditor,
  MobileBar,
  Panel,
  Portal,
  Select,
  Sheet,
  SkeletonRows,
  Status,
  Tabs,
  Tag,
  UserContent,
  useToast,
} from '@pt-studio/ui-kit';
import { ErrorText, useErrorText } from '../errors';
import { DownloadGuide, useFirstTime } from '../help';
import { age, clientName, timeAgo } from '../lib';
import { useSession } from '../session';
import { FieldsView, TypeMark, mediaUrl, seedCls, statusName, useTypeName, type Swarm } from '../torrent';

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

interface GroupDetail {
  id: number;
  type_id: string;
  title: string;
  year: number | null;
  fields: Record<string, unknown>;
  torrents: { id: number; title: string; subtitle: string | null; size: number; fields: Record<string, unknown>; created_at: string }[];
}

interface SwarmDetail extends Swarm {
  peers: { user_id: number | null; username?: string | null; mine: boolean; seeder: boolean; client: string | null; since: string; last_announce: string; seedbox?: boolean; ipv4?: string | null; ipv6?: string | null }[];
  mine: { uploaded: number; downloaded: number; seed_secs: number; completed_at: string | null } | null;
}

function fmtDur(secs: number): string {
  if (secs < 3600) return `${Math.floor(secs / 60)}m`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}h`;
  return `${Math.floor(secs / 86400)}d`;
}

const dlUrl = (id: number) => `/api/p/torrent/v1/torrents/${id}/download`;

function Lightbox({ src, onClose }: { src: string | null; onClose(): void }) {
  useEffect(() => {
    if (!src) return;
    const k = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    document.addEventListener('keydown', k);
    return () => document.removeEventListener('keydown', k);
  }, [src, onClose]);
  if (!src) return null;
  return (
    <Portal>
      <div className="lightbox" role="dialog" aria-modal="true" onClick={onClose}>
        <img src={src} alt="" />
        <span className="lightbox-hint">Esc</span>
      </div>
    </Portal>
  );
}

/** 编辑（本人或 torrent.update）与审核（torrent.update / torrent.delete） */
function Manage({ d }: { d: TorrentDetail }) {
  const t = useT();
  const s = useSession();
  const qc = useQueryClient();
  const toast = useToast();
  const errText = useErrorText();
  const nav = useNavigate();
  const [edit, setEdit] = useState<{ title: string; subtitle: string; description: string } | null>(null);
  const [status, setStatus] = useState({ to: d.status, reason: '' });
  const [stickyDays, setStickyDays] = useState('7');
  const [confirmDel, setConfirmDel] = useState(false);
  const owner = s.me?.id === d.uploader.id;
  const canEdit = owner || s.can('torrent.update');
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
    <div className="stack">
      {canEdit && (
        <section className="stack-s">
          <div className="row-sb">
            <h3>{t('ui.torrent.edit')}</h3>
            <Button size="sm" onClick={() => setEdit({ title: d.title, subtitle: d.subtitle ?? '', description: d.description })}>
              {t('ui.torrent.edit')}
            </Button>
          </div>
          <Sheet
            open={!!edit}
            onClose={() => setEdit(null)}
            title={t('ui.torrent.edit')}
            width={640}
            footer={
              <>
                <Button onClick={() => setEdit(null)}>{t('ui.common.cancel')}</Button>
                <Button variant="primary" type="submit" form="edit-torrent">
                  {t('ui.admin.save')}
                </Button>
              </>
            }
          >
            {edit && (
              <form
                id="edit-torrent"
                className="form"
                onSubmit={(e: FormEvent) => {
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
                  <MarkdownEditor name="edit_description" rows={10} value={edit.description} onChange={(v) => setEdit({ ...edit, description: v })} preview={renderPreview} />
                </Field>
              </form>
            )}
          </Sheet>
        </section>
      )}
      {s.can('torrent.update') && (
        <form
          className="stack-s"
          onSubmit={(e) => {
            e.preventDefault();
            void act(() => api(`/api/p/torrent/v1/admin/torrents/${d.id}/status`, { json: { to: status.to, reason: status.reason || undefined } }));
          }}
        >
          <h3>{t('ui.torrent.set_status')}</h3>
          <div className="row">
            <Select name="status" value={status.to} onChange={(e) => setStatus({ ...status, to: e.target.value })} style={{ width: 'auto' }} options={['candidate', 'pending', 'approved', 'rejected', 'disabled'].map((x) => ({ value: x, label: statusName(t, x) }))} />
            <Input placeholder={t('ui.admin.reason')} value={status.reason} maxLength={500} onChange={(e) => setStatus({ ...status, reason: e.target.value })} style={{ flex: 1, minWidth: 160 }} />
            <Button type="submit">{t('ui.torrent.set_status')}</Button>
          </div>
        </form>
      )}
      {s.can('torrent.update') && (
        <section className="stack-s">
          <h3>{t('ui.torrent.sticky')}</h3>
          {d.sticky_until && <span className="small muted">{t('ui.torrent.sticky_until', { at: d.sticky_until.startsWith('9999') ? t('ui.torrent.sticky_forever') : new Date(d.sticky_until).toLocaleString() })}</span>}
          <div className="row">
            <Select name="sticky_days" value={stickyDays} onChange={(e) => setStickyDays(e.target.value)} style={{ width: 'auto' }} options={['1', '3', '7', '30', '0'].map((x) => ({ value: x, label: x === '0' ? t('ui.torrent.sticky_forever') : t('ui.torrent.sticky_days', { n: x }) }))} />
            <Button onClick={() => void act(() => api(`/api/p/torrent/v1/admin/torrents/${d.id}/sticky`, { json: { sticky: true, days: Number(stickyDays) } }))}>{t('ui.torrent.make_sticky')}</Button>
            {d.sticky_until && <Button onClick={() => void act(() => api(`/api/p/torrent/v1/admin/torrents/${d.id}/sticky`, { json: { sticky: false } }))}>{t('ui.torrent.unsticky')}</Button>}
          </div>
        </section>
      )}
      {s.isStaff && <Slot name="admin.torrent.tabs" ctx={{ torrent: d }} />}
      {s.can('torrent.delete') && (
        <section className="row">
          {confirmDel ? (
            <>
              <span className="small down">{t('ui.torrent.delete_confirm')}</span>
              <Button
                variant="danger"
                onClick={() =>
                  void act(async () => {
                    await api(`/api/p/torrent/v1/admin/torrents/${d.id}?reason=admin`, { method: 'DELETE' });
                    nav('/torrents');
                  })
                }
              >
                {t('ui.torrent.delete')}
              </Button>
              <Button variant="ghost" onClick={() => setConfirmDel(false)}>
                {t('ui.common.cancel')}
              </Button>
            </>
          ) : (
            <Button variant="danger" onClick={() => setConfirmDel(true)}>
              {t('ui.torrent.delete')}
            </Button>
          )}
        </section>
      )}
    </div>
  );
}

function Peers({ d }: { d?: SwarmDetail }) {
  const t = useT();
  if (!d) return <SkeletonRows n={3} />;
  if (d.peers.length === 0) return <Empty compact hint={t('ui.torrent.no_peers')}>{t('ui.torrent.no_peers_title')}</Empty>;
  // 管理员视角：用户名（accounting.stats.read）与 IPv4 / IPv6（accounting.peers.ip，tracker 现读）
  const users = d.peers.some((p) => p.username);
  const ips = d.peers.some((p) => 'ipv4' in p || 'ipv6' in p);
  return (
    <div className="dtw">
      <table className={users || ips ? 'dt min820' : 'dt min640'}>
        <thead>
          <tr>
            <th>{t('ui.torrent.peer_role')}</th>
            {users && <th>{t('ui.torrent.peer_user')}</th>}
            {ips && <th>{t('ui.torrent.peer_ip')}</th>}
            <th>{t('ui.torrent.peer_client')}</th>
            <th>{t('ui.torrent.peer_since')}</th>
            <th>{t('ui.torrent.peer_last')}</th>
          </tr>
        </thead>
        <tbody>
          {d.peers.slice(0, 50).map((p, i) => (
            <tr key={i} data-selected={p.mine ? 'true' : undefined}>
              <td>
                <span className="row" style={{ gap: 4 }}>
                  <Status tone={p.seeder ? 'ok' : 'info'}>{p.seeder ? t('ui.torrent.role_seed') : t('ui.torrent.role_leech')}</Status>
                  {p.mine && <Tag tone="o">{t('ui.torrent.me')}</Tag>}
                  {p.seedbox && <Tag>{t('ui.torrent.seedbox')}</Tag>}
                </span>
              </td>
              {users && <td>{p.user_id && p.username ? <Link to={`/users/${p.user_id}`}>{p.username}</Link> : '-'}</td>}
              {ips && (
                <td className="mono small peer-ip">
                  {p.ipv4 && <span>{p.ipv4}</span>}
                  {p.ipv6 && <span>{p.ipv6}</span>}
                  {!p.ipv4 && !p.ipv6 && '-'}
                </td>
              )}
              <td className="nowrap" title={p.client ?? undefined}>{clientName(p.client) ?? '-'}</td>
              <td className="nowrap small">{timeAgo(Date.parse(p.since))}</td>
              <td className="nowrap small">{timeAgo(Date.parse(p.last_announce))}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function Files({ id, count }: { id: number; count: number }) {
  const t = useT();
  const files = useQuery({
    queryKey: ['torrent-files', String(id)],
    queryFn: () => api<{ items: ({ path: string; size: number } | { path: string[]; length: number })[] }>(`/api/p/torrent/v1/torrents/${id}/files`),
  });
  if (files.isLoading) return <SkeletonRows n={3} />;
  const items = files.data?.items ?? [];
  return (
    <div className="dtw">
      <table className="files">
        <tbody>
          {items.slice(0, 500).map((f, i) => {
            const any = f as { path: string | string[]; size?: number; length?: number };
            const path = Array.isArray(any.path) ? any.path.join('/') : any.path;
            return (
              <tr key={i}>
                <td>{path}</td>
                <td>{formatBytes(any.size ?? any.length ?? 0)}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {count > items.length && <p className="small muted pad">{t('ui.torrent.files_more', { n: count - items.length })}</p>}
    </div>
  );
}

/** 页签：宿主页签 + 插件 `torrent.detail.tabs`（并入同一页签栏） */
function DetailTabs({ d, ctx, intro, swarm, manage }: { d: TorrentDetail; ctx: unknown; intro: ReactNode; swarm?: SwarmDetail; manage: boolean }) {
  const t = useT();
  const entries = useSlotEntries('torrent.detail.tabs');
  const title = useSlotEntryTitle();
  const [tab, setTab] = useState('intro');
  const tabs = [
    { id: 'intro', label: t('ui.torrent.intro') },
    { id: 'files', label: t('ui.torrent.files'), count: d.file_count },
    { id: 'peers', label: t('ui.torrent.peers'), count: swarm ? swarm.seeders + swarm.leechers : undefined },
    ...entries.map((e) => ({ id: `p:${e.plugin}`, label: title(e) })),
    ...(manage ? [{ id: 'manage', label: t('ui.torrent.manage') }] : []),
  ];
  const cur = entries.find((e) => `p:${e.plugin}` === tab);
  return (
    <Panel flush className="dt-tabs">
      <Tabs tabs={tabs} value={tab} onChange={setTab} label={t('ui.torrent.sections')} />
      <div className="tabp" role="tabpanel">
        {tab === 'intro' && intro}
        {tab === 'files' && <Files id={d.id} count={d.file_count} />}
        {tab === 'peers' && <Peers d={swarm} />}
        {cur && <SlotEntryView entry={cur} ctx={ctx} />}
        {tab === 'manage' && <Manage d={d} />}
      </div>
    </Panel>
  );
}

export function TorrentPage() {
  const t = useT();
  const { id } = useParams();
  const s = useSession();
  const typeName = useTypeName();
  const [zoom, setZoom] = useState<string | null>(null);
  const [more, setMore] = useState(false);
  const guide = useFirstTime('pt-dl-guide');
  const tq = useQuery({ queryKey: ['torrent', id], queryFn: () => api<TorrentDetail>(`/api/p/torrent/v1/torrents/${id}`), staleTime: 60_000 });
  const groupId = tq.data?.group_id;
  const group = useQuery({ queryKey: ['group', String(groupId)], queryFn: () => api<GroupDetail>(`/api/p/torrent/v1/groups/${groupId}`), enabled: !!groupId, staleTime: 60_000 });
  const sw = useQuery({ queryKey: ['swarm', id], queryFn: () => api<SwarmDetail>(`/api/p/accounting/v1/torrents/${id}/swarm`), refetchInterval: 30_000, retry: false, enabled: tq.isSuccess });
  if (tq.isLoading) return <SkeletonRows n={6} />;
  if (tq.error) return <ErrorText error={tq.error} />;
  const d = tq.data as TorrentDetail;
  const ctx = { torrent: d };
  const g = group.data;
  const poster = mediaUrl(d.fields?.poster) ?? mediaUrl(g?.fields?.poster);
  const shots = Array.isArray(d.fields?.screenshots) ? (d.fields.screenshots as unknown[]).map(mediaUrl).filter((x): x is string => !!x) : [];
  const overview = typeof g?.fields?.overview === 'string' ? g.fields.overview : null;
  const zhGroup = typeof g?.fields?.title_zh === 'string' ? g.fields.title_zh : '';
  const rating = typeof g?.fields?.rating === 'number' ? g.fields.rating : null;
  const genres = Array.isArray(g?.fields?.genres) ? (g?.fields?.genres as string[]) : [];
  const zh = d.subtitle || zhGroup;
  const swarm = sw.data;
  const mine = swarm?.mine;
  const canManage = s.me?.id === d.uploader.id || s.can('torrent.update') || s.can('torrent.delete') || s.isStaff;
  const created = Date.parse(d.created_at);
  const uploader =
    d.uploader.anonymous || d.uploader.id === undefined ? (
      <span>{t('ui.torrent.anonymous')}</span>
    ) : (
      <span className="who">
        <Link className="u" to={`/users/${d.uploader.id}`}>
          {d.uploader.username ?? `#${d.uploader.id}`}
        </Link>
        <Slot name="user.name.decorations" ctx={{ user_id: d.uploader.id, size: 'sm' }} className="user-deco" />
      </span>
    );
  const actions = (
    <>
      <Slot name="torrent.detail.actions" ctx={ctx} className="row slot-acts" />
    </>
  );
  const intro = (
    <div className="stack">
      {overview && <p className="overview">{overview}</p>}
      {d.fields && Object.keys(d.fields).length > 0 && (
        <section className="stack-s">
          <div className="row-sb">
            <h3>{t('ui.torrent.info')}</h3>
            {d.group_id && <Link className="link small" to={`/groups/${d.group_id}`}>{t('ui.group.view')}</Link>}
          </div>
          <FieldsView typeId={d.type_id} fields={d.fields} />
        </section>
      )}
      <Slot name="torrent.detail.info" ctx={ctx} className="stack" />
      {shots.length > 0 && (
        <section className="stack-s">
          <h3>{t('ui.torrent.screenshots')}</h3>
          <div className="shots">
            {shots.map((u) => (
              <button key={u} type="button" onClick={() => setZoom(u)} aria-label={t('ui.torrent.zoom')}>
                <img src={u} alt="" loading="lazy" />
              </button>
            ))}
          </div>
        </section>
      )}
      {d.description && (
        <section className="stack-s">
          <h3>{t('ui.torrent.description')}</h3>
          {d.description_html !== undefined ? <UserContent html={d.description_html} /> : <UserContent text={d.description} />}
        </section>
      )}
    </div>
  );
  return (
    <div className="torrent">
      <Lightbox src={zoom} onClose={() => setZoom(null)} />
      <DownloadGuide ctl={guide} />
      <nav className="crumb" aria-label="breadcrumb">
        <Link to="/torrents">{t('ui.nav.torrents')}</Link>
        <Link to={`/torrents?f.type_id=${d.type_id}`}>{typeName(d.type_id)}</Link>
        <span>#{d.id}</span>
      </nav>
      <section className="dt-hero" data-primary style={poster ? ({ ['--dt-bg' as string]: `url(${poster})` } as React.CSSProperties) : undefined}>
        <button type="button" className="dt-cover" onClick={() => poster && setZoom(poster)} aria-label={t('ui.torrent.zoom')} disabled={!poster}>
          <Cover src={poster} title={zh || d.title} sub={zh ? d.title : null} rating={rating} />
        </button>
        <div className="dt-main">
          <div className="row dt-chips">
            <TypeMark typeId={d.type_id} />
            {d.status !== 'approved' && <Status tone="warn">{statusName(t, d.status)}</Status>}
            <Slot name="torrent.detail.header" ctx={ctx} className="row" />
            {genres.slice(0, 4).map((x) => (
              <Tag key={x}>{x}</Tag>
            ))}
          </div>
          <h1>
            {zh || d.title}
            {zh && <small>{d.title}</small>}
          </h1>
          <div className="dt-meta">
            <span>
              {t('ui.torrent.size')} <b className="num">{formatBytes(d.size)}</b>
            </span>
            <span>
              {t('ui.torrent.files')} <b className="num">{d.file_count}</b>
            </span>
            <span title={new Date(created).toLocaleString()}>
              {t('ui.torrent.created')} <b>{timeAgo(created)}</b>
            </span>
            <span>
              {t('ui.torrent.uploader')} {uploader}
            </span>
            {d.info_hash_v1 && (
              <span className="row" style={{ gap: 6 }}>
                Hash <code className="mono">{d.info_hash_v1.slice(0, 8)}…{d.info_hash_v1.slice(-6)}</code>
                <CopyButton size="xs" text={d.info_hash_v1} title={t('ui.torrent.copy_hash')} />
              </span>
            )}
          </div>
          {(rating !== null || swarm) && (
            <div className="scores">
              {rating !== null && (
                <div className="score">
                  <b>{rating.toFixed(1)}</b>
                  <span>{t('ui.torrent.rating')}</span>
                </div>
              )}
              {swarm && (
                <>
                  <div className="score">
                    <b className={seedCls(swarm.seeders)}>{swarm.seeders}</b>
                    <span>{t('ui.torrent.seeders')}</span>
                  </div>
                  <div className="score">
                    <b className="down">{swarm.leechers}</b>
                    <span>{t('ui.torrent.leechers')}</span>
                  </div>
                  <div className="score">
                    <b>{swarm.completed}</b>
                    <span>{t('ui.torrent.completed')}</span>
                  </div>
                </>
              )}
            </div>
          )}
          <div className="dt-acts m-hide">
            {d.can_download ? (
              <>
                <a className="btn btn-primary" href={dlUrl(d.id)} download onClick={() => guide.trigger()}>
                  {t('ui.torrent.download')}
                </a>
                <CopyButton size="md" text={`${location.origin}${dlUrl(d.id)}`} label={t('ui.torrent.copy_link')} />
              </>
            ) : (
              <span className="small muted">{t('ui.torrent.cannot_download')}</span>
            )}
            {actions}
          </div>
          {d.can_download && (
            <p className="small muted dt-hint m-hide">
              {t('ui.torrent.download_hint')}{' '}
              <button type="button" className="txt-btn link" onClick={() => guide.show()}>
                {t('ui.help.how_to_download')}
              </button>
            </p>
          )}
        </div>
      </section>
      <KeyFacts
        items={[
          { label: t('ui.torrent.seeders_leechers'), value: swarm ? `${swarm.seeders} / ${swarm.leechers}` : '–', hint: swarm ? t('ui.torrent.completed_n', { n: swarm.completed }) : undefined, tone: swarm && swarm.seeders === 0 ? 'down' : 'up' },
          { label: t('ui.torrent.size'), value: formatBytes(d.size), hint: t('ui.torrent.files_n', { n: d.file_count }) },
          mine
            ? { label: t('ui.torrent.my_progress'), value: mine.completed_at ? t('ui.torrent.my_done') : formatBytes(mine.downloaded), hint: `↑ ${formatBytes(mine.uploaded)}`, tone: 'moon' }
            : { label: t('ui.torrent.created'), value: age(created), hint: d.uploader.username ?? undefined },
          { label: t('ui.torrent.uploader'), value: d.uploader.anonymous ? t('ui.torrent.anonymous') : (d.uploader.username ?? '-') },
        ]}
      />
      {d.can_download && (
        <MobileBar>
          <a className="btn btn-primary" href={dlUrl(d.id)} download onClick={() => guide.trigger()}>
            {t('ui.torrent.download')}
          </a>
          <CopyButton size="md" className="narrow" text={`${location.origin}${dlUrl(d.id)}`} label={t('ui.torrent.copy_link_short')} />
          <Button className="narrow" onClick={() => setMore(true)}>
            {t('ui.common.more')}
          </Button>
        </MobileBar>
      )}
      <Sheet open={more} onClose={() => setMore(false)} title={t('ui.torrent.more_actions')}>
        <div className="stack-s sheet-actions">{actions}</div>
      </Sheet>
      <div className="dt-grid">
        <Suspense fallback={<SkeletonRows n={4} />}>
          <DetailTabs d={d} ctx={ctx} intro={intro} swarm={swarm} manage={canManage} />
        </Suspense>
        <div className="stack">
          <Panel title={t('ui.torrent.health')} className="kf-hide">
            <div className="health">
              <div>
                <b className={swarm ? seedCls(swarm.seeders) : undefined}>{swarm?.seeders ?? '–'}</b>
                <span>{t('ui.torrent.seeders')}</span>
              </div>
              <div>
                <b className="down">{swarm?.leechers ?? '–'}</b>
                <span>{t('ui.torrent.leechers')}</span>
              </div>
              <div>
                <b>{swarm?.completed ?? '–'}</b>
                <span>{t('ui.torrent.completed')}</span>
              </div>
            </div>
          </Panel>
          {mine && (
            <Panel title={t('ui.torrent.my_record_title')} className="kf-hide">
              <dl className="ledger">
                <dt>{t('ui.profile.uploaded')}</dt>
                <dd className="num up">{formatBytes(mine.uploaded)}</dd>
                <dt>{t('ui.profile.downloaded')}</dt>
                <dd className="num down">{formatBytes(mine.downloaded)}</dd>
                <dt>{t('ui.profile.seed_time')}</dt>
                <dd className="num">{fmtDur(mine.seed_secs)}</dd>
                <dt>{t('ui.admin.status')}</dt>
                <dd>{mine.completed_at ? t('ui.torrent.my_done') : t('ui.torrent.my_incomplete')}</dd>
              </dl>
            </Panel>
          )}
          <Slot name="torrent.detail.sidebar" ctx={ctx} className="stack" />
          {g && g.torrents.length > 1 && (
            <Panel title={t('ui.group.torrents', { n: g.torrents.length })} more={<Link to={`/groups/${g.id}`}>{t('ui.group.view')}</Link>} flush>
              <div className="vers">
                {g.torrents.slice(0, 6).map((x) => (
                  <Link key={x.id} to={`/torrents/${x.id}`} className="ver" aria-current={x.id === d.id ? 'true' : undefined}>
                    <b>{x.title}</b>
                    <span>{x.id === d.id ? t('ui.group.current') : formatBytes(x.size)}</span>
                  </Link>
                ))}
              </div>
            </Panel>
          )}
        </div>
      </div>
    </div>
  );
}
