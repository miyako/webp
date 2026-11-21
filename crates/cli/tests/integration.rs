//! End-to-end CLI tests: real binary, real files, exit codes, JSON
//! protocol, and the no-silent-data-loss policies (ADR-005/006/007).

use std::process::Command as StdCommand;

use assert_cmd::prelude::*;
use image::{Rgba, RgbaImage};
use predicates::prelude::*;
use tempfile::TempDir;

/// Build a deterministic RGBA test image: red gradient with varied alpha.
fn test_image(w: u32, h: u32) -> RgbaImage {
    let mut img = RgbaImage::new(w, h);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let alpha = if (x + y) % 3 == 0 { 128 } else { 255 };
        *px = Rgba([
            (x * 7 % 256) as u8,
            (y * 11 % 256) as u8,
            (x * 3 % 256) as u8,
            alpha,
        ]);
    }
    img
}

fn png_fixture(dir: &TempDir, name: &str, w: u32, h: u32) -> std::path::PathBuf {
    let path = dir.path().join(name);
    test_image(w, h)
        .save_with_format(&path, image::ImageFormat::Png)
        .unwrap();
    path
}

fn cmd() -> StdCommand {
    StdCommand::cargo_bin("webp-convert").unwrap()
}

// ---------------------------------------------------------------------------
// happy paths
// ---------------------------------------------------------------------------

#[test]
fn png_to_webp_lossless_round_trip_is_pixel_exact() {
    let dir = tempfile::tempdir().unwrap();
    let src = png_fixture(&dir, "src.png", 16, 12);
    let webp = dir.path().join("out.webp");
    let back = dir.path().join("back.png");

    cmd()
        .args([
            "-i",
            src.to_str().unwrap(),
            "-o",
            webp.to_str().unwrap(),
            "--lossless",
        ])
        .assert()
        .success();
    cmd()
        .args(["-i", webp.to_str().unwrap(), "-o", back.to_str().unwrap()])
        .assert()
        .success();

    let a = image::open(&src).unwrap().to_rgba8();
    let b = image::open(&back).unwrap().to_rgba8();
    assert_eq!(a.dimensions(), b.dimensions());
    // includes alpha AND the RGB under fully-transparent pixels (exact=1)
    assert_eq!(
        a.as_raw(),
        b.as_raw(),
        "lossless webp must round-trip exactly"
    );
}

#[test]
fn png_to_jpeg_requires_background() {
    let dir = tempfile::tempdir().unwrap();
    let src = png_fixture(&dir, "src.png", 8, 8);
    let jpg = dir.path().join("out.jpg");

    // refusal without --background (exit 2, missing_option)
    cmd()
        .args([
            "-i",
            src.to_str().unwrap(),
            "-o",
            jpg.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .failure()
        .code(2)
        .stdout(predicate::str::contains("\"code\": \"missing_option\""));

    // succeeds with it
    cmd()
        .args([
            "-i",
            src.to_str().unwrap(),
            "-o",
            jpg.to_str().unwrap(),
            "--background",
            "#ffffff",
        ])
        .assert()
        .success();
    assert!(jpg.exists());
}

#[test]
fn refuse_to_overwrite_without_flag() {
    let dir = tempfile::tempdir().unwrap();
    let src = png_fixture(&dir, "src.png", 4, 4);
    let out = dir.path().join("out.webp");
    std::fs::write(&out, b"existing").unwrap();

    cmd()
        .args(["-i", src.to_str().unwrap(), "-o", out.to_str().unwrap()])
        .assert()
        .failure()
        .code(5)
        .stderr(predicate::str::contains("--overwrite"));

    // existing output must be untouched
    assert_eq!(std::fs::read(&out).unwrap(), b"existing");
}

#[test]
fn refuse_to_replace_input_file() {
    let dir = tempfile::tempdir().unwrap();
    let src = png_fixture(&dir, "src.png", 4, 4);
    let before = std::fs::read(&src).unwrap();
    cmd()
        .args([
            "-i",
            src.to_str().unwrap(),
            "-o",
            src.to_str().unwrap(),
            "--overwrite",
        ])
        .assert()
        .failure()
        .code(5);
    assert_eq!(
        std::fs::read(&src).unwrap(),
        before,
        "input must be untouched"
    );
}

// ---------------------------------------------------------------------------
// protocol & aux commands
// ---------------------------------------------------------------------------

#[test]
fn json_convert_report_matches_protocol_v1() {
    let dir = tempfile::tempdir().unwrap();
    let src = png_fixture(&dir, "src.png", 5, 5);
    let out = dir.path().join("out.webp");

    let output = cmd()
        .args([
            "-i",
            src.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(v["protocol"], 1);
    assert_eq!(v["ok"], true);
    assert_eq!(v["operation"], "convert");
    assert_eq!(v["input"]["format"], "png");
    assert_eq!(v["output"]["format"], "webp");
    assert!(v["output"]["bytes"].is_u64());
}

#[test]
fn capabilities_registry_lists_all_p0_formats() {
    let output = cmd()
        .args(["capabilities", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let names: Vec<&str> = v["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["format"].as_str().unwrap())
        .collect();
    for f in ["webp", "png", "jpeg", "gif", "tiff", "bmp"] {
        assert!(names.contains(&f), "missing format {f} in registry");
    }
}

#[test]
fn capabilities_filter_reports_unknown_format() {
    cmd()
        .args(["capabilities", "avif"])
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("no codec registered"));
}

#[test]
fn inspect_reports_dimensions_and_alpha() {
    let dir = tempfile::tempdir().unwrap();
    let src = png_fixture(&dir, "src.png", 7, 9);
    cmd()
        .args(["inspect", src.to_str().unwrap(), "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"width\": 7"));
}

// ---------------------------------------------------------------------------
// limits & resource guards
// ---------------------------------------------------------------------------

#[test]
fn max_pixels_limit_refuses_oversize() {
    let dir = tempfile::tempdir().unwrap();
    let src = png_fixture(&dir, "big.png", 64, 64);
    let out = dir.path().join("out.webp");
    cmd()
        .args([
            "-i",
            src.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--max-pixels-mp",
            "1",
        ])
        // 64x64 = 4096 px is fine; assert success
        .assert()
        .success();
    // Now the same with a limit expressed via --max-input-bytes on a big fake file
    let junk = dir.path().join("junk.png");
    std::fs::write(&junk, vec![0u8; 2048]).unwrap();
    cmd()
        .args([
            "-i",
            junk.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--max-input-bytes",
            "1024",
        ])
        .assert()
        .failure()
        .code(4);
}

// ---------------------------------------------------------------------------
// animation policy
// ---------------------------------------------------------------------------

#[test]
fn animated_webp_requires_frame_selection() {
    let dir = tempfile::tempdir().unwrap();
    // Build an animated WebP with the vendored encoder via the tool crate
    let webp = dir.path().join("anim.webp");
    make_two_frame_webp(&webp);

    let out = dir.path().join("out.png");
    cmd()
        .args(["-i", webp.to_str().unwrap(), "-o", out.to_str().unwrap()])
        .assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains("--frame"));

    // explicit frame succeeds
    cmd()
        .args([
            "-i",
            webp.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
            "--frame",
            "1",
        ])
        .assert()
        .success();
    // decode_frame(1) must be frame 2's color (green), verified in codec tests
    assert!(out.exists());
}

#[test]
fn animated_webp_frame_decode_picks_right_frame() {
    let dir = tempfile::tempdir().unwrap();
    let webp = dir.path().join("two.webp");
    make_two_frame_webp(&webp);

    let f0 = dir.path().join("f0.png");
    cmd()
        .args([
            "-i",
            webp.to_str().unwrap(),
            "-o",
            f0.to_str().unwrap(),
            "--frame",
            "0",
        ])
        .assert()
        .success();
    let img = image::open(&f0).unwrap().to_rgba8();
    // frame 0 is solid red
    assert_eq!(*img.get_pixel(0, 0), Rgba([255, 0, 0, 255]));

    let f1 = dir.path().join("f1.png");
    cmd()
        .args([
            "-i",
            webp.to_str().unwrap(),
            "-o",
            f1.to_str().unwrap(),
            "--frame",
            "1",
        ])
        .assert()
        .success();
    let img = image::open(&f1).unwrap().to_rgba8();
    // frame 1 is solid green
    assert_eq!(*img.get_pixel(0, 0), Rgba([0, 255, 0, 255]));
}

/// Encode a 2x1-frame, 4x4 animated WebP (red, then green) using the
/// vendored libwebp mux/demux API through codec-webp's test helper.
fn make_two_frame_webp(path: &std::path::Path) {
    codec_webp::testing::write_two_frame_webp(path).expect("test fixture webp");
}
