//! Single-instance guard (spec §1 "Single-instance guard"): one resident
//! `virshot` process per config file. The guard owns a lock file at
//! `<config_path>.lock` written with the holder's pid via an exclusive
//! create (`create_new`), so two processes cannot hold it at once. An existing
//! lock is inspected: a live pid means another instance is running (typed
//! `Error::AlreadyRunning`); a dead pid or corrupt content is stale and is
//! replaced. `Drop` removes the lock file, so a clean exit frees it for the
//! next start.
//!
//! Wiring: `main.rs` acquires the guard on the resident path only (after
//! logging + config load, before tray/hotkeys/GUI) and keeps it for the whole
//! process lifetime. Headless `virshot shot` does not acquire it — it is a
//! one-shot CLI tool for tests/scripts (spec §8).

use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

/// Instance-lock failures, typed per module (`thiserror` enum).
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    /// The lock file (or its directory) could not be read, created, or
    /// removed (permission denied, missing parent directory).
    #[error("failed to access instance lock {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The lock file is held by a process that is still alive. The resident
    /// startup logs this and exits.
    #[error("another virshot instance is running (pid {pid})")]
    AlreadyRunning { pid: u32 },

    /// A stale lock could not be replaced: it reappeared between removal and
    /// re-creation (a concurrent start won the race). Reported instead of
    /// silently running a second instance.
    #[error("stale instance lock {path} could not be replaced: {message}")]
    StaleLockUnusable { path: PathBuf, message: String },
}

/// The held instance lock. Created by [`InstanceGuard::acquire`] and dropped
/// when the owning process ends; the struct exists only while the lock is ours.
#[derive(Debug)]
pub(crate) struct InstanceGuard {
    lock_path: PathBuf,
}

impl InstanceGuard {
    /// Take the instance lock for `config_path` (`<config_path>.lock`).
    ///
    /// An exclusive create (`create_new`) claims the lock and writes this
    /// process's pid. When the lock file already exists, its pid is checked:
    /// a live process → `Error::AlreadyRunning`; a dead pid or unreadable
    /// content (empty/garbage — treated as stale) → the file is removed and
    /// re-created once; a lock that reappears during that replacement (a
    /// concurrent start won the race) → `Error::StaleLockUnusable`. Any other
    /// file failure → `Error::Io`.
    pub(crate) fn acquire(config_path: &Path) -> Result<Self, Error> {
        let lock_path = lock_path_for(config_path);
        match write_lock(&lock_path, std::process::id()) {
            Ok(()) => Ok(Self { lock_path }),
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                take_over_or_report(&lock_path)
            }
            Err(source) => Err(Error::Io {
                path: lock_path,
                source,
            }),
        }
    }
}

impl Drop for InstanceGuard {
    /// Remove the lock file so the next start can claim it. Best-effort: a
    /// process that cannot delete its own lock (permission change, the file
    /// already gone) still exits — a leftover lock is recovered by the stale
    /// check on the next start.
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.lock_path);
    }
}

/// Write `pid` into a **new** lock file; `AlreadyExists` surfaces as the io
/// kind so `acquire` can distinguish "someone holds it" from a real failure.
fn write_lock(path: &Path, pid: u32) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    file.write_all(format!("{pid}\n").as_bytes())
}

/// Pid recorded in the lock file: `Ok(None)` for content that is not a pid
/// (empty or garbage) — that lock is unusable and counts as stale.
fn read_lock_pid(path: &Path) -> std::io::Result<Option<u32>> {
    let text = std::fs::read_to_string(path)?;
    Ok(text.trim().parse::<u32>().ok())
}

/// Decide what to do with an existing lock: honour a live holder, otherwise
/// replace the stale file.
fn take_over_or_report(lock_path: &Path) -> Result<InstanceGuard, Error> {
    match read_lock_pid(lock_path) {
        Ok(Some(pid)) if pid_alive(pid, probe_pid_alive) => Err(Error::AlreadyRunning { pid }),
        // Dead holder or unusable content: neither owns the app any more.
        Ok(_) => replace_stale_lock(lock_path),
        Err(source) => Err(Error::Io {
            path: lock_path.to_path_buf(),
            source,
        }),
    }
}

/// Remove a stale lock and re-claim it exactly once. A missing file counts as
/// already removed (it vanished concurrently); a reappearing file means another
/// process claimed it in the meantime, which is reported, not ignored.
fn replace_stale_lock(lock_path: &Path) -> Result<InstanceGuard, Error> {
    if let Err(source) = std::fs::remove_file(lock_path)
        && source.kind() != std::io::ErrorKind::NotFound
    {
        return Err(Error::Io {
            path: lock_path.to_path_buf(),
            source,
        });
    }
    match write_lock(lock_path, std::process::id()) {
        Ok(()) => Ok(InstanceGuard {
            lock_path: lock_path.to_path_buf(),
        }),
        Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(Error::StaleLockUnusable {
                path: lock_path.to_path_buf(),
                message: "the lock reappeared while a stale one was being replaced".to_owned(),
            })
        }
        Err(source) => Err(Error::Io {
            path: lock_path.to_path_buf(),
            source,
        }),
    }
}

/// Lock file path derived from the config path this run uses: `<config_path>.lock`.
/// Pure logic, so the derivation is unit-tested without any file access.
fn lock_path_for(config_path: &Path) -> PathBuf {
    let mut path = config_path.to_path_buf();
    path.as_mut_os_string().push(".lock");
    path
}

/// Liveness decision, kept pure so the gate is testable with injected probes:
/// `pid` counts as alive exactly when `probe` says so.
fn pid_alive(pid: u32, probe: impl Fn(u32) -> bool) -> bool {
    probe(pid)
}

/// Linux probe: `/proc/<pid>` exists while the process exists.
#[cfg(target_os = "linux")]
fn probe_pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

/// Windows probe: `tasklist` filtered by pid. A missing/failing `tasklist`
/// leaves liveness *uncertain*, so the existing lock is honoured rather than
/// stolen (the caller gets `Error::AlreadyRunning`); no panic on either case.
#[cfg(windows)]
fn probe_pid_alive(pid: u32) -> bool {
    let Ok(output) = std::process::Command::new("tasklist")
        .arg("/FI")
        .arg(format!("PID eq {pid}"))
        .output()
    else {
        return true;
    };
    String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
}

/// Every other platform has no cheap liveness probe, so an existing lock is
/// treated as held (uncertain → honour it). Outside the MVP targets
/// (linux + windows-gnu) this only keeps the module compiling.
#[cfg(not(any(target_os = "linux", windows)))]
fn probe_pid_alive(_pid: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Scratch directory under `./.tmp/` (repo root, git-ignored), unique per
    /// test and removed when the test ends. A relative path keeps the helper
    /// identical on linux and windows-gnu (no tempdir dependency).
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = PathBuf::from("./.tmp").join(format!("instance-test-{tag}"));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("scratch dir must be creatable");
            Self { path }
        }

        fn config_path(&self) -> PathBuf {
            self.path.join("config.toml")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn lock_path_derived_from_config_path() {
        assert_eq!(
            lock_path_for(Path::new("/home/u/.config/virshot/config.toml")),
            PathBuf::from("/home/u/.config/virshot/config.toml.lock")
        );
        assert_eq!(
            lock_path_for(Path::new("config")),
            PathBuf::from("config.lock")
        );
    }

    #[test]
    fn lock_path_for_portable_config_stays_sibling_of_config() {
        // Portable mode (config beside the exe): the lock derivation is
        // unchanged — `<config>.lock` in the same directory.
        assert_eq!(
            lock_path_for(Path::new("C:\\Program Files\\virshot\\config.toml")),
            PathBuf::from("C:\\Program Files\\virshot\\config.toml.lock")
        );
    }

    #[test]
    fn pid_alive_live_probe_reports_alive() {
        assert!(pid_alive(4242, |_| true));
    }

    #[test]
    fn pid_alive_dead_probe_reports_dead() {
        assert!(!pid_alive(4242, |_| false));
    }

    #[test]
    fn acquire_creates_lock_file_with_own_pid() {
        let dir = TempDir::new("create");
        let config_path = dir.config_path();

        let _guard = InstanceGuard::acquire(&config_path).expect("first acquire must succeed");

        let lock_path = lock_path_for(&config_path);
        let content = std::fs::read_to_string(&lock_path).expect("lock file must exist");
        assert_eq!(content.trim(), std::process::id().to_string());
    }

    #[test]
    fn acquire_second_guard_same_path_fails_already_running_with_that_pid() {
        let dir = TempDir::new("second");
        let config_path = dir.config_path();
        let _first = InstanceGuard::acquire(&config_path).expect("first acquire must succeed");

        let second = InstanceGuard::acquire(&config_path);

        assert!(
            matches!(second, Err(Error::AlreadyRunning { pid }) if pid == std::process::id()),
            "second acquire must report the holder pid: {second:?}"
        );
    }

    #[test]
    fn acquire_removes_stale_lock_of_dead_pid_and_succeeds() {
        let dir = TempDir::new("stale");
        let config_path = dir.config_path();
        let lock_path = lock_path_for(&config_path);
        // Implausible pid: above the practical pid range on both targets, so
        // `/proc/<pid>` (linux) and `tasklist` (windows) report it as absent.
        std::fs::write(&lock_path, "2147483647\n").expect("stale lock must be writable");

        let guard = InstanceGuard::acquire(&config_path).expect("stale lock must be replaced");

        assert!(
            lock_path.exists(),
            "lock must be re-created by this process"
        );
        #[cfg(target_os = "linux")]
        {
            let content = std::fs::read_to_string(&lock_path).expect("lock file must exist");
            assert_eq!(content.trim(), std::process::id().to_string());
        }
        drop(guard);
        assert!(!lock_path.exists(), "drop must remove the lock");
    }

    #[test]
    fn acquire_corrupt_lock_content_treated_as_stale() {
        let dir = TempDir::new("corrupt");
        let config_path = dir.config_path();
        let lock_path = lock_path_for(&config_path);
        std::fs::write(&lock_path, "not-a-pid").expect("corrupt lock must be writable");

        let guard = InstanceGuard::acquire(&config_path).expect("corrupt lock must be replaced");

        assert!(
            lock_path.exists(),
            "lock must be re-created by this process"
        );
        #[cfg(target_os = "linux")]
        {
            let content = std::fs::read_to_string(&lock_path).expect("lock file must exist");
            assert_eq!(content.trim(), std::process::id().to_string());
        }
        drop(guard);
    }

    #[test]
    fn drop_removes_lock_file() {
        let dir = TempDir::new("drop");
        let config_path = dir.config_path();
        let lock_path = lock_path_for(&config_path);

        let guard = InstanceGuard::acquire(&config_path).expect("acquire must succeed");
        assert!(lock_path.exists(), "acquire must create the lock file");

        drop(guard);

        assert!(!lock_path.exists(), "drop must remove the lock file");
    }
}
