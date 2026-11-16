use super::*;

#[test]
fn root_viewport_builder_is_visually_inert() {
    // The ROOT window must have NOTHING to show, not merely be "hidden":
    // eframe 0.37 glow paints the root window synchronously on Event::Resumed
    // and unconditionally shows it on the first frame (`is_first_frame` in
    // native `epi_integration.rs` post_rendering), so `ViewportCommand::Visible(false)`
    // lands only on the NEXT pass. Inertness is the builder contract pinned
    // here: no decorations, 1×1 inner size, transparent, invisible, and OFF
    // the taskbar/Alt-Tab list — that first revealed frame would otherwise
    // register the root window there for one frame (egui-winit maps
    // `taskbar(false)` to winit `skip_taskbar(true)` on Windows,
    // egui-winit-0.37.0/src/lib.rs:2448; overlay chips guard the same way,
    // crates/overlay/src/host.rs:378).
    // egui 0.37 exposes these as public `Option` fields on `ViewportBuilder`
    // (egui-0.37.0/src/viewport.rs:300-316, plus the Windows-section `taskbar`
    // at :328) — there are no accessor methods.
    let builder = root_viewport_builder();
    assert_eq!(builder.decorations, Some(false));
    assert_eq!(builder.inner_size, Some(egui::Vec2::new(1.0, 1.0)));
    assert_eq!(builder.visible, Some(false));
    assert_eq!(builder.transparent, Some(true));
    assert_eq!(builder.taskbar, Some(false));
}

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
