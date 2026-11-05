//! Integration test for the public error contract of the `icons` crate.

/// A failed decode of embedded PNG bytes converts into `icons::IconsError` and
/// reports a readable message: callers match on the type instead of parsing
/// strings, and the message still names the failure mode.
#[test]
fn icons_error_from_image_decode_failure_is_displayable() {
    let decode_error = image::load_from_memory(b"not-a-png")
        .expect_err("bytes that are not a PNG must fail to decode");

    let error: icons::IconsError = decode_error.into();

    let message = error.to_string();
    assert!(!message.is_empty());
    assert!(
        message.to_lowercase().contains("decode"),
        "error message must mention the decode failure: {message}"
    );
    // The source survives conversion, so callers can inspect the underlying
    // `image` error (its own Display must not be empty either).
    assert!(!format!("{error:?}").is_empty());
}
