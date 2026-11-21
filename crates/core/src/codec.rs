//! Codec adapter interface (plan §3.5). Codec-specific behavior is isolated
//! behind this trait; the CLI/policy layer never touches backend details.

use crate::error::{Result, WcError};
use crate::format::ImageFormat;
use crate::model::{AnimatedImage, Decoded, ImageInfo, StillImage};
use crate::policy::EncodeOptions;
use crate::protocol::FormatCaps;

/// Errors during sniff/inspect/decode are ordinary [`WcError`]s (exit 3);
/// encode errors surface as exit 6 internal errors unless the codec maps
/// them more precisely.
pub trait Codec: Send + Sync {
    fn format(&self) -> ImageFormat;

    /// Fast magic-byte sniff. Must be cheap and false for non-matches.
    fn sniff(&self, data: &[u8]) -> bool;

    /// Structural inspection without full pixel decode.
    fn inspect(&self, data: &[u8]) -> Result<ImageInfo>;

    /// Full decode into the internal model.
    fn decode(&self, data: &[u8]) -> Result<Decoded>;

    /// Decode a single frame of an animated input as a still image.
    /// Default: unsupported.
    fn decode_frame(&self, _data: &[u8], _index: u32) -> Result<StillImage> {
        Err(WcError::InvalidArguments(
            "frame selection is not supported for this format".into(),
        ))
    }

    /// Decode a single page of a multi-page container as a still image.
    /// Default: unsupported.
    fn decode_page(&self, _data: &[u8], _index: u32) -> Result<StillImage> {
        Err(WcError::InvalidArguments(
            "page selection is not supported for this format".into(),
        ))
    }

    /// Encode a still image; codecs that cannot express an option should
    /// reject (never silently ignore) unsupported options.
    fn encode_still(&self, img: &StillImage, opts: &EncodeOptions) -> Result<Vec<u8>>;

    /// Encode an animation, preserving frame timing. Frame buffers are
    /// full-canvas. Default: unsupported (the CLI refuses rather than
    /// silently writing only one frame).
    fn encode_animated(&self, _anim: &AnimatedImage, _opts: &EncodeOptions) -> Result<Vec<u8>> {
        Err(WcError::Internal(
            "animated write is not implemented for this codec".into(),
        ))
    }

    /// Actual capabilities of this build (plan §8).
    fn caps(&self) -> FormatCaps;

    /// Human codec/backend version line for `--version`-style output.
    fn backend(&self) -> String;
}
