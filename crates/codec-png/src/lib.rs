//! PNG codec adapter via the `image` crate.

use std::io::Cursor;

use core::{
    codec::Codec,
    error::{Result, WcError},
    format::ImageFormat,
    metadata,
    model::{Decoded, ImageInfo, StillImage},
    policy::EncodeOptions,
    protocol::FormatCaps,
};
use webp_convert_core as core;

pub struct PngCodec;

impl PngCodec {
    fn decoded_to_still(dynimg: image::DynamicImage) -> Result<StillImage> {
        let rgba = dynimg.to_rgba8();
        StillImage::from_rgba8(rgba.dimensions().0, rgba.dimensions().1, rgba.into_raw())
            .map_err(|e| WcError::Internal(e.to_string()))
    }
}

impl Codec for PngCodec {
    fn format(&self) -> ImageFormat {
        ImageFormat::Png
    }

    fn sniff(&self, data: &[u8]) -> bool {
        data.len() >= 8 && data.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
    }

    fn inspect(&self, data: &[u8]) -> Result<ImageInfo> {
        let reader = image::ImageReader::new(Cursor::new(data))
            .with_guessed_format()
            .map_err(|e| WcError::CorruptInput(e.to_string()))?;
        let (w, h) = reader
            .into_dimensions()
            .map_err(|e| WcError::CorruptInput(e.to_string()))?;
        // detect animation (APNG read): the image crate exposes APNG frames
        // through GifDecoder-style APIs only for GIF; APNG = a sequence of
        // PNGs in the same stream that decodes to its first frame today.
        let frames = 1;
        Ok(ImageInfo {
            format: "png".into(),
            width: w,
            height: h,
            frames,
            has_alpha: has_alpha_channel(data),
            animated: Some(false),
            loop_count: None,
            metadata: Some(metadata::probe(data, ImageFormat::Png)),
        })
    }

    fn decode(&self, data: &[u8]) -> Result<Decoded> {
        let img = image::load_from_memory_with_format(data, image::ImageFormat::Png)
            .map_err(|e| WcError::CorruptInput(e.to_string()))?;
        Ok(Decoded::Still(Self::decoded_to_still(img)?))
    }

    fn encode_still(&self, img: &StillImage, _opts: &EncodeOptions) -> Result<Vec<u8>> {
        let dynimg = image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone())
                .ok_or_else(|| WcError::Internal("bad RGBA buffer".into()))?,
        );
        let mut out = Vec::new();
        dynimg
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
            .map_err(|e| WcError::Internal(format!("png encode failed: {e}")))?;
        Ok(out)
    }

    fn caps(&self) -> FormatCaps {
        FormatCaps {
            format: "png".into(),
            read: true,
            write: true,
            animation_read: false, // APNG frames read as first frame in v0
            animation_write: false,
            alpha_read: true,
            alpha_write: true,
            metadata_read: vec!["icc".into(), "exif".into(), "xmp".into(), "comments".into()],
            metadata_write: vec![],
            pages_read: false,
            bit_depth_limit: "8/16-bit normalized to 8-bit on decode".into(),
            backend: "image (image-png)".into(),
            backend_version: env!("CARGO_PKG_VERSION").into(),
            restrictions: vec![
                "APNG input decodes to its first frame (documented loss)".into(),
                "16-bit input normalizes to 8-bit (diagnosed)".into(),
                "metadata is detected and reported as dropped (P1 write-through)".into(),
            ],
        }
    }

    fn backend(&self) -> String {
        "image 0.25 (png)".into()
    }
}

/// Cheap alpha detection: scan for color-types with alpha in the IHDR.
fn has_alpha_channel(data: &[u8]) -> bool {
    // IHDR is always the first chunk (offset 8..29): width height depth color
    if data.len() < 29 || &data[12..16] != b"IHDR" {
        return false;
    }
    // cheap alpha detection: IHDR color type (offset 25): 0=gray, 2=rgb,
    // 3=palette, 4=gray+alpha, 6=rgba
    if data.len() < 26 || &data[12..16] != b"IHDR" {
        return false;
    }
    matches!(data[25], 4 | 6)
}

#[allow(dead_code)]
fn seek_frames_probe(data: &[u8]) -> Result<u32> {
    // reference for later APNG work; keeps AnimationDecoder import honest
    let dec = image::codecs::png::PngDecoder::new(Cursor::new(data))
        .map_err(|e| WcError::CorruptInput(e.to_string()))?;
    let _ = dec.is_apng().unwrap_or(false);
    Ok(1)
}
