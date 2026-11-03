//! Filename template: renders screenshot save paths from a token template,
//! sanitizes rendered segments, validates the template shape, and computes
//! collision variants. Pure string/date logic only — no IO, no `Local::now()`
//! (time is injected, so unit tests are deterministic); folder scanning and
//! subfolder creation happen in `capture` at save time.
//!
//! Wiring arrives with Stage 4 (capture/save) and Stage 5 (CLI): until then
//! nothing consumes this module, so `dead_code` is allowed — the semantics
//! stay locked by unit tests meanwhile (same precedent as `capture`).
#![allow(dead_code)]

use chrono::{DateTime, Local};

/// Tokens recognized in a template, per spec §4.2/§7: `%Y %y %mo %d %H %M %S
/// %iso %t %n %ext`. A `%` followed by anything else is a typed error.
const TOKENS: &[&str] = &["Y", "y", "mo", "d", "H", "M", "S", "iso", "t", "n", "ext"];

/// `%t` truncation limit (spec §4.2: "обрезка до ~60 символов").
const TITLE_MAX_CHARS: usize = 60;

/// Replacement for an empty/whitespace-only title (spec §4.2).
const UNTITLED: &str = "untitled";

/// `%t` value when there is no window title — the full-screen target.
const FULLSCREEN_LITERAL: &str = "fullscreen";

/// Template failures, typed per module (`thiserror` enum, no stringly
/// `Err(String)`). Messages are toast/log-ready (spec §4.2: validation
/// failure → toast + log) and always quote the offending template.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    /// The template is an absolute path (`/…`, `\…`, `C:\…`, UNC); templates
    /// must be relative to `save_path`.
    #[error(
        "template must be a relative path inside the save folder, got absolute path: {template}"
    )]
    AbsoluteTemplate { template: String },

    /// The template contains a `..` segment, which would escape `save_path`.
    #[error("template must not contain \"..\" segments, got: {template}")]
    ParentSegment { template: String },

    /// The template contains a token outside [`TOKENS`]; `token` is the
    /// offending `%…` sequence (e.g. `"%month"`).
    #[error("unknown token \"{token}\" in template: {template}")]
    UnknownToken { token: String, template: String },
}

/// Render a template into a `save_path`-relative path with `/` separators.
///
/// Tokens per [`TOKENS`]; `%t` renders `title` (window target) or
/// `fullscreen` when `title` is `None` (full-screen target); `%n` renders
/// `counter` (the collision counter from the folder scan at save time —
/// callers normally pass 1); `%ext` renders `ext` (`png`/`jpg`/`webp`). Every
/// rendered segment is sanitized, so a `/` or `\` inside a token value becomes
/// `-` and never creates a new path segment.
///
/// Failure modes (see [`Error`]):
/// - `Error::AbsoluteTemplate`: the template is an absolute path.
/// - `Error::ParentSegment`: the template contains a `..` segment.
/// - `Error::UnknownToken`: a `%…` sequence that is not one of [`TOKENS`].
///
/// Never panics; missing subfolders are created by the caller at save time.
pub(crate) fn render(
    template: &str,
    now: DateTime<Local>,
    title: Option<&str>,
    ext: &str,
    counter: u32,
) -> Result<String, Error> {
    if is_absolute(template) {
        return Err(Error::AbsoluteTemplate {
            template: template.to_owned(),
        });
    }
    let mut segments = Vec::new();
    for segment in template.split(['/', '\\']) {
        if segment == ".." {
            return Err(Error::ParentSegment {
                template: template.to_owned(),
            });
        }
        segments.push(render_segment(segment, now, title, ext, counter, template)?);
    }
    Ok(segments.join("/"))
}

/// Collision variant of an already rendered path (which has no `%n`, or has
/// `%n` already resolved by [`render`]): counters >= 2 insert `-N` before the
/// extension of the final segment (`screenshot.png` → `screenshot-2.png`), so
/// an existing file is never overwritten. Counter 1 (or 0) returns the path
/// unchanged.
pub(crate) fn collision_variant(path: &str, counter: u32) -> String {
    if counter < 2 {
        return path.to_owned();
    }
    // Only the final segment is touched: the extension is the last `.` of it.
    let split_at = path.rfind(['/', '\\']).map(|index| index + 1).unwrap_or(0);
    let (dir, name) = path.split_at(split_at);
    match name.rfind('.') {
        Some(dot) => format!("{dir}{}-{counter}{}", &name[..dot], &name[dot..]),
        None => format!("{dir}{name}-{counter}"),
    }
}

/// True for `/…`, `\…` (incl. UNC `\\server\share`) and drive-prefixed `C:…`.
fn is_absolute(template: &str) -> bool {
    let bytes = template.as_bytes();
    matches!(bytes.first(), Some(b'/' | b'\\'))
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

/// Render one template segment: expand `%token` runs, then sanitize the result.
/// The `%` run must be followed by an exact token name; anything else is an
/// `UnknownToken` error (so strftime-style `%m`/`%month` is rejected, not
/// silently half-expanded).
fn render_segment(
    segment: &str,
    now: DateTime<Local>,
    title: Option<&str>,
    ext: &str,
    counter: u32,
    template: &str,
) -> Result<String, Error> {
    let mut rendered = String::new();
    let mut rest = segment;
    while let Some(percent) = rest.find('%') {
        rendered.push_str(&rest[..percent]);
        let after = &rest[percent + 1..];
        let run = after
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(after.len());
        let token = &after[..run];
        if !TOKENS.contains(&token) {
            return Err(Error::UnknownToken {
                token: format!("%{token}"),
                template: template.to_owned(),
            });
        }
        rendered.push_str(&token_value(token, now, title, ext, counter));
        rest = &after[run..];
    }
    rendered.push_str(rest);
    Ok(sanitize_segment(&rendered))
}

/// Value of a recognized token. `%t` is sanitized and truncated here (the
/// truncation applies to the title only, never to a whole segment, so
/// `%t-%iso.%ext` keeps its date and extension).
fn token_value(
    token: &str,
    now: DateTime<Local>,
    title: Option<&str>,
    ext: &str,
    counter: u32,
) -> String {
    match token {
        "Y" => now.format("%Y").to_string(),
        "y" => now.format("%y").to_string(),
        "mo" => now.format("%m").to_string(),
        "d" => now.format("%d").to_string(),
        "H" => now.format("%H").to_string(),
        "M" => now.format("%M").to_string(),
        "S" => now.format("%S").to_string(),
        "iso" => now.format("%Y-%m-%d_%H-%M-%S").to_string(),
        "t" => sanitize_segment(title.unwrap_or(FULLSCREEN_LITERAL))
            .chars()
            .take(TITLE_MAX_CHARS)
            .collect(),
        "n" => counter.to_string(),
        "ext" => ext.to_owned(),
        // Unreachable: `render_segment` rejects every token outside TOKENS,
        // whose names are exactly the arms above. Kept as a value, not a panic.
        other => other.to_owned(),
    }
}

/// Sanitize one path segment: leading/trailing whitespace dropped, an empty
/// result becomes `untitled`, then every invalid character is replaced by `-`.
/// Invalid = Windows-forbidden `<>:"/\|?*`, ASCII control chars (< 0x20, which
/// covers `\0`), i.e. the union of what Windows and POSIX filenames reject.
fn sanitize_segment(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return UNTITLED.to_owned();
    }
    trimmed
        .chars()
        .map(|c| if is_invalid_char(c) { '-' } else { c })
        .collect()
}

fn is_invalid_char(c: char) -> bool {
    matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c < '\u{20}'
}

#[cfg(test)]
#[path = "template.tests.rs"]
mod tests;
