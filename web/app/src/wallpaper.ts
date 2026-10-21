// 全站背景壁纸（芙莉莲素材）：固定在页面最底层的淡化大图，用户在「设置 → 外观」选择，存在本机 localStorage。
import { useEffect, useState } from 'react';

const KEY = 'pt-wallpaper';
const EVENT = 'pt-wallpaper-change';

export const NO_WALLPAPER = 'none';

/** 可选壁纸：id 为大图路径，thumb 为设置页缩略图；label 为界面文案键 */
export const WALLPAPERS = [
  { id: '/frieren/frieren-sky-landscape.jpg', label: 'ui.wallpaper.grimoire' },
  { id: '/frieren/frieren-hero-field.jpg', label: 'ui.wallpaper.tree' },
  { id: '/frieren/frieren-wide-scenery.jpg', label: 'ui.wallpaper.ruins' },
  { id: '/frieren/frieren-hero-banner-generated.jpg', label: 'ui.wallpaper.castle' },
  { id: '/frieren/frieren-sunset-horizon.jpg', label: 'ui.wallpaper.sofa' },
  { id: '/frieren/frieren-magic-journey.jpg', label: 'ui.wallpaper.street' },
].map((w) => ({ ...w, thumb: w.id.replace('/frieren/', '/frieren/thumbs/') }));

export const DEFAULT_WALLPAPER = WALLPAPERS[0]!.id;

export function getWallpaper(): string {
  try {
    const v = localStorage.getItem(KEY);
    if (v === NO_WALLPAPER || WALLPAPERS.some((w) => w.id === v)) return v as string;
  } catch {
    /* 隐私模式 */
  }
  return DEFAULT_WALLPAPER;
}

export function setWallpaper(v: string) {
  try {
    localStorage.setItem(KEY, v);
  } catch {
    /* 隐私模式：只在本次会话生效 */
  }
  window.dispatchEvent(new CustomEvent(EVENT, { detail: v }));
}

export function useWallpaper(): string {
  const [v, setV] = useState(getWallpaper);
  useEffect(() => {
    const on = (e: Event) => setV((e as CustomEvent<string>).detail ?? getWallpaper());
    window.addEventListener(EVENT, on);
    return () => window.removeEventListener(EVENT, on);
  }, []);
  return v;
}
