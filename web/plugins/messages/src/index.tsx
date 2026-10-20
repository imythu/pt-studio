// messages 前端：导航里的未读站内信数；站内信（/p/messages/ 与 /p/messages/<id>）为主从两级（UI-WEB P2）：
// 左侧文件夹与会话列表，右侧会话（回复、举报某条消息、拉黑、删除）；写信（/p/messages/new?to=）；
// 群发公告（「公告」文件夹、首页未读公告、后台 /admin/p/messages/ 发布与撤回）；设置页「站内信」分区（接收范围、黑名单）。
import { useCallback, useEffect, useState } from "react";
import { ApiError, renderPreview } from "@pt-studio/sdk";
import type { PluginApi, PluginFrontend, SlotProps } from "@pt-studio/slots";
import { registerMessages } from "@pt-studio/terms";
import {
  Avatar,
  Button,
  Checkbox,
  Empty,
  Field,
  Input,
  MarkdownEditor,
  MasterDetail,
  NavItem,
  Note,
  PageHead,
  Panel,
  PLink,
  Segmented,
  Select,
  Sheet,
  SkeletonRows,
  Status,
  Table,
  Tabs,
  UserContent,
} from "@pt-studio/ui-kit";

registerMessages("zh-CN", {
  "messages.ui.prefs_tab": "站内信",
  "messages.ui.accept": "谁可以给我发站内信",
  "messages.ui.accept.all": "所有用户",
  "messages.ui.accept.staff_only": "仅管理组",
  "messages.ui.blocklist": "黑名单",
  "messages.ui.blocklist_hint": "被拉黑的用户不能再给你发站内信。",
  "messages.ui.block_user": "拉黑用户（用户名）",
  "messages.ui.block_add": "拉黑",
  "messages.ui.unblock": "解除",
  "messages.ui.saved": "已保存",
  "messages.ui.menu": "站内信",
  "messages.ui.pick": "选择左侧的会话查看",
  "messages.ui.empty": "这里还没有消息",
  "messages.ui.back": "收件箱",
  "messages.ui.reply_to": "回复 {name}…",
  "messages.ui.unblock_user": "解除拉黑",
  "messages.ui.report_reason": "举报理由",
  "messages.ui.compose_hint": "收件人填用户名，多个用逗号分隔。",
  "messages.ui.admin_lede": "公告会显示在所有符合等级要求的用户首页，并出现在站内信的「公告」里。",
  "messages.ui.title": "站内信",
  "messages.ui.inbox": "收件箱",
  "messages.ui.sent": "已发送",
  "messages.ui.system": "系统消息",
  "messages.ui.compose": "写信",
  "messages.ui.to": "收件人（用户名，逗号分隔）",
  "messages.ui.subject": "标题",
  "messages.ui.body": "正文",
  "messages.ui.send": "发送",
  "messages.ui.reply": "回复",
  "messages.ui.delete": "删除会话",
  "messages.ui.block": "拉黑对方",
  "messages.ui.blocked": "已拉黑",
  "messages.ui.report": "举报",
  "messages.ui.consent": "我同意把这条消息提交给管理员",
  "messages.ui.reported": "已举报",
  "messages.ui.with": "与",
  "messages.ui.system_sender": "系统",
  "messages.ui.broadcasts": "公告",
  "messages.ui.hide": "不再显示",
  "messages.ui.got_it": "知道了",
  "messages.ui.admin": "群发公告",
  "messages.ui.new_broadcast": "发布公告",
  "messages.ui.min_class": "最低等级 ID（留空 = 所有人）",
  "messages.ui.expires_days": "有效天数（留空 = 长期）",
  "messages.ui.publish": "发布",
  "messages.ui.published": "公告已发布",
  "messages.ui.revoke": "撤回",
  "messages.ui.revoked": "已撤回",
  "messages.ui.read_by": "{n} 人已读",
});
registerMessages("en", {
  "messages.ui.prefs_tab": "Messages",
  "messages.ui.accept": "Who can message me",
  "messages.ui.accept.all": "Everyone",
  "messages.ui.accept.staff_only": "Staff only",
  "messages.ui.blocklist": "Blocked users",
  "messages.ui.blocklist_hint": "Blocked users can no longer message you.",
  "messages.ui.block_user": "Block a user (username)",
  "messages.ui.block_add": "Block",
  "messages.ui.unblock": "Unblock",
  "messages.ui.saved": "Saved",
  "messages.ui.menu": "Messages",
  "messages.ui.pick": "Pick a conversation on the left",
  "messages.ui.empty": "No messages here yet",
  "messages.ui.back": "Inbox",
  "messages.ui.reply_to": "Reply to {name}…",
  "messages.ui.unblock_user": "Unblock",
  "messages.ui.report_reason": "Reason",
  "messages.ui.compose_hint": "Enter usernames, separated by commas.",
  "messages.ui.admin_lede": "Announcements appear on the home page of every user who meets the class requirement, and under Messages → Announcements.",
  "messages.ui.title": "Messages",
  "messages.ui.inbox": "Inbox",
  "messages.ui.sent": "Sent",
  "messages.ui.system": "System",
  "messages.ui.compose": "New message",
  "messages.ui.to": "To (usernames, comma separated)",
  "messages.ui.subject": "Subject",
  "messages.ui.body": "Message",
  "messages.ui.send": "Send",
  "messages.ui.reply": "Reply",
  "messages.ui.delete": "Delete conversation",
  "messages.ui.block": "Block sender",
  "messages.ui.blocked": "Blocked",
  "messages.ui.report": "Report",
  "messages.ui.consent": "I agree to share this message with staff",
  "messages.ui.reported": "Reported",
  "messages.ui.with": "With",
  "messages.ui.system_sender": "System",
  "messages.ui.broadcasts": "Announcements",
  "messages.ui.hide": "Hide",
  "messages.ui.got_it": "Got it",
  "messages.ui.admin": "Broadcasts",
  "messages.ui.new_broadcast": "New broadcast",
  "messages.ui.min_class": "Minimum class ID (empty = everyone)",
  "messages.ui.expires_days": "Valid for days (empty = no expiry)",
  "messages.ui.publish": "Publish",
  "messages.ui.published": "Broadcast published",
  "messages.ui.revoke": "Revoke",
  "messages.ui.revoked": "revoked",
  "messages.ui.read_by": "read by {n}",
});

function errText(api: PluginApi, e: unknown): string {
  if (e instanceof ApiError) {
    const s = api.t(e.code);
    return s === e.code ? e.message : s;
  }
  return String(e);
}

function rel(ms: number): string {
  const d = new Date(ms);
  const now = new Date();
  if (d.toDateString() === now.toDateString()) return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return d.toLocaleDateString([], { month: "2-digit", day: "2-digit" });
}

function MenuItem({ api }: SlotProps) {
  const [n, setN] = useState(0);
  useEffect(() => {
    // 与宿主外壳共用同一查询缓存（侧栏、底栏同时挂载时只发一次请求）
    void api.queryClient
      .fetchQuery({
        queryKey: ["unread", "messages"],
        queryFn: () => api.json<{ private: number; broadcasts?: number }>("/api/p/messages/v1/unread"),
        staleTime: 30_000,
      })
      .then((r) => setN(r.private + (r.broadcasts ?? 0)))
      .catch(() => undefined);
  }, [api]);
  return (
    <NavItem to="/p/messages/" navigate={api.navigate} count={n} alert>
      {api.t("messages.ui.menu")}
    </NavItem>
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

interface Broadcast {
  id: number;
  subject: string;
  html: string;
  created_at: number;
  unread: boolean;
}

/** 首页：未读公告 */
function Announcements({ api }: SlotProps) {
  const [rows, setRows] = useState<Broadcast[]>([]);
  useEffect(() => {
    if (!api.user()) return;
    void api
      .json<{ items: Broadcast[] }>("/api/p/messages/v1/broadcasts")
      .then((r) => setRows(r.items.filter((b) => b.unread).slice(0, 3)))
      .catch(() => undefined);
  }, [api]);
  if (rows.length === 0) return null;
  return (
    <Panel
      title={api.t("messages.ui.broadcasts")}
      flush
      actions={
        <Button
          size="sm"
          onClick={() =>
            void api.json("/api/p/messages/v1/broadcasts/read", { json: {} }).then(() => setRows([]))
          }
        >
          {api.t("messages.ui.got_it")}
        </Button>
      }
    >
      <div className="announce">
        {rows.map((b) => (
          <details key={b.id} className="announce-row" data-testid="broadcast">
            <summary>
              <span className="announce-k">{api.t("messages.ui.broadcasts")}</span>
              <b>{b.subject}</b>
              <time>{new Date(b.created_at).toLocaleDateString()}</time>
            </summary>
            <UserContent html={b.html} />
          </details>
        ))}
      </div>
    </Panel>
  );
}

const FOLDERS = ["inbox", "sent", "system", "broadcasts"] as const;
type Folder = (typeof FOLDERS)[number];

function ConvList({ api, folder, current }: { api: PluginApi; folder: Folder; current: number }) {
  const [rows, setRows] = useState<Conv[] | null>(null);
  useEffect(() => {
    setRows(null);
    void api
      .json<{ items: Conv[] }>(`/api/p/messages/v1/conversations?folder=${folder}`)
      .then((r) => setRows(r.items))
      .catch(() => setRows([]));
  }, [api, folder]);
  if (rows === null) return <SkeletonRows n={5} />;
  if (rows.length === 0) return <Empty compact>{api.t("messages.ui.empty")}</Empty>;
  return (
    <div className="convs">
      {rows.map((r) => {
        const who = r.kind === "system" ? api.t("messages.ui.system_sender") : r.participants.map((p) => p.username).join(", ");
        return (
          <PLink key={r.id} to={`/p/messages/${r.id}?f=${folder}`} navigate={api.navigate} className={current === r.id ? "conv on" : "conv"}>
            <Avatar name={who} size={36} fallbackSrc={r.kind === "system" ? undefined : null} />
            <b>{who}</b>
            <span className="tiny muted" style={{ gridColumn: 3, gridRow: 1 }}>
              {rel(r.last_message_at)}
            </span>
            <span className="tiny muted">{r.subject}</span>
            {r.unread > 0 && (
              <span className="unread-n" style={{ gridColumn: 3, gridRow: 2 }}>
                {r.unread}
              </span>
            )}
          </PLink>
        );
      })}
    </div>
  );
}

function Broadcasts({ api, pick, current }: { api: PluginApi; pick(b: Broadcast): void; current: number }) {
  const [rows, setRows] = useState<Broadcast[] | null>(null);
  useEffect(() => {
    void api.json<{ items: Broadcast[] }>("/api/p/messages/v1/broadcasts").then((r) => setRows(r.items));
    // 打开公告文件夹即全部已读
    void api.json("/api/p/messages/v1/broadcasts/read", { json: {} }).catch(() => undefined);
  }, [api]);
  if (rows === null) return <SkeletonRows n={4} />;
  if (rows.length === 0) return <Empty compact>{api.t("messages.ui.empty")}</Empty>;
  return (
    <div className="convs">
      {rows.map((b) => (
        <button key={b.id} type="button" className="conv" aria-current={current === b.id ? "true" : undefined} onClick={() => pick(b)}>
          <Avatar name="!" size={36} fallbackSrc={null} />
          <b>{b.subject}</b>
          <span className="tiny muted" style={{ gridColumn: 3, gridRow: 1 }}>
            {rel(b.created_at)}
          </span>
          <span className="tiny muted">{api.t("messages.ui.broadcasts")}</span>
          {b.unread && <Status tone="warn">new</Status>}
        </button>
      ))}
    </div>
  );
}

interface Msg {
  id: number;
  sender: { id: number; username: string | null } | null;
  html: string;
  created_at: number;
}

function ReportMessage({ api, id, onClose }: { api: PluginApi; id: number | null; onClose(): void }) {
  const [consent, setConsent] = useState(false);
  const [code, setCode] = useState("spam");
  const label = (c: string) => {
    const k = `reports.reason.message.${c}`;
    const v = api.t(`term:${k}`);
    return v === k ? c : v;
  };
  async function send() {
    if (id === null) return;
    try {
      await api.json(`/api/p/messages/v1/messages/${id}/report`, { method: "POST", json: { reason_code: code, note: "", consent } });
      api.toast(api.t("messages.ui.reported"), "success");
      onClose();
    } catch (e) {
      api.toast(errText(api, e), "error");
    }
  }
  return (
    <Sheet
      open={id !== null}
      onClose={onClose}
      title={api.t("messages.ui.report")}
      footer={
        <>
          <Button onClick={onClose}>{api.t("ui.common.cancel")}</Button>
          <Button variant="danger" disabled={!consent} onClick={() => void send()}>
            {api.t("messages.ui.report")}
          </Button>
        </>
      }
    >
      <Field label={api.t("messages.ui.report_reason")}>
        <Select value={code} onChange={(e) => setCode(e.target.value)} options={["spam", "abuse", "scam"].map((c) => ({ value: c, label: label(c) }))} />
      </Field>
      <Checkbox label={api.t("messages.ui.consent")} checked={consent} onChange={setConsent} />
    </Sheet>
  );
}

function Conversation({ api, id }: { api: PluginApi; id: number }) {
  const [c, setC] = useState<{ subject: string; kind: string; participants: { id: number; username: string }[]; messages: Msg[] } | null>(null);
  const [body, setBody] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [blocked, setBlocked] = useState(false);
  const [report, setReport] = useState<number | null>(null);
  const me = api.user();
  const load = useCallback(async () => setC(await api.json(`/api/p/messages/v1/conversations/${id}`)), [api, id]);
  useEffect(() => {
    setC(null);
    setErr(null);
    void load()
      .then(() => api.queryClient.invalidateQueries({ queryKey: ["unread", "messages"] }))
      .catch((e) => setErr(errText(api, e)));
  }, [api, load]);
  async function reply() {
    setErr(null);
    try {
      await api.json(`/api/p/messages/v1/conversations/${id}/messages`, { method: "POST", json: { body, markup: "markdown" } });
      setBody("");
      await load();
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  async function remove() {
    await api.fetch(`/api/p/messages/v1/conversations/${id}`, { method: "DELETE" });
    api.navigate("/p/messages/");
  }
  async function block() {
    const other = c?.participants.find((p) => p.id !== me?.id);
    if (!other) return;
    if (blocked) {
      await api.fetch(`/api/p/messages/v1/blocks/${other.id}`, { method: "DELETE" });
      setBlocked(false);
      return;
    }
    await api.fetch("/api/p/messages/v1/blocks", { method: "POST", json: { user: other.id } });
    setBlocked(true);
    api.toast(api.t("messages.ui.blocked"), "success");
  }
  if (!c) return err ? <div className="pad"><Note tone="bad">{err}</Note></div> : <SkeletonRows n={4} />;
  const other = c.participants.find((p) => p.id !== me?.id);
  return (
    <>
      <header className="panel-h conv-h">
        <h2>{c.subject}</h2>
        {other && c.kind === "user" && (
          <span className="sub">
            {api.t("messages.ui.with")}{" "}
            <PLink to={`/users/${other.id}`} navigate={api.navigate} className="u">
              {other.username}
            </PLink>
          </span>
        )}
        <div className="acts">
          {c.kind === "user" && (
            <Button size="xs" variant="ghost" onClick={() => void block()}>
              {api.t(blocked ? "messages.ui.unblock_user" : "messages.ui.block")}
            </Button>
          )}
          <Button size="xs" variant="ghost" onClick={() => void remove()}>
            {api.t("messages.ui.delete")}
          </Button>
        </div>
      </header>
      <div className="thread">
        {c.messages.map((m) => {
          const mine = m.sender?.id === me?.id;
          const name = m.sender ? (m.sender.username ?? `#${m.sender.id}`) : api.t("messages.ui.system_sender");
          return (
            <div key={m.id} className={mine ? "bub me" : "bub"} data-testid="message">
              <Avatar name={name} size={36} />
              <div>
                <div className="bub-h">
                  {!mine && <b>{name}</b>}
                  <span>{new Date(m.created_at).toLocaleString()}</span>
                  {m.sender && !mine && (
                    <button type="button" className="txt-btn tiny" onClick={() => setReport(m.id)}>
                      {api.t("messages.ui.report")}
                    </button>
                  )}
                </div>
                <div className="bub-b">
                  <UserContent html={m.html} />
                </div>
              </div>
            </div>
          );
        })}
      </div>
      {c.kind === "user" && (
        <form
          className="composer"
          onSubmit={(e) => {
            e.preventDefault();
            void reply();
          }}
        >
          <MarkdownEditor name="message_reply" rows={3} ariaLabel={api.t("messages.ui.reply")} placeholder={api.t("messages.ui.reply_to", { name: other?.username ?? "" })} value={body} onChange={setBody} preview={renderPreview} />
          <div className="form-acts">
            {err && <span className="pt-error">{err}</span>}
            <Button type="submit" variant="primary" disabled={!body.trim()} className="push-right">
              {api.t("messages.ui.reply")}
            </Button>
          </div>
        </form>
      )}
      <ReportMessage api={api} id={report} onClose={() => setReport(null)} />
    </>
  );
}

function Inbox({ api, id }: { api: PluginApi; id: number }) {
  const q = new URLSearchParams(window.location.search);
  const [folder, setFolder] = useState<Folder>(() => (FOLDERS as readonly string[]).includes(q.get("f") ?? "") ? (q.get("f") as Folder) : "inbox");
  const [bc, setBc] = useState<Broadcast | null>(null);
  const showDetail = id > 0 || bc !== null;
  return (
    <div className="stack">
      <PageHead
        title={api.t("messages.ui.title")}
        actions={
          <Button variant="primary" size="sm" onClick={() => api.navigate("/p/messages/new")}>
            {api.t("messages.ui.compose")}
          </Button>
        }
      />
      <MasterDetail
        joined
        listWidth={330}
        showDetail={showDetail}
        backLabel={api.t("messages.ui.back")}
        onBack={() => {
          setBc(null);
          if (id > 0) api.navigate(`/p/messages/?f=${folder}`);
        }}
        list={
          <>
            <Tabs
              variant="flush"
              tabs={FOLDERS.map((f) => ({ id: f, label: api.t(`messages.ui.${f}`) }))}
              value={folder}
              onChange={(f) => {
                setFolder(f as Folder);
                setBc(null);
              }}
            />
            {folder === "broadcasts" ? <Broadcasts api={api} current={bc?.id ?? 0} pick={setBc} /> : <ConvList api={api} folder={folder} current={id} />}
          </>
        }
        detail={
          bc ? (
            <>
              <header className="panel-h conv-h">
                <h2>{bc.subject}</h2>
                <span className="sub">{new Date(bc.created_at).toLocaleString()}</span>
                <div className="acts">
                  <Button
                    size="xs"
                    variant="ghost"
                    onClick={() =>
                      void api.fetch(`/api/p/messages/v1/broadcasts/${bc.id}`, { method: "DELETE" }).then(() => {
                        setBc(null);
                        setFolder("broadcasts");
                      })
                    }
                  >
                    {api.t("messages.ui.hide")}
                  </Button>
                </div>
              </header>
              <div className="pad">
                <UserContent html={bc.html} />
              </div>
            </>
          ) : id > 0 ? (
            <Conversation key={id} api={api} id={id} />
          ) : (
            <div className="md-empty">{api.t("messages.ui.pick")}</div>
          )
        }
      />
    </div>
  );
}

function Compose({ api }: { api: PluginApi }) {
  const [to, setTo] = useState(() => new URLSearchParams(window.location.search).get("to") ?? "");
  const [subject, setSubject] = useState("");
  const [body, setBody] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  async function send() {
    setErr(null);
    setBusy(true);
    try {
      const r = await api.json<{ conversation_id: number }>("/api/p/messages/v1/conversations", {
        method: "POST",
        json: { recipients: to.split(",").map((s) => s.trim()).filter(Boolean), subject, body, markup: "markdown" },
        idempotent: true,
      });
      api.navigate(`/p/messages/${r.conversation_id}`);
    } catch (e) {
      setErr(errText(api, e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <div className="stack">
      <PageHead
        crumb={
          <>
            <PLink to="/p/messages/" navigate={api.navigate}>
              {api.t("messages.ui.title")}
            </PLink>
            <span>{api.t("messages.ui.compose")}</span>
          </>
        }
        title={api.t("messages.ui.compose")}
      />
      <Panel primary>
        <form
          className="form narrow-form"
          onSubmit={(e) => {
            e.preventDefault();
            void send();
          }}
        >
          <Field label={api.t("messages.ui.to")} hint={api.t("messages.ui.compose_hint")}>
            <Input value={to} onChange={(e) => setTo(e.target.value)} required />
          </Field>
          <Field label={api.t("messages.ui.subject")}>
            <Input value={subject} maxLength={200} onChange={(e) => setSubject(e.target.value)} required />
          </Field>
          <Field label={api.t("messages.ui.body")}>
            <MarkdownEditor name="message_body" rows={10} value={body} onChange={setBody} preview={renderPreview} />
          </Field>
          {err && <Note tone="bad">{err}</Note>}
          <div className="form-acts">
            <Button type="submit" variant="primary" disabled={busy || !to.trim() || !subject.trim() || !body.trim()}>
              {api.t("messages.ui.send")}
            </Button>
          </div>
        </form>
      </Panel>
    </div>
  );
}

function Admin({ api }: { api: PluginApi }) {
  type Row = { id: number; subject: string; min_class: number | null; created_at: number; expires_at: number | null; revoked: boolean; created_by: string | null; read: number };
  const [rows, setRows] = useState<Row[] | null>(null);
  const [open, setOpen] = useState(false);
  const [f, setF] = useState({ subject: "", body: "", min_class: "", expires_days: "" });
  const load = useCallback(() => void api.json<{ items: Row[] }>("/api/p/messages/v1/admin/broadcasts").then((r) => setRows(r.items)), [api]);
  useEffect(load, [load]);
  const publish = async () => {
    try {
      await api.json("/api/p/messages/v1/admin/broadcasts", {
        json: { subject: f.subject, body: f.body, min_class: f.min_class ? Number(f.min_class) : null, expires_days: f.expires_days ? Number(f.expires_days) : null },
      });
      api.toast(api.t("messages.ui.published"), "success");
      setF({ subject: "", body: "", min_class: "", expires_days: "" });
      setOpen(false);
      load();
    } catch (e) {
      api.toast(errText(api, e), "error");
    }
  };
  return (
    <div className="stack">
      <PageHead
        title={api.t("messages.ui.admin")}
        lede={api.t("messages.ui.admin_lede")}
        actions={
          <Button variant="primary" size="sm" onClick={() => setOpen(true)}>
            {api.t("messages.ui.new_broadcast")}
          </Button>
        }
      />
      <Panel flush primary>
        {rows === null ? (
          <SkeletonRows n={4} />
        ) : (
          <Table
            rows={rows}
            rowKey={(r) => r.id}
            empty={api.t("messages.ui.empty")}
            minWidth={640}
            columns={[
              { key: "s", header: api.t("messages.ui.subject"), render: (r) => <b>{r.subject}</b> },
              { key: "b", header: api.t("messages.ui.min_class"), render: (r) => <span className="small">{r.min_class ? `≥ ${r.min_class}` : "—"}</span> },
              { key: "c", header: "", render: (r) => <span className="small muted nowrap">{r.created_by ?? "—"} · {new Date(r.created_at).toLocaleDateString()}{r.expires_at ? ` → ${new Date(r.expires_at).toLocaleDateString()}` : ""}</span> },
              { key: "r", header: "", right: true, render: (r) => <span className="small nowrap">{api.t("messages.ui.read_by", { n: r.read })}</span> },
              {
                key: "a",
                header: "",
                render: (r) =>
                  r.revoked ? (
                    <Status tone="mute">{api.t("messages.ui.revoked")}</Status>
                  ) : (
                    <div className="acts">
                      <Button size="xs" onClick={() => void api.json(`/api/p/messages/v1/admin/broadcasts/${r.id}/revoke`, { json: {} }).then(load)}>
                        {api.t("messages.ui.revoke")}
                      </Button>
                    </div>
                  ),
              },
            ]}
          />
        )}
      </Panel>
      <Sheet
        open={open}
        onClose={() => setOpen(false)}
        title={api.t("messages.ui.new_broadcast")}
        width={620}
        footer={
          <Button variant="primary" type="submit" form="broadcast-form" disabled={!f.subject.trim() || !f.body.trim()}>
            {api.t("messages.ui.publish")}
          </Button>
        }
      >
        <form
          id="broadcast-form"
          className="form"
          onSubmit={(e) => {
            e.preventDefault();
            void publish();
          }}
        >
          <Field label={api.t("messages.ui.subject")}>
            <Input name="broadcast_subject" value={f.subject} maxLength={200} required onChange={(e) => setF({ ...f, subject: e.target.value })} />
          </Field>
          <Field label={api.t("messages.ui.body")}>
            <MarkdownEditor name="broadcast_body" rows={8} value={f.body} required onChange={(body) => setF({ ...f, body })} preview={renderPreview} />
          </Field>
          <div className="form-2">
            <Field label={api.t("messages.ui.min_class")}>
              <Input type="number" min={1} value={f.min_class} onChange={(e) => setF({ ...f, min_class: e.target.value })} />
            </Field>
            <Field label={api.t("messages.ui.expires_days")}>
              <Input type="number" min={1} max={365} value={f.expires_days} onChange={(e) => setF({ ...f, expires_days: e.target.value })} />
            </Field>
          </div>
        </form>
      </Sheet>
    </div>
  );
}

/** 设置页「站内信」分区：接收范围与黑名单 */
function Prefs({ api }: SlotProps) {
  const [accept, setAccept] = useState<string | null>(null);
  const [blocks, setBlocks] = useState<{ id: number; username: string | null }[]>([]);
  const [name, setName] = useState("");
  const load = useCallback(async () => {
    const [s, b] = await Promise.all([
      api.json<{ accept: string }>("/api/p/messages/v1/settings"),
      api.json<{ items: { id: number; username: string | null }[] }>("/api/p/messages/v1/blocks"),
    ]);
    setAccept(s.accept);
    setBlocks(b.items);
  }, [api]);
  useEffect(() => {
    void load().catch(() => undefined);
  }, [load]);
  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      api.toast(api.t("messages.ui.saved"), "success");
      await load();
    } catch (e) {
      api.toast(errText(api, e), "error");
      await load().catch(() => undefined);
    }
  };
  if (accept === null) return <SkeletonRows n={3} />;
  return (
    <div className="stack">
      <div className="set-row">
        <div>
          <b>{api.t("messages.ui.accept")}</b>
        </div>
        <Segmented
          value={accept}
          label={api.t("messages.ui.accept")}
          onChange={(v) => {
            // 先切换选中状态，失败时 act 的重新加载会恢复服务端的值
            setAccept(v);
            void act(() => api.json("/api/p/messages/v1/settings", { method: "PUT", json: { accept: v } }));
          }}
          options={["all", "staff_only"].map((v) => ({ value: v, label: api.t(`messages.ui.accept.${v}`) }))}
        />
      </div>
      <div className="stack-s">
        <h3>{api.t("messages.ui.blocklist")}</h3>
        <p className="small muted">{api.t("messages.ui.blocklist_hint")}</p>
        {blocks.length > 0 && (
          <Table
            rows={blocks}
            rowKey={(r) => r.id}
            columns={[
              { key: "u", header: "", render: (r) => <PLink to={`/users/${r.id}`} navigate={api.navigate} className="u">{r.username ?? `#${r.id}`}</PLink> },
              {
                key: "x",
                header: "",
                render: (r) => (
                  <div className="acts">
                    <Button size="xs" variant="ghost" onClick={() => void act(() => api.json(`/api/p/messages/v1/blocks/${r.id}`, { method: "DELETE" }))}>
                      {api.t("messages.ui.unblock")}
                    </Button>
                  </div>
                ),
              },
            ]}
          />
        )}
        <form
          className="row"
          onSubmit={(e) => {
            e.preventDefault();
            void act(async () => {
              await api.json("/api/p/messages/v1/blocks", { json: { user: name.trim() } });
              setName("");
            });
          }}
        >
          <Input name="block_user" aria-label={api.t("messages.ui.block_user")} placeholder={api.t("messages.ui.block_user")} value={name} onChange={(e) => setName(e.target.value)} maxLength={64} style={{ maxWidth: 280 }} />
          <Button type="submit" size="sm" variant="danger" disabled={!name.trim()}>
            {api.t("messages.ui.block_add")}
          </Button>
        </form>
      </div>
    </div>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  const rest = path.replace(/^\/p\/messages\/?/, "");
  if (rest.startsWith("new") || rest.startsWith("compose")) return <Compose api={api} />;
  const id = Number(rest.split("/")[0]) || 0;
  return <Inbox api={api} id={id} />;
}

export default {
  slots: {
    "nav.user_menu": { component: MenuItem, order: 6 },
    "home.announcements": { component: Announcements, order: 5 },
    "user.settings.tabs": { component: Prefs, order: 30, title_term: "messages.ui.prefs_tab" },
  },
  routes: [
    { path: "/p/messages/*", component: Pages },
    { path: "/admin/p/messages/*", component: Admin },
  ],
} satisfies PluginFrontend;
