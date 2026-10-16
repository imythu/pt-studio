// levels 前端：个人页的等级（任何人的个人页都显示等级；本人另显示升级进度与保级警告）、
// 等级说明页（/p/levels/：全部等级、说明、升级与保级条件、我的进度）、用户菜单入口。
// 等级的名称 / 说明可在插件设置中修改；为空时用内置术语（随界面语言）。
import { useEffect, useState } from 'react';
import { registerMessages } from '@pt-studio/terms';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { Badge, Card, Progress, Stat, Table } from '@pt-studio/ui-kit';

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

export default {
  slots: {
    'user.profile.stats': { component: LevelStat, order: 10 },
    'nav.user_menu': { component: MenuItem, order: 30 },
  },
  routes: [{ path: '/p/levels/*', component: Page }],
} satisfies PluginFrontend;
