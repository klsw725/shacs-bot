/// Format and pixel size read from the file header (PNG, JPEG or WebP).
pub(crate) fn measure(bytes: &[u8]) -> Option<(&'static str, u32, u32)> {
    let be32 = |at: usize| {
        let b = bytes.get(at..at + 4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.get(12..16) == Some(b"IHDR".as_slice()) {
        return Some(("png", be32(16)?, be32(20)?));
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        return jpeg(bytes).map(|(width, height)| ("jpeg", width, height));
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP".as_slice()) {
        return webp(bytes).map(|(width, height)| ("webp", width, height));
    }
    None
}

fn jpeg(bytes: &[u8]) -> Option<(u32, u32)> {
    let be16 = |at: usize| u32::from(u16::from_be_bytes([bytes[at], bytes[at + 1]]));
    let mut at = 2;
    while at + 9 < bytes.len() {
        if bytes[at] != 0xff {
            return None;
        }
        let marker = bytes[at + 1];
        if marker == 0xff {
            at += 1;
            continue;
        }
        if matches!(marker, 0x01 | 0xd0..=0xd8) {
            at += 2;
            continue;
        }
        if matches!(marker, 0xc0..=0xcf) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
            return Some((be16(at + 7), be16(at + 5)));
        }
        at += 2 + be16(at + 2) as usize;
    }
    None
}

fn webp(bytes: &[u8]) -> Option<(u32, u32)> {
    let le = |at: usize, len: usize| {
        let b = bytes.get(at..at + len)?;
        Some(
            b.iter()
                .rev()
                .fold(0u32, |acc, byte| acc << 8 | u32::from(*byte)),
        )
    };
    match bytes.get(12..16)? {
        b"VP8 " => Some((le(26, 2)? & 0x3fff, le(28, 2)? & 0x3fff)),
        b"VP8L" => {
            let bits = le(21, 4)?;
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        b"VP8X" => Some((le(24, 3)? + 1, le(27, 3)? + 1)),
        _ => None,
    }
}
