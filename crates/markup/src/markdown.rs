//! Markdown → HTML：遍历 pulldown-cmark 事件，只输出白名单标签；原始 HTML 当作文本；标题降级为 h3～h6。

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::Out;

fn heading(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 3,
        HeadingLevel::H2 => 4,
        HeadingLevel::H3 => 5,
        _ => 6,
    }
}

pub(crate) fn render(src: &str, out: &mut Out<'_, '_>) {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    let mut code_block = 0u32;
    let mut quote_depth = 0u8;
    let mut links: Vec<bool> = Vec::new();
    // 图片：收集 alt 文本，结束时输出
    let mut image: Option<(String, String)> = None;
    let mut in_head = false;
    for ev in Parser::new_ext(src, opts) {
        if let Some((_, alt)) = &mut image {
            match ev {
                Event::End(TagEnd::Image) => {
                    if let Some((url, alt)) = image.take() {
                        out.image(&url, &alt, None);
                    }
                }
                Event::Text(t) | Event::Code(t) => alt.push_str(&t),
                _ => {}
            }
            continue;
        }
        match ev {
            Event::Start(tag) => match tag {
                Tag::Paragraph => out.raw("<p>"),
                Tag::Heading { level, .. } => out.raw(&format!("<h{}>", heading(level))),
                Tag::BlockQuote(_) => {
                    quote_depth = quote_depth.saturating_add(1);
                    if quote_depth <= out.opt.max_quote_depth {
                        out.raw("<blockquote>");
                    }
                }
                Tag::CodeBlock(_) => {
                    code_block += 1;
                    out.raw("<pre><code>");
                }
                Tag::List(Some(_)) => out.raw("<ol>"),
                Tag::List(None) => out.raw("<ul>"),
                Tag::Item => out.raw("<li>"),
                Tag::Emphasis => out.raw("<em>"),
                Tag::Strong => out.raw("<strong>"),
                Tag::Strikethrough => out.raw("<del>"),
                Tag::Link { dest_url, .. } => {
                    let ok = out.link_ok(&dest_url);
                    if ok {
                        out.open_link(&dest_url);
                    }
                    links.push(ok);
                }
                Tag::Image { dest_url, .. } => image = Some((dest_url.to_string(), String::new())),
                Tag::Table(_) => out.raw("<table>"),
                Tag::TableHead => {
                    in_head = true;
                    out.raw("<thead><tr>");
                }
                Tag::TableRow => out.raw("<tr>"),
                Tag::TableCell => out.raw(if in_head { "<th>" } else { "<td>" }),
                _ => {}
            },
            Event::End(tag) => match tag {
                TagEnd::Paragraph => out.raw("</p>"),
                TagEnd::Heading(level) => out.raw(&format!("</h{}>", heading(level))),
                TagEnd::BlockQuote(_) => {
                    if quote_depth <= out.opt.max_quote_depth {
                        out.raw("</blockquote>");
                    }
                    quote_depth = quote_depth.saturating_sub(1);
                }
                TagEnd::CodeBlock => {
                    code_block = code_block.saturating_sub(1);
                    out.raw("</code></pre>");
                }
                TagEnd::List(true) => out.raw("</ol>"),
                TagEnd::List(false) => out.raw("</ul>"),
                TagEnd::Item => out.raw("</li>"),
                TagEnd::Emphasis => out.raw("</em>"),
                TagEnd::Strong => out.raw("</strong>"),
                TagEnd::Strikethrough => out.raw("</del>"),
                TagEnd::Link => {
                    if links.pop() == Some(true) {
                        out.raw("</a>");
                    }
                }
                TagEnd::Table => out.raw("</tbody></table>"),
                TagEnd::TableHead => {
                    in_head = false;
                    out.raw("</tr></thead><tbody>");
                }
                TagEnd::TableRow => out.raw("</tr>"),
                TagEnd::TableCell => out.raw(if in_head { "</th>" } else { "</td>" }),
                _ => {}
            },
            Event::Text(t) => out.text(&t, code_block == 0),
            Event::Code(t) => {
                out.raw("<code>");
                out.text(&t, false);
                out.raw("</code>");
            }
            // 原始 HTML、数学公式、脚注引用：当作文本
            Event::Html(t)
            | Event::InlineHtml(t)
            | Event::InlineMath(t)
            | Event::DisplayMath(t)
            | Event::FootnoteReference(t) => {
                out.text(&t, false);
            }
            Event::SoftBreak => out.raw("\n"),
            Event::HardBreak => out.raw("<br>"),
            Event::Rule => out.raw("<hr>"),
            Event::TaskListMarker(done) => out.raw(if done {
                "<input type=\"checkbox\" disabled checked> "
            } else {
                "<input type=\"checkbox\" disabled> "
            }),
        }
    }
}
