//! Streams and safety (plan "Phase 4"): every input/output stream
//! combination, the stdout discipline (ADR-006), atomic output, overwrite
//! and alias guards, resource limits, and failure cleanup.
//!
//! Platform note: byte-exact stdout assertions are the Windows binary-safety
//! regression net too — Rust std never translates bytes, but these tests
//! pin the behavior on every CI platform.

mod common;
pub use common::{dir_entries, fixture, run_with_stdin, stderr_string, stdout_string};

fn png_fix() -> String {
    fixture("static/png/canonical.png")
        .to_string_lossy()
        .into_owned()
}

/// Run the same conversion file→file to get reference output bytes.
/// This is a *same-format* reference (png→png): `-o -` without --format
/// defaults the output format to the input format, a documented behavior
/// that has its own test below.
fn reference_same_format_bytes() -> Vec<u8> {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("ref.png");
    let out_s = out.to_string_lossy().into_owned();
    let o = run_with_stdin(&[&png_fix(), &out_s], None);
    assert!(
        o.status.success(),
        "reference conversion failed: {}",
        stderr_string(&o)
    );
    std::fs::read(&out).unwrap()
}

/// Same via an explicit `webp` target (extension-driven), for tests that
/// assert a specific output format rather than the input-format default.
fn reference_webp_bytes() -> Vec<u8> {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("ref.webp");
    let out_s = out.to_string_lossy().into_owned();
    let o = run_with_stdin(&[&png_fix(), &out_s], None);
    assert!(
        o.status.success(),
        "reference conversion failed: {}",
        stderr_string(&o)
    );
    std::fs::read(&out).unwrap()
}

// ---------------------------------------------------------------------------
// stream combinations
// ---------------------------------------------------------------------------

#[test]
fn stdin_to_file_matches_direct_conversion() {
    let reference = reference_webp_bytes();
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.webp");
    let out_s = out.to_string_lossy().into_owned();

    let png = std::fs::read(png_fix()).unwrap();
    let o = run_with_stdin(&["-i", "-", "-o", &out_s], Some(&png));

    assert!(o.status.success(), "stderr: {}", stderr_string(&o));
    assert_eq!(std::fs::read(&out).unwrap(), reference);
}

#[test]
fn file_to_stdout_is_byte_exact_bare_form() {
    let reference = reference_same_format_bytes();
    let o = run_with_stdin(&[&png_fix(), "-"], None);

    assert!(o.status.success(), "stderr: {}", stderr_string(&o));
    // stdout: exactly the image bytes, nothing else
    assert_eq!(
        o.stdout, reference,
        "stdout must be byte-identical to the file output"
    );
    // the report still reached stderr
    let err = stderr_string(&o);
    assert!(
        err.contains("wrote") || err.contains("stdout"),
        "stderr: {err}"
    );
    // and carries no stray text
    assert!(!stdout_string(&o).contains("webp-convert"));
}

#[test]
fn stdin_to_stdout_pipeline() {
    let reference = reference_same_format_bytes();
    let png = std::fs::read(png_fix()).unwrap();
    let o = run_with_stdin(&["-i", "-", "-o", "-"], Some(&png));

    assert!(o.status.success(), "stderr: {}", stderr_string(&o));
    assert_eq!(o.stdout, reference);
    assert!(!stderr_string(&o).is_empty(), "report must reach stderr");
}

#[test]
fn stdin_to_stdout_bare_dash_dash() {
    let reference = reference_same_format_bytes();
    let png = std::fs::read(png_fix()).unwrap();
    let o = run_with_stdin(&["-", "-"], Some(&png));

    assert!(o.status.success(), "stderr: {}", stderr_string(&o));
    assert_eq!(o.stdout, reference);
}

#[test]
fn convert_subcommand_accepts_streams() {
    let reference = reference_webp_bytes();
    let png = std::fs::read(png_fix()).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.webp");
    let out_s = out.to_string_lossy().into_owned();

    // convert - FILE (subcommand form takes positionals; -i/-o is
    // deliberately rejected in that position)
    let o = run_with_stdin(&["convert", "-", &out_s], Some(&png));
    assert!(o.status.success(), "stderr: {}", stderr_string(&o));
    assert_eq!(std::fs::read(&out).unwrap(), reference);

    // convert FILE - (to stdout; "-" keeps the input format by default)
    let o2 = run_with_stdin(&["convert", &png_fix(), "-"], None);
    assert!(o2.status.success(), "stderr: {}", stderr_string(&o2));
    assert_eq!(o2.stdout, reference_same_format_bytes());
}

#[test]
fn stdin_format_is_sniffed_by_magic_not_extension() {
    // the *output* is .png but the *input* via stdin has no extension to
    // lean on: webp bytes must be detected by magic (ADR-003) and converted
    let webp_fix = fixture("static/webp/canonical.webp");
    let webp_bytes = std::fs::read(&webp_fix).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.png");
    let out_s = out.to_string_lossy().into_owned();
    let o = run_with_stdin(&["-i", "-", "-o", &out_s], Some(&webp_bytes));

    assert!(o.status.success(), "stderr: {}", stderr_string(&o));
    let (w, h) = image::image_dimensions(&out).unwrap();
    assert_eq!((w, h), (48, 32));
}

#[test]
fn stdout_without_format_defaults_to_input_format() {
    // documented rule (plan §6.2-ish): no --format and no extension to
    // infer from ("-") → keep the input format
    let reference = reference_same_format_bytes();
    let png = std::fs::read(png_fix()).unwrap();
    let o = run_with_stdin(&["-i", "-", "-o", "-"], Some(&png));
    assert!(o.status.success(), "stderr: {}", stderr_string(&o));
    assert_eq!(o.stdout, reference, "-o - must reproduce the input format");
    assert!(
        stderr_string(&o).contains(", png)"),
        "report names png: {}",
        stderr_string(&o)
    );
}

#[test]
fn stdout_with_explicit_format_overrides_default() {
    let reference = reference_webp_bytes();
    let png = std::fs::read(png_fix()).unwrap();
    let o = run_with_stdin(&["--format", "webp", "-i", "-", "-o", "-"], Some(&png));
    assert!(o.status.success(), "stderr: {}", stderr_string(&o));
    assert_eq!(o.stdout, reference, "--format wins for stdout output");
}

// ---------------------------------------------------------------------------
// stdout discipline (ADR-006): binary stdout stays pure in every mode
// ---------------------------------------------------------------------------

#[test]
fn json_report_never_touches_image_stdout() {
    let reference = reference_same_format_bytes();
    let png = std::fs::read(png_fix()).unwrap();
    let o = run_with_stdin(&["--json", "-i", "-", "-o", "-"], Some(&png));

    assert!(o.status.success(), "stderr: {}", stderr_string(&o));
    assert_eq!(
        o.stdout, reference,
        "with --json and stdout output, stdout is image bytes only"
    );
    let err = stderr_string(&o);
    let rep: serde_json::Value = serde_json::from_str(err.trim_start())
        .unwrap_or_else(|e| panic!("stderr must be the JSON report: {e}; got {err:?}"));
    assert_eq!(rep["ok"], serde_json::json!(true));
    assert_eq!(rep["output"]["mode"], serde_json::json!("stdout"));
}

#[test]
fn json_report_on_stdout_when_output_is_file() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.webp");
    let out_s = out.to_string_lossy().into_owned();
    let o = run_with_stdin(&["--json", &png_fix(), &out_s], None);

    assert!(o.status.success(), "stderr: {}", stderr_string(&o));
    let rep: serde_json::Value = serde_json::from_str(&stdout_string(&o))
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; got {:?}", stdout_string(&o)));
    assert_eq!(rep["operation"], serde_json::json!("convert"));
    assert!(
        stderr_string(&o).is_empty(),
        "no report leakage to stderr in JSON+file mode"
    );
}

#[test]
fn failed_stdout_run_writes_nothing_to_stdout() {
    // corrupt input + image-stdout mode: the error JSON goes to stderr and
    // stdout stays empty
    let bad = fixture("malformed/truncated/webp_headers.webp");
    let o = run_with_stdin(
        &["--json", "-i", bad.to_string_lossy().as_ref(), "-o", "-"],
        None,
    );

    assert_eq!(o.status.code(), Some(3));
    assert!(
        o.stdout.is_empty(),
        "no bytes (and no JSON) on stdout on failure"
    );
    let err = stderr_string(&o);
    let rep: serde_json::Value = serde_json::from_str(err.trim_start())
        .unwrap_or_else(|e| panic!("stderr must be JSON: {e}; got {err:?}"));
    assert_eq!(rep["ok"], serde_json::json!(false));
}

// ---------------------------------------------------------------------------
// output safety: overwrite, aliasing, atomicity, cleanup
// ---------------------------------------------------------------------------

#[test]
fn overwrite_guard_then_flag() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.webp");
    let out_s = out.to_string_lossy().into_owned();
    std::fs::write(&out, b"PRECIOUS").unwrap();

    // refused without the flag
    let o = run_with_stdin(&[&png_fix(), &out_s], None);
    assert_eq!(o.status.code(), Some(5), "stderr: {}", stderr_string(&o));
    assert_eq!(
        std::fs::read(&out).unwrap(),
        b"PRECIOUS",
        "output untouched"
    );
    assert!(
        !dir_entries(tmp.path()).iter().any(|n| n.contains("tmp")),
        "no temp litter"
    );

    // permitted with the flag
    let o2 = run_with_stdin(&["--overwrite", &png_fix(), &out_s], None);
    assert!(o2.status.success(), "stderr: {}", stderr_string(&o2));
    assert_ne!(std::fs::read(&out).unwrap(), b"PRECIOUS");
}

#[test]
fn refuse_output_equals_input_even_with_overwrite() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("in.png");
    std::fs::copy(png_fix(), &src).unwrap();
    let before = std::fs::read(&src).unwrap();
    let src_s = src.to_string_lossy().into_owned();

    let o = run_with_stdin(&["--overwrite", &src_s, &src_s], None);
    assert_eq!(o.status.code(), Some(5), "stderr: {}", stderr_string(&o));
    assert_eq!(
        std::fs::read(&src).unwrap(),
        before,
        "input must be untouched"
    );
}

#[cfg(unix)]
#[test]
fn refuse_output_via_symlink_to_input() {
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("in.png");
    std::fs::copy(png_fix(), &real).unwrap();
    let link = tmp.path().join("link.png");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let before = std::fs::read(&real).unwrap();
    let (link_s, real_s) = (
        link.to_string_lossy().into_owned(),
        real.to_string_lossy().into_owned(),
    );

    // output path is the input path directly
    let o = run_with_stdin(&["--overwrite", &link_s, &real_s], None);
    assert_eq!(o.status.code(), Some(5), "stderr: {}", stderr_string(&o));
    assert_eq!(std::fs::read(&real).unwrap(), before);

    // output path is a symlink pointing at the input
    let o2 = run_with_stdin(&["--overwrite", &real_s, &link_s], None);
    assert_eq!(o2.status.code(), Some(5), "stderr: {}", stderr_string(&o2));
    assert_eq!(
        std::fs::read(&real).unwrap(),
        before,
        "symlinked output must not replace input"
    );
}

#[test]
fn failure_leaves_existing_output_untouched_no_litter() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.webp");
    let out_s = out.to_string_lossy().into_owned();
    std::fs::write(&out, b"KEEP").unwrap();

    let bad = fixture("malformed/corrupt/not_an_image.bin");
    let o = run_with_stdin(&["-i", bad.to_string_lossy().as_ref(), "-o", &out_s], None);
    assert_eq!(o.status.code(), Some(3), "stderr: {}", stderr_string(&o));
    assert_eq!(
        std::fs::read(&out).unwrap(),
        b"KEEP",
        "failed run must not touch output"
    );
    assert_eq!(
        dir_entries(tmp.path()),
        vec!["out.webp".to_string()],
        "no temp litter"
    );
}

#[test]
fn output_to_missing_directory_fails_cleanly() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("no-such-dir/out.webp");
    let out_s = out.to_string_lossy().into_owned();
    let o = run_with_stdin(&[&png_fix(), &out_s], None);
    assert_eq!(o.status.code(), Some(5), "stderr: {}", stderr_string(&o));
    assert!(!tmp.path().join("no-such-dir").exists());
}

#[test]
fn dry_run_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.webp");
    let out_s = out.to_string_lossy().into_owned();
    let o = run_with_stdin(&["--dry-run", &png_fix(), &out_s], None);
    assert!(o.status.success(), "stderr: {}", stderr_string(&o));
    assert!(!out.exists(), "dry-run must not create the output");
    assert_eq!(dir_entries(tmp.path()), Vec::<String>::new());
}

// ---------------------------------------------------------------------------
// resource limits
// ---------------------------------------------------------------------------

#[test]
fn max_input_bytes_enforced_on_file_and_stdin() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.webp");
    let out_s = out.to_string_lossy().into_owned();

    let o = run_with_stdin(&["--max-input-bytes", "10", &png_fix(), &out_s], None);
    assert_eq!(o.status.code(), Some(4), "stderr: {}", stderr_string(&o));
    assert!(stderr_string(&o).contains("max-input-bytes"));

    let png = std::fs::read(png_fix()).unwrap();
    let o2 = run_with_stdin(
        &["--max-input-bytes", "10", "-i", "-", "-o", &out_s],
        Some(&png),
    );
    assert_eq!(o2.status.code(), Some(4), "stderr: {}", stderr_string(&o2));
    assert!(!out.exists());
}

#[test]
fn max_pixels_mp_enforced() {
    // canonical is 48x32 = 1536 pixels; a 0 MP budget refuses it (exit 4)
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.webp");
    let out_s = out.to_string_lossy().into_owned();
    let o = run_with_stdin(&["--max-pixels-mp", "0", &png_fix(), &out_s], None);
    assert_eq!(o.status.code(), Some(4), "stderr: {}", stderr_string(&o));
    assert!(stderr_string(&o).contains("exceeds --max-pixels-mp"));
    assert!(!out.exists());
}

#[test]
fn max_frames_enforced_before_decode() {
    let anim = fixture("animated/webp/two_frames.webp");
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.png");
    let out_s = out.to_string_lossy().into_owned();

    let o = run_with_stdin(
        &[
            "--max-frames",
            "1",
            "-i",
            anim.to_string_lossy().as_ref(),
            "--frame",
            "0",
            "-o",
            &out_s,
        ],
        None,
    );
    assert_eq!(o.status.code(), Some(4), "stderr: {}", stderr_string(&o));
    assert!(stderr_string(&o).contains("--max-frames"));
    assert!(!out.exists());

    // with a generous budget the same invocation succeeds
    let o2 = run_with_stdin(
        &[
            "--max-frames",
            "16",
            "-i",
            anim.to_string_lossy().as_ref(),
            "--frame",
            "0",
            "-o",
            &out_s,
        ],
        None,
    );
    assert!(o2.status.success(), "stderr: {}", stderr_string(&o2));
    assert!(out.exists());
}

#[test]
fn timeout_deadline_reports_resource_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let out = tmp.path().join("out.webp");
    let out_s = out.to_string_lossy().into_owned();
    // zero-second deadline: expired by the time the first stage check runs
    let o = run_with_stdin(&["--timeout", "0", &png_fix(), &out_s], None);
    assert_eq!(o.status.code(), Some(4), "stderr: {}", stderr_string(&o));
    assert!(stderr_string(&o).contains("deadline"));
    assert!(!out.exists());

    // a sane deadline does not interfere
    let o2 = run_with_stdin(&["--timeout", "60", &png_fix(), &out_s], None);
    assert!(o2.status.success(), "stderr: {}", stderr_string(&o2));
}

// ---------------------------------------------------------------------------
// corrupt input via both sources
// ---------------------------------------------------------------------------

#[test]
fn truncated_and_corrupt_inputs_fail_with_exit_3() {
    let cases = [
        "malformed/truncated/png_idat.png",
        "malformed/truncated/gif_header.gif",
        "malformed/truncated/webp_headers.webp",
        "malformed/corrupt/gif_bad_palette.gif",
        "malformed/corrupt/png_signature_only.png",
        "malformed/corrupt/webp_riff_short.webp",
        "malformed/corrupt/not_an_image.bin",
    ];
    for rel in cases {
        let path = fixture(rel);
        let tmp = tempfile::tempdir().unwrap();
        let out = tmp.path().join("out.png");
        let out_s = out.to_string_lossy().into_owned();
        let o = run_with_stdin(&["-i", path.to_string_lossy().as_ref(), "-o", &out_s], None);
        assert_eq!(
            o.status.code(),
            Some(3),
            "{rel}: stderr: {}",
            stderr_string(&o)
        );
        assert!(!out.exists(), "{rel}: no output on failure");

        // same bytes over stdin
        let bytes = std::fs::read(&path).unwrap();
        let o2 = run_with_stdin(&["-i", "-", "-o", &out_s], Some(&bytes));
        assert_eq!(
            o2.status.code(),
            Some(3),
            "{rel} via stdin: stderr: {}",
            stderr_string(&o2)
        );
    }
}
