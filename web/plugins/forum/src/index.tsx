// forum 前端：版块首页、主题列表与发主题（可带投票）、主题页（分页、引用、感谢、编辑 / 删除、投票、订阅、版主工具栏、
// 帖子编辑历史）、首页公告（home.announcements）、主导航入口、后台版块管理（新建、编辑、停用、增删版主）。
import { useEffect, useState, type FormEvent, type ReactNode } from 'react';
import { ApiError, renderPreview } from '@pt-studio/sdk';
import { Slot, type PluginApi, type PluginFrontend, type SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Button, Checkbox, Empty, Field, Input, MarkdownEditor, NavItem as UiNavItem, Note, PageHead, Pager, Panel, PLink, Progress, Select, Sheet, SkeletonRows, Status, Switch, Table, Tag, Textarea, UserContent } from '@pt-studio/ui-kit';

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
  'forum.ui.remove_poll': '移除投票',
  'forum.ui.no_topics': '这个版块还没有主题。',
  'forum.ui.replies': '回复',
  'forum.ui.topic': '主题',
  'forum.ui.edited': '已编辑',
  'forum.ui.delete_confirm': '确定删除 #{n} 楼？删除后其他人只能看到“已删除”。',
  'forum.ui.move': '移动',
  'forum.ui.description': '版块简介',
  'forum.ui.mod_user_id': '用户 ID',
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
  'forum.ui.remove_poll': 'Remove poll',
  'forum.ui.no_topics': 'No topics in this board yet.',
  'forum.ui.replies': 'Replies',
  'forum.ui.topic': 'Topic',
  'forum.ui.edited': 'edited',
  'forum.ui.delete_confirm': 'Delete post #{n}? Others will only see "(deleted)".',
  'forum.ui.move': 'Move',
  'forum.ui.description': 'Description',
  'forum.ui.mod_user_id': 'User ID',
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
const when = (ms: number | null | undefined) => (ms ? new Date(ms).toLocaleString(undefined, { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' }) : '—');

function A({ api, to, children, className }: { api: PluginApi; to: string; children: ReactNode; className?: string }) {
  return (
    <PLink to={to} navigate={api.navigate} className={className}>
      {children}
    </PLink>
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
  if (err && !cats) return <Note tone="bad">{err}</Note>;
  if (!cats) return <SkeletonRows n={8} />;
  return (
    <div className="stack">
      <PageHead
        title={api.t('forum.ui.boards')}
        lede={api.t('forum.ui.tagline')}
        actions={
          api.user() && (
            <Button onClick={() => void catchup()}>
              {api.t('forum.ui.catchup')}
            </Button>
          )
        }
      />
      {cats.length === 0 && (
        <Panel>
          <Empty
            hint={
              api.isStaff() && (
                <A api={api} to="/admin/p/forum/">
                  {api.t('forum.ui.no_boards_admin')}
                </A>
              )
            }
          >
            {api.t('forum.ui.no_boards')}
          </Empty>
        </Panel>
      )}
      {cats.map((c, i) => (
        <Panel key={c.id} title={c.name} flush primary={i === 0}>
          {c.boards.map((b) => (
            <div key={b.id} className="board">
              <div>
                <A api={api} to={`/p/forum/boards/${b.id}`}>
                  {b.unread && <span className="unread-dot" title={api.t('forum.ui.unread')} />}
                  <b>{b.name}</b>
                </A>
                {b.description && <p>{b.description}</p>}
              </div>
              <span className="n">
                {b.topic_count}
                <small>{api.t('forum.ui.topics')}</small>
              </span>
              <span className="n">
                {b.post_count}
                <small>{api.t('forum.ui.posts')}</small>
              </span>
              <div className="last">
                {b.last_topic ? (
                  <>
                    <A api={api} to={`/p/forum/topics/${b.last_topic.id}`}>
                      {b.last_topic.title}
                    </A>
                    <span className="muted num">{when(b.last_post_at)}</span>
                  </>
                ) : (
                  <span className="muted">—</span>
                )}
              </div>
            </div>
          ))}
        </Panel>
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
  if (!t.announcement && !t.sticky && !t.featured && !t.locked && !t.poll) return null;
  return (
    <span className="flags">
      {t.announcement && <Tag tone="x">{api.t('forum.ui.announcement')}</Tag>}
      {t.sticky && <Tag tone="o">{api.t('forum.ui.sticky')}</Tag>}
      {t.featured && <Tag tone="g">{api.t('forum.ui.featured')}</Tag>}
      {t.locked && <Tag>{api.t('forum.ui.locked')}</Tag>}
      {t.poll && <Tag tone="k">{api.t('forum.ui.poll')}</Tag>}
    </span>
  );
}

interface Draft {
  title: string;
  body: string;
  poll?: { question: string; options: string[]; max_choices: number };
}

/** 发帖表单：formId 给定时不渲染自己的提交按钮（由抽屉底栏的 `form=` 按钮提交） */
function Composer({ api, onSubmit, withTitle, formId, compact }: { api: PluginApi; onSubmit: (v: Draft) => Promise<void>; withTitle?: boolean; formId?: string; compact?: boolean }) {
  const [title, setTitle] = useState('');
  const [body, setBody] = useState('');
  const [poll, setPoll] = useState<{ question: string; options: string; max: string } | null>(null);
  const [busy, setBusy] = useState(false);
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
    <form id={formId} onSubmit={(e) => void submit(e)} className="form">
      {withTitle && (
        <Field label={api.t('forum.ui.title')}>
          <Input name="topic_title" maxLength={120} value={title} onChange={(e) => setTitle(e.target.value)} required />
        </Field>
      )}
      <Field label={api.t('forum.ui.body')}>
        <MarkdownEditor name="post_body" rows={compact ? 4 : 8} value={body} onChange={setBody} required preview={renderPreview} />
      </Field>
      {withTitle &&
        (poll ? (
          <fieldset className="poll">
            <Field label={api.t('forum.ui.poll_question')}>
              <Input name="poll_question" value={poll.question} onChange={(e) => setPoll({ ...poll, question: e.target.value })} required />
            </Field>
            <Field label={api.t('forum.ui.poll_options')}>
              <Textarea name="poll_options" rows={3} value={poll.options} onChange={(e) => setPoll({ ...poll, options: e.target.value })} required />
            </Field>
            <Field label={api.t('forum.ui.poll_max')}>
              <Input name="poll_max" type="number" min={1} value={poll.max} onChange={(e) => setPoll({ ...poll, max: e.target.value })} />
            </Field>
            <div className="form-acts">
              <Button size="sm" variant="ghost" onClick={() => setPoll(null)}>
                {api.t('forum.ui.remove_poll')}
              </Button>
            </div>
          </fieldset>
        ) : (
          <div className="form-acts">
            <Button size="sm" variant="ghost" onClick={() => setPoll({ question: '', options: '', max: '1' })}>
              {api.t('forum.ui.add_poll')}
            </Button>
          </div>
        ))}
      {!formId && (
        <div className="form-acts">
          <Button type="submit" variant="primary" disabled={busy}>
            {api.t('forum.ui.submit')}
          </Button>
        </div>
      )}
    </form>
  );
}

function TopicList({ api, id }: { api: PluginApi; id: number }) {
  const [page, setPage] = useState(1);
  const [d, setD] = useState<{ board: { name: string; can_create: boolean }; topics: TopicRow[]; total: number; per_page: number } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [composing, setComposing] = useState(false);
  const load = () =>
    api
      .json<{ board: { name: string; can_create: boolean }; topics: TopicRow[]; total: number; per_page: number }>(`/api/p/forum/v1/boards/${id}/topics?page=${page}`)
      .then(setD)
      .catch((e: unknown) => setErr(errText(api, e)));
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, page]);
  if (err) return <Note tone="bad">{err}</Note>;
  if (!d) return <SkeletonRows n={8} />;
  const create = async (v: Draft) => {
    const r = await api.json<{ id: number }>(`/api/p/forum/v1/boards/${id}/topics`, { json: { title: v.title, body: v.body, markup: 'markdown', poll: v.poll ?? null } });
    api.navigate(`/p/forum/topics/${r.id}`);
  };
  const pages = Math.max(1, Math.ceil(d.total / d.per_page));
  return (
    <div className="stack">
      <PageHead
        crumb={
          <>
            <A api={api} to="/p/forum/">
              {api.t('forum.ui.boards')}
            </A>
            <span>{d.board.name}</span>
          </>
        }
        title={d.board.name}
        actions={
          d.board.can_create && (
            <Button variant="primary" onClick={() => setComposing(true)}>
              {api.t('forum.ui.new_topic')}
            </Button>
          )
        }
      />
      <Panel flush primary>
        {d.topics.length === 0 ? (
          <Empty compact>{api.t('forum.ui.no_topics')}</Empty>
        ) : (
          d.topics.map((t) => (
            <div key={t.id} className="topic">
              <div>
                <Flags api={api} t={t} />
                <A api={api} to={`/p/forum/topics/${t.id}`} className="ttl">
                  {t.unread && <span className="unread-dot" title={api.t('forum.ui.unread')} />}
                  {t.title}
                </A>
                <div className="meta">{t.author ?? ''}</div>
              </div>
              <span className="n">
                {Math.max(0, t.post_count - 1)}
                <small>{api.t('forum.ui.replies')}</small>
              </span>
              <span className="n">
                {t.post_count}
                <small>{api.t('forum.ui.posts')}</small>
              </span>
              <div className="last">
                <span>{t.last_poster ?? ''}</span>
                <span className="muted num">{when(t.last_post_at)}</span>
              </div>
            </div>
          ))
        )}
      </Panel>
      <Pager page={page} pages={pages} onChange={setPage} prev={api.t('forum.ui.prev')} next={api.t('forum.ui.next')} />
      <Sheet
        open={composing}
        onClose={() => setComposing(false)}
        title={api.t('forum.ui.new_topic')}
        width={720}
        footer={
          <>
            <Button variant="ghost" onClick={() => setComposing(false)}>
              {api.t('forum.ui.cancel')}
            </Button>
            <Button type="submit" form="forum-new-topic" variant="primary">
              {api.t('forum.ui.submit')}
            </Button>
          </>
        }
      >
        <Composer api={api} withTitle onSubmit={create} formId="forum-new-topic" />
      </Sheet>
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
    <div className="poll">
      <div className="row-sb">
        <b>{`${api.t('forum.ui.poll')} · ${poll.question}`}</b>
        {poll.closed && <Status tone="mute">{api.t('forum.ui.closed')}</Status>}
      </div>
      {poll.options.map((o, i) => {
        const c = poll.counts[i] ?? 0;
        return (
          <label key={i} className="poll-o">
            <input type={poll.max_choices === 1 ? 'radio' : 'checkbox'} name="poll" checked={picks.includes(i)} disabled={poll.closed} onChange={() => toggle(i)} />
            <span>{o}</span>
            <span className="num small muted right">
              {c} {total ? `· ${Math.round((c * 100) / total)}%` : ''}
            </span>
            <Progress value={c} max={Math.max(total, 1)} label={o} />
          </label>
        );
      })}
      {!poll.closed && api.user() && (
        <div className="form-acts">
          <Button size="sm" variant="primary" disabled={picks.length === 0} onClick={() => void vote()}>
            {api.t('forum.ui.vote')}
          </Button>
          <span className="small muted">{api.t('forum.ui.votes', { n: total })}</span>
        </div>
      )}
    </div>
  );
}

function TopicPage({ api, id }: { api: PluginApi; id: number }) {
  const startPost = new URLSearchParams(window.location.search).get('post');
  const [page, setPage] = useState<number | null>(null);
  const [d, setD] = useState<TopicView | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [quote, setQuote] = useState<number | null>(null);
  const [editing, setEditing] = useState<{ id: number; body: string } | null>(null);
  const [deleting, setDeleting] = useState<Post | null>(null);
  const [modOpen, setModOpen] = useState(false);
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
  if (err) return <Note tone="bad">{err}</Note>;
  if (!d) return <SkeletonRows n={8} />;
  const t = d.topic;
  const me = api.user()?.id;
  const act = async (f: () => Promise<unknown>) => {
    try {
      await f();
      await load();
      return true;
    } catch (e) {
      api.toast(errText(api, e), 'error');
      return false;
    }
  };
  const replyTo = async (v: Draft) => {
    await api.json(`/api/p/forum/v1/topics/${id}/posts`, { json: { body: v.body, markup: 'markdown', quote_of: quote } });
    setQuote(null);
    setPage(Math.floor(t.post_count / d.per_page) + 1);
    await load();
  };
  const pages = Math.max(1, Math.ceil(t.post_count / d.per_page));
  const mod = (action: string, extra: Record<string, unknown> = {}) => act(() => api.json(`/api/p/forum/v1/topics/${id}/moderate`, { json: { action, ...extra } }));
  const quoted = quote ? d.posts.find((p) => p.id === quote) : undefined;
  return (
    <div className="stack">
      <PageHead
        crumb={
          <>
            <A api={api} to="/p/forum/">
              {api.t('forum.ui.boards')}
            </A>
            <A api={api} to={`/p/forum/boards/${t.board_id}`}>
              {t.board}
            </A>
            <span>{api.t('forum.ui.topic')}</span>
          </>
        }
        title={
          <>
            <Flags api={api} t={t} />
            {t.title}
          </>
        }
        actions={
          <>
            {api.user() && (
              <Button
                onClick={() => void act(() => (t.subscribed ? api.fetch(`/api/p/forum/v1/topics/${id}/subscription`, { method: 'DELETE' }) : api.json(`/api/p/forum/v1/topics/${id}/subscription`, { json: {} })))}
              >
                {api.t(t.subscribed ? 'forum.ui.unsubscribe' : 'forum.ui.subscribe')}
              </Button>
            )}
            {t.moderator && <Button onClick={() => setModOpen(true)}>{api.t('forum.ui.mod')}</Button>}
          </>
        }
      />
      {d.poll && <PollBox api={api} tid={id} poll={d.poll} reload={load} />}
      <Panel flush primary>
        {d.posts.map((p) => (
          <article key={p.id} className="post" id={`post-${p.id}`}>
            <div className="post-a">
              {p.author_id !== null ? (
                <A api={api} to={`/users/${p.author_id}`}>
                  <b>{p.author ?? `#${p.author_id}`}</b>
                </A>
              ) : (
                <b>{p.author ?? '—'}</b>
              )}
              {p.author_id !== null && <Slot name="user.name.decorations" ctx={{ user_id: p.author_id, size: 'sm' }} className="user-deco" />}
            </div>
            <div className="post-b">
              <div className="post-h">
                <span className="num">
                  {when(p.created_at)}
                  {p.edited_at && <span title={when(p.edited_at)}>{` · ${api.t('forum.ui.edited')}`}</span>}
                </span>
                <span className="floor">{api.t('forum.ui.floor', { n: p.floor })}</span>
              </div>
              {p.state === 'visible' ? <UserContent html={p.html ?? ''} /> : <p className="muted">{api.t('forum.ui.deleted')}</p>}
              {p.state === 'visible' && api.user() && (
                <div className="post-f">
                  <span className="thx">{p.thanks > 0 ? `${api.t('forum.ui.thanks')} ${p.thanks}` : ''}</span>
                  {t.moderator && p.edited_at && <PostHistory api={api} id={p.id} />}
                  {p.can_edit && (
                    <Button size="xs" variant="ghost" onClick={() => setEditing({ id: p.id, body: p.source ?? '' })}>
                      {api.t('forum.ui.edit')}
                    </Button>
                  )}
                  {p.can_delete && (
                    <Button size="xs" variant="ghost" onClick={() => setDeleting(p)}>
                      {api.t('forum.ui.delete')}
                    </Button>
                  )}
                  {p.author_id !== me && (
                    <Button
                      size="xs"
                      variant={p.thanked ? 'gold' : 'ghost'}
                      aria-pressed={p.thanked}
                      onClick={() => void act(() => (p.thanked ? api.fetch(`/api/p/forum/v1/posts/${p.id}/thanks`, { method: 'DELETE' }) : api.json(`/api/p/forum/v1/posts/${p.id}/thanks`, { json: {} })))}
                    >
                      {api.t('forum.ui.thanks')}
                    </Button>
                  )}
                  {t.can_reply && (
                    <Button
                      size="xs"
                      variant="ghost"
                      onClick={() => {
                        setQuote(p.id);
                        document.getElementById('forum-reply')?.scrollIntoView({ behavior: 'smooth', block: 'center' });
                      }}
                    >
                      {api.t('forum.ui.quote')}
                    </Button>
                  )}
                </div>
              )}
            </div>
          </article>
        ))}
      </Panel>
      <Pager page={d.page} pages={pages} onChange={setPage} prev={api.t('forum.ui.prev')} next={api.t('forum.ui.next')} />
      {t.can_reply ? (
        <Panel
          id="forum-reply"
          title={api.t('forum.ui.reply')}
          actions={
            quoted && (
              <Button size="xs" variant="ghost" onClick={() => setQuote(null)}>
                {`${api.t('forum.ui.quote')} ${api.t('forum.ui.floor', { n: quoted.floor })} ✕`}
              </Button>
            )
          }
        >
          <Composer api={api} onSubmit={replyTo} compact />
        </Panel>
      ) : (
        t.locked && <Note>{api.t('forum.topic_locked')}</Note>
      )}
      <Sheet
        open={editing !== null}
        onClose={() => setEditing(null)}
        title={api.t('forum.ui.edit')}
        width={720}
        footer={
          <>
            <Button variant="ghost" onClick={() => setEditing(null)}>
              {api.t('forum.ui.cancel')}
            </Button>
            <Button type="submit" form="forum-edit" variant="primary">
              {api.t('forum.ui.save')}
            </Button>
          </>
        }
      >
        {editing && (
          <form
            id="forum-edit"
            className="form"
            onSubmit={(e) => {
              e.preventDefault();
              void act(() => api.json(`/api/p/forum/v1/posts/${editing.id}`, { method: 'PATCH', json: { body: editing.body } })).then((ok) => ok && setEditing(null));
            }}
          >
            <MarkdownEditor name="post_edit" rows={10} value={editing.body} onChange={(b) => setEditing({ ...editing, body: b })} preview={renderPreview} />
          </form>
        )}
      </Sheet>
      <Sheet
        open={deleting !== null}
        onClose={() => setDeleting(null)}
        title={api.t('forum.ui.delete')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDeleting(null)}>
              {api.t('forum.ui.cancel')}
            </Button>
            <Button
              variant="danger"
              onClick={() => deleting && void act(() => api.fetch(`/api/p/forum/v1/posts/${deleting.id}`, { method: 'DELETE' })).then(() => setDeleting(null))}
            >
              {api.t('forum.ui.delete')}
            </Button>
          </>
        }
      >
        <p>{deleting && api.t('forum.ui.delete_confirm', { n: deleting.floor })}</p>
      </Sheet>
      {t.moderator && (
        <Sheet open={modOpen} onClose={() => setModOpen(false)} title={api.t('forum.ui.mod')}>
          <div className="form">
            {(
              [
                ['sticky', t.sticky, 'unsticky', 'sticky'],
                ['locked', t.locked, 'unlock', 'lock'],
                ['featured', t.featured, 'unfeature', 'feature'],
                ['announcement', t.announcement, 'unannounce', 'announce'],
              ] as const
            ).map(([k, on, off, onAct]) => (
              <Switch key={k} showLabel checked={on} label={api.t(`forum.ui.${k}`)} onChange={() => void mod(on ? off : onAct)} />
            ))}
            <Field label={api.t('forum.ui.move_to')}>
              <div className="form-acts">
                <Input name="move_to" type="number" value={moveTo} onChange={(e) => setMoveTo(e.target.value)} />
                <Button disabled={!moveTo} onClick={() => void mod('move', { to_board: Number(moveTo) }).then((ok) => ok && setModOpen(false))}>
                  {api.t('forum.ui.move')}
                </Button>
              </div>
            </Field>
          </div>
        </Sheet>
      )}
    </div>
  );
}

// ---------------- 首页公告、导航 ----------------

function Announcements({ api }: SlotProps) {
  const [items, setItems] = useState<{ id: number; title: string }[]>([]);
  useEffect(() => {
    void api
      .json<{ items: { id: number; title: string }[] }>('/api/p/forum/v1/announcements')
      .then((r) => setItems(r.items))
      .catch(() => undefined);
  }, [api]);
  if (!items.length) return null;
  return (
    <Panel title={api.t('forum.ui.announcements')} flush>
      <div className="announce">
        {items.map((i) => (
          <div key={i.id} className="announce-row">
            <A api={api} to={`/p/forum/topics/${i.id}`} className="announce-link">
              <span className="announce-k">{api.t('forum.ui.announcement')}</span>
              <b>{i.title}</b>
            </A>
          </div>
        ))}
      </div>
    </Panel>
  );
}

function MainNav({ api }: SlotProps) {
  return (
    <UiNavItem to="/p/forum/" navigate={api.navigate}>
      {api.t('forum.ui.nav')}
    </UiNavItem>
  );
}

/** 帖子编辑历史（版主）：在抽屉里列出每次修改前的原文 */
function PostHistory({ api, id }: { api: PluginApi; id: number }) {
  const [items, setItems] = useState<{ editor_id: number; editor: string | null; source: string; edited_at: number }[] | null>(null);
  const open = async () => {
    try {
      setItems((await api.json<{ items: NonNullable<typeof items> }>(`/api/p/forum/v1/posts/${id}/history`)).items);
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  };
  return (
    <>
      <Button size="xs" variant="ghost" onClick={() => void open()}>
        {api.t('forum.ui.history')}
      </Button>
      <Sheet open={items !== null} onClose={() => setItems(null)} title={api.t('forum.ui.history')} width={640}>
        <div className="stack">
          {items?.map((h, i) => (
            <div key={i} className="stack-s">
              <span className="small muted">{api.t('forum.ui.history_item', { at: new Date(h.edited_at).toLocaleString(), editor: h.editor ?? `#${h.editor_id}` })}</span>
              <pre className="pre pt-prewrap">{h.source}</pre>
            </div>
          ))}
        </div>
      </Sheet>
    </>
  );
}

// ---------------- 后台 ----------------

type AdminBoard = { id: number; category_id: number; name: string; description: string | null; acl: Record<string, { min_class: number | null; roles?: string[] }>; active: boolean; moderators: number[] };
interface AdminData {
  categories: { id: number; name: string }[];
  boards: AdminBoard[];
}

const ACL_KEYS = ['read', 'reply', 'create'] as const;

function Admin({ api }: { api: PluginApi }) {
  const [d, setD] = useState<AdminData | null>(null);
  const [sheet, setSheet] = useState<'cat' | 'board' | null>(null);
  const [cat, setCat] = useState('');
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
      return true;
    } catch (e) {
      api.toast(errText(api, e), 'error');
      return false;
    }
  };
  if (!d) return <SkeletonRows n={6} />;
  const board = editing === null ? undefined : d.boards.find((b) => b.id === editing);
  return (
    <div className="stack">
      <PageHead
        title={api.t('forum.ui.admin')}
        actions={
          <>
            <Button onClick={() => setSheet('cat')}>{api.t('forum.ui.add_category')}</Button>
            <Button variant="primary" disabled={d.categories.length === 0} onClick={() => setSheet('board')}>
              {api.t('forum.ui.add_board')}
            </Button>
          </>
        }
      />
      <Panel flush primary>
        <Table
          rows={d.boards}
          rowKey={(b) => b.id}
          empty={api.t('forum.ui.no_boards')}
          minWidth={820}
          onRowClick={(b) => setEditing(b.id)}
          columns={[
            { key: 'id', header: 'ID', render: (b) => <span className="num muted">{b.id}</span> },
            { key: 'c', header: api.t('forum.ui.category'), render: (b) => d.categories.find((c) => c.id === b.category_id)?.name ?? '' },
            {
              key: 'n',
              header: api.t('forum.ui.board'),
              render: (b) => (
                <>
                  <b>{b.name}</b> {!b.active && <Status tone="mute">{api.t('forum.ui.inactive')}</Status>}
                  {b.description && <div className="tiny muted">{b.description}</div>}
                </>
              ),
            },
            { key: 'a', header: 'ACL', render: (b) => <span className="pt-mono tiny">{ACL_KEYS.map((k) => `${k}≥${b.acl?.[k]?.min_class ?? '-'}`).join(' ')}</span> },
            { key: 'm', header: api.t('forum.ui.moderators'), render: (b) => <span className="num small">{b.moderators.map((m) => `#${m}`).join(' ') || '—'}</span> },
            {
              key: 'x',
              header: '',
              right: true,
              render: (b) => (
                <Button size="sm" variant="ghost" onClick={() => setEditing(b.id)}>
                  {api.t('forum.ui.edit')}
                </Button>
              ),
            },
          ]}
        />
      </Panel>
      <Sheet
        open={sheet === 'cat'}
        onClose={() => setSheet(null)}
        title={api.t('forum.ui.add_category')}
        footer={
          <>
            <Button variant="ghost" onClick={() => setSheet(null)}>
              {api.t('forum.ui.cancel')}
            </Button>
            <Button type="submit" form="forum-cat" variant="primary">
              {api.t('forum.ui.add_category')}
            </Button>
          </>
        }
      >
        <form
          id="forum-cat"
          className="form"
          onSubmit={(e) => {
            e.preventDefault();
            void act(() => api.json('/api/p/forum/v1/admin/categories', { json: { name: cat } })).then((ok) => {
              if (ok) {
                setCat('');
                setSheet(null);
              }
            });
          }}
        >
          <Field label={api.t('forum.ui.category')}>
            <Input name="category_name" value={cat} onChange={(e) => setCat(e.target.value)} required maxLength={60} />
          </Field>
        </form>
      </Sheet>
      <BoardSheet api={api} open={sheet === 'board'} categories={d.categories} onClose={() => setSheet(null)} onSaved={() => void load()} />
      <BoardSheet key={editing ?? 'none'} api={api} open={!!board} board={board} categories={d.categories} onClose={() => setEditing(null)} onSaved={() => void load()} act={act} />
    </div>
  );
}

/** 版块新建 / 编辑抽屉；编辑时附带版主增删 */
function BoardSheet({
  api,
  open,
  board,
  categories,
  onClose,
  onSaved,
  act,
}: {
  api: PluginApi;
  open: boolean;
  board?: AdminBoard;
  categories: AdminData['categories'];
  onClose(): void;
  onSaved(): void;
  act?: (fn: () => Promise<unknown>) => Promise<boolean>;
}) {
  const s = (v: number | null | undefined) => (v === null || v === undefined ? '' : String(v));
  const init = () => ({
    category_id: board ? String(board.category_id) : '',
    name: board?.name ?? '',
    description: board?.description ?? '',
    read: s(board?.acl?.read?.min_class),
    reply: s(board?.acl?.reply?.min_class),
    create: s(board?.acl?.create?.min_class),
    active: board?.active ?? true,
  });
  const [f, setF] = useState(init);
  const [modId, setModId] = useState('');
  const num = (v: string) => (v.trim() === '' ? null : Number(v));
  const formId = board ? 'forum-board-edit' : 'forum-board-new';
  const save = async (e: FormEvent) => {
    e.preventDefault();
    try {
      if (board) {
        await api.json(`/api/p/forum/v1/admin/boards/${board.id}`, {
          method: 'PATCH',
          json: {
            category_id: Number(f.category_id),
            name: f.name,
            description: f.description,
            active: f.active,
            // 只编辑最低等级；按角色的放行规则原样保留
            acl: Object.fromEntries(ACL_KEYS.map((k) => [k, { min_class: num(f[k]), roles: board.acl?.[k]?.roles ?? [] }])),
          },
        });
      } else {
        await api.json('/api/p/forum/v1/admin/boards', {
          json: {
            category_id: Number(f.category_id),
            name: f.name,
            description: f.description || null,
            acl: Object.fromEntries(ACL_KEYS.map((k) => [k, { min_class: num(f[k]) }])),
          },
        });
        setF(init());
      }
      api.toast(api.t('forum.ui.saved'), 'success');
      onSaved();
      onClose();
    } catch (err) {
      api.toast(errText(api, err), 'error');
    }
  };
  return (
    <Sheet
      open={open}
      onClose={onClose}
      title={board ? api.t('forum.ui.edit_board', { name: board.name }) : api.t('forum.ui.add_board')}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {api.t('forum.ui.cancel')}
          </Button>
          <Button type="submit" form={formId} variant="primary">
            {api.t('forum.ui.save')}
          </Button>
        </>
      }
    >
      <form id={formId} className="form" onSubmit={(e) => void save(e)}>
        <div className="form-2">
          <Field label={api.t('forum.ui.category')}>
            <Select
              name={board ? 'edit_board_category' : 'board_category'}
              value={f.category_id}
              onChange={(e) => setF({ ...f, category_id: e.target.value })}
              required
              options={[...(board ? [] : [{ value: '', label: '—' }]), ...categories.map((c) => ({ value: String(c.id), label: c.name }))]}
            />
          </Field>
          <Field label={api.t('forum.ui.board')}>
            <Input name={board ? 'edit_board_name' : 'board_name'} value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} required maxLength={60} />
          </Field>
        </div>
        <Field label={api.t('forum.ui.description')}>
          <Input name={board ? 'edit_board_description' : 'board_description'} value={f.description} onChange={(e) => setF({ ...f, description: e.target.value })} maxLength={300} />
        </Field>
        <div className="form-2">
          {ACL_KEYS.map((k) => (
            <Field key={k} label={api.t(`forum.ui.min_${k}`)}>
              <Input name={board ? `edit_acl_${k}` : `acl_${k}`} type="number" value={f[k]} onChange={(e) => setF({ ...f, [k]: e.target.value })} />
            </Field>
          ))}
        </div>
        {board && <Checkbox name="edit_board_active" checked={f.active} onChange={(v) => setF({ ...f, active: v })} label={api.t('forum.ui.board_active')} />}
      </form>
      {board && act && (
        <div className="stack-s">
          <span className="eyebrow">{api.t('forum.ui.moderators')}</span>
          <div className="row">
            {board.moderators.length === 0 && <span className="small muted">—</span>}
            {board.moderators.map((m) => (
              <Button key={m} size="xs" variant="ghost" title={api.t('forum.ui.remove_moderator', { id: m })} onClick={() => void act(() => api.json(`/api/p/forum/v1/admin/boards/${board.id}/moderators/${m}`, { method: 'DELETE' }))}>
                {`#${m} ✕`}
              </Button>
            ))}
          </div>
          <div className="form-acts">
            <Input name={`mod_${board.id}`} type="number" placeholder={api.t('forum.ui.mod_user_id')} value={modId} onChange={(e) => setModId(e.target.value)} />
            <Button disabled={!modId} onClick={() => void act(() => api.json(`/api/p/forum/v1/admin/boards/${board.id}/moderators`, { json: { user_id: Number(modId) } })).then((ok) => ok && setModId(''))}>
              {api.t('forum.ui.add_moderator')}
            </Button>
          </div>
        </div>
      )}
    </Sheet>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  if (path.startsWith('/admin/')) return <Admin api={api} />;
  const b = /^\/p\/forum\/boards\/(\d+)/.exec(path);
  if (b?.[1]) return <TopicList key={b[1]} api={api} id={Number(b[1])} />;
  const t = /^\/p\/forum\/topics\/(\d+)/.exec(path);
  if (t?.[1]) return <TopicPage key={t[1]} api={api} id={Number(t[1])} />;
  return <Boards api={api} />;
}

export default {
  slots: {
    'home.announcements': { component: Announcements, order: 10 },
    'nav.main': { component: MainNav, order: 20 },
  },
  routes: [
    { path: '/p/forum/*', component: Pages },
    { path: '/admin/p/forum/*', component: Pages },
  ],
} satisfies PluginFrontend;
