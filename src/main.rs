//! virshot — MVP wiring stage. `main.rs` only wires things together: CLI
//! parsing (spec §8) and startup wiring; business rules stay in the modules.
//! `capture` holds the capture/save pipeline (foreground target, png/jpg/webp
//! encoding, collision-free template paths): the CLI runs `run_capture`
//! synchronously for headless `shot`, the hotkey wiring (Stage 6) will use
//! `spawn_capture` (save on a separate thread — spec §4.2); `config` holds the
//! settings schema, the OS-convention config file path, and first-run
//! auto-creation of the default config; `logging` holds level parsing,
//! log-file placement, and global subscriber init; `template` holds
//! filename-template rendering, sanitization, validation, and collision
//! variants (pure logic — consumed by `capture`); `bindings` turns the config
//! `controls` into typed capture actions with crate hotkey combos plus the
//! debounce gate (pure logic); `hotkeys` owns the `global-hotkey` manager, the
//! registration status report, and the input-thread event loop that dispatches
//! debounced triggers into `spawn_capture` (spec §6 Input Thread). Gamepad
//! input (Stage 4, docs/spec.md §4.1 pad field): `gamepad` runs the Windows
//! GameInput pump on its own thread (`crates/gamepad` path dep, target-scoped
//! to Windows) and fires pad binds — including Guide/Share system buttons —
//! through the SAME shared debounce gate as the hotkey listener, so an action
//! fires once per interval whatever triggered it; Linux has no GameInput
//! backend (gilrs stays out of scope), where the module only logs. The MVP
//! keyboard trigger is the global hotkey, wired in Stage 6. Bare `virshot` is the resident app (Stage 8):
//! loads config, registers hotkeys via `hotkeys::HotkeysController` (manager
//! on the MAIN thread — Windows WM_HOTKEY thread affinity), opens the eframe
//! settings window on the MAIN thread (`gui`); the window X button hides the
//! window and keeps the process resident (cancel-close + hide), so the
//! hotkeys stay live. Notifications go through `crates/toast`: the GUI's
//! eframe loop hosts `crates/overlay`'s `OverlayHost` (toast chips are
//! click-through, transparent, content-protected child viewports), and its
//! `ToastService` handle is attached to a latest-only hand-off slot
//! (`capture::ToastHandoff`) that every trigger thread reads per capture
//! (Stage 4 of the overlay/toast plan). Tray (Stage 10, spec §4.5): attempted
//! BEFORE the GUI via `tray::TrayController` so it exists even when the GUI
//! cannot open; a tray failure is a typed ERROR log and the app keeps running
//! without a tray.
//! While the GUI runs, `gui::logic` maps tray actions (left click/Settings →
//! re-show the hidden window; Take screenshot → full-screen capture; Exit →
//! real shutdown, the only exit path). If the GUI cannot open, the main
//! thread parks on an idle sleep tick that still polls the tray: Exit sets
//! the shutdown flag and the park loop returns (exit 0); a GUI-failure
//! fallback keeps the process resident rather than exiting (spec §5). The
//! single-instance guard (Stage 11, spec §1) is taken right after the config
//! load and held for the whole resident lifetime, so a second `virshot` on the
//! same config exits with a typed ERROR (exit 1) before any tray, hotkey, or
//! window exists, and the lock is freed by `Drop` on a clean exit. Headless
//! `shot` is deliberately NOT guarded: it is a one-shot CLI tool for
//! tests/scripts (spec §8), so scripted captures keep working while the
//! resident app runs.
//!
//! CLI contract (spec §8): global flags `--config`, `--log`,
//! `--log-level=DEBUG|INFO|ERROR` (default INFO), `--version`, `--help`;
//! headless subcommand `virshot shot [--window] [--out <path>]
//! [--format png|jpg|webp]` — no GUI/tray/overlay; the config file is the
//! source of defaults; `--out`/`--format` override it; the saved full path
//! goes to stdout; exit 0 = success, exit 1 = capture/save error. Unknown
//! flags and invalid `--format`/`--log-level` values are clap usage errors
//! (clap's own exit code 2) — the 0/1 contract covers shot outcomes only.
//!
//! Logging: default `virshot.log` next to the config file, `--log` overrides
//! it, `--log-level` default INFO, no rotation. The flag owns the level: the
//! `VIRSHOT_LOG_LEVEL` env override from Stage 1 is gone.

#![forbid(unsafe_code)]
#![windows_subsystem = "windows"]

// rust-i18n init lives at the CRATE ROOT: `rust_i18n::t!` expands to
// `crate::_rust_i18n_t!`, so this call must be in main.rs for `t!` to resolve
// anywhere in the crate; catalogs are embedded at build time from `locales/`.
// `fallback = "en"` is required — without it a miss returns the dotted key.
rust_i18n::i18n!("locales", fallback = "en");

// Cargo does not track `locales/app.yml` for this crate: rust-i18n reads the
// catalog during macro expansion, and its build script's `rerun-if-changed`
// scope is the rust-i18n crate's own directory, so rustc's dep-info never
// lists the catalog — editing a translation would ship stale strings without
// a rebuild. `include_str!` adds a tracked dep-info edge, so catalog edits
// recompile this crate.
const I18N_CATALOG_SOURCE: &str = include_str!("../locales/app.yml");

mod bindings;
mod capture;
mod config;
mod fonts;
mod gamepad;
mod gui;
mod hotkeys;
mod instance;
mod logging;
mod template;
mod tray;

use chrono::Local;
use clap::Parser;
use clap::Subcommand;
use clap::ValueEnum;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use crate::capture::CaptureCommand;
use crate::capture::CaptureMode;
use crate::capture::ToastHandoff;
use crate::capture::handoff_handle;
use crate::capture::new_handoff;
use crate::config::Format;
use crate::config::General;
use crate::config::Settings;
use crate::logging::Level;

/// Idle tick for the resident app loop: idle CPU stays far under the §5 budget.
const IDLE_TICK: std::time::Duration = std::time::Duration::from_secs(1);

/// Set by the tray `Exit` action when the process runs WITHOUT a GUI (the
/// GUI-failure park loop): the loop checks it each tick and returns, so the
/// process exits 0. With the GUI running, the same tray action goes through
/// the eframe close flow (`gui::SettingsApp::exit_requested`) instead; this
/// flag is the park-loop half of the tray Exit contract (spec §4.5).
static RESIDENT_SHUTDOWN: AtomicBool = AtomicBool::new(false);

/// Grace the async log writer thread gets before a run that exits right after
/// logging (`logging` has no shutdown hook): headless `run_shot` and the
/// resident guard-failure exit.
const LOG_DRAIN_GRACE: std::time::Duration = std::time::Duration::from_millis(200);

/// CLI surface (spec §8). The global options are accepted before or after the
/// subcommand.
#[derive(Parser)]
#[command(version, about = "Ultra-lightweight gamepad screenshot utility")]
struct Cli {
    /// Config file; defaults to the OS-convention `<config dir>/virshot/config.toml`.
    #[arg(long, global = true, default_value_os_t = config::default_config_path())]
    config: PathBuf,

    /// Log file; defaults to `virshot.log` next to `--config`.
    #[arg(long, global = true)]
    log: Option<PathBuf>,

    /// Verbosity, default `logging::default_level()` (INFO). Parsed by
    /// `logging::parse_level` (case insensitive); any other value is a clap
    /// usage error (exit 2).
    #[arg(long, global = true, value_parser = logging::parse_level)]
    log_level: Option<Level>,

    /// Absent = resident app (bare `virshot`); `shot` = headless capture.
    #[command(subcommand)]
    command: Option<Command>,
}

/// Subcommands (spec §8). Only `shot` exists in the MVP CLI.
#[derive(Subcommand)]
enum Command {
    /// Headless single capture: prints the saved path to stdout; exit 0 on
    /// success, 1 on error; no GUI/tray/overlay.
    Shot {
        /// Capture the foreground window instead of the full screen.
        #[arg(long)]
        window: bool,

        /// Exact output file path; overrides the template-produced path.
        #[arg(long)]
        out: Option<PathBuf>,

        /// Image format; overrides `general.format` from the config.
        #[arg(long, value_enum)]
        format: Option<CliFormat>,
    },
}

/// `--format` values (spec §8: `png|jpg|webp`); mapped onto `config::Format`.
#[derive(ValueEnum, Clone, Copy)]
enum CliFormat {
    Png,
    Jpg,
    Webp,
}

impl CliFormat {
    fn to_format(self) -> Format {
        match self {
            CliFormat::Png => Format::Png,
            CliFormat::Jpg => Format::Jpeg,
            CliFormat::Webp => Format::Webp,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let config_path = cli.config;
    let level = cli.log_level.unwrap_or_else(logging::default_level);
    let log_path = cli
        .log
        .unwrap_or_else(|| logging::default_log_path(&config_path));
    // The default log is a sibling of the config file, whose directory only
    // appears on first run (`config::load_or_create` creates it). Logging is
    // initialised before the config load so the config module can log through
    // the subscriber — hence the log directory is created up front, best-effort.
    prepare_log_dir(&log_path);
    if let Err(err) = logging::init_logging(&log_path, level) {
        // Startup init is the allowed console-print exception: if logging
        // itself is unavailable there is no subscriber to report through.
        eprintln!("Logging unavailable: {err}");
    }
    tracing::info!("virshot starting; logging to {}", log_path.display());

    match cli.command {
        Some(Command::Shot {
            window,
            out,
            format,
        }) => run_shot(&config_path, window, out, format),
        None => run_resident(&config_path),
    }
}

/// Best-effort creation of the log file's parent directory; `init_logging`
/// still reports any I/O failure it hits afterwards (startup init must not
/// fail on this step).
fn prepare_log_dir(log_path: &Path) {
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
}

/// Headless single capture (spec §8): load settings from `config_path`
/// (first run auto-creates the defaults — the config is the default source),
/// apply the flag overrides, run `capture::run_capture` synchronously on the
/// main thread (no GUI/tray/overlay, no save thread — the CLI must print the
/// path before exiting), then map the outcome per the contract: success
/// prints the saved full path to stdout and exits 0; failure prints the typed
/// error to stderr and exits 1.
fn run_shot(
    config_path: &Path,
    window: bool,
    out: Option<PathBuf>,
    format: Option<CliFormat>,
) -> ExitCode {
    let settings = config::load_or_create(config_path);
    let Settings {
        mut general,
        controls,
        overlay,
    } = settings;
    if let Some(format) = format {
        general.format = format.to_format();
    }
    if let Some(out_path) = &out
        && !apply_out_override(&mut general, out_path)
    {
        return ExitCode::FAILURE;
    }

    let mode = if window {
        CaptureMode::Window
    } else {
        CaptureMode::FullScreen
    };
    let command = CaptureCommand {
        mode,
        settings: Settings {
            general,
            controls,
            overlay,
        },
    };

    // Headless one-shot: no GUI, so no toast service exists and no handle can
    // be passed — the notification is skipped and the CLI contract (stdout
    // path / stderr error + exit code) is unchanged (spec §4.3).
    let outcome = capture::run_capture(command, Local::now(), None);
    let code = match outcome {
        Ok(path) => {
            tracing::info!("Saved: {}", path.display());
            println!("{}", path.display());
            ExitCode::SUCCESS
        }
        Err(err) => {
            tracing::error!("Capture failed: {err}");
            eprintln!("Capture failed: {err}");
            ExitCode::FAILURE
        }
    };
    // The log writer thread drains the queue asynchronously and `logging` has
    // no shutdown hook: a headless run exits right after printing, so give the
    // writer a bounded moment or the final INFO/ERROR line could be lost.
    std::thread::sleep(LOG_DRAIN_GRACE);
    code
}

/// `--out` override decision: the value is the **exact output file path**. The
/// template-produced path is replaced by making the value's parent directory
/// `save_path` and its file name the filename template — the save pipeline
/// (collision scan, subfolder creation, encoding) stays owned by `capture`,
/// and the spec §4.2 collision rule still applies: when the file already
/// exists, `capture` appends `-N` before the extension
/// (`template::collision_variant`) instead of overwriting it. `--format`
/// chooses the encoding, not the extension: the saved file keeps the exact
/// name from `--out` even if its extension disagrees with the format. Returns
/// false (error printed to stderr) when the path has no usable file name
/// (a directory/root path) or the name is not valid UTF-8.
fn apply_out_override(general: &mut General, out: &Path) -> bool {
    let Some(name) = out.file_name().and_then(|name| name.to_str()) else {
        eprintln!("invalid --out path {}: expected a file path", out.display());
        return false;
    };
    // A relative file name has no parent: `save_path` becomes empty and the
    // path resolves under the current directory.
    general.save_path = out
        .parent()
        .map(|parent| parent.display().to_string())
        .unwrap_or_default();
    general.file_full_screen = name.to_owned();
    general.file_window = name.to_owned();
    true
}

/// Resident app (bare `virshot`, no subcommand): load settings with the spec §7
/// defaults as the fallback — a missing file is auto-created with the defaults
/// (spec §7, first run), a broken file logs the typed error and still runs on
/// defaults without rewriting the user's file (startup init; the loop never
/// fails on config). The single-instance guard (spec §1) is taken right after
/// the config load: a lock held by a live process is a typed ERROR and exit 1,
/// so no tray, hotkey, or window is ever created by a second instance. The
/// settings drive the hotkey wiring: the controller is created on the MAIN
/// thread (spec §4.1 — conflict/unavailable is a status +
/// log, never a crash) and its registration report is logged per hotkey. Then
/// the eframe settings window opens on the MAIN thread (Stage 8): while the
/// window runs, the event loop drives it; closing the window hides it and
/// keeps the process resident (hotkeys stay live; the tray can reopen it).
/// The tray (Stage 10, spec §4.5) is attempted BEFORE the GUI on the MAIN
/// thread so it exists even when the GUI cannot open: its failure is a typed
/// ERROR log and the app keeps running without a tray. If the GUI cannot
/// open, the main thread parks on an idle sleep tick that still polls the
/// tray — the process stays resident rather than exiting (spec §5), and
/// tray `Exit` ends the park loop (exit 0).
fn run_resident(config_path: &Path) -> ExitCode {
    let settings = config::load_or_create(config_path);
    // Locale for the resident UI (spec §2): set right after the config load
    // and BEFORE the tray, hotkey status logs, and the GUI, so every label
    // built later resolves in the configured language (`language` is already
    // normalized to a known code by `config::load_or_default`). Headless
    // `shot` deliberately keeps the process-default English (spec §2: CLI,
    // `--help`, and stderr stay English).
    rust_i18n::set_locale(config::language_code(&settings.general.language));
    // Startup-only diagnostic (never in the event loop or capture path): a
    // non-test use that keeps `I18N_CATALOG_SOURCE` alive, so the const above
    // stays a real dep-info edge without a dead-code warning.
    tracing::debug!("i18n catalog loaded: {} bytes", I18N_CATALOG_SOURCE.len());
    // Single-instance guard (spec §1): one resident process per config file.
    // Acquired here — after logging and the config load, before the tray,
    // hotkeys, and GUI — and held for the whole process lifetime, so `Drop`
    // removes the lock on every exit path (GUI shutdown and the park-loop
    // exit). A lock held by a live process ends startup before anything is
    // built: typed ERROR + exit 1.
    let _guard = match instance::InstanceGuard::acquire(config_path) {
        Ok(guard) => guard,
        Err(err) => {
            tracing::error!("virshot cannot start: {err}");
            // Startup init: no window or tray exists to report through.
            eprintln!("virshot cannot start: {err}");
            // This run exits immediately, so the writer thread gets the same
            // bounded drain a headless `shot` run does — otherwise the typed
            // reason would be lost from the log.
            std::thread::sleep(LOG_DRAIN_GRACE);
            return ExitCode::FAILURE;
        }
    };
    tracing::info!("Instance lock acquired (pid {})", std::process::id());
    // One toast-handle hand-off for the whole resident process (spec §4.3):
    // empty until the GUI attaches the `ToastService` handle in its create
    // closure; capture threads read a clone per trigger, so a capture fired
    // before the GUI exists (or after it is gone) simply carries no handle.
    let handoff = new_handoff();
    // One settings mailbox for the whole resident process: UI Apply publishes
    // it, the hotkey listener clones it per trigger, and the pad pump re-parses
    // pad binds per event from the SAME Arc (Stage 5) — so a re-bind or a pad
    // bind captured in the UI fires without a restart.
    let settings_mailbox = Arc::new(Mutex::new(settings.clone()));
    // One trigger gate for BOTH sources (spec §4.1 OR-fire, ~250 ms per
    // action): the hotkey listener and the gamepad pump fire through it, so a
    // pad press and a keyboard combo of the same action produce ONE shot.
    let debounce = Arc::new(Mutex::new(bindings::Debounce::default()));
    let (controller, registrations) = match hotkeys::HotkeysController::start(
        Arc::clone(&settings_mailbox),
        Arc::clone(&handoff),
        Arc::clone(&debounce),
    ) {
        Ok((controller, status)) => {
            log_registrations(&status);
            (Some(controller), status.registrations)
        }
        Err(err) => {
            tracing::error!("Hotkeys unavailable: {err}");
            (None, Vec::new())
        }
    };

    // Gamepad trigger (Stages 4-5, spec §4.1 pad-field OR-fire, §4.4 pad
    // capture): on Windows the GameInput pump runs on its own thread, fires
    // `pad` binds through the same gate, and reads/writes the pad-capture
    // mailbox the GUI drives. On other platforms it logs and spawns nothing
    // (Linux gilrs is out of scope — AGENTS.md hardware finding). A pump
    // failure is an INFO log — the app keeps running on hotkeys alone.
    let pad_capture = gamepad::new_capture_mailbox();
    gamepad::spawn_pump(
        Arc::clone(&settings_mailbox),
        Arc::clone(&handoff),
        Arc::clone(&debounce),
        Arc::clone(&pad_capture),
    );

    // Tray first (spec §4.5): the icon must exist even if the GUI fails, and
    // it is built on the MAIN thread. Failure keeps the app running.
    let tray = match tray::TrayController::new() {
        Ok(tray) => {
            tracing::info!("System tray ready");
            Some(tray)
        }
        Err(err) => {
            tracing::error!("System tray unavailable: {err}");
            None
        }
    };

    tracing::info!("Settings window opening");
    if let Err(err) = gui::run(
        config_path.to_owned(),
        settings.clone(),
        controller,
        registrations,
        Arc::clone(&handoff),
        tray.clone(),
        Arc::clone(&settings_mailbox),
        Arc::clone(&pad_capture),
    ) {
        tracing::error!("GUI failed: {err}");
        tracing::info!("virshot idle: running without GUI");
        // No GUI means no toast service: the slot stays empty and captures
        // log instead of showing a chip.
        park_without_gui(tray, settings, handoff);
        return ExitCode::SUCCESS;
    }
    // `run_native` only returns on real shutdown (tray Exit) or startup
    // failure; a clean shutdown exits 0.
    ExitCode::SUCCESS
}

/// GUI-failure fallback (spec §5): park the MAIN thread and still serve the
/// tray. Actions are polled on the idle tick (≤ 1 s latency): `Take
/// screenshot` spawns a full-screen capture with the loaded settings and a
/// clone of the toast hand-off slot (empty on this path, so the capture logs
/// instead of showing a chip); `Settings` degrades to a WARN (no window to
/// show — the GUI failure was logged above); `Exit` sets
/// [`RESIDENT_SHUTDOWN`] and the
/// loop returns, so the process exits 0 — the only exit path without a GUI.
fn park_without_gui(tray: Option<tray::TrayController>, settings: Settings, handoff: ToastHandoff) {
    while !RESIDENT_SHUTDOWN.load(Ordering::Relaxed) {
        if let Some(tray) = &tray {
            for action in tray.poll() {
                match action {
                    tray::TrayAction::Exit => {
                        tracing::info!("virshot exiting from tray menu");
                        RESIDENT_SHUTDOWN.store(true, Ordering::Relaxed);
                    }
                    tray::TrayAction::TakeScreenshot => {
                        capture::spawn_capture(
                            CaptureCommand {
                                mode: CaptureMode::FullScreen,
                                settings: settings.clone(),
                            },
                            handoff_handle(&handoff),
                        );
                    }
                    tray::TrayAction::ShowSettings => {
                        tracing::warn!("Tray: Settings requested, but the GUI is unavailable");
                    }
                }
            }
        }
        std::thread::sleep(IDLE_TICK);
    }
}

/// Registration report (spec §4.1): registered combos at INFO, the reserved
/// rect action at INFO, rejected combos at ERROR with a READABLE reason
/// (spec §4.1 v2 — the reason names the combo and the remedy, not the crate's
/// `Debug` dump) plus the Stage 13 focus-retry hint. The app keeps running in
/// every case.
fn log_registrations(status: &hotkeys::StartStatus) {
    for registration in &status.registrations {
        match registration {
            hotkeys::Registration::Registered { action, combo } => {
                tracing::info!("Hotkey registered: {combo} ({})", action.label());
            }
            hotkeys::Registration::Failed {
                action,
                combo,
                reason,
            } => {
                tracing::error!(
                    "Hotkey not registered: {combo} ({}): {reason} — retried when the settings window gains focus",
                    action.label()
                );
            }
            hotkeys::Registration::Reserved { action, combo } => {
                tracing::info!(
                    "Hotkey reserved (rect area, out of MVP scope): {combo} ({})",
                    action.label()
                );
            }
        }
    }
}

#[cfg(test)]
#[path = "i18n.tests.rs"]
mod i18n_tests;
