//! Prometheus 指标（kernel/03 §11）：进程内注册表，文本格式输出。
//!
//! - 内核指标：`pt_http_requests_total{plugin,route,status}`、`pt_http_request_duration_seconds{plugin,route}`、
//!   `pt_hook_invocations_total{hook,plugin,outcome}`、`pt_job_runs_total{plugin,job,outcome}`。
//! - 插件经 `ctx.metrics` 写入，名称自动加 `pt_plugin_<id>_` 前缀；每个插件最多 1000 个序列、每个序列最多 5 个标签，超出的丢弃。
//! - 由 `[http] metrics_bind` 指定的独立地址暴露 `GET /metrics`（不经网关，不要对公网开放）。

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    sync::{LazyLock, Mutex},
};

use pt_plugin_api::host::Metrics;

/// 直方图桶（秒）
const BUCKETS: &[f64] = &[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0];
const MAX_SERIES_PER_PLUGIN: usize = 1000;
const MAX_LABELS: usize = 5;

#[derive(Debug, Clone)]
enum Value {
    Counter(u64),
    Gauge(f64),
    Histogram { buckets: Vec<u64>, sum: f64, count: u64 },
}

#[derive(Default)]
struct Inner {
    /// 名称 → (标签串 → 值)
    series: BTreeMap<String, BTreeMap<String, Value>>,
    help: BTreeMap<String, &'static str>,
    per_owner: BTreeMap<String, usize>,
}

pub struct Registry(Mutex<Inner>);

pub static REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
    let r = Registry(Mutex::new(Inner::default()));
    r.describe("pt_http_requests_total", "HTTP requests handled by plugin routes");
    r.describe("pt_http_request_duration_seconds", "HTTP request latency by plugin route");
    r.describe("pt_hook_invocations_total", "Hook handler invocations by outcome");
    r.describe("pt_job_runs_total", "Cron and delayed job runs by outcome");
    r
});

fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 120
        && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && !s.as_bytes()[0].is_ascii_digit()
}

fn escape(v: &str) -> String {
    v.chars()
        .take(64)
        .flat_map(|c| match c {
            '\\' => vec!['\\', '\\'],
            '"' => vec!['\\', '"'],
            '\n' => vec!['\\', 'n'],
            c => vec![c],
        })
        .collect()
}

fn label_str(labels: &[(&str, &str)]) -> Option<String> {
    if labels.len() > MAX_LABELS || labels.iter().any(|(k, _)| !valid_name(k)) {
        return None;
    }
    let mut ls: Vec<(&str, &str)> = labels.to_vec();
    ls.sort_unstable();
    Some(
        ls.iter()
            .map(|(k, v)| format!("{k}=\"{}\"", escape(v)))
            .collect::<Vec<_>>()
            .join(","),
    )
}

impl Registry {
    fn describe(&self, name: &str, help: &'static str) {
        if let Ok(mut g) = self.0.lock() {
            g.help.insert(name.to_owned(), help);
        }
    }

    fn update(&self, owner: &str, name: &str, labels: &[(&str, &str)], f: impl FnOnce(Option<Value>) -> Value) {
        if !valid_name(name) {
            return;
        }
        let Some(ls) = label_str(labels) else { return };
        let Ok(mut g) = self.0.lock() else { return };
        let exists = g.series.get(name).is_some_and(|m| m.contains_key(&ls));
        if !exists {
            let n = g.per_owner.entry(owner.to_owned()).or_insert(0);
            if owner != "kernel" && *n >= MAX_SERIES_PER_PLUGIN {
                return;
            }
            *n += 1;
        }
        let m = g.series.entry(name.to_owned()).or_default();
        let old = m.remove(&ls);
        m.insert(ls, f(old));
    }

    pub fn counter(&self, owner: &str, name: &str, v: u64, labels: &[(&str, &str)]) {
        self.update(owner, name, labels, |old| match old {
            Some(Value::Counter(c)) => Value::Counter(c.saturating_add(v)),
            _ => Value::Counter(v),
        });
    }

    pub fn gauge(&self, owner: &str, name: &str, v: f64, labels: &[(&str, &str)]) {
        if v.is_finite() {
            self.update(owner, name, labels, |_| Value::Gauge(v));
        }
    }

    pub fn observe(&self, owner: &str, name: &str, v: f64, labels: &[(&str, &str)]) {
        if !v.is_finite() {
            return;
        }
        self.update(owner, name, labels, |old| {
            let (mut buckets, mut sum, mut count) = match old {
                Some(Value::Histogram { buckets, sum, count }) => (buckets, sum, count),
                _ => (vec![0; BUCKETS.len()], 0.0, 0),
            };
            for (i, b) in BUCKETS.iter().enumerate() {
                if v <= *b {
                    buckets[i] += 1;
                }
            }
            sum += v;
            count += 1;
            Value::Histogram { buckets, sum, count }
        });
    }

    /// Prometheus 文本格式（0.0.4）
    pub fn render(&self) -> String {
        let Ok(g) = self.0.lock() else { return String::new() };
        let mut out = String::new();
        for (name, series) in &g.series {
            let kind = match series.values().next() {
                Some(Value::Counter(_)) => "counter",
                Some(Value::Gauge(_)) => "gauge",
                Some(Value::Histogram { .. }) => "histogram",
                None => continue,
            };
            if let Some(h) = g.help.get(name) {
                let _ = writeln!(out, "# HELP {name} {h}");
            }
            let _ = writeln!(out, "# TYPE {name} {kind}");
            for (ls, v) in series {
                let braces = |extra: &str| {
                    let all = [ls.as_str(), extra].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(",");
                    if all.is_empty() { String::new() } else { format!("{{{all}}}") }
                };
                match v {
                    Value::Counter(c) => {
                        let _ = writeln!(out, "{name}{} {c}", braces(""));
                    }
                    Value::Gauge(x) => {
                        let _ = writeln!(out, "{name}{} {x}", braces(""));
                    }
                    Value::Histogram { buckets, sum, count } => {
                        for (b, n) in BUCKETS.iter().zip(buckets) {
                            let _ = writeln!(out, "{name}_bucket{} {n}", braces(&format!("le=\"{b}\"")));
                        }
                        let _ = writeln!(out, "{name}_bucket{} {count}", braces("le=\"+Inf\""));
                        let _ = writeln!(out, "{name}_sum{} {sum}", braces(""));
                        let _ = writeln!(out, "{name}_count{} {count}", braces(""));
                    }
                }
            }
        }
        out
    }
}

/// 插件的 `ctx.metrics`：名称加 `pt_plugin_<id>_` 前缀
pub struct PluginMetrics {
    pub plugin: String,
}

impl PluginMetrics {
    fn name(&self, n: &str) -> String {
        let prefix = format!("pt_plugin_{}_", self.plugin.replace('-', "_"));
        if n.starts_with(&prefix) { n.to_owned() } else { format!("{prefix}{n}") }
    }
}

impl Metrics for PluginMetrics {
    fn counter_add(&self, name: &str, v: u64, labels: &[(&str, &str)]) {
        REGISTRY.counter(&self.plugin, &self.name(name), v, labels);
    }
    fn gauge_set(&self, name: &str, v: f64, labels: &[(&str, &str)]) {
        REGISTRY.gauge(&self.plugin, &self.name(name), v, labels);
    }
    fn histogram_observe(&self, name: &str, v: f64, labels: &[(&str, &str)]) {
        REGISTRY.observe(&self.plugin, &self.name(name), v, labels);
    }
}

/// `GET /metrics`
pub fn router() -> axum::Router {
    axum::Router::new().route(
        "/metrics",
        axum::routing::get(|| async {
            (
                [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
                REGISTRY.render(),
            )
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_prometheus_text() {
        let r = Registry(Mutex::new(Inner::default()));
        r.counter("kernel", "x_total", 2, &[("b", "2"), ("a", "1")]);
        r.counter("kernel", "x_total", 3, &[("a", "1"), ("b", "2")]);
        r.gauge("kernel", "g", 1.5, &[]);
        r.observe("kernel", "h_seconds", 0.02, &[("r", "x\"y")]);
        r.counter("kernel", "Bad-Name", 1, &[]);
        let t = r.render();
        assert!(t.contains("x_total{a=\"1\",b=\"2\"} 5"), "{t}");
        assert!(t.contains("g 1.5"), "{t}");
        assert!(t.contains("h_seconds_bucket{r=\"x\\\"y\",le=\"0.025\"} 1"), "{t}");
        assert!(t.contains("h_seconds_bucket{r=\"x\\\"y\",le=\"0.01\"} 0"), "{t}");
        assert!(t.contains("h_seconds_count{r=\"x\\\"y\"} 1"), "{t}");
        assert!(!t.contains("Bad"));
        // 插件序列上限
        for i in 0..(MAX_SERIES_PER_PLUGIN + 10) {
            r.counter("p", "p_total", 1, &[("i", &i.to_string())]);
        }
        assert_eq!(r.render().matches("p_total{").count(), MAX_SERIES_PER_PLUGIN);
        let pm = PluginMetrics { plugin: "rss-feed".into() };
        assert_eq!(pm.name("hits_total"), "pt_plugin_rss_feed_hits_total");
    }
}
