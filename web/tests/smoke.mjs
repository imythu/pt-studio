// 对一个正在运行的站点做冒烟检查（浏览器）：注册 → 授予管理员 → 登录 → 发布种子 → 列表 / 详情 / 下载 → 个人页 → 后台。
//
//   PT_BASE=http://localhost:8080 PT_SERVER=target/release/pt-server PT_CONFIG=.dev/site/site.toml \
//     node web/tests/smoke.mjs [截图目录]
//
// PT_SERVER / PT_CONFIG 用于执行 `pt-server user grant-role`；不给时跳过后台检查。
import { createHash, randomBytes } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { chromium } from 'playwright';

const BASE = process.env.PT_BASE ?? 'http://localhost:8080';
const SHOTS = process.argv[2];
const PASS = 'correct horse battery';
const USER = process.env.PT_SMOKE_USER ?? `admin${Date.now() % 100000}`;

// ---------- 生成一个 v1 单文件种子 ----------
function benc(v) {
  if (typeof v === 'number') return Buffer.from(`i${v}e`);
  if (Buffer.isBuffer(v)) return Buffer.concat([Buffer.from(`${v.length}:`), v]);
  if (typeof v === 'string') return benc(Buffer.from(v));
  if (Array.isArray(v)) return Buffer.concat([Buffer.from('l'), ...v.map(benc), Buffer.from('e')]);
  const keys = Object.keys(v).sort();
  return Buffer.concat([Buffer.from('d'), ...keys.flatMap((k) => [benc(k), benc(v[k])]), Buffer.from('e')]);
}
function makeTorrent(name, size) {
  const plen = 1 << 18;
  const pieces = Buffer.concat(
    Array.from({ length: Math.ceil(size / plen) }, () => createHash('sha1').update(randomBytes(32)).digest()),
  );
  const file = join(tmpdir(), `${name}.torrent`);
  writeFileSync(file, benc({ announce: 'http://example.invalid/announce', info: { name: `${name}.mkv`, 'piece length': plen, pieces, length: size, private: 1 } }));
  return file;
}

const fallback = '/root/.cache/ms-playwright/chromium-1243/chrome-linux64/chrome';
const browser = await chromium.launch({ executablePath: process.env.PT_CHROME ?? (existsSync(fallback) ? fallback : undefined) });
const ctx = await browser.newContext({ locale: 'zh-CN', viewport: { width: 1366, height: 860 } });
const p = await ctx.newPage();
const problems = [];
p.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`));
p.on('console', (m) => {
  if (m.type() === 'error' && !/Failed to load resource/.test(m.text())) problems.push(`console: ${m.text()}`);
});
p.on('response', (r) => {
  if (r.status() >= 500) problems.push(`http ${r.status()}: ${r.url()}`);
});
p.on('dialog', (d) => void d.accept());
if (SHOTS) mkdirSync(SHOTS, { recursive: true });
let n = 0;
async function shot(name) {
  if (!SHOTS) return;
  await p.waitForLoadState('networkidle').catch(() => undefined);
  await p.screenshot({ path: join(SHOTS, `${String(++n).padStart(2, '0')}-${name}.png`), fullPage: true });
}
const step = (s) => console.log(`• ${s}`);

try {
  step(`home ${BASE}`);
  await p.goto(`${BASE}/`);
  await shot('home-anonymous');

  step(`register ${USER}`);
  await p.goto(`${BASE}/register`);
  await p.getByLabel('用户名').fill(USER);
  await p.getByLabel('邮箱').fill(`${USER}@example.com`);
  await p.getByLabel('密码').fill(PASS);
  await shot('register');
  await p.getByRole('button', { name: '注册' }).click();
  await p.waitForURL(`${BASE}/login`);

  const admin = Boolean(process.env.PT_SERVER && process.env.PT_CONFIG);
  if (admin) {
    step('grant admin via pt-server user grant-role');
    console.log('  ' + execFileSync(process.env.PT_SERVER, ['--config', process.env.PT_CONFIG, 'user', 'grant-role', USER, 'admin']).toString().trim());
  }

  step('login');
  await p.getByLabel('用户名').fill(USER);
  await p.getByLabel('密码').fill(PASS);
  await p.getByRole('button', { name: '登录' }).click();
  await p.waitForURL(`${BASE}/`);
  await p.getByRole('link', { name: USER }).waitFor();
  await shot('home-logged-in');

  step('upload a movie');
  const title = `Smoke.Test.Movie.${new Date().getFullYear()}.1080p.WEB-DL.H264.AAC-PTS`;
  await p.goto(`${BASE}/upload`);
  await p.getByLabel('类型').selectOption('movie');
  await p.locator('input[type=file]').setInputFiles(makeTorrent(`smoke-${Date.now()}`, 700 << 20));
  await p.locator('input[name=title]').fill(title);
  await shot('upload');
  await p.getByRole('button', { name: '发布' }).click();
  await p.waitForURL(/\/torrents\/\d+$/);
  await p.getByText('资源信息').waitFor();
  await shot('torrent-detail');

  step('download the .torrent (passkey injected)');
  const [dl] = await Promise.all([p.waitForEvent('download'), p.getByRole('link', { name: '下载种子' }).click()]);
  const path = await dl.path();
  const head = execFileSync('head', ['-c', '200', path]).toString('latin1');
  if (!/announce\d+:http/.test(head) || !/\/[0-9a-f]{32}/.test(head)) throw new Error(`downloaded file has no passkey announce: ${head}`);

  step('torrent list');
  await p.goto(`${BASE}/torrents`);
  await p.getByText(title).first().waitFor({ timeout: 15000 });
  await shot('torrent-list');

  step('profile');
  await p.getByRole('link', { name: USER }).click();
  await p.getByText('等级').waitFor();
  await shot('profile');

  if (admin) {
    step('admin');
    await p.getByRole('link', { name: '管理' }).click();
    await p.waitForURL(/\/admin/);
    await shot('admin');
  }
  if (problems.length) throw new Error(`browser problems:\n${problems.join('\n')}`);
  console.log('smoke test passed');
} catch (e) {
  await shot('failure').catch(() => undefined);
  console.error(problems.join('\n'));
  console.error(e);
  process.exitCode = 1;
} finally {
  await browser.close();
}
