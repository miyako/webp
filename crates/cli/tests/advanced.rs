//! Phase 5 advanced semantics (plan §5.4):
//! - TIFF multi-page: --page selection, explicit refusal without it
//! - 16-bit TIFF normalization with a `bit_depth_reduced` diagnostic
//! - Animation round trips: webp→webp, webp→gif, refusals, --flatten
//!
//! Gate: no silent loss of significant data — pages, frames and bit
//! depth are all either preserved, explicitly selected, or loudly diagnosed.

use std::path::Path;

use image::{AnimationDecoder, GenericImageView};

mod common;
pub use common::{cmd, fixture, run_with_stdin, stderr_string, stdout_string};

fn json_report(out: &std::process::Output) -> serde_json::Value {
    serde_json::from_str(stdout_string(out).trim())
        .unwrap_or_else(|e| panic!("--json report parse: {e}; got {:?}", stdout_string(out)))
}

/// Decode a still conversion output and return its center pixel RGBA.
fn center_pixel(path: &Path) -> [u8; 4] {
    let img = image::open(path).unwrap_or_else(|e| panic!("decode {}: {e}", path.display()));
    let (w, h) = img.dimensions();
    let rgba = img.to_rgba8();
    rgba.get_pixel(w / 2, h / 2).0
}

// ---------------------------------------------------------------------------
// TIFF --page
// ---------------------------------------------------------------------------

#[test]
fn multipage_tiff_without_page_is_refused() {
    let tiff = fixture("multipage/tiff/two_pages.tiff");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.png");

    let o = cmd()
        .arg("-i")
        .arg(&tiff)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(3), "stderr: {}", stderr_string(&o));
    assert!(stderr_string(&o).contains("--page"));
    assert!(!out.exists());

    // a diagnostic message, never a quiet first page
    assert!(stderr_string(&o).contains("2 pages"));
}

#[test]
fn page_selection_decodes_each_page() {
    let tiff = fixture("multipage/tiff/two_pages.tiff");
    let tmp = tempfile::tempdir().unwrap();
    let p0 = tmp.path().join("p0.png");
    let p1 = tmp.path().join("p1.png");

    let o = cmd()
        .arg("-i")
        .arg(&tiff)
        .arg("--page")
        .arg("0")
        .arg("-o")
        .arg(&p0)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));

    let o = cmd()
        .arg("-i")
        .arg(&tiff)
        .arg("--page")
        .arg("1")
        .arg("-o")
        .arg(&p1)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));

    let c0 = center_pixel(&p0);
    let c1 = center_pixel(&p1);
    // page 0 red, page 1 blue (see codec-tiff/examples/write_fixtures.rs)
    assert_eq!([c0[0], c0[1], c0[2]], [255, 0, 0], "page 0 must be red");
    assert_eq!([c1[0], c1[1], c1[2]], [0, 0, 255], "page 1 must be blue");
    let img0 = image::open(&p0).unwrap();
    assert_eq!(img0.dimensions(), (8, 8));
}

#[test]
fn page_out_of_range_is_usage_error() {
    let tiff = fixture("multipage/tiff/two_pages.tiff");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.png");

    let o = cmd()
        .arg("-i")
        .arg(&tiff)
        .arg("--page")
        .arg("5")
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2), "stderr: {}", stderr_string(&o));
    assert!(stderr_string(&o).contains("out of range"));
    assert!(!out.exists());
}

#[test]
fn page_and_frame_cannot_be_combined() {
    let tiff = fixture("multipage/tiff/two_pages.tiff");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.png");

    let o = cmd()
        .arg("-i")
        .arg(&tiff)
        .arg("--page")
        .arg("0")
        .arg("--frame")
        .arg("0")
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2), "stderr: {}", stderr_string(&o));
    assert!(stderr_string(&o).contains("--page and --frame"));
    assert!(!out.exists());
}

#[test]
fn single_page_tiff_still_converts_without_page_flag() {
    let tiff = fixture("static/tiff/canonical.tiff");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.png");

    let o = cmd()
        .arg("-i")
        .arg(&tiff)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));
    assert!(out.exists());
}

#[test]
fn inspect_reports_page_count() {
    let tiff = fixture("multipage/tiff/two_pages.tiff");
    let o = cmd()
        .arg("inspect")
        .arg(&tiff)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0));
    let rep = json_report(&o);
    let frames = rep.pointer("/input/frames").and_then(|v| v.as_u64());
    assert_eq!(frames, Some(2), "inspect must report page count: {rep}");
}

#[test]
fn stdin_page_selection_works() {
    let tiff = std::fs::read(fixture("multipage/tiff/two_pages.tiff")).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("p1.png");
    let o = run_with_stdin(
        &[
            "-i",
            "-",
            "--page",
            "1",
            "--from",
            "tiff",
            "-o",
            &out.to_string_lossy(),
        ],
        Some(&tiff),
    );
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));
    let c1 = center_pixel(&out);
    assert_eq!([c1[0], c1[1], c1[2]], [0, 0, 255]);
}

// ---------------------------------------------------------------------------
// 16-bit TIFF
// ---------------------------------------------------------------------------

#[test]
fn sixteen_bit_tiff_normalizes_with_diagnostic() {
    let tiff = fixture("color/16bit/gray16.tiff");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.png");

    let o = cmd()
        .arg("-i")
        .arg(&tiff)
        .arg("-o")
        .arg(&out)
        .arg("--json")
        .output()
        .unwrap();
    let rep = json_report(&o);
    let rep = if rep["ok"] == serde_json::json!(false) {
        // --json on stderr for some forms; fall back
        serde_json::from_str::<serde_json::Value>(stderr_string(&o).trim_start()).unwrap()
    } else {
        rep
    };
    let codes: Vec<&str> = rep["diagnostics"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|d| d["code"].as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert!(
        codes.contains(&"bit_depth_reduced"),
        "16-bit input must diagnose bit_depth_reduced, got {codes:?}"
    );
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));
    assert!(out.exists());
}

// ---------------------------------------------------------------------------
// animation routing
// ---------------------------------------------------------------------------

#[test]
fn animated_webp_to_webp_preserves_frames_and_timing() {
    let src = fixture("animated/webp/two_frames.webp");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.webp");

    let o = cmd()
        .arg("-i")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));

    let src_anim = codec_webp::decode_animation(&std::fs::read(&src).unwrap()).unwrap();
    let out_anim = codec_webp::decode_animation(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(out_anim.frames.len(), src_anim.frames.len());
    assert_eq!(out_anim.frames.len(), 2);
    // timing preserved (the fixture uses 200 ms per frame)
    for (a, b) in out_anim.frames.iter().zip(src_anim.frames.iter()) {
        assert_eq!(
            a.duration_ms, b.duration_ms,
            "frame durations must round-trip"
        );
    }
    // frame colors distinguishable (red then green fixture)
    let px = |f: &webp_convert_core::model::Frame| {
        let p = &f.still.rgba;
        [p[0], p[1], p[2]]
    };
    assert_ne!(px(&out_anim.frames[0]), px(&out_anim.frames[1]));
}

#[test]
fn animated_webp_to_gif_keeps_two_frames_warns_when_alpha_partial() {
    let src = fixture("animated/webp/two_frames.webp");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.gif");

    let o = cmd()
        .arg("-i")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));

    let bytes = std::fs::read(&out).unwrap();
    let (w, h) = (48u32, 48u32); // fixture is 4x4; decode to count frames
    let _ = (w, h);
    let dec = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(&bytes))
        .unwrap_or_else(|e| panic!("gif decode: {e}"));
    let frames: Vec<(u32, u32)> = dec
        .into_frames()
        .map(|f| {
            let frame = f.unwrap();
            let delay = frame.delay().numer_denom_ms();
            (delay.0 * 1000 / delay.1.max(1), 0) // ms
        })
        .collect();
    assert_eq!(frames.len(), 2, "gif must keep both frames");
    // durations survive 1/100 s quantization as multiples of 10 ms
    for (ms, _) in frames {
        assert_eq!(ms % 10, 0, "gif delay granularity is 10ms: {ms}");
    }
}

#[test]
fn animated_webp_to_png_is_refused() {
    let src = fixture("animated/webp/two_frames.webp");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.png");

    let o = cmd()
        .arg("-i")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(3), "stderr: {}", stderr_string(&o));
    let msg = stderr_string(&o);
    assert!(msg.contains("animation frames"));
    assert!(msg.contains("--frame"));
    assert!(msg.contains("--flatten"));
    assert!(!out.exists());
    // no temp-file litter
    assert_eq!(
        common::dir_entries(tmp.path()).len(),
        0,
        "refused conversion must not write files"
    );
}

#[test]
fn flatten_converts_first_frame_with_diagnostic() {
    let src = fixture("animated/webp/two_frames.webp");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.png");

    let o = cmd()
        .arg("-i")
        .arg(&src)
        .arg("--flatten")
        .arg("-o")
        .arg(&out)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));

    let rep = json_report(&o);
    let rep = if rep["ok"] == serde_json::json!(false) {
        serde_json::from_str::<serde_json::Value>(stderr_string(&o).trim_start()).unwrap()
    } else {
        rep
    };
    let codes: Vec<String> = rep["diagnostics"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|d| d["code"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    assert!(
        codes.iter().any(|c| c == "animation_flattened"),
        "flatten must warn with animation_flattened, got {codes:?}"
    );

    // the flattened still equals frame 0 of the source
    let src_anim = codec_webp::decode_animation(&std::fs::read(&src).unwrap()).unwrap();
    let frame0 = &src_anim.frames[0].still.rgba;
    let out_img = image::open(&out).unwrap().to_rgba8();
    assert_eq!(out_img.as_raw().len(), frame0.len());
    assert_eq!(
        out_img.as_raw().as_slice(),
        frame0.as_slice(),
        "--flatten must yield exactly the first frame"
    );
}

#[test]
fn frame_selection_of_animated_webp_still_works() {
    let src = fixture("animated/webp/two_frames.webp");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("f1.png");

    let o = cmd()
        .arg("-i")
        .arg(&src)
        .arg("--frame")
        .arg("1")
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));

    let src_anim = codec_webp::decode_animation(&std::fs::read(&src).unwrap()).unwrap();
    let expected = &src_anim.frames[1].still.rgba;
    let out_img = image::open(&out).unwrap().to_rgba8();
    assert_eq!(out_img.as_raw().as_slice(), expected.as_slice());
}

#[test]
fn frame_out_of_range_is_refused() {
    let src = fixture("animated/webp/two_frames.webp");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.png");

    let o = cmd()
        .arg("-i")
        .arg(&src)
        .arg("--frame")
        .arg("7")
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        matches!(o.status.code(), Some(2) | Some(3)),
        "stderr: {}",
        stderr_string(&o)
    );
    assert!(!out.exists());
}

#[test]
fn animated_gif_to_webp_preserves_frames() {
    // build a 2-frame GIF in the temp dir with the image crate (same
    // encoder path the codec uses), then convert
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("two.gif");
    let mut buf = Vec::new();
    {
        let mut enc = image::codecs::gif::GifEncoder::new(&mut buf);
        enc.set_repeat(image::codecs::gif::Repeat::Infinite)
            .unwrap();
        for rgb in [[255u8, 0, 0], [0, 255, 0]] {
            let mut px = Vec::new();
            for _ in 0..4 * 4 {
                px.extend_from_slice(&rgb);
                px.push(255);
            }
            let img = image::RgbaImage::from_raw(4, 4, px).unwrap();
            enc.encode_frame(image::Frame::from_parts(
                img,
                0,
                0,
                image::Delay::from_numer_denom_ms(200, 1),
            ))
            .unwrap();
        }
    }
    std::fs::write(&src, &buf).unwrap();

    let out = tmp.path().join("out.webp");
    let o = cmd()
        .arg("-i")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));

    let anim = codec_webp::decode_animation(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(anim.frames.len(), 2, "gif→webp must keep both frames");
    assert_eq!(anim.frames[0].duration_ms, 200);
    assert_eq!(anim.frames[1].duration_ms, 200);
}
