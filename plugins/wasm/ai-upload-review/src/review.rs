//! 上传预审的纯逻辑（ai-upload-review LLD §1.1～§1.2）：确定性检查、模型提示与输出 schema、结论合并。

use serde_json::{Value, json};

pub const SYSTEM_PROMPT: &str = "你是 PT 站的上传预审助手。根据给出的种子技术信息，找出不符合常见发布规范的问题。
只依据给出的信息判断，不要猜测；不确定时给出 warn 而不是 fail。
输出 JSON：verdict（pass / warn / fail）、findings（code、severity、message，message 用简体中文）、confidence（0~1）。
可用的 code：title_format、spec_inconsistent、description_low_quality、suspicious_files、wrong_type、other。";

/// 影视类型（MediaInfo、压缩包等规则适用）
const AV_TYPES: &[&str] = &["movie", "tv", "anime"];
const VIDEO_EXT: &[&str] = &[
    "mkv", "mp4", "avi", "ts", "m2ts", "wmv", "mov", "iso", "rmvb", "webm",
];
const ARCHIVE_EXT: &[&str] = &["rar", "zip", "7z", "tar", "gz"];
/// 模型可见的文件样本数
const FILE_SAMPLE: usize = 50;
const UNTRUSTED_MAX_CHARS: usize = 2000;

pub fn output_schema() -> Value {
    json!({ "type": "object", "additionalProperties": false, "required": ["verdict", "findings", "confidence"],
      "properties": {
        "verdict": { "type": "string", "enum": ["pass", "warn", "fail"] },
        "confidence": { "type": "number" },
        "findings": { "type": "array", "items": { "type": "object", "additionalProperties": false,
          "required": ["code", "severity", "message"],
          "properties": { "code": { "type": "string", "enum": ["title_format", "spec_inconsistent", "description_low_quality", "suspicious_files", "wrong_type", "other"] },
                          "severity": { "type": "string", "enum": ["low", "medium", "high"] },
                          "message": { "type": "string" } } } } } })
}

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub code: String,
    pub severity: &'static str,
    pub message: String,
    pub source: &'static str,
}

impl Finding {
    pub fn to_json(&self) -> Value {
        json!({ "code": self.code, "severity": self.severity, "message": self.message, "source": self.source })
    }
}

fn ext(p: &str) -> String {
    let name = p.rsplit('/').next().unwrap_or(p).to_ascii_lowercase();
    match name.rsplit_once('.') {
        // 分卷压缩 .r00 / .001
        Some((_, e)) if e.len() == 3 && e.starts_with('r') && e[1..].chars().all(|c| c.is_ascii_digit()) => {
            "rar".into()
        }
        Some((_, e)) => e.to_owned(),
        None => String::new(),
    }
}

/// `files`：`[{path, size}]`（torrent_files 的格式）
pub fn files_of(v: &Value) -> Vec<(String, u64)> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter(|f| f["padding"] != true)
        .filter_map(|f| {
            let path = match &f["path"] {
                Value::String(s) => s.clone(),
                Value::Array(a) => a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("/"),
                _ => return None,
            };
            Some((
                path,
                f["size"].as_u64().or_else(|| f["length"].as_u64()).unwrap_or(0),
            ))
        })
        .collect()
}

/// 确定性检查（LLD §1.1；规格与 MediaInfo 比对需要 ctx.mediainfo，未接入）
pub fn rules(t: &Value, files: &[(String, u64)], nfo_types: &[String]) -> Vec<Finding> {
    let ty = t["type_id"].as_str().unwrap_or("");
    let mut out = Vec::new();
    let mut add = |code: &str, severity: &'static str, message: String| {
        out.push(Finding {
            code: code.into(),
            severity,
            message,
            source: "rule",
        });
    };
    let av = AV_TYPES.contains(&ty);
    if av && t["mediainfo"].as_str().is_none_or(|m| m.trim().is_empty()) {
        add("mediainfo_missing", "medium", "影视资源未提供 MediaInfo".into());
    }
    if (av || ty == "music")
        && !files.is_empty()
        && files
            .iter()
            .all(|(p, _)| ARCHIVE_EXT.contains(&ext(p).as_str()) || ext(p) == "nfo" || ext(p) == "txt")
    {
        add(
            "archive_only",
            "high",
            "文件只有压缩包，影视 / 音乐资源应直接发布媒体文件".into(),
        );
    }
    let videos: Vec<&(String, u64)> = files
        .iter()
        .filter(|(p, _)| VIDEO_EXT.contains(&ext(p).as_str()))
        .collect();
    if av
        && !videos.is_empty()
        && videos
            .iter()
            .all(|(p, s)| *s < 200 << 20 && p.to_lowercase().contains("sample"))
    {
        add("sample_only", "high", "视频文件只有样片".into());
    }
    if nfo_types.iter().any(|x| x == ty)
        && t["has_nfo"] != true
        && !files.iter().any(|(p, _)| ext(p) == "nfo")
    {
        add("nfo_missing", "low", "该类型要求附带 NFO".into());
    }
    out
}

/// 模型输入：可信的结构化事实（text）与上传者填写的内容（untrusted）
pub fn messages(t: &Value, files: &[(String, u64)], rule_findings: &[Finding]) -> Value {
    let sample: Vec<Value> = files
        .iter()
        .take(FILE_SAMPLE)
        .map(|(p, s)| json!({ "path": p, "size": s }))
        .collect();
    let mut fields = t["fields"].clone();
    // 体积大且与审核无关的字段不送模型
    if let Some(o) = fields.as_object_mut() {
        for k in ["logs", "overview", "release_name"] {
            o.remove(k);
        }
    }
    let facts = json!({
        "type_id": t["type_id"], "size": t["size"], "file_count": t["file_count"], "fields": fields,
        "files_sample": sample, "has_nfo": t["has_nfo"], "mediainfo_present": t["mediainfo"].as_str().is_some_and(|m| !m.trim().is_empty()),
        "rule_findings": rule_findings.iter().map(|f| json!({ "code": f.code, "severity": f.severity })).collect::<Vec<_>>(),
    });
    let mut untrusted = format!("标题：{}\n", t["title"].as_str().unwrap_or(""));
    if let Some(s) = t["subtitle"].as_str().filter(|s| !s.is_empty()) {
        untrusted.push_str(&format!("副标题：{s}\n"));
    }
    let desc: String = t["description"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(UNTRUSTED_MAX_CHARS)
        .collect();
    untrusted.push_str(&format!("描述：\n{desc}"));
    json!([{ "role": "user", "parts": [
        { "type": "text", "text": format!("检查以下种子信息（技术事实为 JSON）：\n{facts}\n\n上传者填写的标题与描述：") },
        { "type": "untrusted", "text": untrusted },
    ] }])
}

fn rank(v: &str) -> u8 {
    match v {
        "fail" => 2,
        "warn" => 1,
        _ => 0,
    }
}

/// 合并（LLD §1.2）：结论取确定性结论（任一 high → fail，medium → warn）与模型结论中较严的；
/// 确定性 high 而模型 pass → 记录 `model_disagreed`
pub fn merge(rule_findings: &[Finding], model: Option<&Value>) -> (String, Option<f64>, Vec<Finding>) {
    let rule_verdict = if rule_findings.iter().any(|f| f.severity == "high") {
        "fail"
    } else if rule_findings.iter().any(|f| f.severity == "medium") {
        "warn"
    } else {
        "pass"
    };
    let mut findings = rule_findings.to_vec();
    let Some(m) = model else {
        return (rule_verdict.into(), None, findings);
    };
    let mv = m["verdict"].as_str().unwrap_or("warn");
    for f in m["findings"].as_array().into_iter().flatten() {
        let severity = match f["severity"].as_str() {
            Some("high") => "high",
            Some("medium") => "medium",
            _ => "low",
        };
        findings.push(Finding {
            code: f["code"].as_str().unwrap_or("other").into(),
            severity,
            message: f["message"].as_str().unwrap_or("").chars().take(500).collect(),
            source: "model",
        });
    }
    if rule_verdict == "fail" && mv == "pass" {
        findings.push(Finding {
            code: "model_disagreed".into(),
            severity: "low",
            message: "模型结论与确定性检查矛盾，以确定性检查为准".into(),
            source: "rule",
        });
    }
    let verdict = if rank(mv) > rank(rule_verdict) {
        mv
    } else {
        rule_verdict
    };
    (
        verdict.into(),
        m["confidence"].as_f64().map(|c| c.clamp(0.0, 1.0)),
        findings,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_rules() {
        let t = json!({ "type_id": "movie", "mediainfo": null, "has_nfo": false });
        let f = files_of(
            &json!([{ "path": ["Movie", "movie.part1.rar"], "size": 100 }, { "path": "movie.r00", "size": 100 }, { "path": "x.nfo", "size": 1 }]),
        );
        let codes: Vec<String> = rules(&t, &f, &["movie".into()])
            .into_iter()
            .map(|x| x.code)
            .collect();
        assert_eq!(codes, vec!["mediainfo_missing", "archive_only"]);
        let s = files_of(&json!([{ "path": "Sample/sample.mkv", "size": 50 << 20 }]));
        assert!(rules(&t, &s, &[]).iter().any(|x| x.code == "sample_only"));
        let ok = rules(
            &json!({ "type_id": "generic" }),
            &files_of(&json!([{ "path": "a.zip", "size": 1 }])),
            &[],
        );
        assert!(ok.is_empty());
    }

    #[test]
    fn merging() {
        let high = vec![Finding {
            code: "archive_only".into(),
            severity: "high",
            message: String::new(),
            source: "rule",
        }];
        let (v, c, f) = merge(
            &high,
            Some(&json!({ "verdict": "pass", "confidence": 0.9, "findings": [] })),
        );
        assert_eq!((v.as_str(), c), ("fail", Some(0.9)));
        assert!(f.iter().any(|x| x.code == "model_disagreed"));
        let (v, _, f) = merge(
            &[],
            Some(
                &json!({ "verdict": "warn", "confidence": 2.0, "findings": [{ "code": "title_format", "severity": "low", "message": "m" }] }),
            ),
        );
        assert_eq!((v.as_str(), f[0].source), ("warn", "model"));
        assert_eq!(merge(&[], None).0, "pass");
    }

    #[test]
    fn prompt() {
        let t = json!({ "type_id": "movie", "title": "T", "description": "x".repeat(3000), "fields": { "logs": ["big"], "resolution": "1080p" } });
        let m = messages(&t, &[], &[]);
        let facts = m[0]["parts"][0]["text"].as_str().unwrap_or("");
        assert!(facts.contains("1080p") && !facts.contains("big"));
        assert_eq!(m[0]["parts"][1]["type"], "untrusted");
        assert!(
            m[0]["parts"][1]["text"]
                .as_str()
                .is_some_and(|u| u.chars().count() < 2100)
        );
    }
}
