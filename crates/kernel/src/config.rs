//! `site.toml`（kernel/03 §11）。

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value as Json;

#[derive(Debug, Clone, Deserialize)]
pub struct SiteConfig {
    pub site: SiteSection,
    #[serde(default)]
    pub kernel: KernelSection,
    #[serde(default)]
    pub http: HttpSection,
    pub database: DatabaseSection,
    #[serde(default)]
    pub redis: BTreeMap<String, RedisInstance>,
    #[serde(default)]
    pub plugins: PluginsSection,
    #[serde(default)]
    pub blob: BlobSection,
    #[serde(default)]
    pub services: BTreeMap<String, String>,
    #[serde(default)]
    pub grants: BTreeMap<String, Grant>,
    #[serde(default)]
    pub hooks: BTreeMap<String, HookOverride>,
    #[serde(default)]
    pub events: EventsSection,
    /// 插件静态配置 `[plugin.<id>]`
    #[serde(default)]
    pub plugin: BTreeMap<String, Json>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SiteSection {
    pub name: String,
    pub base_url: String,
    #[serde(default = "default_tz")]
    pub timezone: String,
    #[serde(default = "default_locale")]
    pub default_locale: String,
    #[serde(default)]
    pub locales: Vec<String>,
}

fn default_tz() -> String {
    "UTC".into()
}
fn default_locale() -> String {
    "zh-CN".into()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Prod,
    Dev,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KernelSection {
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default = "yes")]
    pub strict_catalog: bool,
    /// 服务调用的默认超时（kernel/03 §3.3：被调方法默认 1 s，且不超过调用方剩余时间）
    #[serde(default = "default_service_timeout")]
    pub service_timeout_ms: i64,
    /// 钩子超时倍数（只在 dev 模式生效，用于调试构建与并行测试；生产保持目录给出的预算）
    #[serde(default = "one")]
    pub hook_timeout_factor: u32,
}

fn one() -> u32 {
    1
}

impl KernelSection {
    /// 生效的钩子超时倍数
    pub fn hook_factor(&self) -> i64 {
        if self.mode == Mode::Dev {
            i64::from(self.hook_timeout_factor.clamp(1, 100))
        } else {
            1
        }
    }
}

fn default_service_timeout() -> i64 {
    1000
}

impl Default for KernelSection {
    fn default() -> Self {
        Self {
            mode: Mode::Prod,
            node_id: None,
            strict_catalog: true,
            service_timeout_ms: default_service_timeout(),
            hook_timeout_factor: 1,
        }
    }
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct HttpSection {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_body_kb")]
    pub max_body_kb: u32,
    #[serde(default = "default_timeout")]
    pub request_timeout_ms: u32,
    #[serde(default)]
    pub trusted_proxies: Vec<String>,
    #[serde(default)]
    pub rate_limit: RateLimitSection,
    /// 前端构建目录（`web/dist`）：SPA、`static/`、`plugins/<id>/`；不设置时不提供前端
    #[serde(default)]
    pub web_dir: Option<String>,
}

/// `[http.rate_limit]`：全站默认规则，与路由规则叠加
#[derive(Debug, Clone, Deserialize)]
pub struct RateLimitSection {
    #[serde(default = "default_rate")]
    pub default: String,
    /// 按用户等级放宽：`{ "30" = 2, "50" = 5, "100" = 0 }` 表示等级 ≥ 30 的额度 ×2、≥ 50 ×5、≥ 100 不限流。
    /// 取不超过用户等级的最大门槛；未登录或没有匹配项时为 ×1。只作用于站点默认规则与路由规则中的 ip / user / token 规则
    #[serde(default)]
    pub class_multiplier: std::collections::BTreeMap<String, f64>,
}

impl Default for RateLimitSection {
    fn default() -> Self {
        Self {
            default: default_rate(),
            class_multiplier: std::collections::BTreeMap::new(),
        }
    }
}

impl RateLimitSection {
    /// 用户等级对应的倍率：`None` 表示不限流
    pub fn multiplier(&self, class_id: Option<i32>) -> Option<f64> {
        let Some(c) = class_id else { return Some(1.0) };
        let best = self
            .class_multiplier
            .iter()
            .filter_map(|(k, v)| k.trim().parse::<i32>().ok().map(|t| (t, *v)))
            .filter(|(t, _)| *t <= c)
            .max_by_key(|(t, _)| *t);
        match best {
            Some((_, m)) if m <= 0.0 => None,
            Some((_, m)) => Some(m.max(1.0)),
            None => Some(1.0),
        }
    }
}

fn default_rate() -> String {
    "ip:600/m".into()
}

impl Default for HttpSection {
    fn default() -> Self {
        Self {
            rate_limit: RateLimitSection::default(),
            bind: default_bind(),
            max_body_kb: default_body_kb(),
            request_timeout_ms: default_timeout(),
            trusted_proxies: vec![],
            web_dir: None,
        }
    }
}

fn default_bind() -> String {
    "0.0.0.0:31251".into()
}
fn default_body_kb() -> u32 {
    1024
}
fn default_timeout() -> u32 {
    15000
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseSection {
    pub url: String,
    pub admin_url: String,
    #[serde(default = "default_prefix")]
    pub role_prefix: String,
    #[serde(default = "default_pool")]
    pub pool_size: u32,
    #[serde(default = "default_acquire")]
    pub acquire_timeout_ms: u64,
    #[serde(default = "default_stmt_timeout")]
    pub statement_timeout_ms: u64,
    #[serde(default = "default_max_rows")]
    pub max_rows: u32,
    #[serde(default = "yes")]
    pub migrate_on_start: bool,
    #[serde(default)]
    pub extensions: Vec<String>,
}

fn default_prefix() -> String {
    "pt_".into()
}
fn default_pool() -> u32 {
    32
}
fn default_acquire() -> u64 {
    2000
}
fn default_stmt_timeout() -> u64 {
    5000
}
fn default_max_rows() -> u32 {
    10_000
}

#[derive(Debug, Clone, Deserialize)]
pub struct RedisInstance {
    pub url: String,
}

/// `[blob]`：本地文件系统对象存储（开发 / 单机）
#[derive(Debug, Clone, Deserialize, Default)]
pub struct BlobSection {
    #[serde(default)]
    pub dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct PluginsSection {
    #[serde(default)]
    pub dir: Option<String>,
    #[serde(default)]
    pub disabled: Vec<String>,
    #[serde(default)]
    pub allow_unsigned: bool,
    /// 受信任的插件签名公钥（minisign：`RW…` 一行 base64，或 .pub 文件路径）
    #[serde(default)]
    pub trusted_keys: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Grant {
    #[serde(default)]
    pub sensitive: Vec<String>,
    #[serde(default)]
    pub scopes: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct HookOverride {
    #[serde(default)]
    pub on_error: Option<String>,
    #[serde(default)]
    pub priority: BTreeMap<String, i32>,
    #[serde(default)]
    pub disabled_handlers: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventsSection {
    #[serde(default = "default_retention")]
    pub log_retention_days: u32,
    #[serde(default = "default_inbox")]
    pub inbox_retention_days: u32,
    #[serde(default = "default_maxlen")]
    pub stream_maxlen: u64,
    #[serde(default = "default_partitions")]
    pub default_partitions: u32,
    #[serde(default)]
    pub partitions: BTreeMap<String, u32>,
    #[serde(default)]
    pub retention: BTreeMap<String, u32>,
    #[serde(default)]
    pub stream_maxlen_by_type: BTreeMap<String, u64>,
}

impl Default for EventsSection {
    fn default() -> Self {
        Self {
            log_retention_days: default_retention(),
            inbox_retention_days: default_inbox(),
            stream_maxlen: default_maxlen(),
            default_partitions: default_partitions(),
            partitions: BTreeMap::new(),
            retention: BTreeMap::new(),
            stream_maxlen_by_type: BTreeMap::new(),
        }
    }
}

fn default_retention() -> u32 {
    730
}
fn default_inbox() -> u32 {
    14
}
fn default_maxlen() -> u64 {
    1_000_000
}
fn default_partitions() -> u32 {
    4
}

impl EventsSection {
    pub fn partitions_for(&self, event_type: &str) -> u32 {
        self.partitions
            .get(event_type)
            .copied()
            .unwrap_or(self.default_partitions)
            .max(1)
    }
    pub fn maxlen_for(&self, event_type: &str) -> u64 {
        self.stream_maxlen_by_type
            .get(event_type)
            .copied()
            .unwrap_or(self.stream_maxlen)
    }
}

impl SiteConfig {
    /// 解析并展开 `${VAR}` / `${VAR:-默认}`。
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let expanded = expand_env(text, |k| std::env::var(k).ok())?;
        let cfg: SiteConfig = toml::from_str(&expanded)?;
        Ok(cfg)
    }

    pub fn node_id(&self) -> String {
        self.kernel
            .node_id
            .clone()
            .or_else(|| std::env::var("HOSTNAME").ok())
            .unwrap_or_else(|| format!("node-{}", std::process::id()))
    }

    pub fn redis_url(&self, role: &str) -> Option<&str> {
        self.redis
            .get(role)
            .or_else(|| self.redis.get("main"))
            .map(|r| r.url.as_str())
    }
}

pub fn expand_env(text: &str, get: impl Fn(&str) -> Option<String>) -> anyhow::Result<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after
            .find('}')
            .ok_or_else(|| anyhow::anyhow!("unterminated ${{ in site.toml"))?;
        let expr = &after[..end];
        let (name, default) = match expr.split_once(":-") {
            Some((n, d)) => (n, Some(d)),
            None => (expr, None),
        };
        match get(name).or_else(|| default.map(str::to_owned)) {
            Some(v) => out.push_str(&v),
            None => anyhow::bail!("environment variable `{name}` is not set and has no default"),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_expansion() -> anyhow::Result<()> {
        let get = |k: &str| (k == "A").then(|| "1".to_owned());
        assert_eq!(expand_env("x=${A} y=${B:-2}", get)?, "x=1 y=2");
        assert!(expand_env("${C}", get).is_err());
        Ok(())
    }
}

#[cfg(test)]
mod rate_tests {
    use super::RateLimitSection;

    #[test]
    fn class_multiplier() {
        let mut r = RateLimitSection::default();
        r.class_multiplier.insert("30".into(), 2.0);
        r.class_multiplier.insert("50".into(), 5.0);
        r.class_multiplier.insert("100".into(), 0.0);
        assert_eq!(r.multiplier(None), Some(1.0));
        assert_eq!(r.multiplier(Some(20)), Some(1.0));
        assert_eq!(r.multiplier(Some(30)), Some(2.0));
        assert_eq!(r.multiplier(Some(70)), Some(5.0));
        assert_eq!(r.multiplier(Some(100)), None);
    }
}
