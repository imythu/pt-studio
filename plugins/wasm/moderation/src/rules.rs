//! 审核的纯逻辑（moderation LLD §1.1、§1.5）：初始状态规则、状态只能变严、敏感词匹配与遮蔽。

use serde_json::Value;

/// 状态严格程度（approved 最宽松；candidate 与 pending 同级）
pub fn strictness(s: &str) -> u8 {
    match s {
        "approved" => 0,
        "candidate" | "pending" => 1,
        _ => 0,
    }
}

fn cmp(field: &Value, op: &str, value: &Value) -> bool {
    let num = |v: &Value| v.as_f64();
    match op {
        "eq" => field == value,
        "ne" => field != value,
        "lt" => num(field).zip(num(value)).is_some_and(|(a, b)| a < b),
        "lte" => num(field).zip(num(value)).is_some_and(|(a, b)| a <= b),
        "gt" => num(field).zip(num(value)).is_some_and(|(a, b)| a > b),
        "gte" => num(field).zip(num(value)).is_some_and(|(a, b)| a >= b),
        // 字段为数组（如 roles）：有交集；字段为标量：属于列表
        "in" | "not_in" => {
            let list = value.as_array().cloned().unwrap_or_default();
            let hit = match field {
                Value::Array(a) => a.iter().any(|x| list.contains(x)),
                x => list.contains(x),
            };
            hit == (op == "in")
        }
        _ => false,
    }
}

/// 按顺序匹配第一条规则；返回 (规则 ID, 状态)。规则格式非法的条目跳过
pub fn evaluate(rules: &Value, facts: &Value) -> Option<(String, String)> {
    for r in rules.as_array().into_iter().flatten() {
        let status = r["status"]
            .as_str()
            .filter(|s| matches!(*s, "approved" | "pending" | "candidate"))?;
        let conds = r["when"].as_array()?;
        if conds.iter().all(|c| {
            c["field"]
                .as_str()
                .is_some_and(|f| cmp(&facts[f], c["op"].as_str().unwrap_or(""), &c["value"]))
        }) {
            return Some((r["id"].as_str().unwrap_or("").to_owned(), status.to_owned()));
        }
    }
    None
}

/// 规范化：小写、去零宽字符与空白（敏感词匹配用）
fn normalize(s: &str) -> (Vec<char>, Vec<usize>) {
    // 返回规范化字符序列与其在原文中的字符下标
    let mut out = Vec::new();
    let mut idx = Vec::new();
    for (i, c) in s.chars().enumerate() {
        if matches!(c, '\u{200b}'..='\u{200f}' | '\u{feff}' | '\u{2060}') || c.is_whitespace() {
            continue;
        }
        for l in c.to_lowercase() {
            out.push(l);
            idx.push(i);
        }
    }
    (out, idx)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub pattern: String,
    pub action: String,
}

/// 查找命中；返回 (最严重的动作, 命中的原文字符区间)
pub fn find(words: &[Word], text: &str) -> Option<(String, Vec<(usize, usize)>)> {
    let (norm, idx) = normalize(text);
    let mut worst: Option<&str> = None;
    let mut spans = Vec::new();
    let rank = |a: &str| match a {
        "block" => 3,
        "review" => 2,
        "mask" => 1,
        _ => 0,
    };
    for w in words {
        let (p, _) = normalize(&w.pattern);
        if p.is_empty() || p.len() > norm.len() {
            continue;
        }
        for start in 0..=norm.len() - p.len() {
            if norm[start..start + p.len()] == p[..] {
                spans.push((idx[start], idx[start + p.len() - 1]));
                if worst.is_none_or(|x| rank(&w.action) > rank(x)) {
                    worst = Some(&w.action);
                }
            }
        }
    }
    worst.map(|w| (w.to_owned(), spans))
}

/// 遮蔽命中片段（按字符替换为 `*`）
pub fn mask(text: &str, spans: &[(usize, usize)]) -> String {
    text.chars()
        .enumerate()
        .map(|(i, c)| {
            if spans.iter().any(|(a, b)| i >= *a && i <= *b) && !c.is_whitespace() {
                '*'
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rules() {
        let rules = json!([
            { "id": "trusted_role", "when": [{ "field": "roles", "op": "in", "value": ["uploader"] }], "status": "approved" },
            { "id": "veteran", "when": [{ "field": "approved_uploads", "op": "gte", "value": 20 }, { "field": "rejected_ratio", "op": "lt", "value": 0.1 }], "status": "approved" },
            { "id": "new_uploader", "when": [{ "field": "approved_uploads", "op": "lt", "value": 3 }], "status": "candidate" },
        ]);
        let f = |roles: Value, ok: i64, ratio: f64| json!({ "roles": roles, "approved_uploads": ok, "rejected_ratio": ratio });
        assert_eq!(
            evaluate(&rules, &f(json!(["user", "uploader"]), 0, 0.0)).map(|x| x.1),
            Some("approved".into())
        );
        assert_eq!(
            evaluate(&rules, &f(json!(["user"]), 30, 0.05)).map(|x| x.0),
            Some("veteran".into())
        );
        assert_eq!(
            evaluate(&rules, &f(json!(["user"]), 1, 0.0)).map(|x| x.1),
            Some("candidate".into())
        );
        assert_eq!(evaluate(&rules, &f(json!(["user"]), 10, 0.0)), None);
        assert!(strictness("pending") > strictness("approved"));
    }

    #[test]
    fn words() {
        let w = vec![
            Word {
                pattern: "坏词".into(),
                action: "mask".into(),
            },
            Word {
                pattern: "BadSite".into(),
                action: "block".into(),
            },
        ];
        let (a, spans) = find(&w, "这是 坏 词，去 badsite.com").unwrap_or_default();
        assert_eq!(a, "block");
        assert_eq!(
            mask("这是 坏 词，去 badsite.com", &spans),
            "这是 * *，去 *******.com"
        );
        assert_eq!(find(&w[..1], "坏\u{200b}词").map(|x| x.0), Some("mask".into()));
        assert_eq!(find(&w, "正常内容"), None);
    }
}
