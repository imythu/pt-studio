//! ai-upload-review 插件（WASM，docs/plugins/ai-upload-review/）：上传后预审——确定性检查 + 经 `ctx.llm` 的模型预审，
//! 生成报告供审核员参考（不自动处置）。
//!
//! 本版本实现：`torrent.created` → `review` 延迟任务、确定性规则（MediaInfo 缺失、只有压缩包、只有样片、要求 NFO 的类型）、
//! 模型预审（固定系统提示、可信事实与不可信内容分开、结构化输出、幂等键 `aiur:<id>:<版本>`、每日预算）、结论合并、报告表、
//! `ai.upload_reviewed`、HTTP 查看 / 重审 / 反馈。规格与 MediaInfo 比对（ctx.mediainfo）、疑似重复（ctx.search）在后续迭代实现。

#![allow(clippy::missing_errors_doc)]

pt_plugin_sdk::world!("ai-upload-review");

pub mod review;

use exports::pt::plugin::{http_handler, on_cron, on_event};
use pt_plugin_sdk::{
    config, events, invocation, jobs, redis, services,
    sql::{self, SqlValue},
    time,
    types::PluginError,
};
use serde_json::{Value, json};

struct AiUploadReview;

type R<T> = Result<T, PluginError>;

fn err(code: &str, message: &str) -> PluginError {
    PluginError {
        code: code.into(),
        message: message.into(),
        details: None,
        retryable: false,
    }
}

fn settings() -> Value {
    serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null)
}

fn text(v: &SqlValue) -> Option<String> {
    match v {
        SqlValue::Text(s) | SqlValue::Json(s) => Some(s.clone()),
        _ => None,
    }
}

fn int(v: &SqlValue) -> Option<i64> {
    match v {
        SqlValue::Int(i) => Some(*i),
        _ => None,
    }
}

/// 种子全貌（torrent.v_torrent_review）
fn load(id: i64) -> R<Option<Value>> {
    let r = sql::query(
        "SELECT type_id, title, subtitle, description, mediainfo, fields::text, size, file_count, has_nfo, version, files::text
           FROM torrent.v_torrent_review WHERE id = $1",
        &[SqlValue::Int(id)],
    )?;
    let Some(row) = r.rows.first() else {
        return Ok(None);
    };
    let j = |i: usize| {
        text(&row[i])
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .unwrap_or(Value::Null)
    };
    Ok(Some(json!({
        "id": id, "type_id": text(&row[0]), "title": text(&row[1]), "subtitle": text(&row[2]), "description": text(&row[3]),
        "mediainfo": text(&row[4]), "fields": j(5), "size": int(&row[6]), "file_count": int(&row[7]),
        "has_nfo": matches!(row[8], SqlValue::Boolean(true)), "version": int(&row[9]), "files": j(10),
    })))
}

/// 今日模型调用计数 +1，超出预算返回 false
fn within_budget(limit: i64) -> R<bool> {
    let day = time::now() / 86_400_000;
    let key = format!("budget:{day}");
    let r = redis::pipeline(
        &[
            redis::Command {
                name: "INCR".into(),
                args: vec![key.clone().into_bytes()],
            },
            redis::Command {
                name: "EXPIRE".into(),
                args: vec![key.into_bytes(), b"172800".to_vec()],
            },
        ],
        true,
    )?;
    Ok(matches!(r.first(), Some(redis::RedisReply::Int(n)) if *n <= limit))
}

/// 模型预审；Ok(Err(原因)) 表示跳过或失败（只保留确定性结果）
fn model_review(
    t: &Value,
    files: &[(String, u64)],
    rule_findings: &[review::Finding],
    s: &Value,
) -> R<Result<(Value, Value, String), String>> {
    if s["model_enabled"].as_bool() == Some(false) {
        return Ok(Err("disabled".into()));
    }
    if !within_budget(s["daily_model_budget"].as_i64().unwrap_or(500))? {
        return Ok(Err("budget".into()));
    }
    let req = json!({
        "profile": s["profile"].as_str().unwrap_or("cheap"), "system": review::SYSTEM_PROMPT,
        "messages": review::messages(t, files, rule_findings), "tools": [], "tool_mode": "none",
        "output_schema": review::output_schema(), "max_output_tokens": 1500, "effort": "low",
        "idempotency_key": format!("aiur:{}:{}", t["id"], t["version"]), "cache_prefix_messages": 0,
    });
    match services::call("ctx.llm", "complete", &req.to_string()) {
        Ok(out) => {
            let v: Value = serde_json::from_str(&out).unwrap_or(Value::Null);
            if v["stop"] == "refusal" || v["structured"].is_null() {
                return Ok(Err("model_failed".into()));
            }
            Ok(Ok((
                v["structured"].clone(),
                v["usage"].clone(),
                v["model"].as_str().unwrap_or("").to_owned(),
            )))
        }
        // 未安装 / 未配置提供方、结构化输出无效、配额用尽等：只保留确定性结果，不让任务重试
        Err(e)
            if e.code.ends_with(".not_configured")
                || e.code == "kernel.service_unavailable"
                || e.code == "kernel.not_found" =>
        {
            Ok(Err("unavailable".into()))
        }
        Err(e) if e.code.ends_with(".quota_exceeded") => Ok(Err("quota".into())),
        Err(e) if e.code.ends_with(".structured_invalid") => Ok(Err("model_failed".into())),
        Err(e) => Err(e),
    }
}

fn run_review(id: i64) -> R<Option<Value>> {
    let Some(t) = load(id)? else {
        return Ok(None);
    };
    let s = settings();
    let nfo_types: Vec<String> = s["require_nfo_types"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    let files = review::files_of(&t["files"]);
    let rule_findings = review::rules(&t, &files, &nfo_types);
    let (model_out, usage, model, skipped) = match model_review(&t, &files, &rule_findings, &s)? {
        Ok((m, u, model)) => (Some(m), u, Some(model), None),
        Err(reason) => (None, Value::Null, None, Some(reason)),
    };
    let (verdict, confidence, findings) = review::merge(&rule_findings, model_out.as_ref());
    let fj: Vec<Value> = findings.iter().map(review::Finding::to_json).collect();
    let revision = t["version"].as_i64().unwrap_or(1);
    let opt = |v: Option<String>| v.map_or(SqlValue::Null, SqlValue::Text);
    let r = sql::query(
        "INSERT INTO reports (torrent_id, revision, verdict, confidence, findings, model, model_skipped, usage)
         VALUES ($1, $2, $3, $4, $5::jsonb, $6, $7, $8::jsonb)
         ON CONFLICT (torrent_id, revision) DO UPDATE SET verdict = EXCLUDED.verdict, confidence = EXCLUDED.confidence,
             findings = EXCLUDED.findings, model = EXCLUDED.model, model_skipped = EXCLUDED.model_skipped, usage = EXCLUDED.usage,
             feedback = NULL, feedback_by = NULL, created_at = now()
         RETURNING id",
        &[
            SqlValue::Int(id),
            SqlValue::Int(revision),
            SqlValue::Text(verdict.clone()),
            confidence.map_or(SqlValue::Null, SqlValue::Float),
            SqlValue::Text(Value::Array(fj.clone()).to_string()),
            opt(model),
            opt(skipped.clone()),
            if usage.is_null() { SqlValue::Null } else { SqlValue::Text(usage.to_string()) },
        ],
    )?;
    let report_id = r.rows.first().and_then(|row| int(&row[0])).unwrap_or(0);
    events::publish(&events::NewEvent {
        event_type: "ai.upload_reviewed".into(),
        version: 1,
        key: format!("torrent:{id}"),
        payload: json!({ "torrent_id": id, "report_id": report_id, "verdict": verdict,
                         "findings": findings.iter().map(|f| json!({ "code": f.code, "severity": f.severity })).collect::<Vec<_>>(),
                         "model_skipped": skipped })
        .to_string(),
    })?;
    Ok(Some(json!({ "report_id": report_id })))
}

impl on_event::Guest for AiUploadReview {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        if evt.event_type != "torrent.created" || settings()["enabled"].as_bool() == Some(false) {
            return Ok(());
        }
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        let id = p["torrent_id"].as_i64().unwrap_or(0);
        jobs::schedule_once(
            "review",
            time::now(),
            &json!({ "torrent_id": id }).to_string(),
            Some(&format!("review:{id}")),
        )?;
        Ok(())
    }
}

impl on_cron::Guest for AiUploadReview {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        if job.name != "review" {
            return Err(err("kernel.not_found", "unknown job"));
        }
        let p: Value = serde_json::from_str(&job.payload).unwrap_or(Value::Null);
        run_review(p["torrent_id"].as_i64().unwrap_or(0)).map(|_| ())
    }
}

// ---------------- HTTP（LLD §2.3） ----------------

fn resp(status: u16, v: &Value) -> http_handler::Response {
    http_handler::Response {
        status,
        headers: vec![http_handler::Header {
            name: "content-type".into(),
            value: "application/json".into(),
        }],
        body: v.to_string().into_bytes(),
    }
}

fn param(req: &http_handler::Request, k: &str) -> R<i64> {
    req.params
        .iter()
        .find(|(n, _)| n == k)
        .and_then(|(_, v)| v.parse().ok())
        .ok_or_else(|| err("kernel.validation_failed", "bad id"))
}

fn latest(torrent_id: i64) -> R<Option<Value>> {
    let r = sql::query(
        "SELECT json_build_object('report_id', id, 'torrent_id', torrent_id, 'revision', revision, 'verdict', verdict,
                'confidence', confidence, 'findings', findings, 'model', model, 'model_skipped', model_skipped,
                'feedback', feedback, 'created_at', (extract(epoch FROM created_at) * 1000)::bigint)::text
           FROM reports WHERE torrent_id = $1 ORDER BY revision DESC LIMIT 1",
        &[SqlValue::Int(torrent_id)],
    )?;
    Ok(r.rows
        .first()
        .and_then(|row| text(&row[0]))
        .and_then(|s| serde_json::from_str(&s).ok()))
}

impl http_handler::Guest for AiUploadReview {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        match req.route_id.as_str() {
            "get_report" => {
                let id = param(&req, "id")?;
                latest(id)?
                    .map(|r| resp(200, &r))
                    .ok_or_else(|| err("ai-upload-review.not_found", "no report for this torrent"))
            }
            // 重审：同步执行（审核员等待结果）；预算用尽时报错（请求的工作单元回滚，不留下只有规则结果的报告）
            "rerun" => {
                let id = param(&req, "id")?;
                if run_review(id)?.is_none() {
                    return Err(err("kernel.not_found", "torrent not found"));
                }
                let r = latest(id)?.unwrap_or(Value::Null);
                if r["model_skipped"] == "budget" {
                    return Err(err(
                        "ai-upload-review.budget_exhausted",
                        "the daily model budget is used up",
                    ));
                }
                Ok(resp(200, &r))
            }
            "feedback" => {
                let id = param(&req, "id")?;
                let b: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
                let fb = match b["useful"].as_bool() {
                    Some(true) => "useful",
                    Some(false) => "false_positive",
                    None => return Err(err("kernel.validation_failed", "`useful` must be a boolean")),
                };
                let uid = invocation::current()
                    .actor
                    .user_id
                    .map_or(SqlValue::Null, |u| SqlValue::Int(i64::try_from(u).unwrap_or(0)));
                let n = sql::execute(
                    "UPDATE reports SET feedback = $2, feedback_by = $3 WHERE id = $1",
                    &[SqlValue::Int(id), SqlValue::Text(fb.into()), uid],
                )?;
                if n == 0 {
                    return Err(err("ai-upload-review.not_found", "report not found"));
                }
                Ok(http_handler::Response {
                    status: 204,
                    headers: vec![],
                    body: vec![],
                })
            }
            "stats" => {
                let r = sql::query(
                    "SELECT json_build_object('reports', count(*), 'pass', count(*) FILTER (WHERE verdict = 'pass'),
                            'warn', count(*) FILTER (WHERE verdict = 'warn'), 'fail', count(*) FILTER (WHERE verdict = 'fail'),
                            'model_skipped', count(*) FILTER (WHERE model_skipped IS NOT NULL),
                            'useful', count(*) FILTER (WHERE feedback = 'useful'), 'false_positive', count(*) FILTER (WHERE feedback = 'false_positive'))::text
                       FROM reports WHERE created_at > now() - interval '30 days'",
                    &[],
                )?;
                let v = r
                    .rows
                    .first()
                    .and_then(|row| text(&row[0]))
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or(Value::Null);
                Ok(resp(200, &v))
            }
            _ => Err(err("kernel.not_found", "no such route")),
        }
    }
}

export!(AiUploadReview);
