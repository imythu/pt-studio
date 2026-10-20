// comments 前端：种子、作品、求种页的评论区——楼层、回复、引用、感谢、编辑、删除、举报（抽屉）、Markdown / BBCode 与预览、
// 编辑历史（comments.moderate）；个人页「评论」页签（该用户最近的评论）。
import { useCallback, useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import { Slot, type PluginApi, type PluginFrontend, type SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Avatar, Button, Empty, Field, LoadMore, Note, PLink, Segmented, Select, Sheet, SkeletonRows, Textarea, UserContent } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'comments.ui.tab': '评论',
  'comments.ui.count': '{n} 条评论',
  'comments.ui.placeholder': '写下你的评论…',
  'comments.ui.post': '发表',
  'comments.ui.preview': '预览',
  'comments.ui.write': '编辑',
  'comments.ui.reply': '回复',
  'comments.ui.quote': '引用',
  'comments.ui.thanks': '感谢（{n}）',
  'comments.ui.thanked': '已感谢（{n}）',
  'comments.ui.edit': '编辑',
  'comments.ui.save': '保存',
  'comments.ui.cancel': '取消',
  'comments.ui.delete': '删除',
  'comments.ui.deleted': '该评论已删除',
  'comments.ui.edited': '已编辑',
  'comments.ui.more': '加载更多',
  'comments.ui.replying': '回复 #{floor}',
  'comments.ui.quoting': '引用 #{floor}',
  'comments.ui.report': '举报',
  'comments.ui.report_title': '举报评论',
  'comments.ui.reported': '已举报，感谢反馈',
  'comments.ui.reason': '举报理由',
  'comments.ui.history': '编辑历史',
  'comments.ui.history_denied': '只有管理员可以查看编辑历史',
  'comments.ui.history_item': '{at} · 编辑者 #{editor} 修改前：',
  'comments.ui.hide': '收起',
  'comments.ui.recent': '最近的评论',
  'comments.ui.on_torrent': '种子 #{id}',
  'comments.ui.on_group': '作品 #{id}',
  'comments.ui.on_request': '求种 #{id}',
  'comments.ui.empty': '还没有评论',
  'comments.ui.empty_hint': '说说你对这个资源的看法，或者感谢一下发布者。',
});
registerMessages('en', {
  'comments.ui.tab': 'Comments',
  'comments.ui.count': '{n} comments',
  'comments.ui.placeholder': 'Write a comment…',
  'comments.ui.post': 'Post',
  'comments.ui.preview': 'Preview',
  'comments.ui.write': 'Write',
  'comments.ui.reply': 'Reply',
  'comments.ui.quote': 'Quote',
  'comments.ui.thanks': 'Thanks ({n})',
  'comments.ui.thanked': 'Thanked ({n})',
  'comments.ui.edit': 'Edit',
  'comments.ui.save': 'Save',
  'comments.ui.cancel': 'Cancel',
  'comments.ui.delete': 'Delete',
  'comments.ui.deleted': 'This comment was deleted',
  'comments.ui.edited': 'edited',
  'comments.ui.more': 'Load more',
  'comments.ui.replying': 'Replying to #{floor}',
  'comments.ui.quoting': 'Quoting #{floor}',
  'comments.ui.report': 'Report',
  'comments.ui.report_title': 'Report comment',
  'comments.ui.reported': 'Reported, thanks',
  'comments.ui.reason': 'Reason',
  'comments.ui.history': 'Edit history',
  'comments.ui.history_denied': 'Only staff can see the edit history',
  'comments.ui.history_item': '{at} · editor #{editor}, before:',
  'comments.ui.hide': 'Hide',
  'comments.ui.recent': 'Recent comments',
  'comments.ui.on_torrent': 'Torrent #{id}',
  'comments.ui.on_group': 'Title #{id}',
  'comments.ui.on_request': 'Request #{id}',
  'comments.ui.empty': 'No comments yet',
  'comments.ui.empty_hint': 'Share what you think about this release, or thank the uploader.',
});

interface Comment {
  id: number;
  floor: number;
  reply_to: number | null;
  author: { id: number; username: string | null } | null;
  html: string | null;
  deleted: boolean;
  thanks: number;
  thanked_by_me: boolean;
  created_at: number;
  edited_at: number | null;
  can_edit: boolean;
  can_delete: boolean;
  replies?: Comment[];
  reply_count?: number;
}

function errText(api: PluginApi, e: unknown): string {
  if (e instanceof ApiError) {
    const s = api.t(e.code);
    return s === e.code ? e.message : s;
  }
  return String(e);
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

/** 举报评论：理由在抽屉里选择 */
function ReportSheet({ api, id, onClose }: { api: PluginApi; id: number | null; onClose(): void }) {
  const [reasons, setReasons] = useState<string[]>([]);
  const [code, setCode] = useState('');
  useEffect(() => {
    if (id === null) return;
    void api
      .json<{ items: { code: string }[] }>('/api/p/reports/v1/reasons?kind=comment')
      .then((r) => {
        const xs = r.items.map((x) => x.code).filter((c) => c !== 'other');
        setReasons(xs);
        setCode(xs[0] ?? '');
      })
      .catch(() => setReasons([]));
  }, [api, id]);
  const label = (c: string) => {
    const k = `reports.reason.comment.${c}`;
    const v = api.t(`term:${k}`);
    return v === k ? c : v;
  };
  async function send() {
    if (id === null) return;
    try {
      await api.json('/api/p/reports/v1/reports', { method: 'POST', json: { target: `comment:${id}`, reason_code: code, note: '' } });
      api.toast(api.t('comments.ui.reported'), 'success');
      onClose();
    } catch (e) {
      api.toast(errText(api, e), 'error');
    }
  }
  return (
    <Sheet
      open={id !== null}
      onClose={onClose}
      title={api.t('comments.ui.report_title')}
      footer={
        <Button variant="danger" disabled={!code} onClick={() => void send()}>
          {api.t('comments.ui.report')}
        </Button>
      }
    >
      <Field label={api.t('comments.ui.reason')}>
        <Select value={code} onChange={(e) => setCode(e.target.value)} options={reasons.map((c) => ({ value: c, label: label(c) }))} />
      </Field>
    </Sheet>
  );
}

function Item({ api, c, onReply, onQuote, onReport, reload }: { api: PluginApi; c: Comment; onReply?: (c: Comment) => void; onQuote: (c: Comment) => void; onReport(id: number): void; reload(): void }) {
  const [editing, setEditing] = useState<string | null>(null);
  const [history, setHistory] = useState<{ editor_id: number; old_source: string; edited_at: number }[] | null>(null);
  const [thanks, setThanks] = useState({ n: c.thanks, mine: c.thanked_by_me });
  const me = api.user();
  const name = c.author ? (c.author.username ?? `#${c.author.id}`) : '';
  async function startEdit() {
    const r = await api.json<{ source: string }>(`/api/p/comments/v1/comments/${c.id}?raw=1`);
    setEditing(r.source);
  }
  async function save() {
    await api.fetch(`/api/p/comments/v1/comments/${c.id}`, { method: 'PATCH', json: { body: editing } });
    setEditing(null);
    reload();
  }
  async function remove() {
    await api.fetch(`/api/p/comments/v1/comments/${c.id}`, { method: 'DELETE' });
    reload();
  }
  async function toggleHistory() {
    if (history) {
      setHistory(null);
      return;
    }
    try {
      setHistory((await api.json<{ items: { editor_id: number; old_source: string; edited_at: number }[] }>(`/api/p/comments/v1/comments/${c.id}/edits`)).items);
    } catch (e) {
      api.toast(e instanceof ApiError && e.status === 403 ? api.t('comments.ui.history_denied') : errText(api, e), 'error');
    }
  }
  async function thank() {
    await api.fetch(`/api/p/comments/v1/comments/${c.id}/thanks`, { method: 'POST', json: {} });
    setThanks({ n: thanks.n + 1, mine: true });
  }
  return (
    <article className="cmt" data-testid="comment" id={`comment-${c.id}`}>
      <Avatar name={name} size={36} />
      <div className="cmt-b">
        <div className="cmt-h">
          {c.author && (
            <PLink to={`/users/${c.author.id}`} navigate={api.navigate} className="u">
              {name}
            </PLink>
          )}
          {c.author && <Slot name="user.name.decorations" ctx={{ user_id: c.author.id, size: 'sm' }} className="user-deco" />}
          <span className="muted" title={new Date(c.created_at).toLocaleString()}>
            {rel(c.created_at)}
          </span>
          {c.edited_at && (
            <button type="button" className="txt-btn tiny" title={api.t('comments.ui.history')} onClick={() => void toggleHistory()}>
              {api.t(history ? 'comments.ui.hide' : 'comments.ui.edited')}
            </button>
          )}
          <span className="floor">#{c.floor}</span>
        </div>
        {history && (
          <div className="stack-s small muted cmt-hist">
            {history.map((h, i) => (
              <div key={i}>
                {api.t('comments.ui.history_item', { at: new Date(h.edited_at).toLocaleString(), editor: h.editor_id })}
                <pre className="json">{h.old_source}</pre>
              </div>
            ))}
          </div>
        )}
        {c.deleted && !c.html ? (
          <p className="muted small">{api.t('comments.ui.deleted')}</p>
        ) : editing !== null ? (
          <div className="stack-s">
            <Textarea rows={4} value={editing} onChange={(e) => setEditing(e.target.value)} />
            <div className="row">
              <Button size="sm" variant="primary" onClick={() => void save()}>
                {api.t('comments.ui.save')}
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setEditing(null)}>
                {api.t('comments.ui.cancel')}
              </Button>
            </div>
          </div>
        ) : (
          <UserContent html={c.html ?? ''} />
        )}
        {!c.deleted && me && editing === null && (
          <div className="cmt-acts">
            {onReply && (
              <button type="button" className="txt-btn" onClick={() => onReply(c)}>
                {api.t('comments.ui.reply')}
              </button>
            )}
            <button type="button" className="txt-btn" onClick={() => onQuote(c)}>
              {api.t('comments.ui.quote')}
            </button>
            {c.author?.id !== me.id ? (
              <button type="button" className="txt-btn" aria-pressed={thanks.mine} disabled={thanks.mine} onClick={() => void thank()}>
                {api.t(thanks.mine ? 'comments.ui.thanked' : 'comments.ui.thanks', { n: thanks.n })}
              </button>
            ) : (
              thanks.n > 0 && <span className="small muted">{api.t('comments.ui.thanked', { n: thanks.n })}</span>
            )}
            {c.can_edit && (
              <button type="button" className="txt-btn" onClick={() => void startEdit()}>
                {api.t('comments.ui.edit')}
              </button>
            )}
            {c.can_delete && (
              <button type="button" className="txt-btn" onClick={() => void remove()}>
                {api.t('comments.ui.delete')}
              </button>
            )}
            {c.author?.id !== me.id && (
              <button type="button" className="txt-btn" onClick={() => onReport(c.id)}>
                {api.t('comments.ui.report')}
              </button>
            )}
          </div>
        )}
        {c.replies && c.replies.length > 0 && (
          <div className="cmt-replies">
            {c.replies.map((r) => (
              <Item key={r.id} api={api} c={r} onQuote={onQuote} onReport={onReport} reload={reload} />
            ))}
          </div>
        )}
      </div>
    </article>
  );
}

function CommentSection({ api, target }: { api: PluginApi; target: string }) {
  const [items, setItems] = useState<Comment[] | null>(null);
  const [total, setTotal] = useState(0);
  const [cursor, setCursor] = useState<string | null>(null);
  const [body, setBody] = useState('');
  const [markup, setMarkup] = useState<'markdown' | 'bbcode'>('markdown');
  const [preview, setPreview] = useState<string | null>(null);
  const [replyTo, setReplyTo] = useState<Comment | null>(null);
  const [quoteOf, setQuoteOf] = useState<Comment | null>(null);
  const [report, setReport] = useState<number | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    const r = await api.json<{ items: Comment[]; next_cursor: string | null; total: number }>(`/api/p/comments/v1/targets/${target}/comments`);
    setItems(r.items);
    setCursor(r.next_cursor);
    setTotal(r.total);
  }, [api, target]);
  useEffect(() => {
    void load().catch((e) => {
      setItems([]);
      setErr(errText(api, e));
    });
  }, [api, load]);
  async function more() {
    if (!cursor) return;
    const r = await api.json<{ items: Comment[]; next_cursor: string | null }>(`/api/p/comments/v1/targets/${target}/comments?cursor=${cursor}`);
    setItems([...(items ?? []), ...r.items]);
    setCursor(r.next_cursor);
  }
  async function post() {
    setBusy(true);
    setErr(null);
    try {
      await api.json(`/api/p/comments/v1/targets/${target}/comments`, {
        method: 'POST',
        json: { body, markup, reply_to: replyTo?.id ?? null, quote_of: quoteOf?.id ?? null },
        idempotent: true,
      });
      setBody('');
      setPreview(null);
      setReplyTo(null);
      setQuoteOf(null);
      await load();
    } catch (e) {
      setErr(errText(api, e));
    } finally {
      setBusy(false);
    }
  }
  async function doPreview() {
    const r = await api.json<{ html: string }>('/api/p/comments/v1/preview', { method: 'POST', json: { body, markup } });
    setPreview(r.html);
  }
  return (
    <section className="comments">
      {items === null ? (
        <SkeletonRows n={3} />
      ) : items.length === 0 ? (
        <Empty compact hint={api.user() ? api.t('comments.ui.empty_hint') : undefined}>
          {api.t('comments.ui.empty')}
        </Empty>
      ) : (
        <>
          <p className="small muted">{api.t('comments.ui.count', { n: total })}</p>
          {items.map((c) => (
            <Item key={c.id} api={api} c={c} onReply={(x) => setReplyTo(x)} onQuote={(x) => setQuoteOf(x)} onReport={setReport} reload={() => void load()} />
          ))}
        </>
      )}
      {cursor && <LoadMore onClick={() => void more()} label={api.t('comments.ui.more')} />}
      {api.user() && (
        <form
          className="cmt-compose"
          onSubmit={(e) => {
            e.preventDefault();
            void post();
          }}
        >
          {(replyTo || quoteOf) && (
            <div className="row small muted">
              {replyTo && api.t('comments.ui.replying', { floor: replyTo.floor })}
              {quoteOf && api.t('comments.ui.quoting', { floor: quoteOf.floor })}
              <button
                type="button"
                className="txt-btn"
                onClick={() => {
                  setReplyTo(null);
                  setQuoteOf(null);
                }}
              >
                {api.t('comments.ui.cancel')}
              </button>
            </div>
          )}
          {preview === null ? (
            <Textarea rows={4} aria-label={api.t('comments.ui.placeholder')} placeholder={api.t('comments.ui.placeholder')} value={body} onChange={(e) => setBody(e.target.value)} />
          ) : (
            <UserContent html={preview} className="cmt-preview" />
          )}
          <div className="form-acts">
            <Segmented
              value={markup}
              onChange={(v) => setMarkup(v)}
              options={[
                { value: 'markdown', label: 'Markdown' },
                { value: 'bbcode', label: 'BBCode' },
              ]}
            />
            <Button size="sm" variant="ghost" disabled={!body.trim()} onClick={() => (preview === null ? void doPreview() : setPreview(null))}>
              {api.t(preview === null ? 'comments.ui.preview' : 'comments.ui.write')}
            </Button>
            <Button type="submit" variant="primary" className="push-right" disabled={busy || !body.trim()}>
              {api.t('comments.ui.post')}
            </Button>
          </div>
          {err && <Note tone="bad">{err}</Note>}
        </form>
      )}
      <ReportSheet api={api} id={report} onClose={() => setReport(null)} />
    </section>
  );
}

function TorrentTab({ api, ctx }: SlotProps<{ torrent: { id: number } }>) {
  return <CommentSection api={api} target={`torrent:${ctx.torrent.id}`} />;
}

function GroupTab({ api, ctx }: SlotProps<{ group: { id: number } }>) {
  return <CommentSection api={api} target={`group:${ctx.group.id}`} />;
}

function RequestSection({ api, ctx }: SlotProps<{ request: { id: number } }>) {
  return <CommentSection api={api} target={`request:${ctx.request.id}`} />;
}

/** 个人页「评论」页签：该用户最近 50 条可见评论（只列出查看者可见的目标） */
function ProfileTab({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const [rows, setRows] = useState<{ id: number; target: string; floor: number; excerpt: string | null; created_at: number }[] | null>(null);
  useEffect(() => {
    void api
      .json<{ items: NonNullable<typeof rows> }>(`/api/p/comments/v1/users/${ctx.profile.id}/comments`)
      .then((r) => setRows(r.items))
      .catch(() => setRows([]));
  }, [api, ctx.profile.id]);
  if (!rows) return <SkeletonRows n={3} />;
  if (rows.length === 0) return <Empty compact>{api.t('comments.ui.empty')}</Empty>;
  const where = (target: string, id: number): [string, string] => {
    const [kind, tid] = target.split(':');
    if (kind === 'torrent') return [`/torrents/${tid}#comment-${id}`, api.t('comments.ui.on_torrent', { id: tid })];
    if (kind === 'group') return [`/groups/${tid}#comment-${id}`, api.t('comments.ui.on_group', { id: tid })];
    if (kind === 'request') return [`/p/requests/${tid}`, api.t('comments.ui.on_request', { id: tid })];
    return ['', target];
  };
  return (
    <div className="stack-s">
      {rows.map((r) => {
        const [to, label] = where(r.target, r.id);
        return (
          <div key={r.id} className="cmt-line">
            <div className="row small muted">
              {to ? (
                <PLink to={to} navigate={api.navigate} className="link">
                  {label}
                </PLink>
              ) : (
                label
              )}
              <span>#{r.floor}</span>
              <span>{rel(r.created_at)}</span>
            </div>
            <p className="small">{r.excerpt}</p>
          </div>
        );
      })}
    </div>
  );
}

export default {
  slots: {
    'user.profile.tabs': { component: ProfileTab, order: 40, title_term: 'comments.ui.tab' },
    'request.detail.sidebar': { component: RequestSection, order: 10 },
    'torrent.detail.tabs': { component: TorrentTab, order: 10, title_term: 'comments.ui.tab' },
    'group.detail.tabs': { component: GroupTab, order: 10, title_term: 'comments.ui.tab' },
  },
} satisfies PluginFrontend;
