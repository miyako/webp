//! Metadata write-through (plan §5.6 / ADR-008, Phase 6): ICC/EXIF/XMP
//! payloads are extracted from the input and re-embedded into WebP output
//! (the only target with metadata_write caps), with loud diagnostics for
//! every category that is stripped or cannot be carried. Orientation
//! interplay: rotating pixels must rewrite the EXIF orientation to 1.

mod common;
pub use common::{cmd, dir_entries, fixture, run_with_stdin, stderr_string, stdout_string};

use webp_convert_core as core;
use webp_convert_core::metadata::RawMetadata;

fn json_report(out: &std::process::Output, to_file: bool) -> serde_json::Value {
    let text = if to_file {
        stdout_string(out)
    } else {
        stderr_string(out).trim_start().into()
    };
    serde_json::from_str(text.trim())
        .unwrap_or_else(|e| panic!("--json report parse: {e}; got {text:?}"))
}

fn diag_codes(rep: &serde_json::Value) -> Vec<String> {
    rep["diagnostics"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|d| d["code"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn extract_of(path: &std::path::Path) -> RawMetadata {
    core::metadata::extract(&std::fs::read(path).unwrap(), core::ImageFormat::WebP)
}

#[test]
fn png_metadata_is_carried_into_webp() {
    let src = fixture("metadata/exif/png_icc_exif_xmp.png");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.webp");
    let o = cmd()
        .arg("-i")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .arg("--lossless")
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));
    let rep = json_report(&o, true);
    assert!(
        diag_codes(&rep).contains(&"metadata_carried".to_string()),
        "expected metadata_carried, got {:?}",
        diag_codes(&rep)
    );
    let md = extract_of(&out);
    assert!(md.icc.is_some(), "ICC must reach the webp output");
    assert!(md.exif.is_some(), "EXIF must reach the webp output");
    assert!(md.xmp.is_some(), "XMP must reach the webp output");
}

#[test]
fn jpeg_metadata_is_carried_into_webp() {
    let src = fixture("metadata/exif/jpg_icc_exif_xmp.jpg");
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
    let md = extract_of(&out);
    assert!(md.icc.is_some());
    assert!(md.exif.is_some());
    assert!(md.xmp.is_some());
}

#[test]
fn webp_metadata_round_trips() {
    use webp_convert_core::metadata::extract;
    let src = fixture("metadata/exif/png_icc_exif_xmp.png");
    let tmp = tempfile::tempdir().unwrap();
    let mid = tmp.path().join("mid.webp");
    let out = tmp.path().join("out.webp");

    let o = cmd()
        .arg("-i")
        .arg(&src)
        .arg("-o")
        .arg(&mid)
        .arg("--lossless")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));

    let o = cmd()
        .arg("-i")
        .arg(&mid)
        .arg("-o")
        .arg(&out)
        .arg("--lossless")
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));

    let src_data = std::fs::read(&src).unwrap();
    let src_md = extract(&src_data, core::ImageFormat::Png);
    let mid_md = extract_of(&mid);
    // extraction from a WebP target uses the WebP route
    let out_md = extract(&std::fs::read(&out).unwrap(), core::ImageFormat::WebP);
    assert_eq!(
        mid_md, out_md,
        "metadata must survive a webp webp round trip"
    );
    assert_eq!(
        mid_md.icc, src_md.icc,
        "ICC bytes must be carried unchanged"
    );
    assert_eq!(
        mid_md.xmp, src_md.xmp,
        "XMP bytes must be carried unchanged"
    );
}

#[test]
fn metadata_none_strips_explicitly() {
    let src = fixture("metadata/exif/png_icc_exif_xmp.png");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.webp");
    let o = cmd()
        .arg("-i")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .arg("--lossless")
        .arg("--metadata")
        .arg("none")
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));
    let rep = json_report(&o, true);
    let codes = diag_codes(&rep);
    assert!(
        codes.iter().filter(|c| *c == "metadata_stripped").count() == 3,
        "icc+exif+xmp must each be reported as stripped: {codes:?}"
    );
    assert!(!codes.contains(&"metadata_carried".to_string()));
    let md = extract_of(&out);
    assert!(!md.any(), "--metadata none must strip all categories");
}

#[test]
fn metadata_icc_carries_only_icc() {
    let src = fixture("metadata/exif/png_icc_exif_xmp.png");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.webp");
    let o = cmd()
        .arg("-i")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .arg("--lossless")
        .arg("--metadata")
        .arg("icc")
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));
    let rep = json_report(&o, true);
    let codes = diag_codes(&rep);
    assert!(codes.contains(&"metadata_carried".to_string()));
    assert!(
        codes.iter().any(|c| c == "metadata_stripped"),
        "exif/xmp must be stripped with a note: {codes:?}"
    );
    let md = extract_of(&out);
    assert!(md.icc.is_some());
    assert!(md.exif.is_none());
    assert!(md.xmp.is_none());
}

#[test]
fn metadata_icc_without_icc_warns() {
    let src = fixture("static/png/canonical.png");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.webp");
    let o = cmd()
        .arg("-i")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .arg("--metadata")
        .arg("icc")
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));
    let rep = json_report(&o, true);
    let codes = diag_codes(&rep);
    assert!(
        codes.contains(&"metadata_dropped".to_string()),
        "--metadata icc without an input profile must warn: {codes:?}"
    );
}

#[test]
fn non_webp_targets_still_diagnose_drops() {
    let src = fixture("metadata/exif/png_icc_exif_xmp.png");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.bmp");
    let o = cmd()
        .arg("-i")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));
    let rep = json_report(&o, true);
    assert!(
        diag_codes(&rep).contains(&"metadata_dropped".to_string()),
        " bmp cannot carry metadata; drops must still be loud: {:?}",
        diag_codes(&rep)
    );
}

#[test]
fn orient_auto_rewrites_exif_orientation_to_identity() {
    let src = fixture("metadata/orientation/exif6.png");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.webp");
    let o = cmd()
        .arg("-i")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .arg("--orient")
        .arg("auto")
        .arg("--lossless")
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));
    let rep = json_report(&o, true);
    assert!(diag_codes(&rep).contains(&"orientation_applied".to_string()));
    // the carried EXIF must read orientation 1 (identity), not the stale 6
    let out_md = extract_of(&out);
    let exif = out_md.exif.expect("EXIF must be carried");
    assert_eq!(
        core::metadata::exif_orientation(&exif),
        Some(1),
        "carried EXIF orientation must be rewritten to 1 after rotating pixels"
    );
    // pixels must be rotated (6 = 90deg CW); the 8x8 halves swap axes
    // (red on the left vertically -> red on top horizontally)
    let img = image::open(&out).unwrap().to_rgba8();
    let red_left = (0..8).all(|y| img.get_pixel(1, y).0[0] == 255);
    assert!(!red_left, "pixels must be rotated for orientation 6");
    let red_top = (0..8).all(|x| img.get_pixel(x, 1).0[0] == 255);
    assert!(red_top, "rotation 6 puts the left (red) column on top");
}

#[test]
fn orient_none_keeps_stale_orientation_warns() {
    let src = fixture("metadata/orientation/exif6.png");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.webp");
    let o = cmd()
        .arg("-i")
        .arg(&src)
        .arg("-o")
        .arg(&out)
        .arg("--lossless")
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));
    let rep = json_report(&o, true);
    let codes = diag_codes(&rep);
    assert!(codes.contains(&"orientation_ignored".to_string()));
    // pixels NOT rotated; EXIF carried unchanged (color the behavior, not hide it)
    let out_md = extract_of(&out);
    let exif = out_md.exif.expect("EXIF must be carried");
    assert_eq!(core::metadata::exif_orientation(&exif), Some(6));
}

#[test]
fn animated_webp_carries_metadata() {
    let tmp = tempfile::tempdir().unwrap();
    // two-frame animated webp from the committed fixture
    let src = fixture("animated/webp/two_frames.webp");
    // graft the metadata of the committed png fixture onto the animation
    // by converting animation -> webp --metadata from a png? No: metadata
    // comes from the *input*; so use the png fixture path only for its raw
    // payloads via a direct mux unit path instead: convert anim->webp with
    // metadata extracted from... animated inputs: graft by muxing manually.
    let anim_bytes = std::fs::read(&src).unwrap();
    let png_bytes = std::fs::read(fixture("metadata/exif/png_icc_exif_xmp.png")).unwrap();
    let md = core::metadata::extract(&png_bytes, core::ImageFormat::Png);
    let with_md = codec_webp::mux::with_metadata(&anim_bytes, &md, (4, 4), false).unwrap();
    let graft = tmp.path().join("grafted.webp");
    std::fs::write(&graft, &with_md).unwrap();

    let out = tmp.path().join("out.webp");
    let o = cmd()
        .arg("-i")
        .arg(&graft)
        .arg("-o")
        .arg(&out)
        .arg("--lossless")
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "stderr: {}", stderr_string(&o));
    let rep = json_report(&o, true);
    assert!(diag_codes(&rep).contains(&"metadata_carried".to_string()));
    let out_md = extract_of(&out);
    assert!(out_md.icc.is_some());
    assert!(out_md.exif.is_some());
    assert!(out_md.xmp.is_some());
    let anim = codec_webp::decode_animation(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(
        anim.frames.len(),
        2,
        "metadata must not disturb the animation"
    );
}
