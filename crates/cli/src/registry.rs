//! The linked codec set (plan §8) — the single source of registry truth.
//!
//! Built once per process (`OnceLock`) so the engine can hold `&'static`
//! references to codecs; entries always come from codec code, never docs.

use std::sync::OnceLock;

use webp_convert_core::codec::Codec;

static REGISTRY: OnceLock<Vec<Box<dyn Codec>>> = OnceLock::new();

pub fn all_codecs() -> &'static [Box<dyn Codec>] {
    REGISTRY.get_or_init(|| {
        vec![
            Box::new(codec_webp::WebpCodec),
            Box::new(codec_png::PngCodec),
            Box::new(codec_jpeg::JpegCodec),
            Box::new(codec_gif::GifCodec),
            Box::new(codec_tiff::TiffCodec),
            Box::new(codec_bmp::BmpCodec),
        ]
    })
}
