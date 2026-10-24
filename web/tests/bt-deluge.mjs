// Deluge 兼容性：对正在运行的站点 + pt-tracker 做真实传输——Transmission 做种 → Deluge 下载，Deluge 做种 → Transmission 下载；
// 检查文件完整、两端记账、Deluge 看到的 tracker 状态。给出管理员账号时再验证客户端规则：加一条拒绝 Deluge 的规则后，
// Deluge 重新 announce 得到 `client not allowed`，被拒客户端排行出现 Deluge，最后恢复原规则。
//
//   PT_BASE=http://localhost:31251 PT_ADMIN_USER=admin PT_ADMIN_PASS=... node web/tests/bt-deluge.mjs
//
// 需要 deluged（2.x，含 Python 包；Ubuntu `apt install deluged`）与 transmission-cli / transmission-create / transmission-edit。
// 多网卡的机器用 PT_BT_IFACE=eth0 让 Deluge 只在一个网卡上监听与 announce。
import { createHash, randomBytes } from 'node:crypto';
import { spawn, execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const BASE = process.env.PT_BASE ?? 'http://localhost:31251';
const SIZE = Number(process.env.PT_BT_SIZE ?? 16 << 20);
const PASS = 'correct horse battery';
// 两个客户端都在本机：announce 地址换成局域网 IP（Transmission 忽略回环 peer）
const HOST_IP = process.env.PT_BT_HOST ?? execFileSync('hostname', ['-I']).toString().trim().split(/\s+/)[0];
const tag = Date.now() % 1000000;
const work = join(tmpdir(), `pt-de-${tag}`);
const DE_RPC = 58900 + (tag % 1000);
const RPC = join(dirname(fileURLToPath(import.meta.url)), 'deluge-rpc.py');
// 客户端直连 tracker：不继承代理环境变量（Transmission 的 libcurl 会使用 http_proxy）
const CLIENT_ENV = Object.fromEntries(Object.entries(process.env).filter(([k]) => !/_proxy$/i.test(k)));
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

async function login(name, password) {
  const r = await api(null, 'POST', '/api/p/identity/v1/sessions', { username: name, password });
  if (!r.ok) throw new Error(`login ${name}: ${r.status} ${await r.text()}`);
  const j = await r.json();
  const cookie = r.headers.getSetCookie().map((c) => c.split(';')[0]).join('; ');
  return { id: j.user.id, cookie, csrf: j.csrf_token, name };
}

async function account(name) {
  const r = await api(null, 'POST', '/api/p/identity/v1/users', { username: name, email: `${name}@example.com`, password: PASS });
  if (r.status !== 201) throw new Error(`register ${name}: ${r.status} ${await r.text()}`);
  return login(name, PASS);
}

const stats = async (u) => (await api(u, 'GET', '/api/p/accounting/v1/users/me/stats')).json();

function transmission(dir, torrent, port) {
  mkdirSync(join(dir, 'cfg'), { recursive: true });
  const p = spawn('transmission-cli', ['-g', join(dir, 'cfg'), '-w', dir, '-p', String(port), torrent], { stdio: ['ignore', 'pipe', 'pipe'], env: CLIENT_ENV });
  const out = [];
  p.stdout.on('data', (d) => out.push(d.toString()));
  p.stderr.on('data', (d) => out.push(d.toString()));
  p.log = () => out.join('').slice(-2000);
  return p;
}

/** deluged：独立配置目录、固定 RPC 账号 */
function deluged(cfg) {
  mkdirSync(cfg, { recursive: true });
  writeFileSync(join(cfg, 'auth'), 'pt:pt:10\n');
  const p = spawn('deluged', ['-c', cfg, '-p', String(DE_RPC), '-d', '-L', 'warning'], { stdio: ['ignore', 'pipe', 'pipe'], env: CLIENT_ENV });
  const out = [];
  p.stdout.on('data', (d) => out.push(d.toString()));
  p.stderr.on('data', (d) => out.push(d.toString()));
  p.log = () => out.join('').slice(-2000);
  return p;
}

function de(method, ...args) {
  let out;
  try {
    out = execFileSync('python3', [RPC, String(DE_RPC), 'pt', 'pt', method, JSON.stringify(args)], { stdio: ['ignore', 'pipe', 'pipe'] }).toString();
  } catch (e) {
    out = e.stdout?.toString() ?? '';
  }
  const j = JSON.parse(out || '{"error":"no output"}');
  if (j.error) throw new Error(`deluge ${method}: ${j.error}`);
  return j.ok;
}

async function deReady() {
  for (let i = 0; i < 60; i++) {
    try {
      return de('daemon.get_version');
    } catch {
      await sleep(500);
    }
  }
  throw new Error('deluged did not start');
}

/** 关闭 DHT / LSD / PeX / UPnP，只用种子里的 tracker */
function deConfigure(port, dir) {
  const iface = process.env.PT_BT_IFACE;
  de('core.set_config', {
    listen_ports: [port, port],
    random_port: false,
    dht: false,
    lsd: false,
    upnp: false,
    natpmp: false,
    utpex: false,
    download_location: dir,
    ...(iface ? { listen_interface: iface, outgoing_interface: iface } : {}),
  });
}

const deAdd = (file, savepath) => de('core.add_torrent_file', 't.torrent', readFileSync(file).toString('base64'), { download_location: savepath });
/** libtorrent 的强制 announce 仍受 tracker 的 min_interval 约束：用暂停（stopped）+ 继续（started）立即汇报 */
async function reannounce(id) {
  de('core.pause_torrents', [id]);
  await sleep(1500);
  de('core.resume_torrents', [id]);
}
const deStatus = (id) => de('core.get_torrent_status', id, ['state', 'progress', 'tracker_status', 'total_uploaded', 'name']);

/** 制作 payload 与私有种子，作为 owner 发布，返回 { data, name, files: {user: torrentPath} } */
async function publish(owner, users, label) {
  const dir = join(work, label);
  mkdirSync(join(dir, 'src'), { recursive: true });
  const name = `de-${label}-${tag}.bin`;
  const data = randomBytes(SIZE);
  writeFileSync(join(dir, 'src', name), data);
  const raw = join(dir, 'raw.torrent');
  execFileSync('transmission-create', ['-p', '-t', 'http://placeholder.invalid/announce', '-o', raw, join(dir, 'src', name)], { stdio: 'ignore' });
  const fd = new FormData();
  fd.append('meta', JSON.stringify({ type_id: 'generic', title: `DE.Compat.${label}.${tag}`, fields: { kind: 'software', title: `DE ${label} ${tag}`, platform: 'linux' } }));
  fd.append('torrent', new Blob([readFileSync(raw)]), 't.torrent');
  const r = await api(owner, 'POST', '/api/p/torrent/v1/torrents', fd, { 'idempotency-key': `de-${label}-${tag}` });
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

async function waitTracker(id, ok, what) {
  let st;
  for (let i = 0; i < 60; i++) {
    st = deStatus(id);
    if (ok(st.tracker_status ?? '')) return st;
    await sleep(1000);
  }
  throw new Error(`${what}: Deluge tracker status ${JSON.stringify(st)}`);
}

const procs = [];
let restore = null;
try {
  rmSync(work, { recursive: true, force: true });
  mkdirSync(work, { recursive: true });
  step('starting deluged');
  const deProc = deluged(join(work, 'de'));
  procs.push(deProc);
  console.log(`  Deluge ${await deReady()}, libtorrent ${de('core.get_libtorrent_version')}`);
  deConfigure(51435, join(work, 'de-dl'));

  step('registering users');
  const tr = await account(`detr${tag}`);
  const dl = await account(`dede${tag}`);

  // 第一轮：Transmission 做种，Deluge 下载
  step('round 1: Transmission seeds, Deluge downloads');
  const a = await publish(tr, [tr, dl], 'a');
  await sleep(1500);
  const before1 = { tr: (await stats(tr)).uploaded, dl: (await stats(dl)).downloaded };
  const seedA = transmission(join(a.dir, 'src'), a.files[tr.name], 51433);
  procs.push(seedA);
  await sleep(3000);
  mkdirSync(join(a.dir, 'de'), { recursive: true });
  const idA = deAdd(a.files[dl.name], join(a.dir, 'de'));
  await waitFile(join(a.dir, 'de', a.name), a.data, 'Deluge leech', () => `--- transmission ---\n${seedA.log()}\n--- deluge ---\n${deProc.log()}\n${JSON.stringify(deStatus(idA))}`);
  const st = await waitTracker(idA, (s) => s.startsWith('Announce OK'), 'round 1');
  console.log(`  Deluge tracker status: ${st.tracker_status}`);
  seedA.kill('SIGINT');
  await waitTraffic({ user: tr, before: before1.tr }, { user: dl, before: before1.dl }, 'round 1');
  de('core.remove_torrent', idA, false);

  // 第二轮：Deluge 做种（校验已有文件后做种），Transmission 下载
  step('round 2: Deluge seeds, Transmission downloads');
  const b = await publish(dl, [dl, tr], 'b');
  await sleep(1500);
  const before2 = { dl: (await stats(dl)).uploaded, tr: (await stats(tr)).downloaded };
  const idB = deAdd(b.files[dl.name], join(b.dir, 'src'));
  await sleep(3000);
  mkdirSync(join(b.dir, 'tr'), { recursive: true });
  const leechB = transmission(join(b.dir, 'tr'), b.files[tr.name], 51434);
  procs.push(leechB);
  await waitFile(join(b.dir, 'tr', b.name), b.data, 'Transmission leech', () => `--- transmission ---\n${leechB.log()}\n--- deluge ---\n${JSON.stringify(deStatus(idB))}`);
  leechB.kill('SIGINT');
  // 暂停（stopped announce）把做种期间的上传量报上去，再继续做种
  de('core.pause_torrents', [idB]);
  await sleep(1500);
  await waitTraffic({ user: dl, before: before2.dl }, { user: tr, before: before2.tr }, 'round 2');
  de('core.resume_torrents', [idB]);
  await waitTracker(idB, (s) => s.startsWith('Announce OK'), 'round 2 resume');

  // 第三轮：客户端规则（需要管理员）
  if (process.env.PT_ADMIN_USER && process.env.PT_ADMIN_PASS) {
    step('round 3: a deny rule for Deluge takes effect on the next announce');
    const admin = await login(process.env.PT_ADMIN_USER, process.env.PT_ADMIN_PASS);
    const RULES = '/api/p/tracker-bridge/v1/admin/client-rules';
    const cur = await (await api(admin, 'GET', RULES)).json();
    if (!cur.rules) throw new Error(`read rules: ${JSON.stringify(cur)}`);
    restore = { admin, rules: cur.rules, default_action: cur.default_action };
    const put = async (body, version) => {
      const r = await api(admin, 'PUT', RULES, body, { 'if-match': `"${version}"` });
      if (!r.ok) throw new Error(`save rules: ${r.status} ${await r.text()}`);
      return r.json();
    };
    const denied = await put(
      { default_action: cur.default_action, rules: [{ name: 'Deluge（兼容性测试临时拒绝）', action: 'deny', pid_regex: '^-DE(?P<ver>[0-9A-Za-z]{4})-', min_ver: '2.0', enabled: true, note: '' }, ...cur.rules] },
      cur.version,
    );
    restore.version = denied.version;
    await sleep(1500);
    await reannounce(idB);
    const s3 = await waitTracker(idB, (s) => s.includes('client not allowed'), 'round 3');
    console.log(`  Deluge tracker status: ${s3.tracker_status}`);
    let top;
    for (let i = 0; i < 20 && !top; i++) {
      await sleep(1000);
      const d = await (await api(admin, 'GET', '/api/p/tracker-bridge/v1/admin/denied-clients')).json();
      top = d.items?.find((x) => x.peer_id_prefix.startsWith('-DE'));
    }
    if (!top) throw new Error('Deluge did not show up in the denied client ranking');
    console.log(`  denied ranking: ${top.peer_id_prefix} "${top.user_agent}" × ${top.count}`);
    const back = await put({ default_action: restore.default_action, rules: restore.rules }, restore.version);
    restore = null;
    console.log(`  rules restored (version ${back.version})`);
    await sleep(1500);
    await reannounce(idB);
    await waitTracker(idB, (s) => s.startsWith('Announce OK'), 'round 3 restore');
  } else {
    step('round 3 skipped: set PT_ADMIN_USER / PT_ADMIN_PASS to test client rules');
  }
  de('core.pause_torrents', [idB]);
  console.log('Deluge compatibility passed');
} catch (e) {
  console.error(e.message ?? e);
  process.exitCode = 1;
} finally {
  if (restore?.version) {
    await api(restore.admin, 'PUT', '/api/p/tracker-bridge/v1/admin/client-rules', { default_action: restore.default_action, rules: restore.rules }, { 'if-match': `"${restore.version}"` }).catch(() => undefined);
  }
  for (const p of procs) p.kill('SIGKILL');
}
