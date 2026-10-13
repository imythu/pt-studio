//! 知识库（ai-assistant LLD §1.1～§1.3 的纯逻辑）：Markdown 按二级标题切段、检索打分（拉丁词 + 中日韩字符二元组，
//! 类 BM25）、资料块拼装、引用标记校验。

use std::collections::{HashMap, HashSet};

/// 每段最多字符数（超长段落按空行再切）
pub const CHUNK_CHARS: usize = 1500;

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub id: String,
    pub heading: String,
    pub body: String,
}

/// 文档 → 段落：`## 标题` 开新段；标题前的引言归入第一段（标题为文档标题）
pub fn split(doc_id: i64, title: &str, md: &str) -> Vec<Chunk> {
    let mut sections: Vec<(String, String)> = vec![(title.to_owned(), String::new())];
    for line in md.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            sections.push((h.trim().to_owned(), String::new()));
        } else if let Some(last) = sections.last_mut() {
            last.1.push_str(line);
            last.1.push('\n');
        }
    }
    let mut out = Vec::new();
    for (heading, body) in sections {
        let body = body.trim();
        if body.is_empty() {
            continue;
        }
        // 超长：按空行累积切分
        let mut cur = String::new();
        let mut parts = Vec::new();
        for para in body.split("\n\n") {
            if !cur.is_empty() && cur.chars().count() + para.chars().count() > CHUNK_CHARS {
                parts.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push_str("\n\n");
            }
            cur.push_str(para);
        }
        if !cur.is_empty() {
            parts.push(cur);
        }
        for p in parts {
            let p: String = p.chars().take(CHUNK_CHARS * 2).collect();
            out.push(Chunk {
                id: format!("{doc_id}#{}", out.len() + 1),
                heading: heading.clone(),
                body: p,
            });
        }
    }
    out
}

fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
        || ('\u{3400}'..='\u{4dbf}').contains(&c)
        || ('\u{3040}'..='\u{30ff}').contains(&c)
}

/// 检索词：拉丁 / 数字词（小写，≥ 2 字符）与中日韩连续字符的二元组（单字也收入）
pub fn tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut cjk: Vec<char> = Vec::new();
    let flush_cjk = |cjk: &mut Vec<char>, out: &mut Vec<String>| {
        if cjk.len() == 1 {
            out.push(cjk[0].to_string());
        }
        for w in cjk.windows(2) {
            out.push(w.iter().collect());
        }
        cjk.clear();
    };
    for c in s.chars() {
        if is_cjk(c) {
            if word.chars().count() >= 2 {
                out.push(std::mem::take(&mut word).to_lowercase());
            }
            word.clear();
            cjk.push(c);
        } else if c.is_alphanumeric() {
            flush_cjk(&mut cjk, &mut out);
            word.push(c);
        } else {
            flush_cjk(&mut cjk, &mut out);
            if word.chars().count() >= 2 {
                out.push(word.to_lowercase());
            }
            word.clear();
        }
    }
    flush_cjk(&mut cjk, &mut out);
    if word.chars().count() >= 2 {
        out.push(word.to_lowercase());
    }
    out
}

/// 检索：返回得分最高的 `k` 段（得分为 0 的不返回）。标题中的命中权重加倍
pub fn rank<'a>(question: &str, chunks: &'a [(Chunk, String)], k: usize) -> Vec<&'a (Chunk, String)> {
    let q: HashSet<String> = tokens(question).into_iter().collect();
    if q.is_empty() {
        return vec![];
    }
    let docs: Vec<(HashMap<String, usize>, HashSet<String>, usize)> = chunks
        .iter()
        .map(|(c, title)| {
            let toks = tokens(&c.body);
            let mut tf: HashMap<String, usize> = HashMap::new();
            for t in &toks {
                *tf.entry(t.clone()).or_default() += 1;
            }
            let head: HashSet<String> = tokens(&format!("{title} {}", c.heading)).into_iter().collect();
            (tf, head, toks.len().max(1))
        })
        .collect();
    let n = chunks.len() as f64;
    let avg = docs.iter().map(|d| d.2 as f64).sum::<f64>() / n.max(1.0);
    let mut scored: Vec<(f64, usize)> = docs
        .iter()
        .enumerate()
        .map(|(i, (tf, head, len))| {
            let mut s = 0.0;
            for t in &q {
                let df = docs
                    .iter()
                    .filter(|d| d.0.contains_key(t) || d.1.contains(t))
                    .count() as f64;
                let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                let f = *tf.get(t).unwrap_or(&0) as f64;
                let body = f * 2.2 / (f + 1.2 * (0.25 + 0.75 * *len as f64 / avg.max(1.0)));
                s += idf * (body + if head.contains(t) { 2.0 } else { 0.0 });
            }
            (s, i)
        })
        .filter(|(s, _)| *s > 0.0)
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.into_iter().take(k).map(|(_, i)| &chunks[i]).collect()
}

/// `<docs>` 资料块（每段带 `[doc:<id>]` 标记）
pub fn docs_block(hits: &[&(Chunk, String)]) -> String {
    let mut s = String::from("<docs>\n");
    for (c, title) in hits {
        s.push_str(&format!(
            "[doc:{}] {title} / {}\n{}\n\n",
            c.id,
            c.heading,
            c.body.replace("</docs>", "<\\/docs>")
        ));
    }
    s.push_str("</docs>");
    s
}

/// 只保留属于本轮检索结果的引用标记；返回 (清理后的答案, 引用 ID 按出现顺序去重)
pub fn check_citations(answer: &str, allowed: &[String]) -> (String, Vec<String>) {
    let mut out = String::with_capacity(answer.len());
    let mut cited: Vec<String> = Vec::new();
    let mut rest = answer;
    while let Some(p) = rest.find("[doc:") {
        out.push_str(&rest[..p]);
        let tail = &rest[p..];
        let Some(end) = tail.find(']') else {
            out.push_str(tail);
            rest = "";
            break;
        };
        let id = tail[5..end].trim();
        if allowed.iter().any(|a| a == id) {
            out.push_str(&tail[..=end]);
            if !cited.iter().any(|c| c == id) {
                cited.push(id.to_owned());
            }
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    // 去掉标记后可能留下的多余空格
    let cleaned = out.replace("  ", " ").replace(" 。", "。").replace(" ，", "，");
    (cleaned.trim().to_owned(), cited)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULES: &str = "本站规则总览。\n\n## 分享率要求\n\n下载量超过 20 GB 后，分享率低于 0.5 会进入警告期，警告期内仍未达标将禁止下载。\n\n## 保种与 H&R\n\n下载完成后需要做种 72 小时，否则记为 H&R。";

    #[test]
    fn split_and_rank() {
        let chunks = split(12, "站点规则", RULES);
        assert_eq!(
            chunks.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            vec!["12#1", "12#2", "12#3"]
        );
        assert_eq!(chunks[1].heading, "分享率要求");
        let all: Vec<(Chunk, String)> = chunks.into_iter().map(|c| (c, "站点规则".to_owned())).collect();
        let hits = rank("为什么我不能下载？分享率", &all, 2);
        assert_eq!(hits[0].0.id, "12#2");
        let hits = rank("H&R 要做种多久", &all, 1);
        assert_eq!(hits[0].0.id, "12#3");
        assert!(rank("!!!", &all, 3).is_empty());
        let long = "a".repeat(1000) + "\n\n" + &"b".repeat(1000);
        assert_eq!(split(1, "t", &long).len(), 2);
    }

    #[test]
    fn tokens_and_citations() {
        assert_eq!(tokens("H&R 规则 v2"), vec!["规则", "v2"]);
        assert_eq!(tokens("分享率"), vec!["分享", "享率"]);
        let (a, c) = check_citations(
            "警告期内禁止下载 [doc:12#2]，详见 [doc:99#1]。",
            &["12#2".to_owned()],
        );
        assert_eq!(
            (a.as_str(), c),
            ("警告期内禁止下载 [doc:12#2]，详见。", vec!["12#2".to_owned()])
        );
    }
}
