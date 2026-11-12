use super::*;

#[test]
fn app_window_icon_embedded_asset_returns_icon_data_128x128() {
    let icon = app_window_icon().expect("embedded app-icon asset must decode");
    assert_eq!(icon.width, 128);
    assert_eq!(icon.height, 128);
    assert_eq!(icon.rgba.len(), 128 * 128 * 4);
}

#[test]
fn app_window_icon_embedded_asset_has_transparent_corners() {
    // Straight (unmultiplied) alpha: the embedded asset's corner pixels are
    // fully transparent, so the window icon keeps its rounded silhouette.
    let icon = app_window_icon().expect("embedded app-icon asset must decode");
    let stride = icon.width as usize * 4;
    let corners = [
        0,
        stride - 4,
        stride * (icon.height as usize - 1),
        icon.rgba.len() - 4,
    ];
    for corner in corners {
        assert_eq!(
            icon.rgba[corner + 3],
            0,
            "corner alpha must be 0 at {corner}"
        );
    }
}
