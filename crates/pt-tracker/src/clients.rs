//! 客户端规则（`trk:clients`，tracker-bridge LLD §1.5，与 tracker-bridge 的规则测试接口语义完全相同）：
//! 有序规则 + 默认动作；条件为 peer_id 前缀 / 正则（可带 `ver` 组与最低 / 最高版本）、UA 前缀 / 正则，全部满足才命中。
//! 两边共用测试向量 `docs/plugins/tracker-bridge/client-rules-vectors.json`。
//!
//! 被拒客户端计数 [`Denied`]：本地累加，每 10 秒写 `trk:stats:denied:<UTC yyyymmdd>`（ZSET，8 天过期）。

use std::collections::HashMap;
use std::sync::Mutex;

use regex::{Regex, RegexBuilder};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Allow,
    Deny,
}

#[derive(Debug)]
struct Rule {
    id: u64,
    action: Action,
    pid_prefix: Option<Vec<u8>>,
    pid_regex: Option<Regex>,
    min_ver: Option<Vec<u32>>,
    max_ver: Option<Vec<u32>>,
    ua_prefix: Option<Vec<u8>>,
    ua_regex: Option<Regex>,
}

#[derive(Debug)]
pub struct Rules {
    default: Action,
    rules: Vec<Rule>,
}

impl Default for Rules {
    /// 还没有加载到规则集时拒绝所有客户端
    fn default() -> Self {
        Self {
            default: Action::Deny,
            rules: Vec::new(),
        }
    }
}

#[derive(Deserialize)]
struct RawRules {
    default: String,
    #[serde(default)]
    rules: Vec<RawRule>,
}

#[derive(Deserialize)]
struct RawRule {
    #[serde(default)]
    id: u64,
    #[serde(default = "allow")]
    action: String,
    // 旧版导出格式的字段名
    #[serde(alias = "peer_id_prefix")]
    pid_prefix: Option<String>,
    pid_regex: Option<String>,
    min_ver: Option<String>,
    max_ver: Option<String>,
    ua_prefix: Option<String>,
    ua_regex: Option<String>,
}

fn allow() -> String {
    "allow".into()
}

fn action(s: &str) -> Result<Action, String> {
    match s {
        "allow" => Ok(Action::Allow),
        "deny" => Ok(Action::Deny),
        other => Err(format!("unknown action `{other}`")),
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

fn regex(s: &str) -> Result<Regex, String> {
    RegexBuilder::new(s)
        .size_limit(64 << 10)
        .build()
        .map_err(|e| e.to_string())
}

/// `1.2.3` → `[1, 2, 3]`（最多 4 段）
fn version(s: &str) -> Option<Vec<u32>> {
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

/// 右侧补 0 到同长后逐项比较
fn cmp(a: &[u32], b: &[u32]) -> std::cmp::Ordering {
    let n = a.len().max(b.len());
    let at = |v: &[u32], i: usize| v.get(i).copied().unwrap_or(0);
    (0..n)
        .map(|i| at(a, i).cmp(&at(b, i)))
        .find(|o| o.is_ne())
        .unwrap_or(std::cmp::Ordering::Equal)
}

/// peer_id 的 Latin-1 文本（每字节 → U+00XX）
fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}

impl Rules {
    /// 编译 `trk:clients`；任一条规则无效则整体失败（调用方保留旧规则集）
    pub fn compile(json: &str) -> Result<Self, String> {
        let raw: RawRules = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut rules = Vec::with_capacity(raw.rules.len());
        for r in raw.rules {
            let bad = |what: &str| format!("rule {}: invalid {what}", r.id);
            let pid_prefix = r
                .pid_prefix
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| {
                    unescape(s)
                        .filter(|b| b.len() <= 20)
                        .ok_or_else(|| bad("pid_prefix"))
                })
                .transpose()?;
            let pid_regex = r
                .pid_regex
                .as_deref()
                .map(|s| regex(s).map_err(|e| format!("{}: {e}", bad("pid_regex"))))
                .transpose()?;
            let min_ver = r
                .min_ver
                .as_deref()
                .map(|s| version(s).ok_or_else(|| bad("min_ver")))
                .transpose()?;
            let max_ver = r
                .max_ver
                .as_deref()
                .map(|s| version(s).ok_or_else(|| bad("max_ver")))
                .transpose()?;
            let ua_regex = r
                .ua_regex
                .as_deref()
                .map(|s| regex(s).map_err(|e| format!("{}: {e}", bad("ua_regex"))))
                .transpose()?;
            rules.push(Rule {
                id: r.id,
                action: action(&r.action)?,
                pid_prefix,
                pid_regex,
                min_ver,
                max_ver,
                ua_prefix: r.ua_prefix.filter(|s| !s.is_empty()).map(String::into_bytes),
                ua_regex,
            });
        }
        Ok(Self {
            default: action(&raw.default)?,
            rules,
        })
    }

    /// 第一条全部条件满足的规则决定动作；都不满足时按默认动作。`ua` 缺失按空串
    pub fn decide(&self, peer_id: &[u8], ua: &[u8]) -> (Action, Option<u64>) {
        for r in &self.rules {
            if r.pid_prefix.as_ref().is_some_and(|p| !peer_id.starts_with(p)) {
                continue;
            }
            if r.ua_prefix.as_ref().is_some_and(|p| !ua.starts_with(p)) {
                continue;
            }
            if r.ua_regex
                .as_ref()
                .is_some_and(|re| !re.is_match(&String::from_utf8_lossy(ua)))
            {
                continue;
            }
            if let Some(re) = &r.pid_regex {
                let text = latin1(peer_id);
                let Some(m) = re.captures(&text) else { continue };
                if r.min_ver.is_some() || r.max_ver.is_some() {
                    let Some(digits) = m.name("ver").and_then(|v| {
                        v.as_str()
                            .chars()
                            .map(|c| c.to_digit(36))
                            .collect::<Option<Vec<u32>>>()
                    }) else {
                        continue;
                    };
                    if r.min_ver.as_ref().is_some_and(|v| cmp(&digits, v).is_lt())
                        || r.max_ver.as_ref().is_some_and(|v| cmp(&digits, v).is_gt())
                    {
                        continue;
                    }
                }
            }
            return (r.action, Some(r.id));
        }
        (self.default, None)
    }
}

/// 不同 member 的上限，超过后计入 `~overflow|`
const DENIED_MAX_MEMBERS: usize = 10_000;

/// 被拒客户端计数（本地累加，定期刷到 Redis）
#[derive(Default)]
pub struct Denied {
    counts: Mutex<HashMap<String, u64>>,
}

/// ZSET member：`<peer_id 前 8 字节转义>|<UA 前 32 字节>`
pub fn denied_member(peer_id: &[u8], ua: &[u8]) -> String {
    let mut m = String::new();
    for &b in peer_id.iter().take(8) {
        if (0x21..=0x7e).contains(&b) && b != b'%' && b != b'|' {
            m.push(char::from(b));
        } else {
            m.push_str(&format!("%{b:02X}"));
        }
    }
    m.push('|');
    let ua = String::from_utf8_lossy(ua);
    let mut len = 0;
    for c in ua.chars().filter(|c| !c.is_control()) {
        len += c.len_utf8();
        if len > 32 {
            break;
        }
        m.push(c);
    }
    m
}

impl Denied {
    pub fn record(&self, peer_id: &[u8], ua: &[u8]) {
        let member = denied_member(peer_id, ua);
        let mut g = self
            .counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = if g.len() >= DENIED_MAX_MEMBERS && !g.contains_key(&member) {
            "~overflow|".to_owned()
        } else {
            member
        };
        *g.entry(key).or_default() += 1;
    }

    /// 取出当前计数（清空本地表）
    pub fn take(&self) -> HashMap<String, u64> {
        std::mem::take(
            &mut *self
                .counts
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    /// 写入失败时合并回本地表
    pub fn restore(&self, counts: HashMap<String, u64>) {
        let mut g = self
            .counts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (k, n) in counts {
            *g.entry(k).or_default() += n;
        }
    }
}

/// UTC 日期 `yyyymmdd`
pub fn utc_day(unix_secs: u64) -> String {
    // civil_from_days（Howard Hinnant）
    let z = i64::try_from(unix_secs / 86_400).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 与 tracker-bridge 共用的测试向量
    const VECTORS: &str = include_str!("../../../docs/plugins/tracker-bridge/client-rules-vectors.json");

    #[test]
    fn vectors() {
        let v: serde_json::Value = serde_json::from_str(VECTORS).unwrap();
        for g in v["groups"].as_array().unwrap() {
            let rules = Rules::compile(&g["rules"].to_string()).unwrap();
            for c in g["cases"].as_array().unwrap() {
                let pid = unescape(c["peer_id"].as_str().unwrap()).unwrap();
                let ua = c["ua"].as_str().unwrap_or("");
                let (a, id) = rules.decide(&pid, ua.as_bytes());
                let want = if c["action"] == "allow" {
                    Action::Allow
                } else {
                    Action::Deny
                };
                assert_eq!((a, id), (want, c["rule_id"].as_u64()), "{}: {c}", g["name"]);
            }
        }
        for bad in v["invalid"].as_array().unwrap() {
            assert!(
                Rules::compile(&bad["rules"].to_string()).is_err(),
                "{}",
                bad["name"]
            );
        }
    }

    #[test]
    fn legacy_format() {
        let r = Rules::compile(
            r#"{"default":"deny","rules":[{"peer_id_prefix":"-qB"},{"peer_id_prefix":"-XX","action":"deny"}]}"#,
        )
        .unwrap();
        assert_eq!(r.decide(b"-qB4650-abcdefghijkl", b"").0, Action::Allow);
        assert_eq!(r.decide(b"-XX0001-abcdefghijkl", b"").0, Action::Deny);
        assert_eq!(r.decide(b"-ZZ0001-abcdefghijkl", b"").0, Action::Deny);
        assert_eq!(Rules::default().decide(b"-qB", b"").0, Action::Deny);
    }

    #[test]
    fn denied() {
        assert_eq!(
            denied_member(b"-XL0012-\x01\xffabcdefghij", b"Xunlei|0.0.1\n"),
            "-XL0012-|Xunlei|0.0.1"
        );
        assert_eq!(denied_member(b"%|\x00abcdefgh", b""), "%25%7C%00abcde|");
        assert_eq!(
            denied_member(b"-qB", "下载器".repeat(5).as_bytes()).len(),
            3 + 1 + 30
        );
        let d = Denied::default();
        d.record(b"-XL0012-abcdefghijkl", b"x");
        d.record(b"-XL0012-zzzzzzzzzzzz", b"x");
        let t = d.take();
        assert_eq!(t.get("-XL0012-|x"), Some(&2));
        assert!(d.take().is_empty());
        d.restore(t);
        assert_eq!(d.take().len(), 1);
        assert_eq!(utc_day(0), "19700101");
        assert_eq!(utc_day(1_780_000_000), "20260528");
    }
}
