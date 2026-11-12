use super::*;

use crate::config::Settings;

use std::sync::MutexGuard;

/// Take the crate-wide locale lock (`crate::LOCALE_LOCK`): the locale and its
/// cache version are process-global, so locale-touching tests in every file
/// serialize on this one lock, not on a per-file one. A poisoned lock (a panic
/// in another locale test) is recovered rather than unwrapped, like every
/// other mailbox in this crate.
fn locale_guard() -> MutexGuard<'static, ()> {
    crate::LOCALE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Assert a rendered UI string goes through the catalog `key` AND the
/// catalog's `en` value equals the shipped English contract byte-for-byte.
/// Callers must hold `locale_guard()` and pin `"en"` first — this helper
/// calls `t!` itself. Tests that never leave `en` need no further restore:
/// `crate::LOCALE_LOCK` serializes locale mutations process-wide, so no other
/// test changes the locale while this test holds it.
fn assert_en_catalog(key: &str, rendered: &str, contract: &str) {
    assert_eq!(
        rendered,
        rust_i18n::t!(key).as_ref(),
        "the rendered text must come from catalog key `{key}`"
    );
    assert_eq!(
        rust_i18n::t!(key).as_ref(),
        contract,
        "catalog `en` value for `{key}` must equal the shipped English wording"
    );
}

#[test]
fn row_labels_are_english_per_spec() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // Row labels render through catalog `hotkey.row_*`; the en values pin the
    // shipped English wording byte-for-byte.
    assert_en_catalog(
        "hotkey.row_full_screen",
        &row_label(Action::FullScreen),
        "Full screen",
    );
    assert_en_catalog("hotkey.row_window", &row_label(Action::Window), "Window");
    assert_en_catalog("hotkey.row_rect", &row_label(Action::Rect), "Rect area");
}

#[test]
fn bind_hint_mentions_escape_and_backspace() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // One English line at the start of the bind form, painted on every pass
    // (§4.4 v8: it is not gated by capture state). Exact wording comes from
    // catalog `hotkey.bind_hint`; the old `is_ascii` proxy (spec §2 i18n
    // note) is replaced by locale-guarded en equality, because Stage 3
    // brings Cyrillic into the catalog — ASCII no longer means "English".
    assert_en_catalog(
        "hotkey.bind_hint",
        &bind_hint(),
        "Esc to cancel, Backspace to clear",
    );
}

#[test]
fn pad_capture_tooltip_mentions_escape_and_backspace() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // The pad field states the same affordances as the shared hint line
    // (§4.4, v2), sourced through catalog `gamepad.capture_tooltip`: the
    // exact en wording names Esc and Backspace, so the earlier `contains`
    // checks are subsumed by the byte-for-byte contract.
    assert_en_catalog(
        "gamepad.capture_tooltip",
        &pad_capture_tooltip(),
        "Press a gamepad button to bind it — Esc to cancel, Backspace to clear; click again to cancel.",
    );
}

#[test]
fn registration_failure_reason_for_reserved_key_is_unchanged() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // The reserved rect row has its own fixed wording (§4.1): a failed combo is
    // never reserved, so the readable-reason change must not touch it. The
    // reserved tooltip renders through catalog `hotkey.reserved_tooltip`.
    let reserved = Registration::Reserved {
        action: Action::Rect,
        combo: "Ctrl+PrintScreen".to_owned(),
    };
    let reserved_icon = icons::icon_for_status(&reserved).expect("info icon");
    assert_eq!(reserved_icon.tooltip, binding::reserved_tooltip());
    assert_en_catalog(
        "hotkey.reserved_tooltip",
        &reserved_icon.tooltip,
        "reserved — out of MVP scope",
    );

    let draft = Settings::default();
    assert_eq!(binding::keyboard_warning(&draft, Action::Rect, None), None);

    // A FAILED row passes its readable reason through verbatim (the draft
    // collision still outranks it — shipped Stage 10 rule). The reason is a
    // passthrough string, not catalog text, so it needs no locale key.
    let readable = "already registered by another application (PrintScreen is held elsewhere) — free that key or pick another";
    assert_eq!(
        binding::keyboard_warning(&draft, Action::FullScreen, Some(readable)),
        Some(readable.to_owned())
    );
}
