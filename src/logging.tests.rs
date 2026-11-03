use super::Error;
use super::Level;
use super::default_level;
use super::default_log_path;
use super::parse_level;
use std::path::Path;
use std::path::PathBuf;

#[test]
fn parse_level_valid_debug_returns_debug() {
    assert_eq!(parse_level("DEBUG"), Ok(Level::Debug));
    assert_eq!(parse_level("debug"), Ok(Level::Debug));
    assert_eq!(parse_level("Debug"), Ok(Level::Debug));
}

#[test]
fn parse_level_valid_info_returns_info() {
    assert_eq!(parse_level("INFO"), Ok(Level::Info));
    assert_eq!(parse_level("info"), Ok(Level::Info));
}

#[test]
fn parse_level_valid_error_returns_error() {
    assert_eq!(parse_level("ERROR"), Ok(Level::Error));
    assert_eq!(parse_level("error"), Ok(Level::Error));
}

#[test]
fn parse_level_unknown_string_returns_invalid_level_error() {
    let trace = parse_level("TRACE").expect_err("TRACE must be rejected");
    assert!(
        matches!(&trace, Error::InvalidLogLevel(value) if value == "TRACE"),
        "TRACE must be rejected: {trace:?}"
    );
    assert!(
        matches!(parse_level(""), Err(Error::InvalidLogLevel(_))),
        "empty string must be rejected"
    );
    assert!(
        matches!(parse_level("warn"), Err(Error::InvalidLogLevel(_))),
        "warn must be rejected"
    );
    // The typed error keeps the offending value in its message.
    let err = parse_level("warn").expect_err("warn must be rejected");
    assert!(
        err.to_string().contains("warn"),
        "message lost value: {err}"
    );
}

#[test]
fn default_level_is_info() {
    assert_eq!(default_level(), Level::Info);
}

#[test]
fn log_file_path_next_to_config_sibling_of_config_path() {
    let config = Path::new("/home/u/.config/virshot/config.toml");
    assert_eq!(
        default_log_path(config),
        PathBuf::from("/home/u/.config/virshot/virshot.log")
    );
}

#[test]
fn log_file_path_next_to_config_portable_exe_dir() {
    // Windows portable mode: the config lives beside `virshot.exe`, so the
    // log must land beside the exe too (same directory as the config).
    let config = PathBuf::from("C:\\Program Files\\virshot\\config.toml");
    assert_eq!(
        default_log_path(&config),
        PathBuf::from("C:\\Program Files\\virshot\\virshot.log")
    );
}

#[test]
fn log_file_path_next_to_config_windows_path_input() {
    let config = PathBuf::from("C:\\Users\\u\\AppData\\Roaming\\virshot\\config.toml");
    assert_eq!(
        default_log_path(&config),
        PathBuf::from("C:\\Users\\u\\AppData\\Roaming\\virshot\\virshot.log")
    );
}
