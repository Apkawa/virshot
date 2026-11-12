use super::*;

#[test]
fn is_dirty_detects_draft_edits() {
    let applied = Settings::default();
    let mut draft = applied.clone();

    assert!(!is_dirty(&draft, &applied));

    draft.general.save_path = "/tmp/other".to_owned();
    assert!(is_dirty(&draft, &applied));
}

#[test]
fn bottom_buttons_apply_and_ok_require_dirty_draft() {
    // Apply and OK both run `apply()` (§4.4), so they are meaningful only while
    // the draft differs from what is applied AND every row has a key.
    let dirty = bottom_buttons_enabled(true, true);
    assert!(
        dirty.apply_enabled,
        "Apply must be enabled with a dirty, savable draft"
    );
    assert!(
        dirty.ok_enabled,
        "OK must be enabled with a dirty, savable draft"
    );

    let clean = bottom_buttons_enabled(false, true);
    assert!(
        !clean.apply_enabled,
        "Apply must stay disabled with a clean draft"
    );
    assert!(
        !clean.ok_enabled,
        "OK must stay disabled with a clean draft"
    );
}

#[test]
fn bottom_buttons_save_disabled_while_a_binding_is_cleared() {
    // Backspace clears a row to the unbound marker (§4.4); saving it would
    // write a key `bindings::parse_key` rejects (§4.1), which costs the app
    // its hotkeys and pad triggers, so Apply/OK stay disabled until re-bound.
    let blocked = bottom_buttons_enabled(true, false);
    assert!(
        !blocked.apply_enabled,
        "Apply must be disabled for an unbound row"
    );
    assert!(
        !blocked.ok_enabled,
        "OK must be disabled for an unbound row"
    );
    assert!(
        blocked.cancel_enabled,
        "Cancel discards the draft, so it stays enabled"
    );
}

#[test]
fn bottom_buttons_cancel_always_enabled() {
    // Cancel saves nothing — it only discards the draft, so it is enabled in
    // every draft state (§4.4).
    assert!(bottom_buttons_enabled(true, true).cancel_enabled);
    assert!(bottom_buttons_enabled(false, true).cancel_enabled);
    assert!(bottom_buttons_enabled(true, false).cancel_enabled);
}

#[test]
fn bottom_bar_panel_id_is_a_stable_non_empty_salt() {
    // Stage 11 docks the row into a real `egui::Panel::bottom`. egui decides
    // panel placement at paint time, so docking is proven by the Xvfb
    // screenshots (`scripts/ui-verify.sh`), not by a unit test; the id salt is
    // the only seam a test can reach, and pinning it keeps the panel id from
    // drifting into an unnamed layout.
    assert_eq!(BOTTOM_BAR_PANEL_ID, "settings_bottom_bar");
    assert!(!BOTTOM_BAR_PANEL_ID.is_empty());
}

#[test]
fn cancel_draft_restores_applied_and_keeps_applied_unchanged() {
    let applied = Settings::default();
    let mut draft = applied.clone();
    draft.general.save_path = "/tmp/other".to_owned();

    let restored = cancel_draft(&draft, &applied);
    assert_eq!(restored, applied, "Cancel must return the applied settings");
    assert_eq!(
        applied,
        Settings::default(),
        "Cancel must not rewrite the applied settings"
    );
    assert_eq!(
        draft.general.save_path, "/tmp/other",
        "Cancel must not rewrite the draft argument"
    );

    // A clean Cancel yields the same settings too.
    assert_eq!(cancel_draft(&applied, &applied), applied);
}

#[test]
fn close_decision_plain_window_close_hides_to_tray() {
    // The X button is not an exit: the process stays resident (§4.5 — exit is
    // tray-menu-only) and the window only hides.
    assert_eq!(close_decision(false, false), CloseAction::HideToTray);
}

#[test]
fn close_decision_ok_or_cancel_button_closes_for_real() {
    // OK/Cancel are explicit decisions: the close request must not be
    // swallowed by the hide-to-tray branch (§4.4).
    assert_eq!(close_decision(false, true), CloseAction::Exit);
}

#[test]
fn close_decision_tray_exit_closes_for_real() {
    assert_eq!(close_decision(true, false), CloseAction::Exit);
    assert_eq!(close_decision(true, true), CloseAction::Exit);
}
