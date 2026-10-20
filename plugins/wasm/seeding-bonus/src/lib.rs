//! seeding-bonus 插件（WASM）：做种魔力的计算公式与参数（后台插件设置，带默认值）、官种认定、给用户看的计算说明。
//!
//! - 公式沿用 NexusPHP 的做种魔力算法，由 bonus 插件的规则引擎执行：bonus 每小时结算时调用 `ctx.seeding_bonus.rules`
//!   取规则定义（未安装本插件时 bonus 使用自己规则表中的版本）；
//! - 官种：发布名以 `-组名` / `@组名` 结尾或以 `[组名]` 开头、且组名在 `official_groups` 中，或发布者在 `official_uploaders`
//!   中；bonus 在种子发布时调用 `ctx.seeding_bonus.classify` 打上 `official` 标签；
//! - `GET /v1/explain`：当前参数与公式（前端说明页与估算器使用）。

pt_plugin_sdk::world!("seeding-bonus", http);

use exports::pt::plugin::{http_handler, service_provider};
use pt_plugin_sdk::{config, prelude::*, serde_json};

struct SeedingBonus;

/// 后台设置（缺省值与 plugin.toml 的 schema 默认值一致）
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    pub b0: f64,
    pub l: f64,
    pub t0: f64,
    pub n0: f64,
    pub p: f64,
    pub max_count: f64,
    pub official_enabled: bool,
    pub official_factor: f64,
    pub official_groups: Vec<String>,
    pub official_uploaders: Vec<i64>,
    pub donor_factor: f64,
    pub min_seed_minutes: f64,
    pub min_size_mib: f64,
    pub hourly_cap: f64,
}

impl Params {
    pub fn from_settings(s: &Value) -> Self {
        let n = |k: &str, d: f64, min: f64| s[k].as_f64().filter(|x| x.is_finite()).unwrap_or(d).max(min);
        let strs = |k: &str| -> Vec<String> {
            s[k].as_array()
                .map(|a| a.iter().filter_map(Value::as_str).map(|x| x.trim().to_owned()).filter(|x| !x.is_empty()).collect())
                .unwrap_or_default()
        };
        Self {
            b0: n("b0", 100.0, 0.0),
            l: n("l", 300.0, 1.0),
            t0: n("t0", 4.0, 0.1),
            n0: n("n0", 7.0, 2.0),
            p: n("p", 1.0, 0.0),
            max_count: n("max_count", 7.0, 0.0),
            official_enabled: s["official_enabled"].as_bool().unwrap_or(true),
            official_factor: n("official_factor", 0.5, 0.0),
            official_groups: strs("official_groups"),
            official_uploaders: strs("official_uploaders").iter().filter_map(|x| x.parse().ok()).collect(),
            donor_factor: n("donor_factor", 2.0, 0.0),
            min_seed_minutes: n("min_seed_minutes", 1.0, 0.0).min(60.0),
            min_size_mib: n("min_size_mib", 0.0, 0.0),
            hourly_cap: n("hourly_cap", 10_000.0, 0.0),
        }
    }

    /// bonus 规则引擎的定义（bonus 03 §1.2 的 expression 模式；`bonus` / `seed_points` 的币种精度为 1 位小数）
    pub fn rule_def(&self) -> Value {
        let official = if self.official_enabled { self.official_factor } else { 0.0 };
        let base = "B0 * 2 / pi * atan(A / L) + P * min(count, MAX)";
        #[allow(clippy::cast_possible_truncation)]
        let cap = (self.hourly_cap * 10.0).round().min(9e15) as i64;
        json!({
            "mode": "expression",
            "params": { "T0": self.t0, "N0": self.n0, "B0": self.b0, "L": self.l, "P": self.p, "MAX": self.max_count,
                        "DONOR": self.donor_factor, "OFFICIAL": official, "ZERO_FACTOR": 0 },
            "filters": { "min_size": self.min_size_mib * 1_048_576.0, "min_seed_secs": self.min_seed_minutes * 60.0 },
            "torrent_terms": [
                { "name": "A", "expr": "(1 - exp(ln(0.1) / T0 * age_weeks)) * if(tag(\"zero_bonus\"), gib * ZERO_FACTOR, gib) * (1 + sqrt(2) * exp(ln(0.1) * (seeders - 1) / (N0 - 1)))" },
                { "name": "AW", "expr": "A_i * seed_hours" },
                { "name": "OA", "expr": "if(tag(\"official\"), A_i, 0)" }
            ],
            "user_formulas": {
                "seed_points": format!("({base}) * safe_div(AW, A, 0)"),
                "bonus": format!("(({base}) * if(has_role(\"levels:vip\"), DONOR, 1) + B0 * 2 / pi * atan(OA / L) * OFFICIAL) * safe_div(AW, A, 0)"),
            },
            // 上限以最小单位计（1 位小数）
            "caps": { "bonus": cap, "seed_points": cap },
            "tiers": null
        })
    }

    /// 是否官种
    pub fn is_official(&self, title: &str, uploader: Option<i64>) -> bool {
        if !self.official_enabled {
            return false;
        }
        if uploader.is_some_and(|u| self.official_uploaders.contains(&u)) {
            return true;
        }
        release_group(title).is_some_and(|g| self.official_groups.iter().any(|x| x.eq_ignore_ascii_case(&g)))
    }
}

/// 发布名中的发布组：`…-Group`、`…@Group`（去掉文件扩展名）或开头的 `[Group]`
pub fn release_group(title: &str) -> Option<String> {
    let t = title.trim();
    if let Some(rest) = t.strip_prefix('[') {
        let g = rest.split(']').next().unwrap_or("").trim();
        if !g.is_empty() {
            return Some(g.to_owned());
        }
    }
    let t = match t.rsplit_once('.') {
        Some((stem, ext)) if (2..=4).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric()) && !ext.bytes().all(|b| b.is_ascii_digit()) => stem,
        _ => t,
    };
    let i = t.rfind(['-', '@'])?;
    // “标题 - 01” 这类两侧带空格的分隔不是发布组
    let g = &t[i + 1..];
    let spaced = g.starts_with(char::is_whitespace) || t[..i].ends_with(char::is_whitespace);
    (!spaced && !g.is_empty() && !g.contains(char::is_whitespace) && g.len() <= 32).then(|| g.to_owned())
}

fn params() -> Params {
    Params::from_settings(&serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null))
}

impl service_provider::Guest for SeedingBonus {
    fn call(service: String, method: String, args: String) -> R<String> {
        if service != "ctx.seeding_bonus" {
            return Err(err("kernel.not_found", "unknown service"));
        }
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        let out = match method.as_str() {
            "rules" => params().rule_def(),
            "classify" => {
                let official = params().is_official(a["title"].as_str().unwrap_or(""), a["uploader_id"].as_i64());
                json!({ "tags": if official { vec!["official"] } else { vec![] } })
            }
            other => return Err(err("kernel.not_found", &format!("ctx.seeding_bonus has no method `{other}`"))),
        };
        Ok(out.to_string())
    }
}

impl http_handler::Guest for SeedingBonus {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        let reply = match req.route() {
            "explain" => {
                let p = params();
                http::Reply::ok(&json!({
                    "b0": p.b0, "l": p.l, "t0": p.t0, "n0": p.n0, "p": p.p, "max_count": p.max_count,
                    "official_enabled": p.official_enabled, "official_factor": p.official_factor, "official_groups": p.official_groups,
                    "donor_factor": p.donor_factor, "min_seed_minutes": p.min_seed_minutes, "min_size_mib": p.min_size_mib,
                    "hourly_cap": p.hourly_cap,
                }))
            }
            _ => return Err(err("kernel.not_found", "no such route")),
        };
        Ok(reply.into())
    }
}

export!(SeedingBonus);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_nexusphp_preset() {
        let p = Params::from_settings(&Value::Null);
        let d = p.rule_def();
        assert_eq!(d["params"]["B0"], 100.0);
        assert_eq!(d["params"]["OFFICIAL"], 0.5);
        assert_eq!(d["filters"]["min_seed_secs"], 60.0);
        assert_eq!(d["caps"]["bonus"], 100_000);
        let off = Params::from_settings(&json!({ "official_enabled": false }));
        assert_eq!(off.rule_def()["params"]["OFFICIAL"], 0.0);
    }

    #[test]
    fn release_groups() {
        assert_eq!(release_group("Movie.2024.1080p.WEB-DL.H264-PTStudio").as_deref(), Some("PTStudio"));
        assert_eq!(release_group("Movie.2024.1080p.BluRay.x265-PTS.mkv").as_deref(), Some("PTS"));
        assert_eq!(release_group("Show S01 1080p WEB-DL@HDS").as_deref(), Some("HDS"));
        assert_eq!(release_group("[Sakurato] Frieren - 01 [1080p]").as_deref(), Some("Sakurato"));
        assert_eq!(release_group("No Group Here"), None);
        assert_eq!(release_group("Title - 01"), None);
        let p = Params::from_settings(&json!({ "official_groups": ["ptstudio"], "official_uploaders": ["7"] }));
        assert!(p.is_official("A.2024.1080p-PTStudio", None));
        assert!(p.is_official("A.2024.1080p-Other", Some(7)));
        assert!(!p.is_official("A.2024.1080p-Other", Some(8)));
    }
}
