//! WebP codec adapter over vendored libwebp (statically linked).
//!
//! Engine rules captured here (ADR-002 / ADR-007):
//! - lossless output sets `exact = 1` so RGB under fully-transparent pixels
//!   survives (libwebp would otherwise rewrite it);
//! - lossless encoding uses the ARGB import path so no YUV round-trip
//!   quietly distorts colors;
//! - lossy output keeps libwebp defaults for exactness (documented lossy
//!   semantics) and preserves the alpha channel;
//! - `near_lossless` implicitly enables lossless (libwebp semantics); the
//!   CLI rejects that combination explicitly instead of guessing.

use std::mem;

use core::{
    codec::Codec,
    error::{Result, WcError},
    format::ImageFormat,
    model::{AnimatedImage, Decoded, ImageInfo, StillImage},
    policy::EncodeOptions,
    protocol::FormatCaps,
};
use webp_convert_core as core;

/// libwebp hard limits (encoder rejects larger).
pub const MAX_DIMENSION: u32 = 16_383;

pub struct WebpCodec;

impl WebpCodec {
    fn decoded_to_still(w: u32, h: u32, data: *mut u8) -> Result<StillImage> {
        if data.is_null() {
            return Err(WcError::CorruptInput("decoder returned no pixels".into()));
        }
        let len = w as usize * h as usize * 4;
        let rgba = unsafe { std::slice::from_raw_parts(data, len).to_vec() };
        unsafe { libwebp_sys::WebPFree(data.cast()) };
        StillImage::from_rgba8(w, h, rgba).map_err(|e| WcError::Internal(e.to_string()))
    }
}

impl Codec for WebpCodec {
    fn format(&self) -> ImageFormat {
        ImageFormat::WebP
    }

    fn sniff(&self, data: &[u8]) -> bool {
        data.len() >= 12 && &data[..4] == b"RIFF" && &data[8..12] == b"WEBP"
    }

    fn inspect(&self, data: &[u8]) -> Result<ImageInfo> {
        unsafe {
            let mut features: libwebp_sys::WebPBitstreamFeatures = mem::zeroed();
            let status = libwebp_sys::WebPGetFeatures(data.as_ptr(), data.len(), &mut features);
            // VP8_STATUS_OK == 0
            if status != 0 {
                return Err(WcError::CorruptInput(format!(
                    "libwebp could not parse the bitstream (status {status})"
                )));
            }
            let frames = if features.has_animation != 0 {
                count_frames(data)
            } else {
                1
            };
            Ok(ImageInfo {
                format: "webp".into(),
                width: features.width.max(0) as u32,
                height: features.height.max(0) as u32,
                frames,
                has_alpha: features.has_alpha != 0,
                animated: Some(features.has_animation != 0),
                // libwebp exposes loop count through the animation decoder;
                // webp-animation reports it with frame iteration
                loop_count: None,
                metadata: Some(core::metadata::probe(data, ImageFormat::WebP)),
            })
        }
    }

    fn decode(&self, data: &[u8]) -> Result<Decoded> {
        unsafe {
            let mut features: libwebp_sys::WebPBitstreamFeatures = mem::zeroed();
            let status = libwebp_sys::WebPGetFeatures(data.as_ptr(), data.len(), &mut features);
            if status != 0 {
                return Err(WcError::CorruptInput(format!(
                    "libwebp could not parse the bitstream (status {status})"
                )));
            }
            if features.has_animation != 0 {
                return Ok(Decoded::Animated(decode_animation(data)?));
            }
            let mut w: i32 = 0;
            let mut h: i32 = 0;
            let pixels = libwebp_sys::WebPDecodeRGBA(data.as_ptr(), data.len(), &mut w, &mut h);
            let still = Self::decoded_to_still(w.max(0) as u32, h.max(0) as u32, pixels)?;
            Ok(Decoded::Still(still))
        }
    }

    fn encode_still(&self, img: &StillImage, opts: &EncodeOptions) -> Result<Vec<u8>> {
        let (w, h) = (img.width, img.height);
        if w > MAX_DIMENSION || h > MAX_DIMENSION {
            return Err(WcError::ResourceLimit(format!(
                "webp encoders support at most {MAX_DIMENSION}x{MAX_DIMENSION} pixels, got {w}x{h}"
            )));
        }
        let lossless = opts.lossless || opts.near_lossless.unwrap_or(0) > 0;
        unsafe {
            let mut cfg: libwebp_sys::WebPConfig = mem::zeroed();
            if libwebp_sys::WebPConfigInit(&mut cfg) != 1 {
                return Err(WcError::Internal("WebPConfigInit failed".into()));
            }
            if lossless {
                cfg.lossless = 1;
                cfg.exact = 1; // ADR-002: pixel-exact lossless incl. transparent RGB
                cfg.quality = opts.quality.unwrap_or(75.0).clamp(0.0, 100.0);
            } else {
                cfg.quality = opts.quality.unwrap_or(80.0).clamp(0.0, 100.0);
                cfg.exact = 0;
            }
            if let Some(method) = opts.method {
                cfg.method = method.clamp(0, 6) as i32;
            }
            if let Some(aq) = opts.alpha_quality {
                cfg.alpha_quality = aq.clamp(0, 100) as i32;
            }
            if let Some(nl) = opts.near_lossless {
                cfg.near_lossless = nl.clamp(0, 100) as i32;
            }
            if libwebp_sys::WebPValidateConfig(&cfg) != 1 {
                return Err(WcError::InvalidArguments(
                    "webp encoder rejected the configuration (check quality/method/alpha-quality ranges)".into(),
                ));
            }
            let mut pic: libwebp_sys::WebPPicture = mem::zeroed();
            if libwebp_sys::WebPPictureInit(&mut pic) != 1 {
                return Err(WcError::Internal("WebPPictureInit failed".into()));
            }
            pic.width = w as i32;
            pic.height = h as i32;
            if lossless {
                pic.use_argb = 1; // no YUV round-trip for lossless
            }
            let mut writer: libwebp_sys::WebPMemoryWriter = mem::zeroed();
            libwebp_sys::WebPMemoryWriterInit(&mut writer);
            // SAFETY: WebPMemoryWrite is exported with C ABI; the sys crate
            // declares it `unsafe`, but it is an ordinary function pointer,
            // identical in layout to the safe WebPWriterFunction type.
            type WriteFn = unsafe extern "C" fn(
                *const u8,
                usize,
                *const libwebp_sys::WebPPicture,
            ) -> std::os::raw::c_int;
            let write_fn: WriteFn = libwebp_sys::WebPMemoryWrite;
            pic.writer = Some(mem::transmute::<
                WriteFn,
                extern "C" fn(
                    *const u8,
                    usize,
                    *const libwebp_sys::WebPPicture,
                ) -> std::os::raw::c_int,
            >(write_fn));
            pic.custom_ptr = (&mut writer as *mut libwebp_sys::WebPMemoryWriter).cast();

            let imported =
                libwebp_sys::WebPPictureImportRGBA(&mut pic, img.rgba.as_ptr(), (w * 4) as i32);
            let encoded = if imported == 1 {
                libwebp_sys::WebPEncode(&cfg, &mut pic)
            } else {
                0
            };
            let err_code = pic.error_code;
            libwebp_sys::WebPPictureFree(&mut pic);
            if imported == 0 || encoded == 0 {
                libwebp_sys::WebPMemoryWriterClear(&mut writer);
                return Err(internal_encode_error(err_code));
            }
            let out = std::slice::from_raw_parts(writer.mem, writer.size).to_vec();
            libwebp_sys::WebPMemoryWriterClear(&mut writer);
            Ok(out)
        }
    }

    fn caps(&self) -> FormatCaps {
        let v = unsafe { libwebp_sys::WebPGetEncoderVersion() };
        FormatCaps {
            format: "webp".into(),
            read: true,
            write: true,
            animation_read: true,
            animation_write: true,
            alpha_read: true,
            alpha_write: true,
            metadata_read: vec!["icc".into(), "exif".into(), "xmp".into()],
            metadata_write: vec![], // P1
            pages_read: false,
            bit_depth_limit: "8-bit RGBA".into(),
            backend: "libwebp".into(),
            backend_version: format!("0x{v:06X}"),
            restrictions: vec![
                "max encoder dimensions 16383x16383".into(),
                "lossless is pixel-exact only with exact=1 (engine sets it)".into(),
                "ICP/EXIF/XMP chunks are not copied to output in v0".into(),
            ],
        }
    }

    fn backend(&self) -> String {
        let v = unsafe { libwebp_sys::WebPGetEncoderVersion() };
        format!("libwebp 0x{v:06X} (static vendored)")
    }

    fn decode_frame(&self, data: &[u8], index: u32) -> Result<StillImage> {
        // Also works for still WebP as long as index == 0.
        decode_frame(data, index)
    }
}

fn internal_encode_error(code: u32) -> WcError {
    let what = match code {
        0 => "unknown encoding failure",
        1 => "out of memory",
        2 => "bitstream out of memory",
        3 => "null parameter",
        4 => "invalid configuration",
        5 => "bad dimension",
        6 => "partition0 overflow (too few bytes to encode)",
        7 => "partition overflow",
        8 => "bad write",
        _ => "encoding failed",
    };
    WcError::Internal(format!("libwebp encode error {code}: {what}"))
}

/// Frame count for animated WebP (also used by inspect).
pub(crate) fn count_frames(data: &[u8]) -> u32 {
    let dec = match webp_animation::Decoder::new(data) {
        Ok(d) => d,
        Err(_) => return 0,
    };
    dec.into_iter().count() as u32
}

/// Decode an animated WebP into the frame model (used by `--frame`
/// selection and future animation targets). webp-animation yields
/// composited full-canvas frames, so offsets are zero.
pub fn decode_animation(data: &[u8]) -> Result<AnimatedImage> {
    let dec = webp_animation::Decoder::new(data)
        .map_err(|e| WcError::CorruptInput(format!("animated webp decode failed: {e}")))?;
    let mut dims: Option<(u32, u32)> = None;
    let mut frames = Vec::new();
    let mut prev_ts = 0i32;
    for f in dec.into_iter() {
        match dims {
            None => dims = Some(f.dimensions()),
            Some(d) if f.dimensions() != d => {
                return Err(WcError::CorruptInput(
                    "animated webp frame size mismatch".into(),
                ));
            }
            _ => {}
        }
        let (fw, fh) = f.dimensions();
        let timestamp = f.timestamp();
        let duration_ms = (timestamp - prev_ts).max(0) as u32;
        prev_ts = timestamp;
        let img = f
            .into_image()
            .map_err(|e| WcError::Internal(format!("frame conversion failed: {e}")))?;
        let still = StillImage::from_rgba8(fw, fh, img.into_raw())
            .map_err(|e| WcError::Internal(e.to_string()))?;
        frames.push(core::model::Frame {
            still,
            duration_ms,
            offset_x: 0,
            offset_y: 0,
            blend: true,
            disposal: 0,
        });
    }
    let (width, height) =
        dims.ok_or_else(|| WcError::CorruptInput("animated webp contains no frames".into()))?;
    Ok(AnimatedImage {
        width,
        height,
        loop_count: None,
        frames,
    })
}

/// Decode frame `index` of an animated WebP as a still image.
pub fn decode_frame(data: &[u8], index: u32) -> Result<StillImage> {
    let dec = webp_animation::Decoder::new(data)
        .map_err(|e| WcError::CorruptInput(format!("animated webp decode failed: {e}")))?;
    dec.into_iter()
        .nth(index as usize)
        .ok_or_else(|| {
            WcError::InvalidArguments(format!(
                "frame index {index} is out of range for this animated webp"
            ))
        })?
        .into_image()
        .map_err(|e| WcError::Internal(format!("frame conversion failed: {e}")))
        .and_then(|img| {
            StillImage::from_rgba8(img.dimensions().0, img.dimensions().1, img.into_raw())
                .map_err(|e| WcError::Internal(e.to_string()))
        })
}

/// Test fixtures built with the real vendored encoder (used by CLI
/// integration tests). Not part of the public API.
#[doc(hidden)]
pub mod testing {
    use webp_convert_core::error::{Result, WcError};

    /// Write a 4x4 two-frame animated WebP (solid red frame 0, solid green
    /// frame 1, 200 ms apart) to `path`.
    pub fn write_two_frame_webp(path: &std::path::Path) -> Result<()> {
        let (w, h) = (4u32, 4u32);
        let red = [255u8, 0, 0, 255].repeat((w * h) as usize);
        let green = [0u8, 255, 0, 255].repeat((w * h) as usize);

        let mut enc = webp_animation::Encoder::new((w, h))
            .map_err(|e| WcError::Internal(format!("test encoder init failed: {e}")))?;
        enc.add_frame(&red, 0)
            .map_err(|e| WcError::Internal(format!("test add_frame failed: {e}")))?;
        enc.add_frame(&green, 200)
            .map_err(|e| WcError::Internal(format!("test add_frame failed: {e}")))?;
        let data = enc
            .finalize(400)
            .map_err(|e| WcError::Internal(format!("test finalize failed: {e}")))?;
        std::fs::write(path, &*data)
            .map_err(|e| WcError::Output(format!("test fixture write failed: {e}")))
    }
}
