//! 规则集（bonus 03 §1.2）：定义 JSON、预设、编译（种子级项、用户级公式、分档模式）。

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::expr::{self, Compiled, Issue, Scope};

/// 种子级变量（03 §1.1.4），下标即求值环境中的位置
pub const TORRENT_VARS: &[&str] = &[
    "size",
    "gib",
    "seed_secs",
    "seed_hours",
    "seeders",
    "age_secs",
    "age_days",
    "age_weeks",
    "is_uploader",
    "leechers",
    "completed",
    "snatch_seed_secs",
];
/// 用户级内置变量；其后依次为各种子级项的用户总和
pub const USER_VARS: &[&str] = &[
    "count",
    "total_gib",
    "seed_hours_total",
    "class_id",
    "medal_factor",
    "invitee_seed_points",
];

pub const VAR_SNATCH_SEED_SECS: usize = 11;

#[derive(Debug, Clone, Deserialize)]
pub struct Filters {
    #[serde(default)]
    pub min_size: f64,
    #[serde(default)]
    pub min_seed_secs: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TermDef {
    pub name: String,
    pub expr: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TierDef {
    pub key: String,
    pub when: String,
    pub op: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuleDef {
    pub mode: String,
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
    #[serde(default = "no_filters")]
    pub filters: Filters,
    #[serde(default)]
    pub torrent_terms: Vec<TermDef>,
    #[serde(default)]
    pub user_formulas: BTreeMap<String, String>,
    #[serde(default)]
    pub caps: BTreeMap<String, i64>,
    #[serde(default)]
    pub tiers: Option<Vec<TierDef>>,
    #[serde(default)]
    pub tier_output: BTreeMap<String, f64>,
}

fn no_filters() -> Filters {
    Filters {
        min_size: 0.0,
        min_seed_secs: 0.0,
    }
}

/// NexusPHP 形状的预设（03 §1.2 示例）
pub const PRESET_NEXUSPHP: &str = r#"{
  "mode": "expression",
  "params": { "T0": 4, "N0": 7, "B0": 100, "L": 300, "P": 1, "MAX": 7, "DONOR": 2, "OFFICIAL": 0.5, "ZERO_FACTOR": 0 },
  "filters": { "min_size": 0, "min_seed_secs": 60 },
  "torrent_terms": [
    { "name": "A",  "expr": "(1 - exp(ln(0.1) / T0 * age_weeks)) * if(tag(\"zero_bonus\"), gib * ZERO_FACTOR, gib) * (1 + sqrt(2) * exp(ln(0.1) * (seeders - 1) / (N0 - 1)))" },
    { "name": "AW", "expr": "A_i * seed_hours" },
    { "name": "OA", "expr": "if(tag(\"official\"), A_i, 0)" }
  ],
  "user_formulas": {
    "seed_points": "(B0 * 2 / pi * atan(A / L) + P * min(count, MAX)) * safe_div(AW, A, 0)",
    "bonus": "((B0 * 2 / pi * atan(A / L) + P * min(count, MAX)) * if(has_role(\"levels:vip\"), DONOR, 1) + B0 * 2 / pi * atan(OA / L) * OFFICIAL) * safe_div(AW, A, 0)"
  },
  "caps": { "bonus": 100000, "seed_points": 100000 },
  "tiers": null
}"#;

#[derive(Debug, Clone)]
pub struct CompiledTerm {
    pub name: String,
    pub expr: Compiled,
    /// 本项本种子值在种子级环境中的下标（`<名>_i`）
    pub slot: usize,
}

#[derive(Debug, Clone)]
pub struct CompiledRules {
    pub filters: Filters,
    pub torrent_vars: usize,
    pub terms: Vec<CompiledTerm>,
    /// (币种, 公式)
    pub formulas: Vec<(String, Compiled)>,
    pub caps: BTreeMap<String, i64>,
    pub needs_snatch: bool,
    pub needs_roles: bool,
    pub needs_tags: bool,
}

fn param_name_ok(k: &str) -> bool {
    let b = k.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && b[0].is_ascii_uppercase()
        && b.iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_')
}

/// 分档模式编译为表达式模式（03 §1.2）：T_i 依次 add / multiply，TW_i = T_i × seed_hours，用户公式 = TW × 产出系数
fn tiers_to_terms(def: &RuleDef) -> Result<(Vec<TermDef>, BTreeMap<String, String>), Issue> {
    let tiers = def.tiers.as_deref().unwrap_or_default();
    let mut t = "0".to_owned();
    for tier in tiers {
        t = match tier.op.as_str() {
            "add" => format!("({t}) + if({}, {}, 0)", tier.when, tier.value),
            "multiply" => format!("({t}) * if({}, {}, 1)", tier.when, tier.value),
            _ => {
                return Err(Issue {
                    code: "bonus.rules_invalid",
                    pos: 0,
                    message: format!("tier `{}`: op must be add or multiply", tier.key),
                });
            }
        };
    }
    let terms = vec![
        TermDef {
            name: "T".into(),
            expr: t,
        },
        TermDef {
            name: "TW".into(),
            expr: "T_i * seed_hours".into(),
        },
    ];
    let formulas = def
        .tier_output
        .iter()
        .map(|(c, f)| (c.clone(), format!("TW * {f}")))
        .collect();
    Ok((terms, formulas))
}

/// 编译整个规则集；`currencies` 为本插件币种（用户公式的键必须属于其中）。
pub fn compile(def: &RuleDef, currencies: &[String]) -> Result<CompiledRules, Vec<(String, Issue)>> {
    let mut errs: Vec<(String, Issue)> = Vec::new();
    for k in def.params.keys() {
        if !param_name_ok(k) {
            errs.push((
                format!("params.{k}"),
                Issue {
                    code: "bonus.rules_invalid",
                    pos: 0,
                    message: "parameter names must match ^[A-Z][A-Z0-9_]{0,31}$".into(),
                },
            ));
        }
    }
    let (term_defs, formula_defs) = match def.mode.as_str() {
        "expression" => (def.torrent_terms.clone(), def.user_formulas.clone()),
        "tiers" => match tiers_to_terms(def) {
            Ok(x) => x,
            Err(e) => return Err(vec![("tiers".into(), e)]),
        },
        other => {
            return Err(vec![(
                "mode".into(),
                Issue {
                    code: "bonus.rules_invalid",
                    pos: 0,
                    message: format!("unknown mode `{other}`"),
                },
            )]);
        }
    };
    let mut tscope = Scope::new(TORRENT_VARS);
    let mut terms = Vec::new();
    let (mut needs_snatch, mut needs_roles, mut needs_tags) = (false, false, false);
    for t in &term_defs {
        let ok = t.name.len() <= 32
            && t.name.bytes().next().is_some_and(|c| c.is_ascii_alphabetic())
            && t.name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
            && !TORRENT_VARS.contains(&t.name.as_str())
            && !USER_VARS.contains(&t.name.as_str());
        if !ok {
            errs.push((
                format!("torrent_terms.{}", t.name),
                Issue {
                    code: "bonus.rules_invalid",
                    pos: 0,
                    message: "invalid term name".into(),
                },
            ));
            continue;
        }
        match expr::compile(&t.expr, &tscope, &def.params) {
            Ok(c) => {
                needs_snatch |= c.deps.vars.contains(&VAR_SNATCH_SEED_SECS);
                needs_roles |= c.deps.needs_roles;
                needs_tags |= c.deps.needs_tags;
                let slot = tscope.add(&format!("{}_i", t.name));
                terms.push(CompiledTerm {
                    name: t.name.clone(),
                    expr: c,
                    slot,
                });
            }
            Err(e) => errs.push((format!("torrent_terms.{}", t.name), e)),
        }
    }
    let mut uscope = Scope::new(USER_VARS);
    for t in &terms {
        uscope.add(&t.name);
    }
    let mut formulas = Vec::new();
    for (cur, src) in &formula_defs {
        if !currencies.iter().any(|c| c == cur) {
            errs.push((
                format!("user_formulas.{cur}"),
                Issue {
                    code: "bonus.rules_invalid",
                    pos: 0,
                    message: format!("`{cur}` is not a bonus currency"),
                },
            ));
            continue;
        }
        match expr::compile(src, &uscope, &def.params) {
            Ok(c) => {
                needs_roles |= c.deps.needs_roles;
                formulas.push((cur.clone(), c));
            }
            Err(e) => errs.push((format!("user_formulas.{cur}"), e)),
        }
    }
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(CompiledRules {
        filters: def.filters.clone(),
        torrent_vars: tscope.names.len(),
        terms,
        formulas,
        caps: def.caps.clone(),
        needs_snatch,
        needs_roles,
        needs_tags,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curs() -> Vec<String> {
        vec!["bonus".into(), "seed_points".into()]
    }

    #[test]
    fn preset_compiles() {
        let def: RuleDef = serde_json::from_str(PRESET_NEXUSPHP).unwrap_or_else(|e| panic!("{e}"));
        let c = compile(&def, &curs()).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(c.terms.len(), 3);
        assert_eq!(c.formulas.len(), 2);
        assert!(c.needs_roles && c.needs_tags && !c.needs_snatch);
        assert_eq!(c.torrent_vars, TORRENT_VARS.len() + 3);
    }

    #[test]
    fn term_order_and_errors() {
        let mut def: RuleDef = serde_json::from_str(PRESET_NEXUSPHP).unwrap_or_else(|e| panic!("{e}"));
        // 引用后面的项：未知标识符
        def.torrent_terms[0].expr = "AW_i + 1".into();
        let e = compile(&def, &curs()).err().unwrap_or_default();
        assert_eq!(e[0].1.code, "bonus.expression_unknown_identifier");
        let mut def: RuleDef = serde_json::from_str(PRESET_NEXUSPHP).unwrap_or_else(|e| panic!("{e}"));
        def.user_formulas.insert("gold".into(), "1".into());
        assert!(compile(&def, &curs()).is_err());
    }

    #[test]
    fn tiers_mode() {
        let def: RuleDef = serde_json::from_str(
            r#"{ "mode": "tiers",
            "tiers": [ { "key": "big", "when": "size >= 10 * GiB", "op": "add", "value": "1" },
                       { "key": "old", "when": "age_days >= 30", "op": "add", "value": "0.5" },
                       { "key": "official", "when": "tag(\"official\")", "op": "multiply", "value": "2" } ],
            "tier_output": { "bonus": 1.0 } }"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let c = compile(&def, &curs()).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            c.terms.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
            vec!["T", "TW"]
        );
        assert_eq!(c.formulas[0].0, "bonus");
    }
}
