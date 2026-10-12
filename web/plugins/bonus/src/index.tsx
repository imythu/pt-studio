// bonus 前端：个人页显示魔力值余额（仅本人）。
import { useEffect, useState } from 'react';
import type { PluginFrontend, SlotProps } from '@pt-studio/slots';
import { Stat } from '@pt-studio/ui-kit';

interface Summary {
  balances: { currency: string; scale: number; name_term: string; balance: number | string; available: number | string }[];
}

function fmt(v: number | string, scale: number): string {
  const n = Number(v) / 10 ** scale;
  return n.toLocaleString(undefined, { maximumFractionDigits: Math.min(scale, 2) });
}

function BonusStat({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const [s, setS] = useState<Summary | null>(null);
  const self = api.user()?.id === ctx.profile.id;
  useEffect(() => {
    if (self) void api.json<Summary>('/api/p/bonus/v1/users/me/summary').then(setS).catch(() => undefined);
  }, [api, self]);
  if (!s) return null;
  return (
    <>
      {s.balances.map((b) => (
        <Stat key={b.currency} label={api.t(`term:${b.name_term}`)} value={fmt(b.balance, b.scale)} />
      ))}
    </>
  );
}

export default { slots: { 'user.profile.stats': { component: BonusStat, order: 20 } } } satisfies PluginFrontend;
