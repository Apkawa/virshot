//! Build stamp for the PoC: embeds the build-time UNIX epoch, the cargo
//! profile, and (when available) the short git SHA into the binary via
//! `cargo:rustc-env`, so every invocation can print which build it is — the
//! live runs need that to tell the two crate copies apart. Std-only, no
//! dependencies.

fn main() {
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=POC_BUILD_EPOCH={epoch}");

    // Cargo exports `PROFILE` (`debug` / `release`) for the crate compile.
    let profile = std::env::var("PROFILE").ok().unwrap_or_default();
    println!("cargo:rustc-env=POC_BUILD_PROFILE={profile}");

    // Short git SHA of the tree the build ran in; "nogit" when git is missing
    // or the directory is not a repository (the scratch copy may be one or the
    // other depending on where it lives).
    let sha = short_git_sha();
    println!("cargo:rustc-env=POC_BUILD_SHA={sha}");

    // Re-embed only when this script changes; the stamp is compile-time.
    println!("cargo:rerun-if-changed=build.rs");
}

/// `git rev-parse --short HEAD`, or `nogit` when git is unavailable or the
/// build directory is outside a repository. Failures are values, not panics.
fn short_git_sha() -> String {
    match std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
    {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        }
        _ => String::from("nogit"),
    }
}
