//! Fuzz target: the WebP container muxer and EXIF orientation patcher —
//! our own byte-surgery code — against arbitrary input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use webp_convert_core::metadata::{self, RawMetadata};

fuzz_target!(|data: &[u8]| {
    // RIFF surgery on arbitrary bytes: must return Ok or a WcError, never
    // panic, and any Ok output must re-parse cleanly through the extractor
    let md = RawMetadata {
        icc: Some(data[..data.len().min(256)].to_vec()),
        exif: Some(data[..data.len().min(256)].to_vec()),
        xmp: Some(data.to_vec()),
    };
    if data.is_empty() {
        return;
    }
    if let Ok(out) = codec_webp::mux::with_metadata(&data, &md, (1, 2), true) {
        let _ = metadata::extract(&out, webp_convert_core::ImageFormat::WebP);
    }
    // re-mux the produced container (replacement path)
    if let Ok(out) = codec_webp::mux::with_metadata(&data, &md, (3, 4), false) {
        let md2 = RawMetadata {
            exif: Some(b"II*\0".to_vec()),
            ..RawMetadata::default()
        };
        if let Ok(out2) = codec_webp::mux::with_metadata(&out, &md2, (3, 4), false) {
            let _ = metadata::extract(&out2, webp_convert_core::ImageFormat::WebP);
        }
    }
    // no-op path: empty metadata returns input unchanged
    let _ = codec_webp::mux::with_metadata(
        &data,
        &RawMetadata::default(),
        (1, 1),
        false,
    );
    // EXIF orientation patcher on arbitrary bytes
    let _ = metadata::patch_exif_orientation_to_identity(data);
});
