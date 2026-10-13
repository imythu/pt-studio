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

fn leak(s: &str) -> &'static str {
    Box::leak(s.to_owned().into_boxed_str())
}

/// 清单中声明、目录未登记的钩子点（第三方插件，名字以 `<插件 ID>.` 开头）。启动时每个钩子点构造一次。
pub fn manifest_hook_point(owner: &str, hp: &pt_plugin_api::manifest::HookPoint) -> HookPointDef {
    let modifiable: Vec<&'static str> = hp.modifiable.iter().map(|m| leak(m)).collect();
    HookPointDef {
        name: leak(&hp.name),
        owner: leak(owner),
        allow_modify: !modifiable.is_empty(),
        modifiable: Box::leak(modifiable.into_boxed_slice()),
        append_only: false,
        timeout_ms: hp.timeout_ms.unwrap_or(200).clamp(1, 1000),
        deny_on_error: hp.deny_on_error,
        locked: false,
    }
}

/// 目录 + 清单声明的钩子点
#[derive(Debug, Default, Clone)]
pub struct HookRegistry(std::collections::HashMap<String, HookPointDef>);

impl HookRegistry {
    pub fn build<'a>(manifests: impl IntoIterator<Item = &'a pt_plugin_api::manifest::Manifest>) -> Self {
        let mut m = std::collections::HashMap::new();
        for man in manifests {
            for hp in &man.hook_points {
                if hook_point(&hp.name).is_none() && hp.name.starts_with(&format!("{}.", man.id)) {
                    m.insert(hp.name.clone(), manifest_hook_point(&man.id, hp));
                }
            }
        }
        Self(m)
    }

    pub fn get(&self, name: &str) -> Option<HookPointDef> {
        hook_point(name).copied().or_else(|| self.0.get(name).copied())
    }
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
