//! SQL 参数与结果：`sql::query("… WHERE id = $1", &[uid.into()])`，`row.int(0)`、`row.text(1)`。

use serde_json::Value;

use crate::SqlValue;

impl From<i64> for SqlValue {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}

impl From<i32> for SqlValue {
    fn from(v: i32) -> Self {
        Self::Int(i64::from(v))
    }
}

impl From<u32> for SqlValue {
    fn from(v: u32) -> Self {
        Self::Int(i64::from(v))
    }
}

impl From<bool> for SqlValue {
    fn from(v: bool) -> Self {
        Self::Boolean(v)
    }
}

impl From<f64> for SqlValue {
    fn from(v: f64) -> Self {
        Self::Float(v)
    }
}

impl From<&str> for SqlValue {
    fn from(v: &str) -> Self {
        Self::Text(v.to_owned())
    }
}

impl From<String> for SqlValue {
    fn from(v: String) -> Self {
        Self::Text(v)
    }
}

impl From<&String> for SqlValue {
    fn from(v: &String) -> Self {
        Self::Text(v.clone())
    }
}

/// JSON 值按 jsonb 传入
impl From<&Value> for SqlValue {
    fn from(v: &Value) -> Self {
        Self::Json(v.to_string())
    }
}

impl From<Vec<i64>> for SqlValue {
    fn from(v: Vec<i64>) -> Self {
        Self::IntArray(v)
    }
}

impl From<Vec<String>> for SqlValue {
    fn from(v: Vec<String>) -> Self {
        Self::TextArray(v)
    }
}

/// `None` → SQL NULL
impl<T: Into<SqlValue>> From<Option<T>> for SqlValue {
    fn from(v: Option<T>) -> Self {
        v.map_or(Self::Null, Into::into)
    }
}

impl SqlValue {
    /// 文本（text、json、numeric、uuid 列）
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) | Self::Json(s) | Self::Numeric(s) | Self::Uuid(s) => Some(s),
            _ => None,
        }
    }

    /// 整数（bigint / integer / smallint 列；timestamp 列为 Unix 微秒）
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) | Self::Timestamp(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(f) => Some(*f),
            #[allow(clippy::cast_precision_loss)]
            Self::Int(i) => Some(*i as f64),
            Self::Numeric(s) => s.parse().ok(),
            _ => None,
        }
    }

    /// jsonb 列（或 JSON 文本）解析为 `Value`
    pub fn as_json(&self) -> Option<Value> {
        self.as_text().and_then(|s| serde_json::from_str(s).ok())
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

/// 按列序号读取一行（越界或类型不符返回 `None`）
pub trait RowExt {
    fn value(&self, i: usize) -> Option<&SqlValue>;

    fn int(&self, i: usize) -> Option<i64> {
        self.value(i).and_then(SqlValue::as_int)
    }

    fn text(&self, i: usize) -> Option<String> {
        self.value(i).and_then(SqlValue::as_text).map(str::to_owned)
    }

    fn bool(&self, i: usize) -> Option<bool> {
        self.value(i).and_then(SqlValue::as_bool)
    }

    fn float(&self, i: usize) -> Option<f64> {
        self.value(i).and_then(SqlValue::as_f64)
    }

    fn json(&self, i: usize) -> Option<Value> {
        self.value(i).and_then(SqlValue::as_json)
    }
}

impl RowExt for [SqlValue] {
    fn value(&self, i: usize) -> Option<&SqlValue> {
        self.get(i)
    }
}

impl RowExt for Vec<SqlValue> {
    fn value(&self, i: usize) -> Option<&SqlValue> {
        self.get(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn conversions() {
        assert!(matches!(SqlValue::from(5i64), SqlValue::Int(5)));
        assert!(matches!(SqlValue::from("a"), SqlValue::Text(s) if s == "a"));
        assert!(matches!(SqlValue::from(None::<i64>), SqlValue::Null));
        assert!(matches!(SqlValue::from(Some(true)), SqlValue::Boolean(true)));
        assert!(matches!(SqlValue::from(&json!({ "a": 1 })), SqlValue::Json(s) if s == "{\"a\":1}"));
    }

    #[test]
    fn rows() {
        let row = vec![
            SqlValue::Int(7),
            SqlValue::Text("x".into()),
            SqlValue::Json("{\"k\":2}".into()),
            SqlValue::Null,
            SqlValue::Numeric("1.5".into()),
        ];
        assert_eq!(row.int(0), Some(7));
        assert_eq!(row.text(1).as_deref(), Some("x"));
        assert_eq!(row.json(2), Some(json!({ "k": 2 })));
        assert_eq!(row.int(3), None);
        assert_eq!(row.int(9), None);
        assert_eq!(row.float(4), Some(1.5));
        assert!(row[3].is_null());
    }
}
