//! 严格的 bencode 解码、规范编码与根字典拼接（torrent 03 §1.1）。迭代实现，不递归。

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BValue<'a> {
    Int(i64),
    Bytes(&'a [u8]),
    List(Vec<BValue<'a>>),
    /// 键按出现顺序（已验证严格递增）；raw 为整个字典（含 d…e）的原始字节
    Dict {
        entries: Vec<(&'a [u8], BValue<'a>)>,
        raw: &'a [u8],
    },
}

impl<'a> BValue<'a> {
    pub fn get(&self, key: &str) -> Option<&BValue<'a>> {
        match self {
            BValue::Dict { entries, .. } => {
                entries.iter().find(|(k, _)| *k == key.as_bytes()).map(|(_, v)| v)
            }
            _ => None,
        }
    }
    pub fn as_int(&self) -> Option<i64> {
        if let BValue::Int(i) = self { Some(*i) } else { None }
    }
    pub fn as_bytes(&self) -> Option<&'a [u8]> {
        if let BValue::Bytes(b) = self {
            Some(b)
        } else {
            None
        }
    }
    pub fn as_list(&self) -> Option<&[BValue<'a>]> {
        if let BValue::List(l) = self { Some(l) } else { None }
    }
    pub fn entries(&self) -> Option<&[(&'a [u8], BValue<'a>)]> {
        if let BValue::Dict { entries, .. } = self {
            Some(entries)
        } else {
            None
        }
    }
    pub fn raw(&self) -> Option<&'a [u8]> {
        if let BValue::Dict { raw, .. } = self {
            Some(raw)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_depth: usize,
    pub max_items: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_depth: 64,
            max_items: 2_000_000,
        }
    }
}

/// 错误代号即 `torrent.invalid_file` 的 `details.reason`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BError(pub &'static str);

enum Frame<'a> {
    List {
        items: Vec<BValue<'a>>,
    },
    Dict {
        start: usize,
        entries: Vec<(&'a [u8], BValue<'a>)>,
        key: Option<&'a [u8]>,
    },
}

fn read_int(buf: &[u8], pos: &mut usize, end: u8) -> Result<i64, BError> {
    let start = *pos;
    let stop = buf[start..]
        .iter()
        .position(|&c| c == end)
        .ok_or(BError("int_format"))?
        + start;
    let s = &buf[start..stop];
    let digits = s.strip_prefix(b"-").unwrap_or(s);
    if digits.is_empty()
        || !digits.iter().all(u8::is_ascii_digit)
        || (digits.len() > 1 && digits[0] == b'0')
        || (s.starts_with(b"-") && digits == b"0")
    {
        return Err(BError("int_format"));
    }
    let v = std::str::from_utf8(s)
        .ok()
        .and_then(|x| x.parse::<i64>().ok())
        .ok_or(BError("int_format"))?;
    *pos = stop + 1;
    Ok(v)
}

fn read_bytes<'a>(buf: &'a [u8], pos: &mut usize) -> Result<&'a [u8], BError> {
    let start = *pos;
    let colon = buf[start..]
        .iter()
        .position(|&c| c == b':')
        .ok_or(BError("str_len"))?
        + start;
    let len_s = &buf[start..colon];
    if len_s.is_empty() || !len_s.iter().all(u8::is_ascii_digit) || (len_s.len() > 1 && len_s[0] == b'0') {
        return Err(BError("str_len"));
    }
    let len: usize = std::str::from_utf8(len_s)
        .ok()
        .and_then(|x| x.parse().ok())
        .ok_or(BError("str_len"))?;
    let data_start = colon + 1;
    let data_end = data_start
        .checked_add(len)
        .filter(|e| *e <= buf.len())
        .ok_or(BError("str_len"))?;
    *pos = data_end;
    Ok(&buf[data_start..data_end])
}

pub fn decode<'a>(buf: &'a [u8], lim: &Limits) -> Result<BValue<'a>, BError> {
    if buf.first() != Some(&b'd') {
        return Err(BError("root_type"));
    }
    let mut stack: Vec<Frame<'a>> = Vec::new();
    let mut pos = 0usize;
    let mut items = 0usize;
    loop {
        if pos >= buf.len() {
            return Err(BError("truncated"));
        }
        // 字典中等待键
        if let Some(Frame::Dict { key: None, .. }) = stack.last()
            && buf[pos] != b'e'
        {
            let k = read_bytes(buf, &mut pos)?;
            if let Some(Frame::Dict { entries, key, .. }) = stack.last_mut() {
                if entries.last().is_some_and(|(prev, _)| *prev >= k) {
                    return Err(BError("dict_order"));
                }
                *key = Some(k);
            }
            continue;
        }
        let c = buf[pos];
        let value: BValue<'a> = match c {
            b'i' => {
                pos += 1;
                BValue::Int(read_int(buf, &mut pos, b'e')?)
            }
            b'0'..=b'9' => BValue::Bytes(read_bytes(buf, &mut pos)?),
            b'l' | b'd' => {
                if stack.len() >= lim.max_depth {
                    return Err(BError("depth"));
                }
                stack.push(if c == b'l' {
                    Frame::List { items: Vec::new() }
                } else {
                    Frame::Dict {
                        start: pos,
                        entries: Vec::new(),
                        key: None,
                    }
                });
                pos += 1;
                continue;
            }
            b'e' => {
                let frame = stack.pop().ok_or(BError("trailing"))?;
                pos += 1;
                match frame {
                    Frame::List { items } => BValue::List(items),
                    Frame::Dict { start, entries, key } => {
                        if key.is_some() {
                            return Err(BError("dict_value"));
                        }
                        BValue::Dict {
                            entries,
                            raw: &buf[start..pos],
                        }
                    }
                }
            }
            _ => return Err(BError("token")),
        };
        items += 1;
        if items > lim.max_items {
            return Err(BError("items"));
        }
        match stack.last_mut() {
            None => {
                return if pos == buf.len() {
                    Ok(value)
                } else {
                    Err(BError("trailing"))
                };
            }
            Some(Frame::List { items }) => items.push(value),
            Some(Frame::Dict { entries, key, .. }) => {
                let k = key.take().ok_or(BError("dict_value"))?;
                entries.push((k, value));
            }
        }
    }
}

pub fn encode_bytes(b: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(b.len().to_string().as_bytes());
    out.push(b':');
    out.extend_from_slice(b);
}

pub fn encode_int(i: i64, out: &mut Vec<u8>) {
    out.push(b'i');
    out.extend_from_slice(i.to_string().as_bytes());
    out.push(b'e');
}

/// 规范编码：字典键按字节序。
pub fn encode(v: &BValue<'_>, out: &mut Vec<u8>) {
    match v {
        BValue::Int(i) => encode_int(*i, out),
        BValue::Bytes(b) => encode_bytes(b, out),
        BValue::List(l) => {
            out.push(b'l');
            for x in l {
                encode(x, out);
            }
            out.push(b'e');
        }
        BValue::Dict { entries, .. } => {
            let mut sorted: Vec<_> = entries.iter().collect();
            sorted.sort_by(|a, b| a.0.cmp(b.0));
            out.push(b'd');
            for (k, x) in sorted {
                encode_bytes(k, out);
                encode(x, out);
            }
            out.push(b'e');
        }
    }
}

/// 预编码值组成的字典（键为字节序）。
pub fn encode_raw_dict(mut entries: Vec<(Vec<u8>, Vec<u8>)>) -> Vec<u8> {
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = vec![b'd'];
    for (k, v) in entries {
        encode_bytes(&k, &mut out);
        out.extend_from_slice(&v);
    }
    out.push(b'e');
    out
}

/// 根字典拼接：顶层条目保留原始字节（info 的 hash 不变），`set` 覆盖或插入，`remove` 删除。
pub fn splice_root(root_raw: &[u8], set: &[(&[u8], Vec<u8>)], remove: &[&[u8]]) -> Result<Vec<u8>, BError> {
    let root = decode(root_raw, &Limits::default())?;
    let entries = root.entries().ok_or(BError("root_type"))?;
    let mut out: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for (k, v) in entries {
        if remove.contains(k) || set.iter().any(|(sk, _)| sk == k) {
            continue;
        }
        let mut raw = Vec::new();
        match v.raw() {
            Some(r) => raw.extend_from_slice(r),
            None => encode(v, &mut raw),
        }
        out.push((k.to_vec(), raw));
    }
    for (k, v) in set {
        out.push((k.to_vec(), v.clone()));
    }
    Ok(encode_raw_dict(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &[u8]) -> Result<BValue<'_>, BError> {
        decode(s, &Limits::default())
    }

    #[test]
    fn decodes_and_reencodes() -> Result<(), BError> {
        let s = b"d3:bar4:spam3:fooi42e4:listli1ei2eee";
        let v = d(s)?;
        assert_eq!(v.get("foo").and_then(BValue::as_int), Some(42));
        let mut out = Vec::new();
        encode(&v, &mut out);
        assert_eq!(out, s);
        assert_eq!(v.raw(), Some(&s[..]));
        Ok(())
    }

    #[test]
    fn rejects_malformed() {
        for (s, why) in [
            (&b"d3:fooi-0ee"[..], "int_format"),
            (b"d3:fooi03ee", "int_format"),
            (b"d3:foo04:spame", "str_len"),
            (b"d3:foo9:spame", "str_len"),
            (b"d3:zzzi1e3:aaai1ee", "dict_order"),
            (b"d3:aaai1e3:aaai1ee", "dict_order"),
            (b"d3:fooi1eexx", "trailing"),
            (b"li1ee", "root_type"),
            (b"d3:foo", "truncated"),
        ] {
            assert_eq!(d(s).err(), Some(BError(why)), "{}", String::from_utf8_lossy(s));
        }
        let deep = [b"d1:a".repeat(70), b"i1e".to_vec(), b"e".repeat(70)].concat();
        assert_eq!(d(&deep).err(), Some(BError("depth")));
    }

    #[test]
    fn splice_keeps_info_bytes() -> Result<(), BError> {
        let s = b"d8:announce3:old4:infod4:name1:xee";
        let mut ann = Vec::new();
        encode_bytes(b"https://t/announce/pk", &mut ann);
        let out = splice_root(s, &[(b"announce", ann), (b"comment", b"1:c".to_vec())], &[])?;
        assert_eq!(
            out,
            b"d8:announce21:https://t/announce/pk7:comment1:c4:infod4:name1:xee"
        );
        Ok(())
    }
}
