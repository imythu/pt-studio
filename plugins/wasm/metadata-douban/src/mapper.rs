//! 豆瓣数据 → 统一元数据模型（metadata-providers-common §2），以及 PT-Gen 风格的简介。
//!
//! 两种上游：站点自建代理（LLD §1 的字段）与 PT-Gen 兼容端点（pt-gen-cfworker 的字段）。

use pt_plugin_sdk::serde_json::{self, Value, json};

/// 豆瓣条目 ID（即缓存键）：影视为纯数字或 movie 子域链接；图书为 `book:<数字>` 或 book 子域链接；
/// 也可用 ISBN 查图书：`isbn:<ISBN>`，或以 978 / 979 开头的 13 位数字
pub fn parse_id(s: &str) -> Option<String> {
    let s = s.trim();
    let digits = |x: &str| {
        (!x.is_empty() && x.len() <= 12 && x.bytes().all(|b| b.is_ascii_digit())).then(|| x.to_owned())
    };
    if let Some(id) = digits(s) {
        return Some(id);
    }
    if let Some(isbn) = s
        .strip_prefix("isbn:")
        .or_else(|| s.strip_prefix("ISBN:"))
        .map(str::trim)
        .or_else(|| (s.len() == 13 && (s.starts_with("978") || s.starts_with("979"))).then_some(s))
    {
        return parse_isbn(isbn).map(|i| format!("isbn:{i}"));
    }
    if let Some(id) = s.strip_prefix("book:") {
        return digits(id).map(|id| format!("book:{id}"));
    }
    let rest = s.split("douban.com/subject/").nth(1)?;
    let id = digits(&rest.chars().take_while(char::is_ascii_digit).collect::<String>())?;
    Some(if s.contains("book.douban.com/") {
        format!("book:{id}")
    } else {
        id
    })
}

/// ISBN-10 / ISBN-13（可带连字符），校验位正确才接受
pub fn parse_isbn(s: &str) -> Option<String> {
    let c: String = s
        .chars()
        .filter(|c| *c != '-' && *c != ' ')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let d: Vec<u32> = c
        .chars()
        .map(|ch| {
            if ch == 'X' {
                10
            } else {
                ch.to_digit(10).unwrap_or(99)
            }
        })
        .collect();
    let ok = match d.len() {
        13 => {
            d.iter().all(|x| *x < 10)
                && d.iter()
                    .enumerate()
                    .map(|(i, x)| if i % 2 == 0 { *x } else { x * 3 })
                    .sum::<u32>()
                    % 10
                    == 0
        }
        10 => {
            d[..9].iter().all(|x| *x < 10)
                && d[9] <= 10
                && d.iter().zip((1..=10).rev()).map(|(x, w)| x * w).sum::<u32>() % 11 == 0
        }
        _ => false,
    };
    ok.then_some(c)
}

pub fn genre_key(zh: &str) -> &'static str {
    match zh {
        "剧情" => "drama",
        "喜剧" => "comedy",
        "动作" => "action",
        "爱情" => "romance",
        "科幻" => "scifi",
        "动画" => "animation",
        "悬疑" => "mystery",
        "惊悚" => "thriller",
        "恐怖" => "horror",
        "犯罪" => "crime",
        "纪录片" => "documentary",
        "战争" => "war",
        "奇幻" => "fantasy",
        "冒险" => "adventure",
        "家庭" => "family",
        "历史" => "history",
        "音乐" => "music",
        "歌舞" => "musical",
        "传记" => "biography",
        "运动" => "sport",
        "西部" => "western",
        "武侠" => "wuxia",
        "古装" => "costume",
        _ => "other",
    }
}

pub fn country_code(zh: &str) -> Option<&'static str> {
    Some(match zh.trim() {
        "美国" => "US",
        "中国大陆" | "中国" => "CN",
        "中国香港" | "香港" => "HK",
        "中国台湾" | "台湾" => "TW",
        "日本" => "JP",
        "韩国" => "KR",
        "英国" => "GB",
        "法国" => "FR",
        "德国" => "DE",
        "意大利" => "IT",
        "西班牙" => "ES",
        "加拿大" => "CA",
        "澳大利亚" => "AU",
        "印度" => "IN",
        "泰国" => "TH",
        "俄罗斯" => "RU",
        "瑞典" => "SE",
        "丹麦" => "DK",
        "挪威" => "NO",
        "新西兰" => "NZ",
        "爱尔兰" => "IE",
        "墨西哥" => "MX",
        "巴西" => "BR",
        "阿根廷" => "AR",
        "比利时" => "BE",
        "荷兰" => "NL",
        "波兰" => "PL",
        "伊朗" => "IR",
        _ => return None,
    })
}

fn strs(v: &Value) -> Vec<String> {
    match v {
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_owned).or_else(|| x["name"].as_str().map(str::to_owned)))
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect(),
        Value::String(s) => s.split(['/', '、']).map(|x| x.trim().to_owned()).filter(|x| !x.is_empty()).collect(),
        _ => Vec::new(),
    }
}

fn strip_tags(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().chars().take(5000).collect()
}

fn imdb_id(s: Option<&str>) -> Option<String> {
    let s = s?.trim();
    let id = s.rsplit('/').find(|p| !p.is_empty()).unwrap_or(s);
    let ok = id.len() >= 9 && id.len() <= 11 && id.starts_with("tt") && id[2..].bytes().all(|b| b.is_ascii_digit());
    ok.then(|| id.to_owned())
}

fn year_of(v: &Value) -> Option<i64> {
    v.as_i64().or_else(|| v.as_str().and_then(|s| s.get(..4)).and_then(|y| y.parse().ok()))
}

/// 统一模型的公共部分（两种上游共用）
#[allow(clippy::too_many_arguments)]
fn unified(
    id: &str,
    kind: &str,
    title: Option<String>,
    original: Option<String>,
    aliases: Vec<String>,
    year: Option<i64>,
    date: Option<String>,
    imdb: Option<String>,
    genres: &[String],
    countries: &[String],
    languages: Vec<String>,
    people: Vec<Value>,
    runtime: Option<i64>,
    episodes: Option<i64>,
    rating: Option<(f64, i64)>,
    summary: Option<String>,
) -> Value {
    let mut localized = serde_json::Map::new();
    if let Some(t) = &title {
        localized.insert("zh-CN".into(), json!(t));
    }
    let mut g: Vec<&str> = genres.iter().map(|x| genre_key(x)).collect();
    g.dedup();
    json!({
        "kind": kind,
        "titles": { "original": original.or(title), "localized": localized, "aliases": aliases },
        "year": year,
        "release_date": date,
        "ids": { "douban": id, "imdb": imdb },
        "genres": g,
        "countries": countries.iter().filter_map(|c| country_code(c)).collect::<Vec<_>>(),
        "languages": languages,
        "people": people.into_iter().take(50).collect::<Vec<_>>(),
        "runtime_min": runtime,
        "episode_count": episodes,
        "ratings": rating.map_or_else(|| json!({}), |(v, n)| json!({ "douban": { "value": v, "votes": n } })),
        "images": {},
        "overview": summary.map_or_else(|| json!({}), |s| json!({ "zh-CN": strip_tags(&s) })),
        "extra": { "genres_zh": genres, "countries_zh": countries },
        "attribution": { "source": "douban", "url": format!("https://movie.douban.com/subject/{id}/"), "license_note": "数据来源：豆瓣" },
    })
}

fn people(directors: &Value, writers: &Value, casts: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    for (role, v) in [("director", directors), ("writer", writers), ("cast", casts)] {
        for n in strs(v) {
            out.push(json!({ "name": n, "role": role }));
        }
    }
    out
}

/// 站点代理（LLD §1）
pub fn from_proxy(id: &str, v: &Value) -> Value {
    let kind = match v["type"].as_str() {
        Some("tv") => "tv",
        Some("book") => "book",
        _ => "movie",
    };
    let genres = strs(&v["genres"]);
    let countries = strs(&v["countries"]);
    unified(
        id,
        kind,
        v["title"].as_str().map(str::to_owned),
        v["original_title"].as_str().map(str::to_owned),
        strs(&v["aka"]),
        year_of(&v["year"]),
        v["pubdate"].as_str().map(str::to_owned),
        imdb_id(v["imdb"].as_str()),
        &genres,
        &countries,
        strs(&v["languages"]),
        people(&v["directors"], &v["writers"], &v["casts"]),
        v["durations_min"].as_i64(),
        v["episodes_count"].as_i64(),
        v["rating"]["value"].as_f64().map(|x| (x, v["rating"]["count"].as_i64().unwrap_or(0))),
        v["summary"].as_str().map(str::to_owned),
    )
}

/// PT-Gen 兼容端点（pt-gen-cfworker 的字段）
pub fn from_ptgen(id: &str, v: &Value) -> Value {
    let episodes = v["episodes"].as_i64().or_else(|| v["episodes"].as_str().and_then(|s| s.trim().parse().ok()));
    let runtime = v["duration"]
        .as_str()
        .and_then(|d| d.trim().split(|c: char| !c.is_ascii_digit()).find(|x| !x.is_empty()))
        .and_then(|n| n.parse().ok());
    let rating = v["douban_rating_average"]
        .as_f64()
        .or_else(|| v["douban_rating_average"].as_str().and_then(|s| s.parse().ok()))
        .filter(|x| *x > 0.0)
        .map(|x| (x, v["douban_votes"].as_i64().or_else(|| v["douban_votes"].as_str().and_then(|s| s.parse().ok())).unwrap_or(0)));
    let mut aliases = strs(&v["aka"]);
    for t in strs(&v["trans_title"]) {
        if !aliases.contains(&t) {
            aliases.push(t);
        }
    }
    let genres = strs(&v["genre"]);
    let countries = strs(&v["region"]);
    let mut data = unified(
        id,
        if episodes.is_some_and(|e| e > 1) || v["kind"] == "tv" { "tv" } else { "movie" },
        v["chinese_title"].as_str().map(str::to_owned),
        v["foreign_title"].as_str().filter(|s| !s.is_empty()).map(str::to_owned),
        aliases,
        year_of(&v["year"]),
        strs(&v["playdate"]).into_iter().next(),
        imdb_id(v["imdb_id"].as_str().or(v["imdb_link"].as_str())),
        &genres,
        &countries,
        strs(&v["language"]),
        people(&v["director"], &v["writer"], &v["cast"]),
        runtime,
        episodes,
        rating,
        v["introduction"].as_str().map(str::to_owned),
    );
    if let Some(r) = v["imdb_rating"].as_str().filter(|s| !s.is_empty()) {
        data["extra"]["imdb_rating"] = json!(r);
    }
    if let Some(a) = v["awards"].as_str().filter(|s| !s.trim().is_empty()) {
        data["extra"]["awards"] = json!(a.trim());
    }
    data
}

/// `2011-6` / `2011-6-1` / `2011年6月` → (年, 完整日期)
fn pub_date(s: &str) -> (Option<i64>, Option<String>) {
    let n: Vec<u32> = s
        .split(|c: char| !c.is_ascii_digit())
        .filter(|x| !x.is_empty())
        .filter_map(|x| x.parse().ok())
        .collect();
    let year = n
        .first()
        .copied()
        .filter(|y| (1000..=2200).contains(y))
        .map(i64::from);
    let date = match (year, n.get(1), n.get(2)) {
        (Some(y), Some(m @ 1..=12), Some(d @ 1..=31)) => Some(format!("{y:04}-{m:02}-{d:02}")),
        _ => None,
    };
    (year, date)
}

/// 图书（`scrape::book_from_rexxar` / `scrape::book_page` 的中间格式，代理模式同名字段）→ 统一模型
pub fn from_book(id: &str, b: &Value) -> Value {
    let s = |k: &str| {
        b[k].as_str()
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(str::to_owned)
    };
    let title = s("title");
    let (year, date) = s("pubdate").map_or((None, None), |p| pub_date(&p));
    let mut people: Vec<Value> = strs(&b["authors"])
        .into_iter()
        .map(|n| json!({ "name": n, "role": "writer" }))
        .collect();
    people.extend(
        strs(&b["translators"])
            .into_iter()
            .map(|n| json!({ "name": n, "role": "translator" })),
    );
    if let Some(p) = s("publisher") {
        people.push(json!({ "name": p, "role": "publisher" }));
    }
    let mut localized = serde_json::Map::new();
    if let Some(t) = &title {
        localized.insert("zh-CN".into(), json!(t));
    }
    let rating = &b["rating"];
    let summary = s("intro").or_else(|| s("summary"));
    json!({
        "kind": "book",
        "titles": { "original": s("original_title").or_else(|| title.clone()), "localized": localized, "aliases": [] },
        "year": year,
        "release_date": date,
        "ids": { "douban": id, "isbn": s("isbn").and_then(|i| parse_isbn(&i)) },
        "genres": [],
        "countries": [],
        "languages": [],
        "people": people.into_iter().take(50).collect::<Vec<_>>(),
        "runtime_min": null,
        "episode_count": null,
        "ratings": rating["value"].as_f64().filter(|x| *x > 0.0)
            .map_or_else(|| json!({}), |v| json!({ "douban": { "value": v, "votes": rating["count"].as_i64().unwrap_or(0) } })),
        "images": {},
        "overview": summary.map_or_else(|| json!({}), |x| json!({ "zh-CN": strip_tags(&x) })),
        "extra": {
            "subtitle": s("subtitle"), "authors": strs(&b["authors"]), "translators": strs(&b["translators"]),
            "publisher": s("publisher"), "producer": s("producer"), "pubdate": s("pubdate"),
            "pages": b["pages"].as_i64(), "price": s("price"), "binding": s("binding"), "series": s("series"),
            "author_intro": s("author_intro").map(|x| strip_tags(&x)),
        },
        "attribution": { "source": "douban", "url": format!("https://book.douban.com/subject/{id}/"), "license_note": "数据来源：豆瓣" },
    })
}

fn join(v: &Value, key: &str, sep: &str) -> String {
    strs(&v[key]).join(sep)
}

/// PT-Gen 风格的简介（BBCode 或 Markdown）。`poster` 为站内转存后的图片地址。
pub fn intro(data: &Value, poster: Option<&str>, markdown: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    if let Some(p) = poster {
        lines.push(if markdown { format!("![poster]({p})") } else { format!("[img]{p}[/img]") });
        lines.push(String::new());
    }
    if data["kind"] == "book" {
        book_lines(data, &mut lines);
        return finish(&lines, markdown);
    }
    let mut row = |label: &str, value: String| {
        if !value.trim().is_empty() {
            lines.push(format!("◎{label}　{value}"));
        }
    };
    let titles = &data["titles"];
    row("译　　名", join(titles, "aliases", " / "));
    row("片　　名", titles["original"].as_str().unwrap_or("").to_owned());
    row("年　　代", data["year"].as_i64().map(|y| y.to_string()).unwrap_or_default());
    row("产　　地", join(&data["extra"], "countries_zh", " / "));
    row("类　　别", join(&data["extra"], "genres_zh", " / "));
    row("语　　言", join(data, "languages", " / "));
    row("上映日期", data["release_date"].as_str().unwrap_or("").to_owned());
    if let Some(imdb) = data["ids"]["imdb"].as_str() {
        row("IMDb评分", data["extra"]["imdb_rating"].as_str().unwrap_or("").to_owned());
        row("IMDb链接", format!("https://www.imdb.com/title/{imdb}/"));
    }
    if let Some(r) = data["ratings"]["douban"].as_object() {
        row(
            "豆瓣评分",
            format!("{}/10 from {} users", r.get("value").and_then(Value::as_f64).unwrap_or(0.0), r.get("votes").and_then(Value::as_i64).unwrap_or(0)),
        );
    }
    if let Some(id) = data["ids"]["douban"].as_str() {
        row("豆瓣链接", format!("https://movie.douban.com/subject/{id}/"));
    }
    row("集　　数", data["episode_count"].as_i64().map(|e| e.to_string()).unwrap_or_default());
    row("片　　长", data["runtime_min"].as_i64().map(|m| format!("{m} 分钟")).unwrap_or_default());
    let names = |role: &str| -> Vec<String> {
        data["people"]
            .as_array()
            .map(|a| a.iter().filter(|p| p["role"] == role).filter_map(|p| p["name"].as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    };
    row("导　　演", names("director").join(" / "));
    row("编　　剧", names("writer").join(" / "));
    let cast = names("cast");
    if let Some((first, rest)) = cast.split_first() {
        lines.push(format!("◎主　　演　{first}"));
        for c in rest.iter().take(30) {
            lines.push(format!("　　　　　　{c}"));
        }
    }
    paragraphs(&mut lines, "简　　介", data["overview"]["zh-CN"].as_str());
    if let Some(a) = data["extra"]["awards"].as_str() {
        lines.push(String::new());
        lines.push("◎获奖情况".into());
        lines.push(String::new());
        for p in a.lines().map(str::trim) {
            lines.push(if p.is_empty() { String::new() } else { format!("　　{p}") });
        }
    }
    finish(&lines, markdown)
}

fn finish(lines: &[String], markdown: bool) -> String {
    if markdown {
        // Markdown 中每行之间需要硬换行
        lines.iter().map(|l| if l.is_empty() { String::new() } else { format!("{l}  ") }).collect::<Vec<_>>().join("\n")
    } else {
        lines.join("\n")
    }
}

fn paragraphs(lines: &mut Vec<String>, label: &str, text: Option<&str>) {
    if let Some(s) = text.filter(|s| !s.trim().is_empty()) {
        lines.push(String::new());
        lines.push(format!("◎{label}"));
        lines.push(String::new());
        for p in s.lines().map(str::trim).filter(|p| !p.is_empty()) {
            lines.push(format!("　　{p}"));
        }
    }
}

/// 图书版式：书名、作者、出版信息、ISBN、评分、内容简介、作者简介
fn book_lines(data: &Value, lines: &mut Vec<String>) {
    let e = &data["extra"];
    let mut row = |label: &str, value: String| {
        if !value.trim().is_empty() {
            lines.push(format!("◎{label}　{value}"));
        }
    };
    let s = |v: &Value| v.as_str().unwrap_or("").to_owned();
    let title = s(&data["titles"]["localized"]["zh-CN"]);
    row("书　　名", title.clone());
    row("副 标 题", s(&e["subtitle"]));
    let original = s(&data["titles"]["original"]);
    row(
        "原 作 名",
        if original == title {
            String::new()
        } else {
            original
        },
    );
    row("作　　者", join(e, "authors", " / "));
    row("译　　者", join(e, "translators", " / "));
    row("出 版 社", s(&e["publisher"]));
    row("出 品 方", s(&e["producer"]));
    row("出 版 年", s(&e["pubdate"]));
    row(
        "页　　数",
        e["pages"].as_i64().map(|p| p.to_string()).unwrap_or_default(),
    );
    row("定　　价", s(&e["price"]));
    row("装　　帧", s(&e["binding"]));
    row("丛　　书", s(&e["series"]));
    row("ＩＳＢＮ", s(&data["ids"]["isbn"]));
    if let Some(r) = data["ratings"]["douban"].as_object() {
        row(
            "豆瓣评分",
            format!(
                "{}/10 from {} users",
                r.get("value").and_then(Value::as_f64).unwrap_or(0.0),
                r.get("votes").and_then(Value::as_i64).unwrap_or(0)
            ),
        );
    }
    row("豆瓣链接", s(&data["attribution"]["url"]));
    paragraphs(lines, "内容简介", data["overview"]["zh-CN"].as_str());
    paragraphs(lines, "作者简介", e["author_intro"].as_str());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        assert_eq!(parse_id("1292052").as_deref(), Some("1292052"));
        assert_eq!(
            parse_id("https://movie.douban.com/subject/1292052/").as_deref(),
            Some("1292052")
        );
        assert_eq!(
            parse_id("https://book.douban.com/subject/1084336/?from=x").as_deref(),
            Some("book:1084336")
        );
        assert_eq!(parse_id("book:1084336").as_deref(), Some("book:1084336"));
        assert_eq!(parse_id("9787544253994").as_deref(), Some("isbn:9787544253994"));
        assert_eq!(parse_id("isbn:7-5442-5399-6").as_deref(), Some("isbn:7544253996"));
        assert!(parse_id("9787544253995").is_none());
        assert!(parse_id("book:x").is_none());
        assert!(parse_id("https://example.com/1").is_none());
        assert!(parse_id("12a").is_none());
    }

    fn sample_proxy() -> Value {
        json!({ "id": "1292052", "type": "movie", "title": "肖申克的救赎", "original_title": "The Shawshank Redemption", "aka": ["刺激1995", "月黑高飞"],
                "year": 1994, "pubdate": "1994-09-10", "imdb": "tt0111161", "genres": ["剧情", "犯罪"], "countries": ["美国"], "languages": ["英语"],
                "directors": ["弗兰克·德拉邦特"], "casts": ["蒂姆·罗宾斯", "摩根·弗里曼"], "durations_min": 142, "episodes_count": null,
                "rating": { "value": 9.7, "count": 3000000 }, "summary": "<p>二十世纪四十年代末……</p>" })
    }

    #[test]
    fn maps_proxy() {
        let d = from_proxy("1292052", &sample_proxy());
        assert_eq!(d["titles"]["localized"]["zh-CN"], "肖申克的救赎");
        assert_eq!(d["ids"]["imdb"], "tt0111161");
        assert_eq!(d["genres"], json!(["drama", "crime"]));
        assert_eq!(d["countries"], json!(["US"]));
        assert_eq!(d["ratings"]["douban"]["value"], 9.7);
        assert_eq!(d["overview"]["zh-CN"], "二十世纪四十年代末……");
        assert_eq!(d["people"][0], json!({ "name": "弗兰克·德拉邦特", "role": "director" }));
    }

    #[test]
    fn maps_ptgen_and_formats_intro() {
        let v = json!({ "success": true, "chinese_title": "肖申克的救赎", "foreign_title": "The Shawshank Redemption", "aka": ["刺激1995"],
                        "trans_title": ["月黑高飞"], "year": "1994", "region": ["美国"], "genre": ["剧情", "犯罪"], "language": ["英语"],
                        "playdate": ["1994-09-10(多伦多电影节)"], "imdb_id": "tt0111161", "imdb_rating": "9.3/10 from 2900000 users",
                        "douban_rating_average": "9.7", "douban_votes": "3000000", "episodes": "", "duration": "142分钟",
                        "director": [{ "name": "弗兰克·德拉邦特 Frank Darabont" }], "cast": [{ "name": "蒂姆·罗宾斯" }, { "name": "摩根·弗里曼" }],
                        "introduction": "二十世纪四十年代末……\n安迪……" });
        let d = from_ptgen("1292052", &v);
        assert_eq!((d["kind"].clone(), d["runtime_min"].clone(), d["year"].clone()), (json!("movie"), json!(142), json!(1994)));
        assert_eq!(d["titles"]["aliases"], json!(["刺激1995", "月黑高飞"]));
        let t = intro(&d, Some("/media/x.jpg"), false);
        assert!(t.starts_with("[img]/media/x.jpg[/img]"), "{t}");
        assert!(t.contains("◎译　　名　刺激1995 / 月黑高飞"), "{t}");
        assert!(t.contains("◎豆瓣评分　9.7/10 from 3000000 users"), "{t}");
        assert!(t.contains("◎IMDb链接　https://www.imdb.com/title/tt0111161/"), "{t}");
        assert!(t.contains("◎主　　演　蒂姆·罗宾斯\n　　　　　　摩根·弗里曼"), "{t}");
        assert!(t.contains("◎简　　介\n\n　　二十世纪四十年代末……\n　　安迪……"), "{t}");
        assert!(intro(&d, None, true).contains("◎片　　名　The Shawshank Redemption  \n"));
    }
}
