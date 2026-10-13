//! 用户内容渲染（comments LLD §1.1）：Markdown（CommonMark + 表格、删除线、任务列表）与 BBCode → 安全 HTML。
//!
//! 安全模型：不接受原始 HTML（Markdown 中的 HTML 一律当作文本转义）；所有文本与属性经转义后由本 crate 自行输出（不拼接
//! 用户提供的标签）；链接只允许 https / http / 站内相对路径（可选 magnet），统一加 `rel="nofollow noopener ugc"`；图片只允许
//! 站内前缀或白名单主机的 https 地址，否则降级为链接；样式只由 BBCode 渲染器从白名单值生成。输入 ≤ 64 KiB，BBCode 嵌套 ≤ 32。
//! 前端展示时再经 ui-kit 的白名单净化（纵深防御）。

mod bbcode;
mod markdown;

pub const RENDER_VERSION: u32 = 1;
pub const MAX_INPUT: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Markup {
    Markdown,
    BbCode,
}

impl Markup {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "markdown" => Some(Self::Markdown),
            "bbcode" => Some(Self::BbCode),
            _ => None,
        }
    }
}

/// 用户名 → 用户 ID
pub type MentionResolver<'a> = &'a dyn Fn(&str) -> Option<u64>;

pub struct RenderOptions<'a> {
    /// 允许的外部图片主机（只限 https）
    pub image_hosts: &'a [String],
    /// 站内图片路径前缀，如 `/media/`
    pub blob_prefix: &'a str,
    /// 允许 `magnet:` 链接
    pub allow_magnet: bool,
    /// 用户名 → 用户 ID（存在才渲染为链接）
    pub mention_resolver: Option<MentionResolver<'a>>,
    pub max_quote_depth: u8,
}

impl Default for RenderOptions<'_> {
    fn default() -> Self {
        Self {
            image_hosts: &[],
            blob_prefix: "/media/",
            allow_magnet: false,
            mention_resolver: None,
            max_quote_depth: 3,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rendered {
    pub html: String,
    /// 纯文本（摘要、搜索、AI 审核）
    pub text: String,
    /// 被解析为站内用户的 @提醒（去重）
    pub mentions: Vec<u64>,
    pub links: u32,
    pub images: u32,
}

pub fn render(src: &str, markup: Markup, opt: &RenderOptions<'_>) -> Rendered {
    let src = truncate(src, MAX_INPUT);
    let mut out = Out {
        opt,
        r: Rendered::default(),
    };
    match markup {
        Markup::Markdown => markdown::render(src, &mut out),
        Markup::BbCode => bbcode::render(src, &mut out),
    }
    let mut r = out.r;
    r.text = r.text.split_whitespace().collect::<Vec<_>>().join(" ");
    r
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

/// 输出缓冲（渲染器共用的安全输出原语）
pub(crate) struct Out<'a, 'b> {
    pub opt: &'a RenderOptions<'b>,
    pub r: Rendered,
}

impl Out<'_, '_> {
    pub fn raw(&mut self, s: &str) {
        self.r.html.push_str(s);
    }

    /// 文本：转义、记入纯文本；`mentions` 为 true 时解析 @提醒
    pub fn text(&mut self, s: &str, mentions: bool) {
        self.r.text.push_str(s);
        self.r.text.push(' ');
        if !mentions || self.opt.mention_resolver.is_none() || !s.contains('@') {
            self.r.html.push_str(&escape(s));
            return;
        }
        let resolver = self.opt.mention_resolver;
        let chars: Vec<char> = s.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            let prev_ok =
                i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_' || chars[i - 1] == '@');
            if chars[i] == '@' && prev_ok {
                let name: String = chars[i + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
                    .take(32)
                    .collect();
                let name = name.trim_end_matches('.').to_owned();
                if name.len() >= 2
                    && let Some(uid) = resolver.and_then(|f| f(&name))
                {
                    self.r.html.push_str(&format!(
                        "<a href=\"/users/{uid}\" class=\"mention\">@{}</a>",
                        escape(&name)
                    ));
                    if !self.r.mentions.contains(&uid) {
                        self.r.mentions.push(uid);
                    }
                    i += 1 + name.chars().count();
                    continue;
                }
            }
            self.r.html.push_str(&escape(&chars[i].to_string()));
            i += 1;
        }
    }

    /// 链接地址是否允许（https / http / 站内相对路径 / 可选 magnet）
    pub fn link_ok(&self, url: &str) -> bool {
        let u = url.trim();
        if u.is_empty() || u.chars().any(|c| c.is_control() || c == ' ') {
            return false;
        }
        if u.starts_with('/') && !u.starts_with("//") {
            return true;
        }
        let lower = u.to_ascii_lowercase();
        lower.starts_with("https://")
            || lower.starts_with("http://")
            || (self.opt.allow_magnet && lower.starts_with("magnet:?"))
    }

    pub fn image_ok(&self, url: &str) -> bool {
        let u = url.trim();
        if u.starts_with(self.opt.blob_prefix) && !u.contains("..") {
            return true;
        }
        let Some(rest) = u.strip_prefix("https://") else {
            return false;
        };
        let host = rest
            .split(['/', '?', '#'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        self.opt.image_hosts.iter().any(|h| h.eq_ignore_ascii_case(&host))
    }

    pub fn open_link(&mut self, url: &str) {
        self.r.links += 1;
        self.r.html.push_str(&format!(
            "<a href=\"{}\" rel=\"nofollow noopener ugc\">",
            escape(url.trim())
        ));
    }

    /// 图片：不允许的地址降级为链接（或纯文本）
    pub fn image(&mut self, url: &str, alt: &str, size: Option<(u32, u32)>) {
        if self.image_ok(url) {
            self.r.images += 1;
            let dims = size
                .filter(|(w, h)| (1..=4096).contains(w) && (1..=4096).contains(h))
                .map(|(w, h)| format!(" width=\"{w}\" height=\"{h}\""))
                .unwrap_or_default();
            self.r.html.push_str(&format!(
                "<img src=\"{}\" alt=\"{}\" loading=\"lazy\"{dims}>",
                escape(url.trim()),
                escape(alt)
            ));
        } else if self.link_ok(url) {
            self.open_link(url);
            self.text(if alt.is_empty() { url } else { alt }, false);
            self.raw("</a>");
        } else {
            self.text(alt, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> RenderOptions<'static> {
        RenderOptions::default()
    }

    #[test]
    fn markdown_is_safe() {
        let r = render(
            "# 标题\n\n**粗体** <script>alert(1)</script> [x](javascript:alert(1)) [ok](https://a.example/p?q=1&r=2)\n\n<img src=x onerror=alert(1)>",
            Markup::Markdown,
            &opts(),
        );
        assert!(
            !r.html.contains("<script") && !r.html.contains("<img src=x"),
            "{}",
            r.html
        );
        assert!(r.html.contains("&lt;script&gt;"));
        assert!(!r.html.contains("javascript:"), "{}", r.html);
        assert!(
            r.html
                .contains("<a href=\"https://a.example/p?q=1&amp;r=2\" rel=\"nofollow noopener ugc\">ok</a>"),
            "{}",
            r.html
        );
        assert!(
            r.html.contains("<h3>标题</h3>"),
            "headings are demoted: {}",
            r.html
        );
        assert_eq!(r.links, 1);
        assert!(r.text.contains("粗体") && !r.text.contains("**"));
    }

    #[test]
    fn markdown_features() {
        let hosts = vec!["img.example".to_owned()];
        let o = RenderOptions {
            image_hosts: &hosts,
            ..opts()
        };
        let r = render(
            "- [x] done\n- item\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n~~old~~ `code <b>`\n\n![p](https://img.example/a.png) ![q](https://evil.example/b.png)\n\n```\n<raw>\n```",
            Markup::Markdown,
            &o,
        );
        assert!(
            r.html.contains("<table>")
                && r.html.contains("<del>old</del>")
                && r.html.contains("<code>code &lt;b&gt;</code>"),
            "{}",
            r.html
        );
        assert!(
            r.html
                .contains("<img src=\"https://img.example/a.png\" alt=\"p\" loading=\"lazy\">"),
            "{}",
            r.html
        );
        assert!(
            r.html.contains("<a href=\"https://evil.example/b.png\""),
            "non-allowlisted images become links: {}",
            r.html
        );
        assert!(
            r.html.contains("<pre><code>&lt;raw&gt;\n</code></pre>"),
            "{}",
            r.html
        );
        assert!(r.html.contains("type=\"checkbox\" disabled checked"));
        assert_eq!(r.images, 1);
    }

    #[test]
    fn mentions() {
        let f = |n: &str| (n == "alice").then_some(7u64);
        let o = RenderOptions {
            mention_resolver: Some(&f),
            ..opts()
        };
        let r = render("hi @alice and @bob, mail a@alice `@alice`", Markup::Markdown, &o);
        assert_eq!(r.mentions, vec![7]);
        assert_eq!(r.html.matches("class=\"mention\"").count(), 1, "{}", r.html);
        let b = render("[b]@alice[/b] [code]@alice[/code]", Markup::BbCode, &o);
        assert_eq!(b.html.matches("class=\"mention\"").count(), 1, "{}", b.html);
    }

    #[test]
    fn bbcode() {
        let r = render(
            "[b]粗[/b][i]斜[/i] [url=https://a.example]站[/url] [url]javascript:x[/url] [color=#f00]红[/color] [color=red;x:1]坏[/color] [size=9]大[/size]\n[quote=bob]引用[/quote][spoiler=剧透]内容[/spoiler][list][*]一[*]二[/list][code][b]不解析[/b] <x>[/code][center]中[/center][hr][unknown]x[/unknown][b]未闭合",
            Markup::BbCode,
            &opts(),
        );
        let h = &r.html;
        assert!(h.contains("<strong>粗</strong><em>斜</em>"), "{h}");
        assert!(
            h.contains("<a href=\"https://a.example\" rel=\"nofollow noopener ugc\">站</a>"),
            "{h}"
        );
        assert!(!h.contains("href=\"javascript"), "{h}");
        assert!(h.contains("<span style=\"color: #f00\">红</span>"), "{h}");
        assert!(
            h.contains("[color=red;x:1]坏"),
            "invalid attribute values stay literal: {h}"
        );
        assert!(h.contains("[size=9]大"), "{h}");
        assert!(h.contains("<blockquote><cite>bob</cite>引用</blockquote>"), "{h}");
        assert!(
            h.contains("<details><summary>剧透</summary>内容</details>"),
            "{h}"
        );
        assert!(h.contains("<ul><li>一</li><li>二</li></ul>"), "{h}");
        assert!(
            h.contains("<pre><code>[b]不解析[/b] &lt;x&gt;</code></pre>"),
            "{h}"
        );
        assert!(h.contains("<div class=\"center\">中</div><hr>"), "{h}");
        assert!(h.contains("[unknown]x[/unknown]"), "{h}");
        assert!(
            h.ends_with("<strong>未闭合</strong>"),
            "unclosed tags are closed at the end: {h}"
        );
        let deep = "[quote]".repeat(40) + "x";
        let d = render(&deep, Markup::BbCode, &opts());
        assert!(
            d.html.matches("<blockquote>").count() <= 3,
            "quote depth is bounded"
        );
        assert_eq!(
            render("[img]https://evil.example/a.png[/img]", Markup::BbCode, &opts()).images,
            0
        );
        assert_eq!(
            render("[img]/media/x.png[/img]", Markup::BbCode, &opts()).images,
            1
        );
    }

    #[test]
    fn size_limit() {
        let big = "a".repeat(MAX_INPUT + 10) + "é";
        assert!(render(&big, Markup::Markdown, &opts()).text.len() <= MAX_INPUT);
    }
}
