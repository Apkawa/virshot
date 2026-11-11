//! Sibling tests for `about`: version-line formatting, the localized About
//! build line and app title, the embedded changelog, the About window chrome
//! policy, and the `AboutWindow` lifecycle state machine (stage 10). Pure
//! logic; the viewport paint itself is GUI.

use super::*;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

/// `rust_i18n::set_locale` is global process state; serialise locale tests
/// like `tray.tests.rs` does and restore `en` on drop.
static LOCALE_LOCK: Mutex<()> = Mutex::new(());

fn locale_guard() -> std::sync::MutexGuard<'static, ()> {
    LOCALE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn version_line_full_inputs_keeps_exact_contract() {
    let line = version_line("0.3.0-dev", "ab74ee7", "2026-11-10T15:24:32+03:00");
    assert_eq!(
        line,
        "virshot 0.3.0-dev (ab74ee7) built 2026-11-10T15:24:32+03:00"
    );
}

#[test]
fn version_line_missing_sha_and_time_render_unknown() {
    let line = version_line("0.3.0-dev", "", "");
    assert_eq!(line, "virshot 0.3.0-dev (unknown) built unknown");
    assert!(line.contains("built "), "the built marker must survive");
    assert!(
        !line.contains("()"),
        "empty sha must never render as empty parens: {line}"
    );
}

#[test]
fn version_from_build_keeps_moscow_offset_when_embedded() {
    // VIRSHOT_BUILD_TIME is embedded by build.rs for every real build here
    // (clock is readable), so the extended line carries the Moscow offset.
    let line = version_from_build();
    assert!(line.starts_with("virshot "));
    assert!(line.contains(env!("CARGO_PKG_VERSION")));
    assert!(line.contains("built "));
    assert!(
        line.contains("+03:00"),
        "Moscow offset must be embedded: {line}"
    );
}

#[test]
fn changelog_embeds_repo_changelog() {
    assert!(
        !CHANGELOG.is_empty(),
        "CHANGELOG.md must be embedded into the binary"
    );
    assert!(
        CHANGELOG.contains("## "),
        "changelog must carry release sections"
    );
}

#[test]
fn repo_url_pinned_value_matches_origin_remote() {
    // Provenance: `git remote -v` is `git@github.com:Apkawa/virshot.git` and
    // no manifest declares a `repository` field, so the About dialog's link
    // must stay pinned to exactly this web URL.
    assert_eq!(REPO_URL, "https://github.com/Apkawa/virshot");
}

#[test]
fn build_line_full_inputs_renders_sha_and_date_without_marker() {
    // Stage 12 (spec v5): the debug/release marker is gone — the About build
    // line is `<sha> <date>` only.
    let line = build_line("bf7988e", "2026-11-10T19:09:21+03:00");

    assert_eq!(line, "bf7988e 2026-11-10T19:09:21+03:00");
    assert!(
        !line.contains('·'),
        "the marker separator must be gone: {line}"
    );
    assert!(
        !line.contains("release") && !line.contains("debug"),
        "no channel marker may survive in any form: {line}"
    );
}

#[test]
fn build_line_missing_sha_or_time_renders_unknown() {
    let missing_sha = build_line("", "2026-11-10T19:09:21+03:00");
    let missing_time = build_line("bf7988e", "");
    let missing_both = build_line("", "");

    assert_eq!(missing_sha, "unknown 2026-11-10T19:09:21+03:00");
    assert_eq!(missing_time, "bf7988e unknown");
    assert_eq!(missing_both, "unknown unknown");
}

#[test]
fn version_with_dev_exact_release_tag_returns_bare_version() {
    // HEAD carries the release tag `v<version>` ⇒ the bare version, no suffix.
    assert_eq!(version_with_dev("0.1.0", "v0.1.0"), "0.1.0");
}

#[test]
fn version_with_dev_untagged_head_appends_dev_suffix() {
    // shadow-rs `TAG` is "" when HEAD carries no tag (R12), so local builds
    // from an untagged HEAD render dev.
    assert_eq!(version_with_dev("0.1.0", ""), "0.1.0-dev");
}

#[test]
fn version_with_dev_non_release_tag_appends_dev_suffix() {
    // The rolling prerelease tag `nightly` is not the release tag ⇒ dev.
    assert_eq!(version_with_dev("0.1.0", "nightly"), "0.1.0-dev");
}

#[test]
fn version_with_dev_other_version_tag_appends_dev_suffix() {
    // A release tag for a DIFFERENT version does not vouch for this build.
    assert_eq!(version_with_dev("0.2.0", "v0.1.0"), "0.2.0-dev");
}

#[test]
fn window_chrome_keeps_title_close_and_resize_but_drops_minimize_maximize() {
    // Stage 9 policy: the About child window keeps its title bar, close
    // button and OS resize border, but must expose NO minimize and NO
    // maximize buttons.
    let chrome = window_chrome();

    assert!(chrome.decorations, "title bar decorations must stay on");
    assert!(chrome.resizable, "the OS resize border must stay on");
    assert!(chrome.close_button, "the close button must stay on");
    assert!(!chrome.minimize_button, "the minimize button must be off");
    assert!(!chrome.maximize_button, "the maximize button must be off");
}

#[test]
fn select_from_tray_while_closed_opens_and_wants_registration() {
    let mut window = AboutWindow::new();
    assert!(!window.wants_registration(), "fresh state must be closed");
    assert!(
        window.select_from_tray(),
        "tray select on a closed window opens it"
    );
    assert!(
        window.wants_registration(),
        "an open window must be registered"
    );
    assert!(
        window.close_flag().is_some(),
        "an open window hands a close flag to the paint callback"
    );
}

#[test]
fn select_from_tray_while_open_returns_false_and_window_stays_registered() {
    // User decision 2026-11-11: re-selecting About RAISES the open window
    // (`ViewportCommand::Focus`); it must never close or re-open it.
    let mut window = AboutWindow::new();
    assert!(window.select_from_tray());
    assert!(
        !window.select_from_tray(),
        "already open ⇒ focus path, not re-open"
    );
    assert!(
        window.wants_registration(),
        "the focus path must leave registration intact"
    );
}

#[test]
fn observe_close_with_flag_set_stops_registration() {
    let mut window = AboutWindow::new();
    window.select_from_tray();
    let flag = window.close_flag().expect("open window has a close flag");
    flag.store(true, Ordering::Relaxed);
    assert!(window.observe_close(), "a recorded close must report it");
    assert!(
        !window.wants_registration(),
        "a closed window must stop registering (egui GC drops it, eframe drops the window)"
    );
    assert!(
        window.close_flag().is_none(),
        "closed ⇒ nothing to hand to a paint callback"
    );
    assert!(
        !window.observe_close(),
        "a closed window cannot close twice"
    );
}

#[test]
fn observe_close_without_flag_keeps_window_open() {
    let mut window = AboutWindow::new();
    window.select_from_tray();
    assert!(!window.observe_close(), "no close request ⇒ stays open");
    assert!(window.wants_registration(), "stays registered while open");
}

#[test]
fn select_from_tray_after_close_installs_fresh_close_flag() {
    let mut window = AboutWindow::new();
    window.select_from_tray();
    let stale = window.close_flag().expect("open window has a close flag");
    stale.store(true, Ordering::Relaxed);
    window.observe_close();
    assert!(window.select_from_tray(), "re-open after close must open");
    let fresh = window
        .close_flag()
        .expect("re-opened window has a close flag");
    assert!(
        !fresh.load(Ordering::Relaxed),
        "the reopened window's flag must be false, not stale"
    );
    assert!(
        !Arc::ptr_eq(&stale, &fresh),
        "a reopened window must not inherit the stale flag Arc"
    );
}

#[test]
fn app_title_uses_brand_case_and_version() {
    // Stage 12: the header renders the dev version through the same catalog
    // key — brand case stays verbatim, the `-dev` suffix passes through.
    let _guard = locale_guard();

    rust_i18n::set_locale("ru");
    let ru_title = app_title("0.1.0-dev");
    rust_i18n::set_locale("en");
    let en_title = app_title("0.1.0-dev");
    rust_i18n::set_locale("en");

    assert_eq!(ru_title, "ViRShot 0.1.0-dev");
    assert_eq!(en_title, "ViRShot 0.1.0-dev");
}
