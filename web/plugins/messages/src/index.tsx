// messages 前端：用户菜单的未读站内信数、收件箱 / 已发送 / 系统（/p/messages/）、写信（/p/messages/new?to=）、会话（/p/messages/<id>：
// 回复、举报某条消息、拉黑、删除会话）、群发公告（“公告”页签、首页未读公告横幅、后台 /admin/p/messages/ 发布与撤回）。
import { useCallback, useEffect, useState } from "react";
import { ApiError } from "@pt-studio/sdk";
import type { PluginApi, PluginFrontend, SlotProps } from "@pt-studio/slots";
import { registerMessages } from "@pt-studio/terms";
import {
  Badge,
  Button,
  Card,
  Empty,
  Field,
  Input,
  Table,
  Tabs,
  UserContent,
} from "@pt-studio/ui-kit";

registerMessages("zh-CN", {
  "messages.ui.menu": "站内信（{n}）",
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
  "messages.ui.menu": "Messages ({n})",
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
      .json<{ private: number; broadcasts?: number }>(
        "/api/p/messages/v1/unread",
      )
      .then((r) => setN(r.private + (r.broadcasts ?? 0)))
      .catch(() => undefined);
  }, [api]);
  return (
    <a href="/p/messages/" onClick={go(api, "/p/messages/")}>
      {api.t("messages.ui.menu", { n })}
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

interface Broadcast {
  id: number;
  subject: string;
  html: string;
  created_at: number;
  unread: boolean;
}

function BroadcastList({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<Broadcast[]>([]);
  const load = useCallback(
    () =>
      void api
        .json<{ items: Broadcast[] }>("/api/p/messages/v1/broadcasts")
        .then((r) => setRows(r.items)),
    [api],
  );
  useEffect(() => {
    load();
    // 打开公告页签即全部已读
    void api
      .json("/api/p/messages/v1/broadcasts/read", { json: {} })
      .catch(() => undefined);
  }, [api, load]);
  if (rows.length === 0) return <Empty>—</Empty>;
  return (
    <div className="pt-stack">
      {rows.map((b) => (
        <details key={b.id} open={b.unread} className="broadcast">
          <summary>
            <strong>{b.subject}</strong>{" "}
            {b.unread && <Badge tone="danger">new</Badge>}{" "}
            <span className="pt-muted small">
              {new Date(b.created_at).toLocaleString()}
            </span>
          </summary>
          <UserContent html={b.html} />
          <Button
            variant="ghost"
            onClick={() =>
              void api
                .fetch(`/api/p/messages/v1/broadcasts/${b.id}`, {
                  method: "DELETE",
                })
                .then(load)
            }
          >
            {api.t("messages.ui.hide")}
          </Button>
        </details>
      ))}
    </div>
  );
}

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
    <Card
      title={api.t("messages.ui.broadcasts")}
      actions={
        <Button
          onClick={() =>
            void api
              .json("/api/p/messages/v1/broadcasts/read", { json: {} })
              .then(() => setRows([]))
          }
        >
          {api.t("messages.ui.got_it")}
        </Button>
      }
    >
      {rows.map((b) => (
        <div key={b.id} className="broadcast" data-testid="broadcast">
          <strong>{b.subject}</strong>
          <UserContent html={b.html} />
        </div>
      ))}
    </Card>
  );
}

function Admin({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<
    {
      id: number;
      subject: string;
      min_class: number | null;
      created_at: number;
      expires_at: number | null;
      revoked: boolean;
      created_by: string | null;
      read: number;
    }[]
  >([]);
  const [f, setF] = useState({
    subject: "",
    body: "",
    min_class: "",
    expires_days: "",
  });
  const load = useCallback(
    () =>
      void api
        .json<{ items: typeof rows }>("/api/p/messages/v1/admin/broadcasts")
        .then((r) => setRows(r.items)),
    [api],
  );
  useEffect(load, [load]);
  const publish = async () => {
    try {
      await api.json("/api/p/messages/v1/admin/broadcasts", {
        json: {
          subject: f.subject,
          body: f.body,
          min_class: f.min_class ? Number(f.min_class) : null,
          expires_days: f.expires_days ? Number(f.expires_days) : null,
        },
      });
      api.toast(api.t("messages.ui.published"), "success");
      setF({ subject: "", body: "", min_class: "", expires_days: "" });
      load();
    } catch (e) {
      api.toast(errText(api, e), "error");
    }
  };
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t("messages.ui.admin")}</h1>
      <Card title={api.t("messages.ui.new_broadcast")}>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void publish();
          }}
        >
          <Field label={api.t("messages.ui.subject")}>
            <Input
              name="broadcast_subject"
              value={f.subject}
              maxLength={200}
              required
              onChange={(e) => setF({ ...f, subject: e.target.value })}
            />
          </Field>
          <Field label={api.t("messages.ui.body")}>
            <textarea
              className="pt-input"
              name="broadcast_body"
              rows={6}
              value={f.body}
              required
              onChange={(e) => setF({ ...f, body: e.target.value })}
            />
          </Field>
          <Field label={api.t("messages.ui.min_class")}>
            <Input
              type="number"
              min={1}
              value={f.min_class}
              onChange={(e) => setF({ ...f, min_class: e.target.value })}
            />
          </Field>
          <Field label={api.t("messages.ui.expires_days")}>
            <Input
              type="number"
              min={1}
              max={365}
              value={f.expires_days}
              onChange={(e) => setF({ ...f, expires_days: e.target.value })}
            />
          </Field>
          <Button type="submit" variant="primary">
            {api.t("messages.ui.publish")}
          </Button>
        </form>
      </Card>
      <Card title={api.t("messages.ui.broadcasts")}>
        <Table
          rows={rows}
          rowKey={(r) => r.id}
          empty="—"
          columns={[
            {
              key: "s",
              header: "",
              render: (r) => (
                <>
                  <strong>{r.subject}</strong>{" "}
                  {r.revoked && <Badge>{api.t("messages.ui.revoked")}</Badge>}
                  <div className="pt-muted small">
                    {r.created_by ?? "—"} ·{" "}
                    {new Date(r.created_at).toLocaleString()}
                    {r.min_class ? ` · ≥ ${r.min_class}` : ""}
                    {r.expires_at
                      ? ` · → ${new Date(r.expires_at).toLocaleDateString()}`
                      : ""}{" "}
                    · {api.t("messages.ui.read_by", { n: r.read })}
                  </div>
                </>
              ),
            },
            {
              key: "a",
              header: "",
              render: (r) =>
                r.revoked ? null : (
                  <Button
                    onClick={() =>
                      void api
                        .json(
                          `/api/p/messages/v1/admin/broadcasts/${r.id}/revoke`,
                          { json: {} },
                        )
                        .then(load)
                    }
                  >
                    {api.t("messages.ui.revoke")}
                  </Button>
                ),
            },
          ]}
        />
      </Card>
    </div>
  );
}

function Folders({ api }: { api: PluginApi }) {
  const [folder, setFolder] = useState("inbox");
  const [rows, setRows] = useState<Conv[]>([]);
  useEffect(() => {
    if (folder === "broadcasts") return;
    void api
      .json<{ items: Conv[] }>(
        `/api/p/messages/v1/conversations?folder=${folder}`,
      )
      .then((r) => setRows(r.items));
  }, [api, folder]);
  return (
    <Card
      title={api.t("messages.ui.title")}
      actions={
        <Button
          variant="primary"
          onClick={() => api.navigate("/p/messages/new")}
        >
          {api.t("messages.ui.compose")}
        </Button>
      }
    >
      <Tabs
        tabs={["inbox", "sent", "system", "broadcasts"].map((f) => ({
          id: f,
          label: api.t(`messages.ui.${f}`),
        }))}
        value={folder}
        onChange={setFolder}
      />
      {folder === "broadcasts" ? (
        <BroadcastList api={api} />
      ) : (
        <Table
          rows={rows}
          rowKey={(r) => r.id}
          empty={<Empty>—</Empty>}
          columns={[
            {
              key: "s",
              header: api.t("messages.ui.subject"),
              render: (r) => (
                <a
                  href={`/p/messages/${r.id}`}
                  onClick={go(api, `/p/messages/${r.id}`)}
                >
                  {r.subject}{" "}
                  {r.unread > 0 && <Badge tone="danger">{r.unread}</Badge>}
                </a>
              ),
            },
            {
              key: "p",
              header: api.t("messages.ui.with"),
              render: (r) =>
                r.kind === "system"
                  ? api.t("messages.ui.system_sender")
                  : r.participants.map((p) => p.username).join(", "),
            },
            {
              key: "t",
              header: "",
              render: (r) => new Date(r.last_message_at).toLocaleString(),
            },
          ]}
        />
      )}
    </Card>
  );
}

function Compose({ api }: { api: PluginApi }) {
  const [to, setTo] = useState(
    () => new URLSearchParams(window.location.search).get("to") ?? "",
  );
  const [subject, setSubject] = useState("");
  const [body, setBody] = useState("");
  const [err, setErr] = useState<string | null>(null);
  async function send() {
    setErr(null);
    try {
      const r = await api.json<{ conversation_id: number }>(
        "/api/p/messages/v1/conversations",
        {
          method: "POST",
          json: {
            recipients: to
              .split(",")
              .map((s) => s.trim())
              .filter(Boolean),
            subject,
            body,
            markup: "markdown",
          },
          idempotent: true,
        },
      );
      api.navigate(`/p/messages/${r.conversation_id}`);
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  return (
    <Card title={api.t("messages.ui.compose")}>
      <div className="pt-stack">
        <Field label={api.t("messages.ui.to")}>
          <Input value={to} onChange={(e) => setTo(e.target.value)} />
        </Field>
        <Field label={api.t("messages.ui.subject")}>
          <Input
            value={subject}
            maxLength={200}
            onChange={(e) => setSubject(e.target.value)}
          />
        </Field>
        <Field label={api.t("messages.ui.body")}>
          <textarea
            className="pt-input"
            rows={8}
            value={body}
            onChange={(e) => setBody(e.target.value)}
          />
        </Field>
        <Button
          variant="primary"
          disabled={!to.trim() || !subject.trim() || !body.trim()}
          onClick={() => void send()}
        >
          {api.t("messages.ui.send")}
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
      await api.json(`/api/p/messages/v1/messages/${id}/report`, {
        method: "POST",
        json: { reason_code: code, note: "", consent },
      });
      setDone(true);
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  if (done)
    return <span className="pt-muted">{api.t("messages.ui.reported")}</span>;
  if (!open)
    return (
      <Button variant="ghost" onClick={() => setOpen(true)}>
        {api.t("messages.ui.report")}
      </Button>
    );
  return (
    <span className="pt-row">
      <label className="pt-check">
        <input
          type="checkbox"
          checked={consent}
          onChange={(e) => setConsent(e.target.checked)}
        />{" "}
        {api.t("messages.ui.consent")}
      </label>
      {["spam", "abuse", "scam"].map((c) => (
        <Button
          key={c}
          variant="danger"
          disabled={!consent}
          onClick={() => void send(c)}
        >
          {api.t(`term:reports.reason.message.${c}`) ===
          `reports.reason.message.${c}`
            ? c
            : api.t(`term:reports.reason.message.${c}`)}
        </Button>
      ))}
      {err && <span className="pt-error">{err}</span>}
    </span>
  );
}

function Conversation({ api, id }: { api: PluginApi; id: number }) {
  const [c, setC] = useState<{
    subject: string;
    kind: string;
    participants: { id: number; username: string }[];
    messages: Msg[];
  } | null>(null);
  const [body, setBody] = useState("");
  const [err, setErr] = useState<string | null>(null);
  const [blocked, setBlocked] = useState(false);
  const me = api.user();
  const load = useCallback(
    async () => setC(await api.json(`/api/p/messages/v1/conversations/${id}`)),
    [api, id],
  );
  useEffect(() => {
    void load().catch((e) => setErr(errText(api, e)));
  }, [api, load]);
  async function reply() {
    setErr(null);
    try {
      await api.json(`/api/p/messages/v1/conversations/${id}/messages`, {
        method: "POST",
        json: { body, markup: "markdown" },
      });
      setBody("");
      await load();
    } catch (e) {
      setErr(errText(api, e));
    }
  }
  async function remove() {
    await api.fetch(`/api/p/messages/v1/conversations/${id}`, {
      method: "DELETE",
    });
    api.navigate("/p/messages/");
  }
  async function block() {
    const other = c?.participants.find((p) => p.id !== me?.id);
    if (!other) return;
    await api.fetch("/api/p/messages/v1/blocks", {
      method: "POST",
      json: { user: other.id },
    });
    setBlocked(true);
  }
  if (!c) return err ? <p className="pt-error">{err}</p> : null;
  return (
    <Card
      title={c.subject}
      actions={
        <span className="pt-row">
          {c.kind === "user" && (
            <Button
              variant="ghost"
              disabled={blocked}
              onClick={() => void block()}
            >
              {api.t(blocked ? "messages.ui.blocked" : "messages.ui.block")}
            </Button>
          )}
          <Button variant="ghost" onClick={() => void remove()}>
            {api.t("messages.ui.delete")}
          </Button>
        </span>
      }
    >
      {c.messages.map((m) => (
        <div key={m.id} className="pt-comment" data-testid="message">
          <div className="pt-row pt-muted">
            <strong>
              {m.sender
                ? (m.sender.username ?? `#${m.sender.id}`)
                : api.t("messages.ui.system_sender")}
            </strong>
            <span>{new Date(m.created_at).toLocaleString()}</span>
            {m.sender && m.sender.id !== me?.id && (
              <ReportMessage api={api} id={m.id} />
            )}
          </div>
          <UserContent html={m.html} />
        </div>
      ))}
      {c.kind === "user" && (
        <div className="pt-stack">
          <textarea
            className="pt-input"
            rows={4}
            aria-label={api.t("messages.ui.reply")}
            value={body}
            onChange={(e) => setBody(e.target.value)}
          />
          <Button
            variant="primary"
            disabled={!body.trim()}
            onClick={() => void reply()}
          >
            {api.t("messages.ui.reply")}
          </Button>
        </div>
      )}
      {err && <p className="pt-error">{err}</p>}
    </Card>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  const rest = path.replace(/^\/p\/messages\/?/, "");
  if (rest.startsWith("new")) return <Compose api={api} />;
  const id = Number(rest.split("/")[0]);
  if (id > 0) return <Conversation api={api} id={id} />;
  return <Folders api={api} />;
}

export default {
  slots: {
    "nav.user_menu": { component: MenuItem, order: 6 },
    "home.announcements": { component: Announcements, order: 5 },
  },
  routes: [
    { path: "/p/messages/*", component: Pages },
    { path: "/admin/p/messages/*", component: Admin },
  ],
} satisfies PluginFrontend;
