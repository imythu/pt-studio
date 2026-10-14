// 真实 BT 客户端端到端：对正在运行的站点 + pt-tracker，用两个 transmission-cli 实例（做种 / 下载）完成一次真实传输，
// 检查文件完整、tracker 记账到 accounting。
//
//   PT_BASE=http://localhost:8080 node web/tests/bt-real.mjs
//
// 需要 transmission-cli 与 transmission-create（Debian：apt install transmission-cli）。
import { createHash, randomBytes } from 'node:crypto';
import { spawn, execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const BASE = process.env.PT_BASE ?? 'http://localhost:8080';
const SIZE = Number(process.env.PT_BT_SIZE ?? 32 << 20);
const PASS = 'correct horse battery';
// Transmission 4 忽略回环地址上的 peer：本机测试时把 announce 地址换成本机的局域网 IP（announce 不在 info 中，hash 不变）
const HOST_IP = process.env.PT_BT_HOST ?? execFileSync('hostname', ['-I']).toString().trim().split(/\s+/)[0];
const tag = Date.now() % 1000000;
const work = join(tmpdir(), `pt-bt-${tag}`);
const step = (s) => console.log(`• ${s}`);

async function api(user, method, path, body, extra = {}) {
  const headers = { ...extra, origin: BASE };
  if (user) Object.assign(headers, { cookie: user.cookie, 'x-csrf-token': user.csrf, 'sec-fetch-site': 'same-origin' });
  if (body && !(body instanceof FormData)) {
    headers['content-type'] = 'application/json';
    body = JSON.stringify(body);
  }
  const r = await fetch(`${BASE}${path}`, { method, headers, body });
  return r;
}

async function account(name) {
  let r = await api(null, 'POST', '/api/p/identity/v1/users', { username: name, email: `${name}@example.com`, password: PASS });
  if (r.status !== 201) throw new Error(`register ${name}: ${r.status} ${await r.text()}`);
  r = await api(null, 'POST', '/api/p/identity/v1/sessions', { username: name, password: PASS });
  const j = await r.json();
  const cookie = r.headers.getSetCookie().map((c) => c.split(';')[0]).join('; ');
  return { id: j.user.id, cookie, csrf: j.csrf_token, name };
}

function transmission(dir, torrent, port, log) {
  mkdirSync(join(dir, 'cfg'), { recursive: true });
  const p = spawn('transmission-cli', ['-g', join(dir, 'cfg'), '-w', dir, '-p', String(port), '-er', torrent], { stdio: ['ignore', 'pipe', 'pipe'] });
  const out = [];
  p.stdout.on('data', (d) => out.push(d.toString()));
  p.stderr.on('data', (d) => out.push(d.toString()));
  p.log = () => out.join('').slice(-2000);
  return p;
}

async function stats(user) {
  return (await api(user, 'GET', '/api/p/accounting/v1/users/me/stats')).json();
}

const procs = [];
try {
  rmSync(work, { recursive: true, force: true });
  mkdirSync(join(work, 'seed'), { recursive: true });
  mkdirSync(join(work, 'leech'), { recursive: true });
  step(`creating ${SIZE >> 20} MiB payload and a private torrent`);
  const name = `bt-real-${tag}.bin`;
  const data = randomBytes(SIZE);
  writeFileSync(join(work, 'seed', name), data);
  const raw = join(work, 'raw.torrent');
  execFileSync('transmission-create', ['-p', '-t', 'http://placeholder.invalid/announce', '-o', raw, join(work, 'seed', name)], { stdio: 'ignore' });

  step('registering seeder and leecher');
  const seeder = await account(`btseed${tag}`);
  const leecher = await account(`btleech${tag}`);

  step('uploading as seeder');
  const fd = new FormData();
  fd.append('meta', JSON.stringify({ type_id: 'generic', title: `BT.Real.Test.${tag}`, fields: { kind: 'software', title: `BT Real ${tag}`, platform: 'linux' } }));
  fd.append('torrent', new Blob([readFileSync(raw)]), 't.torrent');
  let r = await api(seeder, 'POST', '/api/p/torrent/v1/torrents', fd, { 'idempotency-key': `bt-${tag}` });
  if (r.status !== 201) throw new Error(`upload: ${r.status} ${await r.text()}`);
  const tid = (await r.json()).id;

  step('downloading each user’s .torrent (own passkey)');
  for (const [u, file] of [
    [seeder, 'seed.torrent'],
    [leecher, 'leech.torrent'],
  ]) {
    r = await api(u, 'GET', `/api/p/torrent/v1/torrents/${tid}/download`);
    if (r.status !== 200) throw new Error(`download ${u.name}: ${r.status}`);
    writeFileSync(join(work, file), Buffer.from(await r.arrayBuffer()));
    execFileSync('transmission-edit', ['-r', 'localhost', HOST_IP, join(work, file)], { stdio: 'ignore' });
  }
  // tracker 同步（事件投递）通常 < 1 秒
  await new Promise((res) => setTimeout(res, 1500));

  step('starting transmission: seeder (verifies existing data) and leecher');
  procs.push(transmission(join(work, 'seed'), join(work, 'seed.torrent'), 51413, 'seed'));
  await new Promise((res) => setTimeout(res, 3000));
  procs.push(transmission(join(work, 'leech'), join(work, 'leech.torrent'), 51414, 'leech'));

  step('waiting for the leecher to finish');
  const want = createHash('sha256').update(data).digest('hex');
  const target = join(work, 'leech', name);
  let ok = false;
  for (let i = 0; i < 240; i++) {
    await new Promise((res) => setTimeout(res, 1000));
    if (existsSync(target) && readFileSync(target).length === SIZE && createHash('sha256').update(readFileSync(target)).digest('hex') === want) {
      ok = true;
      console.log(`  complete after ${i + 1}s, sha256 matches`);
      break;
    }
  }
  if (!ok) throw new Error(`leecher did not finish.\n--- seeder ---\n${procs[0].log()}\n--- leecher ---\n${procs[1].log()}`);

  step('stopping clients (final announce) and checking accounting');
  for (const p of procs) p.kill('SIGINT');
  await new Promise((res) => setTimeout(res, 4000));
  let s = null;
  let l = null;
  for (let i = 0; i < 30; i++) {
    s = await stats(seeder);
    l = await stats(leecher);
    if (s.uploaded >= SIZE * 0.9 && l.downloaded >= SIZE * 0.9) break;
    await new Promise((res) => setTimeout(res, 1000));
  }
  console.log(`  seeder uploaded ${s.uploaded}, leecher downloaded ${l.downloaded} (payload ${SIZE})`);
  if (!(s.uploaded >= SIZE * 0.9 && l.downloaded >= SIZE * 0.9)) throw new Error('traffic was not accounted');
  console.log('real BitTorrent transfer through pt-tracker passed');
} catch (e) {
  console.error(e.message ?? e);
  process.exitCode = 1;
} finally {
  for (const p of procs) p.kill('SIGKILL');
}
