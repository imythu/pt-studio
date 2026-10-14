// 前端构建（web HLD §4）：共享依赖打成独立 ESM 模块并写入 import map，宿主与插件前端都把它们标记为 external，
// 运行时共用同一份实例。输出目录 web/dist：
//   index.html（import map 带 __CSP_NONCE__ 占位，由内核替换）、static/vendor/*、static/app.<hash>.{js,css}、plugins/<id>/index.js
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import * as esbuild from 'esbuild';

const web = fileURLToPath(new URL('..', import.meta.url));
const out = join(web, 'dist');
const vendorDir = join(out, 'static/vendor');
const require = createRequire(join(web, 'package.json'));
const dev = process.argv.includes('--dev');

rmSync(out, { recursive: true, force: true });
mkdirSync(vendorDir, { recursive: true });

const hash = (s) => createHash('sha256').update(s).digest('hex').slice(0, 12);
const common = {
  bundle: true,
  format: 'esm',
  target: 'es2022',
  platform: 'browser',
  minify: !dev,
  sourcemap: false,
  legalComments: 'none',
  write: false,
  define: { 'process.env.NODE_ENV': JSON.stringify(dev ? 'development' : 'production') },
  jsx: 'automatic',
  logLevel: 'warning',
};

const REACT_SPECS = ['react', 'react/jsx-runtime', 'react-dom', 'react-dom/client'];
const OWN = {
  '@pt-studio/sdk': 'packages/sdk/src/index.ts',
  '@pt-studio/terms': 'packages/terms/src/index.ts',
  '@pt-studio/slots': 'packages/slots/src/index.tsx',
  '@pt-studio/ui-kit': 'packages/ui-kit/src/index.tsx',
  '@pt-studio/schema-form': 'packages/schema-form/src/index.tsx',
};
const ESM_VENDOR = ['@tanstack/react-query', 'react-router'];
const SHARED = [...REACT_SPECS, ...ESM_VENDOR, ...Object.keys(OWN)];

const importMap = {};

function emit(name, code) {
  const file = `${name}.${hash(code)}.js`;
  writeFileSync(join(vendorDir, file), code);
  return `/static/vendor/${file}`;
}

function textOf(result) {
  return result.outputFiles[0].text;
}

// 1. React 全家：一个模块内打包（CJS 互操作），再为每个说明符生成 ESM 门面
const reactAll = await esbuild.build({
  ...common,
  stdin: {
    contents: `import React from 'react'; import ReactDOM from 'react-dom'; import * as Client from 'react-dom/client'; import * as JSX from 'react/jsx-runtime';
               export { React, ReactDOM, Client, JSX };`,
    resolveDir: web,
    loader: 'js',
  },
});
const reactAllUrl = emit('react-all', textOf(reactAll));
const facades = {
  react: ['React', Object.keys(require('react'))],
  'react-dom': ['ReactDOM', Object.keys(require('react-dom'))],
  'react-dom/client': ['Client', Object.keys(require('react-dom/client'))],
  'react/jsx-runtime': ['JSX', Object.keys(require('react/jsx-runtime'))],
};
const ident = /^[A-Za-z_$][\w$]*$/;
for (const [spec, [binding, keys]] of Object.entries(facades)) {
  const names = keys.filter((k) => k !== 'default' && ident.test(k));
  const code = `import { ${binding} as M } from '${reactAllUrl}';\nexport default M;\nexport const { ${names.join(', ')} } = M;\n`;
  importMap[spec] = emit(spec.replace(/[^a-z0-9]+/gi, '-').replace(/^-+/, ''), code);
}

// 2. ESM 依赖与自有共享包
for (const spec of ESM_VENDOR) {
  const r = await esbuild.build({
    ...common,
    stdin: { contents: `export * from '${spec}';`, resolveDir: web, loader: 'js' },
    external: SHARED.filter((s) => s !== spec),
  });
  importMap[spec] = emit(spec.replace(/[^a-z0-9]+/gi, '-').replace(/^-+/, ''), textOf(r));
}
for (const [spec, entry] of Object.entries(OWN)) {
  const r = await esbuild.build({ ...common, entryPoints: [join(web, entry)], external: SHARED.filter((s) => s !== spec) });
  importMap[spec] = emit(spec.replace(/[^a-z0-9]+/gi, '-').replace(/^-+/, ''), textOf(r));
}

// 3. 宿主应用
const app = await esbuild.build({ ...common, entryPoints: [join(web, 'app/src/main.tsx')], external: SHARED });
const appJs = textOf(app);
const appFile = `app.${hash(appJs)}.js`;
writeFileSync(join(out, 'static', appFile), appJs);
const css = ['themes/tokens.css', 'packages/ui-kit/src/styles.css', 'app/src/app.css'].map((f) => readFileSync(join(web, f), 'utf8')).join('\n');
const cssMin = (await esbuild.transform(css, { loader: 'css', minify: !dev })).code;
const cssFile = `app.${hash(cssMin)}.css`;
writeFileSync(join(out, 'static', cssFile), cssMin);

// 4. 插件前端：web/plugins/<id>/src/index.tsx → dist/plugins/<id>/index.js
const pluginsDir = join(web, 'plugins');
const built = [];
if (existsSync(pluginsDir)) {
  for (const id of readdirSync(pluginsDir)) {
    const entry = ['src/index.tsx', 'src/index.ts'].map((f) => join(pluginsDir, id, f)).find(existsSync);
    if (!entry) continue;
    const r = await esbuild.build({ ...common, entryPoints: [entry], external: SHARED });
    mkdirSync(join(out, 'plugins', id), { recursive: true });
    writeFileSync(join(out, 'plugins', id, 'index.js'), textOf(r));
    built.push(id);
  }
}

// 4b. 站点图标等根目录静态文件：web/assets/* → dist/*（favicon.ico 等浏览器会直接请求根路径）
const assetsDir = join(web, 'assets');
if (existsSync(assetsDir)) for (const f of readdirSync(assetsDir)) copyFileSync(join(assetsDir, f), join(out, f));

// 5. index.html（import map 内联脚本用 nonce；内核返回时替换 __CSP_NONCE__）
const html = `<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>PT Studio</title>
<meta name="theme-color" content="#2e9577">
<meta name="color-scheme" content="light dark">
<link rel="icon" href="/favicon.svg" type="image/svg+xml">
<link rel="icon" href="/favicon.ico" sizes="48x48">
<link rel="apple-touch-icon" href="/apple-touch-icon.png">
<link rel="manifest" href="/site.webmanifest">
<script type="importmap" nonce="__CSP_NONCE__">${JSON.stringify({ imports: importMap })}</script>
<link rel="stylesheet" href="/static/${cssFile}">
<script type="module" src="/static/${appFile}"></script>
</head>
<body>
<div id="root"></div>
<noscript>PT Studio requires JavaScript.</noscript>
</body>
</html>
`;
writeFileSync(join(out, 'index.html'), html);
writeFileSync(join(out, 'importmap.json'), JSON.stringify({ imports: importMap }, null, 2));
console.log(`built app (${(appJs.length / 1024).toFixed(1)} KiB), ${Object.keys(importMap).length} shared modules, plugins: ${built.join(', ') || '-'}`);
