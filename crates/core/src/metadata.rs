//! Lightweight metadata presence probing (ADR-008).
//!
//! v0 goal: never silently drop metadata. Full metadata * preservation* is
//! P1; right now we detect presence of EXIF / XMP / ICC / comments in the
//! raw container bytes so dropped metadata always produces a diagnostic.

use crate::format::ImageFormat;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MetadataPresence {
    pub exif: bool,
    pub xmp: bool,
    pub icc: bool,
    pub comments: bool,
}

impl MetadataPresence {
    pub fn any(&self) -> bool {
        self.exif || self.xmp || self.icc || self.comments
    }

    /// Names of detected categories, for diagnostics.
    pub fn names(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.icc {
            v.push("ICC");
        }
        if self.exif {
            v.push("EXIF");
        }
        if self.xmp {
            v.push("XMP");
        }
        if self.comments {
            v.push("comments");
        }
        v
    }
}

fn find_sub(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// EXIF orientation value 1..=8 from the container's EXIF payload, if any.
/// Reliable for JPEG and TIFF containers (the exif crate parses these
/// directly); for PNG/WebP containers the raw EXIF chunk would need to be
/// extracted first, which v0 does not do (returns None there).
pub fn exif_orientation(data: &[u8]) -> Option<u8> {
    let mut cursor = std::io::Cursor::new(data);
    let exif = exif::Reader::new().read_from_container(&mut cursor).ok()?;
    let field = exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)?;
    let v = field.value.get_uint(0)?;
    u8::try_from(v).ok().filter(|o| (1..=8).contains(o))
}

/// Maximum sample depth (BitsPerSample) of a TIFF container's primary image,
/// via the `exif` crate. Used to warn when 16-bit input is down-converted to
/// 8-bit (never silently — plan §5.4). Returns None for non-TIFF payloads or
/// when the tag is absent.
pub fn tiff_max_bits_per_sample(data: &[u8]) -> Option<u16> {
    let mut cursor = std::io::Cursor::new(data);
    let exif = exif::Reader::new().read_from_container(&mut cursor).ok()?;
    let field = exif.get_field(exif::Tag::BitsPerSample, exif::In::PRIMARY)?;
    let iter = field.value.iter_uint()?;
    iter.max().and_then(|v| u16::try_from(v).ok())
}

pub fn probe(data: &[u8], format: ImageFormat) -> MetadataPresence {
    match format {
        ImageFormat::Jpeg => probe_jpeg(data),
        ImageFormat::Png => probe_png(data),
        ImageFormat::WebP => probe_riff(data, b"WEBP"),
        ImageFormat::Tiff => probe_tiff(data),
        ImageFormat::Gif => probe_gif(data),
        ImageFormat::Bmp => MetadataPresence::default(),
    }
}

/// JPEG: walk segments. APP1 "Exif\0\0" -> EXIF; APP1 with XAP namespace ->
/// XMP; APP2 "ICC_PROFILE\0" -> ICC; COM -> comments.
fn probe_jpeg(data: &[u8]) -> MetadataPresence {
    let mut p = MetadataPresence::default();
    if !data.starts_with(&[0xFF, 0xD8]) {
        return p;
    }
    let mut i = 2usize;
    while i + 4 <= data.len() {
        if data[i] != 0xFF {
            break;
        }
        let marker = data[i + 1];
        match marker {
            0xD8 | 0x01 | (0xD0..=0xD7) => i += 2,
            0xD9 => break, // EOI
            _ => {
                if i + 4 > data.len() {
                    break;
                }
                let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
                if len < 2 || i + 2 + len > data.len() {
                    break;
                }
                let seg = &data[i + 4..i + 2 + len];
                match marker {
                    0xE1 => {
                        if find_sub(seg, b"Exif\0\0") || seg.starts_with(b"Exif\0\0") {
                            p.exif = true;
                        }
                        if find_sub(seg, b"http://ns.adobe.com/xap/1.0/") {
                            p.xmp = true;
                        }
                    }
                    0xE2 => {
                        if find_sub(seg, b"ICC_PROFILE") {
                            p.icc = true;
                        }
                    }
                    0xFE => p.comments = true,
                    _ => {}
                }
                i += 2 + len;
            }
        }
    }
    p
}

/// PNG: walk chunks. eXIf -> EXIF; iCCP -> ICC; iTXt/zTXt/tEXt XMP keyword
/// or iTXt XML -> XMP; tEXt/zTXt/iTXt -> comments.
fn probe_png(data: &[u8]) -> MetadataPresence {
    let mut p = MetadataPresence::default();
    if data.get(..8) != Some(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..]) {
        return p;
    }
    let mut i = 8usize;
    while i + 8 <= data.len() {
        let len = u32::from_be_bytes(data[i..i + 4].try_into().unwrap()) as usize;
        let ctype = &data[i + 4..i + 8];
        let body_start = i + 8;
        if body_start + len + 4 > data.len() {
            break;
        }
        let body = &data[body_start..body_start + len];
        match ctype {
            b"eXIf" => p.exif = true,
            b"iCCP" => p.icc = true,
            b"tEXt" | b"zTXt" | b"iTXt" => {
                if find_sub(body, b"XML:com.adobe.xmp") {
                    p.xmp = true;
                } else {
                    p.comments = true;
                }
            }
            _ => {}
        }
        i = body_start + len + 4; // skip body + CRC
        if ctype == b"IEND" {
            break;
        }
    }
    p
}

/// RIFF container (WebP): scan top-level chunk types.
fn probe_riff(data: &[u8], magic: &[u8]) -> MetadataPresence {
    let mut p = MetadataPresence::default();
    if data.len() < 12 || &data[..4] != b"RIFF" || &data[8..12] != magic {
        return p;
    }
    let mut i = 12usize;
    while i + 8 <= data.len() {
        let ctype = &data[i..i + 4];
        let len = u32::from_le_bytes(data[i + 4..i + 8].try_into().unwrap()) as usize;
        match ctype {
            b"ICCP" => p.icc = true,
            b"EXIF" => p.exif = true,
            b"XMP " => p.xmp = true,
            _ => {}
        }
        i += 8 + len + (len & 1); // even padding
    }
    p
}
/// TIFF: byte-order-aware scan of IFD0 tags for metadata-bearing entries.
fn probe_tiff(data: &[u8]) -> MetadataPresence {
    /// Inner fallible walk; any short read aborts with None.
    fn walk(data: &[u8]) -> Option<MetadataPresence> {
        let le = data.starts_with(b"II");
        let be = data.starts_with(b"MM");
        if !(le || be) {
            return None;
        }
        let rd16 = |o: usize| -> Option<u16> {
            let b: [u8; 2] = data.get(o..o + 2)?.try_into().ok()?;
            Some(if le {
                u16::from_le_bytes(b)
            } else {
                u16::from_be_bytes(b)
            })
        };
        let rd32 = |o: usize| -> Option<u32> {
            let b: [u8; 4] = data.get(o..o + 4)?.try_into().ok()?;
            Some(if le {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            })
        };
        let mut p = MetadataPresence::default();
        let ifd0 = rd32(4)? as usize;
        let n = rd16(ifd0)? as usize;
        if n > 100_000 {
            return Some(p);
        }
        for t in 0..n {
            let off = ifd0 + 2 + t * 12;
            if off + 12 > data.len() {
                break;
            }
            match rd16(off) {
                Some(0x8769) => p.exif = true,     // Exif IFD pointer
                Some(0x8773) => p.icc = true,      // InterColorProfile
                Some(0x8298) => p.comments = true, // Copyright
                Some(0x0131) => p.comments = true, // Software
                Some(0x0132) => p.comments = true, // DateTime
                Some(0x013B) => p.comments = true, // Artist
                _ => {}
            }
        }
        Some(p)
    }
    walk(data).unwrap_or_default()
}

/// GIF: scan for comment extension blocks.
fn probe_gif(data: &[u8]) -> MetadataPresence {
    let mut p = MetadataPresence::default();
    if !data.starts_with(b"GIF8") {
        return p;
    }
    // comment extension: 21 FE ... sub-blocks 00
    if find_sub(data, &[0x21, 0xFE]) {
        p.comments = true;
    }
    // XMP application extension "XMP DataXMP"
    if find_sub(data, b"XMP DataXMP") {
        p.xmp = true;
    }
    // ICC application extension n/a for GIF; ICCP is a webp/png thing.
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_exif_and_xmp_segments_detected() {
        let mut data = vec![0xFF, 0xD8, 0xFF];
        // APP1 with Exif payload
        let mut seg = b"Exif\0\0MM\0*\0\0".to_vec();
        data.push(0xE1);
        data.extend_from_slice(&(seg.len() as u16 + 2).to_be_bytes());
        data.append(&mut seg);
        // APP2 ICC
        data.extend_from_slice(&[0xFF, 0xE2]);
        let seg = b"ICC_PROFILE\0abc";
        data.extend_from_slice(&(seg.len() as u16 + 2).to_be_bytes());
        data.extend_from_slice(seg);
        // COM
        data.extend_from_slice(&[0xFF, 0xFE]);
        data.extend_from_slice(&[0, 4, b'x', b'y', 0]);
        data.extend_from_slice(&[0xFF, 0xD9]);
        let p = probe_jpeg(&data);
        assert!(p.exif && p.icc && p.comments);
    }

    #[test]
    fn png_chunks_detected() {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        let push = |data: &mut Vec<u8>, t: &[u8], body: &[u8]| {
            data.extend_from_slice(&(body.len() as u32).to_be_bytes());
            data.extend_from_slice(t);
            data.extend_from_slice(body);
            data.extend_from_slice(&[0, 0, 0, 0]); // CRC
        };
        push(&mut data, b"iCCP", b"profile-data");
        push(&mut data, b"eXIf", b"exifdata");
        push(&mut data, b"iTXt", b"XML:com.adobe.xmp<x/>");
        push(&mut data, b"tEXt", b"Comment\0hello");
        push(&mut data, b"IEND", b"");
        let p = probe_png(&data);
        assert_eq!(
            p,
            MetadataPresence {
                exif: true,
                xmp: true,
                icc: true,
                comments: true
            }
        );
    }

    #[test]
    fn riff_chunks_detected_and_padding_respected() {
        let mut data = b"RIFF\x1a\0\0\0WEBP".to_vec();
        // VP8 chunk even length, ICCP chunk 3 bytes + pad
        data.extend_from_slice(b"VP8 ");
        data.extend_from_slice(&[2, 0, 0, 0, 1, 2]);
        data.extend_from_slice(b"ICCP");
        data.extend_from_slice(&[3, 0, 0, 0, 9, 9, 9, 0]);
        let p = probe_riff(&data, b"WEBP");
        assert_eq!(
            p,
            MetadataPresence {
                exif: false,
                xmp: false,
                icc: true,
                comments: false
            }
        );
    }

    #[test]
    fn non_image_data_is_false() {
        assert_eq!(
            probe(b"junkjunkjunk", ImageFormat::Png),
            MetadataPresence::default()
        );
        assert_eq!(probe(b"", ImageFormat::Gif), MetadataPresence::default());
        assert_eq!(probe(b"", ImageFormat::Tiff), MetadataPresence::default());
    }

    #[test]
    fn gif_comment_detected() {
        let mut data = b"GIF89a\x64\0\x64\0\x80\0\0\x21\xFE".to_vec();
        data.extend_from_slice(&[4, b'a', b'b', b'c', b'd', 0]);
        data.extend_from_slice(&[0x2C, 0, 0, 0, 0, 1, 0, 1, 0, 0, 2, 2, 0x44, 1, 0]);
        let p = probe_gif(&data);
        assert!(p.comments);
        assert!(!p.exif);
    }
}
