// levels 前端：个人页的等级（任何人的个人页都显示等级；本人另显示升级进度与保级警告）、
// 等级说明页（/p/levels/：全部等级、说明、升级与保级条件、我的进度）、用户菜单入口；
// 后台等级总览（/admin/p/levels/：设置同步状态与错误、每个等级对应的权限角色）。
// 等级的名称 / 说明可在插件设置中修改；为空时用内置术语（随界面语言）。
import { useEffect, useState, type FormEvent } from "react";
import { ApiError } from "@pt-studio/sdk";
import { registerMessages } from "@pt-studio/terms";
import type { PluginApi, PluginFrontend, SlotProps } from "@pt-studio/slots";
import {
  Button,
  Field,
  Input,
  NavItem,
  Note,
  PageHead,
  Panel,
  PLink,
  Progress,
  Select,
  Sheet,
  SideLayout,
  SkeletonRows,
  Stat,
  Status,
  Table,
  Tag,
  withAbbr,
} from "@pt-studio/ui-kit";

interface Cond {
  metric: string;
  op: string;
  value: number;
}

interface ClassInfo {
  id: number;
  key: string;
  type: "normal" | "vip" | "honor" | "staff";
  name: string;
  name_key: string;
  description: string;
  desc_key: string;
  color: string | null;
  promote: Cond[];
  keep: Cond[];
  can_invite: boolean;
  protected: boolean;
  is_default: boolean;
}

interface Brief {
  id: number;
  key?: string;
  name?: string;
  name_key?: string;
  abbr?: string;
  color?: string | null;
}

interface Me {
  /** until：当前等级来自覆盖（VIP、手动锁定）时的到期时间，null 为永久；不是覆盖时没有该字段 */
  class: Brief & { source: string; until?: number | null };
  next:
    | (Brief & {
        progress: {
          metric: string;
          op: string;
          target: string;
          current: string;
          met: boolean;
        }[];
      })
    | null;
  warning: { deadline: number } | null;
  protected: boolean;
}

registerMessages("zh-CN", {
  "levels.ui.class": "等级",
  "levels.ui.menu": "等级说明",
  "levels.ui.title": "用户等级",
  "levels.ui.warning": "保级警告：{date} 前未达标将降级",
  "levels.ui.my": "我的等级",
  "levels.ui.next": "下一等级",
  "levels.ui.top": "已是最高的普通等级",
  "levels.ui.name": "等级",
  "levels.ui.desc": "说明",
  "levels.ui.promote": "升级条件",
  "levels.ui.keep": "保级条件",
  "levels.ui.none": "—",
  "levels.ui.never_demoted": "不降级",
  "levels.ui.manual": "由管理员授予",
  "levels.ui.default": "新用户",
  "levels.ui.no_invite": "不能发邀请",
  "levels.ui.current": "当前",
  "levels.ui.note":
    "普通等级按条件自动评估：流量结算后 15 分钟内升级，每日凌晨全量检查保级条件。不满足保级条件时先发出警告，警告期满仍未达标才降级；升级后有一段保护期。",
  "levels.metric.account_age_days": "注册天数",
  "levels.metric.uploaded_gib": "上传量（GiB）",
  "levels.metric.downloaded_gib": "下载量（GiB）",
  "levels.metric.ratio": "分享率",
  "levels.metric.seed_hours": "做种时长（小时）",
  "levels.metric.avg_seed_hours": "平均做种时长（小时）",
  "levels.metric.seeding_size_gib": "做种体积（GiB）",
  "levels.metric.uploads_count": "发布数",
  "levels.metric.seed_points": "做种积分",
  "levels.ui.tab": "等级",
  "levels.ui.source": "来源：{s}",
  "levels.ui.overrides": "生效中的覆盖",
  "levels.ui.until": "至 {t}",
  "levels.ui.forever": "永久",
  "levels.ui.set_class": "设置等级",
  "levels.ui.lock_days": "锁定天数（留空为永久）",
  "levels.ui.reason": "原因",
  "levels.ui.apply": "设置",
  "levels.ui.unlock": "解除手动锁定",
  "levels.ui.protect_days": "保号天数（0 取消）",
  "levels.ui.protect": "保号",
  "levels.ui.protected_until": "保号至 {t}",
  "levels.ui.done": "已保存",
  "levels.ui.admin": "等级总览",
  "levels.ui.synced": "设置最近一次生效于 {t}",
  "levels.ui.sync_error": "最新的等级设置未能生效：{e}",
  "levels.ui.edit_settings": "编辑等级",
  "levels.ui.role_hint":
    "每个等级有自己的角色 levels:class_<id>，可在角色管理中为它配置权限。把用户设为管理组类型的等级（如“管理员”）只改变头衔与颜色，内置角色（版主 / 管理员）需要另外授予。",
  "levels.ui.type": "类型",
  "levels.ui.type.normal": "普通",
  "levels.ui.type.vip": "VIP",
  "levels.ui.type.honor": "荣誉",
  "levels.ui.type.staff": "管理组",
  "levels.ui.role": "角色",
  "levels.ui.flags": "属性",
  "levels.ui.cancel": "取消",
  "levels.ui.until_col": "截止",
  "levels.ui.source.auto": "按条件自动",
  "levels.ui.source.manual": "管理员设置",
  "levels.ui.kind.manual": "手动设置",
  "levels.ui.kind.honor": "荣誉",
  "levels.ui.kind.vip": "VIP",
  "levels.ui.set_hint":
    "VIP 类型的等级会同时获得 VIP 角色（做种魔力加成）；限时 VIP 填锁定天数，到期自动回到按条件评估的等级。管理组类型的等级（如管理员、维护开发员）只改变头衔与颜色，权限需在「概览」里另外授予角色。",
});
registerMessages("en", {
  "levels.ui.class": "Class",
  "levels.ui.menu": "User classes",
  "levels.ui.title": "User classes",
  "levels.ui.warning":
    "Warning: demotion on {date} unless requirements are met",
  "levels.ui.my": "My class",
  "levels.ui.next": "Next class",
  "levels.ui.top": "Highest regular class reached",
  "levels.ui.name": "Class",
  "levels.ui.desc": "Description",
  "levels.ui.promote": "Promotion",
  "levels.ui.keep": "To keep",
  "levels.ui.none": "—",
  "levels.ui.never_demoted": "Never demoted",
  "levels.ui.manual": "Granted by staff",
  "levels.ui.default": "New users",
  "levels.ui.no_invite": "Cannot invite",
  "levels.ui.current": "Current",
  "levels.ui.note":
    "Regular classes are evaluated automatically: promotions within 15 minutes of traffic settling, keep requirements checked daily. Failing them starts a warning period before demotion; a promotion comes with a protection period.",
  "levels.metric.account_age_days": "Account age (days)",
  "levels.metric.uploaded_gib": "Uploaded (GiB)",
  "levels.metric.downloaded_gib": "Downloaded (GiB)",
  "levels.metric.ratio": "Ratio",
  "levels.metric.seed_hours": "Seeding time (hours)",
  "levels.metric.avg_seed_hours": "Average seeding time (hours)",
  "levels.metric.seeding_size_gib": "Seeding size (GiB)",
  "levels.metric.uploads_count": "Uploads",
  "levels.metric.seed_points": "Seeding points",
  "levels.ui.tab": "Class",
  "levels.ui.source": "Source: {s}",
  "levels.ui.overrides": "Active overrides",
  "levels.ui.until": "until {t}",
  "levels.ui.forever": "permanent",
  "levels.ui.set_class": "Set class",
  "levels.ui.lock_days": "Lock for days (blank = permanent)",
  "levels.ui.reason": "Reason",
  "levels.ui.apply": "Set",
  "levels.ui.unlock": "Remove manual lock",
  "levels.ui.protect_days": "Protect for days (0 removes)",
  "levels.ui.protect": "Protect",
  "levels.ui.protected_until": "Protected until {t}",
  "levels.ui.done": "Saved",
  "levels.ui.admin": "Classes",
  "levels.ui.synced": "Settings last applied {t}",
  "levels.ui.sync_error": "The latest class settings could not be applied: {e}",
  "levels.ui.edit_settings": "Edit classes",
  "levels.ui.role_hint":
    "Each class has its own role levels:class_<id>; configure its permissions under roles. Setting a staff-type class (e.g. Administrator) only changes the title and colour — grant the built-in roles (moderator / admin) separately.",
  "levels.ui.type": "Type",
  "levels.ui.type.normal": "regular",
  "levels.ui.type.vip": "VIP",
  "levels.ui.type.honor": "honour",
  "levels.ui.type.staff": "staff",
  "levels.ui.role": "Role",
  "levels.ui.flags": "Flags",
  "levels.ui.cancel": "Cancel",
  "levels.ui.until_col": "Until",
  "levels.ui.source.auto": "automatic",
  "levels.ui.source.manual": "set by staff",
  "levels.ui.kind.manual": "manual",
  "levels.ui.kind.honor": "honour",
  "levels.ui.kind.vip": "VIP",
  "levels.ui.set_hint":
    "VIP-type classes also get the VIP role (seeding bonus multiplier); for a time-limited VIP enter lock days and the user returns to the automatically evaluated class when it ends. Staff-type classes (e.g. Administrator, SysOp) only change the title and colour; grant roles separately under Overview.",
});

const OPS: Record<string, string> = { gte: "≥", gt: ">", lte: "≤", lt: "<" };

/** 等级列表在页面内只取一次（多个个人页插槽共享） */
let classesPromise: Promise<ClassInfo[]> | null = null;
function loadClasses(api: PluginApi): Promise<ClassInfo[]> {
  classesPromise ??= api
    .json<{ items: ClassInfo[] }>("/api/p/levels/v1/classes")
    .then((r) => r.items)
    .catch((e: unknown) => {
      classesPromise = null;
      throw e;
    });
  return classesPromise;
}

function useClasses(api: PluginApi): ClassInfo[] | null {
  const [list, setList] = useState<ClassInfo[] | null>(null);
  useEffect(() => {
    if (api.user())
      void loadClasses(api)
        .then(setList)
        .catch(() => undefined);
  }, [api]);
  return list;
}

function className(api: PluginApi, c: Brief): string {
  if (c.name) return withAbbr(c.name, c.abbr);
  return withAbbr(
    c.name_key ? api.t(`term:${c.name_key}`) : `#${c.id}`,
    c.abbr,
  );
}

function ClassName({ api, c }: { api: PluginApi; c: Brief }) {
  return (
    <span className="cls-name" style={c.color ? { color: c.color } : undefined}>
      {className(api, c)}
    </span>
  );
}

const condLine = (api: PluginApi, c: Cond) =>
  `${api.t(`levels.metric.${c.metric}`)} ${OPS[c.op] ?? c.op} ${c.value}`;
const condText = (api: PluginApi, cs: Cond[]) =>
  cs.map((c) => condLine(api, c)).join("；");

/** 覆盖等级（VIP 等）的期限：“至 …” / “永久” */
function untilText(api: PluginApi, c: Me["class"]): string | null {
  const u = c.until;
  if (u === undefined) return null;
  return u === null
    ? api.t("levels.ui.forever")
    : api.t("levels.ui.until", { t: new Date(u).toLocaleDateString() });
}

function NextHint({ api, me }: { api: PluginApi; me: Me }) {
  const until = untilText(api, me.class);
  if (until) return <span>{until}</span>;
  if (me.warning)
    return (
      <span className="down">
        {api.t("levels.ui.warning", {
          date: new Date(me.warning.deadline).toLocaleDateString(),
        })}
      </span>
    );
  if (!me.next) return null;
  const met = me.next.progress.filter((p) => p.met).length;
  return (
    <span
      title={me.next.progress
        .map(
          (p) =>
            `${api.t(`levels.metric.${p.metric}`)}: ${p.current} / ${p.target}`,
        )
        .join("\n")}
    >
      → {className(api, me.next)} · {met}/{me.next.progress.length}
    </span>
  );
}

/** 个人页：任何人的等级与 VIP 等的到期时间；本人另显示进度 */
function LevelStat({
  ctx,
  api,
}: SlotProps<{ profile: { id: number; class_id: number } }>) {
  const classes = useClasses(api);
  const [me, setMe] = useState<Me | null>(null);
  const [other, setOther] = useState<Me["class"] | null>(null);
  const self = api.user()?.id === ctx.profile.id;
  useEffect(() => {
    if (self)
      void api
        .json<Me>("/api/p/levels/v1/me")
        .then(setMe)
        .catch(() => undefined);
    else
      void api
        .json<{ class: Me["class"] }>(
          `/api/p/levels/v1/users/${ctx.profile.id}/class`,
        )
        .then((r) => setOther(r.class))
        .catch(() => undefined);
  }, [api, self, ctx.profile.id]);
  const c: Brief | undefined =
    me?.class ?? other ?? classes?.find((x) => x.id === ctx.profile.class_id);
  if (!c) return null;
  const until = other && untilText(api, other);
  return (
    <Stat
      label={api.t("levels.ui.class")}
      to="/p/levels/"
      navigate={api.navigate}
      value={<ClassName api={api} c={c} />}
      hint={
        me ? (
          <NextHint api={api} me={me} />
        ) : until ? (
          <span>{until}</span>
        ) : null
      }
    />
  );
}

function MenuItem({ api }: SlotProps) {
  return (
    <NavItem to="/p/levels/" navigate={api.navigate}>
      {api.t("levels.ui.menu")}
    </NavItem>
  );
}

/** 单项进度：数值型按 current/target 画进度条（“≤ / <” 类条件只显示达标与否） */
function ProgressRow({
  api,
  p,
}: {
  api: PluginApi;
  p: {
    metric: string;
    op: string;
    target: string;
    current: string;
    met: boolean;
  };
}) {
  const up = p.op === "gte" || p.op === "gt";
  const cur = Number(p.current);
  const tgt = Number(p.target);
  return (
    <div className="stack-s">
      <div className="row-sb small">
        <span>{api.t(`levels.metric.${p.metric}`)}</span>
        <span className={p.met ? "num up" : "num"}>
          {p.current}{" "}
          <span className="muted">{`${OPS[p.op] ?? p.op} ${p.target}`}</span>
        </span>
      </div>
      <Progress
        value={p.met ? 1 : up && tgt > 0 ? cur / tgt : 0}
        max={1}
        label={api.t(`levels.metric.${p.metric}`)}
        tone={p.met ? "ok" : undefined}
      />
    </div>
  );
}

function Page({ api }: { api: PluginApi }) {
  const classes = useClasses(api);
  const [me, setMe] = useState<Me | null>(null);
  useEffect(() => {
    void api
      .json<Me>("/api/p/levels/v1/me")
      .then(setMe)
      .catch(() => undefined);
  }, [api]);
  if (!classes) return <SkeletonRows n={8} />;
  const desc = (c: ClassInfo) => {
    if (c.description) return c.description;
    const d = api.t(`term:${c.desc_key}`);
    return d === c.desc_key ? "" : d;
  };
  return (
    <div className="stack">
      <PageHead
        title={api.t("levels.ui.title")}
        lede={api.t("levels.ui.note")}
      />
      {me?.warning && (
        <Note tone="bad">
          {api.t("levels.ui.warning", {
            date: new Date(me.warning.deadline).toLocaleDateString(),
          })}
        </Note>
      )}
      <SideLayout
        width={320}
        sideFirst
        side={
          me && (
            <Panel title={api.t("levels.ui.my")} primary>
              <div className="stack">
                <dl className="ledger">
                  <dt>{api.t("levels.ui.my")}</dt>
                  <dd>
                    <ClassName api={api} c={me.class} />
                    {untilText(api, me.class) && (
                      <span className="muted small">
                        {" "}
                        · {untilText(api, me.class)}
                      </span>
                    )}
                  </dd>
                  <dt>{api.t("levels.ui.next")}</dt>
                  <dd>
                    {me.next ? (
                      <ClassName api={api} c={me.next} />
                    ) : (
                      api.t("levels.ui.top")
                    )}
                  </dd>
                </dl>
                {me.next?.progress.map((p) => (
                  <ProgressRow key={p.metric} api={api} p={p} />
                ))}
              </div>
            </Panel>
          )
        }
      >
        <Panel flush>
          {classes.map((c) => (
            <div
              key={c.key}
              className={me?.class.id === c.id ? "rung cur" : "rung"}
            >
              <div className="stack-s">
                <h3>
                  <ClassName api={api} c={c} />
                </h3>
                <div className="row">
                  {me?.class.id === c.id && (
                    <Tag tone="g">{api.t("levels.ui.current")}</Tag>
                  )}
                  {c.type !== "normal" && (
                    <Tag tone="o">{api.t(`levels.ui.type.${c.type}`)}</Tag>
                  )}
                  {c.is_default && <Tag>{api.t("levels.ui.default")}</Tag>}
                  {!c.can_invite && (
                    <Tag tone="x">{api.t("levels.ui.no_invite")}</Tag>
                  )}
                </div>
                {desc(c) && <p className="muted">{desc(c)}</p>}
              </div>
              <div>
                <span className="eyebrow">{api.t("levels.ui.promote")}</span>
                {c.type !== "normal" ? (
                  <p className="muted">{api.t("levels.ui.manual")}</p>
                ) : c.promote.length === 0 ? (
                  <p className="muted">{api.t("levels.ui.none")}</p>
                ) : (
                  <ul>
                    {c.promote.map((x) => (
                      <li key={x.metric}>{condLine(api, x)}</li>
                    ))}
                  </ul>
                )}
              </div>
              <div>
                <span className="eyebrow">{api.t("levels.ui.keep")}</span>
                {c.type !== "normal" || c.protected || c.keep.length === 0 ? (
                  <p className="muted">{api.t("levels.ui.never_demoted")}</p>
                ) : (
                  <ul>
                    {c.keep.map((x) => (
                      <li key={x.metric}>{condLine(api, x)}</li>
                    ))}
                  </ul>
                )}
              </div>
            </div>
          ))}
        </Panel>
      </SideLayout>
    </div>
  );
}

interface AdminUser extends Me {
  overrides: {
    id: number;
    kind: string;
    class_id: number;
    until: number | null;
    reason: string;
    created_by: string;
  }[];
  protected_until: number | null;
}

const errText = (api: PluginApi, e: unknown) =>
  e instanceof ApiError && api.t(e.code) !== e.code
    ? api.t(e.code)
    : e instanceof Error
      ? e.message
      : String(e);

/** `<prefix>.<code>` 的显示名；没有对应文案时显示原值 */
const label = (api: PluginApi, prefix: string, code: string) => {
  const v = api.t(`${prefix}.${code}`);
  return v === `${prefix}.${code}` ? code : v;
};

/** 后台用户详情页“等级”标签：当前等级与来源、生效中的覆盖；手动设置 / 锁定与保号在抽屉里操作 */
function UserTab({ ctx, api }: SlotProps<{ profile: { id: number } }>) {
  const uid = ctx.profile.id;
  const classes = useClasses(api);
  const [d, setD] = useState<AdminUser | null>(null);
  const [sheet, setSheet] = useState<"class" | "protect" | null>(null);
  const [f, setF] = useState({
    class: "",
    days: "",
    reason: "",
    protect: "30",
  });
  const base = `/api/p/levels/v1/admin/users/${uid}` as const;
  const load = () => void api.json<AdminUser>(base).then(setD);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(load, [uid]);
  const act = async (
    path: `/api/${string}`,
    init: { method?: string; json?: unknown },
  ) => {
    try {
      await api.json(path, init);
      api.toast(api.t("levels.ui.done"), "success");
      setSheet(null);
      load();
    } catch (e) {
      api.toast(errText(api, e), "error");
    }
  };
  const setClass = (e: FormEvent) => {
    e.preventDefault();
    const lock = f.days.trim()
      ? Date.now() + Number(f.days) * 86_400_000
      : null;
    void act(`${base}/class`, {
      json: { class: f.class, lock_until_ms: lock, reason: f.reason },
    });
  };
  const protect = (e: FormEvent) => {
    e.preventDefault();
    void act(`${base}/protect`, {
      json: {
        until_ms:
          Number(f.protect) > 0
            ? Date.now() + Number(f.protect) * 86_400_000
            : null,
      },
    });
  };
  if (!d || !classes) return <SkeletonRows n={3} />;
  const byId = (id: number) => classes.find((c) => c.id === id);
  const cancel = (
    <Button variant="ghost" onClick={() => setSheet(null)}>
      {api.t("levels.ui.cancel")}
    </Button>
  );
  return (
    <div className="stack">
      <div className="row-sb">
        <div className="row">
          <ClassName api={api} c={d.class} />
          <span className="small muted">
            {api.t("levels.ui.source", {
              s: label(api, "levels.ui.source", d.class.source),
            })}
          </span>
          {d.protected_until && (
            <Status tone="ok">
              {api.t("levels.ui.protected_until", {
                t: new Date(d.protected_until).toLocaleDateString(),
              })}
            </Status>
          )}
          {d.warning && (
            <Status tone="bad">
              {api.t("levels.ui.warning", {
                date: new Date(d.warning.deadline).toLocaleDateString(),
              })}
            </Status>
          )}
        </div>
        <div className="form-acts">
          <Button size="sm" variant="primary" onClick={() => setSheet("class")}>
            {api.t("levels.ui.set_class")}
          </Button>
          <Button size="sm" onClick={() => setSheet("protect")}>
            {api.t("levels.ui.protect")}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => void act(`${base}/class`, { method: "DELETE" })}
          >
            {api.t("levels.ui.unlock")}
          </Button>
        </div>
      </div>
      {d.overrides.length > 0 && (
        <Panel title={api.t("levels.ui.overrides")} flush>
          <Table
            rows={d.overrides}
            rowKey={(o) => o.id}
            columns={[
              {
                key: "k",
                header: api.t("levels.ui.type"),
                render: (o) => (
                  <Tag>{label(api, "levels.ui.kind", o.kind)}</Tag>
                ),
              },
              {
                key: "c",
                header: api.t("levels.ui.class"),
                render: (o) =>
                  byId(o.class_id) ? (
                    <ClassName api={api} c={byId(o.class_id) as Brief} />
                  ) : (
                    `#${o.class_id}`
                  ),
              },
              {
                key: "u",
                header: api.t("levels.ui.until_col"),
                render: (o) => (
                  <span className="small nowrap">
                    {o.until
                      ? new Date(o.until).toLocaleString()
                      : api.t("levels.ui.forever")}
                  </span>
                ),
              },
              {
                key: "r",
                header: api.t("levels.ui.reason"),
                render: (o) => (
                  <span className="small muted">
                    {o.reason} · {o.created_by}
                  </span>
                ),
              },
            ]}
          />
        </Panel>
      )}
      <Sheet
        open={sheet === "class"}
        onClose={() => setSheet(null)}
        title={api.t("levels.ui.set_class")}
        footer={
          <>
            {cancel}
            <Button
              type="submit"
              form="level-set"
              variant="primary"
              disabled={!f.class}
            >
              {api.t("levels.ui.apply")}
            </Button>
          </>
        }
      >
        <form id="level-set" className="form" onSubmit={setClass}>
          <Note>{api.t("levels.ui.set_hint")}</Note>
          <Field label={api.t("levels.ui.class")}>
            <Select
              name="level_class"
              value={f.class}
              onChange={(e) => setF({ ...f, class: e.target.value })}
              options={[
                { value: "", label: "—" },
                ...classes.map((c) => ({
                  value: c.key,
                  label: className(api, c),
                })),
              ]}
            />
          </Field>
          <Field label={api.t("levels.ui.lock_days")}>
            <Input
              name="level_days"
              type="number"
              min={1}
              value={f.days}
              onChange={(e) => setF({ ...f, days: e.target.value })}
            />
          </Field>
          <Field label={api.t("levels.ui.reason")}>
            <Input
              name="level_reason"
              maxLength={200}
              value={f.reason}
              onChange={(e) => setF({ ...f, reason: e.target.value })}
            />
          </Field>
        </form>
      </Sheet>
      <Sheet
        open={sheet === "protect"}
        onClose={() => setSheet(null)}
        title={api.t("levels.ui.protect")}
        footer={
          <>
            {cancel}
            <Button type="submit" form="level-protect" variant="primary">
              {api.t("levels.ui.protect")}
            </Button>
          </>
        }
      >
        <form id="level-protect" className="form" onSubmit={protect}>
          <Field label={api.t("levels.ui.protect_days")}>
            <Input
              name="level_protect"
              type="number"
              min={0}
              value={f.protect}
              onChange={(e) => setF({ ...f, protect: e.target.value })}
            />
          </Field>
        </form>
      </Sheet>
    </div>
  );
}

/** 后台等级总览（/admin/p/levels/） */
function Admin({ api }: { api: PluginApi }) {
  const [d, setD] = useState<{
    items: ClassInfo[];
    settings_error: string | null;
    synced_at: number | null;
  } | null>(null);
  useEffect(() => {
    void api
      .json<NonNullable<typeof d>>("/api/p/levels/v1/admin/classes")
      .then(setD);
  }, [api]);
  if (!d) return <SkeletonRows n={6} />;
  return (
    <div className="stack">
      <PageHead
        title={api.t("levels.ui.admin")}
        lede={
          d.synced_at
            ? api.t("levels.ui.synced", {
                t: new Date(d.synced_at).toLocaleString(),
              })
            : undefined
        }
        actions={
          <PLink
            to="/admin/p/levels/settings"
            navigate={api.navigate}
            className="btn"
          >
            {api.t("levels.ui.edit_settings")}
          </PLink>
        }
      />
      {d.settings_error && (
        <Note tone="bad">
          {api.t("levels.ui.sync_error", { e: d.settings_error })}
        </Note>
      )}
      <Note tone="info">{api.t("levels.ui.role_hint")}</Note>
      <Panel flush primary>
        <Table
          rows={d.items}
          rowKey={(c) => c.key}
          columns={[
            {
              key: "n",
              header: api.t("levels.ui.name"),
              render: (c) => <ClassName api={api} c={c} />,
            },
            {
              key: "k",
              header: "key",
              render: (c) => <span className="pt-mono small">{c.key}</span>,
            },
            {
              key: "t",
              header: api.t("levels.ui.type"),
              render: (c) => api.t(`levels.ui.type.${c.type}`),
            },
            {
              key: "f",
              header: api.t("levels.ui.flags"),
              render: (c) => (
                <div className="row">
                  {c.is_default && <Tag>{api.t("levels.ui.default")}</Tag>}
                  {!c.can_invite && (
                    <Tag tone="x">{api.t("levels.ui.no_invite")}</Tag>
                  )}
                  {c.protected && (
                    <Tag tone="k">{api.t("levels.ui.never_demoted")}</Tag>
                  )}
                </div>
              ),
            },
            {
              key: "r",
              header: api.t("levels.ui.role"),
              render: (c) => (
                <span className="pt-mono small">{`levels:class_${c.id}`}</span>
              ),
            },
          ]}
        />
      </Panel>
    </div>
  );
}

export default {
  slots: {
    "admin.user.tabs": {
      component: UserTab,
      order: 5,
      title_term: "levels.ui.tab",
    },
    "user.profile.stats": { component: LevelStat, order: 10 },
    "nav.user_menu": { component: MenuItem, order: 30 },
  },
  routes: [
    { path: "/p/levels/*", component: Page },
    { path: "/admin/p/levels/*", component: Admin },
  ],
} satisfies PluginFrontend;
