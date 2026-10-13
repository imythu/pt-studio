//! 订阅定义与查询（LLD §1.2）。本版本直接查询 torrent / accounting 的只读视图（只含已发布的种子），而不经 `ctx.search`：
//! 订阅需要稳定的“最新若干条”，不需要相关度排序；字段条件按种子字段与分组字段合并后的 JSON 比较。

use pt_plugin_api::{Ctx, PResult, PluginError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Postgres, QueryBuilder, Row};

use crate::xml::{Item, Promo};

pub const MAX_LIMIT: i64 = 100;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub field: String,
    pub op: String,
    pub value: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Definition {
    pub type_ids: Vec<String>,
    pub q: String,
    pub filters: Vec<Filter>,
    /// 促销种类（任一匹配即可），如 `free`、`double_up`
    pub promotion: Vec<String>,
    pub min_size: i64,
    /// 0 = 不限
    pub max_size: i64,
    pub exclude: Vec<String>,
}

fn ident_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn invalid(msg: impl Into<String>) -> PluginError {
    PluginError::new("rss.filter_invalid", msg).with_status(422)
}

impl Definition {
    /// 校验并规整（去掉空词、截断长度）
    pub fn normalize(mut self) -> PResult<Self> {
        let words = |v: Vec<String>| -> Vec<String> {
            v.into_iter()
                .map(|w| w.trim().chars().take(64).collect::<String>())
                .filter(|w| !w.is_empty())
                .take(20)
                .collect()
        };
        self.q = self.q.trim().chars().take(200).collect();
        self.exclude = words(self.exclude);
        self.promotion = words(self.promotion);
        self.type_ids = words(self.type_ids);
        if self.type_ids.iter().chain(&self.promotion).any(|t| !ident_ok(t)) {
            return Err(invalid("type_ids / promotion must be identifiers"));
        }
        if self.filters.len() > 10 {
            return Err(invalid("at most 10 filters"));
        }
        for f in &self.filters {
            if !ident_ok(&f.field) {
                return Err(invalid(format!("bad filter field `{}`", f.field)));
            }
            let ok = match f.op.as_str() {
                "eq" | "has" => !f.value.is_null() && !f.value.is_array() && !f.value.is_object(),
                "in" => f.value.as_array().is_some_and(|a| !a.is_empty() && a.len() <= 50),
                "gte" | "lte" => f.value.is_number(),
                _ => false,
            };
            if !ok {
                return Err(invalid(format!("bad filter `{}` `{}`", f.field, f.op)));
            }
        }
        if self.min_size < 0 || self.max_size < 0 || (self.max_size > 0 && self.max_size < self.min_size) {
            return Err(invalid("bad size range"));
        }
        Ok(self)
    }
}

fn like(w: &str) -> String {
    let e = w.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    format!("%{e}%")
}

/// 取最新的已发布种子（`offset` 供 Torznab 分页）
pub async fn fetch(ctx: &Ctx, d: &Definition, limit: i64, offset: i64) -> PResult<Vec<Item>> {
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
        "SELECT t.id, t.title, t.size, t.created_at, t.type_id, coalesce(g.fields, '{}'::jsonb) || coalesce(f.fields, '{}'::jsonb),
                encode(k.info_hash_v1, 'hex'), coalesce(p.seeders, 0), coalesce(p.leechers, 0), coalesce(p.completed, 0)
           FROM torrent.v_torrent_public t
           JOIN torrent.v_torrent_fields f ON f.id = t.id
           LEFT JOIN torrent.v_group_fields g ON g.group_id = t.group_id
           LEFT JOIN torrent.v_torrent_tracker k ON k.id = t.id
           LEFT JOIN accounting.v_torrent_peers p ON p.torrent_id = t.id
          WHERE t.status = 'approved'",
    );
    if !d.type_ids.is_empty() {
        qb.push(" AND t.type_id = ANY(")
            .push_bind(d.type_ids.clone())
            .push(")");
    }
    for w in d.q.split_whitespace().take(10) {
        qb.push(" AND t.title ILIKE ").push_bind(like(w));
    }
    for w in &d.exclude {
        qb.push(" AND t.title NOT ILIKE ").push_bind(like(w));
    }
    if d.min_size > 0 {
        qb.push(" AND t.size >= ").push_bind(d.min_size);
    }
    if d.max_size > 0 {
        qb.push(" AND t.size <= ").push_bind(d.max_size);
    }
    let merged = "(coalesce(g.fields, '{}'::jsonb) || coalesce(f.fields, '{}'::jsonb))";
    for f in &d.filters {
        qb.push(format!(" AND {merged} -> "));
        qb.push_bind(f.field.clone());
        match f.op.as_str() {
            "eq" => {
                qb.push(" = ").push_bind(f.value.clone()).push("::jsonb");
            }
            // 数组字段（如 hdr、audio_codecs）包含该值
            "has" => {
                qb.push(" @> ")
                    .push_bind(Value::Array(vec![f.value.clone()]))
                    .push("::jsonb");
            }
            "in" => {
                qb.push(" <@ ").push_bind(f.value.clone()).push("::jsonb");
            }
            op => {
                // 只比较数值：先确认是 number 再转换
                qb.push(format!(" IS NOT NULL AND CASE WHEN jsonb_typeof({merged} -> "));
                qb.push_bind(f.field.clone());
                qb.push(format!(") = 'number' THEN ({merged} ->> "));
                qb.push_bind(f.field.clone());
                qb.push(if op == "gte" {
                    ")::numeric >= "
                } else {
                    ")::numeric <= "
                });
                qb.push_bind(f.value.to_string()).push("::numeric ELSE false END");
            }
        }
    }
    qb.push(" ORDER BY t.created_at DESC, t.id DESC LIMIT ")
        .push_bind(limit.clamp(1, 500))
        .push(" OFFSET ")
        .push_bind(offset.clamp(0, 10_000));
    let mut c = ctx.sql.conn().await?;
    let rows = qb.build().fetch_all(&mut *c).await?;
    drop(c);
    let mut items: Vec<Item> = rows
        .iter()
        .map(|r| Item {
            id: r.get(0),
            title: r.get(1),
            size: r.get(2),
            created_at: r.get(3),
            type_id: r.get(4),
            fields: r.get(5),
            info_hash: r.get(6),
            seeders: r.get(7),
            leechers: r.get(8),
            completed: r.get(9),
            promotion: None,
        })
        .collect();
    attach_promotions(ctx, &mut items).await;
    Ok(items)
}

/// 促销状态（ctx.promotions 可选；不可用时视为无促销）
async fn attach_promotions(ctx: &Ctx, items: &mut [Item]) {
    if items.is_empty() {
        return;
    }
    let ids: Vec<i64> = items.iter().map(|i| i.id).collect();
    let r = match ctx
        .call_json(
            "ctx.promotions",
            "current_many",
            serde_json::json!({ "ids": ids }),
        )
        .await
    {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!(error = %e.message, "promotions unavailable");
            return;
        }
    };
    for p in r.as_array().into_iter().flatten() {
        let Some(it) = items.iter_mut().find(|i| Some(i.id) == p["torrent_id"].as_i64()) else {
            continue;
        };
        let mut kinds: Vec<String> = p["global"]
            .as_array()
            .into_iter()
            .flatten()
            .chain(p.get("primary").filter(|v| !v.is_null()))
            .filter_map(|x| x["kind"].as_str().map(str::to_owned))
            .collect();
        kinds.dedup();
        it.promotion = Some(Promo {
            kinds,
            up_permille: p["combined_up_permille"].as_i64().unwrap_or(1000),
            down_permille: p["combined_down_permille"].as_i64().unwrap_or(1000),
        });
    }
}

/// 促销条件：任一促销种类匹配
pub fn promotion_ok(d: &Definition, it: &Item) -> bool {
    d.promotion.is_empty()
        || it
            .promotion
            .as_ref()
            .is_some_and(|p| p.kinds.iter().any(|k| d.promotion.contains(k)))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use serde_json::json;

    fn def(v: Value) -> PResult<Definition> {
        serde_json::from_value::<Definition>(v)
            .map_err(|e| invalid(e.to_string()))?
            .normalize()
    }

    #[test]
    fn validation() {
        assert!(def(json!({ "type_ids": ["movie"], "filters": [{ "field": "resolution", "op": "eq", "value": "2160p" }] })).is_ok());
        assert!(def(json!({ "filters": [{ "field": "Bad Field", "op": "eq", "value": 1 }] })).is_err());
        assert!(def(json!({ "filters": [{ "field": "year", "op": "gte", "value": "x" }] })).is_err());
        assert!(def(json!({ "filters": [{ "field": "year", "op": "in", "value": [] }] })).is_err());
        assert!(def(json!({ "filters": [{ "field": "year", "op": "regex", "value": 1 }] })).is_err());
        assert!(def(json!({ "min_size": 10, "max_size": 5 })).is_err());
        assert!(def(json!({ "unknown": 1 })).is_err());
        let d = def(json!({ "exclude": [" CAM ", "", "TS"], "q": "  x  " })).unwrap();
        assert_eq!(
            (d.exclude, d.q.as_str()),
            (vec!["CAM".to_owned(), "TS".to_owned()], "x")
        );
    }

    #[test]
    fn like_escape() {
        assert_eq!(like("50%_a"), "%50\\%\\_a%");
    }

    #[test]
    fn promotion_filter() {
        let d = Definition {
            promotion: vec!["free".into()],
            ..Default::default()
        };
        let mut it = Item::default();
        assert!(!promotion_ok(&d, &it));
        it.promotion = Some(Promo {
            kinds: vec!["free".into()],
            up_permille: 1000,
            down_permille: 0,
        });
        assert!(promotion_ok(&d, &it));
        assert!(promotion_ok(&Definition::default(), &Item::default()));
    }
}
