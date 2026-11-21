//! Error taxonomy with stable codes and exit codes (plan §7.5, ADR-005).
//!
//! Exit codes: 0 success, 2 invalid arguments, 3 corrupt/unreadable/unsupported
//! input, 4 resource limit exceeded, 5 output error, 6 internal error,
//! 7 protocol error.

use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum WcError {
    /// Exit 2
    #[error("{0}")]
    InvalidArguments(String),
    /// Exit 2 -- an option combination is missing something mandatory
    /// (e.g. JPEG target needs `--background` for alpha input)
    #[error("{0}")]
    MissingOption(String),
    /// Exit 3
    #[error("cannot read input as any supported image: {0}")]
    UnsupportedInput(String),
    /// Exit 3
    #[error("corrupt or truncated input: {0}")]
    CorruptInput(String),
    /// Exit 4
    #[error("resource limit exceeded: {0}")]
    ResourceLimit(String),
    /// Exit 5
    #[error("output error: {0}")]
    Output(String),
    /// Exit 6
    #[error("internal error: {0}")]
    Internal(String),
    /// Exit 7
    #[error("protocol error: {0}")]
    Protocol(String),
}

impl WcError {
    /// Stable machine-readable code for the JSON protocol.
    pub fn code(&self) -> &'static str {
        match self {
            WcError::InvalidArguments(_) => "invalid_arguments",
            WcError::MissingOption(_) => "missing_option",
            WcError::UnsupportedInput(_) => "unsupported_input",
            WcError::CorruptInput(_) => "corrupt_input",
            WcError::ResourceLimit(_) => "resource_limit_exceeded",
            WcError::Output(_) => "output_error",
            WcError::Internal(_) => "internal_error",
            WcError::Protocol(_) => "protocol_error",
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            WcError::InvalidArguments(_) | WcError::MissingOption(_) => 2,
            WcError::UnsupportedInput(_) | WcError::CorruptInput(_) => 3,
            WcError::ResourceLimit(_) => 4,
            WcError::Output(_) => 5,
            WcError::Internal(_) => 6,
            WcError::Protocol(_) => 7,
        }
    }
}

pub type Result<T, E = WcError> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_match_plan() {
        assert_eq!(WcError::InvalidArguments("x".into()).exit_code(), 2);
        assert_eq!(WcError::UnsupportedInput("x".into()).exit_code(), 3);
        assert_eq!(WcError::CorruptInput("x".into()).exit_code(), 3);
        assert_eq!(WcError::ResourceLimit("pixels".into()).exit_code(), 4);
        assert_eq!(WcError::Output("write failed".into()).exit_code(), 5);
        assert_eq!(WcError::Internal("panic-like".into()).exit_code(), 6);
        assert_eq!(WcError::Protocol("bad schema".into()).exit_code(), 7);
    }

    #[test]
    fn codes_are_stable() {
        // frozen contract examples from plan §7.4
        assert_eq!(
            WcError::InvalidArguments("".into()).code(),
            "invalid_arguments"
        );
    }
}
