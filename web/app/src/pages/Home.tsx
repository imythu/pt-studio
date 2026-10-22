// 首页：卷首（问候、旅途天数、签到、分享率刻度盘）、公告插槽、推荐与最新种子、侧栏小组件。未登录时为访客首页。
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useState } from 'react';
import { Link } from 'react-router';
import { api } from '@pt-studio/sdk';
import { Slot } from '@pt-studio/slots';
import { formatBytes, useT } from '@pt-studio/terms';
import { Button, Panel, SkeletonRows, useToast } from '@pt-studio/ui-kit';
import { ErrorText, useErrorText } from '../errors';
import { fmtRatio, fmtSeed, levelName, levelUntil, num, useMyLevel, useMyStats } from '../lib';
import { useInstalled, useManifest } from '../manifest';
import { useSession } from '../session';
import { GuestBar } from '../shell/Layout';
import { TorrentRows, type TorrentDoc } from '../torrent';

interface SearchResult {
  hits: { id: number; document: TorrentDoc; highlights: Record<string, string> }[];
  estimated_total: number;
}

export function Home() {
  const s = useSession();
  if (s.loading) return null;
  return s.me ? <HomeFeed /> : <GuestHome />;
}

/** 分享率刻度盘：外圈刻度缓慢转动；弧长按分享率（上限 10）计 */
function Dial({ ratio, caption, sub }: { ratio: string | number | null | undefined; caption: string; sub?: string }) {
  const r = ratio === null || ratio === undefined ? 10 : Math.min(10, num(ratio));
  const circ = 2 * Math.PI * 84;
  const ticks = Array.from({ length: 72 }, (_, i) => {
    const a = (i / 72) * Math.PI * 2;
    const r1 = 92;
    const r2 = i % 6 === 0 ? 86 : 89;
    return <line key={i} x1={105 + r1 * Math.cos(a)} y1={105 + r1 * Math.sin(a)} x2={105 + r2 * Math.cos(a)} y2={105 + r2 * Math.sin(a)} className={i % 6 === 0 ? 'tk-major' : 'tk'} />;
  });
  return (
    <div className="dial-wrap" aria-hidden="true">
      <svg className="dial" viewBox="0 0 210 210">
        <g className="spin2">
          <circle cx="105" cy="105" r="100" className="dial-dots" />
        </g>
        <g className="spin">{ticks}</g>
        <circle cx="105" cy="105" r="78" className="dial-face" />
        <circle cx="105" cy="105" r="84" className="dial-track" />
        <circle cx="105" cy="105" r="84" className="dial-arc" strokeDasharray={`${(circ * r) / 10} ${circ}`} transform="rotate(-90 105 105)" />
        <text x="105" y="100" textAnchor="middle" className="dial-n">
          {fmtRatio(ratio)}
        </text>
        <text x="105" y="122" textAnchor="middle" className="dial-l">
          {caption}
        </text>
        {sub && (
          <text x="105" y="140" textAnchor="middle" className="dial-s">
            {sub}
          </text>
        )}
      </svg>
    </div>
  );
}

/** 卷首里的签到（checkin 插件未安装时不显示） */
function CheckIn() {
  const t = useT();
  const toast = useToast();
  const errText = useErrorText();
  const qc = useQueryClient();
  const on = useInstalled('checkin');
  const q = useQuery({
    queryKey: ['checkin-me'],
    queryFn: () => api<{ checked_in_today: boolean; streak: number; next_reward?: { daily: number } }>('/api/p/checkin/v1/me'),
    enabled: on,
    retry: false,
  });
  const [busy, setBusy] = useState(false);
  if (!on || !q.data) return null;
  const st = q.data;
  const go = async () => {
    setBusy(true);
    try {
      const r = await api<{ streak: number; rank: number; reward: { total: number } }>('/api/p/checkin/v1/check-ins', { method: 'POST', json: {} });
      toast(t('ui.home.checked', { streak: r.streak, rank: r.rank, total: r.reward.total }), 'success');
      await Promise.all([qc.invalidateQueries({ queryKey: ['checkin-me'] }), qc.invalidateQueries({ queryKey: ['bonus-summary'] })]);
    } catch (e) {
      toast(errText(e), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="pro-acts">
      {st.checked_in_today ? (
        <span className="checked">{t('ui.home.checked_today', { n: st.streak })}</span>
      ) : (
        <>
          <Button variant="gold" onClick={go} disabled={busy}>
            {t('ui.home.checkin', { n: st.next_reward?.daily ?? '' })}
          </Button>
          {st.streak > 0 && <span className="checked">{t('ui.home.streak', { n: st.streak })}</span>}
        </>
      )}
    </div>
  );
}

function Prologue() {
  const t = useT();
  const s = useSession();
  const st = useMyStats();
  const lv = useMyLevel();
  const me = s.me as { username: string; created_at?: number } | null;
  if (!me) return null;
  const days = me.created_at ? Math.max(1, Math.ceil((Date.now() - me.created_at) / 86_400_000)) : null;
  const now = new Date();
  const date = now.toLocaleDateString(document.documentElement.lang || 'zh-CN', { month: 'long', day: 'numeric', weekday: 'long' });
  const seed = st ? fmtSeed(st.seed_secs) : null;
  const next = lv?.next ? levelName(t, lv.next) : null;
  // 升级还差的流量（预设按下载量；自定义等级也可能按上传量）
  const upMet = lv?.next?.progress?.find((p) => (p.metric === 'downloaded_gib' || p.metric === 'uploaded_gib') && !p.met);
  const until = levelUntil(t, lv?.class);
  const toNext = upMet ? `${Math.max(0, num(upMet.target) - num(upMet.current)).toFixed(1)} GiB` : null;
  return (
    <section className="prologue" data-primary>
      <div className="pro-txt">
        <span className="eyebrow">{date}</span>
        <h1>
          {days ? (
            <>
              {t('ui.home.day_a', { name: me.username })}
              <em>{days}</em>
              {t('ui.home.day_b')}
            </>
          ) : (
            t('ui.home.welcome', { name: me.username })
          )}
        </h1>
        {st && (
          <p>
            {st.seeding_count
              ? t('ui.home.seed_line', { n: st.seeding_count, v: seed?.v, u: seed?.u })
              : t('ui.home.seed_line_none')}
            {st.today && st.today.uploaded > 0 ? ` ${t('ui.home.today_up', { v: formatBytes(st.today.uploaded) })}` : ''}
          </p>
        )}
        {until && <p>{t('ui.home.class_until', { name: levelName(t, lv?.class), until })}</p>}
        <CheckIn />
      </div>
      {st && <Dial ratio={st.ratio} caption={t('ui.profile.ratio')} sub={next && toNext ? t('ui.home.to_next', { name: next, v: toNext }) : undefined} />}
    </section>
  );
}

function Latest() {
  const t = useT();
  const res = useQuery({
    queryKey: ['search', 'home-latest'],
    queryFn: () =>
      api<SearchResult>('/api/p/search-meilisearch/v1/search', {
        json: {
          index: 'torrents',
          query: '',
          sort: [
            { field: 'sticky', descending: true },
            { field: 'created_at', descending: true },
          ],
          limit: 12,
        },
      }),
    staleTime: 30_000,
    retry: false,
  });
  return (
    <Panel title={t('ui.home.latest')} more={<Link to="/torrents">{t('ui.home.view_all')}</Link>} flush>
      {res.isLoading ? <SkeletonRows n={6} /> : res.error ? <div className="pad"><ErrorText error={res.error} /></div> : <TorrentRows rows={res.data?.hits.map((h) => h.document) ?? []} />}
    </Panel>
  );
}

function HomeFeed() {
  const me = useSession().me;
  return (
    <div className="stack home">
      <Prologue />
      <Slot name="home.announcements" ctx={{}} className="stack" />
      <div className="home-grid">
        <div className="stack">
          <Slot name="home.sections" ctx={{ user: me }} className="stack" />
          <Latest />
        </div>
        <div className="stack">
          <Slot name="home.widgets" ctx={{}} className="stack" />
        </div>
      </div>
    </div>
  );
}

function GuestHome() {
  const t = useT();
  const m = useManifest();
  return (
    <div className="bare-page">
      <GuestBar />
      <section className="guest-hero" data-primary>
        <div className="stack-s">
          <span className="eyebrow">{t('ui.home.guest_eyebrow')}</span>
          <h1>{t('ui.home.hero')}</h1>
          <p>{t('ui.home.guest_body')}</p>
          <div className="row">
            <Link className="btn btn-primary btn-lg" to="/login">
              {t('ui.nav.login')}
            </Link>
            <Link className="btn btn-lg" to="/register">
              {t('ui.nav.register')}
            </Link>
          </div>
        </div>
      </section>
      <div className="g3 guest-feats">
        {[1, 2, 3].map((i) => (
          <section key={i} className="panel pad-l">
            <h2>{t(`ui.home.f${i}_title`)}</h2>
            <p className="small muted">{t(`ui.home.f${i}_body`, { site: m.site.name })}</p>
          </section>
        ))}
      </div>
    </div>
  );
}
