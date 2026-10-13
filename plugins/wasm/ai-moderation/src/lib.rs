//! ai-moderation 插件（WASM，docs/plugins/ai-moderation/）：评论的 AI 审核——本地规则 + 经 `ctx.llm` 的分类，达到阈值时以系统
//! 身份建举报工单（交给人工处理），不直接删除内容。
//!
//! 本版本实现：`comment.created` → 抽样（新用户必审）→ `classify_item` 延迟任务；本地规则（新账号大量链接、垃圾域名、重复刷屏、
//! passkey / 邮箱 / 手机号）；模型分类（固定系统提示、正文为不可信内容、结构化输出、`cheap` profile、幂等键、每日预算，拒答记为
//! 需人工抽查）；动作（≥ report_threshold 经 `ctx.reports` 建工单并发布 `ai.content_flagged`，≥ review_threshold 进入后台抽查列表）；
//! 工单结论反馈（`report.resolved` → 真阳性 / 误报）；`comment.before_create` 钩子（只用本地规则，默认关闭）；
//! `ctx.ai_moderation.classify`；后台标记列表与按类别准确率。

#![allow(clippy::missing_errors_doc)]

pt_plugin_sdk::world!("ai-moderation");

pub mod local;

use exports::pt::plugin::{hooks, http_handler, on_cron, on_event, service_provider};
use local::Score;
use pt_plugin_sdk::{
    config, events, jobs, redis, services,
    sql::{self, SqlValue},
    time,
    types::PluginError,
};
use serde_json::{Value, json};

struct AiModeration;

type R<T> = Result<T, PluginError>;

fn err(code: &str, message: &str) -> PluginError {
    PluginError {
        code: code.into(),
        message: message.into(),
        details: None,
        retryable: false,
    }
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

fn settings() -> Value {
    serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null)
}

fn f(s: &Value, k: &str, d: f64) -> f64 {
    s[k].as_f64().unwrap_or(d)
}

fn svc(service: &str, method: &str, args: &Value) -> R<Value> {
    let out = services::call(service, method, &args.to_string())?;
    Ok(serde_json::from_str(&out).unwrap_or(Value::Null))
}

fn spam_domains(s: &Value) -> Vec<String> {
    s["spam_domains"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default()
}

fn account_days(uid: Option<i64>) -> R<i64> {
    let Some(uid) = uid else { return Ok(i64::MAX) };
    let r = sql::query(
        "SELECT (extract(epoch FROM now() - created_at) / 86400)::bigint FROM identity.v_user_public WHERE id = $1",
        &[SqlValue::Int(uid)],
    )?;
    Ok(r.rows.first().and_then(|row| int(&row[0])).unwrap_or(i64::MAX))
}

/// 今日模型调用计数 +1；超出预算返回 false
fn within_budget(limit: i64) -> R<bool> {
    let key = format!("budget:{}", time::now() / 86_400_000);
    let n = match redis::call(&redis::Command {
        name: "INCR".into(),
        args: vec![key.as_bytes().to_vec()],
    })? {
        redis::RedisReply::Int(n) => n,
        _ => 0,
    };
    if n == 1 {
        redis::call(&redis::Command {
            name: "EXPIRE".into(),
            args: vec![key.as_bytes().to_vec(), b"172800".to_vec()],
        })?;
    }
    Ok(n <= limit)
}

/// 分类：本地规则命中阈值时不调用模型；返回 (分数, 是否用了模型)
fn classify(plain: &str, context: &str, author: Option<i64>, idem: &str, s: &Value) -> R<(Vec<Score>, bool)> {
    let mut scores = local::rules(plain, account_days(author)?, &spam_domains(s));
    let threshold = f(s, "report_threshold", 0.8);
    if local::max(&scores).is_some_and(|m| m.score >= threshold)
        || s["model_enabled"].as_bool() == Some(false)
    {
        return Ok((scores, false));
    }
    if !within_budget(s["daily_model_budget"].as_i64().unwrap_or(2000))? {
        return Ok((scores, false));
    }
    let mut system = local::SYSTEM_PROMPT.to_owned();
    if let Some(extra) = s["extra_guidelines"].as_str().filter(|x| !x.is_empty()) {
        system.push_str("\n站点补充说明：");
        system.extend(extra.chars().take(2000));
    }
    let req = json!({
        "profile": s["profile"].as_str().unwrap_or("cheap"), "system": system,
        "messages": [{ "role": "user", "parts": [
            { "type": "text", "text": format!("内容位置：{context}\n待审核的文本：") },
            { "type": "untrusted", "text": plain.chars().take(4000).collect::<String>() },
        ] }],
        "tools": [], "tool_mode": "none", "output_schema": local::output_schema(), "max_output_tokens": 600, "effort": "low",
        "idempotency_key": idem, "cache_prefix_messages": 0,
    });
    match svc("ctx.llm", "complete", &req) {
        Ok(v) if v["stop"] == "refusal" => scores.push(Score {
            category: "other".into(),
            score: 0.5,
            rationale: "model_refused".into(),
        }),
        Ok(v) => scores.extend(local::from_model(&v["structured"])),
        // 未配置 / 配额用尽 / 结构化无效：只用本地规则
        Err(e)
            if e.code.ends_with(".not_configured")
                || e.code.ends_with(".quota_exceeded")
                || e.code.ends_with(".structured_invalid")
                || e.code == "kernel.service_unavailable" =>
        {
            return Ok((scores, false));
        }
        Err(e) => return Err(e),
    }
    Ok((scores, true))
}

/// 分类一条评论并执行动作（LLD §1.4）
fn classify_comment(id: i64) -> R<()> {
    let s = settings();
    let c = svc("ctx.comments", "get", &json!({ "id": id }))?;
    if c.is_null() || c["state"] != "visible" {
        return Ok(());
    }
    let target = format!("comment:{id}");
    let plain = c["plain"].as_str().unwrap_or("");
    let context = format!("评论（{}）", c["target"].as_str().unwrap_or(""));
    let (scores, model_used) = classify(
        plain,
        &context,
        c["author_id"].as_i64(),
        &format!("aimod:comment:{id}"),
        &s,
    )?;
    let Some(top) = local::max(&scores).cloned() else {
        return Ok(());
    };
    let report_t = s["category_thresholds"][&top.category]
        .as_f64()
        .unwrap_or_else(|| f(&s, "report_threshold", 0.8));
    let review_t = f(&s, "review_threshold", 0.5);
    if top.score < review_t {
        return Ok(());
    }
    let mut case_id: Option<i64> = None;
    let state = if top.score >= report_t {
        let r = svc(
            "ctx.reports",
            "create",
            &json!({ "target": target, "reason_code": local::reason(&top.category),
                     "note": format!("AI 判定：{} {:.2}；{}", top.category, top.score, top.rationale) }),
        );
        match r {
            Ok(v) => case_id = v["case_id"].as_i64(),
            // 已经举报过（同一目标的系统举报）
            Err(e) if e.code == "reports.already_reported" => {}
            Err(e) => return Err(e),
        }
        "reported"
    } else {
        "review"
    };
    sql::execute(
        "INSERT INTO flags (target, categories, max_category, max_score, source, state, report_case_id) VALUES ($1, $2::jsonb, $3, $4, $5, $6, $7)
         ON CONFLICT (target) DO UPDATE SET categories = EXCLUDED.categories, max_category = EXCLUDED.max_category, max_score = EXCLUDED.max_score,
             source = EXCLUDED.source, state = EXCLUDED.state, report_case_id = coalesce(EXCLUDED.report_case_id, flags.report_case_id)",
        &[
            SqlValue::Text(target.clone()),
            SqlValue::Text(local::scores_json(&scores).to_string()),
            SqlValue::Text(top.category.clone()),
            SqlValue::Float(top.score),
            SqlValue::Text(if model_used { "model" } else { "local" }.into()),
            SqlValue::Text(state.into()),
            case_id.map_or(SqlValue::Null, SqlValue::Int),
        ],
    )?;
    if state == "reported" {
        events::publish(&events::NewEvent {
            event_type: "ai.content_flagged".into(),
            version: 1,
            key: target.clone(),
            payload: json!({ "target": target, "categories": scores.iter().map(|x| json!({ "category": x.category, "score": x.score })).collect::<Vec<_>>(),
                             "score": top.score, "report_case_id": case_id })
            .to_string(),
        })?;
    }
    Ok(())
}

impl on_event::Guest for AiModeration {
    fn handle(evt: on_event::EventEnvelope, _d: on_event::DeliveryInfo) -> R<()> {
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        match evt.event_type.as_str() {
            "comment.created" => {
                let s = settings();
                if s["enabled"].as_bool() == Some(false) {
                    return Ok(());
                }
                let id = p["comment_id"].as_i64().unwrap_or(0);
                // 新用户必审，其他按抽样率（按 ID 散列，结果稳定）
                let new_user =
                    account_days(p["author_id"].as_i64())? < s["new_user_days"].as_i64().unwrap_or(7);
                let rate = f(&s, "sample_rate", 0.2).clamp(0.0, 1.0);
                let sampled = (id.wrapping_mul(2_654_435_761) & 0x3ff) < (rate * 1024.0) as i64;
                if new_user || sampled {
                    jobs::schedule_once(
                        "classify_item",
                        time::now(),
                        &json!({ "kind": "comment", "id": id }).to_string(),
                        Some(&format!("comment:{id}")),
                    )?;
                }
            }
            "report.resolved" => {
                if let Some(case) = p["case_id"].as_i64() {
                    let outcome = match p["resolution"].as_str() {
                        Some("resolved") => "true_positive",
                        Some("invalid") => "false_positive",
                        _ => return Ok(()),
                    };
                    sql::execute(
                        "UPDATE flags SET outcome = $2 WHERE report_case_id = $1",
                        &[SqlValue::Int(case), SqlValue::Text(outcome.into())],
                    )?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl on_cron::Guest for AiModeration {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        match job.name.as_str() {
            "classify_item" => {
                let p: Value = serde_json::from_str(&job.payload).unwrap_or(Value::Null);
                if p["kind"] == "comment" {
                    classify_comment(p["id"].as_i64().unwrap_or(0))?;
                }
                Ok(())
            }
            "cleanup" => {
                sql::execute(
                    "DELETE FROM flags WHERE created_at < now() - interval '180 days'",
                    &[],
                )?;
                Ok(())
            }
            _ => Err(err("kernel.not_found", "unknown job")),
        }
    }
}

/// 发表前（只用本地规则，不调用模型；默认关闭）
impl hooks::Guest for AiModeration {
    fn handle(c: hooks::HookCall) -> R<hooks::HookDecision> {
        let s = settings();
        if c.hook != "comment.before_create" || s["hook_enabled"].as_bool() != Some(true) {
            return Ok(hooks::HookDecision::Allow);
        }
        let input: Value = serde_json::from_str(&c.input).unwrap_or(Value::Null);
        let scores = local::rules(
            input["body"].as_str().unwrap_or(""),
            account_days(input["author_id"].as_i64())?,
            &spam_domains(&s),
        );
        if local::max(&scores).is_some_and(|m| m.category == "spam" && m.score >= 0.95) {
            return Ok(hooks::HookDecision::Deny(hooks::DenyInfo {
                code: "ai-moderation.spam_detected".into(),
                message: "this looks like spam".into(),
                details: None,
            }));
        }
        Ok(hooks::HookDecision::Allow)
    }
}

impl service_provider::Guest for AiModeration {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        if method != "classify" {
            return Err(err(
                "kernel.not_found",
                &format!("ctx.ai_moderation has no method `{method}`"),
            ));
        }
        if !pt_plugin_sdk::invocation::current()
            .caller_scopes
            .iter()
            .any(|s| s == "use")
        {
            return Err(err("kernel.forbidden", "scope use required"));
        }
        let t = a["text"].as_str().unwrap_or("");
        let idem = format!("aimod:svc:{}", time::now());
        let (scores, model_used) =
            classify(t, a["context"].as_str().unwrap_or(""), None, &idem, &settings())?;
        let top = local::max(&scores).cloned();
        Ok(json!({ "categories": local::scores_json(&scores), "max_category": top.as_ref().map(|t| t.category.clone()), "max_score": top.map_or(0.0, |t| t.score),
                   "model_used": model_used })
        .to_string())
    }
}

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

impl http_handler::Guest for AiModeration {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        match req.route_id.as_str() {
            "flags" => {
                let state = req
                    .query
                    .iter()
                    .find(|(k, _)| k == "state")
                    .map(|(_, v)| v.clone());
                let r = sql::query(
                    "SELECT id, target, categories::text, max_category, max_score, source, state, report_case_id, outcome, (extract(epoch FROM created_at) * 1000)::bigint
                       FROM flags WHERE ($1::text IS NULL OR state = $1) ORDER BY id DESC LIMIT 100",
                    &[state.map_or(SqlValue::Null, SqlValue::Text)],
                )?;
                let items: Vec<Value> = r
                    .rows
                    .iter()
                    .map(|row| {
                        json!({ "id": int(&row[0]), "target": text(&row[1]), "categories": text(&row[2]).and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                                "max_category": text(&row[3]), "max_score": match &row[4] { SqlValue::Float(x) => json!(x), _ => Value::Null },
                                "source": text(&row[5]), "state": text(&row[6]), "report_case_id": int(&row[7]), "outcome": text(&row[8]), "created_at": int(&row[9]) })
                    })
                    .collect();
                Ok(resp(200, &json!({ "items": items })))
            }
            "stats" => {
                let r = sql::query(
                    "SELECT max_category, count(*), count(*) FILTER (WHERE outcome = 'true_positive'), count(*) FILTER (WHERE outcome = 'false_positive')
                       FROM flags WHERE state = 'reported' GROUP BY 1 ORDER BY 1",
                    &[],
                )?;
                let items: Vec<Value> = r
                    .rows
                    .iter()
                    .map(|row| {
                        let (tp, fp) = (int(&row[2]).unwrap_or(0), int(&row[3]).unwrap_or(0));
                        json!({ "category": text(&row[0]), "reported": int(&row[1]), "true_positive": tp, "false_positive": fp,
                                "precision": if tp + fp > 0 { json!(tp as f64 / (tp + fp) as f64) } else { Value::Null } })
                    })
                    .collect();
                Ok(resp(200, &json!({ "items": items })))
            }
            _ => Err(err("kernel.not_found", "no such route")),
        }
    }
}

export!(AiModeration);
