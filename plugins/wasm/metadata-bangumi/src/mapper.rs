//! Bangumi `/v0/subjects/{id}` → 统一元数据模型（metadata-providers-common §2、LLD-MDBGM §1）。纯函数。

use pt_plugin_sdk::serde_json::{Map, Value, json};

/// 条目编号：纯数字、`bangumi:<数字>`，或 bgm.tv / bangumi.tv / chii.in 的条目链接
pub fn parse_id(s: &str) -> Option<u64> {
    let s = s.trim();
    let digits = |x: &str| -> Option<u64> {
        let d: String = x.chars().take_while(char::is_ascii_digit).collect();
        (!d.is_empty() && d.len() <= 10)
            .then(|| d.parse().ok())
            .flatten()
            .filter(|n| *n > 0)
    };
    if let Some(rest) = s.strip_prefix("bangumi:") {
        return (rest.bytes().all(|b| b.is_ascii_digit()))
            .then(|| digits(rest))
            .flatten();
    }
    if s.bytes().all(|b| b.is_ascii_digit()) {
        return digits(s);
    }
    ["bgm.tv/subject/", "bangumi.tv/subject/", "chii.in/subject/"]
        .iter()
        .find_map(|p| s.split(p).nth(1))
        .and_then(digits)
}

/// Bangumi 条目类型 → 统一模型 kind
pub fn kind(t: i64) -> &'static str {
    match t {
        1 => "book",
        3 => "music_release_group",
        4 => "game",
        6 => "tv",
        _ => "anime",
    }
}

/// infobox 的值：字符串或 `[{v: …}]` / `[{k: …, v: …}]` 数组，统一为文本列表
fn info_values(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![s.trim().to_owned()],
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x["v"].as_str().or_else(|| x.as_str()))
            .map(|s| s.trim().to_owned())
            .collect(),
        _ => Vec::new(),
    }
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect()
}

fn info<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v["infobox"]
        .as_array()?
        .iter()
        .find(|i| i["key"] == key)
        .map(|i| &i["value"])
}

/// 人名字段：按 `、` / ` / ` / `，` 拆分
fn names(v: Option<&Value>) -> Vec<String> {
    v.map(info_values)
        .unwrap_or_default()
        .iter()
        .flat_map(|s| s.split(['、', '，']).flat_map(|x| x.split(" / ")))
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}

fn date_ok(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && s.bytes()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

pub fn map(v: &Value) -> Value {
    let id = v["id"].as_u64();
    let name = v["name"].as_str().map(str::trim).filter(|s| !s.is_empty());
    let name_cn = v["name_cn"].as_str().map(str::trim).filter(|s| !s.is_empty());
    let mut localized = Map::new();
    if let Some(cn) = name_cn {
        localized.insert("zh-CN".into(), json!(cn));
    }
    if let Some(en) = info(v, "英文名")
        .map(info_values)
        .and_then(|e| e.into_iter().next())
    {
        localized.insert("en".into(), json!(en));
    }
    let mut aliases: Vec<String> = Vec::new();
    for a in ["别名", "英文名"]
        .iter()
        .filter_map(|k| info(v, k))
        .flat_map(info_values)
    {
        if Some(a.as_str()) != name && Some(a.as_str()) != name_cn && !aliases.contains(&a) {
            aliases.push(a);
        }
    }
    let date = v["date"].as_str().filter(|d| date_ok(d));
    let mut people = Vec::new();
    for (key, role) in [
        ("导演", "director"),
        ("原作", "writer"),
        ("作者", "writer"),
        ("动画制作", "developer"),
        ("开发", "developer"),
        ("艺术家", "artist"),
    ] {
        for n in names(info(v, key)) {
            people.push(json!({ "name": n, "role": role }));
        }
    }
    let episodes = v["total_episodes"]
        .as_i64()
        .filter(|n| *n > 0)
        .or_else(|| v["eps"].as_i64().filter(|n| *n > 0));
    let rating = &v["rating"];
    let air_end = ["播放结束", "放送结束"]
        .iter()
        .find_map(|k| info(v, k))
        .map(info_values)
        .and_then(|e| e.into_iter().next());
    json!({
        "kind": kind(v["type"].as_i64().unwrap_or(2)),
        "titles": { "original": name.or(name_cn), "localized": localized, "aliases": aliases },
        "year": date.and_then(|d| d[..4].parse::<i64>().ok()),
        "release_date": date,
        "ids": { "bangumi": id },
        "genres": v["tags"].as_array().map(|t| t.iter().take(10).filter_map(|x| x["name"].as_str()).collect::<Vec<_>>()).unwrap_or_default(),
        "countries": [],
        "languages": [],
        "people": people.into_iter().take(50).collect::<Vec<_>>(),
        "runtime_min": null,
        "episode_count": episodes,
        "ratings": rating["score"].as_f64().filter(|s| *s > 0.0)
            .map_or_else(|| json!({}), |s| json!({ "bangumi": { "value": s, "votes": rating["total"].as_i64().unwrap_or(0) } })),
        "images": {},
        "overview": v["summary"].as_str().map(|s| s.replace('\r', "").trim().chars().take(5000).collect::<String>()).filter(|s| !s.is_empty())
            .map_or_else(|| json!({}), |s| json!({ "zh-CN": s })),
        "extra": { "platform": v["platform"].as_str().filter(|p| !p.is_empty()), "nsfw": v["nsfw"].as_bool().unwrap_or(false),
                   "rank": rating["rank"].as_i64().filter(|r| *r > 0), "air_end": air_end },
        "attribution": { "source": "bangumi", "url": id.map(|i| format!("https://bgm.tv/subject/{i}")), "license_note": "数据来源：Bangumi 番组计划" },
    })
}

/// 搜索结果 / 关联条目的简要信息
pub fn brief(v: &Value) -> Value {
    let name = v["name"].as_str().unwrap_or("");
    let cn = v["name_cn"].as_str().filter(|s| !s.is_empty());
    json!({ "external_id": v["id"].as_u64().map(|i| i.to_string()), "title": cn.unwrap_or(name), "original_title": name,
            "year": v["date"].as_str().and_then(|d| d.get(..4)).and_then(|y| y.parse::<i64>().ok()),
            "kind": kind(v["type"].as_i64().unwrap_or(2)), "relation": v["relation"] })
}

/// 1970-01-01 起的天数（公历）
pub fn days(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// 连载中（动画 / 三次元：没有结束日期且首播在一年内或未来）的条目缓存较短
pub fn volatile(data: &Value, today: i64) -> bool {
    if !matches!(data["kind"].as_str(), Some("anime" | "tv")) || !data["extra"]["air_end"].is_null() {
        return false;
    }
    data["release_date"].as_str().is_none_or(|d| {
        let n = |r: std::ops::Range<usize>| d.get(r).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0);
        days(n(0..4), n(5..7), n(8..10)) > today - 365
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        assert_eq!(parse_id("245665"), Some(245_665));
        assert_eq!(parse_id("bangumi:245665"), Some(245_665));
        assert_eq!(parse_id("https://bgm.tv/subject/245665?x=1"), Some(245_665));
        assert_eq!(parse_id("https://bangumi.tv/subject/245665/"), Some(245_665));
        assert!(parse_id("bangumi:x").is_none());
        assert!(parse_id("0").is_none());
        assert!(parse_id("https://example.com/subject/1").is_none());
    }

    #[test]
    fn maps_subject() {
        let v = json!({ "id": 245_665, "type": 2, "name": "鬼滅の刃", "name_cn": "鬼灭之刃", "date": "2019-04-06", "platform": "TV",
            "summary": "大正时期。\r\n卖炭的少年……", "nsfw": false, "eps": 26, "total_episodes": 26,
            "rating": { "rank": 1566, "total": 22246, "score": 7.3 },
            "tags": [{ "name": "热血", "count": 1 }, { "name": "战斗", "count": 1 }],
            "infobox": [ { "key": "中文名", "value": "鬼灭之刃" },
                         { "key": "别名", "value": [{ "v": "Demon Slayer: Kimetsu no Yaiba" }, { "v": "鬼灭之刃" }] },
                         { "key": "导演", "value": "外崎春雄" }, { "key": "原作", "value": "吾峠呼世晴" },
                         { "key": "动画制作", "value": "ufotable" }, { "key": "播放结束", "value": "2019年9月28日" } ] });
        let d = map(&v);
        assert_eq!(d["kind"], "anime");
        assert_eq!(
            d["titles"],
            json!({ "original": "鬼滅の刃", "localized": { "zh-CN": "鬼灭之刃" }, "aliases": ["Demon Slayer: Kimetsu no Yaiba"] })
        );
        assert_eq!(
            (
                d["year"].clone(),
                d["release_date"].clone(),
                d["episode_count"].clone()
            ),
            (json!(2019), json!("2019-04-06"), json!(26))
        );
        assert_eq!(d["ids"]["bangumi"], 245_665);
        assert_eq!(d["genres"], json!(["热血", "战斗"]));
        assert_eq!(d["ratings"]["bangumi"], json!({ "value": 7.3, "votes": 22246 }));
        assert_eq!(d["overview"]["zh-CN"], "大正时期。\n卖炭的少年……");
        assert_eq!(d["people"][2], json!({ "name": "ufotable", "role": "developer" }));
        assert_eq!(d["extra"]["platform"], "TV");
        assert_eq!(d["attribution"]["url"], "https://bgm.tv/subject/245665");
        // 已完结
        assert!(!volatile(&d, days(2019, 5, 1)));
        let mut airing = d.clone();
        airing["extra"]["air_end"] = Value::Null;
        assert!(volatile(&airing, days(2019, 5, 1)));
        assert!(!volatile(&airing, days(2021, 5, 1)));
        // 书籍：没有中文名、没有评分
        let b = map(
            &json!({ "id": 206_382, "type": 1, "name": "ふたご最前線 4", "name_cn": "", "date": "2007-07-06",
                             "rating": { "score": 0, "total": 0 }, "infobox": [{ "key": "作者", "value": "甲、乙" }] }),
        );
        assert_eq!(
            (
                b["kind"].clone(),
                b["titles"]["localized"].clone(),
                b["ratings"].clone()
            ),
            (json!("book"), json!({}), json!({}))
        );
        assert_eq!(
            b["people"],
            json!([{ "name": "甲", "role": "writer" }, { "name": "乙", "role": "writer" }])
        );
        assert_eq!(
            brief(
                &json!({ "id": 1, "type": 2, "name": "a", "name_cn": "甲", "date": "2020-01-01", "relation": "续集" })
            ),
            json!({ "external_id": "1", "title": "甲", "original_title": "a", "year": 2020, "kind": "anime", "relation": "续集" })
        );
        assert_eq!(days(1970, 1, 1), 0);
        assert_eq!(days(2000, 3, 1), 11_017);
    }
}
