//! Draft dirty-state, bottom-bar button gating, and close decision.

use crate::config::Settings;

/// Whether the draft differs from the applied settings (Apply button hint;
/// closing without Apply discards the draft — spec §4.4).
pub(crate) fn is_dirty(draft: &Settings, applied: &Settings) -> bool {
    draft != applied
}

/// Enabled state of the bottom-row buttons (spec §4.4). Apply and OK both run
/// `apply()`, so they are enabled only while the draft differs from what is
/// applied AND `savable`. The caller composes `savable` from BOTH save gates:
/// every row has a keyboard key (`draft_savable` — an unbound row is the
/// Backspace-cleared marker §4.1 cannot parse) AND no triggers collide
/// (`duplicate_bindings` — §4.4 Stage 3: one combo or pad must trigger one
/// action). Cancel saves nothing — it only discards the draft — so it stays
/// enabled in every state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BottomButtons {
    pub(crate) apply_enabled: bool,
    pub(crate) ok_enabled: bool,
    pub(crate) cancel_enabled: bool,
}

pub(crate) fn bottom_buttons_enabled(dirty: bool, savable: bool) -> BottomButtons {
    let save = dirty && savable;
    BottomButtons {
        apply_enabled: save,
        ok_enabled: save,
        cancel_enabled: true,
    }
}

/// Id salt of the docked bottom panel holding the `Apply | OK | Cancel` row
/// (spec §4.4). The row lives in a real `egui::Panel::bottom` rather than as a
/// trailing row after the tab content, so it hugs the window's bottom edge
/// instead of floating above it.
pub(crate) const BOTTOM_BAR_PANEL_ID: &str = "settings_bottom_bar";

/// Draft after Cancel (spec §4.4): a dirty draft is thrown away and the applied
/// settings come back; a clean draft already equals them, so it is returned as
/// the copy the caller assigns. Neither argument is rewritten — the app keeps
/// its own `applied` for the next Apply, and nothing is saved or published.
pub(crate) fn cancel_draft(draft: &Settings, applied: &Settings) -> Settings {
    if is_dirty(draft, applied) {
        applied.clone()
    } else {
        draft.clone()
    }
}

/// What a window close request means: `Exit` shuts the process down,
/// `HideToTray` hides the window and keeps the process resident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseAction {
    Exit,
    HideToTray,
}

/// Pure close decision (spec §4.4 + §4.5): the tray `Exit` action and the
/// OK/Cancel buttons are explicit exits, so their close request is not
/// cancelled. A plain window close (the X button, no exit flag) hides the
/// window to the tray — exit is tray-menu-only.
pub(crate) fn close_decision(exit_requested: bool, closed_by_button: bool) -> CloseAction {
    if exit_requested || closed_by_button {
        CloseAction::Exit
    } else {
        CloseAction::HideToTray
    }
}

#[cfg(test)]
#[path = "chrome.tests.rs"]
mod tests;
