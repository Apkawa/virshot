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
//! While the GUI runs, `gui::SettingsApp::logic` maps tray actions (left click/Settings →
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

// Build metadata (task 2026-11-10_about-dialog): `shadow!(build)` includes the
// `OUT_DIR/shadow.rs` constants generated by `build.rs` (package version, git
// fields, build timestamps). The generated module contains no `unsafe`, so the
// crate-root `#![forbid(unsafe_code)]` stays intact.
shadow_rs::shadow!(build);

mod about;
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
mod update;

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
use std::sync::mpsc;

use crate::capture::CaptureCommand;
use crate::capture::CaptureMode;
use crate::capture::ToastHandoff;
use crate::capture::handoff_handle;
use crate::capture::new_handoff;
use crate::config::Format;
use crate::config::General;
use crate::config::Settings;
use crate::logging::Level;

#[cfg(test)]
/// Process-wide serialization for tests that mutate the process-global
/// rust-i18n locale. rust-i18n's locale + cache version are global, so every
/// locale-touching test file must hold THIS lock, not a per-file one.
pub(crate) static LOCALE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

/// Startup-check decision (spec §4.7, Stages 7-9): a background check runs at
/// startup ONLY when checks are scheduled (`update_check != Never`). Dev
/// builds are NOT excluded — a `-dev` build checks updates on the same terms
/// as a release build: `update::decide_update` ships an update iff the release
/// `releaseDate` is newer than the embedded build time OR the release VERSION
/// is higher, with the hotfix 2026-11-12 guard: an equal `commit_sha`
/// suppresses the date trigger (a re-packaged nightly of the same commit is
/// not a newer build). `is_dev` is still passed by callers so the build kind
/// is stated at the call site, but no longer changes the decision.
/// `arm_startup_check` applies it on both resident paths; the interval checks
/// live in `gui::SettingsApp::logic` (GUI) and in `park_without_gui` (GUI-less).
fn should_check_at_startup(interval: config::UpdateCheck, _is_dev: bool) -> bool {
    interval != config::UpdateCheck::Never
}

/// Decision behind the restart spawn-failure fallback (spec §5 open question):
/// a re-taken instance lock keeps THIS process resident (`None` — no exit
/// code), an unusable lock ends the run with a typed failure.
fn restart_after_failed_spawn(lock_reacquired: bool) -> Option<ExitCode> {
    if lock_reacquired {
        None
    } else {
        Some(ExitCode::FAILURE)
    }
}

/// CLI surface (spec §8). The global options are accepted before or after the
/// subcommand.
#[derive(Parser)]
#[command(about = "Ultra-lightweight gamepad screenshot utility")]
struct Cli {
    /// Print the build metadata line (`virshot <version> (<sha>) built
    /// <time>`) and exit 0. clap's own version renderer only embeds a
    /// static string (its `string` feature is off for the size budget), and
    /// the build line is computed from build-time constants
    /// (`task 2026-11-10_about-dialog`), so `--version` is an explicit global
    /// flag handled in `main` before any other startup work.
    #[arg(long, global = true)]
    version: bool,

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
    // `--version` prints the build metadata line and exits 0 before any other
    // startup work (no config, log file, tray, or window is created).
    if cli.version {
        println!("{}", about::version_from_build());
        return ExitCode::SUCCESS;
    }
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

    // Startup update cleanup (spec §4.7, Stage 7): leftovers of a previous
    // self-replace and stale `virshot-update-*` install dirs are removed on
    // EVERY start — before the shot/resident dispatch, so the headless CLI
    // path cleans too (it never touches the network). `--version` above exits
    // before logging and is deliberately excluded: it prints metadata and
    // performs no filesystem work. `cleanup_leftovers` never fails — a missing
    // directory is ignored, other I/O failures are logged there and startup
    // continues — so an unresolvable exe path scans the temp dir twice (the
    // exe-dir pass is a cheap no-op) instead of skipping the cleanup.
    let temp_dir = std::env::temp_dir();
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_owned));
    let removed = match exe_dir.as_deref() {
        Some(dir) => update::install::cleanup_leftovers(dir, &temp_dir),
        None => update::install::cleanup_leftovers(&temp_dir, &temp_dir),
    };
    if !removed.is_empty() {
        tracing::info!(
            "Startup update cleanup: {} leftover file(s) removed",
            removed.len()
        );
    }

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
/// tray `Exit` ends the park loop (exit 0). A successful update install closes
/// the window and the tail restarts the new binary (Stage 7, spec §5).
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
    // built: typed ERROR + exit 1. Held in a slot (Stage 7): the restart
    // sequence drops it deliberately BEFORE spawning the new process.
    let mut guard_slot = match instance::InstanceGuard::acquire(config_path) {
        Ok(guard) => Some(guard),
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

    // Auto-update channel (Stage 7 ownership): `main` creates the pair and
    // hands BOTH halves to the GUI — `gui::run` does not create its own. The
    // Receiver is drained ONLY in `App::logic` (the sole drain point); the
    // Sender is moved into the GUI, which clones it per scheduled check (an
    // O(1) clone, no `Arc`). `restart_requested` is the install→restart flag:
    // the GUI sets it when an install replaces the binary and asks the
    // viewport to close; the tail below runs the restart (spec §5).
    let (update_tx, update_rx) = mpsc::channel::<update::CheckOutcome>();
    let restart_requested = Arc::new(AtomicBool::new(false));
    arm_startup_check(&settings, &update_tx);

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
        update_tx,
        update_rx,
        Arc::clone(&restart_requested),
    ) {
        tracing::error!("GUI failed: {err}");
        tracing::info!("virshot idle: running without GUI");
        // No GUI means no toast service: the slot stays empty and captures
        // log instead of showing a chip.
        // The failed GUI attempt consumed the startup channel's receiver with
        // the app it never got to run, so the park loop owns a fresh pair —
        // it drains its own checks (there is no `App::logic` on this path).
        let (park_tx, park_rx) = mpsc::channel::<update::CheckOutcome>();
        arm_startup_check(&settings, &park_tx);
        park_without_gui(
            tray.as_ref(),
            &settings,
            &handoff,
            park_tx,
            park_rx,
            &restart_requested,
        );
    }
    // Restart tail (spec §5, Stage 7). An install success set `restart_requested`
    // and closed the window: drop the lock, spawn the new binary, exit 0. A
    // failed spawn re-took the lock (see `restart_for_update`) — this process
    // stays resident via the park loop and exits 0 later, so the installed
    // binary runs at the next start. Without the flag this is a clean
    // shutdown: the guard drops HERE, removing the lock, and the run exits 0.
    if let Some(code) =
        restart_if_update_requested(config_path, &mut guard_slot, &restart_requested)
    {
        return code;
    }
    if restart_requested.load(Ordering::Relaxed) {
        tracing::info!(
            "virshot idle: restart abandoned, staying resident — the installed binary runs at the next start"
        );
        let (retry_tx, retry_rx) = mpsc::channel::<update::CheckOutcome>();
        arm_startup_check(&settings, &retry_tx);
        park_without_gui(
            tray.as_ref(),
            &settings,
            &handoff,
            retry_tx,
            retry_rx,
            &restart_requested,
        );
    }
    ExitCode::SUCCESS
}

/// Arm the startup background check on `sender` when the settings say so
/// (spec §4.7): one call site per resident path (GUI, park), so the decision
/// and the spawn live in one place. The Sender clone is O(1) — `spawn_check`
/// moves an owned sender into its thread while the caller keeps its own.
fn arm_startup_check(settings: &Settings, sender: &mpsc::Sender<update::CheckOutcome>) {
    let build = update::current_build();
    if should_check_at_startup(settings.general.update_check, build.is_dev()) {
        update::spawn_check(
            settings.general.update_channel,
            build,
            sender.clone(),
            update::fetch_manifest_text,
        );
    }
}

/// The restart sequence after a successful install (spec §5, Stage 7): FIRST
/// the guard in `guard_slot` is dropped — `InstanceGuard::Drop` removes
/// `<config>.lock`, so the NEW process can take the lock and the pid probe
/// must not see this one; the ordering is load-bearing, hence the guard is
/// moved out and dropped before the spawn, not after. THEN `current_exe()` is
/// spawned — the freshly replaced binary — the log writer gets the same
/// bounded drain a `shot` run does, and the run exits 0 (the new process is
/// the app now).
/// On a spawn FAILURE the old process must stay alive and guarded (spec §5
/// open question): the fresh guard is written back into `guard_slot` and the
/// decision is delegated to [`restart_after_failed_spawn`].
fn restart_for_update(
    config_path: &Path,
    guard_slot: &mut Option<instance::InstanceGuard>,
) -> Option<ExitCode> {
    drop(guard_slot.take());
    match std::env::current_exe().and_then(|exe| std::process::Command::new(exe).spawn()) {
        Ok(_child) => {
            tracing::info!("Restarting for update: new process spawned");
            std::thread::sleep(LOG_DRAIN_GRACE);
            Some(ExitCode::SUCCESS)
        }
        Err(err) => {
            tracing::error!("Restart for update failed: {err}");
            let reacquired = match instance::InstanceGuard::acquire(config_path) {
                Ok(guard) => {
                    // Owned by the caller's slot now: this process keeps the
                    // lock and stays resident (no exit — see
                    // `restart_after_failed_spawn`).
                    *guard_slot = Some(guard);
                    tracing::info!("Instance lock re-acquired; virshot stays resident");
                    true
                }
                Err(reacquire_err) => {
                    tracing::error!("Cannot re-acquire the instance lock: {reacquire_err}");
                    false
                }
            };
            restart_after_failed_spawn(reacquired)
        }
    }
}

/// Restart ONLY when an install asked for it (spec §5). Returns `Some(code)`
/// when the restart ran (spawn + exit 0) or when the lock could not be
/// re-taken (exit 1); `None` for a clean shutdown and for the spawn-failure
/// fallback where this process re-took the lock — the caller then parks.
fn restart_if_update_requested(
    config_path: &Path,
    guard_slot: &mut Option<instance::InstanceGuard>,
    restart_requested: &AtomicBool,
) -> Option<ExitCode> {
    if restart_requested.load(Ordering::Relaxed) {
        restart_for_update(config_path, guard_slot)
    } else {
        None
    }
}

/// GUI-failure fallback (spec §5): park the MAIN thread and still serve the
/// tray. Actions are polled on the idle tick (≤ 1 s latency): `Take
/// screenshot` spawns a full-screen capture with the loaded settings and a
/// clone of the toast hand-off slot (empty on this path, so the capture logs
/// instead of showing a chip); `Settings` and `About` degrade to a WARN (no
/// window to show — the GUI failure was logged above); `Exit` sets
/// [`RESIDENT_SHUTDOWN`] and the
/// loop returns, so the process exits 0 — the only exit path without a GUI.
///
/// Stage 7 update plumbing (spec §4.7, §5): this loop is the DRAIN POINT of
/// its own channel — there is no `App::logic` without a GUI. Every tick
/// drains `update_rx`: a `Updated` outcome installs immediately when
/// `general.auto_install` is ON (one plain `std` thread, its result drained on
/// a second channel), and with the flag OFF the discovery is an INFO log ONLY
/// — no window, no toast (§5). `Failed` is a WARN, `UpToDate` a debug line. A
/// successful install sets [`RESIDENT_SHUTDOWN`] (the loop returns) and
/// `restart_requested` (so `run_resident` runs the restart sequence). Checks
/// re-spawn on the interval deadline; only `Never` schedules nothing — dev
/// builds schedule on the same terms as release builds, mirroring
/// `gui::SettingsApp::logic`. The forced «Проверить обновления» button is a GUI
/// affordance (§4.7), so this path has no forced-check entry.
fn park_without_gui(
    tray: Option<&tray::TrayController>,
    settings: &Settings,
    handoff: &ToastHandoff,
    update_tx: mpsc::Sender<update::CheckOutcome>,
    update_rx: mpsc::Receiver<update::CheckOutcome>,
    restart_requested: &AtomicBool,
) {
    let build = update::current_build();
    let interval = settings.general.update_check.interval();
    let mut next_check = update::next_check_deadline(std::time::Instant::now(), interval);
    let mut install_rx: Option<mpsc::Receiver<Result<(), String>>> = None;
    while !RESIDENT_SHUTDOWN.load(Ordering::Relaxed) {
        // Check outcomes: drained here, newest wins. Checks never overlap:
        // the spawn below only happens once the deadline passes and re-arms
        // that deadline immediately, so the tick spacing bounds the schedule
        // (in the GUI the same invariant is the `next_check = None` slot).
        while let Ok(outcome) = update_rx.try_recv() {
            match outcome {
                update::CheckOutcome::Updated(info) => {
                    if settings.general.auto_install {
                        tracing::info!(
                            "Auto-install: update {} ({})",
                            info.version,
                            info.commit_sha
                        );
                        let (tx, rx) = mpsc::channel::<Result<(), String>>();
                        install_rx = Some(rx);
                        // One plain std thread per install: `info` is moved in
                        // (thread boundary — no borrow of the loop's state).
                        std::thread::spawn(move || {
                            let outcome = update::install::install(&info, |path| {
                                self_replace::self_replace(path)
                            })
                            .map_err(|err| err.to_string());
                            let _ = tx.send(outcome);
                        });
                    } else {
                        tracing::info!(
                            "Update available: {} ({}) — auto-install is OFF, nothing installed (spec §5)",
                            info.version,
                            info.commit_sha
                        );
                    }
                }
                update::CheckOutcome::UpToDate => tracing::debug!("update check: up to date"),
                update::CheckOutcome::Failed(err) => tracing::warn!("update check failed: {err}"),
            }
        }
        // Install result, drained on the same tick: success ends the park loop
        // AND marks the restart, so `run_resident` restarts (spec §5).
        let install_result = install_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(result) = install_result {
            install_rx = None;
            match result {
                Ok(()) => {
                    tracing::info!("update installed: restart requested");
                    restart_requested.store(true, Ordering::Relaxed);
                    RESIDENT_SHUTDOWN.store(true, Ordering::Relaxed);
                }
                Err(message) => tracing::warn!("update install failed: {message}"),
            }
        }
        // Interval check (spec §4.7): the deadline re-arms on every spawn, so
        // the schedule follows the interval the settings published. The clones
        // are the thread boundary — `spawn_check` moves owned values into its
        // thread while the loop keeps its own for the next deadline.
        if let Some(deadline) = next_check
            && std::time::Instant::now() >= deadline
        {
            update::spawn_check(
                settings.general.update_channel,
                build.clone(),
                update_tx.clone(),
                update::fetch_manifest_text,
            );
            next_check = update::next_check_deadline(std::time::Instant::now(), interval);
        }
        if let Some(tray) = tray {
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
                                // Owned copy for the capture thread; the loop
                                // keeps reading the same settings per tick.
                                settings: settings.clone(),
                            },
                            handoff_handle(handoff),
                        );
                    }
                    tray::TrayAction::ShowSettings => {
                        tracing::warn!("Tray: Settings requested, but the GUI is unavailable");
                    }
                    tray::TrayAction::About => {
                        tracing::warn!("Tray: About requested, but the GUI is unavailable");
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

#[cfg(test)]
#[path = "main.tests.rs"]
mod tests;
