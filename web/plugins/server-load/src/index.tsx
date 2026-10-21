// server-load 前端：首页侧栏「服务器负载」（用户数 / 上限、今日新注册、近一小时活跃）；后台设置：用户上限、可查看的等级与角色。
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
});

interface Status {
  visible: boolean;
  users?: number;
  user_limit?: number | null;
  new_today?: number;
  active_users?: number;
  seeders?: number;
  leechers?: number;
}

const n = (v: number | undefined) => (v ?? 0).toLocaleString();

function Widget({ api }: SlotProps) {
  const q = useQuery({
    queryKey: ['server-load'],
    queryFn: () => api.json<Status>('/api/p/server-load/v1/status'),
    staleTime: 60_000,
    refetchInterval: 120_000,
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
    </Panel>
  );
}

// ---------------- 后台设置 ----------------

interface SettingsDoc {
  settings: { user_limit?: number; visible_classes?: number[]; visible_roles?: string[] };
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
  const cfg = { user_limit: doc.settings.user_limit ?? 0, visible_classes: doc.settings.visible_classes ?? [], visible_roles: doc.settings.visible_roles ?? [] };
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
