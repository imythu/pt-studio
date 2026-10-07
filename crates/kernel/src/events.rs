//! 事件：outbox 中继、消费者、死信；延迟任务执行（kernel/03 §7、§9.2）。

use std::{sync::Arc, time::Duration};

use pt_plugin_api::{
    Actor, DeliveryInfo, EventEnvelope, InvocationKind, JobInvocation, JobKind, Json, PluginError,
    manifest::{Subscription, TxMode},
};
use redis::{Value, aio::ConnectionManager};
use sqlx::{Row, postgres::PgListener};
use tokio::sync::watch;

use crate::{
    cluster::Lease,
    runtime::{CallSpec, Shared},
};

const LEASE_TTL: Duration = Duration::from_secs(30);
const EVENT_TIMEOUT_MS: i64 = 30_000;

pub fn stream_key(event_type: &str, partition: u32) -> String {
    format!("ev:{event_type}:{partition}")
}

fn main_redis(sh: &Shared) -> Option<ConnectionManager> {
    sh.redis.get("main").cloned()
}

/// 启动全部后台任务（中继、消费者、延迟任务）。`stop` 置 true 时退出。
pub fn spawn_background(sh: Arc<Shared>, stop: watch::Receiver<bool>) -> Vec<tokio::task::JoinHandle<()>> {
    let mut tasks = vec![
        tokio::spawn(relay_loop(sh.clone(), stop.clone())),
        tokio::spawn(delayed_jobs_loop(sh.clone(), stop.clone())),
    ];
    for (pid, rt) in &sh.plugins {
        for decl in &rt.manifest.workers {
            if let Some((_, w)) = rt.workers.iter().find(|(n, _)| *n == decl.name) {
                tasks.push(tokio::spawn(worker_supervisor(
                    sh.clone(),
                    pid.clone(),
                    decl.clone(),
                    w.clone(),
                    stop.clone(),
                )));
            }
        }
        for sub in &rt.manifest.subscriptions {
            for p in 0..sh.events_cfg.partitions_for(&sub.event) {
                tasks.push(tokio::spawn(consumer_loop(
                    sh.clone(),
                    pid.clone(),
                    sub.clone(),
                    p,
                    stop.clone(),
                )));
            }
        }
    }
    tasks
}

async fn sleep_or_stop(d: Duration, stop: &mut watch::Receiver<bool>) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(d) => *stop.borrow(),
        _ = stop.changed() => true,
    }
}

// ---------------- 中继（§7.3） ----------------

async fn relay_loop(sh: Arc<Shared>, mut stop: watch::Receiver<bool>) {
    let Some(redis) = main_redis(&sh) else {
        tracing::warn!("no [redis.main]; event relay disabled");
        return;
    };
    loop {
        let Some(lease) = Lease::try_acquire(&redis, "k:lock:outbox-relay", &sh.node_id, LEASE_TTL).await
        else {
            if sleep_or_stop(Duration::from_secs(5), &mut stop).await {
                return;
            }
            continue;
        };
        let mut listener = PgListener::connect_with(&sh.pool).await.ok();
        if let Some(l) = listener.as_mut() {
            let _ = l.listen("pt_outbox").await;
        }
        loop {
            if *stop.borrow() || lease.is_lost() {
                break;
            }
            match relay_once(&sh, &redis).await {
                Ok(n) if n > 0 => continue,
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "outbox relay failed"),
            }
            // 等待 NOTIFY 或 200 ms 兜底轮询
            match listener.as_mut() {
                Some(l) => {
                    tokio::select! {
                        _ = l.recv() => {}
                        _ = tokio::time::sleep(Duration::from_millis(200)) => {}
                        _ = stop.changed() => {}
                    }
                }
                None => {
                    if sleep_or_stop(Duration::from_millis(200), &mut stop).await {
                        break;
                    }
                }
            }
        }
        lease.release().await;
        if *stop.borrow() {
            return;
        }
    }
}

/// 一轮中继：取出最多 500 条，按顺序 XADD；Redis 全部成功才提交删除。
pub async fn relay_once(sh: &Shared, redis: &ConnectionManager) -> anyhow::Result<usize> {
    let mut tx = sh.pool.begin().await?;
    let rows = sqlx::query(
        "DELETE FROM kernel.event_outbox
          WHERE seq IN (SELECT seq FROM kernel.event_outbox ORDER BY seq LIMIT 500 FOR UPDATE SKIP LOCKED)
         RETURNING seq, event_type, partition, envelope",
    )
    .fetch_all(&mut *tx)
    .await?;
    if rows.is_empty() {
        tx.rollback().await?;
        return Ok(0);
    }
    let mut ordered: Vec<(i64, String, i32, Json)> = rows
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3)))
        .collect();
    ordered.sort_by_key(|r| r.0);
    let mut pipe = redis::pipe();
    for (_, ty, part, env) in &ordered {
        pipe.cmd("XADD")
            .arg(stream_key(ty, u32::try_from(*part).unwrap_or(0)))
            .arg("MAXLEN")
            .arg("~")
            .arg(sh.events_cfg.maxlen_for(ty))
            .arg("*")
            .arg("env")
            .arg(env.to_string());
    }
    let mut c = redis.clone();
    let _: Vec<Value> = pipe.query_async(&mut c).await?;
    tx.commit().await?;
    Ok(ordered.len())
}

// ---------------- 消费（§7.4） ----------------

fn group_name(plugin: &str, sub: &Subscription) -> String {
    format!("{plugin}.{}", sub.name)
}

/// 解析 XREADGROUP / XAUTOCLAIM 返回中的条目：(id, env JSON)。
fn entries(v: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    fn s(v: &Value) -> Option<String> {
        match v {
            Value::BulkString(b) => String::from_utf8(b.clone()).ok(),
            Value::SimpleString(s) => Some(s.clone()),
            _ => None,
        }
    }
    fn entry(e: &Value, out: &mut Vec<(String, String)>) {
        if let Value::Array(parts) = e
            && parts.len() == 2
            && let (Some(id), Value::Array(fields)) = (s(&parts[0]), &parts[1])
        {
            for kv in fields.chunks(2) {
                if kv.len() == 2
                    && s(&kv[0]).as_deref() == Some("env")
                    && let Some(env) = s(&kv[1])
                {
                    out.push((id.clone(), env));
                }
            }
        }
    }
    match v {
        // XREADGROUP: [[stream, [entries...]]]
        Value::Array(streams) => {
            for st in streams {
                if let Value::Array(pair) = st
                    && pair.len() >= 2
                    && let Value::Array(list) = &pair[1]
                {
                    for e in list {
                        entry(e, &mut out);
                    }
                }
            }
        }
        Value::Map(m) => {
            for (_, list) in m {
                if let Value::Array(list) = list {
                    for e in list {
                        entry(e, &mut out);
                    }
                }
            }
        }
        _ => {}
    }
    out
}

async fn consumer_loop(
    sh: Arc<Shared>,
    plugin: String,
    sub: Subscription,
    partition: u32,
    mut stop: watch::Receiver<bool>,
) {
    let Some(redis) = main_redis(&sh) else { return };
    let stream = stream_key(&sub.event, partition);
    let group = group_name(&plugin, &sub);
    let start = if sub.start.as_deref() == Some("earliest") {
        "0"
    } else {
        "$"
    };
    let Some(client) = sh.redis_clients.get("main").cloned() else {
        return;
    };
    // 专用连接：XREADGROUP BLOCK 会占住连接，且不能受共享连接的响应超时限制
    let dedicated = || async {
        client
            .get_multiplexed_async_connection_with_config(
                &redis::AsyncConnectionConfig::new().set_response_timeout(None),
            )
            .await
    };
    let mut c = loop {
        match dedicated().await {
            Ok(c) => break c,
            Err(e) => {
                tracing::warn!(error = %e, "consumer connection failed");
                if sleep_or_stop(Duration::from_secs(1), &mut stop).await {
                    return;
                }
            }
        }
    };
    let _: redis::RedisResult<Value> = redis::cmd("XGROUP")
        .arg("CREATE")
        .arg(&stream)
        .arg(&group)
        .arg(start)
        .arg("MKSTREAM")
        .query_async(&mut c)
        .await;
    let lease_key = format!("k:lock:evc:{group}:{}:{partition}", sub.event);
    loop {
        let Some(lease) = Lease::try_acquire(&redis, &lease_key, &sh.node_id, LEASE_TTL).await else {
            if sleep_or_stop(Duration::from_secs(5), &mut stop).await {
                return;
            }
            continue;
        };
        // 接管时先处理挂起的条目（本消费者名下与超时的）
        let mut pending = true;
        while !*stop.borrow() && !lease.is_lost() {
            let v: redis::RedisResult<Value> = if pending {
                redis::cmd("XAUTOCLAIM")
                    .arg(&stream)
                    .arg(&group)
                    .arg(&sh.node_id)
                    .arg(60_000)
                    .arg("0")
                    .arg("COUNT")
                    .arg(64)
                    .query_async(&mut c)
                    .await
                    .map(|v| match v {
                        // [next-cursor, [entries], [deleted]]
                        Value::Array(mut a) if a.len() >= 2 => {
                            Value::Array(vec![Value::Array(vec![Value::Nil, a.swap_remove(1)])])
                        }
                        other => other,
                    })
            } else {
                redis::cmd("XREADGROUP")
                    .arg("GROUP")
                    .arg(&group)
                    .arg(&sh.node_id)
                    .arg("COUNT")
                    .arg(64)
                    .arg("BLOCK")
                    .arg(1000)
                    .arg("STREAMS")
                    .arg(&stream)
                    .arg(">")
                    .query_async(&mut c)
                    .await
            };
            let batch = match v {
                Ok(v) => entries(&v),
                Err(e) => {
                    tracing::warn!(error = %e, %stream, "xreadgroup failed");
                    if sleep_or_stop(Duration::from_secs(1), &mut stop).await {
                        break;
                    }
                    if let Ok(nc) = dedicated().await {
                        c = nc;
                    }
                    continue;
                }
            };
            if pending && batch.is_empty() {
                pending = false;
                continue;
            }
            for (id, env) in batch {
                deliver(&sh, &plugin, &sub, &stream, &group, &id, &env, &mut stop).await;
                let _: redis::RedisResult<i64> = redis::cmd("XACK")
                    .arg(&stream)
                    .arg(&group)
                    .arg(&id)
                    .query_async(&mut c)
                    .await;
            }
        }
        lease.release().await;
        if *stop.borrow() {
            return;
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn deliver(
    sh: &Shared,
    plugin: &str,
    sub: &Subscription,
    stream: &str,
    group: &str,
    id: &str,
    env: &str,
    stop: &mut watch::Receiver<bool>,
) {
    let envelope: EventEnvelope = match serde_json::from_str(env) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(%stream, %id, error = %e, "malformed envelope; skipped");
            return;
        }
    };
    if !sub.versions.contains(&envelope.version) {
        return;
    }
    let mut attempt = 1u32;
    loop {
        match handle_event(sh, plugin, sub, &envelope, attempt).await {
            Ok(()) => return,
            Err(e) => {
                tracing::warn!(plugin, subscription = %sub.name, event = %envelope.id, attempt, error = %e, "event handler failed");
                if attempt >= sub.max_attempts {
                    dead_letter(sh, plugin, sub, &envelope, attempt, &e).await;
                    return;
                }
                let backoff = Duration::from_secs((1u64 << (attempt - 1).min(8)).min(300));
                if sleep_or_stop(backoff, stop).await {
                    // 关闭时不 ACK：留在 pending 中，由下一个持有者接管
                    tracing::info!(%group, %id, "shutdown during retry");
                    return;
                }
                attempt += 1;
            }
        }
    }
}

/// 执行一次事件处理：inbox 去重 + 插件 on_event（同一工作单元）。
pub async fn handle_event(
    sh: &Shared,
    plugin: &str,
    sub: &Subscription,
    env: &EventEnvelope,
    attempt: u32,
) -> Result<(), PluginError> {
    let rt = sh.plugin(plugin)?;
    let Some(handler) = rt.on_event.clone() else {
        return Err(PluginError::internal(
            "plugin subscribes to events but does not implement on_event",
        ));
    };
    let mut spec = CallSpec::new(InvocationKind::Event, Actor::system(), EVENT_TIMEOUT_MS);
    spec.traceparent = env.traceparent.clone();
    let (ctx, uow) = sh.make_ctx(plugin, spec)?;
    if !uow.claim_inbox(&sub.name, env.id).await? {
        uow.finish(false).await?;
        return Ok(());
    }
    if sub.tx == TxMode::Manual {
        // 手动事务：先提交 inbox 记录（处理方自行保证幂等）
        pt_plugin_api::host::Sql::commit(&*uow).await?;
    }
    let d = DeliveryInfo {
        subscription: sub.name.clone(),
        attempt,
        replay: false,
    };
    let budget = Duration::from_millis(u64::try_from(EVENT_TIMEOUT_MS).unwrap_or(30_000));
    let r = match tokio::time::timeout(budget, handler.handle(&ctx, env.clone(), d)).await {
        Ok(r) => r,
        Err(_) => Err(PluginError::new("kernel.timeout", "event handler timed out")),
    };
    uow.finish(r.is_ok()).await?;
    r
}

async fn dead_letter(
    sh: &Shared,
    plugin: &str,
    sub: &Subscription,
    env: &EventEnvelope,
    attempts: u32,
    e: &PluginError,
) {
    let r = sqlx::query(
        "INSERT INTO kernel.event_dead_letter (plugin_id, subscription, event_id, envelope, attempts, last_error)
         VALUES ($1,$2,$3,$4,$5,$6)",
    )
    .bind(plugin)
    .bind(&sub.name)
    .bind(env.id)
    .bind(serde_json::to_value(env).unwrap_or_default())
    .bind(i32::try_from(attempts).unwrap_or(i32::MAX))
    .bind(serde_json::to_value(e).unwrap_or_default())
    .execute(&sh.pool)
    .await;
    if let Err(err) = r {
        tracing::error!(error = %err, "failed to write dead letter");
    }
    tracing::error!(plugin, subscription = %sub.name, event = %env.id, "event moved to dead letter");
}

// ---------------- 延迟任务（§9.2） ----------------

async fn delayed_jobs_loop(sh: Arc<Shared>, mut stop: watch::Receiver<bool>) {
    let Some(redis) = main_redis(&sh) else { return };
    loop {
        let Some(lease) = Lease::try_acquire(&redis, "k:lock:delayed-jobs", &sh.node_id, LEASE_TTL).await
        else {
            if sleep_or_stop(Duration::from_secs(5), &mut stop).await {
                return;
            }
            continue;
        };
        while !*stop.borrow() && !lease.is_lost() {
            match run_due_jobs(&sh).await {
                Ok(n) if n > 0 => continue,
                Ok(_) => {}
                Err(e) => tracing::warn!(error = %e, "delayed jobs failed"),
            }
            if sleep_or_stop(Duration::from_secs(1), &mut stop).await {
                break;
            }
        }
        lease.release().await;
        if *stop.borrow() {
            return;
        }
    }
}

/// 执行到期的延迟任务；返回处理条数。失败最多重试 3 次（30 s、2 min、10 min）。
pub async fn run_due_jobs(sh: &Shared) -> anyhow::Result<usize> {
    let rows = sqlx::query(
        "UPDATE kernel.delayed_job SET state = 'running', attempts = attempts + 1, updated_at = now()
          WHERE id IN (SELECT id FROM kernel.delayed_job WHERE state = 'pending' AND run_at <= now()
                       ORDER BY run_at LIMIT 100 FOR UPDATE SKIP LOCKED)
         RETURNING id, plugin_id, name, run_at, payload, attempts",
    )
    .fetch_all(&sh.pool)
    .await?;
    for r in &rows {
        let id: uuid::Uuid = r.get(0);
        let plugin: String = r.get(1);
        let name: String = r.get(2);
        let run_at: chrono::DateTime<chrono::Utc> = r.get(3);
        let payload: Json = r.get(4);
        let attempts: i32 = r.get(5);
        let timeout_secs = sh
            .plugins
            .get(&plugin)
            .and_then(|rt| {
                rt.manifest
                    .jobs
                    .iter()
                    .find(|j| j.name == name)
                    .map(|j| j.timeout_secs)
            })
            .unwrap_or(300);
        let result = run_job(
            sh,
            &plugin,
            &name,
            run_at.timestamp_millis(),
            payload,
            &id.to_string(),
            timeout_secs,
        )
        .await;
        let (state, delay, err) = match result {
            Ok(()) => ("done", 0, None),
            Err(e) if attempts < 4 => (
                "pending",
                [30, 120, 600][usize::try_from(attempts - 1).unwrap_or(0).min(2)],
                Some(e),
            ),
            Err(e) => ("failed", 0, Some(e)),
        };
        sqlx::query(
            "UPDATE kernel.delayed_job SET state = $2, run_at = CASE WHEN $3 > 0 THEN now() + make_interval(secs => $3) ELSE run_at END,
                    last_error = $4, updated_at = now() WHERE id = $1",
        )
        .bind(id)
        .bind(state)
        .bind(f64::from(delay))
        .bind(err.map(|e| serde_json::to_value(e).unwrap_or_default()))
        .execute(&sh.pool)
        .await?;
    }
    Ok(rows.len())
}

async fn run_job(
    sh: &Shared,
    plugin: &str,
    name: &str,
    scheduled_at: i64,
    payload: Json,
    run_id: &str,
    timeout_secs: u32,
) -> Result<(), PluginError> {
    let rt = sh.plugin(plugin)?;
    let handler = rt
        .on_cron
        .clone()
        .ok_or_else(|| PluginError::internal("plugin has jobs but no on_cron"))?;
    let timeout_ms = i64::from(timeout_secs) * 1000;
    let (ctx, uow) = sh.make_ctx(
        plugin,
        CallSpec::new(InvocationKind::Cron, Actor::system(), timeout_ms),
    )?;
    let job = JobInvocation {
        name: name.to_owned(),
        kind: JobKind::Delayed,
        scheduled_at,
        payload,
        run_id: run_id.to_owned(),
    };
    let budget = Duration::from_millis(u64::try_from(timeout_ms).unwrap_or(300_000));
    let r = match tokio::time::timeout(budget, handler.run(&ctx, job)).await {
        Ok(r) => r,
        Err(_) => Err(PluginError::new("kernel.timeout", "job timed out")),
    };
    uow.finish(r.is_ok()).await?;
    r
}

// ---------------- 分区 worker（§9.4） ----------------

/// 读取分区数（来源键的 `partitions` 字段），每 30 s 刷新一次；为每个分区启动一个租约循环。
async fn worker_supervisor(
    sh: Arc<Shared>,
    plugin: String,
    decl: pt_plugin_api::manifest::WorkerDecl,
    worker: Arc<dyn pt_plugin_api::PartitionWorker>,
    mut stop: watch::Receiver<bool>,
) {
    let Some(redis) = sh
        .redis
        .get(&decl.redis_role)
        .or_else(|| sh.redis.get("main"))
        .cloned()
    else {
        tracing::warn!(%plugin, worker = %decl.name, "no redis for worker; not started");
        return;
    };
    let mut running: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    loop {
        let mut c = redis.clone();
        let n: u32 = redis::cmd("HGET")
            .arg(&decl.partitions_from)
            .arg("partitions")
            .query_async::<Option<u32>>(&mut c)
            .await
            .ok()
            .flatten()
            .unwrap_or(decl.default_partitions)
            .clamp(1, 4096);
        while u32::try_from(running.len()).unwrap_or(u32::MAX) < n {
            let p = u32::try_from(running.len()).unwrap_or(0);
            running.push(tokio::spawn(partition_loop(
                sh.clone(),
                plugin.clone(),
                decl.name.clone(),
                worker.clone(),
                p,
                stop.clone(),
            )));
        }
        if sleep_or_stop(Duration::from_secs(30), &mut stop).await {
            break;
        }
    }
    for t in running {
        let _ = t.await;
    }
}

async fn partition_loop(
    sh: Arc<Shared>,
    plugin: String,
    name: String,
    worker: Arc<dyn pt_plugin_api::PartitionWorker>,
    partition: u32,
    mut stop: watch::Receiver<bool>,
) {
    let Some(redis) = main_redis(&sh) else { return };
    let key = format!("k:lock:worker:{plugin}:{name}:{partition}");
    loop {
        let Some(lease) = Lease::try_acquire(&redis, &key, &sh.node_id, LEASE_TTL).await else {
            if sleep_or_stop(Duration::from_secs(5), &mut stop).await {
                return;
            }
            continue;
        };
        tracing::debug!(%plugin, worker = %name, partition, fence = lease.fence(), "partition acquired");
        while !*stop.borrow() && !lease.is_lost() {
            let pause = match poll_once(&sh, &plugin, &worker, partition, lease.fence()).await {
                Ok(pt_plugin_api::PollOutcome::Busy) => Duration::ZERO,
                Ok(pt_plugin_api::PollOutcome::Idle { sleep_ms }) => {
                    Duration::from_millis(sleep_ms.clamp(10, 10_000))
                }
                Err(e) => {
                    tracing::warn!(%plugin, worker = %name, partition, error = %e, "worker poll failed");
                    Duration::from_secs(1)
                }
            };
            if !pause.is_zero() && sleep_or_stop(pause, &mut stop).await {
                break;
            }
        }
        lease.release().await;
        if *stop.borrow() {
            return;
        }
    }
}

const WORKER_TIMEOUT_MS: i64 = 30_000;

async fn poll_once(
    sh: &Shared,
    plugin: &str,
    worker: &Arc<dyn pt_plugin_api::PartitionWorker>,
    partition: u32,
    fence: u64,
) -> Result<pt_plugin_api::PollOutcome, PluginError> {
    let (ctx, uow) = sh.make_ctx(
        plugin,
        CallSpec::new(InvocationKind::Worker, Actor::system(), WORKER_TIMEOUT_MS),
    )?;
    let budget = Duration::from_millis(u64::try_from(WORKER_TIMEOUT_MS).unwrap_or(30_000));
    let r = match tokio::time::timeout(budget, worker.poll(&ctx, partition, fence)).await {
        Ok(r) => r,
        Err(_) => Err(PluginError::new("kernel.timeout", "worker poll timed out")),
    };
    uow.finish(r.is_ok()).await?;
    r
}
