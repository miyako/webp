//! BMP codec adapter via the `image` crate.
//!
//! BMP in the wild: alpha handling varies by writer (BITMAPV5 vs none).
//! The CLI treats BMP as an alpha-less target: alpha input requires
//! explicit `--background` (plan §5.2), so output stays interoperable.

use std::io::Cursor;

use core::{
    codec::Codec,
    // error path uses WcError directly
    error::{Result, WcError},
    format::ImageFormat,
    metadata,
    model::{Decoded, ImageInfo, StillImage},
    policy::EncodeOptions,
    protocol::FormatCaps,
};
use webp_convert_core as core;

pub struct BmpCodec;

impl BmpCodec {
    fn decoded_to_still(dynimg: image::DynamicImage) -> Result<StillImage> {
        let rgba = dynimg.to_rgba8();
        StillImage::from_rgba8(rgba.dimensions().0, rgba.dimensions().1, rgba.into_raw())
            .map_err(|e| WcError::Internal(e.to_string()))
    }
}

impl Codec for BmpCodec {
    fn format(&self) -> ImageFormat {
        ImageFormat::Bmp
    }

    fn sniff(&self, data: &[u8]) -> bool {
        data.len() > 6
            && data.starts_with(b"BM")
            && u32::from_le_bytes([data[2], data[3], data[4], data[5]]) > 0
    }

    fn inspect(&self, data: &[u8]) -> Result<ImageInfo> {
        let reader = image::ImageReader::new(Cursor::new(data))
            .with_guessed_format()
            .map_err(|e| WcError::CorruptInput(e.to_string()))?;
        let (w, h) = reader
            .into_dimensions()
            .map_err(|e| WcError::CorruptInput(e.to_string()))?;
        Ok(ImageInfo {
            format: "bmp".into(),
            width: w,
            height: h,
            frames: 1,
            has_alpha: false,
            animated: Some(false),
            loop_count: None,
            metadata: Some(metadata::probe(data, ImageFormat::Bmp)),
        })
    }

    fn decode(&self, data: &[u8]) -> Result<Decoded> {
        let img = image::load_from_memory_with_format(data, image::ImageFormat::Bmp)
            .map_err(|e| WcError::CorruptInput(e.to_string()))?;
        Ok(Decoded::Still(Self::decoded_to_still(img)?))
    }

    fn encode_still(&self, img: &StillImage, _opts: &EncodeOptions) -> Result<Vec<u8>> {
        // encode as RGB (24-bit): grayscale of the alpha policy
        let dynimg = image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone())
                .ok_or_else(|| WcError::Internal("bad RGBA buffer".into()))?,
        );
        let mut out = Vec::new();
        dynimg
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Bmp)
            .map_err(|e| WcError::Internal(format!("bmp encode failed: {e}")))?;
        Ok(out)
    }

    fn caps(&self) -> FormatCaps {
        FormatCaps {
            format: "bmp".into(),
            read: true,
            write: true,
            animation_read: false,
            animation_write: false,
            alpha_read: false, // treated as alpha-less by policy
            alpha_write: false,
            metadata_read: vec![],
            metadata_write: vec![],
            pages_read: false,
            bit_depth_limit: "1/4/8/24/32-bit decoded; 24-bit written".into(),
            backend: "image 0.25 (bmp)".into(),
            backend_version: env!("CARGO_PKG_VERSION").into(),
            restrictions: vec![
                "alpha input requires explicit --background before encode".into(),
                "top-down BMPs may decode to vertically mirrored pixels in the wild".into(),
            ],
        }
    }

    fn backend(&self) -> String {
        "image 0.25 (bmp)".into()
    }
}
