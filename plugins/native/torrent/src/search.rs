//! 搜索贡献：torrent 是 `torrents` 索引的拥有者（search-meilisearch LLD §1.6.2）。
//!
//! 可见性映射：approved → member；candidate / pending → staff（上传者本人可见，匿名发布不写 `vis_owner_id`）；
//! rejected / disabled / deleted → 不进索引（返回 none，搜索插件转为删除）。

use async_trait::async_trait;
use pt_plugin_api::{Ctx, FieldSpec, IndexSchema, Json, PResult, SearchContributor, json};
use sqlx::Row;

pub struct Contributor;

fn field(name: &str, ty: &str, searchable: bool, filterable: bool, sortable: bool, rank: u8) -> FieldSpec {
    FieldSpec {
        name: name.into(),
        field_type: ty.into(),
        searchable,
        filterable,
        sortable,
        rank,
        exact: false,
        label_term: if filterable {
            format!("torrent.field.{name}")
        } else {
            String::new()
        },
    }
}

#[async_trait]
impl SearchContributor for Contributor {
    async fn index_schemas(&self, _ctx: &Ctx) -> Vec<IndexSchema> {
        vec![IndexSchema {
            index: "torrents".into(),
            fields: vec![
                field("title", "string", true, false, false, 0),
                field("subtitle", "string", true, false, false, 1),
                field("group_id", "int", false, true, false, 0),
                field("type_id", "string", false, true, false, 0),
                field("category_id", "int", false, true, false, 0),
                field("size", "int", false, true, true, 0),
                field("file_count", "int", false, true, true, 0),
                field("uploader_id", "int", false, true, false, 0),
                field("status", "string", false, true, false, 0),
            ],
            synonyms: vec![],
        }]
    }

    async fn to_documents(
        &self,
        ctx: &Ctx,
        index: &str,
        entity_ids: &[u64],
    ) -> PResult<Vec<(u64, Option<Json>)>> {
        if index != "torrents" {
            return Ok(entity_ids.iter().map(|id| (*id, None)).collect());
        }
        let ids: Vec<i64> = entity_ids.iter().filter_map(|i| i64::try_from(*i).ok()).collect();
        let mut c = ctx.sql.conn().await?;
        let rows = sqlx::query(
            "SELECT id, group_id, type_id, category_id, title, subtitle, size, file_count, uploader_id, is_anonymous,
                    status, extract(epoch FROM created_at)::bigint AS created
               FROM torrents WHERE id = ANY($1) AND deleted_at IS NULL",
        )
        .bind(&ids)
        .fetch_all(&mut *c)
        .await?;
        let mut docs = std::collections::HashMap::new();
        for r in rows {
            let status: String = r.get("status");
            let vis = match status.as_str() {
                "approved" => "member",
                "candidate" | "pending" => "staff",
                _ => continue,
            };
            let anonymous: bool = r.get("is_anonymous");
            let uploader: Option<i64> = if anonymous { None } else { r.get("uploader_id") };
            let id: i64 = r.get("id");
            let mut d = json!({
                "id": id,
                "group_id": r.get::<Option<i64>, _>("group_id"),
                "type_id": r.get::<String, _>("type_id"),
                "category_id": r.get::<Option<i32>, _>("category_id"),
                "title": r.get::<String, _>("title"),
                "subtitle": r.get::<Option<String>, _>("subtitle"),
                "size": r.get::<i64, _>("size"),
                "file_count": r.get::<i32, _>("file_count"),
                "uploader_id": uploader,
                "status": status,
                "vis_level": vis,
                "vis_min_class": 0,
                "created_at": r.get::<i64, _>("created"),
            });
            if let (Some(u), Some(m)) = (uploader, d.as_object_mut()) {
                m.insert("vis_owner_id".into(), json!(u));
            }
            docs.insert(id, d);
        }
        Ok(entity_ids
            .iter()
            .map(|id| (*id, i64::try_from(*id).ok().and_then(|i| docs.remove(&i))))
            .collect())
    }
}
