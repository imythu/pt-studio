//! search-meilisearch 插件（docs/plugins/search-meilisearch/）。
//!
//! 本版本实现：贡献者 schema 合并与设置生成、事件 → 待写表 → 延迟任务批量写入（文档哈希去重、删除优先、
//! 重建期间双写）、标题规范化与发布名解析、swap 重建状态机、查询（可见性基线、`search.before_query`、
//! 结构化过滤、排序、facet、折叠、游标、高亮、Redis 结果缓存）、联想、索引 schema 视图、`ctx.search`、
//! 失败任务回收。语义搜索、tenant token、多实例、做种数部分更新、同义词管理、一致性巡检、MCP 在后续迭代实现。

pub mod filter;
pub mod meili;
pub mod schema;

use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::{Path, Query, State as AxState},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::Engine;
use pt_plugin_api::{
    Ctx, DeliveryInfo, EventEnvelope, JobInvocation, Lifecycle, NativePlugin, NewEvent, OnCron, OnEvent,
    PResult, PluginError, RedisCommand, RedisReply, RequestCtx,
    host::HookOutcome,
    json,
    services::{RBAC, RbacService},
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::Row;

use crate::{
    filter::Filter,
    meili::Meili,
    schema::{MergedIndex, RESERVED},
};

const MANIFEST: &str = include_str!("../plugin.toml");
const MIGRATIONS: &[(&str, &str)] = &[("0001_init.sql", include_str!("../migrations/0001_init.sql"))];

const HL_PRE: char = '\u{E000}';
const HL_POST: char = '\u{E001}';

// ---------------- 配置与登记 ----------------

fn d_prefix() -> String {
    "pt".into()
}
fn d_req() -> u64 {
    2000
}
fn d_write() -> u64 {
    10_000
}
fn d_window() -> i64 {
    2000
}
fn d_batch() -> i64 {
    1000
}
fn d_ttl() -> u64 {
    45
}
fn d_hits() -> u64 {
    1000
}
fn d_limit() -> u32 {
    100
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
struct Instance {
    id: String,
    url: String,
    #[serde(default)]
    api_key: String,
}

/// site.toml `[plugin.search-meilisearch]`（LLD §4；运行时设置项本版本也放在静态配置中）
#[derive(Debug, Clone, Deserialize)]
struct Config {
    #[serde(default)]
    instances: Vec<Instance>,
    #[serde(default = "d_prefix")]
    index_prefix: String,
    #[serde(default = "d_req")]
    request_timeout_ms: u64,
    #[serde(default = "d_write")]
    write_timeout_ms: u64,
    #[serde(default = "d_window")]
    flush_window_ms: i64,
    #[serde(default = "d_batch")]
    batch_size: i64,
    #[serde(default = "d_ttl")]
    cache_ttl_secs: u64,
    #[serde(default = "d_hits")]
    max_total_hits: u64,
    #[serde(default = "d_limit")]
    max_limit: u32,
    #[serde(default = "yes")]
    auto_rebuild: bool,
}

/// 标准逻辑索引（LLD §1.4）；本版本只有 `torrents`
struct StdIndex {
    name: &'static str,
    owner: &'static str,
    audience: &'static str,
    title_fields: &'static [&'static str],
    distinct: Option<&'static str>,
    /// 重建枚举（拥有者未实现 list-ids 时的视图兜底）
    enumerate: &'static str,
}

const STANDARD: &[StdIndex] = &[StdIndex {
    name: "torrents",
    owner: "torrent",
    audience: "user",
    title_fields: &["title"],
    distinct: Some("group_id"),
    enumerate: "SELECT id FROM torrent.v_torrent_public WHERE id > $1 ORDER BY id LIMIT $2",
}];

fn std_index(name: &str) -> Option<&'static StdIndex> {
    STANDARD.iter().find(|s| s.name == name)
}

#[derive(Default)]
struct State {
    cfg: OnceLock<Config>,
    meili: OnceLock<Option<Meili>>,
}

#[derive(Clone)]
pub struct Search(Arc<State>);

impl Search {
    fn cfg(&self) -> PResult<&Config> {
        self.0
            .cfg
            .get()
            .ok_or_else(|| PluginError::unavailable("search-meilisearch is not initialised"))
    }

    fn meili(&self) -> Option<&Meili> {
        self.0.meili.get().and_then(Option::as_ref)
    }

    fn require_meili(&self) -> PResult<&Meili> {
        self.meili().ok_or_else(|| {
            PluginError::new(
                "search-meilisearch.unavailable",
                "no Meilisearch instance is configured",
            )
        })
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn sha_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

// ---------------- 索引行 ----------------

#[derive(Debug, Clone)]
struct IndexRow {
    name: String,
    owner: String,
    audience: String,
    live_uid: String,
    status: String,
    generation: i32,
    distinct_attr: Option<String>,
    merged: MergedIndex,
    settings_hash: Option<String>,
    shape_hash: Option<String>,
    applied_shape_hash: Option<String>,
    dual_write_uid: Option<String>,
    conflicts: Value,
}

async fn load_index(ctx: &Ctx, name: &str, lock: bool) -> PResult<Option<IndexRow>> {
    let mut c = ctx.sql.conn().await?;
    let r = sqlx::query(&format!(
        "SELECT name, owner_plugin, audience, live_uid, status, generation, distinct_attr, merged_schema, settings_hash,
                shape_hash, applied_shape_hash, dual_write_uid, conflicts FROM indexes WHERE name = $1{}",
        if lock { " FOR UPDATE" } else { "" }
    ))
    .bind(name)
    .fetch_optional(&mut *c)
    .await?;
    Ok(r.map(|r| IndexRow {
        name: r.get(0),
        owner: r.get(1),
        audience: r.get(2),
        live_uid: r.get(3),
        status: r.get(4),
        generation: r.get(5),
        distinct_attr: r.get(6),
        merged: serde_json::from_value(r.get(7)).unwrap_or_default(),
        settings_hash: r.get(8),
        shape_hash: r.get(9),
        applied_shape_hash: r.get(10),
        dual_write_uid: r.get(11),
        conflicts: r.get(12),
    }))
}

async fn require_index(ctx: &Ctx, name: &str) -> PResult<IndexRow> {
    load_index(ctx, name, false)
        .await?
        .filter(|r| r.status != "waiting_owner" && r.status != "disabled")
        .ok_or_else(|| {
            PluginError::new(
                "search-meilisearch.index_not_found",
                format!("index `{name}` not found"),
            )
        })
}

// ---------------- schema 刷新（init） ----------------

async fn refresh_schemas(s: &Search, ctx: &Ctx) -> PResult<()> {
    let cfg = s.cfg()?;
    let mut decls = Vec::new();
    for pid in ctx.search_contributors()? {
        for sc in ctx.contributor_schemas(&pid).await? {
            decls.push((pid.clone(), sc));
        }
    }
    for si in STANDARD {
        let merged = schema::merge(si.name, si.owner, si.title_fields, &decls);
        let has_owner = merged.contributors.iter().any(|c| c == si.owner);
        let settings = schema::build_settings(&merged, cfg.max_total_hits);
        let sh = schema::sha256_hex(&settings);
        let shp = schema::shape_hash(&merged);
        if !merged.conflicts.is_empty() {
            tracing::warn!(index = si.name, conflicts = ?merged.conflicts, "search schema conflicts");
        }
        let live = format!("{}_{}", cfg.index_prefix, si.name);
        let mut c = ctx.sql.conn().await?;
        let r = sqlx::query(
            "INSERT INTO indexes (name, owner_plugin, audience, is_builtin, live_uid, status, distinct_attr, merged_schema,
                                  settings_hash, shape_hash, conflicts)
             VALUES ($1, $2, $3, true, $4, $5, $6, $7, $8, $9, $10)
             ON CONFLICT (name) DO UPDATE SET
                 live_uid = EXCLUDED.live_uid, distinct_attr = EXCLUDED.distinct_attr, merged_schema = EXCLUDED.merged_schema,
                 settings_hash = EXCLUDED.settings_hash, shape_hash = EXCLUDED.shape_hash, conflicts = EXCLUDED.conflicts,
                 status = CASE WHEN EXCLUDED.status = 'waiting_owner' THEN 'waiting_owner'
                               WHEN indexes.status = 'waiting_owner' THEN 'creating' ELSE indexes.status END,
                 updated_at = now(), version = indexes.version + 1
             RETURNING status, applied_settings_hash, applied_shape_hash",
        )
        .bind(si.name)
        .bind(si.owner)
        .bind(si.audience)
        .bind(&live)
        .bind(if has_owner { "creating" } else { "waiting_owner" })
        .bind(si.distinct)
        .bind(serde_json::to_value(&merged).map_err(|e| PluginError::internal(e.to_string()))?)
        .bind(&sh)
        .bind(&shp)
        .bind(serde_json::to_value(&merged.conflicts).unwrap_or_default())
        .fetch_one(&mut *c)
        .await?;
        drop(c);
        let status: String = r.get(0);
        let applied: Option<String> = r.get(1);
        let applied_shape: Option<String> = r.get(2);
        if s.meili().is_some()
            && status != "waiting_owner"
            && (applied.as_deref() != Some(sh.as_str()) || applied_shape.as_deref() != Some(shp.as_str()))
        {
            ctx.jobs
                .schedule_once(
                    "settings_apply",
                    now_ms(),
                    json!({ "index": si.name }),
                    Some(&format!("settings:{}:{}:{}", si.name, &sh[..16], &shp[..16])),
                )
                .await?;
        }
    }
    Ok(())
}

// ---------------- ingest：标脏（LLD §1.5） ----------------

async fn mark(
    s: &Search,
    ctx: &Ctx,
    index: &str,
    ids: &[i64],
    op: &str,
    lane: &str,
    event: Option<uuid::Uuid>,
) -> PResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut c = ctx.sql.conn().await?;
    sqlx::query(
        "INSERT INTO pending_ops (index_name, entity_id, op, lane, source_event_id)
         SELECT $1, x, $3, $4, $5 FROM unnest($2::bigint[]) AS x
         ON CONFLICT (index_name, entity_id) DO UPDATE
            SET op = EXCLUDED.op,
                lane = CASE WHEN pending_ops.lane = 'priority' OR EXCLUDED.lane = 'priority' THEN 'priority' ELSE 'normal' END,
                seq = nextval('pending_ops_seq'), enqueued_at = now(), source_event_id = EXCLUDED.source_event_id",
    )
    .bind(index)
    .bind(ids)
    .bind(op)
    .bind(lane)
    .bind(event)
    .execute(&mut *c)
    .await?;
    // 重建期间：记录被触碰的实体，供 replaying 阶段重放
    sqlx::query(
        "INSERT INTO rebuild_touched (rebuild_id, entity_id)
         SELECT r.id, x FROM rebuilds r, unnest($2::bigint[]) AS x
          WHERE r.index_name = $1 AND r.state NOT IN ('done','failed','cancelled')
         ON CONFLICT DO NOTHING",
    )
    .bind(index)
    .bind(ids)
    .execute(&mut *c)
    .await?;
    drop(c);
    let window = s.cfg()?.flush_window_ms.max(100);
    let now = now_ms();
    let run_at = if lane == "priority" {
        now
    } else {
        (now / window + 1) * window
    };
    ctx.jobs
        .schedule_once(
            "flush",
            run_at,
            json!({ "index": index, "lane": lane }),
            Some(&format!(
                "flush:{index}:{lane}:{}",
                if lane == "priority" { now / 1000 } else { run_at }
            )),
        )
        .await?;
    Ok(())
}

fn ids_of(v: &Value) -> Vec<i64> {
    match v {
        Value::Array(a) => a.iter().filter_map(Value::as_i64).collect(),
        Value::Number(n) => n.as_i64().into_iter().collect(),
        _ => vec![],
    }
}

// ---------------- 文档生成（LLD §1.6） ----------------

struct Built {
    docs: Vec<(i64, Value, String, Option<i64>)>,
    deletes: Vec<i64>,
    partial: Vec<i64>,
}

fn strip_marks(v: &mut Value) {
    match v {
        Value::String(s) if s.contains([HL_PRE, HL_POST]) => {
            *s = s.chars().filter(|c| *c != HL_PRE && *c != HL_POST).collect();
        }
        Value::Array(a) => a.iter_mut().for_each(strip_marks),
        _ => {}
    }
}

async fn build_docs(ctx: &Ctx, m: &MergedIndex, ids: &[i64]) -> PResult<Built> {
    let uids: Vec<u64> = ids.iter().filter_map(|i| u64::try_from(*i).ok()).collect();
    let mut frags: Vec<(String, HashMap<u64, Value>)> = Vec::new();
    let mut partial: Vec<i64> = Vec::new();
    for pid in &m.contributors {
        let r = ctx.contributor_documents(pid, &m.index, &uids).await;
        match r {
            Ok(list) => frags.push((
                pid.clone(),
                list.into_iter()
                    .filter_map(|(id, d)| d.map(|d| (id, d)))
                    .collect(),
            )),
            // 拥有者失败：整批重试；其他贡献者失败：本次缺这些字段（LLD DD-SRCH-14）
            Err(e) if *pid == m.owner => return Err(e),
            Err(e) => {
                tracing::warn!(plugin = %pid, index = %m.index, error = %e, "search contributor failed");
                partial.extend_from_slice(ids);
            }
        }
    }
    let allowed: HashMap<&str, Vec<&str>> = m
        .contributors
        .iter()
        .map(|p| {
            let mut f = m.fields_of(p);
            if *p == m.owner {
                f.extend_from_slice(RESERVED);
            }
            (p.as_str(), f)
        })
        .collect();
    let mut out = Built {
        docs: vec![],
        deletes: vec![],
        partial,
    };
    for (id, uid) in ids.iter().zip(&uids) {
        let owner_doc = frags
            .iter()
            .find(|(p, _)| *p == m.owner)
            .and_then(|(_, f)| f.get(uid));
        let Some(owner_doc) = owner_doc else {
            out.deletes.push(*id);
            continue;
        };
        let mut doc = serde_json::Map::new();
        for (pid, f) in &frags {
            let Some(Value::Object(frag)) = (if pid == &m.owner {
                Some(owner_doc)
            } else {
                f.get(uid)
            }) else {
                continue;
            };
            let ok = allowed.get(pid.as_str()).cloned().unwrap_or_default();
            for (k, v) in frag {
                if ok.contains(&k.as_str()) && !doc.contains_key(k) {
                    let mut v = v.clone();
                    strip_marks(&mut v);
                    doc.insert(k.clone(), v);
                }
            }
        }
        if !matches!(
            doc.get("vis_level").and_then(Value::as_str),
            Some("public" | "member" | "staff")
        ) {
            tracing::warn!(index = %m.index, id, "owner document without a valid vis_level; not indexed");
            out.deletes.push(*id);
            continue;
        }
        doc.insert("id".into(), json!(id));
        // 标题预处理（§1.7）：*_norm 与 rel_*（贡献者已给出的同名字段不覆盖）
        for (i, t) in m.title_fields.iter().enumerate() {
            let Some(title) = doc.get(t).and_then(Value::as_str).map(str::to_owned) else {
                continue;
            };
            doc.insert(
                format!("{t}_norm"),
                json!(pt_release_parser::normalize_title(&title)),
            );
            if i == 0 {
                for (k, v) in rel_fields(&pt_release_parser::parse_release(&title)) {
                    doc.entry(k).or_insert(v);
                }
            }
        }
        for f in m.fields.keys() {
            doc.entry(f.clone()).or_insert(Value::Null);
        }
        let doc = Value::Object(doc);
        let hash = sha_hex(&schema::canonical(&doc));
        let group = doc.get("group_id").and_then(Value::as_i64);
        out.docs.push((*id, doc, hash, group));
    }
    Ok(out)
}

/// 发布名解析结果 → `rel_*` 字段
fn rel_fields(r: &pt_release_parser::ReleaseInfo) -> Vec<(&'static str, Value)> {
    vec![
        ("rel_resolution", json!(r.resolution)),
        ("rel_source", json!(r.source)),
        ("rel_video_codec", json!(r.video_codec)),
        ("rel_audio_codec", json!(r.audio_codec)),
        ("rel_hdr", json!(r.hdr)),
        ("rel_group", json!(r.group)),
        ("rel_year", json!(r.year)),
    ]
}

#[allow(clippy::too_many_arguments)]
async fn record_task(
    ctx: &Ctx,
    meili: &Meili,
    task: u64,
    uid: &str,
    kind: &str,
    lane: Option<&str>,
    index: &str,
    ids: &[i64],
) -> PResult<()> {
    let mut c = ctx.sql.conn().await?;
    sqlx::query(
        "INSERT INTO meili_tasks (instance_id, task_uid, index_uid, kind, lane, index_name, entity_ids)
         VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT DO NOTHING",
    )
    .bind(&meili.id)
    .bind(i64::try_from(task).unwrap_or(i64::MAX))
    .bind(uid)
    .bind(kind)
    .bind(lane)
    .bind(index)
    .bind(ids)
    .execute(&mut *c)
    .await?;
    Ok(())
}

/// 把生成结果写入目标索引并更新 indexed_docs（flush 与重建共用）；返回 Meilisearch 任务 ID
async fn write_built(
    ctx: &Ctx,
    meili: &Meili,
    idx: &str,
    targets: &[&str],
    b: &Built,
    lane: Option<&str>,
) -> PResult<Vec<u64>> {
    let mut tasks = Vec::new();
    let docs: Vec<Value> = b.docs.iter().map(|d| d.1.clone()).collect();
    let doc_ids: Vec<i64> = b.docs.iter().map(|d| d.0).collect();
    for uid in targets {
        if !docs.is_empty() {
            let t = meili.update_documents(uid, &docs).await?;
            tasks.push(t);
            record_task(ctx, meili, t, uid, "docs", lane, idx, &doc_ids).await?;
        }
        if !b.deletes.is_empty() {
            let t = meili.delete_documents(uid, &b.deletes).await?;
            tasks.push(t);
            record_task(ctx, meili, t, uid, "delete", lane, idx, &b.deletes).await?;
        }
    }
    let mut c = ctx.sql.conn().await?;
    if !b.docs.is_empty() {
        let hashes: Vec<String> = b.docs.iter().map(|d| d.2.clone()).collect();
        let groups: Vec<Option<i64>> = b.docs.iter().map(|d| d.3).collect();
        let partial: Vec<bool> = doc_ids.iter().map(|i| b.partial.contains(i)).collect();
        sqlx::query(
            "INSERT INTO indexed_docs (index_name, entity_id, doc_hash, group_id, partial)
             SELECT $1, x.id, x.h, x.g, x.p FROM unnest($2::bigint[], $3::text[], $4::bigint[], $5::bool[]) AS x(id, h, g, p)
             ON CONFLICT (index_name, entity_id) DO UPDATE
                SET doc_hash = EXCLUDED.doc_hash, group_id = EXCLUDED.group_id, partial = EXCLUDED.partial, indexed_at = now()",
        )
        .bind(idx)
        .bind(&doc_ids)
        .bind(&hashes)
        .bind(&groups)
        .bind(&partial)
        .execute(&mut *c)
        .await?;
    }
    if !b.deletes.is_empty() {
        sqlx::query("DELETE FROM indexed_docs WHERE index_name = $1 AND entity_id = ANY($2)")
            .bind(idx)
            .bind(&b.deletes)
            .execute(&mut *c)
            .await?;
    }
    Ok(tasks)
}

async fn bump_cache_gen(ctx: &Ctx, index: &str) {
    if let Err(e) = ctx
        .redis
        .call(RedisCommand::new("INCR").arg(format!("qcgen:{index}")))
        .await
    {
        tracing::warn!(error = %e, "failed to bump search cache generation");
    }
}

// ---------------- flusher（LLD §1.6） ----------------

async fn flush(s: &Search, ctx: &Ctx, index: &str, lane: &str) -> PResult<usize> {
    let Some(meili) = s.meili() else { return Ok(0) };
    let Some(row) = load_index(ctx, index, false).await? else {
        return Ok(0);
    };
    if !matches!(row.status.as_str(), "ready" | "rebuilding" | "stale" | "degraded") {
        // 索引尚未建好：行留在表中，由 settings_apply 之后的重建或 flush_sweeper 处理
        return Ok(0);
    }
    let batch = s.cfg()?.batch_size.clamp(1, 5000);
    let started = std::time::Instant::now();
    let mut total = 0;
    while started.elapsed() < Duration::from_secs(25) {
        let mut c = ctx.sql.conn().await?;
        let rows = sqlx::query(
            "SELECT entity_id, op, seq FROM pending_ops WHERE index_name = $1 AND lane = $2
              ORDER BY seq LIMIT $3 FOR UPDATE SKIP LOCKED",
        )
        .bind(index)
        .bind(lane)
        .bind(batch)
        .fetch_all(&mut *c)
        .await?;
        if rows.is_empty() {
            break;
        }
        let seqs: Vec<(i64, i64)> = rows.iter().map(|r| (r.get(0), r.get(2))).collect();
        let upserts: Vec<i64> = rows
            .iter()
            .filter(|r| r.get::<String, _>(1) == "upsert")
            .map(|r| r.get(0))
            .collect();
        let deletes: Vec<i64> = rows
            .iter()
            .filter(|r| r.get::<String, _>(1) == "delete")
            .map(|r| r.get(0))
            .collect();
        let known: HashMap<i64, String> = sqlx::query(
            "SELECT entity_id, doc_hash FROM indexed_docs WHERE index_name = $1 AND entity_id = ANY($2)",
        )
        .bind(index)
        .bind(&upserts)
        .fetch_all(&mut *c)
        .await?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
        drop(c);
        let mut b = build_docs(ctx, &row.merged, &upserts).await?;
        b.deletes.extend(deletes);
        // 新文档（新发布的种子）要立即出现在列表中：此时也让查询缓存失效
        let has_new = b.docs.iter().any(|d| !known.contains_key(&d.0));
        // 文档未变：跳过写入（仍删除待写行）
        b.docs.retain(|d| known.get(&d.0) != Some(&d.2));
        let mut targets = vec![row.live_uid.as_str()];
        if let Some(d) = row.dual_write_uid.as_deref() {
            targets.push(d);
        }
        let tasks = write_built(ctx, meili, index, &targets, &b, Some(lane)).await?;
        let mut c = ctx.sql.conn().await?;
        sqlx::query(
            "DELETE FROM pending_ops p USING unnest($2::bigint[], $3::bigint[]) AS x(id, seq)
              WHERE p.index_name = $1 AND p.entity_id = x.id AND p.seq = x.seq",
        )
        .bind(index)
        .bind(seqs.iter().map(|x| x.0).collect::<Vec<_>>())
        .bind(seqs.iter().map(|x| x.1).collect::<Vec<_>>())
        .execute(&mut *c)
        .await?;
        drop(c);
        if lane == "priority" || has_new || !b.deletes.is_empty() {
            // 等索引任务完成再让缓存失效，否则失效后立即到来的查询会把旧结果重新缓存
            for t in &tasks {
                let _ = meili.wait(*t, Duration::from_secs(3)).await;
            }
            bump_cache_gen(ctx, index).await;
        }
        total += seqs.len();
        ctx.sql.commit().await?;
    }
    Ok(total)
}

// ---------------- 设置应用与重建（LLD §1.3、§1.17） ----------------

async fn settings_apply(s: &Search, ctx: &Ctx, index: &str) -> PResult<()> {
    let Some(meili) = s.meili() else { return Ok(()) };
    let cfg = s.cfg()?;
    let Some(row) = load_index(ctx, index, true).await? else {
        return Ok(());
    };
    if row.status == "waiting_owner" {
        return Ok(());
    }
    let settings = schema::build_settings(&row.merged, cfg.max_total_hits);
    if !meili.index_exists(&row.live_uid).await? {
        let t = meili.create_index(&row.live_uid).await?;
        meili.wait(t, Duration::from_secs(30)).await?;
    }
    let mut uids = vec![row.live_uid.clone()];
    uids.extend(row.dual_write_uid.clone());
    for uid in &uids {
        let t = meili.patch_settings(uid, &settings).await?;
        if !meili.wait(t, Duration::from_secs(100)).await? {
            return Err(PluginError::new(
                "search-meilisearch.unavailable",
                "settings task did not finish in time",
            )
            .retryable());
        }
    }
    let mut c = ctx.sql.conn().await?;
    sqlx::query(
        "UPDATE indexes SET applied_settings_hash = settings_hash,
                status = CASE WHEN status = 'creating' THEN 'ready' ELSE status END, updated_at = now()
          WHERE name = $1",
    )
    .bind(index)
    .execute(&mut *c)
    .await?;
    drop(c);
    if row.applied_shape_hash != row.shape_hash {
        if cfg.auto_rebuild {
            start_rebuild(ctx, &row, "auto", None, None).await?;
        } else {
            let mut c = ctx.sql.conn().await?;
            sqlx::query("UPDATE indexes SET status = 'stale' WHERE name = $1 AND status = 'ready'")
                .bind(index)
                .execute(&mut *c)
                .await?;
        }
    }
    Ok(())
}

/// 创建重建；已有进行中的重建时返回其 ID（第二项为 false）
async fn start_rebuild(
    ctx: &Ctx,
    row: &IndexRow,
    trigger: &str,
    by: Option<i64>,
    reason: Option<&str>,
) -> PResult<(uuid::Uuid, bool)> {
    let mut c = ctx.sql.conn().await?;
    if let Some(r) = sqlx::query(
        "SELECT id FROM rebuilds WHERE index_name = $1 AND state NOT IN ('done','failed','cancelled')",
    )
    .bind(&row.name)
    .fetch_optional(&mut *c)
    .await?
    {
        return Ok((r.get(0), false));
    }
    let id = uuid::Uuid::now_v7();
    sqlx::query(
        "INSERT INTO rebuilds (id, index_name, state, trigger, target_uid, from_generation, requested_by, reason)
         VALUES ($1, $2, 'preparing', $3, $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(&row.name)
    .bind(trigger)
    .bind(format!("{}__next", row.live_uid))
    .bind(row.generation)
    .bind(by)
    .bind(reason)
    .execute(&mut *c)
    .await?;
    drop(c);
    schedule_step(ctx, id, "preparing", 0, 0).await?;
    Ok((id, true))
}

async fn schedule_step(ctx: &Ctx, id: uuid::Uuid, state: &str, cursor: i64, delay_ms: i64) -> PResult<()> {
    ctx.jobs
        .schedule_once(
            "rebuild_step",
            now_ms() + delay_ms,
            json!({ "rebuild_id": id }),
            Some(&format!("rebuild:{id}:{state}:{cursor}:{}", now_ms() / 1000)),
        )
        .await?;
    Ok(())
}

struct Rebuild {
    id: uuid::Uuid,
    index: String,
    state: String,
    trigger: String,
    target: String,
    cursor: i64,
    attempts: i32,
    started_ms: i64,
}

const REBUILD_BATCH: i64 = 500;

async fn rebuild_step(s: &Search, ctx: &Ctx, id: uuid::Uuid) -> PResult<()> {
    let mut c = ctx.sql.conn().await?;
    let Some(r) = sqlx::query(
        "SELECT id, index_name, state, trigger, target_uid, cursor_id, attempts, (extract(epoch FROM started_at) * 1000)::bigint
           FROM rebuilds WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *c)
    .await?
    else {
        return Ok(());
    };
    drop(c);
    let r = Rebuild {
        id: r.get(0),
        index: r.get(1),
        state: r.get(2),
        trigger: r.get(3),
        target: r.get(4),
        cursor: r.get(5),
        attempts: r.get(6),
        started_ms: r.get(7),
    };
    if matches!(r.state.as_str(), "done" | "failed" | "cancelled") {
        return Ok(());
    }
    match step(s, ctx, &r).await {
        Ok(()) => Ok(()),
        Err(e) => {
            // 记录失败次数（不回滚本步已提交的数据）；连续 5 次失败 → failed，关闭双写，在线索引不受影响
            tracing::warn!(rebuild = %r.id, state = %r.state, error = %e, "rebuild step failed");
            let failed = r.attempts + 1 >= 5;
            let mut c = ctx.sql.conn().await?;
            sqlx::query(
                "UPDATE rebuilds SET attempts = attempts + 1, error = left($2, 2000), updated_at = now(),
                        state = CASE WHEN $3 THEN 'failed' ELSE state END, finished_at = CASE WHEN $3 THEN now() END
                  WHERE id = $1",
            )
            .bind(r.id)
            .bind(e.to_string())
            .bind(failed)
            .execute(&mut *c)
            .await?;
            if failed {
                sqlx::query("UPDATE indexes SET dual_write_uid = NULL, status = 'ready' WHERE name = $1 AND status = 'rebuilding'")
                    .bind(&r.index)
                    .execute(&mut *c)
                    .await?;
            } else {
                drop(c);
                schedule_step(ctx, r.id, &r.state, r.cursor, 2000 << r.attempts.clamp(0, 4)).await?;
            }
            Ok(())
        }
    }
}

/// 索引状态变化后（开始双写、交换完成）立即处理积压的待写行
async fn kick_flush(ctx: &Ctx, index: &str) -> PResult<()> {
    for lane in ["priority", "normal"] {
        ctx.jobs
            .schedule_once(
                "flush",
                now_ms(),
                json!({ "index": index, "lane": lane }),
                Some(&format!("flush:{index}:{lane}:kick:{}", now_ms())),
            )
            .await?;
    }
    Ok(())
}

async fn set_state(ctx: &Ctx, r: &Rebuild, state: &str, cursor: i64, done: i64) -> PResult<()> {
    let mut c = ctx.sql.conn().await?;
    sqlx::query(
        "UPDATE rebuilds SET state = $2, cursor_id = $3, done_count = done_count + $4, attempts = 0, updated_at = now(),
                finished_at = CASE WHEN $2 IN ('done','failed','cancelled') THEN now() END
          WHERE id = $1",
    )
    .bind(r.id)
    .bind(state)
    .bind(cursor)
    .bind(done)
    .execute(&mut *c)
    .await?;
    drop(c);
    if !matches!(state, "done" | "failed" | "cancelled") {
        schedule_step(ctx, r.id, state, cursor, 0).await?;
    }
    Ok(())
}

async fn last_task(ctx: &Ctx, uid: &str) -> PResult<Option<u64>> {
    let mut c = ctx.sql.conn().await?;
    let t: Option<i64> = sqlx::query_scalar("SELECT max(task_uid) FROM meili_tasks WHERE index_uid = $1")
        .bind(uid)
        .fetch_one(&mut *c)
        .await?;
    Ok(t.and_then(|t| u64::try_from(t).ok()))
}

async fn step(s: &Search, ctx: &Ctx, r: &Rebuild) -> PResult<()> {
    let meili = s.require_meili()?;
    let cfg = s.cfg()?;
    let row = load_index(ctx, &r.index, false)
        .await?
        .ok_or_else(|| PluginError::internal("index row disappeared"))?;
    let si = std_index(&r.index).ok_or_else(|| PluginError::internal("not a standard index"))?;
    match r.state.as_str() {
        "preparing" => {
            if meili.index_exists(&r.target).await? {
                let t = meili.delete_index(&r.target).await?;
                meili.wait(t, Duration::from_secs(60)).await?;
            }
            let t = meili.create_index(&r.target).await?;
            meili.wait(t, Duration::from_secs(60)).await?;
            let t = meili
                .patch_settings(
                    &r.target,
                    &schema::build_settings(&row.merged, cfg.max_total_hits),
                )
                .await?;
            if !meili.wait(t, Duration::from_secs(200)).await? {
                return Err(PluginError::unavailable("settings task did not finish in time"));
            }
            let mut c = ctx.sql.conn().await?;
            sqlx::query("UPDATE indexes SET dual_write_uid = $2, status = 'rebuilding', updated_at = now() WHERE name = $1")
                .bind(&r.index)
                .bind(&r.target)
                .execute(&mut *c)
                .await?;
            drop(c);
            kick_flush(ctx, &r.index).await?;
            set_state(ctx, r, "building", 0, 0).await
        }
        "building" => {
            let mut c = ctx.sql.conn().await?;
            let ids: Vec<i64> = sqlx::query_scalar(si.enumerate)
                .bind(r.cursor)
                .bind(REBUILD_BATCH)
                .fetch_all(&mut *c)
                .await?;
            drop(c);
            let Some(max) = ids.last().copied() else {
                return set_state(ctx, r, "replaying", r.cursor, 0).await;
            };
            let b = build_docs(ctx, &row.merged, &ids).await?;
            let n = i64::try_from(b.docs.len()).unwrap_or(0);
            // 只写构建索引；不存在的实体只需从 indexed_docs 移除（target 是新索引）
            let only_docs = Built {
                docs: b.docs,
                deletes: vec![],
                partial: b.partial,
            };
            write_built(ctx, meili, &r.index, &[r.target.as_str()], &only_docs, None).await?;
            if !b.deletes.is_empty() {
                let mut c = ctx.sql.conn().await?;
                sqlx::query("DELETE FROM indexed_docs WHERE index_name = $1 AND entity_id = ANY($2)")
                    .bind(&r.index)
                    .bind(&b.deletes)
                    .execute(&mut *c)
                    .await?;
            }
            set_state(ctx, r, "building", max, n).await
        }
        "replaying" => {
            let mut c = ctx.sql.conn().await?;
            let ids: Vec<i64> = sqlx::query_scalar(
                "SELECT entity_id FROM rebuild_touched WHERE rebuild_id = $1 ORDER BY entity_id LIMIT $2",
            )
            .bind(r.id)
            .bind(REBUILD_BATCH)
            .fetch_all(&mut *c)
            .await?;
            drop(c);
            if ids.is_empty() {
                return set_state(ctx, r, "verifying", r.cursor, 0).await;
            }
            let b = build_docs(ctx, &row.merged, &ids).await?;
            write_built(ctx, meili, &r.index, &[r.target.as_str()], &b, None).await?;
            let mut c = ctx.sql.conn().await?;
            sqlx::query("DELETE FROM rebuild_touched WHERE rebuild_id = $1 AND entity_id = ANY($2)")
                .bind(r.id)
                .bind(&ids)
                .execute(&mut *c)
                .await?;
            drop(c);
            set_state(ctx, r, "replaying", r.cursor, 0).await
        }
        "verifying" => {
            if let Some(t) = last_task(ctx, &r.target).await?
                && !meili.wait(t, Duration::from_secs(60)).await?
            {
                if now_ms() - r.started_ms > 2 * 3600 * 1000 {
                    return Err(PluginError::internal(
                        "rebuild tasks did not finish within 2 hours",
                    ));
                }
                return schedule_step(ctx, r.id, "verifying", r.cursor, 10_000).await;
            }
            let n = i64::try_from(meili.document_count(&r.target).await?).unwrap_or(i64::MAX);
            let mut c = ctx.sql.conn().await?;
            let expected: i64 = sqlx::query_scalar("SELECT count(*) FROM indexed_docs WHERE index_name = $1")
                .bind(&r.index)
                .fetch_one(&mut *c)
                .await?;
            sqlx::query("UPDATE rebuilds SET expected_total = $2 WHERE id = $1")
                .bind(r.id)
                .bind(expected)
                .execute(&mut *c)
                .await?;
            drop(c);
            if (n - expected).abs() > expected / 1000 {
                return Err(PluginError::internal(format!(
                    "document count mismatch: index has {n}, expected {expected}"
                )));
            }
            set_state(ctx, r, "swapping", r.cursor, 0).await
        }
        "swapping" => {
            let t = meili.swap(&row.live_uid, &r.target).await?;
            if !meili.wait(t, Duration::from_secs(60)).await? {
                return Err(PluginError::unavailable("swap task did not finish in time"));
            }
            let mut c = ctx.sql.conn().await?;
            sqlx::query(
                "UPDATE indexes SET generation = generation + 1, dual_write_uid = NULL, status = 'ready',
                        applied_shape_hash = shape_hash, updated_at = now() WHERE name = $1",
            )
            .bind(&r.index)
            .execute(&mut *c)
            .await?;
            drop(c);
            bump_cache_gen(ctx, &r.index).await;
            kick_flush(ctx, &r.index).await?;
            set_state(ctx, r, "cleanup", r.cursor, 0).await
        }
        "cleanup" => {
            // 交换后 target 中是旧一代数据
            let _ = meili.delete_index(&r.target).await?;
            let documents = meili.document_count(&row.live_uid).await.unwrap_or(0);
            let mut c = ctx.sql.conn().await?;
            sqlx::query("DELETE FROM rebuild_touched WHERE rebuild_id = $1")
                .bind(r.id)
                .execute(&mut *c)
                .await?;
            sqlx::query("DELETE FROM meili_tasks WHERE index_uid = $1")
                .bind(&r.target)
                .execute(&mut *c)
                .await?;
            drop(c);
            ctx.events
                .publish(NewEvent::v1(
                    "search.index_rebuilt",
                    "global",
                    json!({ "index": r.index, "documents": documents, "duration_ms": now_ms() - r.started_ms,
                            "generation": row.generation, "trigger": r.trigger }),
                ))
                .await?;
            set_state(ctx, r, "done", r.cursor, 0).await
        }
        _ => Ok(()),
    }
}

/// 取消重建（swapping 之前）
async fn cancel_rebuild(s: &Search, ctx: &Ctx, id: uuid::Uuid) -> PResult<()> {
    let mut c = ctx.sql.conn().await?;
    let r = sqlx::query("SELECT index_name, state, target_uid FROM rebuilds WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *c)
        .await?
        .ok_or_else(|| PluginError::not_found("rebuild not found"))?;
    let (index, state, target): (String, String, String) = (r.get(0), r.get(1), r.get(2));
    match state.as_str() {
        "preparing" | "building" | "replaying" | "verifying" => {}
        "cancelled" => return Ok(()),
        _ => {
            return Err(PluginError::new(
                "search-meilisearch.rebuild_not_cancellable",
                format!("rebuild is {state}"),
            ));
        }
    }
    sqlx::query(
        "UPDATE rebuilds SET state = 'cancelled', finished_at = now(), updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .execute(&mut *c)
    .await?;
    sqlx::query("UPDATE indexes SET dual_write_uid = NULL, status = 'ready' WHERE name = $1 AND status = 'rebuilding'")
        .bind(&index)
        .execute(&mut *c)
        .await?;
    drop(c);
    if let Some(m) = s.meili() {
        let _ = m.delete_index(&target).await;
    }
    Ok(())
}

// ---------------- 失败任务回收（LLD §1.11） ----------------

async fn reap_tasks(s: &Search, ctx: &Ctx) -> PResult<()> {
    let Some(meili) = s.meili() else { return Ok(()) };
    let mut c = ctx.sql.conn().await?;
    let rows = sqlx::query(
        "SELECT task_uid, lane, index_name, entity_ids, kind FROM meili_tasks
          WHERE instance_id = $1 AND status = 'enqueued' AND enqueued_at < now() - interval '30 seconds'
          ORDER BY enqueued_at LIMIT 500",
    )
    .bind(&meili.id)
    .fetch_all(&mut *c)
    .await?;
    drop(c);
    for r in rows {
        let task: i64 = r.get(0);
        let (status, err) = meili.task_status(u64::try_from(task).unwrap_or(0)).await?;
        let mut c = ctx.sql.conn().await?;
        match status.as_str() {
            "succeeded" => {
                sqlx::query("DELETE FROM meili_tasks WHERE instance_id = $1 AND task_uid = $2")
                    .bind(&meili.id)
                    .bind(task)
                    .execute(&mut *c)
                    .await?;
            }
            "failed" | "canceled" => {
                tracing::warn!(task, error = ?err, "meilisearch task failed; re-marking entities");
                sqlx::query(
                    "UPDATE meili_tasks SET status = 'failed', error_code = $3, finished_at = now()
                      WHERE instance_id = $1 AND task_uid = $2",
                )
                .bind(&meili.id)
                .bind(task)
                .bind(err.and_then(|e| e["code"].as_str().map(str::to_owned)))
                .execute(&mut *c)
                .await?;
                drop(c);
                let (lane, index, ids, kind): (Option<String>, Option<String>, Vec<i64>, String) =
                    (r.get(1), r.get(2), r.get(3), r.get(4));
                if let (Some(lane), Some(index)) = (lane, index) {
                    let op = if kind == "delete" { "delete" } else { "upsert" };
                    // 文档哈希作废，确保重写
                    let mut c = ctx.sql.conn().await?;
                    sqlx::query("DELETE FROM indexed_docs WHERE index_name = $1 AND entity_id = ANY($2)")
                        .bind(&index)
                        .bind(&ids)
                        .execute(&mut *c)
                        .await?;
                    drop(c);
                    mark(s, ctx, &index, &ids, op, &lane, None).await?;
                }
            }
            _ => {}
        }
        ctx.sql.commit().await?;
    }
    sqlx::query(
        "DELETE FROM meili_tasks WHERE status <> 'enqueued' AND finished_at < now() - interval '7 days'",
    )
    .execute(&mut *ctx.sql.conn().await?)
    .await?;
    Ok(())
}

// ---------------- 查询（LLD §1.12） ----------------

#[derive(Debug, Clone, Deserialize)]
pub struct SortSpec {
    pub field: String,
    #[serde(default)]
    pub descending: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SearchRequest {
    pub index: String,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub sort: Vec<SortSpec>,
    #[serde(default)]
    pub facets: Vec<String>,
    #[serde(default)]
    pub collapse: bool,
    #[serde(default)]
    pub semantic: bool,
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub cursor: Option<String>,
}

enum Viewer {
    System,
    Anonymous,
    User {
        id: u64,
        class_id: i64,
        restricted: bool,
        admin_index: bool,
    },
}

async fn viewer_of(ctx: &Ctx) -> PResult<Viewer> {
    if ctx.invocation.caller_scopes.iter().any(|s| s == "query:all") {
        return Ok(Viewer::System);
    }
    let Some(uid) = ctx.user_id() else {
        return Ok(Viewer::Anonymous);
    };
    let u = ctx.call_json("ctx.users", "get", json!({ "id": uid })).await?;
    if u.is_null() || matches!(u["status"].as_str(), Some("banned" | "deleted" | "pending")) {
        return Ok(Viewer::Anonymous);
    }
    let check = |perm: &'static str| async move {
        ctx.service::<dyn RbacService>(RBAC)?
            .call(move |s, c| Box::pin(async move { s.check(c, uid, perm).await }))
            .await
    };
    Ok(Viewer::User {
        id: uid,
        class_id: u["class_id"].as_i64().unwrap_or(0),
        restricted: check("search-meilisearch.restricted.read").await?,
        admin_index: check("search-meilisearch.admin_index.read").await?,
    })
}

fn visibility_expr(v: &Viewer, audience: &str) -> String {
    match v {
        _ if audience == "admin" => "id >= 0".into(),
        Viewer::System => "id >= 0".into(),
        Viewer::Anonymous => "vis_level = 'public'".into(),
        Viewer::User {
            id,
            class_id,
            restricted,
            ..
        } => {
            let mut e = format!(
                "(vis_level IN ['public', 'member'] AND vis_min_class <= {class_id}) OR vis_owner_id = {id}"
            );
            if *restricted {
                e.push_str(" OR vis_level = 'staff'");
            }
            format!("({e})")
        }
    }
}

fn err422(code: &str, msg: impl Into<String>) -> PluginError {
    PluginError::new(code, msg)
}

#[derive(Debug, serde::Serialize, Deserialize)]
struct Cursor {
    o: u64,
    h: String,
}

/// 可见性基线 + 钩子；返回最终过滤数组
async fn final_filters(
    ctx: &Ctx,
    row: &IndexRow,
    viewer: &Viewer,
    query: &str,
    user: Vec<String>,
    sort: Vec<String>,
) -> PResult<Vec<String>> {
    let base = visibility_expr(viewer, &row.audience);
    let mut filters = vec![base];
    filters.extend(user);
    let (uid, class) = match viewer {
        Viewer::User { id, class_id, .. } => (Some(*id), Some(*class_id)),
        _ => (None, None),
    };
    let mut input = json!({ "version": 1, "actor": ctx.actor(), "index": row.name, "query": query, "filters": filters, "sort": sort });
    if let (Some(u), Some(c), Some(o)) = (uid, class, input.as_object_mut()) {
        o.insert("user_id".into(), json!(u));
        o.insert("class_id".into(), json!(c));
    }
    match ctx.hooks.run("search.before_query", input).await? {
        HookOutcome::Allowed(out) => {
            let all: Vec<String> = out["filters"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
                .unwrap_or_default();
            if all.len() < filters.len() || all[..filters.len()] != filters[..] {
                return Err(PluginError::new(
                    "kernel.hook_failed",
                    "search.before_query removed filters",
                ));
            }
            for extra in &all[filters.len()..] {
                filter::validate_raw(extra, &row.merged).map_err(|e| {
                    PluginError::new("kernel.hook_failed", format!("invalid appended filter: {e}"))
                })?;
            }
            Ok(all)
        }
        HookOutcome::Denied {
            code,
            message,
            details,
            ..
        } => {
            let e = PluginError::new(code, message);
            Err(match details {
                Some(d) => e.with_details(d),
                None => e,
            })
        }
    }
}

async fn query(s: &Search, ctx: &Ctx, req: SearchRequest) -> PResult<Value> {
    let cfg = s.cfg()?;
    let row = require_index(ctx, &req.index).await?;
    let viewer = viewer_of(ctx).await?;
    if row.audience == "admin"
        && !matches!(
            viewer,
            Viewer::System
                | Viewer::User {
                    admin_index: true,
                    ..
                }
        )
    {
        return Err(PluginError::forbidden(
            "this index needs search-meilisearch.admin_index.read",
        ));
    }
    if req.query.chars().count() > 256 {
        return Err(err422(
            "search-meilisearch.query_too_long",
            "query is longer than 256 characters",
        ));
    }
    if req.semantic {
        return Err(PluginError::new(
            "search-meilisearch.semantic_unavailable",
            "semantic search is not configured",
        ));
    }
    if req.filters.len() > 20 || req.facets.len() > 10 || req.sort.len() > 8 {
        return Err(err422(
            "search-meilisearch.invalid_filter",
            "too many filters, facets or sort fields",
        ));
    }
    let m = &row.merged;
    let user: Vec<String> = req
        .filters
        .iter()
        .map(|f| filter::to_expr(f, m).map_err(|(code, msg)| err422(code, msg)))
        .collect::<PResult<_>>()?;
    let mut sort = Vec::new();
    for sp in &req.sort {
        if !m.field(&sp.field).is_some_and(|f| f.sortable) {
            return Err(err422(
                "search-meilisearch.invalid_sort",
                format!("`{}` is not sortable", sp.field),
            ));
        }
        sort.push(format!(
            "{}:{}",
            sp.field,
            if sp.descending { "desc" } else { "asc" }
        ));
    }
    for f in &req.facets {
        if !m.field(f).is_some_and(|x| x.filterable && !x.ty.numeric()) {
            return Err(err422(
                "search-meilisearch.field_not_filterable",
                format!("`{f}` is not facetable"),
            ));
        }
    }
    let limit = u64::from(req.limit.unwrap_or(20).clamp(1, cfg.max_limit));
    let filters = final_filters(ctx, &row, &viewer, &req.query, user, sort.clone()).await?;
    let q = pt_release_parser::normalize_title(&req.query);
    let strict = req.query.matches('.').count() >= 3;
    let mut hl: Vec<String> = m.title_fields.clone();
    hl.extend(m.title_fields.iter().map(|t| format!("{t}_norm")));
    let mut body = json!({
        "q": q, "filter": filters, "limit": limit, "facets": req.facets,
        "attributesToHighlight": hl, "highlightPreTag": HL_PRE.to_string(), "highlightPostTag": HL_POST.to_string(),
        "matchingStrategy": if strict { "all" } else { "last" },
    });
    if !sort.is_empty() {
        body["sort"] = json!(sort);
    }
    if req.collapse
        && let Some(d) = &row.distinct_attr
    {
        body["distinct"] = json!(d);
    }
    // 游标：offset + 请求指纹（去掉 offset）
    let fp = sha_hex(&body.to_string())[..8].to_owned();
    let offset = match &req.cursor {
        None => 0,
        Some(cur) => {
            let c: Cursor = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(cur)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .ok_or_else(|| PluginError::bad_request("invalid cursor"))?;
            if c.h != fp || c.o + limit > cfg.max_total_hits {
                return Err(PluginError::bad_request("cursor does not match this query"));
            }
            c.o
        }
    };
    body["offset"] = json!(offset);
    // 结果缓存：键含最终请求（自带权限范围）与缓存代数
    let gen_key = format!("qcgen:{}", row.name);
    let gen_no = match ctx.redis.call(RedisCommand::new("GET").arg(&gen_key)).await {
        Ok(RedisReply::Bulk(b)) => String::from_utf8_lossy(&b).into_owned(),
        _ => "0".into(),
    };
    let cache_key = format!(
        "qc:{}:{}:{gen_no}:{}",
        row.name,
        row.generation,
        sha_hex(&body.to_string())
    );
    if cfg.cache_ttl_secs > 0
        && let Ok(RedisReply::Bulk(b)) = ctx.redis.call(RedisCommand::new("GET").arg(&cache_key)).await
        && let Ok(mut v) = serde_json::from_slice::<Value>(&b)
    {
        v["cached"] = json!(true);
        return Ok(v);
    }
    let meili = s.require_meili()?;
    let resp = meili.search(&row.live_uid, &body).await?;
    let hits: Vec<Value> = resp["hits"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|h| {
                    let mut doc = h.clone();
                    let formatted = doc.as_object_mut().and_then(|o| o.remove("_formatted")).unwrap_or(Value::Null);
                    let mut highlights = serde_json::Map::new();
                    for f in &hl {
                        if let Some(t) = formatted[f].as_str()
                            && t.contains(HL_PRE)
                        {
                            highlights.insert(f.clone(), json!(t));
                        }
                    }
                    if let Some(o) = doc.as_object_mut() {
                        o.retain(|k, _| !k.starts_with('_'));
                    }
                    json!({ "id": doc["id"], "document": doc, "highlights": highlights, "ranking_score": null })
                })
                .collect()
        })
        .unwrap_or_default();
    let facets: Vec<Value> = resp["facetDistribution"]
        .as_object()
        .map(|o| {
            o.iter()
                .map(|(f, d)| {
                    let mut buckets: Vec<(String, u64)> = d
                        .as_object()
                        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_u64().unwrap_or(0))).collect())
                        .unwrap_or_default();
                    buckets.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                    json!({ "field": f, "buckets": buckets.iter().map(|(v, c)| json!({ "value": v, "count": c })).collect::<Vec<_>>() })
                })
                .collect()
        })
        .unwrap_or_default();
    let total = resp["estimatedTotalHits"].as_u64().unwrap_or(0);
    let next = offset + limit;
    let next_cursor = (next < total.min(cfg.max_total_hits)).then(|| {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json!({ "o": next, "h": fp }).to_string())
    });
    let out = json!({
        "hits": hits, "facets": facets, "estimated_total": total, "next_cursor": next_cursor,
        "processing_ms": resp["processingTimeMs"].as_u64().unwrap_or(0), "cached": false,
    });
    let text = out.to_string();
    if cfg.cache_ttl_secs > 0 && text.len() <= 256 * 1024 {
        let _ = ctx
            .redis
            .call(
                RedisCommand::new("SET")
                    .arg(&cache_key)
                    .arg(&text)
                    .arg("EX")
                    .arg(cfg.cache_ttl_secs.to_string()),
            )
            .await;
    }
    Ok(out)
}

async fn suggest(s: &Search, ctx: &Ctx, index: &str, prefix: &str, limit: u32) -> PResult<Value> {
    if prefix.chars().count() > 64 {
        return Err(err422(
            "search-meilisearch.query_too_long",
            "prefix is longer than 64 characters",
        ));
    }
    let row = require_index(ctx, index).await?;
    let viewer = viewer_of(ctx).await?;
    if row.audience == "admin"
        && !matches!(
            viewer,
            Viewer::System
                | Viewer::User {
                    admin_index: true,
                    ..
                }
        )
    {
        return Err(PluginError::forbidden(
            "this index needs search-meilisearch.admin_index.read",
        ));
    }
    let Some(title) = row.merged.title_fields.first().cloned() else {
        return Ok(json!({ "items": [] }));
    };
    let filters = final_filters(ctx, &row, &viewer, prefix, vec![], vec![]).await?;
    let norm = format!("{title}_norm");
    let body = json!({
        "q": pt_release_parser::normalize_title(prefix), "limit": limit.clamp(1, 10), "filter": filters,
        "attributesToSearchOn": [title, norm], "attributesToRetrieve": ["id", title],
        "attributesToHighlight": [title], "highlightPreTag": HL_PRE.to_string(), "highlightPostTag": HL_POST.to_string(),
    });
    let resp = s.require_meili()?.search(&row.live_uid, &body).await?;
    let items: Vec<Value> = resp["hits"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|h| json!({ "id": h["id"], "text": h[&title], "highlight": h["_formatted"][&title] }))
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({ "items": items }))
}

/// 合并后的字段视图（筛选面板）
fn schema_view(row: &IndexRow) -> Value {
    let fields: Vec<Value> = row
        .merged
        .fields
        .values()
        .map(|f| {
            let mut ops = vec!["eq", "ne", "in", "not_in", "exists", "is_null"];
            if f.ty.numeric() {
                ops.extend(["gt", "gte", "lt", "lte", "between"]);
            } else {
                ops.push("is_empty");
            }
            json!({
                "name": f.name, "field_type": f.ty.as_str(), "searchable": f.searchable, "filterable": f.filterable,
                "sortable": f.sortable, "facetable": f.filterable && !f.ty.numeric() && !f.label_term.is_empty(),
                "ops": if f.filterable { ops } else { vec![] }, "label_term": f.label_term,
                "source_plugin": f.source_plugin, "generic": f.generic,
            })
        })
        .collect();
    json!({
        "index": row.name, "audience": row.audience, "status": row.status, "fields": fields,
        "distinct_attribute": row.distinct_attr, "semantic_available": false,
    })
}

// ---------------- 导出 ----------------

#[async_trait]
impl Lifecycle for Search {
    async fn init(&self, ctx: &Ctx) -> PResult<()> {
        let cfg: Config = serde_json::from_value(ctx.config.get_static())
            .map_err(|e| PluginError::validation(format!("site.toml [plugin.search-meilisearch]: {e}")))?;
        let valid_prefix = cfg.index_prefix.len() <= 16
            && cfg.index_prefix.starts_with(|c: char| c.is_ascii_lowercase())
            && cfg
                .index_prefix
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        if !valid_prefix {
            return Err(PluginError::validation(
                "index_prefix must match ^[a-z][a-z0-9]{0,15}$",
            ));
        }
        if cfg.instances.len() > 1 {
            return Err(PluginError::validation(
                "only the single-instance mode is supported in this version",
            ));
        }
        let meili = match cfg.instances.first() {
            Some(i) => Some(Meili::new(
                &i.id,
                &i.url,
                &i.api_key,
                cfg.request_timeout_ms,
                cfg.write_timeout_ms,
            )?),
            None => {
                tracing::warn!("search-meilisearch: no instance configured; search is unavailable");
                None
            }
        };
        let _ = self.0.cfg.set(cfg);
        let _ = self.0.meili.set(meili);
        refresh_schemas(self, ctx).await
    }
}

#[async_trait]
impl OnEvent for Search {
    async fn handle(&self, ctx: &Ctx, evt: EventEnvelope, _d: DeliveryInfo) -> PResult<()> {
        let p = &evt.payload;
        let tid = ids_of(&p["torrent_id"]);
        let ev = Some(evt.id);
        match evt.event_type.as_str() {
            "torrent.created" | "torrent.updated" | "torrent.tags_changed" => {
                mark(self, ctx, "torrents", &tid, "upsert", "normal", ev).await
            }
            "torrent.status_changed" => {
                let hidden = matches!(
                    p["to"].as_str(),
                    Some("rejected" | "disabled" | "deleted" | "candidate" | "pending")
                ) || p["from"].as_str() == Some("approved");
                mark(
                    self,
                    ctx,
                    "torrents",
                    &tid,
                    "upsert",
                    if hidden { "priority" } else { "normal" },
                    ev,
                )
                .await
            }
            "torrent.deleted" => mark(self, ctx, "torrents", &tid, "delete", "priority", ev).await,
            "promotion.started" | "promotion.ended" if p["scope"].as_str() != Some("global") => {
                mark(
                    self,
                    ctx,
                    "torrents",
                    &ids_of(&p["torrent_ids"]),
                    "upsert",
                    "normal",
                    ev,
                )
                .await
            }
            _ => Ok(()),
        }
    }
}

#[async_trait]
impl OnCron for Search {
    async fn run(&self, ctx: &Ctx, job: JobInvocation) -> PResult<()> {
        let index = job.payload["index"].as_str().unwrap_or("").to_owned();
        match job.name.as_str() {
            "flush" => {
                let lane = job.payload["lane"].as_str().unwrap_or("normal");
                flush(self, ctx, &index, lane).await.map(|_| ())
            }
            "flush_sweeper" => {
                let mut c = ctx.sql.conn().await?;
                let rows = sqlx::query(
                    "SELECT DISTINCT index_name, lane FROM pending_ops WHERE enqueued_at < now() - interval '10 seconds'",
                )
                .fetch_all(&mut *c)
                .await?;
                drop(c);
                for r in rows {
                    let (i, l): (String, String) = (r.get(0), r.get(1));
                    flush(self, ctx, &i, &l).await?;
                }
                Ok(())
            }
            "settings_apply" => settings_apply(self, ctx, &index).await,
            "rebuild_step" => {
                let id = job.payload["rebuild_id"]
                    .as_str()
                    .and_then(|s| uuid::Uuid::parse_str(s).ok())
                    .ok_or_else(|| PluginError::validation("rebuild_id is required"))?;
                rebuild_step(self, ctx, id).await
            }
            "task_reaper" => reap_tasks(self, ctx).await,
            _ => Err(PluginError::not_found(format!("unknown job `{}`", job.name))),
        }
    }
}

/// `ctx.search`（JSON 分派，LLD §2.1）
#[async_trait]
impl pt_plugin_api::JsonService for Search {
    async fn call(&self, ctx: &Ctx, method: &str, a: Value) -> PResult<Value> {
        let index = a["index"].as_str().unwrap_or("").to_owned();
        let may_write = |row: &IndexRow| {
            ctx.invocation.caller_scopes.iter().any(|s| s == "admin")
                || row.merged.contributors.contains(&ctx.invocation.caller_plugin)
        };
        match method {
            "query" => {
                let req: SearchRequest = serde_json::from_value(a)
                    .map_err(|e| PluginError::validation(format!("search request: {e}")))?;
                query(self, ctx, req).await
            }
            "suggest" => {
                let limit = u32::try_from(a["limit"].as_u64().unwrap_or(8)).unwrap_or(8);
                suggest(self, ctx, &index, a["prefix"].as_str().unwrap_or(""), limit).await
            }
            "schema" => Ok(schema_view(&require_index(ctx, &index).await?)),
            "touch" | "remove" => {
                let row = require_index(ctx, &index).await?;
                if !may_write(&row) {
                    return Err(PluginError::new(
                        "search-meilisearch.index_owner_mismatch",
                        "caller is not a contributor of this index",
                    ));
                }
                let ids = if method == "touch" {
                    ids_of(&a["ids"])
                } else {
                    ids_of(&a["id"])
                };
                if ids.is_empty() || ids.len() > 10_000 {
                    return Err(PluginError::validation("1..=10000 ids are required"));
                }
                let (op, lane) = if method == "touch" {
                    ("upsert", "normal")
                } else {
                    ("delete", "priority")
                };
                mark(self, ctx, &index, &ids, op, lane, None).await?;
                Ok(Value::Null)
            }
            "reindex" => {
                let row = require_index(ctx, &index).await?;
                if !(ctx.invocation.caller_scopes.iter().any(|s| s == "admin")
                    || row.owner == ctx.invocation.caller_plugin)
                {
                    return Err(PluginError::new(
                        "search-meilisearch.index_owner_mismatch",
                        "reindex needs the admin scope or index ownership",
                    ));
                }
                let (id, _) = start_rebuild(ctx, &row, "manual", None, None).await?;
                Ok(json!(id))
            }
            _ => Err(PluginError::not_found(format!("unknown method `{method}`"))),
        }
    }
}

// ---------------- HTTP（LLD §2.4） ----------------

async fn http_search(
    AxState(s): AxState<Search>,
    ctx: RequestCtx,
    Json(req): Json<SearchRequest>,
) -> PResult<Response> {
    Ok(Json(query(&s, &ctx, req).await?).into_response())
}

/// GET 形式（conventions/api.md §6）：`index`、`q`、`sort=-created_at`、`limit`、`cursor`、`collapse`，
/// 其余参数为过滤：`type_id=movie,tv`、`size_gte=1`
async fn http_search_get(
    AxState(s): AxState<Search>,
    ctx: RequestCtx,
    Query(params): Query<Vec<(String, String)>>,
) -> PResult<Response> {
    let mut req = SearchRequest::default();
    for (k, v) in params {
        match k.as_str() {
            "index" => req.index = v,
            "q" => req.query = v,
            "limit" => req.limit = v.parse().ok(),
            "cursor" => req.cursor = Some(v),
            "collapse" => req.collapse = v == "true" || v == "1",
            "semantic" => req.semantic = v == "true" || v == "1",
            "facets" => req.facets = v.split(',').map(str::to_owned).collect(),
            "sort" => {
                for s in v.split(',').filter(|s| !s.is_empty()) {
                    let (field, descending) = match s.strip_prefix('-') {
                        Some(f) => (f.to_owned(), true),
                        None => (s.to_owned(), false),
                    };
                    req.sort.push(SortSpec { field, descending });
                }
            }
            _ => {
                let (field, op) = ["_gte", "_gt", "_lte", "_lt", "_ne"]
                    .iter()
                    .find_map(|sfx| k.strip_suffix(sfx).map(|f| (f.to_owned(), sfx[1..].to_owned())))
                    .unwrap_or_else(|| (k.clone(), "in".into()));
                let values: Vec<Value> = v.split(',').map(|x| json!(x)).collect();
                let op = if op == "in" && values.len() == 1 {
                    "eq".into()
                } else {
                    op
                };
                req.filters.push(Filter { field, op, values });
            }
        }
    }
    Ok(Json(query(&s, &ctx, req).await?).into_response())
}

#[derive(Debug, Deserialize)]
struct SuggestQuery {
    index: String,
    prefix: String,
    limit: Option<u32>,
}

async fn http_suggest(
    AxState(s): AxState<Search>,
    ctx: RequestCtx,
    Query(q): Query<SuggestQuery>,
) -> PResult<Response> {
    Ok(Json(suggest(&s, &ctx, &q.index, &q.prefix, q.limit.unwrap_or(8)).await?).into_response())
}

async fn visible_indexes(ctx: &Ctx) -> PResult<Vec<IndexRow>> {
    let mut out = Vec::new();
    for si in STANDARD {
        if let Some(r) = load_index(ctx, si.name, false).await?
            && r.status != "waiting_owner"
            && r.status != "disabled"
            && r.audience == "user"
        {
            out.push(r);
        }
    }
    Ok(out)
}

async fn http_indexes(ctx: RequestCtx) -> PResult<Response> {
    let items: Vec<Value> = visible_indexes(&ctx).await?.iter().map(schema_view).collect();
    Ok(Json(json!({ "items": items })).into_response())
}

async fn http_schema(ctx: RequestCtx, Path(index): Path<String>) -> PResult<Response> {
    let row = require_index(&ctx, &index).await?;
    if row.audience == "admin" {
        return Err(PluginError::new(
            "search-meilisearch.index_not_found",
            "index not found",
        ));
    }
    let etag = format!("\"{}\"", row.settings_hash.clone().unwrap_or_default());
    let mut resp = Json(schema_view(&row)).into_response();
    if let Ok(v) = axum::http::HeaderValue::from_str(&etag) {
        resp.headers_mut().insert(axum::http::header::ETAG, v);
    }
    resp.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("private, max-age=60"),
    );
    Ok(resp)
}

async fn http_admin_indexes(ctx: RequestCtx) -> PResult<Response> {
    let mut items = Vec::new();
    for si in STANDARD {
        let Some(r) = load_index(&ctx, si.name, false).await? else {
            continue;
        };
        let mut c = ctx.sql.conn().await?;
        let p = sqlx::query(
            "SELECT count(*) FILTER (WHERE lane = 'normal'), count(*) FILTER (WHERE lane = 'priority'),
                    coalesce((extract(epoch FROM now() - min(first_enqueued_at)) * 1000)::bigint, 0)
               FROM pending_ops WHERE index_name = $1",
        )
        .bind(&r.name)
        .fetch_one(&mut *c)
        .await?;
        let docs: i64 = sqlx::query_scalar("SELECT count(*) FROM indexed_docs WHERE index_name = $1")
            .bind(&r.name)
            .fetch_one(&mut *c)
            .await?;
        drop(c);
        items.push(json!({
            "index": r.name, "owner": r.owner, "audience": r.audience, "status": r.status, "live_uid": r.live_uid,
            "generation": r.generation, "documents": docs, "pending_normal": p.get::<i64, _>(0),
            "pending_priority": p.get::<i64, _>(1), "oldest_pending_age_ms": p.get::<i64, _>(2),
            "settings_hash": r.settings_hash, "shape_hash": r.shape_hash, "conflicts": r.conflicts,
            "contributors": r.merged.contributors.iter().map(|c| json!({ "plugin": c, "fields": r.merged.fields_of(c) })).collect::<Vec<_>>(),
        }));
    }
    Ok(Json(json!({ "items": items })).into_response())
}

#[derive(Debug, Default, Deserialize)]
struct RebuildBody {
    #[serde(default)]
    reason: Option<String>,
}

async fn http_create_rebuild(
    AxState(s): AxState<Search>,
    ctx: RequestCtx,
    Path(index): Path<String>,
    body: Option<Json<RebuildBody>>,
) -> PResult<Response> {
    s.require_meili()?;
    let row = require_index(&ctx, &index).await?;
    let reason = body.and_then(|b| b.0.reason).filter(|r| r.chars().count() <= 500);
    let by = ctx.user_id().and_then(|u| i64::try_from(u).ok());
    let (id, created) = start_rebuild(&ctx, &row, "manual", by, reason.as_deref()).await?;
    if !created {
        return Err(PluginError::new(
            "search-meilisearch.rebuild_in_progress",
            "a rebuild is already running",
        )
        .with_details(json!({ "rebuild_id": id })));
    }
    ctx.audit
        .write(pt_plugin_api::AuditEntry {
            action: "search-meilisearch.rebuild".into(),
            target: format!("index:{index}"),
            details: json!({ "rebuild_id": id, "reason": reason }),
        })
        .await?;
    let mut resp = (
        StatusCode::ACCEPTED,
        Json(json!({ "id": id, "state": "preparing" })),
    )
        .into_response();
    if let Ok(v) =
        axum::http::HeaderValue::from_str(&format!("/api/p/search-meilisearch/v1/admin/rebuilds/{id}"))
    {
        resp.headers_mut().insert(axum::http::header::LOCATION, v);
    }
    Ok(resp)
}

async fn http_get_rebuild(ctx: RequestCtx, Path(id): Path<uuid::Uuid>) -> PResult<Response> {
    let mut c = ctx.sql.conn().await?;
    let r = sqlx::query(
        "SELECT index_name, state, from_generation, expected_total, done_count, started_at, finished_at, error, trigger
           FROM rebuilds WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *c)
    .await?
    .ok_or_else(|| PluginError::not_found("rebuild not found"))?;
    type Ts = chrono::DateTime<chrono::Utc>;
    Ok(Json(json!({
        "id": id, "index": r.get::<String, _>(0), "state": r.get::<String, _>(1), "generation": r.get::<i32, _>(2),
        "total": r.get::<Option<i64>, _>(3), "done": r.get::<i64, _>(4), "started_at": r.get::<Ts, _>(5).to_rfc3339(),
        "finished_at": r.get::<Option<Ts>, _>(6).map(|d| d.to_rfc3339()), "error": r.get::<Option<String>, _>(7),
        "trigger": r.get::<String, _>(8),
    }))
    .into_response())
}

async fn http_cancel_rebuild(
    AxState(s): AxState<Search>,
    ctx: RequestCtx,
    Path(id): Path<uuid::Uuid>,
) -> PResult<Response> {
    cancel_rebuild(&s, &ctx, id).await?;
    Ok(StatusCode::ACCEPTED.into_response())
}

struct Routes(Search);

impl pt_plugin_api::HttpRoutes for Routes {
    fn router(&self) -> Router {
        Router::new()
            .route("/v1/search", post(http_search).get(http_search_get))
            .route("/v1/suggest", get(http_suggest))
            .route("/v1/indexes", get(http_indexes))
            .route("/v1/indexes/{index}/schema", get(http_schema))
            .route("/v1/admin/indexes", get(http_admin_indexes))
            .route("/v1/admin/indexes/{index}/rebuilds", post(http_create_rebuild))
            .route("/v1/admin/rebuilds/{id}", get(http_get_rebuild))
            .route("/v1/admin/rebuilds/{id}/cancellations", post(http_cancel_rebuild))
            .with_state(self.0.clone())
    }
}

pub fn plugin() -> NativePlugin {
    let s = Search(Arc::new(State::default()));
    let svc: Arc<dyn pt_plugin_api::JsonService> = Arc::new(s.clone());
    NativePlugin {
        manifest: MANIFEST,
        migrations: MIGRATIONS,
        lifecycle: Some(Arc::new(s.clone())),
        on_event: Some(Arc::new(s.clone())),
        on_cron: Some(Arc::new(s.clone())),
        http: Some(Arc::new(Routes(s))),
        json_services: vec![("ctx.search", svc)],
        ..Default::default()
    }
}
