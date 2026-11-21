//! Fuzz target: sniff → metadata::extract → inspect/decode/encode across
//! every codec, never panicking or aborting on arbitrary input.
//!
//! A cheap per-format declared-dimension gate skips decodes of images whose
//! headers claim enormous pixel counts — those are allocation aborts, not
//! real bugs, and the CLI guards them separately via --max-pixels-mp.

#![no_main]

use core::codec::Codec;
use core::format::{sniff, ImageFormat};
use libfuzzer_sys::fuzz_target;
use webp_convert_core as core;

const MAX_DIM: u64 = 8192;

fn le16(b: &[u8]) -> u64 {
    u64::from(u16::from_le_bytes([b[0], b[1]]))
}
fn be16(b: &[u8]) -> u64 {
    u64::from(u16::from_be_bytes([b[0], b[1]]))
}
fn le24(b: &[u8]) -> u64 {
    u64::from(b[0]) | u64::from(b[1]) << 8 | u64::from(b[2]) << 16
}

fn dims_within(w: u64, h: u64) -> bool {
    // keep total pixel count bounded too: 8192*8192 here, much looser than
    // the CLI's --max-pixels-mp but small enough to never OOM the fuzzer
    w <= MAX_DIM && h <= MAX_DIM && w * h <= 16 * 1024 * 1024
}

/// Best-effort declared-dimension read; None = cannot tell (decode anyway).
fn declared_dims(fmt: ImageFormat, d: &[u8]) -> Option<(u64, u64)> {
    match fmt {
        ImageFormat::Png => {
            // IHDR: signature(8) + len(4) + "IHDR"(4) + width/height(8)
            if d.len() < 24 {
                return None;
            }
            let w = be16(&d[16..18]) as u64 * 0x1_0000 + be16(&d[18..20]) as u64;
            let h = be16(&d[20..22]) as u64 * 0x1_0000 + be16(&d[22..24]) as u64;
            Some((w, h))
        }
        ImageFormat::Gif => {
            if d.len() < 10 {
                return None;
            }
            Some((le16(&d[6..8]), le16(&d[8..10])))
        }
        ImageFormat::Bmp => {
            if d.len() < 26 {
                return None;
            }
            // DIB header: width i32 LE at 18, height i32 LE at 22
            let w = i32::from_le_bytes([d[18], d[19], d[20], d[21]]);
            let h = i32::from_le_bytes([d[22], d[23], d[24], d[25]]);
            Some((w.unsigned_abs() as u64, h.unsigned_abs() as u64))
        }
        ImageFormat::WebP => {
            // VP8X only (VP8/VP8L dimensions are structurally 14-bit)
            if d.len() < 30 || &d[12..16] != b"VP8X" {
                return None;
            }
            Some((le24(&d[24..27]) + 1, le24(&d[27..30]) + 1))
        }
        ImageFormat::Jpeg => {
            // scan segment headers for an SOF0-3/9-11 frame marker
            let mut i = 2usize;
            while i + 4 <= d.len() {
                if d[i] != 0xFF {
                    i += 1;
                    continue;
                }
                let m = d[i + 1];
                if (0xC0..=0xCE).contains(&m) && m != 0xC4 && m != 0xC8 && m != 0xCC {
                    if i + 9 > d.len() {
                        return None;
                    }
                    return Some((be16(&d[i + 7..i + 9]), be16(&d[i + 5..i + 7])));
                }
                if matches!(m, 0x01 | 0xD8 | 0xD9) || (0xD0..=0xD7).contains(&m) {
                    i += 2;
                    continue;
                }
                i += 2 + (le16(&d[i + 2..i + 4]).max(2) as usize);
            }
            None
        }
        ImageFormat::Tiff => {
            // walk header → first IFD → ImageWidth/ImageLength entries
            if d.len() < 8 {
                return None;
            }
            let big_endian = d[0] == b'M' && d[1] == b'M';
            if !big_endian && !(d[0] == b'I' && d[1] == b'I') {
                return None;
            }
            let u16r = |raw: [u8; 2]| {
                if big_endian {
                    u16::from_be_bytes(raw)
                } else {
                    u16::from_le_bytes(raw)
                }
            };
            let u32r = |raw: [u8; 4]| {
                if big_endian {
                    u32::from_be_bytes(raw)
                } else {
                    u32::from_le_bytes(raw)
                }
            };
            let off = u32r([d[4], d[5], d[6], d[7]]) as usize;
            if off + 2 > d.len() {
                return None;
            }
            let n = usize::from(u16r([d[off], d[off + 1]]));
            let mut w = None;
            let mut h = None;
            for k in 0..n {
                let e = off + 2 + k * 12;
                if e + 12 > d.len() {
                    return None;
                }
                match u16r([d[e], d[e + 1]]) {
                    256 => w = Some(u64::from(u32r([d[e + 8], d[e + 9], d[e + 10], d[e + 11]]))),
                    257 => h = Some(u64::from(u32r([d[e + 8], d[e + 9], d[e + 10], d[e + 11]]))),
                    _ => {}
                }
                if w.is_some() && h.is_some() {
                    break;
                }
            }
            match (w, h) {
                (Some(w), Some(h)) => Some((w, h)),
                _ => None,
            }
        }
    }
}

fn decode_allowed(fmt: ImageFormat, data: &[u8]) -> bool {
    match declared_dims(fmt, data) {
        Some((w, h)) => dims_within(w, h),
        None => true, // cannot tell: decoders must reject malformed input safely
    }
}

fuzz_target!(|data: &[u8]| {
    let fmt = sniff(data);

    // metadata extraction must never panic on any bytes
    if let Some(fmt) = fmt {
        let md = core::metadata::extract(data, fmt);
        // EXIF orientation patcher on the extracted payload
        if let Some(exif) = &md.exif {
            let _ = core::metadata::patch_exif_orientation_to_identity(exif);
        }
        let _ = core::metadata::patch_exif_orientation_to_identity(data);
    }

    let codecs: [&dyn Codec; 6] = [
        &codec_png::PngCodec,
        &codec_jpeg::JpegCodec,
        &codec_gif::GifCodec,
        &codec_tiff::TiffCodec,
        &codec_bmp::BmpCodec,
        &codec_webp::WebpCodec,
    ];

    for c in codecs {
        if !c.sniff(data) {
            continue;
        }
        // inspect must be safe on anything that sniffs — but only within
        // the size budget; GIF in particular composites frames during
        // inspection (see codec-gif MAX_INSPECT_PIXELS for the floor)
        if decode_allowed(c.format(), data) {
            let _ = c.inspect(data);
        }

        if !decode_allowed(c.format(), data) {
            continue;
        }

        if let Ok(decoded) = c.decode(data) {
            // round-trip into WebP with default options; encoders are
            // safe code paths worth exercising on adversarial pixels
            let opts = core::policy::EncodeOptions::for_target(ImageFormat::WebP);
            match decoded {
                core::model::Decoded::Still(s) => {
                    let _ = c.encode_still(&s, &opts);
                }
                core::model::Decoded::Animated(a) => {
                    if let Some(first) = a.frames.first() {
                        let _ = c.encode_still(&first.still, &opts);
                    }
                    let _ = c.encode_animated(&a, &opts);
                }
            }
        }
        let _ = c.decode_frame(data, 0);
        let _ = c.decode_frame(data, 1);
        let _ = c.decode_page(data, 0);
        let _ = c.decode_page(data, 1);
        if c.format() == ImageFormat::WebP {
            let _ = codec_webp::decode_animation(data);
        }
    }
});
