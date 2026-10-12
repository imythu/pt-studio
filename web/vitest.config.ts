import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vitest/config';

const src = (p: string) => fileURLToPath(new URL(p, import.meta.url));

export default defineConfig({
  esbuild: { jsx: 'automatic' },
  resolve: {
    alias: {
      '@pt-studio/sdk': src('./packages/sdk/src/index.ts'),
      '@pt-studio/terms': src('./packages/terms/src/index.ts'),
      '@pt-studio/slots': src('./packages/slots/src/index.tsx'),
      '@pt-studio/ui-kit': src('./packages/ui-kit/src/index.tsx'),
      '@pt-studio/schema-form': src('./packages/schema-form/src/index.tsx'),
    },
  },
  test: { environment: 'jsdom', include: ['packages/*/test/**/*.test.{ts,tsx}'] },
});
