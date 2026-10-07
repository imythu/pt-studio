//! 工作单元（kernel/03 §6.2）：每次调用一个事务，`SET LOCAL ROLE` 到插件角色；
//! 事件、审计、延迟任务在提交时以网关角色写入内核表（与业务数据同事务）。

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use pt_plugin_api::{
    AuditEntry, InvocationInfo, Json, NewEvent, PResult, PluginError, TimestampMs,
    host::{Audit, Column, Events, Jobs, Kv, PgTx, RowSet, Sql, SqlConn, SqlValue},
};
use sqlx::{
    Column as _, Executor, PgPool, Postgres, Row, TypeInfo as _, postgres::PgArguments, query::Query,
};

use crate::config::EventsSection;

#[derive(Debug, Clone)]
struct PendingEvent {
    id: uuid::Uuid,
    event_type: String,
    version: u32,
    key: String,
    occurred_at: TimestampMs,
    envelope: Json,
    payload: Json,
}

#[derive(Debug, Clone)]
struct PendingJob {
    id: uuid::Uuid,
    name: String,
    run_at: TimestampMs,
    payload: Json,
    dedupe_key: Option<String>,
}

/// 插件在工作单元中可用的数据库能力。
#[derive(Debug, Clone, Default)]
pub struct UowCaps {
    pub sql: bool,
    pub kv: bool,
    pub publishes: Vec<String>,
    pub jobs: Vec<String>,
    pub max_rows: u32,
}

pub struct Uow {
    pool: PgPool,
    plugin_id: String,
    /// None = 无数据库身份（内核自身的调用上下文）
    role: Option<String>,
    schema: String,
    statement_timeout_ms: u64,
    caps: UowCaps,
    events_cfg: Arc<EventsSection>,
    invocation: InvocationInfo,
    tx: tokio::sync::Mutex<Option<PgTx>>,
    events: Mutex<Vec<PendingEvent>>,
    audits: Mutex<Vec<AuditEntry>>,
    jobs: Mutex<Vec<PendingJob>>,
}

impl std::fmt::Debug for Uow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Uow")
            .field("plugin_id", &self.plugin_id)
            .finish_non_exhaustive()
    }
}

fn qi(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn ms_to_dt(ms: TimestampMs) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(ms).single().unwrap_or_else(Utc::now)
}

impl Uow {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        pool: PgPool,
        plugin_id: &str,
        role: Option<String>,
        schema: String,
        statement_timeout_ms: u64,
        caps: UowCaps,
        events_cfg: Arc<EventsSection>,
        invocation: InvocationInfo,
    ) -> Self {
        Self {
            pool,
            plugin_id: plugin_id.to_owned(),
            role,
            schema,
            statement_timeout_ms,
            caps,
            events_cfg,
            invocation,
            tx: tokio::sync::Mutex::new(None),
            events: Mutex::new(Vec::new()),
            audits: Mutex::new(Vec::new()),
            jobs: Mutex::new(Vec::new()),
        }
    }

    async fn begin(&self) -> PResult<PgTx> {
        let role = self
            .role
            .as_ref()
            .ok_or_else(|| PluginError::capability_denied("no database identity"))?;
        let mut tx = self.pool.begin().await?;
        tx.execute(format!("SET LOCAL ROLE {}", qi(role)).as_str())
            .await?;
        tx.execute(format!("SET LOCAL search_path = {}, ext, pg_catalog", qi(&self.schema)).as_str())
            .await?;
        tx.execute(format!("SET LOCAL statement_timeout = {}", self.statement_timeout_ms).as_str())
            .await?;
        Ok(tx)
    }

    async fn guard(&self) -> PResult<tokio::sync::MutexGuard<'_, Option<PgTx>>> {
        let mut g = self.tx.lock().await;
        if g.is_none() {
            *g = Some(self.begin().await?);
        }
        Ok(g)
    }

    fn require_sql(&self) -> PResult<()> {
        if self.caps.sql {
            Ok(())
        } else {
            Err(PluginError::capability_denied("capability `sql` not declared"))
        }
    }

    fn require_kv(&self) -> PResult<()> {
        if self.caps.kv {
            Ok(())
        } else {
            Err(PluginError::capability_denied("capability `kv` not declared"))
        }
    }

    /// 事件投递的第一条语句：写 inbox。返回 false 表示该事件已处理过（kernel/03 §7.5）。
    pub async fn claim_inbox(&self, subscription: &str, event_id: uuid::Uuid) -> PResult<bool> {
        let mut g = self.guard().await?;
        let tx = g
            .as_mut()
            .ok_or_else(|| PluginError::internal("no transaction"))?;
        let r =
            sqlx::query("INSERT INTO _inbox (subscription, event_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
                .bind(subscription)
                .bind(event_id)
                .execute(&mut **tx)
                .await?;
        Ok(r.rows_affected() == 1)
    }

    /// 提交（成功）或回滚（失败）。
    pub async fn finish(&self, ok: bool) -> PResult<()> {
        if !ok {
            if let Some(tx) = self.tx.lock().await.take() {
                tx.rollback().await?;
            }
            lock(&self.events).clear();
            lock(&self.audits).clear();
            lock(&self.jobs).clear();
            return Ok(());
        }
        self.commit_flow().await
    }

    async fn commit_flow(&self) -> PResult<()> {
        let events = std::mem::take(&mut *lock(&self.events));
        let audits = std::mem::take(&mut *lock(&self.audits));
        let jobs = std::mem::take(&mut *lock(&self.jobs));
        let mut g = self.tx.lock().await;
        let mut tx = match g.take() {
            Some(tx) => tx,
            None if events.is_empty() && audits.is_empty() && jobs.is_empty() => return Ok(()),
            None => self.pool.begin().await?,
        };
        if self.role.is_some() {
            tx.execute("RESET ROLE").await?;
            tx.execute("SET LOCAL search_path = kernel, pg_catalog").await?;
        }
        for e in &events {
            let partition = partition_of(&e.key, self.events_cfg.partitions_for(&e.event_type));
            sqlx::query(
                "INSERT INTO kernel.event_log (event_id, event_type, version, source, key, occurred_at, actor, payload, trace)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
            )
            .bind(e.id)
            .bind(&e.event_type)
            .bind(i32::try_from(e.version).unwrap_or(1))
            .bind(&self.plugin_id)
            .bind(&e.key)
            .bind(ms_to_dt(e.occurred_at))
            .bind(&e.envelope["actor"])
            .bind(&e.payload)
            .bind(serde_json::json!({ "traceparent": e.envelope["traceparent"] }))
            .execute(&mut *tx)
            .await?;
            sqlx::query("INSERT INTO kernel.event_outbox (event_id, event_type, partition, envelope) VALUES ($1,$2,$3,$4)")
                .bind(e.id)
                .bind(&e.event_type)
                .bind(i32::try_from(partition).unwrap_or(0))
                .bind(&e.envelope)
                .execute(&mut *tx)
                .await?;
        }
        for a in &audits {
            sqlx::query(
                "INSERT INTO kernel.audit_log (actor, plugin_id, action, target, outcome, request_id, details)
                 VALUES ($1,$2,$3,$4,'ok',$5,$6)",
            )
            .bind(serde_json::to_value(&self.invocation.actor)?)
            .bind(&self.plugin_id)
            .bind(&a.action)
            .bind(&a.target)
            .bind(&self.invocation.request_id)
            .bind(&a.details)
            .execute(&mut *tx)
            .await?;
        }
        for j in &jobs {
            sqlx::query(
                "INSERT INTO kernel.delayed_job (id, plugin_id, name, run_at, payload, dedupe_key)
                 VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT DO NOTHING",
            )
            .bind(j.id)
            .bind(&self.plugin_id)
            .bind(&j.name)
            .bind(ms_to_dt(j.run_at))
            .bind(&j.payload)
            .bind(&j.dedupe_key)
            .execute(&mut *tx)
            .await?;
        }
        if !events.is_empty() {
            tx.execute("NOTIFY pt_outbox").await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

pub fn partition_of(key: &str, partitions: u32) -> u32 {
    let h = xxhash_rust::xxh3::xxh3_64(key.as_bytes());
    u32::try_from(h % u64::from(partitions.max(1))).unwrap_or(0)
}

fn valid_event_key(k: &str) -> bool {
    if k == "global" {
        return true;
    }
    let Some((kind, rest)) = k.split_once(':') else {
        return false;
    };
    !kind.is_empty()
        && kind.bytes().all(|c| c.is_ascii_lowercase() || c == b'_')
        && (1..=128).contains(&rest.len())
        && rest
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.:-".contains(&c))
}

/// 不指定类型的 NULL 参数（OID 0）：由 PostgreSQL 按上下文推断类型。
/// 若以具体类型（如 text）绑定，写入 bigint 列时会报类型不匹配。
#[derive(Debug, Clone, Copy)]
struct UntypedNull;

impl sqlx::Type<Postgres> for UntypedNull {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_oid(sqlx::postgres::types::Oid(0))
    }
}

impl sqlx::Encode<'_, Postgres> for UntypedNull {
    fn encode_by_ref(
        &self,
        _buf: &mut sqlx::postgres::PgArgumentBuffer,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        Ok(sqlx::encode::IsNull::Yes)
    }
}

fn bind_values<'q>(
    mut q: Query<'q, Postgres, PgArguments>,
    params: &'q [SqlValue],
) -> Query<'q, Postgres, PgArguments> {
    for p in params {
        q = match p {
            SqlValue::Null => q.bind(UntypedNull),
            SqlValue::Boolean(b) => q.bind(*b),
            SqlValue::Int(i) => q.bind(*i),
            SqlValue::Float(f) => q.bind(*f),
            SqlValue::Numeric(s) | SqlValue::Text(s) => q.bind(s.as_str()),
            SqlValue::Bytes(b) => q.bind(b.as_slice()),
            SqlValue::Json(j) => q.bind(j),
            SqlValue::Timestamp(us) => q.bind(Utc.timestamp_micros(*us).single()),
            SqlValue::Uuid(u) => q.bind(*u),
            SqlValue::IntArray(a) => q.bind(a.as_slice()),
            SqlValue::TextArray(a) => q.bind(a.as_slice()),
        };
    }
    q
}

fn decode(row: &sqlx::postgres::PgRow, i: usize) -> PResult<SqlValue> {
    let col = &row.columns()[i];
    let ty = col.type_info().name().to_ascii_uppercase();
    macro_rules! get {
        ($t:ty, $f:expr) => {{
            let v: Option<$t> = row.try_get(i)?;
            v.map_or(SqlValue::Null, $f)
        }};
    }
    Ok(match ty.as_str() {
        "BOOL" => get!(bool, SqlValue::Boolean),
        "INT2" => get!(i16, |v| SqlValue::Int(i64::from(v))),
        "INT4" => get!(i32, |v| SqlValue::Int(i64::from(v))),
        "INT8" => get!(i64, SqlValue::Int),
        "FLOAT4" => get!(f32, |v| SqlValue::Float(f64::from(v))),
        "FLOAT8" => get!(f64, SqlValue::Float),
        "BYTEA" => get!(Vec<u8>, SqlValue::Bytes),
        "JSON" | "JSONB" => get!(Json, SqlValue::Json),
        "TIMESTAMPTZ" => get!(DateTime<Utc>, |v| SqlValue::Timestamp(v.timestamp_micros())),
        "UUID" => get!(uuid::Uuid, SqlValue::Uuid),
        "INT8[]" => get!(Vec<i64>, SqlValue::IntArray),
        "TEXT[]" | "VARCHAR[]" => get!(Vec<String>, SqlValue::TextArray),
        "TEXT" | "VARCHAR" | "BPCHAR" | "NAME" => get!(String, SqlValue::Text),
        _ => {
            // 其他类型以文本返回（numeric 等）；无法以文本解码时报错
            let v: Option<String> = row
                .try_get_unchecked(i)
                .map_err(|_| PluginError::internal(format!("unsupported column type {ty}")))?;
            v.map_or(SqlValue::Null, |s| {
                if ty == "NUMERIC" {
                    SqlValue::Numeric(s)
                } else {
                    SqlValue::Text(s)
                }
            })
        }
    })
}

#[async_trait]
impl Sql for Uow {
    async fn query(&self, stmt: &str, params: &[SqlValue]) -> PResult<RowSet> {
        self.require_sql()?;
        let mut g = self.guard().await?;
        let tx = g
            .as_mut()
            .ok_or_else(|| PluginError::internal("no transaction"))?;
        let rows = bind_values(sqlx::query(stmt), params)
            .fetch_all(&mut **tx)
            .await?;
        let mut set = RowSet::default();
        if let Some(first) = rows.first() {
            set.columns = first
                .columns()
                .iter()
                .map(|c| Column {
                    name: c.name().to_owned(),
                    type_name: c.type_info().name().to_owned(),
                })
                .collect();
        }
        let max = usize::try_from(self.caps.max_rows).unwrap_or(usize::MAX);
        for r in rows.iter().take(max) {
            set.rows.push(
                (0..r.columns().len())
                    .map(|i| decode(r, i))
                    .collect::<PResult<Vec<_>>>()?,
            );
        }
        set.truncated = rows.len() > max;
        Ok(set)
    }

    async fn execute(&self, stmt: &str, params: &[SqlValue]) -> PResult<u64> {
        self.require_sql()?;
        let mut g = self.guard().await?;
        let tx = g
            .as_mut()
            .ok_or_else(|| PluginError::internal("no transaction"))?;
        Ok(bind_values(sqlx::query(stmt), params)
            .execute(&mut **tx)
            .await?
            .rows_affected())
    }

    async fn execute_batch(&self, stmt: &str, param_sets: &[Vec<SqlValue>]) -> PResult<u64> {
        self.require_sql()?;
        let mut g = self.guard().await?;
        let tx = g
            .as_mut()
            .ok_or_else(|| PluginError::internal("no transaction"))?;
        let mut n = 0;
        for params in param_sets {
            n += bind_values(sqlx::query(stmt), params)
                .execute(&mut **tx)
                .await?
                .rows_affected();
        }
        Ok(n)
    }

    async fn commit(&self) -> PResult<()> {
        self.commit_flow().await
    }

    async fn conn(&self) -> PResult<SqlConn<'_>> {
        self.require_sql()?;
        let g = self.guard().await?;
        SqlConn::new(g).ok_or_else(|| PluginError::internal("no transaction"))
    }
}

#[async_trait]
impl Kv for Uow {
    async fn get(&self, key: &str) -> PResult<Option<Vec<u8>>> {
        self.require_kv()?;
        let mut g = self.guard().await?;
        let tx = g
            .as_mut()
            .ok_or_else(|| PluginError::internal("no transaction"))?;
        let v: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT value FROM _kv WHERE key = $1 AND (expires_at IS NULL OR expires_at > now())",
        )
        .bind(key)
        .fetch_optional(&mut **tx)
        .await?;
        Ok(v)
    }

    async fn set(&self, key: &str, value: &[u8], expires_at: Option<TimestampMs>) -> PResult<()> {
        self.require_kv()?;
        if value.len() > 65536 {
            return Err(PluginError::new(
                "kernel.payload_too_large",
                "kv value exceeds 64 KiB",
            ));
        }
        let mut g = self.guard().await?;
        let tx = g
            .as_mut()
            .ok_or_else(|| PluginError::internal("no transaction"))?;
        sqlx::query(
            "INSERT INTO _kv (key, value, expires_at) VALUES ($1, $2, $3)
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, expires_at = EXCLUDED.expires_at, updated_at = now()",
        )
        .bind(key)
        .bind(value)
        .bind(expires_at.map(ms_to_dt))
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> PResult<bool> {
        self.require_kv()?;
        let mut g = self.guard().await?;
        let tx = g
            .as_mut()
            .ok_or_else(|| PluginError::internal("no transaction"))?;
        Ok(sqlx::query("DELETE FROM _kv WHERE key = $1")
            .bind(key)
            .execute(&mut **tx)
            .await?
            .rows_affected()
            == 1)
    }

    async fn compare_and_set(&self, key: &str, expected: Option<&[u8]>, value: &[u8]) -> PResult<bool> {
        self.require_kv()?;
        let mut g = self.guard().await?;
        let tx = g
            .as_mut()
            .ok_or_else(|| PluginError::internal("no transaction"))?;
        let n = match expected {
            None => sqlx::query("INSERT INTO _kv (key, value) VALUES ($1, $2) ON CONFLICT DO NOTHING")
                .bind(key)
                .bind(value)
                .execute(&mut **tx)
                .await?
                .rows_affected(),
            Some(exp) => {
                sqlx::query("UPDATE _kv SET value = $2, updated_at = now() WHERE key = $1 AND value = $3")
                    .bind(key)
                    .bind(value)
                    .bind(exp)
                    .execute(&mut **tx)
                    .await?
                    .rows_affected()
            }
        };
        Ok(n == 1)
    }

    async fn list_keys(&self, prefix: &str, cursor: Option<&str>, limit: u32) -> PResult<Vec<String>> {
        self.require_kv()?;
        let mut g = self.guard().await?;
        let tx = g
            .as_mut()
            .ok_or_else(|| PluginError::internal("no transaction"))?;
        let pattern = format!(
            "{}%",
            prefix
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let keys: Vec<String> = sqlx::query_scalar(
            "SELECT key FROM _kv WHERE key LIKE $1 AND key > coalesce($2, '') ORDER BY key LIMIT $3",
        )
        .bind(pattern)
        .bind(cursor)
        .bind(i64::from(limit.min(1000)))
        .fetch_all(&mut **tx)
        .await?;
        Ok(keys)
    }
}

#[async_trait]
impl Events for Uow {
    async fn publish(&self, evt: NewEvent) -> PResult<uuid::Uuid> {
        if !self.caps.publishes.iter().any(|p| p == &evt.event_type) {
            return Err(PluginError::capability_denied(format!(
                "event `{}` not in publishes",
                evt.event_type
            )));
        }
        if !valid_event_key(&evt.key) {
            return Err(PluginError::validation(format!(
                "invalid event key `{}`",
                evt.key
            )));
        }
        let payload_len = serde_json::to_vec(&evt.payload)?.len();
        if payload_len > 64 * 1024 {
            return Err(PluginError::new(
                "kernel.payload_too_large",
                "event payload exceeds 64 KiB",
            ));
        }
        let id = uuid::Uuid::now_v7();
        let occurred_at = Utc::now().timestamp_millis();
        let envelope = serde_json::json!({
            "id": id, "type": evt.event_type, "version": evt.version, "occurred_at": occurred_at,
            "actor": self.invocation.actor, "source": self.plugin_id, "key": evt.key,
            "payload": evt.payload, "traceparent": self.invocation.traceparent,
        });
        lock(&self.events).push(PendingEvent {
            id,
            event_type: evt.event_type,
            version: evt.version,
            key: evt.key,
            occurred_at,
            envelope,
            payload: evt.payload,
        });
        Ok(id)
    }
}

#[async_trait]
impl Audit for Uow {
    async fn write(&self, e: AuditEntry) -> PResult<()> {
        lock(&self.audits).push(e);
        Ok(())
    }
}

#[async_trait]
impl Jobs for Uow {
    async fn schedule_once(
        &self,
        name: &str,
        run_at: TimestampMs,
        payload: Json,
        dedupe_key: Option<&str>,
    ) -> PResult<String> {
        if !self.caps.jobs.iter().any(|j| j == name) {
            return Err(PluginError::capability_denied(format!(
                "job `{name}` not declared in [[jobs]]"
            )));
        }
        let id = uuid::Uuid::now_v7();
        lock(&self.jobs).push(PendingJob {
            id,
            name: name.to_owned(),
            run_at,
            payload,
            dedupe_key: dedupe_key.map(str::to_owned),
        });
        Ok(id.to_string())
    }

    async fn cancel(&self, job_id: &str) -> PResult<bool> {
        let id = uuid::Uuid::parse_str(job_id).map_err(|_| PluginError::bad_request("invalid job id"))?;
        // 取消是对内核表的写，直接在独立连接上执行（属于本插件的任务才可取消）
        let n = sqlx::query(
            "UPDATE kernel.delayed_job SET state = 'cancelled', updated_at = now()
             WHERE id = $1 AND plugin_id = $2 AND state = 'pending'",
        )
        .bind(id)
        .bind(&self.plugin_id)
        .execute(&self.pool)
        .await?
        .rows_affected();
        Ok(n == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_keys() {
        assert!(valid_event_key("user:42"));
        assert!(valid_event_key("global"));
        assert!(valid_event_key("torrent:1:v2"));
        assert!(!valid_event_key("User:1"));
        assert!(!valid_event_key("user:"));
        assert!(!valid_event_key("user 1"));
    }

    #[test]
    fn partitions_stable() {
        assert_eq!(partition_of("user:1", 1), 0);
        let p = partition_of("user:1", 16);
        assert_eq!(p, partition_of("user:1", 16));
        assert!(p < 16);
    }
}
