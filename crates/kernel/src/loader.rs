//! 插件发现、清单校验、依赖解析与拓扑排序（kernel/03 §2.1～2.3）。

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use pt_plugin_api::{
    API_VERSIONS, NativePlugin,
    manifest::{Manifest, PluginKind, ServiceRef},
};
use sha2::{Digest, Sha256};

use crate::config::SiteConfig;

/// 内核服务（kernel/03 §3 与 catalog/services.md §1），不需要在 `requires` 中声明。
pub const KERNEL_SERVICES: &[&str] = &[
    "ctx.invocation",
    "ctx.log",
    "ctx.config",
    "ctx.kv",
    "ctx.sql",
    "ctx.redis",
    "ctx.events",
    "ctx.http",
    "ctx.blob",
    "ctx.time",
    "ctx.random",
    "ctx.audit",
    "ctx.jobs",
    "ctx.metrics",
    "ctx.hooks",
];

/// 多实现服务（catalog/services.md 的 `multi` 行）。
pub const MULTI_SERVICES: &[&str] = &[];

/// 内核隐式依赖的服务：提供方先激活（§2.3 第 4 步）。
pub const KERNEL_DEPENDS_ON: &[&str] = &["ctx.auth", "ctx.rbac", "ctx.terms"];

pub struct PluginSpec {
    pub manifest: Manifest,
    pub manifest_sha256: String,
    pub native: Option<NativePlugin>,
}

impl std::fmt::Debug for PluginSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginSpec")
            .field("id", &self.manifest.id)
            .field("version", &self.manifest.version)
            .finish()
    }
}

impl PluginSpec {
    pub fn native(np: NativePlugin) -> Result<Self, LoadError> {
        let manifest =
            Manifest::parse(np.manifest).map_err(|e| LoadError::new("E-LOAD-MANIFEST", "?", e.message))?;
        if manifest.kind != PluginKind::Native {
            return Err(LoadError::new(
                "E-LOAD-KIND",
                &manifest.id,
                "compiled-in plugin must declare kind = \"native\"",
            ));
        }
        let manifest_sha256 = hex::encode(Sha256::digest(np.manifest.as_bytes()));
        Ok(Self {
            manifest,
            manifest_sha256,
            native: Some(np),
        })
    }

    pub fn id(&self) -> &str {
        &self.manifest.id
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    pub code: &'static str,
    pub plugin: String,
    pub message: String,
}

impl LoadError {
    pub fn new(code: &'static str, plugin: &str, message: impl Into<String>) -> Self {
        Self {
            code,
            plugin: plugin.to_owned(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "error[{}] plugin `{}`: {}",
            self.code, self.plugin, self.message
        )
    }
}

/// 依赖解析结果。
#[derive(Debug, Clone, Default)]
pub struct Resolution {
    /// 激活顺序（提供方在前）
    pub order: Vec<String>,
    /// 服务 → 提供方（single 为 1 个）
    pub providers: BTreeMap<String, Vec<String>>,
    /// 服务 → 版本
    pub versions: BTreeMap<String, semver::Version>,
    /// 插件 → 可调用的服务
    pub allowed: HashMap<String, HashSet<String>>,
}

/// 校验并解析启用的插件集合。收集全部错误后统一返回（§2.6）。
pub fn resolve(specs: &[PluginSpec], cfg: &SiteConfig) -> Result<Resolution, Vec<LoadError>> {
    let mut errors = Vec::new();
    let mut seen = HashSet::new();
    for s in specs {
        let m = &s.manifest;
        if !seen.insert(m.id.clone()) {
            errors.push(LoadError::new(
                "E-LOAD-DUPLICATE",
                &m.id,
                "plugin id appears more than once",
            ));
        }
        if !API_VERSIONS.contains(&m.api.as_str()) {
            errors.push(LoadError::new(
                "E-LOAD-API",
                &m.id,
                format!("unsupported api `{}`", m.api),
            ));
        }
        if let Some(req) = &m.kernel
            && !req.matches(&kernel_version())
        {
            errors.push(LoadError::new(
                "E-LOAD-KERNEL",
                &m.id,
                format!("requires kernel {req}"),
            ));
        }
    }

    // 1. 服务提供方
    let mut candidates: BTreeMap<String, Vec<(String, semver::Version)>> = BTreeMap::new();
    for s in specs {
        for p in &s.manifest.provides {
            match ServiceRef::parse(p).and_then(|r| Ok((r.provided_version()?, r))) {
                Ok((v, r)) => {
                    if KERNEL_SERVICES.contains(&r.name.as_str()) {
                        errors.push(LoadError::new(
                            "E-DEP-SELECT",
                            s.id(),
                            format!("{} is a kernel service", r.name),
                        ));
                    }
                    candidates.entry(r.name).or_default().push((s.id().to_owned(), v));
                }
                Err(e) => errors.push(LoadError::new("E-LOAD-MANIFEST", s.id(), e.message)),
            }
        }
    }
    let mut res = Resolution::default();
    for (svc, cands) in &candidates {
        let chosen: Vec<(String, semver::Version)> = if MULTI_SERVICES.contains(&svc.as_str()) {
            cands.clone()
        } else if let Some(sel) = cfg.services.get(svc) {
            match cands.iter().find(|(id, _)| id == sel) {
                Some(c) => vec![c.clone()],
                None => {
                    errors.push(LoadError::new(
                        "E-DEP-SELECT",
                        sel,
                        format!(
                            "site.toml selects `{sel}` for {svc} but it does not provide it (or is disabled)"
                        ),
                    ));
                    continue;
                }
            }
        } else if cands.len() == 1 {
            cands.clone()
        } else {
            let ids: Vec<_> = cands.iter().map(|c| c.0.as_str()).collect();
            errors.push(LoadError::new(
                "E-DEP-AMBIGUOUS",
                &ids.join(","),
                format!("{svc} has several providers; choose one in [services]"),
            ));
            continue;
        };
        if let Some((_, v)) = chosen.first() {
            res.versions.insert(svc.clone(), v.clone());
        }
        res.providers
            .insert(svc.clone(), chosen.into_iter().map(|c| c.0).collect());
    }

    // 2. 依赖边
    let ids: BTreeSet<String> = specs.iter().map(|s| s.id().to_owned()).collect();
    let mut edges: BTreeMap<String, BTreeSet<String>> =
        ids.iter().map(|i| (i.clone(), BTreeSet::new())).collect();
    for s in specs {
        let id = s.id().to_owned();
        let allowed = res.allowed.entry(id.clone()).or_default();
        for k in KERNEL_SERVICES {
            allowed.insert((*k).to_owned());
        }
        let reqs = s
            .manifest
            .requires
            .iter()
            .map(|r| (r, true))
            .chain(s.manifest.optional_requires.iter().map(|r| (r, false)));
        for (r, mandatory) in reqs {
            let r = match ServiceRef::parse(r) {
                Ok(r) => r,
                Err(e) => {
                    errors.push(LoadError::new("E-LOAD-MANIFEST", &id, e.message));
                    continue;
                }
            };
            if KERNEL_SERVICES.contains(&r.name.as_str()) {
                continue;
            }
            let Some(provs) = res.providers.get(&r.name) else {
                if mandatory && !MULTI_SERVICES.contains(&r.name.as_str()) {
                    errors.push(LoadError::new(
                        "E-DEP-MISSING",
                        &id,
                        format!("requires `{}` but no enabled plugin provides it", r.name),
                    ));
                }
                continue;
            };
            let want = match r.requirement() {
                Ok(w) => w,
                Err(e) => {
                    errors.push(LoadError::new("E-LOAD-MANIFEST", &id, e.message));
                    continue;
                }
            };
            if let Some(v) = res.versions.get(&r.name)
                && !want.matches(v)
            {
                errors.push(LoadError::new(
                    "E-LOAD-REQUIRES",
                    &id,
                    format!("requires {} {want} but provider has {v}", r.name),
                ));
            }
            allowed.insert(r.name.clone());
            for p in provs {
                if p != &id {
                    edges.entry(id.clone()).or_default().insert(p.clone());
                }
            }
        }
    }
    // 4. 内核隐式依赖：auth / rbac / terms 的提供方及其依赖排在最前（以优先级实现）
    let priority: HashSet<String> = KERNEL_DEPENDS_ON
        .iter()
        .filter_map(|s| res.providers.get(*s))
        .flatten()
        .cloned()
        .collect();

    match topo_sort(&edges, &priority) {
        Ok(order) => res.order = order,
        Err(cycle) => errors.push(LoadError::new("E-DEP-CYCLE", &cycle[0], cycle.join(" → "))),
    }
    if errors.is_empty() { Ok(res) } else { Err(errors) }
}

pub fn kernel_version() -> semver::Version {
    semver::Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| semver::Version::new(0, 1, 0))
}

/// Kahn 算法；每一层先取优先集合，其余按 ID 字典序（各副本顺序一致）。
/// `edges[a]` = a 依赖的插件集合。成功返回提供方在前的顺序；有环返回一个环。
pub fn topo_sort(
    edges: &BTreeMap<String, BTreeSet<String>>,
    priority: &HashSet<String>,
) -> Result<Vec<String>, Vec<String>> {
    let mut remaining: BTreeMap<&str, usize> = edges.iter().map(|(k, v)| (k.as_str(), v.len())).collect();
    let mut dependents: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (k, deps) in edges {
        for d in deps {
            dependents.entry(d.as_str()).or_default().push(k.as_str());
        }
    }
    let mut order = Vec::with_capacity(edges.len());
    loop {
        let ready: Vec<&str> = remaining
            .iter()
            .filter(|(_, n)| **n == 0)
            .map(|(k, _)| *k)
            .collect();
        if ready.is_empty() {
            break;
        }
        // 优先集合（及其依赖）可用时先激活
        let pick: Vec<&str> = {
            let pri: Vec<&str> = ready.iter().copied().filter(|r| priority.contains(*r)).collect();
            if pri.is_empty() { ready } else { pri }
        };
        for r in pick {
            remaining.remove(r);
            order.push(r.to_owned());
            for dep in dependents.get(r).into_iter().flatten() {
                if let Some(n) = remaining.get_mut(dep) {
                    *n -= 1;
                }
            }
        }
    }
    if remaining.is_empty() {
        return Ok(order);
    }
    // 找环：从任一剩余节点沿未完成的依赖走，直到重复
    let start = remaining.keys().next().copied().unwrap_or_default();
    let mut path = vec![start.to_owned()];
    let mut cur = start;
    loop {
        let next = edges[cur].iter().find(|d| remaining.contains_key(d.as_str()));
        let Some(next) = next else { return Err(path) };
        if let Some(pos) = path.iter().position(|p| p == next) {
            let mut cycle = path[pos..].to_vec();
            cycle.push(next.clone());
            return Err(cycle);
        }
        path.push(next.clone());
        cur = next.as_str();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(pairs: &[(&str, &[&str])]) -> BTreeMap<String, BTreeSet<String>> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.iter().map(|s| (*s).to_owned()).collect()))
            .collect()
    }

    #[test]
    fn topo_orders_providers_first() {
        let e = g(&[
            ("shop", &["bonus", "identity"]),
            ("bonus", &["identity"]),
            ("identity", &[]),
            ("anime", &[]),
        ]);
        let o = topo_sort(&e, &HashSet::new()).unwrap_or_default();
        assert_eq!(o, vec!["anime", "identity", "bonus", "shop"]);
        let pri: HashSet<String> = ["identity".to_owned()].into();
        let o = topo_sort(&e, &pri).unwrap_or_default();
        assert_eq!(o[0], "identity");
    }

    #[test]
    fn topo_reports_cycle() {
        let e = g(&[("a", &["b"]), ("b", &["c"]), ("c", &["a"]), ("d", &[])]);
        let c = topo_sort(&e, &HashSet::new()).err().unwrap_or_default();
        assert_eq!(c.first(), c.last());
        assert_eq!(c.len(), 4);
    }
}
