//! Raster image checks. No decoding: only the file signature and the header dimensions are read.
//! SVG is never served. Metadata (EXIF, text chunks) is never read or shown.
pub const MAX_IMG_BYTES: usize = 5_000_000;
pub const MAX_PIXELS: u64 = 16_000_000;
pub const MAX_SIDE: u64 = 16_384;
pub const MAX_SEGMENTS: usize = 1000;
pub const ALL_EXT: [&str; 6] = [".png", ".jpg", ".jpeg", ".gif", ".webp", ""];

#[derive(Debug, PartialEq)]
pub struct BadImage(pub &'static str);

pub fn is_image_ext(path: &str) -> bool {
    let low = path.to_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp"].iter().any(|e| low.ends_with(e))
}

fn kind(d: &[u8]) -> Result<&'static str, BadImage> {
    if d.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok("png")
    } else if d.starts_with(b"\xff\xd8\xff") {
        Ok("jpeg")
    } else if d.starts_with(b"GIF87a") || d.starts_with(b"GIF89a") {
        Ok("gif")
    } else if d.len() >= 12 && &d[..4] == b"RIFF" && &d[8..12] == b"WEBP" {
        Ok("webp")
    } else {
        Err(BadImage("not a supported raster image"))
    }
}

fn be16(d: &[u8], i: usize) -> Result<u64, BadImage> {
    d.get(i..i + 2).map(|b| u16::from_be_bytes([b[0], b[1]]) as u64).ok_or(BadImage("truncated header"))
}
fn le16(d: &[u8], i: usize) -> Result<u64, BadImage> {
    d.get(i..i + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as u64).ok_or(BadImage("truncated header"))
}
fn be32(d: &[u8], i: usize) -> Result<u64, BadImage> {
    d.get(i..i + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as u64).ok_or(BadImage("truncated header"))
}
fn le24(d: &[u8], i: usize) -> Result<u64, BadImage> {
    d.get(i..i + 3).map(|b| u32::from_le_bytes([b[0], b[1], b[2], 0]) as u64).ok_or(BadImage("truncated header"))
}
fn le32(d: &[u8], i: usize) -> Result<u64, BadImage> {
    d.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u64).ok_or(BadImage("truncated header"))
}

pub fn size(d: &[u8], k: &str) -> Result<(u64, u64), BadImage> {
    match k {
        "png" => {
            if d.get(12..16) != Some(b"IHDR") {
                return Err(BadImage("bad png"));
            }
            Ok((be32(d, 16)?, be32(d, 20)?))
        }
        "gif" => Ok((le16(d, 6)?, le16(d, 8)?)),
        "webp" => match d.get(12..16) {
            Some(b"VP8X") => Ok((le24(d, 24)? + 1, le24(d, 27)? + 1)),
            Some(b"VP8L") => {
                let b = le32(d, 21)?;
                Ok(((b & 0x3FFF) + 1, ((b >> 14) & 0x3FFF) + 1))
            }
            Some(b"VP8 ") => Ok((le16(d, 26)? & 0x3FFF, le16(d, 28)? & 0x3FFF)),
            _ => Err(BadImage("bad webp")),
        },
        _ => {
            let mut i = 2usize;
            for _ in 0..MAX_SEGMENTS {
                if i + 9 >= d.len() {
                    break;
                }
                if d[i] != 0xFF {
                    return Err(BadImage("bad jpeg"));
                }
                let m = d[i + 1];
                if m == 0xD8 || m == 0x01 || (0xD0..=0xD7).contains(&m) {
                    i += 2;
                    continue;
                }
                let ln = be16(d, i + 2)? as usize;
                if matches!(m, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
                    return Ok((be16(d, i + 7)?, be16(d, i + 5)?));
                }
                i += 2 + ln;
            }
            Err(BadImage("no jpeg frame header"))
        }
    }
}

/// Returns the mime type, or an error.
pub fn validate(d: &[u8], name: &str) -> Result<&'static str, BadImage> {
    let k = kind(d)?;
    let low = name.to_lowercase();
    let ok = match k {
        "png" => low.ends_with(".png"),
        "jpeg" => low.ends_with(".jpg") || low.ends_with(".jpeg"),
        "gif" => low.ends_with(".gif"),
        _ => low.ends_with(".webp"),
    };
    if !ok {
        return Err(BadImage("extension does not match file signature"));
    }
    let (w, h) = size(d, k)?;
    if w < 1 || h < 1 || w > MAX_SIDE || h > MAX_SIDE || w * h > MAX_PIXELS {
        return Err(BadImage("image dimensions out of range"));
    }
    Ok(match k {
        "png" => "image/png",
        "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        _ => "image/webp",
    })
}
