//! Image loading for image chips.
//!
//! Toast hands the overlay **encoded bytes**; the overlay backend decodes on the
//! thread or host pass that paints the chip. The only work here is reading the
//! file and refusing oversized payloads before they cross the channel.

use std::path::Path;

use crate::error::ToastError;

/// Byte cap for an image payload, mirroring the overlay backend's oversized
/// image guard (`OverlayError::ImageTooLarge`) on the encoded side.
pub(crate) const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;

/// Read encoded image bytes for a chip image.
///
/// Fails with `ToastError::Io` when the file cannot be read and with
/// `ToastError::ImageTooLarge` when it exceeds [`MAX_IMAGE_BYTES`]. Decoding is
/// not attempted here; the overlay backend reports decode failures on its side.
pub fn load(path: &Path) -> Result<Vec<u8>, ToastError> {
    let size = std::fs::metadata(path).map_err(|error| ToastError::Io {
        message: format!("{}: {error}", path.display()),
    })?;
    check_image_size(size.len() as usize)?;
    std::fs::read(path).map_err(|error| ToastError::Io {
        message: format!("{}: {error}", path.display()),
    })
}

/// The size gate, kept separate so it is unit-testable without a filesystem.
pub(crate) fn check_image_size(bytes: usize) -> Result<(), ToastError> {
    if bytes > MAX_IMAGE_BYTES {
        return Err(ToastError::ImageTooLarge {
            bytes,
            max: MAX_IMAGE_BYTES,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::MAX_IMAGE_BYTES;
    use super::check_image_size;
    use super::load;
    use crate::error::ToastError;
    use std::path::Path;

    #[test]
    fn load_helper_rejects_oversized_image() {
        // Pure size gate (the cap mirrors overlay's ImageTooLarge guard without
        // decoding — decode happens on the overlay side).
        assert_eq!(check_image_size(MAX_IMAGE_BYTES), Ok(()));
        assert_eq!(check_image_size(0), Ok(()));
        assert_eq!(
            check_image_size(MAX_IMAGE_BYTES + 1),
            Err(ToastError::ImageTooLarge {
                bytes: MAX_IMAGE_BYTES + 1,
                max: MAX_IMAGE_BYTES,
            })
        );
    }

    #[test]
    fn load_helper_reports_a_missing_file_as_io_error() {
        let result = load(Path::new("toast-definitely-missing.png"));
        assert!(
            matches!(result, Err(ToastError::Io { .. })),
            "a missing file is a typed Io error, not a panic"
        );
    }
}
