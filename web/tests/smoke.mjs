// 对一个正在运行的站点做冒烟检查（浏览器）：注册 → 授予管理员 → 登录 → 发布种子 → 列表 / 详情 / 下载 →
// 模拟 BT 客户端向 pt-tracker announce（做种、下载、完成）→ 个人页流量 → 后台。
//
//   PT_BASE=http://localhost:31251 PT_SERVER=target/release/pt-server PT_CONFIG=.dev/site/site.toml \
//     node web/tests/smoke.mjs [截图目录]
//
// PT_SERVER / PT_CONFIG 用于执行 `pt-server user grant-role`；不给时跳过后台检查。PT_SKIP_TRACKER=1 跳过 tracker 检查。
import { createHash, randomBytes } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { chromium } from 'playwright';

const BASE = process.env.PT_BASE ?? 'http://localhost:31251';
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

// ---------- 解析种子：announce 地址与 info_hash ----------
function bdecode(buf) {
  let i = 0;
  let infoRange = null;
  function val(depth, key) {
    const c = buf[i];
    if (c === 0x69) {
      const e = buf.indexOf(0x65, i);
      const n = Number(buf.subarray(i + 1, e).toString());
      i = e + 1;
      return n;
    }
    if (c === 0x6c) {
      i++;
      const out = [];
      while (buf[i] !== 0x65) out.push(val(depth + 1));
      i++;
      return out;
    }
    if (c === 0x64) {
      const start = i;
      i++;
      const out = {};
      while (buf[i] !== 0x65) {
        const k = val(depth + 1).toString();
        out[k] = val(depth + 1, k);
      }
      i++;
      if (key === 'info' && depth === 1) infoRange = [start, i];
      return out;
    }
    const colon = buf.indexOf(0x3a, i);
    const len = Number(buf.subarray(i, colon).toString());
    const s = buf.subarray(colon + 1, colon + 1 + len);
    i = colon + 1 + len;
    return s;
  }
  const root = val(0);
  return { root, infoHash: createHash('sha1').update(buf.subarray(infoRange[0], infoRange[1])).digest() };
}

/** 模拟 BT 客户端的一次 announce（qBittorrent 风格 peer_id 与 UA） */
async function announce(base, infoHash, peerId, q) {
  const enc = (b) => [...b].map((x) => '%' + x.toString(16).padStart(2, '0')).join('');
  const params = Object.entries({ port: 6881, compact: 1, ...q }).map(([k, v]) => `${k}=${v}`).join('&');
  const r = await fetch(`${base}?info_hash=${enc(infoHash)}&peer_id=${enc(peerId)}&${params}`, { headers: { 'user-agent': 'qBittorrent/4.6.5' } });
  const body = Buffer.from(await r.arrayBuffer());
  const s = body.toString('latin1');
  if (s.includes('failure reason')) throw new Error(`announce failed: ${s}`);
  return s;
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

  if (process.env.PT_SKIP_TRACKER !== '1') {
    step('BT client: seed + leech + complete via pt-tracker');
    const { readFileSync } = await import('node:fs');
    const t = bdecode(readFileSync(path));
    const url = t.root.announce.toString();
    const size = t.root.info.length;
    const seedId = Buffer.from(`-qB4650-${randomBytes(6).toString('hex')}`);
    const leechId = Buffer.from(`-qB4650-${randomBytes(6).toString('hex')}`);
    await announce(url, t.infoHash, seedId, { uploaded: 0, downloaded: 0, left: 0, event: 'started' });
    const r = await announce(url, t.infoHash, leechId, { uploaded: 0, downloaded: 0, left: size, event: 'started', port: 6882 });
    if (!/8:completei1e10:incompletei1e/.test(r)) throw new Error(`unexpected swarm counts: ${r}`);
    await announce(url, t.infoHash, leechId, { uploaded: 0, downloaded: size, left: 0, event: 'completed', port: 6882 });
    await announce(url, t.infoHash, seedId, { uploaded: size, downloaded: 0, left: 0 });
    step('traffic reaches accounting');
    let stats = null;
    for (let i = 0; i < 60; i++) {
      stats = await p.evaluate(async () => (await fetch('/api/p/accounting/v1/users/me/stats')).json());
      if (stats.uploaded > 0 && stats.downloaded > 0) break;
      await p.waitForTimeout(1000);
    }
    if (!(stats.uploaded > 0 && stats.downloaded > 0)) throw new Error(`traffic not accounted: ${JSON.stringify(stats)}`);
    console.log(`  uploaded ${stats.uploaded}, downloaded ${stats.downloaded}, ratio ${stats.ratio}`);
  }

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
