//! 迁移执行与授权（kernel/03 §2.4、§6.1、§6.4、§16）。在管理连接（pt_admin）上执行。

use std::time::Instant;

use sha2::{Digest, Sha256};
use sqlx::{Connection, Executor, PgConnection, Row};

use crate::{config::SiteConfig, loader::PluginSpec};

pub const KERNEL_MIGRATIONS: &[(&str, &str)] = &[(
    "0001_kernel.sql",
    include_str!("../../migrations/0001_kernel.sql"),
)];

#[derive(Debug, thiserror::Error)]
pub enum MigrateError {
    #[error("error[E-MIG-NAME] plugin `{plugin}`: {message}")]
    Name { plugin: String, message: String },
    #[error("error[E-MIG-CHANGED] plugin `{plugin}`: migration {file} was modified after being applied")]
    Changed { plugin: String, file: String },
    #[error("error[E-MIG-FAILED] plugin `{plugin}`: {file}: {source}")]
    Failed {
        plugin: String,
        file: String,
        source: sqlx::Error,
    },
    #[error("error[E-MIG-REJECTED] plugin `{plugin}`: {file}: {reason}")]
    Rejected {
        plugin: String,
        file: String,
        reason: String,
    },
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}

pub fn role_of(cfg: &SiteConfig, schema: &str) -> String {
    format!("{}plugin_{schema}", cfg.database.role_prefix)
}

pub fn gateway_role(cfg: &SiteConfig) -> String {
    format!("{}gateway", cfg.database.role_prefix)
}

fn checksum(sql: &str) -> String {
    hex::encode(Sha256::digest(sql.replace("\r\n", "\n").as_bytes()))
}

fn qi(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// 迁移文件名：`NNNN_<snake>.sql`，从 0001 连续递增。
fn check_names(plugin: &str, files: &[(&str, &str)]) -> Result<Vec<(i32, String)>, MigrateError> {
    let mut out = Vec::new();
    for (i, (name, _)) in files.iter().enumerate() {
        let bad = || MigrateError::Name {
            plugin: plugin.to_owned(),
            message: format!("bad migration file name `{name}`"),
        };
        let stem = name.strip_suffix(".sql").ok_or_else(bad)?;
        let (num, desc) = stem.split_once('_').ok_or_else(bad)?;
        let n: i32 = num.parse().map_err(|_| bad())?;
        if num.len() != 4
            || desc.is_empty()
            || !desc
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        {
            return Err(bad());
        }
        if usize::try_from(n).ok() != Some(i + 1) {
            return Err(MigrateError::Name {
                plugin: plugin.to_owned(),
                message: format!("migrations must be numbered consecutively from 0001 (got `{name}`)"),
            });
        }
        out.push((n, (*name).to_owned()));
    }
    Ok(out)
}

/// 全部迁移与授权。多副本由 advisory lock 串行。
pub async fn migrate_all(cfg: &SiteConfig, plugins: &[&PluginSpec]) -> Result<(), MigrateError> {
    let mut conn = PgConnection::connect(&cfg.database.admin_url).await?;
    conn.execute("SELECT pg_advisory_lock(hashtext('pt-studio:migrate'))")
        .await?;
    let r = migrate_locked(&mut conn, cfg, plugins).await;
    conn.execute("SELECT pg_advisory_unlock(hashtext('pt-studio:migrate'))")
        .await?;
    r
}

async fn migrate_locked(
    conn: &mut PgConnection,
    cfg: &SiteConfig,
    plugins: &[&PluginSpec],
) -> Result<(), MigrateError> {
    let gw = gateway_role(cfg);
    // 引导：kernel schema（属主 pt_gateway）、ext、迁移记录与审计表（属主 pt_admin）
    conn.execute(format!("CREATE SCHEMA IF NOT EXISTS kernel AUTHORIZATION {}", qi(&gw)).as_str())
        .await?;
    // 管理连接需要在 kernel schema 中建迁移记录与审计表（属主 pt_admin）
    conn.execute(format!("SET ROLE {}", qi(&gw)).as_str()).await?;
    conn.execute("GRANT USAGE, CREATE ON SCHEMA kernel TO SESSION_USER")
        .await?;
    conn.execute("RESET ROLE").await?;
    conn.execute("CREATE SCHEMA IF NOT EXISTS ext").await?;
    conn.execute(format!("GRANT USAGE ON SCHEMA ext TO {}", qi(&gw)).as_str())
        .await?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS kernel.plugin_migration (
            plugin_id text NOT NULL, version integer NOT NULL, name text NOT NULL, checksum text NOT NULL,
            applied_at timestamptz NOT NULL DEFAULT now(), duration_ms integer NOT NULL,
            PRIMARY KEY (plugin_id, version))",
    )
    .await?;
    conn.execute(format!("GRANT SELECT ON kernel.plugin_migration TO {}", qi(&gw)).as_str())
        .await?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS kernel.audit_log (
            id bigint GENERATED ALWAYS AS IDENTITY, occurred_at timestamptz NOT NULL DEFAULT now(),
            actor jsonb NOT NULL, plugin_id text NOT NULL, action text NOT NULL, target text,
            outcome text NOT NULL, request_id text, trace_id text, details jsonb NOT NULL DEFAULT '{}',
            PRIMARY KEY (occurred_at, id)) PARTITION BY RANGE (occurred_at)",
    )
    .await?;
    conn.execute("CREATE TABLE IF NOT EXISTS kernel.audit_log_default PARTITION OF kernel.audit_log DEFAULT")
        .await?;
    conn.execute("CREATE INDEX IF NOT EXISTS audit_log_action_idx ON kernel.audit_log (plugin_id, action, occurred_at)")
        .await?;
    conn.execute(format!("GRANT INSERT, SELECT ON kernel.audit_log TO {}", qi(&gw)).as_str())
        .await?;

    for ext in &cfg.database.extensions {
        conn.execute(format!("CREATE EXTENSION IF NOT EXISTS {} SCHEMA ext", qi(ext)).as_str())
            .await?;
    }

    apply_files(conn, "kernel", &gw, "kernel", KERNEL_MIGRATIONS).await?;

    for p in plugins {
        let schema = p.manifest.schema();
        let role = role_of(cfg, &schema);
        ensure_plugin_role(conn, cfg, &role, &schema).await?;
        if p.manifest.database.as_ref().is_some_and(|d| d.plpgsql) {
            conn.execute(format!("GRANT USAGE ON LANGUAGE plpgsql TO {}", qi(&role)).as_str())
                .await?;
        }
        let files = p.native.as_ref().map(|n| n.migrations).unwrap_or(&[]);
        // WASM 插件包不受信任：迁移在管理连接上执行，先做语句检查（kernel/03 §6.3）
        if p.manifest.kind == pt_plugin_api::manifest::PluginKind::Wasm {
            if p.manifest.database.as_ref().is_some_and(|d| d.plpgsql) {
                return Err(MigrateError::Rejected {
                    plugin: p.id().to_owned(),
                    file: "plugin.toml".into(),
                    reason: "plpgsql is only available to native plugins".into(),
                });
            }
            for (name, sql) in files {
                super::sqlcheck::check_migration(sql).map_err(|reason| MigrateError::Rejected {
                    plugin: p.id().to_owned(),
                    file: (*name).to_owned(),
                    reason,
                })?;
            }
        }
        apply_files(conn, p.id(), &role, &schema, files).await?;
    }
    sync_view_grants(conn, cfg, plugins).await?;
    Ok(())
}

async fn ensure_plugin_role(
    conn: &mut PgConnection,
    cfg: &SiteConfig,
    role: &str,
    schema: &str,
) -> Result<(), MigrateError> {
    let gw = gateway_role(cfg);
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = $1)")
        .bind(role)
        .fetch_one(&mut *conn)
        .await?;
    if !exists {
        conn.execute(format!("CREATE ROLE {} NOLOGIN NOINHERIT", qi(role)).as_str())
            .await?;
    }
    // PG16+：网关与管理连接只能 SET ROLE，不继承插件权限
    conn.execute(format!("GRANT {} TO {} WITH INHERIT FALSE, SET TRUE", qi(role), qi(&gw)).as_str())
        .await?;
    conn.execute(format!("GRANT {} TO CURRENT_USER WITH INHERIT FALSE, SET TRUE", qi(role)).as_str())
        .await?;
    conn.execute(
        format!(
            "CREATE SCHEMA IF NOT EXISTS {} AUTHORIZATION {}",
            qi(schema),
            qi(role)
        )
        .as_str(),
    )
    .await?;
    conn.execute(format!("SET ROLE {}", qi(role)).as_str()).await?;
    let r = conn
        .execute(format!("REVOKE ALL ON SCHEMA {} FROM PUBLIC", qi(schema)).as_str())
        .await;
    conn.execute("RESET ROLE").await?;
    r?;
    conn.execute(format!("GRANT USAGE ON SCHEMA ext TO {}", qi(role)).as_str())
        .await?;
    // 内核管理的表（§16.2），属主为插件角色
    let mut tx = conn.begin().await?;
    tx.execute(format!("SET LOCAL ROLE {}", qi(role)).as_str())
        .await?;
    tx.execute(format!("SET LOCAL search_path = {}", qi(schema)).as_str())
        .await?;
    tx.execute(
        "CREATE TABLE IF NOT EXISTS _inbox (
            subscription text NOT NULL, event_id uuid NOT NULL, processed_at timestamptz NOT NULL DEFAULT now(),
            PRIMARY KEY (subscription, event_id))",
    )
    .await?;
    tx.execute("CREATE INDEX IF NOT EXISTS _inbox_processed_at_idx ON _inbox (processed_at)")
        .await?;
    tx.execute(
        "CREATE TABLE IF NOT EXISTS _kv (
            key text PRIMARY KEY, value bytea NOT NULL CHECK (octet_length(value) <= 65536),
            expires_at timestamptz, updated_at timestamptz NOT NULL DEFAULT now())",
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

async fn apply_files(
    conn: &mut PgConnection,
    plugin: &str,
    role: &str,
    schema: &str,
    files: &[(&str, &str)],
) -> Result<(), MigrateError> {
    let names = check_names(plugin, files)?;
    let applied: Vec<(i32, String)> = sqlx::query(
        "SELECT version, checksum FROM kernel.plugin_migration WHERE plugin_id = $1 ORDER BY version",
    )
    .bind(plugin)
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|r| (r.get(0), r.get(1)))
    .collect();
    for ((n, name), (_, sql)) in names.iter().zip(files) {
        let sum = checksum(sql);
        if let Some((_, old)) = applied.iter().find(|(v, _)| v == n) {
            if *old != sum {
                return Err(MigrateError::Changed {
                    plugin: plugin.to_owned(),
                    file: name.clone(),
                });
            }
            continue;
        }
        let started = Instant::now();
        let fail = |source| MigrateError::Failed {
            plugin: plugin.to_owned(),
            file: name.clone(),
            source,
        };
        if sql.starts_with("-- pt:no-transaction") {
            // 逐语句执行（CREATE INDEX CONCURRENTLY 等）
            conn.execute(format!("SET ROLE {}", qi(role)).as_str()).await?;
            conn.execute(format!("SET search_path = {}, ext, pg_catalog", qi(schema)).as_str())
                .await?;
            let r = conn.execute(*sql).await;
            conn.execute("RESET ROLE").await?;
            conn.execute("RESET search_path").await?;
            r.map_err(fail)?;
        } else {
            let mut tx = conn.begin().await?;
            tx.execute(format!("SET LOCAL ROLE {}", qi(role)).as_str())
                .await?;
            tx.execute(format!("SET LOCAL search_path = {}, ext, pg_catalog", qi(schema)).as_str())
                .await?;
            tx.execute("SET LOCAL lock_timeout = '5s'").await?;
            tx.execute("SET LOCAL statement_timeout = '15min'").await?;
            tx.execute(*sql).await.map_err(fail)?;
            tx.execute("RESET ROLE").await?;
            record(&mut tx, plugin, *n, name, &sum, started).await?;
            tx.commit().await?;
            tracing::info!(plugin, file = %name, ms = started.elapsed().as_millis() as u64, "migration applied");
            continue;
        }
        record(conn, plugin, *n, name, &sum, started).await?;
        tracing::info!(plugin, file = %name, "migration applied (no transaction)");
    }
    Ok(())
}

async fn record(
    conn: &mut PgConnection,
    plugin: &str,
    n: i32,
    name: &str,
    sum: &str,
    started: Instant,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO kernel.plugin_migration (plugin_id, version, name, checksum, duration_ms) VALUES ($1,$2,$3,$4,$5)")
        .bind(plugin)
        .bind(n)
        .bind(name)
        .bind(sum)
        .bind(i32::try_from(started.elapsed().as_millis()).unwrap_or(i32::MAX))
        .execute(conn)
        .await?;
    Ok(())
}

/// 只读视图授权（§6.4）：消费方 `capabilities.views` 中的视图，须由提供方在 `[database].views` 中对外声明。
async fn sync_view_grants(
    conn: &mut PgConnection,
    cfg: &SiteConfig,
    plugins: &[&PluginSpec],
) -> Result<(), MigrateError> {
    let mut wanted: Vec<(String, String)> = Vec::new();
    for p in plugins {
        for v in &p.manifest.capabilities.views {
            let Some((schema, view)) = v.split_once('.') else {
                continue;
            };
            let provider = plugins.iter().find(|q| q.manifest.schema() == schema);
            let declared = provider
                .and_then(|q| q.manifest.database.as_ref())
                .is_some_and(|d| d.views.iter().any(|x| x == view || x == v));
            if !declared {
                tracing::warn!(plugin = p.id(), view = %v, "view not exported by an enabled provider; not granted");
                continue;
            }
            wanted.push((p.id().to_owned(), v.clone()));
        }
    }
    for (consumer, v) in &wanted {
        let Some((schema, view)) = v.split_once('.') else {
            continue;
        };
        let consumer_role = role_of(cfg, &consumer.replace('-', "_"));
        let provider_role = role_of(cfg, schema);
        let mut tx = conn.begin().await?;
        tx.execute(format!("SET LOCAL ROLE {}", qi(&provider_role)).as_str())
            .await?;
        tx.execute(format!("GRANT USAGE ON SCHEMA {} TO {}", qi(schema), qi(&consumer_role)).as_str())
            .await?;
        tx.execute(
            format!(
                "GRANT SELECT ON {}.{} TO {}",
                qi(schema),
                qi(view),
                qi(&consumer_role)
            )
            .as_str(),
        )
        .await?;
        tx.execute(format!("SET LOCAL ROLE {}", qi(&gateway_role(cfg))).as_str())
            .await?;
        sqlx::query("INSERT INTO kernel.view_grant (consumer_plugin, view_name) VALUES ($1, $2) ON CONFLICT DO NOTHING")
            .bind(consumer)
            .bind(v)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
    }
    // 撤销多余授权
    let mut tx = conn.begin().await?;
    tx.execute(format!("SET LOCAL ROLE {}", qi(&gateway_role(cfg))).as_str())
        .await?;
    let rows = sqlx::query("SELECT consumer_plugin, view_name FROM kernel.view_grant")
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    let existing: Vec<(String, String)> = rows.into_iter().map(|r| (r.get(0), r.get(1))).collect();
    for (consumer, v) in existing {
        if wanted.iter().any(|(c, w)| *c == consumer && *w == v) {
            continue;
        }
        let Some((schema, view)) = v.split_once('.') else {
            continue;
        };
        let consumer_role = role_of(cfg, &consumer.replace('-', "_"));
        let provider_role = role_of(cfg, schema);
        let mut tx = conn.begin().await?;
        let has_provider: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = $1)")
                .bind(&provider_role)
                .fetch_one(&mut *tx)
                .await?;
        if has_provider {
            tx.execute(format!("SET LOCAL ROLE {}", qi(&provider_role)).as_str())
                .await?;
            let _ = tx
                .execute(
                    format!(
                        "REVOKE SELECT ON {}.{} FROM {}",
                        qi(schema),
                        qi(view),
                        qi(&consumer_role)
                    )
                    .as_str(),
                )
                .await;
        }
        tx.execute(format!("SET LOCAL ROLE {}", qi(&gateway_role(cfg))).as_str())
            .await?;
        sqlx::query("DELETE FROM kernel.view_grant WHERE consumer_plugin = $1 AND view_name = $2")
            .bind(&consumer)
            .bind(&v)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        tracing::info!(consumer, view = %v, "view grant revoked");
    }
    Ok(())
}

/// 运维命令：以某插件的数据库角色执行一条语句（参数均为文本），返回受影响行数。
/// 只供 `pt-server` 的管理子命令使用（如为首个管理员授予角色），不经过插件与审计。
pub async fn exec_as_plugin(
    cfg: &SiteConfig,
    schema: &str,
    sql: &str,
    params: &[&str],
) -> Result<u64, sqlx::Error> {
    let mut conn = PgConnection::connect(&cfg.database.admin_url).await?;
    let mut tx = conn.begin().await?;
    tx.execute(format!("SET LOCAL ROLE {}", qi(&role_of(cfg, schema))).as_str())
        .await?;
    tx.execute(format!("SET LOCAL search_path TO {}", qi(schema)).as_str())
        .await?;
    let mut q = sqlx::query(sql);
    for p in params {
        q = q.bind(*p);
    }
    let n = q.execute(&mut *tx).await?.rows_affected();
    tx.commit().await?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(check_names("x", &[("0001_init.sql", ""), ("0002_more.sql", "")]).is_ok());
        assert!(check_names("x", &[("0002_init.sql", "")]).is_err());
        assert!(check_names("x", &[("1_init.sql", "")]).is_err());
        assert!(check_names("x", &[("0001_Init.sql", "")]).is_err());
    }
}
