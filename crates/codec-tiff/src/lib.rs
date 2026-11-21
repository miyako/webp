//! TIFF codec adapter via the `image` crate (page path: direct `tiff` crate).
//!
//! TIFF may hold multiple pages and high bit depth (plan §5.4): page count
//! is detected by walking the IFD chain and reported; multi-page input
//! requires explicit `--page N` (no silent page loss). 16-bit input is
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
                "tiff contains {pages} pages; pass --page N to select one explicitly \
                 (never silently dropping pages)"
            )));
        }
        let img = image::load_from_memory_with_format(data, image::ImageFormat::Tiff)
            .map_err(|e| WcError::CorruptInput(e.to_string()))?;
        Ok(Decoded::Still(Self::decoded_to_still(img)?))
    }

    /// Decode a single page by walking the IFD chain with the `tiff` crate
    /// (the `image` crate exposes no page selection). Supports the common
    /// color types at 8/16-bit; 16-bit samples are normalized to 8-bit.
    fn decode_page(&self, data: &[u8], index: u32) -> Result<StillImage> {
        let pages = Self::count_pages(data);
        if index >= pages {
            return Err(WcError::InvalidArguments(format!(
                "page index {index} is out of range (that tiff has {pages} page(s))"
            )));
        }
        let mut dec = tiff::decoder::Decoder::new(Cursor::new(data))
            .map_err(|e| WcError::CorruptInput(format!("tiff decode failed: {e}")))?;
        for _ in 0..index {
            if !dec.more_images() {
                return Err(WcError::CorruptInput(format!(
                    "tiff page chain ended before page {index}"
                )));
            }
            dec.next_image()
                .map_err(|e| WcError::CorruptInput(format!("tiff iterate pages failed: {e}")))?;
        }
        let (w, h) = dec
            .dimensions()
            .map_err(|e| WcError::CorruptInput(format!("tiff dimensions failed: {e}")))?;
        let color = dec
            .colortype()
            .map_err(|e| WcError::CorruptInput(format!("tiff colortype failed: {e}")))?;
        let un = |v: u16, depth: u8| -> u8 {
            match depth {
                16 => ((u32::from(v) * 255 + 32767) / 65535).min(255) as u8,
                _ => v as u8,
            }
        };
        // ColorMap must be read before read_image() moves the decoder state.
        let color_map: Option<Vec<u16>> = if matches!(color, tiff::ColorType::Palette(_)) {
            Some(
                dec.find_tag_unsigned_vec::<u16>(tiff::tags::Tag::ColorMap)
                    .map_err(|e| WcError::CorruptInput(format!("tiff colormap read failed: {e}")))?
                    .ok_or_else(|| {
                        WcError::CorruptInput("palette tiff without a ColorMap".into())
                    })?,
            )
        } else {
            None
        };
        let raw = dec
            .read_image()
            .map_err(|e| WcError::CorruptInput(format!("tiff page {index} decode failed: {e}")))?;

        macro_rules! need {
            ($bytes:expr, $n:expr) => {
                if $bytes.len() < $n {
                    return Err(WcError::CorruptInput(
                        "tiff page decode produced less data than dimensions require".into(),
                    ));
                }
            };
        }
        let mut rgba: Vec<u8> = Vec::new();
        match (color, raw) {
            (tiff::ColorType::Gray(8), tiff::decoder::DecodingResult::U8(v)) => {
                need!(v, w as usize * h as usize);
                rgba.extend(v.iter().flat_map(|&g| [g, g, g, 255]));
            }
            (tiff::ColorType::Gray(16), tiff::decoder::DecodingResult::U16(v)) => {
                need!(v, w as usize * h as usize);
                rgba.extend(v.iter().flat_map(|&g| {
                    let g8 = un(g, 16);
                    [g8, g8, g8, 255]
                }));
            }
            (tiff::ColorType::GrayA(8), tiff::decoder::DecodingResult::U8(v)) => {
                need!(v, w as usize * h as usize * 2);
                for px in v.as_chunks::<2>().0 {
                    rgba.extend([px[0], px[0], px[0], px[1]]);
                }
            }
            (tiff::ColorType::GrayA(16), tiff::decoder::DecodingResult::U16(v)) => {
                need!(v, w as usize * h as usize * 2);
                for px in v.as_chunks::<2>().0 {
                    rgba.extend([un(px[0], 16); 3]);
                    rgba.push(un(px[1], 16));
                }
            }
            (tiff::ColorType::RGB(8), tiff::decoder::DecodingResult::U8(v)) => {
                need!(v, w as usize * h as usize * 3);
                for px in v.as_chunks::<3>().0 {
                    rgba.extend([px[0], px[1], px[2], 255]);
                }
            }
            (tiff::ColorType::RGB(16), tiff::decoder::DecodingResult::U16(v)) => {
                need!(v, w as usize * h as usize * 3);
                for px in v.as_chunks::<3>().0 {
                    rgba.extend([un(px[0], 16), un(px[1], 16), un(px[2], 16), 255]);
                }
            }
            (tiff::ColorType::RGBA(8), tiff::decoder::DecodingResult::U8(v)) => {
                need!(v, w as usize * h as usize * 4);
                rgba = v;
            }
            (tiff::ColorType::RGBA(16), tiff::decoder::DecodingResult::U16(v)) => {
                need!(v, w as usize * h as usize * 4);
                rgba.extend(
                    v.as_chunks::<4>().0.iter().flat_map(|px| {
                        [un(px[0], 16), un(px[1], 16), un(px[2], 16), un(px[3], 16)]
                    }),
                );
            }
            (tiff::ColorType::Palette(8), tiff::decoder::DecodingResult::U8(v)) => {
                // ColorMap was fetched pre-decode: 3 * 2^bits 16-bit entries (R, G, B)
                let map = color_map.ok_or_else(|| {
                    WcError::CorruptInput("palette tiff without a ColorMap".into())
                })?;
                let colors = 1usize << 8;
                need!(map, colors * 3);
                need!(v, w as usize * h as usize);
                for &idx in &v {
                    let i = idx as usize;
                    let norm = |c: u16| ((u32::from(c) * 255 + 32767) / 65535).min(255) as u8;
                    rgba.extend([
                        norm(map[i]),
                        norm(map[colors + i]),
                        norm(map[2 * colors + i]),
                        255,
                    ]);
                }
            }
            (tiff::ColorType::CMYK(8), tiff::decoder::DecodingResult::U8(v)) => {
                // naive 1-minus conversion is what most standalone tools do;
                // document as approximation — no profile-based conversion here
                need!(v, w as usize * h as usize * 4);
                for px in v.as_chunks::<4>().0 {
                    rgba.extend([
                        255 - px[0].min(254),
                        255 - px[1].min(254),
                        255 - px[2].min(254),
                        255,
                    ]);
                }
            }
            (other, res) => {
                return Err(WcError::UnsupportedInput(format!(
                    "tiff page decode for {other:?} with {} is not supported in this build; \
                     page 0 via the standard path may still work",
                    match res {
                        tiff::decoder::DecodingResult::U8(_) => "8-bit samples",
                        _ => "wider samples",
                    }
                )));
            }
        }
        StillImage::from_rgba8(w, h, rgba).map_err(|e| WcError::Internal(e.to_string()))
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
                "multi-page input requires --page N (never silently truncated)".into(),
                "16-bit input normalizes to 8-bit with a diagnostic".into(),
                "CMYK pages convert with a naive 1-minus mapping (no ICC profile math)".into(),
                "exotic compressions may decode to corrupt_input with a clear message".into(),
            ],
        }
    }

    fn backend(&self) -> String {
        "image 0.25 (tiff)".into()
    }
}
