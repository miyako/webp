//! Conversion policy input options (plan §5/§6.4). The policy layer never
//! sees codec internals; codecs see only these intents.

use crate::format::ImageFormat;

/// EXIF orientation values 1..=8 (0 or none = unknown/absent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Orientation(pub Option<u8>);

impl Orientation {
    pub fn is_identity(self) -> bool {
        matches!(
            self,
            Orientation(None) | Orientation(Some(1)) | Orientation(Some(0))
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum MetadataMode {
    /// preserve what the target supports and diagnose the rest (ADR-008)
    All,
    /// preserve ICC only
    Icc,
    /// strip everything (still diagnosed)
    None,
}

impl MetadataMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetadataMode::All => "all",
            MetadataMode::Icc => "icc",
            MetadataMode::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum OrientMode {
    /// apply EXIF orientation to pixels
    Auto,
    /// keep pixels as stored, report orientation only
    None,
}

/// Encoder-independent intent for producing output bytes.
#[derive(Debug, Clone)]
pub struct EncodeOptions {
    pub format: ImageFormat,
    /// 0..=100, codec-specific meaning (documented; not portable across codecs)
    pub quality: Option<f32>,
    pub lossless: bool,
    /// encoder effort 0..=6 (webp `method`)
    pub method: Option<u8>,
    /// 0..=100 alpha compression quality (webp)
    pub alpha_quality: Option<u8>,
    /// webp near-lossless 0=off
    pub near_lossless: Option<u8>,
    /// flattening background for alpha-less targets; None = never flatten
    pub background: Option<[u8; 3]>,
    /// EXIF orientation to apply (`--orient auto`) or keep (`none`)
    pub orient: OrientMode,
    pub metadata: MetadataMode,
    /// select frame N of an animated source (0-based)
    pub frame: Option<u32>,
    /// explicit animation flattening (P1; refused in v0 with clear error)
    pub flatten: bool,
}

impl EncodeOptions {
    pub fn for_target(format: ImageFormat) -> Self {
        EncodeOptions {
            format,
            quality: None,
            lossless: false,
            method: None,
            alpha_quality: None,
            near_lossless: None,
            background: None,
            orient: OrientMode::None,
            metadata: MetadataMode::All,
            frame: None,
            flatten: false,
        }
    }
}
