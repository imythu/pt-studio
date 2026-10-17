//! 运行时设置（identity 03 §4.2 子集）：后台 schema 表单写入 `kernel.plugin_settings`，缺省值在此补齐。

use pt_plugin_api::Ctx;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct Policy {
    /// 同一用户名连续失败多少次后软锁（已知设备不受影响）
    pub user_lock_after: i64,
    pub user_window_secs: i64,
    /// 同一 IP 失败多少次后封锁该 IP 的登录
    pub ip_block_after: i64,
    pub ip_window_secs: i64,
    /// 用户名维度从第 3 次失败起指数延迟的上限
    pub max_delay_secs: i64,
    /// 新设备登录时发安全通知
    pub login_alert: bool,
    /// IP 记录超过多少天后截断到 /24（IPv6 /48）
    pub ip_truncate_after_days: i64,
    /// IP 记录与安全事件保留天数
    pub ip_delete_after_days: i64,
    /// 用户在自己的安全页能否看到完整 IP（否则只显示截断形式）
    pub ip_show_full_to_owner: bool,
    /// 注册后需验证邮箱（之前为待验证状态，只有访客权限）；需要已配置邮件渠道
    pub require_email_verification: bool,
    /// 该 rank 及以上的内置角色必须两步验证（会话未通过时不享有这些角色）；None = 不强制
    pub two_factor_required_rank: Option<i16>,
}

fn int(s: &Value, k: &str, d: i64, lo: i64, hi: i64) -> i64 {
    s.get(k).and_then(Value::as_i64).unwrap_or(d).clamp(lo, hi)
}

fn flag(s: &Value, k: &str, d: bool) -> bool {
    s.get(k).and_then(Value::as_bool).unwrap_or(d)
}

impl Policy {
    pub fn from_settings(s: &Value) -> Self {
        let truncate = int(s, "ip_truncate_after_days", 90, 1, 3650);
        Self {
            user_lock_after: int(s, "login_user_lock_after", 10, 3, 100),
            user_window_secs: int(s, "login_user_window_secs", 900, 60, 86_400),
            ip_block_after: int(s, "login_ip_block_after", 50, 5, 10_000),
            ip_window_secs: int(s, "login_ip_window_secs", 3600, 60, 86_400),
            max_delay_secs: int(s, "login_max_delay_secs", 30, 0, 300),
            login_alert: flag(s, "login_alert", true),
            ip_truncate_after_days: truncate,
            ip_delete_after_days: int(s, "ip_delete_after_days", 365, truncate, 3650),
            ip_show_full_to_owner: flag(s, "ip_show_full_to_owner", false),
            require_email_verification: flag(s, "require_email_verification", false),
            two_factor_required_rank: s
                .get("two_factor_required_role")
                .and_then(Value::as_str)
                .and_then(|r| crate::rbac::BUILTIN.iter().find(|(n, _)| *n == r))
                .map(|(_, k)| *k)
                .filter(|k| *k >= 2),
        }
    }

    pub async fn load(ctx: &Ctx) -> Self {
        Self::from_settings(&ctx.config.get_settings().await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_clamping() {
        let p = Policy::from_settings(&serde_json::json!({}));
        assert_eq!((p.user_lock_after, p.ip_block_after, p.max_delay_secs), (10, 50, 30));
        let p = Policy::from_settings(&serde_json::json!({ "login_user_lock_after": 1, "ip_truncate_after_days": 400, "ip_delete_after_days": 30 }));
        assert_eq!(p.user_lock_after, 3);
        assert_eq!(p.ip_delete_after_days, 400, "deletion cannot precede truncation");
    }
}
