//! Public-API unit tests for the root package.
//!
//! Cargo turns a directory under `tests/` into a test target only through its
//! `main.rs`, so this file is the placeholder that makes `tests/unit/` a target;
//! each area is a module declared here (`mod template;` → `tests/unit/template.rs`).
//!
//! An integration test can only reach the crate's public API, and `virshot` is a
//! bin-only package (`src/main.rs`, no `lib` target), so nothing can import it yet.
//! Modules appear here when the root package exposes a `lib` target; private
//! internals stay tested in `src/<module>.tests.rs` (see AGENTS.md, Testing).
