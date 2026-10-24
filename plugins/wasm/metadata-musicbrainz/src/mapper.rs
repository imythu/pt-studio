//! MusicBrainz → 统一元数据模型（metadata-providers-common §2、LLD-MDMB §1）。纯函数。

use pt_plugin_sdk::serde_json::{Map, Value, json};

/// 实体与 MBID
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Id {
    pub entity: &'static str,
    pub mbid: String,
}

impl Id {
    /// 缓存键 / 对外编号：`release-group:<mbid>`
    pub fn key(&self) -> String {
        format!("{}:{}", self.entity, self.mbid)
    }
}

fn uuid(s: &str) -> Option<String> {
    let s = s.trim().to_ascii_lowercase();
    let s = s.get(..36)?;
    let ok = s.char_indices().all(|(i, c)| {
        if matches!(i, 8 | 13 | 18 | 23) {
            c == '-'
        } else {
            c.is_ascii_hexdigit()
        }
    });
    ok.then(|| s.to_owned())
}

/// `release-group:<mbid>` / `release:<mbid>` / `artist:<mbid>`、`musicbrainz.org/<实体>/<mbid>` 链接；只有 MBID 时按发行组处理
pub fn parse_id(s: &str) -> Option<Id> {
    let s = s.trim();
    for entity in ["release-group", "release", "artist"] {
        let id = s
            .strip_prefix(&format!("{entity}:"))
            .or_else(|| s.split(&format!("musicbrainz.org/{entity}/")).nth(1))
            .and_then(uuid);
        if let Some(mbid) = id {
            return Some(Id { entity, mbid });
        }
    }
    (s.len() == 36).then(|| uuid(s)).flatten().map(|mbid| Id {
        entity: "release-group",
        mbid,
    })
}

/// Lucene 查询转义（LLD §2）
pub fn escape(q: &str) -> String {
    let mut out = String::with_capacity(q.len());
    for c in q.chars() {
        if "+-&|!(){}[]^\"~*?:\\/".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// 发行组类型 → type-music 的 `release_type`（次要类型优先）
pub fn release_type(primary: Option<&str>, secondary: &[&str]) -> &'static str {
    for s in secondary {
        let t = match *s {
            "Compilation" => "compilation",
            "Soundtrack" => "soundtrack",
            "Live" => "live",
            "Remix" => "remix",
            "Mixtape/Street" => "mixtape",
            "Interview" => "interview",
            _ => continue,
        };
        return t;
    }
    match primary {
        Some("Album") => "album",
        Some("Single") => "single",
        Some("EP") => "ep",
        _ => "other",
    }
}

/// 介质格式 → type-music 的 `media`
pub fn media(format: &str) -> &'static str {
    let f = format.to_ascii_lowercase();
    if f.contains("sacd") {
        "sacd"
    } else if f.contains("cd") {
        "cd"
    } else if f.contains("digital") {
        "web"
    } else if f.contains("vinyl") || f.contains("\"") {
        "vinyl"
    } else if f.contains("cassette") {
        "cassette"
    } else if f.contains("blu-ray") {
        "bluray"
    } else if f.contains("dvd") {
        "dvd"
    } else if f == "dat" {
        "dat"
    } else {
        "other"
    }
}

fn credits(v: &Value) -> (Vec<Value>, String) {
    let mut people = Vec::new();
    let mut s = String::new();
    for c in v["artist-credit"].as_array().into_iter().flatten() {
        let name = c["name"]
            .as_str()
            .or_else(|| c["artist"]["name"].as_str())
            .unwrap_or("");
        s.push_str(name);
        s.push_str(c["joinphrase"].as_str().unwrap_or(""));
        people.push(json!({ "name": name, "role": "artist", "mbid": c["artist"]["id"] }));
    }
    (people, s)
}

fn date_parts(d: Option<&str>) -> (Option<i64>, Option<String>) {
    let d = d.filter(|d| d.len() >= 4);
    let year = d.and_then(|d| d[..4].parse().ok());
    (year, d.filter(|d| d.len() == 10).map(str::to_owned))
}

fn genres(v: &Value) -> Vec<String> {
    let mut g: Vec<(i64, String)> = v["genres"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| Some((x["count"].as_i64().unwrap_or(0), x["name"].as_str()?.to_owned())))
        .collect();
    g.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    g.into_iter().take(10).map(|(_, n)| n).collect()
}

fn base(
    kind: &str,
    id: &Id,
    title: Option<&str>,
    people: Vec<Value>,
    date: Option<&str>,
    extra: Value,
) -> Value {
    let (year, release_date) = date_parts(date);
    json!({
        "kind": kind,
        "titles": { "original": title, "localized": {}, "aliases": [] },
        "year": year,
        "release_date": release_date,
        "ids": { "mbid": id.mbid },
        "genres": [],
        "countries": [],
        "languages": [],
        "people": people.into_iter().take(50).collect::<Vec<_>>(),
        "runtime_min": null,
        "episode_count": null,
        "ratings": {},
        "images": {},
        "overview": {},
        "extra": extra,
        "attribution": { "source": "musicbrainz", "url": format!("https://musicbrainz.org/{}/{}", id.entity, id.mbid),
                         "license_note": "Data from MusicBrainz (CC0)" },
    })
}

pub fn release_group(id: &Id, v: &Value) -> Value {
    let (people, credit) = credits(v);
    let secondary: Vec<&str> = v["secondary-types"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let mut d = base(
        "music_release_group",
        id,
        v["title"].as_str(),
        people,
        v["first-release-date"].as_str(),
        json!({ "artist_credit_string": credit, "release_type": release_type(v["primary-type"].as_str(), &secondary),
                "primary_type": v["primary-type"], "secondary_types": secondary, "disambiguation": v["disambiguation"].as_str().filter(|s| !s.is_empty()) }),
    );
    d["genres"] = json!(genres(v));
    d
}

pub fn release(id: &Id, v: &Value) -> Value {
    let (people, credit) = credits(v);
    let label = v["label-info"].as_array().and_then(|a| a.first());
    let media_list = v["media"].as_array().cloned().unwrap_or_default();
    let tracks: i64 = media_list.iter().filter_map(|m| m["track-count"].as_i64()).sum();
    let mut d = base(
        "music_release",
        id,
        v["title"].as_str(),
        people,
        v["date"].as_str(),
        json!({ "artist_credit_string": credit,
                "label": label.and_then(|l| l["label"]["name"].as_str()),
                "catalog": label.and_then(|l| l["catalog-number"].as_str()),
                "media": media_list.first().and_then(|m| m["format"].as_str()).map(media),
                "track_count": (tracks > 0).then_some(tracks),
                "barcode": v["barcode"].as_str().filter(|b| !b.is_empty()),
                "release_date": v["date"].as_str(),
                "release_group": v["release-group"]["id"] }),
    );
    if let Some(c) = v["country"].as_str() {
        d["countries"] = json!([c]);
    }
    d["genres"] = json!(genres(v));
    d
}

pub fn artist(id: &Id, v: &Value) -> Value {
    let name = v["name"].as_str();
    let mut aliases: Vec<&str> = Vec::new();
    for a in v["aliases"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["name"].as_str())
    {
        if Some(a) != name && !aliases.contains(&a) {
            aliases.push(a);
        }
    }
    let mut d = base(
        "artist",
        id,
        name,
        Vec::new(),
        v["life-span"]["begin"].as_str(),
        json!({ "artist_type": v["type"], "sort_name": v["sort-name"], "disambiguation": v["disambiguation"].as_str().filter(|s| !s.is_empty()) }),
    );
    d["titles"]["aliases"] = json!(aliases);
    let mut localized = Map::new();
    for a in v["aliases"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["primary"] == true)
    {
        if let (Some(l), Some(n)) = (a["locale"].as_str(), a["name"].as_str()) {
            localized.insert(
                if l == "zh_Hans" || l == "zh" {
                    "zh-CN".into()
                } else {
                    l.replace('_', "-")
                },
                json!(n),
            );
        }
    }
    d["titles"]["localized"] = Value::Object(localized);
    if let Some(c) = v["country"].as_str() {
        d["countries"] = json!([c]);
    }
    d["genres"] = json!(genres(v));
    d
}

/// 搜索结果（发行组）
pub fn brief(v: &Value) -> Value {
    let (_, credit) = credits(v);
    json!({ "external_id": v["id"].as_str().map(|m| format!("release-group:{m}")), "title": v["title"], "artist": credit,
            "year": v["first-release-date"].as_str().and_then(|d| d.get(..4)).and_then(|y| y.parse::<i64>().ok()),
            "kind": "music_release_group", "release_type": release_type(v["primary-type"].as_str(), &[]) })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RG: &str = "b1392450-e666-3926-a536-22c65f834433";

    #[test]
    fn ids_and_escape() {
        assert_eq!(
            parse_id(&format!("release-group:{RG}")).map(|i| i.key()),
            Some(format!("release-group:{RG}"))
        );
        assert_eq!(
            parse_id(&format!("https://musicbrainz.org/release/{}", RG.to_uppercase())).map(|i| i.key()),
            Some(format!("release:{RG}"))
        );
        assert_eq!(parse_id(RG).map(|i| i.entity), Some("release-group"));
        assert!(parse_id("release-group:xyz").is_none());
        assert!(parse_id("https://musicbrainz.org/recording/b1392450-e666-3926-a536-22c65f834433").is_none());
        assert_eq!(
            escape("AC/DC: Back (in) Black!"),
            "AC\\/DC\\: Back \\(in\\) Black\\!"
        );
    }

    #[test]
    fn maps_entities() {
        let id = Id {
            entity: "release-group",
            mbid: RG.into(),
        };
        let v = json!({ "title": "OK Computer", "first-release-date": "1997-05-21", "primary-type": "Album", "secondary-types": [],
            "artist-credit": [{ "name": "Radiohead", "joinphrase": " & ", "artist": { "id": "a74b1b7f-71a5-4011-9441-d0b5e4122711" } },
                              { "name": "Guest", "joinphrase": "", "artist": { "id": "x" } }],
            "genres": [{ "name": "rock", "count": 13 }, { "name": "alternative rock", "count": 27 }] });
        let d = release_group(&id, &v);
        assert_eq!(
            (d["kind"].clone(), d["year"].clone(), d["release_date"].clone()),
            (json!("music_release_group"), json!(1997), json!("1997-05-21"))
        );
        assert_eq!(d["genres"], json!(["alternative rock", "rock"]));
        assert_eq!(d["extra"]["artist_credit_string"], "Radiohead & Guest");
        assert_eq!(d["extra"]["release_type"], "album");
        assert_eq!(
            d["people"][0],
            json!({ "name": "Radiohead", "role": "artist", "mbid": "a74b1b7f-71a5-4011-9441-d0b5e4122711" })
        );
        assert_eq!(
            d["attribution"]["url"],
            format!("https://musicbrainz.org/release-group/{RG}")
        );
        assert_eq!(release_type(Some("Album"), &["Live", "Compilation"]), "live");
        assert_eq!(release_type(Some("Broadcast"), &["Audiobook"]), "other");

        let rid = Id {
            entity: "release",
            mbid: RG.into(),
        };
        let r = release(
            &rid,
            &json!({ "title": "OK Computer", "date": "1997", "country": "GB", "barcode": "724385522925",
            "label-info": [{ "catalog-number": "NODATA 02", "label": { "name": "Parlophone" } }],
            "media": [{ "format": "12\" Vinyl", "track-count": 6 }, { "format": "12\" Vinyl", "track-count": 6 }] }),
        );
        assert_eq!(
            (
                r["year"].clone(),
                r["release_date"].clone(),
                r["countries"].clone()
            ),
            (json!(1997), Value::Null, json!(["GB"]))
        );
        assert_eq!(
            (
                r["extra"]["label"].clone(),
                r["extra"]["catalog"].clone(),
                r["extra"]["media"].clone(),
                r["extra"]["track_count"].clone()
            ),
            (json!("Parlophone"), json!("NODATA 02"), json!("vinyl"), json!(12))
        );
        assert_eq!(
            (
                media("Digital Media"),
                media("Hybrid SACD"),
                media("Enhanced CD"),
                media("Blu-ray")
            ),
            ("web", "sacd", "cd", "bluray")
        );

        let aid = Id {
            entity: "artist",
            mbid: RG.into(),
        };
        let a = artist(
            &aid,
            &json!({ "name": "Radiohead", "type": "Group", "country": "GB", "life-span": { "begin": "1991" },
            "aliases": [{ "name": "电台司令", "locale": "zh_Hans", "primary": true }, { "name": "Radiohead", "primary": null }, { "name": "r/head" }] }),
        );
        assert_eq!(
            a["titles"],
            json!({ "original": "Radiohead", "localized": { "zh-CN": "电台司令" }, "aliases": ["电台司令", "r/head"] })
        );
        assert_eq!(
            (a["extra"]["artist_type"].clone(), a["year"].clone()),
            (json!("Group"), json!(1991))
        );
        assert_eq!(
            brief(
                &json!({ "id": RG, "title": "OK Computer", "first-release-date": "1997-05-21", "primary-type": "Album",
                           "artist-credit": [{ "name": "Radiohead" }] })
            ),
            json!({ "external_id": format!("release-group:{RG}"), "title": "OK Computer", "artist": "Radiohead", "year": 1997,
                    "kind": "music_release_group", "release_type": "album" })
        );
    }
}
