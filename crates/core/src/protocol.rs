//! Versioned JSON protocol (plan §7, ADR-005). Protocol version 1.
//!
//! Stout rule (ADR-006): when the image goes to stdout (`-`), reports must
//! not contaminate the byte stream; the CLI routes reports to stderr then.

use serde::{Deserialize, Serialize};

use crate::diagnostics::Diagnostic;
use crate::error::WcError;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputInfoPayload {
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub has_alpha: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputInfoPayload {
    pub format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub protocol: u32,
    pub ok: bool,
    pub operation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<InputInfoPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<OutputInfoPayload>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
    /// Format capability registry (`formats`/`capabilities` commands)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<FormatCaps>>,
}

impl Report {
    pub fn success(operation: &str) -> Self {
        Report {
            protocol: PROTOCOL_VERSION,
            ok: true,
            operation: operation.into(),
            input: None,
            output: None,
            diagnostics: Vec::new(),
            error: None,
            capabilities: None,
        }
    }

    pub fn failure(operation: &str, err: &WcError) -> Self {
        Report {
            protocol: PROTOCOL_VERSION,
            ok: false,
            operation: operation.into(),
            input: None,
            output: None,
            diagnostics: Vec::new(),
            error: Some(ErrorBody {
                code: err.code().into(),
                message: err.to_string(),
            }),
            capabilities: None,
        }
    }

    pub fn to_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|e| {
            // Never fail to produce a report; degrade to protocol error JSON.
            format!(
                "{{\"protocol\":1,\"ok\":false,\"error\":{{\"code\":\"protocol_error\",\"message\":\"{e}\"}}}}"
            )
        })
    }
}

/// A codec's advertised capabilities (plan §8). The registry reports what
/// the built binary actually contains; entries always come from codec code,
/// never from documentation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatCaps {
    pub format: String,
    pub read: bool,
    pub write: bool,
    pub animation_read: bool,
    pub animation_write: bool,
    pub alpha_read: bool,
    pub alpha_write: bool,
    pub metadata_read: Vec<String>,
    pub metadata_write: Vec<String>,
    pub pages_read: bool,
    pub bit_depth_limit: String,
    pub backend: String,
    pub backend_version: String,
    pub restrictions: Vec<String>,
}

/// Registry of the process's real codecs; powers `formats`, `capabilities`,
/// docs tables and tests. Built by the CLI binary from its linked codecs.
pub struct CapabilityRegistry;

impl CapabilityRegistry {
    /// Build from the actually linked codec set.
    pub fn build(codecs: &[Box<dyn crate::codec::Codec>]) -> Vec<FormatCaps> {
        codecs.iter().map(|c| c.caps()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::Severity;

    #[test]
    fn success_report_matches_shape() {
        let r = Report::success("convert");
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["protocol"], 1);
        assert_eq!(json["ok"], true);
        assert_eq!(json["operation"], "convert");
        // omit sends skipped
        assert!(json.get("input").is_none());
        assert!(json.get("diagnostics").is_none());
    }

    #[test]
    fn warning_report_matches_plan_example() {
        let mut r = Report::success("convert");
        r.diagnostics.push(Diagnostic::warning(
            "metadata_dropped",
            "XMP metadata was not preserved.",
        ));
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["diagnostics"][0]["severity"], "warning");
        assert_eq!(json["diagnostics"][0]["code"], "metadata_dropped");
    }

    #[test]
    fn severity_roundtrip() {
        assert_eq!(
            serde_json::to_value(Severity::Warning).unwrap(),
            serde_json::json!("warning")
        );
    }
}
