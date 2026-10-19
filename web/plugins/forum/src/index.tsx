// forum 前端：版块首页、主题列表与发主题（可带投票）、主题页（分页、引用、感谢、编辑 / 删除、投票、订阅、版主工具栏、
// 帖子编辑历史）、首页公告（home.announcements）、主导航入口、后台版块管理（新建、编辑、停用、增删版主）。
import { useEffect, useState, type FormEvent } from 'react';
import { ApiError, renderPreview } from '@pt-studio/sdk';
import { Slot, type PluginApi, type PluginFrontend, type SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Empty, Field, Input, MarkdownEditor, Table, UserContent } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'forum.ui.nav': '论坛',
  'forum.ui.boards': '论坛',
  'forum.ui.catchup': '全部标为已读',
  'forum.ui.topics': '主题',
  'forum.ui.posts': '帖子',
  'forum.ui.last': '最后发表',
  'forum.ui.unread': '新',
  'forum.ui.new_topic': '发主题',
  'forum.ui.title': '标题',
  'forum.ui.body': '内容（支持 Markdown）',
  'forum.ui.submit': '发表',
  'forum.ui.reply': '回复',
  'forum.ui.quote': '引用',
  'forum.ui.thanks': '感谢',
  'forum.ui.edit': '编辑',
  'forum.ui.delete': '删除',
  'forum.ui.save': '保存',
  'forum.ui.cancel': '取消',
  'forum.ui.deleted': '（已删除）',
  'forum.ui.floor': '#{n}',
  'forum.ui.prev': '上一页',
  'forum.ui.next': '下一页',
  'forum.ui.subscribe': '订阅',
  'forum.ui.unsubscribe': '取消订阅',
  'forum.ui.sticky': '置顶',
  'forum.ui.announcement': '公告',
  'forum.ui.featured': '精华',
  'forum.ui.locked': '已锁定',
  'forum.ui.poll': '投票',
  'forum.ui.add_poll': '附加投票',
  'forum.ui.poll_question': '投票问题',
  'forum.ui.poll_options': '选项（每行一个）',
  'forum.ui.poll_max': '最多选几项',
  'forum.ui.vote': '投票',
  'forum.ui.votes': '{n} 票',
  'forum.ui.closed': '已截止',
  'forum.ui.mod': '版主操作',
  'forum.ui.move_to': '移动到版块 ID',
  'forum.ui.announcements': '公告',
  'forum.ui.admin': '论坛管理',
  'forum.ui.tagline': '旅人们的思想交汇所与魔法研讨地',
  'forum.ui.no_boards': '论坛还没有任何版块。',
  'forum.ui.no_boards_admin': '去论坛管理创建分区与版块 →',
  'forum.ui.category': '分区',
  'forum.ui.board': '版块',
  'forum.ui.add_category': '新建分区',
  'forum.ui.add_board': '新建版块',
  'forum.ui.min_read': '阅读最低等级 ID',
  'forum.ui.min_reply': '回复最低等级 ID',
  'forum.ui.min_create': '发主题最低等级 ID',
  'forum.ui.moderators': '版主（用户 ID）',
  'forum.ui.add_moderator': '添加版主',
  'forum.ui.remove_moderator': '移除版主 #{id}',
  'forum.ui.edit_board': '编辑版块：{name}',
  'forum.ui.board_active': '启用',
  'forum.ui.inactive': '已停用',
  'forum.ui.saved': '已保存',
  'forum.ui.history': '编辑历史',
  'forum.ui.history_item': '{at} · {editor} 修改前：',
  'forum.ui.hide': '收起',
  'forum.board_not_found': '版块不存在',
  'forum.topic_not_found': '主题不存在',
  'forum.post_not_found': '帖子不存在',
  'forum.permission_denied': '没有权限',
  'forum.topic_locked': '主题已锁定',
  'forum.rate_limited': '发帖太快了，请稍后再试',
  'forum.title_invalid': '标题需要 1～120 个字符',
  'forum.body_invalid': '内容为空或过长',
  'forum.poll_closed': '投票已截止',
  'forum.poll_invalid': '投票选项不合法',
  'forum.edit_window_expired': '已超过可编辑时间',
  'forum.moderation_invalid': '操作参数不合法',
});
registerMessages('en', {
  'forum.ui.nav': 'Forum',
  'forum.ui.boards': 'Forum',
  'forum.ui.catchup': 'Mark all read',
  'forum.ui.topics': 'Topics',
  'forum.ui.posts': 'Posts',
  'forum.ui.last': 'Last post',
  'forum.ui.unread': 'New',
  'forum.ui.new_topic': 'New topic',
  'forum.ui.title': 'Title',
  'forum.ui.body': 'Text (Markdown)',
  'forum.ui.submit': 'Post',
  'forum.ui.reply': 'Reply',
  'forum.ui.quote': 'Quote',
  'forum.ui.thanks': 'Thanks',
  'forum.ui.edit': 'Edit',
  'forum.ui.delete': 'Delete',
  'forum.ui.save': 'Save',
  'forum.ui.cancel': 'Cancel',
  'forum.ui.deleted': '(deleted)',
  'forum.ui.floor': '#{n}',
  'forum.ui.prev': 'Previous',
  'forum.ui.next': 'Next',
  'forum.ui.subscribe': 'Subscribe',
  'forum.ui.unsubscribe': 'Unsubscribe',
  'forum.ui.sticky': 'Sticky',
  'forum.ui.announcement': 'Announcement',
  'forum.ui.featured': 'Featured',
  'forum.ui.locked': 'Locked',
  'forum.ui.poll': 'Poll',
  'forum.ui.add_poll': 'Add a poll',
  'forum.ui.poll_question': 'Question',
  'forum.ui.poll_options': 'Options (one per line)',
  'forum.ui.poll_max': 'Max choices',
  'forum.ui.vote': 'Vote',
  'forum.ui.votes': '{n} votes',
  'forum.ui.closed': 'Closed',
  'forum.ui.mod': 'Moderation',
  'forum.ui.move_to': 'Move to board ID',
  'forum.ui.announcements': 'Announcements',
  'forum.ui.admin': 'Forum admin',
  'forum.ui.tagline': 'Where travellers trade thoughts and study magic',
  'forum.ui.no_boards': 'The forum has no boards yet.',
  'forum.ui.no_boards_admin': 'Create categories and boards in forum admin →',
  'forum.ui.category': 'Category',
  'forum.ui.board': 'Board',
  'forum.ui.add_category': 'New category',
  'forum.ui.add_board': 'New board',
  'forum.ui.min_read': 'Min class ID to read',
  'forum.ui.min_reply': 'Min class ID to reply',
  'forum.ui.min_create': 'Min class ID to start topics',
  'forum.ui.moderators': 'Moderators (user IDs)',
  'forum.ui.add_moderator': 'Add moderator',
  'forum.ui.remove_moderator': 'Remove moderator #{id}',
  'forum.ui.edit_board': 'Edit board: {name}',
  'forum.ui.board_active': 'Active',
  'forum.ui.inactive': 'inactive',
  'forum.ui.saved': 'Saved',
  'forum.ui.history': 'Edit history',
  'forum.ui.history_item': '{at} · {editor}, before:',
  'forum.ui.hide': 'Hide',
  'forum.board_not_found': 'Board not found',
  'forum.topic_not_found': 'Topic not found',
  'forum.post_not_found': 'Post not found',
  'forum.permission_denied': 'Permission denied',
  'forum.topic_locked': 'The topic is locked',
  'forum.rate_limited': 'You are posting too fast',
  'forum.title_invalid': 'The title must be 1-120 characters',
  'forum.body_invalid': 'The text is empty or too long',
  'forum.poll_closed': 'The poll is closed',
  'forum.poll_invalid': 'Invalid poll choice',
  'forum.edit_window_expired': 'You can no longer edit this post',
  'forum.moderation_invalid': 'Invalid moderation request',
});

const errText = (api: PluginApi, e: unknown) => (e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e));
const when = (ms: number | null | undefined) => (ms ? new Date(ms).toLocaleString() : '—');

function A({ api, to, children }: { api: PluginApi; to: string; children: React.ReactNode }) {
  return (
    <a
      href={to}
      onClick={(e) => {
        e.preventDefault();
        api.navigate(to);
      }}
    >
      {children}
    </a>
  );
}

// ---------------- 版块首页 ----------------

interface BoardRow {
  id: number;
  name: string;
  description: string | null;
  topic_count: number;
  post_count: number;
  last_post_at: number | null;
  last_topic: { id: number; title: string } | null;
  unread: boolean;
}

function Boards({ api }: { api: PluginApi }) {
  const [cats, setCats] = useState<{ id: number; name: string; boards: BoardRow[] }[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const load = () =>
    api
      .json<{ categories: { id: number; name: string; boards: BoardRow[] }[] }>('/api/p/forum/v1/boards')
      .then((r) => {
        setCats(r.categories);
        setErr(null);
      })
      .catch((e: unknown) => setErr(errText(api, e)));
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const catchup = async () => {
    await api.json('/api/p/forum/v1/catchup', { json: {} });
    await load();
  };
  if (err && !cats) return <p className="pt-error">{err}</p>;
  if (!cats) return null;
  return (
    <div className="forum">
      <div
        className="forum-banner"
        style={{
          position: 'relative',
          borderRadius: 'var(--pt-radius-lg)',
          overflow: 'hidden',
          minHeight: 140,
          marginBottom: 18,
          display: 'flex',
          alignItems: 'flex-end',
          padding: '24px 20px',
          border: '1px solid var(--pt-color-border)',
          boxShadow: 'var(--pt-shadow-sm)',
        }}
      >
        <img
          src="/frieren/frieren-hero-field.jpg"
          alt=""
          style={{
            position: 'absolute',
            inset: 0,
            width: '100%',
            height: '100%',
            objectFit: 'cover',
            objectPosition: 'center 45%',
          }}
        />
        <div
          style={{
            position: 'absolute',
            inset: 0,
            background: 'linear-gradient(180deg, transparent 20%, rgb(15 23 42 / .75) 100%)',
          }}
        />
        <div
          style={{
            position: 'relative',
            zIndex: 1,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            width: '100%',
            flexWrap: 'wrap',
            gap: 12,
          }}
        >
          <div>
            <h1 style={{ margin: 0, color: '#fff', fontSize: 24, textShadow: '0 2px 8px rgb(0 0 0 / .6)' }}>
              {api.t('forum.ui.boards')}
            </h1>
            <p style={{ margin: '4px 0 0', color: '#e2e8f0', fontSize: 13, textShadow: '0 1px 4px rgb(0 0 0 / .6)' }}>
              {api.t('forum.ui.tagline')}
            </p>
          </div>
          {api.user() && (
            <Button
              variant="ghost"
              style={{ color: '#fff', borderColor: 'rgb(255 255 255 / .4)', backdropFilter: 'blur(4px)' }}
              onClick={() => void catchup()}
            >
              {api.t('forum.ui.catchup')}
            </Button>
          )}
        </div>
      </div>
      {cats.length === 0 && (
        <Empty illustration>
          {api.t('forum.ui.no_boards')}
          {api.isStaff() && (
            <>
              {' '}
              <A api={api} to="/admin/p/forum/">
                {api.t('forum.ui.no_boards_admin')}
              </A>
            </>
          )}
        </Empty>
      )}
      {cats.map((c) => (
        <Card key={c.id} title={c.name}>
          <Table
            rows={c.boards}
            rowKey={(b) => b.id}
            columns={[
              {
                key: 'n',
                header: api.t('forum.ui.board'),
                render: (b) => (
                  <>
                    <A api={api} to={`/p/forum/boards/${b.id}`}>
                      <strong>{b.name}</strong>
                    </A>{' '}
                    {b.unread && <Badge tone="primary">{api.t('forum.ui.unread')}</Badge>}
                    {b.description && <div className="pt-muted small">{b.description}</div>}
                  </>
                ),
              },
              { key: 't', header: api.t('forum.ui.topics'), render: (b) => b.topic_count },
              { key: 'p', header: api.t('forum.ui.posts'), render: (b) => b.post_count },
              {
                key: 'l',
                header: api.t('forum.ui.last'),
                render: (b) =>
                  b.last_topic ? (
                    <>
                      <A api={api} to={`/p/forum/topics/${b.last_topic.id}`}>
                        {b.last_topic.title}
                      </A>
                      <div className="pt-muted small">{when(b.last_post_at)}</div>
                    </>
                  ) : (
                    '—'
                  ),
              },
            ]}
          />
        </Card>
      ))}
    </div>
  );
}

// ---------------- 主题列表 ----------------

interface TopicRow {
  id: number;
  title: string;
  author: string | null;
  post_count: number;
  last_post_at: number;
  last_poster: string | null;
  sticky: boolean;
  announcement: boolean;
  featured: boolean;
  locked: boolean;
  unread: boolean;
  poll: boolean;
}

function Flags({ api, t }: { api: PluginApi; t: Partial<TopicRow> }) {
  return (
    <>
      {t.announcement && <Badge tone="danger">{api.t('forum.ui.announcement')}</Badge>} {t.sticky && <Badge tone="primary">{api.t('forum.ui.sticky')}</Badge>}{' '}
      {t.featured && <Badge tone="success">{api.t('forum.ui.featured')}</Badge>} {t.locked && <Badge>{api.t('forum.ui.locked')}</Badge>}{' '}
      {t.poll && <Badge>{api.t('forum.ui.poll')}</Badge>}
    </>
  );
}

function Composer({ api, onSubmit, withTitle, initial }: { api: PluginApi; onSubmit: (v: { title: string; body: string; poll?: unknown }) => Promise<void>; withTitle?: boolean; initial?: string }) {
  const [title, setTitle] = useState('');
  const [body, setBody] = useState(initial ?? '');
  const [poll, setPoll] = useState<{ question: string; options: string; max: string } | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (initial !== undefined) setBody(initial);
  }, [initial]);
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      await onSubmit({
        title,
        body,
        poll: poll ? { question: poll.question, options: poll.options.split('\n').map((o) => o.trim()).filter(Boolean), max_choices: Number(poll.max || '1') } : undefined,
      });
      setTitle('');
      setBody('');
      setPoll(null);
    } catch (err) {
      api.toast(errText(api, err), 'error');
    } finally {
      setBusy(false);
    }
  };
  return (
    <form onSubmit={submit} className="forum-composer">
      {withTitle && (
        <Field label={api.t('forum.ui.title')}>
          <Input name="topic_title" maxLength={120} value={title} onChange={(e) => setTitle(e.target.value)} required />
        </Field>
      )}
      <Field label={api.t('forum.ui.body')}>
        <MarkdownEditor name="post_body" rows={6} value={body} onChange={setBody} required preview={renderPreview} />
      </Field>
      {withTitle &&
        (poll ? (
          <>
            <Field label={api.t('forum.ui.poll_question')}>
              <Input name="poll_question" value={poll.question} onChange={(e) => setPoll({ ...poll, question: e.target.value })} required />
            </Field>
            <Field label={api.t('forum.ui.poll_options')}>
              <textarea className="pt-input" name="poll_options" rows={3} value={poll.options} onChange={(e) => setPoll({ ...poll, options: e.target.value })} required />
            </Field>
            <Field label={api.t('forum.ui.poll_max')}>
              <Input name="poll_max" type="number" min={1} value={poll.max} onChange={(e) => setPoll({ ...poll, max: e.target.value })} />
            </Field>
          </>
        ) : (
          <p>
            <Button type="button" variant="ghost" onClick={() => setPoll({ question: '', options: '', max: '1' })}>
              {api.t('forum.ui.add_poll')}
            </Button>
          </p>
        ))}
      <Button type="submit" variant="primary" disabled={busy}>
        {api.t('forum.ui.submit')}
      </Button>
    </form>
  );
}

function TopicList({ api, id }: { api: PluginApi; id: number }) {
  const [page, setPage] = useState(1);
  const [d, setD] = useState<{ board: { name: string; can_create: boolean }; topics: TopicRow[]; total: number; per_page: number } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const load = () =>
    api
      .json<{ board: { name: string; can_create: boolean }; topics: TopicRow[]; total: number; per_page: number }>(`/api/p/forum/v1/boards/${id}/topics?page=${page}`)
      .then(setD)
      .catch((e: unknown) => setErr(errText(api, e)));
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, page]);
  if (err) return <p className="pt-field-error">{err}</p>;
  if (!d) return null;
  const create = async (v: { title: string; body: string; poll?: unknown }) => {
    const r = await api.json<{ id: number }>(`/api/p/forum/v1/boards/${id}/topics`, { json: { title: v.title, body: v.body, markup: 'markdown', poll: v.poll ?? null } });
    api.navigate(`/p/forum/topics/${r.id}`);
  };
  const pages = Math.max(1, Math.ceil(d.total / d.per_page));
  return (
    <div className="forum">
      <p className="small">
        <A api={api} to="/p/forum/">
          {api.t('forum.ui.boards')}
        </A>{' '}
        › {d.board.name}
      </p>
      <Card title={d.board.name}>
        <Table
          rows={d.topics}
          rowKey={(t) => t.id}
          empty="—"
          columns={[
            {
              key: 't',
              header: api.t('forum.ui.topics'),
              render: (t) => (
                <>
                  <Flags api={api} t={t} />{' '}
                  <A api={api} to={`/p/forum/topics/${t.id}`}>
                    {t.unread ? <strong>{t.title}</strong> : t.title}
                  </A>{' '}
                  <span className="pt-muted small">{t.author ?? ''}</span>
                </>
              ),
            },
            { key: 'p', header: api.t('forum.ui.posts'), render: (t) => t.post_count },
            { key: 'l', header: api.t('forum.ui.last'), render: (t) => <span className="small">{`${t.last_poster ?? ''} · ${when(t.last_post_at)}`}</span> },
          ]}
        />
        {pages > 1 && (
          <p>
            <Button disabled={page <= 1} onClick={() => setPage(page - 1)}>
              {api.t('forum.ui.prev')}
            </Button>{' '}
            {page} / {pages}{' '}
            <Button disabled={page >= pages} onClick={() => setPage(page + 1)}>
              {api.t('forum.ui.next')}
            </Button>
          </p>
        )}
      </Card>
      {d.board.can_create && (
        <Card title={api.t('forum.ui.new_topic')}>
          <Composer api={api} withTitle onSubmit={create} />
        </Card>
      )}
    </div>
  );
}

// ---------------- 主题页 ----------------

interface Post {
  id: number;
  floor: number;
  author_id: number | null;
  author: string | null;
  html: string | null;
  state: string;
  thanks: number;
  thanked: boolean;
  created_at: number;
  edited_at: number | null;
  can_edit: boolean;
  can_delete: boolean;
  /** 可编辑时返回的原文 */
  source?: string | null;
}
interface Poll {
  question: string;
  options: string[];
  counts: number[];
  max_choices: number;
  closed: boolean;
  my_votes: number[];
}
interface TopicView {
  topic: { id: number; title: string; board_id: number; board: string; locked: boolean; sticky: boolean; announcement: boolean; featured: boolean; post_count: number; subscribed: boolean; can_reply: boolean; moderator: boolean };
  poll: Poll | null;
  posts: Post[];
  page: number;
  per_page: number;
}

function PollBox({ api, tid, poll, reload }: { api: PluginApi; tid: number; poll: Poll; reload: () => Promise<void> }) {
  const [picks, setPicks] = useState<number[]>(poll.my_votes);
  const total = poll.counts.reduce((a, b) => a + b, 0);
  const toggle = (i: number) => {
    if (poll.max_choices === 1) setPicks([i]);
    else setPicks(picks.includes(i) ? picks.filter((x) => x !== i) : [...picks, i]);
  };
  const vote = async () => {
    try {
      await api.json(`/api/p/forum/v1/topics/${tid}/vote`, { json: { options: picks } });
      await reload();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <Card title={`${api.t('forum.ui.poll')}：${poll.question}`}>
      {poll.options.map((o, i) => (
        <label key={i} className="pt-check forum-poll-option">
          <input type={poll.max_choices === 1 ? 'radio' : 'checkbox'} name="poll" checked={picks.includes(i)} disabled={poll.closed} onChange={() => toggle(i)} /> {o}{' '}
          <span className="pt-muted small">
            {api.t('forum.ui.votes', { n: poll.counts[i] ?? 0 })} {total ? `(${Math.round(((poll.counts[i] ?? 0) * 100) / total)}%)` : ''}
          </span>
        </label>
      ))}
      {poll.closed ? <Badge>{api.t('forum.ui.closed')}</Badge> : api.user() && <Button onClick={() => void vote()}>{api.t('forum.ui.vote')}</Button>}
    </Card>
  );
}

function TopicPage({ api, id }: { api: PluginApi; id: number }) {
  const startPost = new URLSearchParams(window.location.search).get('post');
  const [page, setPage] = useState<number | null>(null);
  const [d, setD] = useState<TopicView | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [quote, setQuote] = useState<number | null>(null);
  const [editing, setEditing] = useState<{ id: number; body: string } | null>(null);
  const [moveTo, setMoveTo] = useState('');
  const load = async () => {
    const q = page ? `?page=${page}` : startPost ? `?post=${startPost}` : '';
    try {
      setD(await api.json<TopicView>(`/api/p/forum/v1/topics/${id}${q}`));
    } catch (e) {
      setErr(errText(api, e));
    }
  };
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, page]);
  if (err) return <p className="pt-field-error">{err}</p>;
  if (!d) return null;
  const t = d.topic;
  const act = async (f: () => Promise<unknown>) => {
    try {
      await f();
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const replyTo = async (v: { body: string }) => {
    await api.json(`/api/p/forum/v1/topics/${id}/posts`, { json: { body: v.body, markup: 'markdown', quote_of: quote } });
    setQuote(null);
    setPage(Math.floor(t.post_count / d.per_page) + 1);
    await load();
  };
  const startEdit = (p: Post) => setEditing({ id: p.id, body: p.source ?? '' });
  const pages = Math.max(1, Math.ceil(t.post_count / d.per_page));
  const mod = (action: string, extra: Record<string, unknown> = {}) => act(() => api.json(`/api/p/forum/v1/topics/${id}/moderate`, { json: { action, ...extra } }));
  return (
    <div className="forum">
      <p className="small">
        <A api={api} to="/p/forum/">
          {api.t('forum.ui.boards')}
        </A>{' '}
        ›{' '}
        <A api={api} to={`/p/forum/boards/${t.board_id}`}>
          {t.board}
        </A>
      </p>
      <h1>
        {t.title} <Flags api={api} t={t} />
      </h1>
      {api.user() && (
        <p>
          <Button variant="ghost" onClick={() => void act(() => (t.subscribed ? api.fetch(`/api/p/forum/v1/topics/${id}/subscription`, { method: 'DELETE' }) : api.json(`/api/p/forum/v1/topics/${id}/subscription`, { json: {} })))}>
            {api.t(t.subscribed ? 'forum.ui.unsubscribe' : 'forum.ui.subscribe')}
          </Button>
        </p>
      )}
      {d.poll && <PollBox api={api} tid={id} poll={d.poll} reload={load} />}
      {d.posts.map((p) => (
        <Card
          key={p.id}
          title={
            <>
              {`${api.t('forum.ui.floor', { n: p.floor })} · `}
              {p.author_id !== null ? (
                <A api={api} to={`/users/${p.author_id}`}>
                  {p.author ?? `#${p.author_id}`}
                </A>
              ) : (
                (p.author ?? '—')
              )}
              {p.author_id !== null && <Slot name="user.name.decorations" ctx={{ user_id: p.author_id, size: 'sm' }} className="user-deco" />}
              {` · ${when(p.created_at)}${p.edited_at ? ' *' : ''}`}
            </>
          }
        >
          {editing?.id === p.id ? (
            <form
              onSubmit={(e) => {
                e.preventDefault();
                void act(async () => {
                  await api.json(`/api/p/forum/v1/posts/${p.id}`, { method: 'PATCH', json: { body: editing.body } });
                  setEditing(null);
                });
              }}
            >
              <MarkdownEditor name="post_edit" rows={6} value={editing.body} onChange={(b) => setEditing({ ...editing, body: b })} preview={renderPreview} />
              <Button type="submit" variant="primary">
                {api.t('forum.ui.save')}
              </Button>{' '}
              <Button type="button" onClick={() => setEditing(null)}>
                {api.t('forum.ui.cancel')}
              </Button>
            </form>
          ) : p.state === 'visible' ? (
            <UserContent html={p.html ?? ''} />
          ) : (
            <p className="pt-muted">{api.t('forum.ui.deleted')}</p>
          )}
          {p.state === 'visible' && api.user() && (
            <p className="forum-post-actions small">
              {t.can_reply && (
                <Button variant="ghost" onClick={() => setQuote(p.id)}>
                  {api.t('forum.ui.quote')}
                </Button>
              )}
              {p.author_id !== api.user()?.id && (
                <Button variant="ghost" onClick={() => void act(() => (p.thanked ? api.fetch(`/api/p/forum/v1/posts/${p.id}/thanks`, { method: 'DELETE' }) : api.json(`/api/p/forum/v1/posts/${p.id}/thanks`, { json: {} })))}>
                  {api.t('forum.ui.thanks')} {p.thanks > 0 ? p.thanks : ''}
                </Button>
              )}
              {p.author_id === api.user()?.id && p.thanks > 0 && <span className="pt-muted">{`${api.t('forum.ui.thanks')} ${p.thanks}`}</span>}
              {p.can_edit && (
                <Button variant="ghost" onClick={() => startEdit(p)}>
                  {api.t('forum.ui.edit')}
                </Button>
              )}
              {p.can_delete && (
                <Button variant="ghost" onClick={() => window.confirm('?') && void act(() => api.fetch(`/api/p/forum/v1/posts/${p.id}`, { method: 'DELETE' }))}>
                  {api.t('forum.ui.delete')}
                </Button>
              )}
              {t.moderator && p.edited_at && <PostHistory api={api} id={p.id} />}
            </p>
          )}
        </Card>
      ))}
      {pages > 1 && (
        <p>
          <Button disabled={d.page <= 1} onClick={() => setPage(d.page - 1)}>
            {api.t('forum.ui.prev')}
          </Button>{' '}
          {d.page} / {pages}{' '}
          <Button disabled={d.page >= pages} onClick={() => setPage(d.page + 1)}>
            {api.t('forum.ui.next')}
          </Button>
        </p>
      )}
      {t.can_reply && (
        <Card title={quote ? `${api.t('forum.ui.reply')}（${api.t('forum.ui.quote')} #${d.posts.find((p) => p.id === quote)?.floor ?? ''}）` : api.t('forum.ui.reply')}>
          <Composer api={api} onSubmit={replyTo} />
        </Card>
      )}
      {t.moderator && (
        <Card title={api.t('forum.ui.mod')}>
          <div className="searchbar">
            <Button onClick={() => void mod(t.sticky ? 'unsticky' : 'sticky')}>{(t.sticky ? '✕ ' : '') + api.t('forum.ui.sticky')}</Button>
            <Button onClick={() => void mod(t.locked ? 'unlock' : 'lock')}>{(t.locked ? '✕ ' : '') + api.t('forum.ui.locked')}</Button>
            <Button onClick={() => void mod(t.featured ? 'unfeature' : 'feature')}>{(t.featured ? '✕ ' : '') + api.t('forum.ui.featured')}</Button>
            <Button onClick={() => void mod(t.announcement ? 'unannounce' : 'announce')}>{(t.announcement ? '✕ ' : '') + api.t('forum.ui.announcement')}</Button>
            <Input name="move_to" type="number" placeholder={api.t('forum.ui.move_to')} value={moveTo} onChange={(e) => setMoveTo(e.target.value)} />
            <Button disabled={!moveTo} onClick={() => void mod('move', { to_board: Number(moveTo) })}>
              →
            </Button>
          </div>
        </Card>
      )}
    </div>
  );
}

// ---------------- 首页公告、导航 ----------------

function Announcements({ api }: SlotProps) {
  const [items, setItems] = useState<{ id: number; title: string }[]>([]);
  useEffect(() => {
    void api.json<{ items: { id: number; title: string }[] }>('/api/p/forum/v1/announcements').then((r) => setItems(r.items)).catch(() => undefined);
  }, [api]);
  if (!items.length) return null;
  return (
    <Card title={api.t('forum.ui.announcements')}>
      <ul>
        {items.map((i) => (
          <li key={i.id}>
            <A api={api} to={`/p/forum/topics/${i.id}`}>
              {i.title}
            </A>
          </li>
        ))}
      </ul>
    </Card>
  );
}

function NavItem({ api }: SlotProps) {
  return (
    <A api={api} to="/p/forum/">
      {api.t('forum.ui.nav')}
    </A>
  );
}

/** 帖子编辑历史（版主） */
function PostHistory({ api, id }: { api: PluginApi; id: number }) {
  const [items, setItems] = useState<{ editor_id: number; editor: string | null; source: string; edited_at: number }[] | null>(null);
  const toggle = async () => {
    if (items) {
      setItems(null);
      return;
    }
    try {
      setItems((await api.json<{ items: NonNullable<typeof items> }>(`/api/p/forum/v1/posts/${id}/history`)).items);
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <>
      <Button variant="ghost" onClick={() => void toggle()}>
        {api.t(items ? 'forum.ui.hide' : 'forum.ui.history')}
      </Button>
      {items && (
        <span className="pt-stack pt-muted small" style={{ flexBasis: '100%' }}>
          {items.map((h, i) => (
            <span key={i}>
              {api.t('forum.ui.history_item', { at: new Date(h.edited_at).toLocaleString(), editor: h.editor ?? `#${h.editor_id}` })}
              <pre className="pt-mono pt-prewrap">{h.source}</pre>
            </span>
          ))}
        </span>
      )}
    </>
  );
}

// ---------------- 后台 ----------------

interface AdminData {
  categories: { id: number; name: string }[];
  boards: { id: number; category_id: number; name: string; description: string | null; acl: Record<string, { min_class: number | null; roles?: string[] }>; active: boolean; moderators: number[] }[];
}

function Admin({ api }: { api: PluginApi }) {
  const [d, setD] = useState<AdminData | null>(null);
  const [cat, setCat] = useState('');
  const [f, setF] = useState({ category_id: '', name: '', description: '', read: '', reply: '', create: '' });
  const [modIds, setModIds] = useState<Record<number, string>>({});
  const [editing, setEditing] = useState<number | null>(null);
  const load = () => api.json<AdminData>('/api/p/forum/v1/admin/boards').then(setD);
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      await load();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  const num = (s: string) => (s.trim() === '' ? null : Number(s));
  if (!d) return null;
  return (
    <div>
      <Card title={api.t('forum.ui.admin')}>
        <Table
          rows={d.boards}
          rowKey={(b) => b.id}
          columns={[
            { key: 'id', header: 'ID', render: (b) => b.id },
            { key: 'c', header: api.t('forum.ui.category'), render: (b) => d.categories.find((c) => c.id === b.category_id)?.name ?? '' },
            {
              key: 'n',
              header: api.t('forum.ui.board'),
              render: (b) => (
                <>
                  {b.name} {!b.active && <Badge>{api.t('forum.ui.inactive')}</Badge>}{' '}
                  <Button variant="ghost" onClick={() => setEditing(b.id)}>
                    {api.t('forum.ui.edit')}
                  </Button>
                </>
              ),
            },
            { key: 'a', header: 'ACL', render: (b) => <code className="small">{['read', 'reply', 'create'].map((k) => `${k}≥${b.acl?.[k]?.min_class ?? '-'}`).join(' ')}</code> },
            {
              key: 'm',
              header: api.t('forum.ui.moderators'),
              render: (b) => (
                <span className="searchbar">
                  {b.moderators.map((m) => (
                    <Button key={m} variant="ghost" title={api.t('forum.ui.remove_moderator', { id: m })} onClick={() => void act(() => api.json(`/api/p/forum/v1/admin/boards/${b.id}/moderators/${m}`, { method: 'DELETE' }))}>
                      {`#${m} ✕`}
                    </Button>
                  ))}
                  <Input name={`mod_${b.id}`} type="number" value={modIds[b.id] ?? ''} onChange={(e) => setModIds({ ...modIds, [b.id]: e.target.value })} />
                  <Button disabled={!modIds[b.id]} onClick={() => void act(() => api.json(`/api/p/forum/v1/admin/boards/${b.id}/moderators`, { json: { user_id: Number(modIds[b.id]) } }))}>
                    +
                  </Button>
                </span>
              ),
            },
          ]}
        />
      </Card>
      {editing !== null && d.boards.some((b) => b.id === editing) && (
        <BoardEdit
          key={editing}
          api={api}
          board={d.boards.find((b) => b.id === editing) as AdminData['boards'][number]}
          categories={d.categories}
          onDone={() => {
            setEditing(null);
            void load();
          }}
        />
      )}
      <Card title={api.t('forum.ui.add_category')}>
        <form
          className="searchbar"
          onSubmit={(e) => {
            e.preventDefault();
            void act(async () => {
              await api.json('/api/p/forum/v1/admin/categories', { json: { name: cat } });
              setCat('');
            });
          }}
        >
          <Input name="category_name" value={cat} onChange={(e) => setCat(e.target.value)} required maxLength={60} />
          <Button type="submit">{api.t('forum.ui.add_category')}</Button>
        </form>
      </Card>
      <Card title={api.t('forum.ui.add_board')}>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void act(async () => {
              await api.json('/api/p/forum/v1/admin/boards', {
                json: {
                  category_id: Number(f.category_id),
                  name: f.name,
                  description: f.description || null,
                  acl: { read: { min_class: num(f.read) }, reply: { min_class: num(f.reply) }, create: { min_class: num(f.create) } },
                },
              });
              setF({ category_id: '', name: '', description: '', read: '', reply: '', create: '' });
            });
          }}
        >
          <Field label={api.t('forum.ui.category')}>
            <select className="pt-input" name="board_category" value={f.category_id} onChange={(e) => setF({ ...f, category_id: e.target.value })} required>
              <option value="" />
              {d.categories.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          </Field>
          <Field label={api.t('forum.ui.board')}>
            <Input name="board_name" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} required maxLength={60} />
          </Field>
          <Field label="">
            <Input name="board_description" value={f.description} onChange={(e) => setF({ ...f, description: e.target.value })} maxLength={300} />
          </Field>
          <Field label={api.t('forum.ui.min_read')}>
            <Input name="acl_read" type="number" value={f.read} onChange={(e) => setF({ ...f, read: e.target.value })} />
          </Field>
          <Field label={api.t('forum.ui.min_reply')}>
            <Input name="acl_reply" type="number" value={f.reply} onChange={(e) => setF({ ...f, reply: e.target.value })} />
          </Field>
          <Field label={api.t('forum.ui.min_create')}>
            <Input name="acl_create" type="number" value={f.create} onChange={(e) => setF({ ...f, create: e.target.value })} />
          </Field>
          <Button type="submit" variant="primary">
            {api.t('forum.ui.add_board')}
          </Button>
        </form>
      </Card>
    </div>
  );
}

function BoardEdit({ api, board, categories, onDone }: { api: PluginApi; board: AdminData['boards'][number]; categories: AdminData['categories']; onDone(): void }) {
  const s = (v: number | null | undefined) => (v === null || v === undefined ? '' : String(v));
  const [f, setF] = useState({
    category_id: String(board.category_id),
    name: board.name,
    description: board.description ?? '',
    read: s(board.acl?.read?.min_class),
    reply: s(board.acl?.reply?.min_class),
    create: s(board.acl?.create?.min_class),
    active: board.active,
  });
  const num = (v: string) => (v.trim() === '' ? null : Number(v));
  const save = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await api.json(`/api/p/forum/v1/admin/boards/${board.id}`, {
        method: 'PATCH',
        json: {
          category_id: Number(f.category_id),
          name: f.name,
          description: f.description,
          active: f.active,
          // 只编辑最低等级；按角色的放行规则原样保留
          acl: Object.fromEntries((['read', 'reply', 'create'] as const).map((k) => [k, { min_class: num(f[k]), roles: board.acl?.[k]?.roles ?? [] }])),
        },
      });
      api.toast(api.t('forum.ui.saved'), 'success');
      onDone();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  return (
    <Card title={api.t('forum.ui.edit_board', { name: board.name })}>
      <form onSubmit={(e) => void save(e)} className="upload-grid">
        <Field label={api.t('forum.ui.category')}>
          <select className="pt-input" name="edit_board_category" value={f.category_id} onChange={(e) => setF({ ...f, category_id: e.target.value })}>
            {categories.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        </Field>
        <Field label={api.t('forum.ui.board')}>
          <Input name="edit_board_name" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} required maxLength={60} />
        </Field>
        <div className="span2">
          <Field label="">
            <Input name="edit_board_description" value={f.description} onChange={(e) => setF({ ...f, description: e.target.value })} maxLength={300} />
          </Field>
        </div>
        <Field label={api.t('forum.ui.min_read')}>
          <Input name="edit_acl_read" type="number" value={f.read} onChange={(e) => setF({ ...f, read: e.target.value })} />
        </Field>
        <Field label={api.t('forum.ui.min_reply')}>
          <Input name="edit_acl_reply" type="number" value={f.reply} onChange={(e) => setF({ ...f, reply: e.target.value })} />
        </Field>
        <Field label={api.t('forum.ui.min_create')}>
          <Input name="edit_acl_create" type="number" value={f.create} onChange={(e) => setF({ ...f, create: e.target.value })} />
        </Field>
        <label className="pt-check">
          <input type="checkbox" name="edit_board_active" checked={f.active} onChange={(e) => setF({ ...f, active: e.target.checked })} /> {api.t('forum.ui.board_active')}
        </label>
        <div className="span2">
          <Button type="submit" variant="primary">
            {api.t('forum.ui.save')}
          </Button>{' '}
          <Button type="button" onClick={onDone}>
            {api.t('forum.ui.cancel')}
          </Button>
        </div>
      </form>
    </Card>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  if (path.startsWith('/admin/')) return <Admin api={api} />;
  const b = /^\/p\/forum\/boards\/(\d+)/.exec(path);
  if (b?.[1]) return <TopicList api={api} id={Number(b[1])} />;
  const t = /^\/p\/forum\/topics\/(\d+)/.exec(path);
  if (t?.[1]) return <TopicPage api={api} id={Number(t[1])} />;
  return <Boards api={api} />;
}

export default {
  slots: {
    'home.announcements': { component: Announcements, order: 10 },
    'nav.main': { component: NavItem, order: 20 },
  },
  routes: [
    { path: '/p/forum/*', component: Pages },
    { path: '/admin/p/forum/*', component: Pages },
  ],
} satisfies PluginFrontend;
