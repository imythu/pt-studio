import { createContext, useContext } from 'react';
import type { FrontendManifest } from '@pt-studio/sdk';

export const ManifestCtx = createContext<FrontendManifest | null>(null);

export function useManifest(): FrontendManifest {
  const m = useContext(ManifestCtx);
  if (!m) throw new Error('manifest is missing');
  return m;
}

/** 已安装的插件（含无前端的） */
export function useInstalled(id: string): boolean {
  return useManifest().installed.includes(id);
}
