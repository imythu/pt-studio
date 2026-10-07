//! 结构化过滤 → Meilisearch 过滤表达式，以及钩子追加表达式的校验（LLD §1.12.2）。

use serde::Deserialize;
use serde_json::Value;

use crate::schema::{FieldType, MergedIndex};

#[derive(Debug, Clone, Deserialize)]
pub struct Filter {
    pub field: String,
    pub op: String,
    #[serde(default)]
    pub values: Vec<Value>,
}

pub const MAX_IN: usize = 1000;

/// 字符串字面量：单引号包裹，反斜杠与单引号转义，去控制字符
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars().filter(|c| !c.is_control()) {
        if c == '\\' || c == '\'' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('\'');
    out
}

fn literal(ty: FieldType, v: &Value) -> Result<String, String> {
    match ty {
        FieldType::String | FieldType::StringArray => match v {
            Value::String(s) if s.chars().count() <= 256 => Ok(quote(s)),
            Value::String(_) => Err("string value longer than 256 characters".into()),
            Value::Number(n) => Ok(quote(&n.to_string())),
            _ => Err("expected a string".into()),
        },
        FieldType::Int => v
            .as_i64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .map(|i| i.to_string())
            .ok_or_else(|| "expected an integer".into()),
        FieldType::Float => v
            .as_f64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .filter(|f| f.is_finite())
            .map(|f| f.to_string())
            .ok_or_else(|| "expected a number".into()),
        FieldType::Timestamp => v
            .as_i64()
            .or_else(|| {
                v.as_str()
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.timestamp())
            })
            .map(|i| i.to_string())
            .ok_or_else(|| "expected Unix seconds or an RFC 3339 timestamp".into()),
        FieldType::Bool => v
            .as_bool()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            .map(|b| b.to_string())
            .ok_or_else(|| "expected a boolean".into()),
    }
}

/// 错误为 (错误码, 说明)
pub fn to_expr(f: &Filter, m: &MergedIndex) -> Result<String, (&'static str, String)> {
    let invalid = |msg: String| ("search-meilisearch.invalid_filter", format!("{}: {msg}", f.field));
    let field = m.field(&f.field).filter(|x| x.filterable).ok_or_else(|| {
        (
            "search-meilisearch.field_not_filterable",
            format!("`{}` is not filterable", f.field),
        )
    })?;
    let name = &field.name;
    let one = || -> Result<String, (&'static str, String)> {
        match f.values.as_slice() {
            [v] => literal(field.ty, v).map_err(invalid),
            _ => Err(invalid("exactly one value expected".into())),
        }
    };
    let cmp = |op: &str| -> Result<String, (&'static str, String)> {
        if !field.ty.numeric() {
            return Err(invalid("comparison needs a numeric or timestamp field".into()));
        }
        Ok(format!("{name} {op} {}", one()?))
    };
    let list = || -> Result<String, (&'static str, String)> {
        if f.values.is_empty() || f.values.len() > MAX_IN {
            return Err(invalid(format!("1..={MAX_IN} values expected")));
        }
        let vs: Result<Vec<String>, String> = f.values.iter().map(|v| literal(field.ty, v)).collect();
        Ok(vs.map_err(invalid)?.join(", "))
    };
    let none = || -> Result<(), (&'static str, String)> {
        if f.values.is_empty() {
            Ok(())
        } else {
            Err(invalid("no values expected".into()))
        }
    };
    Ok(match f.op.as_str() {
        "eq" => format!("{name} = {}", one()?),
        "ne" => format!("{name} != {}", one()?),
        "in" => format!("{name} IN [{}]", list()?),
        "not_in" => format!("{name} NOT IN [{}]", list()?),
        "gt" => cmp(">")?,
        "gte" => cmp(">=")?,
        "lt" => cmp("<")?,
        "lte" => cmp("<=")?,
        "between" => {
            if !field.ty.numeric() || f.values.len() != 2 {
                return Err(invalid("between needs two values on a numeric field".into()));
            }
            let a = literal(field.ty, &f.values[0]).map_err(invalid)?;
            let b = literal(field.ty, &f.values[1]).map_err(invalid)?;
            format!("{name} {a} TO {b}")
        }
        "exists" => {
            none()?;
            format!("{name} EXISTS")
        }
        "is_null" => {
            none()?;
            format!("{name} IS NULL")
        }
        "is_empty" => {
            none()?;
            format!("{name} IS EMPTY")
        }
        other => return Err(invalid(format!("unknown operator `{other}`"))),
    })
}

// ---------------- 原始表达式校验（钩子追加的过滤） ----------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Op(&'static str),
    Str,
    Word(String),
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < cs.len() {
        let c = cs[i];
        match c {
            _ if c.is_whitespace() => i += 1,
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            '[' => {
                out.push(Tok::LBracket);
                i += 1;
            }
            ']' => {
                out.push(Tok::RBracket);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '=' => {
                out.push(Tok::Op("="));
                i += 1;
            }
            '!' if cs.get(i + 1) == Some(&'=') => {
                out.push(Tok::Op("!="));
                i += 2;
            }
            '>' | '<' => {
                let eq = cs.get(i + 1) == Some(&'=');
                out.push(Tok::Op(match (c, eq) {
                    ('>', true) => ">=",
                    ('>', false) => ">",
                    ('<', true) => "<=",
                    _ => "<",
                }));
                i += if eq { 2 } else { 1 };
            }
            '\'' | '"' => {
                i += 1;
                loop {
                    match cs.get(i) {
                        None => return Err("unterminated string".into()),
                        Some('\\') => i += 2,
                        Some(&q) if q == c => {
                            i += 1;
                            break;
                        }
                        Some(_) => i += 1,
                    }
                }
                out.push(Tok::Str);
            }
            _ if c.is_alphanumeric() || matches!(c, '_' | '.' | '-' | '+') => {
                let start = i;
                while i < cs.len() && (cs[i].is_alphanumeric() || matches!(cs[i], '_' | '.' | '-' | '+')) {
                    i += 1;
                }
                out.push(Tok::Word(cs[start..i].iter().collect()));
            }
            _ => return Err(format!("unexpected character `{c}`")),
        }
    }
    Ok(out)
}

struct P<'a> {
    t: &'a [Tok],
    i: usize,
    m: &'a MergedIndex,
    depth: usize,
}

impl P<'_> {
    fn peek_kw(&self, kw: &str) -> bool {
        matches!(self.t.get(self.i), Some(Tok::Word(w)) if w.eq_ignore_ascii_case(kw))
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        let ok = self.peek_kw(kw);
        if ok {
            self.i += 1;
        }
        ok
    }

    fn expect(&mut self, t: &Tok) -> Result<(), String> {
        if self.t.get(self.i) == Some(t) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected {t:?}"))
        }
    }

    fn or(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > 32 {
            return Err("expression nested too deeply".into());
        }
        self.and()?;
        while self.eat_kw("OR") {
            self.and()?;
        }
        self.depth -= 1;
        Ok(())
    }

    fn and(&mut self) -> Result<(), String> {
        self.not()?;
        while self.eat_kw("AND") {
            self.not()?;
        }
        Ok(())
    }

    fn not(&mut self) -> Result<(), String> {
        if self.eat_kw("NOT") {
            return self.not();
        }
        if self.t.get(self.i) == Some(&Tok::LParen) {
            self.i += 1;
            self.or()?;
            return self.expect(&Tok::RParen);
        }
        self.condition()
    }

    fn value(&mut self) -> Result<(), String> {
        match self.t.get(self.i) {
            Some(Tok::Str | Tok::Word(_)) => {
                self.i += 1;
                Ok(())
            }
            _ => Err("expected a value".into()),
        }
    }

    fn condition(&mut self) -> Result<(), String> {
        let Some(Tok::Word(field)) = self.t.get(self.i).cloned() else {
            return Err("expected a field name".into());
        };
        if !self.m.field(&field).is_some_and(|f| f.filterable) {
            return Err(format!("`{field}` is not filterable"));
        }
        self.i += 1;
        if let Some(Tok::Op(_)) = self.t.get(self.i) {
            self.i += 1;
            return self.value();
        }
        if self.eat_kw("EXISTS") {
            return Ok(());
        }
        if self.eat_kw("IS") {
            self.eat_kw("NOT");
            if self.eat_kw("NULL") || self.eat_kw("EMPTY") {
                return Ok(());
            }
            return Err("expected NULL or EMPTY".into());
        }
        if self.eat_kw("NOT") {
            if self.eat_kw("EXISTS") {
                return Ok(());
            }
            if !self.peek_kw("IN") {
                return Err("expected IN or EXISTS".into());
            }
        }
        if self.eat_kw("IN") {
            self.expect(&Tok::LBracket)?;
            if self.t.get(self.i) != Some(&Tok::RBracket) {
                self.value()?;
                while self.t.get(self.i) == Some(&Tok::Comma) {
                    self.i += 1;
                    self.value()?;
                }
            }
            return self.expect(&Tok::RBracket);
        }
        // `field a TO b`
        self.value()?;
        if !self.eat_kw("TO") {
            return Err("expected an operator".into());
        }
        self.value()
    }
}

/// 解析钩子追加的过滤表达式，确认只引用可筛选字段
pub fn validate_raw(expr: &str, m: &MergedIndex) -> Result<(), String> {
    if expr.len() > 8192 {
        return Err("filter expression too long".into());
    }
    let t = lex(expr)?;
    let mut p = P {
        t: &t,
        i: 0,
        m,
        depth: 0,
    };
    p.or()?;
    if p.i != t.len() {
        return Err("trailing tokens".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use pt_plugin_api::{FieldSpec, IndexSchema};
    use serde_json::json;

    use super::*;
    use crate::schema::merge;

    fn idx() -> MergedIndex {
        let f = |name: &str, ty: &str| FieldSpec {
            name: name.into(),
            field_type: ty.into(),
            searchable: name == "title",
            filterable: name != "title",
            sortable: false,
            rank: 0,
            exact: false,
            label_term: String::new(),
        };
        merge(
            "torrents",
            "torrent",
            &["title"],
            &[(
                "torrent".into(),
                IndexSchema {
                    index: "torrents".into(),
                    fields: vec![f("title", "string"), f("type_id", "string"), f("size", "int")],
                    synonyms: vec![],
                },
            )],
        )
    }

    fn flt(field: &str, op: &str, values: Value) -> Filter {
        Filter {
            field: field.into(),
            op: op.into(),
            values: serde_json::from_value(values).unwrap_or_default(),
        }
    }

    #[test]
    fn structured() {
        let m = idx();
        assert_eq!(
            to_expr(&flt("type_id", "in", json!(["movie", "it's"])), &m),
            Ok("type_id IN ['movie', 'it\\'s']".into())
        );
        assert_eq!(
            to_expr(&flt("size", "gte", json!([10])), &m),
            Ok("size >= 10".into())
        );
        assert_eq!(
            to_expr(&flt("size", "between", json!([1, "5"])), &m),
            Ok("size 1 TO 5".into())
        );
        assert_eq!(
            to_expr(&flt("created_at", "gt", json!(["2026-01-01T00:00:00Z"])), &m),
            Ok("created_at > 1767225600".into())
        );
        assert_eq!(
            to_expr(&flt("title", "eq", json!(["x"])), &m).map_err(|e| e.0),
            Err("search-meilisearch.field_not_filterable")
        );
        assert_eq!(
            to_expr(&flt("type_id", "gt", json!(["x"])), &m).map_err(|e| e.0),
            Err("search-meilisearch.invalid_filter")
        );
        assert_eq!(
            to_expr(&flt("size", "eq", json!(["x; DROP"])), &m).map_err(|e| e.0),
            Err("search-meilisearch.invalid_filter")
        );
        assert_eq!(
            to_expr(&flt("rel_hdr", "exists", json!([])), &m),
            Ok("rel_hdr EXISTS".into())
        );
    }

    #[test]
    fn raw() {
        let m = idx();
        for ok in [
            "vis_min_class <= 3",
            "NOT (vis_level = 'staff' AND type_id = \"movie\")",
            "type_id IN ['a', 'b'] OR size 1 TO 10",
            "rel_hdr IS NOT EMPTY AND vis_owner_id NOT EXISTS",
            "type_id NOT IN []",
        ] {
            assert_eq!(validate_raw(ok, &m), Ok(()), "{ok}");
        }
        for bad in [
            "title = 'x'",
            "secret = 1",
            "size >",
            "(size = 1",
            "size = 1 garbage",
            "size = 1; x",
        ] {
            assert!(validate_raw(bad, &m).is_err(), "{bad}");
        }
    }
}
