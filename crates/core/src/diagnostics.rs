//! Diagnostics: warnings and info emitted alongside results (plan §7.3).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
}

impl Diagnostic {
    pub fn warning(code: &str, message: impl Into<String>) -> Self {
        Diagnostic {
            code: code.into(),
            severity: Severity::Warning,
            message: message.into(),
        }
    }

    pub fn info(code: &str, message: impl Into<String>) -> Self {
        Diagnostic {
            code: code.into(),
            severity: Severity::Info,
            message: message.into(),
        }
    }
}

/// Codes used across the pipeline (kept in one place for the docs table).
pub mod codes {
    pub const EXTENSION_MISMATCH: &str = "extension_mismatch";
    pub const METADATA_DROPPED: &str = "metadata_dropped";
    pub const ALPHA_FLATTENED: &str = "alpha_flattened";
    pub const ALPHA_REDUCED: &str = "alpha_reduced";
    pub const ANIMATION_FLATTENED: &str = "animation_flattened";
    pub const BIT_DEPTH_REDUCED: &str = "bit_depth_reduced";
    pub const OPTIONS_IGNORED: &str = "options_ignored";
    pub const PREMULTIPLIED_SAMPLED_ALPHA: &str = "lossy_alpha";
    pub const ORIENTATION_IGNORED: &str = "orientation_ignored";
    pub const GIF_PALETTE: &str = "gif_palette_quantized";
}
