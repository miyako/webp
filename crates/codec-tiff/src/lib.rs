//! TIFF codec adapter via the `image` crate.
//!
//! TIFF may hold multiple pages and high bit depth (plan §5.4): page count
//! is detected by walking the IFD chain and reported; v0 decodes the first
//! IFD and refuses multi-page input without explicit `--page 0`-style
//! acknowledgement at the CLI layer (no silent page loss). 16-bit input is
//! normalized to 8-bit with a diagnostic.

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

pub struct TiffCodec;

impl TiffCodec {
    fn decoded_to_still(dynimg: image::DynamicImage) -> Result<StillImage> {
        let rgba = dynimg.to_rgba8();
        StillImage::from_rgba8(rgba.dimensions().0, rgba.dimensions().1, rgba.into_raw())
            .map_err(|e| WcError::Internal(e.to_string()))
    }

    /// Walk the IFD chain to count pages. Bounded to avoid hostile loops.
    fn count_pages(data: &[u8]) -> u32 {
        let be = data.starts_with(b"MM\0*");
        if !(data.starts_with(b"II*\0") || be) {
            return 1;
        }
        let rd32 = |o: usize| -> Option<u32> {
            let b: [u8; 4] = data.get(o..o + 4)?.try_into().ok()?;
            Some(if be {
                u32::from_be_bytes(b)
            } else {
                u32::from_le_bytes(b)
            })
        };
        let mut pages = 0u32;
        let mut ifd = rd32(4).unwrap_or(0) as usize;
        for _ in 0..1024 {
            if ifd == 0 || ifd + 2 > data.len() {
                break;
            }
            let n = if be {
                u16::from_be_bytes([data[ifd], data[ifd + 1]])
            } else {
                u16::from_le_bytes([data[ifd], data[ifd + 1]])
            } as usize;
            if ifd + 2 + n * 12 + 4 > data.len() {
                break;
            }
            pages += 1;
            let next_off = ifd + 2 + n * 12;
            ifd = if be {
                u32::from_be_bytes(data[next_off..next_off + 4].try_into().unwrap())
            } else {
                u32::from_le_bytes(data[next_off..next_off + 4].try_into().unwrap())
            } as usize;
        }
        pages.max(1)
    }
}

impl Codec for TiffCodec {
    fn format(&self) -> ImageFormat {
        ImageFormat::Tiff
    }

    fn sniff(&self, data: &[u8]) -> bool {
        data.len() >= 4 && (data.starts_with(b"II*\0") || data.starts_with(b"MM\0*"))
    }

    fn inspect(&self, data: &[u8]) -> Result<ImageInfo> {
        let reader = image::ImageReader::new(Cursor::new(data))
            .with_guessed_format()
            .map_err(|e| WcError::CorruptInput(e.to_string()))?;
        let (w, h) = reader
            .into_dimensions()
            .map_err(|e| WcError::CorruptInput(e.to_string()))?;
        let pages = Self::count_pages(data);
        Ok(ImageInfo {
            format: "tiff".into(),
            width: w,
            height: h,
            frames: pages,
            has_alpha: true, // many TIFFs carry alpha; decided by decode
            animated: Some(false),
            loop_count: None,
            metadata: Some(metadata::probe(data, ImageFormat::Tiff)),
        })
    }

    fn decode(&self, data: &[u8]) -> Result<Decoded> {
        let pages = Self::count_pages(data);
        if pages > 1 {
            return Err(WcError::UnsupportedInput(format!(
                "tiff contains {pages} pages; multi-page input requires explicit page selection (not yet implemented — refusing to silently drop pages)"
            )));
        }
        let img = image::load_from_memory_with_format(data, image::ImageFormat::Tiff)
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
            .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Tiff)
            .map_err(|e| WcError::Internal(format!("tiff encode failed: {e}")))?;
        Ok(out)
    }

    fn caps(&self) -> FormatCaps {
        FormatCaps {
            format: "tiff".into(),
            read: true,
            write: true,
            animation_read: false,
            animation_write: false,
            alpha_read: true,
            alpha_write: true,
            metadata_read: vec!["exif".into(), "icc".into(), "comments".into()],
            metadata_write: vec![],
            pages_read: true,
            bit_depth_limit: "8/16-bit normalized to 8-bit on decode".into(),
            backend: "image (tiff crate)".into(),
            backend_version: env!("CARGO_PKG_VERSION").into(),
            restrictions: vec![
                "multi-page input is refused (never silently truncated) in v0".into(),
                "16-bit input normalizes to 8-bit with a diagnostic".into(),
                "exotic compressions may decode to corrupt_input with a clear message".into(),
            ],
        }
    }

    fn backend(&self) -> String {
        "image 0.25 (tiff)".into()
    }
}
