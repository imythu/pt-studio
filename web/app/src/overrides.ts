// 页面覆盖（web HLD DD-WEB-5）：站点二开时用自定义组件替换框架页面，不修改核心源码。
import type { ComponentType } from 'react';

export type PageId = 'home' | 'login' | 'register' | 'torrents' | 'torrent' | 'group' | 'upload' | 'profile' | 'settings' | 'admin';

export const PAGE_IDS: PageId[] = ['home', 'login', 'register', 'torrents', 'torrent', 'group', 'upload', 'profile', 'settings', 'admin'];

export const overrides: Partial<Record<PageId, ComponentType>> = {};
