//! 剧集 / 动漫的集号：季集字段与范围（scope）规范化、从文件名解析集号、按范围检查文件列表（type-tv LLD §1.2）。
//! 纯函数，无正则依赖。

use serde_json::{Map, Value, json};

use crate::Errors;

/// 单集 / 多集 / 季包 / 全集
pub const SCOPES: &[&str] = &["season_pack", "episode", "episodes", "complete"];

fn int(v: &Value) -> Option<i64> {
    v.as_i64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

/// season / episode / episode_end（整数或数字字符串）与 ongoing 写入 `o`
pub fn normalize_episode_fields(input: &Value, o: &mut Map<String, Value>, errs: &mut Errors, plugin: &str) {
    let get = |k: &str| input.get(k).filter(|v| !v.is_null() && v.as_str() != Some(""));
    for (k, lo, hi) in [("season", 0, 999), ("episode", 1, 9999), ("episode_end", 2, 9999)] {
        if let Some(v) = get(k) {
            match int(v) {
                Some(n) if (lo..=hi).contains(&n) => {
                    o.insert(k.into(), json!(n));
                }
                _ => errs.add(
                    k,
                    &format!("{plugin}.field_invalid"),
                    &format!("must be {lo}..={hi}"),
                ),
            }
        }
    }
    if let Some(b) = get("ongoing").and_then(Value::as_bool) {
        o.insert("ongoing".into(), json!(b));
    }
}

/// 范围：未填时推断（在补全季集之后调用），并校验与季集字段的组合；全集的季记为 0，其余缺省为第 1 季
pub fn normalize_scope(input: &Value, o: &mut Map<String, Value>, errs: &mut Errors, plugin: &str) {
    let get = |k: &str| input.get(k).filter(|v| !v.is_null() && v.as_str() != Some(""));
    let code = format!("{plugin}.scope_invalid");
    // 范围：未填时推断（有集号 → 单集 / 多集；只有季 → 季包；都没有 → 全集）
    let scope = match get("scope").and_then(Value::as_str) {
        Some(s) if SCOPES.contains(&s) => s.to_owned(),
        Some(_) => {
            errs.add("scope", &code, "unknown scope");
            String::new()
        }
        None => match (
            o.contains_key("episode"),
            o.contains_key("episode_end"),
            o.contains_key("season"),
        ) {
            (true, true, _) => "episodes".into(),
            (true, false, _) => "episode".into(),
            (false, _, true) => "season_pack".into(),
            _ => "complete".into(),
        },
    };
    match scope.as_str() {
        "episode" | "episodes" if !o.contains_key("episode") => {
            errs.add("episode", &code, "episode is required")
        }
        "episodes"
            if o.get("episode_end").and_then(Value::as_i64) <= o.get("episode").and_then(Value::as_i64) =>
        {
            errs.add("episode_end", &code, "episode_end must be greater than episode");
        }
        "season_pack" | "complete" => {
            o.remove("episode");
            o.remove("episode_end");
        }
        _ => {}
    }
    if scope == "episode" {
        o.remove("episode_end");
    }
    if scope == "complete" {
        o.insert("season".into(), json!(0));
    } else {
        o.entry("season").or_insert(json!(1));
    }
    o.insert("scope".into(), json!(scope));
}

/// 季集显示：S01、S01E05、S01E05-E08、Complete
pub fn se(f: &Value) -> String {
    let s = f["season"].as_i64().unwrap_or(1);
    match (
        f["scope"].as_str(),
        f["episode"].as_i64(),
        f["episode_end"].as_i64(),
    ) {
        (Some("complete"), ..) => "Complete".into(),
        (_, Some(e), Some(e2)) => format!("S{s:02}E{e:02}-E{e2:02}"),
        (_, Some(e), None) => format!("S{s:02}E{e:02}"),
        _ => format!("S{s:02}"),
    }
}

pub const VIDEO_EXT: &[&str] = &[
    "mkv", "mp4", "avi", "ts", "m2ts", "wmv", "mov", "flv", "webm", "rmvb", "iso",
];

pub fn is_video(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, e)| VIDEO_EXT.contains(&e.to_ascii_lowercase().as_str()))
}

fn digits(s: &[u8], at: usize, max: usize) -> Option<(i64, usize)> {
    let n = s[at..]
        .iter()
        .take(max)
        .take_while(|c| c.is_ascii_digit())
        .count();
    if n == 0 || s.get(at + n).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(&s[at..at + n])
        .ok()?
        .parse()
        .ok()
        .map(|v| (v, at + n))
}

/// 文件名 → (季?, 集, 结束集?)。依次尝试 `SxxEyy[-Ezz]`、`Eyy`、`第yy集 / 话 / 話`、`[yy]` / `[yyv2]`、` - yy `
pub fn parse_episode(path: &str) -> Option<(Option<i64>, i64, Option<i64>)> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let b = stem.as_bytes();
    let lower: Vec<u8> = b.iter().map(u8::to_ascii_lowercase).collect();
    let boundary = |i: usize| i == 0 || !lower[i - 1].is_ascii_alphanumeric();
    // SxxEyy[-Ezz]
    for i in 0..lower.len() {
        if lower[i] == b's'
            && boundary(i)
            && let Some((s, j)) = digits(&lower, i + 1, 3)
            && lower.get(j) == Some(&b'e')
            && let Some((e, k)) = digits(&lower, j + 1, 4)
        {
            let mut end = None;
            let mut k2 = k;
            if lower.get(k2) == Some(&b'-') {
                k2 += 1;
            }
            if lower.get(k2) == Some(&b'e') {
                k2 += 1;
            }
            if k2 != k
                && let Some((e2, _)) = digits(&lower, k2, 4)
                && e2 > e
            {
                end = Some(e2);
            }
            return Some((Some(s), e, end));
        }
    }
    // Eyy（独立词）
    for i in 0..lower.len() {
        if lower[i] == b'e'
            && boundary(i)
            && let Some((e, j)) = digits(&lower, i + 1, 4)
            && lower.get(j).is_none_or(|c| !c.is_ascii_alphanumeric())
        {
            return Some((None, e, None));
        }
    }
    // 第yy集 / 话 / 話
    if let Some(p) = stem.find('第') {
        let rest = &stem[p + '第'.len_utf8()..];
        let n: String = rest.chars().take_while(char::is_ascii_digit).collect();
        let after = rest[n.len()..].chars().next();
        if !n.is_empty() && matches!(after, Some('集' | '话' | '話')) {
            return n.parse().ok().map(|e| (None, e, None));
        }
    }
    // [yy] / [yyv2]
    for (i, c) in b.iter().enumerate() {
        if *c == b'['
            && let Some((e, j)) = digits(b, i + 1, 4)
        {
            let j = if lower.get(j) == Some(&b'v') && lower.get(j + 1).is_some_and(u8::is_ascii_digit) {
                j + 2
            } else {
                j
            };
            if b.get(j) == Some(&b']') && e < 1900 {
                return Some((None, e, None));
            }
        }
    }
    // ` - yy`（动漫常见）
    if let Some(p) = stem.rfind(" - ") {
        let rest = &stem.as_bytes()[p + 3..];
        if let Some((e, j)) = digits(rest, 0, 4) {
            let j = if rest.get(j).is_some_and(|c| c.eq_ignore_ascii_case(&b'v')) {
                j + 2
            } else {
                j
            };
            if rest.get(j).is_none_or(|c| matches!(c, b' ' | b'[' | b'(')) {
                return Some((None, e, None));
            }
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScopeCheck<'a> {
    pub scope: &'a str,
    pub episode: Option<i64>,
    pub episode_end: Option<i64>,
    pub ongoing: bool,
    pub episode_count: Option<i64>,
    pub min_ratio: f64,
}

/// 文件列表（路径, 字节）与范围是否相符；不相符返回原因。样本（≤ 200 个）只检查集号不越界
pub fn check_scope(
    files: &[(String, u64)],
    c: &ScopeCheck<'_>,
    sample_truncated: bool,
) -> Result<(), String> {
    let vids: Vec<&(String, u64)> = files
        .iter()
        .filter(|(p, size)| {
            is_video(p) && *size >= 50 << 20 && !p.to_ascii_lowercase().split('/').any(|seg| seg == "sample")
        })
        .collect();
    let mut eps: Vec<i64> = Vec::new();
    for (p, _) in &vids {
        if let Some((_, e, end)) = parse_episode(p) {
            eps.extend(e..=end.unwrap_or(e));
        }
    }
    eps.sort_unstable();
    eps.dedup();
    match c.scope {
        "episode" => {
            let e = c.episode.ok_or("episode is required")?;
            if vids.is_empty() {
                return Err("no video files".into());
            }
            if eps.iter().any(|x| *x != e) {
                return Err(format!("files contain other episodes than {e}"));
            }
        }
        "episodes" => {
            let (a, b) = (
                c.episode.ok_or("episode is required")?,
                c.episode_end.ok_or("episode_end is required")?,
            );
            if eps.iter().any(|x| *x < a || *x > b) {
                return Err(format!("files contain episodes outside {a}-{b}"));
            }
            if !sample_truncated && eps.len() as i64 != b - a + 1 {
                return Err(format!("expected {} episodes, found {}", b - a + 1, eps.len()));
            }
        }
        "season_pack" => {
            if vids.is_empty() {
                return Err("no video files".into());
            }
            if let (false, Some(n), false) = (c.ongoing, c.episode_count, sample_truncated) {
                let have = eps.len().max(vids.len()) as f64;
                let need = (n as f64 * c.min_ratio).ceil();
                if have < need {
                    return Err(format!(
                        "season pack has {have} of {n} episodes (needs at least {need})"
                    ));
                }
            }
        }
        "complete" if vids.len() < 2 && !sample_truncated => {
            return Err("a complete series needs at least two video files".into());
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn episodes() {
        assert_eq!(parse_episode("Show.S01E05.1080p.mkv"), Some((Some(1), 5, None)));
        assert_eq!(
            parse_episode("dir/Show.S02E01-E03.mkv"),
            Some((Some(2), 1, Some(3)))
        );
        assert_eq!(parse_episode("Show.S02E01E02.mkv"), Some((Some(2), 1, Some(2))));
        assert_eq!(parse_episode("Show E12 [x].mkv"), Some((None, 12, None)));
        assert_eq!(parse_episode("某剧 第08集.mp4"), Some((None, 8, None)));
        assert_eq!(
            parse_episode("[Group] Anime [07v2][1080p].mkv"),
            Some((None, 7, None))
        );
        assert_eq!(
            parse_episode("[喵萌] 某番 - 05v2 [1080p].mkv"),
            Some((None, 5, None))
        );
        assert_eq!(parse_episode("Movie.2019.1080p.mkv"), None);
        assert_eq!(parse_episode("Some.Effect.mkv"), None);
    }

    #[test]
    fn scopes() {
        let big = 100 << 20;
        let f = |names: &[&str]| names.iter().map(|n| ((*n).to_owned(), big)).collect::<Vec<_>>();
        let c = |scope| ScopeCheck {
            scope,
            episode: Some(5),
            episode_end: None,
            ongoing: false,
            episode_count: Some(10),
            min_ratio: 0.8,
        };
        assert!(check_scope(&f(&["S01E05.mkv"]), &c("episode"), false).is_ok());
        assert!(check_scope(&f(&["S01E05.mkv", "S01E06.mkv"]), &c("episode"), false).is_err());
        let r = ScopeCheck {
            episode_end: Some(7),
            ..c("episodes")
        };
        assert!(check_scope(&f(&["S01E05.mkv", "S01E06.mkv", "S01E07.mkv"]), &r, false).is_ok());
        assert!(check_scope(&f(&["S01E05.mkv", "S01E07.mkv"]), &r, false).is_err());
        let pack: Vec<String> = (1..=8).map(|i| format!("Show.S01E{i:02}.mkv")).collect();
        let pack_refs: Vec<&str> = pack.iter().map(String::as_str).collect();
        assert!(check_scope(&f(&pack_refs), &c("season_pack"), false).is_ok());
        assert!(check_scope(&f(&pack_refs[..5]), &c("season_pack"), false).is_err());
        assert!(
            check_scope(
                &f(&pack_refs[..5]),
                &ScopeCheck {
                    ongoing: true,
                    ..c("season_pack")
                },
                false
            )
            .is_ok()
        );
        // sample / 小文件不计入
        let mut with_sample = f(&["S01E05.mkv"]);
        with_sample.push(("Sample/S01E09.mkv".into(), big));
        with_sample.push(("S01E10.mkv".into(), 1000));
        assert!(check_scope(&with_sample, &c("episode"), false).is_ok());
    }
}
