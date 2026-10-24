//! Prometheus 指标：请求数（端点 / 结果 / 失败原因）、announce 事件、announce 延迟直方图、准入缓存规模。
//! 由 `metrics_bind` 指定的独立地址暴露 `GET /metrics`（tracker 端口对公网开放，指标不与它共用）。

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    sync::{LazyLock, Mutex},
};

const BUCKETS: &[f64] = &[0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0];

#[derive(Default)]
struct Inner {
    requests: BTreeMap<(&'static str, &'static str, &'static str), u64>,
    events: BTreeMap<&'static str, u64>,
    buckets: Vec<u64>,
    sum: f64,
    count: u64,
}

static M: LazyLock<Mutex<Inner>> = LazyLock::new(|| {
    Mutex::new(Inner {
        buckets: vec![0; BUCKETS.len()],
        ..Inner::default()
    })
});

/// `endpoint`：announce / scrape；`reason`：失败原因（成功为空）
pub fn request(endpoint: &'static str, reason: Option<&'static str>) {
    if let Ok(mut g) = M.lock() {
        let k = (endpoint, if reason.is_some() { "failure" } else { "ok" }, reason.unwrap_or(""));
        *g.requests.entry(k).or_default() += 1;
    }
}

pub fn event(ev: &'static str) {
    if let Ok(mut g) = M.lock() {
        *g.events.entry(ev).or_default() += 1;
    }
}

/// 已处理的 announce 总数（节点状态的 announce 速率）
pub fn announces_total() -> u64 {
    M.lock().map_or(0, |g| g.count)
}

pub fn announce_latency(secs: f64) {
    if let Ok(mut g) = M.lock() {
        for (i, b) in BUCKETS.iter().enumerate() {
            if secs <= *b {
                g.buckets[i] += 1;
            }
        }
        g.sum += secs;
        g.count += 1;
    }
}

pub fn render(users: usize, torrents: usize) -> String {
    let Ok(g) = M.lock() else { return String::new() };
    let mut o = String::new();
    let _ = writeln!(o, "# HELP pt_tracker_requests_total Requests by endpoint, result and failure reason");
    let _ = writeln!(o, "# TYPE pt_tracker_requests_total counter");
    for ((ep, res, reason), n) in &g.requests {
        let _ = writeln!(o, "pt_tracker_requests_total{{endpoint=\"{ep}\",result=\"{res}\",reason=\"{reason}\"}} {n}");
    }
    let _ = writeln!(o, "# HELP pt_tracker_announce_events_total Accepted announces by event");
    let _ = writeln!(o, "# TYPE pt_tracker_announce_events_total counter");
    for (ev, n) in &g.events {
        let _ = writeln!(o, "pt_tracker_announce_events_total{{event=\"{ev}\"}} {n}");
    }
    let _ = writeln!(o, "# HELP pt_tracker_announce_duration_seconds Announce handling latency");
    let _ = writeln!(o, "# TYPE pt_tracker_announce_duration_seconds histogram");
    for (b, n) in BUCKETS.iter().zip(&g.buckets) {
        let _ = writeln!(o, "pt_tracker_announce_duration_seconds_bucket{{le=\"{b}\"}} {n}");
    }
    let _ = writeln!(o, "pt_tracker_announce_duration_seconds_bucket{{le=\"+Inf\"}} {}", g.count);
    let _ = writeln!(o, "pt_tracker_announce_duration_seconds_sum {}", g.sum);
    let _ = writeln!(o, "pt_tracker_announce_duration_seconds_count {}", g.count);
    let _ = writeln!(o, "# HELP pt_tracker_admission_cached Cached passkeys and torrents");
    let _ = writeln!(o, "# TYPE pt_tracker_admission_cached gauge");
    let _ = writeln!(o, "pt_tracker_admission_cached{{kind=\"passkeys\"}} {users}");
    let _ = writeln!(o, "pt_tracker_admission_cached{{kind=\"torrents\"}} {torrents}");
    o
}
