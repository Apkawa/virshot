//! System tray (spec §4.5): tray lifecycle + event mapping ONLY. GUI/main
//! wire the mapped actions; this module never touches windows or capture.
//!
//! tray-icon 0.26.1 API facts verified in the locked registry source, not from
//! memory:
//! - Icon construction is `Icon::from_rgba(rgba, width, height)` (no `Icon::new`);
//!   the RGBA buffer must be exactly `width * height` pixels.
//! - Builder: `TrayIconBuilder::new().with_icon(..).with_menu(Box::new(menu))
//!   .with_tooltip(..).with_menu_on_left_click(false).build() -> Result<TrayIcon>`;
//!   `TrayIcon` is reference-counted and the icon disappears when it is dropped,
//!   so the controller keeps it alive.
//! - Events arrive on global crossbeam receivers: `TrayIconEvent::receiver()`
//!   and `menu::MenuEvent::receiver()`; `try_recv()` drains them without a
//!   thread or mutex. Handling on the main thread satisfies every backend
//!   (macOS-style affinity included).
//! - `TrayIconEvent` kinds: `Click { button, button_state, .. }`, `DoubleClick`
//!   (Windows only), `Enter`/`Move`/`Leave`. The ksni (SNI/D-Bus) backend emits
//!   `Click { button: Left|Middle, button_state: Up }` for activation — one
//!   event per click, no Down/Up pair; Windows emits Down then Up per click,
//!   so only the Up event is an action (a double-show guard). The ksni host
//!   handles right clicks itself (no event reaches us) and shows the menu via
//!   the D-Bus menu protocol; menu item activation arrives as `MenuEvent { id }`.
//!   `DoubleClick`/hover kinds are ignored.
//! - Menu (muda 0.21.2 re-exported as `tray_icon::menu`): `Menu::new()` +
//!   `append(&item)`, `MenuItem::with_id(id, text, enabled, accelerator)`,
//!   `PredefinedMenuItem::separator()`. Item ids are `MenuId(String)`.
//! - Linux needs a D-Bus session bus for ksni; a missing/unusable bus fails
//!   `build()` with a typed `tray_icon::Error` — the caller logs it and keeps
//!   running (spec §5 fault tolerance).

use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

/// Action the tray asked for; mapped from icon clicks and menu items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrayAction {
    /// Left click on the icon or the `Settings` menu item — show the window.
    ShowSettings,
    /// `Take screenshot` menu item — full-screen capture (spec §4.5).
    TakeScreenshot,
    /// `Exit` menu item — the only exit path (spec §4.5).
    Exit,
}

/// Tray failures (icon/menu creation). A failed tray never aborts the app:
/// the caller logs the typed status and continues without a tray.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("system tray unavailable: {message}")]
    TrayUnavailable { message: String },
}

/// Wrap a backend/muda failure into the typed [`Error`]; startup (`new`) and
/// the menu rebuild (`refresh_menu`) report failures the same way.
fn unavailable(message: String) -> Error {
    Error::TrayUnavailable { message }
}

/// Menu labels — spec §4.5 wording, resolved from the catalog (spec §2:
/// `tray.take_screenshot` / `tray.settings` / `tray.exit`).
fn take_screenshot_label() -> String {
    rust_i18n::t!("tray.take_screenshot").into_owned()
}

fn settings_label() -> String {
    rust_i18n::t!("tray.settings").into_owned()
}

fn exit_label() -> String {
    rust_i18n::t!("tray.exit").into_owned()
}

/// Internal menu item ids (mapped back onto actions in [`poll`]).
const MENU_ID_TAKE_SCREENSHOT: &str = "virshot-take-screenshot";
const MENU_ID_SETTINGS: &str = "virshot-settings";
const MENU_ID_EXIT: &str = "virshot-exit";

/// Tooltip shown by hosts that support one (ksni exposes it) — catalog
/// `tray.tooltip`.
fn tooltip() -> String {
    rust_i18n::t!("tray.tooltip").into_owned()
}

/// The menu label trio, in menu order — the single seam that BOTH menu paths
/// render ([`build_menu`] reads it), so the startup menu and the post-Apply
/// rebuild can never drift from each other or from the catalog (spec §2).
pub(crate) fn menu_labels() -> (String, String, String) {
    (take_screenshot_label(), settings_label(), exit_label())
}

/// Build the §4.5 menu with every label resolved from the CURRENT locale.
/// Startup (`TrayController::new`) and the post-Apply locale switch
/// (`TrayController::refresh_menu`) run THIS builder, so ids, order, and
/// labels have one source and a locale change cannot drift the two paths.
/// Failure mode: muda rejects attaching an item already part of a menu
/// (`Menu::append`); freshly built items never hit it, and callers log and
/// keep running.
fn build_menu() -> Result<Menu, Error> {
    let menu = Menu::new();
    let (take, settings, exit) = menu_labels();

    menu.append(&MenuItem::with_id(
        MENU_ID_TAKE_SCREENSHOT,
        take,
        true,
        None,
    ))
    .map_err(|err| unavailable(err.to_string()))?;
    menu.append(&PredefinedMenuItem::separator())
        .map_err(|err| unavailable(err.to_string()))?;
    menu.append(&MenuItem::with_id(MENU_ID_SETTINGS, settings, true, None))
        .map_err(|err| unavailable(err.to_string()))?;
    menu.append(&MenuItem::with_id(MENU_ID_EXIT, exit, true, None))
        .map_err(|err| unavailable(err.to_string()))?;

    Ok(menu)
}

/// Tray icon source: decode the embedded app-icon asset (`icons::APP_ICON_PNG`,
/// 128×128 RGBA PNG) into the buffer + dimensions `Icon::from_rgba` expects.
/// Failure mode: [`icons::IconsError::IconDecodeError`] when the embedded PNG
/// bytes are corrupt/undecodable; [`TrayController::new`] maps it to [`Error`]
/// and the caller continues without a tray.
fn app_icon_rgba() -> Result<(Vec<u8>, u32, u32), icons::IconsError> {
    icons::rgba(icons::APP_ICON_PNG)
}

/// Icon-click mapping: a left-click UP event opens the settings window.
/// Down events (Windows sends Down+Up per click) and right/middle clicks are
/// ignored — right clicks are menu opens handled by the host/backend menu
/// protocol, which surfaces as [`MenuEvent`].
pub(crate) fn action_for_click(
    button: MouseButton,
    button_state: MouseButtonState,
) -> Option<TrayAction> {
    match (button, button_state) {
        (MouseButton::Left, MouseButtonState::Up) => Some(TrayAction::ShowSettings),
        _ => None,
    }
}

/// Menu-item mapping by id; unknown ids are ignored.
pub(crate) fn action_for_menu_id(id: &MenuId) -> Option<TrayAction> {
    match id.0.as_str() {
        MENU_ID_TAKE_SCREENSHOT => Some(TrayAction::TakeScreenshot),
        MENU_ID_SETTINGS => Some(TrayAction::ShowSettings),
        MENU_ID_EXIT => Some(TrayAction::Exit),
        _ => None,
    }
}

/// Owns the tray icon (keeps it alive) and maps its events to actions.
/// Created on the MAIN thread; `poll` drains the global event receivers
/// non-blockingly, so the GUI logic pass or the park loop picks actions up.
/// Cheap to clone (`TrayIcon` is reference-counted): the controller can be
/// handed to the GUI and still be polled by the park-loop fallback.
#[derive(Clone)]
pub(crate) struct TrayController {
    tray: TrayIcon,
}

impl TrayController {
    /// Create the tray icon from the embedded app-icon asset and the §4.5 menu.
    /// Failure modes: asset decode (`icons::IconsError`), icon buffer/menu
    /// issues and an unavailable tray backend (on Linux ksni: no D-Bus session
    /// bus, no registered StatusNotifier host). The caller logs the typed
    /// error and continues without a tray.
    pub(crate) fn new() -> Result<Self, Error> {
        let (rgba, width, height) = app_icon_rgba().map_err(|err| unavailable(err.to_string()))?;
        let icon =
            Icon::from_rgba(rgba, width, height).map_err(|err| unavailable(err.to_string()))?;
        let menu = build_menu()?;

        let tray = TrayIconBuilder::new()
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .with_tooltip(tooltip())
            // Left click must open the window, not pop the menu (spec §4.5);
            // right click keeps the menu. Linux hosts ignore these flags:
            // ksni emits the left-click event and the host shows the menu.
            .with_menu_on_left_click(false)
            .build()
            .map_err(|err| unavailable(err.to_string()))?;

        Ok(Self { tray })
    }

    /// Rebuild the menu item labels for the CURRENT locale (spec §2 live
    /// locale switch, Stage 2b): `TrayIcon::set_menu` replaces the whole menu,
    /// and this runs the same [`build_menu`] as startup, so after
    /// `rust_i18n::set_locale` the tray menu shows the new language without a
    /// restart.
    /// Failure mode: [`build_menu`] rejecting an item. `set_menu` itself is
    /// infallible in tray-icon 0.26.1 — verified in the locked source
    /// (`lib.rs:574`: `pub fn set_menu(&self, menu: Option<Box<dyn
    /// menu::ContextMenu>>)`, returns nothing), so a built menu always reaches
    /// the backend. The tooltip is left alone: catalog `tray.tooltip` is
    /// "virshot", locale-independent.
    pub(crate) fn refresh_menu(&self) -> Result<(), Error> {
        let menu = build_menu()?;
        self.tray.set_menu(Some(Box::new(menu)));
        Ok(())
    }

    /// Drain both event receivers without blocking and return the mapped
    /// actions (icon events filtered to this tray icon; menu events carry no
    /// icon id, so they are mapped by item id only).
    pub(crate) fn poll(&self) -> Vec<TrayAction> {
        let mut actions = Vec::new();

        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if event.id() != self.tray.id() {
                continue;
            }
            if let TrayIconEvent::Click {
                button,
                button_state,
                ..
            } = event
                && let Some(action) = action_for_click(button, button_state)
            {
                actions.push(action);
            }
        }

        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(action) = action_for_menu_id(&event.id) {
                actions.push(action);
            }
        }

        actions
    }
}

#[cfg(test)]
#[path = "tray.tests.rs"]
mod tests;
