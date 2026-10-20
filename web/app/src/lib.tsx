// 宿主公共工具：时间、数字、分类色、我的数据、站内链接。
import { useQuery } from '@tanstack/react-query';
import type { MouseEvent, ReactNode } from 'react';
import { Link } from 'react-router';
import { api } from '@pt-studio/sdk';
import { useInstalled } from './manifest';
import { useSession } from './session';

/** 相对时间（一周内），更早显示日期 */
export function timeAgo(ms: number): string {
  if (!Number.isFinite(ms)) return '-';
  const secs = Math.round((ms - Date.now()) / 1000);
  const rtf = new Intl.RelativeTimeFormat(document.documentElement.lang || navigator.language, { numeric: 'auto' });
  const abs = Math.abs(secs);
  if (abs < 60) return rtf.format(secs, 'second');
  if (abs < 3600) return rtf.format(Math.trunc(secs / 60), 'minute');
  if (abs < 86400) return rtf.format(Math.trunc(secs / 3600), 'hour');
  if (abs < 7 * 86400) return rtf.format(Math.trunc(secs / 86400), 'day');
  return new Date(ms).toLocaleDateString();
}

/** 存活时间的短格式（列表「存活」列）：3分 / 5时 / 2天 / 3周 / 4月 / 2年 */
export function age(ms: number): string {
  const s = Math.max(0, (Date.now() - ms) / 1000);
  const zh = (document.documentElement.lang || 'zh').startsWith('zh');
  const u = zh ? ['分', '时', '天', '周', '月', '年'] : ['m', 'h', 'd', 'w', 'mo', 'y'];
  if (s < 3600) return `${Math.max(1, Math.floor(s / 60))}${u[0]}`;
  if (s < 86400) return `${Math.floor(s / 3600)}${u[1]}`;
  if (s < 14 * 86400) return `${Math.floor(s / 86400)}${u[2]}`;
  if (s < 60 * 86400) return `${Math.floor(s / 604800)}${u[3]}`;
  if (s < 365 * 86400) return `${Math.floor(s / 2592000)}${u[4]}`;
  return `${Math.floor(s / 31536000)}${u[5]}`;
}

/** 体积的短格式（数据条）：32.41T、128G */
export function shortBytes(n: number): string {
  const u = ['B', 'K', 'M', 'G', 'T', 'P'];
  let i = 0;
  let v = Math.abs(n);
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(2)}${u[i]}`;
}

/** 大数的短格式：128,430 → 128.4k */
export function shortNum(n: number): string {
  if (Math.abs(n) < 10_000) return n.toLocaleString('en-US', { maximumFractionDigits: 1 });
  if (Math.abs(n) < 1_000_000) return `${(n / 1000).toFixed(1)}k`;
  return `${(n / 1_000_000).toFixed(2)}M`;
}

/** 分享率：accounting 返回十进制字符串，没有下载量时为 null（∞） */
export function fmtRatio(r: string | number | null | undefined): string {
  if (r === null || r === undefined) return '∞';
  const v = Number(r);
  return Number.isFinite(v) ? v.toFixed(v >= 100 ? 0 : 3) : '∞';
}

/** 做种时长：小时 / 天 / 年 */
export function fmtSeed(secs: number): { v: string; u: string } {
  const h = secs / 3600;
  if (h < 48) return { v: h.toFixed(h < 10 ? 1 : 0), u: '时' };
  const d = h / 24;
  if (d < 365) return { v: d.toFixed(0), u: '天' };
  return { v: (d / 365).toFixed(1), u: '年' };
}

/** 资源类型的识别色（列表类型下划线、海报底色） */
const TYPE_COLORS: Record<string, string> = {
  movie: 'var(--ember)',
  tv: 'var(--moon)',
  anime: 'var(--gold)',
  music: '#c07a3a',
  doc: 'var(--emerald)',
  documentary: 'var(--emerald)',
  show: '#b06fb0',
  game: '#5a8f8f',
  generic: 'var(--muted)',
};
export function typeColor(id?: string): string {
  return (id && TYPE_COLORS[id]) || 'var(--line-2)';
}

export interface MyStats {
  uploaded: number;
  downloaded: number;
  ratio: string | number | null;
  seed_secs: number;
  seeding_count?: number;
  leeching_count?: number;
  completed_count?: number;
  uploaded_raw?: number;
  downloaded_raw?: number;
  today?: { uploaded: number; downloaded: number; seed_secs: number } | null;
}

/** 我的流量统计（accounting） */
export function useMyStats() {
  const me = useSession().me;
  const on = useInstalled('accounting') && !!me;
  return useQuery({ queryKey: ['my-stats'], queryFn: () => api<MyStats>('/api/p/accounting/v1/users/me/stats'), enabled: on, refetchInterval: 120_000, retry: false, staleTime: 60_000 }).data;
}

export interface Balance {
  currency: string;
  balance: number;
  available: number;
  scale: number;
  name_term?: string;
}

/** 金额：最小单位 → 显示值（小数位 scale） */
export function fromUnits(v: number | string, scale = 0): number {
  return num(v) / 10 ** scale;
}

/** 魔力值（bonus）：返回主货币的可用余额 */
export function useMyBonus(): Balance | undefined {
  const me = useSession().me;
  const on = useInstalled('bonus') && !!me;
  return useQuery({
    queryKey: ['bonus-summary'],
    queryFn: () => api<{ balances: Balance[] }>('/api/p/bonus/v1/users/me/summary'),
    enabled: on,
    refetchInterval: 300_000,
    retry: false,
  }).data?.balances.find((b) => b.currency === 'bonus');
}

/** H&R（hnr） */
export function useMyHnr() {
  const me = useSession().me;
  const on = useInstalled('hnr') && !!me;
  return useQuery({
    queryKey: ['hnr-me'],
    queryFn: () => api<{ active: number; failed: number; max: number; can_download: boolean; rules?: { min_seed_hours: number; min_torrent_ratio: number; window_days: number } }>('/api/p/hnr/v1/me'),
    enabled: on,
    retry: false,
    staleTime: 120_000,
  }).data;
}

export interface LevelClass {
  id: number;
  key: string;
  name: string;
  name_key?: string;
  color: string | null;
  type?: string;
  progress?: { metric: string; op: string; current: string; target: string; met: boolean }[];
}

/** 等级（levels）：当前等级与下一等级进度 */
export function useMyLevel() {
  const me = useSession().me;
  const on = useInstalled('levels') && !!me;
  return useQuery({
    queryKey: ['levels-me'],
    queryFn: () => api<{ class: LevelClass | null; next: LevelClass | null; warning: unknown; protected: boolean }>('/api/p/levels/v1/me'),
    enabled: on,
    retry: false,
    staleTime: 300_000,
  }).data;
}

/** 等级显示名：自定义名称优先，否则按术语键 */
export function levelName(t: (k: string) => string, c?: LevelClass | null): string {
  if (!c) return '';
  if (c.name) return c.name;
  if (c.name_key) {
    const v = t(`term:${c.name_key}`);
    if (v !== c.name_key) return v;
  }
  return c.key;
}

/** 未读数：通知与站内信 */
export function useUnread() {
  const me = useSession().me;
  const notify = useInstalled('notify') && !!me;
  const messages = useInstalled('messages') && !!me;
  const n = useQuery({ queryKey: ['unread', 'notify'], queryFn: () => api<{ count: number }>('/api/p/notify/v1/unread'), enabled: notify, refetchInterval: 60_000, retry: false }).data?.count ?? 0;
  const m = useQuery({ queryKey: ['unread', 'messages'], queryFn: () => api<{ private: number }>('/api/p/messages/v1/unread'), enabled: messages, refetchInterval: 60_000, retry: false }).data?.private ?? 0;
  return { notify: n, messages: m, notifyOn: notify, messagesOn: messages };
}

/** 站内链接（保留中键 / Ctrl 新窗口） */
export function A({ to, children, className, onClick }: { to: string; children: ReactNode; className?: string; onClick?(e: MouseEvent): void }) {
  return (
    <Link to={to} className={className} onClick={onClick}>
      {children}
    </Link>
  );
}

export function num(v: unknown): number {
  const n = typeof v === 'string' ? Number(v) : typeof v === 'number' ? v : NaN;
  return Number.isFinite(n) ? n : 0;
}
