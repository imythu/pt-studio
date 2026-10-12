// 浏览器端到端（由 crates/server/tests/e2e_web.rs 启动内核后调用）：
// 插件前端经 import map 加载并渲染到插槽、注册页邀请码插槽、插件页面路由、后台设置保存、CSP 无违规。
import { existsSync } from 'node:fs';
import { chromium } from 'playwright';

const BASE = process.env.PT_BASE;
const CODE = process.env.PT_INVITE_CODE;
const TID = process.env.PT_TORRENT_ID;
const ADMIN = process.env.PT_ADMIN_USER;
const PASS = 'correct horse';

const fallback = '/root/.cache/ms-playwright/chromium-1243/chrome-linux64/chrome';
const executablePath = process.env.PT_CHROME ?? (existsSync(fallback) ? fallback : undefined);
const browser = await chromium.launch({ executablePath });
const problems = [];
const pages = [];

async function newPage() {
  const ctx = await browser.newContext({ locale: 'zh-CN' });
  const page = await ctx.newPage();
  pages.push(page);
  page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
  page.on('console', (m) => {
    const t = m.text();
    // 资源加载失败按响应记录（见下），这里只收集脚本错误
    if (m.type() === 'error' && !/Failed to load resource/.test(t)) problems.push(`console: ${t}`);
    if (/Content Security Policy|Refused to/.test(t)) problems.push(`csp: ${t}`);
  });
  page.on('response', (r) => {
    // 本测试未配置 Meilisearch：搜索返回 503（页面按错误文案展示）
    if (r.status() >= 500 && !r.url().includes('/api/p/search-meilisearch/')) problems.push(`http ${r.status()}: ${r.url()}`);
  });
  return page;
}

async function login(page, user) {
  await page.goto(`${BASE}/login`);
  await page.getByLabel('用户名').fill(user);
  await page.getByLabel('密码').fill(PASS);
  await page.getByRole('button', { name: '登录' }).click();
  await page.waitForURL(`${BASE}/`);
  await page.getByRole('link', { name: user }).waitFor();
}

function step(name) {
  console.log(`• ${name}`);
}

try {
  // 1. 注册页：invite 插件的邀请码字段（?invite= 预填）→ 注册
  step('register with invite slot');
  const p = await newPage();
  const resp = await p.goto(`${BASE}/register?invite=${encodeURIComponent(CODE)}`);
  const csp = resp?.headers()['content-security-policy'] ?? '';
  if (!/script-src 'self' 'nonce-[0-9a-f]{32}'/.test(csp)) throw new Error(`missing CSP nonce: ${csp}`);
  const codeInput = p.getByLabel('邀请码');
  await codeInput.waitFor();
  if ((await codeInput.inputValue()) !== CODE) throw new Error('invite code was not prefilled');
  await p.getByText(/有效 · 邀请人/).waitFor();
  await p.getByLabel('用户名').fill('web_bob');
  await p.getByLabel('邮箱').fill('web_bob@example.com');
  await p.getByLabel('密码').fill(PASS);
  await p.getByRole('button', { name: '注册' }).click();
  await p.waitForURL(`${BASE}/login`);

  // 2. 登录；用户菜单插槽（invite）
  step('login and user menu slot');
  await login(p, 'web_bob');
  await p.getByRole('link', { name: /邀请（\d+）/ }).waitFor();

  // 3. 个人页：levels 插槽显示等级
  step('profile slots');
  await p.getByRole('link', { name: 'web_bob' }).click();
  await p.getByText('等级').waitFor();
  await p.getByText('用户', { exact: true }).waitFor({ timeout: 15000 });

  // 4. 种子详情：hnr / promotion 插槽、下载链接
  step('torrent detail slots');
  await p.goto(`${BASE}/torrents/${TID}`);
  await p.getByRole('heading', { name: 'Web.Test.Torrent.2026.1080p' }).waitFor();
  await p.getByText(/H&R|免 H&R/).first().waitFor();
  const dl = p.getByRole('link', { name: '下载种子' });
  if ((await dl.getAttribute('href')) !== `/api/p/torrent/v1/torrents/${TID}/download`) throw new Error('download link');

  // 5. 插件页面路由
  step('plugin route /p/invite/');
  await p.getByRole('link', { name: /邀请（\d+）/ }).click();
  await p.getByRole('heading', { name: '我的邀请' }).waitFor();
  await p.getByText('我邀请的人').waitFor();

  // 6. 后台：插件列表与设置保存（JSON 编辑器）
  step('admin settings');
  const a = await newPage();
  await login(a, ADMIN);
  await a.getByRole('link', { name: '管理' }).click();
  await a.getByRole('cell', { name: 'invite', exact: true }).waitFor();
  await a.goto(`${BASE}/admin/p/invite/settings`);
  const editor = a.locator('textarea');
  await editor.waitFor();
  const cur = JSON.parse(await editor.inputValue());
  cur.daily_limit = 7;
  await editor.fill(JSON.stringify(cur, null, 2));
  await a.getByRole('button', { name: '保存' }).click();
  await a.getByText('已保存').waitFor();
  const saved = await a.evaluate(async () => (await fetch('/api/kernel/v1/admin/plugins/invite/settings')).json());
  if (saved.settings.daily_limit !== 7) throw new Error(`settings not saved: ${JSON.stringify(saved)}`);

  // 6b. schema 表单（hnr 声明了 settings_schema）：越界值被客户端拦下，合法值保存
  step('schema settings form');
  await a.goto(`${BASE}/admin/p/hnr/settings`);
  const max = a.getByText('计入上限（达到后限制下载）').locator('..').locator('input');
  await max.waitFor();
  await max.fill('0');
  await a.getByRole('button', { name: '保存' }).click();
  await a.getByText('must be ≥ 1').waitFor();
  await max.fill('3');
  await a.getByRole('button', { name: '保存' }).click();
  await a.getByText('已保存').first().waitFor();
  const hs = await a.evaluate(async () => (await fetch('/api/kernel/v1/admin/plugins/hnr/settings')).json());
  if (hs.settings.max_hnr !== 3 || hs.settings.window_days !== 14) throw new Error(`hnr settings: ${JSON.stringify(hs.settings)}`);

  // 7. 普通用户进不了后台
  step('non-staff admin access');
  await p.goto(`${BASE}/admin`);
  await p.getByText('页面不存在').waitFor();

  if (problems.length) throw new Error(`browser problems:\n${problems.join('\n')}`);
  console.log('e2e ok');
} catch (e) {
  console.error(e instanceof Error ? e.stack : e);
  if (problems.length) console.error(problems.join('\n'));
  for (const pg of pages) {
    console.error(`--- ${pg.url()}\n${(await pg.locator('body').innerText().catch(() => '')).slice(0, 1500)}`);
  }
  process.exitCode = 1;
} finally {
  await browser.close();
}
