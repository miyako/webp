//! WebP container muxing for metadata write-through (plan §5.6 / ADR-008).
//!
//! Adds ICCP / EXIF / `XMP ` chunks to an encoded WebP by byte-level RIFF
//! surgery, creating or updating the leading VP8X chunk with the WebP
//! container flags. All chunk sizes are little-endian u32 and bodies are
//! padded to even lengths, per the RIFF/WebP container spec. Sample order
//! rules: VP8X first, ICCP right after it, EXIF then `XMP ` at the end.

use webp_convert_core::error::{Result, WcError};
use webp_convert_core::metadata::RawMetadata;

const VP8X_ICC: u8 = 0x20;
const VP8X_ALPHA: u8 = 0x10;
const VP8X_EXIF: u8 = 0x08;
const VP8X_XMP: u8 = 0x04;
const VP8X_ANIM: u8 = 0x02;

/// Build a padded RIFF chunk ("FOURCC" + u32 LE size + body + pad).
fn chunk(fourcc: &str, body: &[u8]) -> Vec<u8> {
    let mut c = Vec::with_capacity(8 + body.len() + (body.len() & 1));
    c.extend_from_slice(fourcc.as_bytes());
    c.extend_from_slice(&(body.len() as u32).to_le_bytes());
    c.extend_from_slice(body);
    if body.len() & 1 == 1 {
        c.push(0);
    }
    c
}

/// Parse the top-level chunks of a "RIFF..WEBP" file (bodies copied).
fn split(encoded: &[u8]) -> Result<Vec<(&'static str, Vec<u8>)>> {
    if encoded.len() < 12 || &encoded[..4] != b"RIFF" || &encoded[8..12] != b"WEBP" {
        return Err(WcError::Internal(
            "cannot mux metadata into a malformed WebP container".into(),
        ));
    }
    let mut chunks = Vec::new();
    let mut i = 12usize;
    while i + 8 <= encoded.len() {
        let fourcc = &encoded[i..i + 4];
        let len = u32::from_le_bytes(encoded[i + 4..i + 8].try_into().unwrap()) as usize;
        let start = i + 8;
        if start + len > encoded.len() {
            return Err(WcError::Internal(
                "truncated RIFF chunk while muxing metadata".into(),
            ));
        }
        let name: &'static str = match fourcc {
            b"VP8X" => "VP8X",
            b"ICCP" => "ICCP",
            b"ALPH" => "ALPH",
            b"VP8 " => "VP8 ",
            b"VP8L" => "VP8L",
            b"ANIM" => "ANIM",
            b"ANMF" => "ANMF",
            b"EXIF" => "EXIF",
            b"XMP " => "XMP ",
            _ => "UNKNOWN",
        };
        // UNKNOWN top-level chunks (none are valid WebP) would lose meaning
        // on re-mux; carrying them through preserves bytes without dropping.
        chunks.push((name, encoded[start..start + len].to_vec()));
        i = start + len + (len & 1);
    }
    Ok(chunks)
}

/// 24-bit LE canvas dimension encoding for VP8X (value - 1).
fn le24(v: u32) -> [u8; 3] {
    [
        (v & 0xFF) as u8,
        ((v >> 8) & 0xFF) as u8,
        ((v >> 16) & 0xFF) as u8,
    ]
}

fn be24_from(b: &[u8]) -> u32 {
    u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16
}

/// Attach raw metadata to an encoded WebP (still or animated).
///
/// - If the encoded file has no VP8X, one is synthesized (canvas + flags).
/// - If it has one, the ICC/EXIF/XMP flag bits are updated and the canvas
///   kept from it.
/// - Any pre-existing ICCP/EXIF/XMP chunks are replaced (never duplicated).
/// - The `alpha`/`animation` flags of an existing VP8X are preserved.
pub fn with_metadata(
    encoded: &[u8],
    md: &RawMetadata,
    canvas: (u32, u32),
    has_alpha: bool,
) -> Result<Vec<u8>> {
    if !md.any() {
        return Ok(encoded.to_vec());
    }
    let mut chunks = split(encoded)?;
    let animated = chunks.iter().any(|(n, _)| *n == "ANIM");

    // rebuild (never duplicate) metadata chunks
    chunks.retain(|(n, _)| !matches!(*n, "ICCP" | "EXIF" | "XMP "));

    let (canvas, had_alpha) = match chunks.iter().position(|(n, _)| *n == "VP8X") {
        Some(idx) => {
            let body = chunks.remove(idx).1;
            if body.len() < 10 {
                return Err(WcError::Internal("malformed VP8X while muxing".into()));
            }
            let canvas = (be24_from(&body[4..7]) + 1, be24_from(&body[7..10]) + 1);
            let had_alpha = body[0] & VP8X_ALPHA != 0;
            (canvas, had_alpha)
        }
        None => (canvas, has_alpha),
    };

    let flags = {
        let mut f = if had_alpha || has_alpha {
            VP8X_ALPHA
        } else {
            0
        };
        if animated {
            f |= VP8X_ANIM;
        }
        if md.icc.is_some() {
            f |= VP8X_ICC;
        }
        if md.exif.is_some() {
            f |= VP8X_EXIF;
        }
        if md.xmp.is_some() {
            f |= VP8X_XMP;
        }
        f
    };
    let mut vp8x_body = Vec::with_capacity(10);
    vp8x_body.push(flags);
    vp8x_body.extend_from_slice(&[0, 0, 0]);
    vp8x_body.extend_from_slice(&le24(canvas.0.wrapping_sub(1)));
    vp8x_body.extend_from_slice(&le24(canvas.1.wrapping_sub(1)));

    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF\0\0\0\0WEBP");
    out.extend_from_slice(&chunk("VP8X", &vp8x_body));
    if let Some(icc) = &md.icc {
        out.extend_from_slice(&chunk("ICCP", icc));
    }
    for (name, body) in chunks {
        out.extend_from_slice(&chunk(name, &body));
    }
    if let Some(exif) = &md.exif {
        out.extend_from_slice(&chunk("EXIF", exif));
    }
    if let Some(xmp) = &md.xmp {
        out.extend_from_slice(&chunk("XMP ", xmp));
    }

    let riff_size = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&riff_size.to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_still() -> Vec<u8> {
        // VP8X + VP8L 1x1: canvas dims encoded from (-1, -1) + 1
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF\0\0\0\0WEBP");
        let flags = VP8X_ALPHA;
        let mut body = vec![flags, 0, 0, 0];
        body.extend_from_slice(&le24(0));
        body.extend_from_slice(&le24(0));
        v.extend_from_slice(&chunk("VP8X", &body));
        v.extend_from_slice(&chunk("VP8L", &[0x02]));
        let riff_size = (v.len() - 8) as u32;
        v[4..8].copy_from_slice(&riff_size.to_le_bytes());
        v
    }

    #[test]
    fn mux_round_trips_and_sets_flags() {
        let md = RawMetadata {
            icc: Some(vec![1, 2, 3, 4]),
            exif: Some(b"II*\0".to_vec()),
            xmp: Some(b"<xmp/>".to_vec()),
        };
        let out = with_metadata(&minimal_still(), &md, (1, 1), true).unwrap();
        assert_eq!(&out[8..12], b"WEBP");
        let chunks = split(&out).unwrap();
        let names: Vec<&'static str> = chunks.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, vec!["VP8X", "ICCP", "VP8L", "EXIF", "XMP "]);
        // flags carry all three bits
        assert_eq!(chunks[0].1[0], VP8X_ALPHA | VP8X_ICC | VP8X_EXIF | VP8X_XMP);
        // canvas preserved
        assert_eq!(be24_from(&chunks[0].1[4..7]), 0);
        // riff size correct
        assert_eq!(
            u32::from_le_bytes(out[4..8].try_into().unwrap()) as usize,
            out.len() - 8
        );
    }

    #[test]
    fn mux_replaces_existing_metadata_chunks() {
        let base = minimal_still();
        let once = with_metadata(
            &base,
            &RawMetadata {
                icc: Some(vec![1]),
                ..Default::default()
            },
            (1, 1),
            false,
        )
        .unwrap();
        let twice = with_metadata(
            &once,
            &RawMetadata {
                exif: Some(b"II*\0".to_vec()),
                ..Default::default()
            },
            (1, 1),
            false,
        )
        .unwrap();
        let chunks = split(&twice).unwrap();
        assert_eq!(chunks.iter().filter(|(n, _)| *n == "ICCP").count(), 0);
        assert_eq!(chunks.iter().filter(|(n, _)| *n == "EXIF").count(), 1);
        // ICC flag cleared when the category is absent
        assert_eq!(chunks[0].1[0] & VP8X_ICC, 0);
        assert_ne!(chunks[0].1[0] & VP8X_EXIF, 0);
    }
}
