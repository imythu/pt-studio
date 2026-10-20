// notify 前端：导航里的未读通知数、通知中心（/p/notify/，按模板术语渲染，缺少术语时用回退文本）、设置页“通知”标签
// （类别 × 渠道开关）、邮件退订确认页（/p/notify/unsubscribe?t=，无需登录）。
import { useCallback, useEffect, useState } from 'react';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Empty, LoadMore, NavItem, Note, PageHead, Panel, Segmented, SkeletonRows, Switch, Table } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'notify.ui.menu': '通知',
  'notify.ui.all': '全部',
  'notify.ui.unread': '未读',
  'notify.ui.empty': '没有通知',
  'notify.ui.empty_unread': '没有未读通知',
  'notify.ui.lede': '站内发生的与你有关的事：回复、应求、考核、安全提醒。',
  'notify.ui.prefs_hint': '选择每类通知通过哪些渠道送达。安全通知不能关闭。',
  'notify.ui.title': '通知',
  'notify.ui.read_all': '全部标为已读',
  'notify.ui.unread_only': '只看未读',
  'notify.ui.more': '加载更多',
  'notify.ui.tab': '通知',
  'notify.ui.locked': '安全通知不能关闭',
  'notify.ui.saved': '已保存',
  'notify.ui.category': '类别',
  'notify.ui.center': '站内',
  'notify.ui.email': '邮件',
  'notify.ui.email_unavailable': '本站未启用邮件通知',
  'notify.ui.unsub_title': '退订邮件通知',
  'notify.ui.unsub_confirm': '不再通过邮件接收“{category}”类通知？站内通知不受影响。',
  'notify.ui.unsub_button': '确认退订',
  'notify.ui.unsub_done': '已退订。可以随时在个人设置的“通知”中重新开启。',
  'notify.ui.unsub_invalid': '退订链接无效或已过期。',
});
registerMessages('en', {
  'notify.ui.menu': 'Notifications',
  'notify.ui.all': 'All',
  'notify.ui.unread': 'Unread',
  'notify.ui.empty': 'No notifications',
  'notify.ui.empty_unread': 'No unread notifications',
  'notify.ui.lede': 'Things on the site that involve you: replies, request fills, reviews and security alerts.',
  'notify.ui.prefs_hint': 'Choose how each kind of notification reaches you. Security notifications cannot be turned off.',
  'notify.ui.title': 'Notifications',
  'notify.ui.read_all': 'Mark all as read',
  'notify.ui.unread_only': 'Unread only',
  'notify.ui.more': 'Load more',
  'notify.ui.tab': 'Notifications',
  'notify.ui.locked': 'Security notifications cannot be turned off',
  'notify.ui.saved': 'Saved',
  'notify.ui.category': 'Category',
  'notify.ui.center': 'On site',
  'notify.ui.email': 'Email',
  'notify.ui.email_unavailable': 'Email notifications are not enabled on this site',
  'notify.ui.unsub_title': 'Unsubscribe from emails',
  'notify.ui.unsub_confirm': 'Stop receiving “{category}” notifications by email? On-site notifications are not affected.',
  'notify.ui.unsub_button': 'Unsubscribe',
  'notify.ui.unsub_done': 'Unsubscribed. You can turn it back on any time under Settings → Notifications.',
  'notify.ui.unsub_invalid': 'This unsubscribe link is invalid or has expired.',
});

interface Notification {
  id: number;
  template_key: string;
  args: Record<string, unknown>;
  title: string;
  body: string;
  link: string | null;
  read: boolean;
  created_at: number;
}

/** 模板术语存在时按术语渲染（多语言），否则用回退文本 */
function render(api: PluginApi, n: Notification, part: 'title' | 'body'): string {
  const key = `${n.template_key}.${part}`;
  const v = api.t(`term:${key}`, n.args);
  return v === key ? n[part] : v;
}

/** 类别标签：模板键第一段的术语（notify.category.<段>），没有术语时不显示 */
function kind(api: PluginApi, n: Notification): string {
  const seg = n.template_key.split('.')[0] ?? '';
  const k = `notify.category.${seg}`;
  const v = api.t(`term:${k}`);
  return v === k ? '' : v;
}

function rel(ms: number): string {
  const s = Math.round((Date.now() - ms) / 1000);
  const rtf = new Intl.RelativeTimeFormat(document.documentElement.lang || 'zh-CN', { numeric: 'auto' });
  if (s < 60) return rtf.format(-s, 'second');
  if (s < 3600) return rtf.format(-Math.floor(s / 60), 'minute');
  if (s < 86400) return rtf.format(-Math.floor(s / 3600), 'hour');
  if (s < 7 * 86400) return rtf.format(-Math.floor(s / 86400), 'day');
  return new Date(ms).toLocaleDateString();
}

function MenuItem({ api }: SlotProps) {
  const [n, setN] = useState(0);
  useEffect(() => {
    // 与宿主外壳共用同一查询缓存（侧栏、底栏同时挂载时只发一次请求）
    void api.queryClient
      .fetchQuery({ queryKey: ['unread', 'notify'], queryFn: () => api.json<{ count: number }>('/api/p/notify/v1/unread'), staleTime: 30_000 })
      .then((r) => setN(r.count))
      .catch(() => undefined);
  }, [api]);
  return (
    <NavItem to="/p/notify/" navigate={api.navigate} count={n} alert>
      {api.t('notify.ui.menu')}
    </NavItem>
  );
}

function Inbox({ api }: { api: PluginApi }) {
  const [items, setItems] = useState<Notification[] | null>(null);
  const [cursor, setCursor] = useState<string | null>(null);
  const [filter, setFilter] = useState<'all' | 'unread'>('all');
  const [busy, setBusy] = useState(false);
  const load = useCallback(
    async (more: boolean, cur: string | null) => {
      setBusy(true);
      try {
        const q = new URLSearchParams();
        if (filter === 'unread') q.set('unread_only', '1');
        if (more && cur) q.set('cursor', cur);
        const r = await api.json<{ items: Notification[]; next_cursor: string | null }>(`/api/p/notify/v1/inbox?${q.toString()}`);
        setItems((xs) => (more ? [...(xs ?? []), ...r.items] : r.items));
        setCursor(r.next_cursor);
      } finally {
        setBusy(false);
      }
    },
    [api, filter],
  );
  useEffect(() => {
    void load(false, null);
  }, [load]);
  async function open(n: Notification) {
    await api.fetch('/api/p/notify/v1/inbox/read', { method: 'POST', json: { ids: [n.id] } });
    void api.queryClient.invalidateQueries({ queryKey: ['unread', 'notify'] });
    if (n.link) api.navigate(n.link);
    else setItems((xs) => xs?.map((x) => (x.id === n.id ? { ...x, read: true } : x)) ?? null);
  }
  async function readAll() {
    await api.fetch('/api/p/notify/v1/inbox/read', { method: 'POST', json: { all: true } });
    void api.queryClient.invalidateQueries({ queryKey: ['unread', 'notify'] });
    setItems((xs) => xs?.map((x) => ({ ...x, read: true })) ?? null);
  }
  return (
    <div className="stack">
      <PageHead
        title={api.t('notify.ui.title')}
        lede={api.t('notify.ui.lede')}
        actions={
          <>
            <Segmented
              value={filter}
              onChange={(v) => setFilter(v)}
              options={[
                { value: 'all', label: api.t('notify.ui.all') },
                { value: 'unread', label: api.t('notify.ui.unread') },
              ]}
            />
            <Button size="sm" onClick={() => void readAll()}>
              {api.t('notify.ui.read_all')}
            </Button>
          </>
        }
      />
      <Panel flush primary>
        {items === null ? (
          <SkeletonRows n={6} />
        ) : items.length === 0 ? (
          <Empty>{filter === 'unread' ? api.t('notify.ui.empty_unread') : api.t('notify.ui.empty')}</Empty>
        ) : (
          <div>
            {items.map((n) => {
              const body = render(api, n, 'body');
              const k = kind(api, n);
              return (
                <a
                  key={n.id}
                  href={n.link ?? '#'}
                  data-testid="notification"
                  className={n.read ? 'ntf' : 'ntf unread'}
                  onClick={(e) => {
                    e.preventDefault();
                    void open(n);
                  }}
                >
                  <span className="k">{k}</span>
                  <div>
                    <b>{render(api, n, 'title')}</b>
                    {body && <p>{body}</p>}
                  </div>
                  <time title={new Date(n.created_at).toLocaleString()}>{rel(n.created_at)}</time>
                </a>
              );
            })}
          </div>
        )}
        {cursor && <LoadMore onClick={() => void load(true, cursor)} loading={busy} label={api.t('notify.ui.more')} />}
      </Panel>
    </div>
  );
}

interface Pref {
  center: boolean;
  email: boolean;
  email_available: boolean;
  locked: boolean;
}

function Prefs({ api }: SlotProps) {
  const [prefs, setPrefs] = useState<Record<string, Pref> | null>(null);
  useEffect(() => {
    void api.json<Record<string, Pref>>('/api/p/notify/v1/preferences').then(setPrefs);
  }, [api]);
  async function toggle(cat: string, channel: 'center' | 'email', on: boolean) {
    if (!prefs) return;
    setPrefs({ ...prefs, [cat]: { ...prefs[cat], [channel]: on } as Pref });
    await api.fetch('/api/p/notify/v1/preferences', { method: 'PUT', json: { [cat]: { [channel]: on } } });
    api.toast(api.t('notify.ui.saved'), 'success');
  }
  if (!prefs) return <SkeletonRows n={4} />;
  const emailOk = Object.values(prefs).some((p) => p.email_available);
  const rows = Object.entries(prefs);
  return (
    <div className="stack-s">
      <p className="small muted">{api.t('notify.ui.prefs_hint')}</p>
      <Table
        rows={rows}
        rowKey={([cat]) => cat}
        columns={[
          { key: 'c', header: api.t('notify.ui.category'), render: ([cat, p]) => <span title={p.locked ? api.t('notify.ui.locked') : undefined}>{api.t(`term:notify.category.${cat}`)}</span> },
          { key: 's', header: api.t('notify.ui.center'), render: ([cat, p]) => <Switch label={api.t('notify.ui.center')} checked={p.center} disabled={p.locked} onChange={(v) => void toggle(cat, 'center', v)} /> },
          ...(emailOk ? [{ key: 'e', header: api.t('notify.ui.email'), render: ([cat, p]: [string, Pref]) => <Switch label={api.t('notify.ui.email')} checked={p.email} disabled={p.locked || !p.email_available} onChange={(v) => void toggle(cat, 'email', v)} /> }] : []),
        ]}
      />
      {!emailOk && <Note>{api.t('notify.ui.email_unavailable')}</Note>}
    </div>
  );
}

/** 退订确认页：GET 只校验令牌，点击确认后 POST */
function Unsubscribe({ api }: { api: PluginApi }) {
  const t = new URLSearchParams(window.location.search).get('t') ?? '';
  const url = `/api/p/notify/v1/unsubscribe?t=${encodeURIComponent(t)}` as const;
  const [state, setState] = useState<'loading' | 'ready' | 'done' | 'invalid'>('loading');
  const [category, setCategory] = useState('');
  useEffect(() => {
    api
      .json<{ category: string }>(url)
      .then((r) => {
        setCategory(r.category);
        setState('ready');
      })
      .catch(() => setState('invalid'));
  }, [api, url]);
  async function confirm() {
    try {
      await api.json(url, { method: 'POST', json: {} });
      setState('done');
    } catch {
      setState('invalid');
    }
  }
  return (
    <div className="unsub">
      <Panel title={api.t('notify.ui.unsub_title')} primary>
        <div className="stack-s">
          {state === 'loading' && <SkeletonRows n={1} />}
          {state === 'ready' && (
            <>
              <p>{api.t('notify.ui.unsub_confirm', { category: api.t(`term:notify.category.${category}`) })}</p>
              <div>
                <Button variant="primary" onClick={() => void confirm()}>
                  {api.t('notify.ui.unsub_button')}
                </Button>
              </div>
            </>
          )}
          {state === 'done' && <Note tone="ok">{api.t('notify.ui.unsub_done')}</Note>}
          {state === 'invalid' && <Note tone="bad">{api.t('notify.ui.unsub_invalid')}</Note>}
        </div>
      </Panel>
    </div>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  return path.startsWith('/p/notify/unsubscribe') ? <Unsubscribe api={api} /> : <Inbox api={api} />;
}

export default {
  slots: {
    'nav.user_menu': { component: MenuItem, order: 5 },
    'user.settings.tabs': { component: Prefs, order: 20, title_term: 'notify.ui.tab' },
  },
  routes: [{ path: '/p/notify/*', component: Pages }],
} satisfies PluginFrontend;
