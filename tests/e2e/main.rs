//! E2E regression tests: the built binary driven headless, never the crate API.
//!
//! Cargo turns a directory under `tests/` into a test target only through its
//! `main.rs`, so every area lives in a module file declared here.

mod cli_headless;
