// tickets 前端：用户菜单“联系管理组”、我的工单与新建（/p/tickets/）、工单详情（/p/tickets/<id>：回复、关闭、重新打开；员工可写内部备注、认领）、
// 后台工单队列（/admin/p/tickets/）、仪表盘待处理数。
import { useEffect, useState, type FormEvent } from "react";
import { ApiError, renderPreview } from "@pt-studio/sdk";
import type { PluginApi, PluginFrontend, SlotProps } from "@pt-studio/slots";
import { registerMessages } from "@pt-studio/terms";
import {
  Avatar,
  Button,
  Checkbox,
  Chips,
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
  Sheet,
  SkeletonRows,
  Status,
  Switch,
  Table,
  Tabs,
  Tag,
  UserContent,
  type Tone,
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
  "tickets.ui.cancel": "取消",
  "tickets.ui.lede": "账号、种子、申诉或建议，都可以在这里直接和管理组沟通。",
  "tickets.ui.pick": "选择左侧的工单查看对话",
  "tickets.ui.internal_tag": "内部",
  "tickets.ui.user": "用户",
  "tickets.ui.updated": "更新",
  "tickets.ui.go": "去处理",
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
  "tickets.ui.cancel": "Cancel",
  "tickets.ui.lede": "Account, torrent, appeal or suggestion — talk to staff directly here.",
  "tickets.ui.pick": "Pick a ticket on the left",
  "tickets.ui.internal_tag": "internal",
  "tickets.ui.user": "User",
  "tickets.ui.updated": "Updated",
  "tickets.ui.go": "Open queue",
  "tickets.too_many_open": "Too many open tickets; close resolved ones first",
  "tickets.closed": "This ticket is closed",
  "tickets.reopen_expired":
    "This ticket was closed too long ago; open a new one",
});

const CATS = ["account", "torrent", "appeal", "suggestion", "other"];
const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code ? api.t(e.code) : e instanceof Error ? e.message : String(e);
const stateTone = (s: string): Tone => (s === "open" ? "warn" : s === "pending_user" ? "info" : "mute");
const rel = (ms: number) => new Date(ms).toLocaleString(undefined, { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" });

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

/** 新建工单抽屉 */
function NewTicket({ api, open, onClose }: { api: PluginApi; open: boolean; onClose(): void }) {
  const [f, setF] = useState({ category: "account", subject: "", body: "" });
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      const r = await api.json<{ id: number }>("/api/p/tickets/v1/tickets", { json: f });
      api.toast(api.t("tickets.ui.created"), "success");
      setF({ category: "account", subject: "", body: "" });
      onClose();
      api.navigate(`/p/tickets/${r.id}`);
    } catch (err) {
      api.toast(errText(api, err), "error");
    }
  };
  return (
    <Sheet
      open={open}
      onClose={onClose}
      title={api.t("tickets.ui.new")}
      width={640}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {api.t("tickets.ui.cancel")}
          </Button>
          <Button type="submit" form="ticket-new" variant="primary" disabled={!f.subject.trim() || !f.body.trim()}>
            {api.t("tickets.ui.submit")}
          </Button>
        </>
      }
    >
      <form id="ticket-new" className="form" onSubmit={(e) => void submit(e)}>
        <Field label={api.t("tickets.ui.category")}>
          <Chips label={api.t("tickets.ui.category")} value={f.category} onChange={(category) => setF({ ...f, category })} options={CATS.map((c) => ({ value: c, label: api.t(`tickets.ui.cat.${c}`) }))} />
        </Field>
        <Field label={api.t("tickets.ui.subject")}>
          <Input name="ticket_subject" value={f.subject} maxLength={200} required onChange={(e) => setF({ ...f, subject: e.target.value })} />
        </Field>
        <Field label={api.t("tickets.ui.body")}>
          <MarkdownEditor name="ticket_body" rows={8} value={f.body} maxLength={20000} required onChange={(body) => setF({ ...f, body })} preview={renderPreview} />
        </Field>
      </form>
    </Sheet>
  );
}

function TicketList({ api, rows, current }: { api: PluginApi; rows: Item[] | null; current: number }) {
  if (rows === null) return <SkeletonRows n={5} />;
  if (rows.length === 0) return <Empty compact>{api.t("tickets.ui.none")}</Empty>;
  return (
    <div className="convs">
      {rows.map((r) => (
        <PLink key={r.id} to={`/p/tickets/${r.id}`} navigate={api.navigate} className={current === r.id ? "conv on" : "conv"}>
          <Avatar name={api.t(`tickets.ui.cat.${r.category}`)} size={36} fallbackSrc={null} />
          <b>{r.subject}</b>
          <span className="tiny muted" style={{ gridColumn: 3, gridRow: 1 }}>
            {rel(r.updated_at)}
          </span>
          <span className="tiny muted">{`#${r.id} · ${api.t(`tickets.ui.cat.${r.category}`)}`}</span>
          <span style={{ gridColumn: 3, gridRow: 2 }}>
            <Status tone={stateTone(r.state)}>{api.t(`tickets.ui.state.${r.state}`)}</Status>
          </span>
        </PLink>
      ))}
    </div>
  );
}

/** 我的工单：P2 主从布局（窄屏先列表，选中后进入对话） */
function List({ api, id }: { api: PluginApi; id: number }) {
  const [rows, setRows] = useState<Item[] | null>(null);
  const [creating, setCreating] = useState(false);
  const load = () =>
    void api
      .json<{ items: Item[] }>("/api/p/tickets/v1/tickets")
      .then((r) => setRows(r.items))
      .catch(() => setRows([]));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, [id]);
  return (
    <div className="stack">
      <PageHead
        title={api.t("tickets.ui.title")}
        lede={api.t("tickets.ui.lede")}
        actions={
          <Button variant="primary" size="sm" onClick={() => setCreating(true)}>
            {api.t("tickets.ui.new")}
          </Button>
        }
      />
      <MasterDetail
        joined
        listWidth={330}
        showDetail={id > 0}
        backLabel={api.t("tickets.ui.mine")}
        onBack={() => api.navigate("/p/tickets/")}
        list={<TicketList api={api} rows={rows} current={id} />}
        detail={id > 0 ? <Conversation key={id} api={api} id={id} onChange={load} /> : <div className="md-empty">{api.t("tickets.ui.pick")}</div>}
      />
      <NewTicket api={api} open={creating} onClose={() => setCreating(false)} />
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

function Conversation({ api, id, onChange }: { api: PluginApi; id: number; onChange?(): void }) {
  const [d, setD] = useState<Detail | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [body, setBody] = useState("");
  const [internal, setInternal] = useState(false);
  const load = () =>
    void api
      .json<Detail>(`/api/p/tickets/v1/tickets/${id}`)
      .then(setD)
      .catch((e: unknown) => setErr(errText(api, e)));
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, [id]);
  if (err) return <div className="pad"><Note tone="bad">{err}</Note></div>;
  if (!d) return <SkeletonRows n={5} />;
  const run = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      load();
      onChange?.();
    } catch (e) {
      api.toast(errText(api, e), "error");
    }
  };
  const send = () =>
    run(async () => {
      await api.json(`/api/p/tickets/v1/tickets/${id}/messages`, { json: { body, internal } });
      setBody("");
      setInternal(false);
    });
  const me = api.user();
  const staffView = d.viewer_staff && d.username !== me?.username;
  const canWrite = d.state !== "closed" || staffView;
  return (
    <>
      <header className="panel-h conv-h">
        <h2>{d.subject}</h2>
        <span className="sub">
          <Status tone={stateTone(d.state)}>{api.t(`tickets.ui.state.${d.state}`)}</Status> {api.t(`tickets.ui.cat.${d.category}`)} · {d.username}
          {staffView && ` · ${api.t("tickets.ui.assignee")}: ${d.assignee ?? "—"}`}
        </span>
        <div className="acts">
          {staffView &&
            (d.assignee_id === me?.id ? (
              <Button size="xs" variant="ghost" onClick={() => void run(() => api.json(`/api/p/tickets/v1/admin/tickets/${id}/assign`, { json: { user_id: 0 } }))}>
                {api.t("tickets.ui.release")}
              </Button>
            ) : (
              <Button size="xs" onClick={() => void run(() => api.json(`/api/p/tickets/v1/admin/tickets/${id}/assign`, { json: {} }))}>
                {api.t("tickets.ui.claim")}
              </Button>
            ))}
          {d.state !== "closed" ? (
            <Button size="xs" variant="ghost" onClick={() => void run(() => api.json(`/api/p/tickets/v1/tickets/${id}/close`, { json: {} }))}>
              {api.t("tickets.ui.close")}
            </Button>
          ) : (
            <Button size="xs" variant="ghost" onClick={() => void run(() => api.json(`/api/p/tickets/v1/tickets/${id}/reopen`, { json: {} }))}>
              {api.t("tickets.ui.reopen")}
            </Button>
          )}
        </div>
      </header>
      <div className="thread">
        {d.messages.map((m) => {
          const mine = staffView ? m.staff : !m.staff;
          const name = m.staff ? (m.author ? `${m.author} · ${api.t("tickets.ui.staff")}` : api.t("tickets.ui.staff")) : (m.author ?? "");
          return (
            <div key={m.id} className={["bub", mine ? "me" : "", m.internal ? "internal" : ""].join(" ").trim()} data-testid="ticket-message">
              <Avatar name={m.author ?? name} size={36} />
              <div>
                <div className="bub-h">
                  {!mine && <b>{name}</b>}
                  <span>{new Date(m.created_at).toLocaleString()}</span>
                  {m.internal && <Tag tone="o">{api.t("tickets.ui.internal_tag")}</Tag>}
                </div>
                <div className="bub-b">
                  <UserContent html={m.html} />
                </div>
              </div>
            </div>
          );
        })}
      </div>
      {canWrite && (
        <form
          className="composer"
          onSubmit={(e) => {
            e.preventDefault();
            void send();
          }}
        >
          <MarkdownEditor name="ticket_reply" rows={3} ariaLabel={api.t("tickets.ui.reply")} value={body} onChange={setBody} preview={renderPreview} />
          <div className="form-acts">
            {staffView && <Checkbox name="ticket_internal" checked={internal} onChange={setInternal} label={api.t("tickets.ui.internal")} />}
            <Button type="submit" variant="primary" className="push-right" disabled={!body.trim() || (d.state === "closed" && !internal)}>
              {api.t("tickets.ui.send")}
            </Button>
          </div>
        </form>
      )}
    </>
  );
}

function Pages({ api, path }: { api: PluginApi; path: string }) {
  const m = /^\/p\/tickets\/(\d+)/.exec(path);
  return <List api={api} id={m ? Number(m[1]) : 0} />;
}

function Queue({ api }: { api: PluginApi }) {
  const [state, setState] = useState("open");
  const [mine, setMine] = useState(false);
  const [d, setD] = useState<{ items: Item[]; counts: { open: number; pending_user: number } } | null>(null);
  useEffect(() => {
    setD(null);
    void api.json<NonNullable<typeof d>>(`/api/p/tickets/v1/admin/tickets?state=${state}${mine ? "&mine=1" : ""}`).then(setD);
  }, [api, state, mine]);
  return (
    <div className="stack">
      <PageHead title={api.t("tickets.ui.queue")} actions={<Switch showLabel checked={mine} onChange={setMine} label={api.t("tickets.ui.only_mine")} />} />
      <Panel flush primary>
        <Tabs
          variant="flush"
          value={state}
          onChange={setState}
          tabs={["open", "pending_user", "closed"].map((s) => ({
            id: s,
            label: api.t(`tickets.ui.state.${s}`),
            count: d && s !== "closed" ? d.counts[s as "open" | "pending_user"] : undefined,
          }))}
        />
        {!d ? (
          <SkeletonRows n={6} />
        ) : (
          <Table
            rows={d.items}
            rowKey={(r) => r.id}
            empty={api.t("tickets.ui.none")}
            onRowClick={(r) => api.navigate(`/p/tickets/${r.id}`)}
            columns={[
              {
                key: "s",
                header: api.t("tickets.ui.subject"),
                render: (r) => (
                  <>
                    <PLink to={`/p/tickets/${r.id}`} navigate={api.navigate}>
                      {r.subject}
                    </PLink>{" "}
                    {r.last_reply_staff === false && r.state !== "closed" && <Status tone="bad">{api.t("tickets.ui.waiting_staff")}</Status>}
                  </>
                ),
              },
              { key: "c", header: api.t("tickets.ui.category"), render: (r) => <Tag>{api.t(`tickets.ui.cat.${r.category}`)}</Tag> },
              { key: "u", header: api.t("tickets.ui.user"), render: (r) => r.username ?? "—" },
              { key: "a", header: api.t("tickets.ui.assignee"), render: (r) => <span className="small">{r.assignee ?? "—"}</span> },
              { key: "t", header: api.t("tickets.ui.updated"), right: true, render: (r) => <span className="small muted num nowrap">{rel(r.updated_at)}</span> },
            ]}
          />
        )}
      </Panel>
    </div>
  );
}

function MenuItem({ api }: SlotProps) {
  return (
    <NavItem to="/p/tickets/" navigate={api.navigate}>
      {api.t("tickets.ui.menu")}
    </NavItem>
  );
}

function Widget({ api }: SlotProps) {
  const [c, setC] = useState<{ open: number; pending_user: number } | null>(null);
  useEffect(() => {
    void api
      .json<{ counts: { open: number; pending_user: number } }>("/api/p/tickets/v1/admin/tickets?state=open")
      .then((r) => setC(r.counts))
      .catch(() => undefined);
  }, [api]);
  if (!c || c.open === 0) return null;
  return (
    <Panel
      title={api.t("tickets.ui.queue")}
      more={
        <PLink to="/admin/p/tickets/" navigate={api.navigate}>
          {api.t("tickets.ui.go")}
        </PLink>
      }
    >
      <dl className="ledger">
        <dt>{api.t("tickets.ui.state.open")}</dt>
        <dd className="num down">{c.open}</dd>
        <dt>{api.t("tickets.ui.state.pending_user")}</dt>
        <dd className="num">{c.pending_user}</dd>
      </dl>
    </Panel>
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
