//! CLI argument surface (plan §6).

use clap::{ArgAction, Parser, Subcommand};
use webp_convert_core::error::WcError;
use webp_convert_core::format::ImageFormat;
use webp_convert_core::policy::{MetadataMode, OrientMode};

#[derive(Clone, Parser)]
#[command(
    name = "webp-convert",
    version,
    about = "Convert to and from the WebP image format",
    long_about = "Portable, self-contained image conversion CLI. Supports WebP, PNG, JPEG, GIF, BMP and TIFF.",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Input file, or `-` for stdin (bare form: `webp-convert INPUT OUTPUT`)
    #[arg(value_name = "INPUT", conflicts_with = "input_opt")]
    pub input_: Option<String>,

    /// Output file, or `-` for stdout (bare form only)
    #[arg(value_name = "OUTPUT", conflicts_with = "output_opt")]
    pub output_: Option<String>,

    /// Operation (default: convert)
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Explicit input file or `-`
    #[arg(short = 'i', long, value_name = "FILE")]
    pub input_opt: Option<String>,

    /// Explicit output file or `-`
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output_opt: Option<String>,

    /// Explicit input format
    #[arg(long, value_name = "FMT")]
    pub from: Option<ImageFormat>,

    /// Explicit output format (overrides the output extension)
    #[arg(long, value_name = "FMT", conflicts_with = "to")]
    pub format: Option<ImageFormat>,

    /// Explicit output format (alias of --format)
    #[arg(long, value_name = "FMT")]
    pub to: Option<ImageFormat>,

    /// Quality 0-100 (codec-specific meaning)
    #[arg(short = 'q', long, value_name = "0-100")]
    pub quality: Option<f32>,

    /// Lossless WebP output
    #[arg(long)]
    pub lossless: bool,

    /// Lossy WebP output (explicit default)
    #[arg(long, conflicts_with = "lossless")]
    pub lossy: bool,

    /// Encoder effort 0-6 (WebP `method`; higher = slower/smaller)
    #[arg(long, value_name = "0-6")]
    pub method: Option<u8>,

    /// Alpha compression quality 0-100 (WebP)
    #[arg(long, value_name = "0-100")]
    pub alpha_quality: Option<u8>,

    /// Near-lossless mode 0-100 (WebP; implies lossless)
    #[arg(long, value_name = "0-100", conflicts_with = "lossy")]
    pub near_lossless: Option<u8>,

    /// Flattening background (required for alpha-losing targets)
    #[arg(long, value_name = "COLOR", value_parser = parse_color)]
    pub background: Option<[u8; 3]>,

    /// Metadata policy: all | icc | none
    #[arg(long, value_name = "MODE", default_value = "all")]
    pub metadata: MetadataMode,

    /// Orientation handling: auto (apply EXIF) | none (keep as stored)
    #[arg(long, value_enum, default_value = "none")]
    pub orient: OrientMode,

    /// Flatten an animated input (explicit; not yet implemented)
    #[arg(long)]
    pub flatten: bool,

    /// Select frame N of an animated input (0-based)
    #[arg(long, value_name = "N")]
    pub frame: Option<u32>,

    /// Select page N of a multi-page input (0-based)
    #[arg(long, value_name = "N")]
    pub page: Option<u32>,

    /// Maximum pixel count accepted, in megapixels
    #[arg(long, value_name = "MP", default_value_t = 100)]
    pub max_pixels_mp: u32,

    /// Maximum accepted input size in bytes
    #[arg(long, value_name = "BYTES", default_value_t = 512 * 1024 * 1024)]
    pub max_input_bytes: u64,

    /// Overwrite an existing output file (off by default)
    #[arg(long)]
    pub overwrite: bool,

    /// Validate the pipeline and report without writing any output
    #[arg(long)]
    pub dry_run: bool,

    /// Machine-readable JSON reporting
    #[arg(long, global = true)]
    pub json: bool,

    /// Suppress informational stderr messages
    #[arg(long, action = ArgAction::SetTrue, global = true)]
    pub quiet: bool,
}

fn parse_color(s: &str) -> Result<[u8; 3], String> {
    let hex = s
        .strip_prefix('#')
        .or_else(|| s.strip_prefix("0x"))
        .unwrap_or(s);
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!(
            "COLOR must be RRGGBB hex, e.g. \"#ffffff\" (got {s:?})"
        ));
    }
    Ok([
        u8::from_str_radix(&hex[0..2], 16).map_err(|e| e.to_string())?,
        u8::from_str_radix(&hex[2..4], 16).map_err(|e| e.to_string())?,
        u8::from_str_radix(&hex[4..6], 16).map_err(|e| e.to_string())?,
    ])
}

#[derive(Subcommand, Clone)]
pub enum Command {
    /// Convert INPUT to OUTPUT (the default operation)
    Convert {
        /// Input file, or `-` for stdin
        input: String,
        /// Output file, or `-` for stdout
        output: String,
    },
    /// Inspect an image without converting
    Inspect {
        /// Input file, or `-` for stdin
        input: String,
    },
    /// List supported formats
    Formats,
    /// Print the machine-readable capability registry
    Capabilities {
        /// Optional format filter (e.g. `webp`)
        #[arg(value_name = "FMT")]
        format: Option<String>,
    },
}

/// Parse with stable exit-code mapping: clap usage errors must exit 2,
/// never 1.
pub fn parse_or_exit(args: Vec<String>) -> (Cli, Option<Command>) {
    match Cli::try_parse_from(args) {
        Ok(cli) => {
            let cmd = cli.command.clone();
            (cli, cmd)
        }
        Err(e) => {
            e.print().ok();
            // help/version go to stdout with exit 0; usage errors exit 2
            let code = if e.use_stderr() { 2 } else { 0 };
            std::process::exit(code);
        }
    }
}

/// The canonical parsed view shared by both call forms.
pub struct ConvertInvocation {
    pub input: String,
    pub output: String,
}

/// Resolve input/output from either the bare positional form
/// (`webp-convert IN OUT`) or the `convert` subcommand form.
pub fn resolved_io(cli: &Cli, cmd: &Option<Command>) -> Result<ConvertInvocation, WcError> {
    match cmd {
        Some(Command::Convert { input, output }) => {
            if cli.input_opt.is_some() || cli.output_opt.is_some() {
                return Err(WcError::InvalidArguments(
                    "--input/--output cannot be combined with the positional INPUT OUTPUT form"
                        .into(),
                ));
            }
            Ok(ConvertInvocation {
                input: input.clone(),
                output: output.clone(),
            })
        }
        Some(Command::Inspect { .. })
        | Some(Command::Formats)
        | Some(Command::Capabilities { .. }) => {
            Err(WcError::Internal("not a convert command".into()))
        }
        None => {
            let input = cli.input_opt.clone().or_else(|| cli.input_.clone());
            let output = cli.output_opt.clone().or_else(|| cli.output_.clone());
            match (input, output) {
                (Some(i), Some(o)) => Ok(ConvertInvocation { input: i, output: o }),
                (Some(_), None) => Err(WcError::InvalidArguments(
                    "missing OUTPUT (see --help)".into(),
                )),
                _ => Err(WcError::InvalidArguments(
                    "usage: webp-convert INPUT OUTPUT | webp-convert inspect INPUT | webp-convert formats | webp-convert capabilities --json".into(),
                )),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_color_parsing() {
        assert_eq!(parse_color("#ffffff"), Ok([255, 255, 255]));
        assert_eq!(parse_color("ffffff"), Ok([255, 255, 255]));
        assert_eq!(parse_color("0xff00aa"), Ok([255, 0, 170]));
        assert!(parse_color("#zzz").is_err());
        assert!(parse_color("#12345").is_err());
    }

    #[test]
    fn bare_form_is_valid() {
        let args = vec![
            "webp-convert".to_string(),
            "in.png".to_string(),
            "out.webp".to_string(),
        ];
        let (cli, cmd) = parse_or_exit_test(args);
        assert!(cmd.is_none());
        assert_eq!(cli.input_.as_deref(), Some("in.png"));
        assert_eq!(cli.output_.as_deref(), Some("out.webp"));
    }

    #[test]
    fn subcommand_form_is_valid() {
        let args = vec![
            "webp-convert".to_string(),
            "convert".to_string(),
            "in.png".to_string(),
            "out.webp".to_string(),
        ];
        let (_, cmd) = parse_or_exit_test(args);
        assert!(matches!(cmd, Some(Command::Convert { .. })));
    }

    fn parse_or_exit_test(args: Vec<String>) -> (Cli, Option<Command>) {
        let cli = Cli::try_parse_from(args).unwrap_or_else(|e| panic!("{e}"));
        let cmd = cli.command.clone();
        (cli, cmd)
    }
}
