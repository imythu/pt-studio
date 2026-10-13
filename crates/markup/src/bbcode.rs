//! BBCode → HTML（兼容 NexusPHP / Gazelle 常用标签）：有界解析（嵌套 ≤ 32）；未知标签、非法属性、多余的闭合标签按原文输出；
//! 未闭合的标签在结尾自动闭合；属性值只接受白名单格式。

use crate::Out;

const MAX_DEPTH: usize = 32;
const COLORS: &[&str] = &[
    "black",
    "white",
    "red",
    "green",
    "blue",
    "yellow",
    "orange",
    "purple",
    "pink",
    "gray",
    "grey",
    "brown",
    "cyan",
    "magenta",
    "navy",
    "teal",
    "olive",
    "maroon",
    "silver",
    "gold",
    "darkred",
    "darkgreen",
    "darkblue",
    "darkorange",
    "skyblue",
];

enum Tok<'a> {
    Text(&'a str),
    Open {
        name: String,
        arg: Option<&'a str>,
        raw: &'a str,
    },
    Close {
        name: String,
        raw: &'a str,
    },
}

fn tokenize(s: &str) -> Vec<Tok<'_>> {
    let mut out = Vec::new();
    let mut text_start = 0;
    let mut i = 0;
    let b = s.as_bytes();
    while i < b.len() {
        if b[i] == b'['
            && let Some(rel) = s[i..].find(']').filter(|r| *r <= 300)
        {
            let inner = &s[i + 1..i + rel];
            let raw = &s[i..=i + rel];
            let tok = if let Some(n) = inner.strip_prefix('/') {
                (!n.is_empty() && n.bytes().all(|c| c.is_ascii_alphanumeric())).then(|| Tok::Close {
                    name: n.to_ascii_lowercase(),
                    raw,
                })
            } else {
                let (n, arg) = inner.split_once('=').map_or((inner, None), |(n, a)| (n, Some(a)));
                (!n.is_empty() && n.len() <= 12 && (n == "*" || n.bytes().all(|c| c.is_ascii_alphanumeric())))
                    .then(|| Tok::Open {
                        name: n.to_ascii_lowercase(),
                        arg: arg.map(|a| a.trim_matches(['"', '\''])),
                        raw,
                    })
            };
            if let Some(t) = tok {
                if text_start < i {
                    out.push(Tok::Text(&s[text_start..i]));
                }
                out.push(t);
                i += rel + 1;
                text_start = i;
                continue;
            }
        }
        i += 1;
    }
    if text_start < s.len() {
        out.push(Tok::Text(&s[text_start..]));
    }
    out
}

fn color_ok(c: &str) -> bool {
    let c = c.trim();
    if let Some(h) = c.strip_prefix('#') {
        return matches!(h.len(), 3 | 6) && h.bytes().all(|b| b.is_ascii_hexdigit());
    }
    COLORS.contains(&c.to_ascii_lowercase().as_str())
}

fn font_size(n: &str) -> Option<&'static str> {
    Some(match n.trim() {
        "1" => "0.75em",
        "2" => "0.875em",
        "3" => "1em",
        "4" => "1.15em",
        "5" => "1.3em",
        "6" => "1.6em",
        "7" => "2em",
        _ => return None,
    })
}

struct Open {
    name: String,
    close: &'static str,
    item_open: bool,
}

/// 文本：换行转 `<br>`
fn text(out: &mut Out<'_, '_>, s: &str, mentions: bool) {
    for (i, line) in s.split('\n').enumerate() {
        if i > 0 {
            out.raw("<br>");
        }
        if !line.is_empty() {
            out.text(line, mentions);
        }
    }
}

pub(crate) fn render(src: &str, out: &mut Out<'_, '_>) {
    let src = src.replace("\r\n", "\n");
    let toks = tokenize(&src);
    let mut stack: Vec<Open> = Vec::new();
    let mut quote_depth = 0u8;
    let mut i = 0;
    // 取到匹配闭合标签为止的原文（code / url / img 等内容不再解析）
    let take_until = |toks: &[Tok<'_>], from: usize, name: &str| -> Option<(String, usize)> {
        let mut s = String::new();
        for (j, t) in toks.iter().enumerate().skip(from) {
            match t {
                Tok::Close { name: n, .. } if n == name => return Some((s, j)),
                Tok::Text(x) => s.push_str(x),
                Tok::Open { raw, .. } | Tok::Close { raw, .. } => s.push_str(raw),
            }
        }
        None
    };
    while i < toks.len() {
        match &toks[i] {
            Tok::Text(t) => text(out, t, true),
            Tok::Open { name, arg, raw } => {
                let name = name.as_str();
                // 内容原样（不解析）的标签
                if matches!(name, "code" | "pre" | "mediainfo" | "url" | "img")
                    && (name != "url" || arg.is_none())
                {
                    match take_until(&toks, i + 1, name) {
                        Some((content, end)) => {
                            match name {
                                "url" => {
                                    if out.link_ok(&content) {
                                        out.open_link(&content);
                                        out.text(content.trim(), false);
                                        out.raw("</a>");
                                    } else {
                                        out.text(&content, false);
                                    }
                                }
                                "img" => {
                                    let size =
                                        arg.and_then(|a| a.split_once(['x', 'X'])).and_then(|(w, h)| {
                                            Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
                                        });
                                    out.image(&content, "", size);
                                }
                                _ => {
                                    out.raw(if name == "mediainfo" {
                                        "<pre class=\"mediainfo\"><code>"
                                    } else {
                                        "<pre><code>"
                                    });
                                    out.text(content.trim_matches('\n'), false);
                                    out.raw("</code></pre>");
                                }
                            }
                            i = end + 1;
                            continue;
                        }
                        None => {
                            text(out, raw, false);
                            i += 1;
                            continue;
                        }
                    }
                }
                if name == "hr" {
                    out.raw("<hr>");
                    i += 1;
                    continue;
                }
                if name == "*" {
                    if let Some(top) = stack.last_mut().filter(|t| t.name == "list") {
                        if top.item_open {
                            out.raw("</li>");
                        }
                        top.item_open = true;
                        out.raw("<li>");
                    } else {
                        text(out, raw, false);
                    }
                    i += 1;
                    continue;
                }
                let open: Option<(String, &'static str)> = if stack.len() >= MAX_DEPTH {
                    None
                } else {
                    match (name, arg) {
                        ("b", None) => Some(("<strong>".into(), "</strong>")),
                        ("i", None) => Some(("<em>".into(), "</em>")),
                        ("u", None) => Some(("<u>".into(), "</u>")),
                        ("s", None) => Some(("<del>".into(), "</del>")),
                        ("center", None) => Some(("<div class=\"center\">".into(), "</div>")),
                        ("list", _) => Some((
                            if arg.is_some() { "<ol>" } else { "<ul>" }.into(),
                            if arg.is_some() { "</ol>" } else { "</ul>" },
                        )),
                        ("url", Some(a)) if out.link_ok(a) => {
                            out.r.links += 1;
                            Some((
                                format!(
                                    "<a href=\"{}\" rel=\"nofollow noopener ugc\">",
                                    crate::escape(a.trim())
                                ),
                                "</a>",
                            ))
                        }
                        ("color", Some(c)) if color_ok(c) => Some((
                            format!("<span style=\"color: {}\">", crate::escape(c.trim())),
                            "</span>",
                        )),
                        ("size", Some(n)) => {
                            font_size(n).map(|f| (format!("<span style=\"font-size: {f}\">"), "</span>"))
                        }
                        ("quote", a) if quote_depth < out.opt.max_quote_depth => {
                            quote_depth += 1;
                            let cite = a
                                .map(|n| format!("<cite>{}</cite>", crate::escape(n)))
                                .unwrap_or_default();
                            Some((format!("<blockquote>{cite}"), "</blockquote>"))
                        }
                        ("spoiler", a) => Some((
                            format!(
                                "<details><summary>{}</summary>",
                                crate::escape(a.unwrap_or("剧透"))
                            ),
                            "</details>",
                        )),
                        _ => None,
                    }
                };
                match open {
                    Some((html, close)) => {
                        out.raw(&html);
                        stack.push(Open {
                            name: name.to_owned(),
                            close,
                            item_open: false,
                        });
                    }
                    None => text(out, raw, false),
                }
            }
            Tok::Close { name, raw } => {
                if let Some(pos) = stack.iter().rposition(|o| &o.name == name) {
                    while stack.len() > pos {
                        if let Some(o) = stack.pop() {
                            if o.item_open {
                                out.raw("</li>");
                            }
                            if o.name == "quote" {
                                quote_depth = quote_depth.saturating_sub(1);
                            }
                            out.raw(o.close);
                        }
                    }
                } else {
                    text(out, raw, false);
                }
            }
        }
        i += 1;
    }
    while let Some(o) = stack.pop() {
        if o.item_open {
            out.raw("</li>");
        }
        out.raw(o.close);
    }
}
