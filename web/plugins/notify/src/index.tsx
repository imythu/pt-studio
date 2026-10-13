// notify 前端：用户菜单的未读通知数、通知中心（/p/notify/，按模板术语渲染，缺少术语时用回退文本）、设置页“通知”标签
// （类别 × 渠道开关）、邮件退订确认页（/p/notify/unsubscribe?t=，无需登录）。
import { useCallback, useEffect, useState } from 'react';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Card, Empty } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'notify.ui.menu': '通知（{n}）',
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
  'notify.ui.menu': 'Notifications ({n})',
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

function MenuItem({ api }: SlotProps) {
  const [n, setN] = useState(0);
  useEffect(() => {
    void api
      .json<{ count: number }>('/api/p/notify/v1/unread')
      .then((r) => setN(r.count))
      .catch(() => undefined);
  }, [api]);
  return (
    <a
      href="/p/notify/"
      onClick={(e) => {
        e.preventDefault();
        api.navigate('/p/notify/');
      }}
    >
      {api.t('notify.ui.menu', { n })}
    </a>
  );
}

function Inbox({ api }: { api: PluginApi }) {
  const [items, setItems] = useState<Notification[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const [unreadOnly, setUnreadOnly] = useState(false);
  const load = useCallback(
    async (more = false) => {
      const q = new URLSearchParams();
      if (unreadOnly) q.set('unread_only', '1');
      if (more && cursor) q.set('cursor', cursor);
      const r = await api.json<{ items: Notification[]; next_cursor: string | null }>(`/api/p/notify/v1/inbox?${q.toString()}`);
      setItems(more ? [...items, ...r.items] : r.items);
      setCursor(r.next_cursor);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [api, unreadOnly, cursor],
  );
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [api, unreadOnly]);
  async function open(n: Notification) {
    await api.fetch('/api/p/notify/v1/inbox/read', { method: 'POST', json: { ids: [n.id] } });
    if (n.link) api.navigate(n.link);
    else setItems(items.map((x) => (x.id === n.id ? { ...x, read: true } : x)));
  }
  async function readAll() {
    await api.fetch('/api/p/notify/v1/inbox/read', { method: 'POST', json: { all: true } });
    setItems(items.map((x) => ({ ...x, read: true })));
  }
  return (
    <Card
      title={api.t('notify.ui.title')}
      actions={
        <span className="pt-row">
          <label className="pt-check">
            <input type="checkbox" checked={unreadOnly} onChange={(e) => setUnreadOnly(e.target.checked)} /> {api.t('notify.ui.unread_only')}
          </label>
          <Button variant="ghost" onClick={() => void readAll()}>
            {api.t('notify.ui.read_all')}
          </Button>
        </span>
      }
    >
      {items.length === 0 && <Empty>—</Empty>}
      <ul className="pt-list">
        {items.map((n) => (
          <li key={n.id} data-testid="notification" className={n.read ? 'pt-muted' : ''}>
            <a
              href={n.link ?? '#'}
              onClick={(e) => {
                e.preventDefault();
                void open(n);
              }}
            >
              <strong>{render(api, n, 'title')}</strong>
            </a>
            {render(api, n, 'body') && <span> · {render(api, n, 'body')}</span>}
            <span className="pt-muted"> · {new Date(n.created_at).toLocaleString()}</span>
          </li>
        ))}
      </ul>
      {cursor && (
        <Button variant="ghost" onClick={() => void load(true)}>
          {api.t('notify.ui.more')}
        </Button>
      )}
    </Card>
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
  const [saved, setSaved] = useState(false);
  useEffect(() => {
    void api.json<Record<string, Pref>>('/api/p/notify/v1/preferences').then(setPrefs);
  }, [api]);
  async function toggle(cat: string, channel: 'center' | 'email', on: boolean) {
    if (!prefs) return;
    setPrefs({ ...prefs, [cat]: { ...prefs[cat], [channel]: on } as Pref });
    await api.fetch('/api/p/notify/v1/preferences', { method: 'PUT', json: { [cat]: { [channel]: on } } });
    setSaved(true);
  }
  if (!prefs) return null;
  const emailOk = Object.values(prefs).some((p) => p.email_available);
  return (
    <Card title={api.t('notify.ui.tab')}>
      <table className="pt-table">
        <thead>
          <tr>
            <th>{api.t('notify.ui.category')}</th>
            <th>{api.t('notify.ui.center')}</th>
            {emailOk && <th>{api.t('notify.ui.email')}</th>}
          </tr>
        </thead>
        <tbody>
          {Object.entries(prefs).map(([cat, p]) => (
            <tr key={cat} title={p.locked ? api.t('notify.ui.locked') : undefined}>
              <td>{api.t(`term:notify.category.${cat}`)}</td>
              <td>
                <input type="checkbox" checked={p.center} disabled={p.locked} onChange={(e) => void toggle(cat, 'center', e.target.checked)} />
              </td>
              {emailOk && (
                <td>
                  <input type="checkbox" checked={p.email} disabled={p.locked} onChange={(e) => void toggle(cat, 'email', e.target.checked)} />
                </td>
              )}
            </tr>
          ))}
        </tbody>
      </table>
      {!emailOk && <p className="pt-muted">{api.t('notify.ui.email_unavailable')}</p>}
      {saved && <p className="pt-muted">{api.t('notify.ui.saved')}</p>}
    </Card>
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
    <Card title={api.t('notify.ui.unsub_title')}>
      {state === 'ready' && (
        <>
          <p>{api.t('notify.ui.unsub_confirm', { category: api.t(`term:notify.category.${category}`) })}</p>
          <Button variant="primary" onClick={() => void confirm()}>
            {api.t('notify.ui.unsub_button')}
          </Button>
        </>
      )}
      {state === 'done' && <p>{api.t('notify.ui.unsub_done')}</p>}
      {state === 'invalid' && <p className="pt-error">{api.t('notify.ui.unsub_invalid')}</p>}
    </Card>
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
