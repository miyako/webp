//! Core types, error taxonomy, JSON protocol, image model, conversion policy.
//!
//! The crate name in code is `webp_convert_core`; binary/crate names use
//! dashes, Rust identifiers use underscores.

pub mod codec;
pub mod diagnostics;
pub mod error;
pub mod format;
pub mod metadata;
pub mod model;
pub mod policy;
pub mod protocol;
pub mod rgba;

pub use codec::Codec;
pub use diagnostics::{Diagnostic, Severity};
pub use error::{Result, WcError};
pub use format::ImageFormat;
pub use model::{AnimatedImage, Decoded, Frame, ImageInfo, SourceInfo, StillImage};
pub use policy::{EncodeOptions, MetadataMode, OrientMode, Orientation};
pub use protocol::{
    CapabilityRegistry, ErrorBody, FormatCaps, InputInfoPayload, OutputInfoPayload, Report,
};
