//! 直连模式（做法参考开源的 PT-Gen / pt-gen-cfworker），输出与 PT-Gen 端点相同的字段，再交给 `mapper::from_ptgen` 归一化：
//!
//! - 首选移动端接口 `m.douban.com/rexxar/api/v2/{movie|tv}/<id>`（含 credits / awards）与 h5 简介页（IMDb）：
//!   服务器 IP 访问桌面页常被要求登录，移动端接口通常仍可用；
//! - 备用：解析桌面条目页（ld+json + `#info` 区块 + 简介 + 获奖页）。
//!
//! 只用字符串切片，不引入 HTML 解析库（WASM 体积）；豆瓣改版时以测试中的样例页为准调整。

use pt_plugin_sdk::serde_json::{self, Value, json};

/// 页面状态：反爬拦截页、条目不存在页
pub enum Page {
    Blocked,
    NotFound,
    Ok(Value),
}

fn between<'a>(s: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let i = s.find(start)? + start.len();
    let j = s[i..].find(end)?;
    Some(&s[i..i + j])
}

fn all_between<'a>(s: &'a str, start: &str, end: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find(start) {
        let from = i + start.len();
        let Some(j) = rest[from..].find(end) else { break };
        out.push(&rest[from..from + j]);
        rest = &rest[from + j + end.len()..];
    }
    out
}

/// HTML 实体（常用命名实体 + 数字实体）
pub fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(j) = rest.bytes().take(12).position(|b| b == b';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..j];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "middot" => Some('·'),
            _ => ent
                .strip_prefix("#x")
                .or_else(|| ent.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[j + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// 去标签、解实体；`<br>` 与块级结束标签换行，每行去首尾空白，丢弃空行
pub fn text(html: &str) -> String {
    let mut buf = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(i) = rest.find('<') {
        buf.push_str(&rest[..i]);
        let Some(j) = rest[i..].find('>') else {
            rest = "";
            break;
        };
        let tag = rest[i + 1..i + j].trim_start_matches('/').to_ascii_lowercase();
        let name = tag.split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        if matches!(name, "br" | "p" | "div" | "li" | "h2" | "ul") {
            buf.push('\n');
        }
        rest = &rest[i + j + 1..];
    }
    buf.push_str(rest);
    decode(&buf)
        .lines()
        .map(|l| l.trim_matches(|c: char| c.is_whitespace() || c == '\u{3000}'))
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `#info` 区块：`标签: 值` 逐行
fn info_fields(html: &str) -> Vec<(String, String)> {
    let Some(block) = between(html, "<div id=\"info\"", "</div>") else { return Vec::new() };
    let block = block.split_once('>').map_or(block, |(_, b)| b);
    block
        .split("<br")
        .filter_map(|line| {
            let line = line.split_once('>').map_or(line, |(a, b)| if a.trim_start().starts_with('/') || a.trim().is_empty() { b } else { line });
            let t = text(line).replace('\n', " ");
            let (k, v) = t.split_once(':').or_else(|| t.split_once('：'))?;
            let (k, v) = (k.trim(), v.trim());
            (!k.is_empty() && !v.is_empty()).then(|| (k.to_owned(), v.to_owned()))
        })
        .collect()
}

fn split_slash(v: &str) -> Vec<Value> {
    v.split(" / ").map(str::trim).filter(|x| !x.is_empty()).map(|x| json!(x)).collect()
}

fn names(v: &Value) -> Value {
    let list = match v {
        Value::Array(a) => a.clone(),
        Value::Object(_) => vec![v.clone()],
        _ => Vec::new(),
    };
    Value::Array(list.iter().filter_map(|p| p["name"].as_str()).map(|n| json!({ "name": decode(n).trim() })).collect())
}

/// 解析条目页
pub fn subject(html: &str) -> Page {
    if html.contains("检测到有异常请求") || (html.contains("sec.douban.com") && !html.contains("v:itemreviewed")) {
        return Page::Blocked;
    }
    if html.contains("你想访问的页面不存在") || !html.contains("v:itemreviewed") {
        return Page::NotFound;
    }
    let ld: Value = between(html, "<script type=\"application/ld+json\">", "</script>")
        .map(|s| s.replace(['\r', '\n', '\t'], ""))
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    let chinese = between(html, "<title>", "</title>").map(|t| text(t).replace("(豆瓣)", "").trim().to_owned()).unwrap_or_default();
    let full = between(html, "property=\"v:itemreviewed\">", "</span>").map(text).unwrap_or_default();
    let foreign = full.strip_prefix(chinese.as_str()).unwrap_or("").trim().to_owned();
    let year = between(html, "<span class=\"year\">", "</span>")
        .map(|y| text(y).trim_matches(['(', ')']).to_owned())
        .unwrap_or_default();
    let info = info_fields(html);
    let field = |k: &str| info.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
    let genre: Vec<Value> = all_between(html, "property=\"v:genre\">", "</span>").into_iter().map(|g| json!(text(g))).collect();
    let mut playdate: Vec<String> = all_between(html, "property=\"v:initialReleaseDate\"", "</span>")
        .into_iter()
        .filter_map(|s| s.split_once('>').map(|(_, t)| text(t)))
        .collect();
    playdate.sort();
    let duration = field("片长")
        .or_else(|| field("单集片长"))
        .map(str::to_owned)
        .or_else(|| between(html, "property=\"v:runtime\"", "</span>").and_then(|s| s.split_once('>')).map(|(_, t)| text(t)))
        .unwrap_or_default();
    // 简介：展开后的全文优先
    let intro = between(html, "<span class=\"all hidden\">", "</span>")
        .or_else(|| between(html, "property=\"v:summary\"", "</span>").and_then(|s| s.split_once('>').map(|(_, t)| t)))
        .map(text)
        .unwrap_or_default();
    let imdb = field("IMDb").or_else(|| field("IMDb链接")).map(|s| s.split_whitespace().next().unwrap_or("").to_owned());
    // 海报换成大图（s_ratio_poster → l_ratio_poster）
    let poster = ld["image"]
        .as_str()
        .map(|p| p.replace("s_ratio_poster", "l_ratio_poster").replace("/spic/", "/lpic/"))
        .or_else(|| between(html, "<div id=\"mainpic\"", "</div>").and_then(|m| between(m, "src=\"", "\"")).map(str::to_owned));
    let rating = &ld["aggregateRating"];
    Page::Ok(json!({
        "success": true,
        "chinese_title": chinese,
        "foreign_title": foreign,
        "aka": field("又名").map(split_slash).unwrap_or_default(),
        "trans_title": [],
        "year": year,
        "region": field("制片国家/地区").map(split_slash).unwrap_or_default(),
        "genre": genre,
        "language": field("语言").map(split_slash).unwrap_or_default(),
        "playdate": playdate,
        "imdb_id": imdb,
        "douban_rating_average": rating["ratingValue"].as_str().map(str::to_owned).or_else(|| rating["ratingValue"].as_f64().map(|x| x.to_string())),
        "douban_votes": rating["ratingCount"].as_str().map(str::to_owned).or_else(|| rating["ratingCount"].as_i64().map(|x| x.to_string())),
        "episodes": field("集数").unwrap_or(""),
        "duration": duration,
        "director": names(&ld["director"]),
        "writer": names(&ld["author"]),
        "cast": names(&ld["actor"]),
        "introduction": intro,
        "poster": poster,
    }))
}

fn person(p: &Value) -> Option<Value> {
    let name = p["name"].as_str()?.trim();
    let latin = p["latin_name"].as_str().map(str::trim).filter(|l| !l.is_empty() && *l != name);
    Some(json!({ "name": latin.map_or_else(|| name.to_owned(), |l| format!("{name} {l}")) }))
}

/// 移动端接口：条目 + 演职员（`credits`，可缺）+ 获奖（`awards`，可缺）+ IMDb（h5 简介页，可缺）
pub fn from_rexxar(v: &Value, credits: &Value, awards: &Value, imdb: Option<String>) -> Value {
    let strs = |k: &str| v[k].as_array().cloned().unwrap_or_default();
    let by_cat = |cat: &str| -> Vec<Value> {
        credits["items"].as_array().map(|a| a.iter().filter(|p| p["category"] == cat).filter_map(person).collect()).unwrap_or_default()
    };
    let fallback = |k: &str| -> Vec<Value> { v[k].as_array().map(|a| a.iter().filter_map(person).collect()).unwrap_or_default() };
    let pick = |cat: &str, k: &str| {
        let c = by_cat(cat);
        if c.is_empty() { fallback(k) } else { c }
    };
    let title = v["title"].as_str().unwrap_or("").trim();
    let original = v["original_title"].as_str().map(str::trim).filter(|o| *o != title).unwrap_or("");
    let episodes = v["episodes_count"].as_i64().filter(|e| *e > 0);
    let poster = v["pic"]["large"].as_str().or(v["cover_url"].as_str()).map(|p| p.replace("m_ratio_poster", "l_ratio_poster"));
    json!({
        "success": true,
        "chinese_title": title,
        "foreign_title": original,
        "aka": strs("aka"),
        "trans_title": [],
        "year": v["year"].as_str().unwrap_or(""),
        "region": strs("countries"),
        "genre": strs("genres"),
        "language": strs("languages"),
        "playdate": strs("pubdate"),
        "imdb_id": imdb,
        "douban_rating_average": v["rating"]["value"].as_f64().filter(|x| *x > 0.0).map(|x| x.to_string()),
        "douban_votes": v["rating"]["count"].as_i64().map(|x| x.to_string()),
        "episodes": episodes.map(|e| e.to_string()).unwrap_or_default(),
        "duration": v["durations"].as_array().and_then(|a| a.first()).and_then(Value::as_str).unwrap_or(""),
        "director": pick("导演", "directors"),
        "writer": by_cat("编剧"),
        "cast": pick("演员", "actors"),
        "introduction": v["intro"].as_str().map(text).unwrap_or_default(),
        "poster": poster,
        "awards": rexxar_awards(awards),
        "kind": if v["is_tv"] == true || v["subtype"] == "tv" { "tv" } else { "movie" },
    })
}

fn rexxar_awards(a: &Value) -> String {
    a["awards"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|x| {
                    let c = &x["ceremony"];
                    let head = match c["year"].as_i64() {
                        Some(y) => format!("{} ({y})", c["title"].as_str()?),
                        None => c["title"].as_str()?.to_owned(),
                    };
                    let rows = x["categories"].as_array().map_or_else(Vec::new, |cs| {
                        cs.iter()
                            .filter_map(|cat| {
                                let t = cat["category"]["title"].as_str()?;
                                let won = if cat["is_won"] == true { "" } else { "(提名)" };
                                let who: Vec<&str> = cat["celebrities"].as_array().map(|p| p.iter().filter_map(|c| c["name"].as_str()).collect()).unwrap_or_default();
                                Some(if who.is_empty() { format!("{t}{won}") } else { format!("{t}{won} {}", who.join(" / ")) })
                            })
                            .collect()
                    });
                    Some(std::iter::once(head).chain(rows).collect::<Vec<_>>().join("\n"))
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .unwrap_or_default()
}

/// h5 简介页中的 IMDb 编号
pub fn h5_imdb(html: &str) -> Option<String> {
    let after = html.split("<td>IMDb</td>").nth(1)?;
    let id = text(between(after, "<td>", "</td>")?);
    (id.starts_with("tt") && id.len() <= 12).then_some(id)
}

/// 移动端搜索（`/rexxar/api/v2/search/movie?q=`）
pub fn rexxar_search(body: &[u8]) -> Vec<Value> {
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    v["items"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|x| matches!(x["target_type"].as_str(), Some("movie" | "tv")))
                .filter_map(|x| {
                    let t = &x["target"];
                    Some(json!({ "external_id": t["id"].as_str()?, "title": t["title"], "subtitle": t["card_subtitle"],
                                 "year": t["year"].as_str().and_then(|y| y.parse::<i64>().ok()), "kind": x["target_type"] }))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// h5 简介页的表格（图书：作者、出版社、ISBN、原作名……）：每个 `<tr>` 取前两个 `<td>`
pub fn h5_table(html: &str) -> Vec<(String, String)> {
    html.split("<tr")
        .skip(1)
        .filter_map(|row| {
            let cells = all_between(row.split("</tr>").next().unwrap_or(row), "<td>", "</td>");
            let (k, v) = (text(cells.first()?), text(cells.get(1)?));
            (!k.is_empty() && !v.is_empty()).then_some((k, v.replace('\n', " ")))
        })
        .collect()
}

fn first_num(s: &str) -> Option<i64> {
    s.split(|c: char| !c.is_ascii_digit())
        .find(|x| !x.is_empty())?
        .parse()
        .ok()
}

/// 图书的 `#info` / h5 表格字段（两处键名相同）→ 图书中间格式（`mapper::from_book` 的输入）
fn book_fields(info: &[(String, String)]) -> Value {
    let field = |k: &str| info.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
    let list = |k: &str| field(k).map(split_slash).unwrap_or_default();
    let isbn: Option<String> =
        field("ISBN").map(|i| i.chars().filter(|c| c.is_ascii_digit() || *c == 'X').collect());
    json!({
        "subtitle": field("副标题"),
        "original_title": field("原作名"),
        "authors": list("作者"),
        "translators": list("译者"),
        "publisher": field("出版社"),
        "producer": field("出品方"),
        "pubdate": field("出版年"),
        "pages": field("页数").and_then(first_num),
        "price": field("定价"),
        "binding": field("装帧"),
        "series": field("丛书"),
        "isbn": isbn.filter(|i| i.len() == 10 || i.len() == 13),
    })
}

/// 与 `fallback` 合并：`v` 中为空的字段用 `fallback` 的
fn merge(mut v: Value, fallback: Value) -> Value {
    if let (Some(o), Value::Object(f)) = (v.as_object_mut(), fallback) {
        for (k, x) in f {
            let empty = match o.get(&k) {
                None | Some(Value::Null) => true,
                Some(Value::String(s)) => s.is_empty(),
                Some(Value::Array(a)) => a.is_empty(),
                _ => false,
            };
            if empty {
                o.insert(k, x);
            }
        }
    }
    v
}

/// 移动端图书接口（`/rexxar/api/v2/book/<id>`，没有 ISBN 与原作名）+ h5 简介页表格（可缺）
pub fn book_from_rexxar(v: &Value, desc: &[(String, String)]) -> Value {
    let strs = |k: &str| -> Vec<Value> {
        v[k].as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| json!(s))
                    .collect()
            })
            .unwrap_or_default()
    };
    let first = |k: &str| {
        v[k].as_array()
            .and_then(|a| a.first())
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    };
    let subtitle = v["book_subtitle"]
        .as_str()
        .filter(|s| !s.is_empty())
        .or_else(|| first("subtitle"));
    let own = json!({
        "title": v["title"].as_str().unwrap_or("").trim(),
        "subtitle": subtitle,
        "authors": strs("author"),
        "translators": strs("translator"),
        "publisher": first("press"),
        "producer": first("producers"),
        "pubdate": first("pubdate"),
        "pages": first("pages").and_then(first_num),
        "price": first("price"),
        "series": v["book_series"]["title"].as_str(),
        "rating": v["rating"]["value"].as_f64().filter(|x| *x > 0.0).map(|x| json!({ "value": x, "count": v["rating"]["count"] })),
        "poster": v["pic"]["large"].as_str().or(v["cover_url"].as_str()),
        "intro": v["intro"].as_str().map(text).unwrap_or_default(),
        "author_intro": v["author_intro"].as_str().map(text).unwrap_or_default(),
    });
    merge(own, book_fields(desc))
}

/// 图书简介区块（“内容简介” / “作者简介”）：展开后的全文优先，去掉内嵌样式
fn book_section(html: &str, heading: &str) -> String {
    let Some(rest) = html.split(&format!("<span>{heading}</span>")).nth(1) else {
        return String::new();
    };
    let rest = rest.split("<h2").next().unwrap_or(rest);
    let rest = rest.split("<span class=\"all hidden\">").nth(1).unwrap_or(rest);
    let mut s = between(rest, "<div class=\"intro\">", "</div>")
        .unwrap_or("")
        .to_owned();
    while let (Some(i), Some(j)) = (s.find("<style"), s.find("</style>")) {
        if j < i {
            break;
        }
        s.replace_range(i..j + "</style>".len(), "");
    }
    text(&s)
}

/// 解析图书条目页（`book.douban.com/subject/<id>/`）
pub fn book_page(html: &str) -> Page {
    if html.contains("检测到有异常请求")
        || (html.contains("sec.douban.com") && !html.contains("v:itemreviewed"))
    {
        return Page::Blocked;
    }
    if html.contains("你想访问的页面不存在") || !html.contains("v:itemreviewed") {
        return Page::NotFound;
    }
    let ld: Value = between(html, "<script type=\"application/ld+json\">", "</script>")
        .map(|s| s.replace(['\r', '\n', '\t'], ""))
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    let mut v = book_fields(&info_fields(html));
    let rating = between(html, "property=\"v:average\">", "<")
        .map(|r| r.trim().to_owned())
        .and_then(|r| r.parse::<f64>().ok())
        .filter(|x| *x > 0.0);
    let votes = between(html, "property=\"v:votes\">", "<")
        .and_then(|n| n.trim().parse::<i64>().ok())
        .unwrap_or(0);
    let poster = between(html, "<div id=\"mainpic\"", "</div>")
        .and_then(|m| between(m, "href=\"", "\""))
        .filter(|p| p.contains("doubanio.com"))
        .or_else(|| between(html, "<meta property=\"og:image\" content=\"", "\""));
    v["title"] = json!(
        between(html, "property=\"v:itemreviewed\">", "</span>")
            .map(text)
            .unwrap_or_default()
    );
    if v["isbn"].is_null() {
        v["isbn"] = ld["isbn"].clone();
    }
    v["rating"] = rating.map_or(Value::Null, |x| json!({ "value": x, "count": votes }));
    v["poster"] = json!(poster);
    v["intro"] = json!(book_section(html, "内容简介"));
    v["author_intro"] = json!(book_section(html, "作者简介"));
    Page::Ok(v)
}

/// 移动端图书搜索（`/rexxar/api/v2/search/book?q=`），年份取自卡片副标题
pub fn rexxar_book_search(body: &[u8]) -> Vec<Value> {
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    v["items"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|x| x["target_type"] == "book")
                .filter_map(|x| {
                    let t = &x["target"];
                    let sub = t["card_subtitle"].as_str().unwrap_or("");
                    let year = sub.split(" / ").find_map(|p| p.trim().get(..4).filter(|y| y.bytes().all(|b| b.is_ascii_digit())).and_then(|y| y.parse::<i64>().ok()));
                    Some(json!({ "external_id": format!("book:{}", t["id"].as_str()?), "title": t["title"], "subtitle": sub, "year": year, "kind": "book" }))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 获奖页（`/subject/<id>/awards/`）：每个奖项一段，标题一行、各奖项一行
pub fn awards(html: &str) -> String {
    let Some(article) = html.split("<div class=\"article\">").nth(1) else { return String::new() };
    article
        .split("<div class=\"awards")
        .skip(1)
        .filter_map(|block| {
            let head = between(block, "<h2>", "</h2>").map(|h| text(&h.replace("<span", " <span")).replace('\n', " "))?;
            let items: Vec<String> = all_between(block, "<ul class=\"award\">", "</ul>").into_iter().map(|u| text(u).replace('\n', " ")).collect();
            Some(std::iter::once(head).chain(items).collect::<Vec<_>>().join("\n"))
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 搜索建议接口（movie / book 子域的 `/j/subject_suggest?q=`）→ 统一的搜索结果
pub fn suggest(body: &[u8]) -> Vec<Value> {
    let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    v.as_array()
        .map(|a| {
            a.iter()
                .filter(|x| matches!(x["type"].as_str(), Some("movie" | "b")) && x["id"].as_str().is_some())
                .map(|x| {
                    let year = x["year"].as_str().and_then(|y| y.parse::<i64>().ok());
                    if x["type"] == "b" {
                        return json!({ "external_id": format!("book:{}", x["id"].as_str().unwrap_or("")), "title": x["title"],
                                       "subtitle": x["author_name"], "year": year, "kind": "book" });
                    }
                    let tv = x["episode"].as_str().is_some_and(|e| !e.is_empty());
                    json!({ "external_id": x["id"], "title": x["title"], "original_title": x["sub_title"],
                            "year": year, "kind": if tv { "tv" } else { "movie" } })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str = r#"<html><head><title>
        肖申克的救赎 (豆瓣)
</title>
<script type="application/ld+json">
{
  "@context": "http://schema.org",
  "name": "肖申克的救赎 The Shawshank Redemption",
  "url": "/subject/1292052/",
  "image": "https://img3.doubanio.com/view/photo/s_ratio_poster/public/p480747492.jpg",
  "director": [{"@type": "Person", "url": "/celebrity/1047973/", "name": "弗兰克·德拉邦特 Frank Darabont"}],
  "author": [{"@type": "Person", "url": "/celebrity/1047973/", "name": "弗兰克·德拉邦特 Frank Darabont"}],
  "actor": [{"@type": "Person", "url": "/celebrity/1054521/", "name": "蒂姆·罗宾斯 Tim Robbins"}, {"@type": "Person", "url": "/celebrity/1054534/", "name": "摩根·弗里曼 Morgan Freeman"}],
  "datePublished": "1994-09-10",
  "genre": ["剧情", "犯罪"],
  "duration": "PT2H22M",
  "description": "20世纪40年代末，小有成就的青年银行家安迪...",
  "@type": "Movie",
  "aggregateRating": {"@type": "AggregateRating", "ratingCount": "3156789", "bestRating": "10", "worstRating": "2", "ratingValue": "9.7"}
}
</script></head><body>
<div id="content"><h1><span property="v:itemreviewed">肖申克的救赎 The Shawshank Redemption</span>
<span class="year">(1994)</span></h1>
<div id="mainpic" class=""><a class="nbgnbg" href="x"><img src="https://img3.doubanio.com/view/photo/s_ratio_poster/public/p480747492.jpg" /></a></div>
<div id="info">
        <span ><span class='pl'>导演</span>: <span class='attrs'><a href="/celebrity/1047973/" rel="v:directedBy">弗兰克·德拉邦特</a></span></span><br/>
        <span class="pl">类型:</span> <span property="v:genre">剧情</span> / <span property="v:genre">犯罪</span><br/>
        <span class="pl">制片国家/地区:</span> 美国<br/>
        <span class="pl">语言:</span> 英语<br/>
        <span class="pl">上映日期:</span> <span property="v:initialReleaseDate" content="1994-09-10(多伦多电影节)">1994-09-10(多伦多电影节)</span> / <span property="v:initialReleaseDate" content="1994-10-14(美国)">1994-10-14(美国)</span><br/>
        <span class="pl">片长:</span> <span property="v:runtime" content="142">142分钟</span><br/>
        <span class="pl">又名:</span> 月黑高飞(港) / 刺激1995(台)<br/>
        <span class="pl">IMDb:</span> tt0111161<br>
</div>
<span property="v:summary" class="">
  短简介
</span>
<span class="all hidden">
  　　20世纪40年代末，小有成就的青年银行家安迪&middot;杜佛兰……<br />
  　　监狱是座枷锁……
</span>
</div></body></html>"#;

    #[test]
    fn parses_subject_page() {
        let Page::Ok(v) = subject(PAGE) else { panic!("not parsed") };
        assert_eq!(v["chinese_title"], "肖申克的救赎");
        assert_eq!(v["foreign_title"], "The Shawshank Redemption");
        assert_eq!(v["year"], "1994");
        assert_eq!(v["region"], json!(["美国"]));
        assert_eq!(v["language"], json!(["英语"]));
        assert_eq!(v["genre"], json!(["剧情", "犯罪"]));
        assert_eq!(v["aka"], json!(["月黑高飞(港)", "刺激1995(台)"]));
        assert_eq!(v["imdb_id"], "tt0111161");
        assert_eq!(v["duration"], "142分钟");
        assert_eq!(v["playdate"], json!(["1994-09-10(多伦多电影节)", "1994-10-14(美国)"]));
        assert_eq!(v["douban_rating_average"], "9.7");
        assert_eq!(v["cast"][1]["name"], "摩根·弗里曼 Morgan Freeman");
        assert_eq!(v["introduction"], "20世纪40年代末，小有成就的青年银行家安迪·杜佛兰……\n监狱是座枷锁……");
        assert_eq!(v["poster"], "https://img3.doubanio.com/view/photo/l_ratio_poster/public/p480747492.jpg");
        let d = crate::mapper::from_ptgen("1292052", &v);
        assert_eq!((d["kind"].clone(), d["runtime_min"].clone()), (json!("movie"), json!(142)));
        assert_eq!(d["countries"], json!(["US"]));
    }

    #[test]
    fn detects_block_and_missing() {
        assert!(matches!(subject("<html>检测到有异常请求从你的 IP 发出</html>"), Page::Blocked));
        assert!(matches!(subject("<html><title>页面不存在</title>你想访问的页面不存在</html>"), Page::NotFound));
    }

    #[test]
    fn parses_awards_and_suggest() {
        let a = r#"<div class="article"><div class="awards"><div class="hd"><h2><a href="x">第67届奥斯卡金像奖</a><span class="year">(1995)</span></h2></div>
            <ul class="award"><li><a>最佳影片(提名)</a></li><li><a>妮基·马文</a></li></ul>
            <ul class="award"><li>最佳男主角(提名)</li><li><a>摩根·弗里曼</a></li></ul></div></div>"#;
        assert_eq!(awards(a), "第67届奥斯卡金像奖 (1995)\n最佳影片(提名) 妮基·马文\n最佳男主角(提名) 摩根·弗里曼");
        let s = br#"[{"episode":"","img":"x","title":"t0","url":"u","type":"movie","year":"1994","sub_title":"The Shawshank Redemption","id":"1292052"},
                     {"episode":"12","title":"t","type":"movie","year":"2023","sub_title":"s","id":"36000000"},
                     {"title":"b","type":"book","id":"1"}]"#;
        let r = suggest(s);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0]["year"], 1994);
        assert_eq!(r[1]["kind"], "tv");
    }

    #[test]
    fn maps_rexxar() {
        let v = json!({ "title": "肖申克的救赎", "original_title": "The Shawshank Redemption", "aka": ["月黑高飞(港)"], "year": "1994",
                        "countries": ["美国"], "genres": ["剧情", "犯罪"], "languages": ["英语"], "pubdate": ["1994-09-10(多伦多电影节)"],
                        "rating": { "count": 3352109, "value": 9.7 }, "episodes_count": 0, "durations": ["142分钟"], "is_tv": false,
                        "directors": [{ "name": "弗兰克·德拉邦特" }], "actors": [{ "name": "蒂姆·罗宾斯" }],
                        "intro": "一场谋杀案……\n　　瑞德……", "pic": { "large": "https://img3.doubanio.com/view/photo/m_ratio_poster/public/p1.jpg" } });
        let credits = json!({ "items": [
            { "category": "导演", "name": "弗兰克·德拉邦特", "latin_name": "Frank Darabont" },
            { "category": "演员", "name": "蒂姆·罗宾斯", "latin_name": "Tim Robbins" },
            { "category": "编剧", "name": "斯蒂芬·金", "latin_name": "Stephen King" } ] });
        let awards = json!({ "awards": [{ "ceremony": { "title": "第67届奥斯卡金像奖", "year": 1995 },
            "categories": [{ "category": { "title": "最佳影片" }, "is_won": false, "celebrities": [{ "name": "妮基·马文" }] },
                           { "category": { "title": "最佳剪辑" }, "is_won": true, "celebrities": [] }] }] });
        let r = from_rexxar(&v, &credits, &awards, Some("tt0111161".into()));
        assert_eq!(r["director"], json!([{ "name": "弗兰克·德拉邦特 Frank Darabont" }]));
        assert_eq!(r["writer"][0]["name"], "斯蒂芬·金 Stephen King");
        assert_eq!(r["poster"], "https://img3.doubanio.com/view/photo/l_ratio_poster/public/p1.jpg");
        assert_eq!(r["awards"], "第67届奥斯卡金像奖 (1995)\n最佳影片(提名) 妮基·马文\n最佳剪辑");
        assert_eq!(r["introduction"], "一场谋杀案……\n瑞德……");
        let d = crate::mapper::from_ptgen("1292052", &r);
        assert_eq!((d["kind"].clone(), d["runtime_min"].clone(), d["ids"]["imdb"].clone()), (json!("movie"), json!(142), json!("tt0111161")));
        // 没有 credits 时退回条目自带的名单
        assert_eq!(from_rexxar(&v, &Value::Null, &Value::Null, None)["cast"], json!([{ "name": "蒂姆·罗宾斯" }]));
        assert_eq!(h5_imdb("<tr><td>IMDb</td>\n   <td>tt0111161</td></tr>").as_deref(), Some("tt0111161"));
        let s = br#"{"items":[{"target_type":"tv","target":{"id":"37040714","title":"t","year":"2026","card_subtitle":"x"}},{"target_type":"doulist","target":{"id":"1"}}]}"#;
        assert_eq!(rexxar_search(s), vec![json!({ "external_id": "37040714", "title": "t", "subtitle": "x", "year": 2026, "kind": "tv" })]);
    }

    const BOOK_PAGE: &str = r#"<html><head><title>示例之书 (豆瓣)</title>
<script type="application/ld+json">
{
  "@context":"http://schema.org",
  "@type":"Book",
  "name" : "示例之书",
  "author": [ { "@type": "Person", "name": "[英] 某作者" } ],
  "url" : "https://book.douban.com/subject/1000001/",
  "isbn" : "9787111111115"
}
</script>
<meta property="og:image" content="https://img1.doubanio.com/view/subject/l/public/s1.jpg" />
</head><body>
<h1><span property="v:itemreviewed">示例之书</span></h1>
<div id="mainpic">
  <a class="nbg"
      href="https://img1.doubanio.com/view/subject/l/public/s1.jpg" title="示例之书">
    <img src="https://img1.doubanio.com/view/subject/s/public/s1.jpg" /></a>
</div>
<div id="info">
    <span>
      <span class="pl"> 作者</span>:
            <a href="/author/1">[英] 某作者</a>
            /
            <a href="/author/2">另一位</a>
    </span><br/>
    <span class="pl">出版社:</span>
      <a href="https://book.douban.com/press/1">示例出版社</a>
    <br>
    <span class="pl">原作名:</span> An Example Book<br/>
    <span>
      <span class="pl"> 译者</span>:
        <a href="/search/x">某译者</a>
    </span><br/>
    <span class="pl">出版年:</span> 2011-6<br/>
      <span class="pl">ISBN:</span> 9787111111115<br/>
    <span class="pl">页数:</span> 360<br/>
    <span class="pl">定价:</span> 39.50元<br/>
    <span class="pl">装帧:</span> 精装<br/>
    <span class="pl">丛书:</span>&nbsp;<a href="https://book.douban.com/series/1">示例文库</a><br>
</div>
<strong class="ll rating_num" property="v:average"> 9.1 </strong>
<a href="comments" class="rating_people"><span property="v:votes">1234</span>人评价</a>
<h2><span>内容简介</span>&nbsp;&middot;</h2>
<div class="indent" id="link-report">
  <span class="short"><div class="intro"><p>短的……</p></div></span>
  <span class="all hidden">
    <div class="intro">
<style type="text/css" media="screen">
.intro p{text-indent:2em;}
</style>
    <p>第一段&middot;全文。</p><p>第二段。</p></div>
  </span>
</div>
<h2><span>作者简介</span>&nbsp;&middot;</h2>
<div class="indent "><div class="intro"><p>某作者，生于某地。</p></div></div>
<h2><span>目录</span></h2>
</body></html>"#;

    #[test]
    fn parses_book_page() {
        let Page::Ok(v) = book_page(BOOK_PAGE) else {
            panic!("not parsed")
        };
        assert_eq!(v["title"], "示例之书");
        assert_eq!(v["authors"], json!(["[英] 某作者", "另一位"]));
        assert_eq!(v["translators"], json!(["某译者"]));
        assert_eq!(v["publisher"], "示例出版社");
        assert_eq!(v["original_title"], "An Example Book");
        assert_eq!(v["pubdate"], "2011-6");
        assert_eq!(v["isbn"], "9787111111115");
        assert_eq!(v["pages"], 360);
        assert_eq!(v["binding"], "精装");
        assert_eq!(v["series"], "示例文库");
        assert_eq!(v["rating"], json!({ "value": 9.1, "count": 1234 }));
        assert_eq!(
            v["poster"],
            "https://img1.doubanio.com/view/subject/l/public/s1.jpg"
        );
        assert_eq!(v["intro"], "第一段·全文。\n第二段。");
        assert_eq!(v["author_intro"], "某作者，生于某地。");
        assert!(matches!(
            book_page("<html>你想访问的页面不存在</html>"),
            Page::NotFound
        ));
    }

    #[test]
    fn maps_book_rexxar_and_search() {
        let v = json!({ "id": "1000001", "title": "示例之书", "book_subtitle": "", "subtitle": [], "author": ["[英] 某作者"], "translator": ["某译者"],
                        "press": ["示例出版社"], "producers": ["示例文化"], "pubdate": ["2011-6"], "pages": ["360页"], "price": ["39.50元"],
                        "book_series": { "title": "示例文库" }, "rating": { "count": 1234, "value": 9.1 },
                        "pic": { "large": "https://img1.doubanio.com/view/subject/l/public/s1.jpg" }, "intro": "第一段。\n第二段。", "author_intro": "某作者。" });
        let desc = h5_table(
            "<table><tr>\n <td>作者</td>\n <td>[英] 某作者</td></tr><tr><td>ISBN</td>\n<td>9787111111115</td></tr>\
                             <tr><td>原作名</td><td>An Example Book</td></tr><tr><td>装帧</td><td>精装</td></tr></table>",
        );
        assert_eq!(desc.len(), 4);
        let b = book_from_rexxar(&v, &desc);
        assert_eq!(
            (
                b["isbn"].clone(),
                b["original_title"].clone(),
                b["binding"].clone()
            ),
            (json!("9787111111115"), json!("An Example Book"), json!("精装"))
        );
        assert_eq!(
            (b["pages"].clone(), b["producer"].clone(), b["subtitle"].clone()),
            (json!(360), json!("示例文化"), Value::Null)
        );
        // 移动端接口没有 ISBN 时仍可用
        assert!(book_from_rexxar(&v, &[])["isbn"].is_null());
        let s = r#"{"items":[{"target_type":"book","target":{"id":"1000001","title":"t","year":null,"card_subtitle":"[英] 某作者 / 2011 / 示例出版社"}},
                              {"target_type":"movie","target":{"id":"2"}}]}"#;
        assert_eq!(
            rexxar_book_search(s.as_bytes()),
            vec![
                json!({ "external_id": "book:1000001", "title": "t", "subtitle": "[英] 某作者 / 2011 / 示例出版社", "year": 2011, "kind": "book" })
            ]
        );
        let r = suggest(br#"[{"title":"t","author_name":"a","year":"2011","type":"b","id":"1000001"}]"#);
        assert_eq!(r[0]["external_id"], "book:1000001");
        let d = crate::mapper::from_book("1000001", &b);
        assert_eq!(d["kind"], "book");
        assert_eq!(d["year"], 2011);
        assert!(d["release_date"].is_null());
        assert_eq!(d["ids"], json!({ "douban": "1000001", "isbn": "9787111111115" }));
        assert_eq!(d["titles"]["original"], "An Example Book");
        assert_eq!(d["people"][1], json!({ "name": "某译者", "role": "translator" }));
        assert_eq!(d["ratings"]["douban"], json!({ "value": 9.1, "votes": 1234 }));
        assert_eq!(
            d["attribution"]["url"],
            "https://book.douban.com/subject/1000001/"
        );
        let t = crate::mapper::intro(&d, Some("/media/b.jpg"), false);
        assert!(t.starts_with("[img]/media/b.jpg[/img]\n\n◎书　　名　示例之书\n◎原 作 名　An Example Book\n◎作　　者　[英] 某作者"), "{t}");
        assert!(t.contains("◎ＩＳＢＮ　9787111111115"), "{t}");
        assert!(
            t.contains("◎豆瓣链接　https://book.douban.com/subject/1000001/"),
            "{t}"
        );
        assert!(
            t.ends_with("◎内容简介\n\n　　第一段。\n　　第二段。\n\n◎作者简介\n\n　　某作者。"),
            "{t}"
        );
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(decode("a&amp;b &#38; &#x4e2d; &bogus x"), "a&b & 中 &bogus x");
    }
}

