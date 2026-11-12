//! Sibling tests for `update_notes`: GitHub Releases API JSON parsing (the
//! release `body` git-cliff renders, plus `html_url`), channel URL pinning for
//! both API endpoints and both release-page fallback links, and the notes
//! pipeline behind an injected fake fetcher. No network is performed here:
//! `fetch_release_notes` is exercised only by the real app, never by these
//! tests.

use super::*;
use crate::config::UpdateChannel;
use std::sync::Arc;
use std::sync::Mutex;

/// The REAL GitHub API response shape for a tag release, trimmed to the
/// fields this module reads: `body` is git-cliff markdown with escaped
/// `\n` (so parsing must unescape them into real newlines), `html_url` is the
/// release page the dialog links to.
const RELEASE_FIXTURE: &str = r###"{
  "name": "virshot 0.1.0",
  "tag_name": "v0.1.0",
  "html_url": "https://github.com/Apkawa/virshot/releases/tag/v0.1.0",
  "body": "## Changelog\n\n### Added\n\n- Gamepad trigger via GameInput (`Guide` button)\n- Settings window with hotkey capture\n\n### Fixed\n\n- Tray click no longer re-opens the hidden window\n"
}"###;

/// The expected `body` of [`RELEASE_FIXTURE`] after JSON unescaping: real
/// newlines, not literal `\n` sequences.
const RELEASE_FIXTURE_BODY: &str = "## Changelog\n\n### Added\n\n- Gamepad trigger via GameInput (`Guide` button)\n- Settings window with hotkey capture\n\n### Fixed\n\n- Tray click no longer re-opens the hidden window\n";

/// The rolling `nightly` prerelease response: same shape, its own page URL.
const NIGHTLY_FIXTURE: &str = r###"{
  "name": "nightly",
  "tag_name": "nightly",
  "html_url": "https://github.com/Apkawa/virshot/releases/tag/nightly",
  "body": "## Changelog\n\n### Features\n\n- Update dialog renders release notes\n\n### Fixes\n\n- Tray exit path restores the hotkey manager\n"
}"###;

/// Fake fetcher standing in for [`fetch_release_notes`] — the Stage 7 seam
/// returns PARSED notes: record the URL it was asked for, then parse the
/// fixed payload exactly as production does, so channel selection AND parse
/// coverage stay without any request. The `Arc` is shared ownership: the
/// fetcher owns the cell while the test reads it back.
fn recording_fetch(
    text: &'static str,
    seen: Arc<Mutex<String>>,
) -> impl Fn(&str) -> Result<ReleaseNotes, NotesError> {
    move |url| {
        *seen.lock().unwrap() = url.to_owned();
        parse_release_notes(text)
    }
}

fn ok_fetch(text: &'static str) -> impl Fn(&str) -> Result<ReleaseNotes, NotesError> {
    move |_url| parse_release_notes(text)
}

fn err_fetch(err: NotesError) -> impl Fn(&str) -> Result<ReleaseNotes, NotesError> {
    move |_url| Err(err.clone())
}

#[test]
fn notes_url_channels_are_exact_and_distinct() {
    assert_eq!(
        notes_url(UpdateChannel::Stable),
        "https://api.github.com/repos/Apkawa/virshot/releases/latest"
    );
    assert_eq!(
        notes_url(UpdateChannel::Nightly),
        "https://api.github.com/repos/Apkawa/virshot/releases/tags/nightly"
    );
    assert_ne!(
        notes_url(UpdateChannel::Stable),
        notes_url(UpdateChannel::Nightly),
        "the channels must not share one endpoint"
    );
}

#[test]
fn release_page_url_channels_are_exact_and_distinct() {
    assert_eq!(
        release_page_url(UpdateChannel::Stable),
        "https://github.com/Apkawa/virshot/releases/latest"
    );
    assert_eq!(
        release_page_url(UpdateChannel::Nightly),
        "https://github.com/Apkawa/virshot/releases/tag/nightly"
    );
    assert_ne!(
        release_page_url(UpdateChannel::Stable),
        release_page_url(UpdateChannel::Nightly),
        "the fallback link must follow the channel"
    );
}

#[test]
fn parse_release_notes_real_shape_extracts_body_and_html_url() {
    let notes = parse_release_notes(RELEASE_FIXTURE).expect("the real API shape must parse");

    assert_eq!(notes.body, RELEASE_FIXTURE_BODY);
    assert_eq!(
        notes.html_url,
        "https://github.com/Apkawa/virshot/releases/tag/v0.1.0"
    );
    assert_eq!(
        notes.body.lines().count(),
        10,
        "escaped \\n must become real newlines"
    );
}

#[test]
fn parse_release_notes_missing_body_yields_empty_body() {
    // git-cliff can produce an empty body, and GitHub omits the field then:
    // an empty changelog is data (the dialog shows the fallback summary),
    // not a parse failure.
    let notes = parse_release_notes(
        r#"{"html_url": "https://github.com/Apkawa/virshot/releases/tag/nightly"}"#,
    )
    .expect("a missing body must not be an error");

    assert_eq!(notes.body, "");
    assert_eq!(
        notes.html_url,
        "https://github.com/Apkawa/virshot/releases/tag/nightly"
    );
}

#[test]
fn parse_release_notes_empty_body_string_yields_empty_body() {
    let notes = parse_release_notes(
        r#"{"body": "", "html_url": "https://github.com/Apkawa/virshot/releases/tag/v0.1.0"}"#,
    )
    .expect("an explicit empty body must parse");

    assert_eq!(notes.body, "");
}

#[test]
fn parse_release_notes_missing_html_url_returns_parse_error() {
    let err = parse_release_notes(r###"{"body": "## Changelog\n\n- notes\n"}"###)
        .expect_err("html_url is required");

    assert!(matches!(err, NotesError::Parse(_)));
}

#[test]
fn parse_release_notes_blank_html_url_returns_parse_error() {
    let err = parse_release_notes(r###"{"body": "## Changelog", "html_url": ""}"###)
        .expect_err("a blank html_url cannot be linked");

    assert!(matches!(err, NotesError::Parse(_)));
}

#[test]
fn parse_release_notes_malformed_json_returns_parse_error() {
    let err = parse_release_notes("{\"body\": \"notes\", \"html_url\":")
        .expect_err("truncated JSON must not parse");

    assert!(matches!(err, NotesError::Parse(_)));
}

#[test]
fn parse_release_notes_non_string_body_returns_parse_error() {
    let err = parse_release_notes(
        r#"{"body": 7, "html_url": "https://github.com/Apkawa/virshot/releases/tag/v0.1.0"}"#,
    )
    .expect_err("a non-string body is not changelog markdown");

    assert!(matches!(err, NotesError::Parse(_)));
}

#[test]
fn run_notes_stable_channel_fetches_latest_url() {
    let seen = Arc::new(Mutex::new(String::new()));
    let notes = run_notes(
        UpdateChannel::Stable,
        recording_fetch(RELEASE_FIXTURE, Arc::clone(&seen)),
    )
    .expect("stable fetch must produce notes");

    assert_eq!(
        seen.lock().unwrap().as_str(),
        "https://api.github.com/repos/Apkawa/virshot/releases/latest"
    );
    assert_eq!(notes.body, RELEASE_FIXTURE_BODY);
}

#[test]
fn run_notes_nightly_channel_fetches_tags_url() {
    let seen = Arc::new(Mutex::new(String::new()));
    let notes = run_notes(
        UpdateChannel::Nightly,
        recording_fetch(NIGHTLY_FIXTURE, Arc::clone(&seen)),
    )
    .expect("nightly fetch must produce notes");

    assert_eq!(
        seen.lock().unwrap().as_str(),
        "https://api.github.com/repos/Apkawa/virshot/releases/tags/nightly"
    );
    assert_eq!(
        notes.html_url,
        "https://github.com/Apkawa/virshot/releases/tag/nightly"
    );
}

#[test]
fn run_notes_http_403_propagates_http_status_error() {
    // Anonymous GitHub API limit (60 req/h) answered 403 from this sandbox:
    // the error must reach the dialog intact so it can render the fallback.
    let err = run_notes(
        UpdateChannel::Stable,
        err_fetch(NotesError::HttpStatus(403)),
    )
    .expect_err("a rate-limited fetch must not yield notes");

    assert_eq!(err, NotesError::HttpStatus(403));
}

#[test]
fn spawn_notes_fetch_sends_one_ok_value() {
    let (sender, receiver) = mpsc::channel();

    spawn_notes_fetch(UpdateChannel::Nightly, sender, ok_fetch(NIGHTLY_FIXTURE));

    let outcome = receiver.recv().expect("the thread must send one value");
    assert!(outcome.is_ok(), "a successful fetch must send Ok notes");
    assert!(
        receiver.recv().is_err(),
        "the thread must send exactly one value and close the channel"
    );
}

#[test]
fn spawn_notes_fetch_sends_one_err_value() {
    let (sender, receiver) = mpsc::channel();

    spawn_notes_fetch(
        UpdateChannel::Stable,
        sender,
        err_fetch(NotesError::Fetch("no network".to_owned())),
    );

    let outcome = receiver.recv().expect("the thread must send one value");
    assert_eq!(
        outcome.expect_err("a failed fetch must send Err"),
        NotesError::Fetch("no network".to_owned())
    );
    assert!(
        receiver.recv().is_err(),
        "the thread must send exactly one value and close the channel"
    );
}
