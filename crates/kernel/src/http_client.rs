//! 插件出站 HTTP（plugin-api.md §4.3 `http_outbound`、security.md SSRF 防护，评审 R1 B-04）。
//!
//! - 只允许清单 `capabilities.http_outbound` 列出的主机：`"api.example.com"`、`"*.example.com"`、`"config:<静态配置键>"`
//!   （值为 URL 或主机名），或对象 `{ host | config, allow_private = true }`；对象 `{ setting = "<设置键>" }` 在请求时按插件
//!   运行时设置中该键的 URL 放行其主机（管理员在后台填写；只允许公网地址）；
//! - 解析后检查地址：默认禁止私有 / 回环 / 链路本地 / 组播等地址（`allow_private` 的条目除外），并把连接固定到已检查的地址
//!   （防 DNS rebinding）；
//! - 只允许 https（`allow_private` 条目与 dev 模式可用 http）；不跟随重定向（由插件自行处理）；
//! - 钩子调用链中禁止出站；超时 ≤ 30 s；响应体 ≤ 8 MiB。

use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use async_trait::async_trait;
use pt_plugin_api::{InvocationKind, Json, PResult, PluginError, host::HttpClient};

const MAX_BODY: usize = 8 << 20;
const MAX_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
enum HostPat {
    Exact(String),
    /// `*.example.com`：只匹配子域
    Suffix(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Rule {
    host: HostPat,
    allow_private: bool,
}

/// 某个插件的出站策略（启动时由清单与静态配置解析）
#[derive(Debug, Clone, Default)]
pub struct OutboundPolicy {
    rules: Vec<Rule>,
    /// `{ setting = "<key>" }`：主机取自运行时设置（请求时读取）
    setting_keys: Vec<String>,
    dev: bool,
}

fn host_of(v: &str) -> Option<String> {
    let rest = v.split_once("://").map_or(v, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = if host.starts_with('[') {
        host.split(']').next().map(|h| h.trim_start_matches('['))?
    } else {
        host.split(':').next()?
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

impl OutboundPolicy {
    /// `entries` 为清单中的 `http_outbound`；`static_config` 用于解析 `config:<key>`
    pub fn parse(entries: &[Json], static_config: &Json, dev: bool) -> Result<Self, String> {
        let mut rules = Vec::new();
        let mut setting_keys = Vec::new();
        for e in entries {
            let (spec, allow_private) = match e {
                Json::String(s) => (s.clone(), false),
                Json::Object(o) if o.contains_key("setting") => {
                    let k = o.get("setting").and_then(Json::as_str).ok_or("`setting` must be a string")?;
                    setting_keys.push(k.to_owned());
                    continue;
                }
                Json::Object(o) => {
                    let spec = o
                        .get("host")
                        .and_then(Json::as_str)
                        .map(str::to_owned)
                        .or_else(|| {
                            o.get("config")
                                .and_then(Json::as_str)
                                .map(|k| format!("config:{k}"))
                        })
                        .ok_or("http_outbound object needs `host` or `config`")?;
                    (
                        spec,
                        o.get("allow_private").and_then(Json::as_bool).unwrap_or(false),
                    )
                }
                _ => return Err("http_outbound entries must be strings or tables".into()),
            };
            let host = match spec.strip_prefix("config:") {
                Some(key) => match static_config.get(key).and_then(Json::as_str) {
                    Some(v) => host_of(v).ok_or_else(|| format!("config `{key}` has no host"))?,
                    // 未配置：该条目不生效（插件以“不可用”状态运行）
                    None => continue,
                },
                None => spec.to_ascii_lowercase(),
            };
            let pat = match host.strip_prefix("*.") {
                Some(s) if !s.is_empty() => HostPat::Suffix(s.to_owned()),
                _ if host.contains('*') => return Err(format!("invalid host pattern `{host}`")),
                _ => HostPat::Exact(host),
            };
            rules.push(Rule {
                host: pat,
                allow_private,
            });
        }
        Ok(Self { rules, setting_keys, dev })
    }

    fn rule_for(&self, host: &str) -> Option<&Rule> {
        self.rules.iter().find(|r| match &r.host {
            HostPat::Exact(h) => h == host,
            HostPat::Suffix(s) => {
                host.len() > s.len() + 1
                    && host.ends_with(s.as_str())
                    && host.as_bytes()[host.len() - s.len() - 1] == b'.'
            }
        })
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty() && self.setting_keys.is_empty()
    }
}

/// 公网地址判定（私有、回环、链路本地、CGNAT、文档、基准测试、组播、保留段均视为非公网）
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_broadcast()
                || v.is_unspecified()
                || v.is_documentation()
                || v.is_multicast()
                || o[0] == 0
                || (o[0] == 100 && (o[1] & 0xc0) == 64)
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                || (o[0] == 198 && (o[1] & 0xfe) == 18)
                || o[0] >= 240)
        }
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let s = v.segments();
            !(v.is_loopback()
                || v.is_unspecified()
                || v.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00
                || (s[0] & 0xffc0) == 0xfe80
                || s[0] == 0x2001 && s[1] == 0x0db8)
        }
    }
}

pub struct KernelHttp {
    pub plugin: String,
    pub policy: Arc<OutboundPolicy>,
    pub kind: InvocationKind,
    /// 读取 `{ setting = … }` 条目对应的运行时设置
    pub pool: Option<sqlx::PgPool>,
}

impl KernelHttp {
    /// 运行时设置中声明为出站地址的键：其 URL 的主机与请求主机一致时放行（公网地址、https）
    async fn setting_rule(&self, host: &str) -> Option<Rule> {
        let pool = self.pool.as_ref().filter(|_| !self.policy.setting_keys.is_empty())?;
        let settings: Json = sqlx::query_scalar("SELECT settings FROM kernel.plugin_settings WHERE plugin_id = $1")
            .bind(&self.plugin)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten()?;
        self.policy
            .setting_keys
            .iter()
            .filter_map(|k| settings.get(k).and_then(Json::as_str).and_then(host_of))
            .any(|h| h == host)
            .then(|| Rule { host: HostPat::Exact(host.to_owned()), allow_private: false })
    }
}

fn denied(msg: impl Into<String>) -> PluginError {
    PluginError::capability_denied(msg)
}

#[async_trait]
impl HttpClient for KernelHttp {
    async fn send(
        &self,
        req: http::Request<bytes::Bytes>,
        timeout: Duration,
    ) -> PResult<http::Response<bytes::Bytes>> {
        if self.kind == InvocationKind::Hook {
            return Err(denied("outbound HTTP is not allowed inside hooks"));
        }
        let uri = req.uri().clone();
        let scheme = uri.scheme_str().unwrap_or("");
        let host = uri
            .host()
            .map(|h| h.trim_matches(['[', ']']).to_ascii_lowercase())
            .ok_or_else(|| PluginError::bad_request("URL has no host"))?;
        let rule = match self.policy.rule_for(&host) {
            Some(r) => r.clone(),
            None => self.setting_rule(&host).await.ok_or_else(|| {
                denied(format!(
                    "plugin `{}` may not call host `{host}` (http_outbound)",
                    self.plugin
                ))
            })?,
        };
        match scheme {
            "https" => {}
            "http" if rule.allow_private || self.policy.dev => {}
            _ => return Err(denied("only https URLs are allowed")),
        }
        let port = uri.port_u16().unwrap_or(if scheme == "https" { 443 } else { 80 });
        let addrs: Vec<SocketAddr> = match host.parse::<IpAddr>() {
            Ok(ip) => vec![SocketAddr::new(ip, port)],
            Err(_) => tokio::net::lookup_host((host.as_str(), port))
                .await
                .map_err(|e| {
                    PluginError::unavailable(format!("DNS lookup for {host} failed: {e}")).retryable()
                })?
                .collect(),
        };
        if addrs.is_empty() {
            return Err(PluginError::unavailable(format!("{host} has no addresses")));
        }
        if !rule.allow_private && addrs.iter().any(|a| !is_public(a.ip())) {
            return Err(denied(format!("{host} resolves to a non-public address")));
        }
        let addr = addrs[0];
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout.min(MAX_TIMEOUT))
            .no_proxy();
        if host.parse::<IpAddr>().is_err() {
            builder = builder.resolve(&host, addr);
        }
        let client = builder
            .build()
            .map_err(|e| PluginError::internal(format!("http client: {e}")))?;
        let (parts, body) = req.into_parts();
        let method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes())
            .map_err(|_| PluginError::bad_request("bad method"))?;
        let mut rb = client.request(method, uri.to_string()).body(body.to_vec());
        for (k, v) in &parts.headers {
            // 禁止插件伪造逐跳 / 主机头
            if matches!(
                k.as_str(),
                "host" | "connection" | "transfer-encoding" | "proxy-authorization"
            ) {
                continue;
            }
            rb = rb.header(k.as_str(), v.as_bytes());
        }
        let mut resp = rb
            .send()
            .await
            .map_err(|e| PluginError::unavailable(format!("request to {host} failed: {e}")).retryable())?;
        let status = resp.status().as_u16();
        let mut out = http::Response::builder().status(status);
        for (k, v) in resp.headers() {
            out = out.header(k.as_str(), v.as_bytes());
        }
        let mut buf = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| {
            PluginError::unavailable(format!("reading response from {host} failed: {e}")).retryable()
        })? {
            if buf.len() + chunk.len() > MAX_BODY {
                return Err(PluginError::new(
                    "kernel.resource_exhausted",
                    "response body is larger than 8 MiB",
                ));
            }
            buf.extend_from_slice(&chunk);
        }
        out.body(bytes::Bytes::from(buf))
            .map_err(|e| PluginError::internal(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn policy() {
        let p = OutboundPolicy::parse(
            &[
                json!("api.themoviedb.org"),
                json!("*.tmdb.org"),
                json!("config:base"),
                json!({ "config": "local", "allow_private": true }),
                json!("config:missing"),
            ],
            &json!({ "base": "https://proxy.example.com:8443/3", "local": "http://127.0.0.1:9000" }),
            false,
        )
        .unwrap_or_default();
        assert!(p.rule_for("api.themoviedb.org").is_some());
        assert!(p.rule_for("image.tmdb.org").is_some());
        assert!(p.rule_for("tmdb.org").is_none());
        assert!(p.rule_for("eviltmdb.org").is_none());
        assert!(p.rule_for("proxy.example.com").is_some_and(|r| !r.allow_private));
        assert!(p.rule_for("127.0.0.1").is_some_and(|r| r.allow_private));
        assert!(OutboundPolicy::parse(&[json!("a.*.com")], &json!({}), false).is_err());
    }

    #[test]
    fn public_addresses() {
        for ip in [
            "10.1.2.3",
            "127.0.0.1",
            "169.254.169.254",
            "192.168.1.1",
            "172.16.0.1",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:10.0.0.1",
        ] {
            assert!(
                !is_public(ip.parse().unwrap_or(IpAddr::from([0, 0, 0, 0]))),
                "{ip}"
            );
        }
        for ip in ["1.1.1.1", "93.184.216.34", "2606:4700::1111"] {
            assert!(
                is_public(ip.parse().unwrap_or(IpAddr::from([0, 0, 0, 0]))),
                "{ip}"
            );
        }
    }

    #[tokio::test]
    async fn blocks_before_connecting() {
        let c = KernelHttp {
            plugin: "x".into(),
            policy: Arc::new(
                OutboundPolicy::parse(&[json!("localhost")], &json!({}), false).unwrap_or_default(),
            ),
            kind: InvocationKind::Http,
            pool: None,
        };
        let req = |u: &str| {
            http::Request::builder()
                .uri(u)
                .body(bytes::Bytes::new())
                .unwrap_or_default()
        };
        let e = c
            .send(req("https://localhost/"), Duration::from_secs(1))
            .await
            .err()
            .map(|e| e.message)
            .unwrap_or_default();
        assert!(e.contains("non-public"), "{e}");
        let e = c
            .send(req("https://other.example/"), Duration::from_secs(1))
            .await
            .err()
            .map(|e| e.message)
            .unwrap_or_default();
        assert!(e.contains("may not call host"), "{e}");
        let e = c
            .send(req("http://localhost/"), Duration::from_secs(1))
            .await
            .err()
            .map(|e| e.message)
            .unwrap_or_default();
        assert!(e.contains("only https"), "{e}");
        let hook = KernelHttp {
            kind: InvocationKind::Hook,
            ..c
        };
        assert!(
            hook.send(req("https://localhost/"), Duration::from_secs(1))
                .await
                .is_err()
        );
    }
}
