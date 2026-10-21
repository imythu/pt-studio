// server-load 前端：首页侧栏「服务器负载」（用户数 / 上限、今日新注册、活跃；CPU 各核心、内存、磁盘 IO、负载均值）；
// 后台设置：用户上限、是否显示主机负载、可查看的等级与角色。
import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Checkbox, Field, Input, Note, Panel, Progress, SkeletonRows } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'server-load.ui.title': '服务器负载',
  'server-load.ui.users': '注册用户',
  'server-load.ui.of_limit': '上限 {n}',
  'server-load.ui.no_limit': '不限人数',
  'server-load.ui.load': '用户负载 {p}%',
  'server-load.ui.new_today': '今日新注册',
  'server-load.ui.active': '活跃用户',
  'server-load.ui.active_hint': '近一小时有做种或下载',
  'server-load.ui.peers': '连接',
  'server-load.ui.peers_v': '↑{s} ↓{l}',
  'server-load.ui.set_limit': '用户上限',
  'server-load.ui.set_limit_hint': '0 = 使用「注册策略」插件里的全站用户上限',
  'server-load.ui.set_who': '谁能查看',
  'server-load.ui.set_who_hint': '等级和角色都不勾选时所有用户可见；勾选后满足其一即可查看。管理员始终可见。',
  'server-load.ui.set_classes': '用户等级',
  'server-load.ui.set_roles': '角色',
  'server-load.ui.no_levels': '未安装等级插件',
  'server-load.ui.save': '保存',
  'server-load.ui.saved': '已保存',
  'server-load.ui.cpu': 'CPU',
  'server-load.ui.cores': '{n} 核',
  'server-load.ui.core': '核心 {i}：{p}%（iowait {w}%）',
  'server-load.ui.iowait': 'iowait {p}%',
  'server-load.ui.mem': '内存',
  'server-load.ui.mem_v': '{u} / {t}',
  'server-load.ui.mem_cache': '缓存 {c}',
  'server-load.ui.swap': '交换区 {u} / {t}',
  'server-load.ui.disk': '磁盘 IO',
  'server-load.ui.disk_row': '读 {r}/s · 写 {w}/s · 繁忙 {u}%',
  'server-load.ui.no_disk': '没有可统计的磁盘',
  'server-load.ui.disk_idle': '另有 {n} 块磁盘空闲',
  'server-load.ui.proc': '本站进程',
  'server-load.ui.proc_hint': 'pt-server 自身的占用；CPU 按单核计（同 top），多线程时可超过 100%',
  'server-load.ui.proc_cpu': 'CPU',
  'server-load.ui.proc_cpu_v': '占整机 {s}%',
  'server-load.ui.proc_mem': '内存',
  'server-load.ui.proc_mem_v': '占内存 {p}%',
  'server-load.ui.proc_threads': '线程',
  'server-load.ui.loadavg': '负载 {a}',
  'server-load.ui.uptime': '已运行 {d}',
  'server-load.ui.days': '{n} 天',
  'server-load.ui.hours': '{n} 小时',
  'server-load.ui.set_system': '显示主机负载（CPU 各核心、内存、磁盘 IO）',
  'server-load.ui.set_system_hint': '数据来自服务器的 /proc；在 Docker 中显示的是宿主机的值',
});
registerMessages('en', {
  'server-load.ui.title': 'Server load',
  'server-load.ui.users': 'Registered users',
  'server-load.ui.of_limit': 'limit {n}',
  'server-load.ui.no_limit': 'no limit',
  'server-load.ui.load': '{p}% of capacity',
  'server-load.ui.new_today': 'New today',
  'server-load.ui.active': 'Active users',
  'server-load.ui.active_hint': 'Seeding or downloading in the last hour',
  'server-load.ui.peers': 'Peers',
  'server-load.ui.peers_v': '↑{s} ↓{l}',
  'server-load.ui.set_limit': 'User limit',
  'server-load.ui.set_limit_hint': '0 = use the site-wide user limit from the registration policy plugin',
  'server-load.ui.set_who': 'Who can see it',
  'server-load.ui.set_who_hint': 'Leave both empty to show it to everyone; otherwise users matching any selected class or role can see it. Admins always can.',
  'server-load.ui.set_classes': 'User classes',
  'server-load.ui.set_roles': 'Roles',
  'server-load.ui.no_levels': 'The levels plugin is not installed',
  'server-load.ui.save': 'Save',
  'server-load.ui.saved': 'Saved',
  'server-load.ui.cpu': 'CPU',
  'server-load.ui.cores': '{n} cores',
  'server-load.ui.core': 'Core {i}: {p}% (iowait {w}%)',
  'server-load.ui.iowait': 'iowait {p}%',
  'server-load.ui.mem': 'Memory',
  'server-load.ui.mem_v': '{u} / {t}',
  'server-load.ui.mem_cache': 'cache {c}',
  'server-load.ui.swap': 'swap {u} / {t}',
  'server-load.ui.disk': 'Disk IO',
  'server-load.ui.disk_row': 'read {r}/s · write {w}/s · busy {u}%',
  'server-load.ui.no_disk': 'No disks to report',
  'server-load.ui.disk_idle': '{n} more idle',
  'server-load.ui.proc': 'This site',
  'server-load.ui.proc_hint': 'Usage of the pt-server process itself; CPU is per core (like top) and can exceed 100% with many threads',
  'server-load.ui.proc_cpu': 'CPU',
  'server-load.ui.proc_cpu_v': '{s}% of host',
  'server-load.ui.proc_mem': 'Memory',
  'server-load.ui.proc_mem_v': '{p}% of RAM',
  'server-load.ui.proc_threads': 'Threads',
  'server-load.ui.loadavg': 'load {a}',
  'server-load.ui.uptime': 'up {d}',
  'server-load.ui.days': '{n} d',
  'server-load.ui.hours': '{n} h',
  'server-load.ui.set_system': 'Show host load (per-core CPU, memory, disk IO)',
  'server-load.ui.set_system_hint': 'Read from /proc on the server; inside Docker this is the host',
});

interface Status {
  visible: boolean;
  users?: number;
  user_limit?: number | null;
  new_today?: number;
  active_users?: number;
  seeders?: number;
  leechers?: number;
  system?: System | null;
}

interface System {
  cpu: { usage: number; iowait: number } | null;
  cores: { usage: number; iowait: number }[];
  memory: { total: number; used: number; cached: number; swap_total: number; swap_used: number };
  disks: { name: string; read_bps: number; write_bps: number; util: number }[];
  process: { cpu: number; cpu_share: number; rss: number | null; threads: number | null } | null;
  load: number[];
  uptime: number | null;
}

const n = (v: number | undefined) => (v ?? 0).toLocaleString();

function bytes(v: number): string {
  const u = ['B', 'KiB', 'MiB', 'GiB', 'TiB'];
  let i = 0;
  while (v >= 1024 && i < u.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 || i === 0 ? Math.round(v) : v.toFixed(1)} ${u[i]}`;
}

/** 负载色：<60 正常、<85 偏高、其余告警 */
const tone = (p: number) => (p >= 85 ? 'bad' : p >= 60 ? 'warn' : 'ok');

function uptime(api: PluginApi, s: number): string {
  const d = Math.floor(s / 86400);
  return d >= 1 ? api.t('server-load.ui.days', { n: d }) : api.t('server-load.ui.hours', { n: Math.max(1, Math.floor(s / 3600)) });
}

function Host({ api, sys }: { api: PluginApi; sys: System }) {
  const m = sys.memory;
  const memPct = m.total ? Math.round((m.used / m.total) * 100) : 0;
  const memTone = tone(memPct);
  // 只列有读写的盘（最多 4 块）；都空闲时列第一块，其余合成一句「另有 N 块空闲」
  const busy = sys.disks.filter((d) => d.read_bps + d.write_bps > 0 || d.util > 0);
  const disks = (busy.length ? busy : sys.disks).slice(0, busy.length ? 4 : 1);
  const idle = sys.disks.length - disks.length;
  return (
    <div className="sl-host">
      {sys.cpu && (
        <section>
          <div className="sl-h">
            <b>{api.t('server-load.ui.cpu')}</b>
            <span className="num">{Math.round(sys.cpu.usage)}%</span>
            <span className="tiny muted">
              {api.t('server-load.ui.cores', { n: sys.cores.length })} · {api.t('server-load.ui.iowait', { p: sys.cpu.iowait })}
            </span>
          </div>
          <div className="sl-cores" role="list">
            {sys.cores.map((c, i) => (
              <span
                key={i}
                role="listitem"
                className={`sl-core is-${tone(c.usage)}`}
                title={api.t('server-load.ui.core', { i, p: c.usage, w: c.iowait })}
                aria-label={api.t('server-load.ui.core', { i, p: c.usage, w: c.iowait })}
              >
                <i style={{ height: `${Math.max(2, Math.min(100, c.usage))}%` }} />
              </span>
            ))}
          </div>
        </section>
      )}
      {m.total > 0 && (
        <section>
          <div className="sl-h">
            <b>{api.t('server-load.ui.mem')}</b>
            <span className="num">{memPct}%</span>
            <span className="tiny muted">{api.t('server-load.ui.mem_v', { u: bytes(m.used), t: bytes(m.total) })}</span>
          </div>
          <Progress value={memPct} max={100} label={api.t('server-load.ui.mem')} tone={memTone === 'warn' ? undefined : memTone} />
          <p className="tiny muted sl-cap">
            {api.t('server-load.ui.mem_cache', { c: bytes(m.cached) })}
            {m.swap_total > 0 && <> · {api.t('server-load.ui.swap', { u: bytes(m.swap_used), t: bytes(m.swap_total) })}</>}
          </p>
        </section>
      )}
      <section>
        <div className="sl-h">
          <b>{api.t('server-load.ui.disk')}</b>
        </div>
        {sys.disks.length === 0 ? (
          <p className="tiny muted sl-cap">{api.t('server-load.ui.no_disk')}</p>
        ) : (
          <ul className="sl-disks">
            {disks.map((d) => (
              <li key={d.name}>
                <span className="sl-dn">{d.name}</span>
                <span className="sl-du">
                  <i className={`is-${tone(d.util)}`} style={{ width: `${Math.min(100, d.util)}%` }} />
                </span>
                <span className="tiny muted sl-dr">{api.t('server-load.ui.disk_row', { r: bytes(d.read_bps), w: bytes(d.write_bps), u: d.util })}</span>
              </li>
            ))}
          </ul>
        )}
        {idle > 0 && <p className="tiny muted sl-cap">{api.t('server-load.ui.disk_idle', { n: idle })}</p>}
      </section>
      {sys.process && (
        <section title={api.t('server-load.ui.proc_hint')}>
          <div className="sl-h">
            <b>{api.t('server-load.ui.proc')}</b>
          </div>
          <dl className="sl-facts sl-proc">
            <div>
              <dt>{api.t('server-load.ui.proc_cpu')}</dt>
              <dd className="num">
                {sys.process.cpu}%<small>{api.t('server-load.ui.proc_cpu_v', { s: sys.process.cpu_share })}</small>
              </dd>
            </div>
            {sys.process.rss != null && (
              <div>
                <dt>{api.t('server-load.ui.proc_mem')}</dt>
                <dd className="num">
                  {bytes(sys.process.rss)}
                  {m.total > 0 && <small>{api.t('server-load.ui.proc_mem_v', { p: Math.round((sys.process.rss / m.total) * 1000) / 10 })}</small>}
                </dd>
              </div>
            )}
            {sys.process.threads != null && (
              <div>
                <dt>{api.t('server-load.ui.proc_threads')}</dt>
                <dd className="num">{sys.process.threads}</dd>
              </div>
            )}
          </dl>
        </section>
      )}
      {(sys.load.length > 0 || sys.uptime) && (
        <p className="tiny muted sl-foot">
          {sys.load.length > 0 && api.t('server-load.ui.loadavg', { a: sys.load.map((x) => x.toFixed(2)).join(' ') })}
          {sys.load.length > 0 && sys.uptime ? ' · ' : ''}
          {sys.uptime ? api.t('server-load.ui.uptime', { d: uptime(api, sys.uptime) }) : ''}
        </p>
      )}
    </div>
  );
}

function Widget({ api }: SlotProps) {
  const q = useQuery({
    queryKey: ['server-load'],
    queryFn: () => api.json<Status>('/api/p/server-load/v1/status'),
    staleTime: 4_000,
    // 有主机负载时每 5 秒刷新（页面在后台时 react-query 不轮询）；只看人数时 2 分钟
    refetchInterval: (query) => (query.state.data?.system ? 5_000 : 120_000),
    retry: false,
  });
  const s = q.data;
  if (!s?.visible) return null;
  const limit = s.user_limit ?? null;
  const pct = limit ? Math.min(100, Math.round(((s.users ?? 0) / limit) * 100)) : null;
  return (
    <Panel title={api.t('server-load.ui.title')} className="sl-panel">
      <div className="sl-users">
        <span className="small muted">{api.t('server-load.ui.users')}</span>
        <b className="num">
          {n(s.users)}
          <small> / {limit ? n(limit) : '∞'}</small>
        </b>
      </div>
      {pct !== null && (
        <>
          <Progress value={pct} max={100} label={api.t('server-load.ui.load', { p: pct })} tone={pct >= 90 ? 'bad' : pct >= 70 ? undefined : 'ok'} />
          <p className="tiny muted sl-cap">
            {api.t('server-load.ui.load', { p: pct })} · {api.t('server-load.ui.of_limit', { n: n(limit ?? 0) })}
          </p>
        </>
      )}
      {pct === null && <p className="tiny muted sl-cap">{api.t('server-load.ui.no_limit')}</p>}
      <dl className="sl-facts">
        <div>
          <dt>{api.t('server-load.ui.new_today')}</dt>
          <dd className="num">{n(s.new_today)}</dd>
        </div>
        <div title={api.t('server-load.ui.active_hint')}>
          <dt>{api.t('server-load.ui.active')}</dt>
          <dd className="num">{n(s.active_users)}</dd>
        </div>
        <div>
          <dt>{api.t('server-load.ui.peers')}</dt>
          <dd className="num">{api.t('server-load.ui.peers_v', { s: n(s.seeders), l: n(s.leechers) })}</dd>
        </div>
      </dl>
      {s.system && <Host api={api} sys={s.system} />}
    </Panel>
  );
}

// ---------------- 后台设置 ----------------

interface SettingsDoc {
  settings: { user_limit?: number; show_system?: boolean; visible_classes?: number[]; visible_roles?: string[] };
  version: number;
}
interface LevelClass {
  id: number;
  key: string;
  name: string;
  name_key: string;
  rank: number;
}

const SETTINGS = '/api/kernel/v1/admin/plugins/server-load/settings';
const ROLES = ['user', 'uploader', 'moderator', 'admin', 'sysop'];

/** 等级显示名：自定义名 > 术语 > key（与宿主 levelName 一致） */
function className(api: PluginApi, c: LevelClass): string {
  if (c.name) return c.name;
  const t = api.t(`term:${c.name_key}`);
  return t && t !== c.name_key && t !== `term:${c.name_key}` ? t : c.key;
}

function Settings({ api }: { api: PluginApi }) {
  const [doc, setDoc] = useState<SettingsDoc | null>(null);
  const [classes, setClasses] = useState<LevelClass[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const load = async () => setDoc(await api.json<SettingsDoc>(SETTINGS));
  useEffect(() => {
    load().catch((e: unknown) => setError(e instanceof Error ? e.message : String(e)));
    api
      .json<{ items: LevelClass[] }>('/api/p/levels/v1/classes')
      .then((r) => setClasses([...r.items].sort((a, b) => a.rank - b.rank)))
      .catch(() => setClasses([]));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  if (error && !doc) return <Note tone="bad">{error}</Note>;
  if (!doc) return <SkeletonRows n={5} />;
  const cfg = { user_limit: doc.settings.user_limit ?? 0, show_system: doc.settings.show_system ?? true, visible_classes: doc.settings.visible_classes ?? [], visible_roles: doc.settings.visible_roles ?? [] };
  const set = (patch: Partial<typeof cfg>) => setDoc({ ...doc, settings: { ...cfg, ...patch } });
  const toggle = <T,>(list: T[], v: T, on: boolean) => (on ? [...list.filter((x) => x !== v), v] : list.filter((x) => x !== v));
  const save = async () => {
    setBusy(true);
    try {
      await api.json(SETTINGS, { method: 'PUT', json: { settings: cfg, version: doc.version } });
      await load();
      api.toast(api.t('server-load.ui.saved'), 'success');
    } catch (e) {
      api.toast(e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <Panel>
      <div className="form">
        <Field label={api.t('server-load.ui.set_limit')} hint={api.t('server-load.ui.set_limit_hint')}>
          <Input type="number" min={0} value={String(cfg.user_limit)} onChange={(e) => set({ user_limit: Math.max(0, Math.floor(Number(e.target.value) || 0)) })} />
        </Field>
        <div className="sl-sys">
          <Checkbox label={api.t('server-load.ui.set_system')} checked={cfg.show_system} onChange={(on) => set({ show_system: on })} />
          <p className="hint">{api.t('server-load.ui.set_system_hint')}</p>
        </div>
        <fieldset className="sl-who">
          <legend className="fl-label">{api.t('server-load.ui.set_who')}</legend>
          <p className="hint">{api.t('server-load.ui.set_who_hint')}</p>
          <div className="sl-who-b">
            <b className="small">{api.t('server-load.ui.set_classes')}</b>
            <div className="pt-checks">
              {classes === null ? (
                <SkeletonRows n={1} />
              ) : classes.length === 0 ? (
                <span className="small muted">{api.t('server-load.ui.no_levels')}</span>
              ) : (
                classes.map((c) => (
                  <Checkbox key={c.id} label={className(api, c)} checked={cfg.visible_classes.includes(c.id)} onChange={(on) => set({ visible_classes: toggle(cfg.visible_classes, c.id, on) })} />
                ))
              )}
            </div>
            <b className="small">{api.t('server-load.ui.set_roles')}</b>
            <div className="pt-checks">
              {ROLES.map((r) => (
                <Checkbox key={r} label={api.t(`term:identity.role.${r}`)} checked={cfg.visible_roles.includes(r)} onChange={(on) => set({ visible_roles: toggle(cfg.visible_roles, r, on) })} />
              ))}
            </div>
          </div>
        </fieldset>
        <div>
          <Button variant="primary" onClick={() => void save()} disabled={busy}>
            {api.t('server-load.ui.save')}
          </Button>
        </div>
      </div>
    </Panel>
  );
}

export default {
  slots: { 'home.widgets': { component: Widget, order: 90 } },
  settings: Settings,
} satisfies PluginFrontend;
