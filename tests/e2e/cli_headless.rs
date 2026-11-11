//! E2E regression tests for the CLI contract (spec §8): run the built binary
//! (`CARGO_BIN_EXE_virshot`) headless. No TDD — these lock the behaviour that
//! shipped in Stage 5.
//!
//! This build host has no X11/XWayland display, so capture cannot succeed
//! here: the automated tests assert the headless failure path — exit 1, empty
//! stdout, auto-created config with defaults, error line in the log file. The
//! success-path test is `#[ignore]`d: it needs a desktop host with
//! X11/XWayland (linux) or Windows; run it there with
//! `cargo test -- --ignored`.
//!
//! Scratch directories live under `./.tmp/` (repo root, git-ignored), unique
//! per test, removed on drop. Tests never run bare `virshot` — that parks the
//! resident loop forever.

use std::path::PathBuf;
use std::process::Command;
use std::process::Output;

/// The built binary for this package (cargo provides the env value for tests).
fn virshot() -> Command {
    Command::new(env!("CARGO_BIN_EXE_virshot"))
}

/// Scratch directory under `./.tmp/`, unique per tag, removed when dropped.
struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    fn new(tag: &str) -> Self {
        let path = PathBuf::from("./.tmp").join(format!("cli-e2e-{tag}"));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("scratch dir must be creatable");
        Self { path }
    }

    fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn run(args: &[&str]) -> Output {
    virshot()
        .args(args)
        .output()
        .expect("binary must be runnable")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

#[test]
fn cli_version_flag_exits_zero_and_prints_version() {
    let output = run(&["--version"]);
    assert!(output.status.success(), "--version must exit 0");
    let text = stdout(&output);
    assert!(
        text.contains("virshot"),
        "version line must name virshot: {text}"
    );
    assert!(
        text.contains(env!("CARGO_PKG_VERSION")),
        "version line must carry the package version: {text}"
    );
    // Extended build-metadata contract (task 2026-11-10_about-dialog): the
    // line is `virshot <version> (<sha>) built <build_time>`. Checked by plain
    // containment, no regex dependency.
    assert!(
        text.contains("built "),
        "version line must carry the build marker: {text}"
    );
    assert!(
        text.contains("+03:00"),
        "build time must use the Moscow ISO-8601 offset: {text}"
    );
    // Regex-free check of the parenthesised sha token: the text between the
    // first '(' and the following ')' must be a non-empty token (sha or
    // `unknown`).
    let sha_token = text
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(inner, _)| inner)
        .expect("version line must contain a parenthesised sha token");
    assert!(
        !sha_token.is_empty(),
        "the parenthesised sha token must not be empty: {text}"
    );
}

#[test]
fn cli_help_flag_exits_zero_and_documents_shot_subcommand() {
    let output = run(&["--help"]);
    assert!(output.status.success(), "--help must exit 0");
    let help = stdout(&output);
    for expected in ["shot", "--config", "--log", "--log-level"] {
        assert!(
            help.contains(expected),
            "help must document {expected}: {help}"
        );
    }
}

#[test]
fn cli_shot_help_exits_zero_and_documents_window_out_format() {
    let output = run(&["shot", "--help"]);
    assert!(output.status.success(), "shot --help must exit 0");
    let help = stdout(&output);
    for expected in ["--window", "--out", "--format"] {
        assert!(
            help.contains(expected),
            "shot help must document {expected}: {help}"
        );
    }
}

#[test]
fn cli_shot_without_display_exits_one_and_records_failure_in_log() {
    let dir = ScratchDir::new("headless-failure");
    let config = dir.join("config.toml");
    let log = dir.join("e2e.log");
    let config_arg = config.display().to_string();
    let log_arg = log.display().to_string();
    let output = run(&["shot", "--config", &config_arg, "--log", &log_arg]);

    assert_eq!(output.status.code(), Some(1), "capture failure must exit 1");
    assert!(
        !stderr(&output).trim().is_empty(),
        "stderr must carry the error message"
    );
    assert!(
        stdout(&output).is_empty(),
        "no saved path may be printed on failure: {}",
        stdout(&output)
    );

    assert!(
        config.exists(),
        "a missing config must be auto-created with defaults"
    );
    let config_text = std::fs::read_to_string(&config).expect("created config must be readable");
    assert!(
        config_text.contains("[general]"),
        "auto-created config must contain the defaults: {config_text}"
    );
    // Stage 14 shipped behavior (spec §7 v5.0): the default `save_path` is
    // resolved from the home directory, not a hardcoded literal, so the
    // auto-created file ends with the Pictures/ViRShot suffix. Checked
    // separator-agnostically so the assertion holds on Windows too.
    let save_path_line = config_text
        .lines()
        .find(|line| line.trim_start().starts_with("save_path"))
        .expect("auto-created config must carry a save_path line");
    assert!(
        save_path_line.contains("Pictures") && save_path_line.ends_with("ViRShot\""),
        "save_path must be the home-resolved Pictures/ViRShot: {save_path_line}"
    );

    let log_text = std::fs::read_to_string(&log).expect("log file must exist");
    assert!(
        log_text.contains("ERROR"),
        "log must contain an ERROR line: {log_text}"
    );
    assert!(
        log_text.contains("Capture failed"),
        "log must record the capture failure: {log_text}"
    );
}

#[test]
fn cli_shot_format_override_webp_parses_and_headless_run_exits_one() {
    let dir = ScratchDir::new("format-webp");
    let config = dir.join("config.toml");
    let log = dir.join("e2e.log");
    let config_arg = config.display().to_string();
    let log_arg = log.display().to_string();
    let output = run(&[
        "shot",
        "--config",
        &config_arg,
        "--log",
        &log_arg,
        "--format",
        "webp",
    ]);

    // `--format webp` parses (clap accepts it, otherwise exit would be 2);
    // the capture itself still fails on this headless host → exit 1.
    assert_eq!(output.status.code(), Some(1), "capture failure must exit 1");
    assert!(
        !stderr(&output).trim().is_empty(),
        "stderr must carry the error"
    );
    assert!(stdout(&output).is_empty(), "no path printed on failure");
    let log_text = std::fs::read_to_string(&log).expect("log file must exist");
    assert!(
        log_text.contains("Capture failed"),
        "log must record the capture failure: {log_text}"
    );
}

#[test]
fn cli_shot_window_and_out_override_parse_and_failure_writes_no_file() {
    let dir = ScratchDir::new("out-override");
    let config = dir.join("config.toml");
    let log = dir.join("e2e.log");
    let out = dir.join("shots/out.jpg");
    let config_arg = config.display().to_string();
    let log_arg = log.display().to_string();
    let out_arg = out.display().to_string();
    let output = run(&[
        "shot",
        "--config",
        &config_arg,
        "--log",
        &log_arg,
        "--window",
        "--out",
        &out_arg,
        "--format",
        "jpg",
    ]);

    // The flags parse and the override plumbing runs; the capture fails on
    // this headless host → exit 1, stdout empty. The success path (exact
    // `--out` file written, `-N` collision variant, path printed) is covered
    // by the ignored desktop test below.
    assert_eq!(output.status.code(), Some(1), "capture failure must exit 1");
    assert!(
        stdout(&output).is_empty(),
        "no saved path may be printed on failure: {}",
        stdout(&output)
    );
    assert!(
        !out.exists(),
        "a failed capture must not create the --out file"
    );
    assert!(
        !dir.join("shots").exists(),
        "nothing may be saved before the capture succeeds"
    );
}

#[test]
fn cli_shot_invalid_format_value_rejected_by_clap_exits_two() {
    let output = run(&["shot", "--format", "bmp"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "invalid --format value must be a clap usage error (exit 2)"
    );
    assert!(
        stderr(&output).contains("--format"),
        "usage error must name the flag: {}",
        stderr(&output)
    );
}

#[test]
fn cli_invalid_log_level_rejected_by_clap_exits_two() {
    let output = run(&["--log-level", "TRACE", "shot"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "invalid --log-level value must be a clap usage error (exit 2)"
    );
    assert!(
        stderr(&output).contains("--log-level"),
        "usage error must name the flag: {}",
        stderr(&output)
    );
}

#[test]
fn cli_shot_default_log_is_sibling_of_config() {
    let dir = ScratchDir::new("default-log");
    let config = dir.join("virshot/config.toml");
    let config_arg = config.display().to_string();
    let output = run(&["shot", "--config", &config_arg]);

    assert_eq!(output.status.code(), Some(1), "capture failure must exit 1");
    let log = dir.join("virshot/virshot.log");
    assert!(
        log.exists(),
        "default log must be virshot.log next to the config file"
    );
    let log_text = std::fs::read_to_string(&log).expect("log file must be readable");
    assert!(
        log_text.contains("Capture failed"),
        "log must record the capture failure: {log_text}"
    );
}

/// Success path (spec §8: saved path printed to stdout, exit 0) needs a real
/// capture, which needs a desktop session: X11/XWayland on linux or Windows.
/// This build host is headless (no X11 socket), so this stays out of the
/// automated gate. Run manually on a desktop host: `cargo test -- --ignored`.
#[test]
#[ignore]
fn cli_shot_full_screen_success_prints_saved_path_and_exits_zero() {
    let dir = ScratchDir::new("success");
    let config = dir.join("config.toml");
    let out = dir.join("shot.png");
    let config_arg = config.display().to_string();
    let out_arg = out.display().to_string();
    let output = run(&["shot", "--config", &config_arg, "--out", &out_arg]);

    assert!(
        output.status.success(),
        "capture on a desktop host must exit 0: stderr={}",
        stderr(&output)
    );
    assert_eq!(
        stdout(&output).trim(),
        out.display().to_string(),
        "stdout must be the exact saved path"
    );
    assert!(out.exists(), "--out file must be written");
    let decoded = image::open(&out).expect("saved file must decode");
    assert!(
        decoded.width() > 0 && decoded.height() > 0,
        "captured image must not be empty"
    );
}
