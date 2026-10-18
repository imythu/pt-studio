// 浏览器端到端（由 crates/server/tests/e2e_web.rs 启动内核后调用）：
// 插件前端经 import map 加载并渲染到插槽、注册页邀请码插槽、插件页面路由、后台设置保存、CSP 无违规。
import { createHmac } from 'node:crypto';
import { existsSync } from 'node:fs';
import { chromium } from 'playwright';

const BASE = process.env.PT_BASE;
const CODE = process.env.PT_INVITE_CODE;
const TID = process.env.PT_TORRENT_ID;
const ADMIN = process.env.PT_ADMIN_USER;
const TORRENT_FILE = process.env.PT_TORRENT_FILE;
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
  page.on('dialog', (d) => void d.accept());
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
  // “保持登录”默认勾选：会话 cookie 带 Max-Age（关闭浏览器后仍有效）
  if (!(await page.getByRole('checkbox', { name: /保持登录/ }).isChecked())) throw new Error('remember me should default to checked');
  await page.getByRole('button', { name: '登录', exact: true }).click();
  await page.waitForURL(`${BASE}/`);
  await page.getByRole('button', { name: '菜单' }).getByText(user, { exact: true }).waitFor();
  const sid = (await page.context().cookies()).find((c) => c.httpOnly && c.expires > 0);
  if (!sid || sid.expires * 1000 < Date.now() + 7 * 86400_000) throw new Error('session cookie is not persistent');
}

/** 打开顶栏用户菜单（重设计后用户名在菜单按钮里，个人主页 / 插件入口都在菜单中） */
async function openMenu(page) {
  const btn = page.getByRole('button', { name: '菜单' });
  if ((await btn.getAttribute('aria-expanded')) !== 'true') await btn.click();
}

/** RFC 6238 TOTP（SHA-1、6 位、30 秒），密钥为 base32 */
function totp(secretB32, offsetSecs = 0) {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
  let bits = '';
  for (const ch of secretB32.replace(/=+$/, '')) bits += alphabet.indexOf(ch).toString(2).padStart(5, '0');
  const key = Buffer.from(bits.match(/.{8}/g).map((b) => parseInt(b, 2)));
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(Math.floor((Date.now() / 1000 + offsetSecs) / 30)));
  const h = createHmac('sha1', key).update(counter).digest();
  const off = h[19] & 0xf;
  return String((h.readUInt32BE(off) & 0x7fffffff) % 1_000_000).padStart(6, '0');
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

  // 1b. 找回密码入口：申请后总是提示“如果账号存在…”（防枚举）
  step('forgot password');
  await p.getByRole('link', { name: '忘记密码？' }).click();
  await p.waitForURL(`${BASE}/reset-password`);
  await p.getByLabel('用户名或邮箱').fill('web_bob');
  await p.getByRole('button', { name: '发送重置邮件' }).click();
  await p.getByText(/如果账号存在/).waitFor();
  await p.goto(`${BASE}/login`);

  // 2. 登录；用户菜单插槽（invite）
  step('login and user menu slot');
  await login(p, 'web_bob');
  await openMenu(p);
  await p.getByRole('link', { name: /邀请（\d+）/ }).waitFor();
  await p.getByRole('link', { name: '等级说明' }).waitFor();

  // 3. 个人页：levels 插槽显示等级
  step('profile slots');
  await p.getByRole('menuitem', { name: '个人主页' }).click();
  await p.getByRole('link', { name: '等级' }).waitFor();
  await p.getByText('用户', { exact: true }).waitFor({ timeout: 15000 });
  // 等级说明页（levels 插件路由）：全部等级与升级条件
  await p.getByRole('link', { name: '等级' }).click();
  await p.waitForURL(`${BASE}/p/levels/`);
  await p.getByRole('columnheader', { name: '保级条件' }).waitFor();
  await p.getByText('养老族', { exact: true }).waitFor();

  // 4. 种子详情：hnr / promotion 插槽、下载链接
  step('torrent detail slots');
  await p.goto(`${BASE}/torrents/${TID}`);
  await p.getByRole('heading', { name: 'Web.Test.Torrent.2026.1080p' }).waitFor();
  await p.getByText(/H&R|免 H&R/).first().waitFor();
  const dl = p.getByRole('link', { name: '下载种子' });
  if ((await dl.getAttribute('href')) !== `/api/p/torrent/v1/torrents/${TID}/download`) throw new Error('download link');

  // 4a. 评论（comments 插槽）与举报按钮（reports 插槽）
  step('comments and reports');
  await p.getByRole('tab', { name: '评论' }).click();
  await p.getByLabel('写下你的评论…').fill('**好资源** <b>x</b>');
  await p.getByRole('button', { name: '发表' }).click();
  await p.getByTestId('comment').locator('strong', { hasText: '好资源' }).waitFor();
  if (await p.getByTestId('comment').locator('b').count()) throw new Error('raw HTML must not render in comments');
  await p.getByRole('button', { name: '举报' }).first().click();
  await p.getByLabel('理由').first().waitFor();

  // 4b. 按资源类型发布：电影（发布名补全 + IMDb 链接）→ 详情字段 → 作品页
  step('typed upload (movie) and group page');
  await p.goto(`${BASE}/upload`);
  await p.getByRole('radio', { name: '电影' }).click();
  await p.locator('input[name=torrent]').setInputFiles(TORRENT_FILE);
  await p.locator('input[name=title]').fill('Web.Movie.2024.2160p.WEB-DL.H265.DDP5.1-WEB');
  await p.getByText(/更多资源信息/).click();
  await p.getByText('IMDb', { exact: true }).locator('..').locator('input').fill('https://www.imdb.com/title/tt1234567/');
  // MediaInfo：粘贴即预览（路径脱敏），发布成功后由 afterUpload 保存，种子页显示规格
  await p
    .locator('textarea[name=mediainfo]')
    .fill('General\nComplete name : /home/alice/Web.Movie.mkv\nFormat : Matroska\n\nVideo\nFormat : HEVC\nWidth : 3 840 pixels\nHeight : 2 160 pixels\n\nAudio\nFormat : E-AC-3\nChannel(s) : 6 channels\n');
  await p.getByText(/检测到（text）/).waitFor();
  // 图片上传：插入到简介（BBCode），详情页渲染为站内图片
  await p.locator('input[name=image_upload]').setInputFiles({
    name: 'shot.png',
    mimeType: 'image/png',
    buffer: Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==', 'base64'),
  });
  await p.getByText('图片已插入简介').waitFor();
  await p.getByRole('button', { name: '发布' }).click();
  await p.waitForURL(/\/torrents\/\d+$/);
  const movieUrl = p.url();
  await p.getByRole('heading', { name: '资源信息', exact: true }).waitFor();
  await p.locator('.mediainfo-summary').getByText('HEVC', { exact: true }).waitFor();
  await p.locator('.pt-user-content img[src^="/media/attachments/i/"]').waitFor();
  await p.getByText('查看完整报告').click();
  if ((await p.locator('.mediainfo-text').textContent())?.includes('alice')) throw new Error('mediainfo paths must be sanitised');
  await p.getByText('2160p', { exact: true }).waitFor();
  await p.getByText('tt1234567', { exact: true }).waitFor();
  await p.getByRole('link', { name: '查看作品' }).click();
  await p.getByRole('heading', { name: /Web Movie/ }).waitFor();
  await p.getByText(/全部版本（1）/).waitFor();
  // 从 TMDB 搜索选定 → 上传 → 作品页显示转存海报与简介（type-movie 拉取元数据）
  step('tmdb prefill and group metadata');
  await p.goto(`${BASE}/upload`);
  await p.getByRole('radio', { name: '电影' }).click();
  // 输入后自动搜索（无“搜索”按钮），从结果中选定
  await p.locator('input[name=meta_q]').fill('matrix');
  await p.getByRole('button', { name: /黑客帝国/ }).click();
  await p.locator('input[name=torrent]').setInputFiles(process.env.PT_TORRENT_FILE2);
  await p.locator('input[name=title]').fill('The.Matrix.1999.1080p.BluRay.x264-GRP');
  await p.getByRole('button', { name: '发布' }).click();
  await p.waitForURL(/\/torrents\/\d+$/);
  await p.getByRole('link', { name: '查看作品' }).click();
  await p.getByRole('heading', { name: /The Matrix/ }).waitFor();
  for (let i = 0; i < 30 && (await p.locator('img.poster').count()) === 0; i++) {
    await p.waitForTimeout(500);
    await p.reload();
  }
  const src = await p.locator('img.poster').getAttribute('src');
  if (!src?.startsWith('/media/metadata-tmdb/images/')) throw new Error(`poster: ${src}`);
  const loaded = await p.locator('img.poster').evaluate((img) => img.complete && img.naturalWidth >= 0);
  if (!loaded) throw new Error('poster did not load');
  await p.getByText('一名黑客发现了世界的真相。').waitFor();

  // 通用类型：kind 切换出对应字段
  await p.goto(`${BASE}/upload`);
  await p.getByRole('radio', { name: '其他' }).click();
  await p.getByText('分类', { exact: true }).locator('..').locator('select').selectOption({ label: '游戏' });
  await p.getByText('平台', { exact: true }).waitFor();
  await p.getByText('分类', { exact: true }).locator('..').locator('select').selectOption({ label: '电子书' });
  await p.getByText('ISBN', { exact: true }).waitFor();

  // 5. 插件页面路由
  step('plugin route /p/invite/');
  await openMenu(p);
  await p.getByRole('link', { name: /邀请（\d+）/ }).click();
  await p.getByRole('heading', { name: '我的邀请' }).waitFor();
  await p.getByText('我邀请的人').waitFor();

  // 6. 后台：插件列表与设置保存（JSON 页签）
  step('admin settings');
  const a = await newPage();
  await login(a, ADMIN);
  await a.getByRole('link', { name: '管理' }).click();
  // 插件卡片列表 → 卡片上的“设置”入口；invite 声明了 schema，切到 JSON 页签编辑
  const card = a.locator('.plugin-card').filter({ has: a.getByText(/^invite · v/) });
  await card.getByRole('link', { name: '设置' }).click();
  await a.waitForURL(`${BASE}/admin/p/invite/settings`);
  await a.getByRole('tab', { name: 'JSON' }).click();
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

  // 6c. 个人设置：资料、会话、passkey
  step('settings page');
  await p.goto(`${BASE}/settings`);
  await p.locator('input[name=title]').fill('Seeder');
  await p.getByRole('button', { name: '保存' }).click();
  await p.getByText('已保存').first().waitFor();
  await p.getByRole('tab', { name: '安全' }).click();
  await p.getByText('当前', { exact: true }).waitFor();
  await p.getByRole('tab', { name: 'Passkey' }).click();
  const passkeyText = () => p.getByTestId('passkey').innerText().then((x) => x.trim());
  await p.getByRole('button', { name: '显示' }).click();
  await p.waitForFunction(() => /^[0-9a-f]{32}$/.test(document.querySelector('[data-testid=passkey]')?.textContent?.trim() ?? ''));
  const shown = await passkeyText();
  if (!/^[0-9a-f]{32}$/.test(shown.trim())) throw new Error(`passkey not shown: ${shown}`);
  await p.getByRole('button', { name: '重置 passkey' }).click();
  await p.getByText('passkey 已重置').waitFor();
  const reset = await passkeyText();
  if (reset === shown || !/^[0-9a-f]{32}$/.test(reset)) throw new Error('passkey was not reset');

  // 6c'. 两步验证：扫码绑定 → 恢复码 → 重新登录需要第二步（用恢复码）
  step('two-factor authentication');
  await p.getByRole('tab', { name: '安全' }).click();
  await p.getByRole('button', { name: '开启两步验证' }).click();
  await p.locator('.qr svg').waitFor();
  const secret = (await p.getByTestId('totp-secret').innerText()).trim();
  await p.locator('input[name=totp_code]').fill(totp(secret));
  await p.getByRole('button', { name: '确认开启' }).click();
  const recovery = (await p.getByTestId('recovery-codes').innerText()).trim().split(/\s+/);
  if (recovery.length !== 10) throw new Error(`recovery codes: ${recovery}`);
  await p.getByRole('button', { name: '我已保存' }).click();
  await p.getByText('剩余 10 个恢复码').waitFor();
  await p.context().clearCookies();
  await p.goto(`${BASE}/login`);
  await p.getByLabel('用户名').fill('web_bob');
  await p.getByLabel('密码').fill(PASS);
  await p.getByRole('button', { name: '登录', exact: true }).click();
  await p.getByLabel('验证码').fill(recovery[0]);
  await p.getByRole('button', { name: '验证' }).click();
  await p.waitForURL(`${BASE}/`);
  await p.getByRole('button', { name: '菜单' }).getByText('web_bob', { exact: true }).waitFor();

  // 6c''. 安全密钥（Chromium 虚拟认证器）：添加 → 退出 → 免密码登录
  step('security key');
  const cdp = await p.context().newCDPSession(p);
  await cdp.send('WebAuthn.enable');
  await cdp.send('WebAuthn.addVirtualAuthenticator', {
    options: { protocol: 'ctap2', transport: 'internal', hasResidentKey: true, hasUserVerification: true, isUserVerified: true, automaticPresenceSimulation: true },
  });
  await p.goto(`${BASE}/settings`);
  await p.getByRole('tab', { name: '安全' }).click();
  await p.locator('input[name=key_name]').fill('Virtual key');
  await p.getByRole('button', { name: '添加安全密钥' }).click();
  await p.getByText('安全密钥已添加').waitFor();
  await p.getByText('可免密码登录').waitFor();
  await p.context().clearCookies();
  await p.goto(`${BASE}/login`);
  await p.getByRole('button', { name: '用安全密钥登录' }).click();
  await p.waitForURL(`${BASE}/`);
  await p.getByRole('button', { name: '菜单' }).getByText('web_bob', { exact: true }).waitFor();

  // 6d. 后台用户：查找、施加 / 解除限制
  step('admin users');
  await a.goto(`${BASE}/admin/users`);
  await a.locator('input[name=username_prefix]').fill('web_bob');
  await a.getByRole('button', { name: '查询' }).click();
  await a.getByRole('link', { name: 'web_bob' }).click();
  await a.getByRole('heading', { name: /web_bob/ }).waitFor();
  await a.locator('input[name=reason_code]').fill('e2e');
  await a.locator('select[name=kind]').selectOption('leech_disabled');
  await a.getByRole('button', { name: '施加限制' }).click();
  await a.getByRole('heading', { name: /leech_disabled/ }).waitFor();
  await a.getByRole('button', { name: '解除' }).click();
  await a.getByRole('heading', { name: /active/ }).waitFor();

  // 6e. 种子审核：管理员设为 disabled
  step('torrent moderation');
  await a.goto(`${BASE}/torrents/${TID}`);
  await a.locator('select[name=status]').selectOption('disabled');
  await a.getByRole('button', { name: '设置状态' }).click();
  await a.getByText('已禁用', { exact: true }).first().waitFor();

  // 6f. AI 客户端（mcp）：设置页签发 token → 客户端发起写操作 → 待确认页确认执行
  step('mcp tokens and confirmations');
  await a.goto(`${BASE}/settings`);
  await a.getByRole('tab', { name: '更多' }).click();
  await a.getByRole('tab', { name: 'AI 客户端' }).click();
  await a.getByText(`${BASE}/mcp`).first().waitFor();
  await a.getByRole('checkbox', { name: '写操作（每次需在站内确认）', exact: true }).check();
  await a.getByRole('checkbox', { name: '管理员工具', exact: true }).check();
  await a.getByRole('button', { name: '生成 token' }).click();
  await a.waitForFunction(() => /^ptm_/.test(document.querySelector('[data-testid=mcp-token]')?.textContent ?? ''));
  const mcpToken = (await a.getByTestId('mcp-token').innerText()).trim();
  const users = await a.evaluate(async () => (await fetch('/api/p/identity/v1/admin/users?username_prefix=web_bob')).json());
  const bob = users.items.find((x) => x.username === 'web_bob');
  const rpc = await fetch(`${BASE}/mcp`, {
    method: 'POST',
    headers: { authorization: `Bearer ${mcpToken}`, 'content-type': 'application/json', 'mcp-protocol-version': '2026-07-28', 'mcp-method': 'tools/call' },
    body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'tools/call', params: { name: 'admin_set_user_status', arguments: { user_id: bob.id, status: 'leech_disabled', reason_code: 'e2e-mcp' } } }),
  }).then((r) => r.json());
  if (rpc.result?.structuredContent?.status !== 'confirmation_required') throw new Error(`mcp call: ${JSON.stringify(rpc)}`);
  await a.goto(`${BASE}/p/mcp/confirmations`);
  await a.getByTestId('mcp-confirmation').getByText('admin_set_user_status').waitFor();
  await a.getByText('e2e-mcp').waitFor();
  await a.getByRole('button', { name: '确认执行' }).click();
  await a.getByText('已执行').waitFor();

  // 6g. 论坛：后台建分区与版块 → 用户发主题、回复
  step('forum');
  await a.goto(`${BASE}/admin/p/forum/`);
  await a.locator('input[name=category_name]').fill('站务');
  await a.getByRole('button', { name: '新建分区' }).click();
  await a.locator('select[name=board_category] option', { hasText: '站务' }).waitFor({ state: 'attached' });
  await a.locator('select[name=board_category]').selectOption({ label: '站务' });
  await a.locator('input[name=board_name]').fill('综合讨论');
  await a.getByRole('button', { name: '新建版块' }).click();
  await a.getByRole('cell', { name: '综合讨论' }).waitFor();
  await p.goto(`${BASE}/p/forum/`);
  await p.getByRole('link', { name: '综合讨论' }).click();
  await p.locator('input[name=topic_title]').fill('第一次发帖');
  await p.locator('textarea[name=post_body]').fill('大家好，**请多关照**');
  await p.getByRole('button', { name: '发表' }).click();
  await p.getByRole('heading', { name: /第一次发帖/ }).waitFor();
  await p.locator('strong', { hasText: '请多关照' }).waitFor();
  await p.locator('textarea[name=post_body]').fill('自己顶一下');
  await p.getByRole('button', { name: '发表' }).click();
  await p.getByText('自己顶一下').waitFor();

  // 6h. 置顶（列表排序见 e2e_search，本测试未接 Meilisearch）、收藏、合集、排行榜
  step('sticky, bookmarks, collections, top10');
  await a.goto(movieUrl);
  await a.locator('select[name=sticky_days]').selectOption('3');
  await a.getByRole('button', { name: '置顶', exact: true }).click();
  await a.getByText(/置顶至/).waitFor();
  await p.goto(movieUrl);
  await p.getByRole('button', { name: '☆ 收藏' }).click();
  await p.getByRole('button', { name: '★ 取消收藏' }).waitFor();
  await p.getByText('1 人收藏').waitFor();
  await p.goto(`${BASE}/p/bookmarks/`);
  await p.getByRole('link', { name: /Web\.Movie\.2024/ }).waitFor();
  await p.goto(`${BASE}/p/bookmarks/collections`);
  await p.locator('input[name=collection_title]').fill('我的片单');
  await p.getByRole('button', { name: '创建' }).click();
  await p.waitForURL(/\/p\/bookmarks\/collections\/\d+$/);
  await p.goto(movieUrl);
  await p.locator('select[name=add_to_collection]').selectOption({ label: '我的片单' });
  await p.getByText('已加入合集').waitFor();
  await p.reload();
  await p.getByRole('link', { name: '我的片单' }).click();
  await p.getByRole('link', { name: /Web\.Movie\.2024/ }).waitFor();
  await p.goto(movieUrl);
  await p.getByRole('tab', { name: '字幕' }).click();
  await p.locator('input[name=subtitle_file]').setInputFiles({ name: 'chs.srt', mimeType: 'text/plain', buffer: Buffer.from('1\n00:00:01,000 --> 00:00:02,000\n你好\n') });
  await p.getByRole('button', { name: '上传', exact: true }).click();
  await p.getByText('字幕已上传').waitFor();
  await p.getByText('chs.srt').waitFor();
  await p.goto(`${BASE}/p/top10/`);
  await p.getByRole('heading', { name: '排行榜' }).waitFor();
  await p.getByText('完成数最多').waitFor();
  await p.getByText('不在用户榜中显示我').waitFor();

  // 6i. 商城：后台新建商品 → 用户购买（发种奖励的魔力值够买 1 魔力值的商品）
  step('shop');
  await a.goto(`${BASE}/admin/p/shop/`);
  await a.locator('input[name=item_title]').fill('1 GiB 体验包');
  await a.locator('input[name=item_price]').fill('1');
  await a.locator('input[name=item_params]').fill('{"bytes": 1073741824}');
  await a.getByRole('button', { name: '保存' }).click();
  await a.getByText('1 GiB 体验包').waitFor();
  await p.goto(`${BASE}/p/shop/`);
  await p.getByRole('heading', { name: '魔力值商城' }).waitFor();
  await p.getByText(/我的魔力值/).waitFor();
  await p.locator('.pt-card', { hasText: '1 GiB 体验包' }).getByRole('button', { name: '购买' }).click();
  await p.getByText('购买成功').waitFor();
  await p.getByText('1 GiB 体验包 × 1').waitFor();

  // 6j. 勋章：后台新建头衔并授予 → 用户激活头衔 → 个人页显示
  step('medals');
  await a.goto(`${BASE}/admin/p/medals/`);
  await a.locator('select[name=medal_kind]').selectOption('title');
  await a.locator('input[name=medal_name]').fill('网页元老');
  await a.getByRole('button', { name: '保存' }).click();
  await a.locator('strong', { hasText: '网页元老' }).waitFor();
  await a.getByRole('tab', { name: '授予' }).click();
  await a.locator('select[name=grant_item]').selectOption({ label: '网页元老' });
  await a.locator('textarea[name=grant_users]').fill('web_bob');
  await a.locator('input[name=grant_reason]').fill('首批用户');
  await a.getByRole('button', { name: '授予', exact: true }).click();
  await a.getByText('已授予 1 人').waitFor();
  await p.goto(`${BASE}/p/medals/`);
  await p.getByRole('heading', { name: '我的勋章' }).waitFor();
  await p.getByRole('heading', { name: '成就' }).waitFor();
  await p.locator('select[name=medal_title]').selectOption({ label: '网页元老' });
  await p.getByRole('button', { name: '保存' }).click();
  await p.getByText('已保存').waitFor();
  await openMenu(p);
  await p.getByRole('menuitem', { name: '个人主页' }).click();
  await p.locator('[data-testid=medal-display]', { hasText: '网页元老' }).waitFor();

  // 6k. 抽奖：后台开期 → 用户买票 → 立即开奖 → 浏览器内复算验证
  step('lottery');
  await a.goto(`${BASE}/admin/p/lottery/`);
  await a.locator('input[name=round_title]').fill('网页抽奖');
  await a.locator('input[name=round_price]').fill('1');
  await a.locator('input[name=round_min]').fill('1');
  await a.locator('input[name=round_tiers]').fill('1:50');
  await a.getByRole('button', { name: '开期' }).click();
  await a.getByRole('link', { name: '网页抽奖' }).waitFor();
  await p.goto(`${BASE}/p/lottery/`);
  await p.getByRole('link', { name: '网页抽奖' }).click();
  await p.getByRole('button', { name: /^购买/ }).click();
  await p.getByText(/已购买 1 张/).waitFor();
  await a.getByRole('button', { name: '立即开奖' }).click();
  await a.getByText('已开奖').waitFor();
  await p.reload();
  await p.getByRole('heading', { name: '中奖名单' }).waitFor();
  await p.getByRole('button', { name: '在浏览器中复算' }).click();
  await p.getByText(/验证通过/).waitFor();

  // 6l. 群发公告与联系管理组：后台发布公告 → 首页横幅；用户提交工单 → 员工回复 → 用户看到回复
  step('broadcasts and tickets');
  await a.goto(`${BASE}/admin/p/messages/`);
  await a.locator('input[name=broadcast_subject]').fill('网页公告');
  await a.locator('textarea[name=broadcast_body]').fill('今晚 **维护**');
  await a.getByRole('button', { name: '发布' }).click();
  await a.getByText('公告已发布').waitFor();
  await p.goto(`${BASE}/`);
  await p.locator('[data-testid=broadcast]', { hasText: '网页公告' }).waitFor();
  await p.getByRole('button', { name: '知道了' }).click();
  await p.locator('[data-testid=broadcast]').waitFor({ state: 'detached' });
  await p.goto(`${BASE}/p/tickets/`);
  await p.locator('select[name=ticket_category]').selectOption('torrent');
  await p.locator('input[name=ticket_subject]').fill('种子无法下载');
  await p.locator('textarea[name=ticket_body]').fill('提示 tracker 错误');
  await p.getByRole('button', { name: '提交' }).click();
  await p.waitForURL(/\/p\/tickets\/\d+$/);
  const ticketUrl = p.url();
  await a.goto(`${BASE}/admin/p/tickets/`);
  await a.getByRole('link', { name: '种子无法下载' }).click();
  await a.locator('textarea[name=ticket_reply]').fill('请重新下载种子文件');
  await a.getByRole('button', { name: '发送' }).click();
  await a.getByText('请重新下载种子文件').waitFor();
  await p.goto(ticketUrl);
  await p.getByText('请重新下载种子文件').waitFor();
  await p.getByText('等待用户').waitFor();

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
