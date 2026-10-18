// levels 前端：个人页的等级（任何人的个人页都显示等级；本人另显示升级进度与保级警告）、
// 等级说明页（/p/levels/：全部等级、说明、升级与保级条件、我的进度）、用户菜单入口。
// 等级的名称 / 说明可在插件设置中修改；为空时用内置术语（随界面语言）。
import { useEffect, useState, type FormEvent } from 'react';
import { ApiError } from '@pt-studio/sdk';
import { registerMessages } from '@pt-studio/terms';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { Badge, Button, Card, Field, Input, Progress, Stat, Table } from '@pt-studio/ui-kit';

interface Cond {
  metric: string;
  op: string;
  value: number;
}

interface ClassInfo {
  id: number;
  key: string;
  type: 'normal' | 'vip' | 'honor' | 'staff';
  name: string;
  name_key: string;
  description: string;
  desc_key: string;
  color: string | null;
  promote: Cond[];
  keep: Cond[];
  can_invite: boolean;
  protected: boolean;
  is_default: boolean;
}

interface Brief {
  id: number;
  key?: string;
  name?: string;
  name_key?: string;
  color?: string | null;
}

interface Me {
  class: Brief & { source: string };
  next: (Brief & { progress: { metric: string; op: string; target: string; current: string; met: boolean }[] }) | null;
  warning: { deadline: number } | null;
  protected: boolean;
}

registerMessages('zh-CN', {
  'levels.ui.class': '等级',
  'levels.ui.menu': '等级说明',
  'levels.ui.title': '用户等级',
  'levels.ui.warning': '保级警告：{date} 前未达标将降级',
  'levels.ui.my': '我的等级',
  'levels.ui.next': '下一等级',
  'levels.ui.top': '已是最高的普通等级',
  'levels.ui.name': '等级',
  'levels.ui.desc': '说明',
  'levels.ui.promote': '升级条件',
  'levels.ui.keep': '保级条件',
  'levels.ui.none': '—',
  'levels.ui.never_demoted': '不降级',
  'levels.ui.manual': '由管理员授予',
  'levels.ui.default': '新用户',
  'levels.ui.no_invite': '不能发邀请',
  'levels.ui.current': '当前',
  'levels.ui.note': '普通等级按条件自动评估：流量结算后 15 分钟内升级，每日凌晨全量检查保级条件。不满足保级条件时先发出警告，警告期满仍未达标才降级；升级后有一段保护期。',
  'levels.metric.account_age_days': '注册天数',
  'levels.metric.uploaded_gib': '上传量（GiB）',
  'levels.metric.downloaded_gib': '下载量（GiB）',
  'levels.metric.ratio': '分享率',
  'levels.metric.seed_hours': '做种时长（小时）',
  'levels.metric.avg_seed_hours': '平均做种时长（小时）',
  'levels.metric.seeding_size_gib': '做种体积（GiB）',
  'levels.metric.uploads_count': '发布数',
  'levels.ui.tab': '等级',
  'levels.ui.source': '来源：{s}',
  'levels.ui.overrides': '生效中的覆盖',
  'levels.ui.until': '至 {t}',
  'levels.ui.forever': '永久',
  'levels.ui.set_class': '设置等级',
  'levels.ui.lock_days': '锁定天数（留空为永久）',
  'levels.ui.reason': '原因',
  'levels.ui.apply': '设置',
  'levels.ui.unlock': '解除手动锁定',
  'levels.ui.protect_days': '保号天数（0 取消）',
  'levels.ui.protect': '保号',
  'levels.ui.protected_until': '保号至 {t}',
  'levels.ui.done': '已保存',
});
registerMessages('en', {
  'levels.ui.class': 'Class',
  'levels.ui.menu': 'User classes',
  'levels.ui.title': 'User classes',
  'levels.ui.warning': 'Warning: demotion on {date} unless requirements are met',
  'levels.ui.my': 'My class',
  'levels.ui.next': 'Next class',
  'levels.ui.top': 'Highest regular class reached',
  'levels.ui.name': 'Class',
  'levels.ui.desc': 'Description',
  'levels.ui.promote': 'Promotion',
  'levels.ui.keep': 'To keep',
  'levels.ui.none': '—',
  'levels.ui.never_demoted': 'Never demoted',
  'levels.ui.manual': 'Granted by staff',
  'levels.ui.default': 'New users',
  'levels.ui.no_invite': 'Cannot invite',
  'levels.ui.current': 'Current',
  'levels.ui.note':
    'Regular classes are evaluated automatically: promotions within 15 minutes of traffic settling, keep requirements checked daily. Failing them starts a warning period before demotion; a promotion comes with a protection period.',
  'levels.metric.account_age_days': 'Account age (days)',
  'levels.metric.uploaded_gib': 'Uploaded (GiB)',
  'levels.metric.downloaded_gib': 'Downloaded (GiB)',
  'levels.metric.ratio': 'Ratio',
  'levels.metric.seed_hours': 'Seeding time (hours)',
  'levels.metric.avg_seed_hours': 'Average seeding time (hours)',
  'levels.metric.seeding_size_gib': 'Seeding size (GiB)',
  'levels.metric.uploads_count': 'Uploads',
  'levels.ui.tab': 'Class',
  'levels.ui.source': 'Source: {s}',
  'levels.ui.overrides': 'Active overrides',
  'levels.ui.until': 'until {t}',
  'levels.ui.forever': 'permanent',
  'levels.ui.set_class': 'Set class',
  'levels.ui.lock_days': 'Lock for days (blank = permanent)',
  'levels.ui.reason': 'Reason',
  'levels.ui.apply': 'Set',
  'levels.ui.unlock': 'Remove manual lock',
  'levels.ui.protect_days': 'Protect for days (0 removes)',
  'levels.ui.protect': 'Protect',
  'levels.ui.protected_until': 'Protected until {t}',
  'levels.ui.done': 'Saved',
});

const OPS: Record<string, string> = { gte: '≥', gt: '>', lte: '≤', lt: '<' };

/** 等级列表在页面内只取一次（多个个人页插槽共享） */
let classesPromise: Promise<ClassInfo[]> | null = null;
function loadClasses(api: PluginApi): Promise<ClassInfo[]> {
  classesPromise ??= api
    .json<{ items: ClassInfo[] }>('/api/p/levels/v1/classes')
    .then((r) => r.items)
    .catch((e: unknown) => {
      classesPromise = null;
      throw e;
    });
  return classesPromise;
}

function useClasses(api: PluginApi): ClassInfo[] | null {
  const [list, setList] = useState<ClassInfo[] | null>(null);
  useEffect(() => {
    if (api.user()) void loadClasses(api).then(setList).catch(() => undefined);
  }, [api]);
  return list;
}

function className(api: PluginApi, c: Brief): string {
  if (c.name) return c.name;
  return c.name_key ? api.t(`term:${c.name_key}`) : `#${c.id}`;
}

function ClassName({ api, c }: { api: PluginApi; c: Brief }) {
  return (
    <span className="pt-class-name" style={c.color ? { color: c.color, fontWeight: 600 } : { fontWeight: 600 }}>
      {className(api, c)}
    </span>
  );
}

function condText(api: PluginApi, cs: Cond[]): string {
  return cs.map((c) => `${api.t(`levels.metric.${c.metric}`)} ${OPS[c.op] ?? c.op} ${c.value}`).join('；');
}

function NextHint({ api, me }: { api: PluginApi; me: Me }) {
  if (me.warning)
    return <span className="pt-field-error">{api.t('levels.ui.warning', { date: new Date(me.warning.deadline).toLocaleDateString() })}</span>;
  if (!me.next) return null;
  const met = me.next.progress.filter((p) => p.met).length;
  return (
    <span title={me.next.progress.map((p) => `${api.t(`levels.metric.${p.metric}`)}: ${p.current} / ${p.target}`).join('\n')}>
      → {className(api, me.next)} <Progress value={met} max={me.next.progress.length} />
    </span>
  );
}

/** 个人页：任何人的等级；本人另显示进度 */
function LevelStat({ ctx, api }: SlotProps<{ profile: { id: number; class_id: number } }>) {
  const classes = useClasses(api);
  const [me, setMe] = useState<Me | null>(null);
  const self = api.user()?.id === ctx.profile.id;
  useEffect(() => {
    if (self) void api.json<Me>('/api/p/levels/v1/me').then(setMe).catch(() => undefined);
  }, [api, self]);
  const c: Brief | undefined = me?.class ?? classes?.find((x) => x.id === ctx.profile.class_id);
  if (!c) return null;
  return (
    <Stat
      label={
        <a
          href="/p/levels/"
          onClick={(e) => {
            e.preventDefault();
            api.navigate('/p/levels/');
          }}
        >
          {api.t('levels.ui.class')}
        </a>
      }
      value={<ClassName api={api} c={c} />}
      hint={me ? <NextHint api={api} me={me} /> : null}
    />
  );
}

function MenuItem({ api }: SlotProps) {
  return (
    <a
      href="/p/levels/"
      onClick={(e) => {
        e.preventDefault();
        api.navigate('/p/levels/');
      }}
    >
      {api.t('levels.ui.menu')}
    </a>
  );
}

function Page({ api }: { api: PluginApi }) {
  const classes = useClasses(api);
  const [me, setMe] = useState<Me | null>(null);
  useEffect(() => {
    void api.json<Me>('/api/p/levels/v1/me').then(setMe).catch(() => undefined);
  }, [api]);
  if (!classes) return null;
  const desc = (c: ClassInfo) => {
    if (c.description) return c.description;
    const d = api.t(`term:${c.desc_key}`);
    return d === c.desc_key ? '' : d;
  };
  return (
    <div className="pt-stack">
      {me && (
        <Card title={api.t('levels.ui.my')}>
          <div className="pt-row">
            <Stat label={api.t('levels.ui.my')} value={<ClassName api={api} c={me.class} />} />
            <Stat label={api.t('levels.ui.next')} value={me.next ? <ClassName api={api} c={me.next} /> : api.t('levels.ui.top')} hint={<NextHint api={api} me={me} />} />
          </div>
          {me.next && (
            <Table
              rows={me.next.progress}
              rowKey={(p) => p.metric}
              columns={[
                { key: 'm', header: api.t('levels.ui.promote'), render: (p) => api.t(`levels.metric.${p.metric}`) },
                { key: 't', header: '', render: (p) => `${OPS[p.op] ?? p.op} ${p.target}` },
                { key: 'c', header: api.t('levels.ui.current'), render: (p) => <Badge tone={p.met ? 'success' : 'muted'}>{p.current}</Badge> },
              ]}
            />
          )}
        </Card>
      )}
      <Card title={api.t('levels.ui.title')}>
        <p className="pt-muted small">{api.t('levels.ui.note')}</p>
        <Table
          rows={classes}
          rowKey={(c) => c.key}
          columns={[
            {
              key: 'n',
              header: api.t('levels.ui.name'),
              render: (c) => (
                <>
                  <ClassName api={api} c={c} /> {me?.class.id === c.id && <Badge tone="primary">{api.t('levels.ui.current')}</Badge>}
                  {c.is_default && <Badge>{api.t('levels.ui.default')}</Badge>}
                </>
              ),
            },
            { key: 'd', header: api.t('levels.ui.desc'), render: (c) => desc(c) },
            {
              key: 'p',
              header: api.t('levels.ui.promote'),
              render: (c) => (c.type === 'normal' ? condText(api, c.promote) || api.t('levels.ui.none') : api.t('levels.ui.manual')),
            },
            {
              key: 'k',
              header: api.t('levels.ui.keep'),
              render: (c) =>
                c.type !== 'normal' || c.protected || c.keep.length === 0 ? api.t('levels.ui.never_demoted') : condText(api, c.keep),
            },
          ]}
        />
      </Card>
    </div>
  );
}

interface AdminUser extends Me {
  overrides: { id: number; kind: string; class_id: number; until: number | null; reason: string; created_by: string }[];
  protected_until: number | null;
}

const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e);

/** 后台用户详情页“等级”标签：当前等级与来源、生效中的覆盖、手动设置 / 锁定、解除、保号 */
function UserTab({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const uid = ctx.profile.id;
  const classes = useClasses(api);
  const [d, setD] = useState<AdminUser | null>(null);
  const [f, setF] = useState({ class: '', days: '', reason: '', protect: '30' });
  const base = `/api/p/levels/v1/admin/users/${uid}` as const;
  const load = () => void api.json<AdminUser>(base).then(setD);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, [uid]);
  const act = async (path: `/api/${string}`, init: { method?: string; json?: unknown }) => {
    try {
      await api.json(path, init);
      api.toast(api.t('levels.ui.done'), 'success');
      load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const setClass = (e: FormEvent) => {
    e.preventDefault();
    const lock = f.days.trim() ? Date.now() + Number(f.days) * 86_400_000 : null;
    void act(`${base}/class`, { json: { class: f.class, lock_until_ms: lock, reason: f.reason } });
  };
  if (!d || !classes) return null;
  const byId = (id: number) => classes.find((c) => c.id === id);
  return (
    <Card>
      <p>
        <ClassName api={api} c={d.class} /> <span className="pt-muted small">{api.t('levels.ui.source', { s: d.class.source })}</span>{' '}
        {d.protected_until && <Badge tone="success">{api.t('levels.ui.protected_until', { t: new Date(d.protected_until).toLocaleDateString() })}</Badge>}{' '}
        {d.warning && <Badge tone="danger">{api.t('levels.ui.warning', { date: new Date(d.warning.deadline).toLocaleDateString() })}</Badge>}
      </p>
      {d.overrides.length > 0 && (
        <Table
          rows={d.overrides}
          rowKey={(o) => o.id}
          columns={[
            { key: 'k', header: api.t('levels.ui.overrides'), render: (o) => <Badge>{o.kind}</Badge> },
            { key: 'c', header: '', render: (o) => (byId(o.class_id) ? <ClassName api={api} c={byId(o.class_id) as Brief} /> : `#${o.class_id}`) },
            { key: 'u', header: '', render: (o) => (o.until ? api.t('levels.ui.until', { t: new Date(o.until).toLocaleString() }) : api.t('levels.ui.forever')) },
            { key: 'r', header: '', render: (o) => <span className="pt-muted small">{o.reason} · {o.created_by}</span> },
          ]}
        />
      )}
      <form onSubmit={setClass} className="upload-grid">
        <Field label={api.t('levels.ui.set_class')}>
          <select className="pt-input" name="level_class" value={f.class} onChange={(e) => setF({ ...f, class: e.target.value })}>
            <option value="" />
            {classes.map((c) => (
              <option key={c.id} value={c.key}>
                {className(api, c)}
              </option>
            ))}
          </select>
        </Field>
        <Field label={api.t('levels.ui.lock_days')}>
          <Input name="level_days" type="number" min={1} value={f.days} onChange={(e) => setF({ ...f, days: e.target.value })} />
        </Field>
        <div className="span2">
          <Field label={api.t('levels.ui.reason')}>
            <Input name="level_reason" maxLength={200} value={f.reason} onChange={(e) => setF({ ...f, reason: e.target.value })} />
          </Field>
          <Button type="submit" variant="primary" disabled={!f.class}>
            {api.t('levels.ui.apply')}
          </Button>{' '}
          <Button type="button" onClick={() => void act(`${base}/class`, { method: 'DELETE' })}>
            {api.t('levels.ui.unlock')}
          </Button>
        </div>
      </form>
      <div className="searchbar">
        <Input name="level_protect" type="number" min={0} aria-label={api.t('levels.ui.protect_days')} title={api.t('levels.ui.protect_days')} value={f.protect} onChange={(e) => setF({ ...f, protect: e.target.value })} />
        <Button
          type="button"
          onClick={() => void act(`${base}/protect`, { json: { until_ms: Number(f.protect) > 0 ? Date.now() + Number(f.protect) * 86_400_000 : null } })}
        >
          {api.t('levels.ui.protect')}
        </Button>
      </div>
    </Card>
  );
}

export default {
  slots: {
    'admin.user.tabs': { component: UserTab, order: 5, title_term: 'levels.ui.tab' },
    'user.profile.stats': { component: LevelStat, order: 10 },
    'nav.user_menu': { component: MenuItem, order: 30 },
  },
  routes: [{ path: '/p/levels/*', component: Page }],
} satisfies PluginFrontend;
