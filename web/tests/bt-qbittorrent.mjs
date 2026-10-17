// qBittorrent 兼容性：对正在运行的站点 + pt-tracker 做两轮真实传输——Transmission 做种 → qBittorrent 下载，
// qBittorrent 做种 → Transmission 下载；检查文件完整、两端记账、qBittorrent 看到的 tracker 状态。
//
//   PT_BASE=http://localhost:31251 node web/tests/bt-qbittorrent.mjs
//
// 需要 qbittorrent-nox（≥ 4.6，WebUI API v2）与 transmission-cli / transmission-create / transmission-edit。
// 多网卡的机器用 PT_BT_IFACE=eth0 让 qBittorrent 只绑定一个网卡。
import { createHash, randomBytes } from 'node:crypto';
import { spawn, execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const BASE = process.env.PT_BASE ?? 'http://localhost:31251';
const SIZE = Number(process.env.PT_BT_SIZE ?? 16 << 20);
const PASS = 'correct horse battery';
// 两个客户端都在本机：announce 地址换成局域网 IP（Transmission 忽略回环 peer）
const HOST_IP = process.env.PT_BT_HOST ?? execFileSync('hostname', ['-I']).toString().trim().split(/\s+/)[0];
const tag = Date.now() % 1000000;
const work = join(tmpdir(), `pt-qb-${tag}`);
const QB_UI = 18080 + (tag % 1000);
const step = (s) => console.log(`• ${s}`);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function api(user, method, path, body, extra = {}) {
  const headers = { ...extra, origin: BASE };
  if (user) Object.assign(headers, { cookie: user.cookie, 'x-csrf-token': user.csrf, 'sec-fetch-site': 'same-origin' });
  if (body && !(body instanceof FormData)) {
    headers['content-type'] = 'application/json';
    body = JSON.stringify(body);
  }
  return fetch(`${BASE}${path}`, { method, headers, body });
}

async function account(name) {
  let r = await api(null, 'POST', '/api/p/identity/v1/users', { username: name, email: `${name}@example.com`, password: PASS });
  if (r.status !== 201) throw new Error(`register ${name}: ${r.status} ${await r.text()}`);
  r = await api(null, 'POST', '/api/p/identity/v1/sessions', { username: name, password: PASS });
  const j = await r.json();
  const cookie = r.headers.getSetCookie().map((c) => c.split(';')[0]).join('; ');
  return { id: j.user.id, cookie, csrf: j.csrf_token, name };
}

const stats = async (u) => (await api(u, 'GET', '/api/p/accounting/v1/users/me/stats')).json();

function transmission(dir, torrent, port) {
  mkdirSync(join(dir, 'cfg'), { recursive: true });
  const p = spawn('transmission-cli', ['-g', join(dir, 'cfg'), '-w', dir, '-p', String(port), '-er', torrent], { stdio: ['ignore', 'pipe', 'pipe'] });
  const out = [];
  p.stdout.on('data', (d) => out.push(d.toString()));
  p.stderr.on('data', (d) => out.push(d.toString()));
  p.log = () => out.join('').slice(-2000);
  return p;
}

/** qbittorrent-nox：独立配置目录、本机免登录 WebUI、关闭 DHT / PeX / LSD */
function qbittorrent(profile, port) {
  const cfg = join(profile, 'qBittorrent', 'config');
  mkdirSync(cfg, { recursive: true });
  writeFileSync(
    join(cfg, 'qBittorrent.conf'),
    [
      '[LegalNotice]',
      'Accepted=true',
      '[BitTorrent]',
      `Session\\Port=${port}`,
      'Session\\DHTEnabled=false',
      'Session\\PeXEnabled=false',
      'Session\\LSDEnabled=false',
      'Session\\AnonymousModeEnabled=false',
      'Session\\QueueingSystemEnabled=false',
      ...(process.env.PT_BT_QB_ENCRYPTION ? [`Session\\Encryption=${process.env.PT_BT_QB_ENCRYPTION}`] : []),
      // 只用一个网卡：多网卡（VPN / 透明代理）时 libtorrent 会从每个网卡分别 announce，到不了 tracker 的那一路显示超时
      ...(process.env.PT_BT_IFACE ? [`Session\\Interface=${process.env.PT_BT_IFACE}`, `Session\\InterfaceName=${process.env.PT_BT_IFACE}`] : []),
      '[Preferences]',
      'WebUI\\LocalHostAuth=false',
      `WebUI\\Port=${QB_UI}`,
      'WebUI\\CSRFProtection=false',
      'WebUI\\HostHeaderValidation=false',
      '',
    ].join('\n'),
  );
  const p = spawn('qbittorrent-nox', [`--profile=${profile}`, `--webui-port=${QB_UI}`, '--confirm-legal-notice'], { stdio: ['ignore', 'pipe', 'pipe'] });
  const out = [];
  p.stdout.on('data', (d) => out.push(d.toString()));
  p.stderr.on('data', (d) => out.push(d.toString()));
  p.log = () => out.join('').slice(-2000);
  return p;
}

const qb = (path, init) => fetch(`http://127.0.0.1:${QB_UI}/api/v2${path}`, init);

async function qbReady() {
  for (let i = 0; i < 60; i++) {
    try {
      const r = await qb('/app/version');
      if (r.ok) return (await r.text()).trim();
    } catch {
      /* 还没启动 */
    }
    await sleep(500);
  }
  throw new Error('qbittorrent-nox WebUI did not start');
}

async function qbAdd(torrentFile, savepath) {
  const fd = new FormData();
  fd.append('torrents', new Blob([readFileSync(torrentFile)]), 't.torrent');
  fd.append('savepath', savepath);
  const r = await qb('/torrents/add', { method: 'POST', body: fd });
  if (!r.ok) throw new Error(`qb add: ${r.status} ${await r.text()}`);
}

const qbInfo = async () => (await (await qb('/torrents/info')).json())[0];
const qbTrackers = async (hash) => (await (await qb(`/torrents/trackers?hash=${hash}`)).json()).filter((t) => t.url.startsWith('http'));

/** 制作 payload 与私有种子，作为 owner 发布，返回 { data, name, tid, files: {user: torrentPath} } */
async function publish(owner, users, label) {
  const dir = join(work, label);
  mkdirSync(join(dir, 'src'), { recursive: true });
  const name = `qb-${label}-${tag}.bin`;
  const data = randomBytes(SIZE);
  writeFileSync(join(dir, 'src', name), data);
  const raw = join(dir, 'raw.torrent');
  execFileSync('transmission-create', ['-p', '-t', 'http://placeholder.invalid/announce', '-o', raw, join(dir, 'src', name)], { stdio: 'ignore' });
  const fd = new FormData();
  fd.append('meta', JSON.stringify({ type_id: 'generic', title: `QB.Compat.${label}.${tag}`, fields: { kind: 'software', title: `QB ${label} ${tag}`, platform: 'linux' } }));
  fd.append('torrent', new Blob([readFileSync(raw)]), 't.torrent');
  const r = await api(owner, 'POST', '/api/p/torrent/v1/torrents', fd, { 'idempotency-key': `qb-${label}-${tag}` });
  if (r.status !== 201) throw new Error(`upload: ${r.status} ${await r.text()}`);
  const tid = (await r.json()).id;
  const files = {};
  for (const u of users) {
    const d = await api(u, 'GET', `/api/p/torrent/v1/torrents/${tid}/download`);
    if (d.status !== 200) throw new Error(`download ${u.name}: ${d.status}`);
    files[u.name] = join(dir, `${u.name}.torrent`);
    writeFileSync(files[u.name], Buffer.from(await d.arrayBuffer()));
    execFileSync('transmission-edit', ['-r', 'localhost:31252', `${HOST_IP}:${process.env.PT_BT_TRACKER_PORT ?? 31252}`, files[u.name]], { stdio: 'ignore' });
  }
  return { dir, data, name, files };
}

async function waitFile(path, data, what, logs) {
  const want = createHash('sha256').update(data).digest('hex');
  for (let i = 0; i < 240; i++) {
    await sleep(1000);
    if (existsSync(path) && readFileSync(path).length === data.length && createHash('sha256').update(readFileSync(path)).digest('hex') === want) {
      console.log(`  ${what} complete after ${i + 1}s, sha256 matches`);
      return;
    }
  }
  throw new Error(`${what} did not finish\n${logs()}`);
}

async function waitTraffic(up, down, label) {
  let s;
  let l;
  for (let i = 0; i < 40; i++) {
    s = await stats(up.user);
    l = await stats(down.user);
    if (s.uploaded - up.before >= SIZE * 0.9 && l.downloaded - down.before >= SIZE * 0.9) break;
    await sleep(1000);
  }
  console.log(`  ${label}: uploaded +${s.uploaded - up.before}, downloaded +${l.downloaded - down.before} (payload ${SIZE})`);
  if (!(s.uploaded - up.before >= SIZE * 0.9 && l.downloaded - down.before >= SIZE * 0.9)) throw new Error(`${label}: traffic was not accounted`);
}

const procs = [];
try {
  rmSync(work, { recursive: true, force: true });
  mkdirSync(work, { recursive: true });
  step('starting qbittorrent-nox');
  const qbProc = qbittorrent(join(work, 'qb'), 51425);
  procs.push(qbProc);
  console.log(`  qBittorrent ${await qbReady()}`);

  step('registering users');
  const tr = await account(`qbtr${tag}`);
  const q = await account(`qbqb${tag}`);

  // 第一轮：Transmission 做种，qBittorrent 下载
  step('round 1: Transmission seeds, qBittorrent downloads');
  const a = await publish(tr, [tr, q], 'a');
  await sleep(1500);
  const before1 = { tr: (await stats(tr)).uploaded, q: (await stats(q)).downloaded };
  const seedA = transmission(join(a.dir, 'src'), a.files[tr.name], 51423);
  procs.push(seedA);
  await sleep(3000);
  mkdirSync(join(a.dir, 'qb'), { recursive: true });
  await qbAdd(a.files[q.name], join(a.dir, 'qb'));
  await waitFile(join(a.dir, 'qb', a.name), a.data, 'qBittorrent leech', async () => `--- transmission ---\n${seedA.log()}\n--- qb ---\n${qbProc.log()}`);
  const info = await qbInfo();
  const trk = await qbTrackers(info.hash);
  // qBittorrent 的 tracker 状态：2 = 工作中，没有错误消息
  if (!trk.length || trk.some((t) => t.status !== 2 || (t.msg && t.msg.length))) throw new Error(`qb tracker status: ${JSON.stringify(trk)}`);
  console.log(`  qBittorrent tracker status ok (${trk[0].num_seeds} seeds / ${trk[0].num_leeches} leeches reported)`);
  seedA.kill('SIGINT');
  await waitTraffic({ user: tr, before: before1.tr }, { user: q, before: before1.q }, 'round 1');

  // 第二轮：qBittorrent 做种（校验已有文件后做种），Transmission 下载
  step('round 2: qBittorrent seeds, Transmission downloads');
  const b = await publish(q, [q, tr], 'b');
  await sleep(1500);
  const before2 = { q: (await stats(q)).uploaded, tr: (await stats(tr)).downloaded };
  await qbAdd(b.files[q.name], join(b.dir, 'src'));
  await sleep(3000);
  mkdirSync(join(b.dir, 'tr'), { recursive: true });
  const leechB = transmission(join(b.dir, 'tr'), b.files[tr.name], 51424);
  procs.push(leechB);
  await waitFile(join(b.dir, 'tr', b.name), b.data, 'Transmission leech', async () => `--- transmission ---\n${leechB.log()}\n--- qb ---\n${qbProc.log()}`);
  leechB.kill('SIGINT');
  // qBittorrent 暂停 = stopped announce
  await qb('/torrents/stop', { method: 'POST', body: new URLSearchParams({ hashes: 'all' }) });
  await qb('/torrents/pause', { method: 'POST', body: new URLSearchParams({ hashes: 'all' }) }).catch(() => undefined);
  await waitTraffic({ user: q, before: before2.q }, { user: tr, before: before2.tr }, 'round 2');
  console.log('qBittorrent compatibility passed');
} catch (e) {
  console.error(e.message ?? e);
  process.exitCode = 1;
} finally {
  for (const p of procs) p.kill('SIGKILL');
}
