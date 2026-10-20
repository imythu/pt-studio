// ai-assistant 前端：用户菜单入口、问答页（/p/ai-assistant/：左侧历史会话、右侧对话，窄屏默认显示对话；来源引用、反馈、删除会话）、
// 规则文档页（/p/ai-assistant/docs/<id>，引用链接指向这里）、后台规则文档管理（/admin/p/ai-assistant/docs，抽屉编辑）。
import { useCallback, useEffect, useRef, useState } from 'react';
import { ApiError, renderPreview } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Checkbox, Field, Input, MasterDetail, NavItem, Note, PageHead, Panel, PLink, Sheet, SkeletonRows, Spinner, Status, Table, Tag, Textarea, UserContent } from '@pt-studio/ui-kit';

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
  'ai-assistant.ui.back': '会话',
  'ai-assistant.ui.no_sessions': '还没有会话',
  'ai-assistant.ui.you': '我',
  'ai-assistant.ui.bot': '助手',
  'ai-assistant.ui.thinking': '正在查阅规则…',
  'ai-assistant.ui.cancel': '取消',
  'ai-assistant.ui.new_doc': '新建文档',
  'ai-assistant.ui.edit_doc': '编辑文档',
  'ai-assistant.ui.inactive': '停用',
  'ai-assistant.ui.doc_lede': '站内助手回答问题时检索这些文档并标注来源。',
  'ai-assistant.ui.send_hint': 'Ctrl + Enter 发送',
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
  'ai-assistant.ui.back': 'Chats',
  'ai-assistant.ui.no_sessions': 'No chats yet',
  'ai-assistant.ui.you': 'Me',
  'ai-assistant.ui.bot': 'Assistant',
  'ai-assistant.ui.thinking': 'Checking the rules…',
  'ai-assistant.ui.cancel': 'Cancel',
  'ai-assistant.ui.new_doc': 'New document',
  'ai-assistant.ui.edit_doc': 'Edit document',
  'ai-assistant.ui.inactive': 'Inactive',
  'ai-assistant.ui.doc_lede': 'The assistant searches these documents and cites them in answers.',
  'ai-assistant.ui.send_hint': 'Ctrl + Enter to send',
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
    <NavItem to="/p/ai-assistant/" navigate={api.navigate}>
      {api.t('ai-assistant.ui.menu')}
    </NavItem>
  );
}

function Chat({ api }: { api: PluginApi }) {
  const [sessions, setSessions] = useState<{ id: string; first_question: string | null }[] | null>(null);
  const [sid, setSid] = useState<string | null>(null);
  const [msgs, setMsgs] = useState<Msg[]>([]);
  const [q, setQ] = useState('');
  const [pending, setPending] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [thanked, setThanked] = useState<number[]>([]);
  // 窄屏：默认显示对话（P2，起点是详情）；点“会话”回到列表
  const [showList, setShowList] = useState(false);
  const end = useRef<HTMLDivElement | null>(null);
  const loadSessions = useCallback(async () => {
    const r = await api.json<{ items: { id: string; first_question: string | null }[] }>('/api/p/ai-assistant/v1/sessions');
    setSessions(r.items);
  }, [api]);
  useEffect(() => {
    void loadSessions().catch(() => setSessions([]));
  }, [loadSessions]);
  useEffect(() => {
    end.current?.scrollIntoView({ block: 'nearest' });
  }, [msgs.length, pending]);
  async function open(id: string) {
    const r = await api.json<{ messages: Msg[] }>(`/api/p/ai-assistant/v1/sessions/${id}`);
    setSid(id);
    setMsgs(r.messages);
    setShowList(false);
  }
  async function ask() {
    const question = q.trim();
    if (!question || pending) return;
    setPending(question);
    setQ('');
    setErr(null);
    try {
      const r = await api.json<Msg & { session_id: string; message_id: number }>('/api/p/ai-assistant/v1/ask', { method: 'POST', json: { session_id: sid, question } });
      setSid(r.session_id);
      setMsgs((old) => [...old, { ...r, id: r.message_id, question }]);
      void loadSessions();
    } catch (e) {
      setErr(errText(api, e));
      setQ(question);
    } finally {
      setPending(null);
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
  const fresh = () => {
    setSid(null);
    setMsgs([]);
    setShowList(false);
  };
  const me = api.t('ai-assistant.ui.you');
  const bot = api.t('ai-assistant.ui.bot');
  return (
    <div className="stack">
      <PageHead title={api.t('ai-assistant.ui.title')} />
      <MasterDetail
        joined
        listWidth={280}
        showDetail={!showList}
        backLabel={api.t('ai-assistant.ui.back')}
        onBack={() => setShowList(true)}
        list={
          <>
            <header className="panel-h">
              <h2>{api.t('ai-assistant.ui.sessions')}</h2>
              <div className="acts">
                <Button size="xs" onClick={fresh}>
                  {api.t('ai-assistant.ui.new')}
                </Button>
              </div>
            </header>
            <div className="convs">
              {sessions === null ? (
                <SkeletonRows n={4} />
              ) : sessions.length === 0 ? (
                <div className="md-empty">{api.t('ai-assistant.ui.no_sessions')}</div>
              ) : (
                sessions.map((s) => (
                  <button key={s.id} type="button" className="conv ai-conv" aria-current={s.id === sid} onClick={() => void open(s.id)}>
                    <b>{s.first_question ?? s.id}</b>
                  </button>
                ))
              )}
            </div>
          </>
        }
        detail={
          <>
            <header className="panel-h conv-h">
              <h2>{msgs[0]?.question ?? api.t('ai-assistant.ui.new')}</h2>
              <div className="acts">
                {sid && (
                  <Button size="xs" variant="ghost" onClick={() => void remove()}>
                    {api.t('ai-assistant.ui.delete')}
                  </Button>
                )}
              </div>
            </header>
            <div className="thread">
              {msgs.length === 0 && !pending && <Note tone="info">{api.t('ai-assistant.ui.hint')}</Note>}
              {msgs.map((m) => (
                <div key={m.id} className="stack-s" data-testid="assistant-message">
                  <div className="bub me">
                    <span className="ava ai-ava">{me.slice(0, 1)}</span>
                    <div className="bub-b">{m.question}</div>
                  </div>
                  <div className="bub">
                    <span className="ava ai-ava bot">AI</span>
                    <div className="bub-b">
                      <div className="bub-h">
                        <b>{bot}</b>
                        {m.tools_used && m.tools_used.length > 0 && <span>{api.t('ai-assistant.ui.tools', { tools: m.tools_used.join(', ') })}</span>}
                      </div>
                      <div className="pt-prewrap">{m.answer}</div>
                      {m.citations.length > 0 && (
                        <div className="row ai-cites">
                          <span className="tiny muted">{api.t('ai-assistant.ui.sources')}</span>
                          {m.citations.map((c) => (
                            <PLink key={c.id} to={c.url} navigate={api.navigate} className="tag">
                              {c.title}
                            </PLink>
                          ))}
                        </div>
                      )}
                      <div className="row ai-fb">
                        {thanked.includes(m.id) || m.feedback ? (
                          <span className="tiny muted">{api.t('ai-assistant.ui.thanks')}</span>
                        ) : (
                          <>
                            <Button size="xs" variant="ghost" onClick={() => void feedback(m.id, true)}>
                              {api.t('ai-assistant.ui.useful')}
                            </Button>
                            <Button size="xs" variant="ghost" onClick={() => void feedback(m.id, false)}>
                              {api.t('ai-assistant.ui.not_useful')}
                            </Button>
                          </>
                        )}
                      </div>
                    </div>
                  </div>
                </div>
              ))}
              {pending && (
                <div className="stack-s">
                  <div className="bub me">
                    <span className="ava ai-ava">{me.slice(0, 1)}</span>
                    <div className="bub-b">{pending}</div>
                  </div>
                  <div className="bub">
                    <span className="ava ai-ava bot">AI</span>
                    <div className="bub-b row">
                      <Spinner /> <span className="small muted">{api.t('ai-assistant.ui.thinking')}</span>
                    </div>
                  </div>
                </div>
              )}
              <div ref={end} />
            </div>
            <form
              className="composer"
              onSubmit={(e) => {
                e.preventDefault();
                void ask();
              }}
            >
              {err && <Note tone="bad">{err}</Note>}
              <Textarea
                aria-label={api.t('ai-assistant.ui.ask')}
                rows={3}
                maxLength={2000}
                placeholder={api.t('ai-assistant.ui.placeholder')}
                value={q}
                onChange={(e) => setQ(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
                    e.preventDefault();
                    void ask();
                  }
                }}
              />
              <div className="row-sb">
                <span className="tiny muted">{api.t('ai-assistant.ui.send_hint')}</span>
                <Button type="submit" variant="primary" disabled={!!pending || !q.trim()}>
                  {api.t('ai-assistant.ui.ask')}
                </Button>
              </div>
            </form>
          </>
        }
      />
    </div>
  );
}

function Doc({ api, id }: { api: PluginApi; id: string }) {
  const [doc, setDoc] = useState<{ title: string; body_md: string } | null>(null);
  const [html, setHtml] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    void api
      .json<{ title: string; body_md: string }>(`/api/p/ai-assistant/v1/docs/${encodeURIComponent(id)}`)
      .then((d) => {
        setDoc(d);
        return renderPreview(d.body_md).then(setHtml);
      })
      .catch((e) => setErr(errText(api, e)));
  }, [api, id]);
  if (err) return <Note tone="bad">{err}</Note>;
  if (!doc) return <SkeletonRows n={8} />;
  return (
    <div className="stack narrow-form">
      <PageHead
        title={doc.title}
        crumb={
          <>
            <PLink to="/p/ai-assistant/" navigate={api.navigate}>
              {api.t('ai-assistant.ui.title')}
            </PLink>
            <span>{api.t('ai-assistant.ui.docs')}</span>
          </>
        }
      />
      <Panel primary>{html !== null ? <UserContent html={html} /> : <div className="pt-prewrap">{doc.body_md}</div>}</Panel>
    </div>
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
  const [rows, setRows] = useState<DocRow[] | null>(null);
  const [edit, setEdit] = useState<{ id: number | null; title: string; body_md: string; tags: string; active: boolean } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const load = useCallback(async () => {
    const r = await api.json<{ items: DocRow[] }>('/api/p/ai-assistant/v1/admin/docs');
    setRows(r.items);
  }, [api]);
  useEffect(() => {
    void load().catch((e) => setErr(errText(api, e)));
  }, [api, load]);
  async function startEdit(r: DocRow) {
    const d = await api.json<{ id: number; title: string; body_md: string; tags: string[] }>(`/api/p/ai-assistant/v1/docs/${r.id}`);
    setEdit({ id: d.id, title: d.title, body_md: d.body_md, tags: d.tags.join(', '), active: r.active });
  }
  async function save() {
    if (!edit) return;
    const body = { title: edit.title, body_md: edit.body_md, tags: edit.tags.split(',').map((s) => s.trim()).filter(Boolean), active: edit.active };
    try {
      if (edit.id === null) await api.json('/api/p/ai-assistant/v1/admin/docs', { method: 'POST', json: body });
      else await api.fetch(`/api/p/ai-assistant/v1/admin/docs/${edit.id}`, { method: 'PUT', json: body });
      setEdit(null);
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  }
  async function remove(id: number) {
    await api.fetch(`/api/p/ai-assistant/v1/admin/docs/${id}`, { method: 'DELETE' });
    await load();
  }
  return (
    <div className="stack">
      <PageHead
        title={api.t('ai-assistant.ui.docs')}
        lede={api.t('ai-assistant.ui.doc_lede')}
        actions={
          <Button variant="primary" onClick={() => setEdit({ id: null, title: '', body_md: '', tags: '', active: true })}>
            {api.t('ai-assistant.ui.add')}
          </Button>
        }
      />
      {err && <Note tone="bad">{err}</Note>}
      <Panel flush primary>
        {rows === null ? (
          <SkeletonRows n={4} />
        ) : (
          <Table
            rows={rows}
            rowKey={(r) => r.id}
            empty="—"
            onRowClick={(r) => void startEdit(r)}
            columns={[
              {
                key: 't',
                header: api.t('ai-assistant.ui.doc_title'),
                render: (r) => (
                  <>
                    <b>{r.title}</b>
                    {r.tags.length > 0 && (
                      <div className="row">
                        {r.tags.map((t) => (
                          <Tag key={t}>{t}</Tag>
                        ))}
                      </div>
                    )}
                  </>
                ),
              },
              { key: 'c', header: api.t('ai-assistant.ui.doc_chunks'), right: true, render: (r) => <span className="num">{r.chunks}</span> },
              { key: 'a', header: api.t('ai-assistant.ui.doc_active'), render: (r) => (r.active ? <Status tone="ok">{api.t('ai-assistant.ui.doc_active')}</Status> : <Status tone="mute">{api.t('ai-assistant.ui.inactive')}</Status>) },
              {
                key: 'x',
                header: '',
                right: true,
                render: (r) => (
                  <div className="form-acts">
                    <Button size="sm" variant="ghost" onClick={() => void startEdit(r)}>
                      {api.t('ai-assistant.ui.edit')}
                    </Button>
                    <Button size="sm" variant="ghost" onClick={() => void remove(r.id)}>
                      {api.t('ai-assistant.ui.remove')}
                    </Button>
                  </div>
                ),
              },
            ]}
          />
        )}
      </Panel>
      <Sheet
        open={edit !== null}
        onClose={() => setEdit(null)}
        width={720}
        title={api.t(edit?.id == null ? 'ai-assistant.ui.new_doc' : 'ai-assistant.ui.edit_doc')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setEdit(null)}>
              {api.t('ai-assistant.ui.cancel')}
            </Button>
            <Button variant="primary" disabled={!edit?.title.trim()} onClick={() => void save()}>
              {api.t('ai-assistant.ui.save')}
            </Button>
          </>
        }
      >
        {edit && (
          <div className="form">
            <Field label={api.t('ai-assistant.ui.doc_title')}>
              <Input name="doc_title" value={edit.title} onChange={(e) => setEdit({ ...edit, title: e.target.value })} />
            </Field>
            <Field label={api.t('ai-assistant.ui.doc_body')}>
              <Textarea className="ta code" name="doc_body" rows={16} value={edit.body_md} onChange={(e) => setEdit({ ...edit, body_md: e.target.value })} />
            </Field>
            <Field label={api.t('ai-assistant.ui.doc_tags')}>
              <Input name="doc_tags" value={edit.tags} onChange={(e) => setEdit({ ...edit, tags: e.target.value })} />
            </Field>
            <Checkbox label={api.t('ai-assistant.ui.doc_active')} checked={edit.active} onChange={(v) => setEdit({ ...edit, active: v })} />
          </div>
        )}
      </Sheet>
    </div>
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
