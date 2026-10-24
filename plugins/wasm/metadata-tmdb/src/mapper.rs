//! TMDB 响应 → 统一元数据模型（metadata-providers-common §2、metadata-tmdb LLD §1.2）。纯函数。

use serde_json::{Map, Value, json};

pub const LICENSE_NOTE: &str = "This product uses the TMDB API but is not endorsed or certified by TMDB.";

/// 规范 ID：`movie:603`、`tv:1399`、`tv:1399:s1`；`imdb:tt…` 需要先经 /find 换算
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Id {
    Movie(u64),
    Tv(u64, Option<u32>),
    Imdb(String),
}

pub fn parse_id(s: &str) -> Option<Id> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("movie:") {
        return rest.parse().ok().map(Id::Movie);
    }
    if let Some(rest) = s.strip_prefix("tv:") {
        let (id, season) = match rest.split_once(":s") {
            Some((i, n)) => (i, Some(n.parse().ok()?)),
            None => (rest, None),
        };
        return id.parse().ok().map(|i| Id::Tv(i, season));
    }
    if let Some(rest) = s.strip_prefix("imdb:") {
        let ok = rest.len() >= 9 && rest.starts_with("tt") && rest[2..].bytes().all(|b| b.is_ascii_digit());
        return ok.then(|| Id::Imdb(rest.to_owned()));
    }
    None
}

impl Id {
    pub fn key(&self) -> String {
        match self {
            Id::Movie(i) => format!("movie:{i}"),
            Id::Tv(i, None) => format!("tv:{i}"),
            Id::Tv(i, Some(s)) => format!("tv:{i}:s{s}"),
            Id::Imdb(t) => format!("imdb:{t}"),
        }
    }
}

fn genre(id: i64) -> Option<&'static str> {
    Some(match id {
        28 => "action",
        12 => "adventure",
        16 => "animation",
        35 => "comedy",
        80 => "crime",
        99 => "documentary",
        18 => "drama",
        10751 => "family",
        14 => "fantasy",
        36 => "history",
        27 => "horror",
        10402 => "music",
        9648 => "mystery",
        10749 => "romance",
        878 => "scifi",
        10770 => "tv_movie",
        53 => "thriller",
        10752 => "war",
        37 => "western",
        10759 => "action_adventure",
        10762 => "kids",
        10763 => "news",
        10764 => "reality",
        10765 => "scifi_fantasy",
        10766 => "soap",
        10767 => "talk",
        10768 => "war_politics",
        _ => return None,
    })
}

/// 保留的翻译标题（站点界面语言与常用中文变体）
const TITLE_LOCALES: [&str; 5] = ["zh-CN", "zh-TW", "zh-HK", "en-US", "ja-JP"];

/// 缓存时长：近 90 天上映 / 首播或仍在连载的剧集变化快，用短 TTL（LLD §1.4）
/// `today` 为 [`day_of`] 的结果
pub fn volatile(data: &Value, today: i64) -> bool {
    if data["extra"]["status"] == "returning" {
        return true;
    }
    data["release_date"]
        .as_str()
        .and_then(days)
        .is_some_and(|d| (today - 90..=today + 365).contains(&d))
}

/// Unix 毫秒 → 与 [`days`] 同一坐标的天数
pub fn day_of(ms: i64) -> i64 {
    days("1970-01-01").unwrap_or(0) + ms.div_euclid(86_400_000)
}

/// `YYYY-MM-DD` → 自 0000-03-01 起的天数（只用于比较远近）
fn days(d: &str) -> Option<i64> {
    let mut it = d.get(..10)?.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, dd) = (it.next()??, it.next()??, it.next()??);
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    Some(365 * y + y / 4 - y / 100 + y / 400 + (153 * m + 2) / 5 + dd - 1)
}

fn s(v: &Value) -> Option<String> {
    v.as_str().filter(|x| !x.is_empty()).map(str::to_owned)
}

/// `kind` 为 movie / tv；`poster` / `backdrop` 为转存后的路径（由调用方填）
pub fn map(kind: &str, r: &Value, locale: &str) -> Value {
    let movie = kind == "movie";
    let title = s(&r[if movie { "title" } else { "name" }]);
    let original = s(&r[if movie { "original_title" } else { "original_name" }]);
    // 季（/tv/{id}/season/{n}）用 air_date
    let date = s(&r[if movie { "release_date" } else { "first_air_date" }]).or_else(|| s(&r["air_date"]));
    let year = date
        .as_deref()
        .and_then(|d| d.get(..4))
        .and_then(|y| y.parse::<i64>().ok());
    let mut localized = Map::new();
    // `append_to_response=translations`：其他语言的标题；当前语言以主体字段为准
    for t in r["translations"]["translations"].as_array().into_iter().flatten() {
        let (Some(lang), Some(region)) = (t["iso_639_1"].as_str(), t["iso_3166_1"].as_str()) else {
            continue;
        };
        let tag = format!("{lang}-{region}");
        if let (true, Some(v)) = (
            TITLE_LOCALES.contains(&tag.as_str()),
            s(&t["data"][if movie { "title" } else { "name" }]),
        ) {
            localized.insert(tag, json!(v));
        }
    }
    if let Some(t) = &title {
        localized.insert(locale.into(), json!(t));
    }
    let mut ids = Map::new();
    if let Some(id) = r["id"].as_u64() {
        ids.insert("tmdb".into(), json!(id));
    }
    if let Some(i) = s(&r["external_ids"]["imdb_id"]).or_else(|| s(&r["imdb_id"])) {
        ids.insert("imdb".into(), json!(i));
    }
    if let Some(t) = r["external_ids"]["tvdb_id"].as_u64() {
        ids.insert("tvdb".into(), json!(t));
    }
    let genres: Vec<&str> = r["genres"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|g| g["id"].as_i64().and_then(genre))
                .collect()
        })
        .unwrap_or_default();
    let countries: Vec<String> = if movie {
        r["production_countries"]
            .as_array()
            .map(|a| a.iter().filter_map(|c| s(&c["iso_3166_1"])).collect())
            .unwrap_or_default()
    } else {
        r["origin_country"]
            .as_array()
            .map(|a| a.iter().filter_map(s).collect())
            .unwrap_or_default()
    };
    let mut languages: Vec<String> = r["spoken_languages"]
        .as_array()
        .map(|a| a.iter().filter_map(|l| s(&l["iso_639_1"])).collect())
        .unwrap_or_default();
    if let Some(o) = s(&r["original_language"])
        && !languages.contains(&o)
    {
        languages.insert(0, o);
    }
    let mut people = Vec::new();
    for c in r["credits"]["crew"].as_array().into_iter().flatten() {
        let role = match c["job"].as_str() {
            Some("Director") => "director",
            Some("Screenplay" | "Writer") => "writer",
            _ => continue,
        };
        if let Some(n) = s(&c["name"]) {
            people.push(json!({ "name": n, "role": role }));
        }
    }
    for c in r["credits"]["cast"].as_array().into_iter().flatten().take(20) {
        if let Some(n) = s(&c["name"]) {
            people.push(json!({ "name": n, "role": "cast" }));
        }
    }
    people.truncate(50);
    let runtime = r["runtime"]
        .as_i64()
        .or_else(|| r["episode_run_time"][0].as_i64());
    let mut overview = Map::new();
    if let Some(o) = s(&r["overview"]) {
        overview.insert(locale.into(), json!(o.chars().take(5000).collect::<String>()));
    }
    let id = r["id"].as_u64().unwrap_or(0);
    let mut extra = Map::new();
    if !movie {
        let st = match r["status"].as_str() {
            Some("Returning Series") => Some("returning"),
            Some("Ended") => Some("ended"),
            Some("Canceled") => Some("canceled"),
            _ => None,
        };
        if let Some(st) = st {
            extra.insert("status".into(), json!(st));
        }
    }
    json!({
        "kind": kind,
        "titles": { "original": original, "localized": localized, "aliases": [] },
        "year": year, "release_date": date, "ids": ids, "genres": genres, "countries": countries, "languages": languages,
        "people": people, "runtime_min": runtime, "episode_count": r["number_of_episodes"].as_i64().or_else(|| r["episodes"].as_array().map(|a| a.len() as i64)),
        "season": r["season_number"].as_i64(),
        "ratings": r["vote_average"].as_f64().map_or(json!({}), |v| json!({ "tmdb": { "value": v, "votes": r["vote_count"].as_i64().unwrap_or(0) } })),
        "images": {}, "overview": overview, "extra": extra,
        "attribution": { "source": "tmdb", "url": format!("https://www.themoviedb.org/{kind}/{id}"), "license_note": LICENSE_NOTE },
    })
}

/// 搜索结果的简要形式
pub fn brief(kind: &str, r: &Value) -> Value {
    let movie = kind == "movie";
    let date = s(&r[if movie { "release_date" } else { "first_air_date" }]);
    json!({
        "external_id": format!("{kind}:{}", r["id"].as_u64().unwrap_or(0)),
        "title": s(&r[if movie { "title" } else { "name" }]),
        "original": s(&r[if movie { "original_title" } else { "original_name" }]),
        "year": date.as_deref().and_then(|d| d.get(..4)).and_then(|y| y.parse::<i64>().ok()),
        "poster_path": s(&r["poster_path"]),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids() {
        assert_eq!(parse_id("movie:603"), Some(Id::Movie(603)));
        assert_eq!(parse_id("tv:1399:s2"), Some(Id::Tv(1399, Some(2))));
        assert_eq!(parse_id("imdb:tt0133093"), Some(Id::Imdb("tt0133093".into())));
        assert_eq!(parse_id("imdb:nope"), None);
        assert_eq!(parse_id("603"), None);
        assert_eq!(Id::Tv(1, Some(3)).key(), "tv:1:s3");
    }

    #[test]
    fn maps_movie() {
        let r = json!({
            "id": 603, "title": "黑客帝国", "original_title": "The Matrix", "release_date": "1999-03-30", "imdb_id": "tt0133093",
            "genres": [{ "id": 28 }, { "id": 878 }, { "id": 99999 }], "production_countries": [{ "iso_3166_1": "US" }],
            "spoken_languages": [{ "iso_639_1": "en" }], "original_language": "en", "runtime": 136, "vote_average": 8.2, "vote_count": 25000,
            "overview": "…", "credits": { "crew": [{ "job": "Director", "name": "Lana Wachowski" }, { "job": "Grip", "name": "X" }], "cast": [{ "name": "Keanu Reeves" }] },
        });
        let m = map("movie", &r, "zh-CN");
        assert_eq!(m["titles"]["original"], "The Matrix");
        assert_eq!(m["titles"]["localized"]["zh-CN"], "黑客帝国");
        assert_eq!(m["year"], 1999);
        assert_eq!(m["ids"], json!({ "tmdb": 603, "imdb": "tt0133093" }));
        assert_eq!(m["genres"], json!(["action", "scifi"]));
        assert_eq!(m["languages"], json!(["en"]));
        assert_eq!(
            m["people"],
            json!([{ "name": "Lana Wachowski", "role": "director" }, { "name": "Keanu Reeves", "role": "cast" }])
        );
        assert_eq!(m["ratings"]["tmdb"]["value"], 8.2);
        assert_eq!(m["attribution"]["url"], "https://www.themoviedb.org/movie/603");
    }

    #[test]
    fn maps_tv() {
        let r = json!({ "id": 1399, "name": "权力的游戏", "original_name": "Game of Thrones", "first_air_date": "2011-04-17",
                        "origin_country": ["US"], "status": "Ended", "number_of_episodes": 73, "episode_run_time": [60],
                        "external_ids": { "imdb_id": "tt0944947", "tvdb_id": 121361 } });
        let m = map("tv", &r, "zh-CN");
        assert_eq!(
            (
                m["year"].as_i64(),
                m["runtime_min"].as_i64(),
                m["episode_count"].as_i64()
            ),
            (Some(2011), Some(60), Some(73))
        );
        assert_eq!(m["extra"]["status"], "ended");
        assert_eq!(m["ids"]["tvdb"], 121361);
    }

    #[test]
    fn translated_titles() {
        let r = json!({ "id": 1, "title": "黑客帝国", "translations": { "translations": [
            { "iso_639_1": "zh", "iso_3166_1": "TW", "data": { "title": "駭客任務" } },
            { "iso_639_1": "zh", "iso_3166_1": "CN", "data": { "title": "旧译名" } },
            { "iso_639_1": "en", "iso_3166_1": "US", "data": { "title": "The Matrix" } },
            { "iso_639_1": "fr", "iso_3166_1": "FR", "data": { "title": "Matrix" } },
            { "iso_639_1": "ja", "iso_3166_1": "JP", "data": { "title": "" } },
        ] } });
        let m = map("movie", &r, "zh-CN");
        assert_eq!(
            m["titles"]["localized"],
            json!({ "zh-CN": "黑客帝国", "zh-TW": "駭客任務", "en-US": "The Matrix" })
        );
    }

    #[test]
    fn volatility() {
        // 2026-10-24T00:00:00Z
        let today = day_of(1_792_800_000_000);
        assert_eq!(Some(today), days("2026-10-24"));
        let ended = json!({ "release_date": "1999-03-30", "extra": {} });
        assert!(!volatile(&ended, today));
        assert!(volatile(
            &json!({ "release_date": "2026-08-01", "extra": {} }),
            today
        ));
        assert!(volatile(
            &json!({ "release_date": "2027-03-01", "extra": {} }),
            today
        ));
        assert!(!volatile(
            &json!({ "release_date": "2026-07-01", "extra": {} }),
            today
        ));
        assert!(volatile(
            &json!({ "release_date": "2011-04-17", "extra": { "status": "returning" } }),
            today
        ));
        assert!(!volatile(&json!({ "release_date": null, "extra": {} }), today));
    }
}
