//! About window content (spec §4.5, task 2026-11-10_about-dialog): a header
//! (app icon + localized app title), the repository link, the faint build
//! line, then the embedded CHANGELOG.md rendered with `egui_commonmark` —
//! the layout mirrors the reference screenshot. The module owns the viewport
//! lifecycle state machine (stage 10) and what is painted inside the viewport,
//! including the opaque background: the app-wide clear color is transparent
//! and egui paints no background for a child viewport. The viewport itself is
//! registered by `gui` on the main eframe loop.

use egui_commonmark::{CommonMarkCache, CommonMarkScrollOptions, CommonMarkViewer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Lifecycle state of the About child viewport (task stage 10): open/closed
/// plus the close signal observed inside the CHILD's own UI pass. An OS close
/// produces winit `CloseRequested`, which egui-winit ignores; eframe pushes
/// `ViewportEvent::Close` into the CHILD's `ViewportInfo`
/// (`eframe glow_integration.rs:1075`), the child pass runs before the root
/// pass (`run.rs:147-152`), and eframe clears the child's info after its pass
/// (`glow_integration.rs:853`), so a root-pass read of `raw.viewports` can
/// never see it (research R10). The paint callback records the request into
/// `close_flag`, and the next root pass turns it into "closed" via
/// [`AboutWindow::observe_close`]: registration stops, egui GCs the viewport,
/// eframe drops the `Arc<Window>` — the window is destroyed (repo precedent:
/// `crates/overlay/src/host.rs:313-323`, `:494-501`).
pub(crate) struct AboutWindow {
    open: bool,
    close_flag: Arc<AtomicBool>,
}

impl AboutWindow {
    /// Closed state with an inert flag; opening replaces the flag, so this
    /// one is never handed to a paint callback.
    pub(crate) fn new() -> Self {
        Self {
            open: false,
            close_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Open a closed window with a FRESH close flag — a reopened viewport
    /// must not inherit a stale close request. Idempotent when already open.
    /// Returns whether the window newly opened.
    pub(crate) fn open(&mut self) -> bool {
        if self.open {
            return false;
        }
        self.close_flag = Arc::new(AtomicBool::new(false));
        self.open = true;
        true
    }

    /// Tray `About` selection (user decision 2026-11-11): `true` = newly
    /// opened (caller uploads the icon lazily and repaints), `false` =
    /// already open — the caller raises the window with
    /// `ViewportCommand::Focus`, never closes or re-opens it.
    pub(crate) fn select_from_tray(&mut self) -> bool {
        self.open()
    }

    /// Consume the close signal recorded by the child paint pass. Returns
    /// `true` when the flag was set and the window just closed; a closed
    /// window has nothing left to close, so it never reports again.
    pub(crate) fn observe_close(&mut self) -> bool {
        if self.open && self.close_flag.load(Ordering::Relaxed) {
            self.open = false;
            return true;
        }
        false
    }

    /// Whether the logic pass must call `show_viewport_deferred` for the
    /// viewport. A closed window never registers, which is what destroys the
    /// OS window (egui GC → eframe drops `Arc<Window>`).
    pub(crate) fn wants_registration(&self) -> bool {
        self.open
    }

    /// The close-flag handle for the paint callback, `None` when closed —
    /// there is nothing to record into. The `Arc` clone is O(1) (one
    /// refcount bump): the same handle is shared with app state.
    pub(crate) fn close_flag(&self) -> Option<Arc<AtomicBool>> {
        if self.open {
            Some(Arc::clone(&self.close_flag))
        } else {
            None
        }
    }
}

/// The changelog embedded at compile time (`include_str!` resolves relative to
/// this file). Release flow keeps `CHANGELOG.md` current; an empty file still
/// compiles and renders the localized placeholder instead.
const CHANGELOG: &str = include_str!("../CHANGELOG.md");

/// Repository URL for the About dialog link. Authored here because no
/// manifest declares a `repository` field; the value mirrors the `origin`
/// remote `git@github.com:Apkawa/virshot.git`.
pub(crate) const REPO_URL: &str = "https://github.com/Apkawa/virshot";

/// OS window-chrome policy for the About child viewport (task stage 9): the
/// window keeps its title bar, close button and OS resize border, but exposes
/// no minimize and no maximize buttons. `gui` folds these flags into
/// `egui::ViewportBuilder` at viewport creation; the builder must stay
/// CONSTANT per viewport id `virshot-about` — egui applies builder changes by
/// recreating the OS window, so varying them per pass would destroy and rebuild
/// the window. Platform behavior: Windows/macOS honor the button flags (on
/// Windows `WS_MINIMIZEBOX`/`WS_MAXIMIZEBOX` are dropped while `WS_CAPTION` +
/// `WS_SIZEBOX` stay); on X11/Wayland they are documented as inert by design.
pub(crate) struct WindowChrome {
    pub(crate) decorations: bool,
    pub(crate) resizable: bool,
    pub(crate) close_button: bool,
    pub(crate) minimize_button: bool,
    pub(crate) maximize_button: bool,
}

/// The window-chrome policy for the About viewport, as authored in
/// [`WindowChrome`]. Pure value, no state.
pub(crate) fn window_chrome() -> WindowChrome {
    WindowChrome {
        decorations: true,
        resizable: true,
        close_button: true,
        minimize_button: false,
        maximize_button: false,
    }
}

/// Build line contract for `--version`: `virshot <version> (<sha>) built
/// <build_time>`. Every value is injected by the caller (build-time
/// constants), so this function never reads env or the clock. Missing values
/// render as `unknown` rather than empty parens or a trailing empty field.
pub(crate) fn version_line(version: &str, short_sha: &str, build_time: &str) -> String {
    let sha = if short_sha.is_empty() {
        "unknown"
    } else {
        short_sha
    };
    let built = if build_time.is_empty() {
        "unknown"
    } else {
        build_time
    };
    format!("virshot {version} ({sha}) built {built}")
}

/// Build line for the About viewport: `{sha} {date}`. Stage 12 (spec v5,
/// user decision 2026-11-11) drops the debug/release marker — the localized
/// `about.marker_debug`/`about.marker_release` keys are gone from the catalog.
/// Every value is injected by the caller (build-time constants), so this
/// function never reads env or the clock. Empty sha or empty date render as
/// `unknown`, never as blank fields.
pub(crate) fn build_line(short_sha: &str, build_time: &str) -> String {
    let sha = if short_sha.is_empty() {
        "unknown"
    } else {
        short_sha
    };
    let date = if build_time.is_empty() {
        "unknown"
    } else {
        build_time
    };
    format!("{sha} {date}")
}

/// Version string stamped with the dev marker (stage 12): an untagged HEAD is
/// a dev build, so `version` gains a `-dev` suffix unless HEAD carries exactly
/// the release tag `v<version>`. Research R12: shadow-rs fills `build::TAG`
/// from `git tag -l --contains HEAD` (empty string when HEAD carries no tag),
/// so an empty tag, a prerelease tag like `nightly`, or a release tag for a
/// different version all render `<version>-dev`. Pure — tag and version are
/// injected by the caller, never read from git here.
pub(crate) fn version_with_dev(version: &str, tag: &str) -> String {
    if tag == format!("v{version}") {
        version.to_owned()
    } else {
        format!("{version}-dev")
    }
}

/// The window header title, localized through the catalog (`about.app_title`,
/// brand case `ViRShot %{version}` in every locale).
pub(crate) fn app_title(version: &str) -> String {
    rust_i18n::t!("about.app_title", version = version).into_owned()
}

/// The Moscow build date: `build.rs` embeds `VIRSHOT_BUILD_TIME`; an
/// unreadable clock at build time leaves it empty, so the fallback is
/// shadow-rs' own `BUILD_TIME_3339` (build-host timezone).
fn build_date() -> &'static str {
    let moscow = env!("VIRSHOT_BUILD_TIME");
    if moscow.is_empty() {
        crate::build::BUILD_TIME_3339
    } else {
        moscow
    }
}

/// `--version` output built from the real build values: package version (with
/// the stage 12 `-dev` suffix when HEAD carries no release tag) and short
/// commit from the `shadow!`-generated `build` module + the Moscow date.
pub(crate) fn version_from_build() -> String {
    version_line(
        &version_with_dev(crate::build::PKG_VERSION, crate::build::TAG),
        crate::build::SHORT_COMMIT,
        build_date(),
    )
}

/// Paint the About viewport. The layout mirrors the reference screenshot:
/// header row (app icon + bold title), repository hyperlink, the faint build
/// line, separator, then the whole changelog in a scrollable area (task
/// decision: full CHANGELOG, no extra features — image/syntax loaders stay
/// off). The background is an opaque `CentralPanel` filled with
/// `visuals.panel_fill`: `App::clear_color` is TRANSPARENT app-wide (toast and
/// overlay viewports need it) and egui paints no background for a deferred
/// child viewport, so About must fill its own. The icon texture is owned by
/// `gui` and handed in — `paint` never loads textures; `None` (decode failure
/// or texture not yet built) skips the image and the header text still
/// renders. The viewer is `!Send`, so it is created inside the viewport paint
/// here; `CommonMarkCache` lives in the app state and is handed in by `gui`.
/// `close_flag` is the shared [`AboutWindow`] signal: an OS close lands as
/// `CloseRequested` in THIS pass's `ViewportInfo` (`ViewportInfo::take()`
/// moves the events out before eframe clears the info), so `paint` records it
/// here — the root pass cannot observe it (R10). Same thread as the root
/// logic pass, so `Ordering::Relaxed` is sufficient.
pub(crate) fn paint(
    ui: &mut egui::Ui,
    cache: &mut CommonMarkCache,
    icon: Option<&egui::TextureHandle>,
    close_flag: &AtomicBool,
) {
    if ui.input(|i| i.viewport().close_requested()) {
        close_flag.store(true, Ordering::Relaxed);
    }
    // `Frame::central_panel(&Style)` fills `visuals.panel_fill` across the
    // whole viewport; the symmetric margin keeps content off the window edges.
    let frame = egui::Frame::central_panel(ui.style()).inner_margin(egui::Margin::symmetric(16, 8));
    egui::CentralPanel::default().frame(frame).show(ui, |ui| {
        ui.horizontal(|ui| {
            if let Some(icon) = icon {
                ui.image((icon.id(), egui::vec2(48.0, 48.0)));
            }
            ui.label(
                egui::RichText::new(app_title(&version_with_dev(
                    crate::build::PKG_VERSION,
                    crate::build::TAG,
                )))
                .size(22.0)
                .strong(),
            );
        });
        ui.hyperlink_to(rust_i18n::t!("about.repo_link"), REPO_URL);
        // egui 0.37 `Visuals` has no `faint_text_color` field; the faint
        // text tone is the `weak_text_color()` method (theme text color
        // dimmed by `weak_text_alpha`).
        let faint = ui.style().visuals.weak_text_color();
        ui.label(
            egui::RichText::new(build_line(crate::build::SHORT_COMMIT, build_date()))
                .small()
                .color(faint),
        );
        ui.separator();
        ui.label(rust_i18n::t!("about.changelog_heading"));

        let changelog = if CHANGELOG.is_empty() {
            rust_i18n::t!("about.changelog_missing").into_owned()
        } else {
            CHANGELOG.to_owned()
        };
        CommonMarkViewer::new().show_scrollable(
            // Stable across passes (`Id::unique` hashes with fixed seeds), so the
            // viewer cache entry survives repaints.
            egui::Id::unique("virshot-about-changelog"),
            ui,
            cache,
            &CommonMarkScrollOptions::default().viewport_cache(true),
            &changelog,
        );
    });
}

#[cfg(test)]
#[path = "about.tests.rs"]
mod tests;
