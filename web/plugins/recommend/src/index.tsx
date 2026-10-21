// recommend 前端：首页主栏“为你推荐 / 近期热门 / 需要做种 / 最新上架”，海报书架横向滚动。
import { useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import type { PluginFrontend, SlotProps } from '@pt-studio/slots';
import { formatBytes, registerMessages } from '@pt-studio/terms';
import { Cover, Panel, SkeletonRows, Tabs, Tag, useMediaQuery } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'recommend.ui.for_you': '为你推荐',
  'recommend.ui.for_you_cold': '猜你喜欢',
  'recommend.ui.hot': '近期热门',
  'recommend.ui.needs_seed': '需要做种',
  'recommend.ui.needs_seed_hint': '这些资源做种的人很少，续种能帮大家，也更容易积累上传量',
  'recommend.ui.fresh': '最新上架',
  'recommend.ui.mine': '已在做种',
  'recommend.ui.empty': '站里还没有资源，去发布第一个吧',
  'recommend.ui.personal_hint': '根据你做种和下载过的内容推荐',
  'recommend.ui.cold_hint': '还不了解你的口味，先看看大家在看什么；做种或下载几个资源后会越来越准',
});
registerMessages('en', {
  'recommend.ui.for_you': 'Recommended for you',
  'recommend.ui.for_you_cold': 'You might like',
  'recommend.ui.hot': 'Trending',
  'recommend.ui.needs_seed': 'Needs seeders',
  'recommend.ui.needs_seed_hint': 'Few people are seeding these. Seeding helps everyone and earns upload credit',
  'recommend.ui.fresh': 'Just added',
  'recommend.ui.mine': 'Seeding',
  'recommend.ui.empty': 'Nothing here yet. Be the first to upload',
  'recommend.ui.personal_hint': 'Based on what you have seeded and downloaded',
  'recommend.ui.cold_hint': 'We do not know your taste yet. Seed or download a few releases and this gets better',
});

interface Item {
  id: number;
  title: string;
  title_zh: string | null;
  type_id: string;
  size: number;
  poster: string | null;
  rating: number | null;
  genres: string[];
  seeders: number;
  leechers: number;
  completed: number;
  mine: boolean;
  reason: string | null;
}
interface Section {
  id: string;
  personalised?: boolean;
  items: Item[];
}

const TYPE_NAME: Record<string, string> = { movie: '电影', tv: '剧集', anime: '动漫', music: '音乐' };

/** 发布名 → 去掉点号的可读名（保留到年份 / 季号之前） */
function readable(title: string): string {
  const cut = title.search(/[.\s](?:19|20)\d{2}[.\s]|[.\s]S\d{1,2}(?:E\d+)?[.\s]|[.\s](?:2160p|1080p|720p)/i);
  return (cut > 0 ? title.slice(0, cut) : title).replace(/\./g, ' ').trim();
}

function CardItem({ item, api }: { item: Item; api: SlotProps['api'] }) {
  const href = `/torrents/${item.id}`;
  const name = item.title_zh ?? readable(item.title);
  return (
    <a
      href={href}
      title={item.title}
      onClick={(e) => {
        e.preventDefault();
        api.navigate(href);
      }}
    >
      <Cover src={item.poster} title={name} sub={item.title_zh ? readable(item.title) : null} rating={item.rating} corner={item.mine ? <Tag tone="g">{api.t('recommend.ui.mine')}</Tag> : null} />
      <span className="nm">{name}</span>
      <span className="mt">
        {TYPE_NAME[item.type_id] ?? item.type_id} · <span className="c-seed">↑{item.seeders}</span> ↓{item.leechers} · {formatBytes(item.size)}
      </span>
      {item.reason && <span className="mt">{item.reason}</span>}
    </a>
  );
}

function Sections({ api }: SlotProps) {
  const narrow = useMediaQuery('(max-width: 640px)');
  const [tab, setTab] = useState('');
  const q = useQuery({
    queryKey: ['recommend-home'],
    queryFn: () => api.json<{ sections: Section[] }>('/api/p/recommend/v1/home'),
    staleTime: 60_000,
    retry: false,
  });
  if (!api.user()) return null;
  if (q.isLoading) {
    return (
      <Panel flush>
        <SkeletonRows n={3} />
      </Panel>
    );
  }
  const sections = q.data?.sections ?? [];
  if (sections.length === 0) return null;
  if (narrow) {
    // 手机：多个书架合成一块，页签切换，卡片缩小，省去提示语
    const cur = sections.find((s) => s.id === tab) ?? sections[0]!;
    return (
      <Panel flush className="rec-m">
        <Tabs
          variant="flush"
          value={cur.id}
          onChange={setTab}
          tabs={sections.map((s) => ({ id: s.id, label: api.t(`recommend.ui.${s.id === 'for_you' && !s.personalised ? 'for_you_cold' : s.id}`) }))}
        />
        <div className="shelf sm">
          {cur.items.map((it) => (
            <CardItem key={it.id} item={it} api={api} />
          ))}
        </div>
      </Panel>
    );
  }
  return (
    <>
      {sections.map((s) => {
        const key = s.id === 'for_you' && !s.personalised ? 'for_you_cold' : s.id;
        const hint = s.id === 'for_you' ? (s.personalised ? 'personal_hint' : 'cold_hint') : s.id === 'needs_seed' ? 'needs_seed_hint' : null;
        return (
          <Panel key={s.id} title={api.t(`recommend.ui.${key}`)} sub={hint ? api.t(`recommend.ui.${hint}`) : undefined} flush>
            <div className="shelf">
              {s.items.map((it) => (
                <CardItem key={it.id} item={it} api={api} />
              ))}
            </div>
          </Panel>
        );
      })}
    </>
  );
}

export default {
  slots: { 'home.sections': { component: Sections, order: 10 } },
} satisfies PluginFrontend;
