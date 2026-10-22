//! 邮件渠道（LLD §1.4、§1.7）：提供方请求构造（http_relay / postmark / sendgrid / resend）、邮件正文、退订令牌。纯函数，便于单元测试。

use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::Sha256;

/// 邮件配置（`[plugin.notify]` 静态配置；凭据不放在后台可改的设置里）
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub provider: String,
    pub endpoint: String,
    pub token: String,
    pub from: String,
    pub site_name: String,
    pub site_url: String,
    pub unsubscribe_secret: String,
}

impl Config {
    pub fn from_static(s: &Value) -> Option<Self> {
        let st = |k: &str| s[k].as_str().unwrap_or("").trim().to_owned();
        let c = Self {
            provider: st("email_provider"),
            endpoint: st("email_endpoint"),
            token: st("email_token"),
            from: st("email_from"),
            site_name: Some(st("site_name"))
                .filter(|n| !n.is_empty())
                .or_else(|| s["_site"]["name"].as_str().map(str::to_owned))
                .unwrap_or_else(|| "PT Studio".into()),
            site_url: st("site_url").trim_end_matches('/').to_owned(),
            unsubscribe_secret: st("unsubscribe_secret"),
        };
        let ok = match c.provider.as_str() {
            "http_relay" => c.endpoint.starts_with("http"),
            "postmark" | "sendgrid" | "resend" => !c.token.is_empty(),
            _ => false,
        };
        (ok && c.from.contains('@')).then_some(c)
    }
}

pub struct Message {
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: String,
    /// 一键退订地址（List-Unsubscribe 头，RFC 8058）
    pub unsubscribe: Option<String>,
}

/// 退订：正文中的确认页链接与邮件头中的一键退订接口
pub struct Unsubscribe {
    pub page: String,
    pub one_click: String,
}

pub struct Request {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// 组装邮件：标题与正文是调用方给出的纯文本（不当作模板或 HTML）
pub fn compose(
    c: &Config,
    to: &str,
    title: &str,
    body: &str,
    link: Option<&str>,
    unsubscribe: Option<Unsubscribe>,
) -> Message {
    let title = title.trim();
    let subject = if title.is_empty() {
        format!("[{}] notification", c.site_name)
    } else {
        format!("[{}] {}", c.site_name, title.replace(['\r', '\n'], " "))
    };
    let url = link
        .filter(|_| !c.site_url.is_empty())
        .map(|l| format!("{}{l}", c.site_url));
    let mut text = body.trim().to_owned();
    let mut html = format!("<p>{}</p>", esc(body.trim()).replace('\n', "<br>"));
    if let Some(u) = &url {
        text.push_str(&format!("\n\n{u}"));
        html.push_str(&format!("<p><a href=\"{0}\">{0}</a></p>", esc(u)));
    }
    if let Some(u) = &unsubscribe {
        text.push_str(&format!("\n\n--\nUnsubscribe: {}", u.page));
        html.push_str(&format!(
            "<hr><p style=\"color:#888;font-size:12px\"><a href=\"{}\">Unsubscribe</a></p>",
            esc(&u.page)
        ));
    }
    Message {
        to: to.into(),
        subject,
        text,
        html,
        unsubscribe: unsubscribe.map(|u| u.one_click),
    }
}

fn list_headers(m: &Message) -> Vec<(String, String)> {
    m.unsubscribe
        .as_ref()
        .map(|u| {
            vec![
                ("List-Unsubscribe".to_owned(), format!("<{u}>")),
                (
                    "List-Unsubscribe-Post".to_owned(),
                    "List-Unsubscribe=One-Click".to_owned(),
                ),
            ]
        })
        .unwrap_or_default()
}

/// 提供方请求
pub fn request(c: &Config, m: &Message) -> Request {
    let hdrs = list_headers(m);
    let json_ct = ("content-type".to_owned(), "application/json".to_owned());
    match c.provider.as_str() {
        "postmark" => Request {
            url: "https://api.postmarkapp.com/email".into(),
            headers: vec![
                json_ct,
                ("accept".into(), "application/json".into()),
                ("x-postmark-server-token".into(), c.token.clone()),
            ],
            body: json!({ "From": c.from, "To": m.to, "Subject": m.subject, "TextBody": m.text, "HtmlBody": m.html,
                          "MessageStream": "outbound",
                          "Headers": hdrs.iter().map(|(k, v)| json!({ "Name": k, "Value": v })).collect::<Vec<_>>() })
            .to_string()
            .into_bytes(),
        },
        "sendgrid" => {
            let mut b = json!({ "personalizations": [{ "to": [{ "email": m.to }] }], "from": { "email": c.from },
                                "subject": m.subject,
                                "content": [{ "type": "text/plain", "value": m.text }, { "type": "text/html", "value": m.html }] });
            if !hdrs.is_empty() {
                b["headers"] = hdrs.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<serde_json::Map<_, _>>().into();
            }
            Request {
                url: "https://api.sendgrid.com/v3/mail/send".into(),
                headers: vec![json_ct, ("authorization".into(), format!("Bearer {}", c.token))],
                body: b.to_string().into_bytes(),
            }
        }
        "resend" => Request {
            url: "https://api.resend.com/emails".into(),
            headers: vec![json_ct, ("authorization".into(), format!("Bearer {}", c.token))],
            body: json!({ "from": c.from, "to": [m.to], "subject": m.subject, "text": m.text, "html": m.html,
                          "headers": hdrs.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<serde_json::Map<_, _>>() })
            .to_string()
            .into_bytes(),
        },
        // http_relay：站点自建的 HTTP→SMTP 中继
        _ => {
            let mut headers = vec![json_ct];
            if !c.token.is_empty() {
                headers.push(("authorization".into(), format!("Bearer {}", c.token)));
            }
            Request {
                url: c.endpoint.clone(),
                headers,
                body: json!({ "from": c.from, "to": m.to, "subject": m.subject, "text": m.text, "html": m.html,
                              "headers": hdrs.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<serde_json::Map<_, _>>() })
                .to_string()
                .into_bytes(),
            }
        }
    }
}

/// 投递结果归类：成功 / 临时失败（重试） / 永久失败
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Sent,
    Retry,
    Permanent,
}

pub fn classify(status: u16) -> Outcome {
    match status {
        200..=299 => Outcome::Sent,
        429 | 500..=599 => Outcome::Retry,
        _ => Outcome::Permanent,
    }
}

// ---------------- 退订令牌 ----------------

fn mac(secret: &str, payload: &str) -> String {
    let Ok(mut m) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return String::new();
    };
    m.update(payload.as_bytes());
    m.finalize()
        .into_bytes()
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `<uid>.<category>.<过期秒>.<HMAC 前 16 字节>`
pub fn unsubscribe_token(secret: &str, uid: i64, category: &str, exp_secs: i64) -> String {
    let payload = format!("{uid}.{category}.{exp_secs}");
    format!("{payload}.{}", mac(secret, &payload))
}

/// 校验令牌：返回 (用户, 类别)
pub fn verify_token(secret: &str, token: &str, now_secs: i64) -> Option<(i64, String)> {
    if secret.is_empty() {
        return None;
    }
    let (payload, sig) = token.rsplit_once('.')?;
    let expected = mac(secret, payload);
    // 定长比较
    if sig.len() != expected.len()
        || sig
            .bytes()
            .zip(expected.bytes())
            .fold(0u8, |a, (x, y)| a | (x ^ y))
            != 0
    {
        return None;
    }
    let mut f = payload.split('.');
    let uid: i64 = f.next()?.parse().ok()?;
    let cat = f.next()?.to_owned();
    let exp: i64 = f.next()?.parse().ok()?;
    (exp > now_secs && f.next().is_none()).then_some((uid, cat))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(provider: &str) -> Config {
        Config::from_static(&json!({ "email_provider": provider, "email_endpoint": "http://relay.local/send", "email_token": "tk",
                                     "email_from": "noreply@pt.example", "site_name": "PT", "site_url": "https://pt.example/" }))
        .unwrap()
    }

    #[test]
    fn config_validation() {
        assert!(Config::from_static(&json!({})).is_none());
        assert!(Config::from_static(&json!({ "email_provider": "postmark", "email_from": "a@b" })).is_none());
        assert!(
            Config::from_static(
                &json!({ "email_provider": "smtp", "email_token": "x", "email_from": "a@b" })
            )
            .is_none()
        );
        assert_eq!(cfg("http_relay").site_url, "https://pt.example");
    }

    #[test]
    fn composes_and_escapes() {
        let u = Unsubscribe {
            page: "https://pt.example/p?t=1".into(),
            one_click: "https://pt.example/api?t=1".into(),
        };
        let m = compose(
            &cfg("http_relay"),
            "u@x.y",
            "Hi\r\nthere",
            "a <b>\nc",
            Some("/torrents/1"),
            Some(u),
        );
        assert_eq!(m.subject, "[PT] Hi  there");
        assert!(m.html.contains("a &lt;b&gt;<br>c"), "{}", m.html);
        assert!(
            m.text.contains("https://pt.example/torrents/1")
                && m.text.contains("Unsubscribe: https://pt.example/p?t=1")
        );
        assert_eq!(m.unsubscribe.as_deref(), Some("https://pt.example/api?t=1"));
    }

    #[test]
    fn provider_requests() {
        let u = Unsubscribe {
            page: "https://p".into(),
            one_click: "https://u".into(),
        };
        let m = compose(&cfg("postmark"), "u@x.y", "t", "b", None, Some(u));
        let r = request(&cfg("postmark"), &m);
        let b: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(
            (
                r.url.as_str(),
                b["To"].as_str(),
                b["Headers"][0]["Value"].as_str()
            ),
            (
                "https://api.postmarkapp.com/email",
                Some("u@x.y"),
                Some("<https://u>")
            )
        );
        assert!(
            r.headers
                .iter()
                .any(|(k, v)| k == "x-postmark-server-token" && v == "tk")
        );
        let r = request(&cfg("sendgrid"), &m);
        let b: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(b["personalizations"][0]["to"][0]["email"], "u@x.y");
        assert_eq!(
            b["headers"]["List-Unsubscribe-Post"],
            "List-Unsubscribe=One-Click"
        );
        let r = request(&cfg("resend"), &m);
        let b: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(
            (
                r.url.as_str(),
                b["to"][0].as_str(),
                b["headers"]["List-Unsubscribe"].as_str()
            ),
            (
                "https://api.resend.com/emails",
                Some("u@x.y"),
                Some("<https://u>")
            )
        );
        assert!(
            r.headers
                .iter()
                .any(|(k, v)| k == "authorization" && v == "Bearer tk")
        );
        let r = request(&cfg("http_relay"), &m);
        let b: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(
            (r.url.as_str(), b["from"].as_str()),
            ("http://relay.local/send", Some("noreply@pt.example"))
        );
    }

    #[test]
    fn classification() {
        assert_eq!(classify(202), Outcome::Sent);
        assert_eq!(classify(429), Outcome::Retry);
        assert_eq!(classify(503), Outcome::Retry);
        assert_eq!(classify(422), Outcome::Permanent);
    }

    #[test]
    fn tokens() {
        let t = unsubscribe_token("s3cret", 42, "social", 2000);
        assert_eq!(verify_token("s3cret", &t, 1000), Some((42, "social".to_owned())));
        assert_eq!(verify_token("s3cret", &t, 2001), None, "expired");
        assert_eq!(verify_token("other", &t, 1000), None);
        assert_eq!(verify_token("", &t, 1000), None);
        let forged = t.replacen("42.", "43.", 1);
        assert_eq!(verify_token("s3cret", &forged, 1000), None);
    }
}
