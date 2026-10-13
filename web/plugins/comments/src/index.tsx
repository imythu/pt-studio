// comments 前端：种子与作品页的评论区（tabs 插槽）——楼层、回复、引用、感谢、编辑、删除、举报、Markdown / BBCode 与预览。
import { useCallback, useEffect, useState } from 'react';
import { ApiError } from '@pt-studio/sdk';
import type { PluginApi, PluginFrontend, SlotProps } from '@pt-studio/slots';
import { registerMessages } from '@pt-studio/terms';
import { Badge, Button, Card, Empty, UserContent } from '@pt-studio/ui-kit';

registerMessages('zh-CN', {
  'comments.ui.tab': '评论',
  'comments.ui.count': '{n} 条评论',
  'comments.ui.placeholder': '写下你的评论…',
  'comments.ui.post': '发表',
  'comments.ui.preview': '预览',
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
  'comments.ui.reported': '已举报，感谢反馈',
  'comments.ui.reason': '举报理由',
});
registerMessages('en', {
  'comments.ui.tab': 'Comments',
  'comments.ui.count': '{n} comments',
  'comments.ui.placeholder': 'Write a comment…',
  'comments.ui.post': 'Post',
  'comments.ui.preview': 'Preview',
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
  'comments.ui.reported': 'Reported, thanks',
  'comments.ui.reason': 'Reason',
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

function ReportButton({ api, id }: { api: PluginApi; id: number }) {
  const [reasons, setReasons] = useState<string[] | null>(null);
  const [done, setDone] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  async function open() {
    try {
      const r = await api.json<{ items: { code: string }[] }>('/api/p/reports/v1/reasons?kind=comment');
      setReasons(r.items.map((x) => x.code));
    } catch {
      setReasons([]);
    }
  }
  async function send(code: string) {
    try {
      await api.json('/api/p/reports/v1/reports', { method: 'POST', json: { target: `comment:${id}`, reason_code: code, note: '' } });
      setDone(true);
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  if (done) return <span className="pt-muted">{api.t('comments.ui.reported')}</span>;
  if (reasons === null)
    return (
      <Button variant="ghost" onClick={() => void open()}>
        {api.t('comments.ui.report')}
      </Button>
    );
  return (
    <span className="pt-row">
      <select className="pt-input" aria-label={api.t('comments.ui.reason')} defaultValue="" onChange={(e) => e.target.value && void send(e.target.value)}>
        <option value="">{api.t('comments.ui.reason')}</option>
        {reasons
          .filter((c) => c !== 'other')
          .map((c) => (
            <option key={c} value={c}>
              {api.t(`term:reports.reason.comment.${c}`) === `reports.reason.comment.${c}` ? c : api.t(`term:reports.reason.comment.${c}`)}
            </option>
          ))}
      </select>
      {err && <span className="pt-error">{err}</span>}
    </span>
  );
}

function Item({ api, c, onReply, onQuote, reload }: { api: PluginApi; c: Comment; onReply?: (c: Comment) => void; onQuote: (c: Comment) => void; reload(): void }) {
  const [editing, setEditing] = useState<string | null>(null);
  const [thanks, setThanks] = useState({ n: c.thanks, mine: c.thanked_by_me });
  const me = api.user();
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
  async function thank() {
    await api.fetch(`/api/p/comments/v1/comments/${c.id}/thanks`, { method: 'POST', json: {} });
    setThanks({ n: thanks.n + 1, mine: true });
  }
  return (
    <div className="pt-comment" data-testid="comment" id={`comment-${c.id}`}>
      <div className="pt-row pt-muted">
        <Badge>#{c.floor}</Badge>
        {c.author ? (
          <a
            href={`/users/${c.author.id}`}
            onClick={(e) => {
              e.preventDefault();
              api.navigate(`/users/${c.author?.id}`);
            }}
          >
            {c.author.username ?? `#${c.author.id}`}
          </a>
        ) : null}
        <span>{new Date(c.created_at).toLocaleString()}</span>
        {c.edited_at && <span>· {api.t('comments.ui.edited')}</span>}
      </div>
      {c.deleted && !c.html ? (
        <p className="pt-muted">{api.t('comments.ui.deleted')}</p>
      ) : editing !== null ? (
        <div className="pt-stack">
          <textarea className="pt-input" rows={4} value={editing} onChange={(e) => setEditing(e.target.value)} />
          <span className="pt-row">
            <Button variant="primary" onClick={() => void save()}>
              {api.t('comments.ui.save')}
            </Button>
            <Button variant="ghost" onClick={() => setEditing(null)}>
              {api.t('comments.ui.cancel')}
            </Button>
          </span>
        </div>
      ) : (
        <UserContent html={c.html ?? ''} />
      )}
      {!c.deleted && me && editing === null && (
        <div className="pt-row">
          {onReply && (
            <Button variant="ghost" onClick={() => onReply(c)}>
              {api.t('comments.ui.reply')}
            </Button>
          )}
          <Button variant="ghost" onClick={() => onQuote(c)}>
            {api.t('comments.ui.quote')}
          </Button>
          {c.author?.id !== me.id && (
            <Button variant="ghost" disabled={thanks.mine} onClick={() => void thank()}>
              {api.t(thanks.mine ? 'comments.ui.thanked' : 'comments.ui.thanks', { n: thanks.n })}
            </Button>
          )}
          {c.can_edit && (
            <Button variant="ghost" onClick={() => void startEdit()}>
              {api.t('comments.ui.edit')}
            </Button>
          )}
          {c.can_delete && (
            <Button variant="ghost" onClick={() => void remove()}>
              {api.t('comments.ui.delete')}
            </Button>
          )}
          {c.author?.id !== me.id && <ReportButton api={api} id={c.id} />}
        </div>
      )}
      {c.replies && c.replies.length > 0 && (
        <div className="pt-replies">
          {c.replies.map((r) => (
            <Item key={r.id} api={api} c={r} onQuote={onQuote} reload={reload} />
          ))}
        </div>
      )}
    </div>
  );
}

function CommentSection({ api, target }: { api: PluginApi; target: string }) {
  const [items, setItems] = useState<Comment[]>([]);
  const [total, setTotal] = useState(0);
  const [cursor, setCursor] = useState<string | null>(null);
  const [body, setBody] = useState('');
  const [markup, setMarkup] = useState<'markdown' | 'bbcode'>('markdown');
  const [preview, setPreview] = useState<string | null>(null);
  const [replyTo, setReplyTo] = useState<Comment | null>(null);
  const [quoteOf, setQuoteOf] = useState<Comment | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    const r = await api.json<{ items: Comment[]; next_cursor: string | null; total: number }>(`/api/p/comments/v1/targets/${target}/comments`);
    setItems(r.items);
    setCursor(r.next_cursor);
    setTotal(r.total);
  }, [api, target]);
  useEffect(() => {
    void load().catch((e) => setErr(errText(api, e)));
  }, [api, load]);
  async function more() {
    if (!cursor) return;
    const r = await api.json<{ items: Comment[]; next_cursor: string | null }>(`/api/p/comments/v1/targets/${target}/comments?cursor=${cursor}`);
    setItems([...items, ...r.items]);
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
    <Card title={api.t('comments.ui.count', { n: total })}>
      {items.length === 0 && <Empty>—</Empty>}
      {items.map((c) => (
        <Item key={c.id} api={api} c={c} onReply={(x) => setReplyTo(x)} onQuote={(x) => setQuoteOf(x)} reload={() => void load()} />
      ))}
      {cursor && (
        <Button variant="ghost" onClick={() => void more()}>
          {api.t('comments.ui.more')}
        </Button>
      )}
      {api.user() && (
        <div className="pt-stack">
          {replyTo && (
            <span className="pt-row pt-muted">
              {api.t('comments.ui.replying', { floor: replyTo.floor })}
              <Button variant="ghost" onClick={() => setReplyTo(null)}>
                ✕
              </Button>
            </span>
          )}
          {quoteOf && (
            <span className="pt-row pt-muted">
              {api.t('comments.ui.quoting', { floor: quoteOf.floor })}
              <Button variant="ghost" onClick={() => setQuoteOf(null)}>
                ✕
              </Button>
            </span>
          )}
          <textarea
            className="pt-input"
            rows={4}
            aria-label={api.t('comments.ui.placeholder')}
            placeholder={api.t('comments.ui.placeholder')}
            value={body}
            onChange={(e) => setBody(e.target.value)}
          />
          <span className="pt-row">
            <select className="pt-input" value={markup} onChange={(e) => setMarkup(e.target.value as 'markdown' | 'bbcode')}>
              <option value="markdown">Markdown</option>
              <option value="bbcode">BBCode</option>
            </select>
            <Button variant="ghost" disabled={!body.trim()} onClick={() => void doPreview()}>
              {api.t('comments.ui.preview')}
            </Button>
            <Button variant="primary" disabled={busy || !body.trim()} onClick={() => void post()}>
              {api.t('comments.ui.post')}
            </Button>
          </span>
          {err && <p className="pt-error">{err}</p>}
          {preview !== null && <UserContent html={preview} className="pt-preview" />}
        </div>
      )}
    </Card>
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

export default {
  slots: {
    'request.detail.sidebar': { component: RequestSection, order: 10 },
    'torrent.detail.tabs': { component: TorrentTab, order: 10, title_term: 'comments.ui.tab' },
    'group.detail.tabs': { component: GroupTab, order: 10, title_term: 'comments.ui.tab' },
  },
} satisfies PluginFrontend;
