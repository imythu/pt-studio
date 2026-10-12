// promotion 前端：种子列表行徽标（批量查询）与详情页促销状态。
import { useEffect, useState } from 'react';
import { registerMessages } from '@pt-studio/terms';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { Badge } from '@pt-studio/ui-kit';

interface State {
  torrent_id: number;
  primary: { kind: string; name_key: string; ends_at: string | null } | null;
  global: { kind: string; name_key: string }[];
}

function loader(api: PluginApi) {
  return api.batch<number, State>('states', async (ids) => {
    const r = await api.json<{ items: State[] }>(`/api/p/promotion/v1/torrents?ids=${ids.join(',')}`);
    return new Map(r.items.map((s) => [s.torrent_id, s]));
  });
}

function tone(kind: string): 'free' | 'double' | 'primary' {
  return kind.includes('free') ? 'free' : kind.includes('double') ? 'double' : 'primary';
}

function useState_(api: PluginApi, id: number): State | null {
  const [s, setS] = useState<State | null>(null);
  useEffect(() => {
    let live = true;
    void loader(api)(id)
      .then((v) => live && setS(v ?? null))
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [api, id]);
  return s;
}

function PromoBadges({ s, api }: { s: State | null; api: PluginApi }) {
  if (!s) return null;
  const all = [...(s.primary ? [s.primary] : []), ...s.global];
  return (
    <>
      {all.map((p, i) => (
        <Badge key={i} tone={tone(p.kind)}>
          {api.t(`term:${p.name_key}`)}
        </Badge>
      ))}
    </>
  );
}

function RowBadge({ ctx, api }: SlotProps<{ torrent: { id: number } }>) {
  return <PromoBadges s={useState_(api, ctx.torrent.id)} api={api} />;
}

function Header({ ctx, api }: SlotProps<{ torrent: { id: number } }>) {
  const s = useState_(api, ctx.torrent.id);
  if (!s?.primary) return <PromoBadges s={s} api={api} />;
  return (
    <span>
      <PromoBadges s={s} api={api} />
      {s.primary.ends_at && <span className="pt-muted small">{api.t('promotion.ui.until', { at: new Date(s.primary.ends_at).toLocaleString() })}</span>}
    </span>
  );
}

registerMessages('zh-CN', { 'promotion.ui.until': '至 {at}' });
registerMessages('en', { 'promotion.ui.until': 'until {at}' });

export default {
  slots: {
    'torrent.list.row_badges': { component: RowBadge, order: 10 },
    'torrent.detail.header': { component: Header, order: 10 },
  },
} satisfies PluginFrontend;
