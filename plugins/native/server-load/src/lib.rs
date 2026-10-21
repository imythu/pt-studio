//! server-load 插件（Native）：首页侧栏的服务器负载。
//!
//! - `GET /v1/status`：
//!   - 站点：注册用户数、用户上限（本插件设置 `user_limit`，为 0 时取 `ctx.registration` 的全站用户上限）、
//!     今日新注册、近一小时活跃用户（有活跃连接的用户）与连接数（做种 / 下载）；
//!   - 主机（设置 `show_system`，默认开）：每个 CPU 核心的使用率与 iowait、内存 / 交换区、各磁盘读写吞吐与繁忙度、
//!     本进程（pt-server）的 CPU / 常驻内存 / 线程数、负载均值与运行时长。读 `/proc`（容器内看到的是宿主机的值），两次采样求差；采样结果缓存 2 秒，
//!     上次采样在 1–60 秒之内时直接用它作基准，否则现场间隔 500ms 再采一次。非 Linux 返回 `system: null`。
//! - 可见范围：设置 `visible_classes`（等级 ID）与 `visible_roles`（角色）都为空时所有登录用户可见；
//!   否则用户的等级在列表中、或拥有列表中的任一角色即可查看（满足其一）；管理员始终可见。
//!   无权查看时返回 `{ "visible": false }`（不报错，前端据此不显示组件）。

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{Json, Router, response::IntoResponse, routing::get};
use pt_plugin_api::{HttpRoutes, NativePlugin, PResult, PluginError, RequestCtx};
use serde_json::{Value, json};
use sqlx::Row;

pub const MANIFEST: &str = include_str!("../plugin.toml");

/// 后台设置
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub user_limit: i64,
    pub classes: Vec<i64>,
    pub roles: Vec<String>,
    pub show_system: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { user_limit: 0, classes: Vec::new(), roles: Vec::new(), show_system: true }
    }
}

impl Settings {
    pub fn from_json(s: &Value) -> Self {
        Self {
            user_limit: s["user_limit"].as_i64().unwrap_or(0).max(0),
            classes: s["visible_classes"].as_array().into_iter().flatten().filter_map(Value::as_i64).collect(),
            roles: s["visible_roles"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            show_system: s["show_system"].as_bool().unwrap_or(true),
        }
    }
}

/// 管理员始终可见
const STAFF_ROLES: &[&str] = &["admin", "sysop"];

/// 可见性判定（纯函数）：未设限制时所有人可见；否则等级或角色满足其一
pub fn visible(s: &Settings, class_id: Option<i64>, roles: &[String]) -> bool {
    if roles.iter().any(|r| STAFF_ROLES.contains(&r.as_str())) {
        return true;
    }
    if s.classes.is_empty() && s.roles.is_empty() {
        return true;
    }
    class_id.is_some_and(|c| s.classes.contains(&c)) || roles.iter().any(|r| s.roles.contains(r))
}

// ---------------------------------------------------------------- /proc 采样

/// 一个 CPU（`cpu` 汇总或 `cpuN`）的累计时间片
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CpuTimes {
    pub total: u64,
    pub idle: u64,
    pub iowait: u64,
}

/// 一块磁盘的累计计数（扇区固定 512 字节）
#[derive(Debug, Clone, PartialEq)]
pub struct DiskTimes {
    pub name: String,
    pub read_sectors: u64,
    pub write_sectors: u64,
    pub io_ms: u64,
}

#[derive(Debug, Clone)]
struct Snapshot {
    at: Instant,
    /// `[0]` 为汇总，其后按核心序号
    cpus: Vec<CpuTimes>,
    disks: Vec<DiskTimes>,
    /// 本进程累计 CPU 时间片（utime + stime）
    proc_ticks: u64,
}

/// 解析 `/proc/stat` 的 `cpu*` 行：user nice system idle iowait irq softirq steal（guest 已计入 user，不重复加）
pub fn parse_stat(text: &str) -> Vec<CpuTimes> {
    text.lines()
        .filter(|l| l.starts_with("cpu"))
        .map(|l| {
            let v: Vec<u64> = l.split_whitespace().skip(1).take(8).filter_map(|x| x.parse().ok()).collect();
            let at = |i: usize| v.get(i).copied().unwrap_or(0);
            CpuTimes { total: v.iter().sum(), idle: at(3), iowait: at(4) }
        })
        .collect()
}

/// 解析 `/proc/meminfo`（字节）：总量、可用、缓存（含 buffers）、交换区总量、交换区空闲
pub fn parse_meminfo(text: &str) -> [u64; 5] {
    let kb = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
            .and_then(|r| r.split_whitespace().next()?.parse::<u64>().ok())
            .unwrap_or(0)
            .saturating_mul(1024)
    };
    [kb("MemTotal"), kb("MemAvailable"), kb("Buffers") + kb("Cached"), kb("SwapTotal"), kb("SwapFree")]
}

/// 虚拟设备与光驱等不计
fn virtual_disk(name: &str) -> bool {
    ["loop", "ram", "zram", "sr", "fd", "nbd"].iter().any(|p| name.starts_with(p))
}

/// 解析 `/proc/diskstats`：只保留整盘（去掉 sda1、nvme0n1p1 这类分区与虚拟设备），且开机以来有过读写的
pub fn parse_diskstats(text: &str) -> Vec<DiskTimes> {
    let all: Vec<DiskTimes> = text
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let n = |i: usize| f.get(i).and_then(|x| x.parse::<u64>().ok());
            Some(DiskTimes { name: (*f.get(2)?).to_owned(), read_sectors: n(5)?, write_sectors: n(9)?, io_ms: n(12)? })
        })
        .filter(|d| !virtual_disk(&d.name))
        .collect();
    let names: Vec<&str> = all.iter().map(|d| d.name.as_str()).collect();
    let partition = |name: &str| {
        names.iter().any(|p| {
            *p != name
                && name.strip_prefix(p).is_some_and(|rest| {
                    let digits = rest.strip_prefix('p').unwrap_or(rest);
                    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
                })
        })
    };
    all.iter()
        .filter(|d| !partition(&d.name) && d.read_sectors + d.write_sectors > 0)
        .cloned()
        .collect()
}

/// 解析 `/proc/self/stat`：(utime + stime, 线程数)。进程名可能含空格和括号，从最后一个 `)` 之后数字段
pub fn parse_self_stat(text: &str) -> Option<(u64, u64)> {
    let rest = &text[text.rfind(')')? + 1..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    let n = |i: usize| f.get(i).and_then(|x| x.parse::<u64>().ok());
    Some((n(11)? + n(12)?, n(17)?))
}

/// `/proc/self/status` 里的 `VmRSS`（字节）
pub fn parse_rss(text: &str) -> Option<u64> {
    text.lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))
        .and_then(|r| r.split_whitespace().next()?.parse::<u64>().ok())
        .map(|kb| kb.saturating_mul(1024))
}

fn read(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn snapshot() -> Option<Snapshot> {
    let cpus = parse_stat(&read("/proc/stat")?);
    if cpus.is_empty() {
        return None;
    }
    let disks = read("/proc/diskstats").map(|t| parse_diskstats(&t)).unwrap_or_default();
    let proc_ticks = read("/proc/self/stat").and_then(|t| parse_self_stat(&t)).map_or(0, |x| x.0);
    Some(Snapshot { at: Instant::now(), cpus, disks, proc_ticks })
}

fn pct(part: u64, whole: u64) -> f64 {
    if whole == 0 { 0.0 } else { (part as f64 * 1000.0 / whole as f64).round() / 10.0 }
}

/// 两次采样之差：CPU 使用率（汇总与每核）、各磁盘吞吐与繁忙度（按吞吐降序）、本进程 CPU
/// （`cpu` 按单核计、多线程可超过 100%，同 top；`cpu_share` 占整机）
fn usage(a: &Snapshot, b: &Snapshot) -> (Value, Vec<Value>, Vec<Value>, Value) {
    let one = |x: &CpuTimes, y: &CpuTimes| {
        let total = y.total.saturating_sub(x.total);
        let idle = y.idle.saturating_sub(x.idle);
        let iowait = y.iowait.saturating_sub(x.iowait);
        json!({ "usage": pct(total.saturating_sub(idle + iowait), total), "iowait": pct(iowait, total) })
    };
    let pairs: Vec<Value> = a.cpus.iter().zip(&b.cpus).map(|(x, y)| one(x, y)).collect();
    let (overall, cores) = match pairs.split_first() {
        Some((o, rest)) => (o.clone(), rest.to_vec()),
        None => (Value::Null, Vec::new()),
    };
    let secs = b.at.duration_since(a.at).as_secs_f64().max(0.001);
    let ms = secs * 1000.0;
    let mut disks: Vec<(u64, Value)> = b
        .disks
        .iter()
        .filter_map(|d| {
            let p = a.disks.iter().find(|x| x.name == d.name)?;
            let rate = |now: u64, then: u64| (now.saturating_sub(then) as f64 * 512.0 / secs).round() as u64;
            let (r, w) = (rate(d.read_sectors, p.read_sectors), rate(d.write_sectors, p.write_sectors));
            Some((r + w, json!({
                "name": d.name,
                "read_bps": r,
                "write_bps": w,
                "util": ((d.io_ms.saturating_sub(p.io_ms) as f64 / ms * 1000.0).round() / 10.0).min(100.0),
            })))
        })
        .collect();
    // 吞吐大的在前（同为 0 时保持 /proc 顺序）
    disks.sort_by_key(|d| std::cmp::Reverse(d.0));
    let (Some(x), Some(y)) = (a.cpus.first(), b.cpus.first()) else {
        return (overall, cores, disks.into_iter().map(|d| d.1).collect(), Value::Null);
    };
    let total = y.total.saturating_sub(x.total);
    let ticks = b.proc_ticks.saturating_sub(a.proc_ticks);
    let ncores = (b.cpus.len().saturating_sub(1)).max(1) as u64;
    let process = json!({ "cpu": pct(ticks * ncores, total), "cpu_share": pct(ticks, total) });
    (overall, cores, disks.into_iter().map(|d| d.1).collect(), process)
}

static LAST: Mutex<Option<Snapshot>> = Mutex::new(None);
static CACHE: Mutex<Option<(Instant, Value)>> = Mutex::new(None);

async fn system() -> Value {
    if let Ok(c) = CACHE.lock()
        && let Some((at, v)) = c.as_ref()
        && at.elapsed() < Duration::from_secs(2)
    {
        return v.clone();
    }
    let prev = LAST.lock().ok().and_then(|l| l.clone()).filter(|p| {
        let age = p.at.elapsed();
        age >= Duration::from_secs(1) && age <= Duration::from_secs(60)
    });
    let base = match prev {
        Some(p) => Some(p),
        None => {
            let s = snapshot();
            if s.is_some() {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            s
        }
    };
    let (Some(base), Some(now)) = (base, snapshot()) else {
        return Value::Null;
    };
    let (cpu, cores, disks, mut process) = usage(&base, &now);
    if let Some(p) = process.as_object_mut() {
        let rss = read("/proc/self/status").and_then(|t| parse_rss(&t));
        let threads = read("/proc/self/stat").and_then(|t| parse_self_stat(&t)).map(|x| x.1);
        p.insert("rss".into(), json!(rss));
        p.insert("threads".into(), json!(threads));
    }
    let [total, available, cached, swap_total, swap_free] = read("/proc/meminfo").map(|t| parse_meminfo(&t)).unwrap_or_default();
    let load: Vec<f64> = read("/proc/loadavg")
        .map(|t| t.split_whitespace().take(3).filter_map(|x| x.parse().ok()).collect())
        .unwrap_or_default();
    let uptime = read("/proc/uptime")
        .and_then(|t| t.split_whitespace().next()?.parse::<f64>().ok())
        .map(|s| s as u64);
    let v = json!({
        "cpu": cpu,
        "cores": cores,
        "memory": { "total": total, "used": total.saturating_sub(available), "cached": cached,
                    "swap_total": swap_total, "swap_used": swap_total.saturating_sub(swap_free) },
        "disks": disks,
        "process": process,
        "load": load,
        "uptime": uptime,
    });
    if let Ok(mut l) = LAST.lock() {
        *l = Some(now);
    }
    if let Ok(mut c) = CACHE.lock() {
        *c = Some((Instant::now(), v.clone()));
    }
    v
}

// ---------------------------------------------------------------- 路由

async fn status(ctx: RequestCtx) -> PResult<axum::response::Response> {
    let uid = i64::try_from(ctx.require_user()?).map_err(|_| PluginError::bad_request("bad user id"))?;
    let s = Settings::from_json(&ctx.config.get_settings().await);
    let mut c = ctx.sql.conn().await?;
    let class_id: Option<i64> = sqlx::query_scalar("SELECT class_id::int8 FROM identity.v_user_public WHERE id = $1")
        .bind(uid)
        .fetch_optional(&mut *c)
        .await?
        .flatten();
    let roles: Vec<String> = if s.classes.is_empty() && s.roles.is_empty() {
        Vec::new()
    } else {
        ctx.call_json("ctx.rbac", "roles_of", json!({ "user": uid }))
            .await
            .ok()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default()
    };
    if !visible(&s, class_id, &roles) {
        return Ok(Json(json!({ "visible": false })).into_response());
    }
    let u = sqlx::query(
        "SELECT count(*) AS users,
                count(*) FILTER (WHERE created_at >= date_trunc('day', now())) AS today
           FROM identity.v_user_public WHERE status <> 'deleted'",
    )
    .fetch_one(&mut *c)
    .await?;
    let p = sqlx::query(
        "SELECT count(DISTINCT user_id) AS active, count(*) FILTER (WHERE is_seeder) AS seeders,
                count(*) FILTER (WHERE NOT is_seeder) AS leechers
           FROM accounting.v_swarm_peers",
    )
    .fetch_one(&mut *c)
    .await?;
    drop(c);
    let limit = if s.user_limit > 0 {
        s.user_limit
    } else {
        // 注册策略未安装或调用失败时视为不限
        ctx.call_json("ctx.registration", "status", json!({}))
            .await
            .ok()
            .and_then(|v| v["max_users"].as_i64())
            .unwrap_or(0)
    };
    let system = if s.show_system { system().await } else { Value::Null };
    Ok(Json(json!({
        "visible": true,
        "users": u.get::<i64, _>("users"),
        "user_limit": (limit > 0).then_some(limit),
        "new_today": u.get::<i64, _>("today"),
        "active_users": p.get::<i64, _>("active"),
        "seeders": p.get::<i64, _>("seeders"),
        "leechers": p.get::<i64, _>("leechers"),
        "system": system,
    }))
    .into_response())
}

struct Routes;

impl HttpRoutes for Routes {
    fn router(&self) -> Router {
        Router::new().route("/v1/status", get(status))
    }
}

pub fn plugin() -> NativePlugin {
    NativePlugin { manifest: MANIFEST, http: Some(Arc::new(Routes)), ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles(r: &[&str]) -> Vec<String> {
        r.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn visibility() {
        let open = Settings::default();
        assert!(visible(&open, Some(1), &roles(&["user"])));
        let s = Settings { classes: vec![3, 4], roles: roles(&["uploader"]), ..Settings::default() };
        // 等级满足
        assert!(visible(&s, Some(3), &roles(&["user"])));
        // 角色满足
        assert!(visible(&s, Some(1), &roles(&["user", "uploader"])));
        // 都不满足
        assert!(!visible(&s, Some(1), &roles(&["user"])));
        assert!(!visible(&s, None, &[]));
        // 管理员始终可见
        assert!(visible(&s, Some(1), &roles(&["admin"])));
    }

    #[test]
    fn settings_parse() {
        let s = Settings::from_json(&json!({ "user_limit": 5000, "visible_classes": [2, "x", 5], "visible_roles": ["moderator"], "show_system": false }));
        assert_eq!(s, Settings { user_limit: 5000, classes: vec![2, 5], roles: roles(&["moderator"]), show_system: false });
        assert_eq!(Settings::from_json(&Value::Null), Settings::default());
    }

    #[test]
    fn proc_parsing() {
        let stat = "cpu  100 0 50 800 50 0 0 0 0 0\ncpu0 50 0 25 400 25 0 0 0 0 0\ncpu1 50 0 25 400 25 0 0 0 0 0\nintr 1 2 3\n";
        let c = parse_stat(stat);
        assert_eq!(c.len(), 3);
        assert_eq!(c[0], CpuTimes { total: 1000, idle: 800, iowait: 50 });
        let mem = "MemTotal:       1000 kB\nMemFree:  100 kB\nMemAvailable:    400 kB\nBuffers: 10 kB\nCached: 90 kB\nSwapTotal: 0 kB\nSwapFree: 0 kB\n";
        assert_eq!(parse_meminfo(mem), [1_024_000, 409_600, 102_400, 0, 0]);
        let ds = "   7       0 loop0 10 0 80 0 0 0 0 0 0 5 0\n 259 0 nvme0n1 100 0 2000 10 50 0 1000 5 0 300 15\n 259 1 nvme0n1p1 90 0 1900 10 50 0 1000 5 0 290 15\n   8 0 sda 1 0 8 0 0 0 0 0 0 1 0\n   8 1 sda1 1 0 8 0 0 0 0 0 0 1 0\n   8 16 sdb 0 0 0 0 0 0 0 0 0 0 0\n";
        let d = parse_diskstats(ds);
        assert_eq!(d.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["nvme0n1", "sda"]);
        assert_eq!((d[0].read_sectors, d[0].write_sectors, d[0].io_ms), (2000, 1000, 300));
        let st = "4242 (pt server) S 1 4242 4242 0 -1 4194560 100 0 0 0 150 50 0 0 20 0 12 0 1000 123456 789 18446744073709551615";
        assert_eq!(parse_self_stat(st), Some((200, 12)));
        assert_eq!(parse_rss("Name:\tpt-server\nVmRSS:\t  2048 kB\n"), Some(2_097_152));
    }

    #[test]
    fn deltas() {
        let t0 = Instant::now();
        let a = Snapshot {
            at: t0,
            cpus: vec![CpuTimes { total: 1000, idle: 800, iowait: 0 }, CpuTimes { total: 500, idle: 400, iowait: 0 }],
            disks: vec![DiskTimes { name: "sda".into(), read_sectors: 0, write_sectors: 0, io_ms: 0 }],
            proc_ticks: 10,
        };
        let b = Snapshot {
            at: t0 + Duration::from_secs(2),
            cpus: vec![CpuTimes { total: 1200, idle: 900, iowait: 20 }, CpuTimes { total: 600, idle: 410, iowait: 0 }],
            disks: vec![DiskTimes { name: "sda".into(), read_sectors: 4096, write_sectors: 2048, io_ms: 500 }],
            proc_ticks: 30,
        };
        let (cpu, cores, disks, process) = usage(&a, &b);
        // 1 个核心：20 / 200 = 10%
        assert_eq!(process, json!({ "cpu": 10.0, "cpu_share": 10.0 }));
        assert_eq!(cpu, json!({ "usage": 40.0, "iowait": 10.0 }));
        assert_eq!(cores, vec![json!({ "usage": 90.0, "iowait": 0.0 })]);
        assert_eq!(disks, vec![json!({ "name": "sda", "read_bps": 1_048_576, "write_bps": 524_288, "util": 25.0 })]);
    }

    #[test]
    fn live_proc() {
        // 在 Linux 上能读到至少一个核心
        if std::path::Path::new("/proc/stat").exists() {
            assert!(snapshot().is_some_and(|s| s.cpus.len() >= 2));
        }
    }
}
