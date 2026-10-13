//! RSS 2.0 / Torznab XML（LLD §1.3～§1.5）：转义、分类映射、条目渲染、caps 与 Newznab 错误。纯函数，便于单元测试。

use chrono::{DateTime, Utc};
use serde_json::Value;

/// 条目（查询结果，见 query.rs）
#[derive(Debug, Clone, Default)]
pub struct Item {
    pub id: i64,
    pub title: String,
    pub size: i64,
    pub created_at: DateTime<Utc>,
    pub type_id: String,
    /// 种子字段与分组字段合并（种子字段优先）
    pub fields: Value,
    pub info_hash: Option<String>,
    pub seeders: i64,
    pub leechers: i64,
    pub completed: i64,
    /// 促销（kind、上传 / 下载系数千分比）；无促销为 None
    pub promotion: Option<Promo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Promo {
    pub kinds: Vec<String>,
    pub up_permille: i64,
    pub down_permille: i64,
}

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            // XML 1.0 不允许的控制字符
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => o.push(c),
        }
    }
    o
}

const LOSSLESS: &[&str] = &["flac", "alac", "wav", "ape", "wv", "dsd", "aiff"];

/// 分类映射（LLD §1.5）：返回 Newznab 分类（主分类在前）
pub fn categories(type_id: &str, f: &Value) -> Vec<u32> {
    let res = f["resolution"].as_str().unwrap_or("").to_ascii_lowercase();
    let uhd = matches!(res.as_str(), "2160p" | "4320p");
    let hd = matches!(res.as_str(), "1080p" | "1080i" | "720p");
    match type_id {
        "movie" => {
            if f["release_type"].as_str() == Some("disc") {
                vec![2050, 2000]
            } else if uhd {
                vec![2045, 2000]
            } else if hd {
                vec![2040, 2000]
            } else {
                vec![2000]
            }
        }
        "tv" => {
            if uhd {
                vec![5045, 5000]
            } else if hd {
                vec![5040, 5000]
            } else {
                vec![5000]
            }
        }
        "anime" => vec![5070, 5000],
        "music" => {
            let fmt = f["format"].as_str().unwrap_or("").to_ascii_lowercase();
            if LOSSLESS.contains(&fmt.as_str()) {
                vec![3040, 3000]
            } else if fmt == "mp3" {
                vec![3010, 3000]
            } else {
                vec![3000]
            }
        }
        "generic" => match f["kind"].as_str().unwrap_or("") {
            "game" => vec![4050, 4000],
            "software" => vec![4000],
            "ebook" => vec![7020, 7000],
            "course" => vec![7000],
            _ => vec![8000],
        },
        _ => vec![8000],
    }
}

/// Newznab 分类 → 可能的 type_id（Torznab `cat` 参数的预筛选；精确子分类在取回后再比对）
pub fn types_for_category(cat: u32) -> &'static [&'static str] {
    match cat {
        2000..=2999 => &["movie"],
        5070 => &["anime"],
        5000..=5999 => &["tv", "anime"],
        3000..=3999 => &["music"],
        4000..=4999 | 7000..=7999 | 8000..=8999 => &["generic"],
        _ => &[],
    }
}

/// 条目是否属于请求的分类（父分类匹配其全部子分类）
pub fn matches_categories(item_cats: &[u32], wanted: &[u32]) -> bool {
    wanted.is_empty()
        || item_cats.iter().any(|c| {
            wanted
                .iter()
                .any(|w| w == c || (w % 1000 == 0 && c / 1000 == w / 1000))
        })
}

/// (分类, 名称, 子分类)
type CategoryGroup = (u32, &'static str, &'static [(u32, &'static str)]);

const CATEGORY_NAMES: &[CategoryGroup] = &[
    (
        2000,
        "Movies",
        &[(2040, "Movies/HD"), (2045, "Movies/UHD"), (2050, "Movies/BluRay")],
    ),
    (3000, "Audio", &[(3010, "Audio/MP3"), (3040, "Audio/Lossless")]),
    (4000, "PC", &[(4050, "PC/Games")]),
    (
        5000,
        "TV",
        &[(5040, "TV/HD"), (5045, "TV/UHD"), (5070, "TV/Anime")],
    ),
    (7000, "Books", &[(7020, "Books/EBook")]),
    (8000, "Other", &[]),
];

fn rfc2822(t: &DateTime<Utc>) -> String {
    t.format("%a, %d %b %Y %H:%M:%S +0000").to_string()
}

fn attr(name: &str, value: impl std::fmt::Display) -> String {
    format!(
        "<torznab:attr name=\"{name}\" value=\"{}\"/>",
        esc(&value.to_string())
    )
}

/// 摘要（纯文本）：规格 · 促销 · 做种数
fn summary(it: &Item) -> String {
    let mut parts: Vec<String> = [
        "resolution",
        "source",
        "video_codec",
        "format",
        "encoding",
        "kind",
    ]
    .iter()
    .filter_map(|k| it.fields[*k].as_str().map(str::to_owned))
    .collect();
    if let Some(p) = &it.promotion {
        parts.push(p.kinds.join("+"));
    }
    parts.push(format!("{} seeders", it.seeders));
    parts.join(" · ")
}

pub struct Channel<'a> {
    pub site_name: &'a str,
    pub site_url: &'a str,
    pub title: &'a str,
    pub key: &'a str,
}

/// 单个条目（RSS 订阅与 Torznab 共用）
pub fn item(ch: &Channel, it: &Item) -> String {
    let host = ch
        .site_url
        .split("://")
        .nth(1)
        .unwrap_or(ch.site_url)
        .trim_end_matches('/');
    let dl = format!("{}/api/p/rss/v1/dl/{}?k={}", ch.site_url, it.id, ch.key);
    let page = format!("{}/torrents/{}", ch.site_url, it.id);
    let mut x = String::new();
    x.push_str("<item>");
    x.push_str(&format!("<title>{}</title>", esc(&it.title)));
    x.push_str(&format!(
        "<guid isPermaLink=\"false\">{}-{}</guid>",
        esc(host),
        it.id
    ));
    x.push_str(&format!("<link>{}</link>", esc(&dl)));
    x.push_str(&format!("<comments>{}</comments>", esc(&page)));
    x.push_str(&format!("<pubDate>{}</pubDate>", rfc2822(&it.created_at)));
    x.push_str(&format!("<size>{}</size>", it.size));
    x.push_str(&format!("<description>{}</description>", esc(&summary(it))));
    x.push_str(&format!(
        "<enclosure url=\"{}\" length=\"{}\" type=\"application/x-bittorrent\"/>",
        esc(&dl),
        it.size
    ));
    for c in categories(&it.type_id, &it.fields) {
        x.push_str(&format!("<category>{c}</category>"));
        x.push_str(&attr("category", c));
    }
    x.push_str(&attr("size", it.size));
    x.push_str(&attr("seeders", it.seeders));
    x.push_str(&attr("peers", it.seeders + it.leechers));
    x.push_str(&attr("grabs", it.completed));
    if let Some(h) = &it.info_hash {
        x.push_str(&attr("infohash", h));
    }
    let (up, down) = it
        .promotion
        .as_ref()
        .map_or((1000, 1000), |p| (p.up_permille, p.down_permille));
    x.push_str(&attr("downloadvolumefactor", factor(down)));
    x.push_str(&attr("uploadvolumefactor", factor(up)));
    if let Some(imdb) = it.fields["imdb_id"].as_str().and_then(|s| s.strip_prefix("tt")) {
        x.push_str(&attr("imdb", imdb));
        x.push_str(&attr("imdbid", format!("tt{imdb}")));
    }
    for (k, name) in [("tmdb_id", "tmdbid"), ("tvdb_id", "tvdbid")] {
        if let Some(v) = it.fields[k].as_i64() {
            x.push_str(&attr(name, v));
        }
    }
    for (k, name) in [("season", "season"), ("episode", "episode")] {
        if let Some(v) = it.fields[k].as_i64() {
            x.push_str(&attr(name, v));
        }
    }
    x.push_str("</item>");
    x
}

fn factor(permille: i64) -> String {
    let v = permille.max(0);
    if v % 1000 == 0 {
        format!("{}", v / 1000)
    } else {
        format!("{}.{:03}", v / 1000, v % 1000)
            .trim_end_matches('0')
            .to_owned()
    }
}

pub fn channel(ch: &Channel, items: &[Item]) -> String {
    let mut x = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rss version=\"2.0\" xmlns:atom=\"http://www.w3.org/2005/Atom\" xmlns:torznab=\"http://torznab.com/schemas/2015/feed\"><channel>",
    );
    x.push_str(&format!(
        "<title>{}</title><link>{}</link><description>{}</description><ttl>5</ttl>",
        esc(&format!("{} - {}", ch.site_name, ch.title)),
        esc(ch.site_url),
        esc(ch.title)
    ));
    for it in items {
        x.push_str(&item(ch, it));
    }
    x.push_str("</channel></rss>");
    x
}

pub fn caps(site_name: &str) -> String {
    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<caps>");
    x.push_str(&format!(
        "<server title=\"{}\"/><limits max=\"100\" default=\"50\"/>",
        esc(site_name)
    ));
    x.push_str(
        "<searching><search available=\"yes\" supportedParams=\"q\"/>\
<tv-search available=\"yes\" supportedParams=\"q,season,ep,imdbid,tvdbid,tmdbid\"/>\
<movie-search available=\"yes\" supportedParams=\"q,imdbid,tmdbid\"/>\
<music-search available=\"yes\" supportedParams=\"q,artist,album\"/>\
<book-search available=\"yes\" supportedParams=\"q,author,title\"/></searching><categories>",
    );
    for (id, name, subs) in CATEGORY_NAMES {
        x.push_str(&format!("<category id=\"{id}\" name=\"{name}\">"));
        for (sid, sname) in *subs {
            x.push_str(&format!("<subcat id=\"{sid}\" name=\"{sname}\"/>"));
        }
        x.push_str("</category>");
    }
    x.push_str("</categories></caps>");
    x
}

pub fn error(code: u32, description: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<error code=\"{code}\" description=\"{}\"/>",
        esc(description)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn escaping() {
        assert_eq!(esc("a<b>&\"'\u{1}c"), "a&lt;b&gt;&amp;&quot;&apos;c");
    }

    #[test]
    fn category_map() {
        assert_eq!(
            categories("movie", &json!({ "resolution": "2160p" })),
            vec![2045, 2000]
        );
        assert_eq!(
            categories("movie", &json!({ "resolution": "1080p" })),
            vec![2040, 2000]
        );
        assert_eq!(
            categories("movie", &json!({ "resolution": "1080p", "release_type": "disc" })),
            vec![2050, 2000]
        );
        assert_eq!(
            categories("tv", &json!({ "resolution": "720p" })),
            vec![5040, 5000]
        );
        assert_eq!(categories("anime", &json!({})), vec![5070, 5000]);
        assert_eq!(
            categories("music", &json!({ "format": "FLAC" })),
            vec![3040, 3000]
        );
        assert_eq!(
            categories("generic", &json!({ "kind": "game" })),
            vec![4050, 4000]
        );
        assert_eq!(categories("generic", &json!({ "kind": "other" })), vec![8000]);
    }

    #[test]
    fn category_filter() {
        assert!(matches_categories(&[2045, 2000], &[2000]));
        assert!(matches_categories(&[2045, 2000], &[2045]));
        assert!(!matches_categories(&[2040, 2000], &[2045]));
        assert!(matches_categories(&[5070, 5000], &[5070]));
        assert!(!matches_categories(&[3000], &[2000, 5000]));
        assert!(matches_categories(&[3000], &[]));
        assert_eq!(types_for_category(5070), &["anime"]);
    }

    #[test]
    fn factors() {
        assert_eq!(factor(0), "0");
        assert_eq!(factor(1000), "1");
        assert_eq!(factor(500), "0.5");
        assert_eq!(factor(2000), "2");
        assert_eq!(factor(1250), "1.25");
    }

    #[test]
    fn renders_item() {
        let ch = Channel {
            site_name: "S",
            site_url: "https://pt.example",
            title: "f",
            key: "ptf_x",
        };
        let it = Item {
            id: 7,
            title: "A & B".into(),
            size: 10,
            type_id: "movie".into(),
            fields: json!({ "resolution": "2160p", "imdb_id": "tt0111161" }),
            info_hash: Some("ab".into()),
            seeders: 3,
            leechers: 1,
            promotion: Some(Promo {
                kinds: vec!["free".into()],
                up_permille: 1000,
                down_permille: 0,
            }),
            ..Default::default()
        };
        let x = channel(&ch, &[it]);
        assert!(x.contains("<title>A &amp; B</title>"), "{x}");
        assert!(x.contains("<guid isPermaLink=\"false\">pt.example-7</guid>"));
        assert!(x.contains("url=\"https://pt.example/api/p/rss/v1/dl/7?k=ptf_x\""));
        assert!(x.contains("name=\"category\" value=\"2045\""));
        assert!(x.contains("name=\"peers\" value=\"4\""));
        assert!(x.contains("name=\"downloadvolumefactor\" value=\"0\""));
        assert!(x.contains("name=\"imdb\" value=\"0111161\""));
        assert!(x.contains("2160p · free · 3 seeders"));
    }
}
