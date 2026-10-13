//! llm-claude 插件（Native，docs/plugins/llm-claude/）：`ctx.llm` 的 Claude 实现（Messages API）。
//!
//! 本版本实现：JSON 分派的 `complete` / `models` / `embed`（不支持，501）、profile（作用域 `profile:<name>`）、请求映射
//! （系统提示缓存断点、不可信内容包裹与说明、工具、结构化输出 schema 降级、助手历史原样回放）、响应解析、结构化输出本地
//! 校验、重试（429 / 5xx / 连接错误，尊重 `retry-after`，受调用截止时间约束）、错误映射、敏感模式脱敏、幂等（kv，24 小时）、
//! 每插件 / 每用户每日 token 配额（Redis）、`llm.usage_recorded` 与每日用量表、后台用量查询与连通测试。

pub mod mapper;

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use axum::{
    Json, Router,
    extract::Query,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use mapper::Profile;
use pt_plugin_api::{
    Ctx, HttpRoutes, NativePlugin, NewEvent, PResult, PluginError, RedisCommand, RequestCtx,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;

pub const MANIFEST: &str = include_str!("../plugin.toml");
pub const MIGRATIONS: &[(&str, &str)] = &[("0001_init.sql", include_str!("../migrations/0001_init.sql"))];

const DEFAULT_MODEL: &str = "claude-opus-5-5";
const API_VERSION: &str = "2023-06-01";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const MAX_RETRIES: u32 = 2;
const IDEM_TTL_MS: i64 = 24 * 3600 * 1000;

fn e(code: &str, msg: impl Into<String>) -> PluginError {
    PluginError::new(format!("llm-claude.{code}"), msg)
}

/// 静态配置（site.toml `[plugin.llm-claude]`）
#[derive(Debug, Clone)]
struct Settings {
    api_key: Option<String>,
    api_base: String,
    fallbacks: bool,
    profiles: Vec<Profile>,
    per_plugin_daily_tokens: u64,
    per_user_daily_tokens: u64,
}

impl Settings {
    fn from(v: &Value) -> Self {
        let mut profiles: Vec<Profile> = v["profiles"]
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, p)| Profile {
                name: name.clone(),
                model: p["model"].as_str().unwrap_or(DEFAULT_MODEL).to_owned(),
                max_tokens: p["max_tokens"].as_u64().unwrap_or(16000),
                effort: p["effort"].as_str().map(str::to_owned),
            })
            .collect();
        if !profiles.iter().any(|p| p.name == "default") {
            profiles.push(Profile {
                name: "default".into(),
                model: DEFAULT_MODEL.into(),
                max_tokens: 16000,
                effort: Some("medium".into()),
            });
        }
        profiles.sort_by(|a, b| a.name.cmp(&b.name));
        Self {
            api_key: v["api_key"].as_str().filter(|k| !k.is_empty()).map(str::to_owned),
            api_base: v["api_base"]
                .as_str()
                .unwrap_or("https://api.anthropic.com")
                .trim_end_matches('/')
                .to_owned(),
            // 服务端回退为 beta 能力，默认关闭
            fallbacks: v["fallbacks_enabled"].as_bool().unwrap_or(false),
            profiles,
            per_plugin_daily_tokens: v["per_plugin_daily_tokens"].as_u64().unwrap_or(0),
            per_user_daily_tokens: v["per_user_daily_tokens"].as_u64().unwrap_or(0),
        }
    }

    /// 未配置的 profile 退回 default（作用域检查仍按请求的名字）
    fn profile(&self, name: &str) -> &Profile {
        self.profiles
            .iter()
            .find(|p| p.name == name)
            .or_else(|| self.profiles.iter().find(|p| p.name == "default"))
            .unwrap_or(&self.profiles[0])
    }
}

fn today() -> String {
    chrono::Utc::now().format("%Y%m%d").to_string()
}

async fn counter(ctx: &Ctx, key: &str) -> PResult<u64> {
    let r = ctx.redis.call(RedisCommand::new("GET").arg(key)).await?;
    Ok(match r {
        pt_plugin_api::RedisReply::Bulk(b) => String::from_utf8_lossy(&b).parse().unwrap_or(0),
        other => other.as_int().and_then(|i| u64::try_from(i).ok()).unwrap_or(0),
    })
}

async fn add(ctx: &Ctx, key: &str, n: u64) -> PResult<()> {
    ctx.redis
        .pipeline(
            vec![
                RedisCommand::new("INCRBY").arg(key).arg(n.to_string()),
                RedisCommand::new("EXPIRE").arg(key).arg("172800"),
            ],
            true,
        )
        .await?;
    Ok(())
}

/// 调用一次 Messages API（含重试）；返回 (响应 JSON, request-id)
async fn send(ctx: &Ctx, s: &Settings, key: &str, body: &Value) -> PResult<(Value, Option<String>)> {
    let bytes = bytes::Bytes::from(body.to_string());
    let mut attempt = 0;
    loop {
        let remaining = ctx.invocation.deadline - chrono::Utc::now().timestamp_millis();
        if remaining <= 500 {
            return Err(
                PluginError::new("kernel.timeout", "deadline exceeded before calling the model").retryable(),
            );
        }
        let mut rb = http::Request::post(format!("{}/v1/messages", s.api_base))
            .header("content-type", "application/json")
            .header("x-api-key", key)
            .header("anthropic-version", API_VERSION);
        if s.fallbacks {
            rb = rb.header("anthropic-beta", FALLBACK_BETA);
        }
        let req = rb
            .body(bytes.clone())
            .map_err(|err| PluginError::internal(err.to_string()))?;
        let timeout = Duration::from_millis(u64::try_from(remaining.min(120_000)).unwrap_or(120_000));
        let (status, retry_after, out) = match ctx.http.send(req, timeout).await {
            Ok(resp) => {
                let st = resp.status().as_u16();
                let ra = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok());
                let rid = resp
                    .headers()
                    .get("request-id")
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned);
                let v: Value = serde_json::from_slice(resp.body()).unwrap_or(Value::Null);
                if (200..300).contains(&st) {
                    return Ok((v, rid));
                }
                (st, ra, Some((v, rid)))
            }
            // 能力拒绝（主机不在 http_outbound）等不重试
            Err(err) if !err.retryable => return Err(err),
            Err(_) => (0, None, None),
        };
        let (suffix, retry) = if status == 0 {
            ("upstream_error", true)
        } else {
            mapper::classify(status)
        };
        if retry && attempt < MAX_RETRIES {
            let wait = retry_after.map_or_else(
                || Duration::from_secs(if attempt == 0 { 1 } else { 4 }),
                |s| Duration::from_secs(s.min(30)),
            );
            let left = ctx.invocation.deadline - chrono::Utc::now().timestamp_millis();
            if i64::try_from(wait.as_millis()).unwrap_or(i64::MAX) + 1000 < left {
                attempt += 1;
                tokio::time::sleep(wait).await;
                continue;
            }
        }
        let (msg, rid) = out.map_or(
            (String::from("connection to the model service failed"), None),
            |(v, rid)| {
                (
                    v["error"]["message"]
                        .as_str()
                        .map(|m| mapper::scrub(m).0)
                        .unwrap_or_else(|| format!("upstream status {status}")),
                    rid,
                )
            },
        );
        tracing::warn!(
            status,
            request_id = rid.as_deref().unwrap_or(""),
            "llm-claude upstream error"
        );
        let mut err = if suffix == "validation_failed" {
            PluginError::validation(msg)
        } else {
            e(suffix, msg)
        };
        err.details = Some(json!({ "status": status, "request_id": rid }));
        if retry {
            err = err.retryable();
        }
        return Err(err);
    }
}

/// `complete`：`caller` 为计量归属的插件（服务调用时为调用方，后台测试时为本插件）
async fn complete(ctx: &Ctx, caller: &str, mut req: Value) -> PResult<Value> {
    let s = Settings::from(&ctx.config.get_static());
    let key = s
        .api_key
        .clone()
        .ok_or_else(|| e("not_configured", "api_key is not configured"))?;
    let pname = req["profile"].as_str().unwrap_or("default").to_owned();
    if caller != "llm-claude" && pname != "default" && !ctx.invocation.has_scope(&format!("profile:{pname}"))
    {
        return Err(PluginError::forbidden(format!("scope profile:{pname} required")));
    }
    let profile = s.profile(&pname).clone();
    let idem = req["idempotency_key"]
        .as_str()
        .filter(|k| !k.is_empty())
        .map(|k| format!("idem:{caller}:{k}"));
    if let Some(k) = &idem
        && let Some(hit) = ctx.kv.get(k).await?
        && let Ok(v) = serde_json::from_slice::<Value>(&hit)
    {
        return Ok(v);
    }
    let hits = mapper::scrub_request(&mut req);
    if !hits.is_empty() {
        tracing::warn!(caller, kinds = ?hits, "llm-claude scrubbed sensitive patterns from a request");
    }
    let body = mapper::build(&req, &profile, s.fallbacks).map_err(PluginError::validation)?;
    // 配额预检：输出上限 + 粗估输入（字符数 / 3）
    let est = body["max_tokens"].as_u64().unwrap_or(0)
        + (body["messages"].to_string().chars().count() as u64
            + body["system"].to_string().chars().count() as u64)
            / 3;
    let day = today();
    let pkey = format!("q:p:{caller}:{day}");
    let end_user = req["end_user"].as_u64();
    let ukey = end_user.map(|u| format!("q:u:{u}:{day}"));
    if s.per_plugin_daily_tokens > 0 && counter(ctx, &pkey).await? + est > s.per_plugin_daily_tokens {
        return Err(e(
            "quota_exceeded",
            format!("daily token quota of `{caller}` is used up"),
        )
        .retryable());
    }
    if let (true, Some(k)) = (s.per_user_daily_tokens > 0, &ukey)
        && counter(ctx, k).await? + est > s.per_user_daily_tokens
    {
        return Err(e("quota_exceeded", "daily token quota of this user is used up"));
    }
    let started = std::time::Instant::now();
    let (resp, rid) = send(ctx, &s, &key, &body).await?;
    let latency = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    let mut out = mapper::parse(&resp);
    out["provider_request_id"] = json!(rid);
    let model = out["model"].as_str().unwrap_or(&profile.model).to_owned();
    let (inp, outp, cached) = (
        out["usage"]["input_tokens"].as_u64().unwrap_or(0),
        out["usage"]["output_tokens"].as_u64().unwrap_or(0),
        out["usage"]["cached_input_tokens"].as_u64().unwrap_or(0),
    );
    add(ctx, &pkey, inp + outp).await?;
    if let Some(k) = &ukey {
        add(ctx, k, inp + outp).await?;
    }
    let stop = out["stop"].as_str().unwrap_or("other").to_owned();
    {
        let mut c = ctx.sql.conn().await?;
        sqlx::query(
            "INSERT INTO usage_daily (day, caller_plugin, model, calls, input_tokens, output_tokens, cached_input_tokens, refusals)
             VALUES (current_date, $1, $2, 1, $3, $4, $5, $6)
             ON CONFLICT (day, caller_plugin, model) DO UPDATE SET calls = usage_daily.calls + 1,
                 input_tokens = usage_daily.input_tokens + EXCLUDED.input_tokens, output_tokens = usage_daily.output_tokens + EXCLUDED.output_tokens,
                 cached_input_tokens = usage_daily.cached_input_tokens + EXCLUDED.cached_input_tokens, refusals = usage_daily.refusals + EXCLUDED.refusals",
        )
        .bind(caller)
        .bind(&model)
        .bind(i64::try_from(inp).unwrap_or(i64::MAX))
        .bind(i64::try_from(outp).unwrap_or(i64::MAX))
        .bind(i64::try_from(cached).unwrap_or(i64::MAX))
        .bind(i64::from(stop == "refusal"))
        .execute(&mut *c)
        .await?;
    }
    ctx.events
        .publish(NewEvent::v1(
            "llm.usage_recorded",
            format!("plugin:{caller}"),
            json!({ "caller_plugin": caller, "model": model, "input_tokens": inp, "output_tokens": outp, "cached_input_tokens": cached,
                    "latency_ms": latency, "stop": stop, "end_user_id": end_user, "fallback_used": out["fallback_used"] }),
        ))
        .await?;
    // 结构化输出：本地按原始 schema 校验（拒答时没有结构化结果）
    if let Some(schema) = req.get("output_schema").filter(|s| !s.is_null())
        && stop != "refusal"
    {
        let parsed: Value = serde_json::from_str(out["text"].as_str().unwrap_or(""))
            .map_err(|_| e("structured_invalid", "the model did not return valid JSON").retryable())?;
        mapper::validate(schema, &parsed).map_err(|m| e("structured_invalid", m).retryable())?;
        out["structured"] = parsed;
    }
    if let Some(k) = &idem {
        ctx.kv
            .set(
                k,
                out.to_string().as_bytes(),
                Some(chrono::Utc::now().timestamp_millis() + IDEM_TTL_MS),
            )
            .await?;
    }
    Ok(out)
}

fn models(ctx: &Ctx) -> Value {
    let s = Settings::from(&ctx.config.get_static());
    Value::Array(
        s.profiles
            .iter()
            .map(|p| {
                let ctx_tokens = if p.model.starts_with("claude-haiku") { 200_000 } else { 1_000_000 };
                json!({ "profile": p.name, "model": p.model, "supports_tools": true, "supports_structured": true, "supports_embed": false,
                        "context_tokens": ctx_tokens })
            })
            .collect(),
    )
}

struct LlmSvc;

#[async_trait]
impl pt_plugin_api::JsonService for LlmSvc {
    async fn call(&self, ctx: &Ctx, method: &str, a: Value) -> PResult<Value> {
        if !ctx.invocation.has_scope("use") {
            return Err(PluginError::forbidden("scope use required"));
        }
        match method {
            "complete" => {
                let caller = ctx.invocation.caller_plugin.clone();
                complete(ctx, &caller, a).await
            }
            "models" => Ok(models(ctx)),
            "embed" => Err(e("unsupported", "embeddings are not provided by llm-claude")),
            other => Err(PluginError::not_found(format!("ctx.llm has no method `{other}`"))),
        }
    }
}

// ---------------- 后台 HTTP（LLD §2.4） ----------------

#[derive(Deserialize)]
struct UsageQuery {
    from: Option<chrono::NaiveDate>,
    to: Option<chrono::NaiveDate>,
    group_by: Option<String>,
}

async fn http_usage(ctx: RequestCtx, Query(q): Query<UsageQuery>) -> PResult<Response> {
    let to = q.to.unwrap_or_else(|| chrono::Utc::now().date_naive());
    let from = q.from.unwrap_or(to - chrono::Days::new(30));
    let col = match q.group_by.as_deref().unwrap_or("plugin") {
        "plugin" => "caller_plugin",
        "model" => "model",
        "day" => "day::text",
        _ => return Err(PluginError::validation("group_by must be plugin, model or day")),
    };
    let mut c = ctx.sql.conn().await?;
    let rows = sqlx::query(&format!(
        "SELECT {col} AS k, sum(calls)::bigint, sum(input_tokens)::bigint, sum(output_tokens)::bigint, sum(cached_input_tokens)::bigint,
                sum(refusals)::bigint
           FROM usage_daily WHERE day BETWEEN $1 AND $2 GROUP BY 1 ORDER BY 1"
    ))
    .bind(from)
    .bind(to)
    .fetch_all(&mut *c)
    .await?;
    let items: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({ "key": r.get::<String, _>(0), "calls": r.get::<i64, _>(1), "input_tokens": r.get::<i64, _>(2),
                    "output_tokens": r.get::<i64, _>(3), "cached_input_tokens": r.get::<i64, _>(4), "refusals": r.get::<i64, _>(5) })
        })
        .collect();
    Ok(Json(json!({ "from": from, "to": to, "items": items })).into_response())
}

#[derive(Deserialize)]
struct TestBody {
    profile: Option<String>,
}

async fn http_test(ctx: RequestCtx, Json(b): Json<TestBody>) -> PResult<Response> {
    let started = std::time::Instant::now();
    let r = complete(
        &ctx,
        "llm-claude",
        json!({ "profile": b.profile.unwrap_or_else(|| "default".into()), "system": "Reply with the single word: pong",
                "messages": [{ "role": "user", "parts": [{ "type": "text", "text": "ping" }] }], "max_output_tokens": 16, "effort": "low" }),
    )
    .await?;
    Ok(Json(json!({ "model": r["model"], "text": r["text"], "usage": r["usage"], "latency_ms": started.elapsed().as_millis() as u64 }))
        .into_response())
}

struct Routes;

impl HttpRoutes for Routes {
    fn router(&self) -> Router {
        Router::new()
            .route("/v1/admin/usage", get(http_usage))
            .route("/v1/admin/test", post(http_test))
    }
}

pub fn plugin() -> NativePlugin {
    NativePlugin {
        manifest: MANIFEST,
        migrations: MIGRATIONS,
        http: Some(Arc::new(Routes)),
        json_services: vec![("ctx.llm", Arc::new(LlmSvc))],
        ..Default::default()
    }
}
