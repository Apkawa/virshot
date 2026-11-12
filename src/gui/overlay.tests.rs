use super::*;

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

/// The Overlay tab test-button gate (Stage 7): enabled exactly while the
/// "Show overlay toast" checkbox is checked. A direct `ToastHandle::show`
/// bypasses `capture::should_show`, so this GUI-side gate is the only thing
/// keeping the test chip honest to spec §4.3.
#[test]
fn overlay_test_button_enabled_only_when_overlay_toast_enabled() {
    assert!(overlay_test_button_enabled(true));
    assert!(!overlay_test_button_enabled(false));
}

/// The test chip text is the real save success wording (spec §4.3): built by
/// the very helper `capture::toast_from_outcome` uses for a real save, so the
/// button cannot drift from the `Saved: <file name>` format.
#[test]
fn overlay_test_message_matches_real_save_success_wording() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    assert_eq!(
        overlay_test_message(OVERLAY_TEST_FILENAME),
        rust_i18n::t!("toast.saved", file_name = OVERLAY_TEST_FILENAME).into_owned(),
        "the chip text must render through catalog toast.saved"
    );
    assert_eq!(
        overlay_test_message(OVERLAY_TEST_FILENAME),
        "Saved: test.png",
        "catalog `en` value must match the shipped English contract"
    );
    assert_eq!(
        overlay_test_message(OVERLAY_TEST_FILENAME),
        capture::saved_text(Path::new("/any/dir/test.png")),
        "the test chip must spell the success text exactly like a real save"
    );
}

/// Pure half of the "no file" promise (Stage 7): the test chip is a text chip
/// carrying the fixed honest name — no image bytes, no save path — so the
/// click path has nothing to write; the Xvfb driver proves `out/` stays empty
/// and the log never carries a real `Saved: <path>` capture line.
#[test]
fn overlay_test_does_not_write_a_file() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    let chip = test_overlay_toast(&Overlay::default());
    assert!(chip.image.is_none(), "test chip must carry no image bytes");
    assert_eq!(
        chip.text,
        overlay_test_message(OVERLAY_TEST_FILENAME),
        "chip text must name the honest test file, never a real saved path"
    );
}

/// The chip reflects the CURRENT draft — corner, monitor selection, and the
/// clamped hold follow the same rules a real save applies to `config::Overlay`,
/// so the chip shows what the user just configured even before Apply.
#[test]
fn overlay_test_toast_mirrors_draft_position_duration_and_monitors() {
    let draft = Overlay {
        enabled: true,
        position: OverlayPosition::TopRight,
        duration_ms: 250,
        on_all_monitors: false,
    };
    let chip = test_overlay_toast(&draft);
    assert_eq!(chip.corner, Corner::TopRight);
    assert_eq!(chip.monitor, MonitorSel::Index(0), "primary is index 0");
    assert_eq!(chip.duration_ms, 250);
    assert!(matches!(chip.color, ToastColor::Success));

    let sticky_all = Overlay {
        enabled: true,
        position: OverlayPosition::BottomLeft,
        duration_ms: 0,
        on_all_monitors: true,
    };
    let chip = test_overlay_toast(&sticky_all);
    assert_eq!(chip.duration_ms, 0, "0 stays sticky, like a real chip");
    assert_eq!(chip.monitor, MonitorSel::All);
    assert_eq!(chip.corner, Corner::BottomLeft);

    let huge = Overlay {
        duration_ms: u64::MAX,
        ..Overlay::default()
    };
    assert_eq!(
        test_overlay_toast(&huge).duration_ms,
        u32::MAX,
        "an absurd duration clamps instead of wrapping, like capture's rule"
    );
}

/// Disabled-button tooltip wording (pinned, spec §4.4 English UI): the hint
/// names the checkbox the user must tick before the test chip is possible,
/// sourced through catalog `overlay.test_disabled_hint` under a pinned `en`.
#[test]
fn overlay_test_disabled_hint_names_the_overlay_checkbox() {
    let _guard = locale_guard();
    rust_i18n::set_locale("en");

    assert_en_catalog(
        "overlay.test_disabled_hint",
        &overlay_test_disabled_hint(),
        "Enable the overlay toast to test it",
    );
}
