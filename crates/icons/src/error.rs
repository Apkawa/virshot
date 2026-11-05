//! Typed icon-asset failures (thiserror), so callers match on the type instead
//! of parsing strings.

use thiserror::Error;

/// Everything that can go wrong turning embedded icon bytes into an image.
#[derive(Debug, Error)]
pub enum IconsError {
    /// The embedded PNG bytes (`include_bytes!` asset) could not be decoded by
    /// `image`: a missing or corrupted asset, or a format the trimmed `image`
    /// feature set (`png` only) does not support.
    #[error("failed to decode icon asset: {source}")]
    IconDecodeError {
        /// The underlying `image` decode failure, kept so callers can inspect
        /// the concrete cause. `#[from]` makes any `image::ImageError` convert
        /// into `IconsError` with `?` or `.into()`.
        #[from]
        source: image::ImageError,
    },
}
