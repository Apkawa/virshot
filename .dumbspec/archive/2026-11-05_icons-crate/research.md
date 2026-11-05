# 2026-11-05_icons-crate: research

Superficial context scan (codebase + toolchain), gathered via research fan-out on 2026-11-05. Facts below are verified against repo files and locked sources under `.cache/cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`.

## Current tray icon (to be replaced)

- `src/tray.rs` — tray module, one file, tests in sibling `src/tray.tests.rs` (`#[cfg(test)] #[path = ...] mod tests;` at tray.rs:246-248).
- `pub(crate) fn camera_glyph_rgba() -> Vec<u8>` (src/tray.rs:78): procedural 32×32 camera glyph painted into `image::ImageBuffer<Rgba<u8>, Vec<u8>>` (helpers `fill_rect` :94, `fill_circle` :110); returns raw RGBA bytes (4096), transparent background. Colors: BODY `[80,80,80,255]`, LENS_RING `[200,200,200,255]`, LENS_CORE `[24,24,24,255]` (:71-73). `ICON_SIZE: u32 = 32` (:68).
- Hand-off: `tray_icon::Icon::from_rgba(camera_glyph_rgba(), ICON_SIZE, ICON_SIZE)` (src/tray.rs:173). tray-icon 0.26.1 (Cargo.toml:32, `default-features = false, features = ["ksni"]`) — `Icon::from_rgba(rgba: Vec<u8>, width: u32, height: u32) -> Result<Icon, BadIcon>`; failures map to `tray::Error::TrayUnavailable`.
- Creation: exactly one call site — `tray::TrayController::new()` on the main thread at startup (src/main.rs:392); `TrayIconBuilder::new().with_icon(icon).with_menu(...).with_tooltip("virshot").with_menu_on_left_click(false).build()` (:201-210). No icon-change path exists. Controller cloned into `gui::run` / SettingsApp (src/gui.rs:530).
- Tests in `src/tray.tests.rs` assert buffer len 32×32×4, lens darker than body, transparent corners — they must move/be replaced when the glyph is replaced.

## Window icon API (currently unset)

- Locked stack: winit 0.30.13, eframe 0.37.0, egui 0.37.0, egui-winit 0.37.0.
- virshot sets NO window icon: `eframe::NativeOptions { viewport: ViewportBuilder::default().with_title(...)... }` (src/gui.rs:1187-1195) → eframe falls back to bundled default egui "e" icon (`load_default_egui_icon`, eframe-0.37.0/src/native/epi_integration.rs:243-257).
- Route without new deps: `NativeOptions.viewport.icon = Some(Arc<egui::IconData>)` via `ViewportBuilder::with_icon` → egui-winit `to_winit_icon` → winit `WindowAttributes::with_window_icon(winit::window::Icon::from_rgba(...))` (egui-winit-0.37.0/src/lib.rs:2372-2375).
- Runtime change: `ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(Arc<IconData>)))` → `Window::set_window_icon` (egui-winit :2091-2094). Caveat: on Windows eframe's `AppTitleIconSetter` (taskbar ICON_BIG) captures the creation-time icon only; runtime command updates winit ICON_SMALL.
- `egui::IconData { rgba: Vec<u8>, width: u32, height: u32 }` — RGBA8, **straight (unmultiplied) alpha**, row-major; square recommended; winit validation: `rgba.len() % 4 == 0 && width*height == len/4` else `BadIcon`. Windows: WM_SETICON ICON_SMALL (winit) + ICON_BIG (eframe re-encode to PNG). Linux X11: `_NET_WM_ICON`. Wayland: no-op in winit 0.30.13.
- Root crate has no direct winit dep (only `crates/overlay` declares winit) — the egui `IconData` route adds **zero new deps**.
- PNG→IconData helper already public, no new deps: `eframe::icon_data::from_png_bytes(&bytes) -> Result<egui::IconData, image::ImageError>` (eframe-0.37.0/src/icon_data.rs:24).

## egui image/texture rendering (locked versions)

- `egui::ImageSource` (0.37) = `Uri | Texture | Textureless...`: actually `Uri | Texture(SizedTexture) | Bytes`; egui core has **no image decoder** and `Loaders::default()` installs no image loaders; eframe installs none → `ui.image(bytes)`, `ui.image("file://…")`, `egui::include_image!` all fail with `NoImageLoaders`. `egui_extras` is not in Cargo.lock.
- Canonical in-repo pattern (crates/overlay/src/content.rs `ImageCache::texture_for`): `image::load_from_memory(bytes)` → `to_rgba8()` → `egui::ColorImage::from_rgba_unmultiplied([w,h], pixels.as_raw())` → `ctx.load_texture(name, img, TextureOptions)` **once**, outside the UI loop → paint via `ui.painter().image(id, rect, uv, tint)` or `ui.image((id, size_vec2))`. Guard `ctx.input(|i| i.max_texture_side)`.
- `Context::load_texture_cached(name, Id, image, options)` exists for safe per-frame calls; `TextureHandle::set` updates pixels.
- SVG at runtime would need `egui_extras/svg` → `resvg`/`usvg` = **new crates.io deps → requires user approval**. Dep-free runtime rendering = pre-generated raster textures or egui shapes.
- Bundled egui font has only 5 special glyphs; `⚠`/`ℹ` are absent — this is why src/gui.rs:451-460 uses ASCII "!"/"i" glyphs today (`icon_for_status` :437, `icon_glyph` :455, `icon_color` :463).

## Crate conventions (for crates/icons)

- Manifest template (matches overlay/toast): `name = "icons"`, `version = "0.1.0"`, `edition = "2024"`; deps only from locked set (`thiserror = "2"`, `image = { version = "0.25", default-features = false, features = ["png","jpeg","webp"] }`); `[dev-dependencies] eframe = { version = "0.37.0", default-features = false, features = ["glow","default_fonts","x11","wayland"] }` for Linux example runs.
- Root wiring: `icons = { path = "crates/icons" }` in `[dependencies]` (all platforms) + append `"crates/icons"` to `[workspace] members` (Cargo.toml:12-14).
- `src/lib.rs`: `#![forbid(unsafe_code)]` (pure-Rust crate, like toast); one module per file; selective `pub use`; `error.rs` with `#[derive(Debug, Error)] pub enum IconsError`.
- Tests in crates/ live inline at END of module file (overlay/toast/gamepad precedent); names `<unit>_<scenario>_<expectation>`; deterministic.
- Examples: one per crate, head doc shows `xvfb-run -a ./cargo.sh run --example <name> -p icons` (overlay_demo/toast_debug precedent).
- Gates: `./scripts/check.sh` = root-package check/test/clippy (icons compiles as root dep); crate tests via `./cargo.sh test -p icons` or `--workspace` at boundaries; `cargo fmt` covers all members; all cargo invocations via `./cargo.sh` (CARGO_HOME=./.cache/cargo).

## rsvg-convert toolchain (verified in sandbox)

- `/usr/bin/rsvg-convert` version 2.58.0 (librsvg2-bin). `pkg-config --exists librsvg` fails → a Rust librsvg binding is not buildable here; the **CLI is the generator** (no new crates.io dep needed — external tool, already installed, user-stated).
- Verified command: `rsvg-convert -f png -w 32 -h 32 -o out.png in.svg` → exit 0, `PNG image data, 32 x 32, 8-bit/color RGBA, non-interlaced`. Long form `--format png --width 256 --height 256 --output ...` works. Note: `-h` = height (help is `-?`).
- PNG output carries straight alpha (cairo ARGB premultiplied internally but PNG is RGBA non-premultiplied); `ColorImage::from_rgba_unmultiplied` / `IconData` consume it directly.

## Existing assets

- No PNG/SVG asset files in the repo today (`fd -g '*.png'` → nothing tracked). Generated rasters must be committed as tracked assets under `crates/icons/` and embedded via `include_bytes!`.
