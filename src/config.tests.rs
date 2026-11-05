use super::*;

#[test]
fn settings_defaults_match_spec_section_7() {
    let settings = Settings::default();

    // `save_path` is environment-resolved (spec §7 v5.0), asserted in
    // `settings_default_save_path_matches_the_home_resolved_value`.
    assert_eq!(
        settings.general.file_full_screen,
        "FullScreen/%y-%mo/%iso.%ext"
    );
    assert_eq!(settings.general.file_window, "%t/%y-%mo/%t-%iso.%ext");
    assert_eq!(settings.general.format, Format::Png);
    assert_eq!(settings.general.jpeg_quality, 90);
    assert_eq!(settings.general.webp_quality, 90);

    assert_eq!(settings.controls.full_screen.key, "PrintScreen");
    assert_eq!(settings.controls.full_screen.mods, Vec::<String>::new());
    assert_eq!(settings.controls.full_screen.pad, None);
    assert_eq!(settings.controls.window.key, "PrintScreen");
    assert_eq!(settings.controls.window.mods, vec!["Alt".to_owned()]);
    assert_eq!(settings.controls.window.pad, None);
    assert_eq!(settings.controls.rect.key, "PrintScreen");
    assert_eq!(settings.controls.rect.mods, vec!["Ctrl".to_owned()]);
    assert_eq!(settings.controls.rect.pad, None);

    assert!(settings.overlay.enabled);
    assert_eq!(settings.overlay.position, OverlayPosition::BottomRight);
    assert_eq!(settings.overlay.duration_ms, 2000);
    assert!(settings.overlay.on_all_monitors);
}

#[test]
fn overlay_position_default_is_bottom_right() {
    assert_eq!(OverlayPosition::default(), OverlayPosition::BottomRight);
}

#[test]
fn format_default_is_png() {
    assert_eq!(Format::default(), Format::Png);
}

#[test]
fn format_extension_png_returns_png() {
    assert_eq!(Format::Png.extension(), "png");
}

#[test]
fn format_extension_jpeg_returns_jpg() {
    assert_eq!(Format::Jpeg.extension(), "jpg");
}

#[test]
fn format_extension_webp_returns_webp() {
    assert_eq!(Format::Webp.extension(), "webp");
}

/// Path as a `/`-separated string. `PathBuf::join` inserts the host
/// separator, so a Windows-style fixture must be compared after
/// normalisation to assert the same value on linux and windows-gnu.
fn normalised(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

#[test]
fn config_path_from_env_windows_appdata_returns_appdata_virshot_config() {
    let path = config_path_from_env(Some("C:\\Users\\u\\AppData\\Roaming"), None, None);

    assert_eq!(
        normalised(&path),
        "C:/Users/u/AppData/Roaming/virshot/config.toml"
    );
}

#[test]
fn config_path_from_env_xdg_config_home_preferred() {
    let path = config_path_from_env(None, Some("/home/u/.config-custom"), Some("/home/u"));

    assert_eq!(
        normalised(&path),
        "/home/u/.config-custom/virshot/config.toml"
    );
}

#[test]
fn config_path_from_env_falls_back_to_home_dot_config() {
    let path = config_path_from_env(None, None, Some("/home/u"));

    assert_eq!(normalised(&path), "/home/u/.config/virshot/config.toml");
}

#[test]
fn config_path_from_env_no_env_values_returns_relative_config_dir() {
    let path = config_path_from_env(None, None, None);

    assert!(path.is_relative());
    assert!(
        normalised(&path).ends_with("virshot/config.toml"),
        "fallback must still be virshot/config.toml: {path:?}"
    );
}

#[test]
fn default_config_path_real_env_ends_with_virshot_config() {
    let path = default_config_path();

    // Windows portable mode puts `config.toml` directly beside the exe, so
    // only the file-name suffix is asserted there; unix keeps the full
    // `<config dir>/virshot/config.toml` convention.
    let suffix = if cfg!(windows) {
        "config.toml"
    } else {
        "virshot/config.toml"
    };
    assert!(
        normalised(&path).ends_with(suffix),
        "config path must end with {suffix}: {path:?}"
    );
}

#[cfg(not(windows))]
#[test]
fn default_config_path_unix_keeps_env_driven_xdg_convention() {
    // Stage 15 must not change the unix default: it stays exactly the
    // `config_path_from_env` result for the real process environment.
    let path = default_config_path();
    let expected = config_path_from_env(
        std::env::var("APPDATA").ok().as_deref(),
        std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    );

    assert_eq!(
        path, expected,
        "unix default config path must stay env-driven"
    );
}

/// Portable-mode candidate dirs, `/`-normalised for cross-target asserts.
fn normalised_dirs(dirs: &[PathBuf]) -> Vec<String> {
    dirs.iter().map(|dir| normalised(dir)).collect()
}

#[test]
fn windows_config_candidates_exe_dir_first() {
    let candidates = windows_config_candidates(
        Some(Path::new("C:\\Program Files\\virshot")),
        Some(Path::new("C:\\Users\\u\\AppData\\Roaming")),
    );

    assert_eq!(candidates.len(), 3);
    assert_eq!(normalised_dirs(&candidates)[0], "C:/Program Files/virshot");
    assert_eq!(
        normalised_dirs(&candidates)[1],
        "C:/Users/u/AppData/Roaming/virshot"
    );
    // The last candidate is always the env-less fallback convention.
    assert!(candidates[2].is_relative());
    assert!(normalised(&candidates[2]).ends_with("virshot"));
}

#[test]
fn windows_config_candidates_appdata_when_no_exe_dir() {
    let candidates =
        windows_config_candidates(None, Some(Path::new("C:\\Users\\u\\AppData\\Roaming")));

    assert_eq!(candidates.len(), 2);
    assert_eq!(
        normalised_dirs(&candidates)[0],
        "C:/Users/u/AppData/Roaming/virshot"
    );
    assert!(candidates[1].is_relative());
    assert!(normalised(&candidates[1]).ends_with("virshot"));
}

#[test]
fn windows_config_candidates_last_fallback_when_no_env() {
    let candidates = windows_config_candidates(None, None);

    assert_eq!(candidates.len(), 1);
    assert!(candidates[0].is_relative());
    assert!(normalised(&candidates[0]).ends_with("virshot"));
}

#[test]
fn windows_config_path_writable_exe_dir_selects_exe_dir() {
    let path = windows_config_path(
        Some(Path::new("C:\\Program Files\\virshot")),
        Some(Path::new("C:\\Users\\u\\AppData\\Roaming")),
        |_dir| Ok(()),
    );

    assert_eq!(normalised(&path), "C:/Program Files/virshot/config.toml");
}

#[test]
fn windows_config_path_unwritable_exe_dir_selects_appdata() {
    let exe_dir = Path::new("C:\\Program Files\\virshot");
    let probe = |dir: &Path| {
        if dir == exe_dir {
            Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
        } else {
            Ok(())
        }
    };

    let path = windows_config_path(
        Some(exe_dir),
        Some(Path::new("C:\\Users\\u\\AppData\\Roaming")),
        probe,
    );

    assert_eq!(
        normalised(&path),
        "C:/Users/u/AppData/Roaming/virshot/config.toml"
    );
}

#[test]
fn windows_config_path_all_unwritable_selects_fallback() {
    let probe = |_dir: &Path| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));

    let path = windows_config_path(
        Some(Path::new("C:\\Program Files\\virshot")),
        Some(Path::new("C:\\Users\\u\\AppData\\Roaming")),
        probe,
    );

    assert!(
        path.is_relative(),
        "all-unwritable must use the fallback dir"
    );
    assert!(normalised(&path).ends_with("virshot/config.toml"));
}

#[test]
fn dir_writable_probe_success() {
    assert!(dir_writable(Path::new("any/dir"), |_dir| Ok(())));
}

#[test]
fn dir_writable_probe_permission_denied_falls_back() {
    let probe = |_dir: &Path| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));

    assert!(!dir_writable(Path::new("any/dir"), probe));
}

#[test]
fn dir_writable_probe_removes_probe_file() {
    // Real probe against a fresh scratch dir: usable, and the probe file
    // must not survive the check (the scratch dir itself is Drop-cleaned).
    let dir = TempDir::new("probe");

    assert!(dir_writable(&dir.path, probe_dir_writable));
    assert!(
        !dir.config_path(PROBE_FILE_NAME).exists(),
        "the probe file must be removed after a successful probe"
    );
}

#[test]
fn resolve_home_windows_userprofile_env_returns_userprofile() {
    // The Windows convention variable, exercised on every target through
    // the pure `home_from_var` rule (same approach as config_path_from_env).
    let home = home_from_var("USERPROFILE", |name| {
        (name == "USERPROFILE").then(|| "C:\\Users\\u".to_owned())
    });

    assert_eq!(home, Some(PathBuf::from("C:\\Users\\u")));
}

#[test]
fn resolve_home_unix_home_env_returns_home() {
    let home = home_from_var("HOME", |name| {
        (name == "HOME").then(|| "/home/u".to_owned())
    });

    assert_eq!(home, Some(PathBuf::from("/home/u")));
}

#[test]
fn resolve_home_windows_userprofile_env_is_the_gated_variable() {
    // On Windows `resolve_home` must ask `USERPROFILE` and ignore `HOME`;
    // elsewhere it must ask `HOME` and ignore `USERPROFILE`.
    let env = |name: &str| {
        if cfg!(windows) {
            (name == "HOME").then(|| "/home/u".to_owned())
        } else {
            (name == "USERPROFILE").then(|| "C:\\Users\\u".to_owned())
        }
    };

    assert_eq!(resolve_home(env), None);
}

#[test]
fn resolve_home_missing_returns_none() {
    assert_eq!(resolve_home(|_name| None), None);
}

#[test]
fn default_save_path_appends_pictures_virshot() {
    let path = default_save_path(Some(Path::new("/home/u")));

    assert_eq!(normalised(&path), "/home/u/Pictures/ViRShot");
}

#[test]
fn default_save_path_falls_back_without_home() {
    let path = default_save_path(None);

    assert!(path.is_relative(), "no home must give a cwd-relative path");
    assert_eq!(normalised(&path), "Pictures/ViRShot");
}

#[test]
fn default_config_save_path_is_home_resolved() {
    let settings = Settings {
        general: general_with_home(Some(Path::new("/home/u"))),
        ..Settings::default()
    };

    assert_eq!(
        normalised(Path::new(&settings.general.save_path)),
        "/home/u/Pictures/ViRShot"
    );
    // Everything else still follows spec §7.
    assert_eq!(settings.general.format, Format::Png);
    assert_eq!(settings.general.jpeg_quality, 90);
    assert_eq!(settings.general.webp_quality, 90);
    assert_eq!(settings.controls, Controls::default());
    assert_eq!(settings.overlay, Overlay::default());
}

#[test]
fn settings_default_save_path_matches_the_home_resolved_value() {
    // `Settings::default()` (and so a partial config's missing keys and the
    // auto-created first-run file) must equal the helper result for the
    // same environment.
    let settings = Settings::default();

    assert_eq!(
        settings.general.save_path,
        default_save_path(home_dir().as_deref())
            .display()
            .to_string()
    );
    assert!(
        normalised(Path::new(&settings.general.save_path)).ends_with("/Pictures/ViRShot"),
        "default save path must end with Pictures/ViRShot: {}",
        settings.general.save_path
    );
}

/// Scratch directory under `./.tmp/` (repo root, git-ignored), unique per
/// test and removed when the test ends. A relative path keeps the helper
/// identical on linux and windows-gnu (no tempdir dependency, no home dir).
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Fresh unique directory for `tag`; `./` is the package root, where
    /// cargo runs unit tests on every target.
    fn new(tag: &str) -> Self {
        let path = PathBuf::from("./.tmp").join(format!("config-test-{tag}"));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch dir must be creatable");
        Self { path }
    }

    fn config_path(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Spec §7 draft schema, verbatim values. `pad` is omitted where the spec
/// says `null`: TOML 1.1 has no null literal, and the serde default is
/// already `None`.
const SPEC_TOML: &str = r#"
[general]
save_path = "C:\\Users\\User\\Pictures\\Screenshots"
file_full_screen = "FullScreen/%y-%mo/%y-%mo-%d/%iso.%ext"
file_window = "%t/%y-%mo/%y-%mo-%d/%t-%iso.%ext"
format = "png"
jpeg_quality = 90
webp_quality = 90

[controls.full_screen]
key = "PrintScreen"
mods = []

[controls.window]
key = "PrintScreen"
mods = ["Alt"]
pad = "Share"

[controls.rect]
key = "PrintScreen"
mods = ["Ctrl"]

[overlay]
enabled = true
position = "BottomRight"
duration_ms = 2000
on_all_monitors = true
"#;

#[test]
fn load_valid_toml_returns_settings() {
    let dir = TempDir::new("valid");
    let path = dir.config_path("config.toml");
    std::fs::write(&path, SPEC_TOML).expect("fixture must be writable");

    let settings = load(&path).expect("spec §7 config must load");

    assert_eq!(
        settings.general.save_path,
        "C:\\Users\\User\\Pictures\\Screenshots"
    );
    assert_eq!(
        settings.general.file_full_screen,
        "FullScreen/%y-%mo/%y-%mo-%d/%iso.%ext"
    );
    assert_eq!(
        settings.general.file_window,
        "%t/%y-%mo/%y-%mo-%d/%t-%iso.%ext"
    );
    assert_eq!(settings.general.format, Format::Png);
    assert_eq!(settings.general.jpeg_quality, 90);
    assert_eq!(settings.general.webp_quality, 90);

    assert_eq!(settings.controls.full_screen.mods, Vec::<String>::new());
    assert_eq!(settings.controls.full_screen.pad, None);
    assert_eq!(settings.controls.window.mods, vec!["Alt".to_owned()]);
    assert_eq!(settings.controls.window.pad, Some("Share".to_owned()));
    assert_eq!(settings.controls.rect.mods, vec!["Ctrl".to_owned()]);
    assert_eq!(settings.controls.rect.pad, None);

    assert!(settings.overlay.enabled);
    assert_eq!(settings.overlay.position, OverlayPosition::BottomRight);
    assert_eq!(settings.overlay.duration_ms, 2000);
    assert!(settings.overlay.on_all_monitors);
}

#[test]
fn load_partial_toml_fills_defaults() {
    let dir = TempDir::new("partial");
    let path = dir.config_path("config.toml");
    std::fs::write(&path, "[overlay]\nenabled = false\n").expect("fixture must be writable");

    let settings = load(&path).expect("partial config must load");

    assert!(!settings.overlay.enabled);
    assert_eq!(settings.overlay.position, OverlayPosition::BottomRight);
    assert_eq!(settings.overlay.duration_ms, 2000);
    assert!(settings.overlay.on_all_monitors);
    assert_eq!(settings.general, General::default());
    assert_eq!(settings.controls, Controls::default());
}

#[test]
fn load_missing_file_returns_io_error() {
    let dir = TempDir::new("missing");
    let path = dir.config_path("absent.toml");

    let err = load(&path).expect_err("missing file must fail");

    assert!(matches!(err, Error::Io { .. }));
    assert!(err.to_string().contains(&path.display().to_string()));
}

#[test]
fn load_invalid_toml_returns_parse_error_with_line() {
    let dir = TempDir::new("invalid");
    let path = dir.config_path("config.toml");
    std::fs::write(&path, "[general]\nsave_path = \n").expect("fixture must be writable");

    let err = load(&path).expect_err("broken TOML must fail");

    let Error::Parse { line, .. } = err else {
        panic!("expected Error::Parse, got {err:?}");
    };
    assert_eq!(line, Some(2));
    assert!(err.to_string().contains(&path.display().to_string()));
}

#[test]
fn load_missing_file_io_error_marks_not_found() {
    let dir = TempDir::new("not-found");
    let path = dir.config_path("absent.toml");

    let err = load(&path).expect_err("missing file must fail");

    let Error::Io { not_found, .. } = err else {
        panic!("expected Error::Io, got {err:?}");
    };
    assert!(not_found, "a missing config file must be flagged not_found");
}

#[test]
fn fallback_reason_parse_error_returns_parse() {
    let err = Error::Parse {
        path: PathBuf::from("config.toml"),
        message: "expected string".to_owned(),
        line: Some(2),
    };

    assert_eq!(fallback_reason(&err), Some("parse"));
}

#[test]
fn fallback_reason_missing_file_returns_none() {
    let err = Error::Io {
        path: PathBuf::from("config.toml"),
        message: "read: No such file or directory (os error 2)".to_owned(),
        not_found: true,
    };

    assert_eq!(fallback_reason(&err), None);
}

#[test]
fn fallback_reason_other_io_error_returns_io() {
    let err = Error::Io {
        path: PathBuf::from("config.toml"),
        message: "read: Permission denied (os error 13)".to_owned(),
        not_found: false,
    };

    assert_eq!(fallback_reason(&err), Some("io"));
}

#[test]
fn load_or_default_corrupt_toml_returns_defaults() {
    let dir = TempDir::new("corrupt");
    let path = dir.config_path("config.toml");
    std::fs::write(&path, "[general]\nsave_path = \n").expect("fixture must be writable");

    let settings = load_or_default(&path);

    assert_eq!(settings, Settings::default());
}

#[test]
fn load_or_default_missing_file_returns_defaults() {
    let dir = TempDir::new("absent");
    let path = dir.config_path("absent.toml");

    let settings = load_or_default(&path);

    assert_eq!(settings, Settings::default());
}

#[test]
fn load_or_default_valid_toml_returns_parsed_settings() {
    let dir = TempDir::new("fallback-valid");
    let path = dir.config_path("config.toml");
    std::fs::write(&path, SPEC_TOML).expect("fixture must be writable");

    let settings = load_or_default(&path);

    assert_eq!(settings, load(&path).expect("spec §7 config must load"));
    assert_eq!(settings.controls.window.pad, Some("Share".to_owned()));
    assert_eq!(settings.general.webp_quality, 90);
}

#[test]
fn load_or_create_missing_file_creates_file_with_defaults() {
    let dir = TempDir::new("auto-create");
    let path = dir.config_path("config.toml");

    let settings = load_or_create(&path);

    assert_eq!(settings, Settings::default());
    assert!(path.exists(), "first run must create the config file");
    let on_disk = load(&path).expect("created config must load back");
    assert_eq!(on_disk, Settings::default());
}

#[test]
fn load_or_create_corrupt_file_returns_defaults_without_rewrite() {
    let dir = TempDir::new("auto-create-corrupt");
    let path = dir.config_path("config.toml");
    let broken = "[general]\nsave_path = \n";
    std::fs::write(&path, broken).expect("fixture must be writable");

    let settings = load_or_create(&path);

    assert_eq!(settings, Settings::default());
    let on_disk = std::fs::read_to_string(&path).expect("broken config must stay readable");
    assert_eq!(
        on_disk, broken,
        "a corrupt config must not be silently overwritten"
    );
}

#[test]
fn load_or_create_valid_file_returns_parsed_no_rewrite() {
    let dir = TempDir::new("auto-create-valid");
    let path = dir.config_path("config.toml");
    std::fs::write(&path, SPEC_TOML).expect("fixture must be writable");

    let settings = load_or_create(&path);

    assert_eq!(settings, load(&path).expect("spec §7 config must load"));
    let on_disk = std::fs::read_to_string(&path).expect("config must stay readable");
    assert_eq!(
        on_disk, SPEC_TOML,
        "an existing config must not be rewritten"
    );
}

#[test]
fn load_or_create_parent_dir_created() {
    let dir = TempDir::new("auto-create-nested");
    let path = dir.config_path("virshot/config.toml");

    let settings = load_or_create(&path);

    assert_eq!(settings, Settings::default());
    assert!(
        path.exists(),
        "missing parent dirs must be created before save"
    );
    let on_disk = load(&path).expect("created config must load back");
    assert_eq!(on_disk, Settings::default());
}

#[test]
fn save_then_load_round_trip_preserves_settings() {
    let dir = TempDir::new("round-trip");
    let path = dir.config_path("config.toml");
    let settings = Settings {
        general: General {
            save_path: "D:\\shots".to_owned(),
            file_full_screen: "%iso.%ext".to_owned(),
            file_window: "%t-%iso.%ext".to_owned(),
            format: Format::Webp,
            jpeg_quality: 42,
            webp_quality: 77,
        },
        controls: Controls {
            full_screen: Binding::default(),
            window: Binding {
                key: "F2".to_owned(),
                mods: vec!["Alt".to_owned(), "Shift".to_owned()],
                pad: Some("Share".to_owned()),
            },
            rect: Binding::default(),
        },
        overlay: Overlay {
            enabled: false,
            position: OverlayPosition::TopLeft,
            duration_ms: 1500,
            on_all_monitors: false,
        },
    };

    save(&settings, &path).expect("save must succeed");
    let loaded = load(&path).expect("saved config must load back");

    assert_eq!(loaded, settings);
}

#[test]
fn save_creates_file_with_toml_content() {
    let dir = TempDir::new("content");
    let path = dir.config_path("config.toml");

    save(&Settings::default(), &path).expect("save must succeed");

    let text = std::fs::read_to_string(&path).expect("saved file must be readable");
    assert!(text.contains("[general]"));
    assert!(text.contains("file_full_screen"));
}
