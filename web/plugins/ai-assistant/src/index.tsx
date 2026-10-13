// ai-assistant 前端：用户菜单入口、问答页（/p/ai-assistant/，会话列表、提问、来源引用、反馈、删除会话）、
// 规则文档页（/p/ai-assistant/docs/<id>，引用链接指向这里）、后台规则文档管理（/admin/p/ai-assistant/docs）。
import { useCallback, useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Card, Empty, Field, Input, Spinner, Table } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'ai-assistant.ui.menu': '站内助手',
  'ai-assistant.ui.title': '站内助手',
  'ai-assistant.ui.hint': '可以问本站规则与使用方法，例如“为什么我不能下载？”“H&R 怎么消除？”。回答依据站点规则文档，并标注来源。',
  'ai-assistant.ui.ask': '提问',
  'ai-assistant.ui.placeholder': '输入你的问题（2000 字以内）',
  'ai-assistant.ui.new': '新会话',
  'ai-assistant.ui.sessions': '历史会话',
  'ai-assistant.ui.sources': '来源',
  'ai-assistant.ui.tools': '查看了：{tools}',
  'ai-assistant.ui.useful': '有帮助',
  'ai-assistant.ui.not_useful': '没帮助',
  'ai-assistant.ui.thanks': '感谢反馈',
  'ai-assistant.ui.delete': '删除会话',
  'ai-assistant.ui.docs': '规则文档',
  'ai-assistant.ui.doc_title': '标题',
  'ai-assistant.ui.doc_body': '正文（Markdown，按二级标题切段）',
  'ai-assistant.ui.doc_tags': '标签（逗号分隔）',
  'ai-assistant.ui.doc_active': '启用',
  'ai-assistant.ui.doc_chunks': '段落',
  'ai-assistant.ui.save': '保存',
  'ai-assistant.ui.edit': '编辑',
  'ai-assistant.ui.remove': '删除',
  'ai-assistant.ui.add': '新建文档',
});
registerMessages('en', {
  'ai-assistant.ui.menu': 'Assistant',
  'ai-assistant.ui.title': 'Site assistant',
  'ai-assistant.ui.hint': 'Ask about site rules and how things work, e.g. “Why can’t I download?”. Answers are based on the site rules and cite their sources.',
  'ai-assistant.ui.ask': 'Ask',
  'ai-assistant.ui.placeholder': 'Your question (up to 2000 characters)',
  'ai-assistant.ui.new': 'New conversation',
  'ai-assistant.ui.sessions': 'Conversations',
  'ai-assistant.ui.sources': 'Sources',
  'ai-assistant.ui.tools': 'Checked: {tools}',
  'ai-assistant.ui.useful': 'Helpful',
  'ai-assistant.ui.not_useful': 'Not helpful',
  'ai-assistant.ui.thanks': 'Thanks for the feedback',
  'ai-assistant.ui.delete': 'Delete conversation',
  'ai-assistant.ui.docs': 'Rule documents',
  'ai-assistant.ui.doc_title': 'Title',
  'ai-assistant.ui.doc_body': 'Body (Markdown, split by level-2 headings)',
  'ai-assistant.ui.doc_tags': 'Tags (comma separated)',
  'ai-assistant.ui.doc_active': 'Active',
  'ai-assistant.ui.doc_chunks': 'Chunks',
  'ai-assistant.ui.save': 'Save',
  'ai-assistant.ui.edit': 'Edit',
  'ai-assistant.ui.remove': 'Delete',
  'ai-assistant.ui.add': 'New document',
});

function errText(api: PluginApi, e: unknown): string {
  if (e instanceof ApiError) {
    const s = api.t(e.code);
    return s === e.code ? e.message : s;
  }
  return String(e);
}

interface Citation {
  id: string;
  title: string;
  url: string;
}

interface Msg {
  id: number;
  question: string;
  answer: string;
  citations: Citation[];
  tools_used?: string[];
  feedback?: string | null;
}

function MenuItem({ api }: SlotProps) {
  return (
    <a
      href="/p/ai-assistant/"
      onClick={(e) => {
        e.preventDefault();
        api.navigate('/p/ai-assistant/');
      }}
    >
      {api.t('ai-assistant.ui.menu')}
    </a>
  );
}

function link(api: PluginApi, url: string, text: string) {
  return (
    <a
      href={url}
      onClick={(e) => {
        e.preventDefault();
        api.navigate(url);
      }}
    >
      {text}
    </a>
  );
}

function Chat({ api }: { api: PluginApi }) {
  const [sessions, setSessions] = useState<{ id: string; first_question: string | null }[]>([]);
  const [sid, setSid] = useState<string | null>(null);
  const [msgs, setMsgs] = useState<Msg[]>([]);
  const [q, setQ] = useState('');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [thanked, setThanked] = useState<number[]>([]);
  const loadSessions = useCallback(async () => {
    const r = await api.json<{ items: { id: string; first_question: string | null }[] }>('/api/p/ai-assistant/v1/sessions');
    setSessions(r.items);
  }, [api]);
  useEffect(() => {
    void loadSessions().catch(() => undefined);
  }, [loadSessions]);
  async function open(id: string) {
    const r = await api.json<{ messages: Msg[] }>(`/api/p/ai-assistant/v1/sessions/${id}`);
    setSid(id);
    setMsgs(r.messages);
  }
  async function ask() {
    if (!q.trim()) return;
    setBusy(true);
    setErr(null);
    try {
      const r = await api.json<Msg & { session_id: string; message_id: number }>('/api/p/ai-assistant/v1/ask', { method: 'POST', json: { session_id: sid, question: q } });
      setSid(r.session_id);
      setMsgs([...msgs, { ...r, id: r.message_id, question: q }]);
      setQ('');
      void loadSessions();
    } catch (e) {
      setErr(errText(api, e));
    } finally {
      setBusy(false);
    }
  }
  async function feedback(id: number, useful: boolean) {
    await api.fetch(`/api/p/ai-assistant/v1/messages/${id}/feedback`, { method: 'POST', json: { useful } });
    setThanked([...thanked, id]);
  }
  async function remove() {
    if (!sid) return;
    await api.fetch(`/api/p/ai-assistant/v1/sessions/${sid}`, { method: 'DELETE' });
    setSid(null);
    setMsgs([]);
    void loadSessions();
  }
  return (
    <div className="pt-row pt-align-start">
      <Card title={api.t('ai-assistant.ui.sessions')} className="pt-side">
        <Button
          variant="ghost"
          onClick={() => {
            setSid(null);
            setMsgs([]);
          }}
        >
          {api.t('ai-assistant.ui.new')}
        </Button>
        <ul className="pt-list">
          {sessions.map((s) => (
            <li key={s.id}>
              <a
                href="#"
                onClick={(e) => {
                  e.preventDefault();
                  void open(s.id);
                }}
              >
                {(s.first_question ?? s.id).slice(0, 40)}
              </a>
            </li>
          ))}
        </ul>
      </Card>
      <Card title={api.t('ai-assistant.ui.title')} actions={sid ? <Button variant="ghost" onClick={() => void remove()}>{api.t('ai-assistant.ui.delete')}</Button> : undefined}>
        <p className="pt-muted">{api.t('ai-assistant.ui.hint')}</p>
        {msgs.map((m) => (
          <div key={m.id} className="pt-card pt-stack" data-testid="assistant-message">
            <p>
              <strong>{m.question}</strong>
            </p>
            <p className="pt-prewrap">{m.answer}</p>
            {m.citations.length > 0 && (
              <p className="pt-muted">
                {api.t('ai-assistant.ui.sources')}：
                {m.citations.map((c) => (
                  <span key={c.id}> {link(api, c.url, c.title)}</span>
                ))}
              </p>
            )}
            {m.tools_used && m.tools_used.length > 0 && <p className="pt-muted">{api.t('ai-assistant.ui.tools', { tools: m.tools_used.join(', ') })}</p>}
            {thanked.includes(m.id) || m.feedback ? (
              <span className="pt-muted">{api.t('ai-assistant.ui.thanks')}</span>
            ) : (
              <div className="pt-row">
                <Button variant="ghost" onClick={() => void feedback(m.id, true)}>
                  {api.t('ai-assistant.ui.useful')}
                </Button>
                <Button variant="ghost" onClick={() => void feedback(m.id, false)}>
                  {api.t('ai-assistant.ui.not_useful')}
                </Button>
              </div>
            )}
          </div>
        ))}
        <Field label={api.t('ai-assistant.ui.ask')} error={err}>
          <textarea className="pt-input" rows={3} maxLength={2000} placeholder={api.t('ai-assistant.ui.placeholder')} value={q} onChange={(e) => setQ(e.target.value)} />
        </Field>
        <Button variant="primary" disabled={busy || !q.trim()} onClick={() => void ask()}>
          {busy ? <Spinner /> : api.t('ai-assistant.ui.ask')}
        </Button>
      </Card>
    </div>
  );
}

function Doc({ api, id }: { api: PluginApi; id: string }) {
  const [doc, setDoc] = useState<{ title: string; body_md: string } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    void api
      .json<{ title: string; body_md: string }>(`/api/p/ai-assistant/v1/docs/${encodeURIComponent(id)}`)
      .then(setDoc)
      .catch((e) => setErr(errText(api, e)));
  }, [api, id]);
  if (err) return <p className="pt-error">{err}</p>;
  if (!doc) return <Spinner />;
  return (
    <Card title={doc.title}>
      <div className="pt-prewrap">{doc.body_md}</div>
    </Card>
  );
}

interface DocRow {
  id: number;
  title: string;
  tags: string[];
  active: boolean;
  chunks: number;
}

function AdminDocs({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<DocRow[]>([]);
  const [edit, setEdit] = useState<{ id: number | null; title: string; body_md: string; tags: string; active: boolean } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(async () => {
    const r = await api.json<{ items: DocRow[] }>('/api/p/ai-assistant/v1/admin/docs');
    setRows(r.items);
  }, [api]);
  useEffect(() => {
    void load().catch((e) => setErr(errText(api, e)));
  }, [api, load]);
  async function startEdit(id: number) {
    const d = await api.json<{ id: number; title: string; body_md: string; tags: string[] }>(`/api/p/ai-assistant/v1/docs/${id}`);
    setEdit({ id: d.id, title: d.title, body_md: d.body_md, tags: d.tags.join(', '), active: true });
  }
  async function save() {
    if (!edit) return;
    setErr(null);
    const body = { title: edit.title, body_md: edit.body_md, tags: edit.tags.split(',').map((s) => s.trim()).filter(Boolean), active: edit.active };
    try {
      if (edit.id === null) await api.json('/api/p/ai-assistant/v1/admin/docs', { method: 'POST', json: body });
      else await api.fetch(`/api/p/ai-assistant/v1/admin/docs/${edit.id}`, { method: 'PUT', json: body });
      setEdit(null);
      await load();
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  async function remove(id: number) {
    await api.fetch(`/api/p/ai-assistant/v1/admin/docs/${id}`, { method: 'DELETE' });
    await load();
  }
  return (
    <Card title={api.t('ai-assistant.ui.docs')} actions={<Button onClick={() => setEdit({ id: null, title: '', body_md: '', tags: '', active: true })}>{api.t('ai-assistant.ui.add')}</Button>}>
      {err && <p className="pt-error">{err}</p>}
      {edit && (
        <div className="pt-stack">
          <Field label={api.t('ai-assistant.ui.doc_title')}>
            <Input value={edit.title} onChange={(e) => setEdit({ ...edit, title: e.target.value })} />
          </Field>
          <Field label={api.t('ai-assistant.ui.doc_body')}>
            <textarea className="pt-input pt-mono" rows={16} value={edit.body_md} onChange={(e) => setEdit({ ...edit, body_md: e.target.value })} />
          </Field>
          <Field label={api.t('ai-assistant.ui.doc_tags')}>
            <Input value={edit.tags} onChange={(e) => setEdit({ ...edit, tags: e.target.value })} />
          </Field>
          <label className="pt-check">
            <input type="checkbox" checked={edit.active} onChange={(e) => setEdit({ ...edit, active: e.target.checked })} /> {api.t('ai-assistant.ui.doc_active')}
          </label>
          <Button variant="primary" onClick={() => void save()}>
            {api.t('ai-assistant.ui.save')}
          </Button>
        </div>
      )}
      <Table
        rows={rows}
        rowKey={(r) => r.id}
        empty={<Empty>—</Empty>}
        columns={[
          { key: 't', header: api.t('ai-assistant.ui.doc_title'), render: (r) => r.title },
          { key: 'c', header: api.t('ai-assistant.ui.doc_chunks'), render: (r) => r.chunks },
          { key: 'a', header: api.t('ai-assistant.ui.doc_active'), render: (r) => (r.active ? '✓' : '✗') },
          {
            key: 'x',
            header: '',
            render: (r) => (
              <span className="pt-row">
                <Button variant="ghost" onClick={() => void startEdit(r.id)}>
                  {api.t('ai-assistant.ui.edit')}
                </Button>
                <Button variant="danger" onClick={() => void remove(r.id)}>
                  {api.t('ai-assistant.ui.remove')}
                </Button>
              </span>
            ),
          },
        ]}
      />
    </Card>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  if (path.startsWith('/admin/p/ai-assistant')) return <AdminDocs api={api} />;
  const m = /^\/p\/ai-assistant\/docs\/([^/]+)/.exec(path);
  if (m?.[1]) return <Doc api={api} id={m[1]} />;
  return <Chat api={api} />;
}

export default {
  slots: {
    'nav.user_menu': { component: MenuItem, order: 70 },
  },
  routes: [
    { path: '/p/ai-assistant/*', component: Pages },
    { path: '/admin/p/ai-assistant/*', component: Pages },
  ],
} satisfies PluginFrontend;
