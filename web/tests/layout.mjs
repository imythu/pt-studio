// 布局自动检查（docs/components/web/05-界面设计规范.md §6）：以管理员登录，在 360 / 390 / 1100 / 1440 宽度下逐页检查
//   1. 页面不横向滚动；2. 手机宽度下 data-primary 区域顶部在首屏 62% 以内；3. 主从页面窄屏只显示一级；
//   4. 页面上第一个打开抽屉的按钮（PageHead 操作区）打开后抽屉完整落在屏幕内、底部按钮不被遮挡；MobileBar 可点击；
//   5. 无脚本错误与 CSP 违规。
// 用法：PT_BASE=http://127.0.0.1:31251 PT_USER=admin PT_PASS=... node tests/layout.mjs [路径...]
import { existsSync } from 'node:fs';
import { chromium } from 'playwright';

const BASE = process.env.PT_BASE ?? 'http://127.0.0.1:31251';
const USER = process.env.PT_USER;
const PASS = process.env.PT_PASS;
if (!USER || !PASS) throw new Error('PT_USER / PT_PASS are required');

const DEFAULT_PATHS = [
  '/',
  '/torrents',
  '/upload',
  '/me',
  '/settings',
  '/p/invite/',
  '/p/levels/',
  '/p/bonus/',
  '/p/checkin/',
  '/p/lottery/',
  '/p/requests/',
  '/p/requests/new',
  '/p/messages/',
  '/p/notify/',
  '/p/hnr/',
  '/p/ledger/transfer',
  '/p/ai-assistant/',
  '/p/forum/',
  '/p/shop/',
  '/p/medals/',
  '/p/top10/',
  '/p/tickets/',
  '/p/donations/',
  '/p/bookmarks/',
  '/admin',
  '/admin/users',
  '/admin/audit',
  '/admin/p/invite/',
  '/admin/p/lottery/',
  '/admin/p/levels/',
  '/admin/p/ledger/',
  '/admin/p/accounting/',
  '/admin/p/promotion/',
  '/admin/p/moderation/',
  '/admin/p/ai-assistant/',
  '/admin/p/search-meilisearch/',
  '/p/attachments/',
  '/p/newbie-exam/',
  '/p/ratio-policy/',
  '/p/anticheat/',
  '/admin/p/anticheat/',
  '/admin/p/shop/',
  '/admin/p/medals/',
  '/admin/p/forum/',
  '/admin/p/tickets/',
  '/admin/p/donations/',
  '/admin/p/seedbox/',
  '/admin/p/type-generic/',
  '/admin/p/llm-claude/',
  '/admin/p/registration-policy/',
  '/admin/p/ratio-policy/',
];
const paths = process.argv.slice(2).length ? process.argv.slice(2) : DEFAULT_PATHS;
const SIZES = [
  [360, 740],
  [390, 844],
  [1100, 900],
  [1440, 900],
];

const shell = '/root/.cache/ms-playwright/chromium_headless_shell-1243/chrome-headless-shell-linux64/chrome-headless-shell';
const executablePath = process.env.PT_CHROME ?? (existsSync(shell) ? shell : undefined);
const browser = await chromium.launch({ executablePath });
const failures = [];

/** 页面内检查：返回问题列表（空 = 通过） */
function inspect(H) {
  const W = document.documentElement.clientWidth;
  const out = [];
  if (document.documentElement.scrollWidth > W + 1) out.push(`horizontal scroll ${document.documentElement.scrollWidth}>${W}`);
  const main = document.querySelector('main') ?? document.body;
  if (W < 760) {
    const el = main.querySelector('[data-primary]');
    if (el) {
      const top = el.getBoundingClientRect().top + scrollY;
      if (top > H * 0.62) out.push(`primary starts at ${Math.round(top)}px (> 62% of ${H})`);
    }
    for (const md of main.querySelectorAll('.md')) {
      const l = md.querySelector(':scope > .md-list');
      const d = md.querySelector(':scope > .md-detail');
      if (l && d && l.offsetParent !== null && d.offsetParent !== null) out.push('master-detail shows both levels');
    }
    const bar = document.querySelector('.m-bar');
    if (bar) {
      const r = bar.getBoundingClientRect();
      const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      if (r.top < 0 || r.bottom > H + 1 || !bar.contains(hit)) out.push('mobile bar is covered or off screen');
    }
  }
  return out;
}

function inspectSheet(H) {
  const sh = document.querySelector('.sheet');
  if (!sh) return [];
  const pn = sh.querySelector('.sheet-pn');
  const W = document.documentElement.clientWidth;
  const r = pn.getBoundingClientRect();
  if (r.top < -1 || r.bottom > H + 1 || r.left < -1 || r.right > W + 1) return ['sheet is off screen'];
  const ft = pn.querySelector('footer') ?? pn;
  const fr = ft.getBoundingClientRect();
  if (!sh.contains(document.elementFromPoint(fr.left + fr.width / 2, fr.bottom - 6))) return ['sheet footer is covered'];
  return [];
}

for (const [w, h] of SIZES) {
  const ctx = await browser.newContext({ viewport: { width: w, height: h }, locale: 'zh-CN', reducedMotion: 'reduce', isMobile: w < 760, hasTouch: w < 760 });
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(`pageerror: ${e.message.split('\n')[0]}`));
  page.on('console', (m) => {
    const t = m.text().split('\n')[0].slice(0, 200);
    if (/Content Security Policy|Refused to/.test(t)) errors.push(`csp: ${t}`);
    else if (m.type() === 'error' && !/Failed to load resource/.test(t)) errors.push(`console: ${t}`);
  });
  await page.goto(`${BASE}/login`);
  await page.locator('input[name=username]').fill(USER);
  await page.locator('input[name=password]').fill(PASS);
  await page.getByRole('button', { name: '登录', exact: true }).click();
  await page.waitForURL((u) => !u.pathname.startsWith('/login'), { timeout: 15000 });
  for (const path of paths) {
    errors.length = 0;
    const resp = await page.goto(`${BASE}${path}`);
    await page.waitForLoadState('networkidle').catch(() => undefined);
    await page.waitForTimeout(300);
    // 插件未启用时路由为 404：跳过
    if (resp && resp.status() >= 400) continue;
    if (await page.getByText('页面不存在').count()) continue;
    const bad = await page.evaluate(inspect, h);
    // 抽屉：点 PageHead 操作区里第一个按钮（新建 / 发布等），有抽屉弹出就检查
    const act = page.locator('.page-h .acts button').first();
    if (await act.count()) {
      await act.click().catch(() => undefined);
      await page.waitForTimeout(200);
      bad.push(...(await page.evaluate(inspectSheet, h)));
      await page.keyboard.press('Escape');
    }
    bad.push(...errors);
    if (bad.length) failures.push(`${w}×${h} ${path}: ${[...new Set(bad)].join('; ')}`);
  }
  await ctx.close();
}
await browser.close();
if (failures.length) {
  console.error(failures.join('\n'));
  process.exitCode = 1;
} else {
  console.log(`layout ok (${paths.length} pages × ${SIZES.length} sizes)`);
}
