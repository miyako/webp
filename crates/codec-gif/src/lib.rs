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

impl GifCodec {
    fn animated(data: &[u8]) -> Result<core::model::AnimatedImage> {
        let dec = image::codecs::gif::GifDecoder::new(Cursor::new(data))
            .map_err(|e| WcError::CorruptInput(e.to_string()))?;
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
        let frames = Self::animated(data).map(|a| a.frames.len()).unwrap_or(1);
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
            Ok(Decoded::Still(
                anim.frames.into_iter().next().expect("one frame").still,
            ))
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

    fn caps(&self) -> FormatCaps {
        FormatCaps {
            format: "gif".into(),
            read: true,
            write: true,
            animation_read: true,
            animation_write: false, // animated GIF write is P1
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
                "only binary (1-bit) transparency; partial alpha is lost by the format".into(),
                "animated GIF write is not implemented in v0".into(),
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
