//! 统一元数据模型（metadata-providers-common §2）→ Group 字段补丁。纯函数。

use serde_json::{Map, Value, json};

/// 剧（`tv:<id>`）与季（`tv:<id>:s<n>`）的统一元数据模型 → Group 字段补丁（type-tv / type-anime；季的海报、简介、集数优先）
pub fn show_patch(show: &Value, season: Option<&Value>, locale: &str) -> Value {
    let mut p = Map::new();
    let first = |v: &Value| {
        v.as_object()
            .and_then(|o| o.values().next())
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    if let Some(v) = show["titles"]["original"].as_str() {
        p.insert("original_title".into(), json!(v));
    }
    // 无译名时 TMDB 返回原名，不当作中文名
    if let Some(v) = show["titles"]["localized"][locale]
        .as_str()
        .filter(|v| Some(*v) != show["titles"]["original"].as_str())
    {
        p.insert("title_zh".into(), json!(v));
    }
    let poster = season
        .and_then(|s| s["images"]["poster"].as_str())
        .or_else(|| show["images"]["poster"].as_str());
    if let Some(v) = poster {
        p.insert("poster".into(), json!(v));
    }
    let ov = season
        .and_then(|s| {
            s["overview"][locale]
                .as_str()
                .map(str::to_owned)
                .or_else(|| first(&s["overview"]))
        })
        .or_else(|| {
            show["overview"][locale]
                .as_str()
                .map(str::to_owned)
                .or_else(|| first(&show["overview"]))
        });
    if let Some(v) = ov {
        p.insert("overview".into(), json!(v));
    }
    for k in ["genres", "countries"] {
        if let Some(a) = show[k].as_array().filter(|a| !a.is_empty()) {
            p.insert(k.into(), json!(a));
        }
    }
    if let Some(n) = season
        .and_then(|s| s["episode_count"].as_i64())
        .or_else(|| show["episode_count"].as_i64())
    {
        p.insert("episode_count".into(), json!(n));
    }
    if let Some(st) = show["extra"]["status"].as_str() {
        p.insert("status".into(), json!(st));
    }
    if let Some(v) = show["ratings"]["tmdb"]["value"].as_f64() {
        p.insert("rating".into(), json!(v));
    }
    if let Some(v) = show["ids"]["tmdb"].as_u64() {
        p.insert("tmdb_tv_id".into(), json!(v));
    }
    if let Some(v) = show["ids"]["imdb"].as_str() {
        p.insert("imdb_id".into(), json!(v));
    }
    Value::Object(p)
}
