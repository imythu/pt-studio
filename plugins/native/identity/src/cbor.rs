//! 最小 CBOR 解码（RFC 8949 子集），供 WebAuthn 解析 attestationObject 与 COSE 公钥。
//!
//! 只支持确定长度的整数、字节串、文本串、数组、映射与 false / true / null；不定长、标签与浮点视为错误。

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Int(i128),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Value>),
    Map(Vec<(Value, Value)>),
    Bool(bool),
    Null,
}

impl Value {
    /// 映射中按整数键取值（COSE）
    pub fn get_int(&self, key: i128) -> Option<&Value> {
        match self {
            Value::Map(m) => m.iter().find(|(k, _)| *k == Value::Int(key)).map(|(_, v)| v),
            _ => None,
        }
    }
    /// 映射中按文本键取值
    pub fn get_text(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(m) => m
                .iter()
                .find(|(k, _)| matches!(k, Value::Text(t) if t == key))
                .map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }
    pub fn as_int(&self) -> Option<i128> {
        match self {
            Value::Int(i) => Some(*i),
            _ => None,
        }
    }
}

const MAX_DEPTH: u8 = 8;
const MAX_ITEMS: u64 = 1024;

/// 解码一个数据项，返回 (值, 消耗的字节数)
pub fn decode(buf: &[u8]) -> Option<(Value, usize)> {
    let mut pos = 0;
    let v = item(buf, &mut pos, 0)?;
    Some((v, pos))
}

fn take<'a>(buf: &'a [u8], pos: &mut usize, n: usize) -> Option<&'a [u8]> {
    let end = pos.checked_add(n)?;
    let s = buf.get(*pos..end)?;
    *pos = end;
    Some(s)
}

fn arg(buf: &[u8], pos: &mut usize, info: u8) -> Option<u64> {
    Some(match info {
        0..=23 => u64::from(info),
        24 => u64::from(take(buf, pos, 1)?[0]),
        25 => u64::from(u16::from_be_bytes(take(buf, pos, 2)?.try_into().ok()?)),
        26 => u64::from(u32::from_be_bytes(take(buf, pos, 4)?.try_into().ok()?)),
        27 => u64::from_be_bytes(take(buf, pos, 8)?.try_into().ok()?),
        _ => return None,
    })
}

fn item(buf: &[u8], pos: &mut usize, depth: u8) -> Option<Value> {
    if depth > MAX_DEPTH {
        return None;
    }
    let b = take(buf, pos, 1)?[0];
    let (major, info) = (b >> 5, b & 0x1f);
    if major == 7 {
        return match info {
            20 => Some(Value::Bool(false)),
            21 => Some(Value::Bool(true)),
            22 => Some(Value::Null),
            _ => None,
        };
    }
    let n = arg(buf, pos, info)?;
    match major {
        0 => Some(Value::Int(i128::from(n))),
        1 => Some(Value::Int(-1 - i128::from(n))),
        2 => Some(Value::Bytes(take(buf, pos, usize::try_from(n).ok()?)?.to_vec())),
        3 => String::from_utf8(take(buf, pos, usize::try_from(n).ok()?)?.to_vec())
            .ok()
            .map(Value::Text),
        4 if n <= MAX_ITEMS => (0..n)
            .map(|_| item(buf, pos, depth + 1))
            .collect::<Option<Vec<_>>>()
            .map(Value::Array),
        5 if n <= MAX_ITEMS => (0..n)
            .map(|_| Some((item(buf, pos, depth + 1)?, item(buf, pos, depth + 1)?)))
            .collect::<Option<Vec<_>>>()
            .map(Value::Map),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc8949_examples() {
        assert_eq!(decode(&[0x00]), Some((Value::Int(0), 1)));
        assert_eq!(decode(&[0x18, 0x64]), Some((Value::Int(100), 2)));
        assert_eq!(decode(&[0x38, 0x63]), Some((Value::Int(-100), 2)));
        assert_eq!(decode(&[0x26]), Some((Value::Int(-7), 1)));
        assert_eq!(decode(&[0x44, 1, 2, 3, 4]), Some((Value::Bytes(vec![1, 2, 3, 4]), 5)));
        assert_eq!(decode(&[0x64, b'I', b'E', b'T', b'F']), Some((Value::Text("IETF".into()), 5)));
        // {1: 2, "a": [true, null]} 后跟多余字节：只消耗第一个数据项
        let (v, used) = decode(&[0xa2, 0x01, 0x02, 0x61, b'a', 0x82, 0xf5, 0xf6, 0xff]).unwrap_or((Value::Null, 0));
        assert_eq!(used, 8);
        assert_eq!(v.get_int(1), Some(&Value::Int(2)));
        assert_eq!(v.get_text("a"), Some(&Value::Array(vec![Value::Bool(true), Value::Null])));
        // 截断、不定长、过深
        assert_eq!(decode(&[0x44, 1, 2]), None);
        assert_eq!(decode(&[0x5f]), None);
        assert_eq!(decode(&[0x81; 20]), None);
    }
}
