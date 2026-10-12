//! torrent 插件（docs/plugins/torrent/）：上传、规范化、存储与下载 .torrent 文件。
//!
//! 本版本实现 P1 子集：上传（含 `torrent.before_upload`）、详情、文件列表、下载（注入 passkey、
//! `torrent.before_download`）。资源类型插件、Group 编辑、状态机、签名链接、批量下载在后续迭代实现。

pub mod bencode;
pub mod fixtures;
pub mod meta;
pub mod search;

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::{Multipart, Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use pt_plugin_api::{
    Ctx, HttpRoutes, Lifecycle, NativePlugin, NewEvent, PResult, PluginError, RequestCtx,
    host::HookOutcome,
    json,
    services::{RBAC, RbacService},
};
use pt_plugin_identity::api::{USERS, UsersService};
use serde::Deserialize;
use sqlx::Row;

use crate::meta::{HybridPolicy, Policy};

pub const MANIFEST: &str = include_str!("../plugin.toml");
pub const MIGRATIONS: &[(&str, &str)] = &[
    ("0001_init.sql", include_str!("../migrations/0001_init.sql")),
    ("0002_groups.sql", include_str!("../migrations/0002_groups.sql")),
];

/// 静态配置（`site.toml [plugin.torrent]`，03 §4；运行时设置在后台接入前同样取自这里）。
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub announce_base_urls: Vec<String>,
    #[serde(default = "default_source")]
    pub source_tag: String,
    #[serde(default = "default_types")]
    pub types: Vec<String>,
    #[serde(default = "default_status")]
    pub default_initial_status: String,
    #[serde(default)]
    pub reject_non_private: bool,
    #[serde(default = "default_hybrid")]
    pub hybrid_policy: String,
    #[serde(default)]
    pub accept_v2_only: bool,
    #[serde(default = "default_max_kb")]
    pub max_torrent_kb: usize,
    #[serde(default = "default_max_files")]
    pub max_files: usize,
    #[serde(default = "default_true")]
    pub allow_anonymous: bool,
    #[serde(default = "default_source")]
    pub created_by: String,
    #[serde(default = "default_comment")]
    pub comment_template: String,
}

fn default_source() -> String {
    "PT Studio".into()
}
fn default_types() -> Vec<String> {
    vec!["generic".into()]
}
fn default_status() -> String {
    "approved".into()
}
fn default_hybrid() -> String {
    "keep_hybrid".into()
}
fn default_max_kb() -> usize {
    10_240
}
fn default_max_files() -> usize {
    20_000
}
fn default_true() -> bool {
    true
}
fn default_comment() -> String {
    "torrent #{id}".into()
}

impl Config {
    fn policy(&self) -> Policy {
        Policy {
            hybrid: match self.hybrid_policy.as_str() {
                "strip_v2" => HybridPolicy::StripV2,
                "reject" => HybridPolicy::Reject,
                _ => HybridPolicy::KeepHybrid,
            },
            accept_v2_only: self.accept_v2_only,
            reject_non_private: self.reject_non_private,
            source_tag: self.source_tag.clone(),
            max_files: self.max_files,
        }
    }
}

/// (插件 ID, TypeInfo) 列表
pub type TypeList = Arc<Vec<(String, pt_plugin_api::TypeInfo)>>;

#[derive(Debug, Default)]
pub struct Torrent {
    cfg: OnceLock<Config>,
    /// 资源类型信息缓存：(插件 ID, TypeInfo)，30 s 刷新（type-generic 的 kind 可在运行时变化）
    types: tokio::sync::Mutex<Option<(std::time::Instant, TypeList)>>,
}

/// Group 字段：只保留 group_schema 中声明的属性
fn group_fields(schema: Option<&serde_json::Value>, fields: &serde_json::Value) -> serde_json::Value {
    let Some(props) = schema
        .and_then(|s| s.get("properties"))
        .and_then(serde_json::Value::as_object)
    else {
        return json!({});
    };
    let mut out = serde_json::Map::new();
    for k in props.keys() {
        if let Some(v) = fields.get(k).filter(|v| !v.is_null()) {
            out.insert(k.clone(), v.clone());
        }
    }
    serde_json::Value::Object(out)
}

impl Torrent {
    /// 已加载的资源类型（插件 ID, TypeInfo）
    pub async fn types(&self, ctx: &Ctx) -> PResult<TypeList> {
        let mut g = self.types.lock().await;
        if let Some((at, v)) = g.as_ref()
            && at.elapsed() < std::time::Duration::from_secs(30)
        {
            return Ok(v.clone());
        }
        let all: Vec<_> = ctx
            .content_types()
            .await?
            .into_iter()
            .filter(|(_, i)| !i.type_id.is_empty())
            .collect();
        let v = Arc::new(all);
        *g = Some((std::time::Instant::now(), v.clone()));
        Ok(v)
    }

    async fn type_provider(
        &self,
        ctx: &Ctx,
        type_id: &str,
    ) -> PResult<Option<(String, pt_plugin_api::TypeInfo)>> {
        Ok(self
            .types(ctx)
            .await?
            .iter()
            .find(|(_, i)| i.type_id == type_id)
            .cloned())
    }

    fn cfg(&self) -> PResult<&Config> {
        self.cfg
            .get()
            .ok_or_else(|| PluginError::unavailable("torrent is not initialised"))
    }
}

#[async_trait]
impl Lifecycle for Torrent {
    async fn init(&self, ctx: &Ctx) -> PResult<()> {
        let cfg: Config = serde_json::from_value(ctx.config.get_static())
            .map_err(|e| PluginError::validation(format!("site.toml [plugin.torrent]: {e}")))?;
        if cfg.announce_base_urls.is_empty() || cfg.announce_base_urls.len() > 4 {
            return Err(PluginError::validation(
                "[plugin.torrent] announce_base_urls must have 1-4 entries",
            ));
        }
        for u in &cfg.announce_base_urls {
            if !(u.starts_with("http://") || u.starts_with("https://")) || !u.ends_with("/announce") {
                return Err(PluginError::validation(format!(
                    "announce url `{u}` must be http(s)://…/announce"
                )));
            }
        }
        let _ = self.cfg.set(cfg);
        Ok(())
    }
}

// ---------------- 可见性（03 §1.9） ----------------

#[derive(Debug, Clone)]
struct Row0 {
    id: i64,
    group_id: Option<i64>,
    type_id: String,
    title: String,
    subtitle: Option<String>,
    description: String,
    fields: serde_json::Value,
    size: i64,
    file_count: i32,
    meta_version: i16,
    ih_v1: Option<Vec<u8>>,
    ih_v2: Option<Vec<u8>>,
    file_version: i32,
    blob_key: String,
    uploader_id: Option<i64>,
    anonymous: bool,
    status: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

async fn load(ctx: &Ctx, id: i64) -> PResult<Option<Row0>> {
    let mut c = ctx.sql.conn().await?;
    let r = sqlx::query(
        "SELECT id, group_id, type_id, title, subtitle, description, fields, size, file_count, meta_version,
                info_hash_v1, info_hash_v2, file_version, blob_key, uploader_id, is_anonymous, status, created_at
           FROM torrents WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut *c)
    .await?;
    Ok(r.map(|r| Row0 {
        id: r.get(0),
        group_id: r.get(1),
        type_id: r.get(2),
        title: r.get(3),
        subtitle: r.get(4),
        description: r.get(5),
        fields: r.get(6),
        size: r.get(7),
        file_count: r.get(8),
        meta_version: r.get(9),
        ih_v1: r.get(10),
        ih_v2: r.get(11),
        file_version: r.get(12),
        blob_key: r.get(13),
        uploader_id: r.get(14),
        anonymous: r.get(15),
        status: r.get(16),
        created_at: r.get(17),
    }))
}

async fn has(ctx: &Ctx, perm: &str) -> PResult<bool> {
    let Some(uid) = ctx.user_id() else {
        return Ok(false);
    };
    let rbac = ctx.service::<dyn RbacService>(RBAC)?;
    let perm = perm.to_owned();
    rbac.call(move |s, c| Box::pin(async move { s.check(c, uid, &perm).await }))
        .await
}

fn is_owner(ctx: &Ctx, t: &Row0) -> bool {
    ctx.user_id()
        .and_then(|u| i64::try_from(u).ok())
        .is_some_and(|u| Some(u) == t.uploader_id)
}

async fn can_view(ctx: &Ctx, t: &Row0) -> PResult<bool> {
    Ok(match t.status.as_str() {
        "approved" | "disabled" => has(ctx, "torrent.download").await?,
        "candidate" | "pending" | "rejected" => is_owner(ctx, t) || has(ctx, "torrent.update").await?,
        _ => has(ctx, "torrent.delete").await?,
    })
}

fn can_download(ctx: &Ctx, t: &Row0) -> bool {
    t.status == "approved" || (matches!(t.status.as_str(), "candidate" | "pending") && is_owner(ctx, t))
}

async fn reveal_uploader(ctx: &Ctx, t: &Row0) -> PResult<bool> {
    Ok(!t.anonymous || is_owner(ctx, t) || has(ctx, "torrent.anonymous.reveal").await?)
}

// ---------------- 上传（03 §1.4） ----------------

#[derive(Debug, Clone, Deserialize)]
pub struct UploadMeta {
    pub type_id: String,
    #[serde(default)]
    pub fields: serde_json::Value,
    /// 为空时用资源类型的规范标题
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub mediainfo: Option<String>,
    #[serde(default)]
    pub anonymous: bool,
}

fn sanitize_title(s: &str) -> PResult<String> {
    let t: String = s
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_owned();
    if t.is_empty() || t.chars().count() > 300 {
        return Err(PluginError::validation("title must be 1-300 characters").with_details(json!({ "errors": [
            { "path": "/title", "code": "kernel.validation_failed", "message": "title must be 1-300 characters" }
        ]})));
    }
    Ok(t)
}

pub async fn upload(t: &Torrent, ctx: &Ctx, m: UploadMeta, file: bytes::Bytes) -> PResult<serde_json::Value> {
    let cfg = t.cfg()?;
    let uploader = i64::try_from(ctx.require_user()?).map_err(|_| PluginError::bad_request("bad user id"))?;
    if file.len() > cfg.max_torrent_kb * 1024 {
        return Err(PluginError::new("torrent.too_large", "torrent file is too large").with_status(413));
    }
    let (parsed, norm) = meta::process(&file, &cfg.policy())?;
    let provider = t.type_provider(ctx, &m.type_id).await?;
    if provider.is_none() && !cfg.types.iter().any(|x| x == &m.type_id) {
        return Err(PluginError::new(
            "torrent.type_unknown",
            format!("unknown type `{}`", m.type_id),
        ));
    }
    let mut fields = if m.fields.is_null() {
        json!({})
    } else {
        m.fields.clone()
    };
    if !fields.is_object() {
        return Err(PluginError::validation("fields must be an object"));
    }
    if m.anonymous && !cfg.allow_anonymous {
        return Err(PluginError::validation("anonymous uploads are disabled"));
    }
    // 资源类型（content-types-common §2）：规范化字段（发布名默认取标题）、规范标题、分组键
    let mut group_key: Option<String> = None;
    let mut title_in = m.title.trim().to_owned();
    if let Some((p, _)) = &provider {
        if let (Some(o), false) = (fields.as_object_mut(), title_in.is_empty()) {
            o.entry("release_name").or_insert_with(|| json!(title_in));
        }
        fields = ctx.content_type_call(p, "normalize", fields).await?;
        if title_in.is_empty() {
            title_in = ctx
                .content_type_call(p, "canonical_title", fields.clone())
                .await?
                .as_str()
                .unwrap_or_default()
                .to_owned();
        }
        group_key = ctx
            .content_type_call(p, "group_key", fields.clone())
            .await?
            .as_str()
            .map(str::to_owned);
    }
    let title = sanitize_title(&title_in)?;
    let ih_v1 = norm.ih_v1.map(|h| h.to_vec());
    let ih_v2 = norm.ih_v2.map(|h| h.to_vec());
    {
        let mut c = ctx.sql.conn().await?;
        let dup: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM torrents WHERE deleted_at IS NULL AND (info_hash_v1 = $1 OR info_hash_v2 = $2) LIMIT 1",
        )
        .bind(&ih_v1)
        .bind(&ih_v2)
        .fetch_optional(&mut *c)
        .await?;
        if let Some(d) = dup {
            return Err(
                PluginError::new("torrent.duplicate", "this torrent already exists")
                    .with_details(json!({ "torrent_id": d })),
            );
        }
    }
    // 已有 Group（catalog/hooks.md `group_id?`、`group_fields?`）：供类型插件按作品元数据检查（如季包集数）。
    // 钩子内不能回调 ctx.torrents（重入），所以直接带上字段
    let existing_group: Option<(i64, serde_json::Value)> = match &group_key {
        Some(k) => {
            let mut c = ctx.sql.conn().await?;
            sqlx::query_as(
                "SELECT id, fields FROM groups WHERE type_id = $1 AND group_key = $2 AND deleted_at IS NULL AND merged_into IS NULL",
            )
            .bind(&m.type_id)
            .bind(k)
            .fetch_optional(&mut *c)
            .await?
        }
        None => None,
    };
    let input = json!({
        "version": 1, "actor": ctx.invocation.actor, "uploader_id": uploader, "type_id": m.type_id,
        "group_id": existing_group.as_ref().map(|g| g.0), "group_fields": existing_group.map(|g| g.1),
        "title": title, "size": parsed.total_size, "file_count": parsed.files.len(),
        "files_sample": parsed.files.iter().take(200).map(|f| json!({ "path": f.path.join("/"), "size": f.size })).collect::<Vec<_>>(),
        "info_hash_v1": ih_v1.as_ref().map(hex::encode), "info_hash_v2": ih_v2.as_ref().map(hex::encode),
        "fields": fields, "anonymous": m.anonymous, "nfo_present": false,
        "content_fp": parsed.pieces_sha1.map(hex::encode), "meta_version": norm.meta as i16,
    });
    let out = match ctx.hooks.run("torrent.before_upload", input).await? {
        HookOutcome::Denied {
            code,
            message,
            details,
            ..
        } => {
            let mut e = PluginError::new(code, message);
            e.details = details;
            return Err(e);
        }
        HookOutcome::Allowed(v) => v,
    };
    let title = out
        .get("title")
        .and_then(serde_json::Value::as_str)
        .map_or(Ok(title.clone()), sanitize_title)?;
    let fields = out.get("fields").cloned().unwrap_or(fields);
    let status = out
        .get("initial_status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&cfg.default_initial_status)
        .to_owned();
    if !matches!(status.as_str(), "candidate" | "pending" | "approved") {
        return Err(PluginError::internal("invalid initial status"));
    }
    let key = format!(
        "torrents/{}.torrent",
        hex::encode(
            ih_v1
                .as_ref()
                .or(ih_v2.as_ref())
                .ok_or_else(|| meta::invalid("no_hash"))?
        )
    );
    ctx.blob
        .put(&key, "application/x-bittorrent", norm.stored.clone().into())
        .await?;

    let mut c = ctx.sql.conn().await?;
    let mut new_group = false;
    let group_id: Option<i64> = match &group_key {
        Some(k) if k.chars().count() <= 256 => {
            // 资源类型可给出规范作品标题（如剧集“剧名 第 N 季”）
            let gtitle = fields
                .get("group_title")
                .or_else(|| fields.get("title"))
                .and_then(serde_json::Value::as_str)
                .map_or_else(|| title.clone(), |s| s.chars().take(300).collect());
            let year = fields
                .get("year")
                .or_else(|| fields.get("first_air_year"))
                .and_then(serde_json::Value::as_i64)
                .and_then(|y| i32::try_from(y).ok())
                .filter(|y| (1800..=2200).contains(y));
            let gfields = group_fields(
                provider.as_ref().and_then(|(_, i)| i.group_schema.as_ref()),
                &fields,
            );
            let r: (i64, bool) = sqlx::query_as(
                "INSERT INTO groups (type_id, group_key, title, year, fields, torrent_count, last_upload_at)
                 VALUES ($1, $2, $3, $4, $5, 1, now())
                 ON CONFLICT (type_id, group_key) WHERE deleted_at IS NULL AND merged_into IS NULL
                 DO UPDATE SET torrent_count = groups.torrent_count + 1, last_upload_at = now(), updated_at = now()
                 RETURNING id, (xmax = 0)",
            )
            .bind(&m.type_id)
            .bind(k)
            .bind(&gtitle)
            .bind(year)
            .bind(&gfields)
            .fetch_one(&mut *c)
            .await?;
            new_group = r.1;
            Some(r.0)
        }
        _ => None,
    };
    let id: i64 = match sqlx::query_scalar(
        "INSERT INTO torrents (type_id, title, subtitle, description, mediainfo, fields, size, file_count, piece_length,
                               meta_version, info_hash_v1, info_hash_v2, content_fp, blob_key, uploader_id, is_anonymous, status, group_id)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18) RETURNING id",
    )
    .bind(&m.type_id)
    .bind(&title)
    .bind(&m.subtitle)
    .bind(&m.description)
    .bind(&m.mediainfo)
    .bind(&fields)
    .bind(i64::try_from(parsed.total_size).unwrap_or(i64::MAX))
    .bind(i32::try_from(parsed.files.len()).unwrap_or(i32::MAX))
    .bind(i32::try_from(parsed.piece_length).unwrap_or(0))
    .bind(norm.meta as i16)
    .bind(&ih_v1)
    .bind(&ih_v2)
    .bind(parsed.pieces_sha1.map(|h| h.to_vec()))
    .bind(&key)
    .bind(uploader)
    .bind(m.anonymous)
    .bind(&status)
    .bind(group_id)
    .fetch_one(&mut *c)
    .await
    {
        Ok(id) => id,
        Err(sqlx::Error::Database(db)) if db.constraint().is_some_and(|n| n.starts_with("torrents_info_hash")) => {
            return Err(PluginError::new("torrent.duplicate", "this torrent already exists"));
        }
        Err(e) => return Err(e.into()),
    };
    sqlx::query("INSERT INTO torrent_file_versions (torrent_id, file_version, info_hash_v1, info_hash_v2, blob_key, size) VALUES ($1,1,$2,$3,$4,$5)")
        .bind(id)
        .bind(&ih_v1)
        .bind(&ih_v2)
        .bind(&key)
        .bind(i64::try_from(norm.stored.len()).unwrap_or(0))
        .execute(&mut *c)
        .await?;
    sqlx::query("INSERT INTO torrent_files (torrent_id, file_version, file_count, total_size, files) VALUES ($1,1,$2,$3,$4)")
        .bind(id)
        .bind(i32::try_from(parsed.files.len()).unwrap_or(i32::MAX))
        .bind(i64::try_from(parsed.total_size).unwrap_or(i64::MAX))
        .bind(serde_json::to_value(&parsed.files)?)
        .execute(&mut *c)
        .await?;
    drop(c);
    if let (Some(g), true) = (group_id, new_group) {
        ctx.events
            .publish(NewEvent::v1(
                "group.created",
                format!("group:{g}"),
                json!({
                    "group_id": g, "type_id": m.type_id, "group_key": group_key, "title": fields.get("title"),
                }),
            ))
            .await?;
    }
    ctx.events
        .publish(NewEvent::v1("torrent.created", format!("torrent:{id}"), json!({
            "torrent_id": id, "group_id": group_id, "type_id": m.type_id, "uploader_id": uploader, "anonymous": m.anonymous,
            "size": parsed.total_size, "file_count": parsed.files.len(), "status": status,
            "info_hash_v1": ih_v1.as_ref().map(hex::encode), "info_hash_v2": ih_v2.as_ref().map(hex::encode),
            "meta_version": norm.meta as i16, "title": title, "created_at": chrono::Utc::now().timestamp_millis(),
        })))
        .await?;
    Ok(json!({ "id": id, "group_id": group_id, "status": status,
               "info_hash_v1": ih_v1.as_ref().map(hex::encode), "info_hash_v2": ih_v2.as_ref().map(hex::encode),
               "need_redownload": true, "download_url": format!("/api/p/torrent/v1/torrents/{id}/download") }))
}

// ---------------- 下载（03 §1.9） ----------------

fn benc_str(s: &str) -> Vec<u8> {
    let mut v = Vec::new();
    bencode::encode_bytes(s.as_bytes(), &mut v);
    v
}

fn ascii_filename(title: &str) -> String {
    let s: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .take(150)
        .collect();
    format!("{s}.torrent")
}

pub async fn build_download(t: &Torrent, ctx: &Ctx, id: i64, via: &str) -> PResult<(Vec<u8>, String)> {
    let cfg = t.cfg()?;
    let uid = ctx.require_user()?;
    let row = load(ctx, id)
        .await?
        .ok_or_else(|| PluginError::not_found("torrent not found"))?;
    if !can_view(ctx, &row).await? {
        return Err(PluginError::not_found("torrent not found"));
    }
    if !can_download(ctx, &row) {
        return Err(PluginError::new(
            "torrent.not_downloadable",
            "this torrent cannot be downloaded now",
        ));
    }
    let users = ctx.service::<dyn UsersService>(USERS)?;
    let user = users
        .call(move |s, c| s.get(c, uid))
        .await?
        .ok_or_else(|| PluginError::forbidden("user not found"))?;
    match user.status.as_str() {
        "banned" => return Err(PluginError::new("identity.account_banned", "account is banned")),
        "pending" => return Err(PluginError::new("identity.account_pending", "account is pending")),
        "leech_disabled" if !is_owner(ctx, &row) => {
            return Err(PluginError::new(
                "identity.download_disabled",
                "downloads are disabled for this account",
            ));
        }
        "active" | "leech_disabled" => {}
        _ => return Err(PluginError::forbidden("account cannot download")),
    }
    let input = json!({ "version": 1, "actor": ctx.invocation.actor, "user_id": uid, "torrent_id": id,
                        "uploader_id": row.uploader_id, "size": row.size, "status": row.status, "via": via });
    if let HookOutcome::Denied {
        code,
        message,
        details,
        ..
    } = ctx.hooks.run("torrent.before_download", input).await?
    {
        let mut e = PluginError::new(code, message);
        e.details = details;
        return Err(e);
    }
    let pk = users
        .call(move |s, c| s.passkey_of(c, uid))
        .await?
        .filter(|p| p.len() == 32 && p.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or_else(|| {
            PluginError::new(
                "torrent.passkey_unavailable",
                "passkey is not available; reset it first",
            )
        })?;
    let stored = ctx.blob.get(&row.blob_key).await?.ok_or_else(|| {
        tracing::error!(torrent = id, key = %row.blob_key, "torrent file missing from object storage");
        PluginError::new("torrent.file_missing", "torrent file is missing").with_status(500)
    })?;
    let base = &cfg.announce_base_urls;
    let mut set: Vec<(&[u8], Vec<u8>)> = vec![(b"announce", benc_str(&format!("{}/{pk}", base[0])))];
    if base.len() > 1 {
        let mut tier = vec![b'l'];
        for b in base {
            tier.extend(benc_str(&format!("{b}/{pk}")));
        }
        tier.push(b'e');
        set.push((b"announce-list", [b"l".to_vec(), tier, b"e".to_vec()].concat()));
    }
    set.push((
        b"comment",
        benc_str(&cfg.comment_template.replace("{id}", &id.to_string())),
    ));
    set.push((b"created by", benc_str(&cfg.created_by)));
    let mut date = Vec::new();
    bencode::encode_int(row.created_at.timestamp(), &mut date);
    set.push((b"creation date", date));
    let bytes = bencode::splice_root(&stored, &set, &[])?;

    let uid_i = i64::try_from(uid).unwrap_or_default();
    let mut c = ctx.sql.conn().await?;
    sqlx::query(
        "INSERT INTO user_downloads (user_id, torrent_id) VALUES ($1, $2)
         ON CONFLICT (user_id, torrent_id) DO UPDATE SET last_at = now(), times = user_downloads.times + 1",
    )
    .bind(uid_i)
    .bind(id)
    .execute(&mut *c)
    .await?;
    drop(c);
    ctx.events
        .publish(NewEvent::v1(
            "torrent.downloaded",
            format!("user:{uid}"),
            json!({ "torrent_id": id, "user_id": uid, "via": via }),
        ))
        .await?;
    Ok((bytes, ascii_filename(&row.title)))
}

// ---------------- HTTP ----------------

async fn http_upload(State(t): State<Arc<Torrent>>, ctx: RequestCtx, mut mp: Multipart) -> PResult<Response> {
    let mut file = None;
    let mut meta_json = None;
    while let Some(field) = mp
        .next_field()
        .await
        .map_err(|e| PluginError::bad_request(format!("multipart: {e}")))?
    {
        match field.name() {
            Some("torrent") => {
                file = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| PluginError::bad_request(format!("multipart: {e}")))?,
                )
            }
            Some("meta") => {
                meta_json = Some(
                    field
                        .bytes()
                        .await
                        .map_err(|e| PluginError::bad_request(format!("multipart: {e}")))?,
                )
            }
            _ => {}
        }
    }
    let file = file.ok_or_else(|| PluginError::validation("part `torrent` is required"))?;
    let meta: UploadMeta = serde_json::from_slice(
        &meta_json.ok_or_else(|| PluginError::validation("part `meta` is required"))?,
    )?;
    let body = upload(&t, &ctx, meta, file).await?;
    let loc = format!("/api/p/torrent/v1/torrents/{}", body["id"]);
    let mut resp = (StatusCode::CREATED, Json(body)).into_response();
    if let Ok(v) = HeaderValue::from_str(&loc) {
        resp.headers_mut().insert(header::LOCATION, v);
    }
    Ok(resp)
}

async fn http_get(ctx: RequestCtx, Path(id): Path<i64>) -> PResult<Response> {
    let t = load(&ctx, id)
        .await?
        .ok_or_else(|| PluginError::not_found("torrent not found"))?;
    if !can_view(&ctx, &t).await? {
        return Err(PluginError::not_found("torrent not found"));
    }
    let uploader = if reveal_uploader(&ctx, &t).await? {
        json!({ "id": t.uploader_id })
    } else {
        json!({ "anonymous": true })
    };
    Ok(Json(json!({
        "id": t.id, "group_id": t.group_id, "type_id": t.type_id, "title": t.title, "subtitle": t.subtitle,
        "size": t.size, "file_count": t.file_count, "status": t.status, "uploader": uploader,
        "info_hash_v1": t.ih_v1.as_ref().map(hex::encode), "info_hash_v2": t.ih_v2.as_ref().map(hex::encode),
        "meta_version": t.meta_version, "file_version": t.file_version, "description": t.description, "fields": t.fields,
        "created_at": t.created_at.to_rfc3339(), "can_download": can_download(&ctx, &t),
    }))
    .into_response())
}

/// 可上传的资源类型：资源类型插件提供 schema；只在配置中登记的类型没有 schema
async fn http_types(State(t): State<Arc<Torrent>>, ctx: RequestCtx) -> PResult<Response> {
    let cfg = t.cfg()?;
    let mut items: Vec<serde_json::Value> = t
        .types(&ctx)
        .await?
        .iter()
        .map(|(p, i)| {
            json!({ "type_id": i.type_id, "name_term": i.name_term, "plugin": p,
                              "field_schema": i.field_schema, "group_schema": i.group_schema })
        })
        .collect();
    for ty in &cfg.types {
        if !items.iter().any(|i| i["type_id"] == ty.as_str()) {
            items.push(
                json!({ "type_id": ty, "name_term": format!("torrent.type.{ty}"), "plugin": null,
                               "field_schema": null, "group_schema": null }),
            );
        }
    }
    Ok(Json(json!({ "items": items })).into_response())
}

/// Group 详情与其下已发布的种子（按体积排序）
async fn http_group(ctx: RequestCtx, Path(id): Path<i64>) -> PResult<Response> {
    if !has(&ctx, "torrent.download").await? {
        return Err(PluginError::forbidden("not allowed"));
    }
    let mut c = ctx.sql.conn().await?;
    let g = sqlx::query(
        "SELECT id, type_id, group_key, title, year, fields, torrent_count, created_at FROM groups
          WHERE id = $1 AND deleted_at IS NULL AND merged_into IS NULL",
    )
    .bind(id)
    .fetch_optional(&mut *c)
    .await?
    .ok_or_else(|| PluginError::not_found("group not found"))?;
    let rows = sqlx::query(
        "SELECT id, title, subtitle, size, fields, created_at FROM torrents
          WHERE group_id = $1 AND status = 'approved' AND deleted_at IS NULL ORDER BY size DESC, id LIMIT 500",
    )
    .bind(id)
    .fetch_all(&mut *c)
    .await?;
    let torrents: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| json!({ "id": r.get::<i64, _>(0), "title": r.get::<String, _>(1), "subtitle": r.get::<Option<String>, _>(2),
                         "size": r.get::<i64, _>(3), "fields": r.get::<serde_json::Value, _>(4),
                         "created_at": r.get::<chrono::DateTime<chrono::Utc>, _>(5).to_rfc3339() }))
        .collect();
    Ok(Json(json!({
        "id": g.get::<i64, _>(0), "type_id": g.get::<String, _>(1), "group_key": g.get::<String, _>(2),
        "title": g.get::<String, _>(3), "year": g.get::<Option<i32>, _>(4), "fields": g.get::<serde_json::Value, _>(5),
        "torrent_count": g.get::<i32, _>(6), "created_at": g.get::<chrono::DateTime<chrono::Utc>, _>(7).to_rfc3339(),
        "torrents": torrents,
    }))
    .into_response())
}

async fn http_files(ctx: RequestCtx, Path(id): Path<i64>) -> PResult<Response> {
    let t = load(&ctx, id)
        .await?
        .ok_or_else(|| PluginError::not_found("torrent not found"))?;
    if !can_view(&ctx, &t).await? {
        return Err(PluginError::not_found("torrent not found"));
    }
    let mut c = ctx.sql.conn().await?;
    let files: serde_json::Value =
        sqlx::query_scalar("SELECT files FROM torrent_files WHERE torrent_id = $1 AND file_version = $2")
            .bind(id)
            .bind(t.file_version)
            .fetch_one(&mut *c)
            .await?;
    Ok(Json(json!({ "items": files, "next_cursor": null })).into_response())
}

async fn http_download(
    State(t): State<Arc<Torrent>>,
    ctx: RequestCtx,
    Path(id): Path<i64>,
) -> PResult<Response> {
    let (bytes, filename) = build_download(&t, &ctx, id, "web").await?;
    let mut resp = bytes.into_response();
    let h = resp.headers_mut();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-bittorrent"),
    );
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename=\"{filename}\"")) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    h.insert("x-content-type-options", HeaderValue::from_static("nosniff"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    Ok(resp)
}

// ---------------- 编辑、状态、删除（03 §1.6～§1.8 子集） ----------------

/// merge-patch：字段缺省 → None；显式 null → Some(None)
fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

#[derive(Debug, Deserialize)]
struct TorrentPatch {
    #[serde(default)]
    title: Option<String>,
    #[serde(default, deserialize_with = "present")]
    subtitle: Option<Option<String>>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    fields: Option<serde_json::Value>,
}

/// 编辑：本人（候选 / 待审 / 已发布）或 `torrent.update`；fields 经资源类型重新规范化，分组键变化时移到新 Group
async fn http_update(
    State(t): State<Arc<Torrent>>,
    ctx: RequestCtx,
    Path(id): Path<i64>,
    Json(p): Json<TorrentPatch>,
) -> PResult<Response> {
    let row = load(&ctx, id)
        .await?
        .filter(|r| r.status != "deleted")
        .ok_or_else(|| PluginError::not_found("torrent not found"))?;
    if !(is_owner(&ctx, &row) || has(&ctx, "torrent.update").await?) {
        return Err(PluginError::forbidden("not allowed to edit this torrent"));
    }
    let mut changed: Vec<&str> = Vec::new();
    let title = match &p.title {
        Some(x) => {
            changed.push("title");
            sanitize_title(x)?
        }
        None => row.title.clone(),
    };
    let subtitle = match p.subtitle {
        Some(x) => {
            changed.push("subtitle");
            x.map(|s| s.trim().chars().take(300).collect::<String>())
                .filter(|s| !s.is_empty())
        }
        None => row.subtitle.clone(),
    };
    let description = match p.description {
        Some(d) if d.chars().count() <= 65536 => {
            changed.push("description");
            d
        }
        Some(_) => return Err(PluginError::validation("description is too long")),
        None => row.description.clone(),
    };
    let mut fields = row.fields.clone();
    let mut group_id = row.group_id;
    if let Some(f) = p.fields {
        if !f.is_object() {
            return Err(PluginError::validation("fields must be an object"));
        }
        changed.push("fields");
        fields = f;
        if let Some((plugin, info)) = t.type_provider(&ctx, &row.type_id).await? {
            fields = ctx.content_type_call(&plugin, "normalize", fields).await?;
            let key = ctx
                .content_type_call(&plugin, "group_key", fields.clone())
                .await?
                .as_str()
                .map(str::to_owned);
            let mut c = ctx.sql.conn().await?;
            let cur_key: Option<String> = match row.group_id {
                Some(g) => {
                    sqlx::query_scalar("SELECT group_key FROM groups WHERE id = $1")
                        .bind(g)
                        .fetch_optional(&mut *c)
                        .await?
                }
                None => None,
            };
            if key != cur_key {
                if let Some(old) = row.group_id {
                    sqlx::query("UPDATE groups SET torrent_count = greatest(torrent_count - 1, 0), updated_at = now() WHERE id = $1")
                        .bind(old)
                        .execute(&mut *c)
                        .await?;
                }
                group_id = match &key {
                    Some(k) => {
                        let gtitle = fields
                            .get("group_title")
                            .or_else(|| fields.get("title"))
                            .and_then(serde_json::Value::as_str)
                            .map_or_else(|| title.clone(), str::to_owned);
                        let year = fields
                            .get("year")
                            .or_else(|| fields.get("first_air_year"))
                            .and_then(serde_json::Value::as_i64)
                            .and_then(|y| i32::try_from(y).ok());
                        let r: (i64, bool) = sqlx::query_as(
                            "INSERT INTO groups (type_id, group_key, title, year, fields, torrent_count, last_upload_at)
                             VALUES ($1, $2, $3, $4, $5, 1, now())
                             ON CONFLICT (type_id, group_key) WHERE deleted_at IS NULL AND merged_into IS NULL
                             DO UPDATE SET torrent_count = groups.torrent_count + 1, updated_at = now()
                             RETURNING id, (xmax = 0)",
                        )
                        .bind(&row.type_id)
                        .bind(k)
                        .bind(&gtitle)
                        .bind(year)
                        .bind(group_fields(info.group_schema.as_ref(), &fields))
                        .fetch_one(&mut *c)
                        .await?;
                        if r.1 {
                            drop(c);
                            ctx.events
                                .publish(NewEvent::v1(
                                    "group.created",
                                    format!("group:{}", r.0),
                                    json!({
                                    "group_id": r.0, "type_id": row.type_id, "group_key": k }),
                                ))
                                .await?;
                        }
                        Some(r.0)
                    }
                    None => None,
                };
                changed.push("group_id");
            }
        }
    }
    if changed.is_empty() {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    let mut c = ctx.sql.conn().await?;
    sqlx::query(
        "UPDATE torrents SET title = $2, subtitle = $3, description = $4, fields = $5, group_id = $6,
                updated_at = now(), version = version + 1 WHERE id = $1",
    )
    .bind(id)
    .bind(&title)
    .bind(&subtitle)
    .bind(&description)
    .bind(&fields)
    .bind(group_id)
    .execute(&mut *c)
    .await?;
    drop(c);
    ctx.events
        .publish(NewEvent::v1(
            "torrent.updated",
            format!("torrent:{id}"),
            json!({
            "torrent_id": id, "fields": changed, "group_id": group_id, "old_group_id": row.group_id }),
        ))
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Debug, Deserialize)]
struct StatusBody {
    to: String,
    #[serde(default)]
    reason: Option<String>,
}

/// 审核状态（`torrent.update`）：candidate / pending / approved / rejected / disabled
async fn http_set_status(
    ctx: RequestCtx,
    Path(id): Path<i64>,
    Json(b): Json<StatusBody>,
) -> PResult<Response> {
    if !matches!(
        b.to.as_str(),
        "candidate" | "pending" | "approved" | "rejected" | "disabled"
    ) {
        return Err(PluginError::validation("unknown status"));
    }
    let reason = b.reason.map(|r| r.chars().take(500).collect::<String>());
    let by = ctx.user_id();
    let mut c = ctx.sql.conn().await?;
    let from: Option<String> =
        sqlx::query_scalar("SELECT status FROM torrents WHERE id = $1 AND deleted_at IS NULL FOR UPDATE")
            .bind(id)
            .fetch_optional(&mut *c)
            .await?;
    let from = from.ok_or_else(|| PluginError::not_found("torrent not found"))?;
    if from == b.to {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    sqlx::query("UPDATE torrents SET status = $2, status_reason = $3, updated_at = now(), version = version + 1 WHERE id = $1")
        .bind(id)
        .bind(&b.to)
        .bind(&reason)
        .execute(&mut *c)
        .await?;
    drop(c);
    ctx.events
        .publish(NewEvent::v1(
            "torrent.status_changed",
            format!("torrent:{id}"),
            json!({
            "torrent_id": id, "from": from, "to": b.to, "reason": reason, "by_user_id": by }),
        ))
        .await?;
    ctx.audit
        .write(pt_plugin_api::AuditEntry {
            action: "torrent.status_change".into(),
            target: format!("torrent:{id}"),
            details: json!({ "from": from, "to": b.to, "reason": reason }),
        })
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Debug, Deserialize)]
struct DeleteQuery {
    reason: Option<String>,
}

/// 软删除（`torrent.delete`）：status = deleted、deleted_at；Group 计数减一
async fn http_delete(
    ctx: RequestCtx,
    Path(id): Path<i64>,
    Query(q): Query<DeleteQuery>,
) -> PResult<Response> {
    let reason = q
        .reason
        .unwrap_or_else(|| "admin".into())
        .chars()
        .take(32)
        .collect::<String>();
    let mut c = ctx.sql.conn().await?;
    let row: Option<(String, Option<i64>)> = sqlx::query_as(
        "SELECT status, group_id FROM torrents WHERE id = $1 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *c)
    .await?;
    let (from, group) = row.ok_or_else(|| PluginError::not_found("torrent not found"))?;
    sqlx::query("UPDATE torrents SET status = 'deleted', deleted_at = now(), delete_reason = $2, updated_at = now(), version = version + 1 WHERE id = $1")
        .bind(id)
        .bind(&reason)
        .execute(&mut *c)
        .await?;
    if let Some(g) = group {
        sqlx::query("UPDATE groups SET torrent_count = greatest(torrent_count - 1, 0), updated_at = now() WHERE id = $1")
            .bind(g)
            .execute(&mut *c)
            .await?;
    }
    drop(c);
    let by = ctx.user_id();
    ctx.events
        .publish(NewEvent::v1(
            "torrent.status_changed",
            format!("torrent:{id}"),
            json!({
            "torrent_id": id, "from": from, "to": "deleted", "reason": reason, "by_user_id": by }),
        ))
        .await?;
    ctx.events
        .publish(NewEvent::v1(
            "torrent.deleted",
            format!("torrent:{id}"),
            json!({ "torrent_id": id, "reason": reason, "by_user_id": by }),
        ))
        .await?;
    ctx.audit
        .write(pt_plugin_api::AuditEntry {
            action: "torrent.delete".into(),
            target: format!("torrent:{id}"),
            details: json!({ "reason": reason }),
        })
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

// ---------------- ctx.torrents（JSON 分派） ----------------

struct TorrentsSvc;

#[async_trait]
impl pt_plugin_api::JsonService for TorrentsSvc {
    async fn call(&self, ctx: &Ctx, method: &str, a: serde_json::Value) -> PResult<serde_json::Value> {
        let gid = a
            .get("id")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| PluginError::validation("id"))?;
        match method {
            "get_group" => {
                let mut c = ctx.sql.conn().await?;
                let r = sqlx::query("SELECT id, type_id, group_key, title, year, fields, torrent_count FROM groups WHERE id = $1 AND deleted_at IS NULL")
                    .bind(gid)
                    .fetch_optional(&mut *c)
                    .await?;
                Ok(r.map_or(serde_json::Value::Null, |r| json!({
                    "id": r.get::<i64, _>(0), "type_id": r.get::<String, _>(1), "group_key": r.get::<String, _>(2),
                    "title": r.get::<String, _>(3), "year": r.get::<Option<i32>, _>(4), "fields": r.get::<serde_json::Value, _>(5),
                    "torrent_count": r.get::<i32, _>(6) })))
            }
            // 资源类型插件按元数据更新 Group（作用域 write:group，只能更新本类型的 Group）
            "update_group" => {
                if !ctx.invocation.has_scope("write:group") {
                    return Err(PluginError::forbidden("scope write:group required"));
                }
                let patch = a.get("fields").cloned().unwrap_or_else(|| json!({}));
                let Some(obj) = patch.as_object() else {
                    return Err(PluginError::validation("fields must be an object"));
                };
                let title = a
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                    .map(|t| t.chars().take(300).collect::<String>());
                let year = a
                    .get("year")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|y| i32::try_from(y).ok())
                    .filter(|y| (1800..=2200).contains(y));
                let caller_type = ctx
                    .invocation
                    .caller_plugin
                    .strip_prefix("type-")
                    .unwrap_or("")
                    .to_owned();
                let mut c = ctx.sql.conn().await?;
                let n = sqlx::query(
                    "UPDATE groups SET fields = fields || $2, title = coalesce($3, title), year = coalesce($4, year), updated_at = now()
                      WHERE id = $1 AND type_id = $5 AND deleted_at IS NULL",
                )
                .bind(gid)
                .bind(&patch)
                .bind(&title)
                .bind(year)
                .bind(&caller_type)
                .execute(&mut *c)
                .await?
                .rows_affected();
                drop(c);
                if n == 0 {
                    return Err(PluginError::not_found("group not found for this content type"));
                }
                let mut keys: Vec<&String> = obj.keys().collect();
                keys.sort();
                ctx.events
                    .publish(NewEvent::v1(
                        "group.updated",
                        format!("group:{gid}"),
                        json!({ "group_id": gid, "fields": keys }),
                    ))
                    .await?;
                Ok(json!({ "updated": true }))
            }
            other => Err(PluginError::not_found(format!(
                "ctx.torrents has no method `{other}`"
            ))),
        }
    }
}

struct Routes(Arc<Torrent>);

impl HttpRoutes for Routes {
    fn router(&self) -> Router {
        Router::new()
            .route("/v1/torrents", post(http_upload))
            .route("/v1/types", get(http_types))
            .route("/v1/groups/{id}", get(http_group))
            .route("/v1/torrents/{id}", get(http_get).patch(http_update))
            .route("/v1/admin/torrents/{id}/status", post(http_set_status))
            .route("/v1/admin/torrents/{id}", axum::routing::delete(http_delete))
            .route("/v1/torrents/{id}/files", get(http_files))
            .route("/v1/torrents/{id}/download", get(http_download))
            .with_state(self.0.clone())
    }
}

pub fn plugin() -> NativePlugin {
    let t = Arc::new(Torrent::default());
    NativePlugin {
        manifest: MANIFEST,
        migrations: MIGRATIONS,
        lifecycle: Some(t.clone()),
        http: Some(Arc::new(Routes(t))),
        search_contributor: Some(Arc::new(search::Contributor)),
        json_services: vec![("ctx.torrents", Arc::new(TorrentsSvc))],
        ..Default::default()
    }
}
