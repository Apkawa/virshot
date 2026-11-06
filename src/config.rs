//! Configuration model: the settings struct, its spec §7 defaults (the
//! `save_path` resolved from the home directory — §7 v5.0, no hardcoded path),
//! TOML load/save (`config.toml`), the OS-convention config file location
//! (Windows portable mode: beside `virshot.exe`, falling back to `%APPDATA%`
//! and the env-less convention when the exe dir is not writable), and
//! first-run auto-creation of the default config.
//!
//! Wiring: `main.rs` calls `default_config_path` and `load_or_create` (which
//! calls `load_or_default` → `load`, and `save` on first run); `gui.rs` uses
//! `home_dir` for the Browse picker start directory, so every item is used in
//! the non-test build — no `#![allow(dead_code)]` needed.

use std::path::Path;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// ` (line N)` suffix for a parse-error message; empty when there is no span.
fn line_note(line: &Option<usize>) -> String {
    line.map(|number| format!(" (line {number})"))
        .unwrap_or_default()
}

/// Config file errors, typed per module (`thiserror` enum, no stringly `Err(String)`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Error {
    /// The config file could not be opened/read (missing file, permission
    /// denied) or written (missing parent directory, permission denied).
    /// `io::Error` is not `Clone`, so the failure is kept as an owned message.
    /// `not_found` records an OS "no such file or directory" failure: a missing
    /// config is the expected first-run state, unlike every other I/O failure.
    #[error("failed to access config file {path}: {message}")]
    Io {
        path: PathBuf,
        message: String,
        not_found: bool,
    },

    /// The file is valid UTF-8 but not TOML for [`Settings`]: a syntax error
    /// or a type/field mismatch. `line` is the 1-based line of the offending
    /// token when the `toml` crate reported a span, `None` otherwise.
    #[error("invalid TOML in config file {path}{}: {message}", line_note(line))]
    Parse {
        path: PathBuf,
        message: String,
        line: Option<usize>,
    },
}

/// 1-based line number of the byte `offset` in `input`.
fn line_of(input: &str, offset: usize) -> usize {
    input
        .bytes()
        .take(offset)
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

/// Whether `err` is the OS reporting a missing file or directory.
fn is_missing(err: &std::io::Error) -> bool {
    err.kind() == std::io::ErrorKind::NotFound
}

/// Attributes a file failure to `path`, naming the failed operation.
/// `io::Error` (and `toml::ser::Error`) are not `Clone`, so the message is
/// captured as an owned string; `not_found` carries the missing-file flag for
/// [`fallback_reason`].
fn file_error(path: &Path, action: &str, err: impl std::fmt::Display, not_found: bool) -> Error {
    Error::Io {
        path: path.to_path_buf(),
        message: format!("{action}: {err}"),
        not_found,
    }
}

/// Attributes a TOML failure to `path`; the line is derived from the crate's
/// span against the raw input (no span means no line).
fn parse_error(path: &Path, input: &str, err: toml::de::Error) -> Error {
    Error::Parse {
        path: path.to_path_buf(),
        message: err.message().to_owned(),
        line: err.span().map(|span| line_of(input, span.start)),
    }
}

/// Reads `path` and deserializes it into [`Settings`].
///
/// Keys absent from the file fall back to the spec §7 defaults (`#[serde(default)]`).
/// Fails with `Error::Io` when the file cannot be opened or read (including a
/// missing file), and with `Error::Parse` when the contents are not TOML for
/// [`Settings`].
pub(crate) fn load(path: &Path) -> Result<Settings, Error> {
    let input = std::fs::read_to_string(path)
        .map_err(|err| file_error(path, "read", &err, is_missing(&err)))?;
    toml::from_str::<Settings>(&input).map_err(|err| parse_error(path, &input, err))
}

/// Why [`load_or_default`] logs `err` before falling back to defaults:
/// `Some("parse")` for an unparsable config, `Some("io")` for an I/O failure
/// that is not a missing file, `None` when the config file is simply absent —
/// defaults are the expected outcome there, so nothing is logged. Pure logic:
/// the decision is unit-testable without a tracing subscriber.
fn fallback_reason(err: &Error) -> Option<&str> {
    match err {
        Error::Parse { .. } => Some("parse"),
        Error::Io { not_found, .. } if *not_found => None,
        Error::Io { .. } => Some("io"),
    }
}

/// Load `path`, or fall back to the spec §7 [`Settings`] defaults when the
/// file cannot be used. A parse failure or a non-missing I/O failure is logged
/// with the full typed message (including the line for a parse error); a
/// missing file is the normal first-run state and is not logged. Never fails,
/// so the app loop can call it and keep running.
pub(crate) fn load_or_default(path: &Path) -> Settings {
    match load(path) {
        Ok(settings) => settings,
        Err(err) => {
            if fallback_reason(&err).is_some() {
                tracing::error!("config: {err}");
            }
            Settings::default()
        }
    }
}

/// Load like [`load_or_default`] and, when the config file is absent, write
/// the spec §7 defaults to `path` (spec §7: first run creates the default
/// config; success logged at INFO).
///
/// A corrupt or unreadable config is deliberately *not* rewritten:
/// [`load_or_default`] logged it as ERROR and returned defaults, and
/// overwriting would silently destroy a file the user can still fix. If the
/// creation itself fails (e.g. missing parent dir cannot be created,
/// permission denied), the failure is logged at WARN and defaults are still
/// returned — startup must never fail on config.
pub(crate) fn load_or_create(path: &Path) -> Settings {
    let settings = load_or_default(path);
    if !path.exists() {
        create_default_file(path);
    }
    settings
}

/// Write the spec §7 defaults to `path`, creating missing parent directories
/// first. Failures are logged (WARN) instead of returned: callers keep
/// running on the defaults they already have.
fn create_default_file(path: &Path) {
    if let Some(parent) = path.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        tracing::warn!("config: create dir {}: {err}", parent.display());
        return;
    }
    match save(&Settings::default(), path) {
        Ok(()) => tracing::info!("config: created default config at {}", path.display()),
        Err(err) => tracing::warn!("config: {err}"),
    }
}

/// Serializes `settings` as pretty TOML and writes `path` (create/truncate).
///
/// Fails with `Error::Io` when the settings cannot be serialized or the file
/// cannot be written (including a missing parent directory).
pub(crate) fn save(settings: &Settings, path: &Path) -> Result<(), Error> {
    let text = toml::to_string_pretty(settings)
        .map_err(|err| file_error(path, "serialize", &err, false))?;
    std::fs::write(path, text).map_err(|err| file_error(path, "write", &err, is_missing(&err)))
}

/// Config file name, placed inside the OS-convention config directory.
const CONFIG_FILE_NAME: &str = "config.toml";

/// Directory used when the environment provides none: on Windows a cwd-relative
/// path, elsewhere a cwd-relative `.config` (matching the `$HOME/.config` shape).
#[cfg(windows)]
const FALLBACK_CONFIG_DIR: &str = ".";

/// Non-Windows counterpart of [`FALLBACK_CONFIG_DIR`].
#[cfg(not(windows))]
const FALLBACK_CONFIG_DIR: &str = ".config";

/// Config path from the environment values, in convention order: `appdata`,
/// then `xdg` (`$XDG_CONFIG_HOME`), then `home/.config` (`$HOME`), then
/// [`FALLBACK_CONFIG_DIR`] when all are absent. This is the **unix** default
/// convention (Windows uses [`windows_config_path`] — portable mode). Pure
/// logic over passed-in values (no `env` access, no cfg branch) so the same
/// rules are unit-tested on linux and windows-gnu — manual env handling,
/// deliberately without the `dirs` crate.
#[cfg(any(not(windows), test))]
fn config_path_from_env(appdata: Option<&str>, xdg: Option<&str>, home: Option<&str>) -> PathBuf {
    let dir = appdata
        .map(PathBuf::from)
        .or_else(|| xdg.map(PathBuf::from))
        .or_else(|| home.map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from(FALLBACK_CONFIG_DIR));
    dir.join("virshot").join(CONFIG_FILE_NAME)
}

/// Probe file name used by [`probe_dir_writable`]: created inside a candidate
/// dir and removed right after the probe (best-effort cleanup), so startup
/// checks leave no residue.
#[cfg(any(windows, test))]
const PROBE_FILE_NAME: &str = "virshot.probe.tmp";

/// Windows portable-mode config dirs (spec §7 v5.0), in candidate order: the
/// directory of `virshot.exe` (config directly beside the exe), then
/// `<appdata>/virshot` (`%APPDATA%`), then the env-less
/// [`FALLBACK_CONFIG_DIR`]/`virshot` convention. The vec always ends with the
/// fallback, so it is never empty. Pure — no env or filesystem access — so the
/// order is unit-tested on every target.
#[cfg(any(windows, test))]
fn windows_config_candidates(exe_dir: Option<&Path>, appdata_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(exe_dir) = exe_dir {
        candidates.push(exe_dir.to_path_buf());
    }
    if let Some(appdata_dir) = appdata_dir {
        candidates.push(appdata_dir.join("virshot"));
    }
    candidates.push(PathBuf::from(FALLBACK_CONFIG_DIR).join("virshot"));
    candidates
}

/// Whether `dir` is usable (writable): `Ok` from the injected `probe` means
/// usable; any probe failure (`PermissionDenied`, a missing dir) means
/// unusable, so the caller moves to the next candidate.
#[cfg(any(windows, test))]
fn dir_writable(dir: &Path, probe: impl Fn(&Path) -> std::io::Result<()>) -> bool {
    probe(dir).is_ok()
}

/// Startup writability probe: ensure `dir` exists (`create_dir_all` — the same
/// step config auto-creation needs anyway), then exclusively create
/// `virshot.probe.tmp` inside it and remove the file afterwards (cleanup is
/// attempted even when the creation itself failed). Errors propagate so
/// [`dir_writable`] can reject the candidate.
#[cfg(any(windows, test))]
fn probe_dir_writable(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe_path = dir.join(PROBE_FILE_NAME);
    let opened = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe_path);
    // Attempt cleanup regardless of the outcome: a probe file must never stay.
    let _ = std::fs::remove_file(&probe_path);
    opened.map(drop)
}

/// Portable-mode config path: the first candidate dir that passes `probe`,
/// joined with [`CONFIG_FILE_NAME`]; if none is writable, the last (fallback)
/// candidate. When an exe dir was available but rejected, the fallback choice
/// is logged at WARN. Pure over the injected dirs and probe, so the selection
/// rules are unit-tested on linux too.
#[cfg(any(windows, test))]
fn windows_config_path(
    exe_dir: Option<&Path>,
    appdata_dir: Option<&Path>,
    probe: impl Fn(&Path) -> std::io::Result<()>,
) -> PathBuf {
    let candidates = windows_config_candidates(exe_dir, appdata_dir);
    for dir in &candidates {
        if dir_writable(dir, &probe) {
            // A candidate other than the exe dir means the portable dir was rejected.
            if dir != exe_dir.unwrap_or(dir) {
                tracing::warn!(
                    "config dir beside exe not writable, using {}",
                    dir.display()
                );
            }
            return dir.join(CONFIG_FILE_NAME);
        }
    }
    let fallback = candidates
        .last()
        .cloned()
        .unwrap_or_else(|| PathBuf::from(FALLBACK_CONFIG_DIR).join("virshot"));
    if exe_dir.is_some() {
        tracing::warn!(
            "config dir beside exe not writable, using {}",
            fallback.display()
        );
    }
    fallback.join(CONFIG_FILE_NAME)
}

/// Environment variable naming the user home directory on this OS: Windows
/// `%USERPROFILE%`, unix `$HOME` (spec §7 v5.0).
#[cfg(windows)]
const HOME_ENV_VAR: &str = "USERPROFILE";

/// Non-Windows counterpart of [`HOME_ENV_VAR`].
#[cfg(not(windows))]
const HOME_ENV_VAR: &str = "HOME";

/// Home directory for `var` from an injected env lookup (`env(name)` returns
/// the value, or `None` when unset/non-UTF-8). Pure: the OS variable name is
/// the only cfg-dependent part, so the lookup rules are unit-tested on linux
/// and windows-gnu alike — the same manual-env approach as
/// [`config_path_from_env`], deliberately without the `dirs` crate.
fn home_from_var(var: &str, env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    env(var).map(PathBuf::from)
}

/// Home directory of this OS convention from the injected env lookup.
pub(crate) fn resolve_home(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    home_from_var(HOME_ENV_VAR, env)
}

/// Home directory from the process environment; a non-UTF-8 value counts as
/// absent, like an unset one.
pub(crate) fn home_dir() -> Option<PathBuf> {
    resolve_home(|name| std::env::var(name).ok())
}

/// Spec §7 default save path: `<home>/Pictures/ViRShot` (Windows
/// `%USERPROFILE%\Pictures\ViRShot`, Linux `~/Pictures/ViRShot`).
/// With no home directory the path stays cwd-relative
/// (`Pictures/ViRShot`), like [`FALLBACK_CONFIG_DIR`] does for the config
/// file: startup still has a writable place rather than a fake absolute path,
/// and `capture` creates the directory on the first save.
pub(crate) fn default_save_path(home: Option<&Path>) -> PathBuf {
    match home {
        Some(home) => home.join("Pictures").join("ViRShot"),
        None => PathBuf::from(FALLBACK_SAVE_DIR).join("ViRShot"),
    }
}

/// Directory used when the environment provides no home: cwd-relative.
const FALLBACK_SAVE_DIR: &str = "Pictures";

/// Default config location. Windows portable mode (spec §7 v5.0):
/// `<exe dir>/config.toml` beside `virshot.exe`; if that dir is not writable,
/// `%APPDATA%\virshot\config.toml`; if `%APPDATA%` is absent too, the env-less
/// [`FALLBACK_CONFIG_DIR`] convention — rejected exe dir is logged at WARN
/// ([`windows_config_path`]). Linux: the unchanged env convention —
/// `%APPDATA%` else `$XDG_CONFIG_HOME` else `$HOME/.config` else
/// [`FALLBACK_CONFIG_DIR`] ([`config_path_from_env`]). Reads the process
/// environment directly; a non-UTF-8 value counts as absent, like an unset
/// one.
pub(crate) fn default_config_path() -> PathBuf {
    #[cfg(windows)]
    {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf));
        let appdata = std::env::var("APPDATA").ok().map(PathBuf::from);
        windows_config_path(exe_dir.as_deref(), appdata.as_deref(), probe_dir_writable)
    }
    #[cfg(not(windows))]
    {
        config_path_from_env(
            std::env::var("APPDATA").ok().as_deref(),
            std::env::var("XDG_CONFIG_HOME").ok().as_deref(),
            std::env::var("HOME").ok().as_deref(),
        )
    }
}

/// Image format for saved screenshots; serialized as `"png"`, `"jpg"`, `"webp"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) enum Format {
    #[default]
    #[serde(rename = "png")]
    Png,
    #[serde(rename = "jpg")]
    Jpeg,
    #[serde(rename = "webp")]
    Webp,
}

impl Format {
    /// File extension for the format (`png`/`jpg`/`webp`), consumed by the
    /// `capture` module as the template `%ext` value.
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
            Format::Webp => "webp",
        }
    }
}

/// Overlay screen corner; serialized with the variant name verbatim
/// (`"TopLeft"`, `"TopRight"`, `"BottomLeft"`, `"BottomRight"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) enum OverlayPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

/// Output directory, filename templates, image format, and encoder qualities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct General {
    pub(crate) save_path: String,
    pub(crate) file_full_screen: String,
    pub(crate) file_window: String,
    pub(crate) format: Format,
    pub(crate) jpeg_quality: u8,
    pub(crate) webp_quality: u8,
}

impl Default for General {
    /// Spec §7 defaults with the `save_path` resolved from the real home
    /// directory ([`home_dir`] + [`default_save_path`]) — resolved per call,
    /// i.e. at startup and whenever a partial config fills the missing keys.
    fn default() -> Self {
        general_with_home(home_dir().as_deref())
    }
}

/// `General` defaults for an injected home directory: pure, so the spec §7
/// defaults are unit-tested without touching the process environment.
fn general_with_home(home: Option<&Path>) -> General {
    General {
        save_path: default_save_path(home).display().to_string(),
        file_full_screen: "FullScreen/%y-%mo/%iso.%ext".to_owned(),
        file_window: "%t/%y-%mo/%t-%iso.%ext".to_owned(),
        format: Format::Png,
        jpeg_quality: 90,
        webp_quality: 90,
    }
}

/// One capture trigger: keyboard hotkey plus optional gamepad button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Binding {
    pub(crate) key: String,
    pub(crate) mods: Vec<String>,
    /// Gamepad trigger, pad notation (parsed by `bindings::parse_pad`,
    /// case-insensitive): a canonical GameInput button name — the device-info
    /// label the capture UI writes, e.g. `"XboxX"`, `"XboxLeftTrigger"`,
    /// `"XboxDPadUp"`, `"Select"` (the full table mirrors
    /// `gamepad::button_name`; see `bindings::GAMEPAD_CANONICAL_NAMES`); its
    /// controller button index resolves from device-info at runtime, so the
    /// same name may be a different button on different pads — §9. Legacy
    /// gilrs-style names (`"South"`, `"North"`, `"LeftShoulder"`, …) pin fixed
    /// indices for pre-Stage-5 configs. `unknown:<N>` for a raw controller
    /// button index (`N` decimal 0..=63, the capture fallback on an unlabeled
    /// device), or a system button: `"Guide"`, `"Share"`. `None` means no
    /// gamepad trigger; any other value is a config error
    /// (`bindings::Error::UnknownPad`).
    pub(crate) pad: Option<String>,
}

impl Default for Binding {
    fn default() -> Self {
        Self {
            key: "PrintScreen".to_owned(),
            mods: Vec::new(),
            pad: None,
        }
    }
}

/// Capture bindings: full-screen, window (`Alt`), rect (`Ctrl`, reserved and
/// inactive in MVP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Controls {
    pub(crate) full_screen: Binding,
    pub(crate) window: Binding,
    pub(crate) rect: Binding,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            full_screen: Binding::default(),
            window: Binding {
                key: "PrintScreen".to_owned(),
                mods: vec!["Alt".to_owned()],
                pad: None,
            },
            rect: Binding {
                key: "PrintScreen".to_owned(),
                mods: vec!["Ctrl".to_owned()],
                pad: None,
            },
        }
    }
}

/// Transient capture-feedback overlay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Overlay {
    pub(crate) enabled: bool,
    pub(crate) position: OverlayPosition,
    pub(crate) duration_ms: u64,
    pub(crate) on_all_monitors: bool,
}

impl Default for Overlay {
    fn default() -> Self {
        Self {
            enabled: true,
            position: OverlayPosition::BottomRight,
            duration_ms: 2000,
            on_all_monitors: true,
        }
    }
}

/// Full settings model matching spec §7; struct-level `#[serde(default)]`
/// makes partial TOML configs fall back to the spec defaults per section.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub(crate) general: General,
    pub(crate) controls: Controls,
    pub(crate) overlay: Overlay,
}

#[cfg(test)]
#[path = "config.tests.rs"]
mod tests;
