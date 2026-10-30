//! anticheat 插件（WASM，docs/plugins/anticheat/）。
//!
//! accounting 的基础信号与其他插件上报的证据 → 数据驱动的规则集评分 → 风险分段与案件 → 人工复核（认领、决定、四眼批准）
//! 与自动处置（仅结论性规则 + 可逆措施，带熔断）→ 处置以期望状态落地到 identity / accounting / ledger → 申诉与回滚、
//! 规则统计与回测模拟、AI 解释。
//!
//! 与设计的差异：后台设置为扁平键；申诉令牌密钥未配置时使用插件 kv 中生成的随机密钥。
//! 设计之外：多账号关联（`links`，同网段证据汇总的账号图与复核结论）。

pt_plugin_sdk::world!("anticheat", http);

pub mod api;
pub mod appeals;
pub mod core;
pub mod db;
pub mod decisions;
pub mod effects;
pub mod jobs;
pub mod links;
pub mod rules;

use exports::pt::plugin::{hooks, http_handler, mcp_tool, on_cron, on_event, service_provider};
use pt_plugin_sdk::{
    R, err,
    serde_json::{Value, json},
};

struct Anticheat;

impl hooks::Guest for Anticheat {
    fn handle(c: hooks::HookCall) -> R<hooks::HookDecision> {
        let input: Value = serde_json::from_str(&c.input).unwrap_or(Value::Null);
        Ok(match api::hook(&c.hook, &input)? {
            api::Hook::Allow => hooks::HookDecision::Allow,
            api::Hook::Deny(code, message) => hooks::HookDecision::Deny(hooks::DenyInfo {
                code: code.into(),
                message: message.into(),
                details: None,
            }),
        })
    }
}

impl on_event::Guest for Anticheat {
    fn handle(evt: on_event::EventEnvelope, d: on_event::DeliveryInfo) -> R<()> {
        let p: Value = serde_json::from_str(&evt.payload).unwrap_or(Value::Null);
        match d.subscription.as_str() {
            "on_signal" => jobs::on_signal(&p),
            "on_restriction_changed" => effects::on_restriction_changed(&p),
            "on_user_deleted" => jobs::on_user_deleted(&p, &evt.id),
            _ => Ok(()),
        }
    }
}

impl on_cron::Guest for Anticheat {
    fn run(job: on_cron::JobInvocation) -> R<()> {
        let p: Value = serde_json::from_str(&job.payload).unwrap_or(json!({}));
        let id = |k: &str| p[k].as_i64().ok_or_else(|| err("kernel.validation_failed", k));
        match job.name.as_str() {
            // cron
            "rescore_decay" => jobs::rescore_decay(job.scheduled_at),
            "daily_upload_spike" => jobs::daily_spike(),
            "sanction_sweep" => effects::sweep().map(|_| ()),
            "effect_reconcile" => effects::reconcile(),
            "queue_maintenance" => jobs::queue_maintenance(),
            "rule_stats" => jobs::rule_stats(job.scheduled_at),
            "retention" => jobs::retention(),
            // 延迟任务
            "run_effects" => effects::run(id("user_id")?),
            "sanction_expire" => effects::expire(id("sanction_id")?),
            "rescore_batch" => jobs::rescore_batch(&p),
            "simulate_batch" => jobs::simulate_batch(&p),
            "enrich_case" => jobs::enrich_case(id("case_id")?),
            "explain_case" => jobs::explain(id("case_id")?, p["requested_by"].as_i64()).map(|_| ()),
            "purge_user_step" => jobs::purge_user_step(&p),
            other => Err(err("kernel.not_found", &format!("unknown job `{other}`"))),
        }
    }
}

impl http_handler::Guest for Anticheat {
    fn handle(req: http_handler::Request) -> R<http_handler::Response> {
        Ok(api::http(&req)?.into())
    }
}

impl service_provider::Guest for Anticheat {
    fn call(_service: String, method: String, args: String) -> R<String> {
        let a: Value = serde_json::from_str(&args).unwrap_or(Value::Null);
        Ok(api::service(&method, &a)?.to_string())
    }
}

impl mcp_tool::Guest for Anticheat {
    fn call(tool: String, arguments: String) -> R<String> {
        let a: Value = serde_json::from_str(&arguments).unwrap_or(Value::Null);
        Ok(api::mcp(&tool, &a)?.to_string())
    }
}

export!(Anticheat);
