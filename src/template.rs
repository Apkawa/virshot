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
mod tests {
    use super::Error;
    use super::collision_variant;
    use super::render;
    use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};

    /// Fixed injected instant: 2026-11-03 00:12:05 local — the spec §4.2
    /// `%iso` example (`2026-11-03_00-12-05`). Wall-clock values are what the
    /// tokens render, so the expectation holds in any host time zone.
    fn fixed_now() -> DateTime<Local> {
        let naive = NaiveDateTime::new(
            NaiveDate::from_ymd_opt(2026, 11, 3).unwrap(),
            NaiveTime::from_hms_opt(0, 12, 5).unwrap(),
        );
        Local.from_local_datetime(&naive).unwrap()
    }

    /// Second fixed instant with single-digit month/day/hour/minute/second.
    fn fixed_padded_now() -> DateTime<Local> {
        let naive = NaiveDateTime::new(
            NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(),
            NaiveTime::from_hms_opt(3, 4, 5).unwrap(),
        );
        Local.from_local_datetime(&naive).unwrap()
    }

    fn full_screen_template() -> &'static str {
        "FullScreen/%y-%mo/%y-%mo-%d/%iso.%ext"
    }

    fn window_template() -> &'static str {
        "%t/%y-%mo/%y-%mo-%d/%t-%iso.%ext"
    }

    // ---------- token rendering ----------

    #[test]
    fn render_full_screen_default_template_renders_iso_path() {
        let path = render(full_screen_template(), fixed_now(), None, "png", 1).unwrap();
        assert_eq!(path, "FullScreen/26-11/26-11-03/2026-11-03_00-12-05.png");
    }

    #[test]
    fn render_window_default_template_renders_title_path() {
        let path = render(
            window_template(),
            fixed_now(),
            Some("Cyberpunk 2077"),
            "jpg",
            1,
        )
        .unwrap();
        assert_eq!(
            path,
            "Cyberpunk 2077/26-11/26-11-03/Cyberpunk 2077-2026-11-03_00-12-05.jpg"
        );
    }

    #[test]
    fn render_all_tokens_in_one_segment_expands_in_order() {
        let path = render("%Y-%y-%mo-%d-%H-%M-%S-%n-%ext", fixed_now(), None, "png", 1).unwrap();
        assert_eq!(path, "2026-26-11-03-00-12-05-1-png");
    }

    #[test]
    fn render_iso_token_matches_spec_example_format() {
        let path = render("%iso.%ext", fixed_now(), None, "png", 1).unwrap();
        assert_eq!(path, "2026-11-03_00-12-05.png");
    }

    #[test]
    fn render_date_tokens_are_zero_padded() {
        let path = render(
            "%y-%mo-%d-%H-%M-%S.%ext",
            fixed_padded_now(),
            None,
            "png",
            1,
        )
        .unwrap();
        assert_eq!(path, "26-01-02-03-04-05.png");
    }

    #[test]
    fn render_title_none_becomes_fullscreen_literal() {
        let path = render("%t.%ext", fixed_now(), None, "png", 1).unwrap();
        assert_eq!(path, "fullscreen.png");
    }

    #[test]
    fn render_is_deterministic_for_injected_datetime() {
        let first = render(full_screen_template(), fixed_now(), None, "png", 1).unwrap();
        let second = render(full_screen_template(), fixed_now(), None, "png", 1).unwrap();
        assert_eq!(first, second, "same inputs must render identically");
    }

    #[test]
    fn render_n_token_uses_counter_value_in_place() {
        let path = render("%t-%n-%iso.%ext", fixed_now(), Some("game"), "webp", 2).unwrap();
        assert_eq!(path, "game-2-2026-11-03_00-12-05.webp");
    }

    #[test]
    fn render_backslash_separator_is_normalized_to_slash() {
        let path = render("FullScreen\\%y-%mo\\%iso.%ext", fixed_now(), None, "png", 1).unwrap();
        assert_eq!(path, "FullScreen/26-11/2026-11-03_00-12-05.png");
    }

    // ---------- sanitization ----------

    #[test]
    fn sanitize_title_replaces_windows_forbidden_chars_with_dash() {
        let title = r#"a<b>c:d"e|f?g*h"#;
        let path = render("%t.%ext", fixed_now(), Some(title), "png", 1).unwrap();
        assert_eq!(path, "a-b-c-d-e-f-g-h.png");
    }

    #[test]
    fn sanitize_title_slash_never_creates_new_segment() {
        let path = render(
            "%t/%iso.%ext",
            fixed_now(),
            Some("C:/Windows/game.exe"),
            "png",
            1,
        )
        .unwrap();
        assert_eq!(path, "C--Windows-game.exe/2026-11-03_00-12-05.png");
        assert_eq!(path.split('/').count(), 2, "title must not add a segment");
    }

    #[test]
    fn sanitize_title_replaces_control_chars_with_dash() {
        let path = render("%t.%ext", fixed_now(), Some("a\tb\nc\x01d"), "png", 1).unwrap();
        assert_eq!(path, "a-b-c-d.png");
    }

    #[test]
    fn sanitize_empty_title_becomes_untitled() {
        let path = render("%t.%ext", fixed_now(), Some(""), "png", 1).unwrap();
        assert_eq!(path, "untitled.png");
    }

    #[test]
    fn sanitize_whitespace_only_title_becomes_untitled() {
        let path = render("%t.%ext", fixed_now(), Some("   \t\n "), "png", 1).unwrap();
        assert_eq!(path, "untitled.png");
    }

    #[test]
    fn sanitize_title_truncates_to_60_chars_and_keeps_extension() {
        let title = "T".repeat(70);
        let path = render("%t.%ext", fixed_now(), Some(&title), "png", 1).unwrap();
        assert_eq!(path, format!("{}.png", "T".repeat(60)));
    }

    #[test]
    fn sanitize_folder_segment_invalid_chars_become_dash() {
        let path = render("My<Folders>/%iso.%ext", fixed_now(), None, "png", 1).unwrap();
        assert_eq!(path, "My-Folders-/2026-11-03_00-12-05.png");
    }

    #[test]
    fn sanitize_keeps_unicode_title_unchanged() {
        let path = render("%t.%ext", fixed_now(), Some("Привет, мир!"), "png", 1).unwrap();
        assert_eq!(path, "Привет, мир!.png");
    }

    // ---------- validation ----------

    #[test]
    fn render_absolute_unix_path_is_rejected() {
        let err = render("/tmp/shots/%iso.%ext", fixed_now(), None, "png", 1).unwrap_err();
        assert!(
            matches!(&err, Error::AbsoluteTemplate { template } if template == "/tmp/shots/%iso.%ext"),
            "expected AbsoluteTemplate, got {err:?}"
        );
    }

    #[test]
    fn render_absolute_windows_drive_path_is_rejected() {
        let template = r"C:\Users\User\Pictures\%iso.%ext";
        let err = render(template, fixed_now(), None, "png", 1).unwrap_err();
        assert!(
            matches!(&err, Error::AbsoluteTemplate { .. }),
            "expected AbsoluteTemplate, got {err:?}"
        );
    }

    #[test]
    fn render_unc_path_is_rejected() {
        let template = r"\\server\share\%iso.%ext";
        let err = render(template, fixed_now(), None, "png", 1).unwrap_err();
        assert!(
            matches!(&err, Error::AbsoluteTemplate { .. }),
            "expected AbsoluteTemplate, got {err:?}"
        );
    }

    #[test]
    fn render_leading_parent_segment_is_rejected() {
        let err = render("../%iso.%ext", fixed_now(), None, "png", 1).unwrap_err();
        assert!(
            matches!(&err, Error::ParentSegment { template } if template == "../%iso.%ext"),
            "expected ParentSegment, got {err:?}"
        );
    }

    #[test]
    fn render_parent_segment_inside_folder_is_rejected() {
        let err = render("shots/../secret/%iso.%ext", fixed_now(), None, "png", 1).unwrap_err();
        assert!(
            matches!(&err, Error::ParentSegment { .. }),
            "expected ParentSegment, got {err:?}"
        );
    }

    #[test]
    fn render_unknown_token_is_rejected_with_token_name() {
        let err = render("%month.%ext", fixed_now(), None, "png", 1).unwrap_err();
        assert!(
            matches!(&err, Error::UnknownToken { token, template }
                if token == "%month" && template == "%month.%ext"),
            "expected UnknownToken %month, got {err:?}"
        );
    }

    #[test]
    fn render_strftime_minute_token_is_rejected() {
        let err = render("%d%m.%ext", fixed_now(), None, "png", 1).unwrap_err();
        assert!(
            matches!(&err, Error::UnknownToken { token, .. } if token == "%m"),
            "expected UnknownToken %m, got {err:?}"
        );
    }

    #[test]
    fn render_lone_percent_is_rejected() {
        let err = render("shot%.png", fixed_now(), None, "png", 1).unwrap_err();
        assert!(
            matches!(&err, Error::UnknownToken { token, .. } if token == "%"),
            "expected UnknownToken %, got {err:?}"
        );
    }

    #[test]
    fn error_messages_quote_template_for_toast_and_log() {
        let absolute = Error::AbsoluteTemplate {
            template: "/tmp/%iso.%ext".to_owned(),
        };
        let parent = Error::ParentSegment {
            template: "../%iso.%ext".to_owned(),
        };
        let unknown = Error::UnknownToken {
            token: "%month".to_owned(),
            template: "%month.%ext".to_owned(),
        };
        assert!(absolute.to_string().contains("/tmp/%iso.%ext"));
        assert!(parent.to_string().contains("../%iso.%ext"));
        assert!(unknown.to_string().contains("%month"));
        assert!(unknown.to_string().contains("%month.%ext"));
    }

    // ---------- collision variants ----------

    #[test]
    fn collision_variant_counter_one_returns_path_unchanged() {
        let path = "FullScreen/26-11/2026-11-03_00-12-05.png";
        assert_eq!(collision_variant(path, 1), path);
    }

    #[test]
    fn collision_variant_counter_below_two_returns_path_unchanged() {
        let path = "screenshot.png";
        assert_eq!(collision_variant(path, 0), path);
    }

    #[test]
    fn collision_variant_inserts_suffix_before_extension() {
        let path = collision_variant("screenshot.png", 2);
        assert_eq!(path, "screenshot-2.png");
    }

    #[test]
    fn collision_variant_inserts_before_last_dot_of_dotted_name() {
        let path = collision_variant("a.b.png", 3);
        assert_eq!(path, "a.b-3.png");
    }

    #[test]
    fn collision_variant_without_extension_appends_suffix() {
        let path = collision_variant("FullScreen/shot", 4);
        assert_eq!(path, "FullScreen/shot-4");
    }

    #[test]
    fn collision_variant_touches_only_the_final_segment() {
        let path = collision_variant("Full.26-11/2026-11-03_00-12-05.png", 9);
        assert_eq!(path, "Full.26-11/2026-11-03_00-12-05-9.png");
    }
}
