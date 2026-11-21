//! Internal image model (ADR-004). A still image is a one-frame animation.

use serde::Serialize;

/// Structured description of an input, used by `inspect` and reports.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ImageInfo {
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub has_alpha: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loop_count: Option<i32>,
    /// raw container bytes that hold metadata (from the presence probe)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<crate::metadata::MetadataPresence>,
}

/// Where an image came from (file path or stdin), for diagnostics only.
#[derive(Debug, Clone)]
pub enum SourceInfo {
    File(String),
    Stdin,
}

impl std::fmt::Display for SourceInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceInfo::File(p) => write!(f, "{p}"),
            SourceInfo::Stdin => write!(f, "<stdin>"),
        }
    }
}

/// A raw still image in canonical RGBA8 layout (4 bytes/pixel, row-major).
#[derive(Debug, Clone)]
pub struct StillImage {
    pub width: u32,
    pub height: u32,
    /// len == width*height*4
    pub rgba: Vec<u8>,
    /// whether any pixel has alpha < 255 (informational; also set when the
    /// decoder knows alpha exists)
    pub has_alpha: bool,
    pub source: Option<SourceInfo>,
}

impl StillImage {
    pub fn from_rgba8(width: u32, height: u32, rgba: Vec<u8>) -> crate::Result<Self> {
        if rgba.len() != width as usize * height as usize * 4 {
            return Err(crate::WcError::Internal(
                "RGBA buffer size does not match dimensions".into(),
            ));
        }
        let has_alpha = rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 255);
        Ok(StillImage {
            width,
            height,
            rgba,
            has_alpha,
            source: None,
        })
    }

    pub fn pixel_count(&self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

/// A single animation frame: composited RGBA plus timing/geometry.
#[derive(Debug, Clone)]
pub struct Frame {
    pub still: StillImage,
    /// frame duration in milliseconds
    pub duration_ms: u32,
    /// offset of the frame inside the canvas
    pub offset_x: u32,
    pub offset_y: u32,
    /// blend with previous canvas contents (false = overwrite)
    pub blend: bool,
    /// 0 = leave canvas, 1 = restore background, 2 = restore previous
    pub disposal: u8,
}

#[derive(Debug, Clone)]
pub struct AnimatedImage {
    pub width: u32,
    pub height: u32,
    /// None = unknown in this build
    pub loop_count: Option<i32>,
    pub frames: Vec<Frame>,
}

/// What a decoder produces.
#[derive(Debug, Clone)]
pub enum Decoded {
    Still(StillImage),
    Animated(AnimatedImage),
}

impl Decoded {
    pub fn still(self) -> crate::Result<StillImage> {
        match self {
            Decoded::Still(s) => Ok(s),
            Decoded::Animated(_) => Err(crate::WcError::Internal(
                "expected still image but decoded an animation".into(),
            )),
        }
    }

    /// Frame count for inspection (1 for stills).
    pub fn frames(&self) -> u32 {
        match self {
            Decoded::Still(_) => 1,
            Decoded::Animated(a) => a.frames.len() as u32,
        }
    }
}
