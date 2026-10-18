// tickets 前端：用户菜单“联系管理组”、我的工单与新建（/p/tickets/）、工单详情（/p/tickets/<id>：回复、关闭、重新打开；员工可写内部备注、认领）、
// 后台工单队列（/admin/p/tickets/）、仪表盘待处理数。
import { useEffect, useState, type ReactNode } from "react";
import { ApiError, renderPreview } from "@pt-studio/sdk";
import type { PluginApi, PluginFrontend, SlotProps } from "@pt-studio/slots";
import { registerMessages } from "@pt-studio/terms";
import {
  Badge,
  Button,
  Card,
  Field,
  Input,
  Table,
  Tabs,
  UserContent,
  MarkdownEditor,
} from "@pt-studio/ui-kit";

registerMessages("zh-CN", {
  "tickets.ui.menu": "联系管理组",
  "tickets.ui.title": "联系管理组",
  "tickets.ui.mine": "我的工单",
  "tickets.ui.new": "提交新工单",
  "tickets.ui.category": "类别",
  "tickets.ui.subject": "标题",
  "tickets.ui.body": "详细描述",
  "tickets.ui.submit": "提交",
  "tickets.ui.created": "工单已提交，管理组会尽快回复",
  "tickets.ui.reply": "回复",
  "tickets.ui.send": "发送",
  "tickets.ui.internal": "内部备注（用户看不到）",
  "tickets.ui.close": "关闭工单",
  "tickets.ui.reopen": "重新打开",
  "tickets.ui.claim": "认领",
  "tickets.ui.release": "取消认领",
  "tickets.ui.staff": "管理组",
  "tickets.ui.assignee": "处理人",
  "tickets.ui.queue": "工单",
  "tickets.ui.only_mine": "只看我认领的",
  "tickets.ui.none": "没有工单",
  "tickets.ui.waiting_staff": "等待回复",
  "tickets.ui.state.open": "待处理",
  "tickets.ui.state.pending_user": "等待用户",
  "tickets.ui.state.closed": "已关闭",
  "tickets.ui.cat.account": "账号",
  "tickets.ui.cat.torrent": "种子",
  "tickets.ui.cat.appeal": "申诉",
  "tickets.ui.cat.suggestion": "建议",
  "tickets.ui.cat.other": "其他",
  "tickets.too_many_open": "未关闭的工单太多，请先关闭已解决的工单",
  "tickets.closed": "工单已关闭",
  "tickets.reopen_expired": "工单关闭太久，请提交新工单",
});
registerMessages("en", {
  "tickets.ui.menu": "Contact staff",
  "tickets.ui.title": "Contact staff",
  "tickets.ui.mine": "My tickets",
  "tickets.ui.new": "New ticket",
  "tickets.ui.category": "Category",
  "tickets.ui.subject": "Subject",
  "tickets.ui.body": "Details",
  "tickets.ui.submit": "Submit",
  "tickets.ui.created": "Ticket submitted; staff will reply soon",
  "tickets.ui.reply": "Reply",
  "tickets.ui.send": "Send",
  "tickets.ui.internal": "Internal note (hidden from the user)",
  "tickets.ui.close": "Close ticket",
  "tickets.ui.reopen": "Reopen",
  "tickets.ui.claim": "Claim",
  "tickets.ui.release": "Unassign",
  "tickets.ui.staff": "Staff",
  "tickets.ui.assignee": "Assignee",
  "tickets.ui.queue": "Tickets",
  "tickets.ui.only_mine": "Only mine",
  "tickets.ui.none": "No tickets",
  "tickets.ui.waiting_staff": "Awaiting reply",
  "tickets.ui.state.open": "Open",
  "tickets.ui.state.pending_user": "Waiting for user",
  "tickets.ui.state.closed": "Closed",
  "tickets.ui.cat.account": "Account",
  "tickets.ui.cat.torrent": "Torrent",
  "tickets.ui.cat.appeal": "Appeal",
  "tickets.ui.cat.suggestion": "Suggestion",
  "tickets.ui.cat.other": "Other",
  "tickets.too_many_open": "Too many open tickets; close resolved ones first",
  "tickets.closed": "This ticket is closed",
  "tickets.reopen_expired":
    "This ticket was closed too long ago; open a new one",
});

const CATS = ["account", "torrent", "appeal", "suggestion", "other"];
const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code
    ? api.t(e.code)
    : e instanceof Error
      ? e.message
      : String(e);
const stateTone = (s: string) =>
  s === "open" ? "danger" : s === "pending_user" ? "primary" : "muted";

function A({
  api,
  to,
  children,
}: {
  api: PluginApi;
  to: string;
  children: ReactNode;
}) {
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

interface Item {
  id: number;
  category: string;
  subject: string;
  state: string;
  created_at: number;
  updated_at: number;
  username: string | null;
  assignee: string | null;
  assignee_id: number | null;
  last_reply_staff: boolean | null;
}

function List({ api }: { api: PluginApi }) {
  const [rows, setRows] = useState<Item[]>([]);
  const [f, setF] = useState({ category: "account", subject: "", body: "" });
  const load = () =>
    void api
      .json<{ items: Item[] }>("/api/p/tickets/v1/tickets")
      .then((r) => setRows(r.items));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, []);
  const submit = async () => {
    try {
      const r = await api.json<{ id: number }>("/api/p/tickets/v1/tickets", {
        json: f,
      });
      api.toast(api.t("tickets.ui.created"), "success");
      api.navigate(`/p/tickets/${r.id}`);
    } catch (e) {
      api.toast(errText(api, e), "error");
    }
  };
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t("tickets.ui.title")}</h1>
      <Card title={api.t("tickets.ui.mine")}>
        <Table
          rows={rows}
          rowKey={(r) => r.id}
          empty={api.t("tickets.ui.none")}
          columns={[
            {
              key: "s",
              header: "",
              render: (r) => (
                <>
                  <A api={api} to={`/p/tickets/${r.id}`}>
                    {r.subject}
                  </A>{" "}
                  <Badge tone={stateTone(r.state)}>
                    {api.t(`tickets.ui.state.${r.state}`)}
                  </Badge>
                </>
              ),
            },
            {
              key: "c",
              header: "",
              render: (r) => api.t(`tickets.ui.cat.${r.category}`),
            },
            {
              key: "t",
              header: "",
              render: (r) => (
                <span className="pt-muted small">
                  {new Date(r.updated_at).toLocaleString()}
                </span>
              ),
            },
          ]}
        />
      </Card>
      <Card title={api.t("tickets.ui.new")}>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <Field label={api.t("tickets.ui.category")}>
            <select
              className="pt-input"
              name="ticket_category"
              value={f.category}
              onChange={(e) => setF({ ...f, category: e.target.value })}
            >
              {CATS.map((c) => (
                <option key={c} value={c}>
                  {api.t(`tickets.ui.cat.${c}`)}
                </option>
              ))}
            </select>
          </Field>
          <Field label={api.t("tickets.ui.subject")}>
            <Input
              name="ticket_subject"
              value={f.subject}
              maxLength={200}
              required
              onChange={(e) => setF({ ...f, subject: e.target.value })}
            />
          </Field>
          <Field label={api.t("tickets.ui.body")}>
            <MarkdownEditor name="ticket_body" rows={6} value={f.body} maxLength={20000} required onChange={(body) => setF({ ...f, body })} preview={renderPreview} />
          </Field>
          <Button
            type="submit"
            variant="primary"
            disabled={!f.subject.trim() || !f.body.trim()}
          >
            {api.t("tickets.ui.submit")}
          </Button>
        </form>
      </Card>
    </div>
  );
}

interface Detail extends Item {
  viewer_staff: boolean;
  messages: {
    id: number;
    staff: boolean;
    internal: boolean;
    html: string;
    created_at: number;
    author: string | null;
  }[];
}

function TicketPage({ api, id }: { api: PluginApi; id: number }) {
  const [d, setD] = useState<Detail | null>(null);
  const [body, setBody] = useState("");
  const [internal, setInternal] = useState(false);
  const load = () =>
    void api.json<Detail>(`/api/p/tickets/v1/tickets/${id}`).then(setD);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, [id]);
  if (!d) return null;
  const run = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      load();
    } catch (e) {
      api.toast(errText(api, e), "error");
    }
  };
  const send = () =>
    run(async () => {
      await api.json(`/api/p/tickets/v1/tickets/${id}/messages`, {
        json: { body, internal },
      });
      setBody("");
      setInternal(false);
    });
  const me = api.user();
  const staffView = d.viewer_staff && d.username !== me?.username;
  return (
    <div className="pt-stack">
      <h1 className="page-title">
        {d.subject}{" "}
        <Badge tone={stateTone(d.state)}>
          {api.t(`tickets.ui.state.${d.state}`)}
        </Badge>
      </h1>
      <p className="pt-muted small">
        {api.t(`tickets.ui.cat.${d.category}`)} · {d.username}
        {staffView &&
          ` · ${api.t("tickets.ui.assignee")}: ${d.assignee ?? "—"}`}
      </p>
      {d.messages.map((m) => (
        <Card
          key={m.id}
          className={
            m.internal ? "ticket-internal" : m.staff ? "ticket-staff" : ""
          }
        >
          <p className="pt-muted small">
            <strong>
              {m.staff
                ? m.author
                  ? `${m.author}（${api.t("tickets.ui.staff")}）`
                  : api.t("tickets.ui.staff")
                : m.author}
            </strong>{" "}
            · {new Date(m.created_at).toLocaleString()}
            {m.internal && (
              <>
                {" "}
                <Badge>{api.t("tickets.ui.internal")}</Badge>
              </>
            )}
          </p>
          <UserContent html={m.html} />
        </Card>
      ))}
      {(d.state !== "closed" || staffView) && (
        <Card title={api.t("tickets.ui.reply")}>
          <MarkdownEditor name="ticket_reply" rows={4} ariaLabel={api.t("tickets.ui.reply")} value={body} onChange={setBody} preview={renderPreview} />
          {staffView && (
            <label className="pt-check">
              <input
                type="checkbox"
                checked={internal}
                onChange={(e) => setInternal(e.target.checked)}
              />{" "}
              {api.t("tickets.ui.internal")}
            </label>
          )}
          <Button
            variant="primary"
            disabled={!body.trim() || (d.state === "closed" && !internal)}
            onClick={() => void send()}
          >
            {api.t("tickets.ui.send")}
          </Button>
        </Card>
      )}
      <div className="pt-row">
        {d.state !== "closed" ? (
          <Button
            onClick={() =>
              void run(() =>
                api.json(`/api/p/tickets/v1/tickets/${id}/close`, { json: {} }),
              )
            }
          >
            {api.t("tickets.ui.close")}
          </Button>
        ) : (
          <Button
            onClick={() =>
              void run(() =>
                api.json(`/api/p/tickets/v1/tickets/${id}/reopen`, {
                  json: {},
                }),
              )
            }
          >
            {api.t("tickets.ui.reopen")}
          </Button>
        )}
        {staffView &&
          (d.assignee_id === me?.id ? (
            <Button
              onClick={() =>
                void run(() =>
                  api.json(`/api/p/tickets/v1/admin/tickets/${id}/assign`, {
                    json: { user_id: 0 },
                  }),
                )
              }
            >
              {api.t("tickets.ui.release")}
            </Button>
          ) : (
            <Button
              onClick={() =>
                void run(() =>
                  api.json(`/api/p/tickets/v1/admin/tickets/${id}/assign`, {
                    json: {},
                  }),
                )
              }
            >
              {api.t("tickets.ui.claim")}
            </Button>
          ))}
      </div>
    </div>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  const m = /^\/p\/tickets\/(\d+)/.exec(path);
  return m ? <TicketPage api={api} id={Number(m[1])} /> : <List api={api} />;
}

function Queue({ api }: { api: PluginApi }) {
  const [state, setState] = useState("open");
  const [mine, setMine] = useState(false);
  const [d, setD] = useState<{
    items: Item[];
    counts: { open: number; pending_user: number };
  } | null>(null);
  useEffect(() => {
    void api
      .json<NonNullable<typeof d>>(
        `/api/p/tickets/v1/admin/tickets?state=${state}${mine ? "&mine=1" : ""}`,
      )
      .then(setD);
  }, [api, state, mine]);
  return (
    <div className="pt-stack">
      <h1 className="page-title">{api.t("tickets.ui.queue")}</h1>
      <Tabs
        value={state}
        onChange={setState}
        tabs={["open", "pending_user", "closed"].map((s) => ({
          id: s,
          label: `${api.t(`tickets.ui.state.${s}`)}${d && s !== "closed" ? ` (${d.counts[s as "open" | "pending_user"]})` : ""}`,
        }))}
      />
      <label className="pt-check">
        <input
          type="checkbox"
          checked={mine}
          onChange={(e) => setMine(e.target.checked)}
        />{" "}
        {api.t("tickets.ui.only_mine")}
      </label>
      <Card>
        <Table
          rows={d?.items ?? []}
          rowKey={(r) => r.id}
          empty={api.t("tickets.ui.none")}
          columns={[
            {
              key: "s",
              header: "",
              render: (r) => (
                <>
                  <A api={api} to={`/p/tickets/${r.id}`}>
                    {r.subject}
                  </A>{" "}
                  {r.last_reply_staff === false && r.state !== "closed" && (
                    <Badge tone="danger">
                      {api.t("tickets.ui.waiting_staff")}
                    </Badge>
                  )}
                  <div className="pt-muted small">
                    {api.t(`tickets.ui.cat.${r.category}`)} · {r.username}
                  </div>
                </>
              ),
            },
            { key: "a", header: "", render: (r) => r.assignee ?? "—" },
            {
              key: "t",
              header: "",
              render: (r) => (
                <span className="pt-muted small">
                  {new Date(r.updated_at).toLocaleString()}
                </span>
              ),
            },
          ]}
        />
      </Card>
    </div>
  );
}

function MenuItem({ api }: SlotProps) {
  return (
    <A api={api} to="/p/tickets/">
      {api.t("tickets.ui.menu")}
    </A>
  );
}

function Widget({ api }: SlotProps) {
  const [c, setC] = useState<{ open: number; pending_user: number } | null>(
    null,
  );
  useEffect(() => {
    void api
      .json<{ counts: { open: number; pending_user: number } }>(
        "/api/p/tickets/v1/admin/tickets?state=open",
      )
      .then((r) => setC(r.counts))
      .catch(() => undefined);
  }, [api]);
  if (!c || c.open === 0) return null;
  return (
    <Card
      title={
        <A api={api} to="/admin/p/tickets/">
          {api.t("tickets.ui.queue")}
        </A>
      }
    >
      <p>
        {api.t("tickets.ui.state.open")} {c.open} ·{" "}
        {api.t("tickets.ui.state.pending_user")} {c.pending_user}
      </p>
    </Card>
  );
}

export default {
  slots: {
    "nav.user_menu": { component: MenuItem, order: 90 },
    "admin.dashboard.widgets": { component: Widget, order: 20 },
  },
  routes: [
    { path: "/p/tickets/*", component: Pages },
    { path: "/admin/p/tickets/*", component: Queue },
  ],
} satisfies PluginFrontend;
