// registration-policy 前端：注册页的开放状态提示（关闭 / 仅邀请 / 限时开放与剩余名额 / 需审核），后台限时开放窗口管理。
import { useEffect, useState, type FormEvent } from "react";
import { ApiError } from "@pt-studio/sdk";
import type { PluginApi, PluginFrontend, SlotProps } from "@pt-studio/slots";
import { registerMessages } from "@pt-studio/terms";
import {
  Button,
  Field,
  Input,
  Note,
  PageHead,
  Panel,
  Sheet,
  Status,
  Table,
} from "@pt-studio/ui-kit";

registerMessages("zh-CN", {
  "registration-policy.ui.closed": "注册已关闭。",
  "registration-policy.ui.invite_only": "目前只接受邀请注册，请填写邀请码。",
  "registration-policy.ui.next_open": "下次开放：{at}",
  "registration-policy.ui.window": "限时开放中，{at} 截止",
  "registration-policy.ui.slots_left": "，剩余 {n} 个名额",
  "registration-policy.ui.review":
    "注册后需要管理员审核，审核通过前只能浏览公开内容。",
  "registration-policy.ui.windows": "限时开放注册",
  "registration-policy.ui.starts_at": "开始时间",
  "registration-policy.ui.ends_at": "结束时间",
  "registration-policy.ui.slots": "名额（留空不限）",
  "registration-policy.ui.note": "备注",
  "registration-policy.ui.create": "创建",
  "registration-policy.ui.registered": "已注册",
  "registration-policy.ui.cancel": "取消",
  "registration-policy.ui.cancelled": "已取消",
  "registration-policy.ui.hint":
    "开放注册的开关、时段、名额上限与邮箱域名规则在“插件设置”中修改。",
  "registration-policy.closed": "注册已关闭",
  "registration-policy.invite_required": "目前只接受邀请注册",
  "registration-policy.email_domain_denied": "这个邮箱域名不能用于注册",
  "registration-policy.ip_limit_reached":
    "你所在的网络近期注册的账号过多，请稍后再试",
  "registration-policy.window_overlap": "与已有的开放时段重叠",
});
registerMessages("en", {
  "registration-policy.ui.closed": "Registration is closed.",
  "registration-policy.ui.invite_only":
    "Registration is invite-only right now; enter an invite code.",
  "registration-policy.ui.next_open": "Opens next at {at}",
  "registration-policy.ui.window": "Open for a limited time until {at}",
  "registration-policy.ui.slots_left": ", {n} places left",
  "registration-policy.ui.review":
    "New accounts are reviewed by staff before they are activated.",
  "registration-policy.ui.windows": "Registration rounds",
  "registration-policy.ui.starts_at": "Starts",
  "registration-policy.ui.ends_at": "Ends",
  "registration-policy.ui.slots": "Places (empty = unlimited)",
  "registration-policy.ui.note": "Note",
  "registration-policy.ui.create": "Create",
  "registration-policy.ui.registered": "Registered",
  "registration-policy.ui.cancel": "Cancel",
  "registration-policy.ui.cancelled": "Cancelled",
  "registration-policy.ui.hint":
    "The open switch, schedule, caps and email-domain rules are in the plugin settings.",
  "registration-policy.closed": "Registration is closed",
  "registration-policy.invite_required":
    "Registration is invite-only right now",
  "registration-policy.email_domain_denied":
    "This email domain cannot be used to register",
  "registration-policy.ip_limit_reached":
    "Too many accounts were registered from your network recently",
  "registration-policy.window_overlap": "This overlaps another round",
});

interface Status {
  open: boolean;
  invite: boolean;
  review: boolean;
  next_open_at: number | null;
  window: {
    ends_at: number;
    slots: number | null;
    slots_left: number | null;
  } | null;
  message: string | null;
}

function RegisterNotice({ api }: SlotProps) {
  const [s, setS] = useState<Status | null>(null);
  useEffect(() => {
    api
      .json<Status>("/api/p/registration-policy/v1/status")
      .then(setS, () => setS(null));
  }, [api]);
  if (!s) return null;
  const at = (ms: number) => new Date(ms).toLocaleString();
  if (!s.open) {
    return (
      <div role="status" data-testid="registration-status">
        <Note tone="warn">
          {s.invite
            ? api.t("registration-policy.ui.invite_only")
            : api.t("registration-policy.ui.closed")}
          {s.next_open_at
            ? ` ${api.t("registration-policy.ui.next_open", { at: at(s.next_open_at) })}`
            : ""}
          {s.message ? ` ${s.message}` : ""}
        </Note>
      </div>
    );
  }
  if (!s.window && !s.review) return null;
  return (
    <div role="status" data-testid="registration-status">
      <Note tone="info">
        {s.window &&
          api.t("registration-policy.ui.window", { at: at(s.window.ends_at) })}
        {s.window?.slots_left != null &&
          api.t("registration-policy.ui.slots_left", {
            n: s.window.slots_left,
          })}
        {s.window && s.review ? " · " : ""}
        {s.review && api.t("registration-policy.ui.review")}
      </Note>
    </div>
  );
}

interface Win {
  id: number;
  starts_at: number;
  ends_at: number;
  slots: number | null;
  note: string | null;
  cancelled: boolean;
  registered: number;
}

/** datetime-local 输入值 → Unix 毫秒 */
const fromLocal = (v: string) => new Date(v).getTime();

function Windows({ api }: { api: PluginApi }) {
  const [items, setItems] = useState<Win[]>([]);
  const [f, setF] = useState({
    starts_at: "",
    ends_at: "",
    slots: "",
    note: "",
  });
  const [open, setOpen] = useState(false);
  const load = () =>
    api
      .json<{ items: Win[] }>("/api/p/registration-policy/v1/admin/windows")
      .then((r) => setItems(r.items));
  useEffect(() => {
    void load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const fail = (e: unknown) => {
    const msg =
      e instanceof ApiError && api.t(e.code) !== e.code
        ? api.t(e.code)
        : e instanceof Error
          ? e.message
          : String(e);
    api.toast(msg, "error");
  };
  const create = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await api.json("/api/p/registration-policy/v1/admin/windows", {
        json: {
          starts_at: fromLocal(f.starts_at),
          ends_at: fromLocal(f.ends_at),
          slots: f.slots ? Number(f.slots) : null,
          note: f.note || null,
        },
      });
      setF({ starts_at: "", ends_at: "", slots: "", note: "" });
      setOpen(false);
      await load();
    } catch (err) {
      fail(err);
    }
  };
  const cancel = async (w: Win) => {
    try {
      await api.fetch(`/api/p/registration-policy/v1/admin/windows/${w.id}`, {
        method: "DELETE",
      });
      await load();
    } catch (err) {
      fail(err);
    }
  };
  const set = (k: keyof typeof f) => (e: { target: { value: string } }) =>
    setF({ ...f, [k]: e.target.value });
  return (
    <div className="stack">
      <PageHead
        title={api.t("registration-policy.ui.windows")}
        lede={api.t("registration-policy.ui.hint")}
        actions={
          <Button variant="primary" onClick={() => setOpen(true)}>
            {api.t("registration-policy.ui.create")}
          </Button>
        }
      />
      <Panel flush primary>
        <Table
          rows={items}
          rowKey={(w) => w.id}
          empty="—"
          columns={[
            {
              key: "s",
              header: api.t("registration-policy.ui.starts_at"),
              render: (w) => (
                <span className="num small">
                  {new Date(w.starts_at).toLocaleString()}
                </span>
              ),
            },
            {
              key: "e",
              header: api.t("registration-policy.ui.ends_at"),
              render: (w) => (
                <span className="num small">
                  {new Date(w.ends_at).toLocaleString()}
                </span>
              ),
            },
            {
              key: "n",
              header: api.t("registration-policy.ui.registered"),
              right: true,
              render: (w) => (
                <span className="num">{`${w.registered} / ${w.slots ?? "∞"}`}</span>
              ),
            },
            {
              key: "note",
              header: api.t("registration-policy.ui.note"),
              render: (w) => <span className="small">{w.note ?? ""}</span>,
            },
            {
              key: "a",
              header: "",
              right: true,
              render: (w) =>
                w.cancelled ? (
                  <Status tone="mute">
                    {api.t("registration-policy.ui.cancelled")}
                  </Status>
                ) : (
                  <Button
                    size="xs"
                    variant="ghost"
                    onClick={() => void cancel(w)}
                  >
                    {api.t("registration-policy.ui.cancel")}
                  </Button>
                ),
            },
          ]}
        />
      </Panel>
      <Sheet
        open={open}
        onClose={() => setOpen(false)}
        title={api.t("registration-policy.ui.create")}
        footer={
          <Button type="submit" form="reg-window" variant="primary">
            {api.t("registration-policy.ui.create")}
          </Button>
        }
      >
        <form
          id="reg-window"
          className="form"
          onSubmit={(e) => void create(e)}
        >
          <div className="form-2">
            <Field label={api.t("registration-policy.ui.starts_at")}>
              <Input
                name="starts_at"
                type="datetime-local"
                value={f.starts_at}
                onChange={set("starts_at")}
                required
              />
            </Field>
            <Field label={api.t("registration-policy.ui.ends_at")}>
              <Input
                name="ends_at"
                type="datetime-local"
                value={f.ends_at}
                onChange={set("ends_at")}
                required
              />
            </Field>
          </div>
          <Field label={api.t("registration-policy.ui.slots")}>
            <Input
              name="slots"
              type="number"
              min={1}
              value={f.slots}
              onChange={set("slots")}
            />
          </Field>
          <Field label={api.t("registration-policy.ui.note")}>
            <Input
              name="note"
              maxLength={200}
              value={f.note}
              onChange={set("note")}
            />
          </Field>
        </form>
      </Sheet>
    </div>
  );
}

export default {
  slots: {
    "auth.register.form_extra": { component: RegisterNotice, order: 1 },
  },
  routes: [{ path: "/admin/p/registration-policy/*", component: Windows }],
} satisfies PluginFrontend;
