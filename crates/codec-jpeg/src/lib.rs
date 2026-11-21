//! JPEG codec adapter via the `image` crate.
//!
//! JPEG has no alpha channel (plan §5.2): decode yields opaque pixels, and
//! encode requires the policy layer to have flattened alpha explicitly
//! (`--background`). Flattening is *never* implicit — the CLI enforces it.

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

pub struct JpegCodec;

impl JpegCodec {
    fn decoded_to_still(dynimg: image::DynamicImage) -> Result<StillImage> {
        let rgba = dynimg.to_rgba8();
        StillImage::from_rgba8(rgba.dimensions().0, rgba.dimensions().1, rgba.into_raw())
            .map_err(|e| WcError::Internal(e.to_string()))
    }
}

impl Codec for JpegCodec {
    fn format(&self) -> ImageFormat {
        ImageFormat::Jpeg
    }

    fn sniff(&self, data: &[u8]) -> bool {
        data.len() >= 3 && data.starts_with(&[0xFF, 0xD8, 0xFF])
    }

    fn inspect(&self, data: &[u8]) -> Result<ImageInfo> {
        let reader = image::ImageReader::new(Cursor::new(data))
            .with_guessed_format()
            .map_err(|e| WcError::CorruptInput(e.to_string()))?;
        let (w, h) = reader
            .into_dimensions()
            .map_err(|e| WcError::CorruptInput(e.to_string()))?;
        Ok(ImageInfo {
            format: "jpeg".into(),
            width: w,
            height: h,
            frames: 1,
            has_alpha: false,
            animated: Some(false),
            loop_count: None,
            metadata: Some(metadata::probe(data, ImageFormat::Jpeg)),
        })
    }

    fn decode(&self, data: &[u8]) -> Result<Decoded> {
        let img = image::load_from_memory_with_format(data, image::ImageFormat::Jpeg)
            .map_err(|e| WcError::CorruptInput(e.to_string()))?;
        Ok(Decoded::Still(Self::decoded_to_still(img)?))
    }

    fn encode_still(&self, img: &StillImage, opts: &EncodeOptions) -> Result<Vec<u8>> {
        let quality = opts.quality.unwrap_or(85.0).clamp(0.0, 100.0) as u8;
        let dynimg = image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone())
                .ok_or_else(|| WcError::Internal("bad RGBA buffer".into()))?,
        );
        let mut out = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
        dynimg
            .write_with_encoder(enc)
            .map_err(|e| WcError::Internal(format!("jpeg encode failed: {e}")))?;
        Ok(out)
    }

    fn caps(&self) -> FormatCaps {
        FormatCaps {
            format: "jpeg".into(),
            read: true,
            write: true,
            animation_read: false,
            animation_write: false,
            alpha_read: false,
            alpha_write: false,
            metadata_read: vec!["exif".into(), "xmp".into(), "icc".into(), "comments".into()],
            metadata_write: vec![],
            pages_read: false,
            bit_depth_limit: "8-bit RGB(A); CMYK decoded to RGB".into(),
            backend: "image (zune-jpeg / jpeg-encoder)".into(),
            backend_version: env!("CARGO_PKG_VERSION").into(),
            restrictions: vec![
                "no alpha channel; alpha input requires explicit --background".into(),
                "quality 0-100 is JPEG-specific; not comparable with WebP quality".into(),
                "progressive encoding is not configurable in v0".into(),
            ],
        }
    }

    fn backend(&self) -> String {
        "image 0.25 (jpeg)".into()
    }
}
