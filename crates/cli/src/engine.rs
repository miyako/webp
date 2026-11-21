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
    model::{AnimatedImage, Decoded, StillImage},
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
    let info = codec.inspect(&input.bytes).map_err(|e| match e {
        // resource-limit findings from header checks keep their exit
        // semantics; only decode failures become corrupt-input errors
        e @ WcError::ResourceLimit(_) => e,
        e => WcError::CorruptInput(format!("could not read {}: {e}", input.origin)),
    })?;

    // pixel budget comes from the declared geometry, before any decode work
    // (decoders composite onto full-canvas buffers for animated input)
    let info_pixels = u64::from(info.width) * u64::from(info.height);
    let max_pixels = u64::from(cli.max_pixels_mp) * 1_000_000;
    if info_pixels > max_pixels {
        return Err(WcError::ResourceLimit(format!(
            "image canvas is {}x{} ({info_pixels} pixels); exceeds --max-pixels-mp {} ({max_pixels} pixels)",
            info.width, info.height, cli.max_pixels_mp
        )));
    }

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

    // ---- decode (still, page, frame, or animation) ----
    let decoded = decode_input(codec, &input.bytes, cli, &info)?;
    deadline.check("decode")?;

    // ---- limits (canvas: one frame of an animation) -------
    let canvas = match &decoded {
        Decoded::Still(s) => (s.pixel_count(), s.width, s.height),
        Decoded::Animated(a) => (u64::from(a.width) * u64::from(a.height), a.width, a.height),
    };
    if canvas.0 > max_pixels {
        return Err(WcError::ResourceLimit(format!(
            "image canvas is {}x{} ({} pixels); exceeds --max-pixels-mp {} ({} pixels)",
            canvas.1, canvas.2, canvas.0, cli.max_pixels_mp, max_pixels
        )));
    }

    // ---- target format + options ----
    let target = choose_target_format(codec, cli, &inv.output, &mut diags)?;
    let mut opts = encode_options(cli, target);

    // ---- route: still vs animated ----
    // Animated input accepts three explicit exits: --frame (a still frame),
    // --flatten (first frame as a still, warned), or an animation-capable
    // target. Enforced above in decode_input / below here (no silent drops).
    enum Route {
        Still(StillImage),
        Animated(AnimatedImage),
    }
    let mut route = match decoded {
        Decoded::Still(s) => Route::Still(s),
        Decoded::Animated(a) => {
            if cli.flatten {
                let n = a.frames.len();
                let first = a
                    .frames
                    .into_iter()
                    .next()
                    .ok_or_else(|| WcError::Internal("animated image with no frames".into()))?;
                diags.push(Diagnostic::warning(
                    codes::ANIMATION_FLATTENED,
                    format!("flattened {n}-frame animation to its first frame (--flatten)"),
                ));
                Route::Still(first.still)
            } else {
                let target_codecs_animate = registry::all_codecs()
                    .iter()
                    .find(|c| c.format() == target)
                    .map(|c| c.caps().animation_write)
                    .unwrap_or(false);
                if !target_codecs_animate {
                    return Err(WcError::UnsupportedInput(format!(
                        "input contains {} animation frames and {} cannot store \
                         animations; use --frame N, --flatten, or write to webp/gif \
                         (no silent dropping of frames)",
                        a.frames.len(),
                        target.as_str()
                    )));
                }
                // alpha policy for the animated route: GIF quantizes partial
                // alpha to 1-bit; PNG-style full alpha only reaches webp
                if a.frames.iter().any(|f| f.still.has_alpha)
                    && a.frames.iter().any(|f| {
                        f.still
                            .rgba
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .any(|p| p[3] != 0 && p[3] != 255)
                    })
                    && !target.supports_alpha()
                {
                    // unreachable today (animated targets are gif/webp, both
                    // alpha-capable); kept so the rule survives codec growth
                    return Err(WcError::MissingOption(format!(
                        "target {} cannot store alpha; --background #RRGGBB is required",
                        target.as_str()
                    )));
                }
                if target == ImageFormat::Gif
                    && a.frames.iter().any(|f| {
                        f.still
                            .rgba
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .any(|p| p[3] > 0 && p[3] < 255)
                    })
                {
                    diags.push(Diagnostic::warning(
                        codes::ALPHA_REDUCED,
                        "partial alpha values are reduced to 1-bit transparency by GIF",
                    ));
                }
                // metadata write-through applies to animated outputs too
                // (no orientation patch: pixels are not rotated on this route)
                resolve_metadata_out(
                    &input.bytes,
                    codec.format(),
                    target,
                    &mut opts,
                    false,
                    &mut diags,
                );
                Route::Animated(a)
            }
        }
    };

    // ---- policy (never loses data silently) ----
    let still_for_policy = match &mut route {
        Route::Still(still) => Some(still),
        Route::Animated(_) => None, // per-frame loss rules applied above
    };
    if let Some(still) = still_for_policy {
        apply_policy(
            still,
            &mut opts,
            target,
            codec.format(),
            cli,
            &input.bytes,
            &mut diags,
        )?;
    }

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
    let bytes = match route {
        Route::Still(still) => target_codec.encode_still(&still, &opts).map_err(|e| {
            WcError::Internal(format!("could not encode {} output: {e}", target.as_str()))
        })?,
        Route::Animated(anim) => target_codec.encode_animated(&anim, &opts).map_err(|e| {
            WcError::Internal(format!(
                "could not encode {} animation: {e}",
                target.as_str()
            ))
        })?,
    };
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

/// Route decode: --page selection for multi-page containers, --frame
/// selection, single-frame stills, and the never-silently-drop animation
/// rule (animated-output routing lives in run_convert).
fn decode_input(
    codec: &dyn Codec,
    payload: &[u8],
    cli: &Cli,
    _info: &core::model::ImageInfo,
) -> Result<Decoded> {
    if let Some(page) = cli.page {
        // frame and page selection are mutually exclusive concerns; clap
        // allows both, the engine rejects the ambiguous combination
        if cli.frame.is_some() {
            return Err(WcError::InvalidArguments(
                "--page and --frame cannot be combined (pages pick container pages, \
                 frames pick animation frames)"
                    .into(),
            ));
        }
        return codec.decode_page(payload, page).map(Decoded::Still);
    }
    if let Some(index) = cli.frame {
        return codec.decode_frame(payload, index).map(Decoded::Still);
    }

    match codec.decode(payload)? {
        Decoded::Still(s) => Ok(Decoded::Still(s)),
        Decoded::Animated(a) if a.frames.len() <= 1 => Ok(Decoded::Still(
            a.frames
                .into_iter()
                .next()
                .ok_or_else(|| WcError::Internal("animated image with no frames".into()))?
                .still,
        )),
        // multi-frame animations are always handed through: run_convert owns
        // the routing decision (--flatten → first frame, --frame handled
        // above, animation-capable target, or a loud refusal — never silent)
        Decoded::Animated(a) => Ok(Decoded::Animated(a)),
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
        metadata_out: core::metadata::RawMetadata::default(),
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
    // 16-bit TIFF input is normalized to 8-bit by the decoder — say so
    // (plan §5.4: bit-depth reduction is a loss and must not be silent).
    if input_format == ImageFormat::Tiff
        && core::metadata::tiff_max_bits_per_sample(payload).is_some_and(|b| b > 8)
    {
        diags.push(Diagnostic::info(
            codes::BIT_DEPTH_REDUCED,
            "16-bit samples normalized to 8-bit on decode",
        ));
    }

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

    // -- metadata (ADR-008 + plan §5.6): per-category write-through with
    //    loud diagnosis of everything that is not carried -------------------
    let pixel_orientation_changed = orientation.is_some() && opts.orient == OrientMode::Auto;
    resolve_metadata_out(
        payload,
        input_format,
        target,
        opts,
        pixel_orientation_changed,
        diags,
    );

    Ok(())
}

/// Per-category metadata write-through decision (plan §5.6 / ADR-008):
/// extract what the input carries, attach what the target can store and the
/// mode allows, and loudly diagnose every other category — never drop in
/// silence. Also used for the animated route (no orientation patch there).
fn resolve_metadata_out(
    payload: &[u8],
    input_format: ImageFormat,
    target: ImageFormat,
    opts: &mut EncodeOptions,
    pixel_orientation_changed: bool,
    diags: &mut Vec<Diagnostic>,
) {
    let target_supports = |cat: &str| -> bool {
        registry::all_codecs()
            .iter()
            .find(|c| c.format() == target)
            .map(|c| c.caps().metadata_write.iter().any(|n| n == cat))
            .unwrap_or(false)
    };
    let presence = core::metadata::probe(payload, input_format);
    let raw = core::metadata::extract(payload, input_format);
    if opts.metadata == MetadataMode::Icc && !presence.icc {
        diags.push(Diagnostic::warning(
            codes::METADATA_DROPPED,
            "--metadata icc was requested but no ICC profile is present",
        ));
    }
    let categories = [
        (presence.icc, raw.icc.clone(), "icc", 0),
        (presence.exif, raw.exif.clone(), "exif", 1),
        (presence.xmp, raw.xmp.clone(), "xmp", 2),
    ];
    let mut carried: Vec<&str> = Vec::new();
    for (is_present, bytes, cat, slot) in categories {
        if !is_present {
            continue;
        }
        let allowed = match opts.metadata {
            MetadataMode::All => true,
            MetadataMode::Icc => cat == "icc",
            MetadataMode::None => false,
        };
        if !allowed {
            diags.push(Diagnostic::info(
                codes::METADATA_STRIPPED,
                format!(
                    "{cat} metadata stripped as requested (--metadata {})",
                    opts.metadata.as_str()
                ),
            ));
            continue;
        }
        match bytes {
            Some(bytes) if target_supports(cat) => {
                // rotating pixels must not leave a stale orientation tag:
                // downstream viewers would rotate the image a second time
                let bytes = if pixel_orientation_changed && cat == "exif" {
                    match core::metadata::patch_exif_orientation_to_identity(&bytes) {
                        Some(patched) => patched,
                        None => {
                            diags.push(Diagnostic::warning(
                                codes::METADATA_DROPPED,
                                "EXIF orientation could not be rewritten after rotating \
                                 pixels; EXIF dropped to avoid double rotation",
                            ));
                            continue;
                        }
                    }
                } else {
                    bytes
                };
                match slot {
                    0 => opts.metadata_out.icc = Some(bytes),
                    1 => opts.metadata_out.exif = Some(bytes),
                    _ => opts.metadata_out.xmp = Some(bytes),
                }
                carried.push(cat);
            }
            Some(_) => {
                diags.push(Diagnostic::warning(
                    codes::METADATA_DROPPED,
                    format!(
                        "input carries {cat} metadata but the {cat} payload could not be \
                         extracted or {} cannot store it, so it will be dropped",
                        target.as_str()
                    ),
                ));
            }
            None => {
                diags.push(Diagnostic::warning(
                    codes::METADATA_DROPPED,
                    format!(
                        "input carries {cat} metadata but {cat} extraction failed; \
                         it will be dropped (no partial/stale metadata is ever written)"
                    ),
                ));
            }
        }
    }
    if !carried.is_empty() {
        diags.push(Diagnostic::info(
            codes::METADATA_CARRIED,
            format!("metadata carried to output: {}", carried.join(", ")),
        ));
    }
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
