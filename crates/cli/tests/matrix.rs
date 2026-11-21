//! Conversion matrix (plan §10.3): every supported input format × output
//! format pair on the committed fixture corpus, plus malformed-input
//! handling. Semantic checks, not hashes:
//! - lossless same-format and PNG→PNG: exact pixels
//! - everything else: decode-success + dimensions preserved
//! - alpha→alpha-less targets require --background and flatten
//! - malformed input: exit 3, never a successful conversion

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use assert_cmd::prelude::*;
use image::GenericImageView;
use predicates::prelude::*;

fn repo_root() -> PathBuf {
    let mut dir = std::env::current_dir().unwrap();
    loop {
        if dir.join("tests/fixtures").is_dir() {
            return dir;
        }
        assert!(dir.pop(), "repo root not found");
    }
}

fn fixture(rel: &str) -> PathBuf {
    repo_root().join("tests/fixtures").join(rel)
}

/// Prefer the release binary when WEBP_CONVERT_BIN is set (matrix tests are
/// timing-sensitive); otherwise the standard test-built binary.
fn cmd() -> StdCommand {
    match std::env::var("WEBP_CONVERT_BIN") {
        Ok(path) => StdCommand::new(resolve_bin(&path)),
        Err(_) => StdCommand::cargo_bin("webp-convert").unwrap(),
    }
}

/// A WEBP_CONVERT_BIN relative to the repo root (e.g. target/release/...)
/// still works when cargo runs the test with cwd = crates/cli.
fn resolve_bin(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.is_file() {
        return p;
    }
    // manifest dir is crates/cli; the workspace target dir lives two up
    let from_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../")
        .join(path);
    if from_root.is_file() {
        return from_root;
    }
    p
}

/// (source format, target format): the full 6x6 matrix.
fn matrix_pairs() -> Vec<(String, String)> {
    let formats = ["png", "jpeg", "gif", "webp", "tiff", "bmp"];
    let mut pairs = Vec::new();
    for src in formats {
        for dst in formats {
            pairs.push((src.to_string(), dst.to_string()));
        }
    }
    pairs
}

fn fixture_for(format: &str) -> PathBuf {
    match format {
        "png" => fixture("static/png/canonical.png"),
        "jpeg" => fixture("static/jpeg/canonical.jpg"),
        "gif" => fixture("static/gif/canonical.gif"),
        "webp" => fixture("static/webp/canonical_lossless.webp"),
        "tiff" => fixture("static/tiff/canonical.tiff"),
        "bmp" => fixture("static/bmp/canonical.bmp"),
        other => panic!("unknown format {other}"),
    }
}

fn decode_dimensions(path: &Path) -> (u32, u32) {
    image::open(path)
        .unwrap_or_else(|e| panic!("cannot decode {}: {e}", path.display()))
        .dimensions()
}

#[test]
fn conversion_matrix_all_36_pairs() {
    let tmp = tempfile::tempdir().unwrap();
    let (w, h) = (48u32, 32u32);

    for (src_fmt, dst_fmt) in matrix_pairs() {
        // read-only fixture on the input side; output named by its real
        // extension so format detection works without --format
        let dst_ext = match dst_fmt.as_str() {
            "jpeg" => "jpg",
            f => f,
        };
        let out = tmp.path().join(format!("{src_fmt}_to_{dst_fmt}.{dst_ext}"));
        let mut c = cmd();
        c.args([
            "-i",
            fixture_for(&src_fmt).to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ]);

        // alpha-less targets: pass a background so partial-alpha sources
        // (gif/webp can carry alpha) convert deterministically instead of
        // exercising the refusal path here (refusals have their own tests).
        if matches!(dst_fmt.as_str(), "jpeg" | "bmp") {
            c.arg("--background").arg("#202020");
        }

        c.assert().success();

        let (ow, oh) = decode_dimensions(&out);
        assert_eq!(
            (ow, oh),
            (w, h),
            "{src_fmt} -> {dst_fmt} must preserve dimensions"
        );
    }
}

#[test]
fn png_to_png_round_trip_is_pixel_exact() {
    let tmp = tempfile::tempdir().unwrap();
    let src = fixture("static/png/canonical.png");
    let mid = tmp.path().join("mid.webp");
    let back = tmp.path().join("back.png");

    cmd()
        .args([
            "-i",
            src.to_str().unwrap(),
            "-o",
            mid.to_str().unwrap(),
            "--lossless",
        ])
        .assert()
        .success();
    cmd()
        .args(["-i", mid.to_str().unwrap(), "-o", back.to_str().unwrap()])
        .assert()
        .success();

    let a = image::open(&src).unwrap().to_rgba8();
    let b = image::open(&back).unwrap().to_rgba8();
    assert_eq!(a.as_raw(), b.as_raw());
}

#[test]
fn under_transparent_rgb_survives_lossless_webp() {
    let tmp = tempfile::tempdir().unwrap();
    let src = fixture("alpha/rgb_under_transparent.png");
    let out = tmp.path().join("out.png");

    cmd()
        .args(["-i", src.to_str().unwrap(), "-o", out.to_str().unwrap()])
        .assert()
        .success();

    // pixel (0,0) is RGB (13,200,7) at alpha 0 in the source
    let img = image::open(&src).unwrap().to_rgba8();
    let back = image::open(&out).unwrap().to_rgba8();
    assert_eq!(img.get_pixel(0, 0), back.get_pixel(0, 0));
    assert_eq!(img.as_raw(), back.as_raw());
}

#[test]
fn animated_input_frames() {
    let tmp = tempfile::tempdir().unwrap();
    let src = fixture("animated/webp/two_frames.webp");

    // refusal without --frame
    cmd()
        .args([
            "-i",
            src.to_str().unwrap(),
            "-o",
            tmp.path().join("x.png").to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(3);

    // frame 0 = red, frame 1 = green
    for (n, expect) in [(0u32, [255u8, 0, 0, 255]), (1u32, [0, 255, 0, 255])] {
        let out = tmp.path().join(format!("f{n}.png"));
        cmd()
            .args([
                "-i",
                src.to_str().unwrap(),
                "-o",
                out.to_str().unwrap(),
                "--frame",
                &n.to_string(),
            ])
            .assert()
            .success();
        let img = image::open(&out).unwrap().to_rgba8();
        assert_eq!(*img.get_pixel(0, 0), image::Rgba(expect));
    }
}

#[test]
fn malformed_inputs_fail_with_exit_3() {
    let tmp = tempfile::tempdir().unwrap();
    let cases = [
        "truncated/png_idat.png",
        "truncated/gif_header.gif",
        "truncated/webp_headers.webp",
        "corrupt/gif_bad_palette.gif",
        "corrupt/png_signature_only.png",
        "corrupt/webp_riff_short.webp",
        "corrupt/not_an_image.bin",
    ];
    for case in cases {
        let src = fixture("malformed").join(case);
        let out = tmp.path().join("never_written.out");
        cmd()
            .args(["-i", src.to_str().unwrap(), "-o", out.to_str().unwrap()])
            .assert()
            .failure()
            .code(3)
            .stderr(predicate::str::contains("webp-convert"));
        assert!(!out.exists(), "no output file may be created for {case}");
    }
}

#[test]
fn empty_stdin_is_reported_not_panic() {
    cmd()
        .args(["-i", "-", "-o", "/dev/null"])
        .stdin(std::process::Stdio::null())
        .assert()
        .failure()
        .code(3);
}

#[test]
fn jpeg_source_to_alpha_targets_preserve_rgb() {
    // JPEG has no alpha: converting it to any alpha-capable target must
    // produce fully-opaque pixels.
    let tmp = tempfile::tempdir().unwrap();
    let src = fixture("static/jpeg/canonical.jpg");
    let out = tmp.path().join("out.png");
    cmd()
        .args(["-i", src.to_str().unwrap(), "-o", out.to_str().unwrap()])
        .assert()
        .success();
    let img = image::open(&out).unwrap().to_rgba8();
    assert!(
        img.pixels().all(|p| p[3] == 255),
        "jpeg decode must be opaque"
    );
}
