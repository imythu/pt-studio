//! API 冻结检查：`wit-frozen/pt-plugin-1.0.0.wit` 是 1.0.0 发布时的快照。当前 WIT 中，快照里的每个接口必须原样保留
//! （忽略注释与空白），每个世界只能增加条目。新功能放进新接口（并提升次版本），不要修改已有接口。

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::BTreeMap;

type Blocks = BTreeMap<(String, String), Vec<String>>;

/// 去掉注释与空白差异后的顶层块：(种类, 名字) → 行列表
fn blocks(src: &str) -> Blocks {
    let lines: Vec<String> = src
        .lines()
        .map(|l| {
            l.split("//")
                .next()
                .unwrap_or("")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|l| !l.is_empty())
        .collect();
    let mut out = BTreeMap::new();
    let mut i = 0;
    while i < lines.len() {
        let head: Vec<&str> = lines[i].split(' ').collect();
        if (head[0] == "interface" || head[0] == "world") && lines[i].ends_with('{') {
            let key = (head[0].to_owned(), head[1].to_owned());
            let mut depth = 0usize;
            let mut body = Vec::new();
            loop {
                let cur = &lines[i];
                depth += cur.matches('{').count();
                depth -= cur.matches('}').count().min(depth);
                body.push(cur.clone());
                i += 1;
                if depth == 0 || i >= lines.len() {
                    break;
                }
            }
            out.insert(key, body);
        } else {
            i += 1;
        }
    }
    out
}

#[test]
fn frozen_api_is_preserved() {
    let frozen = blocks(include_str!("../wit-frozen/pt-plugin-1.0.0.wit"));
    let current = blocks(include_str!("../wit/plugin.wit"));
    assert!(frozen.len() > 20, "snapshot parsed: {}", frozen.len());
    for (key, body) in &frozen {
        let cur = current
            .get(key)
            .unwrap_or_else(|| panic!("{} `{}` was removed from the frozen 1.0 API", key.0, key.1));
        if key.0 == "interface" {
            assert_eq!(
                cur, body,
                "interface `{}` changed; frozen interfaces must not change",
                key.1
            );
        } else {
            for line in body {
                assert!(cur.contains(line), "world `{}` lost `{line}`", key.1);
            }
        }
    }
}

#[test]
fn detects_changes() {
    let base = "interface a {\n  f: func() -> u8;\n}\nworld w {\n  import a;\n}\n";
    let changed = blocks("interface a {\n  f: func() -> u16;\n}\n");
    assert_ne!(
        blocks(base)[&("interface".into(), "a".into())],
        changed[&("interface".into(), "a".into())]
    );
    // 注释与缩进不影响
    let same = blocks("// x\ninterface a {\n    f: func() -> u8; // y\n}\n");
    assert_eq!(
        blocks(base)[&("interface".into(), "a".into())],
        same[&("interface".into(), "a".into())]
    );
}

#[test]
fn package_version_matches_crate() {
    let src = include_str!("../wit/plugin.wit");
    let (major, minor) = pt_plugin_api::API_VERSION;
    assert!(
        src.contains(&format!("package pt:plugin@{major}.{minor}.")),
        "WIT package version must match API_VERSION"
    );
}

#[test]
fn template_wit_copy_is_current() {
    // 模板要能复制到仓库外使用，所以带一份 WIT 副本（不是符号链接）；改 WIT 时同步更新
    assert_eq!(
        include_str!("../../../templates/wasm-plugin/wit/deps/pt-plugin/plugin.wit"),
        include_str!("../wit/plugin.wit"),
        "copy crates/plugin-api/wit/plugin.wit to templates/wasm-plugin/wit/deps/pt-plugin/"
    );
}
