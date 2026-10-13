//! 评论的纯逻辑（comments LLD §1.2～§1.4）：目标解析、重复内容指纹（simhash）、引用前缀。

/// 支持的目标类型
pub const KINDS: &[&str] = &["torrent", "group", "request"];

/// `torrent:5501` → ("torrent", 5501)
pub fn parse_target(t: &str) -> Option<(&str, i64)> {
    let (k, id) = t.split_once(':')?;
    let id: i64 = id.parse().ok().filter(|i| *i > 0)?;
    KINDS.iter().find(|x| **x == k).map(|k| (*k, id))
}

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// 规范化（去空白与标点、小写）后按字符三元组计算 64 位 simhash
pub fn simhash(text: &str) -> u64 {
    let norm: Vec<char> = text
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    if norm.is_empty() {
        return 0;
    }
    let grams: Vec<String> = if norm.len() < 3 {
        vec![norm.iter().collect()]
    } else {
        norm.windows(3).map(|w| w.iter().collect()).collect()
    };
    let mut v = [0i32; 64];
    for g in &grams {
        let h = fnv(g);
        for (bit, slot) in v.iter_mut().enumerate() {
            if h >> bit & 1 == 1 {
                *slot += 1;
            } else {
                *slot -= 1;
            }
        }
    }
    v.iter()
        .enumerate()
        .fold(0u64, |acc, (bit, n)| if *n > 0 { acc | 1 << bit } else { acc })
}

pub fn near_duplicate(a: u64, b: u64) -> bool {
    (a ^ b).count_ones() <= 3
}

/// 引用前缀（截断 500 字）
pub fn quote_prefix(markup: &str, name: &str, source: &str) -> String {
    let s: String = source.chars().take(500).collect();
    if markup == "bbcode" {
        format!("[quote={name}]{s}[/quote]\n")
    } else {
        let quoted: Vec<String> = s.lines().map(|l| format!("> {l}")).collect();
        format!("> @{name} 写道：\n{}\n\n", quoted.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_and_fingerprints() {
        assert_eq!(parse_target("torrent:5501"), Some(("torrent", 5501)));
        assert_eq!(parse_target("user:1"), None);
        assert_eq!(parse_target("torrent:-1"), None);
        let a = simhash("感谢分享，这个版本的画质非常好，字幕也很准确！");
        let b = simhash("感谢分享！这个版本的画质非常好，字幕也很准确");
        let c = simhash("下载速度很慢，有人能帮忙保种吗？做种时间不够");
        assert!(near_duplicate(a, b), "{:b}", a ^ b);
        assert!(!near_duplicate(a, c));
        assert!(
            quote_prefix("markdown", "bob", "第一行\n第二行")
                .starts_with("> @bob 写道：\n> 第一行\n> 第二行")
        );
        assert_eq!(quote_prefix("bbcode", "bob", "x"), "[quote=bob]x[/quote]\n");
    }
}
