//! recommend 插件（Native）：首页智能推荐。
//!
//! 数据来自 torrent / accounting 的只读视图。三个栏目：
//! - `for_you`：按用户做种 / 下载过的资源学习偏好（类型、题材），给候选打分：类型亲和度 + 题材重合 + 新鲜度 +
//!   热度 + 评分 + 缺种加成；没有历史的新用户退化为热门 + 新鲜度。已拥有的资源不推荐。
//! - `hot`：近期热度（完成数、在线做种 / 下载人数、新鲜度）。
//! - `needs_seed`：做种人数很少且有人需要的资源（做种者的好去处）。
//!   首页下方的“最新上架”列表由站点自身提供，这里不重复。

use std::{collections::HashMap, sync::Arc};

use axum::{Json, Router, response::IntoResponse, routing::get};
use pt_plugin_api::{HttpRoutes, NativePlugin, PResult, RequestCtx};
use serde_json::{Value, json};
use sqlx::Row;

pub const MANIFEST: &str = include_str!("../plugin.toml");
pub const MIGRATIONS: &[(&str, &str)] = &[("0001_init.sql", include_str!("../migrations/0001_init.sql"))];

#[derive(Debug, Clone)]
struct Cand {
    id: i64,
    title: String,
    type_id: String,
    size: i64,
    created_at: chrono::DateTime<chrono::Utc>,
    fields: Value,
    seeders: i64,
    leechers: i64,
    completed: i64,
    mine: bool,
}

impl Cand {
    fn genres(&self) -> Vec<String> {
        self.fields["genres"]
            .as_array()
            .map(|a| a.iter().filter_map(|g| g.as_str().map(str::to_owned)).collect())
            .unwrap_or_default()
    }

    fn rating(&self) -> f64 {
        self.fields["rating"].as_f64().unwrap_or(0.0)
    }

    fn age_days(&self) -> f64 {
        (chrono::Utc::now() - self.created_at).num_minutes().max(0) as f64 / 1440.0
    }

    fn json(&self, reason: Option<String>) -> Value {
        let media = |k: &str| {
            self.fields[k]
                .as_str()
                .filter(|s| s.starts_with("/media/"))
                .map(str::to_owned)
        };
        let zh = ["title_zh", "title_cn"]
            .iter()
            .find_map(|k| self.fields[*k].as_str().filter(|s| !s.is_empty()))
            .map(str::to_owned);
        json!({
            "id": self.id, "title": self.title, "type_id": self.type_id, "size": self.size,
            "created_at": self.created_at.to_rfc3339(), "poster": media("poster"), "title_zh": zh,
            "rating": self.fields["rating"].as_f64(), "genres": self.genres(),
            "seeders": self.seeders, "leechers": self.leechers, "completed": self.completed,
            "mine": self.mine, "reason": reason,
        })
    }
}

/// 用户偏好：类型与题材的权重（来自做种 / 下载过的资源，完成过的权重更高）
#[derive(Default)]
struct Taste {
    types: HashMap<String, f64>,
    genres: HashMap<String, f64>,
    liked_title: Option<String>,
}

fn norm(m: &HashMap<String, f64>) -> HashMap<String, f64> {
    let max = m.values().copied().fold(0.0_f64, f64::max);
    if max <= 0.0 {
        return HashMap::new();
    }
    m.iter().map(|(k, v)| (k.clone(), v / max)).collect()
}

async fn taste(ctx: &RequestCtx, uid: i64) -> PResult<Taste> {
    let mut c = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT t.type_id, t.title, coalesce(g.fields, '{}'::jsonb) AS gf, (s.completed_at IS NOT NULL) AS done
           FROM accounting.v_snatch s
           JOIN torrent.v_torrent_public t ON t.id = s.torrent_id
           LEFT JOIN torrent.v_group_fields g ON g.group_id = t.group_id
          WHERE s.user_id = $1 ORDER BY s.last_action_at DESC LIMIT 200",
    )
    .bind(uid)
    .fetch_all(&mut *c)
    .await?;
    let mut t = Taste::default();
    for r in &rows {
        let w = if r.get::<bool, _>("done") { 2.0 } else { 1.0 };
        *t.types.entry(r.get::<String, _>("type_id")).or_default() += w;
        let gf: Value = r.get("gf");
        if let Some(a) = gf["genres"].as_array() {
            for g in a.iter().filter_map(Value::as_str) {
                *t.genres.entry(g.to_owned()).or_default() += w;
            }
        }
        if t.liked_title.is_none() {
            t.liked_title = Some(
                gf["title_zh"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map_or_else(|| r.get::<String, _>("title"), str::to_owned),
            );
        }
    }
    Ok(t)
}

async fn candidates(ctx: &RequestCtx, uid: i64, days: i64) -> PResult<Vec<Cand>> {
    let mut c = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT t.id, t.title, t.type_id, t.size, t.created_at,
                coalesce(g.fields, '{}'::jsonb) || coalesce(f.fields, '{}'::jsonb) AS fields,
                coalesce(p.seeders, 0) AS seeders, coalesce(p.leechers, 0) AS leechers, coalesce(p.completed, 0) AS completed,
                EXISTS (SELECT 1 FROM accounting.v_snatch s WHERE s.user_id = $1 AND s.torrent_id = t.id) AS mine
           FROM torrent.v_torrent_public t
           JOIN torrent.v_torrent_fields f ON f.id = t.id
           LEFT JOIN torrent.v_group_fields g ON g.group_id = t.group_id
           LEFT JOIN accounting.v_torrent_peers p ON p.torrent_id = t.id
          WHERE t.status = 'approved' AND t.created_at > now() - make_interval(days => $2::int)
          ORDER BY t.created_at DESC LIMIT 400",
    )
    .bind(uid)
    .bind(i32::try_from(days).unwrap_or(60))
    .fetch_all(&mut *c)
    .await?;
    Ok(rows
        .iter()
        .map(|r| Cand {
            id: r.get("id"),
            title: r.get("title"),
            type_id: r.get("type_id"),
            size: r.get("size"),
            created_at: r.get("created_at"),
            fields: r.get("fields"),
            seeders: r.get("seeders"),
            leechers: r.get("leechers"),
            completed: r.get("completed"),
            mine: r.get("mine"),
        })
        .collect())
}

fn popularity(c: &Cand) -> f64 {
    ((c.completed as f64) + 1.0).ln()
        + 0.6 * ((c.leechers as f64) + 1.0).ln()
        + 0.3 * ((c.seeders as f64) + 1.0).ln()
}

fn freshness(c: &Cand) -> f64 {
    // 半衰期约 10 天
    (-c.age_days() / 14.4).exp()
}

fn type_label(t: &str) -> &str {
    match t {
        "movie" => "电影",
        "tv" => "剧集",
        "anime" => "动漫",
        "music" => "音乐",
        _ => t,
    }
}

fn score_for_you(
    c: &Cand,
    taste: &Taste,
    types: &HashMap<String, f64>,
    genres: &HashMap<String, f64>,
) -> (f64, Option<String>) {
    let ta = types.get(&c.type_id).copied().unwrap_or(0.0);
    let gs = c.genres();
    let (best_g, ga) = gs
        .iter()
        .filter_map(|g| genres.get(g).map(|w| (g.clone(), *w)))
        .fold(
            (None, 0.0),
            |(bg, bw), (g, w)| if w > bw { (Some(g), w) } else { (bg, bw) },
        );
    let overlap = if gs.is_empty() {
        0.0
    } else {
        gs.iter().filter_map(|g| genres.get(g)).sum::<f64>() / gs.len() as f64
    };
    let need = if c.seeders <= 1 { 0.4 } else { 0.0 };
    let s = 3.0 * ta + 2.5 * overlap + 1.5 * freshness(c) + 0.8 * popularity(c) + 0.15 * c.rating() + need;
    let reason = if let Some(g) = best_g.filter(|_| ga > 0.4) {
        Some(format!("你常看「{g}」"))
    } else if ta > 0.4 {
        Some(match &taste.liked_title {
            Some(t) if taste.types.len() == 1 => format!(
                "喜欢{}的你，可能也想看（最近在看《{}》）",
                type_label(&c.type_id),
                t
            ),
            _ => format!("你常看{}", type_label(&c.type_id)),
        })
    } else if c.age_days() < 3.0 {
        Some("刚刚上架".into())
    } else if c.completed >= 3 {
        Some("大家都在下".into())
    } else {
        None
    };
    (s, reason)
}

async fn home(ctx: RequestCtx) -> PResult<axum::response::Response> {
    let uid = i64::try_from(ctx.require_user()?)
        .map_err(|_| pt_plugin_api::PluginError::bad_request("bad user id"))?;
    let settings = ctx.config.get_settings().await;
    if settings["enabled"].as_bool() == Some(false) {
        return Ok(Json(json!({ "sections": [] })).into_response());
    }
    let n = settings["per_section"].as_i64().unwrap_or(8).clamp(3, 24) as usize;
    let days = settings["window_days"].as_i64().unwrap_or(60).clamp(1, 365);
    let taste = taste(&ctx, uid).await?;
    let all = candidates(&ctx, uid, days).await?;
    let (types, genres) = (norm(&taste.types), norm(&taste.genres));
    let cold = taste.types.is_empty();

    let mut sections = Vec::new();

    // 为你推荐：不含已经做种 / 下载过的
    let mut fy: Vec<(f64, Option<String>, &Cand)> = all
        .iter()
        .filter(|c| !c.mine)
        .map(|c| {
            let (s, r) = score_for_you(c, &taste, &types, &genres);
            (s, r, c)
        })
        .collect();
    fy.sort_by(|a, b| b.0.total_cmp(&a.0));
    if !fy.is_empty() {
        sections.push(json!({
            "id": "for_you", "personalised": !cold,
            "items": fy.iter().take(n).map(|(_, r, c)| c.json(r.clone())).collect::<Vec<_>>(),
        }));
    }

    // 近期热门
    let mut hot: Vec<&Cand> = all.iter().collect();
    hot.sort_by(|a, b| (popularity(b) + freshness(b)).total_cmp(&(popularity(a) + freshness(a))));
    sections.push(json!({
        "id": "hot", "items": hot.iter().take(n).map(|c| c.json(None)).collect::<Vec<_>>(),
    }));

    // 需要做种：做种者少、有人下或完成过，体积适中的优先
    let mut need: Vec<&Cand> = all
        .iter()
        .filter(|c| c.seeders <= 2 && c.age_days() > 0.0)
        .collect();
    need.sort_by(|a, b| {
        let k = |c: &Cand| (c.leechers as f64 * 2.0 + c.completed as f64 + 1.0) / (c.seeders as f64 + 1.0);
        k(b).total_cmp(&k(a))
    });
    if !need.is_empty() {
        sections.push(json!({
            "id": "needs_seed",
            "items": need.iter().take(n).map(|c| c.json(Some(format!("仅 {} 人做种", c.seeders)))).collect::<Vec<_>>(),
        }));
    }

    sections.retain(|s| s["items"].as_array().is_some_and(|a| !a.is_empty()));
    Ok(Json(json!({ "sections": sections })).into_response())
}

struct Routes;

impl HttpRoutes for Routes {
    fn router(&self) -> Router {
        Router::new().route("/v1/home", get(home))
    }
}

pub fn plugin() -> NativePlugin {
    NativePlugin {
        manifest: MANIFEST,
        migrations: MIGRATIONS,
        http: Some(Arc::new(Routes)),
        ..Default::default()
    }
}
