//! 客户端规则（LLD §1.5）：校验、导出 `trk:clients`、规则测试与预设模板。
//!
//! 匹配语义与 pt-tracker（`crates/pt-tracker/src/clients.rs`）完全相同：两边各自实现（pt-tracker 不链接站点代码，D16），
//! 共用测试向量 `docs/plugins/tracker-bridge/client-rules-vectors.json`。

use pt_plugin_api::json;
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 规则数上限
pub const MAX_RULES: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub name: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid_regex: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_ver: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_ver: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ua_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ua_regex: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub note: String,
}

fn yes() -> bool {
    true
}

impl Rule {
    /// 空字符串视为未设置
    pub fn normalized(mut self) -> Self {
        for f in [
            &mut self.pid_prefix,
            &mut self.pid_regex,
            &mut self.min_ver,
            &mut self.max_ver,
            &mut self.ua_prefix,
            &mut self.ua_regex,
        ] {
            if f.as_deref().is_some_and(str::is_empty) {
                *f = None;
            }
        }
        self.name = self.name.trim().to_owned();
        self
    }
}

/// `%XX` 转义解码（其余字节原样）
pub fn unescape(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = b.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Some(out)
}

/// 字节 → 可读文本：可打印 ASCII（除 `%`）原样，其余 `%XX`
pub fn escape(b: &[u8]) -> String {
    let mut s = String::with_capacity(b.len());
    for &c in b {
        if (0x20..=0x7e).contains(&c) && c != b'%' {
            s.push(char::from(c));
        } else {
            s.push_str(&format!("%{c:02X}"));
        }
    }
    s
}

fn regex(s: &str) -> Result<Regex, String> {
    RegexBuilder::new(s)
        .size_limit(64 << 10)
        .build()
        .map_err(|e| e.to_string())
}

/// `1.2.3` → `[1, 2, 3]`（1～4 段）
pub fn version(s: &str) -> Option<Vec<u32>> {
    let v: Vec<u32> = s
        .split('.')
        .map(|p| {
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
                None
            } else {
                p.parse().ok()
            }
        })
        .collect::<Option<_>>()?;
    (1..=4).contains(&v.len()).then_some(v)
}

fn cmp(a: &[u32], b: &[u32]) -> std::cmp::Ordering {
    let n = a.len().max(b.len());
    let at = |v: &[u32], i: usize| v.get(i).copied().unwrap_or(0);
    (0..n)
        .map(|i| at(a, i).cmp(&at(b, i)))
        .find(|o| o.is_ne())
        .unwrap_or(std::cmp::Ordering::Equal)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FieldError {
    pub path: String,
    pub code: &'static str,
    pub message: String,
}

/// 整体校验（错误全部收集，`path` 为 `/rules/<下标>/<字段>`）
pub fn validate(default_action: &str, rules: &[Rule]) -> Vec<FieldError> {
    let mut errs = Vec::new();
    let mut push = |path: String, code: &'static str, message: String| errs.push(FieldError { path, code, message });
    if !matches!(default_action, "allow" | "deny") {
        push("/default_action".into(), "invalid_action", "default_action must be allow or deny".into());
    }
    if rules.len() > MAX_RULES {
        push("/rules".into(), "too_many", format!("at most {MAX_RULES} rules"));
    }
    for (i, r) in rules.iter().enumerate() {
        let p = |f: &str| format!("/rules/{i}/{f}");
        let n = r.name.chars().count();
        if n == 0 || n > 64 {
            push(p("name"), "length", "name must be 1-64 characters".into());
        }
        if !matches!(r.action.as_str(), "allow" | "deny") {
            push(p("action"), "invalid_action", "action must be allow or deny".into());
        }
        if r.pid_prefix.is_none() && r.pid_regex.is_none() && r.ua_prefix.is_none() && r.ua_regex.is_none() {
            push(p("pid_prefix"), "no_condition", "a rule needs a peer_id or User-Agent condition".into());
        }
        if let Some(s) = &r.pid_prefix {
            match unescape(s) {
                None => push(p("pid_prefix"), "bad_escape", "invalid %XX escape".into()),
                Some(b) if b.len() > 20 => push(p("pid_prefix"), "too_long", "peer_id prefix is at most 20 bytes".into()),
                Some(_) => {}
            }
        }
        let mut has_ver = false;
        if let Some(s) = &r.pid_regex {
            if s.chars().count() > 256 {
                push(p("pid_regex"), "too_long", "regex is at most 256 characters".into());
            } else {
                match regex(s) {
                    Ok(re) => has_ver = re.capture_names().any(|n| n == Some("ver")),
                    Err(e) => push(p("pid_regex"), "regex_syntax", e),
                }
            }
        }
        for (f, v) in [("min_ver", &r.min_ver), ("max_ver", &r.max_ver)] {
            if let Some(v) = v {
                if version(v).is_none() {
                    push(p(f), "version_format", "version must look like 4.3 or 0.13.8".into());
                } else if !has_ver {
                    push(p(f), "requires_ver_group", format!("{f} needs a pid_regex with a (?P<ver>…) group"));
                }
            }
        }
        if let (Some(a), Some(b)) = (r.min_ver.as_deref().and_then(version), r.max_ver.as_deref().and_then(version))
            && cmp(&a, &b).is_gt()
        {
            push(p("max_ver"), "version_range", "max_ver is lower than min_ver".into());
        }
        if r.ua_prefix.as_ref().is_some_and(|s| s.chars().count() > 64) {
            push(p("ua_prefix"), "too_long", "User-Agent prefix is at most 64 characters".into());
        }
        if let Some(s) = &r.ua_regex {
            if s.chars().count() > 256 {
                push(p("ua_regex"), "too_long", "regex is at most 256 characters".into());
            } else if let Err(e) = regex(s) {
                push(p("ua_regex"), "regex_syntax", e);
            }
        }
        if r.note.chars().count() > 500 {
            push(p("note"), "too_long", "note is at most 500 characters".into());
        }
    }
    errs
}

/// `trk:clients` 的内容（不含 `ver`）：只导出启用的规则，未设置的字段省略
pub fn export(default_action: &str, rules: &[Rule]) -> Value {
    let rules: Vec<Value> = rules
        .iter()
        .filter(|r| r.enabled)
        .map(|r| {
            let mut v = json!({ "id": r.id.unwrap_or(0), "name": r.name, "action": r.action });
            for (k, f) in [
                ("pid_prefix", &r.pid_prefix),
                ("pid_regex", &r.pid_regex),
                ("min_ver", &r.min_ver),
                ("max_ver", &r.max_ver),
                ("ua_prefix", &r.ua_prefix),
                ("ua_regex", &r.ua_regex),
            ] {
                if let Some(s) = f {
                    v[k] = json!(s);
                }
            }
            v
        })
        .collect();
    json!({ "default": default_action, "rules": rules })
}

/// 判定结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub action: String,
    /// 命中规则在列表中的下标（未命中为 None，按默认动作）
    pub index: Option<usize>,
    /// 命中规则的 `ver` 组解析出的版本
    pub version: Option<Vec<u32>>,
}

/// 与 pt-tracker 相同的匹配：第一条全部条件满足的启用规则决定动作。调用前须已通过 [`validate`]
pub fn decide(default_action: &str, rules: &[Rule], peer_id: &[u8], ua: &[u8]) -> Decision {
    let latin1: String = peer_id.iter().map(|&c| char::from(c)).collect();
    let ua_text = String::from_utf8_lossy(ua);
    for (i, r) in rules.iter().enumerate().filter(|(_, r)| r.enabled) {
        if r
            .pid_prefix
            .as_deref()
            .and_then(unescape)
            .is_some_and(|p| !peer_id.starts_with(&p))
        {
            continue;
        }
        if r.ua_prefix.as_ref().is_some_and(|p| !ua.starts_with(p.as_bytes())) {
            continue;
        }
        if r
            .ua_regex
            .as_deref()
            .and_then(|s| regex(s).ok())
            .is_some_and(|re| !re.is_match(&ua_text))
        {
            continue;
        }
        let mut ver = None;
        if let Some(re) = r.pid_regex.as_deref().and_then(|s| regex(s).ok()) {
            let Some(m) = re.captures(&latin1) else { continue };
            let digits = m.name("ver").and_then(|v| {
                v.as_str()
                    .chars()
                    .map(|c| c.to_digit(36))
                    .collect::<Option<Vec<u32>>>()
            });
            if r.min_ver.is_some() || r.max_ver.is_some() {
                let Some(d) = &digits else { continue };
                if r.min_ver.as_deref().and_then(version).is_some_and(|v| cmp(d, &v).is_lt())
                    || r.max_ver.as_deref().and_then(version).is_some_and(|v| cmp(d, &v).is_gt())
                {
                    continue;
                }
            }
            ver = digits;
        }
        return Decision {
            action: r.action.clone(),
            index: Some(i),
            version: ver,
        };
    }
    Decision {
        action: default_action.to_owned(),
        index: None,
        version: None,
    }
}

/// 规则测试接口的 peer_id：40 位 hex、`%XX` 编码或 20 字节文本
pub fn parse_peer_id(s: &str) -> Option<Vec<u8>> {
    let b = if s.len() == 40 && s.bytes().all(|c| c.is_ascii_hexdigit()) {
        hex::decode(s).ok()?
    } else {
        unescape(s)?
    };
    (!b.is_empty() && b.len() <= 20).then_some(b)
}

fn preset(name: &str, action: &str, f: &[(&str, &str)], note: &str) -> Rule {
    let mut r = Rule {
        name: name.into(),
        action: action.into(),
        enabled: true,
        note: note.into(),
        ..Rule::default()
    };
    for (k, v) in f {
        let v = Some((*v).to_owned());
        match *k {
            "pid_prefix" => r.pid_prefix = v,
            "pid_regex" => r.pid_regex = v,
            "min_ver" => r.min_ver = v,
            "max_ver" => r.max_ver = v,
            "ua_prefix" => r.ua_prefix = v,
            _ => r.ua_regex = v,
        }
    }
    r
}

/// 预设模板（LLD §1.5）：管理员一键加入后可编辑
pub fn presets() -> Vec<Rule> {
    vec![
        preset(
            "qBittorrent 4.6.0（有缺陷）",
            "deny",
            &[("pid_prefix", "-qB4600-")],
            "4.6.0 汇报异常，官方已在 4.6.1 修复",
        ),
        preset(
            "qBittorrent ≥ 4.3",
            "allow",
            &[("pid_regex", "^-qB(?P<ver>[0-9A-Z]{4})-"), ("min_ver", "4.3")],
            "",
        ),
        preset(
            "Transmission ≥ 3.0",
            "allow",
            &[("pid_regex", "^-TR(?P<ver>[0-9]{3})[0-9Z]-"), ("min_ver", "3.0")],
            "",
        ),
        preset(
            "Deluge 2.x",
            "allow",
            &[("pid_regex", "^-DE(?P<ver>[0-9A-Za-z]{4})-"), ("min_ver", "2.0")],
            "Deluge 2.1.2 的 peer_id 为 -DE212s-（末位小写字母）",
        ),
        preset(
            "rTorrent / libTorrent ≥ 0.13.8",
            "allow",
            &[("pid_regex", "^-lt(?P<ver>[0-9A-Z]{3})"), ("min_ver", "0.13.8")],
            "Flood、ruTorrent 也使用 rTorrent",
        ),
        preset(
            "libtorrent-rasterbar ≥ 1.2",
            "allow",
            &[("pid_regex", "^-LT(?P<ver>[0-9A-Z]{3})"), ("min_ver", "1.2")],
            "",
        ),
        preset(
            "BiglyBT",
            "allow",
            &[("pid_prefix", "-BI")],
            "",
        ),
        preset(
            "µTorrent ≤ 3.5.5",
            "allow",
            &[("pid_regex", "^-UT(?P<ver>[0-9A-Z]{3})"), ("max_ver", "3.5.5")],
            "3.6 起带广告与挖矿组件，许多站点只允许到 3.5.5",
        ),
        preset("迅雷", "deny", &[("pid_prefix", "-XL")], "只下载不上传"),
        preset("迅雷（SD）", "deny", &[("pid_prefix", "-SD")], ""),
    ]
}

/// 旧版设置（`clients` 白名单 + `allow_unknown_clients`）→ 规则集，首次启用规则表时迁移
pub fn from_legacy(settings: &Value, defaults: &[(&str, &str)]) -> (String, Vec<Rule>) {
    let rule = |name: &str, prefix: &str| Rule {
        name: name.chars().take(64).collect(),
        action: "allow".into(),
        pid_prefix: Some(escape(prefix.as_bytes())),
        enabled: true,
        ..Rule::default()
    };
    let rules = match settings["clients"].as_array().filter(|a| !a.is_empty()) {
        Some(list) => list
            .iter()
            .filter_map(|r| Some(rule(r["name"].as_str()?, r["peer_id_prefix"].as_str()?)))
            .filter(|r| !r.name.is_empty() && r.pid_prefix.as_deref().is_some_and(|p| !p.is_empty()))
            .collect(),
        None => defaults.iter().map(|(n, p)| rule(n, p)).collect(),
    };
    let default = if settings["allow_unknown_clients"].as_bool() == Some(true) {
        "allow"
    } else {
        "deny"
    };
    (default.into(), rules)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// 与 pt-tracker 共用的测试向量
    const VECTORS: &str = include_str!("../../../../docs/plugins/tracker-bridge/client-rules-vectors.json");

    fn rules_of(v: &Value) -> (String, Vec<Rule>, Vec<i64>) {
        let mut ids = Vec::new();
        let rules = v["rules"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|r| {
                        ids.push(r["id"].as_i64().unwrap_or(0));
                        let mut r = r.clone();
                        if r["name"].is_null() {
                            r["name"] = json!("r");
                        }
                        serde_json::from_value::<Rule>(r).unwrap()
                    })
                    .collect()
            })
            .unwrap_or_default();
        (v["default"].as_str().unwrap_or("").to_owned(), rules, ids)
    }

    #[test]
    fn vectors() {
        let v: Value = serde_json::from_str(VECTORS).unwrap();
        for g in v["groups"].as_array().unwrap() {
            let (d, rules, ids) = rules_of(&g["rules"]);
            assert!(validate(&d, &rules).is_empty(), "{}: {:?}", g["name"], validate(&d, &rules));
            for c in g["cases"].as_array().unwrap() {
                let pid = unescape(c["peer_id"].as_str().unwrap()).unwrap();
                let r = decide(&d, &rules, &pid, c["ua"].as_str().unwrap_or("").as_bytes());
                assert_eq!(
                    (r.action.as_str(), r.index.map(|i| ids[i])),
                    (c["action"].as_str().unwrap(), c["rule_id"].as_i64()),
                    "{}: {c}",
                    g["name"]
                );
            }
        }
        for bad in v["invalid"].as_array().unwrap() {
            let (d, rules, _) = rules_of(&bad["rules"]);
            assert!(!validate(&d, &rules).is_empty(), "{}", bad["name"]);
        }
    }

    #[test]
    fn validation_paths() {
        let r = |f: &str, v: &str| {
            serde_json::from_value::<Rule>(json!({ "name": "x", "action": "allow", f: v })).unwrap()
        };
        let mut ver_only = r("pid_prefix", "-qB");
        ver_only.min_ver = Some("4.3".into());
        let errs = validate("deny", &[r("pid_regex", "("), ver_only, Rule { name: String::new(), action: "allow".into(), ..Rule::default() }]);
        let codes: Vec<(&str, &str)> = errs.iter().map(|e| (e.path.as_str(), e.code)).collect();
        assert!(codes.contains(&("/rules/0/pid_regex", "regex_syntax")), "{codes:?}");
        assert!(codes.contains(&("/rules/1/min_ver", "requires_ver_group")), "{codes:?}");
        assert!(codes.contains(&("/rules/2/name", "length")), "{codes:?}");
        assert!(codes.contains(&("/rules/2/pid_prefix", "no_condition")), "{codes:?}");
        let mut range = r("pid_regex", "^-qB(?P<ver>.{4})");
        range.min_ver = Some("5".into());
        range.max_ver = Some("4.9".into());
        assert_eq!(validate("deny", &[range])[0].code, "version_range");
        assert!(validate("deny", &presets()).is_empty());
    }

    #[test]
    fn disabled_and_export() {
        let mut a = serde_json::from_value::<Rule>(json!({ "id": 1, "name": "a", "action": "deny", "pid_prefix": "-qB", "pid_regex": "" }))
            .unwrap()
            .normalized();
        assert!(a.pid_regex.is_none());
        let b = serde_json::from_value::<Rule>(json!({ "id": 2, "name": "b", "action": "allow", "pid_prefix": "-qB" })).unwrap();
        assert_eq!(decide("deny", &[a.clone(), b.clone()], b"-qB4630-", b"").index, Some(0));
        a.enabled = false;
        let r = decide("deny", &[a.clone(), b.clone()], b"-qB4630-", b"");
        assert_eq!((r.action.as_str(), r.index), ("allow", Some(1)));
        let e = export("deny", &[a, b]);
        assert_eq!(e["rules"].as_array().map(Vec::len), Some(1));
        assert_eq!(e["rules"][0], json!({ "id": 2, "name": "b", "action": "allow", "pid_prefix": "-qB" }));
        let v = decide("deny", &presets(), b"-qB4630-a1b2c3d4e5f6", b"qBittorrent/4.6.3");
        assert_eq!(v.version, Some(vec![4, 6, 3, 0]));
        // 实测 Deluge 2.1.2（libtorrent 2.0.10）
        let v = decide("deny", &presets(), b"-DE212s-a1b2c3d4e5f6", b"Deluge/2.1.2.dev0 libtorrent/2.0.10.0");
        assert_eq!((v.action.as_str(), v.version), ("allow", Some(vec![2, 1, 2, 28])));
    }

    #[test]
    fn peer_ids_and_legacy() {
        assert_eq!(parse_peer_id("-qB4630-a1b2c3d4e5f6").map(|b| b.len()), Some(20));
        assert_eq!(parse_peer_id(&hex::encode(b"-qB4630-a1b2c3d4e5f6")), Some(b"-qB4630-a1b2c3d4e5f6".to_vec()));
        assert_eq!(parse_peer_id("%00%FF"), Some(vec![0, 255]));
        assert!(parse_peer_id("-qB4630-a1b2c3d4e5f6x").is_none());
        assert!(parse_peer_id("%G0").is_none());
        assert_eq!(escape(b"-UT\x00%"), "-UT%00%25");
        let (d, r) = from_legacy(&json!({}), &[("qBittorrent", "-qB")]);
        assert_eq!((d.as_str(), r.len(), r[0].pid_prefix.as_deref()), ("deny", 1, Some("-qB")));
        let (d, r) = from_legacy(
            &json!({ "allow_unknown_clients": true, "clients": [{ "name": "x", "peer_id_prefix": "-XX" }, { "name": "", "peer_id_prefix": "-YY" }] }),
            &[],
        );
        assert_eq!((d.as_str(), r.len()), ("allow", 1));
    }
}
