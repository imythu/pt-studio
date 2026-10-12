// 从 docs/platform/catalog/slots.md 生成插槽名与类型（与内核 catalog 同源）。
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../..', import.meta.url));
const md = readFileSync(`${root}/docs/platform/catalog/slots.md`, 'utf8');
const section = md.split('## 2. 插槽清单')[1]?.split('\n## ')[0] ?? '';
const rows = [];
for (const line of section.split('\n')) {
  const m = /^\| `([a-z0-9_.]+)` \| ([a-z]+)/.exec(line);
  if (m) rows.push([m[1], m[2]]);
}
if (rows.length === 0) throw new Error('no slots parsed');
const out = `// 由 tools/gen-slots.mjs 从 docs/platform/catalog/slots.md 生成，请勿手改。
export const SLOT_KINDS = {
${rows.map(([n, k]) => `  '${n}': '${k}',`).join('\n')}
} as const;

export type SlotName = keyof typeof SLOT_KINDS;
export type SlotKind = (typeof SLOT_KINDS)[SlotName];
`;
writeFileSync(`${root}/web/packages/slots/src/catalog.gen.ts`, out);
console.log(`slots: ${rows.length}`);
