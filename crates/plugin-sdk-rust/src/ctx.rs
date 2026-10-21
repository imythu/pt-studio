//! 调用上下文与常用宿主操作的便捷封装。

use serde_json::Value;

use crate::{PluginError, R, config, err, events, hook_runner, invocation, jobs, services, types};

/// 本次调用的上下文
pub fn current() -> types::InvocationInfo {
    invocation::current()
}

/// 当前登录用户；匿名时返回 `kernel.unauthenticated`
pub fn current_user() -> R<i64> {
    current_user_opt().ok_or_else(|| err("kernel.unauthenticated", "login required"))
}

/// 当前用户（匿名为 `None`）
pub fn current_user_opt() -> Option<i64> {
    invocation::current()
        .actor
        .user_id
        .and_then(|u| i64::try_from(u).ok())
}

/// 服务调用的调用方插件（其他入口为 `"kernel"`）
pub fn caller() -> String {
    invocation::current().caller_plugin
}

/// 调用方在本服务上是否获授该作用域
pub fn has_scope(scope: &str) -> bool {
    invocation::current().caller_scopes.iter().any(|s| s == scope)
}

/// 要求调用方具备作用域，否则返回 `kernel.forbidden`
pub fn require_scope(scope: &str) -> R<()> {
    if has_scope(scope) {
        Ok(())
    } else {
        Err(err("kernel.forbidden", &format!("scope `{scope}` required")))
    }
}

/// 后台可改的设置（JSON 对象；未设置的键不会自动填入 schema 默认值，读取时自行给默认值）
pub fn settings() -> Value {
    serde_json::from_str(&config::get_settings()).unwrap_or(Value::Null)
}

/// site.toml `[plugin.<id>]` 中的静态配置
pub fn static_config() -> Value {
    serde_json::from_str(&config::get_static()).unwrap_or(Value::Null)
}

/// 调用业务服务（JSON 分派），如 `ctx::call("ctx.ledger", "transfer", &json!({...}))`
pub fn call(service: &str, method: &str, args: &Value) -> R<Value> {
    let out = services::call(service, method, &args.to_string())?;
    serde_json::from_str(&out).map_err(|_| err("kernel.internal", "service returned invalid JSON"))
}

/// 币种最小单位的倍数（10^scale）：插件设置与界面里的金额按显示单位（如 18 魔力值），经 ledger 记账前乘以它，
/// 读回余额时除以它。ledger 未安装或币种不存在时为 1
pub fn currency_unit(code: &str) -> i64 {
    call("ctx.ledger", "currencies", &serde_json::json!({}))
        .ok()
        .and_then(|v| {
            v.as_array()?
                .iter()
                .find(|c| c["code"] == code)
                .and_then(|c| c["scale"].as_u64())
        })
        .and_then(|s| 10i64.checked_pow(u32::try_from(s).ok()?))
        .unwrap_or(1)
}

/// 发布事件（载荷版本 1）；事件类型须在清单 `publishes` 中声明
pub fn publish(event_type: &str, key: &str, payload: &Value) -> R<String> {
    publish_version(event_type, 1, key, payload)
}

pub fn publish_version(event_type: &str, version: u32, key: &str, payload: &Value) -> R<String> {
    events::publish(&events::NewEvent {
        event_type: event_type.into(),
        version,
        key: key.into(),
        payload: payload.to_string(),
    })
}

/// 运行本插件定义的钩子点：放行时返回合并补丁后的输入，被拒绝时返回拒绝方给出的错误
pub fn run_hook(hook: &str, input: &Value) -> R<Value> {
    match hook_runner::run(hook, &input.to_string())? {
        hook_runner::HookOutcome::Allowed(v) => {
            Ok(serde_json::from_str(&v).unwrap_or_else(|_| input.clone()))
        }
        hook_runner::HookOutcome::Denied(d) => Err(PluginError {
            code: d.code,
            message: d.message,
            details: Some(serde_json::json!({ "by_plugin": d.by_plugin }).to_string()),
            retryable: false,
        }),
    }
}

/// 安排一次性延迟任务（任务名须在清单 `[[jobs]]` 中声明）；到期后经 `on-cron.run` 回调
pub fn schedule(name: &str, run_at_ms: i64, payload: &Value, dedupe_key: Option<&str>) -> R<String> {
    jobs::schedule_once(name, run_at_ms, &payload.to_string(), dedupe_key)
}
