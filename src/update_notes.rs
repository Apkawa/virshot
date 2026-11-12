//! Release DESCRIPTION for the update dialog (task 2026-11-11_auto-update,
//! Stage 4): the GitHub release `body` (git-cliff changelog markdown) plus the
//! release-page URL, read from the GitHub Releases API.
//!
//! One responsibility: the GitHub Releases API contract. The `release.yaml`
//! metadata contract lives in `update`; the fallback composition (summary
//! fields + release-page link) lives in `gui` (Stage 6), which consumes the
//! typed errors here and [`release_page_url`] — this module supplies data,
//! it does not render or decide fallbacks.
//!
//! A fetch happens ONLY when the dialog opens (lazy), never during a
//! background check: the anonymous GitHub API limit is 60 req/h, while the
//! periodic check goes through the release-asset download endpoint (`update`)
//! and costs no API quota. Nothing here schedules a fetch — the lazy rule is
//! enforced by the Stage 6 wiring.

use std::sync::mpsc;
use std::time::Duration;

use crate::config;

/// Whole-request budget for a notes fetch, from DNS to the last body byte
/// (ureq `timeout_global`). Same budget as the metadata fetch in `update`.
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// API endpoint for the newest stable release. `/releases/latest` excludes
/// prereleases, so the `nightly` tag never leaks into the stable channel.
const STABLE_NOTES_URL: &str = "https://api.github.com/repos/Apkawa/virshot/releases/latest";

/// API endpoint for the rolling `nightly` prerelease, addressed by tag: the
/// tag target moves per build, so this always describes the newest nightly.
const NIGHTLY_NOTES_URL: &str = "https://api.github.com/repos/Apkawa/virshot/releases/tags/nightly";

/// Release page for the stable channel — the fallback link when the API
/// fetch fails (`releases/latest` resolves to the newest stable release).
const STABLE_RELEASE_PAGE_URL: &str = "https://github.com/Apkawa/virshot/releases/latest";

/// Release page for the nightly channel — the tag page, since the rolling
/// prerelease is identified by its `nightly` tag, not by a version tag.
const NIGHTLY_RELEASE_PAGE_URL: &str = "https://github.com/Apkawa/virshot/releases/tag/nightly";

/// Notes fetch failures, typed per module (`thiserror` enum). `Clone` so one
/// outcome can be logged and surfaced as dialog text from the same value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum NotesError {
    /// The request itself failed: DNS/TLS/timeout/transport error from ureq,
    /// or the body could not be read. Carries the crate's error message.
    #[error("failed to fetch release notes: {0}")]
    Fetch(String),

    /// The API answered a non-2xx status. Data, not a crash: a `403` here is
    /// the anonymous GitHub API rate limit (60 req/h), which means "notes
    /// unavailable" — the caller falls back to the `release.yaml` summary plus
    /// [`release_page_url`], it is never fatal. A `404` on `stable` before any
    /// stable release exists is the same degraded outcome.
    #[error("release notes fetch returned HTTP status {0}")]
    HttpStatus(u16),

    /// The body is not a usable release description: malformed JSON, a
    /// missing/blank `html_url`, or a `body` that is not a string. Carries the
    /// serde_json message verbatim. An ABSENT or empty `body` is NOT an error
    /// — see [`parse_release_notes`].
    #[error("invalid release notes: {0}")]
    Parse(String),
}

/// The release description for the dialog. `body` is git-cliff markdown and
/// MAY BE EMPTY (git-cliff can render an empty changelog, and GitHub then
/// omits the field): the caller shows the `release.yaml` fallback summary
/// instead. `html_url` is the release page to link to.
#[derive(Debug)]
pub(crate) struct ReleaseNotes {
    pub body: String,
    pub html_url: String,
}

/// Wire shape of the GitHub API release object, trimmed to what the dialog
/// needs. Unknown keys are IGNORED (no `deny_unknown_fields`): the API carries
/// dozens of fields and may add more, so the contract is strict about what
/// MUST be present (`html_url`), not about what may appear. `body` is optional
/// so a missing field parses (see [`parse_release_notes`]).
#[derive(serde::Deserialize)]
struct ReleaseJson {
    #[serde(default)]
    body: Option<String>,
    html_url: String,
}

/// The API endpoint describing the release of `channel`.
pub(crate) fn notes_url(channel: config::UpdateChannel) -> &'static str {
    match channel {
        config::UpdateChannel::Stable => STABLE_NOTES_URL,
        config::UpdateChannel::Nightly => NIGHTLY_NOTES_URL,
    }
}

/// The release PAGE URL for `channel` — the fallback link when the notes
/// fetch fails, returned without any request.
pub(crate) fn release_page_url(channel: config::UpdateChannel) -> &'static str {
    match channel {
        config::UpdateChannel::Stable => STABLE_RELEASE_PAGE_URL,
        config::UpdateChannel::Nightly => NIGHTLY_RELEASE_PAGE_URL,
    }
}

/// Parses a GitHub API release object. Failure modes, all
/// [`NotesError::Parse`]: malformed JSON (serde syntax message), a missing or
/// blank `html_url` (nothing to link to), or a `body` that is not a string.
///
/// A MISSING or empty `body` yields an empty `body`, deliberately: git-cliff
/// may render an empty changelog and GitHub omits the field then, so an empty
/// description is data the dialog degrades on (fallback summary + link), not a
/// fetch failure.
pub(crate) fn parse_release_notes(json: &str) -> Result<ReleaseNotes, NotesError> {
    let parsed: ReleaseJson =
        serde_json::from_str(json).map_err(|err| NotesError::Parse(err.to_string()))?;

    let html_url = parsed.html_url;
    if html_url.trim().is_empty() {
        return Err(NotesError::Parse(
            "release JSON carries no usable html_url".to_owned(),
        ));
    }

    Ok(ReleaseNotes {
        // Owned by the parsed value, moved out — no clone needed.
        body: parsed.body.unwrap_or_default(),
        html_url,
    })
}

/// Blocking GET of the API release object. Failure modes:
/// [`NotesError::Fetch`] on a transport/TLS/timeout/body error,
/// [`NotesError::HttpStatus`] on any non-2xx status (a `403` is the anonymous
/// rate limit: notes unavailable, not fatal). Never panics. Not exercised by
/// unit tests — no network there.
///
/// The agent-config builder is duplicated from `update::fetch_manifest_text`
/// rather than reused: that function returns `UpdateError`, and this module
/// must produce [`NotesError`] for its own seam — a mapping layer would be an
/// abstraction over three lines (AGENTS.md: tolerate local duplication).
pub(crate) fn fetch_release_notes(url: &str) -> Result<ReleaseNotes, NotesError> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .timeout_global(Some(FETCH_TIMEOUT))
            .user_agent(format!("virshot/{}", crate::build::PKG_VERSION))
            .max_redirects(10)
            .http_status_as_error(false) // non-2xx is data: read status, never panic
            .build(),
    );

    let mut response = agent
        .get(url)
        .call()
        .map_err(|err| NotesError::Fetch(err.to_string()))?;

    let status = response.status();
    if !status.is_success() {
        return Err(NotesError::HttpStatus(status.into()));
    }

    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|err| NotesError::Fetch(err.to_string()))?;

    parse_release_notes(&body)
}

/// The notes pipeline, pure behind the `fetch` seam: pick the endpoint for
/// `channel` and fetch through it. Production passes [`fetch_release_notes`]
/// (fetch AND parse live in this module); tests inject a fake fetcher that
/// parses fixture JSON the same way, so URL selection and parse coverage
/// stay without any network. Every failure becomes a typed [`NotesError`];
/// never panics.
pub(crate) fn run_notes(
    channel: config::UpdateChannel,
    fetch: impl Fn(&str) -> Result<ReleaseNotes, NotesError>,
) -> Result<ReleaseNotes, NotesError> {
    fetch(notes_url(channel))
}

/// Spawns ONE plain `std::thread` that runs [`run_notes`] and delivers exactly
/// one value to `sender` — `Ok(ReleaseNotes)` or a typed `Err` — then closes
/// the channel. Called when the update dialog OPENS (lazy), never from a
/// background check. The thread never panics (`run_notes` is total); if the
/// receiver is gone (dialog closed / app shutdown), the send error is ignored.
/// Production passes [`fetch_release_notes`] directly — no error mapping layer
/// sits between the seam and this module's own error enum.
pub(crate) fn spawn_notes_fetch(
    channel: config::UpdateChannel,
    sender: mpsc::Sender<Result<ReleaseNotes, NotesError>>,
    fetch: impl Fn(&str) -> Result<ReleaseNotes, NotesError> + Send + 'static,
) {
    std::thread::spawn(move || {
        let outcome = run_notes(channel, fetch);
        let _ = sender.send(outcome);
    });
}

#[cfg(test)]
#[path = "update_notes.tests.rs"]
mod tests;
