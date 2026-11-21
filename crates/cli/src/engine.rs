//! The conversion engine (ADR-005): read → detect → inspect → decode →
//! policy → encode → safety → write → report. Every rule that prevents
//! silent data loss lives here, not in codecs or I/O.

use std::str::FromStr;
use std::time::{Duration, Instant};

use core::{
    codec::Codec,
    diagnostics::{codes, Diagnostic, Severity},
    error::{Result, WcError},
    format::{self, ImageFormat},
    model::{Decoded, StillImage},
    policy::{EncodeOptions, MetadataMode, OrientMode},
    protocol::{InputInfoPayload, OutputInfoPayload, Report},
    rgba,
};
use webp_convert_core as core;

use crate::args::{self, Cli, Command};
use crate::io;
use crate::registry;

// ---------------------------------------------------------------------------
// entry point
// ---------------------------------------------------------------------------

/// Run one invocation; returns the process exit code.
pub fn run(cli: &Cli) -> i32 {
    let result = match &cli.command {
        Some(Command::Inspect { input }) => run_inspect(cli, input),
        Some(Command::Formats) => run_formats(cli),
        Some(Command::Capabilities { format }) => run_capabilities(cli, format.as_deref()),
        Some(Command::Convert { .. }) | None => run_convert(cli),
    };
    result.unwrap_or_else(|e| {
        let op = match &cli.command {
            Some(Command::Inspect { .. }) => "inspect",
            Some(Command::Formats) | Some(Command::Capabilities { .. }) => "capabilities",
            _ => "convert",
        };
        let report = Report::failure(op, &e);
        // JSON error documents land on stdout (machine consumers parse the
        // same stream), except when an image would have gone there.
        let image_to_stdout = matches!(current_output_spec(cli).as_deref(), Some("-"));
        let stream = if image_to_stdout {
            ReportStream::Stderr
        } else {
            ReportStream::Stdout
        };
        emit(cli, &report, stream);
        e.exit_code()
    })
}

// ---------------------------------------------------------------------------
// report sink (ADR-006: binary stdout is never contaminated)
// ---------------------------------------------------------------------------

enum ReportStream {
    Stdout,
    Stderr,
}

/// JSON goes to stdout for machine consumers *unless* the image went to
/// stdout; then the JSON goes to stderr (ADR-006). Human text always goes
/// to stderr so stdout remains clean for scripting.
fn emit(cli: &Cli, report: &Report, stream: ReportStream) {
    let to_stdout = cli.json && matches!(stream, ReportStream::Stdout);
    let text = report.to_pretty();
    if to_stdout {
        println!("{text}");
        return;
    }
    // stderr: compact human line + diagnostics, or the JSON document
    if cli.json {
        eprintln!("{text}");
        return;
    }
    match (&report.output, report.ok) {
        (Some(out), true) => {
            eprintln!(
                "webp-convert: wrote {} ({} bytes, {})",
                out.mode.as_deref().unwrap_or("file"),
                out.bytes,
                out.format,
            );
        }
        (None, true) => eprintln!("webp-convert: {} ok", report.operation),
        (_, false) => {
            if let Some(err) = &report.error {
                eprintln!("webp-convert: error ({}): {}", err.code, err.message);
            }
        }
    }
    for d in &report.diagnostics {
        eprintln!(
            "webp-convert: {} ({}): {}",
            sev_word(d.severity),
            d.code,
            d.message
        );
    }
}

fn sev_word(s: Severity) -> &'static str {
    match s {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
}

/// The output spec of the current invocation, if one is given.
fn current_output_spec(cli: &Cli) -> Option<String> {
    match &cli.command {
        Some(Command::Convert { output, .. }) => Some(output.clone()),
        None => cli.output_opt.clone().or_else(|| cli.output_.clone()),
        _ => None,
    }
}

fn emit_diagnostics(cli: &Cli, diags: &[Diagnostic]) {
    if cli.quiet {
        return;
    }
    for d in diags {
        eprintln!(
            "webp-convert: {} ({}): {}",
            sev_word(d.severity),
            d.code,
            d.message
        );
    }
}

// ---------------------------------------------------------------------------
// convert
// ---------------------------------------------------------------------------

/// Processing deadline: `--timeout` bounds the pipeline between stages.
/// Native codec calls and blocking reads cannot be interrupted from within;
/// this catches cumulative overrun between them (plan §6.4 "where feasible";
/// a wrapper can enforce a hard kill externally).
struct Deadline {
    start: Instant,
    limit: Option<Duration>,
}

impl Deadline {
    fn from_cli(cli: &Cli) -> Deadline {
        Deadline {
            start: Instant::now(),
            limit: cli.timeout.map(Duration::from_secs),
        }
    }

    fn check(&self, stage: &str) -> Result<()> {
        let Some(limit) = self.limit else {
            return Ok(());
        };
        if self.start.elapsed() > limit {
            return Err(WcError::ResourceLimit(format!(
                "processing deadline of {}s exceeded at stage '{stage}'; \
                 re-run with a larger --timeout or drop the flag",
                limit.as_secs()
            )));
        }
        Ok(())
    }
}

fn run_convert(cli: &Cli) -> Result<i32> {
    let deadline = Deadline::from_cli(cli);
    let inv = args::resolved_io(cli, &cli.command)?;
    let input = io::read_input(&inv.input, cli.max_input_bytes)?;
    deadline.check("read")?;
    let codec = choose_input_codec(&input.bytes, &inv.input, cli.from)?;
    let info = codec
        .inspect(&input.bytes)
        .map_err(|e| WcError::CorruptInput(format!("could not read {}: {e}", input.origin)))?;

    // frame-count budget comes from the header, before any decode work
    if let Some(max_frames) = cli.max_frames {
        if info.frames > max_frames {
            return Err(WcError::ResourceLimit(format!(
                "image carries {} frames; exceeds --max-frames {max_frames}",
                info.frames
            )));
        }
    }

    let mut diags: Vec<Diagnostic> = Vec::new();

    // ---- decode ----
    let mut still = decode_still(codec, &input.bytes, cli)?;
    deadline.check("decode")?;

    // ---- limits ----
    let max_pixels = u64::from(cli.max_pixels_mp) * 1_000_000;
    if still.pixel_count() > max_pixels {
        return Err(WcError::ResourceLimit(format!(
            "image is {}x{} ({} pixels); exceeds --max-pixels-mp {} ({} pixels)",
            still.width,
            still.height,
            still.pixel_count(),
            cli.max_pixels_mp,
            max_pixels
        )));
    }

    // ---- target format + options ----
    let target = choose_target_format(codec, cli, &inv.output, &mut diags)?;
    let mut opts = encode_options(cli, target);

    // ---- policy (never loses data silently) ----
    apply_policy(
        &mut still,
        &mut opts,
        target,
        codec.format(),
        cli,
        &input.bytes,
        &mut diags,
    )?;

    if cli.dry_run {
        let mut report = Report::success("convert");
        report.input = Some(input_payload(&info));
        report.output = Some(OutputInfoPayload {
            format: target.as_str().into(),
            mode: Some("dry-run".into()),
            bytes: 0,
        });
        report.diagnostics.clone_from(&diags);
        emit(cli, &report, ReportStream::Stderr);
        emit_diagnostics(cli, &diags);
        return Ok(0);
    }

    // ---- encode via the codec registered for the TARGET format ----
    let target_codec = registry::all_codecs()
        .iter()
        .find(|c| c.format() == target)
        .ok_or_else(|| {
            WcError::UnsupportedInput(format!(
                "no codec registered for output format {}",
                target.as_str()
            ))
        })?;
    if !target_codec.caps().write {
        return Err(WcError::UnsupportedInput(format!(
            "this build cannot write {} (read-only codec)",
            target.as_str()
        )));
    }
    let bytes = target_codec.encode_still(&still, &opts).map_err(|e| {
        WcError::Internal(format!("could not encode {} output: {e}", target.as_str()))
    })?;
    deadline.check("encode")?;

    // ---- output routing ----
    match io::resolve_output(&inv.output)? {
        io::Output::Stdout => {
            // ADR-006: image bytes only on stdout; report goes to stderr.
            io::write_stdout(&bytes)?;
        }
        io::Output::File(path) => {
            io::ensure_writable(&path, cli.overwrite, input.path.as_deref())?;
            io::write_output_file(&path, &bytes)?;
        }
    }
    deadline.check("write")?;

    let mut report = Report::success("convert");
    report.input = Some(input_payload(&info));
    report.output = Some(output_payload(&inv.output, target, bytes.len()));
    report.diagnostics.clone_from(&diags);
    // JSON report must not touch stdout when the image is there
    let stream = if inv.output == "-" {
        ReportStream::Stderr
    } else {
        ReportStream::Stdout
    };
    emit(cli, &report, stream);
    emit_diagnostics(cli, &diags);
    Ok(0)
}

fn input_payload(info: &core::model::ImageInfo) -> InputInfoPayload {
    InputInfoPayload {
        format: info.format.clone(),
        width: info.width,
        height: info.height,
        frames: info.frames,
        has_alpha: info.has_alpha,
    }
}

fn output_payload(spec: &str, target: ImageFormat, bytes: usize) -> OutputInfoPayload {
    OutputInfoPayload {
        format: target.as_str().into(),
        mode: Some(if spec == "-" {
            "stdout".into()
        } else {
            spec.into()
        }),
        bytes: bytes as u64,
    }
}

/// Input codec: --from wins, then magic sniffing, then extension (ADR-003).
fn choose_input_codec(
    payload: &[u8],
    spec: &str,
    from: Option<ImageFormat>,
) -> Result<&'static (dyn Codec + 'static)> {
    let codecs = registry::all_codecs();
    if let Some(fmt) = from {
        return codecs
            .iter()
            .find(|c| c.format() == fmt)
            .map(|c| &**c)
            .ok_or_else(|| WcError::Internal(format!("no codec registered for {fmt}")));
    }
    if let Some(fmt) = format::sniff(payload) {
        return codecs
            .iter()
            .find(|c| c.format() == fmt)
            .map(|c| &**c)
            .ok_or_else(|| {
                WcError::UnsupportedInput(format!("detected {fmt} but no codec is registered"))
            });
    }
    let ext = std::path::Path::new(spec)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if let Ok(fmt) = ImageFormat::from_str(&ext) {
        if let Some(c) = codecs.iter().find(|c| c.format() == fmt) {
            return Ok(&**c);
        }
    }
    Err(WcError::UnsupportedInput(
        "could not determine the input format (magic bytes unrecognised; try --from FMT)".into(),
    ))
}

/// Route decode: --frame selection, single-frame stills, and the
/// never-silently-drop animation rule.
fn decode_still(codec: &dyn Codec, payload: &[u8], cli: &Cli) -> Result<StillImage> {
    if let Some(index) = cli.frame {
        return codec.decode_frame(payload, index);
    }

    match codec.decode(payload)? {
        Decoded::Still(s) => Ok(s),
        Decoded::Animated(a) if a.frames.len() <= 1 => {
            Ok(a.frames.into_iter().next().expect(">= 1 frame").still)
        }
        Decoded::Animated(a) => Err(WcError::UnsupportedInput(format!(
            "input contains {} animation frames; --frame N is required to pick one \
             (no silent dropping of frames)",
            a.frames.len()
        ))),
    }
}

/// Output format: --format/--to wins, then the output extension, then the
/// input format for stdout.
fn choose_target_format(
    codec: &dyn Codec,
    cli: &Cli,
    output_spec: &str,
    diags: &mut Vec<Diagnostic>,
) -> Result<ImageFormat> {
    let explicit = cli.format.or(cli.to);
    let target = if let Some(fmt) = explicit {
        fmt
    } else {
        let ext = std::path::Path::new(output_spec)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        match ImageFormat::from_str(&ext) {
            Ok(fmt) => fmt,
            Err(_) if output_spec == "-" => codec.format(),
            Err(_) => {
                return Err(WcError::InvalidArguments(format!(
                    "cannot determine the output format from extension '{ext}'; use --format FMT"
                )));
            }
        }
    };

    // extension mismatch note (informational, never fatal)
    if explicit.is_some() && output_spec != "-" {
        let ext = std::path::Path::new(output_spec)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        if !ext.is_empty() && !target.extensions().contains(&ext.as_str()) {
            diags.push(Diagnostic::warning(
                codes::EXTENSION_MISMATCH,
                format!(
                    "output extension '.{ext}' does not match format {}",
                    target.as_str()
                ),
            ));
        }
    }
    Ok(target)
}

fn encode_options(cli: &Cli, target: ImageFormat) -> EncodeOptions {
    EncodeOptions {
        format: target,
        quality: cli.quality,
        lossless: cli.lossless,
        method: cli.method,
        alpha_quality: cli.alpha_quality,
        near_lossless: cli.near_lossless,
        background: cli.background,
        orient: cli.orient,
        metadata: cli.metadata,
        frame: cli.frame,
        flatten: cli.flatten,
    }
}

/// Data-loss policy: option validity, alpha flattening, metadata, orientation
/// (ADR-005, ADR-008). Every prevented loss becomes a precise error or a
/// diagnostic — never a quiet rewrite.
fn apply_policy(
    still: &mut StillImage,
    opts: &mut EncodeOptions,
    target: ImageFormat,
    input_format: ImageFormat,
    cli: &Cli,
    payload: &[u8],
    diags: &mut Vec<Diagnostic>,
) -> Result<()> {
    let _ = cli;

    // -- lossless/near-lossless only mean something for WebP --------------
    if (opts.lossless || opts.near_lossless.is_some()) && target != ImageFormat::WebP {
        return Err(WcError::InvalidArguments(format!(
            "--lossless/--near-lossless applies to webp output only (target is {})",
            target.as_str()
        )));
    }
    if opts.near_lossless.is_some() && cli_lossy(opts) {
        return Err(WcError::InvalidArguments(
            "--near-lossless implies lossless; --lossy conflicts with it".into(),
        ));
    }

    // -- alpha flattening: never implicit ---------------------------------
    if still.has_alpha && !target.supports_alpha() {
        match opts.background {
            Some(bg) => {
                *still = rgba::flatten_background(still, bg);
                diags.push(Diagnostic::info(
                    codes::ALPHA_FLATTENED,
                    format!(
                        "alpha flattened onto background #{:02X}{:02X}{:02X}",
                        bg[0], bg[1], bg[2]
                    ),
                ));
            }
            None => {
                return Err(WcError::MissingOption(format!(
                    "target {} cannot store alpha; --background #RRGGBB is required to flatten \
                     (no silent loss)",
                    target.as_str()
                )));
            }
        }
    }

    // -- orientation -------------------------------------------------------
    let orientation = core::metadata::exif_orientation(payload).filter(|o| *o != 1);
    if let Some(o) = orientation {
        match opts.orient {
            OrientMode::Auto => {
                *still = rgba::apply_orientation(still, o)?;
                diags.push(Diagnostic::info(
                    "orientation_applied",
                    format!("EXIF orientation {o} applied to pixels"),
                ));
            }
            OrientMode::None => {
                diags.push(Diagnostic::warning(
                    codes::ORIENTATION_IGNORED,
                    format!(
                        "EXIF orientation {o} kept as stored (--orient none); \
                         the output may display rotated"
                    ),
                ));
            }
        }
    }

    // -- metadata (ADR-008): v0 detects presence and diagnoses drops -------
    match opts.metadata {
        MetadataMode::All | MetadataMode::Icc => {
            let presence = core::metadata::probe(payload, input_format);
            if opts.metadata == MetadataMode::Icc && !presence.icc {
                diags.push(Diagnostic::warning(
                    codes::METADATA_DROPPED,
                    "--metadata icc was requested but no ICC profile is present",
                ));
            }
            if presence.any() {
                diags.push(Diagnostic::warning(
                    codes::METADATA_DROPPED,
                    format!(
                        "input carries {} metadata; writing metadata to the output is not \
                         supported in this build, so it will be dropped",
                        presence.names().join(", ")
                    ),
                ));
            }
        }
        MetadataMode::None => {
            if core::metadata::probe(payload, input_format).any() {
                diags.push(Diagnostic::info(
                    "metadata_stripped",
                    "metadata stripped as requested (--metadata none)",
                ));
            }
        }
    }

    Ok(())
}

fn cli_lossy(_opts: &EncodeOptions) -> bool {
    // --lossy is validated at the args layer (conflicts_with lossless);
    // near-lossless + lossy is checked there too. Kept as a hook.
    false
}

// ---------------------------------------------------------------------------
// inspect / formats / capabilities
// ---------------------------------------------------------------------------

fn run_inspect(cli: &Cli, input_spec: &str) -> Result<i32> {
    let input = io::read_input(input_spec, cli.max_input_bytes)?;
    let codec = choose_input_codec(&input.bytes, &input.origin, cli.from)?;
    let info = codec.inspect(&input.bytes)?;

    if cli.json {
        let mut report = Report::success("inspect");
        report.input = Some(input_payload(&info));
        println!("{}", report.to_pretty());
    } else {
        let anim = match info.animated {
            Some(true) => format!(", {} frames, animated", info.frames),
            Some(false) if info.frames > 1 => format!(", {} page(s)", info.frames),
            _ => String::new(),
        };
        eprintln!(
            "{}: {} {}x{}, alpha: {}{}",
            input.origin,
            info.format,
            info.width,
            info.height,
            if info.has_alpha { "yes" } else { "no" },
            anim,
        );
    }
    Ok(0)
}

fn run_formats(cli: &Cli) -> Result<i32> {
    let codecs = registry::all_codecs();
    if cli.json {
        let mut report = Report::success("formats");
        report.capabilities = Some(core::protocol::CapabilityRegistry::build(codecs));
        println!("{}", report.to_pretty());
    } else {
        let names: Vec<String> = codecs.iter().map(|c| c.caps().format).collect();
        eprintln!("supported formats: {}", names.join(", "));
    }
    Ok(0)
}

fn run_capabilities(cli: &Cli, filter: Option<&str>) -> Result<i32> {
    let codecs = registry::all_codecs();
    let caps: Vec<_> = codecs
        .iter()
        .map(|c| c.caps())
        .filter(|c| filter.is_none_or(|f| c.format == f))
        .collect();
    if let Some(f) = filter {
        if caps.is_empty() {
            return Err(WcError::InvalidArguments(format!(
                "no codec registered for format '{f}'"
            )));
        }
    }
    if cli.json {
        let mut report = Report::success("capabilities");
        report.capabilities = Some(caps);
        println!("{}", report.to_pretty());
    } else {
        for c in &caps {
            eprintln!(
                "{}: read={} write={} animation(r/w)={}/{} alpha(r/w)={}/{} backend={}",
                c.format,
                c.read,
                c.write,
                c.animation_read,
                c.animation_write,
                c.alpha_read,
                c.alpha_write,
                c.backend,
            );
            for r in &c.restrictions {
                eprintln!("  - {r}");
            }
        }
    }
    Ok(0)
}
