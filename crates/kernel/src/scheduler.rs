//! 定时任务（kernel/03 §9.1）：5 段 cron（分 时 日 月 周），按站点时区解释。
//!
//! 每个副本独立计算下次触发时间；到点后以 `SET k:cron:<plugin>:<job>:<fire_ts> NX` 去重，
//! 以租约 `k:lock:cron:<plugin>:<job>` 禁止重叠；运行记录写 `kernel.cron_run`。

use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Datelike, Duration as CDuration, TimeZone, Timelike, Utc};
use chrono_tz::Tz;
use pt_plugin_api::{Actor, InvocationKind, JobInvocation, JobKind, PluginError, manifest::CronDecl};
use tokio::sync::watch;

use crate::{
    cluster::Lease,
    runtime::{CallSpec, Shared},
};

/// 解析后的 cron 表达式（各字段为允许值的位集）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cron {
    minutes: u64,
    hours: u32,
    days: u32,
    months: u16,
    weekdays: u8,
    /// 日与周都不是 `*` 时按 cron 惯例取“或”
    dom_star: bool,
    dow_star: bool,
}

fn field(spec: &str, lo: u32, hi: u32) -> Result<(u64, bool), String> {
    let mut bits = 0u64;
    let star = spec == "*";
    for part in spec.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.parse::<u32>().map_err(|_| format!("bad step in `{part}`"))?),
            None => (part, 1),
        };
        if step == 0 {
            return Err(format!("zero step in `{part}`"));
        }
        let (a, b) = if range == "*" {
            (lo, hi)
        } else if let Some((a, b)) = range.split_once('-') {
            (
                a.parse().map_err(|_| format!("bad range `{range}`"))?,
                b.parse().map_err(|_| format!("bad range `{range}`"))?,
            )
        } else {
            let v: u32 = range.parse().map_err(|_| format!("bad value `{range}`"))?;
            // `5/15` 表示从 5 起每 15
            if part.contains('/') { (v, hi) } else { (v, v) }
        };
        if a < lo || b > hi || a > b {
            return Err(format!("`{part}` out of range {lo}-{hi}"));
        }
        let mut v = a;
        while v <= b {
            bits |= 1 << v;
            v += step;
        }
    }
    Ok((bits, star))
}

impl Cron {
    pub fn parse(s: &str) -> Result<Self, String> {
        let f: Vec<&str> = s.split_whitespace().collect();
        if f.len() != 5 {
            return Err("cron expression must have 5 fields (min hour day month weekday)".into());
        }
        let (minutes, _) = field(f[0], 0, 59)?;
        let (hours, _) = field(f[1], 0, 23)?;
        let (days, dom_star) = field(f[2], 1, 31)?;
        let (months, _) = field(f[3], 1, 12)?;
        // 周：0 与 7 都表示周日
        let (mut weekdays, dow_star) = field(f[4], 0, 7)?;
        if weekdays & (1 << 7) != 0 {
            weekdays |= 1;
        }
        Ok(Self {
            minutes,
            hours: u32::try_from(hours).unwrap_or(0),
            days: u32::try_from(days).unwrap_or(0),
            months: u16::try_from(months).unwrap_or(0),
            weekdays: u8::try_from(weekdays & 0x7f).unwrap_or(0),
            dom_star,
            dow_star,
        })
    }

    fn day_matches<T: Datelike>(&self, d: &T) -> bool {
        let dom = self.days & (1 << d.day()) != 0;
        let dow = self.weekdays & (1 << d.weekday().num_days_from_sunday()) != 0;
        match (self.dom_star, self.dow_star) {
            (true, true) => true,
            (true, false) => dow,
            (false, true) => dom,
            (false, false) => dom || dow,
        }
    }

    /// 严格晚于 `after` 的下一次触发时间（按 `tz` 的本地时间匹配；夏令时跳过的本地时间不触发）。
    pub fn next_after(&self, after: DateTime<Utc>, tz: Tz) -> Option<DateTime<Utc>> {
        let local = after.with_timezone(&tz).naive_local();
        let mut t = local.with_second(0)?.with_nanosecond(0)? + CDuration::minutes(1);
        // 最多向前搜索 5 年（按分钟步进会过慢，因此按天 / 小时跳跃）
        let limit = t + CDuration::days(366 * 5);
        while t < limit {
            if self.months & (1 << t.month()) == 0 || !self.day_matches(&t) {
                t = (t.date() + CDuration::days(1)).and_hms_opt(0, 0, 0)?;
                continue;
            }
            if self.hours & (1 << t.hour()) == 0 {
                t = t.with_minute(0)? + CDuration::hours(1);
                continue;
            }
            if self.minutes & (1 << t.minute()) == 0 {
                t += CDuration::minutes(1);
                continue;
            }
            if let Some(dt) = tz.from_local_datetime(&t).earliest() {
                return Some(dt.with_timezone(&Utc));
            }
            t += CDuration::minutes(1);
        }
        None
    }

    /// 不晚于 `at` 的上一次触发时间（catch_up 判断用）：窗口由小到大回溯，找到后在窗口内前推到最后一个触发点。
    pub fn prev_at_or_before(&self, at: DateTime<Utc>, tz: Tz) -> Option<DateTime<Utc>> {
        for back in [
            CDuration::hours(2),
            CDuration::days(2),
            CDuration::days(9),
            CDuration::days(63),
            CDuration::days(800),
        ] {
            let Some(mut cur) = self.next_after(at - back, tz) else {
                continue;
            };
            if cur > at {
                continue;
            }
            while let Some(n) = self.next_after(cur, tz) {
                if n > at {
                    break;
                }
                cur = n;
            }
            return Some(cur);
        }
        None
    }
}

pub fn site_tz(sh: &Shared) -> Tz {
    sh.cfg.site.timezone.parse().unwrap_or(Tz::UTC)
}

/// 启动全部 cron 任务的调度循环。
pub fn spawn(sh: Arc<Shared>, stop: watch::Receiver<bool>) -> Vec<tokio::task::JoinHandle<()>> {
    let mut tasks = Vec::new();
    for (pid, rt) in &sh.plugins {
        for c in &rt.manifest.cron {
            match Cron::parse(&c.schedule) {
                Ok(cron) => tasks.push(tokio::spawn(cron_loop(
                    sh.clone(),
                    pid.clone(),
                    c.clone(),
                    cron,
                    stop.clone(),
                ))),
                Err(e) => {
                    tracing::error!(plugin = %pid, job = %c.name, error = %e, "invalid cron schedule; job disabled")
                }
            }
        }
    }
    tasks
}

async fn cron_loop(
    sh: Arc<Shared>,
    plugin: String,
    decl: CronDecl,
    cron: Cron,
    mut stop: watch::Receiver<bool>,
) {
    let tz = site_tz(&sh);
    // catch_up = "once"：最近一次成功早于“上一次应触发时间”时补跑
    if decl.catch_up.as_deref() == Some("once")
        && let Some(prev) = cron.prev_at_or_before(Utc::now(), tz)
    {
        let last_ok: Option<DateTime<Utc>> = sqlx::query_scalar(
            "SELECT max(scheduled_at) FROM kernel.cron_run WHERE plugin_id = $1 AND job = $2 AND outcome = 'ok'",
        )
        .bind(&plugin)
        .bind(&decl.name)
        .fetch_one(&sh.pool)
        .await
        .ok()
        .flatten();
        if last_ok.is_none_or(|t| t < prev) {
            fire(&sh, &plugin, &decl, prev, JobKind::Cron, serde_json::json!({})).await;
        }
    }
    let mut after = Utc::now();
    loop {
        let Some(next) = cron.next_after(after, tz) else {
            return;
        };
        let wait = (next - Utc::now()).to_std().unwrap_or(Duration::ZERO);
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            _ = stop.changed() => return,
        }
        if *stop.borrow() {
            return;
        }
        fire(&sh, &plugin, &decl, next, JobKind::Cron, serde_json::json!({})).await;
        after = next;
    }
}

/// 触发一次（去重、禁止重叠、记录运行）。返回运行结果；被其他副本抢先或仍在运行时返回 None。
pub async fn fire(
    sh: &Shared,
    plugin: &str,
    decl: &CronDecl,
    at: DateTime<Utc>,
    kind: JobKind,
    payload: serde_json::Value,
) -> Option<Result<(), PluginError>> {
    let redis = sh.redis.get("main")?;
    let mut c = redis.clone();
    if kind == JobKind::Cron {
        let key = format!("k:cron:{plugin}:{}:{}", decl.name, at.timestamp());
        let won: Option<String> = redis::cmd("SET")
            .arg(&key)
            .arg(&sh.node_id)
            .arg("NX")
            .arg("EX")
            .arg(86_400)
            .query_async(&mut c)
            .await
            .ok()?;
        won?;
    }
    let lock_key = format!("k:lock:cron:{plugin}:{}", decl.name);
    let run_id = uuid::Uuid::now_v7();
    let kind_s = if kind == JobKind::Manual { "manual" } else { "cron" };
    let Some(lease) = Lease::try_acquire(redis, &lock_key, &sh.node_id, Duration::from_secs(30)).await else {
        let _ = sqlx::query(
            "INSERT INTO kernel.cron_run (run_id, plugin_id, job, kind, scheduled_at, started_at, finished_at, node_id, outcome)
             VALUES ($1,$2,$3,$4,$5,now(),now(),$6,'skipped')",
        )
        .bind(run_id)
        .bind(plugin)
        .bind(&decl.name)
        .bind(kind_s)
        .bind(at)
        .bind(&sh.node_id)
        .execute(&sh.pool)
        .await;
        tracing::warn!(plugin, job = %decl.name, "previous run still in progress; skipped");
        return None;
    };
    let _ = sqlx::query(
        "INSERT INTO kernel.cron_run (run_id, plugin_id, job, kind, scheduled_at, started_at, node_id) VALUES ($1,$2,$3,$4,$5,now(),$6)",
    )
    .bind(run_id)
    .bind(plugin)
    .bind(&decl.name)
    .bind(kind_s)
    .bind(at)
    .bind(&sh.node_id)
    .execute(&sh.pool)
    .await;
    let r = run(sh, plugin, decl, at, kind, payload, &run_id.to_string(), &lease).await;
    let (outcome, err) = match &r {
        Ok(()) => ("ok", None),
        Err(e) if e.code == "kernel.timeout" => ("timeout", Some(e)),
        Err(e) if e.code == "kernel.lock_lost" => ("lock_lost", Some(e)),
        Err(e) => ("failed", Some(e)),
    };
    crate::metrics::REGISTRY.counter(
        "kernel",
        "pt_job_runs_total",
        1,
        &[("plugin", plugin), ("job", &decl.name), ("outcome", outcome)],
    );
    let _ = sqlx::query(
        "UPDATE kernel.cron_run SET finished_at = now(), outcome = $2, error = $3 WHERE run_id = $1",
    )
    .bind(run_id)
    .bind(outcome)
    .bind(err.map(|e| serde_json::to_value(e).unwrap_or_default()))
    .execute(&sh.pool)
    .await;
    if let Some(e) = err {
        tracing::warn!(plugin, job = %decl.name, error = %e, "cron job failed");
    }
    lease.release().await;
    Some(r)
}

#[allow(clippy::too_many_arguments)]
async fn run(
    sh: &Shared,
    plugin: &str,
    decl: &CronDecl,
    at: DateTime<Utc>,
    kind: JobKind,
    payload: serde_json::Value,
    run_id: &str,
    lease: &Lease,
) -> Result<(), PluginError> {
    let rt = sh.plugin(plugin)?;
    let handler = rt
        .on_cron
        .clone()
        .ok_or_else(|| PluginError::internal("plugin declares cron but has no on_cron"))?;
    let timeout_ms = i64::from(decl.timeout_secs) * 1000;
    let (ctx, uow) = sh.make_ctx(
        plugin,
        CallSpec::new(InvocationKind::Cron, Actor::system(), timeout_ms),
    )?;
    let job = JobInvocation {
        name: decl.name.clone(),
        kind,
        scheduled_at: at.timestamp_millis(),
        payload,
        run_id: run_id.to_owned(),
    };
    let mut lost = lease.lost();
    let budget = Duration::from_millis(u64::try_from(timeout_ms).unwrap_or(300_000));
    let r = tokio::select! {
        r = tokio::time::timeout(budget, handler.run(&ctx, job)) => match r {
            Ok(r) => r,
            Err(_) => Err(PluginError::new("kernel.timeout", "cron job timed out")),
        },
        _ = lost.changed() => Err(PluginError::new("kernel.lock_lost", "cron lock lost")),
    };
    uow.finish(r.is_ok()).await?;
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s)
            .map(|d| d.with_timezone(&Utc))
            .unwrap_or_default()
    }

    #[test]
    fn parses_and_computes_next() {
        let c = Cron::parse("40 * * * *").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            c.next_after(at("2026-10-07T08:39:59Z"), Tz::UTC),
            Some(at("2026-10-07T08:40:00Z"))
        );
        assert_eq!(
            c.next_after(at("2026-10-07T08:40:00Z"), Tz::UTC),
            Some(at("2026-10-07T09:40:00Z"))
        );
        let c = Cron::parse("*/15 2-3 * * 1-5").unwrap_or_else(|e| panic!("{e}"));
        // 2026-10-10 是周六 → 下一个工作日周一 10-12
        assert_eq!(
            c.next_after(at("2026-10-09T03:50:00Z"), Tz::UTC),
            Some(at("2026-10-12T02:00:00Z"))
        );
        // 时区：上海 00:05 = UTC 前一日 16:05
        let c = Cron::parse("5 0 * * *").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            c.next_after(at("2026-10-07T00:00:00Z"), Tz::Asia__Shanghai),
            Some(at("2026-10-07T16:05:00Z"))
        );
        // 日与周都限定时取“或”：每月 1 日或周日
        let c = Cron::parse("0 0 1 * 0").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            c.next_after(at("2026-10-07T00:00:00Z"), Tz::UTC),
            Some(at("2026-10-11T00:00:00Z"))
        );
        assert_eq!(
            c.prev_at_or_before(at("2026-10-07T00:00:00Z"), Tz::UTC),
            Some(at("2026-10-04T00:00:00Z"))
        );
        for bad in ["* * * *", "60 * * * *", "*/0 * * * *", "a * * * *", "5-1 * * * *"] {
            assert!(Cron::parse(bad).is_err(), "{bad}");
        }
    }
}
