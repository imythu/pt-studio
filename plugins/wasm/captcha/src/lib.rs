//! captcha 插件（WASM）：人机验证令牌校验。
//!
//! - `ctx.captcha.verify {action, token, ip?}` → `{ok, required}`：未配置服务商或该操作未开启时 `required = false`；
//!   服务不可用时按设置 `fail_open` 放行或返回 `captcha.unavailable`（503）。
//! - `GET /v1/config`：前端渲染组件所需的服务商、site key 与开启的操作（不含密钥）。
//!
//! 钩子中不能出站 HTTP，因此校验以服务的形式提供，由 identity 在登录、注册、找回密码前调用。

pt_plugin_sdk::world!("captcha", http);

use exports::pt::plugin::{http_handler, service_provider};
use pt_plugin_sdk::{config, http_client, prelude::*, serde_json};

struct Captcha;

const ACTIONS: &[&str] = &["login", "register", "password_reset"];

/// site.toml `[plugin.captcha]`：provider、site_key、secret 三者齐全才启用
struct Provider {
    name: String,
    site_key: String,
    secret: String,
    verify_url: String,
}

fn provider() -> Option<Provider> {
    let st: Value = serde_json::from_str(&config::get_static()).unwrap_or(Value::Null);
    let s = |k: &str| st[k].as_str().unwrap_or("").trim().to_owned();
    let name = s("provider");
    let default_url = match name.as_str() {
        "turnstile" => "https://challenges.cloudflare.com/turnstile/v0/siteverify",
        "hcaptcha" => "https://api.hcaptcha.com/siteverify",
        "recaptcha" => "https://www.google.com/recaptcha/api/siteverify",
        _ => return None,
    };
    let p = Provider {
        site_key: s("site_key"),
        secret: s("secret"),
        verify_url: Some(s("verify_url")).filter(|u| !u.is_empty()).unwrap_or_else(|| default_url.to_owned()),
        name,
    };
    (!p.site_key.is_empty() && !p.secret.is_empty()).then_some(p)
}

fn settings() -> (Vec<String>, bool) {
    let s: Value = serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null);
    let actions = s["actions"].as_array().map_or_else(
        || vec!["register".to_owned(), "password_reset".to_owned()],
        |a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter(|x| ACTIONS.contains(x))
                .map(str::to_owned)
                .collect()
        },
    );
    (actions, s["fail_open"].as_bool().unwrap_or(false))
}

/// application/x-www-form-urlencoded
fn form_encode(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| {
            let enc = |s: &str| {
                s.bytes()
                    .map(|b| match b {
                        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
                        _ => format!("%{b:02X}"),
                    })
                    .collect::<String>()
            };
            format!("{}={}", enc(k), enc(v))
        })
        .collect::<Vec<_>>()
        .join("&")
}

fn verify(a: &Value) -> R<Value> {
    let action = a["action"].as_str().unwrap_or("");
    let Some(p) = provider() else {
        return Ok(json!({ "ok": true, "required": false }));
    };
    let (actions, fail_open) = settings();
    if !actions.iter().any(|x| x == action) {
        return Ok(json!({ "ok": true, "required": false }));
    }
    let token = a["token"].as_str().unwrap_or("").trim();
    if token.is_empty() || token.len() > 4096 {
        return Ok(json!({ "ok": false, "required": true }));
    }
    let mut pairs = vec![("secret", p.secret.as_str()), ("response", token)];
    if let Some(ip) = a["ip"].as_str() {
        pairs.push(("remoteip", ip));
    }
    let resp = http_client::send(&http_client::OutboundRequest {
        method: "POST".into(),
        url: p.verify_url.clone(),
        headers: vec![http_client::Header {
            name: "content-type".into(),
            value: "application/x-www-form-urlencoded".into(),
        }],
        body: Some(form_encode(&pairs).into_bytes()),
        timeout_ms: 5_000,
    });
    let body = match resp {
        Ok(r) if r.status == 200 => serde_json::from_slice::<Value>(&r.body).unwrap_or(Value::Null),
        other => {
            let why = match other {
                Ok(r) => format!("{} returned {}", p.name, r.status),
                Err(e) => e.message,
            };
            logging::warn("captcha verification unavailable", &[("reason", &why)]);
            if fail_open {
                return Ok(json!({ "ok": true, "required": true, "degraded": true }));
            }
            return Err(err("captcha.unavailable", "human verification is temporarily unavailable"));
        }
    };
    let ok = body["success"] == true;
    if !ok {
        logging::info("captcha rejected", &[("errors", &body["error-codes"].to_string())]);
    }
    Ok(json!({ "ok": ok, "required": true }))
}

impl service_provider::Guest for Captcha {
    fn call(service: String, method: String, args: String) -> R<String> {
        if service != "ctx.captcha" {
            return Err(err("kernel.not_found", "unknown service"));
        }
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        match method.as_str() {
            "verify" => Ok(verify(&a)?.to_string()),
            other => Err(err("kernel.not_found", &format!("ctx.captcha has no method `{other}`"))),
        }
    }
}

impl http_handler::Guest for Captcha {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        match req.route() {
            "config" => {
                let (actions, _) = settings();
                let v = match provider() {
                    Some(p) => json!({ "enabled": true, "provider": p.name, "site_key": p.site_key, "actions": actions }),
                    None => json!({ "enabled": false, "actions": [] }),
                };
                Ok(http::Reply::ok(&v).into())
            }
            _ => Err(err("kernel.not_found", "no such route")),
        }
    }
}

export!(Captcha);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_forms() {
        assert_eq!(form_encode(&[("a", "x y"), ("b", "1&2=3")]), "a=x%20y&b=1%262%3D3");
    }
}
