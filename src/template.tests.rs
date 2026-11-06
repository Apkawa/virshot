use super::Error;
use super::TOKENS;
use super::collision_variant;
use super::render;
use super::token_value;
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

// ---------- token table ↔ renderer agreement ----------

/// Plausible lookalikes that must NOT be recognized (spec §4.2: only the 11
/// exact spellings; no `%m`, `%month`, `%app`, `%f`). These are probes, not a
/// copy of the table — the recognized set comes from `TOKENS` itself.
const REJECTED_TOKEN_PROBES: &[&str] = &[
    "", "m", "month", "app", "f", "E", "h", "s", "yy", "mS", "percent",
];

/// The display table is exactly what the renderer supports: every entry
/// renders through `render` to a value (never its own name), and every probed
/// name outside the table is rejected by `render` and echoed verbatim by
/// `token_value`. Both directions go through the module's own code paths, so
/// the table cannot drift away from the renderer (the UI list is derived from
/// it, so this pins the accordion too).
#[test]
fn token_table_lists_every_placeholder_the_renderer_supports() {
    let now = fixed_now();
    let title = Some("Cyberpunk 2077");

    for (name, description) in TOKENS {
        assert!(
            !description.is_empty(),
            "token \"{name}\" must carry a description for the UI reference"
        );
        let path = render(&format!("%{name}.%ext"), now, title, "png", 1).unwrap();
        assert!(
            path.ends_with(".png"),
            "token \"{name}\" must render inside a segment, got {path}"
        );
        assert_ne!(
            token_value(name, now, title, "png", 1),
            *name,
            "token \"{name}\" must expand to a value, not echo its name"
        );
    }

    for probe in REJECTED_TOKEN_PROBES {
        assert!(
            !TOKENS.iter().any(|(name, _)| name == probe),
            "probe \"{probe}\" must stay outside the table"
        );
        let template = format!("%{probe}.%ext");
        let err = render(&template, now, title, "png", 1).unwrap_err();
        assert!(
            matches!(&err, Error::UnknownToken { token, .. } if *token == format!("%{probe}")),
            "expected UnknownToken for \"{probe}\", got {err:?}"
        );
        assert_eq!(
            token_value(probe, now, title, "png", 1),
            *probe,
            "unlisted token must hit the echo fallback in token_value"
        );
    }

    // Spec §4.2 pins the exact count: `%Y %y %mo %d %H %M %S %iso %t %n %ext`.
    assert_eq!(TOKENS.len(), 11, "token table must list spec §4.2 tokens");
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
