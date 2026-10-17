//! 图片识别（按文件头，不信任 Content-Type）、尺寸读取、元数据去除（EXIF / XMP / 文本块，防止泄露位置与设备）。
//! 不解码像素：只解析容器结构，格式不对或结构损坏一律拒绝。

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl Kind {
    pub fn ext(self) -> &'static str {
        match self {
            Kind::Png => "png",
            Kind::Jpeg => "jpg",
            Kind::Gif => "gif",
            Kind::Webp => "webp",
        }
    }
    pub fn mime(self) -> &'static str {
        match self {
            Kind::Png => "image/png",
            Kind::Jpeg => "image/jpeg",
            Kind::Gif => "image/gif",
            Kind::Webp => "image/webp",
        }
    }
}

pub fn detect(b: &[u8]) -> Option<Kind> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Kind::Png)
    } else if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(Kind::Jpeg)
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some(Kind::Gif)
    } else if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some(Kind::Webp)
    } else {
        None
    }
}

fn be16(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from(u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?])))
}
fn be32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]))
}
fn le16(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from(u16::from_le_bytes([*b.get(i)?, *b.get(i + 1)?])))
}
fn le24(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from(*b.get(i)?) | u32::from(*b.get(i + 1)?) << 8 | u32::from(*b.get(i + 2)?) << 16)
}
fn le32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]))
}

/// 去除元数据并返回 (新字节, 宽, 高)
pub fn clean(kind: Kind, b: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let r = match kind {
        Kind::Png => png(b),
        Kind::Jpeg => jpeg(b),
        Kind::Gif => gif(b),
        Kind::Webp => webp(b),
    }?;
    (r.1 > 0 && r.2 > 0).then_some(r)
}

/// PNG：逐块复制，丢弃文本 / EXIF / 时间块；要求 IHDR 在首、IEND 结尾
fn png(b: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut out = b[..8].to_vec();
    let mut i = 8;
    let (mut w, mut h, mut end) = (0, 0, false);
    while i + 12 <= b.len() {
        let len = usize::try_from(be32(b, i)?).ok()?;
        let ty = b.get(i + 4..i + 8)?;
        let total = 12usize.checked_add(len)?;
        let chunk = b.get(i..i.checked_add(total)?)?;
        if i == 8 {
            if ty != b"IHDR" {
                return None;
            }
            w = be32(b, i + 8)?;
            h = be32(b, i + 12)?;
        }
        if !matches!(ty, b"eXIf" | b"tEXt" | b"zTXt" | b"iTXt" | b"tIME") {
            out.extend_from_slice(chunk);
        }
        i += total;
        if ty == b"IEND" {
            end = true;
            break;
        }
    }
    end.then_some((out, w, h))
}

/// JPEG：保留 APP0（JFIF）/ APP2（ICC）/ APP14（Adobe）与图像段，丢弃 APP1（EXIF / XMP）、APP13（IPTC）、其余 APPn 与 COM
fn jpeg(b: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut out = vec![0xFF, 0xD8];
    let mut i = 2;
    let (mut w, mut h) = (0, 0);
    loop {
        // 段之间允许填充 0xFF
        while b.get(i) == Some(&0xFF) && b.get(i + 1) == Some(&0xFF) {
            i += 1;
        }
        if *b.get(i)? != 0xFF {
            return None;
        }
        let m = *b.get(i + 1)?;
        if m == 0xD9 {
            out.extend_from_slice(&[0xFF, 0xD9]);
            break;
        }
        if (0xD0..=0xD7).contains(&m) || m == 0x01 {
            out.extend_from_slice(&b[i..i + 2]);
            i += 2;
            continue;
        }
        let len = usize::try_from(be16(b, i + 2)?).ok()?;
        if len < 2 {
            return None;
        }
        let seg = b.get(i..i + 2 + len)?;
        if matches!(m, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            h = be16(b, i + 5)?;
            w = be16(b, i + 7)?;
        }
        let drop = (0xE1..=0xEF).contains(&m) && !matches!(m, 0xE2 | 0xEE) || m == 0xFE;
        if !drop {
            out.extend_from_slice(seg);
        }
        i += 2 + len;
        if m == 0xDA {
            // 扫描数据：原样复制到结尾（含后续 RST / 多次扫描 / EOI）
            out.extend_from_slice(b.get(i..)?);
            if !b.ends_with(&[0xFF, 0xD9]) {
                return None;
            }
            break;
        }
    }
    Some((out, w, h))
}

/// GIF：只读尺寸（GIF 的注释扩展不含位置信息，原样保留）
fn gif(b: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    if b.len() < 14 || *b.last()? != 0x3B {
        return None;
    }
    Some((b.to_vec(), le16(b, 6)?, le16(b, 8)?))
}

/// WebP：去掉 EXIF / XMP 块，清除 VP8X 中对应标志位并重写 RIFF 长度
fn webp(b: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let riff = usize::try_from(le32(b, 4)?).ok()?;
    if riff + 8 > b.len() || riff < 4 {
        return None;
    }
    let mut out = b"RIFF\0\0\0\0WEBP".to_vec();
    let mut i = 12;
    let (mut w, mut h) = (0, 0);
    let mut vp8x_at = None;
    while i + 8 <= riff + 8 {
        let ty = b.get(i..i + 4)?;
        let len = usize::try_from(le32(b, i + 4)?).ok()?;
        let padded = len + (len & 1);
        let chunk = b.get(i..(i + 8 + padded).min(b.len()))?;
        match ty {
            b"VP8X" => {
                w = le24(b, i + 12)? + 1;
                h = le24(b, i + 15)? + 1;
                vp8x_at = Some(out.len());
            }
            b"VP8 " if w == 0 => {
                // 关键帧头：3 字节帧标签 + 起始码 9d 01 2a + 14 位宽高
                if b.get(i + 11..i + 14)? != [0x9d, 0x01, 0x2a] {
                    return None;
                }
                w = le16(b, i + 14)? & 0x3FFF;
                h = le16(b, i + 16)? & 0x3FFF;
            }
            b"VP8L" if w == 0 => {
                if *b.get(i + 8)? != 0x2F {
                    return None;
                }
                let bits = le32(b, i + 9)?;
                w = (bits & 0x3FFF) + 1;
                h = ((bits >> 14) & 0x3FFF) + 1;
            }
            _ => {}
        }
        if !matches!(ty, b"EXIF" | b"XMP ") {
            out.extend_from_slice(chunk);
        }
        i += 8 + padded;
    }
    if let Some(at) = vp8x_at {
        // 标志字节：bit 3 = EXIF，bit 2 = XMP
        out[at + 8] &= !0b0000_1100;
    }
    let size = u32::try_from(out.len() - 8).ok()?;
    out[4..8].copy_from_slice(&size.to_le_bytes());
    Some((out, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_chunk(ty: &[u8], data: &[u8]) -> Vec<u8> {
        let mut c = (u32::try_from(data.len()).unwrap_or(0)).to_be_bytes().to_vec();
        c.extend_from_slice(ty);
        c.extend_from_slice(data);
        c.extend_from_slice(&[0, 0, 0, 0]); // CRC 不校验
        c
    }

    pub fn sample_png(w: u32, h: u32, with_text: bool) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut ihdr = w.to_be_bytes().to_vec();
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        b.extend(png_chunk(b"IHDR", &ihdr));
        if with_text {
            b.extend(png_chunk(b"tEXt", b"Comment\0secret location"));
            b.extend(png_chunk(b"eXIf", b"MM\0*GPS"));
        }
        b.extend(png_chunk(b"IDAT", &[1, 2, 3]));
        b.extend(png_chunk(b"IEND", &[]));
        b
    }

    #[test]
    fn png_strips_text() {
        let src = sample_png(640, 480, true);
        assert_eq!(detect(&src), Some(Kind::Png));
        let (out, w, h) = clean(Kind::Png, &src).unwrap_or_default();
        assert_eq!((w, h), (640, 480));
        assert!(!out.windows(6).any(|x| x == b"secret") && !out.windows(4).any(|x| x == b"eXIf"));
        assert!(out.windows(4).any(|x| x == b"IDAT"));
        assert!(clean(Kind::Png, &src[..src.len() - 5]).is_none(), "truncated");
    }

    #[test]
    fn jpeg_strips_exif() {
        let mut b = vec![0xFF, 0xD8];
        b.extend_from_slice(&[0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 1, 0, 0, 1, 0, 1, 0, 0]);
        let exif = b"Exif\0\0GPS-40.7N";
        b.extend_from_slice(&[0xFF, 0xE1, 0, u8::try_from(exif.len() + 2).unwrap_or(0)]);
        b.extend_from_slice(exif);
        b.extend_from_slice(&[0xFF, 0xFE, 0, 6, b'c', b'a', b'm', b'!']);
        // SOF0：长度 11，精度 8，高 100，宽 200，1 分量
        b.extend_from_slice(&[0xFF, 0xC0, 0, 11, 8, 0, 100, 0, 200, 1, 1, 0x11, 0]);
        b.extend_from_slice(&[0xFF, 0xDA, 0, 8, 1, 1, 0, 0, 0x3F, 0, 0xAB, 0xCD, 0xFF, 0xD9]);
        assert_eq!(detect(&b), Some(Kind::Jpeg));
        let (out, w, h) = clean(Kind::Jpeg, &b).unwrap_or_default();
        assert_eq!((w, h), (200, 100));
        assert!(!out.windows(3).any(|x| x == b"GPS") && !out.windows(4).any(|x| x == b"cam!"));
        assert!(out.windows(4).any(|x| x == b"JFIF") && out.ends_with(&[0xFF, 0xD9]));
    }

    #[test]
    fn gif_and_webp() {
        let mut g = b"GIF89a".to_vec();
        g.extend_from_slice(&[0x20, 0x03, 0x58, 0x02, 0, 0, 0, 0x3B]);
        assert_eq!(clean(Kind::Gif, &g).map(|x| (x.1, x.2)), Some((800, 600)));
        // VP8X（带 EXIF 标志）+ EXIF + VP8L
        let mut body = b"WEBP".to_vec();
        body.extend_from_slice(b"VP8X");
        body.extend_from_slice(&10u32.to_le_bytes());
        body.extend_from_slice(&[0b0000_1000, 0, 0, 0, 0x7F, 0x02, 0, 0xDF, 0x01, 0]); // 640 x 480
        body.extend_from_slice(b"EXIF");
        body.extend_from_slice(&4u32.to_le_bytes());
        body.extend_from_slice(b"GPS!");
        body.extend_from_slice(b"VP8L");
        body.extend_from_slice(&5u32.to_le_bytes());
        body.extend_from_slice(&[0x2F, 0, 0, 0, 0, 0]);
        let mut w = b"RIFF".to_vec();
        w.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_le_bytes());
        w.extend(body);
        assert_eq!(detect(&w), Some(Kind::Webp));
        let (out, ww, hh) = clean(Kind::Webp, &w).unwrap_or_default();
        assert_eq!((ww, hh), (640, 480));
        assert!(!out.windows(4).any(|x| x == b"GPS!"));
        assert_eq!(out[20] & 0b1000, 0, "EXIF flag cleared");
        assert_eq!(le32(&out, 4).map(|n| n as usize + 8), Some(out.len()));
        assert!(detect(b"hello").is_none());
    }
}
