//! 规则集 DSL（LLD §1.2）与评分（§1.4）。纯函数：输入证据、上下文、规则集与时间，输出分数与构成。
//!
//! 校验在 JSON 值上逐项进行（错误带 JSON Pointer 路径），通过后再反序列化为类型化的 [`Ruleset`]。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const MAX_RULES: usize = 200;
pub const MAX_BYTES: usize = 65_536;
const DAY_MS: f64 = 86_400_000.0;

// ---------------- 类型 ----------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sev {
    Low,
    Medium,
    High,
}

impl Sev {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Band {
    None,
    Low,
    Watch,
    High,
    Critical,
}

impl Band {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::None),
            "low" => Some(Self::Low),
            "watch" => Some(Self::Watch),
            "high" => Some(Self::High),
            "critical" => Some(Self::Critical),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Watch => "watch",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Enforce,
    Shadow,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HitsFn {
    Log2,
    Linear,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Defaults {
    pub window_days: f64,
    pub half_life_days: f64,
    pub hits_fn: HitsFn,
    pub hits_cap: f64,
    pub max_evidence: i64,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            window_days: 90.0,
            half_life_days: 14.0,
            hits_fn: HitsFn::Log2,
            hits_cap: 4.0,
            max_evidence: 2000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thresholds {
    pub low: f64,
    pub watch: f64,
    pub case: f64,
    pub critical: f64,
    pub transfer_block: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Match {
    pub kinds: Vec<String>,
    #[serde(default)]
    pub severity_gte: Option<Sev>,
    #[serde(default, rename = "where")]
    pub where_: Vec<Pred>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Measure {
    #[serde(rename = "fn")]
    pub func: String,
    pub window_days: f64,
    #[serde(rename = "match")]
    pub match_: Match,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pred {
    #[serde(default)]
    pub field: Option<String>,
    #[serde(default)]
    pub measure: Option<Measure>,
    pub op: String,
    #[serde(default)]
    pub value: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CtxMult {
    pub when: Vec<Pred>,
    pub mult: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvRule {
    pub id: String,
    #[serde(default)]
    pub mode: Mode,
    #[serde(rename = "match")]
    pub match_: Match,
    pub weight: f64,
    #[serde(default)]
    pub severity_mult: BTreeMap<String, f64>,
    #[serde(default)]
    pub hits_fn: Option<HitsFn>,
    #[serde(default)]
    pub hits_cap: Option<f64>,
    #[serde(default)]
    pub half_life_days: Option<f64>,
    #[serde(default)]
    pub context_mult: Vec<CtxMult>,
    #[serde(default)]
    pub per_torrent_cap: Option<f64>,
    pub cap: f64,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub gte: f64,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggRule {
    pub id: String,
    #[serde(default)]
    pub mode: Mode,
    #[serde(rename = "match")]
    pub match_: Match,
    pub window_days: f64,
    pub measure: String,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MulRule {
    pub id: String,
    #[serde(default)]
    pub mode: Mode,
    pub when: Vec<Pred>,
    pub factor: f64,
    #[serde(default)]
    pub applies_to_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Rule {
    Evidence(EvRule),
    Aggregate(AggRule),
    Multiplier(MulRule),
}

impl Rule {
    pub fn id(&self) -> &str {
        match self {
            Self::Evidence(r) => &r.id,
            Self::Aggregate(r) => &r.id,
            Self::Multiplier(r) => &r.id,
        }
    }
    pub fn mode(&self) -> Mode {
        match self {
            Self::Evidence(r) => r.mode,
            Self::Aggregate(r) => r.mode,
            Self::Multiplier(r) => r.mode,
        }
    }
    pub fn tags(&self) -> &[String] {
        match self {
            Self::Evidence(r) => &r.tags,
            Self::Aggregate(r) => &r.tags,
            Self::Multiplier(_) => &[],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PolicyWhen {
    #[serde(default)]
    pub band_gte: Option<Band>,
    #[serde(default)]
    pub score_gte: Option<f64>,
    #[serde(default)]
    pub rules_any: Vec<String>,
    #[serde(default)]
    pub tags_any: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionSpec {
    pub kind: String,
    #[serde(default)]
    pub days: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub id: String,
    pub when: PolicyWhen,
    pub actions: Vec<ActionSpec>,
    #[serde(default)]
    pub auto: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ruleset {
    pub schema: u32,
    #[serde(default)]
    pub defaults: Defaults,
    pub thresholds: Thresholds,
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub policies: Vec<Policy>,
}

impl Ruleset {
    /// 证据查询窗口：默认窗口与全部聚合 / 度量窗口的最大值
    pub fn max_window_days(&self) -> f64 {
        let mut w = self.defaults.window_days;
        let preds = |ps: &[Pred], w: &mut f64| {
            for p in ps {
                if let Some(m) = &p.measure {
                    *w = w.max(m.window_days);
                }
            }
        };
        for r in &self.rules {
            match r {
                Rule::Aggregate(a) => w = w.max(a.window_days),
                Rule::Multiplier(m) => preds(&m.when, &mut w),
                Rule::Evidence(e) => preds(&e.match_.where_, &mut w),
            }
        }
        w
    }

    pub fn band(&self, score: f64) -> Band {
        let t = &self.thresholds;
        if score >= t.critical {
            Band::Critical
        } else if score >= t.case {
            Band::High
        } else if score >= t.watch {
            Band::Watch
        } else if score >= t.low {
            Band::Low
        } else {
            Band::None
        }
    }

    pub fn rule(&self, id: &str) -> Option<&Rule> {
        self.rules.iter().find(|r| r.id() == id)
    }

    pub fn is_conclusive(&self, id: &str) -> bool {
        self.rule(id)
            .is_some_and(|r| r.tags().iter().any(|t| t == "conclusive"))
    }
}

/// 一条证据（评分输入）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub id: i64,
    pub kind: String,
    pub severity: Option<Sev>,
    pub hits: i64,
    pub torrent_id: Option<i64>,
    pub details: Value,
    pub occurred_at: i64,
}

/// 评分上下文（LLD §1.3）
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Ctx {
    pub account_age_days: i64,
    pub class_id: i64,
    pub is_protected: bool,
    pub confirmed_cases_365d: i64,
    pub warnings_90d: i64,
    /// 该用户登记为盒子的种子
    #[serde(default)]
    pub seedbox: BTreeSet<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BreakdownItem {
    pub rule: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub mode: Mode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub top_torrents: Vec<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub factor: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Score {
    pub score: i64,
    pub shadow_score: i64,
    pub band: Band,
    pub tags: Vec<String>,
    pub breakdown: Vec<BreakdownItem>,
    /// 贡献 > 0 的 enforce 规则
    pub rules_hit: Vec<String>,
    /// 贡献 > 0 的 shadow 规则
    pub shadow_hits: Vec<String>,
    /// 各证据规则 / 聚合规则匹配的证据 ID
    pub matched: BTreeMap<String, Vec<i64>>,
    pub evidence_used: usize,
    pub truncated: bool,
}

impl Score {
    /// 构成中贡献 ≥ 10% 的 enforce 规则（决定时记入统计）
    pub fn contributing(&self) -> Vec<String> {
        let total: f64 = self
            .breakdown
            .iter()
            .filter(|b| b.mode == Mode::Enforce)
            .filter_map(|b| b.score)
            .sum();
        self.breakdown
            .iter()
            .filter(|b| b.mode == Mode::Enforce && b.score.is_some_and(|s| total > 0.0 && s >= total * 0.1))
            .map(|b| b.rule.clone())
            .collect()
    }
}

// ---------------- 谓词 ----------------

fn path_value(path: &str, e: Option<&Evidence>, ctx: &Ctx) -> Option<Value> {
    if let Some(k) = path.strip_prefix("details.") {
        return e.and_then(|e| e.details.get(k)).cloned();
    }
    match path {
        "hits" => e.map(|e| json!(e.hits)),
        "severity" => e.map(|e| json!(e.severity.unwrap_or(Sev::Medium).as_str())),
        "torrent_id" => e.and_then(|e| e.torrent_id).map(|t| json!(t)),
        "ctx.account_age_days" => Some(json!(ctx.account_age_days)),
        "ctx.class_id" => Some(json!(ctx.class_id)),
        "ctx.is_protected" => Some(json!(ctx.is_protected)),
        "ctx.confirmed_cases_365d" => Some(json!(ctx.confirmed_cases_365d)),
        "ctx.warnings_90d" => Some(json!(ctx.warnings_90d)),
        "ctx.is_seedbox" => e.map(|e| json!(e.torrent_id.is_some_and(|t| ctx.seedbox.contains(&t)))),
        _ => None,
    }
}

/// 证据 details 中带 `seedbox: true`（产生时来自盒子网段）的种子
pub fn seedbox_from_evidence(ev: &[Evidence]) -> BTreeSet<i64> {
    ev.iter()
        .filter(|e| e.details["seedbox"] == json!(true))
        .filter_map(|e| e.torrent_id)
        .collect()
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
}

fn values_eq(a: &Value, b: &Value) -> bool {
    match (num(a), num(b)) {
        (Some(x), Some(y)) => (x - y).abs() < f64::EPSILON,
        _ => a == b,
    }
}

/// 比较目标：字面值，或同一证据的另一个数值字段 `{ref, mul}`
fn target(v: &Value, e: Option<&Evidence>, ctx: &Ctx) -> Option<Value> {
    if let Some(r) = v.get("ref").and_then(Value::as_str) {
        let base = num(&path_value(r, e, ctx)?)?;
        let mul = v.get("mul").and_then(Value::as_f64).unwrap_or(1.0);
        return Some(json!(base * mul));
    }
    Some(v.clone())
}

fn compare(op: &str, a: Option<&Value>, b: Option<&Value>) -> bool {
    if op == "exists" {
        let want = b.and_then(Value::as_bool).unwrap_or(true);
        return a.is_some_and(|v| !v.is_null()) == want;
    }
    let (Some(a), Some(b)) = (a, b) else {
        return false;
    };
    match op {
        "eq" => values_eq(a, b),
        "ne" => !values_eq(a, b),
        "in" => b.as_array().is_some_and(|l| l.iter().any(|x| values_eq(a, x))),
        "nin" => b.as_array().is_some_and(|l| !l.iter().any(|x| values_eq(a, x))),
        _ => match (num(a), num(b)) {
            (Some(x), Some(y)) => match op {
                "gt" => x > y,
                "gte" => x >= y,
                "lt" => x < y,
                "lte" => x <= y,
                _ => false,
            },
            _ => false,
        },
    }
}

fn matches(m: &Match, e: &Evidence, ctx: &Ctx, all: &[Evidence], now: i64) -> bool {
    m.kinds.contains(&e.kind)
        && m.severity_gte
            .is_none_or(|s| e.severity.unwrap_or(Sev::Medium) >= s)
        && m.where_.iter().all(|p| pred(p, Some(e), ctx, all, now))
}

fn pred(p: &Pred, e: Option<&Evidence>, ctx: &Ctx, all: &[Evidence], now: i64) -> bool {
    let a = match (&p.field, &p.measure) {
        (Some(f), _) => path_value(f, e, ctx),
        (None, Some(m)) => Some(json!(measure_value(
            &m.func,
            &window(all, m.window_days, now, |x| {
                matches(&m.match_, x, ctx, all, now)
            })
        ))),
        _ => None,
    };
    let b = target(&p.value, e, ctx);
    compare(&p.op, a.as_ref(), b.as_ref())
}

fn window(all: &[Evidence], days: f64, now: i64, f: impl Fn(&Evidence) -> bool) -> Vec<&Evidence> {
    let from = now as f64 - days * DAY_MS;
    all.iter()
        .filter(|e| e.occurred_at as f64 >= from && e.occurred_at <= now && f(e))
        .collect()
}

fn measure_value(func: &str, sel: &[&Evidence]) -> f64 {
    let field = |path: &str, e: &Evidence| -> Option<f64> {
        if path == "hits" {
            return Some(e.hits as f64);
        }
        path.strip_prefix("details.")
            .and_then(|k| e.details.get(k))
            .and_then(Value::as_f64)
    };
    match func {
        "count" => sel.len() as f64,
        "distinct_torrents" => sel
            .iter()
            .filter_map(|e| e.torrent_id)
            .collect::<HashSet<_>>()
            .len() as f64,
        "distinct_kinds" => sel.iter().map(|e| e.kind.as_str()).collect::<HashSet<_>>().len() as f64,
        f => {
            if let Some(p) = f.strip_prefix("sum:") {
                sel.iter().filter_map(|e| field(p, e)).sum()
            } else if let Some(p) = f.strip_prefix("max:") {
                sel.iter().filter_map(|e| field(p, e)).fold(0.0, f64::max)
            } else {
                0.0
            }
        }
    }
}

fn hitsf(f: HitsFn, cap: f64, hits: i64) -> f64 {
    let h = hits.max(1) as f64;
    let v = match f {
        HitsFn::Log2 => 1.0 + h.log2(),
        HitsFn::Linear => h,
        HitsFn::None => 1.0,
    };
    v.min(cap.max(1.0))
}

fn sev_mult(r: &EvRule, s: Option<Sev>) -> f64 {
    let s = s.unwrap_or(Sev::Medium);
    r.severity_mult.get(s.as_str()).copied().unwrap_or(match s {
        Sev::Low => 0.5,
        Sev::Medium => 1.0,
        Sev::High => 2.0,
    })
}

/// 四舍六入五成双
pub fn round_half_even(x: f64) -> i64 {
    let r = x.round();
    let v = if (x - x.trunc()).abs() == 0.5 && (r as i64) % 2 != 0 {
        r - x.signum()
    } else {
        r
    };
    v as i64
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

// ---------------- 评分 ----------------

/// 评分（LLD §1.4）。`ev` 须按 `occurred_at DESC, id DESC` 排列且已排除误判证据；`truncated` 由调用方给出
pub fn compute(rs: &Ruleset, ev: &[Evidence], ctx: &Ctx, now: i64, truncated: bool) -> Score {
    let mut contrib: HashMap<&str, f64> = HashMap::new();
    let mut per_t: HashMap<(&str, i64), f64> = HashMap::new();
    let mut matched: BTreeMap<String, Vec<i64>> = BTreeMap::new();
    let in_window =
        |e: &Evidence, days: f64| e.occurred_at as f64 >= now as f64 - days * DAY_MS && e.occurred_at <= now;
    for e in ev.iter().filter(|e| in_window(e, rs.defaults.window_days)) {
        for r in &rs.rules {
            let Rule::Evidence(r) = r else { continue };
            if r.mode == Mode::Off || !matches(&r.match_, e, ctx, ev, now) {
                continue;
            }
            let age_d = (now - e.occurred_at).max(0) as f64 / DAY_MS;
            let half = r.half_life_days.unwrap_or(rs.defaults.half_life_days).max(0.01);
            let cm: f64 = r
                .context_mult
                .iter()
                .filter(|m| m.when.iter().all(|p| pred(p, Some(e), ctx, ev, now)))
                .map(|m| m.mult)
                .product();
            let mut c = r.weight
                * sev_mult(r, e.severity)
                * hitsf(
                    r.hits_fn.unwrap_or(rs.defaults.hits_fn),
                    r.hits_cap.unwrap_or(rs.defaults.hits_cap),
                    e.hits,
                )
                * (-age_d / half).exp2()
                * cm;
            let k = (r.id.as_str(), e.torrent_id.unwrap_or(0));
            let pt_cap = r.per_torrent_cap.unwrap_or(r.cap);
            let used_t = per_t.get(&k).copied().unwrap_or(0.0);
            let used_r = contrib.get(r.id.as_str()).copied().unwrap_or(0.0);
            c = c.min(pt_cap - used_t).min(r.cap - used_r).max(0.0);
            *per_t.entry(k).or_insert(0.0) += c;
            *contrib.entry(r.id.as_str()).or_insert(0.0) += c;
            matched.entry(r.id.clone()).or_default().push(e.id);
        }
    }
    for r in &rs.rules {
        let Rule::Aggregate(a) = r else { continue };
        if a.mode == Mode::Off {
            continue;
        }
        let sel = window(ev, a.window_days, now, |e| matches(&a.match_, e, ctx, ev, now));
        let v = measure_value(&a.measure, &sel);
        let s = a
            .steps
            .iter()
            .filter(|s| v >= s.gte)
            .map(|s| s.score)
            .fold(0.0, f64::max);
        if s > 0.0 {
            matched
                .entry(a.id.clone())
                .or_default()
                .extend(sel.iter().map(|e| e.id));
        }
        contrib.insert(a.id.as_str(), s);
    }
    // 乘数：enforce 乘数作用于分数与影子分，shadow 乘数只作用于影子分
    let mut f_enf: HashMap<&str, f64> = HashMap::new();
    let mut f_sh: HashMap<&str, f64> = HashMap::new();
    let mut mult_items = Vec::new();
    for r in &rs.rules {
        let Rule::Multiplier(m) = r else { continue };
        if m.mode == Mode::Off || !m.when.iter().all(|p| pred(p, None, ctx, ev, now)) {
            continue;
        }
        for t in &rs.rules {
            if !contrib.contains_key(t.id()) {
                continue;
            }
            if m.applies_to_tags.is_empty() || t.tags().iter().any(|x| m.applies_to_tags.contains(x)) {
                let f = if m.mode == Mode::Enforce {
                    &mut f_enf
                } else {
                    &mut f_sh
                };
                *f.entry(t.id()).or_insert(1.0) *= m.factor;
            }
        }
        mult_items.push(BreakdownItem {
            rule: m.id.clone(),
            kind: "multiplier".into(),
            mode: m.mode,
            score: None,
            evidence: None,
            top_torrents: vec![],
            factor: Some(m.factor),
        });
    }
    let (mut enforce, mut shadow) = (0.0, 0.0);
    let mut breakdown = Vec::new();
    let mut tags = BTreeSet::new();
    let (mut rules_hit, mut shadow_hits) = (Vec::new(), Vec::new());
    for r in &rs.rules {
        if matches!(r, Rule::Multiplier(_)) || r.mode() == Mode::Off {
            continue;
        }
        let c = contrib.get(r.id()).copied().unwrap_or(0.0);
        if c <= 0.0 {
            continue;
        }
        let fe = f_enf.get(r.id()).copied().unwrap_or(1.0);
        let fs = f_sh.get(r.id()).copied().unwrap_or(1.0);
        if r.mode() == Mode::Enforce {
            enforce += c * fe;
            tags.extend(r.tags().iter().cloned());
            rules_hit.push(r.id().to_owned());
        } else {
            shadow_hits.push(r.id().to_owned());
        }
        shadow += c * fe * fs;
        // 种子贡献前三
        let mut tops: Vec<(i64, f64)> = per_t
            .iter()
            .filter(|((id, t), _)| *id == r.id() && *t > 0)
            .map(|((_, t), v)| (*t, *v))
            .collect();
        tops.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        breakdown.push(BreakdownItem {
            rule: r.id().to_owned(),
            kind: if matches!(r, Rule::Aggregate(_)) {
                "aggregate"
            } else {
                "evidence"
            }
            .into(),
            mode: r.mode(),
            score: Some(round2(c * fe)),
            evidence: Some(matched.get(r.id()).map_or(0, Vec::len)),
            top_torrents: tops.into_iter().take(3).map(|x| x.0).collect(),
            factor: None,
        });
    }
    breakdown.sort_by(|a, b| {
        b.score
            .unwrap_or(0.0)
            .total_cmp(&a.score.unwrap_or(0.0))
            .then(a.rule.cmp(&b.rule))
    });
    breakdown.extend(mult_items);
    let score = round_half_even(enforce).clamp(0, 100);
    let shadow_score = round_half_even(shadow).clamp(0, 100);
    matched.retain(|k, _| contrib.get(k.as_str()).is_some_and(|c| *c > 0.0));
    Score {
        score,
        shadow_score,
        band: rs.band(score as f64),
        tags: tags.into_iter().collect(),
        breakdown,
        rules_hit,
        shadow_hits,
        matched,
        evidence_used: ev.len(),
        truncated,
    }
}

/// 策略条件（全部给出的条件成立）
pub fn policy_applies(p: &Policy, s: &Score) -> bool {
    p.when.band_gte.is_none_or(|b| s.band >= b)
        && p.when.score_gte.is_none_or(|x| s.score as f64 >= x)
        && (p.when.rules_any.is_empty() || p.when.rules_any.iter().any(|r| s.rules_hit.contains(r)))
        && (p.when.tags_any.is_empty() || p.when.tags_any.iter().any(|t| s.tags.contains(t)))
}

// ---------------- 校验 ----------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VErr {
    pub path: String,
    pub code: String,
    pub message: String,
}

struct V {
    errs: Vec<VErr>,
}

impl V {
    fn push(&mut self, path: &str, code: &str, message: impl Into<String>) {
        if self.errs.len() < 100 {
            self.errs.push(VErr {
                path: path.into(),
                code: format!("anticheat.{code}"),
                message: message.into(),
            });
        }
    }

    fn num(&mut self, o: &Value, path: &str, key: &str, lo: f64, hi: f64, required: bool) -> Option<f64> {
        let p = format!("{path}/{key}");
        match o.get(key) {
            None | Some(Value::Null) => {
                if required {
                    self.push(&p, "value_out_of_range", format!("`{key}` is required"));
                }
                None
            }
            Some(v) => match v.as_f64() {
                Some(x) if x >= lo && x <= hi => Some(x),
                _ => {
                    self.push(
                        &p,
                        "value_out_of_range",
                        format!("`{key}` must be a number in {lo}..={hi}"),
                    );
                    None
                }
            },
        }
    }

    fn enum_str(
        &mut self,
        o: &Value,
        path: &str,
        key: &str,
        allowed: &[&str],
        required: bool,
    ) -> Option<String> {
        let p = format!("{path}/{key}");
        match o.get(key) {
            None | Some(Value::Null) => {
                if required {
                    self.push(&p, "value_out_of_range", format!("`{key}` is required"));
                }
                None
            }
            Some(Value::String(s)) if allowed.contains(&s.as_str()) => Some(s.clone()),
            Some(_) => {
                self.push(
                    &p,
                    "value_out_of_range",
                    format!("`{key}` must be one of {allowed:?}"),
                );
                None
            }
        }
    }
}

fn ident_ok(s: &str) -> bool {
    let b = s.as_bytes();
    (2..=48).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

fn tag_ok(s: &str) -> bool {
    (2..=32).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_lowercase() || c == b'_')
}

fn detail_key_ok(k: &str) -> bool {
    (1..=32).contains(&k.len()) && k.bytes().all(|c| c.is_ascii_lowercase() || c == b'_')
}

const CTX_FIELDS: &[&str] = &[
    "ctx.account_age_days",
    "ctx.class_id",
    "ctx.is_protected",
    "ctx.confirmed_cases_365d",
    "ctx.warnings_90d",
    "ctx.is_seedbox",
];
const OPS: &[&str] = &["eq", "ne", "gt", "gte", "lt", "lte", "in", "nin", "exists"];
const MODES: &[&str] = &["enforce", "shadow", "off"];
const SEVS: &[&str] = &["low", "medium", "high"];
const BANDS: &[&str] = &["none", "low", "watch", "high", "critical"];
pub const ACTION_KINDS: &[&str] = &[
    "warn",
    "freeze_traffic",
    "lock_wallet",
    "disable_download",
    "ban",
    "adjust_traffic",
    "reset_passkey",
];

/// 字段路径是否合法；`evidence_ok` 为 false 时（乘数规则）只允许上下文字段
fn field_ok(f: &str, evidence_ok: bool) -> bool {
    if CTX_FIELDS.contains(&f) {
        return evidence_ok || f != "ctx.is_seedbox";
    }
    evidence_ok
        && (matches!(f, "hits" | "severity" | "torrent_id")
            || f.strip_prefix("details.").is_some_and(detail_key_ok))
}

fn numeric_path_ok(f: &str) -> bool {
    matches!(f, "hits" | "torrent_id") || f.strip_prefix("details.").is_some_and(detail_key_ok)
}

fn measure_ok(m: &str) -> bool {
    matches!(m, "count" | "distinct_torrents" | "distinct_kinds")
        || m.strip_prefix("sum:")
            .or_else(|| m.strip_prefix("max:"))
            .is_some_and(numeric_path_ok)
}

impl V {
    fn kinds_match(&mut self, m: Option<&Value>, path: &str, evidence_ok: bool) {
        let Some(m) = m.filter(|m| m.is_object()) else {
            self.push(path, "unknown_field", "`match` must be an object");
            return;
        };
        match m.get("kinds").and_then(Value::as_array) {
            Some(k)
                if (1..=10).contains(&k.len())
                    && k.iter()
                        .all(|x| x.as_str().is_some_and(|s| (1..=64).contains(&s.len()))) => {}
            _ => self.push(
                &format!("{path}/kinds"),
                "value_out_of_range",
                "`kinds` must list 1..=10 kinds",
            ),
        }
        self.enum_str(m, path, "severity_gte", SEVS, false);
        if let Some(w) = m.get("where") {
            self.preds(w, &format!("{path}/where"), evidence_ok);
        }
        for k in m
            .as_object()
            .map(|o| o.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
        {
            if !matches!(k.as_str(), "kinds" | "severity_gte" | "where") {
                self.push(
                    &format!("{path}/{k}"),
                    "unknown_field",
                    format!("unknown key `{k}`"),
                );
            }
        }
    }

    fn preds(&mut self, w: &Value, path: &str, evidence_ok: bool) {
        let Some(list) = w.as_array().filter(|l| l.len() <= 10) else {
            self.push(
                path,
                "value_out_of_range",
                "predicates must be an array of at most 10",
            );
            return;
        };
        for (i, p) in list.iter().enumerate() {
            let pp = format!("{path}/{i}");
            match (p.get("field"), p.get("measure")) {
                (Some(f), None) => {
                    if !f.as_str().is_some_and(|f| field_ok(f, evidence_ok)) {
                        self.push(
                            &format!("{pp}/field"),
                            "unknown_field",
                            format!("unknown field {f}"),
                        );
                    }
                }
                (None, Some(m)) => {
                    let mp = format!("{pp}/measure");
                    if !m.get("fn").and_then(Value::as_str).is_some_and(measure_ok) {
                        self.push(
                            &format!("{mp}/fn"),
                            "unknown_operator",
                            "unknown measure function",
                        );
                    }
                    self.num(m, &mp, "window_days", 1.0, 180.0, true);
                    self.kinds_match(m.get("match"), &format!("{mp}/match"), true);
                }
                _ => self.push(
                    &pp,
                    "unknown_field",
                    "predicate needs exactly one of `field` or `measure`",
                ),
            }
            let op = p.get("op").and_then(Value::as_str).unwrap_or("");
            if !OPS.contains(&op) {
                self.push(
                    &format!("{pp}/op"),
                    "unknown_operator",
                    format!("unknown operator {op}"),
                );
            }
            let v = p.get("value");
            if op != "exists" && v.is_none() {
                self.push(
                    &format!("{pp}/value"),
                    "value_out_of_range",
                    "`value` is required",
                );
            }
            if matches!(op, "in" | "nin") && !v.is_some_and(Value::is_array) {
                self.push(
                    &format!("{pp}/value"),
                    "value_out_of_range",
                    "`in` / `nin` need an array",
                );
            }
            if let Some(r) = v.and_then(|v| v.get("ref")) {
                if !evidence_ok || !r.as_str().is_some_and(numeric_path_ok) {
                    self.push(
                        &format!("{pp}/value/ref"),
                        "reference_invalid",
                        "`ref` must be a numeric evidence field",
                    );
                }
                if v.and_then(|v| v.get("mul")).is_some_and(|m| m.as_f64().is_none()) {
                    self.push(
                        &format!("{pp}/value/mul"),
                        "value_out_of_range",
                        "`mul` must be a number",
                    );
                }
            }
        }
    }

    fn tags(&mut self, o: &Value, path: &str, key: &str, max: usize) {
        if let Some(t) = o.get(key) {
            let ok = t
                .as_array()
                .is_some_and(|l| l.len() <= max && l.iter().all(|x| x.as_str().is_some_and(tag_ok)));
            if !ok {
                self.push(
                    &format!("{path}/{key}"),
                    "value_out_of_range",
                    format!("`{key}`: at most {max} tags matching ^[a-z_]{{2,32}}$"),
                );
            }
        }
    }
}

/// 语义校验（LLD §1.2）；`auto_ban_enabled` 为站点设置
pub fn validate(def: &Value, auto_ban_enabled: bool) -> Vec<VErr> {
    let mut v = V { errs: Vec::new() };
    if def.to_string().len() > MAX_BYTES {
        v.push("", "value_out_of_range", "ruleset exceeds 64 KiB");
        return v.errs;
    }
    if !def.is_object() {
        v.push("", "unknown_field", "ruleset must be an object");
        return v.errs;
    }
    if def.get("schema").and_then(Value::as_u64) != Some(1) {
        v.push("/schema", "value_out_of_range", "schema must be 1");
    }
    if let Some(d) = def.get("defaults") {
        v.num(d, "/defaults", "window_days", 1.0, 365.0, false);
        v.num(d, "/defaults", "half_life_days", 1.0, 365.0, false);
        v.enum_str(d, "/defaults", "hits_fn", &["log2", "linear", "none"], false);
        v.num(d, "/defaults", "hits_cap", 1.0, 20.0, false);
        v.num(d, "/defaults", "max_evidence", 100.0, 5000.0, false);
    }
    match def.get("thresholds") {
        Some(t) => {
            let g = |v: &mut V, k| v.num(t, "/thresholds", k, 0.0, 100.0, true);
            let (lo, wa, ca, cr, tb) = (
                g(&mut v, "low"),
                g(&mut v, "watch"),
                g(&mut v, "case"),
                g(&mut v, "critical"),
                g(&mut v, "transfer_block"),
            );
            if let (Some(lo), Some(wa), Some(ca), Some(cr)) = (lo, wa, ca, cr) {
                if !(0.0 < lo && lo < wa) {
                    v.push(
                        "/thresholds/watch",
                        "threshold_order",
                        "watch must be greater than low (> 0)",
                    );
                }
                if wa >= ca {
                    v.push(
                        "/thresholds/case",
                        "threshold_order",
                        "case must be greater than watch",
                    );
                }
                if ca > cr {
                    v.push(
                        "/thresholds/critical",
                        "threshold_order",
                        "critical must be ≥ case",
                    );
                }
                if tb.is_some_and(|tb| tb < wa) {
                    v.push(
                        "/thresholds/transfer_block",
                        "threshold_order",
                        "transfer_block must be ≥ watch",
                    );
                }
            }
        }
        None => v.push("/thresholds", "value_out_of_range", "`thresholds` is required"),
    }
    let rules = def
        .get("rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if def.get("rules").is_none_or(|r| !r.is_array()) {
        v.push("/rules", "value_out_of_range", "`rules` must be an array");
    }
    if rules.len() > MAX_RULES {
        v.push("/rules", "value_out_of_range", "at most 200 rules");
    }
    let mut ids: HashMap<String, (bool, Vec<String>)> = HashMap::new();
    for (i, r) in rules.iter().enumerate() {
        let p = format!("/rules/{i}");
        let id = r.get("id").and_then(Value::as_str).unwrap_or("");
        if !ident_ok(id) {
            v.push(
                &format!("{p}/id"),
                "value_out_of_range",
                "id must match ^[a-z][a-z0-9_]{1,47}$",
            );
        } else if ids.contains_key(id) {
            v.push(
                &format!("{p}/id"),
                "duplicate_rule_id",
                format!("duplicate rule id `{id}`"),
            );
        }
        v.enum_str(r, &p, "mode", MODES, false);
        let tags: Vec<String> = r
            .get("tags")
            .and_then(Value::as_array)
            .map(|l| l.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default();
        let ty = r.get("type").and_then(Value::as_str).unwrap_or("");
        ids.insert(id.to_owned(), (ty != "multiplier", tags));
        match ty {
            "evidence" => {
                v.kinds_match(r.get("match"), &format!("{p}/match"), true);
                v.num(r, &p, "weight", 0.0, 100.0, true);
                if let Some(sm) = r.get("severity_mult") {
                    let ok = sm.as_object().is_some_and(|o| {
                        o.iter().all(|(k, x)| {
                            SEVS.contains(&k.as_str()) && x.as_f64().is_some_and(|f| (0.0..=5.0).contains(&f))
                        })
                    });
                    if !ok {
                        v.push(
                            &format!("{p}/severity_mult"),
                            "value_out_of_range",
                            "severity_mult: {low|medium|high: 0..=5}",
                        );
                    }
                }
                v.enum_str(r, &p, "hits_fn", &["log2", "linear", "none"], false);
                v.num(r, &p, "hits_cap", 1.0, 20.0, false);
                v.num(r, &p, "half_life_days", 1.0, 365.0, false);
                if let Some(cm) = r.get("context_mult") {
                    match cm.as_array().filter(|l| l.len() <= 5) {
                        Some(l) => {
                            for (j, m) in l.iter().enumerate() {
                                let mp = format!("{p}/context_mult/{j}");
                                v.preds(m.get("when").unwrap_or(&Value::Null), &format!("{mp}/when"), true);
                                v.num(m, &mp, "mult", 0.0, 3.0, true);
                            }
                        }
                        None => v.push(
                            &format!("{p}/context_mult"),
                            "value_out_of_range",
                            "at most 5 context multipliers",
                        ),
                    }
                }
                v.num(r, &p, "per_torrent_cap", 0.0, 100.0, false);
                v.num(r, &p, "cap", 0.0, 100.0, true);
                v.tags(r, &p, "tags", 5);
            }
            "aggregate" => {
                v.kinds_match(r.get("match"), &format!("{p}/match"), true);
                v.num(r, &p, "window_days", 1.0, 180.0, true);
                if !r.get("measure").and_then(Value::as_str).is_some_and(measure_ok) {
                    v.push(&format!("{p}/measure"), "unknown_operator", "unknown measure");
                }
                match r
                    .get("steps")
                    .and_then(Value::as_array)
                    .filter(|s| (1..=10).contains(&s.len()))
                {
                    Some(steps) => {
                        let mut last = f64::NEG_INFINITY;
                        for (j, s) in steps.iter().enumerate() {
                            let sp = format!("{p}/steps/{j}");
                            let g = v.num(s, &sp, "gte", 0.0, 1e15, true);
                            v.num(s, &sp, "score", 0.0, 100.0, true);
                            if let Some(g) = g {
                                if g <= last {
                                    v.push(
                                        &format!("{sp}/gte"),
                                        "value_out_of_range",
                                        "steps must be strictly increasing",
                                    );
                                }
                                last = g;
                            }
                        }
                    }
                    None => v.push(
                        &format!("{p}/steps"),
                        "value_out_of_range",
                        "1..=10 steps required",
                    ),
                }
                v.tags(r, &p, "tags", 5);
            }
            "multiplier" => {
                v.preds(r.get("when").unwrap_or(&Value::Null), &format!("{p}/when"), false);
                v.num(r, &p, "factor", 0.1, 3.0, true);
                v.tags(r, &p, "applies_to_tags", 10);
            }
            other => v.push(
                &format!("{p}/type"),
                "unknown_field",
                format!("unknown rule type `{other}`"),
            ),
        }
    }
    let mut pids = HashSet::new();
    for (i, pol) in def
        .get("policies")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let p = format!("/policies/{i}");
        let id = pol.get("id").and_then(Value::as_str).unwrap_or("");
        if !ident_ok(id) {
            v.push(
                &format!("{p}/id"),
                "value_out_of_range",
                "id must match ^[a-z][a-z0-9_]{1,47}$",
            );
        } else if !pids.insert(id.to_owned()) {
            v.push(
                &format!("{p}/id"),
                "duplicate_rule_id",
                format!("duplicate policy id `{id}`"),
            );
        }
        let w = pol.get("when").cloned().unwrap_or(json!({}));
        v.enum_str(&w, &format!("{p}/when"), "band_gte", BANDS, false);
        v.num(&w, &format!("{p}/when"), "score_gte", 0.0, 100.0, false);
        let rules_any: Vec<String> = w
            .get("rules_any")
            .and_then(Value::as_array)
            .map(|l| l.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default();
        for (j, r) in rules_any.iter().enumerate() {
            if !ids.get(r).is_some_and(|x| x.0) {
                v.push(
                    &format!("{p}/when/rules_any/{j}"),
                    "reference_invalid",
                    format!("unknown rule `{r}`"),
                );
            }
        }
        let tags_any: Vec<String> = w
            .get("tags_any")
            .and_then(Value::as_array)
            .map(|l| l.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default();
        let actions = pol
            .get("actions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if !(1..=6).contains(&actions.len()) {
            v.push(
                &format!("{p}/actions"),
                "value_out_of_range",
                "1..=6 actions required",
            );
        }
        let auto = pol.get("auto").and_then(Value::as_bool).unwrap_or(false);
        let mut kinds = HashSet::new();
        for (j, a) in actions.iter().enumerate() {
            let ap = format!("{p}/actions/{j}");
            let k = v.enum_str(a, &ap, "kind", ACTION_KINDS, true);
            v.num(a, &ap, "days", 1.0, 3650.0, false);
            if let Some(k) = k {
                if !kinds.insert(k.clone()) {
                    v.push(
                        &format!("{ap}/kind"),
                        "value_out_of_range",
                        "duplicate action kind",
                    );
                }
                if auto && matches!(k.as_str(), "adjust_traffic" | "reset_passkey") {
                    v.push(
                        &format!("{ap}/kind"),
                        "value_out_of_range",
                        format!("`{k}` cannot be automatic"),
                    );
                }
                if auto && matches!(k.as_str(), "ban" | "disable_download") && !auto_ban_enabled {
                    v.push(
                        &format!("{ap}/kind"),
                        "value_out_of_range",
                        "automatic ban / disable_download requires auto_ban_enabled",
                    );
                }
                if k == "freeze_traffic" && a.get("days").is_none() && auto {
                    v.push(
                        &format!("{ap}/days"),
                        "value_out_of_range",
                        "automatic freeze_traffic needs `days`",
                    );
                }
            }
        }
        if auto {
            let conclusive = rules_any
                .iter()
                .any(|r| ids.get(r).is_some_and(|x| x.1.iter().any(|t| t == "conclusive")))
                || tags_any.iter().any(|t| t == "conclusive");
            if !conclusive {
                v.push(
                    &format!("{p}/when"),
                    "auto_policy_not_conclusive",
                    "automatic policies must reference a rule tagged `conclusive`",
                );
            }
        }
    }
    // 结构合法后再做一次类型化解析（兜底类型错误）
    if v.errs.is_empty()
        && let Err(e) = serde_json::from_value::<Ruleset>(def.clone())
    {
        v.push("", "unknown_field", e.to_string());
    }
    v.errs
}

/// 校验并解析
pub fn parse(def: &Value, auto_ban_enabled: bool) -> Result<Ruleset, Vec<VErr>> {
    let errs = validate(def, auto_ban_enabled);
    if !errs.is_empty() {
        return Err(errs);
    }
    serde_json::from_value(def.clone()).map_err(|e| {
        vec![VErr {
            path: String::new(),
            code: "anticheat.unknown_field".into(),
            message: e.to_string(),
        }]
    })
}

/// 默认规则集 v1（LLD §1.2；`counter_jump` 另含 pt-tracker 的 `started_nonzero`）
pub fn default_ruleset() -> Value {
    serde_json::from_str(DEFAULT_RULESET).unwrap_or(Value::Null)
}

pub const DEFAULT_RULESET: &str = include_str!("default_ruleset.json");

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400_000;
    const NOW: i64 = 1_800_000_000_000;

    fn ev(id: i64, kind: &str, sev: Sev, tid: i64, details: Value, age_days: f64) -> Evidence {
        Evidence {
            id,
            kind: kind.into(),
            severity: Some(sev),
            hits: 1,
            torrent_id: Some(tid),
            details,
            occurred_at: NOW - (age_days * DAY as f64) as i64,
        }
    }

    fn rs() -> Ruleset {
        parse(&default_ruleset(), false).unwrap()
    }

    #[test]
    fn default_ruleset_is_valid() {
        assert_eq!(validate(&default_ruleset(), false), vec![]);
        assert!(rs().is_conclusive("speed_extreme"));
        assert_eq!(rs().max_window_days(), 90.0);
    }

    #[test]
    fn rounding() {
        assert_eq!(round_half_even(2.5), 2);
        assert_eq!(round_half_even(3.5), 4);
        assert_eq!(round_half_even(-2.5), -2);
        assert_eq!(round_half_even(2.4999), 2);
        assert_eq!(round_half_even(59.6), 60);
    }

    #[test]
    fn speed_extreme_is_conclusive_and_scores() {
        let rs = rs();
        let e = ev(
            1,
            "speed_exceeded",
            Sev::High,
            7,
            json!({ "bytes": 21_474_836_480u64, "speed_bps": 715_827_882, "limit_bps": 125_000_000 }),
            0.0,
        );
        let s = compute(
            &rs,
            &[e],
            &Ctx {
                account_age_days: 400,
                ..Ctx::default()
            },
            NOW,
            false,
        );
        // speed_high 20 × 1 + speed_extreme 60
        assert_eq!(s.score, 80);
        assert_eq!(s.band, Band::Critical);
        assert!(s.rules_hit.contains(&"speed_extreme".into()));
        assert!(rs.policies.iter().any(|p| p.auto && policy_applies(p, &s)));
        assert_eq!(s.breakdown[0].rule, "speed_extreme");
        assert_eq!(s.breakdown[0].top_torrents, vec![7]);
        // 新账号乘数作用于 upload_fraud：(20 + 60) × 1.2 → 封顶 100
        let s2 = compute(
            &rs,
            &s_ev(),
            &Ctx {
                account_age_days: 3,
                ..Ctx::default()
            },
            NOW,
            false,
        );
        assert_eq!(s2.score, 96);
    }

    fn s_ev() -> Vec<Evidence> {
        vec![ev(
            1,
            "speed_exceeded",
            Sev::High,
            7,
            json!({ "bytes": 21_474_836_480u64, "speed_bps": 715_827_882, "limit_bps": 125_000_000 }),
            0.0,
        )]
    }

    #[test]
    fn decay_caps_and_seedbox() {
        let rs = rs();
        let ctx = Ctx {
            account_age_days: 400,
            ..Ctx::default()
        };
        // 一条 medium 速度证据：8 分；14 天后衰减一半
        let fresh = compute(
            &rs,
            &[ev(1, "speed_exceeded", Sev::Medium, 1, json!({}), 0.0)],
            &ctx,
            NOW,
            false,
        );
        let old = compute(
            &rs,
            &[ev(1, "speed_exceeded", Sev::Medium, 1, json!({}), 14.0)],
            &ctx,
            NOW,
            false,
        );
        assert_eq!((fresh.score, old.score), (8, 4));
        // 盒子：减半
        let mut sb = ctx.clone();
        sb.seedbox.insert(1);
        let boxed = compute(
            &rs,
            &[ev(1, "speed_exceeded", Sev::Medium, 1, json!({}), 0.0)],
            &sb,
            NOW,
            false,
        );
        assert_eq!(boxed.score, 4);
        // 证据 details 带 seedbox 标记时，同一种子的证据都按盒子计
        let flagged = [
            ev(
                1,
                "speed_exceeded",
                Sev::Medium,
                1,
                json!({ "seedbox": true }),
                0.0,
            ),
            ev(
                2,
                "speed_exceeded",
                Sev::Medium,
                3,
                json!({ "seedbox": false }),
                0.0,
            ),
        ];
        assert_eq!(seedbox_from_evidence(&flagged), BTreeSet::from([1]));
        // 单种子上限 16、规则上限 24
        let many: Vec<Evidence> = (0..10)
            .map(|i| ev(i, "speed_exceeded", Sev::Medium, i % 2, json!({}), 0.0))
            .collect();
        let s = compute(&rs, &many, &ctx, NOW, false);
        assert_eq!(s.score, 24);
        // 窗口外的证据不计
        let gone = compute(
            &rs,
            &[ev(1, "speed_exceeded", Sev::High, 1, json!({}), 91.0)],
            &ctx,
            NOW,
            false,
        );
        assert_eq!(gone.score, 0);
        assert_eq!(gone.band, Band::None);
    }

    #[test]
    fn aggregate_and_multipliers() {
        let rs = rs();
        let ctx = Ctx {
            account_age_days: 400,
            ..Ctx::default()
        };
        // 3 个种子的交叉校验：单条 6 × 3 = 18（种子上限 12 各自），聚合 25
        let cc: Vec<Evidence> = (1..=3)
            .map(|t| ev(t, "cross_check_mismatch", Sev::Medium, t, json!({}), 1.0))
            .collect();
        let s = compute(&rs, &cc, &ctx, NOW, false);
        assert!(s.rules_hit.contains(&"cross_swarm".into()));
        let agg = s.breakdown.iter().find(|b| b.rule == "cross_swarm").unwrap();
        assert_eq!(agg.score, Some(25.0));
        // 再犯：× 1.5 作用于全部
        let rec = compute(
            &rs,
            &cc,
            &Ctx {
                confirmed_cases_365d: 1,
                ..ctx.clone()
            },
            NOW,
            false,
        );
        assert!(rec.score > s.score);
        // shadow 规则只进影子分
        let sip = compute(
            &rs,
            &[ev(
                9,
                "anticheat.shared_ip_counterparty",
                Sev::Medium,
                3,
                json!({}),
                0.0,
            )],
            &ctx,
            NOW,
            false,
        );
        assert_eq!((sip.score, sip.shadow_score), (0, 20));
        assert_eq!(sip.shadow_hits, vec!["shared_ip".to_owned()]);
        // 复核确认同一人：30 分（不按严重度放大），半衰期 90 天，类别 multi_account
        let same = |age| {
            compute(
                &rs,
                &[ev(
                    10,
                    "anticheat.same_person_confirmed",
                    Sev::High,
                    0,
                    json!({}),
                    age,
                )],
                &ctx,
                NOW,
                false,
            )
        };
        let fresh = same(0.0);
        assert_eq!(fresh.score, 30);
        assert_eq!(fresh.band, Band::Watch);
        assert_eq!(fresh.tags, vec!["multi_account".to_owned()]);
        assert_eq!(same(90.0).score, 15);
        // 佐证：14 天内 3 种不同信号 → 1.3
        let mixed = vec![
            ev(1, "speed_exceeded", Sev::Medium, 1, json!({}), 0.0),
            ev(2, "peer_not_in_swarm", Sev::Medium, 1, json!({}), 0.0),
            ev(3, "cross_check_mismatch", Sev::Medium, 1, json!({}), 0.0),
        ];
        let m = compute(&rs, &mixed, &ctx, NOW, false);
        assert!(
            m.breakdown
                .iter()
                .any(|b| b.rule == "corroboration" && b.factor == Some(1.3))
        );
        // (8 + 10 + 6) × 1.3 = 31.2
        assert_eq!(m.score, 31);
        assert_eq!(m.band, Band::Watch);
    }

    #[test]
    fn predicates() {
        let ctx = Ctx::default();
        let e = ev(
            1,
            "counter_jump",
            Sev::Medium,
            1,
            json!({ "kind": "started_nonzero", "bytes": 10, "limit": 4 }),
            0.0,
        );
        let p = |v: Value| serde_json::from_value::<Pred>(v).unwrap();
        let ok = |v: Value| pred(&p(v), Some(&e), &ctx, &[], NOW);
        assert!(ok(
            json!({ "field": "details.kind", "op": "in", "value": ["started_nonzero", "x"] })
        ));
        assert!(!ok(
            json!({ "field": "details.kind", "op": "nin", "value": ["started_nonzero"] })
        ));
        assert!(ok(
            json!({ "field": "details.bytes", "op": "gte", "value": { "ref": "details.limit", "mul": 2.5 } })
        ));
        assert!(!ok(
            json!({ "field": "details.bytes", "op": "gt", "value": { "ref": "details.limit", "mul": 2.5 } })
        ));
        assert!(ok(
            json!({ "field": "details.missing", "op": "exists", "value": false })
        ));
        assert!(!ok(json!({ "field": "details.missing", "op": "eq", "value": 1 })));
        assert!(ok(json!({ "field": "severity", "op": "eq", "value": "medium" })));
        assert!(!ok(json!({ "field": "details.kind", "op": "gt", "value": 1 })));
    }

    #[test]
    fn validation_errors() {
        let mut d = default_ruleset();
        d["thresholds"]["case"] = json!(20);
        d["rules"][0]["match"]["where"][0]["op"] = json!("regex");
        d["rules"][1]["id"] = json!("speed_medium");
        d["policies"][0]["when"]["rules_any"] = json!(["speed_high"]);
        d["rules"][5]["value"] = json!(1);
        let e = validate(&d, false);
        let codes: Vec<(&str, &str)> = e.iter().map(|x| (x.path.as_str(), x.code.as_str())).collect();
        assert!(
            codes.contains(&("/thresholds/case", "anticheat.threshold_order")),
            "{codes:?}"
        );
        assert!(
            codes.contains(&("/rules/0/match/where/0/op", "anticheat.unknown_operator")),
            "{codes:?}"
        );
        assert!(
            codes.contains(&("/rules/1/id", "anticheat.duplicate_rule_id")),
            "{codes:?}"
        );
        assert!(
            codes.contains(&("/policies/0/when", "anticheat.auto_policy_not_conclusive")),
            "{codes:?}"
        );
        // 自动封禁需要站点开关
        let mut b = default_ruleset();
        b["policies"][0]["actions"] = json!([{ "kind": "ban" }]);
        assert!(
            validate(&b, false)
                .iter()
                .any(|x| x.path == "/policies/0/actions/0/kind")
        );
        assert!(validate(&b, true).is_empty());
        // 乘数规则不能用证据字段
        let mut m = default_ruleset();
        let last = m["rules"].as_array().unwrap().len() - 1;
        m["rules"][last]["when"][0]["field"] = json!("hits");
        assert!(
            validate(&m, false)
                .iter()
                .any(|x| x.code == "anticheat.unknown_field")
        );
    }
}
