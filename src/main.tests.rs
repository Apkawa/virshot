//! Unit tests for the Stage 7 startup decisions in `main.rs` (task
//! 2026-11-11_auto-update, spec §4.7/§5): whether a startup check is armed,
//! and what the restart sequence does when spawning the new process fails.
//! Pure logic only — no threads, no network, no process spawn.

use super::*;

#[test]
fn should_check_at_startup_regular_interval_release_build_arms_check() {
    // Every scheduled interval arms the startup check on a release build
    // (spec §4.7: background check at startup AND by interval).
    assert!(
        should_check_at_startup(config::UpdateCheck::Hour, false),
        "Hour must arm the startup check"
    );
    assert!(
        should_check_at_startup(config::UpdateCheck::Day, false),
        "Day (the default) must arm the startup check"
    );
    assert!(
        should_check_at_startup(config::UpdateCheck::Week, false),
        "Week must arm the startup check"
    );
}

#[test]
fn should_check_at_startup_never_interval_skips_check() {
    // `Never` means NO network at startup: the GUI-less park path and the GUI
    // path both stay silent (spec §4.7).
    assert!(
        !should_check_at_startup(config::UpdateCheck::Never, false),
        "Never must skip the startup check"
    );
}

#[test]
fn should_check_at_startup_dev_build_regular_interval_arms_check() {
    // A `-dev` build is effectively a nightly from `master` (spec §4.7), so it
    // checks updates on the same terms as a release build: dev-ness no longer
    // excludes the startup check.
    assert!(
        should_check_at_startup(config::UpdateCheck::Day, true),
        "a dev build with a scheduled interval must arm the startup check"
    );
    assert!(
        should_check_at_startup(config::UpdateCheck::Hour, true),
        "a dev build must arm the startup check on every scheduled interval"
    );
}

#[test]
fn restart_after_failed_spawn_reacquired_lock_keeps_process_alive() {
    // Spawn failed but the instance lock was re-taken: the OLD process stays
    // resident (spec §5 open question: the restart is abandoned, the app
    // keeps running with the new version on disk).
    assert_eq!(
        restart_after_failed_spawn(true),
        None,
        "a re-taken lock must keep the process resident (no exit code)"
    );
}

#[test]
fn restart_after_failed_spawn_unusable_lock_exits_with_failure() {
    // Spawn failed AND the lock cannot be re-taken: staying alive would leave
    // the process unguarded, so this run exits with a typed failure.
    assert_eq!(
        restart_after_failed_spawn(false),
        Some(ExitCode::FAILURE),
        "an unusable lock must end this run with exit 1"
    );
}
