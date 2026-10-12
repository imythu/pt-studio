//! 资源类型插件共用工具（content-types-common §2、§5）：标题模板、字段错误、外部 ID 提取。纯函数。

pub mod episodes;
pub mod metadata;
pub mod specs;

use serde_json::{Value, json};

/// 模板片段
#[derive(Debug, Clone, PartialEq)]
enum Part {
    Lit(String),
    /// `{field}` 或 `{field?前缀|后缀}`（字段为空时整段省略）
    Field {
        name: String,
        pre: String,
        post: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Template(Vec<Part>);

impl Template {
    /// 解析模板；`fields` 给出允许引用的字段（`None` 不检查）
    pub fn parse(src: &str, fields: Option<&[&str]>) -> Result<Self, String> {
        let mut parts = Vec::new();
        let mut lit = String::new();
        let mut chars = src.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '{' {
                lit.push(c);
                continue;
            }
            let mut inner = String::new();
            let mut closed = false;
            for d in chars.by_ref() {
                if d == '}' {
                    closed = true;
                    break;
                }
                inner.push(d);
            }
            if !closed {
                return Err("unclosed `{`".into());
            }
            if !lit.is_empty() {
                parts.push(Part::Lit(std::mem::take(&mut lit)));
            }
            let (name, rest) = inner
                .split_once('?')
                .map_or((inner.as_str(), None), |(n, r)| (n, Some(r)));
            let name = name.trim();
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            {
                return Err(format!("invalid field name `{name}`"));
            }
            if fields.is_some_and(|f| !f.contains(&name)) {
                return Err(format!("unknown field `{name}`"));
            }
            let (pre, post) = match rest {
                Some(r) => r.split_once('|').map_or((r.to_owned(), String::new()), |(a, b)| {
                    (a.to_owned(), b.to_owned())
                }),
                None => (String::new(), String::new()),
            };
            parts.push(Part::Field {
                name: name.to_owned(),
                pre,
                post,
            });
        }
        if !lit.is_empty() {
            parts.push(Part::Lit(lit));
        }
        Ok(Self(parts))
    }

    /// 渲染；`label` 把字段值转换为显示文本（数组元素以空格连接），结果合并多余空格
    pub fn render(&self, fields: &Value, label: &dyn Fn(&str, &Value) -> Option<String>) -> String {
        let mut out = String::new();
        for p in &self.0 {
            match p {
                Part::Lit(s) => out.push_str(s),
                Part::Field { name, pre, post } => {
                    let v = &fields[name.as_str()];
                    let text = match v {
                        Value::Array(a) => {
                            let xs: Vec<String> = a.iter().filter_map(|x| label(name, x)).collect();
                            (!xs.is_empty()).then(|| xs.join(" "))
                        }
                        Value::Null => None,
                        x => label(name, x),
                    };
                    match text.filter(|t| !t.is_empty()) {
                        Some(t) => {
                            out.push_str(pre);
                            out.push_str(&t);
                            out.push_str(post);
                        }
                        // 字段为空：去掉前面紧邻的连接符（如 `-{release_group?}` 的 `-`）
                        None if pre.is_empty() && post.is_empty() => {
                            while out.ends_with(['-', '.', '@']) {
                                out.pop();
                            }
                        }
                        None => {}
                    }
                }
            }
        }
        out.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .trim_end_matches(['-', ' '])
            .to_owned()
    }
}

/// 字符串 / 数字值的默认显示文本
pub fn plain(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(true) => Some("yes".into()),
        _ => None,
    }
}

/// 字段错误收集，输出 `details.errors[]`（JSON Pointer）
#[derive(Debug, Default)]
pub struct Errors(Vec<(String, String, String)>);

impl Errors {
    pub fn add(&mut self, field: &str, code: &str, message: &str) {
        self.0
            .push((format!("/{field}"), code.to_owned(), message.to_owned()));
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// 第一个错误的字段名（错误码 `<plugin>.<field>_invalid`）
    pub fn first_field(&self) -> Option<&str> {
        self.0.first().map(|(p, _, _)| p.trim_start_matches('/'))
    }

    pub fn details(&self) -> Value {
        json!({ "errors": self.0.iter().map(|(p, c, m)| json!({ "path": p, "code": c, "message": m })).collect::<Vec<_>>() })
    }
}

/// 从链接或裸 ID 中提取外部 ID
pub fn imdb_id(s: &str) -> Option<String> {
    s.match_indices("tt").find_map(|(i, _)| {
        let digits: String = s[i + 2..].chars().take_while(char::is_ascii_digit).collect();
        (7..=9).contains(&digits.len()).then(|| format!("tt{digits}"))
    })
}

/// `https://www.themoviedb.org/movie/603-the-matrix`、`603` → `603`（`kind` 为 movie / tv）
pub fn tmdb_id(s: &str, kind: &str) -> Option<u64> {
    let s = s.trim();
    if let Ok(n) = s.parse() {
        return Some(n);
    }
    let i = s.find(&format!("/{kind}/"))? + kind.len() + 2;
    s[i..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

/// `https://movie.douban.com/subject/1292052/`、`1292052` → `1292052`
pub fn douban_id(s: &str) -> Option<String> {
    let s = s.trim();
    if !s.is_empty() && s.len() <= 12 && s.bytes().all(|b| b.is_ascii_digit()) {
        return Some(s.to_owned());
    }
    let i = s.find("/subject/")? + 9;
    let d: String = s[i..].chars().take_while(char::is_ascii_digit).collect();
    (!d.is_empty()).then_some(d)
}

/// 分组用的标题规范化：小写、只保留字母数字（含 CJK），空白合并为单个 `-`
pub fn norm_key(s: &str) -> String {
    let mut out = String::new();
    for w in s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()) {
        if !out.is_empty() {
            out.push('-');
        }
        out.push_str(&w.to_lowercase());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates() {
        let t = Template::parse(
            "{title} {year} {edition?[|] }{resolution} {hdr?| }{video_codec} -{release_group}",
            None,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let f = json!({ "title": "Heat", "year": 1995, "resolution": "1080p", "hdr": ["DV", "HDR10"], "video_codec": "HEVC", "release_group": "ABC" });
        assert_eq!(
            t.render(&f, &|_, v| plain(v)),
            "Heat 1995 1080p DV HDR10 HEVC -ABC"
        );
        let g = json!({ "title": "Heat", "year": 1995, "edition": "Director's Cut", "resolution": "720p", "video_codec": "AVC" });
        assert_eq!(
            t.render(&g, &|_, v| plain(v)),
            "Heat 1995 [Director's Cut] 720p AVC"
        );
        assert!(Template::parse("{nope}", Some(&["title"])).is_err());
        assert!(Template::parse("{title", None).is_err());
    }

    #[test]
    fn ids() {
        assert_eq!(
            imdb_id("https://www.imdb.com/title/tt0111161/").as_deref(),
            Some("tt0111161")
        );
        assert_eq!(imdb_id("tt12"), None);
        assert_eq!(
            tmdb_id("https://www.themoviedb.org/movie/603-the-matrix", "movie"),
            Some(603)
        );
        assert_eq!(tmdb_id("603", "movie"), Some(603));
        assert_eq!(
            douban_id("https://movie.douban.com/subject/1292052/").as_deref(),
            Some("1292052")
        );
        assert_eq!(norm_key("The  Wandering Earth: II"), "the-wandering-earth-ii");
        assert_eq!(norm_key("流浪地球 2"), "流浪地球-2");
    }
}
