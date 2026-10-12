// @pt-studio/terms：站点术语与界面文案（web LLD §5）。
// `t('term:bonus.currency')` 取术语名，`|short`、`|unit` 取短名与单位；其余键查界面文案（按命名空间注册），找不到时返回键本身。

import { createContext, createElement, useContext, useEffect, useMemo, useState, type ReactNode } from 'react';
import { api } from '@pt-studio/sdk';

export interface TermValue {
  name: string;
  short?: string;
  unit?: string;
  icon?: string;
}

type Dict = Record<string, string>;

const messages = new Map<string, Dict>(); // `${locale}` → 合并后的文案

/** 注册界面文案（宿主与插件资源包）；插件键建议以插件 ID 为前缀 */
export function registerMessages(locale: string, dict: Dict): void {
  messages.set(locale, { ...(messages.get(locale) ?? {}), ...dict });
}

export function interpolate(s: string, args?: Record<string, unknown>): string {
  if (!args) return s;
  return s.replace(/\{(\w+)\}/g, (m, k: string) => (k in args ? String(args[k]) : m));
}

export interface Translator {
  locale: string;
  t(key: string, args?: Record<string, unknown>): string;
  term(key: string): TermValue | undefined;
}

export function makeTranslator(locale: string, terms: Record<string, TermValue>, fallbackLocale = 'zh-CN'): Translator {
  const ui = (k: string) => messages.get(locale)?.[k] ?? messages.get(fallbackLocale)?.[k];
  return {
    locale,
    term: (k) => terms[k],
    t(key, args) {
      if (key.startsWith('term:')) {
        const [k, part] = key.slice(5).split('|') as [string, string | undefined];
        const v = terms[k];
        if (!v) return k;
        const s = part === 'short' ? (v.short ?? v.name) : part === 'unit' ? (v.unit ?? '') : v.name;
        return interpolate(s, args);
      }
      return interpolate(ui(key) ?? key, args);
    },
  };
}

const Ctx = createContext<Translator>(makeTranslator('zh-CN', {}));

export function TermsProvider({ locale, children }: { locale: string; children: ReactNode }) {
  const [terms, setTerms] = useState<Record<string, TermValue>>({});
  useEffect(() => {
    let live = true;
    api<{ terms: Record<string, TermValue> }>(`/api/kernel/v1/terms?locale=${encodeURIComponent(locale)}`)
      .then((r) => live && setTerms(r.terms))
      .catch(() => undefined);
    return () => {
      live = false;
    };
  }, [locale]);
  const tr = useMemo(() => makeTranslator(locale, terms), [locale, terms]);
  return createElement(Ctx.Provider, { value: tr }, children);
}

export function useT(): Translator['t'] {
  return useContext(Ctx).t;
}

export function useTranslator(): Translator {
  return useContext(Ctx);
}

export function useTerm(key: string): TermValue | undefined {
  return useContext(Ctx).term(key);
}

/** 字节数格式化（PT 站常用） */
export function formatBytes(n: number): string {
  if (!Number.isFinite(n)) return '∞';
  const u = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB'];
  let i = 0;
  let v = Math.abs(n);
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${n < 0 ? '-' : ''}${i === 0 ? v : v.toFixed(2)} ${u[i]}`;
}

export function formatDuration(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return d > 0 ? `${d}d ${h}h` : h > 0 ? `${h}h ${m}m` : `${m}m`;
}
