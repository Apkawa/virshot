//! Logging module: level parsing, log-file placement, and global subscriber setup.
//!
//! Note: this `tracing-subscriber` version (0.3.23) has no `fmt::non_blocking`,
//! so non-blocking file writes are implemented here with a dedicated writer
//! thread plus a channel (std only, no new dependency).

use std::io;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::mpsc;

/// File name placed next to the config file.
const LOG_FILE_NAME: &str = "virshot.log";

/// Crate-level verbosity. Deliberately narrower than `tracing::Level`: the MVP
/// config only supports debug/info/error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Level {
    Debug,
    Info,
    Error,
}

impl From<Level> for tracing::Level {
    fn from(level: Level) -> tracing::Level {
        match level {
            Level::Debug => tracing::Level::DEBUG,
            Level::Info => tracing::Level::INFO,
            Level::Error => tracing::Level::ERROR,
        }
    }
}

/// Logging errors, typed per module (`thiserror` enum, no stringly `Err(String)`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum Error {
    /// Unrecognised level name; the offending value is kept in the message.
    #[error("invalid log level {0:?}: expected debug, info, or error")]
    InvalidLogLevel(String),

    /// The log file could not be opened, or the log writer thread could not start.
    /// `io::Error` itself is not `Clone`, so the kind and message are kept as
    /// owned values (error is reported again on repeated init attempts).
    #[error("failed to open log file {path}: {kind:?}: {message}")]
    Io {
        path: PathBuf,
        kind: io::ErrorKind,
        message: String,
    },
}

/// Parses a level name case-insensitively (`debug`, `info`, `error`), ignoring
/// surrounding whitespace. Anything else — `trace`, `warn`, `""` — fails with
/// `Error::InvalidLogLevel`.
pub(crate) fn parse_level(s: &str) -> Result<Level, Error> {
    let trimmed = s.trim();
    match trimmed.to_ascii_lowercase().as_str() {
        "debug" => Ok(Level::Debug),
        "info" => Ok(Level::Info),
        "error" => Ok(Level::Error),
        _ => Err(Error::InvalidLogLevel(trimmed.to_owned())),
    }
}

/// Default verbosity when no config is available yet: INFO.
pub(crate) fn default_level() -> Level {
    Level::Info
}

/// Log file next to `config_path`: same directory, fixed name `virshot.log`.
///
/// Both `/` and `\` are treated as separators, so a Windows-style path resolves
/// the same way on Linux test hosts (plain `Path::parent` would collapse a
/// backslash path into an empty directory).
pub(crate) fn default_log_path(config_path: &Path) -> PathBuf {
    let text = config_path.display().to_string();
    let dir = match text.rfind(['/', '\\']) {
        Some(separator) => &text[..=separator],
        None => "",
    };
    PathBuf::from(format!("{dir}{LOG_FILE_NAME}"))
}

/// Hands log lines to the writer thread so the caller never blocks on disk I/O.
/// `mpsc::Sender` is `Send` but not `Sync`, while the global subscriber must be
/// `Sync` — hence the `Mutex` wrapper (locked for one send per line only).
struct LogWriter {
    sender: Arc<Mutex<mpsc::Sender<Vec<u8>>>>,
}

impl io::Write for LogWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let sender = self
            .sender
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "log writer lock poisoned"))?;
        sender
            .send(buf.to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "log writer thread stopped"))?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        // Nothing buffered here: the bytes are already queued for the thread.
        Ok(())
    }
}

/// Writer thread: appends queued lines to the file. Write failures are swallowed
/// on purpose — logging must never take down the gamepad loop. Lines still in
/// the queue at process exit are lost (no shutdown hook at Stage 1).
fn drain_log(receiver: mpsc::Receiver<Vec<u8>>, mut file: std::fs::File) {
    for batch in receiver {
        let _ = file.write_all(&batch);
        let _ = file.flush();
    }
}

/// Keeps the io failure's kind and message, attributed to the log path.
fn io_error(log_path: &Path, err: io::Error) -> Error {
    Error::Io {
        path: log_path.to_path_buf(),
        kind: err.kind(),
        message: err.to_string(),
    }
}

/// Opens `log_path` (create + append) and installs the global
/// `tracing_subscriber` fmt subscriber: non-blocking file writer, max level
/// `level`, no ANSI, target and level in every line.
///
/// Installed once — a later call returns `Ok` and leaves the first subscriber in
/// place (also when some other subscriber was already set globally). Fails with
/// `Error::Io` if the file cannot be opened or the writer thread cannot be
/// spawned; it never panics.
pub(crate) fn init_logging(log_path: &Path, level: Level) -> Result<(), Error> {
    static INIT: OnceLock<Result<(), Error>> = OnceLock::new();
    // `OnceLock` rather than `Once` so a failed first attempt is reported again
    // instead of being swallowed as success.
    INIT.get_or_init(|| install(log_path, level)).clone()
}

fn install(log_path: &Path, level: Level) -> Result<(), Error> {
    let file = match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        Ok(file) => file,
        Err(err) => return Err(io_error(log_path, err)),
    };

    let (sender, receiver) = mpsc::channel::<Vec<u8>>();
    if let Err(err) = std::thread::Builder::new()
        .name("virshot-log".to_owned())
        .spawn(move || drain_log(receiver, file))
    {
        return Err(io_error(log_path, err));
    }

    let sender = Arc::new(Mutex::new(sender));
    let make_writer = move || LogWriter {
        sender: Arc::clone(&sender),
    };

    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_target(true)
        .with_level(true)
        .with_max_level(tracing::Level::from(level))
        .with_writer(make_writer);

    // `SubscriberBuilder::try_init` installs the subscriber AND wires `log`
    // records into tracing through `tracing_log::LogTracer` (the default
    // `tracing-log` feature of tracing-subscriber 0.3.23, kept enabled by this
    // manifest), so eframe's internal `log::error!("Failed to initialize a
    // window for viewport …")` lands in this file instead of vanishing. 0.3.23
    // has no `with_tracing_logs()` — `try_init` is this version's bridge, and
    // it needs no new dependency. It also mirrors the subscriber's max level
    // onto `log`, so filtered `log` diagnostics are skipped cheaply.
    //
    // It reports an error when a global subscriber — or a separate `log`
    // logger — already exists, which is exactly the "already initialised"
    // case: report success and keep whatever is already in place.
    subscriber.try_init().ok();
    Ok(())
}

#[cfg(test)]
#[path = "logging.tests.rs"]
mod tests;
