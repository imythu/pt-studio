// messages 前端：用户菜单的未读站内信数、收件箱 / 已发送 / 系统（/p/messages/）、写信（/p/messages/new?to=）、会话（/p/messages/<id>：
// 回复、举报某条消息、拉黑、删除会话）。
import { useCallback, useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Empty, Field, Input, Table, Tabs, UserContent } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'messages.ui.menu': '站内信（{n}）',
  'messages.ui.title': '站内信',
  'messages.ui.inbox': '收件箱',
  'messages.ui.sent': '已发送',
  'messages.ui.system': '系统消息',
  'messages.ui.compose': '写信',
  'messages.ui.to': '收件人（用户名，逗号分隔）',
  'messages.ui.subject': '标题',
  'messages.ui.body': '正文',
  'messages.ui.send': '发送',
  'messages.ui.reply': '回复',
  'messages.ui.delete': '删除会话',
  'messages.ui.block': '拉黑对方',
  'messages.ui.blocked': '已拉黑',
  'messages.ui.report': '举报',
  'messages.ui.consent': '我同意把这条消息提交给管理员',
  'messages.ui.reported': '已举报',
  'messages.ui.with': '与',
  'messages.ui.system_sender': '系统',
});
registerMessages('en', {
  'messages.ui.menu': 'Messages ({n})',
  'messages.ui.title': 'Messages',
  'messages.ui.inbox': 'Inbox',
  'messages.ui.sent': 'Sent',
  'messages.ui.system': 'System',
  'messages.ui.compose': 'New message',
  'messages.ui.to': 'To (usernames, comma separated)',
  'messages.ui.subject': 'Subject',
  'messages.ui.body': 'Message',
  'messages.ui.send': 'Send',
  'messages.ui.reply': 'Reply',
  'messages.ui.delete': 'Delete conversation',
  'messages.ui.block': 'Block sender',
  'messages.ui.blocked': 'Blocked',
  'messages.ui.report': 'Report',
  'messages.ui.consent': 'I agree to share this message with staff',
  'messages.ui.reported': 'Reported',
  'messages.ui.with': 'With',
  'messages.ui.system_sender': 'System',
});

function errText(api: PluginApi, e: unknown): string {
  if (e instanceof ApiError) {
    const s = api.t(e.code);
    return s === e.code ? e.message : s;
  }
  return String(e);
}

function go(api: PluginApi, to: string) {
  return (e: { preventDefault(): void }) => {
    e.preventDefault();
    api.navigate(to);
  };
}

function MenuItem({ api }: SlotProps) {
  const [n, setN] = useState(0);
  useEffect(() => {
    void api
      .json<{ private: number }>('/api/p/messages/v1/unread')
      .then((r) => setN(r.private))
      .catch(() => undefined);
  }, [api]);
  return (
    <a href="/p/messages/" onClick={go(api, '/p/messages/')}>
      {api.t('messages.ui.menu', { n })}
    </a>
  );
}

interface Conv {
  id: number;
  subject: string;
  kind: string;
  unread: number;
  last_message_at: number;
  participants: { id: number; username: string }[];
}

function Folders({ api }: { api: PluginApi }) {
  const [folder, setFolder] = useState('inbox');
  const [rows, setRows] = useState<Conv[]>([]);
  useEffect(() => {
    void api.json<{ items: Conv[] }>(`/api/p/messages/v1/conversations?folder=${folder}`).then((r) => setRows(r.items));
  }, [api, folder]);
  return (
    <Card title={api.t('messages.ui.title')} actions={<Button variant="primary" onClick={() => api.navigate('/p/messages/new')}>{api.t('messages.ui.compose')}</Button>}>
      <Tabs
        tabs={['inbox', 'sent', 'system'].map((f) => ({ id: f, label: api.t(`messages.ui.${f}`) }))}
        value={folder}
        onChange={setFolder}
      />
      <Table
        rows={rows}
        rowKey={(r) => r.id}
        empty={<Empty>—</Empty>}
        columns={[
          {
            key: 's',
            header: api.t('messages.ui.subject'),
            render: (r) => (
              <a href={`/p/messages/${r.id}`} onClick={go(api, `/p/messages/${r.id}`)}>
                {r.subject} {r.unread > 0 && <Badge tone="danger">{r.unread}</Badge>}
              </a>
            ),
          },
          { key: 'p', header: api.t('messages.ui.with'), render: (r) => (r.kind === 'system' ? api.t('messages.ui.system_sender') : r.participants.map((p) => p.username).join(', ')) },
          { key: 't', header: '', render: (r) => new Date(r.last_message_at).toLocaleString() },
        ]}
      />
    </Card>
  );
}

function Compose({ api }: { api: PluginApi }) {
  const [to, setTo] = useState(() => new URLSearchParams(window.location.search).get('to') ?? '');
  const [subject, setSubject] = useState('');
  const [body, setBody] = useState('');
  const [err, setErr] = useState<string | null>(null);
  async function send() {
    setErr(null);
    try {
      const r = await api.json<{ conversation_id: number }>('/api/p/messages/v1/conversations', {
        method: 'POST',
        json: { recipients: to.split(',').map((s) => s.trim()).filter(Boolean), subject, body, markup: 'markdown' },
        idempotent: true,
      });
      api.navigate(`/p/messages/${r.conversation_id}`);
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  return (
    <Card title={api.t('messages.ui.compose')}>
      <div className="pt-stack">
        <Field label={api.t('messages.ui.to')}>
          <Input value={to} onChange={(e) => setTo(e.target.value)} />
        </Field>
        <Field label={api.t('messages.ui.subject')}>
          <Input value={subject} maxLength={200} onChange={(e) => setSubject(e.target.value)} />
        </Field>
        <Field label={api.t('messages.ui.body')}>
          <textarea className="pt-input" rows={8} value={body} onChange={(e) => setBody(e.target.value)} />
        </Field>
        <Button variant="primary" disabled={!to.trim() || !subject.trim() || !body.trim()} onClick={() => void send()}>
          {api.t('messages.ui.send')}
        </Button>
        {err && <p className="pt-error">{err}</p>}
      </div>
    </Card>
  );
}

interface Msg {
  id: number;
  sender: { id: number; username: string | null } | null;
  html: string;
  created_at: number;
}

function ReportMessage({ api, id }: { api: PluginApi; id: number }) {
  const [open, setOpen] = useState(false);
  const [consent, setConsent] = useState(false);
  const [done, setDone] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  async function send(code: string) {
    try {
      await api.json(`/api/p/messages/v1/messages/${id}/report`, { method: 'POST', json: { reason_code: code, note: '', consent } });
      setDone(true);
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  if (done) return <span className="pt-muted">{api.t('messages.ui.reported')}</span>;
  if (!open)
    return (
      <Button variant="ghost" onClick={() => setOpen(true)}>
        {api.t('messages.ui.report')}
      </Button>
    );
  return (
    <span className="pt-row">
      <label className="pt-check">
        <input type="checkbox" checked={consent} onChange={(e) => setConsent(e.target.checked)} /> {api.t('messages.ui.consent')}
      </label>
      {['spam', 'abuse', 'scam'].map((c) => (
        <Button key={c} variant="danger" disabled={!consent} onClick={() => void send(c)}>
          {api.t(`term:reports.reason.message.${c}`) === `reports.reason.message.${c}` ? c : api.t(`term:reports.reason.message.${c}`)}
        </Button>
      ))}
      {err && <span className="pt-error">{err}</span>}
    </span>
  );
}

function Conversation({ api, id }: { api: PluginApi; id: number }) {
  const [c, setC] = useState<{ subject: string; kind: string; participants: { id: number; username: string }[]; messages: Msg[] } | null>(null);
  const [body, setBody] = useState('');
  const [err, setErr] = useState<string | null>(null);
  const [blocked, setBlocked] = useState(false);
  const me = api.user();
  const load = useCallback(async () => setC(await api.json(`/api/p/messages/v1/conversations/${id}`)), [api, id]);
  useEffect(() => {
    void load().catch((e) => setErr(errText(api, e)));
  }, [api, load]);
  async function reply() {
    setErr(null);
    try {
      await api.json(`/api/p/messages/v1/conversations/${id}/messages`, { method: 'POST', json: { body, markup: 'markdown' } });
      setBody('');
      await load();
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  async function remove() {
    await api.fetch(`/api/p/messages/v1/conversations/${id}`, { method: 'DELETE' });
    api.navigate('/p/messages/');
  }
  async function block() {
    const other = c?.participants.find((p) => p.id !== me?.id);
    if (!other) return;
    await api.fetch('/api/p/messages/v1/blocks', { method: 'POST', json: { user: other.id } });
    setBlocked(true);
  }
  if (!c) return err ? <p className="pt-error">{err}</p> : null;
  return (
    <Card
      title={c.subject}
      actions={
        <span className="pt-row">
          {c.kind === 'user' && (
            <Button variant="ghost" disabled={blocked} onClick={() => void block()}>
              {api.t(blocked ? 'messages.ui.blocked' : 'messages.ui.block')}
            </Button>
          )}
          <Button variant="ghost" onClick={() => void remove()}>
            {api.t('messages.ui.delete')}
          </Button>
        </span>
      }
    >
      {c.messages.map((m) => (
        <div key={m.id} className="pt-comment" data-testid="message">
          <div className="pt-row pt-muted">
            <strong>{m.sender ? (m.sender.username ?? `#${m.sender.id}`) : api.t('messages.ui.system_sender')}</strong>
            <span>{new Date(m.created_at).toLocaleString()}</span>
            {m.sender && m.sender.id !== me?.id && <ReportMessage api={api} id={m.id} />}
          </div>
          <UserContent html={m.html} />
        </div>
      ))}
      {c.kind === 'user' && (
        <div className="pt-stack">
          <textarea className="pt-input" rows={4} aria-label={api.t('messages.ui.reply')} value={body} onChange={(e) => setBody(e.target.value)} />
          <Button variant="primary" disabled={!body.trim()} onClick={() => void reply()}>
            {api.t('messages.ui.reply')}
          </Button>
        </div>
      )}
      {err && <p className="pt-error">{err}</p>}
    </Card>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  const rest = path.replace(/^\/p\/messages\/?/, '');
  if (rest.startsWith('new')) return <Compose api={api} />;
  const id = Number(rest.split('/')[0]);
  if (id > 0) return <Conversation api={api} id={id} />;
  return <Folders api={api} />;
}

export default {
  slots: {
    'nav.user_menu': { component: MenuItem, order: 6 },
  },
  routes: [{ path: '/p/messages/*', component: Pages }],
} satisfies PluginFrontend;
