// 资源类型（torrent `/v1/types`）：上传表单 schema、字段显示名、类型徽标。
import { useQuery } from '@tanstack/react-query';
import { api } from '@pt-studio/sdk';
import type { Schema } from '@pt-studio/schema-form';
import { branchOf } from '@pt-studio/schema-form';

export interface TypeInfo {
  type_id: string;
  name_term: string;
  plugin: string | null;
  field_schema: Schema | null;
  group_schema: Schema | null;
}

export function useTypes() {
  return useQuery({
    queryKey: ['torrent-types'],
    queryFn: async () => (await api<{ items: TypeInfo[] }>('/api/p/torrent/v1/types')).items,
    staleTime: 60_000,
  });
}

/** 某类型下字段值对应的 schema（type-generic 时取 kind 分支） */
export function fieldSchemas(info: TypeInfo | undefined, fields: Record<string, unknown>): Record<string, Schema> {
  const s = info?.field_schema;
  if (!s) return {};
  const b = s.oneOf ? branchOf(s, fields) : s;
  return b?.properties ?? s.properties ?? {};
}
