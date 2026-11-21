//! GIF codec adapter via the `image` and `gif` crates.
//!
//! GIF restrictions (plan §5.2/§5.4): 256-color palette, 1-bit transparency,
//! per-frame rectangles. Encoding a full-RGBA still quantizes color; the
//! codec reports this with a `gif_palette_quantized` diagnostic and never
//! pretends to be lossless.

use std::io::Cursor;

use image::{AnimationDecoder, ImageDecoder};

use core::{
    codec::Codec,
    error::{Result, WcError},
    format::ImageFormat,
    metadata,
    model::{Decoded, Frame, ImageInfo, StillImage},
    policy::EncodeOptions,
    protocol::FormatCaps,
};
use webp_convert_core as core;

pub struct GifCodec;

/// Absolute canvas budget used to guard GIF inspection before any frame
/// compositing happens. The `image` crate composites each frame onto a
/// full logical-screen buffer; a header alone can request 65535x65535
/// (~17 GB), which must fail fast, not OOM. This is a safety ceiling
/// independent of the CLI's --max-pixels-mp flag (which is checked after
/// inspect). Normal GIFs stay far below it.
const MAX_INSPECT_PIXELS: u64 = 64 * 1024 * 1024;

impl GifCodec {
    fn animated(data: &[u8]) -> Result<core::model::AnimatedImage> {
        // cheap header check before the decoder composites any frame
        if data.len() >= 10 {
            let w = u64::from(u16::from_le_bytes([data[6], data[7]]));
            let h = u64::from(u16::from_le_bytes([data[8], data[9]]));
            if w * h > MAX_INSPECT_PIXELS {
                return Err(WcError::ResourceLimit(format!(
                    "GIF canvas {w}x{h} ({p} pixels) exceeds the {m} pixel safety limit",
                    p = w * h,
                    m = MAX_INSPECT_PIXELS
                )));
            }
        }
        // the header gate above is cheap; the decoder itself allocates the
        // full composite canvas per frame, so also hand it explicit limits
        // (GifDecoder defaults to no_limits(); ImageReader's 512 MiB default
        // is what the other codecs already get via load_from_memory)
        let mut dec = image::codecs::gif::GifDecoder::new(Cursor::new(data))
            .map_err(|e| WcError::CorruptInput(e.to_string()))?;
        dec.set_limits(image::Limits::default())
            .map_err(|e| WcError::ResourceLimit(format!("GIF exceeds decode limits: {e}")))?;
        let (w, h) = (dec.dimensions().0, dec.dimensions().1);
        let mut frames = Vec::new();
        for f in dec.into_frames() {
            let fr = f.map_err(|e| WcError::CorruptInput(e.to_string()))?;
            // frame buffer is canvas-size and composited in the image crate
            let (fw, fh) = fr.buffer().dimensions();
            let still = StillImage::from_rgba8(fw, fh, fr.buffer().clone().into_raw())
                .map_err(|e| WcError::Internal(e.to_string()))?;
            // Delay is in milliseconds; GIF delays quantize to 1/100 s
            let duration_ms = fr.delay().numer_denom_ms().0;
            frames.push(Frame {
                still,
                duration_ms,
                offset_x: fr.left(),
                offset_y: fr.top(),
                blend: true,
                disposal: 0,
            });
        }
        Ok(core::model::AnimatedImage {
            width: w,
            height: h,
            loop_count: None,
            frames,
        })
    }
}

impl Codec for GifCodec {
    fn format(&self) -> ImageFormat {
        ImageFormat::Gif
    }

    fn sniff(&self, data: &[u8]) -> bool {
        data.len() >= 6 && (data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a"))
    }

    fn inspect(&self, data: &[u8]) -> Result<ImageInfo> {
        let reader = image::ImageReader::new(Cursor::new(data))
            .with_guessed_format()
            .map_err(|e| WcError::CorruptInput(e.to_string()))?;
        let (w, h) = reader
            .into_dimensions()
            .map_err(|e| WcError::CorruptInput(e.to_string()))?;
        // inspect needs the frame count from a real composite pass; a
        // resource-limit finding must propagate, not degrade to "1 frame"
        let frames = match Self::animated(data) {
            Ok(a) => a.frames.len(),
            Err(e @ WcError::ResourceLimit(_)) => return Err(e),
            Err(_) => 1,
        };
        Ok(ImageInfo {
            format: "gif".into(),
            width: w,
            height: h,
            frames: frames.max(1) as u32,
            has_alpha: true, // transparency possible globally (1-bit)
            animated: Some(frames > 1),
            loop_count: None,
            metadata: Some(metadata::probe(data, ImageFormat::Gif)),
        })
    }

    fn decode(&self, data: &[u8]) -> Result<Decoded> {
        let anim = Self::animated(data)?;
        if anim.frames.len() > 1 {
            Ok(Decoded::Animated(anim))
        } else {
            // a GIF whose only "frame" fails to decode composites to an
            // empty list here; that is corrupt input, not a panic
            let f =
                anim.frames.into_iter().next().ok_or_else(|| {
                    WcError::CorruptInput("GIF carries no decodable frames".into())
                })?;
            Ok(Decoded::Still(f.still))
        }
    }

    /// GIF has no alpha flattening concern (1-bit transparency); full-color
    /// input is quantized to a 256-color adaptive palette.
    fn encode_still(&self, img: &StillImage, _opts: &EncodeOptions) -> Result<Vec<u8>> {
        let rgba = image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone())
            .ok_or_else(|| WcError::Internal("bad RGBA buffer".into()))?;
        let mut out = Vec::new();
        {
            let mut enc = image::codecs::gif::GifEncoder::new(&mut out);
            enc.encode_frame(image::Frame::new(rgba))
                .map_err(|e| WcError::Internal(format!("gif encode failed: {e}")))?;
        }
        Ok(out)
    }

    /// Encode an animation: full-canvas RGBA frames quantized per frame
    /// (256 colors, 1-bit transparency) by the image crate's GIF encoder.
    /// Timing maps ms → GIF's 1/100 s units (rounding up, min 1 unit);
    /// some duration information below 10 ms granularity is unavoidably
    /// lost by the format — declared, not silent.
    fn encode_animated(
        &self,
        anim: &core::model::AnimatedImage,
        _opts: &EncodeOptions,
    ) -> Result<Vec<u8>> {
        if anim.frames.is_empty() {
            return Err(WcError::Internal(
                "cannot encode an animation with no frames".into(),
            ));
        }
        let (w, h) = (anim.width, anim.height);
        let mut out = Vec::new();
        {
            let mut enc = image::codecs::gif::GifEncoder::new(&mut out);
            enc.set_repeat(image::codecs::gif::Repeat::Infinite)
                .map_err(|e| WcError::Internal(format!("gif repeat setup failed: {e}")))?;
            for f in &anim.frames {
                let buffer = image::RgbaImage::from_raw(w, h, f.still.rgba.clone())
                    .ok_or_else(|| WcError::Internal("animation frame buffer mismatch".into()))?;
                let fr = image::Frame::from_parts(
                    buffer,
                    0,
                    0,
                    image::Delay::from_numer_denom_ms(f.duration_ms.max(10), 1),
                );
                enc.encode_frame(fr)
                    .map_err(|e| WcError::Internal(format!("gif animation encode: {e}")))?;
            }
        }
        Ok(out)
    }

    fn caps(&self) -> FormatCaps {
        FormatCaps {
            format: "gif".into(),
            read: true,
            write: true,
            animation_read: true,
            animation_write: true,
            alpha_read: true,
            alpha_write: true,
            metadata_read: vec!["xmp".into(), "comments".into()],
            metadata_write: vec![],
            pages_read: false,
            bit_depth_limit: "8-bit indexed (256 colors), 1-bit alpha".into(),
            backend: "image 0.25 + gif 0.14".into(),
            backend_version: env!("CARGO_PKG_VERSION").into(),
            restrictions: vec![
                "256-color palette; RGBA stills are quantized (diagnosed)".into(),
                "only binary (1-bit) transparency; partial alpha is lost by the format (diagnosed)"
                    .into(),
                "timing quantizes to 1/100 s units; delays under 10 ms become one unit".into(),
                "animated output loops infinitely (input loop count is not exposed by the decoder)"
                    .into(),
            ],
        }
    }

    fn backend(&self) -> String {
        "image 0.25 (gif + gif crate)".into()
    }

    /// GIF frames are sub-rectangles; compose each onto its canvas by hand.
    fn decode_frame(&self, data: &[u8], index: u32) -> Result<StillImage> {
        let anim = Self::animated(data)?;
        let frame = anim
            .frames
            .get(index as usize)
            .ok_or_else(|| {
                WcError::InvalidArguments(format!(
                    "frame index {} is out of range (that GIF has {} frame(s))",
                    index,
                    anim.frames.len()
                ))
            })?
            .clone();
        Ok(frame.still)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression (fuzzing, phase 6): a 20-byte header claiming a 65535x65535
    /// logical screen must hit the inspection safety ceiling, never the
    /// 17 GB composite allocation.
    #[test]
    fn oversized_canvas_is_a_resource_limit_before_compositing() {
        let mut data = b"GIF89a".to_vec();
        data.extend_from_slice(&0xFFFFu16.to_le_bytes()); // logical width
        data.extend_from_slice(&0xFFFFu16.to_le_bytes()); // logical height
        data.extend_from_slice(b"\0garbage");
        let err = GifCodec.inspect(&data).unwrap_err();
        assert!(matches!(err, WcError::ResourceLimit(_)), "got: {err}");
        assert_eq!(err.exit_code(), 4);
    }
}
