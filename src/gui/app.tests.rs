use super::*;

use crate::config::Binding;

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

/// Hotkey-row fixture: one `Binding` exactly as a draft row would hold it.
fn binding(key: &str, mods: &[&str], pad: Option<&str>) -> Binding {
    Binding {
        key: key.to_owned(),
        mods: mods.iter().copied().map(str::to_owned).collect(),
        pad: pad.map(str::to_owned),
    }
}

#[test]
fn bottom_buttons_save_disabled_while_bindings_duplicate() {
    let mut draft = Settings::default();
    draft.controls.window = binding("PrintScreen", &[], None);
    assert!(chrome::is_dirty(&draft, &Settings::default()));
    assert!(!binding::duplicate_bindings(&draft).is_empty());
    // The Stage-3 gate composes uniqueness into `savable`; Cancel stays free.
    let savable = binding::draft_savable(&draft) && binding::duplicate_bindings(&draft).is_empty();
    assert!(!savable, "a duplicated draft must not be savable");
    let blocked = chrome::bottom_buttons_enabled(true, savable);
    assert!(
        !blocked.apply_enabled,
        "Apply must be disabled for a duplicate"
    );
    assert!(!blocked.ok_enabled, "OK must be disabled for a duplicate");
    assert!(
        blocked.cancel_enabled,
        "Cancel stays enabled while a duplicate blocks saving"
    );
}

#[test]
fn tab_labels_are_english() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    // Each tab header renders through its catalog key; the en value pins the
    // shipped English wording byte-for-byte.
    assert_en_catalog("general.tab_general", &Tab::General.label(), "General");
    assert_en_catalog("general.tab_hotkeys", &Tab::Hotkeys.label(), "Hotkeys");
    assert_en_catalog("general.tab_overlay", &Tab::Overlay.label(), "Overlay");
}

/// Stage 3 (spec §2/§7): under the `ru` locale the tab headers render the
/// Russian catalog values. `ru` is restored BEFORE the assertions so a
/// failure never leaves it installed for other tests.
#[test]
fn tab_labels_follow_russian_locale() {
    let _guard = locale_guard();
    rust_i18n::set_locale("ru");

    let labels = [
        Tab::General.label(),
        Tab::Hotkeys.label(),
        Tab::Overlay.label(),
    ];

    rust_i18n::set_locale("en");

    assert_eq!(
        labels,
        [
            "Общие".to_owned(),
            "Горячие клавиши".to_owned(),
            "Оверлей".to_owned(),
        ],
        "tab headers must render Russian under the ru locale"
    );
}
