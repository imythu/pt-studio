//! 插件清单 `plugin.toml` 的模型（plugin-api.md §4）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::{PResult, PluginError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    Wasm,
    Native,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub name_term: Option<String>,
    pub version: semver::Version,
    pub kind: PluginKind,
    pub api: String,
    #[serde(default)]
    pub kernel: Option<semver::VersionReq>,
    pub license: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub provides: Vec<String>,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default)]
    pub optional_requires: Vec<String>,
    #[serde(default)]
    pub publishes: Vec<String>,
    #[serde(default)]
    pub capabilities: Capabilities,
    #[serde(default)]
    pub subscriptions: Vec<Subscription>,
    #[serde(default)]
    pub hooks: Vec<HookHandler>,
    #[serde(default)]
    pub hook_points: Vec<HookPoint>,
    #[serde(default)]
    pub cron: Vec<CronDecl>,
    #[serde(default)]
    pub jobs: Vec<JobDecl>,
    #[serde(default)]
    pub http: Option<HttpDecl>,
    #[serde(default)]
    pub permissions: Vec<PermissionDef>,
    #[serde(default)]
    pub config: Option<ConfigDecl>,
    #[serde(default)]
    pub database: Option<DatabaseDecl>,
    #[serde(default)]
    pub redis: Option<RedisDecl>,
    #[serde(default)]
    pub wasm: Option<WasmDecl>,
    #[serde(default)]
    pub terms: BTreeMap<String, Json>,
    #[serde(default)]
    pub frontend: Option<FrontendDecl>,
    #[serde(default)]
    pub mcp: Option<McpDecl>,
    #[serde(default)]
    pub workers: Vec<WorkerDecl>,
    #[serde(default)]
    pub notify: Option<Json>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    #[serde(default = "yes")]
    pub kv: bool,
    #[serde(default)]
    pub sql: bool,
    #[serde(default)]
    pub redis: bool,
    #[serde(default)]
    pub redis_global: Vec<RedisGlobal>,
    #[serde(default)]
    pub blob: bool,
    #[serde(default)]
    pub http_outbound: Vec<Json>,
    #[serde(default)]
    pub views: Vec<String>,
    #[serde(default)]
    pub sensitive: Vec<String>,
    #[serde(default)]
    pub sensitive_optional: Vec<String>,
    #[serde(default)]
    pub search_contributor: bool,
    #[serde(default)]
    pub scopes: BTreeMap<String, Vec<String>>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedisGlobal {
    pub role: String,
    pub prefix: String,
    pub mode: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TxMode {
    #[default]
    Auto,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Subscription {
    pub name: String,
    pub event: String,
    #[serde(default = "v1")]
    pub versions: Vec<u32>,
    #[serde(default)]
    pub ordered: bool,
    #[serde(default = "ten")]
    pub max_attempts: u32,
    #[serde(default)]
    pub tx: TxMode,
    #[serde(default = "one")]
    pub batch: u32,
    /// "latest"（默认）| "earliest"
    #[serde(default)]
    pub start: Option<String>,
}

fn v1() -> Vec<u32> {
    vec![1]
}
fn ten() -> u32 {
    10
}
fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookHandler {
    pub hook: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub timeout_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HookPoint {
    pub name: String,
    #[serde(default)]
    pub input_schema: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CronDecl {
    pub name: String,
    pub schedule: String,
    #[serde(default = "three_hundred")]
    pub timeout_secs: u32,
    #[serde(default)]
    pub catch_up: Option<String>,
    #[serde(default = "manual")]
    pub tx: TxMode,
}

fn three_hundred() -> u32 {
    300
}
fn manual() -> TxMode {
    TxMode::Manual
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobDecl {
    pub name: String,
    #[serde(default = "three_hundred")]
    pub timeout_secs: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpDecl {
    #[serde(default)]
    pub openapi: Option<String>,
    #[serde(default)]
    pub routes: Vec<RouteDecl>,
    /// 只允许 mcp 插件（kernel/03 §8.7）
    #[serde(default)]
    pub root_routes: Vec<RouteDecl>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthLevel {
    Public,
    User,
    Admin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteDecl {
    pub id: String,
    pub method: String,
    pub path: String,
    pub auth: AuthLevel,
    #[serde(default)]
    pub permission: Option<String>,
    #[serde(default)]
    pub rate_limit: Option<String>,
    #[serde(default)]
    pub idempotent: bool,
    #[serde(default)]
    pub max_body_kb: Option<u32>,
    #[serde(default)]
    pub timeout_ms: Option<u32>,
    #[serde(default)]
    pub reauth_secs: Option<u32>,
    #[serde(default)]
    pub validate_request: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PermissionDef {
    pub name: String,
    #[serde(default)]
    pub description_term: Option<String>,
    #[serde(default)]
    pub default_roles: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigDecl {
    #[serde(default)]
    pub static_schema: Option<String>,
    #[serde(default)]
    pub settings_schema: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseDecl {
    #[serde(default)]
    pub migrations: Option<String>,
    #[serde(default)]
    pub views: Vec<String>,
    #[serde(default)]
    pub partitions: Vec<Json>,
    #[serde(default)]
    pub plpgsql: bool,
    #[serde(default)]
    pub max_rows: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedisDecl {
    #[serde(default)]
    pub quota_mb: Option<u32>,
    #[serde(default)]
    pub max_keys: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WasmDecl {
    #[serde(default)]
    pub memory_mb: Option<u32>,
    #[serde(default)]
    pub max_concurrency: Option<u32>,
    #[serde(default)]
    pub default_timeout_ms: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrontendDecl {
    #[serde(default)]
    pub entry: Option<String>,
    #[serde(default)]
    pub slots: Vec<String>,
    #[serde(default)]
    pub routes: Vec<String>,
    #[serde(default)]
    pub settings_ui: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpDecl {
    #[serde(default)]
    pub tools: Vec<Json>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerDecl {
    pub name: String,
    pub stream_prefix: String,
    pub partitions_from: String,
}

/// `ctx.bonus@0.1.0` / `ctx.ledger@^0.1` / `ctx.users`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRef {
    pub name: String,
    pub version: Option<String>,
}

impl ServiceRef {
    pub fn parse(s: &str) -> PResult<Self> {
        let (name, version) = match s.split_once('@') {
            Some((n, v)) => (n, Some(v.to_owned())),
            None => (s, None),
        };
        if !name.starts_with("ctx.") || name.len() < 5 {
            return Err(PluginError::validation(format!(
                "invalid service reference `{s}`"
            )));
        }
        Ok(Self {
            name: name.to_owned(),
            version,
        })
    }

    pub fn provided_version(&self) -> PResult<semver::Version> {
        let v = self.version.as_deref().unwrap_or("0.1.0");
        semver::Version::parse(v).map_err(|e| PluginError::validation(format!("{}: {e}", self.name)))
    }

    pub fn requirement(&self) -> PResult<semver::VersionReq> {
        match &self.version {
            None => Ok(semver::VersionReq::STAR),
            Some(v) => semver::VersionReq::parse(v)
                .map_err(|e| PluginError::validation(format!("{}: {e}", self.name))),
        }
    }
}

impl Manifest {
    pub fn parse(text: &str) -> PResult<Self> {
        let m: Manifest =
            toml::from_str(text).map_err(|e| PluginError::validation(format!("plugin.toml: {e}")))?;
        m.validate_basic()?;
        Ok(m)
    }

    /// 与 catalog 无关的格式检查。
    pub fn validate_basic(&self) -> PResult<()> {
        let id_ok = {
            let b = self.id.as_bytes();
            (2..=32).contains(&b.len())
                && b[0].is_ascii_lowercase()
                && b.iter()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
        };
        if !id_ok {
            return Err(PluginError::validation(format!(
                "invalid plugin id `{}`",
                self.id
            )));
        }
        for s in self
            .provides
            .iter()
            .chain(&self.requires)
            .chain(&self.optional_requires)
        {
            ServiceRef::parse(s)?;
        }
        for p in &self.permissions {
            if !p.name.starts_with(&format!("{}.", self.id)) {
                return Err(PluginError::validation(format!(
                    "permission `{}` must start with `{}.`",
                    p.name, self.id
                )));
            }
        }
        for k in self.terms.keys() {
            if !k.starts_with(&format!("{}.", self.id)) {
                return Err(PluginError::validation(format!(
                    "term `{k}` must start with `{}.`",
                    self.id
                )));
            }
        }
        if let Some(h) = &self.http {
            for r in &h.routes {
                let ok = r.path.starts_with("/v")
                    && r.path[2..]
                        .split('/')
                        .next()
                        .is_some_and(|v| !v.is_empty() && v.bytes().all(|c| c.is_ascii_digit()));
                if !ok {
                    return Err(PluginError::validation(format!(
                        "route `{}` path must start with /v<N>/",
                        r.id
                    )));
                }
                if r.auth == AuthLevel::Admin && r.permission.is_none() {
                    return Err(PluginError::validation(format!(
                        "admin route `{}` requires permission",
                        r.id
                    )));
                }
            }
            if !h.root_routes.is_empty() && self.id != "mcp" {
                return Err(PluginError::validation(
                    "only the mcp plugin may declare root routes",
                ));
            }
        }
        if let Some(f) = &self.frontend {
            for r in &f.routes {
                if !(r.starts_with(&format!("/p/{}/", self.id))
                    || r.starts_with(&format!("/admin/p/{}/", self.id)))
                {
                    return Err(PluginError::validation(format!(
                        "frontend route `{r}` has wrong prefix"
                    )));
                }
            }
        }
        Ok(())
    }

    /// 本插件的 PG schema 名（连字符改为下划线）。
    pub fn schema(&self) -> String {
        self.id.replace('-', "_")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal() -> PResult<()> {
        let m = Manifest::parse(
            r#"
id = "bonus"
name = "魔力值"
version = "1.2.0"
kind = "wasm"
api = "pt:plugin@0.1"
license = "MIT OR Apache-2.0"
provides = ["ctx.bonus@0.1.0"]
requires = ["ctx.ledger@^0.1"]
publishes = ["bonus.hourly_settled"]

[capabilities]
sql = true

[[http.routes]]
id = "get_summary"
method = "GET"
path = "/v1/summary"
auth = "user"
"#,
        )?;
        assert_eq!(m.schema(), "bonus");
        assert!(m.capabilities.kv);
        assert_eq!(
            ServiceRef::parse(&m.requires[0])?.requirement()?.to_string(),
            "^0.1"
        );
        Ok(())
    }

    #[test]
    fn rejects_bad_route_and_unknown_field() {
        let base = "id = \"xx\"\nname=\"x\"\nversion=\"0.1.0\"\nkind=\"native\"\napi=\"pt:plugin@0.1\"\nlicense=\"MIT\"\n";
        assert!(Manifest::parse(&format!("{base}bogus = 1\n")).is_err());
        assert!(
            Manifest::parse(&format!(
                "{base}[[http.routes]]\nid=\"a\"\nmethod=\"GET\"\npath=\"/x\"\nauth=\"user\"\n"
            ))
            .is_err()
        );
    }
}
