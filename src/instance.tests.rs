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
