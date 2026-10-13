//! 抓轨日志基础检查（type-music LLD §1.2）：识别 EAC / XLD / whipper / CUERipper，按扣分制估算 0～100 分并列出问题。
//! 不验证日志校验和 / 签名的算法（只检查是否存在）。纯函数。

use serde_json::{Value, json};

/// 规则表版本（结果中记录，规则变化时递增，旧结果可重新检查）
pub const CHECKER_VERSION: i64 = 1;

/// 单个日志的大小上限（预检接口）
pub const MAX_LOG_BYTES: usize = 512 * 1024;

/// 随种子保存的日志总大小上限：种子字段 `fields` 整体 ≤ 64 KiB（torrent 表约束）
pub const MAX_STORED_LOG_BYTES: usize = 56 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Issue {
    pub code: &'static str,
    pub track: Option<i64>,
    pub deduction: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub software: Option<&'static str>,
    pub version: Option<String>,
    /// 未知软件为 None
    pub score: Option<i64>,
    pub issues: Vec<Issue>,
}

impl Report {
    pub fn to_json(&self) -> Value {
        json!({
            "software": self.software, "version": self.version, "score": self.score, "checker_version": CHECKER_VERSION,
            "issues": self.issues.iter().map(|i| json!({
                "code": i.code, "track": i.track, "deduction": i.deduction, "message_term": format!("type-music.log_issue.{}", i.code),
            })).collect::<Vec<_>>(),
        })
    }
}

/// 字节 → 文本：UTF-16 LE / BE（BOM）、UTF-8（可带 BOM）；不是合法 UTF-8 时按 Latin-1 逐字节解码（只用于匹配 ASCII 关键行）
pub fn decode(bytes: &[u8]) -> String {
    let utf16 = |b: &[u8], le: bool| {
        let units: Vec<u16> = b
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| {
                if le {
                    u16::from_le_bytes(*c)
                } else {
                    u16::from_be_bytes(*c)
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, true),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, false),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        _ => match std::str::from_utf8(bytes) {
            Ok(s) => s.to_owned(),
            Err(_) => bytes.iter().map(|b| char::from(*b)).collect(),
        },
    }
}

/// `键 : 值` 行（键大小写不敏感，冒号两侧空白任意）
fn value<'a>(lines: &[&'a str], key: &str) -> Option<&'a str> {
    let k = key.to_lowercase();
    lines.iter().find_map(|l| {
        let (a, b) = l.split_once(':')?;
        (a.trim().to_lowercase() == k).then(|| b.trim())
    })
}

/// 以 `键` 开头的行的其余部分（EAC 轨内写作 `Test CRC 1A2B3C4D`，whipper 写作 `Test CRC: 1A2B3C4D`）
fn prefixed<'a>(lines: &[&'a str], key: &str) -> Option<&'a str> {
    lines.iter().find_map(|l| {
        let rest = l.strip_prefix(key)?;
        // `CRC32 hash` 不能匹配 `CRC32 hash (test run)`
        let rest = rest.trim_start();
        (!rest.starts_with('(')).then(|| rest.trim_start_matches(':').trim())
    })
}

fn detect(text: &str) -> (Option<&'static str>, Option<String>) {
    let first_word_after = |marker: &str| {
        text.find(marker)
            .map(|p| {
                text[p + marker.len()..]
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_owned()
            })
            .filter(|v| !v.is_empty())
    };
    if text.contains("Exact Audio Copy V") {
        return (Some("eac"), first_word_after("Exact Audio Copy V"));
    }
    if text.contains("X Lossless Decoder version") {
        return (Some("xld"), first_word_after("X Lossless Decoder version"));
    }
    if text.contains("Log created by: whipper") {
        return (Some("whipper"), first_word_after("Log created by: whipper"));
    }
    if text.contains("CUERipper v") {
        return (Some("cueripper"), first_word_after("CUERipper v"));
    }
    (None, None)
}

/// 按轨切分（`Track  1` / `Track 01`），返回 (轨号, 该轨的行)
fn tracks<'a>(lines: &[&'a str]) -> Vec<(i64, Vec<&'a str>)> {
    let mut out: Vec<(i64, Vec<&'a str>)> = Vec::new();
    for l in lines {
        let t = l.trim();
        if let Some(rest) = t.strip_prefix("Track")
            && let Ok(n) = rest.trim().trim_end_matches(':').parse::<i64>()
            && rest.starts_with(' ')
        {
            out.push((n, Vec::new()));
            continue;
        }
        // 汇总段不属于最后一轨
        if t.starts_with("No errors occurred")
            || t.starts_with("There were errors")
            || t.starts_with("AccurateRip summary")
            || t.starts_with("End of status report")
            || t.starts_with("====")
        {
            out.push((0, Vec::new()));
            continue;
        }
        if let Some(last) = out.last_mut()
            && last.0 > 0
        {
            last.1.push(t);
        }
    }
    out.retain(|(n, _)| *n > 0);
    out
}

pub fn check(text: &str) -> Report {
    let (software, version) = detect(text);
    let Some(sw) = software else {
        return Report {
            software: None,
            version: None,
            score: None,
            issues: vec![Issue {
                code: "unknown_ripper",
                track: None,
                deduction: 0,
            }],
        };
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut issues: Vec<Issue> = Vec::new();
    let mut add = |code: &'static str, track: Option<i64>, deduction: i64| {
        issues.push(Issue {
            code,
            track,
            deduction,
        })
    };

    // 读取模式
    let secure = match sw {
        "eac" => value(&lines, "Read mode").is_some_and(|v| v.starts_with("Secure")),
        "xld" => value(&lines, "Ripper mode").is_some_and(|v| v.contains("Secure")),
        "whipper" => true,
        _ => value(&lines, "Read mode").is_none_or(|v| v.starts_with("Secure")),
    };
    if !secure {
        add("insecure_mode", None, 20);
    }
    // 读取偏移
    let offset = value(&lines, "Read offset correction").and_then(|v| v.trim().parse::<i64>().ok());
    if offset.is_none_or(|o| o == 0) && sw != "whipper" {
        add("zero_offset", None, 5);
    }
    // 音频缓存
    let cache_ok = match sw {
        "eac" => value(&lines, "Defeat audio cache").is_some_and(|v| v.eq_ignore_ascii_case("yes")),
        "xld" => value(&lines, "Disable audio cache").is_some_and(|v| v.eq_ignore_ascii_case("ok")),
        "whipper" => value(&lines, "Defeat audio cache").is_none_or(|v| v.eq_ignore_ascii_case("true")),
        _ => true,
    };
    if !cache_ok {
        add("audio_cache", None, 10);
    }
    // 空隙
    let gaps_ok = match sw {
        "eac" => value(&lines, "Gap handling").is_some_and(|v| !v.to_lowercase().contains("not detected")),
        "xld" => value(&lines, "Gap status").is_some_and(|v| v.starts_with("Analyzed")),
        _ => true,
    };
    if !gaps_ok {
        add("gaps_not_detected", None, 5);
    }
    // 归一化
    if text.contains("Normalize to") {
        add("normalized", None, 30);
    }
    // 校验和 / 签名：只检查存在
    let has_checksum = match sw {
        "eac" => text.contains("==== Log checksum"),
        "xld" => text.contains("-----BEGIN XLD SIGNATURE-----"),
        "whipper" => text.contains("SHA-256 hash:"),
        _ => true,
    };
    if has_checksum {
        add("checksum_unverified", None, 0);
    } else {
        add("no_checksum", None, 15);
    }
    // 逐轨
    let ts = tracks(&lines);
    if ts.is_empty() {
        add("no_tracks", None, 0);
    }
    let mut any_test = false;
    for (n, tl) in &ts {
        let v = |k: &str| prefixed(tl, k);
        let (test, copy) = match sw {
            "xld" => (v("CRC32 hash (test run)"), v("CRC32 hash")),
            _ => (v("Test CRC"), v("Copy CRC")),
        };
        if test.is_some() {
            any_test = true;
        }
        if let (Some(a), Some(b)) = (test, copy)
            && !a.eq_ignore_ascii_case(b)
        {
            add("crc_mismatch", Some(*n), 20);
        }
        let mut errors = 0;
        for l in tl {
            if l.starts_with("Suspicious position")
                || l.starts_with("Timing problem")
                || l.starts_with("Missing samples")
            {
                errors += 1;
            }
            // XLD 统计：读取错误 / 损坏扇区
            if let Some((k, val)) = l.split_once(':')
                && matches!(
                    k.trim(),
                    "Read error"
                        | "Damaged sector count"
                        | "Skipped (treated as error)"
                        | "Inconsistency in error sectors"
                )
                && val.trim().parse::<i64>().is_ok_and(|x| x > 0)
            {
                errors += 1;
            }
        }
        if errors > 0 {
            add("read_errors", Some(*n), (errors * 5).min(20));
        }
        let accurate = tl.iter().any(|l| l.contains("Accurately ripped"));
        let absent = tl
            .iter()
            .any(|l| l.contains("not present in database") || l.contains("Track not present"));
        if !accurate
            && !absent
            && tl
                .iter()
                .any(|l| l.contains("AccurateRip") || l.contains("Cannot be verified"))
        {
            add("not_accurate", Some(*n), 0);
        }
    }
    if !ts.is_empty() && !any_test {
        add("no_test_copy", None, 10);
    }
    let total: i64 = issues.iter().map(|i| i.deduction).sum();
    Report {
        software: Some(sw),
        version,
        score: Some((100 - total).max(0)),
        issues,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const EAC_GOOD: &str = "Exact Audio Copy V1.6 from 23. October 2020\r\n\r\nEAC extraction logfile from 1. January 2021, 12:00\r\n\r\nArtist / Album\r\n\r\n\
Used drive  : PLEXTOR DVDR   PX-716A   Adapter: 1  ID: 0\r\n\r\nRead mode               : Secure\r\nUtilize accurate stream : Yes\r\n\
Defeat audio cache      : Yes\r\nMake use of C2 pointers : No\r\n\r\nRead offset correction                      : 30\r\n\
Gap handling                                : Appended to previous track\r\n\r\n\
Track  1\r\n\r\n     Filename C:\\01 - A.wav\r\n\r\n     Peak level 98.0 %\r\n     Test CRC 1A2B3C4D\r\n     Copy CRC 1A2B3C4D\r\n\
     Accurately ripped (confidence 5)  [ABCDEF12]  (AR v2)\r\n     Copy OK\r\n\r\n\
Track  2\r\n\r\n     Filename C:\\02 - B.wav\r\n\r\n     Test CRC 99887766\r\n     Copy CRC 99887766\r\n\
     Accurately ripped (confidence 5)  [12345678]  (AR v2)\r\n     Copy OK\r\n\r\nNo errors occurred\r\n\r\nEnd of status report\r\n\r\n\
==== Log checksum 0123456789ABCDEF ====\r\n";

    #[test]
    fn eac_perfect() {
        let r = check(EAC_GOOD);
        assert_eq!(
            (r.software, r.version.as_deref(), r.score),
            (Some("eac"), Some("1.6"), Some(100)),
            "{:?}",
            r.issues
        );
        assert_eq!(
            r.issues.iter().map(|i| i.code).collect::<Vec<_>>(),
            vec!["checksum_unverified"]
        );
    }

    #[test]
    fn eac_problems() {
        let bad = EAC_GOOD
            .replace(
                "Read mode               : Secure",
                "Read mode               : Burst",
            )
            .replace("Test CRC 99887766", "Test CRC 11111111")
            .replace(
                "     Copy OK\r\n\r\nNo errors",
                "     Suspicious position 0:02:20\r\n     Suspicious position 0:02:21\r\n\r\nNo errors",
            )
            .replace("==== Log checksum 0123456789ABCDEF ====\r\n", "");
        let r = check(&bad);
        // −20 读取模式、−20 CRC、−10 两处可疑、−15 无校验和
        assert_eq!(r.score, Some(35), "{:?}", r.issues);
        assert!(
            r.issues
                .iter()
                .any(|i| i.code == "crc_mismatch" && i.track == Some(2))
        );
        let no_test = EAC_GOOD
            .replace("     Test CRC 1A2B3C4D\r\n", "")
            .replace("     Test CRC 99887766\r\n", "");
        assert_eq!(check(&no_test).score, Some(90));
        assert_eq!(
            check(&EAC_GOOD.replace(
                "correction                      : 30",
                "correction                      : 0"
            ))
            .score,
            Some(95)
        );
    }

    #[test]
    fn xld_and_unknown() {
        let xld = "X Lossless Decoder version 20230627 (156.2)\n\nXLD extraction logfile from 2023-07-01\n\n\
Ripper mode             : XLD Secure Ripper\nDisable audio cache     : OK\nRead offset correction  : 667\nGap status              : Analyzed, Appended\n\n\
Track 01\n    CRC32 hash (test run)  : 1A2B3C4D\n    CRC32 hash             : 1A2B3C4D\n        ->Accurately ripped (v1+v2, confidence 3+2/5)\n\
    Statistics\n        Read error                           : 0\n        Damaged sector count                 : 2\n\n\
No errors occurred\n\nEnd of status report\n\n-----BEGIN XLD SIGNATURE-----\nABC\n-----END XLD SIGNATURE-----\n";
        let r = check(xld);
        assert_eq!((r.software, r.score), (Some("xld"), Some(95)), "{:?}", r.issues);
        let u = check("some random text");
        assert_eq!((u.score, u.issues[0].code), (None, "unknown_ripper"));
    }

    #[test]
    fn decoding() {
        let mut b = vec![0xFF, 0xFE];
        for u in "Exact Audio Copy V1.0".encode_utf16() {
            b.extend(u.to_le_bytes());
        }
        assert_eq!(decode(&b), "Exact Audio Copy V1.0");
        assert_eq!(decode(b"\xEF\xBB\xBFabc"), "abc");
        assert_eq!(decode(&[0x41, 0xE9]), "Aé");
    }
}
