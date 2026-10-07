//! 内置 catalog（由 build.rs 从 docs/platform/catalog 生成）。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookPointDef {
    pub name: &'static str,
    pub owner: &'static str,
    pub allow_modify: bool,
    /// 可修改字段（点分路径，如 `attributes.inviter_id`）
    pub modifiable: &'static [&'static str],
    /// 数组字段只能追加（search.before_query 的 filters）
    pub append_only: bool,
    pub timeout_ms: u32,
    pub deny_on_error: bool,
    pub locked: bool,
}

include!(concat!(env!("OUT_DIR"), "/hooks_catalog.rs"));

pub fn hook_point(name: &str) -> Option<&'static HookPointDef> {
    HOOK_POINTS.iter().find(|h| h.name == name)
}

/// 错误码在目录中登记的 HTTP 状态。
pub fn error_status(code: &str) -> Option<u16> {
    ERROR_STATUS.iter().find(|(c, _)| *c == code).map(|(_, s)| *s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hooks_parsed_from_docs() {
        let reg = hook_point("user.before_register").copied();
        assert!(
            reg.is_some_and(|h| h.owner == "identity" && h.locked && h.deny_on_error && h.timeout_ms == 500)
        );
        assert!(reg.is_some_and(|h| h.modifiable.contains(&"attributes.invite_id")));
        let dl = hook_point("torrent.before_download").copied();
        assert!(dl.is_some_and(|h| !h.deny_on_error && !h.allow_modify));
        assert!(hook_point("search.before_query").is_some_and(|h| h.append_only));
        assert!(HOOK_POINTS.len() >= 15);
        assert_eq!(error_status("identity.invalid_credentials"), Some(401));
        assert_eq!(error_status("llm-openai-compat.upstream_error"), Some(502));
        assert_eq!(error_status("kernel.precondition_failed"), Some(412));
    }
}
