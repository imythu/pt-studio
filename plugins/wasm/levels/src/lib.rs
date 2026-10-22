//! levels 插件（WASM，docs/plugins/levels/）。
//!
//! 本版本实现：等级定义（NexusPHP 预设）、角色同步（每个等级一个 `levels:class_<id>` 角色并绑定到等级）、
//! 指标（注册天数、上传 / 下载 / 分享率、做种时长、发布数、做种体积）、评估（升级、保级警告、到期降级、
//! 升级保护期、手动覆盖与保号）、增量评估（流量结算后标记）与每日全量评估、`invite.before_issue` 钩子、
//! `ctx.levels` 服务、HTTP。VIP 授予（`grant_vip`，顺延、幂等）与覆盖到期自动结束。
//! 邀请奖励、不活跃处理、荣誉覆盖、模拟评估在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

pt_plugin_sdk::world!("levels");

use std::collections::BTreeMap;

use exports::pt::plugin::{hooks, http_handler, lifecycle, mcp_tool, on_cron, on_event, service_provider};
use pt_plugin_sdk::{
    RowExt, events, invocation, services,
    sql::{self, SqlValue},
    types::PluginError,
};
use serde_json::{Value, json};

struct Levels;

type R<T> = Result<T, PluginError>;

fn err(code: &str, message: &str) -> PluginError {
    PluginError {
        code: code.into(),
        message: message.into(),
        details: None,
        retryable: false,
    }
}

fn int(v: &SqlValue) -> i64 {
    match v {
        SqlValue::Int(i) => *i,
        _ => 0,
    }
}

fn num(v: &SqlValue) -> f64 {
    match v {
        SqlValue::Int(i) => *i as f64,
        SqlValue::Float(f) => *f,
        SqlValue::Numeric(s) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn text(v: &SqlValue) -> String {
    match v {
        SqlValue::Text(s) => s.clone(),
        _ => String::new(),
    }
}

fn call(service: &str, method: &str, args: &Value) -> R<Value> {
    let out = services::call(service, method, &args.to_string())?;
    serde_json::from_str(&out).map_err(|_| err("levels.bad_service_reply", "invalid JSON from service"))
}

fn publish(event_type: &str, user: i64, payload: Value) -> R<()> {
    events::publish(&events::NewEvent {
        event_type: event_type.into(),
        version: 1,
        key: format!("user:{user}"),
        payload: payload.to_string(),
    })?;
    Ok(())
}

/// 运行时选项（插件设置；旧版本写在 site.toml `[plugin.levels]` 的同名键仍然有效）
#[derive(Debug, Clone, Copy)]
struct Opts {
    one_step: bool,
    protect_days: i64,
    warning_days: i64,
}

fn opts() -> Opts {
    let (set, st) = (pt_plugin_sdk::ctx::settings(), pt_plugin_sdk::ctx::static_config());
    let get = |k: &str| {
        if set[k].is_null() {
            st[k].clone()
        } else {
            set[k].clone()
        }
    };
    Opts {
        one_step: get("promote_mode").as_str() == Some("one_step"),
        protect_days: get("protect_days").as_i64().unwrap_or(7).clamp(0, 365),
        warning_days: get("warning_days").as_i64().unwrap_or(7).clamp(1, 90),
    }
}

// ---------------- 等级定义：插件设置 → classes 表 ----------------

const MANIFEST: &str = include_str!("../plugin.toml");

/// 条件可用的指标（单位已换算：GiB、小时），与 settings_schema 的枚举一致
const METRICS: [&str; 9] = [
    "account_age_days",
    "uploaded_gib",
    "downloaded_gib",
    "ratio",
    "seed_hours",
    "avg_seed_hours",
    "seeding_size_gib",
    "uploads_count",
    "seed_points",
];
const OPS: [&str; 4] = ["gte", "gt", "lte", "lt"];
const TYPES: [&str; 4] = ["normal", "vip", "honor", "staff"];
const MAX_CLASSES: usize = 64;
/// 设置规范化规则变化时递增，使已同步的设置重新处理
const SYNC_VERSION: &str = "4";

/// 预设等级的固定数字 id：与旧版本数据以及按数字 id 配置的其他规则（限流倍数、魔力加成等）保持兼容
/// 预设等级的常用简写（NexusPHP 社区惯用；设置中的 abbr 为空时使用）
fn preset_abbr(key: &str) -> &'static str {
    match key {
        "power_user" => "PU",
        "elite_user" => "EU",
        "crazy_user" => "CU",
        "insane_user" => "IU",
        "veteran_user" => "VU",
        "extreme_user" => "EXU",
        "ultimate_user" => "UU",
        "nexus_master" => "NM",
        _ => "",
    }
}

fn preset_id(key: &str) -> Option<i64> {
    Some(match key {
        "peasant" => 10,
        "user" => 20,
        "power_user" => 30,
        "elite_user" => 40,
        "crazy_user" => 50,
        "insane_user" => 60,
        "veteran_user" => 70,
        "extreme_user" => 80,
        "ultimate_user" => 90,
        "nexus_master" => 100,
        "vip" => 200,
        "retiree" => 210,
        "uploader" => 220,
        "moderator" => 230,
        "administrator" => 240,
        "sysop" => 250,
        "staff_leader" => 260,
        _ => return None,
    })
}

/// NexusPHP 预设（plugin.toml 中 settings_schema.properties.classes.default，只维护这一份）
fn preset_classes() -> Value {
    toml::from_str::<toml::Table>(MANIFEST)
        .ok()
        .and_then(|t| serde_json::to_value(t).ok())
        .map(|v| v["settings_schema"]["properties"]["classes"]["default"].clone())
        .unwrap_or(Value::Null)
}

/// 校验后的等级定义
#[derive(Debug, Clone, PartialEq)]
struct Def {
    key: String,
    name: String,
    abbr: String,
    description: String,
    kind: String,
    is_default: bool,
    promote: Vec<Value>,
    keep: Vec<Value>,
    can_invite: bool,
    protected: bool,
    color: String,
}

fn valid_key(k: &str) -> bool {
    let mut cs = k.chars();
    cs.next().is_some_and(|c| c.is_ascii_lowercase())
        && k.len() <= 32
        && cs.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn conditions(v: &Value, at: &str) -> Result<Vec<Value>, String> {
    let Some(list) = v.as_array() else {
        return if v.is_null() {
            Ok(vec![])
        } else {
            Err(format!("{at}: must be a list"))
        };
    };
    if list.len() > 16 {
        return Err(format!("{at}: at most 16 conditions"));
    }
    list.iter()
        .enumerate()
        .map(|(i, c)| {
            let metric = c["metric"].as_str().unwrap_or("");
            let op = c["op"].as_str().unwrap_or("");
            let value = c["value"].as_f64().filter(|v| v.is_finite() && *v >= 0.0);
            match (METRICS.contains(&metric), OPS.contains(&op), value) {
                (true, true, Some(value)) => Ok(json!({ "metric": metric, "op": op, "value": value })),
                (false, ..) => Err(format!("{at}[{i}]: unknown metric `{metric}`")),
                (_, false, _) => Err(format!("{at}[{i}]: unknown operator `{op}`")),
                _ => Err(format!("{at}[{i}]: value must be a non-negative number")),
            }
        })
        .collect()
}

/// 校验设置中的等级列表；任何一处无效都整体拒绝（保留上一份有效定义）
fn validate(raw: &Value) -> Result<Vec<Def>, String> {
    let list = raw.as_array().ok_or("classes must be a list")?;
    if list.is_empty() || list.len() > MAX_CLASSES {
        return Err(format!("classes: 1 to {MAX_CLASSES} entries required"));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::with_capacity(list.len());
    for (i, c) in list.iter().enumerate() {
        let key = c["key"].as_str().unwrap_or("").to_owned();
        let at = format!("classes[{i}] ({key})");
        if !valid_key(&key) {
            return Err(format!("{at}: key must match ^[a-z][a-z0-9_]{{0,31}}$"));
        }
        if !seen.insert(key.clone()) {
            return Err(format!("{at}: duplicate key"));
        }
        let kind = c["type"].as_str().unwrap_or("normal").to_owned();
        if !TYPES.contains(&kind.as_str()) {
            return Err(format!("{at}: unknown type `{kind}`"));
        }
        let name = c["name"].as_str().unwrap_or("").trim().to_owned();
        let abbr = c["abbr"].as_str().unwrap_or("").trim().to_owned();
        let description = c["description"].as_str().unwrap_or("").trim().to_owned();
        if name.chars().count() > 32 || abbr.chars().count() > 8 || description.chars().count() > 1000 {
            return Err(format!("{at}: name ≤ 32, abbr ≤ 8 and description ≤ 1000 characters"));
        }
        let color = c["color"].as_str().unwrap_or("").to_owned();
        if !(color.is_empty()
            || (color.len() == 7 && color.starts_with('#') && color[1..].chars().all(|h| h.is_ascii_hexdigit())))
        {
            return Err(format!("{at}: color must look like #e53935"));
        }
        out.push(Def {
            key,
            name,
            abbr,
            description,
            is_default: c["is_default"].as_bool().unwrap_or(false),
            promote: conditions(&c["promote"], &format!("{at}.promote"))?,
            keep: conditions(&c["keep"], &format!("{at}.keep"))?,
            can_invite: c["can_invite"].as_bool().unwrap_or(true),
            protected: c["protected"].as_bool().unwrap_or(false),
            kind,
            color,
        });
    }
    let defaults: Vec<&Def> = out.iter().filter(|d| d.is_default).collect();
    match defaults.as_slice() {
        [d] if d.kind == "normal" => Ok(out),
        [_] => Err("the default class must be a normal class".into()),
        _ => Err("exactly one class must be the default class".into()),
    }
}

/// 设置摘要（FNV-1a，只用于判断是否变化）
fn digest(s: &str) -> String {
    let h = s
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    format!("{h:016x}")
}

/// 把插件设置中的等级定义同步到 classes 表（设置未变化时只读一行）。
/// 所有值都以参数绑定写入；设置无效时记录原因并保留上一份有效定义。
fn sync_classes() -> R<()> {
    let set = pt_plugin_sdk::ctx::settings();
    let raw = match &set["classes"] {
        Value::Array(a) if !a.is_empty() => set["classes"].clone(),
        _ => preset_classes(),
    };
    let hash = digest(&format!("{SYNC_VERSION}:{raw}"));
    let st = sql::query("SELECT hash FROM sync_state FOR UPDATE", &[])?;
    if st.rows.first().map(|r| text(&r[0])).as_deref() == Some(hash.as_str()) {
        return Ok(());
    }
    let defs = match validate(&raw) {
        Ok(d) => d,
        Err(e) => {
            pt_plugin_sdk::logging::warn("levels: invalid classes setting; keeping the previous classes", &[("error", &e)]);
            sql::execute(
                "UPDATE sync_state SET hash = $1, error = $2, synced_at = now()",
                &[SqlValue::Text(hash), SqlValue::Text(e)],
            )?;
            return Ok(());
        }
    };
    let existing: BTreeMap<String, i64> = sql::query("SELECT slug, id FROM classes", &[])?
        .rows
        .iter()
        .map(|r| (text(&r[0]), int(&r[1])))
        .collect();
    let used: std::collections::BTreeSet<i64> = existing.values().copied().collect();
    let mut next = used.iter().copied().max().unwrap_or(0).max(999);
    // 先把名次挪开，避免逐行更新时撞上“普通等级名次唯一”索引
    sql::execute("UPDATE classes SET rank = -id", &[])?;
    let mut keys = Vec::with_capacity(defs.len());
    for (i, d) in defs.iter().enumerate() {
        let id = match existing.get(&d.key) {
            Some(id) => *id,
            None => match preset_id(&d.key).filter(|id| !used.contains(id)) {
                Some(id) => id,
                None => {
                    next += 1;
                    next
                }
            },
        };
        let rank = i64::try_from(i + 1).unwrap_or(0) * 10;
        sql::execute(
            "INSERT INTO classes (id, slug, rank, type, promote, keep, can_invite, protected, is_default, name, description, color, active, preset, abbr)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, nullif($12, ''), true, 'settings', $13)
             ON CONFLICT (id) DO UPDATE SET slug = EXCLUDED.slug, rank = EXCLUDED.rank, type = EXCLUDED.type,
                    promote = EXCLUDED.promote, keep = EXCLUDED.keep, can_invite = EXCLUDED.can_invite,
                    protected = EXCLUDED.protected, is_default = EXCLUDED.is_default, name = EXCLUDED.name,
                    description = EXCLUDED.description, color = EXCLUDED.color, abbr = EXCLUDED.abbr, active = true,
                    revision = classes.revision + 1, updated_at = now()",
            &[
                SqlValue::Int(id),
                SqlValue::Text(d.key.clone()),
                SqlValue::Int(rank),
                SqlValue::Text(d.kind.clone()),
                SqlValue::Json(json!(d.promote).to_string()),
                SqlValue::Json(json!(d.keep).to_string()),
                SqlValue::Boolean(d.can_invite),
                SqlValue::Boolean(d.protected),
                SqlValue::Boolean(d.is_default),
                SqlValue::Text(d.name.clone()),
                SqlValue::Text(d.description.clone()),
                SqlValue::Text(d.color.clone()),
                SqlValue::Text(d.abbr.clone()),
            ],
        )?;
        keys.push(d.key.clone());
    }
    // 设置中已删除的等级停用（行保留：历史与覆盖记录仍引用它）
    sql::execute(
        "UPDATE classes SET active = false, is_default = false, updated_at = now() WHERE NOT (slug = ANY($1)) AND active",
        &[SqlValue::TextArray(keys)],
    )?;
    sql::execute(
        "UPDATE sync_state SET hash = $1, error = NULL, full_pending = true, synced_at = now()",
        &[SqlValue::Text(hash)],
    )?;
    sync_roles()
}

/// VIP 类型的等级共有的角色（bonus 的做种魔力公式用 `has_role("levels:vip")` 判断 VIP 加成）
const VIP_ROLE: &str = "levels:vip";

/// 每个等级一个角色并绑定到该等级（权限由管理员在 identity 中为角色配置）；VIP 类型的等级再绑定 [`VIP_ROLE`]
fn sync_roles() -> R<()> {
    call("ctx.rbac", "define_role", &json!({ "role": VIP_ROLE }))?;
    for c in classes()? {
        let role = format!("levels:class_{}", c.id);
        call("ctx.rbac", "define_role", &json!({ "role": role }))?;
        let roles = if c.kind == "vip" { json!([role, VIP_ROLE]) } else { json!([role]) };
        call(
            "ctx.rbac",
            "set_class_roles",
            &json!({ "class_id": c.id, "roles": roles }),
        )?;
    }
    Ok(())
}

const GIB: f64 = 1_073_741_824.0;
/// 做种积分（bonus 插件的 `seed_points` 币种，1 位小数）的最小单位换算
const SEED_POINTS_UNIT: f64 = 10.0;

#[derive(Debug, Clone)]
struct Class {
    id: i64,
    key: String,
    rank: i64,
    kind: String,
    promote: Vec<Value>,
    /// 空 = 不因条件降级
    keep: Vec<Value>,
    can_invite: bool,
    protected: bool,
    is_default: bool,
    name: String,
    /// 设置中的简写，空时用 [`preset_abbr`]
    abbr: String,
    description: String,
    color: String,
}

impl Class {
    fn abbr(&self) -> &str {
        if self.abbr.is_empty() { preset_abbr(&self.key) } else { &self.abbr }
    }

    /// 对外表示：name / description 为空时前端用 name_key / desc_key 的内置术语；abbr 为空表示没有简写
    fn public(&self) -> Value {
        json!({ "id": self.id, "key": self.key, "rank": self.rank, "type": self.kind,
                "name": self.name, "name_key": format!("levels.class.{}.name", self.key), "abbr": self.abbr(),
                "description": self.description, "desc_key": format!("levels.class.{}.desc", self.key),
                "color": (!self.color.is_empty()).then_some(&self.color),
                "promote": self.promote, "keep": self.keep, "can_invite": self.can_invite,
                "protected": self.protected, "is_default": self.is_default })
    }

    fn brief(&self) -> Value {
        json!({ "id": self.id, "key": self.key, "type": self.kind, "name": self.name,
                "name_key": format!("levels.class.{}.name", self.key), "abbr": self.abbr(),
                "color": (!self.color.is_empty()).then_some(&self.color) })
    }
}

fn classes() -> R<Vec<Class>> {
    let r = sql::query(
        "SELECT id, slug, rank, type, promote::text, keep::text, can_invite, protected, is_default, name, description, coalesce(color, ''), abbr
           FROM classes WHERE active ORDER BY rank, id",
        &[],
    )?;
    let list = |v: &SqlValue| -> Vec<Value> {
        match v {
            SqlValue::Text(t) => serde_json::from_str(t).unwrap_or_default(),
            _ => vec![],
        }
    };
    Ok(r.rows
        .iter()
        .map(|row| Class {
            id: int(&row[0]),
            key: text(&row[1]),
            rank: int(&row[2]),
            kind: text(&row[3]),
            promote: list(&row[4]),
            keep: list(&row[5]),
            can_invite: matches!(row[6], SqlValue::Boolean(true)),
            protected: matches!(row[7], SqlValue::Boolean(true)),
            is_default: matches!(row[8], SqlValue::Boolean(true)),
            name: text(&row[9]),
            description: text(&row[10]),
            color: text(&row[11]),
            abbr: text(&row[12]),
        })
        .collect())
}

// ---------------- 指标与谓词（03 §1.2） ----------------

/// 用户指标（GiB、小时）；ratio 在下载为 0 时为 +∞
type Metrics = BTreeMap<String, f64>;

fn metrics_of(ids: &[i64]) -> R<BTreeMap<i64, Metrics>> {
    let r = sql::query(
        "SELECT u.id, extract(epoch FROM now() - u.created_at)::float8 / 86400,
                coalesce(t.uploaded, 0), coalesce(t.downloaded, 0), coalesce(t.seed_secs, 0),
                coalesce(t.completed_count, 0), coalesce(t.seeding_size, 0), coalesce(c.approved_count, 0),
                coalesce(b.balance, 0)::float8
           FROM identity.v_user_public u
           LEFT JOIN accounting.v_user_totals t ON t.user_id = u.id
           LEFT JOIN torrent.v_uploader_counts c ON c.user_id = u.id
           LEFT JOIN ledger.v_balance b ON b.user_id = u.id AND b.currency = 'seed_points'
          WHERE u.id = ANY($1) AND u.status <> 'deleted'",
        &[SqlValue::IntArray(ids.to_vec())],
    )?;
    Ok(r.rows
        .iter()
        .map(|row| {
            let (up, down) = (num(&row[2]), num(&row[3]));
            let seed_hours = num(&row[4]) / 3600.0;
            let mut m = Metrics::new();
            m.insert("account_age_days".into(), num(&row[1]).floor());
            m.insert("uploaded_gib".into(), up / GIB);
            m.insert("downloaded_gib".into(), down / GIB);
            m.insert("ratio".into(), if down > 0.0 { up / down } else { f64::INFINITY });
            m.insert("seed_hours".into(), seed_hours);
            m.insert("avg_seed_hours".into(), seed_hours / num(&row[5]).max(1.0));
            m.insert("seeding_size_gib".into(), num(&row[6]) / GIB);
            m.insert("uploads_count".into(), num(&row[7]));
            m.insert("seed_points".into(), num(&row[8]) / SEED_POINTS_UNIT);
            (int(&row[0]), m)
        })
        .collect())
}

fn holds(m: &Metrics, p: &Value) -> bool {
    let Some(cur) = p["metric"].as_str().and_then(|f| m.get(f)).copied() else {
        return false;
    };
    let Some(v) = p["value"].as_f64() else {
        return false;
    };
    match p["op"].as_str() {
        Some("gte") => cur >= v,
        Some("gt") => cur > v,
        Some("lte") => cur <= v,
        Some("lt") => cur < v,
        _ => false,
    }
}

fn all(m: &Metrics, ps: &[Value]) -> bool {
    ps.iter().all(|p| holds(m, p))
}

fn fmt_metric(field: &str, v: f64) -> String {
    if v.is_infinite() {
        "inf".into()
    } else if matches!(field, "account_age_days" | "uploads_count" | "seed_points") {
        format!("{}", v.floor() as i64)
    } else if field.ends_with("_hours") {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}

fn progress(m: &Metrics, ps: &[Value]) -> Vec<Value> {
    ps.iter()
        .map(|p| {
            let f = p["metric"].as_str().unwrap_or("");
            json!({ "metric": f, "op": p["op"], "target": fmt_metric(f, p["value"].as_f64().unwrap_or(0.0)),
                    "current": fmt_metric(f, m.get(f).copied().unwrap_or(0.0)), "met": holds(m, p) })
        })
        .collect()
}

// ---------------- 评估（HLD §5.1、03 §1.3） ----------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Full,
    PromoteOnly,
}

struct State {
    class_id: i64,
    base_class_id: i64,
    protected: bool,
    warning_deadline_passed: Option<bool>,
}

fn ensure_row(user: i64, cls: &[Class]) -> R<State> {
    let default = cls.iter().find(|c| c.is_default).map_or(0, |c| c.id);
    sql::execute(
        "INSERT INTO user_levels (user_id, class_id, base_class_id, source) VALUES ($1, $2, $2, 'default') ON CONFLICT DO NOTHING",
        &[SqlValue::Int(user), SqlValue::Int(default)],
    )?;
    let r = sql::query(
        "SELECT class_id, base_class_id,
                (coalesce(protect_until, '-infinity') > now() OR coalesce(protected_until, '-infinity') > now()),
                CASE WHEN warning_deadline IS NULL THEN NULL ELSE warning_deadline <= now() END
           FROM user_levels WHERE user_id = $1 FOR UPDATE",
        &[SqlValue::Int(user)],
    )?;
    let row = &r.rows[0];
    Ok(State {
        class_id: int(&row[0]),
        base_class_id: int(&row[1]),
        protected: matches!(row[2], SqlValue::Boolean(true)),
        warning_deadline_passed: match row[3] {
            SqlValue::Boolean(b) => Some(b),
            _ => None,
        },
    })
}

fn active_override(user: i64) -> R<Option<i64>> {
    let r = sql::query(
        "SELECT o.class_id FROM overrides o JOIN classes c ON c.id = o.class_id AND c.active
          WHERE o.user_id = $1 AND o.ended_at IS NULL AND (o.until IS NULL OR o.until > now())
          ORDER BY CASE o.kind WHEN 'manual' THEN 300 WHEN 'honor' THEN 200 ELSE 100 END DESC, o.id DESC LIMIT 1",
        &[SqlValue::Int(user)],
    )?;
    Ok(r.rows.first().map(|row| int(&row[0])))
}

/// 设置等级（经 identity），写历史与事件
fn apply(user: i64, from: i64, to: i64, base: i64, source: &str, cause: &str) -> R<()> {
    call(
        "ctx.users",
        "set_class",
        &json!({ "id": user, "class_id": to, "reason": format!("levels.{cause}") }),
    )?;
    sql::execute(
        "UPDATE user_levels SET class_id = $2, base_class_id = $3, source = $4, evaluated_at = now(), updated_at = now() WHERE user_id = $1",
        &[
            SqlValue::Int(user),
            SqlValue::Int(to),
            SqlValue::Int(base),
            SqlValue::Text(source.into()),
        ],
    )?;
    if from != to {
        sql::execute(
            "INSERT INTO history (user_id, from_class, to_class, cause, actor) VALUES ($1, $2, $3, $4, 'levels')",
            &[
                SqlValue::Int(user),
                SqlValue::Int(from),
                SqlValue::Int(to),
                SqlValue::Text(cause.into()),
            ],
        )?;
    }
    Ok(())
}

fn evaluate(user: i64, m: &Metrics, mode: Mode, cls: &[Class], o: Opts) -> R<()> {
    let st = ensure_row(user, cls)?;
    let normal: Vec<&Class> = cls.iter().filter(|c| c.kind == "normal").collect();
    if let Some(ov) = active_override(user)? {
        if st.class_id != ov {
            apply(user, st.class_id, ov, st.base_class_id, "manual", "override")?;
        }
        return Ok(());
    }
    let base = normal.iter().find(|c| c.id == st.base_class_id).copied();
    let cur = base.or_else(|| normal.iter().find(|c| c.is_default).copied());
    let Some(cur) = cur else { return Ok(()) };
    // 覆盖到期：回到基线等级；基线等级已从设置中删除：回到默认等级
    if st.class_id != cur.id || st.base_class_id != cur.id {
        let cause = if base.is_some() { "override_expired" } else { "class_removed" };
        apply(user, st.class_id, cur.id, cur.id, "auto", cause)?;
    }
    let one_step = o.one_step;
    let mut cands: Vec<&Class> = normal
        .iter()
        .filter(|c| c.rank > cur.rank && !c.promote.is_empty() && all(m, &c.promote))
        .copied()
        .collect();
    if one_step {
        let next_rank = normal.iter().filter(|c| c.rank > cur.rank).map(|c| c.rank).min();
        cands.retain(|c| Some(c.rank) == next_rank);
    }
    if let Some(target) = cands.iter().max_by_key(|c| c.rank) {
        let protect_days = o.protect_days;
        apply(user, cur.id, target.id, target.id, "auto", "promoted")?;
        sql::execute(
            "UPDATE user_levels SET protect_until = now() + make_interval(days => $2::int), warning_class_id = NULL,
                    warning_deadline = NULL, warning_failed = NULL WHERE user_id = $1",
            &[SqlValue::Int(user), SqlValue::Int(protect_days)],
        )?;
        publish(
            "level.promoted",
            user,
            json!({ "user_id": user, "from_class": cur.id, "to_class": target.id, "cause": "auto" }),
        )?;
        return Ok(());
    }
    if mode == Mode::PromoteOnly || st.protected || cur.protected {
        return Ok(());
    }
    if all(m, &cur.keep) {
        if st.warning_deadline_passed.is_some() {
            sql::execute(
                "UPDATE user_levels SET warning_class_id = NULL, warning_deadline = NULL, warning_failed = NULL WHERE user_id = $1",
                &[SqlValue::Int(user)],
            )?;
            publish(
                "level.warning_cleared",
                user,
                json!({ "user_id": user, "class_id": cur.id }),
            )?;
        }
        return Ok(());
    }
    let failed: Vec<Value> = cur.keep.iter().filter(|p| !holds(m, p)).cloned().collect();
    match st.warning_deadline_passed {
        None => {
            let days = o.warning_days;
            let r = sql::query(
                "UPDATE user_levels SET warning_class_id = $2, warning_deadline = now() + make_interval(days => $3::int), warning_failed = $4
                  WHERE user_id = $1 RETURNING (extract(epoch FROM warning_deadline) * 1000)::bigint",
                &[SqlValue::Int(user), SqlValue::Int(cur.id), SqlValue::Int(days), SqlValue::Json(json!(failed).to_string())],
            )?;
            publish(
                "level.warned",
                user,
                json!({ "user_id": user, "class_id": cur.id, "deadline": int(&r.rows[0][0]),
                                                  "failed_conditions": failed }),
            )?;
        }
        Some(false) => {}
        Some(true) => {
            // 降到较低等级中保级条件满足的最高者，没有则最低等级
            let target = normal
                .iter()
                .filter(|c| c.rank < cur.rank && all(m, &c.keep))
                .max_by_key(|c| c.rank)
                .or_else(|| normal.first())
                .copied()
                .unwrap_or(cur);
            apply(user, cur.id, target.id, target.id, "auto", "demoted")?;
            sql::execute(
                "UPDATE user_levels SET warning_class_id = NULL, warning_deadline = NULL, warning_failed = NULL WHERE user_id = $1",
                &[SqlValue::Int(user)],
            )?;
            publish(
                "level.demoted",
                user,
                json!({ "user_id": user, "from_class": cur.id, "to_class": target.id, "reason": "keep_failed" }),
            )?;
        }
    }
    Ok(())
}

fn evaluate_batch(ids: &[i64], mode: Mode) -> R<usize> {
    let (cls, o) = (classes()?, opts());
    let ms = metrics_of(ids)?;
    let mut sorted = ids.to_vec();
    sorted.sort_unstable();
    for uid in &sorted {
        if let Some(m) = ms.get(uid) {
            evaluate(*uid, m, mode, &cls, o)?;
        }
    }
    Ok(sorted.len())
}

/// 全量评估（键集分页，每批提交）：每日任务，以及等级定义变化后的一次补评
fn full_evaluate() -> R<()> {
    let mut last = 0i64;
    loop {
        let r = sql::query(
            "SELECT id FROM identity.v_user_public WHERE id > $1 AND status <> 'deleted' ORDER BY id LIMIT 2000",
            &[SqlValue::Int(last)],
        )?;
        let ids: Vec<i64> = r.rows.iter().map(|row| int(&row[0])).collect();
        let Some(max) = ids.last().copied() else {
            return Ok(());
        };
        evaluate_batch(&ids, Mode::Full)?;
        sql::commit()?;
        last = max;
    }
}

/// VIP 授予（捐赠、商城、抽奖等）：在当前有效的 VIP 覆盖之后顺延 days 天；`permanent: true` 授予永久 VIP（覆盖无到期时间），
/// 已是永久 VIP 时按天授予不再生效。source_ref 幂等（重复调用返回同一结果）
fn grant_vip(user: i64, a: &Value) -> R<Value> {
    let permanent = a["permanent"].as_bool().unwrap_or(false);
    let days = if permanent {
        0
    } else {
        a["days"].as_i64().filter(|d| (1..=3650).contains(d))
            .ok_or_else(|| err("kernel.validation_failed", "days must be 1..=3650"))?
    };
    let source_ref = a["source_ref"].as_str().filter(|s| !s.is_empty() && s.len() <= 200)
        .ok_or_else(|| err("kernel.validation_failed", "source_ref is required"))?;
    if user <= 0 {
        return Err(err("kernel.validation_failed", "user is required"));
    }
    let done = sql::query(
        "SELECT (extract(epoch FROM until) * 1000)::bigint, until IS NULL FROM overrides WHERE source_ref = $1",
        &[SqlValue::Text(source_ref.into())],
    )?;
    if let Some(row) = done.rows.first() {
        let until = match row[0] { SqlValue::Int(v) => Some(v), _ => None };
        return Ok(json!({ "until_ms": until, "permanent": row.bool(1).unwrap_or(false), "duplicate": true }));
    }
    let cls = classes()?;
    let vip = cls
        .iter()
        .find(|c| c.kind == "vip" && c.key == "vip")
        .or_else(|| cls.iter().find(|c| c.kind == "vip"))
        .ok_or_else(|| err("levels.class_not_found", "no VIP class configured"))?
        .id;
    ensure_row(user, &cls)?;
    let has_permanent = !sql::query(
        "SELECT 1 FROM overrides WHERE user_id = $1 AND kind = 'vip' AND ended_at IS NULL AND until IS NULL",
        &[SqlValue::Int(user)],
    )?
    .rows
    .is_empty();
    if has_permanent {
        return Ok(json!({ "until_ms": null, "permanent": true, "duplicate": false, "already_permanent": true }));
    }
    let prev = sql::query(
        "UPDATE overrides SET ended_at = now() WHERE user_id = $1 AND kind = 'vip' AND ended_at IS NULL AND until > now()
          RETURNING (extract(epoch FROM until) * 1000)::bigint",
        &[SqlValue::Int(user)],
    )?;
    let start = prev.rows.iter().map(|row| int(&row[0])).max();
    let r = sql::query(
        "INSERT INTO overrides (user_id, kind, class_id, until, reason, created_by, source_ref)
         VALUES ($1, 'vip', $2, CASE WHEN $8 THEN NULL ELSE coalesce(to_timestamp($3::float8 / 1000), now()) + make_interval(days => $4::int) END, $5, $6, $7)
         RETURNING (extract(epoch FROM until) * 1000)::bigint",
        &[
            SqlValue::Int(user),
            SqlValue::Int(vip),
            start.map_or(SqlValue::Null, SqlValue::Int),
            SqlValue::Int(days),
            SqlValue::Text(a["reason"].as_str().unwrap_or("").chars().take(200).collect()),
            SqlValue::Text(format!("plugin:{}", invocation::current().caller_plugin)),
            SqlValue::Text(source_ref.into()),
            SqlValue::Boolean(permanent),
        ],
    )?;
    evaluate_batch(&[user], Mode::Full)?;
    let until = r.rows.first().and_then(|row| match row[0] { SqlValue::Int(v) => Some(v), _ => None });
    Ok(json!({ "until_ms": until, "permanent": permanent, "duplicate": false }))
}

/// 已到期但未结束的覆盖：结束并重新评估这些用户（回到基线等级），由 incr_evaluate 执行
fn end_expired_overrides() -> R<()> {
    let r = sql::query(
        "UPDATE overrides SET ended_at = now() WHERE ended_at IS NULL AND until <= now() RETURNING user_id",
        &[],
    )?;
    let mut ids: Vec<i64> = r.rows.iter().map(|row| int(&row[0])).collect();
    ids.dedup();
    if !ids.is_empty() {
        evaluate_batch(&ids, Mode::Full)?;
    }
    sql::commit()
}

fn level_info(user: i64) -> R<Value> {
    let cls = classes()?;
    let st = ensure_row(user, &cls)?;
    let m = metrics_of(&[user])?.remove(&user).unwrap_or_default();
    let cur = cls.iter().find(|c| c.id == st.base_class_id);
    let next = cur.and_then(|c| {
        cls.iter()
            .filter(|n| n.kind == "normal" && n.rank > c.rank && !n.promote.is_empty())
            .min_by_key(|n| n.rank)
    });
    let w = sql::query(
        "SELECT (extract(epoch FROM warning_deadline) * 1000)::bigint, source FROM user_levels WHERE user_id = $1",
        &[SqlValue::Int(user)],
    )?;
    let src = w.rows.first().map(|r| text(&r[1])).unwrap_or_default();
    let mut class = cls
        .iter()
        .find(|c| c.id == st.class_id)
        .map_or_else(|| json!({ "id": st.class_id }), Class::brief);
    class["source"] = json!(src);
    // 当前等级来自覆盖（VIP、手动锁定等）时带上到期时间（null 为永久）
    let o = sql::query(
        "SELECT (extract(epoch FROM until) * 1000)::bigint FROM overrides
          WHERE user_id = $1 AND class_id = $2 AND ended_at IS NULL AND (until IS NULL OR until > now())
          ORDER BY until IS NULL DESC, until DESC LIMIT 1",
        &[SqlValue::Int(user), SqlValue::Int(st.class_id)],
    )?;
    if let Some(r) = o.rows.first() {
        class["until"] = match r[0] { SqlValue::Int(t) => json!(t), _ => Value::Null };
    }
    Ok(json!({
        "class": class,
        "next": next.map(|n| { let mut b = n.brief(); b["progress"] = json!(progress(&m, &n.promote)); b }),
        "warning": w.rows.first().and_then(|r| match r[0] { SqlValue::Int(d) => Some(json!({ "deadline": d })), _ => None }),
        "protected": st.protected || cls.iter().any(|c| c.id == st.class_id && c.protected),
    }))
}

// ---------------- 导出 ----------------

impl lifecycle::Guest for Levels {
    fn init() -> R<()> {
        // 等级定义来自插件设置（未设置时用 NexusPHP 预设）；设置未变化时 sync 不做事，角色照常对齐
        sync_classes()?;
        sync_roles()
    }
}

impl hooks::Guest for Levels {
    fn handle(c: hooks::HookCall) -> R<hooks::HookDecision> {
        if c.hook == "invite.before_issue" {
            let input: Value = serde_json::from_str(&c.input).unwrap_or(Value::Null);
            let inviter = input["inviter_id"].as_i64().unwrap_or(0);
            let r = sql::query(
                "SELECT c.can_invite FROM user_levels u JOIN classes c ON c.id = u.class_id WHERE u.user_id = $1",
                &[SqlValue::Int(inviter)],
            )?;
            if matches!(r.rows.first().map(|row| &row[0]), Some(SqlValue::Boolean(false))) {
                return Ok(hooks::HookDecision::Deny(hooks::DenyInfo {
                    code: "levels.class_required".into(),
                    message: "your class cannot send invites".into(),
                    details: None,
                }));
            }
        }
        Ok(hooks::HookDecision::Allow)
    }
}

impl on_event::Guest for Levels {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        let p: Value =
            serde_json::from_str(&evt.payload).map_err(|_| err("levels.bad_event", "invalid payload"))?;
        let uid = p["user_id"].as_i64().unwrap_or(0);
        match evt.event_type.as_str() {
            // 新用户：写入默认等级并同步到 identity
            "user.registered" => {
                sync_classes()?;
                let cls = classes()?;
                let st = ensure_row(uid, &cls)?;
                if p["class_id"].as_i64() != Some(st.class_id) {
                    call(
                        "ctx.users",
                        "set_class",
                        &json!({ "id": uid, "class_id": st.class_id, "reason": "levels.default" }),
                    )?;
                }
            }
            "traffic.settled" => {
                sql::execute(
                    "INSERT INTO dirty_users (user_id, since) VALUES ($1, now()) ON CONFLICT DO NOTHING",
                    &[SqlValue::Int(uid)],
                )?;
            }
            "user.logged_in" => {
                sql::execute(
                    "UPDATE user_levels SET last_active_day = current_date WHERE user_id = $1 AND last_active_day IS DISTINCT FROM current_date",
                    &[SqlValue::Int(uid)],
                )?;
            }
            _ => {}
        }
        Ok(())
    }
}

impl on_cron::Guest for Levels {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        sync_classes()?;
        sql::commit()?;
        match job.name.as_str() {
            // 增量评估：只升级（03 §1.4）；等级定义变化后先补一次全量评估
            "incr_evaluate" => {
                let pending = sql::query(
                    "UPDATE sync_state SET full_pending = false WHERE full_pending RETURNING 1",
                    &[],
                )?;
                if !pending.rows.is_empty() {
                    sql::commit()?;
                    full_evaluate()?;
                }
                end_expired_overrides()?;
                loop {
                let r = sql::query(
                    "DELETE FROM dirty_users WHERE user_id IN (SELECT user_id FROM dirty_users ORDER BY since LIMIT 2000 FOR UPDATE SKIP LOCKED)
                     RETURNING user_id",
                    &[],
                )?;
                let ids: Vec<i64> = r.rows.iter().map(|row| int(&row[0])).collect();
                if ids.is_empty() {
                    return Ok(());
                }
                evaluate_batch(&ids, Mode::PromoteOnly)?;
                sql::commit()?;
                }
            }
            // 每日全量评估（键集分页，每批提交）
            "daily_evaluate" => {
                sql::execute("UPDATE sync_state SET full_pending = false WHERE full_pending", &[])?;
                full_evaluate()
            }
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

fn json_response(status: u16, v: &Value) -> http_handler::Response {
    http_handler::Response {
        status,
        headers: vec![http_handler::Header {
            name: "content-type".into(),
            value: "application/json".into(),
        }],
        body: v.to_string().into_bytes(),
    }
}

fn param(req: &http_handler::Request, k: &str) -> R<i64> {
    req.params
        .iter()
        .find(|(n, _)| n == k)
        .and_then(|(_, v)| v.parse().ok())
        .ok_or_else(|| err("kernel.bad_request", "bad path parameter"))
}

fn me() -> R<i64> {
    invocation::current()
        .actor
        .user_id
        .and_then(|u| i64::try_from(u).ok())
        .ok_or_else(|| err("kernel.unauthenticated", "login required"))
}

impl http_handler::Guest for Levels {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let body = || -> R<Value> {
            serde_json::from_slice(&req.body).map_err(|_| err("kernel.bad_request", "invalid JSON body"))
        };
        // 设置可能刚在后台改过：先同步（未变化时只读一行）
        sync_classes()?;
        let v = match req.route_id.as_str() {
            "me" => level_info(me()?)?,
            "classes" => json!({ "items": classes()?.iter().map(Class::public).collect::<Vec<_>>() }),
            "admin_classes" => {
                let st = sql::query(
                    "SELECT error, (extract(epoch FROM synced_at) * 1000)::bigint FROM sync_state",
                    &[],
                )?;
                let row = st.rows.first();
                json!({ "items": classes()?.iter().map(Class::public).collect::<Vec<_>>(),
                        "settings_error": row.and_then(|r| match &r[0] { SqlValue::Text(t) => Some(t.clone()), _ => None }),
                        "synced_at": row.map(|r| int(&r[1])) })
            }
            "admin_user" => {
                let uid = param(&req, "user_id")?;
                let mut v = level_info(uid)?;
                let o = sql::query(
                    "SELECT o.id, o.kind, o.class_id, (extract(epoch FROM o.until) * 1000)::bigint, o.reason, o.created_by,
                            (extract(epoch FROM o.created_at) * 1000)::bigint
                       FROM overrides o WHERE o.user_id = $1 AND o.ended_at IS NULL AND (o.until IS NULL OR o.until > now()) ORDER BY o.id DESC",
                    &[SqlValue::Int(uid)],
                )?;
                v["overrides"] = json!(o.rows.iter().map(|r| json!({ "id": int(&r[0]), "kind": text(&r[1]), "class_id": int(&r[2]),
                    "until": match r[3] { SqlValue::Int(t) => Some(t), _ => None }, "reason": text(&r[4]), "created_by": text(&r[5]),
                    "created_at": int(&r[6]) })).collect::<Vec<_>>());
                let p = sql::query(
                    "SELECT (extract(epoch FROM protected_until) * 1000)::bigint FROM user_levels WHERE user_id = $1 AND protected_until > now()",
                    &[SqlValue::Int(uid)],
                )?;
                v["protected_until"] = json!(p.rows.first().and_then(|r| match r[0] { SqlValue::Int(t) => Some(t), _ => None }));
                v
            }
            "set_user_class" => {
                let (uid, b) = (param(&req, "user_id")?, body()?);
                // 等级以 key（如 "vip"）或数字 id 指定
                let cls = classes()?;
                let found = match (b["class"].as_str(), b["class_id"].as_i64()) {
                    (Some(k), _) => cls.iter().find(|c| c.key == k),
                    (None, Some(id)) => cls.iter().find(|c| c.id == id),
                    _ => return Err(err("kernel.validation_failed", "class (key) or class_id is required")),
                };
                let class = found.ok_or_else(|| err("levels.class_not_found", "class not found"))?.id;
                let until = b["lock_until_ms"].as_i64();
                sql::execute(
                    "UPDATE overrides SET ended_at = now() WHERE user_id = $1 AND kind = 'manual' AND ended_at IS NULL",
                    &[SqlValue::Int(uid)],
                )?;
                sql::execute(
                    "INSERT INTO overrides (user_id, kind, class_id, until, reason, created_by) VALUES ($1, 'manual', $2, to_timestamp($3::float8 / 1000), $4, $5)",
                    &[
                        SqlValue::Int(uid),
                        SqlValue::Int(class),
                        until.map_or(SqlValue::Null, SqlValue::Int),
                        SqlValue::Text(b["reason"].as_str().unwrap_or("").into()),
                        SqlValue::Text(format!("user:{}", me()?)),
                    ],
                )?;
                evaluate_batch(&[uid], Mode::Full)?;
                return Ok(http_handler::Response {
                    status: 204,
                    headers: vec![],
                    body: vec![],
                });
            }
            "unlock_user_class" => {
                let uid = param(&req, "user_id")?;
                sql::execute(
                    "UPDATE overrides SET ended_at = now() WHERE user_id = $1 AND kind = 'manual' AND ended_at IS NULL",
                    &[SqlValue::Int(uid)],
                )?;
                evaluate_batch(&[uid], Mode::Full)?;
                return Ok(http_handler::Response {
                    status: 204,
                    headers: vec![],
                    body: vec![],
                });
            }
            "protect_user" => {
                let (uid, b) = (param(&req, "user_id")?, body()?);
                let cls = classes()?;
                ensure_row(uid, &cls)?;
                sql::execute(
                    "UPDATE user_levels SET protected_until = to_timestamp($2::float8 / 1000) WHERE user_id = $1",
                    &[
                        SqlValue::Int(uid),
                        b["until_ms"].as_i64().map_or(SqlValue::Null, SqlValue::Int),
                    ],
                )?;
                return Ok(http_handler::Response {
                    status: 204,
                    headers: vec![],
                    body: vec![],
                });
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(json_response(200, &v))
    }
}

impl service_provider::Guest for Levels {
    fn call(service: String, method: String, args: String) -> R<String> {
        if service != "ctx.levels" {
            return Err(err("kernel.not_found", "unknown service"));
        }
        let a: Value =
            serde_json::from_str(&args).map_err(|_| err("kernel.bad_request", "invalid JSON arguments"))?;
        let user = a["user"].as_i64().unwrap_or(0);
        let inv = invocation::current();
        sync_classes()?;
        let out = match method.as_str() {
            "level_of" => level_info(user)?,
            "classes" => json!(classes()?.iter().map(Class::public).collect::<Vec<_>>()),
            "evaluate" => {
                if !inv.caller_scopes.iter().any(|s| s == "evaluate") {
                    return Err(err("levels.scope_required", "scope `evaluate` required"));
                }
                evaluate_batch(&[user], Mode::Full)?;
                level_info(user)?
            }
            "is_protected" => level_info(user)?["protected"].clone(),
            "vip_until" => {
                let r = sql::query(
                    "SELECT (extract(epoch FROM max(until) FILTER (WHERE until > now())) * 1000)::bigint, bool_or(until IS NULL) FROM overrides
                      WHERE user_id = $1 AND kind = 'vip' AND ended_at IS NULL AND (until IS NULL OR until > now())",
                    &[SqlValue::Int(user)],
                )?;
                let row = r.rows.first();
                let permanent = row.and_then(|row| row.bool(1)).unwrap_or(false);
                json!({ "until_ms": if permanent { None } else { row.and_then(|row| match row[0] { SqlValue::Int(v) => Some(v), _ => None }) }, "permanent": permanent })
            }
            "grant_vip" => {
                if !inv.caller_scopes.iter().any(|s| s == "grant") {
                    return Err(err("levels.scope_required", "scope `grant` required"));
                }
                grant_vip(user, &a)?
            }
            _ => return Err(err("kernel.not_found", "unknown method")),
        };
        Ok(out.to_string())
    }
}

// ---------------- MCP 工具（catalog/mcp-tools.md） ----------------

impl mcp_tool::Guest for Levels {
    fn call(tool: String, _arguments: String) -> R<String> {
        match tool.as_str() {
            "get_my_level" => {
                sync_classes()?;
                Ok(level_info(me()?)?.to_string())
            }
            _ => Err(err("kernel.not_found", "unknown tool")),
        }
    }
}

export!(Levels);

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pairs: &[(&str, f64)]) -> Metrics {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
    }

    #[test]
    fn preset_is_valid() {
        let defs = validate(&preset_classes()).unwrap();
        assert_eq!(defs.len(), 17);
        assert!(defs.iter().all(|d| preset_id(&d.key).is_some()));
        assert_eq!(defs.iter().filter(|d| d.is_default).map(|d| d.key.as_str()).collect::<Vec<_>>(), ["user"]);
        let pu = defs.iter().find(|d| d.key == "power_user").unwrap();
        let x = m(&[("ratio", f64::INFINITY), ("uploaded_gib", 60.0), ("account_age_days", 30.0)]);
        // 还需要做种积分（魔力值不参与升级）
        assert!(!all(&x, &pu.promote));
        let x = m(&[("ratio", f64::INFINITY), ("uploaded_gib", 60.0), ("account_age_days", 30.0), ("seed_points", 40_000.0)]);
        assert!(all(&x, &pu.promote));
        assert!(!all(&m(&[("ratio", 0.9)]), &pu.keep));
    }

    #[test]
    fn validation_rejects_bad_settings() {
        let ok = json!([{ "key": "user", "type": "normal", "is_default": true }]);
        assert!(validate(&ok).is_ok());
        for bad in [
            json!([{ "key": "User", "is_default": true }]),
            json!([{ "key": "a; DROP TABLE classes", "is_default": true }]),
            json!([{ "key": "user", "is_default": true }, { "key": "user" }]),
            json!([{ "key": "user" }]),
            json!([{ "key": "vip", "type": "vip", "is_default": true }]),
            json!([{ "key": "user", "is_default": true, "promote": [{ "metric": "x", "op": "gte", "value": 1 }] }]),
            json!([{ "key": "user", "is_default": true, "keep": [{ "metric": "ratio", "op": "gte", "value": -1 }] }]),
            json!([{ "key": "user", "is_default": true, "color": "red" }]),
            json!([]),
        ] {
            assert!(validate(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn predicates_and_progress() {
        let y = m(&[("ratio", 0.4)]);
        assert!(!holds(&y, &json!({"metric": "ratio", "op": "gte", "value": 0.5})));
        assert!(!holds(&y, &json!({"metric": "missing", "op": "gte", "value": 0})));
        let x = m(&[("ratio", f64::INFINITY), ("seed_hours", 12.345)]);
        let p = progress(
            &x,
            &[json!({"metric": "ratio", "op": "gte", "value": 1.05}), json!({"metric": "seed_hours", "op": "gte", "value": 24})],
        );
        assert_eq!(p[0]["current"], "inf");
        assert_eq!(p[0]["target"], "1.05");
        assert_eq!(p[1]["current"], "12.3");
        assert_eq!(digest("a"), digest("a"));
        assert_ne!(digest("a"), digest("b"));
    }
}
