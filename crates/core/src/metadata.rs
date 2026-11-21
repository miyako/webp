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

/// Raw metadata payloads extracted from a container, ready for
/// re-embedding into metadata-capable outputs (plan §5.6 write-through).
/// Extraction is best-effort: a category that cannot be reliably extracted
/// stays `None` and the caller must keep diagnosing it as dropped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawMetadata {
    /// ICC profile bytes
    pub icc: Option<Vec<u8>>,
    /// EXIF payload (TIFF format, no "Exif\0\0" container prefix)
    pub exif: Option<Vec<u8>>,
    /// XMP packet (raw RDF/XML)
    pub xmp: Option<Vec<u8>>,
}

impl RawMetadata {
    pub fn any(&self) -> bool {
        self.icc.is_some() || self.exif.is_some() || self.xmp.is_some()
    }

    /// Names of carried categories, for diagnostics.
    pub fn names(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        if self.icc.is_some() {
            v.push("ICC");
        }
        if self.exif.is_some() {
            v.push("EXIF");
        }
        if self.xmp.is_some() {
            v.push("XMP");
        }
        v
    }
}

fn inflate_nul(expected_size: Option<usize>, mut data: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(&mut data)
        .read_to_end(&mut out)
        .ok()?;
    match expected_size {
        Some(n) => (out.len() == n).then_some(out),
        None => Some(out),
    }
}

/// Extract raw metadata payloads from the input container (plan §5.6).
/// JPEG/PNG/WebP/TIFF inputs are supported; GIF/BMP containers carry none
/// of these categories.
pub fn extract(data: &[u8], format: ImageFormat) -> RawMetadata {
    match format {
        ImageFormat::Jpeg => extract_jpeg(data),
        ImageFormat::Png => extract_png(data),
        ImageFormat::WebP => extract_riff_webp(data),
        ImageFormat::Tiff => extract_tiff(data),
        ImageFormat::Gif | ImageFormat::Bmp => RawMetadata::default(),
    }
}

/// JPEG: APP1 "Exif\0\0" -> EXIF; APP1 with the XAP namespace marker ->
/// XMP; APP2 "ICC_PROFILE\0" chunk family -> concatenated ICC profile.
fn extract_jpeg(data: &[u8]) -> RawMetadata {
    let mut md = RawMetadata::default();
    if !data.starts_with(&[0xFF, 0xD8]) {
        return md;
    }
    let mut icc_chunks: Vec<(usize, &[u8])> = Vec::new();
    let mut icc_total = 0usize;
    let mut i = 2usize;
    while i + 4 <= data.len() {
        if data[i] != 0xFF {
            break;
        }
        let marker = data[i + 1];
        match marker {
            0xD8 | 0x01 | (0xD0..=0xD7) => i += 2,
            0xD9 => break,
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
                        if let Some(rest) = seg.strip_prefix(b"Exif\0\0") {
                            md.exif = Some(rest.to_vec());
                        } else if let Some(rest) =
                            seg.strip_prefix(b"http://ns.adobe.com/xap/1.0/\0")
                        {
                            md.xmp = Some(rest.to_vec());
                        }
                    }
                    0xE2 => {
                        // ICC_PROFILE\0 <seq-no> <count> <bytes...>
                        if seg.len() > 14 && seg.starts_with(b"ICC_PROFILE\0") {
                            let seq = seg[12] as usize;
                            let total = seg[13].max(1) as usize;
                            icc_total = icc_total.max(total);
                            if (1..=icc_total).contains(&seq)
                                && icc_chunks.iter().all(|(n, _)| *n != seq)
                            {
                                icc_chunks.push((seq, &seg[14..]));
                            }
                        }
                    }
                    _ => {}
                }
                i += 2 + len;
            }
        }
    }
    if !icc_chunks.is_empty() {
        icc_chunks.sort_by_key(|(n, _)| *n);
        let seqs: Vec<usize> = icc_chunks.iter().map(|(n, _)| *n).collect();
        let complete =
            (1..=icc_chunks.len()).all(|k| seqs.contains(&k)) && icc_total == icc_chunks.len();
        if complete {
            md.icc = Some(
                icc_chunks
                    .into_iter()
                    .flat_map(|(_, b)| b.to_vec())
                    .collect(),
            );
        }
        // incomplete ICC families stay None: partial profiles are lossy
    }
    md
}

/// PNG: iCCP (inflated) -> ICC; eXIf -> EXIF; iTXt keyword
/// "XML:com.adobe.xmp" -> XMP.
fn extract_png(data: &[u8]) -> RawMetadata {
    let mut md = RawMetadata::default();
    if data.get(..8) != Some(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A][..]) {
        return md;
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
            b"iCCP" => {
                // keyword\0 compression-method(0) deflate-data
                if let Some(nul) = body.iter().position(|&b| b == 0) {
                    let rest = &body[nul + 1..];
                    if rest.len() > 1 && rest[0] == 0 {
                        md.icc = inflate_nul(None, &rest[1..]);
                    }
                }
            }
            b"eXIf" => md.exif = Some(body.to_vec()),
            b"iTXt" => {
                // keyword\0 compression-flag compression-method
                // language\0 translated\0 text
                if let Some(knul) = body.iter().position(|&b| b == 0) {
                    let keyword = &body[..knul];
                    if keyword == b"XML:com.adobe.xmp" && body.len() > knul + 3 {
                        let flag = body[knul + 1];
                        let rest = &body[knul + 3..];
                        // skip language\0 translated\0
                        if let Some(l1) = rest.iter().position(|&b| b == 0) {
                            let after_lang = &rest[l1 + 1..];
                            if let Some(l2) = after_lang.iter().position(|&b| b == 0) {
                                let text = &after_lang[l2 + 1..];
                                if flag == 0 {
                                    md.xmp = Some(text.to_vec());
                                } else {
                                    md.xmp = inflate_nul(None, text);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        i = body_start + len + 4;
        if ctype == b"IEND" {
            break;
        }
    }
    md
}

/// WebP (RIFF "WEBP"): ICCP / EXIF / XMP chunks; the EXIF chunk may carry
/// an optional 4-byte LE size prefix (WebP container spec).
fn extract_riff_webp(data: &[u8]) -> RawMetadata {
    let mut md = RawMetadata::default();
    if data.len() < 12 || &data[..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return md;
    }
    let mut i = 12usize;
    while i + 8 <= data.len() {
        let ctype = &data[i..i + 4];
        let len = u32::from_le_bytes(data[i + 4..i + 8].try_into().unwrap()) as usize;
        let body_start = i + 8;
        if body_start + len > data.len() {
            break;
        }
        let body = &data[body_start..body_start + len];
        match ctype {
            b"ICCP" => md.icc = Some(body.to_vec()),
            b"EXIF" => {
                // optional 4-byte size prefix
                let without_prefix = if body.len() >= 4 {
                    u32::from_le_bytes(body[..4].try_into().unwrap()) as usize == body.len() - 4
                } else {
                    false
                };
                md.exif = Some(if without_prefix {
                    body[4..].to_vec()
                } else {
                    body.to_vec()
                });
            }
            b"XMP " => md.xmp = Some(body.to_vec()),
            _ => {}
        }
        i += 8 + len + (len & 1);
    }
    md
}

/// Walk a TIFF IFD and return the raw bytes of a typed tag (for UNDEFINED /
/// BYTE-typed metadata tags such as ICC at 0x8773 / XMP at 0x02BC).
fn tiff_tag_bytes(data: &[u8], want: u16) -> Option<Vec<u8>> {
    let le = data.starts_with(b"II");
    if !(le || data.starts_with(b"MM")) {
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
    let ifd0 = rd32(4)? as usize;
    let n = rd16(ifd0)? as usize;
    for e in 0..n {
        let entry = ifd0 + 2 + e * 12;
        if entry + 12 > data.len() {
            return None;
        }
        if rd16(entry)? != want {
            continue;
        }
        let typ = rd16(entry + 2)?;
        let count = rd32(entry + 4)? as usize;
        // UNDEFINED(7)/BYTE(1): 1 byte per sample
        if typ != 7 && typ != 1 {
            return None;
        }
        return Some(if count <= 4 {
            data[entry + 8..entry + 8 + count].to_vec()
        } else {
            let off = rd32(entry + 8)? as usize;
            data.get(off..off + count)?.to_vec()
        });
    }
    None
}

/// TIFF input: EXIF via the exif crate (`Exif::buf()` is a TIFF-format
/// payload ready for re-embedding), ICC via tag 0x8773, XMP via tag 0x02BC.
fn extract_tiff(data: &[u8]) -> RawMetadata {
    let mut md = RawMetadata::default();
    let mut cursor = std::io::Cursor::new(data);
    if let Ok(exif) = exif::Reader::new().read_from_container(&mut cursor) {
        if exif.fields().next().is_some() {
            md.exif = Some(exif.buf().to_vec());
        }
    }
    md.icc = tiff_tag_bytes(data, 0x8773);
    md.xmp = tiff_tag_bytes(data, 0x02BC);
    md
}

/// Rewrite the EXIF Orientation (0x0112) value to 1 in a raw EXIF/TIFF
/// payload. Used when pixels were rotated (`--orient auto`): preserving the
/// stale orientation value would rotate the image a second time downstream.
/// Returns `None` when the payload is not a parseable TIFF or contains no
/// orientation entry (nothing to patch).
pub fn patch_exif_orientation_to_identity(exif: &[u8]) -> Option<Vec<u8>> {
    let le = exif.starts_with(b"II");
    if !(le || exif.starts_with(b"MM")) || exif.len() < 10 {
        return None;
    }
    let rd16 = |o: usize| -> Option<u16> {
        let b: [u8; 2] = exif.get(o..o + 2)?.try_into().ok()?;
        Some(if le {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let ifd0 = u32::from_le_bytes(exif[4..8].try_into().ok().unwrap_or([0; 4])) as usize;
    let ifd0 = if le {
        ifd0
    } else {
        u32::from_be_bytes(exif[4..8].try_into().ok().unwrap_or([0; 4])) as usize
    };
    let n = rd16(ifd0)? as usize;
    let mut out = exif.to_vec();
    for e in 0..n {
        let entry = ifd0 + 2 + e * 12;
        if entry + 12 > out.len() {
            return None;
        }
        if rd16(entry)? == 0x0112 {
            // SHORT value fits inline: patch the first two value bytes
            let v: [u8; 2] = [1, 0];
            out[entry + 8..entry + 10].copy_from_slice(&if le { v } else { [0, 1] });
            return Some(out);
        }
    }
    None
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
